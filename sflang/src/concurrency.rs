//! concurrency.rs — 并发原语与同步原语（调度器任务版）
//!
//! 设计要点（阶段四）：
//!   - `run` 启动的并发体是调度器任务（scheduler.rs）：阻塞类操作在任务内
//!     挂起（park），不占 OS 线程；任务可承载量从千级（OS 线程）提升到十万级
//!   - 双上下文：同一组原语在任务上下文挂起、在线程上下文（主脚本、poolRun
//!     工作线程、threadRun）Condvar 阻塞——行为对脚本完全一致
//!   - 两种挂起模型（vm.rs Pause）：
//!     * 值注入型（chanRecv/onceDo 等待者）：唤醒后从调用指令之后继续，
//!       结果位占位 undefined 被注入值替换（wake_with）
//!     * 等待-重试型（wgWait/semAcquire）：唤醒后回退重试调用（重查条件）。
//!       Mutex 虽是资源等待，但采用交接式注入（unlock 把所有权随唤醒移交），
//!       避免重试竞争；RWMutex 同理，放行时同步登记状态（readers/writer）
//!   - 挂起登记与条件检查在同一把原语内部锁内完成（先置 BLOCKED 再入队），
//!     从根本上消除"检查后、登记前"到达的唤醒丢失窗口
//!
//! API 概览（与旧版完全兼容）：
//!   channel:  newChannel / chanSend / chanRecv / chanTryRecv
//!   mutex:    newMutex / lock / unlock / tryLock
//!   rwmutex:  newRWMutex / rlock / runlock / wlock / wunlock
//!   waitgroup:newWaitGroup / wgAdd / wgDone / wgWait
//!   sem:      newSemaphore / semAcquire / semRelease
//!   once:     newOnce / onceDo

use std::collections::VecDeque;

use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use crate::function::BuiltinDoc;
use crate::scheduler::{self, Task};
use crate::value::Value;
use crate::vm::VM;

// ---- 并发原语文档 ----

static DOC_THREAD_RUN: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "threadRun(fn, args...) -> undefined",
    summary: "在独立 OS 线程中执行函数（threadRun 逃生门）。与 run（调度器任务）的区别：threadRun 每次创建真线程（~17µs、~8MB 栈预留），适合单个长时 CPU 密集任务独占一个核；一般并发请用 run（任务，~µs 级、万级可行）。与旧版 run 线程语义一致：共享 globals、异常打印不传播。",
    params: &[
        ("fn", "要执行的函数值"),
        ("args...", "传递给函数的参数"),
    ],
    returns: "undefined",
    examples: &[
        "threadRun(func(n) { var s = heavyCompute(n) ; pln(s) }, 1000000)",
    ],
    errors: &[
        "fn 不是函数值时打印启动失败提示",
        "函数内异常打印 [threadRun 线程异常]，不传播",
    ],
};

static DOC_POOL_RUN: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "poolRun(fn, workers, items) -> array",
    summary: "有界并发执行：启动 workers 个工作线程，并发处理 items 中的每一项（对每项调用 fn(item)）。阻塞直到全部完成，返回与 items 等长的结果数组（顺序与 items 一一对应）。fn 抛异常时对应位置返回 error 值，不影响其余项。workers 大于 items 数量时按 items 数量启动（不启多余线程）。",
    params: &[
        ("fn", "处理函数，接收一个 item，返回结果"),
        ("workers", "工作线程数（整数，1..=1024）"),
        ("items", "待处理项数组"),
    ],
    returns: "array 结果数组，results[i] 为 fn(items[i]) 的返回值（异常时为 error 值）",
    examples: &[
        "var urls = [\"http://a\", \"http://b\", \"http://c\"]",
        "var bodies = poolRun(func(u) { return getWeb(u) }, 8, urls)",
        "for i in range(len(bodies)) { pln(urls[i], \"->\", len(bodies[i])) }",
    ],
    errors: &[
        "fn 不是函数值时返回错误",
        "workers 非整数或超出 1..=1024 范围时返回错误",
        "items 不是数组时返回错误",
        "items 为空数组时直接返回空数组（不启动线程）",
        "fn 对某项抛异常时该项结果为 error 值（用 isErr 判断），整体不中断",
    ],
};

static DOC_NEW_CHANNEL: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "newChannel() -> channel",
    summary: "创建无缓冲通道（mpsc），用于任务/线程间通信。配合 run 和 chanSend/chanRecv 使用。接收方可挂起等待，不占 OS 线程。",
    params: &[],
    returns: "channel 通道对象",
    examples: &[
        "var ch = newChannel()",
        "run sender()       // 子任务 chanSend(ch, 42)",
        "var v = chanRecv(ch) // 主线程接收",
    ],
    errors: &[],
};

static DOC_CHAN_SEND: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "chanSend(ch, val) -> undefined",
    summary: "向通道发送值。有等待的接收者时直接交接唤醒（值不经队列）；否则入队等待接收。永不阻塞（通道无界）。",
    params: &[("ch", "channel 对象"), ("val", "要发送的值")],
    returns: "undefined",
    examples: &["chanSend(ch, 42)"],
    errors: &["ch 参数应为 channel 类型"],
};

static DOC_CHAN_RECV: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "chanRecv(ch) -> value",
    summary: "从通道接收值（无数据时挂起等待：任务内挂起不占 OS 线程，主线程内阻塞）。",
    params: &[("ch", "channel 对象")],
    returns: "接收到的值；通道关闭后返回 undefined",
    examples: &["var v = chanRecv(ch)"],
    errors: &[],
};

static DOC_CHAN_TRY_RECV: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "chanTryRecv(ch) -> value|undefined",
    summary: "非阻塞接收：暂无数据或通道已关闭（发送端全部丢弃）均返回 undefined，两者不区分；需要区分时请由发送方在协议上约定结束标记。",
    params: &[("ch", "channel 对象")],
    returns: "值或 undefined（暂无数据或已关闭时）",
    examples: &["var v = chanTryRecv(ch); if v != undefined { pln(v) }"],
    errors: &[],
};

static DOC_NEW_MUTEX: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "newMutex() -> mutex",
    summary: "创建互斥锁，用于保护共享数据的并发访问。锁等待在任务内挂起（不占 OS 线程）。",
    params: &[],
    returns: "mutex 锁对象",
    examples: &[
        "var m = newMutex()",
        "lock(m); count++; unlock(m)",
    ],
    errors: &[],
};

