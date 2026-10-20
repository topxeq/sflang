//! builtins_proc.rs — 子进程控制内置函数（进程句柄/流式读取/终止）
//!
//! 与 systemCmd（同步阻塞、只回 stdout）互补：procStart 返回进程句柄，
//! 输出经后台读线程流入缓冲，procRead 非阻塞取用——供 GUI 编辑器等宿主
//! 实现"运行不冻结界面 + 实时输出 + 停止按钮"。
//!
//! 函数列表：
//!   procStart(cmd[, arg...])  — 启动子进程（stdout/stderr 分管道收集），返回 process 句柄
//!   procRead(p)               — 非阻塞读取当前已产生的合并输出（UTF-8 边界安全）
//!   procIsRunning(p)          — 进程是否仍在运行
//!   procExitCode(p)           — 退出码（未退出返回 undefined）
//!   procWait(p)               — 阻塞等待退出，返回退出码
//!   procKill(p)               — 终止进程
//!
//! 设计要点：
//!   - 读线程：procStart 时为 stdout/stderr 各起一个读线程，持续把字节推入
//!     共享缓冲（无锁争用窗口：每流一把锁）；procRead 同时排空两缓冲。
//!   - UTF-8 边界：多字节字符可能被管道分段，procRead 只输出"当前缓冲中
//!     完整的 UTF-8 前缀"，残缺尾部留在缓冲等待后续字节（from_utf8 的
//!     error_len 语义），避免中文输出被截成乱码。
//!   - 句柄值语义：与 channel/mutex 等原生对象一致（Value::Native 包装）。

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::builtins_helpers as bh;
use crate::function::BuiltinDoc;
use crate::value::Value;
use crate::vm::VM;

/// ProcHandle 子进程句柄：子进程 + 双流缓冲 + 状态。
pub struct ProcHandle {
    /// child 子进程（kill/wait 需要 &mut）
    child: Mutex<Child>,
    /// out_buf stdout 缓冲（读线程写入、procRead 排空）
    out_buf: Mutex<Vec<u8>>,
    /// err_buf stderr 缓冲
    err_buf: Mutex<Vec<u8>>,
    /// running 进程是否仍在运行（waitpid 后置 false）
    running: AtomicBool,
    /// exit_code 退出码（运行中为 None）
    exit_code: Mutex<Option<i32>>,
}

/// doc 宏：签名/分类统一的样板收敛。
macro_rules! doc {
    ($name:ident, $sig:expr, $summary:expr, $params:expr, $returns:expr, $examples:expr, $errors:expr) => {
        static $name: BuiltinDoc = BuiltinDoc {
            category: "process",
            signature: $sig,
            summary: $summary,
            params: $params,
            returns: $returns,
            examples: $examples,
            errors: $errors,
        };
    };
}

doc!(DOC_PROC_START,
    "procStart(cmd[, arg...]) -> process",
    "启动子进程（不等待），stdout/stderr 分别由后台线程收集，返回进程句柄。",
    &[
        ("cmd", "可执行程序路径（不经 shell 解析，参数分开传）"),
        ("arg...", "可选追加参数（string，逐个传给程序）"),
    ],
    "process 进程句柄（配合 procRead/procIsRunning/procKill 等使用）",
    &[
        "procStart(\"sf\")                 // 直接启动（错误返回 error 对象，如程序不存在）",
        "procStart(sfExe, scriptPath)     // 带参数启动（推荐，避免 shell 转义问题）",
    ],
    &[
        "程序不存在/无执行权限返回 error（含系统错误信息）",
        "不经 shell：通配符、管道等 shell 语法不会被解析；需要 shell 时用 procStart(\"cmd\", \"/C\", \"...\")",
    ]);

doc!(DOC_PROC_READ,
    "procRead(p) -> string",
    "非阻塞读取子进程当前已产生的输出（stdout+stderr 合并，按产生顺序近似），不等待。",
    &[("p", "procStart 返回的进程句柄")],
    "string 本次新产生的输出（无新输出为空串；UTF-8 多字节截断部分留待下次）",
    &[
        "var chunk = procRead(p); if chunk != \"\" { pl(chunk) }   // 轮询模式",
        "while procIsRunning(p) { var c = procRead(p); sleepMs(50) }  // 典型循环",
    ],
    &[
        "非阻塞：进程无新输出立即返回空串，不会挂起",
        "多字节字符被管道截断时，残缺尾部留缓冲，下次凑齐再输出（不丢字节、不产生乱码）",
    ]);

doc!(DOC_PROC_IS_RUNNING,
    "procIsRunning(p) -> bool",
    "子进程是否仍在运行。",
    &[("p", "进程句柄")],
    "bool true=运行中",
    &["while procIsRunning(p) { procRead(p); sleepMs(50) }"],
    &[]);

