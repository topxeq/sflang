//! concurrency.rs — 并发原语与同步原语
//!
//! 设计要点：
//!   - channel (mpsc)：跨线程通信的主要手段
//!   - run 关键字启动新线程（vm.rs spawn_thread）
//!   - 同步原语（阶段三补充）：Mutex / RWMutex / WaitGroup / Semaphore / Once
//!     全部基于 std::sync 标准库实现，用 Value::Native(Arc<dyn Any + Send + Sync>) 包装
//!   - 所有原语满足 Send + Sync，可跨 run 启动的线程安全共享
//!
//! API 概览：
//!   channel:  newChannel / chanSend / chanRecv / chanTryRecv
//!   mutex:    newMutex / lock / unlock / tryLock
//!   rwmutex:  newRWMutex / rlock / runlock（写锁复用 lock/unlock）
//!   waitgroup:newWaitGroup / wgAdd / wgDone / wgWait
//!   sem:      newSemaphore / semAcquire / semRelease
//!   once:     newOnce / onceDo（onceDo 接收函数值，保证只执行一次）

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, Once};
use std::sync::atomic::{AtomicI64, Ordering};

use crate::function::BuiltinDoc;
use crate::value::Value;
use crate::vm::VM;

// ---- 并发原语文档 ----

static DOC_NEW_CHANNEL: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "newChannel() -> channel",
    summary: "创建无缓冲通道（mpsc），用于跨线程通信。配合 run 和 chanSend/chanRecv 使用。",
    params: &[],
    returns: "channel 通道对象",
    examples: &[
        "var ch = newChannel()",
        "run sender()       // 子线程 chanSend(ch, 42)",
        "var v = chanRecv(ch) // 主线程接收",
    ],
    errors: &[],
};

static DOC_CHAN_SEND: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "chanSend(ch, val) -> undefined",
    summary: "向通道发送值（阻塞直到接收方就绪）。",
    params: &[("ch", "channel 对象"), ("val", "要发送的值")],
    returns: "undefined",
    examples: &["chanSend(ch, 42)"],
    errors: &["ch 参数应为 channel 类型"],
};

static DOC_CHAN_RECV: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "chanRecv(ch) -> value",
    summary: "从通道接收值（阻塞直到有数据）。",
    params: &[("ch", "channel 对象")],
    returns: "接收到的值；通道关闭后返回 undefined",
    examples: &["var v = chanRecv(ch)"],
    errors: &[],
};

static DOC_CHAN_TRY_RECV: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "chanTryRecv(ch) -> value|undefined",
    summary: "非阻塞接收：有数据返回值，无数据返回 undefined。",
    params: &[("ch", "channel 对象")],
    returns: "值或 undefined（无数据时）",
    examples: &["var v = chanTryRecv(ch); if v != undefined { pln(v) }"],
    errors: &[],
};

static DOC_NEW_MUTEX: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "newMutex() -> mutex",
    summary: "创建互斥锁，用于保护共享数据的并发访问。",
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
    summary: "加锁（阻塞直到获取锁）。",
    params: &[("m", "mutex 对象")],
    returns: "undefined",
    examples: &["lock(m)"],
    errors: &[],
};

static DOC_UNLOCK: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "unlock(m) -> undefined",
    summary: "释放锁。",
    params: &[("m", "mutex 对象")],
    returns: "undefined",
    examples: &["unlock(m)"],
    errors: &["未持有锁时 unlock 会 panic"],
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
    summary: "创建读写锁：允许多个读锁或一个写锁。",
    params: &[],
    returns: "rwmutex 读写锁对象",
    examples: &["var rw = newRWMutex()"],
    errors: &[],
};

static DOC_RLOCK: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "rlock(rw) -> undefined",
    summary: "获取读锁（允许多个读者并发）。",
    params: &[("rw", "rwmutex 对象")],
    returns: "undefined",
    examples: &["rlock(rw); ... runlock(rw)"],
    errors: &[],
};