static DOC_LOCK: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "lock(m) -> undefined",
    summary: "加锁（锁被占时等待：任务内挂起，主线程内阻塞）。解锁时所有权直接交接给被唤醒的等待者。",
    params: &[("m", "mutex 对象")],
    returns: "undefined",
    examples: &["lock(m)"],
    errors: &[],
};

static DOC_UNLOCK: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "unlock(m) -> undefined",
    summary: "释放锁。有任务等待时所有权直接交接（held 保持 true）；否则置空闲并唤醒线程等待者。不校验属主（Go 语义宽松）；未持锁时为幂等空操作。",
    params: &[("m", "mutex 对象")],
    returns: "undefined",
    examples: &["unlock(m)"],
    errors: &[],
};

static DOC_TRY_LOCK: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "tryLock(m) -> bool",
    summary: "尝试加锁（非阻塞）：成功返回 true，锁被占用返回 false。",
    params: &[("m", "mutex 对象")],
    returns: "bool 是否成功获取锁",
    examples: &["if tryLock(m) { ... unlock(m) }"],
    errors: &[],
};

static DOC_NEW_RWMUTEX: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "newRWMutex() -> rwmutex",
    summary: "创建读写锁：允许多个读锁或一个写锁。有写者排队时新读者排队（防写者饥饿）。",
    params: &[],
    returns: "rwmutex 读写锁对象",
    examples: &["var rw = newRWMutex()"],
    errors: &[],
};

static DOC_RLOCK: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "rlock(rw) -> undefined",
    summary: "获取读锁（共享，多读者并发；有写者持有或排队时等待）。",
    params: &[("rw", "rwmutex 对象")],
    returns: "undefined",
    examples: &["rlock(rw); ... runlock(rw)"],
    errors: &[],
};

static DOC_RUNLOCK: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "runlock(rw) -> undefined",
    summary: "释放读锁。读者归零时按队列顺序放行等待者（连续读者一并放行，写者需无读者）。",
    params: &[("rw", "rwmutex 对象")],
    returns: "undefined",
    examples: &["runlock(rw)"],
    errors: &[],
};

static DOC_WLOCK: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "wlock(rw) -> undefined",
    summary: "获取写锁（排他；有读者或写者时等待，等待期间新读者排队防写者饥饿）。",
    params: &[("rw", "rwmutex 对象")],
    returns: "undefined",
    examples: &["wlock(rw); ... wunlock(rw)"],
    errors: &[],
};

static DOC_WUNLOCK: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "wunlock(rw) -> undefined",
    summary: "释放写锁并放行等待者（按队列顺序：连续读者或单个写者）。",
    params: &[("rw", "rwmutex 对象")],
    returns: "undefined",
    examples: &["wunlock(rw)"],
    errors: &[],
};

static DOC_NEW_WAITGROUP: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "newWaitGroup() -> waitGroup",
    summary: "创建 WaitGroup，用于等待一组并发任务完成。",
    params: &[],
    returns: "waitGroup 对象",
    examples: &[
        "var wg = newWaitGroup()",
        "wgAdd(wg, 3); for i := 0; i < 3; i++ { run worker(wg) }",
        "wgWait(wg)  // 等待 3 个任务完成",
    ],
    errors: &[],
};

static DOC_WG_ADD: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "wgAdd(wg, n) -> undefined",
    summary: "增加等待计数 n（可为负，对应批量 Done）。",
    params: &[("wg", "waitGroup 对象"), ("n", "增加的计数（int）")],
    returns: "undefined",
    examples: &["wgAdd(wg, 3)"],
    errors: &[
        "计数溢出 int 范围时返回错误",
        "计数变负时返回错误（Done 次数超过 Add）",
    ],
};

static DOC_WG_DONE: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "wgDone(wg) -> undefined",
    summary: "标记一个任务完成（计数减 1）。计数归零时唤醒全部等待者（唤醒后重查计数）。",
    params: &[("wg", "waitGroup 对象")],
    returns: "undefined",
    examples: &["wgDone(wg)"],
    errors: &["计数减为负数报错（Done 次数超过 Add）"],
};

static DOC_WG_WAIT: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "wgWait(wg) -> undefined",
    summary: "等待计数归零（所有任务完成）。计数非零时挂起等待（任务内不占 OS 线程）；唤醒后重查计数，仍非零则继续等。",
    params: &[("wg", "waitGroup 对象")],
    returns: "undefined",
    examples: &["wgWait(wg)"],
    errors: &[],
};

static DOC_NEW_SEMAPHORE: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "newSemaphore(n) -> semaphore",
    summary: "创建信号量，限制同时访问的并发数。",
    params: &[("n", "最大并发数（正整数，缺省为 1）")],
    returns: "semaphore 对象",
    examples: &["var sem = newSemaphore(5)  // 最多 5 个并发"],
    errors: &[
        "参数非整数（如字符串、undefined）时返回错误",
        "参数 <= 0 时返回错误（合法范围 >= 1）",
    ],
};

static DOC_SEM_ACQUIRE: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "semAcquire(sem) -> undefined",
    summary: "获取信号量（无空位时挂起等待；唤醒后重查空位，仍无则继续等）。",
    params: &[("sem", "semaphore 对象")],
    returns: "undefined",
    examples: &["semAcquire(sem)"],
    errors: &[],
};

static DOC_SEM_RELEASE: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "semRelease(sem) -> undefined",
    summary: "释放信号量（空位加 1，唤醒一个等待者；唤醒后由其重查空位）。",
    params: &[("sem", "semaphore 对象")],
    returns: "undefined",
    examples: &["semRelease(sem)"],
    errors: &[],
};

static DOC_NEW_ONCE: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "newOnce() -> once",
    summary: "创建 Once 对象，保证初始化代码只执行一次。",
    params: &[],
    returns: "once 对象",
    examples: &[
        "var o = newOnce()",
        "onceDo(o, func() { pln(\"只执行一次\") })",
    ],
    errors: &[],
};

static DOC_ONCE_DO: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "onceDo(o, fn) -> value",
    summary: "保证 fn 只在第一次调用时执行（并发安全）。返回首次执行的结果；fn 出错时该错误会返回给所有调用方（不吞掉）。并发调用方挂起等待首次执行完成并直接取得其结果。",
    params: &[("o", "once 对象"), ("fn", "要执行的函数")],
    returns: "首次执行的返回值（后续调用返回同一结果）",
    examples: &["onceDo(o, initFunc)"],
    errors: &[
        "fn 执行出错时返回该错误",
        "回调内递归调用同一 once 时返回错误（而不是永久阻塞）",
        "回调内不允许再挂起当前任务（回调经 call_function_value 执行，挂起被禁用并返回明确错误）",
    ],
};

