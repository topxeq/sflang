//! core_fix_test — try/catch/finally/defer 状态机与核心控制流的回归测试
//!
//! 对应审查报告中的高危问题：finally 必然执行、return 不被吞、catch 无变量、
//! break/continue 穿越 finally、throw 路径执行 defer、深循环 try-catch 不栈溢出、
//! 两层闭包捕获、非法赋值目标、C 风格 for 等。

use sflang::Sflang;
use sflang::value::Value;

/// eval 求值代码块并返回结果（函数体包装，src 内需显式 return）。
fn eval(src: &str) -> Value {
    let mut sf = Sflang::new();
    let wrapped = format!("func __f() {{ {} }} var __r = __f()", src);
    sf.run_string(&wrapped).expect("eval failed");
    sf.get_global("__r").expect("__r not set")
}

/// SharedBuf 共享输出缓冲（set_output 需要 'static + Send）。
#[derive(Clone)]
struct SharedBuf(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl SharedBuf {
    fn new() -> Self {
        SharedBuf(std::sync::Arc::new(std::sync::Mutex::new(Vec::new())))
    }
}

impl std::io::Write for SharedBuf {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

/// run_out 执行代码并捕获输出文本（pln 输出到 VM 的输出缓冲）。
fn run_out(src: &str) -> (Result<Value, Value>, SharedBuf) {
    let mut sf = Sflang::new();
    let buf = SharedBuf::new();
    sf.set_output(buf.clone());
    let r = sf.run_string(src);
    (r, buf)
}

/// norm 把输出字节规范化为行数组（去空行）。
fn lines(buf: &SharedBuf) -> Vec<String> {
    String::from_utf8_lossy(&buf.0.lock().unwrap())
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

// ---- try / catch / finally 语义 ----

/// H1：catch 不带变量时 catch 块必须执行（此前被静默丢弃）。
#[test]
fn test_catch_without_var_executes() {
    let (r, out) = run_out(r#"try { throw("x") } catch { pln("CAUGHT") } pln("after")"#);
    assert!(r.is_ok());
    assert_eq!(lines(&out), vec!["CAUGHT", "after"]);
}

/// H2：try+catch+finally 正常路径 finally 必须执行（此前被跳过）。
#[test]
fn test_finally_on_normal_path() {
    let (r, out) = run_out(r#"try { pln("try") } catch (e) { pln("catch") } finally { pln("FIN") } pln("end")"#);
    assert!(r.is_ok());
    assert_eq!(lines(&out), vec!["try", "FIN", "end"]);
}

/// H3：try+catch（无 finally）中的 return 生效，不被吞（此前返回 "after-try"）。
#[test]
fn test_return_in_try_catch_only() {
    assert_eq!(eval(r#"try { return "from-try" } catch (e) { return "from-catch" } return "after""#),
               Value::str("from-try"));
}

/// H4：catch 中的 return 也要执行 finally（此前 finally 被跳过）。
#[test]
fn test_return_in_catch_runs_finally() {
    let (r, out) = run_out(r#"
func f() {
    try { throw("boom") } catch (e) { return "caught" } finally { pln("FIN") }
}
pln(f())"#);
    assert!(r.is_ok());
    assert_eq!(lines(&out), vec!["FIN", "caught"]);
}

/// H4b：catch 中再 throw 也要执行 finally，异常继续传播。
#[test]
fn test_throw_in_catch_runs_finally_and_propagates() {
    let (r, out) = run_out(r#"
func f() {
    try { throw("a") } catch (e) { throw("b:" + e) } finally { pln("FIN") }
}
try { f() } catch (e) { pln("caught", e) }"#);
    assert!(r.is_ok());
    assert_eq!(lines(&out), vec!["FIN", "caught b:a"]);
}

/// H5：finally 中的 return 覆盖挂起的 return（标准语义）。
#[test]
fn test_return_in_finally_overrides() {
    assert_eq!(eval(r#"try { return "try" } finally { return "finally" }"#),
               Value::str("finally"));
}

/// finally 中的 throw 覆盖挂起的异常。
#[test]
fn test_throw_in_finally_overrides() {
    let (r, out) = run_out(r#"
func f() { try { throw("orig") } finally { throw("in-fin") } }
try { f() } catch (e) { pln("got", e) }"#);
    assert!(r.is_ok());
    assert_eq!(lines(&out), vec!["got in-fin"]);
}

/// break 穿出 try 块：finally 执行，且控制流不再错乱（此前 break 后循环"复活"）。
#[test]
fn test_break_crossing_try() {
    let (r, out) = run_out(r#"
var log = []
var i = 0
while i < 3 {
    i++
    try {
        if i == 1 { break }
        push(log, "body" + i)
    } catch (e) {
        push(log, "STALE")
    }
}
push(log, "after:" + i)
pln(strJoin(log, ","))"#);
    assert!(r.is_ok());
    assert_eq!(lines(&out), vec!["after:1"]);
}

/// break 穿出带 finally 的 try：finally 必须执行。
#[test]
fn test_break_crossing_finally() {
    let (r, out) = run_out(r#"
var t = 0
for i in range(3) {
    try { if i == 1 { break }; t += i } finally { t += 100 }
}
pln(t)"#);
    assert!(r.is_ok());
    // i=0: +0 且 finally +100 → 100；i=1: break 但 finally 仍 +100 → 200
    assert_eq!(lines(&out), vec!["200"]);
}

/// continue 穿出 try/finally：finally 必须执行（此前被跳过）。
#[test]
fn test_continue_crossing_finally() {
    let (r, out) = run_out(r#"
var t = 0
for i in range(3) {
    try { if i == 1 { continue }; t += i } finally { t += 100 }
}
pln(t)"#);
    assert!(r.is_ok());
    // i=0: 0+100；i=1: continue 但 finally +100；i=2: 2+100 → 302
    assert_eq!(lines(&out), vec!["302"]);
}

/// 嵌套 try：内层异常由内层 catch 处理，外层不受影响。
#[test]
fn test_nested_try() {
    assert_eq!(eval(r#"
var r = ""
try {
    try { throw("inner") } catch (e) { r = "inner:" + e }
    r += "|outer-body"
} catch (e) { r += "|outer-catch" } finally { r += "|fin" }
return r"#),
        Value::str("inner:inner|outer-body|fin"));
}

/// try 内表达式半求值时抛异常，操作数栈应被恢复（无残留垃圾）。
#[test]
fn test_stack_snapshot_on_throw() {
    // 1 + f() 中 f 抛异常：catch 后继续大量运算，栈不应错乱
    assert_eq!(eval(r#"
func bad() { throw("x") }
var acc = 0
try { acc = 1 + bad() } catch (e) { acc = 10 }
acc = acc + 1
acc = acc * 2
return acc"#),
        Value::Int(22));
}

// ---- defer 语义 ----

/// throw 穿透时 defer 必须执行（此前被跳过导致 defer unlock 永久死锁）。
#[test]
fn test_defer_runs_on_throw() {
    let (r, out) = run_out(r#"
func f() {
    defer pln("DEFER")
    throw("boom")
}
try { f() } catch (e) { pln("caught", e) }"#);
    assert!(r.is_ok());
    assert_eq!(lines(&out), vec!["DEFER", "caught boom"]);
}

/// defer 逆序执行；finally（try 退出）先于 defer（函数退出）。
#[test]
fn test_defer_order_vs_finally() {
    let (r, out) = run_out(r#"
func f() {
    defer pln("defer")
    try { pln("body") } finally { pln("finally") }
    pln("tail")
}
f()"#);
    assert!(r.is_ok());
    assert_eq!(lines(&out), vec!["body", "finally", "tail", "defer"]);
}

/// 多个 defer：一个抛错仍执行其余（锁资源全部释放）。
#[test]
fn test_defer_error_runs_rest() {
    let (r, out) = run_out(r#"
func boom() { throw("d2") }
func f() {
    defer pln("d1")
    defer boom()
    defer pln("d3")
}
try { f() } catch (e) { pln("caught", e) }"#);
    assert!(r.is_ok());
    // 逆序：d3 → boom(d2 抛错) → d1 仍执行；最终 defer 错误传播为 Throw
    assert_eq!(lines(&out), vec!["d3", "d1", "caught d2"]);
}

// ---- 深循环 try-catch（栈溢出回归）----

/// 20 万次循环内 throw-catch 不应耗尽 Rust 栈（此前每次嵌套一层 run_frame）。
#[test]
fn test_deep_loop_try_catch_no_stack_overflow() {
    let (r, out) = run_out(r#"
var i = 0
while i < 200000 {
    try { throw("x") } catch (e) { }
    i++
}
pln("survived", i)"#);
    assert!(r.is_ok());
    assert_eq!(lines(&out), vec!["survived 200000"]);
}

// ---- 闭包 ----

/// 两层嵌套闭包捕获外层变量（此前 vm.rs OpClosure 越界 panic）。
#[test]
fn test_two_level_closure_capture() {
    assert_eq!(eval(r#"
func outer() {
    var n = 10
    func middle() {
        func inner() { n += 1; return n }
        return inner
    }
    return middle
}
var mk = outer()
var inner = mk()
return inner()"#),
        Value::Int(11));
}

/// 两层捕获的写共享：多次调用修改同一变量。
#[test]
fn test_two_level_closure_shared_write() {
    assert_eq!(eval(r#"
func outer() {
    var n = 0
    func middle() {
        func inner() { n += 2; return n }
        return inner
    }
    return middle
}
var mk = outer()
var i1 = mk()
var i2 = mk()
i1()
i2()
return i1()"#),
        Value::Int(6));
}

// ---- 解析器回归 ----

/// `1 = 2` 非法赋值目标必须报编译错误（此前静默写入空名变量）。
#[test]
fn test_invalid_assign_target_rejected() {
    let mut sf = Sflang::new();
    assert!(sf.run_string("1 = 2").is_err());
    let mut sf2 = Sflang::new();
    assert!(sf2.run_string("f() = 1").is_err());
    // 正常前缀自增仍然可用（分号分隔，避免 `1` 与 `++x` 粘连成 `1++`）
    let mut sf3 = Sflang::new();
    assert!(sf3.run_string("var x = 1; ++x; pln(x)").is_ok());
    // 对字面量自增必须报错
    let mut sf4 = Sflang::new();
    assert!(sf4.run_string("var y = 1; ++5; pln(y)").is_err());
}

/// C 风格 for 的普通表达式 init（此前分号错位导致误导性报错）。
#[test]
fn test_c_style_for_expr_init() {
    let (r, out) = run_out(r#"
var i
for (i = 0; i < 3; i++) { pln(i) }
pln("after", i)"#);
    assert!(r.is_ok());
    assert_eq!(lines(&out), vec!["0", "1", "2", "after 3"]);
}

/// C 风格 for 的 var init 与空 init 混用。
#[test]
fn test_c_style_for_var_init() {
    let (r, out) = run_out(r#"
for (var j = 2; j < 6; j += 2) { pln(j) }"#);
    assert!(r.is_ok());
    assert_eq!(lines(&out), vec!["2", "4"]);
}

/// 裸 try（无 catch 无 finally）必须被拒绝。
#[test]
fn test_bare_try_rejected() {
    let mut sf = Sflang::new();
    assert!(sf.run_string("try { pln(1) }").is_err());
}

/// -9223372036854775808（i64::MIN）字面量可写（此前解析失败）。
#[test]
fn test_i64_min_literal() {
    assert_eq!(eval("return -9223372036854775807 - 1"), Value::Int(i64::MIN));
    let mut sf = Sflang::new();
    assert!(sf.run_string("var x = -9223372036854775808\npln(x)").is_ok());
}

// ---- 词法器回归 ----

/// `//` 与 `#` 行注释、`/* */` 块注释内含括号不影响解析。
#[test]
fn test_comments_with_parens() {
    assert_eq!(eval("// 注释 ( 未闭合括号\nreturn 42"), Value::Int(42));
    assert_eq!(eval("# 注释 ( 也没问题\nreturn 43"), Value::Int(43));
    assert_eq!(eval("/* 块注释 ( { [ */ return 44"), Value::Int(44));
}

/// 插值表达式内含字符串字面量花括号（配对感知）。
#[test]
fn test_interp_with_brace_in_string() {
    let mut sf = Sflang::new();
    sf.run_string(r#"var s = "a}b" pln("v=${s}")"#).expect("ok");
}

/// 插值文本含 \0 转义不再导致分段错乱。
#[test]
fn test_interp_with_nul_escape() {
    let mut sf = Sflang::new();
    sf.run_string(r#"var x = "X" var s = "a\0b${x}c" pln(len(s))"#).expect("ok");
}

// ---- defer + finally + return 组合冒烟 ----

/// 组合场景：锁式资源释放在所有退出路径成立。
#[test]
fn test_resource_cleanup_all_paths() {
    let (r, out) = run_out(r#"
var released = []
func withRes(name, fail) {
    defer push(released, name)
    if fail { throw("fail:" + name) }
    return "ok:" + name
}
try { withRes("a", false) } catch (e) { }
try { withRes("b", true) } catch (e) { }
pln(strJoin(released, ","))"#);
    assert!(r.is_ok());
    assert_eq!(lines(&out), vec!["a,b"]);
}
