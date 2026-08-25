//! builtins_async.rs — 后台异步执行内置函数
//!
//! 提供 runAsync 在独立线程中执行函数，结果通过全局队列回传。
//! GUI 事件循环每 20ms 检查队列，有结果时调 guiEval 通知前端。
//!
//! 函数列表：
//!   runAsync(fn, args...) -> taskId   启动后台任务
//!   runAsyncResults() -> array        检查已完成的结果

use std::sync::{Arc, Mutex};

use crate::function::BuiltinDoc;
use crate::value::Value;
use crate::vm::VM;

static DOC_RUN_ASYNC: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "runAsync(fn, args...) -> int",
    summary: "在后台线程执行函数，返回任务 ID。结果通过 runAsyncResults() 或 GUI 事件循环回调获取。",
    params: &[
        ("fn", "要执行的函数值"),
        ("args...", "传递给函数的参数"),
    ],
    returns: "int 任务 ID（用于标识结果）",
    examples: &[
        "runAsync(func(host, path) { return sshListDetail(host, path) }, \"--host=1.2.3.4\", \"--remotePath=/\")",
    ],
    errors: &[
        "fn 不是函数值会抛异常",
        "isError 判定规则：fn 抛异常 → isError=true；fn 返回 error 值（如 sshRun 失败时 Ok(error_value)）→ isError=true；其他 → isError=false",
        "工作线程独立 VM，共享 globals；任务结果通过全局队列异步取回",
    ],
};

static DOC_RUN_ASYNC_RESULTS: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "runAsyncResults() -> array<map{id, result, isError}>",
    summary: "检查已完成的后台任务结果。每次调用返回并清除已完成的结果。",
    params: &[],
    returns: "array<map{id:int, result:value, isError:bool}> 空数组表示无已完成任务",
    examples: &["var results = runAsyncResults()"],
    errors: &[
        "isError=true 时 result 是 error 值（用 getErrStr 取消息）；isError=false 时 result 是函数返回值",
        "GUI 事件循环每 20ms 自动调用 drain，前端 window.onAsyncResult(id, json, isError) 接收",
    ],
};

/// AsyncResult 后台任务完成后的结果。
pub struct AsyncResult {
    pub id: u64,
    pub result: Value,
    pub is_error: bool,
}

/// 全局异步结果队列。
static ASYNC_RESULTS: std::sync::OnceLock<Mutex<Vec<AsyncResult>>> = std::sync::OnceLock::new();
/// 全局任务 ID 计数器。
static ASYNC_ID: std::sync::OnceLock<std::sync::atomic::AtomicU64> = std::sync::OnceLock::new();

fn results_queue() -> &'static Mutex<Vec<AsyncResult>> {
    ASYNC_RESULTS.get_or_init(|| Mutex::new(Vec::new()))
}

fn id_counter() -> &'static std::sync::atomic::AtomicU64 {
    ASYNC_ID.get_or_init(|| std::sync::atomic::AtomicU64::new(1))
}

/// register 注册异步执行内置函数。
pub fn register(vm: &mut VM) {
    vm.register_builtin_doc("runAsync", bi_run_async, &DOC_RUN_ASYNC);
    vm.register_builtin_doc("runAsyncResults", bi_run_async_results, &DOC_RUN_ASYNC_RESULTS);
}

/// bi_run_async 在后台线程执行函数。
///
/// 创建独立 VM（共享 globals 和 output），在工作线程中执行函数，
/// 结果存入全局队列，由 GUI 事件循环或 runAsyncResults() 取出。
fn bi_run_async(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "runAsync")?;

    let callee = args[0].clone();
    let call_args: Vec<Value> = args[1..].to_vec();
    let globals = vm.globals_handle();
    let task_id = id_counter().fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    std::thread::spawn(move || {
        // 在工作线程中创建独立 VM
        let mut worker_vm = VM::new();
        worker_vm.set_globals_handle(globals);
        // 执行函数
        let result = worker_vm.call_function_value(callee, call_args);
        // 推断 is_error：
        //   - VM 抛异常 (Err) → is_error = true
        //   - 函数返回 Value::Error（如 sshRun 失败时 Ok(error_value(...))）→ is_error = true
        //   - 其他 Ok 值 → is_error = false
        //
        // 第二种检查很重要：Sflang 的内置函数按"返回错误对象为主"的设计
        // 约定（参考 AGENTS.md），失败时返回 error 值而非抛异常。
        // 若不识别这种情况，前端会误把错误对象当作正常结果处理。
        let (val, is_err) = match result {
            Ok(v) => {
                let is_err = matches!(v, crate::value::Value::Error(_));
                (v, is_err)
            }
            Err(e) => (e, true),
        };
        // 存入全局队列
        let mut queue = results_queue().lock().unwrap();
        queue.push(AsyncResult {
            id: task_id,
            result: val,
            is_error: is_err,
        });
    });

    Ok(Value::Int(task_id as i64))
}