/// register 注册所有并发相关内置函数。
pub fn register(vm: &mut VM) {
    // channel
    vm.register_builtin_doc("newChannel", bi_new_channel, &DOC_NEW_CHANNEL);
    vm.register_builtin_doc("chanSend", bi_chan_send, &DOC_CHAN_SEND);
    vm.register_builtin_doc("chanRecv", bi_chan_recv, &DOC_CHAN_RECV);
    vm.register_builtin_doc("chanTryRecv", bi_chan_try_recv, &DOC_CHAN_TRY_RECV);
    // mutex
    vm.register_builtin_doc("newMutex", bi_new_mutex, &DOC_NEW_MUTEX);
    vm.register_builtin_doc("lock", bi_lock, &DOC_LOCK);
    vm.register_builtin_doc("unlock", bi_unlock, &DOC_UNLOCK);
    vm.register_builtin_doc("tryLock", bi_try_lock, &DOC_TRY_LOCK);
    // rwmutex
    vm.register_builtin_doc("newRWMutex", bi_new_rwmutex, &DOC_NEW_RWMUTEX);
    vm.register_builtin_doc("rlock", bi_rlock, &DOC_RLOCK);
    vm.register_builtin_doc("runlock", bi_runlock, &DOC_RUNLOCK);
    vm.register_builtin_doc("wlock", bi_wlock, &DOC_WLOCK);
    vm.register_builtin_doc("wunlock", bi_wunlock, &DOC_WUNLOCK);
    // waitgroup
    vm.register_builtin_doc("newWaitGroup", bi_new_waitgroup, &DOC_NEW_WAITGROUP);
    vm.register_builtin_doc("wgAdd", bi_wg_add, &DOC_WG_ADD);
    vm.register_builtin_doc("wgDone", bi_wg_done, &DOC_WG_DONE);
    vm.register_builtin_doc("wgWait", bi_wg_wait, &DOC_WG_WAIT);
    // semaphore
    vm.register_builtin_doc("newSemaphore", bi_new_semaphore, &DOC_NEW_SEMAPHORE);
    vm.register_builtin_doc("semAcquire", bi_sem_acquire, &DOC_SEM_ACQUIRE);
    vm.register_builtin_doc("semRelease", bi_sem_release, &DOC_SEM_RELEASE);
    // once
    vm.register_builtin_doc("newOnce", bi_new_once, &DOC_NEW_ONCE);
    vm.register_builtin_doc("onceDo", bi_once_do, &DOC_ONCE_DO);
    // 有界工作池
    vm.register_builtin_doc("poolRun", bi_pool_run, &DOC_POOL_RUN);
    // 真线程逃生门（CPU 密集长任务）
    vm.register_builtin_doc("threadRun", bi_thread_run, &DOC_THREAD_RUN);
}

/// bi_thread_run 在独立 OS 线程中执行函数（threadRun 逃生门）。
///
/// 保留旧版 run 线程语义：独立 VM 共享 globals/out、8MB 栈、异常打印不传播。
/// 适用场景：单个长时 CPU 密集计算（任务模型下会与其他任务分时共享 worker，
/// 预算抢占带来切换开销；threadRun 独占线程跑满一核）。一般并发请用 run。
fn bi_thread_run(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value(
            "threadRun() 需要 1 个以上参数 (fn, args...)",
        ));
    }
    let callee = args[0].clone();
    let call_args: Vec<Value> = args[1..].to_vec();
    let globals = vm.globals_handle();
    let out = vm.output_handle();
    // 8MB 栈（与旧 run 线程一致；CPU 密集任务可能深递归）
    let spawned = std::thread::Builder::new()
        .name("sf-threadrun".to_string())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let mut tvm = VM::new();
            tvm.set_globals_handle(globals);
            tvm.set_output_handle(out);
            let res = tvm.call_function_value(callee, call_args);
            if let Err(v) = res {
                let msg = match &v {
                    Value::Error(e) => e.message.clone(),
                    other => other.to_str(),
                };
                let _ = std::io::Write::write_fmt(
                    &mut *tvm.output_handle().lock().unwrap(),
                    format_args!("[threadRun 线程异常] {}
", msg),
                );
            }
        });
    if let Err(e) = spawned {
        let _ = std::io::Write::write_fmt(
            &mut *vm.output_handle().lock().unwrap(),
            format_args!("[threadRun 线程启动失败] {}
", e),
        );
    }
    Ok(Value::Undefined)
}

// ============ 通用 downcast 辅助 ============

