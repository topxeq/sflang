//! scheduler.rs — M:N 协作式任务调度器（goroutine 式轻量并发）
//!
//! 设计要点（阶段二：调度器核心）：
//!   - Task：一个可调度的执行单元 = 私有 VM（帧栈 + 操作数栈）+ 共享 globals/out。
//!     创建成本约几百纳秒（内置函数表已全局化，VM 轻量），内存初始 ~KB 级，
//!     对比 OS 线程（~17µs 创建、~50KB 提交 + 8MB 栈预留）。
//!   - 调度：N 个 worker 线程（N = CPU 核数）从共享就绪队列取任务执行切片。
//!     切片 = 最多 TASK_SLICE_FUEL 条指令（vm.rs TASK_SLICE_FUEL），耗尽让出
//!     （协作式预算抢占，防止 CPU 密集任务饿占 worker）。
//!   - 挂起/唤醒：任务内阻塞类操作（chanRecv/lock/sleep 等，阶段四接入）通过
//!     park 把任务登记到等待方（等待队列/定时器堆），worker 立即切换其他任务；
//!     条件满足时 wake 把任务放回就绪队列。阻塞发生在家在用户态任务切换，
//!     不占 OS 线程——这是与旧 OS 线程模型的本质区别。
//!   - 结果注入：挂起点在操作数栈顶留 undefined 占位值；携带结果的唤醒
//!     （wake_with，如 chanRecv 收到数据）把结果写入 pending_injection，
//!     任务下个切片开始时由 runner 替换占位值。注入在切片开始时统一应用，
//!     避免与切片执行中的栈操作竞争。
//!   - 状态机（Task.state）：
//!     Ready --checkout(CAS)--> Running --切片结束--> Ready（让出）/ Finished（完成）
//!        ^                                      |
//!        |        park（登记等待）               v
//!        +---- wake（CAS Blocked→Ready）------ Blocked
//!     wake 的 CAS 防止双重入队；checkout 的 CAS 防止双重执行。
//!
//! 与旧模型的兼容：非任务上下文（主脚本、poolRun 工作线程、threadRun）中
//! 阻塞类内置函数继续走阻塞路径（阶段四在原语内分派）；任务上下文走 park。

use std::collections::{BinaryHeap, VecDeque};
use std::cmp::Reverse;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Instant;

use crate::value::Value;
use crate::vm::VM;

/// 任务状态编码（Task.state，AtomicU8）。
mod task_state {
    pub const READY: u8 = 0; // 在就绪队列中
    pub const RUNNING: u8 = 1; // 正被某个 worker 执行
    pub const BLOCKED: u8 = 2; // 挂起（等待队列/定时器中）
    pub const FINISHED: u8 = 3; // 已完成
}

/// SliceOutcome 单个任务切片的执行结果（vm.rs run_task_slice 返回）。
pub enum SliceOutcome {
    /// Completed 任务执行完毕（正常返回或抛出异常，作为任务最终结果）。
    Completed(Result<Value, Value>),
    /// Yielded 燃料耗尽让出，任务可再次调度。
    Yielded,
    /// Parked 任务挂起（等待条件已登记），唤醒前不要调度。
    Parked,
}

/// Task 一个可调度的执行单元。
///
/// vm 持有任务的全部执行状态（帧栈、操作数栈、暂停原因）。worker 执行切片
/// 时短暂锁定；挂起/让出后锁释放，任务状态归调度器管理。
pub struct Task {
    /// id 全局唯一任务 ID（诊断信息用，随进程递增）。
    pub id: u64,
    /// vm 任务私有的虚拟机（globals/out 为共享句柄，见 spawn_task）。
    pub vm: Mutex<VM>,
    /// state 任务状态（task_state 常量；CAS 驱动的状态机）。
    state: AtomicU8,
    /// pending_injection 待注入的结果值（wake_with 写入，切片开始时应用）。
    ///
    /// 注入时机选在切片开始（runner 持有 vm 锁、栈稳定：栈顶必为挂起点的
    /// undefined 占位值），避免与切片执行中的栈操作竞争。
    pending_injection: Mutex<Option<Value>>,
}

impl Task {
    /// current_state 读取任务状态（诊断用）。
    pub fn state_name(&self) -> &'static str {
        match self.state.load(Ordering::SeqCst) {
            task_state::READY => "ready",
            task_state::RUNNING => "running",
            task_state::BLOCKED => "blocked",
            _ => "finished",
        }
    }
}