doc!(DOC_PROC_EXIT_CODE,
    "procExitCode(p) -> int|undefined",
    "子进程退出码；仍在运行时返回 undefined。",
    &[("p", "进程句柄")],
    "int 退出码（正常退出 0；被 kill 的进程码依平台而定）；运行中 undefined",
    &["if !procIsRunning(p) { pln(\"exit: \" + toStr(procExitCode(p))) }"],
    &[]);

doc!(DOC_PROC_WAIT,
    "procWait(p) -> int",
    "阻塞等待子进程退出，返回退出码。",
    &[("p", "进程句柄")],
    "int 退出码",
    &["var code = procWait(p)"],
    &[
        "阻塞当前执行体：GUI/并发场景请在 run 任务中调用",
        "进程已被 kill 时返回平台的终止码（Windows 负数/Unix 信号语义）",
    ]);

doc!(DOC_PROC_KILL,
    "procKill(p) -> undefined",
    "强制终止子进程。",
    &[("p", "进程句柄")],
    "undefined",
    &["procKill(p)"],
    &[
        "强杀不等待清理：子进程的子进程（孙进程）可能残留，属平台行为",
        "已退出的进程再 kill 为无操作（不报错）",
    ]);

/// proc_downcast 取进程句柄（类型不符时给出 AI 友好错误）。
fn proc_downcast<'a>(v: &'a Value, fn_name: &str) -> Result<&'a Arc<ProcHandle>, Value> {
    match v {
        Value::Native(n) => n
            .downcast_ref::<Arc<ProcHandle>>()
            .ok_or_else(|| {
                crate::value::error_value(format!(
                    "{}() 第 1 个参数应为 process 句柄，得到 {} (可能原因：传入了其他原生对象或已失效的值)",
                    fn_name,
                    v.type_name_ex()
                ))
            }),
        other => Err(crate::value::error_value(format!(
            "{}() 第 1 个参数应为 process 句柄，得到 {} (可能原因：参数顺序错误或未用 procStart 创建)",
            fn_name,
            other.type_name_ex()
        ))),
    }
}

/// bi_proc_start 启动子进程并装好读线程。
fn bi_proc_start(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value(
            "procStart() 需要至少 1 个参数 (cmd[, arg...])",
        ));
    }
    let mut cmd = Command::new(bh::as_str(args, 0, "procStart")?);
    for (i, a) in args.iter().enumerate().skip(1) {
        cmd.arg(bh::as_str(args, i, "procStart")?);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Err(crate::value::error_value(format!(
                "procStart() 启动失败: {} (可能原因：程序不存在、无执行权限、路径错误)",
                e
            )));
        }
    };
    let out_pipe = child.stdout.take().ok_or_else(|| {
        crate::value::error_value("procStart() 内部错误：stdout 管道未建立")
    })?;
    let err_pipe = child.stderr.take().ok_or_else(|| {
        crate::value::error_value("procStart() 内部错误：stderr 管道未建立")
    })?;
    let h = Arc::new(ProcHandle {
        child: Mutex::new(child),
        out_buf: Mutex::new(Vec::new()),
        err_buf: Mutex::new(Vec::new()),
        running: AtomicBool::new(true),
        exit_code: Mutex::new(None),
    });
    // stdout/stderr 各一个读线程：持续把字节推入句柄缓冲，EOF 后自然结束
    {
        let h2 = h.clone();
        std::thread::Builder::new()
            .name("sf-proc-out".to_string())
            .spawn(move || reader_loop(h2, true, Box::new(out_pipe)))
            .ok();
        let h3 = h.clone();
        std::thread::Builder::new()
            .name("sf-proc-err".to_string())
            .spawn(move || reader_loop(h3, false, Box::new(err_pipe)))
            .ok();
    }
    // Native 约定：外层 Arc<dyn Any> 内存的是句柄的 Arc（与 channel/mutex 等一致）
    Ok(Value::Native(Arc::new(h)))
}

/// reader_loop 读线程主体：持续把管道字节推入句柄缓冲。
fn reader_loop(h: Arc<ProcHandle>, is_stdout: bool, mut pipe: Box<dyn Read + Send>) {
    let mut tmp = [0u8; 8192];
    loop {
        match pipe.read(&mut tmp) {
            Ok(0) => break, // EOF：管道关闭
            Ok(n) => {
                let mut b = if is_stdout { h.out_buf.lock().unwrap() } else { h.err_buf.lock().unwrap() };
                b.extend_from_slice(&tmp[..n]);
            }
            Err(_) => break,
        }
    }
}