/// downcast 将 Native 值 downcast 为指定类型，失败返回 AI 友好错误。
///
/// `what` 为原语类型名（如 "mutex"），用于错误信息。
fn downcast<'a, T: 'static>(v: &'a Value, what: &str, fn_name: &str) -> Result<&'a Arc<T>, Value> {
    match v {
        Value::Native(n) => n.downcast_ref::<Arc<T>>().ok_or_else(|| {
            crate::value::error_value(format!(
                "{}() 参数不是 {} (可能原因：传入了错误类型的同步原语或 undefined)",
                fn_name, what,
            ))
        }),
        other => Err(crate::value::error_value(format!(
            "{}() 参数应为 {}，得到 {} (可能原因：参数顺序错误或未用 new{} 创建)",
            fn_name, what, other.type_name(), what,
        ))),
    }
}

// ============ Channel（值注入型） ============

/// ChanState 通道内部状态（由 inner 锁保护）。
struct ChanState {
    /// queue 数据队列（无界）
    queue: VecDeque<Value>,
    /// recv_waiters 等待数据的接收任务（挂起中；线程接收者走 cv 不在此列）
    recv_waiters: VecDeque<Arc<Task>>,
}

/// Channel Sflang 的 channel 类型。
///
/// 发送：有等待的接收任务时直接交接（wake_with 注入值，不入队）；否则入队
/// 并通知线程接收者。接收：任务上下文空队列时挂起登记（持锁登记，与发送方
/// 的"交接 or 入队"决策互斥，无丢唤醒窗口）；线程上下文 cv 阻塞。
/// 发送端全部丢弃后 chanRecv 返回 undefined（无阻塞等待时）。
pub struct Channel {
    /// inner 通道状态锁（队列 + 接收等待队列；检查与登记的原子性由它保证）
    inner: Mutex<ChanState>,
    /// cv 线程接收者的等待通知
    cv: Condvar,
}

/// bi_new_channel 创建新 channel。
fn bi_new_channel(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    let chan = Channel {
        inner: Mutex::new(ChanState {
            queue: VecDeque::new(),
            recv_waiters: VecDeque::new(),
        }),
        cv: Condvar::new(),
    };
    Ok(Value::Native(Arc::new(Arc::new(chan))))
}

/// bi_chan_send 发送值到 channel（永不阻塞：无界通道）。
///
/// 有等待的接收任务 → 直接交接（wake_with 注入）；否则入队 + 通知线程接收者。
fn bi_chan_send(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.len() < 2 {
        return Err(crate::value::error_value("chanSend() 需要 2 个参数 (channel, value)"));
    }
    let chan = downcast::<Channel>(&args[0], "channel", "chanSend")?;
    let mut g = chan.inner.lock().unwrap();
    if let Some(t) = g.recv_waiters.pop_front() {
        // 直接交接：值注入给挂起的接收任务（不经队列）
        drop(g);
        scheduler::wake_task_with(&t, args[1].clone());
    } else {
        g.queue.push_back(args[1].clone());
        drop(g);
        chan.cv.notify_one();
    }
    Ok(Value::Undefined)
}

/// bi_chan_recv 从 channel 接收值。
///
/// 任务上下文：有数据取走返回；空队列时持锁挂起登记（注入型唤醒）。
/// 线程上下文：cv 阻塞循环。发送端全部丢弃时（无阻塞等待）返回 undefined。
fn bi_chan_recv(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("chanRecv() 需要 1 个参数"));
    }
    let chan = downcast::<Channel>(&args[0], "channel", "chanRecv")?;
    // 任务上下文：挂起等待（注入型：唤醒时值已替换占位符）
    if scheduler::current_task().is_some() {
        let task = scheduler::current_task().unwrap();
        let mut g = chan.inner.lock().unwrap();
        if let Some(v) = g.queue.pop_front() {
            return Ok(v);
        }
        // 持锁挂起登记：与发送方的"交接 or 入队"决策互斥，无丢唤醒窗口
        scheduler::park_current_task_inject(vm)?;
        g.recv_waiters.push_back(task);
        drop(g);
        return Ok(Value::Undefined); // 占位值，唤醒时被注入值替换
    }
    // 线程上下文：cv 阻塞循环
    let mut g = chan.inner.lock().unwrap();
    loop {
        if let Some(v) = g.queue.pop_front() {
            return Ok(v);
        }
        g = chan.cv.wait(g).unwrap();
    }
}

/// bi_chan_try_recv 非阻塞接收。
///
/// 暂无数据、通道已关闭（发送端全部丢弃）均返回 undefined（不区分）；
/// 不打扰挂起中的接收任务（只从数据队列取）。
fn bi_chan_try_recv(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("chanTryRecv() 需要 1 个参数"));
    }
    let chan = downcast::<Channel>(&args[0], "channel", "chanTryRecv")?;
    let mut g = chan.inner.lock().unwrap();
    match g.queue.pop_front() {
        Some(v) => Ok(v),
        None => Ok(Value::Undefined),
    }
}

// ============ Mutex（交接式注入型） ============

/// MuState 互斥锁内部状态（由 inner 锁保护）。
struct MuState {
    /// held 是否被持有。有任务等待者时，解锁把所有权随唤醒交接（held 保持 true）
    held: bool,
    /// waiters 等待锁的任务队列（FIFO；线程等待者走 cv）
    waiters: VecDeque<Arc<Task>>,
}

/// MutexT Sflang 互斥锁。
///
/// 交接语义：unlock 时若队首有等待任务，held 保持 true、直接唤醒该任务——
/// 其 lock() 从挂起点继续（值注入型：占位 undefined 即返回值），锁所有权
/// 随唤醒完成移交，无需重试竞争。线程等待者走 cv（经典 while 循环）。
pub struct MutexT {
    /// inner 锁状态（held + 任务等待队列）
    inner: Mutex<MuState>,
    /// cv 线程等待者的通知
    cv: Condvar,
}

impl MutexT {
    /// release 释放锁（供通用 close 函数复用）。已释放则无操作（幂等）。
    pub fn release(&self) {
        let mut g = self.inner.lock().unwrap();
        if let Some(t) = g.waiters.pop_front() {
            drop(g);
            scheduler::wake_task_with(&t, Value::Undefined);
        } else if g.held {
            g.held = false;
            self.cv.notify_one();
        }
    }
}

fn bi_new_mutex(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Native(Arc::new(Arc::new(MutexT {
        inner: Mutex::new(MuState { held: false, waiters: VecDeque::new() }),
        cv: Condvar::new(),
    }))))
}

/// bi_lock 阻塞获取互斥锁（临界区起点）。
///
/// 任务上下文：锁空闲取走；被占时持锁挂起登记（注入型：解锁方交接唤醒）。
/// 线程上下文：cv while 循环。
fn bi_lock(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("lock() 需要 1 个参数 (mutex)"));
    }
    let m = downcast::<MutexT>(&args[0], "mutex", "lock")?;
    let mut g = m.inner.lock().unwrap();
    if !g.held {
        g.held = true;
        return Ok(Value::Undefined);
    }
    // 任务上下文：挂起等待交接
    if let Some(task) = scheduler::current_task() {
        scheduler::park_current_task_inject(vm)?;
        g.waiters.push_back(task);
        drop(g);
        return Ok(Value::Undefined); // 占位：唤醒即已持有锁（交接语义）
    }
    // 线程上下文：cv 阻塞
    while g.held {
        g = m.cv.wait(g).unwrap();
    }
    g.held = true;
    Ok(Value::Undefined)
}

/// bi_unlock 释放互斥锁（临界区终点）。
///
/// 有等待任务 → 所有权交接（held 保持 true，唤醒队首）；否则置空闲并通知
/// 线程等待者。未持锁时幂等（无等待者且 held==false → 空操作）。
fn bi_unlock(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("unlock() 需要 1 个参数 (mutex)"));
    }
    let m = downcast::<MutexT>(&args[0], "mutex", "unlock")?;
    let mut g = m.inner.lock().unwrap();
    if let Some(t) = g.waiters.pop_front() {
        // 交接：held 保持 true，锁所有权随唤醒移交给 t
        drop(g);
        scheduler::wake_task_with(&t, Value::Undefined);
        return Ok(Value::Undefined);
    }
    g.held = false;
    drop(g);
    m.cv.notify_one();
    Ok(Value::Undefined)
}