/// bi_run_async_results 检查已完成的后台任务结果。
fn bi_run_async_results(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    let mut queue = results_queue().lock().unwrap();
    if queue.is_empty() {
        return Ok(Value::Array(Arc::new(Mutex::new(Vec::new()))));
    }
    let results: Vec<Value> = queue.drain(..).map(|r| {
        let mut m = crate::ord_map::OrdMap::new();
        m.set("id".to_string(), Value::Int(r.id as i64));
        m.set("result".to_string(), r.result);
        m.set("isError".to_string(), Value::Bool(r.is_error));
        Value::Map(Arc::new(Mutex::new(m)))
    }).collect();
    Ok(Value::Array(Arc::new(Mutex::new(results))))
}

/// drain_async_results 取出所有已完成的结果（供 GUI 事件循环调用）。
pub fn drain_async_results() -> Vec<AsyncResult> {
    let mut queue = results_queue().lock().unwrap();
    queue.drain(..).collect()
}

// ============================================================================
// 流式事件队列（用于 SSH PTY 等持续数据流场景）
// ============================================================================
//
// 与 ASYNC_RESULTS（一次性任务结果）并列。runAsync 模型要求闭包返回后才推送结果，
// 但 PTY/流式场景需要工作线程持续多次推送数据片段，无法用 runAsync 表达。
// 故新增本队列：任意线程可调 push_stream_event 推一段数据，GUI 事件循环
// 每 20ms 在 MainEventsCleared 阶段 drain 并通过 window.onStreamData 推送前端。

/// StreamKind 流事件的种类。
#[derive(Clone, Copy)]
pub enum StreamKind {
    /// 普通数据片段（PTY stdout/stderr 合流）。
    Data,
    /// 流正常结束（远端 EOF）。
    Eof,
    /// 流异常（错误信息在 data 字段）。
    Error,
    /// 远端进程退出（extra 字段是退出码）。
    Exit(u32),
}

/// StreamEvent 一段流事件。
pub struct StreamEvent {
    /// 流 ID，由 sshShellOpen 等创建时分配，前端用它区分不同会话。
    pub stream_id: u64,
    /// 事件数据：Data/Error 时是 Value::Str（UTF-8 lossy）；Eof/Exit 时是 Undefined。
    pub data: Value,
    /// 事件种类。
    pub kind: StreamKind,
}

/// STREAM_EVENTS 全局流事件队列。
static STREAM_EVENTS: std::sync::OnceLock<Mutex<Vec<StreamEvent>>> = std::sync::OnceLock::new();

/// stream_events_queue 取全局流事件队列引用（首次访问时初始化）。
fn stream_events_queue() -> &'static Mutex<Vec<StreamEvent>> {
    STREAM_EVENTS.get_or_init(|| Mutex::new(Vec::new()))
}

/// 全局流 ID 计数器（与 ASYNC_ID 独立，避免混淆）。
static STREAM_ID: std::sync::OnceLock<std::sync::atomic::AtomicU64> = std::sync::OnceLock::new();

/// next_stream_id 分配一个新的流 ID。
pub fn next_stream_id() -> u64 {
    let counter = STREAM_ID.get_or_init(|| std::sync::atomic::AtomicU64::new(1));
    counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

/// push_stream_event 从任意线程推送一段流事件。
///
/// PTY worker 收到远端数据时调用，无需 VM 引用，避免 GUI/工作线程的 VM 重入竞争。
/// data 应为 Value::Str（UTF-8 lossy 转换后），Eof/Exit 时传 Value::Undefined。
pub fn push_stream_event(stream_id: u64, data: Value, kind: StreamKind) {
    let q = stream_events_queue();
    q.lock().unwrap().push(StreamEvent { stream_id, data, kind });
}

/// drain_stream_events 取出所有流事件（GUI 事件循环每帧调用）。
pub fn drain_stream_events() -> Vec<StreamEvent> {
    let mut q = stream_events_queue().lock().unwrap();
    q.drain(..).collect()
}
