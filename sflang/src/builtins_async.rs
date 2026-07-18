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
    errors: &[],
};

static DOC_RUN_ASYNC_RESULTS: BuiltinDoc = BuiltinDoc {
    category: "concurrency",
    signature: "runAsyncResults() -> array<map{id, result, isError}>",
    summary: "检查已完成的后台任务结果。每次调用返回并清除已完成的结果。",
    params: &[],
    returns: "array<map{id:int, result:value, isError:bool}> 空数组表示无已完成任务",
    examples: &["var results = runAsyncResults()"],
    errors: &[],
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
        let (val, is_err) = match result {
            Ok(v) => (v, false),
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