/// bi_try_lock 非阻塞尝试获取锁，成功返回 true，失败（已被持有）返回 false。
fn bi_try_lock(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("tryLock() 需要 1 个参数 (mutex)"));
    }
    let m = downcast::<MutexT>(&args[0], "mutex", "tryLock")?;
    let mut g = m.inner.lock().unwrap();
    if g.held {
        Ok(Value::Bool(false))
    } else {
        g.held = true;
        Ok(Value::Bool(true))
    }
}

// ============ RWMutex（放行时登记状态的注入型） ============

/// RWState 读写锁的持有状态（由 inner 锁保护）。
struct RWState {
    /// readers 当前持读锁数（含放行时登记给被唤醒读者的）
    readers: u32,
    /// writer 是否有写者持有写锁（含放行时登记给被唤醒写者的）
    writer: bool,
    /// writer_waiting 排队写者数（>0 时新读者排队，防写者饥饿）
    writer_waiting: u32,
}

/// RWWaiter 读写锁的等待任务。
enum RWWaiter {
    /// Reader 等待读锁的任务
    Reader(Arc<Task>),
    /// Writer 等待写锁的任务
    Writer(Arc<Task>),
}

/// RWInner 读写锁内部（状态 + 等待队列）。
struct RWInner {
    /// state 持有状态
    state: RWState,
    /// waiters 等待任务队列（FIFO；线程等待者走 cv）
    waiters: VecDeque<RWWaiter>,
}

/// RWMutexT 读写锁。
///
/// 放行（admit）语义：释放端在持有 inner 锁时按 FIFO 放行等待任务，并同步
/// 登记其持有状态（读者 readers+1 / 写者 writer=true），被唤醒者从挂起点
/// 继续（注入型）即已持有对应锁。写者放行条件：无读者；写者排队时新读者
/// 排队（防写者饥饿）。线程等待者走 cv（while 循环重查状态）。
/// 单一 inner 锁保护全部状态，无 ABBA 死锁。
pub struct RWMutexT {
    /// inner 状态 + 等待队列
    inner: Mutex<RWInner>,
    /// cv 线程等待者的通知
    cv: Condvar,
}

impl RWMutexT {
    /// admit 放行等待任务（须持有 inner 锁；按 FIFO，同步登记持有状态）。
    fn admit(g: &mut RWInner) {
        loop {
            let can_pop = match g.waiters.front() {
                Some(RWWaiter::Reader(_)) => !g.state.writer,
                Some(RWWaiter::Writer(_)) => !g.state.writer && g.state.readers == 0,
                None => false,
            };
            if !can_pop {
                break;
            }
            match g.waiters.pop_front().unwrap() {
                RWWaiter::Reader(t) => {
                    g.state.readers += 1;
                    scheduler::wake_task_with(&t, Value::Undefined);
                }
                RWWaiter::Writer(t) => {
                    g.state.writer_waiting -= 1;
                    g.state.writer = true;
                    scheduler::wake_task_with(&t, Value::Undefined);
                    break; // 写者独占，停止放行
                }
            }
        }
    }

    /// release 释放锁（供通用 close 复用）：优先写锁，其次一个读锁。幂等。
    pub fn release(&self) {
        let mut g = self.inner.lock().unwrap();
        if g.state.writer {
            g.state.writer = false;
        } else if g.state.readers > 0 {
            g.state.readers -= 1;
        } else {
            return; // 无锁可释放
        }
        Self::admit(&mut g);
        drop(g);
        self.cv.notify_all();
    }
}

fn bi_new_rwmutex(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Native(Arc::new(Arc::new(RWMutexT {
        inner: Mutex::new(RWInner {
            state: RWState { readers: 0, writer: false, writer_waiting: 0 },
            waiters: VecDeque::new(),
        }),
        cv: Condvar::new(),
    }))))
}

/// bi_rlock 获取读锁（共享）。
///
/// 有写者持有或有写者排队时排队（防写者饥饿）；任务挂起 / 线程 cv 等待。
fn bi_rlock(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("rlock() 需要 1 个参数 (rwmutex)"));
    }
    let m = downcast::<RWMutexT>(&args[0], "rwmutex", "rlock")?;
    let mut g = m.inner.lock().unwrap();
    if !g.state.writer && g.state.writer_waiting == 0 {
        g.state.readers += 1;
        return Ok(Value::Undefined);
    }
    if let Some(task) = scheduler::current_task() {
        scheduler::park_current_task_inject(vm)?;
        g.waiters.push_back(RWWaiter::Reader(task));
        drop(g);
        return Ok(Value::Undefined); // 放行时已登记 readers+1
    }
    while g.state.writer || g.state.writer_waiting > 0 {
        g = m.cv.wait(g).unwrap();
    }
    g.state.readers += 1;
    Ok(Value::Undefined)
}

/// bi_runlock 释放读锁。读者归零时放行等待者（写者优先条件满足时）。
fn bi_runlock(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("runlock() 需要 1 个参数 (rwmutex)"));
    }
    let m = downcast::<RWMutexT>(&args[0], "rwmutex", "runlock")?;
    let mut g = m.inner.lock().unwrap();
    if g.state.readers > 0 {
        g.state.readers -= 1;
    }
    RWMutexT::admit(&mut g);
    drop(g);
    m.cv.notify_all();
    Ok(Value::Undefined)
}

/// bi_wlock 获取写锁（独占）。等待期间置 writer_waiting，新读者排队。
fn bi_wlock(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("wlock() 需要 1 个参数 (rwmutex)"));
    }
    let m = downcast::<RWMutexT>(&args[0], "rwmutex", "wlock")?;
    let mut g = m.inner.lock().unwrap();
    if !g.state.writer && g.state.readers == 0 {
        g.state.writer = true;
        return Ok(Value::Undefined);
    }
    if let Some(task) = scheduler::current_task() {
        g.state.writer_waiting += 1;
        scheduler::park_current_task_inject(vm)?;
        g.waiters.push_back(RWWaiter::Writer(task));
        drop(g);
        return Ok(Value::Undefined); // 放行时已登记 writer=true
    }
    g.state.writer_waiting += 1;
    while g.state.writer || g.state.readers > 0 {
        g = m.cv.wait(g).unwrap();
    }
    g.state.writer_waiting -= 1;
    g.state.writer = true;
    Ok(Value::Undefined)
}

/// bi_wunlock 释放写锁并放行等待者。
fn bi_wunlock(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("wunlock() 需要 1 个参数 (rwmutex)"));
    }
    let m = downcast::<RWMutexT>(&args[0], "rwmutex", "wunlock")?;
    let mut g = m.inner.lock().unwrap();
    g.state.writer = false;
    RWMutexT::admit(&mut g);
    drop(g);
    m.cv.notify_all();
    Ok(Value::Undefined)
}