/// drain_utf8 从缓冲取出"完整 UTF-8 前缀"，残缺尾部留在缓冲。
fn drain_utf8(buf: &mut Vec<u8>, out: &mut String) {
    if buf.is_empty() {
        return;
    }
    match std::str::from_utf8(buf) {
        Ok(s) => {
            out.push_str(s);
            buf.clear();
        }
        Err(e) => {
            // e.valid_up_to() 之前是完整 UTF-8；error_len 为 None 表示尾部缺字节
            let valid = e.valid_up_to();
            if valid > 0 {
                // SAFETY: valid_up_to 保证前缀是合法 UTF-8
                out.push_str(unsafe { std::str::from_utf8_unchecked(&buf[..valid]) });
                buf.drain(..valid);
            }
            // error_len 为 Some(0) 的情况即编码非法字节：跳过 1 字节避免死锁
            if e.error_len() == Some(0) {
                buf.remove(0);
            }
        }
    }
}

/// bi_proc_read 非阻塞读取合并输出。
fn bi_proc_read(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "procRead")?;
    let h = proc_downcast(&args[0], "procRead")?;
    let mut out = String::new();
    {
        let mut ob = h.out_buf.lock().unwrap();
        drain_utf8(&mut ob, &mut out);
    }
    {
        let mut eb = h.err_buf.lock().unwrap();
        drain_utf8(&mut eb, &mut out);
    }
    Ok(Value::str(&out))
}

/// bi_proc_is_running 进程是否仍在运行。
fn bi_proc_is_running(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "procIsRunning")?;
    let h = proc_downcast(&args[0], "procIsRunning")?;
    if h.running.load(Ordering::SeqCst) {
        // try_wait 探测退出（不阻塞）；退出则回收状态
        let mut child = h.child.lock().unwrap();
        if let Ok(Some(status)) = child.try_wait() {
            h.running.store(false, Ordering::SeqCst);
            *h.exit_code.lock().unwrap() = Some(status.code().unwrap_or(-1));
            return Ok(Value::Bool(false));
        }
        return Ok(Value::Bool(true));
    }
    Ok(Value::Bool(false))
}

/// bi_proc_exit_code 退出码（运行中 undefined）。
fn bi_proc_exit_code(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "procExitCode")?;
    let h = proc_downcast(&args[0], "procExitCode")?;
    // 先探测一次，运行结束后能立刻取到退出码
    if h.running.load(Ordering::SeqCst) {
        let mut child = h.child.lock().unwrap();
        if let Ok(Some(status)) = child.try_wait() {
            h.running.store(false, Ordering::SeqCst);
            *h.exit_code.lock().unwrap() = Some(status.code().unwrap_or(-1));
        }
    }
    match *h.exit_code.lock().unwrap() {
        Some(c) => Ok(Value::Int(c as i64)),
        None => Ok(Value::Undefined),
    }
}

/// bi_proc_wait 阻塞等待退出。
fn bi_proc_wait(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "procWait")?;
    let h = proc_downcast(&args[0], "procWait")?;
    if h.running.load(Ordering::SeqCst) {
        let mut child = h.child.lock().unwrap();
        match child.wait() {
            Ok(status) => {
                h.running.store(false, Ordering::SeqCst);
                *h.exit_code.lock().unwrap() = Some(status.code().unwrap_or(-1));
            }
            Err(e) => {
                return Err(crate::value::error_value(format!(
                    "procWait() 等待失败: {} (可能原因：进程已被外部回收)",
                    e
                )));
            }
        }
    }
    match *h.exit_code.lock().unwrap() {
        Some(c) => Ok(Value::Int(c as i64)),
        None => Ok(Value::Int(-1)),
    }
}

/// bi_proc_kill 强制终止。
fn bi_proc_kill(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "procKill")?;
    let h = proc_downcast(&args[0], "procKill")?;
    if h.running.load(Ordering::SeqCst) {
        let mut child = h.child.lock().unwrap();
        if let Ok(Some(status)) = child.try_wait() {
            h.running.store(false, Ordering::SeqCst);
            *h.exit_code.lock().unwrap() = Some(status.code().unwrap_or(-1));
        } else {
            let _ = child.kill();
            let _ = child.wait();
            h.running.store(false, Ordering::SeqCst);
            // Windows kill 后 code() 通常为 None（终止状态），记 -1
            *h.exit_code.lock().unwrap() = Some(-1);
        }
    }
    Ok(Value::Undefined)
}

/// register 注册全部进程内置函数。
pub fn register(vm: &mut VM) {
    vm.register_builtin_doc("procStart", bi_proc_start, &DOC_PROC_START);
    vm.register_builtin_doc("procRead", bi_proc_read, &DOC_PROC_READ);
    vm.register_builtin_doc("procIsRunning", bi_proc_is_running, &DOC_PROC_IS_RUNNING);
    vm.register_builtin_doc("procExitCode", bi_proc_exit_code, &DOC_PROC_EXIT_CODE);
    vm.register_builtin_doc("procWait", bi_proc_wait, &DOC_PROC_WAIT);
    vm.register_builtin_doc("procKill", bi_proc_kill, &DOC_PROC_KILL);
}