// Ord 按 id 定序：定时器堆需要 (Instant, Arc<Task>) 可比较，
// 同刻度时按任务 ID 定序（确定性，无实际语义）。
impl PartialEq for Task {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for Task {}
impl PartialOrd for Task {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Task {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id.cmp(&other.id)
    }
}

/// SchedInner 调度器内部状态（全局锁保护；高竞争场景可再分片，见文末注）。
struct SchedInner {
    /// ready 就绪队列（FIFO；worker 从头取，唤醒/让出从尾入）。
    ready: VecDeque<Arc<Task>>,
    /// timers 定时器堆：（到期时刻, 任务）——sleep 类挂起的唤醒源。
    timers: BinaryHeap<Reverse<(Instant, Arc<Task>)>>,
    /// next_task_id 任务 ID 分配器。
    next_task_id: u64,
}

/// SCHED 全局调度器单例。
///
/// 进程级共享：多个嵌入式 Sflang 实例的任务共用同一组 worker
/// （任务各自持有 globals 句柄，实例间数据隔离不受影响）。
static SCHED: OnceLock<Scheduler> = OnceLock::new();

/// Scheduler 调度器：就绪队列 + 定时器 + worker 线程池。
struct Scheduler {
    inner: Mutex<SchedInner>,
    /// cv 就绪队列/定时器变化通知（worker 空闲时等待）。
    cv: Condvar,
    /// worker_count worker 线程数（启动时按 CPU 核数确定）。
    worker_count: usize,
}

/// next_task_id 全局任务 ID 分配。
fn next_task_id() -> u64 {
    static ID: AtomicU64 = AtomicU64::new(1);
    ID.fetch_add(1, Ordering::SeqCst)
}

/// sched 获取全局调度器（首次访问时创建并启动 worker 线程）。
fn sched() -> &'static Scheduler {
    SCHED.get_or_init(|| {
        // worker 数默认 = CPU 核数；可用环境变量 SF_TASK_WORKERS 覆盖
        // （IO 密集负载可调大；注意任务内阻塞 IO 本身仍占一个 worker）
        let workers = std::env::var("SF_TASK_WORKERS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|&n| n >= 1 && n <= 1024)
            .or_else(|| {
                std::thread::available_parallelism()
                    .map(|n| n.get())
                    .ok()
            })
            .unwrap_or(4);
        let s = Scheduler {
            inner: Mutex::new(SchedInner {
                ready: VecDeque::new(),
                timers: BinaryHeap::new(),
                next_task_id: 1,
            }),
            cv: Condvar::new(),
            worker_count: workers,
        };
        // 启动 worker 线程（常驻；进程退出随之终止）
        for i in 0..workers {
            std::thread::Builder::new()
                .name(format!("sf-worker-{}", i))
                .spawn(worker_loop)
                .expect("scheduler: worker 线程启动失败");
        }
        s
    })
}

/// worker_loop worker 线程主循环：取任务→执行切片→按结果处置。
fn worker_loop() {
    loop {
        // ---- 取任务（无任务时按最近定时器定时等待） ----
        let task = {
            let s = sched();
            let mut g = s.inner.lock().unwrap();
            loop {
                // 到期定时器：唤醒对应任务（CAS Blocked→Ready 入就绪队列）
                let now = Instant::now();
                let mut fired = false;
                while let Some(Reverse((deadline, _t))) = g.timers.peek() {
                    if *deadline <= now {
                        let Reverse((_, t)) = g.timers.pop().unwrap();
                        if t.state.compare_exchange(
                            task_state::BLOCKED,
                            task_state::READY,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        )
                        .is_ok()
                        {
                            g.ready.push_back(t);
                        }
                        fired = true;
                    } else {
                        break;
                    }
                }
                if fired {
                    continue; // 定时器唤醒的任务已入队，回头取
                }
                if let Some(t) = g.ready.pop_front() {
                    break t;
                }
                // 无就绪任务：按最近定时器到期时间等待（无定时器则无限等待）
                let timeout = g
                    .timers
                    .peek()
                    .map(|Reverse((d, _))| d.saturating_duration_since(now));
                g = match timeout {
                    Some(d) => s.cv.wait_timeout(g, d).unwrap().0,
                    None => s.cv.wait(g).unwrap(),
                };
            }
        };

        // ---- checkout：CAS Ready→Running（防止双重执行） ----
        if task
            .state
            .compare_exchange(
                task_state::READY,
                task_state::RUNNING,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_err()
        {
            // 已被其他 worker 执行或已完成：跳过（重复入队的防御）
            continue;
        }

        run_task_slice_once(&task);
    }
}

/// run_task_slice_once 执行任务的一个切片并按结果处置。
///
/// 供 worker 循环调用；也暴露给测试（在受控上下文中驱动任务）。
fn run_task_slice_once(task: &Arc<Task>) {
    // thread-local 当前任务：阻塞类内置函数据此选择 park（任务）或阻塞（线程）
    CURRENT_TASK.with(|c| *c.borrow_mut() = Some(task.clone()));

    // 应用挂起期间到达的结果注入（替换挂起点的 undefined 占位值）
    {
        let inj = task.pending_injection.lock().unwrap().take();
        if let Some(v) = inj {
            let mut vm = task.vm.lock().unwrap();
            if let Some(slot) = vm_stack_last_mut(&mut vm) {
                *slot = v;
            }
        }
    }

    let outcome = {
        let mut vm = task.vm.lock().unwrap();
        vm.run_task_slice()
    };

    CURRENT_TASK.with(|c| *c.borrow_mut() = None);

    match outcome {
        SliceOutcome::Completed(Err(e)) => {
            // 任务结束：状态置 Finished；VM 随任务引用释放（内存回收）。
            // 任务内异常：打印提示，不传播（与旧 run 线程行为一致，避免 panic）
            task.state.store(task_state::FINISHED, Ordering::SeqCst);
            let msg = match &e {
                Value::Error(x) => x.message.clone(),
                other => other.to_str(),
            };
            let out = task.vm.lock().unwrap().output_handle();
            let _ = std::io::Write::write_fmt(
                &mut *out.lock().unwrap(),
                format_args!("[run 任务异常] {}\n", msg),
            );
        }
        SliceOutcome::Completed(_r) => {
            // 任务正常结束：状态置 Finished；VM 随任务引用释放（内存回收）
            task.state.store(task_state::FINISHED, Ordering::SeqCst);
        }
        SliceOutcome::Yielded => {
            // 让出：回就绪队列尾（CAS 失败说明已被唤醒方改为 Ready，跳过）
            if task
                .state
                .compare_exchange(
                    task_state::RUNNING,
                    task_state::READY,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                )
                .is_ok()
            {
                let s = sched();
                s.inner.lock().unwrap().ready.push_back(task.clone());
                s.cv.notify_one();
            }
        }
        SliceOutcome::Parked => {
            // 挂起：park 已置 Blocked。若 wake 在切片收尾前已抢先唤醒
            // （state 已是 Ready），补一次入队；否则留在等待方处。
            if task.state.load(Ordering::SeqCst) == task_state::READY {
                let s = sched();
                s.inner.lock().unwrap().ready.push_back(task.clone());
                s.cv.notify_one();
            }
        }
    }
}

/// vm_stack_last_mut 取任务 VM 操作数栈顶的可变引用（注入用）。
///
/// 挂起点的栈顶必为 undefined 占位值；空栈（任务体为内置函数等边缘）时
/// 无处注入，静默跳过（结果丢弃——与占位值语义一致）。
fn vm_stack_last_mut(vm: &mut VM) -> Option<&mut Value> {
    vm.stack_last_mut()
}

// ---- 当前任务上下文（thread-local） ----

thread_local! {
    /// CURRENT_TASK 当前线程正在执行的任务（worker 执行切片期间非 None）。
    /// 阻塞类内置函数据此区分"任务上下文（park）"与"线程上下文（阻塞）"。
    static CURRENT_TASK: std::cell::RefCell<Option<Arc<Task>>> =
        const { std::cell::RefCell::new(None) };
}

/// current_task 取当前线程正在执行的任务（非任务上下文返回 None）。
pub fn current_task() -> Option<Arc<Task>> {
    CURRENT_TASK.with(|c| c.borrow().clone())
}

// ---- 公开 API（内置函数与测试使用） ----

/// spawn_task 创建并调度一个新任务。
///
/// - callee/args：任务体函数与实参（函数值；内置函数亦可，立即在切片中执行完）；
/// - globals/out：与派生方共享的全局环境与输出句柄（与 run 语义一致）。
///
/// 返回任务 ID。任务为 fire-and-forget：异常打印到输出（与旧 run 线程一致），
/// 结果通过 channel/等待方获取。
pub fn spawn_task(
    callee: Value,
    args: Vec<Value>,
    globals: Arc<Mutex<std::collections::HashMap<String, Value>>>,
    out: Arc<Mutex<dyn std::io::Write + Send>>,
) -> Result<u64, Value> {
    // 任务私有 VM：内置函数表已全局化，创建成本 ~几百 ns
    let mut vm = VM::new();
    vm.set_globals_handle(globals);
    vm.set_output_handle(out);
    vm.prepare_task_call(callee, args)?;

    let s = sched();
    let id = next_task_id();
    let task = Arc::new(Task {
        id,
        vm: Mutex::new(vm),
        state: AtomicU8::new(task_state::READY),
        pending_injection: Mutex::new(None),
    });
    s.inner.lock().unwrap().ready.push_back(task.clone());
    s.cv.notify_one();
    Ok(id)
}

/// wake_task 唤醒挂起的任务（不注入结果；占位 undefined 即结果语义）。
///
/// 适用于唤醒即完成的等待（lock 获得锁、wgWait 计数归零、信号量空位等）。
/// CAS Blocked→Ready 防止与切片收尾路径竞争导致的双重入队。
pub fn wake_task(task: &Arc<Task>) {
    if task
        .state
        .compare_exchange(
            task_state::BLOCKED,
            task_state::READY,
            Ordering::SeqCst,
            Ordering::SeqCst,
        )
        .is_ok()
    {
        let s = sched();
        s.inner.lock().unwrap().ready.push_back(task.clone());
        s.cv.notify_one();
    }
}

/// wake_task_with 唤醒挂起的任务并注入结果值（如 chanRecv 收到的数据）。
///
/// 结果写入 pending_injection，任务下个切片开始时替换挂起点的占位值。
/// 同一挂起只应被唤醒一次（等待方弹出 waiter 即删）。
pub fn wake_task_with(task: &Arc<Task>, value: Value) {
    *task.pending_injection.lock().unwrap() = Some(value);
    wake_task(task);
}

/// park_current_task_retry 挂起当前任务（等待-重试型：lock/wgWait/semAcquire）。
///
/// 唤醒后任务回退到挂起点调用指令重新执行（重查条件）。
/// 调用契约（内置函数内）：
///   1. 本函数先置 state=BLOCKED，调用方随后把任务登记进等待队列——顺序保证
///      不丢唤醒（唤醒方只从等待队列取任务；先登记后置状态会丢注册前到达的
///      唤醒）；
///   2. 调用方随后返回占位值（本模型下占位值不压栈，机器循环回退重试）；
///   3. defer 上下文中允许（收尾状态机条目回队重试语义成立）。
pub fn park_current_task_retry(vm: &mut VM) -> Result<(), Value> {
    let task = current_task().ok_or_else(|| {
        crate::value::error_value("park_current_task_retry 仅可在任务上下文中调用")
    })?;
    task.state.store(task_state::BLOCKED, Ordering::SeqCst);
    vm.set_pause(crate::vm::Pause::Parked);
    Ok(())
}

/// park_current_task_inject 挂起当前任务（值注入型：chanRecv/onceDo 等待者/sleep）。
///
/// 唤醒后任务从挂起点调用指令之后继续，占位 undefined 被注入值替换
/// （plain wake 不注入时占位值即结果语义，如 sleep）。
/// defer 上下文中拒绝：注入型没有"条目回队重试"语义（重试会重复执行
/// sleep/chanRecv），为避免状态错乱直接返回明确错误。
pub fn park_current_task_inject(vm: &mut VM) -> Result<(), Value> {
    if vm.is_in_defer_context() {
        return Err(crate::value::error_value(
            "值等待类操作（chanRecv/onceDo/sleep 等）暂不支持在 defer 中挂起当前任务 (可能原因：defer 触发的函数内部调用了会等待的操作；请把该操作移出 defer，或改用 chanTryRecv 等非阻塞形式)",
        ));
    }
    if vm.is_park_disabled() {
        return Err(crate::value::error_value(
            "回调函数内不能调用挂起类操作（chanRecv/lock/wgWait/semAcquire/onceDo/sleep 等） (可能原因：onceDo/sort 等的回调函数内部等待；请把等待移到回调外执行，或改用 chanTryRecv/tryLock 等非阻塞形式)",
        ));
    }
    let task = current_task().ok_or_else(|| {
        crate::value::error_value("park_current_task_inject 仅可在任务上下文中调用")
    })?;
    task.state.store(task_state::BLOCKED, Ordering::SeqCst);
    vm.set_pause(crate::vm::Pause::ParkedInject);
    Ok(())
}

/// schedule_timer 为当前任务登记定时唤醒（sleep 类挂起用，阶段四接入）。
///
/// 前置：已 park_current_task（状态 BLOCKED）。到期时调度器把任务放回就绪队列。
pub fn schedule_timer(task: &Arc<Task>, deadline: Instant) {
    let s = sched();
    let mut g = s.inner.lock().unwrap();
    g.timers.push(Reverse((deadline, task.clone())));
    s.cv.notify_one();
}

/// queued_task_count 当前排队中的任务数（诊断用）。
pub fn queued_task_count() -> usize {
    sched().inner.lock().unwrap().ready.len()
}

// ---- 测试 ----

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use std::sync::atomic::AtomicUsize;

    /// compile_fn 编译源码为函数值（取脚本中名为 name 的函数）。
    fn compile_fn(src: &str, name: &str) -> Value {
        use crate::{compile, parse_program, tokenize};
        let tokens = tokenize(src, "test.sf").expect("tokenize");
        let prog = parse_program(tokens, "test.sf").expect("parse");
        let code = Arc::new(compile(&prog).expect("compile"));
        let mut vm = VM::new();
        vm.run(code).expect("run defs");
        vm.get_global(name).expect("function in globals")
    }

    /// wait_until 轮询等待条件成立（任务异步完成）。
    fn wait_until(f: impl Fn() -> bool, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        f()
    }

    /// new_out 生成静默输出句柄。
    fn new_out() -> Arc<Mutex<dyn std::io::Write + Send>> {
        Arc::new(Mutex::new(std::io::sink()))
    }

    /// test_spawn_task_writes_global 任务执行并写入共享全局环境。
    #[test]
    fn test_spawn_task_writes_global() {
        // 任务体给全局赋值（任务 VM 与本测试共享同一 globals 句柄）
        let f = compile_fn("func work() { resultG = 42; return 42 }", "work");
        let globals: Arc<Mutex<std::collections::HashMap<String, Value>>> =
            Arc::new(Mutex::new(std::collections::HashMap::new()));
        spawn_task(f, vec![], globals.clone(), new_out()).expect("spawn");
        assert!(
            wait_until(
                || globals.lock().unwrap().contains_key("resultG"),
                Duration::from_secs(5)
            ),
            "任务应完成并写入全局变量"
        );
        assert_eq!(
            globals.lock().unwrap().get("resultG").cloned(),
            Some(Value::Int(42))
        );
    }

    /// test_many_tasks_all_complete 大量长任务在切片抢占下全部完成（不饿死）。
    #[test]
    fn test_many_tasks_all_complete() {
        // 每个任务 ~5 万次循环（远超单切片 10 万条指令预算的边界情形，
        // 200 个并发任务强制多次切片切换）；完成后向共享列表 push 自己的 n，
        // 以列表长度达到 200 判定全部完成
        let f = compile_fn(
            "func work(n) { var s = 0; for i in range(50000) { s = s + i }; push(doneList, n); return s }",
            "work",
        );
        // 预置共享结果列表（任务体内 push 到它）
        let globals: Arc<Mutex<std::collections::HashMap<String, Value>>> =
            Arc::new(Mutex::new(std::collections::HashMap::new()));
        globals.lock().unwrap().insert(
            "doneList".to_string(),
            Value::Array(Arc::new(Mutex::new(Vec::new()))),
        );
        let out = new_out();
        for n in 0..200 {
            spawn_task(f.clone(), vec![Value::Int(n)], globals.clone(), out.clone())
                .expect("spawn");
        }
        assert!(
            wait_until(
                || {
                    let g = globals.lock().unwrap();
                    match g.get("doneList") {
                        Some(Value::Array(a)) => a.lock().unwrap().len() >= 200,
                        _ => false,
                    }
                },
                Duration::from_secs(60)
            ),
            "200 个任务都应完成（各 push 一次）"
        );
    }

    /// test_wake_after_finish 无害唤醒：对已完成任务 wake 不 panic、无副作用。
    #[test]
    fn test_wake_after_finish_no_panic() {
        let f = compile_fn("func noop() { return 1 }", "noop");
        let globals: Arc<Mutex<std::collections::HashMap<String, Value>>> =
            Arc::new(Mutex::new(std::collections::HashMap::new()));
        let out = new_out();
        spawn_task(f, vec![], globals, out).expect("spawn");
        assert!(
            wait_until(|| queued_task_count() == 0, Duration::from_secs(5)),
            "noop 任务应完成"
        );
    }

    /// test_spawn_not_callable 非函数任务体返回错误对象。
    #[test]
    fn test_spawn_not_callable() {
        let globals: Arc<Mutex<std::collections::HashMap<String, Value>>> =
            Arc::new(Mutex::new(std::collections::HashMap::new()));
        let r = spawn_task(Value::Int(1), vec![], globals, new_out());
        assert!(r.is_err(), "非函数任务体应报错");
    }
}