// ============ WaitGroup（等待-重试型） ============

/// WaitGroupT 等待组。
///
/// wgWait 为等待-重试型挂起：wgDone 使计数归零时唤醒全部等待任务，
/// 被唤醒者回退重查计数（仍非零——如并发 wgAdd——则继续等），语义稳健。
/// 线程等待者走 cv。
pub struct WaitGroupT {
    /// counter 等待计数
    counter: AtomicI64,
    /// waiters 计数非零时挂起的等待任务（wgWait 登记；归零时全部唤醒）
    waiters: Mutex<VecDeque<Arc<Task>>>,
    /// cv 线程等待者的通知
    cv: Condvar,
    /// mu 保护计数更新的检查-写入原子性（与旧版一致）
    mu: Mutex<()>,
}

fn bi_new_waitgroup(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Native(Arc::new(Arc::new(WaitGroupT {
        counter: AtomicI64::new(0),
        waiters: Mutex::new(VecDeque::new()),
        cv: Condvar::new(),
        mu: Mutex::new(()),
    }))))
}

/// notify_zero 计数归零时的统一唤醒：唤醒全部任务等待者（重查型）+ 通知线程。
fn notify_zero(wg: &WaitGroupT) {
    let waiters: VecDeque<Arc<Task>> = {
        let mut w = wg.waiters.lock().unwrap();
        std::mem::take(&mut *w)
    };
    for t in waiters {
        scheduler::wake_task(&t); // 重查型：唤醒后重查计数
    }
    wg.cv.notify_all();
}

/// bi_wg_add 增加等待计数（n 可为负，对应 Done 批量）。
///
/// 计数加 n 溢出 i64 范围、或结果为负时返回错误（Go 语义：Add 不得使计数变负）。
fn bi_wg_add(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.len() < 2 {
        return Err(crate::value::error_value("wgAdd() 需要 2 个参数 (waitgroup, n)"));
    }
    let wg = downcast::<WaitGroupT>(&args[0], "waitgroup", "wgAdd")?;
    let n = args[1].to_int().ok_or_else(|| {
        crate::value::error_value("wgAdd() 第二个参数需为整数 (可能原因：参数顺序错误)")
    })?;
    let _g = wg.mu.lock().unwrap();
    let cur = wg.counter.load(Ordering::SeqCst);
    // 先 checked_add 再写入：直接 fetch_add 溢出时 debug 下会 panic、release 下回绕
    let new = cur.checked_add(n).ok_or_else(|| {
        crate::value::error_value(format!(
            "wgAdd() 计数溢出：{} + {} 超出 int 表示范围 (可能原因：n 过大)",
            cur, n
        ))
    })?;
    if new < 0 {
        return Err(crate::value::error_value(
            "wgAdd() 会使计数变负 (可能原因：Done 次数超过 Add)",
        ));
    }
    wg.counter.store(new, Ordering::SeqCst);
    if new == 0 {
        notify_zero(wg);
    }
    Ok(Value::Undefined)
}

/// bi_wg_done 完成一个等待（计数 -1）。归零时唤醒全部等待者。
fn bi_wg_done(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("wgDone() 需要 1 个参数 (waitgroup)"));
    }
    let wg = downcast::<WaitGroupT>(&args[0], "waitgroup", "wgDone")?;
    let _g = wg.mu.lock().unwrap();
    let prev = wg.counter.fetch_sub(1, Ordering::SeqCst);
    if prev <= 0 {
        wg.counter.fetch_add(1, Ordering::SeqCst);
        return Err(crate::value::error_value(
            "wgDone() 计数已为 0 (可能原因：Done 次数超过 Add)",
        ));
    }
    if wg.counter.load(Ordering::SeqCst) == 0 {
        notify_zero(wg);
    }
    Ok(Value::Undefined)
}

/// bi_wg_wait 阻塞至计数归零（等待-重试型挂起；线程上下文 cv 循环）。
fn bi_wg_wait(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("wgWait() 需要 1 个参数 (waitgroup)"));
    }
    let wg = downcast::<WaitGroupT>(&args[0], "waitgroup", "wgWait")?;
    // 任务上下文：计数非零 → 挂起（重查型：唤醒后重新执行本调用）
    if scheduler::current_task().is_some() {
        let _g = wg.mu.lock().unwrap();
        if wg.counter.load(Ordering::SeqCst) == 0 {
            return Ok(Value::Undefined);
        }
        scheduler::park_current_task_retry(vm)?;
        wg.waiters.lock().unwrap().push_back(scheduler::current_task().unwrap());
        drop(_g);
        return Ok(Value::Undefined); // 占位：唤醒后机器回退重试
    }
    // 线程上下文：cv 循环
    let mut g = wg.mu.lock().unwrap();
    while wg.counter.load(Ordering::SeqCst) != 0 {
        g = wg.cv.wait(g).unwrap();
    }
    Ok(Value::Undefined)
}

// ============ Semaphore（等待-重试型） ============

/// SemaphoreT 计数信号量。
///
/// semAcquire 为等待-重试型挂起：semRelease 空位 +1 时唤醒一个等待任务，
/// 被唤醒者重查空位（可能被线程竞争者先取走——则重新挂起），语义稳健。
pub struct SemaphoreT {
    /// count 剩余空位
    count: AtomicI64,
    /// waiters 等待空位的任务队列（FIFO；线程等待者走 cv）
    waiters: Mutex<VecDeque<Arc<Task>>>,
    /// cv 线程等待者的通知
    cv: Condvar,
    /// mu 保护检查-扣减原子性
    mu: Mutex<()>,
}

fn bi_new_semaphore(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    // 无参数时默认 1；有参数则必须是整数且 > 0，非法值返回错误（不静默取默认值）
    let n = if args.is_empty() {
        1
    } else {
        args[0].to_int().ok_or_else(|| {
            crate::value::error_value(
                "newSemaphore() 参数需为整数 (可能原因：传入了字符串、undefined 等非整数值)",
            )
        })?
    };
    if n <= 0 {
        return Err(crate::value::error_value(
            "newSemaphore() 最大并发数必须为正整数（合法范围：>= 1）(可能原因：传入了 0 或负数)",
        ));
    }
    Ok(Value::Native(Arc::new(Arc::new(SemaphoreT {
        count: AtomicI64::new(n),
        waiters: Mutex::new(VecDeque::new()),
        cv: Condvar::new(),
        mu: Mutex::new(()),
    }))))
}