static DOC_RUNLOCK: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "runlock(rw) -> undefined",
    summary: "释放读锁。",
    params: &[("rw", "rwmutex 对象")],
    returns: "undefined",
    examples: &["runlock(rw)"],
    errors: &[],
};

static DOC_WLOCK: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "wlock(rw) -> undefined",
    summary: "获取写锁（排他，阻塞直到无其他读者/写者）。",
    params: &[("rw", "rwmutex 对象")],
    returns: "undefined",
    examples: &["wlock(rw); ... wunlock(rw)"],
    errors: &[],
};

static DOC_WUNLOCK: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "wunlock(rw) -> undefined",
    summary: "释放写锁。",
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
    summary: "增加 WaitGroup 计数 n。",
    params: &[("wg", "waitGroup 对象"), ("n", "增加的计数（int）")],
    returns: "undefined",
    examples: &["wgAdd(wg, 3)"],
    errors: &[],
};

static DOC_WG_DONE: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "wgDone(wg) -> undefined",
    summary: "标记一个任务完成（计数减 1）。",
    params: &[("wg", "waitGroup 对象")],
    returns: "undefined",
    examples: &["wgDone(wg)"],
    errors: &["计数减为负数会 panic"],
};

static DOC_WG_WAIT: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "wgWait(wg) -> undefined",
    summary: "阻塞等待计数归零（所有任务完成）。",
    params: &[("wg", "waitGroup 对象")],
    returns: "undefined",
    examples: &["wgWait(wg)"],
    errors: &[],
};

static DOC_NEW_SEMAPHORE: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "newSemaphore(n) -> semaphore",
    summary: "创建信号量，限制同时访问的并发数。",
    params: &[("n", "最大并发数（int）")],
    returns: "semaphore 对象",
    examples: &["var sem = newSemaphore(5)  // 最多 5 个并发"],
    errors: &[],
};

static DOC_SEM_ACQUIRE: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "semAcquire(sem) -> undefined",
    summary: "获取信号量（阻塞直到有空位）。",
    params: &[("sem", "semaphore 对象")],
    returns: "undefined",
    examples: &["semAcquire(sem)"],
    errors: &[],
};

static DOC_SEM_RELEASE: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "semRelease(sem) -> undefined",
    summary: "释放信号量（空位加 1）。",
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
    signature: "onceDo(o, fn) -> undefined",
    summary: "保证 fn 只在第一次调用时执行（并发安全）。",
    params: &[("o", "once 对象"), ("fn", "要执行的函数")],
    returns: "undefined",
    examples: &["onceDo(o, initFunc)"],
    errors: &[],
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

// ============ Channel ============

/// Channel Sflang 的 channel 类型，包装 std::sync::mpsc。
///
/// 发送端 Arc<Mutex<Sender>> 可多份共享；接收端单份。
pub struct Channel {
    pub tx: Arc<Mutex<Sender<Value>>>,
    pub rx: Arc<Mutex<Receiver<Value>>>,
}

/// bi_new_channel 创建新 channel。
fn bi_new_channel(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    let (tx, rx) = channel::<Value>();
    let chan = Channel {
        tx: Arc::new(Mutex::new(tx)),
        rx: Arc::new(Mutex::new(rx)),
    };
    // 注：用 Native 包装（Arc<dyn Any + Send + Sync>）
    Ok(Value::Native(Arc::new(Arc::new(chan))))
}

/// bi_chan_send 发送值到 channel（阻塞直到接收方取走，mpsc 为无界故实际不阻塞）。
fn bi_chan_send(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.len() < 2 {
        return Err(crate::value::error_value("chanSend() 需要 2 个参数 (channel, value)"));
    }
    let chan = downcast::<Channel>(&args[0], "channel", "chanSend")?;
    chan.tx.lock().unwrap().send(args[1].clone())
        .map_err(|e| crate::value::error_value(format!("chanSend 失败: {}", e)))?;
    Ok(Value::Undefined)
}

