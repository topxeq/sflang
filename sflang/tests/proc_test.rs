//! builtins_proc.rs 的回归测试：进程句柄/流式读取/退出码/终止。
//!
//! 仅在非 wasm 平台编译（依赖 std::process 与系统 shell）。
//! 测试用 cmd /C（Windows）或 sh -c（Unix）产生已知输出。

#![cfg(not(target_arch = "wasm32"))]

use sflang::value::Value;
use sflang::Sflang;

/// eval 求值代码块（IIFE 包裹，需显式 return）。
fn eval(src: &str) -> Value {
    let mut sf = Sflang::new();
    let wrapped = format!("func __f() {{ {} }} var __r = __f()", src);
    sf.run_string(&wrapped).expect("eval failed");
    sf.get_global("__r").expect("__r not set")
}

/// run 执行代码，返回运行结果。
fn run(src: &str) -> Result<Value, Value> {
    let mut sf = Sflang::new();
    sf.run_string(src)
}

/// err_msg 取出错误信息字符串。
fn err_msg(r: Result<Value, Value>) -> String {
    match r {
        Err(Value::Error(e)) => e.message.clone(),
        Err(other) => panic!("expected Value::Error, got {:?}", other),
        Ok(v) => panic!("expected error, got ok value: {}", v.inspect()),
    }
}

/// shell_echo 返回执行 `echo <text>` 的平台命令片段（procStart 直 argv 形式）。
fn shell_echo() -> (&'static str, &'static str) {
    if cfg!(windows) {
        ("cmd", "/C echo proc-test-ok")
    } else {
        ("sh", "-c echo proc-test-ok")
    }
}

/// procStart 启动不存在程序返回 error。
#[test]
fn test_proc_start_missing_program_errors() {
    let r = run("var p = procStart(\"__no_such_program_9x7y__\", \"arg\")");
    let msg = err_msg(r);
    assert!(msg.contains("procStart() 启动失败"), "msg={}", msg);
}

/// procRead 收齐子进程全部输出后返回空串（EOF 后缓冲排空）。
#[test]
fn test_proc_read_collects_output() {
    let (prog, arg) = shell_echo();
    let v = eval(&format!(
        "var p = procStart(\"{}\", \"{}\")\nvar got = \"\"\nfor i in range(100) {{\n    got += procRead(p)\n    if !procIsRunning(p) {{ got += procRead(p); break }}\n    sleepMs(30)\n}}\nreturn got",
        prog, arg
    ));
    let got = v.to_str();
    assert!(got.contains("proc-test-ok"), "got={:?}", got);
    // 排空后再读应为空
    let again = eval(&format!(
        "var p = procStart(\"{}\", \"{}\")\nfor i in range(100) {{ procRead(p); if !procIsRunning(p) {{ procRead(p); break }}; sleepMs(30) }}\nreturn procRead(p)",
        prog, arg
    ));
    assert_eq!(again.to_str(), "");
}

/// procExitCode 返回退出码（成功 0 / 失败非 0）。
#[test]
fn test_proc_exit_code() {
    let (prog, arg) = shell_echo();
    let ok_code = eval(&format!(
        "var p = procStart(\"{}\", \"{}\")\nwhile procIsRunning(p) {{ sleepMs(20) }}\nreturn procExitCode(p)",
        prog, arg
    ));
    assert_eq!(ok_code.to_int(), Some(0));

    let fail_code = if cfg!(windows) {
        eval("var p = procStart(\"cmd\", \"/C\", \"exit /b 7\")\nwhile procIsRunning(p) { sleepMs(20) }\nreturn procExitCode(p)")
    } else {
        eval("var p = procStart(\"sh\", \"-c\", \"exit 7\")\nwhile procIsRunning(p) { sleepMs(20) }\nreturn procExitCode(p)")
    };
    assert_eq!(fail_code.to_int(), Some(7));
}

/// procIsRunning 在进程退出后转 false；运行中为 true（长任务）。
#[test]
fn test_proc_is_running_lifecycle() {
    let sleeper = if cfg!(windows) {
        "var p = procStart(\"cmd\", \"/C\", \"ping -n 2 127.0.0.1 >nul\")"
    } else {
        "var p = procStart(\"sh\", \"-c\", \"sleep 1\")"
    };
    let v = eval(&format!(
        "{}\nvar early = procIsRunning(p)\nwhile procIsRunning(p) {{ sleepMs(50) }}\nreturn [early, procIsRunning(p)]",
        sleeper
    ));
    let arr = v.inspect();
    assert!(arr.contains("true") && arr.contains("false"), "arr={}", arr);
}

/// procKill 终止长任务，之后 IsRunning 为 false。
#[test]
fn test_proc_kill_stops_process() {
    let sleeper = if cfg!(windows) {
        "var p = procStart(\"cmd\", \"/C\", \"ping -n 30 127.0.0.1 >nul\")"
    } else {
        "var p = procStart(\"sh\", \"-c\", \"sleep 30\")"
    };
    let v = eval(&format!(
        "{}\nvar before = procIsRunning(p)\nprocKill(p)\nvar after = procIsRunning(p)\nreturn [before, after]",
        sleeper
    ));
    let arr = v.inspect();
    assert!(arr.contains("true") && arr.contains("false"), "arr={}", arr);
}

/// 传非 process 句柄给出 AI 友好错误。
#[test]
fn test_proc_read_rejects_non_process() {
    let msg = err_msg(run("procRead(42)"));
    assert!(msg.contains("process 句柄"), "msg={}", msg);
    let msg2 = err_msg(run("procRead(\"x\")"));
    assert!(msg2.contains("得到 string"), "msg2={}", msg2);
}