/// bi_sem_acquire 获取信号量（P 操作）。
fn bi_sem_acquire(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("semAcquire() 需要 1 个参数 (semaphore)"));
    }
    let sem = downcast::<SemaphoreT>(&args[0], "semaphore", "semAcquire")?;
    // 任务上下文
    if scheduler::current_task().is_some() {
        let _g = sem.mu.lock().unwrap();
        if sem.count.load(Ordering::SeqCst) > 0 {
            sem.count.fetch_sub(1, Ordering::SeqCst);
            return Ok(Value::Undefined);
        }
        scheduler::park_current_task_retry(vm)?;
        sem.waiters.lock().unwrap().push_back(scheduler::current_task().unwrap());
        drop(_g);
        return Ok(Value::Undefined); // 占位：唤醒后回退重试
    }
    // 线程上下文
    let mut g = sem.mu.lock().unwrap();
    while sem.count.load(Ordering::SeqCst) <= 0 {
        g = sem.cv.wait(g).unwrap();
    }
    sem.count.fetch_sub(1, Ordering::SeqCst);
    Ok(Value::Undefined)
}

/// bi_sem_release 释放信号量（V 操作）。空位 +1，唤醒一个任务等待者 + 通知线程。
fn bi_sem_release(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("semRelease() 需要 1 个参数 (semaphore)"));
    }
    let sem = downcast::<SemaphoreT>(&args[0], "semaphore", "semRelease")?;
    let _g = sem.mu.lock().unwrap();
    sem.count.fetch_add(1, Ordering::SeqCst);
    let t = sem.waiters.lock().unwrap().pop_front();
    drop(_g);
    if let Some(t) = t {
        scheduler::wake_task(&t); // 重查型：唤醒后重查空位
    }
    sem.cv.notify_one();
    Ok(Value::Undefined)
}

// ============ Once（值注入型等待者） ============

/// ExecId 首次执行者的身份（递归检测用；任务与线程统一编号空间不同）。
#[derive(Clone, Copy, PartialEq)]
enum ExecId {
    /// Thread 线程上下文的执行者
    Thread(std::thread::ThreadId),
    /// Task 任务上下文的执行者
    Task(u64),
}

/// current_exec_id 当前执行者身份。
fn current_exec_id() -> ExecId {
    match scheduler::current_task() {
        Some(t) => ExecId::Task(t.id),
        None => ExecId::Thread(std::thread::current().id()),
    }
}

/// OnceState once 的内部状态（由单一互斥锁保护）。
struct OnceState {
    /// phase 执行阶段：0 = 未开始，1 = 执行中，2 = 已完成
    phase: u8,
    /// executor 正在执行回调者的身份（检测同执行者递归调用）
    executor: Option<ExecId>,
    /// result 首次回调的执行结果（phase == 2 后有效，供后续调用克隆返回）
    result: Option<Result<Value, Value>>,
}

/// OnceT 单次执行原语，onceDo(once, func) 保证 func 只执行一次（并发安全）。
///
/// 并发调用方（任务）挂起等待首次执行完成，完成时以结果注入唤醒（直接取得
/// 结果，无需重试）；线程调用方走 cv。不直接使用 std::sync::Once 的原因：
///   - Once 的闭包无法把 Result 传出，回调的错误会被吞掉；
///   - 同一线程在回调内递归调用同一 once 时会永久阻塞。
/// 此处用 Mutex + Condvar + 任务等待队列自行实现，支持错误传播与递归检测。
pub struct OnceT {
    /// state 状态（phase/executor/result）
    state: Mutex<OnceState>,
    /// waiters 等待首次执行完成的任务（完成时以结果注入唤醒）
    waiters: Mutex<VecDeque<Arc<Task>>>,
    /// cv 线程等待者的通知
    cv: Condvar,
}

fn bi_new_once(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Native(Arc::new(Arc::new(OnceT {
        state: Mutex::new(OnceState { phase: 0, executor: None, result: None }),
        waiters: Mutex::new(VecDeque::new()),
        cv: Condvar::new(),
    }))))
}

/// OncePanicGuard onceDo 回调执行期间的 unwind 保护。
///
/// 若回调（用户代码）内部 panic 而被外层 catch_unwind 捕获，此守卫在
/// 展开时把 phase 从 1 复位为 0 并唤醒等待者，使后续调用可重新执行，
/// 而不是永远停留在"执行中"导致其他线程永久阻塞。
struct OncePanicGuard<'a> {
    once: &'a OnceT,
    /// 是否仍处于保护状态（正常完成后解除，避免误复位 phase=2）
    armed: bool,
}

impl Drop for OncePanicGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            let mut g = self.once.state.lock().unwrap();
            if g.phase == 1 {
                g.phase = 0;
                g.executor = None;
            }
            drop(g);
            // 假性完成通知：唤醒等待者重查（phase 已复位 0 → 重新执行回调）
            let waiters: VecDeque<Arc<Task>> =
                std::mem::take(&mut *self.once.waiters.lock().unwrap());
            for t in waiters {
                scheduler::wake_task(&t);
            }
            self.once.cv.notify_all();
        }
    }
}

/// finish_once 首次执行完成：记录结果并唤醒全部等待者（重查型：唤醒后
/// 重新执行 onceDo，phase==2 分支返回原始 Result，错误以抛出语义传播）。
fn finish_once(once: &OnceT, res: &Result<Value, Value>) {
    let mut g = once.state.lock().unwrap();
    g.phase = 2;
    g.executor = None;
    g.result = Some(res.clone());
    drop(g);
    let waiters: VecDeque<Arc<Task>> = std::mem::take(&mut *once.waiters.lock().unwrap());
    for t in waiters {
        scheduler::wake_task(&t);
    }
    once.cv.notify_all();
}