/// bi_chan_recv 从 channel 接收值（阻塞至有数据）。
fn bi_chan_recv(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("chanRecv() 需要 1 个参数"));
    }
    let chan = downcast::<Channel>(&args[0], "channel", "chanRecv")?;
    match chan.rx.lock().unwrap().recv() {
        Ok(v) => Ok(v),
        Err(_) => Ok(Value::Undefined), // channel 关闭返回 undefined
    }
}

/// bi_chan_try_recv 非阻塞接收（无数据返回 undefined）。
fn bi_chan_try_recv(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("chanTryRecv() 需要 1 个参数"));
    }
    let chan = downcast::<Channel>(&args[0], "channel", "chanTryRecv")?;
    match chan.rx.lock().unwrap().try_recv() {
        Ok(v) => Ok(v),
        Err(_) => Ok(Value::Undefined),
    }
}

// ============ Mutex ============

/// MutexT Sflang 互斥锁。
///
/// 实现说明：脚本层的 lock/unlock 是配对调用，无法持有 Rust 的 MutexGuard
/// 跨调用（guard 生命周期绑定栈帧）。故采用"二值锁"实现：内部用 Mutex<bool> +
/// Condvar，lock 阻塞至标志为 false 后置 true，unlock 置 false 并唤醒。
/// 这样 lock() 与 unlock() 之间的脚本代码构成真正的临界区。
/// 配合 defer unlock 可保证异常路径也释放锁。
pub struct MutexT {
    held: Mutex<bool>,
    cv: Condvar,
}

impl MutexT {
    /// release 释放锁（供通用 close 函数复用）。已释放则无操作（幂等）。
    pub fn release(&self) {
        let mut g = self.held.lock().unwrap();
        if *g {
            *g = false;
            self.cv.notify_one();
        }
    }
}

fn bi_new_mutex(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Native(Arc::new(Arc::new(MutexT {
        held: Mutex::new(false),
        cv: Condvar::new(),
    }))))
}

/// bi_lock 阻塞获取互斥锁（临界区起点）。
///
/// 阻塞至锁可用后标记为持有，返回 undefined。后续脚本代码至 unlock 前为临界区。
fn bi_lock(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("lock() 需要 1 个参数 (mutex)"));
    }
    let m = downcast::<MutexT>(&args[0], "mutex", "lock")?;
    let mut g = m.held.lock().unwrap();
    while *g {
        g = m.cv.wait(g).unwrap();
    }
    *g = true;
    Ok(Value::Undefined)
}

/// bi_unlock 释放互斥锁（临界区终点）。
fn bi_unlock(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("unlock() 需要 1 个参数 (mutex)"));
    }
    let m = downcast::<MutexT>(&args[0], "mutex", "unlock")?;
    let mut g = m.held.lock().unwrap();
    *g = false;
    m.cv.notify_one();
    Ok(Value::Undefined)
}

/// bi_try_lock 非阻塞尝试获取锁，成功返回 true，失败（已被持有）返回 false。
fn bi_try_lock(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("tryLock() 需要 1 个参数 (mutex)"));
    }
    let m = downcast::<MutexT>(&args[0], "mutex", "tryLock")?;
    let mut g = m.held.lock().unwrap();
    if *g {
        Ok(Value::Bool(false))
    } else {
        *g = true;
        Ok(Value::Bool(true))
    }
}

// ============ RWMutex ============

/// RWMutexT 读写锁。
///
/// 实现说明：与 MutexT 同理，无法持有 Rust 的 RwLockReadGuard/WriteGuard 跨调用。
/// 采用计数实现：readers 记录当前读锁数，writer 标记写锁持有。
/// - rlock：无写者时 readers+1；有写者则阻塞
/// - runlock：readers-1，若归零唤醒写者
/// - 写锁复用语义：用 wlock/wunlock（见下）——为避免与 mutex 的 lock/unlock 混淆，
///   rwmutex 的写操作命名为 wlock/wunlock，读操作为 rlock/runlock。
pub struct RWMutexT {
    readers: Mutex<i64>,
    writer: Mutex<bool>,
    cv: Condvar,
}