/// bi_once_do 保证传入的函数只执行一次（并发安全）。
///
/// 语义：
///   - 首次调用执行 fn，其结果（含错误）被记录，并原样返回给调用方；
///   - 并发调用阻塞等待，后续调用直接返回首次执行的结果（错误同样返回，不吞掉）；
///   - 回调内同执行者递归调用同一 once 时返回错误（而不是永久阻塞）；
///   - 回调 panic（unwind）时自动复位状态，后续调用可重试；
///   - 回调内不允许再挂起当前任务（call_function_value 已禁用并返回明确错误）。
fn bi_once_do(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.len() < 2 {
        return Err(crate::value::error_value("onceDo() 需要 2 个参数 (once, func)"));
    }
    let once = downcast::<OnceT>(&args[0], "once", "onceDo")?;
    let func = args[1].clone();
    let me = current_exec_id();
    let mut g = once.state.lock().unwrap();
    loop {
        match g.phase {
            0 => {
                // 抢到执行权：标记执行中并记录身份，然后释放锁执行回调
                g.phase = 1;
                g.executor = Some(me);
                drop(g);
                // 回调 panic 时由守卫复位状态
                let mut guard = OncePanicGuard { once, armed: true };
                let res = vm.call_function_value(func, Vec::new());
                guard.armed = false;
                drop(guard);
                // 记录结果并唤醒所有等待者
                finish_once(once, &res);
                return res;
            }
            1 => {
                if g.executor == Some(me) {
                    // 同执行者递归调用：等自己完成会永久死锁，直接返回错误
                    return Err(crate::value::error_value(
                        "onceDo() 回调内不能递归调用同一 once (可能原因：回调函数内部再次 onceDo 了同一个 once 对象)",
                    ));
                }
                // 其他执行者正在执行：等待其完成。
                // 任务=等待-重试型挂起（唤醒后重新执行本调用，phase==2 分支取原始
                // Result——错误以"抛出"语义传播，与直接执行者及旧版一致；
                // 若用值注入会把错误当返回值传入，语义不一致）
                if let Some(task) = scheduler::current_task() {
                    scheduler::park_current_task_retry(vm)?;
                    once.waiters.lock().unwrap().push_back(task);
                    drop(g);
                    return Ok(Value::Undefined); // 占位：唤醒后机器回退重试
                }
                g = once.cv.wait(g).unwrap();
            }
            _ => {
                // 已完成：返回首次执行的结果（错误也照常返回）
                return g
                    .result
                    .clone()
                    .unwrap_or_else(|| Ok(Value::Undefined));
            }
        }
    }
}

// ============ poolRun 有界工作池 ============

/// bi_pool_run 有界并发执行：workers 个线程消费 items，结果按输入顺序返回。
///
/// 实现要点：
///   - 原子索引分发任务：工作线程循环 next.fetch_add(1) 抢占下一项，
///     无锁分发、天然负载均衡（慢线程少拿任务）
///   - 结果槽按输入顺序预分配（Arc<Vec<Arc<Mutex<Option<Value>>>>>），
///     工作线程按下标写入，保证结果顺序与 items 一一对应
///   - 每个工作线程独立 VM（共享 globals 与输出），与 run 任务同模型；
///     内置函数表已全局化（阶段0a），每线程不再重复注册
///   - fn 对某项抛异常：该项结果槽写入 error 值（符合"返回错误对象为主"约定），
///     不中断其余项
fn bi_pool_run(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.len() < 3 {
        return Err(crate::value::error_value(
            "poolRun() 需要 3 个参数 (fn, workers, items)",
        ));
    }
    // 参数校验：函数 / 工作线程数 / 数组，非法时给出 AI 友好错误
    let func = args[0].clone();
    if !matches!(func, Value::Func(_) | Value::Builtin(_)) {
        return Err(crate::value::error_value(format!(
            "poolRun() 第 1 个参数应为函数值，得到 {} (可能原因：参数顺序错误，应为 poolRun(fn, workers, items))",
            func.type_name()
        )));
    }
    let workers = args[1].to_int().ok_or_else(|| {
        crate::value::error_value(
            "poolRun() 第 2 个参数 workers 需为整数 (可能原因：传入了字符串或 undefined)",
        )
    })?;
    if workers < 1 || workers > 1024 {
        return Err(crate::value::error_value(format!(
            "poolRun() workers 超出范围: {} (合法范围 1..=1024; 可能原因：worker 数传了 0/负数或过大值)",
            workers
        )));
    }
    let items: Arc<Vec<Value>> = match &args[2] {
        Value::Array(a) => Arc::new(a.lock().unwrap().clone()),
        other => {
            return Err(crate::value::error_value(format!(
                "poolRun() 第 3 个参数应为数组，得到 {} (可能原因：参数顺序错误)",
                other.type_name()
            )))
        }
    };
    // 空任务直接返回空数组（不启动线程）
    if items.is_empty() {
        return Ok(Value::Array(Arc::new(Mutex::new(Vec::new()))));
    }

    // 原子任务索引：工作线程抢占下一项
    let next = Arc::new(AtomicUsize::new(0));
    // 结果槽：预分配、按下标写入（保证结果顺序与输入一致）
    let slots: Arc<Vec<Arc<Mutex<Option<Value>>>>> = Arc::new(
        (0..items.len()).map(|_| Arc::new(Mutex::new(None))).collect(),
    );

    // 工作线程数不超过任务数（不启多余线程）
    let n_workers = (workers as usize).min(items.len());
    let globals = vm.globals_handle();
    let out = vm.output_handle();

    let mut handles = Vec::with_capacity(n_workers);
    for _ in 0..n_workers {
        let func = func.clone();
        let items = items.clone();
        let next = next.clone();
        let slots = slots.clone();
        let globals = globals.clone();
        let out = out.clone();
        handles.push(std::thread::spawn(move || {
            // 工作线程独立 VM（共享全局环境与输出），与 run 任务同模型
            let mut wvm = VM::new();
            wvm.set_globals_handle(globals);
            wvm.set_output_handle(out);
            loop {
                let idx = next.fetch_add(1, Ordering::SeqCst);
                if idx >= items.len() {
                    break;
                }
                // fn 对该项抛异常：统一包装为 error 值写入结果槽，不中断其余项
                // （throw 的非 error 值也包装，保证 results 中异常项恒为 error 类型，
                //   脚本侧用 isErr 判断即可，符合"返回错误对象为主"的约定）
                let val = match wvm.call_function_value(func.clone(), vec![items[idx].clone()]) {
                    Ok(v) => v,
                    Err(e) => match e {
                        Value::Error(_) => e,
                        other => crate::value::error_value(format!(
                            "poolRun: 任务处理函数抛出异常: {}",
                            other.to_str()
                        )),
                    },
                };
                *slots[idx].lock().unwrap() = Some(val);
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }

    // 汇总结果（顺序与 items 一致；每个槽必被恰好写入一次）
    let mut results = Vec::with_capacity(items.len());
    for slot in slots.iter() {
        let v = slot.lock().unwrap().take().unwrap_or(Value::Undefined);
        results.push(v);
    }
    Ok(Value::Array(Arc::new(Mutex::new(results))))
}