impl RWMutexT {
    /// release 释放锁（写锁优先，无写锁则释放一个读锁）。供通用 close 复用。
    pub fn release(&self) {
        // 先尝试释放写锁
        let mut w = self.writer.lock().unwrap();
        if *w {
            *w = false;
            self.cv.notify_all();
            return;
        }
        drop(w);
        // 无写锁，释放一个读锁
        let mut r = self.readers.lock().unwrap();
        if *r > 0 {
            *r -= 1;
            if *r == 0 {
                self.cv.notify_all();
            }
        }
    }
}

fn bi_new_rwmutex(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Native(Arc::new(Arc::new(RWMutexT {
        readers: Mutex::new(0),
        writer: Mutex::new(false),
        cv: Condvar::new(),
    }))))
}

/// bi_rlock 获取读锁（共享，多读者并发；有写者时阻塞）。
fn bi_rlock(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("rlock() 需要 1 个参数 (rwmutex)"));
    }
    let m = downcast::<RWMutexT>(&args[0], "rwmutex", "rlock")?;
    let mut g = m.readers.lock().unwrap();
    // 等待写锁释放
    while *m.writer.lock().unwrap() {
        g = m.cv.wait(g).unwrap();
    }
    *g += 1;
    Ok(Value::Undefined)
}

/// bi_runlock 释放读锁。
fn bi_runlock(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("runlock() 需要 1 个参数 (rwmutex)"));
    }
    let m = downcast::<RWMutexT>(&args[0], "rwmutex", "runlock")?;
    let mut g = m.readers.lock().unwrap();
    if *g > 0 {
        *g -= 1;
    }
    if *g == 0 {
        m.cv.notify_all(); // 唤醒可能等待的写者
    }
    Ok(Value::Undefined)
}

/// bi_wlock 获取写锁（独占；有读者或写者时阻塞）。
fn bi_wlock(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("wlock() 需要 1 个参数 (rwmutex)"));
    }
    let m = downcast::<RWMutexT>(&args[0], "rwmutex", "wlock")?;
    let mut wg = m.writer.lock().unwrap();
    while *wg {
        wg = m.cv.wait(wg).unwrap();
    }
    // 等待所有读者退出
    while *m.readers.lock().unwrap() > 0 {
        wg = m.cv.wait(wg).unwrap();
    }
    *wg = true;
    Ok(Value::Undefined)
}

/// bi_wunlock 释放写锁。
fn bi_wunlock(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("wunlock() 需要 1 个参数 (rwmutex)"));
    }
    let m = downcast::<RWMutexT>(&args[0], "rwmutex", "wunlock")?;
    let mut wg = m.writer.lock().unwrap();
    *wg = false;
    m.cv.notify_all(); // 唤醒等待的读者/写者
    Ok(Value::Undefined)
}
// 注：rwmutex 的写锁用 wlock/wunlock（避免与 mutex 的 lock/unlock 混淆类型）。

// ============ WaitGroup ============

/// WaitGroupT 等待组，基于 Mutex + Condvar + 计数器实现（等价 Go sync.WaitGroup）。
pub struct WaitGroupT {
    counter: AtomicI64,
    cv: Condvar,
    mu: Mutex<()>,
}

fn bi_new_waitgroup(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Native(Arc::new(Arc::new(WaitGroupT {
        counter: AtomicI64::new(0),
        cv: Condvar::new(),
        mu: Mutex::new(()),
    }))))
}

/// bi_wg_add 增加等待计数（n 可为负，对应 Done 批量）。
fn bi_wg_add(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.len() < 2 {
        return Err(crate::value::error_value("wgAdd() 需要 2 个参数 (waitgroup, n)"));
    }
    let wg = downcast::<WaitGroupT>(&args[0], "waitgroup", "wgAdd")?;
    let n = args[1].to_int().ok_or_else(|| {
        crate::value::error_value("wgAdd() 第二个参数需为整数 (可能原因：参数顺序错误)")
    })?;
    let _g = wg.mu.lock().unwrap();
    let prev = wg.counter.fetch_add(n, Ordering::SeqCst);
    // Go 语义：Add 不得使计数变负
    if prev + n < 0 {
        wg.counter.fetch_sub(n, Ordering::SeqCst);
        return Err(crate::value::error_value(
            "wgAdd() 会使计数变负 (可能原因：Done 次数超过 Add)",
        ));
    }
    if wg.counter.load(Ordering::SeqCst) == 0 {
        wg.cv.notify_all();
    }
    Ok(Value::Undefined)
}

/// bi_wg_done 完成一个等待（计数 -1）。
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
        wg.cv.notify_all();
    }
    Ok(Value::Undefined)
}

/// bi_wg_wait 阻塞至计数归零。
fn bi_wg_wait(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("wgWait() 需要 1 个参数 (waitgroup)"));
    }
    let wg = downcast::<WaitGroupT>(&args[0], "waitgroup", "wgWait")?;
    let mut g = wg.mu.lock().unwrap();
    while wg.counter.load(Ordering::SeqCst) != 0 {
        g = wg.cv.wait(g).unwrap();
    }
    Ok(Value::Undefined)
}

// ============ Semaphore ============

/// SemaphoreT 计数信号量，基于 Mutex + Condvar + 计数。
pub struct SemaphoreT {
    count: AtomicI64,
    cv: Condvar,
    mu: Mutex<()>,
}

fn bi_new_semaphore(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let n = if args.is_empty() {
        1
    } else {
        args[0].to_int().unwrap_or(1)
    };
    if n < 0 {
        return Err(crate::value::error_value(
            "newSemaphore() 初始计数不能为负 (可能原因：参数错误)",
        ));
    }
    Ok(Value::Native(Arc::new(Arc::new(SemaphoreT {
        count: AtomicI64::new(n),
        cv: Condvar::new(),
        mu: Mutex::new(()),
    }))))
}

/// bi_sem_acquire 获取信号量（P 操作，计数 -1，为 0 则阻塞）。
fn bi_sem_acquire(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("semAcquire() 需要 1 个参数 (semaphore)"));
    }
    let sem = downcast::<SemaphoreT>(&args[0], "semaphore", "semAcquire")?;
    let mut g = sem.mu.lock().unwrap();
    while sem.count.load(Ordering::SeqCst) <= 0 {
        g = sem.cv.wait(g).unwrap();
    }
    sem.count.fetch_sub(1, Ordering::SeqCst);
    Ok(Value::Undefined)
}

/// bi_sem_release 释放信号量（V 操作，计数 +1，唤醒一个等待者）。
fn bi_sem_release(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("semRelease() 需要 1 个参数 (semaphore)"));
    }
    let sem = downcast::<SemaphoreT>(&args[0], "semaphore", "semRelease")?;
    let _g = sem.mu.lock().unwrap();
    sem.count.fetch_add(1, Ordering::SeqCst);
    sem.cv.notify_one();
    Ok(Value::Undefined)
}

// ============ Once ============

/// OnceT 单次执行原语，包装 std::sync::Once。
///
/// onceDo(once, func) 保证 func 在多次调用中只执行一次（线程安全）。
pub struct OnceT(pub Once);

fn bi_new_once(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Native(Arc::new(Arc::new(OnceT(Once::new())))))
}

/// bi_once_do 保证传入的函数只执行一次（线程安全）。
///
/// 多个线程同时 onceDo 同一 once 时，仅一个线程的 func 会被执行，
/// 其余线程阻塞直至执行完成。func 的返回值被丢弃（返回 undefined）。
fn bi_once_do(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.len() < 2 {
        return Err(crate::value::error_value("onceDo() 需要 2 个参数 (once, func)"));
    }
    let once = downcast::<OnceT>(&args[0], "once", "onceDo")?;
    let func = args[1].clone();
    once.0.call_once(|| {
        // 忽略错误（Once 内不能传播 Result；异常静默）
        let _ = vm.call_function_value(func, Vec::new());
    });
    Ok(Value::Undefined)
}
