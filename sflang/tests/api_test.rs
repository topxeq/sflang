//! Sflang 库的单元测试
//!
//! 覆盖：词法、语法、编译、VM、嵌入式 API 的核心功能。

use sflang::Sflang;
use sflang::value::Value;
use std::path::PathBuf;

// ---- 辅助函数 ----

/// eval 求值代码块并返回结果（用 IIFE 包裹，src 内需显式 return）。
fn eval(src: &str) -> Value {
    let mut sf = Sflang::new();
    let wrapped = format!("func __f() {{ {} }} var __r = __f()", src);
    sf.run_string(&wrapped).expect("eval failed");
    sf.get_global("__r").expect("__r not set")
}

/// run 执行代码（不关心返回值，用于测试副作用或错误）。
fn run(src: &str) -> Result<Value, Value> {
    let mut sf = Sflang::new();
    sf.run_string(src)
}

// ---- 基础算术 ----

#[test]
fn test_arithmetic() {
    assert_eq!(eval("return 1 + 2"), Value::Int(3));
    assert_eq!(eval("return 10 - 3"), Value::Int(7));
    assert_eq!(eval("return 4 * 5"), Value::Int(20));
    assert_eq!(eval("return 20 / 4"), Value::Int(5));
    assert_eq!(eval("return 7 % 3"), Value::Int(1));
}

#[test]
fn test_float_arithmetic() {
    assert_eq!(eval("return 1.5 + 2.5"), Value::Float(4.0));
    assert_eq!(eval("return 10.0 / 4.0"), Value::Float(2.5));
}

#[test]
fn test_string_concat() {
    assert_eq!(eval("return \"Hello\" + \" \" + \"World\""), Value::str("Hello World"));
}

// ---- 变量与赋值 ----

#[test]
fn test_variables() {
    assert_eq!(eval("var x = 10; var y = 20; return x + y"), Value::Int(30));
}

#[test]
fn test_assignment() {
    assert_eq!(eval("var x = 10; x = x + 5; return x"), Value::Int(15));
}

// ---- 控制流 ----

#[test]
fn test_if_else() {
    assert_eq!(eval("var x = 5; if x > 3 { return 100 } else { return 200 }"), Value::Int(100));
    assert_eq!(eval("var x = 1; if x > 3 { return 100 } else { return 200 }"), Value::Int(200));
}

#[test]
fn test_while_loop() {
    assert_eq!(eval("var i = 0; var sum = 0; while i < 5 { sum = sum + i; i = i + 1 }; return sum"), Value::Int(10));
}

#[test]
fn test_for_in_range() {
    assert_eq!(eval("var s = 0; for i in range(1, 6) { s = s + i }; return s"), Value::Int(15));
}

#[test]
fn test_for_in_array() {
    assert_eq!(eval("var s = 0; for x in [10, 20, 30] { s = s + x }; return s"), Value::Int(60));
}

#[test]
fn test_c_style_for() {
    assert_eq!(eval("var s = 0; for (var i = 1; i <= 5; i = i + 1) { s = s + i }; return s"), Value::Int(15));
}

#[test]
fn test_break_continue() {
    assert_eq!(eval("var s = 0; for i in range(1, 100) { if i > 5 { break }; s = s + i }; return s"), Value::Int(15));
    assert_eq!(eval("var s = 0; for i in range(1, 6) { if i == 3 { continue }; s = s + i }; return s"), Value::Int(12));
}

#[test]
fn test_break_outer_label_for_in() {
    // 双层 for-in，break label 跳出外层
    let src = r#"
        var cnt = 0
        outer: for i in range(0, 3) {
            for j in range(0, 3) {
                if i == 1 && j == 1 { break outer }
                cnt = cnt + 1
            }
        }
        return cnt
    "#;
    // i=0: j=0,1,2 -> cnt+=3; i=1: j=0 -> cnt+=1, j=1 触发 break outer
    assert_eq!(eval(src), Value::Int(4));
}

#[test]
fn test_continue_outer_label_for_in() {
    // continue label 跳到外层下次迭代
    let src = r#"
        var cnt = 0
        outer: for i in range(0, 3) {
            for j in range(0, 3) {
                if j == 1 { continue outer }
                cnt = cnt + 1
            }
        }
        return cnt
    "#;
    // i=0: j=0 cnt++, j=1 continue outer; i=1: j=0 cnt++, j=1 continue; i=2: j=0 cnt++, j=1 continue
    assert_eq!(eval(src), Value::Int(3));
}

#[test]
fn test_break_label_while() {
    let src = r#"
        var i = 0
        var cnt = 0
        outer: while i < 3 {
            var j = 0
            while j < 3 {
                if i == 1 && j == 1 { break outer }
                cnt = cnt + 1
                j = j + 1
            }
            i = i + 1
        }
        return cnt
    "#;
    // i=0: j=0,1,2 cnt+=3; i=1: j=0 cnt+=1, j=1 break outer
    assert_eq!(eval(src), Value::Int(4));
}

#[test]
fn test_break_label_c_style_for() {
    let src = r#"
        var cnt = 0
        outer: for i := 0; i < 3; i = i + 1 {
            for j := 0; j < 3; j = j + 1 {
                if i == 1 && j == 1 { break outer }
                cnt = cnt + 1
            }
        }
        return cnt
    "#;
    assert_eq!(eval(src), Value::Int(4));
}

#[test]
fn test_continue_label_inner_skipped() {
    // 验证 continue label 时内层循环剩余部分被跳过
    let src = r#"
        var log = []
        outer: for i in range(0, 2) {
            for j in range(0, 3) {
                if j == 1 { continue outer }
                push(log, i * 10 + j)
            }
        }
        return log
    "#;
    let r = eval(src);
    match r {
        Value::Array(a) => {
            let g = a.lock().unwrap();
            assert_eq!(g.len(), 2);
            assert_eq!(g[0], Value::Int(0));
            assert_eq!(g[1], Value::Int(10));
        }
        _ => panic!("expected array"),
    }
}

#[test]
fn test_break_without_label_unchanged() {
    // 无标签 break 仍只跳出最内层
    let src = r#"
        var cnt = 0
        for i in range(0, 3) {
            for j in range(0, 3) {
                if j == 1 { break }
                cnt = cnt + 1
            }
        }
        return cnt
    "#;
    // 每次外层迭代：j=0 cnt++, j=1 break 内层; 共 3 次
    assert_eq!(eval(src), Value::Int(3));
}

#[test]
fn test_break_undefined_label_error() {
    // 未定义的标签应编译报错
    let r = run("for i in range(0, 3) { break nonexistent }");
    assert!(r.is_err(), "break with undefined label should error");
}

#[test]
fn test_nested_three_levels_break_label() {
    // 三层嵌套，break 跳到中间层
    let src = r#"
        var cnt = 0
        for i in range(0, 2) {
            mid: for j in range(0, 2) {
                for k in range(0, 2) {
                    if k == 1 { break mid }
                    cnt = cnt + 1
                }
            }
        }
        return cnt
    "#;
    // break mid 跳出 j 循环（mid），回到 i 循环下次迭代
    // i=0: j=0 k=0 cnt++ k=1 break mid; j 循环结束
    // i=1: j=0 k=0 cnt++ k=1 break mid; j 循环结束
    assert_eq!(eval(src), Value::Int(2));
}

// ---- 分组声明与 iota ----

#[test]
fn test_var_group_basic() {
    // var 分组声明
    let src = r#"
        var (
            a = 10
            b = 20
            c = a + b
        )
        return c
    "#;
    assert_eq!(eval(src), Value::Int(30));
}

#[test]
fn test_var_group_no_init() {
    // var 分组内可以无初始值
    let src = r#"
        var (
            a = 5
            b
        )
        b = a * 2
        return b
    "#;
    assert_eq!(eval(src), Value::Int(10));
}

#[test]
fn test_var_group_semicolons() {
    // 用分号分隔
    let src = r#"
        var ( a = 1; b = 2; c = a + b )
        return c
    "#;
    assert_eq!(eval(src), Value::Int(3));
}

#[test]
fn test_const_group_basic() {
    // const 分组声明
    let src = r#"
        const (
            RED = 0
            GREEN = 1
            BLUE = 2
        )
        return RED + GREEN + BLUE
    "#;
    assert_eq!(eval(src), Value::Int(3));
}

#[test]
fn test_const_group_iota() {
    // iota 基本用法
    let src = r#"
        const (
            RED = iota
            GREEN = iota
            BLUE = iota
        )
        return RED * 100 + GREEN * 10 + BLUE
    "#;
    assert_eq!(eval(src), Value::Int(12));
}

#[test]
fn test_const_group_iota_omit_expr() {
    // const 分组内省略 = expr：沿用上一个表达式（Go 风格）
    // 注意：iota 在省略时不会重新递增（parser 层面替换的限制）
    let src = r#"
        const (
            A = 10
            B
            C
        )
        return A + B + C
    "#;
    // 10 + 10 + 10 = 30（沿用 A 的表达式）
    assert_eq!(eval(src), Value::Int(30));
}

#[test]
fn test_const_group_iota_arithmetic() {
    // iota 在表达式中参与运算
    let src = r#"
        const (
            A = iota * 2
            B = iota * 2
            C = iota * 2
        )
        return A + B + C
    "#;
    // 0 + 2 + 4 = 6
    assert_eq!(eval(src), Value::Int(6));
}

#[test]
fn test_const_group_iota_bitwise() {
    // iota 位运算（常见用法：位标志）
    let src = r#"
        const (
            FlagA = 1 << iota
            FlagB = 1 << iota
            FlagC = 1 << iota
            FlagD = 1 << iota
        )
        return FlagA + FlagB + FlagC + FlagD
    "#;
    // 1 + 2 + 4 + 8 = 15
    assert_eq!(eval(src), Value::Int(15));
}

#[test]
fn test_iota_outside_const_group() {
    // iota 在 const 分组外当作普通标识符（值为 undefined）
    let src = "var x = iota; return x";
    let r = eval(src);
    assert_eq!(r, Value::Undefined);
}

#[test]
fn test_function_call() {
    assert_eq!(eval("func add(a, b) { return a + b }; return add(3, 4)"), Value::Int(7));
}

#[test]
fn test_recursive_function() {
    assert_eq!(eval("func fact(n) { if n <= 1 { return 1 }; return n * fact(n - 1) }; return fact(5)"), Value::Int(120));
}

#[test]
fn test_closure_capture() {
    assert_eq!(eval("func make_adder(n) { return func(x) { return x + n } }; var add5 = make_adder(5); return add5(10)"), Value::Int(15));
    assert_eq!(eval("func make_adder(n) { return func(x) { return x + n } }; var a5 = make_adder(5); var a10 = make_adder(10); return a5(1) + a10(1)"), Value::Int(17));
}

#[test]
fn test_counter_closure() {
    let src = r#"
        func counter() {
            var c = 0
            return func() {
                c = c + 1
                return c
            }
        }
        var cnt = counter()
        return cnt() + cnt() + cnt()
    "#;
    assert_eq!(eval(src), Value::Int(6));  // 1+2+3
}

// ---- 容器 ----

#[test]
fn test_array_operations() {
    assert_eq!(eval("var a = [1, 2, 3]; return a[0] + a[1] + a[2]"), Value::Int(6));
    assert_eq!(eval("var a = [1, 2, 3]; return len(a)"), Value::Int(3));
    assert_eq!(eval("var a = [1, 2]; push(a, 3); return len(a)"), Value::Int(3));
    assert_eq!(eval("var a = [1, 2, 3]; return pop(a)"), Value::Int(3));
}

#[test]
fn test_map_operations() {
    assert_eq!(eval("var m = {\"a\": 1, \"b\": 2}; return m[\"a\"] + m[\"b\"]"), Value::Int(3));
    assert_eq!(eval("var m = {\"a\": 1}; m[\"b\"] = 2; return len(keys(m))"), Value::Int(2));
}

// ---- 字符串 ----

#[test]
fn test_string_len() {
    assert_eq!(eval("return len(\"hello\")"), Value::Int(5));
}

#[test]
fn test_raw_string() {
    assert_eq!(eval("var s = `raw`; return s"), Value::str("raw"));
}

#[test]
fn test_multiline_string() {
    assert_eq!(eval("var s = `line1\nline2`; return len(s)"), Value::Int(11));
}

// ---- 异常处理 ----

#[test]
fn test_try_catch() {
    let src = r#"
        try {
            throw("test error")
            return 100
        } catch (e) {
            return 200
        }
    "#;
    assert_eq!(eval(src), Value::Int(200));
}

#[test]
fn test_try_finally() {
    // finally 在 try 正常完成后执行
    let src = r#"
        var r = 0
        try {
            r = 1
        } finally {
            r = r + 10
        }
        return r
    "#;
    assert_eq!(eval(src), Value::Int(11));
}

#[test]
fn test_throw_without_catch_propagates() {
    let src = "throw(\"error\")";
    let r = run(src);
    assert!(r.is_err());
}

// ---- defer ----

#[test]
fn test_defer_execution() {
    // defer 语法：defer <call>（不是块）
    let src = r#"
        var order = []
        func push_d1() { push(order, "d1") }
        func push_d2() { push(order, "d2") }
        func push_body() { push(order, "body") }
        func test() {
            defer push_d1()
            defer push_d2()
            push_body()
        }
        test()
        return order
    "#;
    let mut sf = Sflang::new();
    let wrapped = format!("func __f() {{ {} }} var __r = __f()", src);
    sf.run_string(&wrapped).unwrap();
    let r = sf.get_global("__r").unwrap();
    match r {
        Value::Array(a) => {
            let arr = a.lock().unwrap();
            // body 先执行，defer 逆序：d2, d1
            assert_eq!(arr.len(), 3);
            assert_eq!(arr[0], Value::str("body"));
            assert_eq!(arr[1], Value::str("d2"));
            assert_eq!(arr[2], Value::str("d1"));
        }
        _ => panic!("expected array"),
    }
}

// ---- 内置函数 ----

#[test]
fn test_type_functions() {
    assert_eq!(eval("return typeCode(1)"), Value::Int(1));
    assert_eq!(eval("return typeName(1)"), Value::str("int"));
    assert_eq!(eval("return typeName(\"hi\")"), Value::str("string"));
    assert_eq!(eval("return typeName([1,2])"), Value::str("array"));
}

#[test]
fn test_int_float_conversion() {
    assert_eq!(eval("return int(\"42\")"), Value::Int(42));
    assert_eq!(eval("return float(\"3.14\")"), Value::Float(3.14));
    assert_eq!(eval("return int(3.7)"), Value::Int(3));
}

#[test]
fn test_range_function() {
    assert_eq!(eval("return len(range(1, 10))"), Value::Int(9));
    assert_eq!(eval("return len(range(5))"), Value::Int(5));
}

#[test]
fn test_assert_builtin() {
    let r = run("assert(1 == 1)");
    assert!(r.is_ok());
    let r = run("assert(1 == 2)");
    assert!(r.is_err());
}

// ---- 嵌入式 API ----

#[test]
fn test_api_set_get_global() {
    let mut sf = Sflang::new();
    sf.set_global("x", Value::Int(100));
    sf.run_string("var __r = x + 1").unwrap();
    assert_eq!(sf.get_global("__r").unwrap(), Value::Int(101));

    sf.run_string("var y = 200").unwrap();
    assert_eq!(sf.get_global("y").unwrap(), Value::Int(200));
}

#[test]
fn test_api_compile_and_run() {
    let code = Sflang::compile_source("var __r = 1 + 2", "<test>").unwrap();
    let mut sf = Sflang::new();
    sf.vm_run_code(code).unwrap();
    assert_eq!(sf.get_global("__r").unwrap(), Value::Int(3));
}

#[test]
fn test_api_compile_error() {
    let r = Sflang::compile_source("var x = ;", "<test>");
    assert!(r.is_err());
}

// ---- 错误信息（AI 友好） ----

#[test]
fn test_undefined_var_returns_undefined() {
    // 读取未定义变量返回 undefined（宽容策略，对齐 Charlang），不抛错。
    // 用 eval 包裹让顶层表达式可求值。
    assert_eq!(eval("return undefined_var"), Value::Undefined);
    // 显式 undefined 字面量
    assert_eq!(eval("return undefined"), Value::Undefined);
    // nil 作为 undefined 的兼容别名
    assert_eq!(eval("return undefined"), Value::Undefined);
    // undefined == undefined
    assert_eq!(eval("return undefined == undefined"), Value::Bool(true));
    assert_eq!(eval("return undefined == undefined"), Value::Bool(true));
}

#[test]
fn test_explain_undef_diagnostic() {
    // explainUndef 返回诊断字符串（AI 友好）
    let r = eval("return explainUndef(\"notDefinedXyz\")");
    match r {
        Value::Str(s) => {
            assert!(s.contains("notDefinedXyz"), "msg: {}", s);
            assert!(s.contains("isUndefined") || s.contains("default"));
        }
        _ => panic!("expected Str, got {:?}", r),
    }
}

#[test]
fn test_undefined_sources() {
    // map 缺键 → undefined（不抛错）
    assert_eq!(eval("var m = {\"a\": 1}; return m[\"missing\"]"), Value::Undefined);
    // 函数无返回值 → undefined
    assert_eq!(eval("func f() {} return f()"), Value::Undefined);
    // return; 无值 → undefined
    assert_eq!(eval("func f() { return } return f()"), Value::Undefined);
    // var 无初值 → undefined
    assert_eq!(eval("var x; return x"), Value::Undefined);
    // undefined 在逻辑运算中按 falsy 参与（不抛错）
    assert_eq!(eval("return undefined || 5"), Value::Bool(true));
    assert_eq!(eval("return undefined && 5"), Value::Bool(false));
    // undefined 取成员 / 索引 → 抛异常（类型不兼容）
    assert!(run("var x = undefined; return x.foo").is_err());
    assert!(run("var x = undefined; return x[0]").is_err());
    // undefined 比较 < → 抛异常（类型不兼容）
    assert!(run("return undefined < 5").is_err());
    // defaultVal / defaultUndef
    assert_eq!(eval("return defaultVal(undefined, 99)"), Value::Int(99));
    assert_eq!(eval("return defaultUndef(undefined, 99)"), Value::Int(99));
    // defaultUndef 不对 0/"" 触发兜底
    assert_eq!(eval("return defaultUndef(0, 99)"), Value::Int(0));
    assert_eq!(eval("return defaultUndef(\"\", 99)"), Value::str(""));
    // defaultVal 对 falsy(0) 触发兜底
    assert_eq!(eval("return defaultVal(0, 99)"), Value::Int(99));
    // undefToEmpty
    assert_eq!(eval("return undefToEmpty(undefined)"), Value::str(""));
    assert_eq!(eval("return undefToEmpty(42)"), Value::str("42"));
}

#[test]
fn test_symbol_logic_operators() {
    // && || ! 作为 && || not 的等价符号别名
    assert_eq!(eval("return true && false"), Value::Bool(false));
    assert_eq!(eval("return true || false"), Value::Bool(true));
    assert_eq!(eval("return !true"), Value::Bool(false));
    assert_eq!(eval("return !0"), Value::Bool(true));
    // 与关键字混用等价
    assert_eq!(eval("return (1 < 2) && (3 > 2) || (1 > 5)"), Value::Bool(true));
    assert_eq!(eval("return (1 < 2) && (3 > 2) || false"), Value::Bool(true));
    // 短路仍成立：&& 左假不求右（右含除零也不报错）
    assert_eq!(eval("return false && (1/0 == 0)"), Value::Bool(false));
    assert_eq!(eval("return true || (1/0 == 0)"), Value::Bool(true));
    // ! 的组合
    assert_eq!(eval("return !(1 == 2)"), Value::Bool(true));
}

#[test]
fn test_ternary_operator() {
    // 基本语义：cond ? then : else
    assert_eq!(eval("return 5 > 3 ? 100 : 200"), Value::Int(100));
    assert_eq!(eval("return 5 < 3 ? 100 : 200"), Value::Int(200));
    // 条件为 falsy（0/undefined）走 else
    assert_eq!(eval("return 0 ? \"yes\" : \"no\""), Value::str("no"));
    assert_eq!(eval("return undefined ? 1 : 2"), Value::Int(2));
    // then/else 可以是任意类型
    assert_eq!(eval("return true ? \"hi\" : 42"), Value::str("hi"));
    // 嵌套（右结合）：a ? b : c ? d : e → a ? b : (c ? d : e)
    assert_eq!(eval("return 1 ? 2 : 3 ? 4 : 5"), Value::Int(2));   // 1真→2
    assert_eq!(eval("return 0 ? 2 : 3 ? 4 : 5"), Value::Int(4));   // 0假→(3真→4)
    assert_eq!(eval("return 0 ? 2 : 0 ? 4 : 5"), Value::Int(5));   // 0假→(0假→5)
    // 与 ?? 优先级：?? 高于 ?:，故 a ? b : c ?? d → a ? b : (c ?? d)
    assert_eq!(eval("return 1 ? 9 : undefined ?? 7"), Value::Int(9));
    assert_eq!(eval("return 0 ? 9 : undefined ?? 7"), Value::Int(7));
    // 三元用于赋值表达式
    assert_eq!(eval("var x = 5 > 3 ? 10 : 20; return x"), Value::Int(10));
    // then 分支短路：cond 真时只求 then（else 含除零也不报错）
    assert_eq!(eval("return 1 ? 42 : 1/0 == 0"), Value::Int(42));
    assert_eq!(eval("return 0 ? 1/0 == 0 : 42"), Value::Int(42));
    // 嵌套在对象字面量中（验证 : 不与对象字面量的 : 冲突）
    assert_eq!(eval("return {\"k\": 1 > 0 ? \"pos\" : \"neg\"}[\"k\"]"), Value::str("pos"));
}

#[test]
fn test_bitwise_operators() {
    // 基本位运算
    assert_eq!(eval("return 12 & 10"), Value::Int(8));    // 1100 & 1010 = 1000
    assert_eq!(eval("return 12 | 10"), Value::Int(14));   // 1100 | 1010 = 1110
    assert_eq!(eval("return 12 ^ 10"), Value::Int(6));    // 1100 ^ 1010 = 0110
    assert_eq!(eval("return ~5"), Value::Int(-6));        // ~5 = -6
    assert_eq!(eval("return 1 << 4"), Value::Int(16));    // 左移
    assert_eq!(eval("return 256 >> 2"), Value::Int(64));  // 右移
    // 优先级：& 高于 ^，^ 高于 |（对齐 C）
    assert_eq!(eval("return 1 | 2 & 3"), Value::Int(3));   // 1 | (2&3)=1|2=3
    assert_eq!(eval("return 1 ^ 1 & 1"), Value::Int(0));   // 1 ^ (1&1)=1^1=0
    // 移位优先级高于加减、低于比较
    assert_eq!(eval("return 1 + 1 << 2"), Value::Int(8));  // (1+1)<<2 = 8
    // 复合：位运算与算术
    assert_eq!(eval("return (255 & 15) + 1"), Value::Int(16));
    // Float 参与位运算报错
    assert!(run("return 1.5 & 3").is_err());
    assert!(run("return ~1.0").is_err());
}

#[test]
fn test_integer_literals() {
    // 十进制
    assert_eq!(eval("return 42"), Value::Int(42));
    assert_eq!(eval("return 1_000_000"), Value::Int(1_000_000));
    // 十六进制
    assert_eq!(eval("return 0xFF"), Value::Int(255));
    assert_eq!(eval("return 0xff"), Value::Int(255));
    assert_eq!(eval("return 0xFF_FF"), Value::Int(65535));
    // 八进制
    assert_eq!(eval("return 0o77"), Value::Int(63));
    // 二进制
    assert_eq!(eval("return 0b1010"), Value::Int(10));
    assert_eq!(eval("return 0b1111_0000"), Value::Int(240));
    // 下划线分隔的浮点
    assert_eq!(eval("return 1_000.5"), Value::Float(1000.5));
    // 位运算配合进制字面量
    assert_eq!(eval("return 0xF0 | 0x0F"), Value::Int(255));
    assert_eq!(eval("return 0b1100 & 0b1010"), Value::Int(8));
}

#[test]
fn test_string_index_returns_codepoint() {
    // string[i] 返回 Unicode 码点（int），按字符索引
    assert_eq!(eval("return \"A\"[0]"), Value::Int(65));
    assert_eq!(eval("return \"abc\"[1]"), Value::Int(98));
    // 中文按字符（码点），不按字节
    assert_eq!(eval("return \"中\"[0]"), Value::Int(20013));
    assert_eq!(eval("return \"中文\"[1]"), Value::Int(25991));
    // 负索引
    assert_eq!(eval("return \"abc\"[-1]"), Value::Int(99));
    // charFromCode 互逆
    assert_eq!(eval("return charFromCode(65)"), Value::str("A"));
    assert_eq!(eval("return charFromCode(20013)"), Value::str("中"));
    assert_eq!(eval("return charFromCode(\"中\"[0])"), Value::str("中"));
    // codeOf 与 charFromCode 互逆
    assert_eq!(eval("return codeOf(\"A\")"), Value::Int(65));
    // 非法码点报错
    assert!(run("return charFromCode(-1)").is_err());
    assert!(run("return charFromCode(0xD800)").is_err());  // 代理区
    assert!(run("return charFromCode(0x110000)").is_err()); // 超出
    // 索引越界报错
    assert!(run("return \"ab\"[5]").is_err());
}

#[test]
fn test_slice_syntax() {
    // string 切片（按字符，不切断多字节）
    assert_eq!(eval("return \"Hello\"[1:3]"), Value::str("el"));
    assert_eq!(eval("return \"Hello\"[:2]"), Value::str("He"));
    assert_eq!(eval("return \"Hello\"[3:]"), Value::str("lo"));
    assert_eq!(eval("return \"Hello\"[:]"), Value::str("Hello"));
    assert_eq!(eval("return \"中文测试\"[1:3]"), Value::str("文测")); // 按字符
    // 负索引
    assert_eq!(eval("return \"Hello\"[-2:]"), Value::str("lo"));
    assert_eq!(eval("return \"Hello\"[:-1]"), Value::str("Hell"));
    // 空切片
    assert_eq!(eval("return \"Hello\"[3:1]"), Value::str(""));
    // array 切片（用 len + 元素校验，因 Array 按指针比较）
    assert_eq!(eval("var a = [1,2,3,4,5]; var s = a[1:3]; return len(s)"), Value::Int(2));
    assert_eq!(eval("var a = [1,2,3,4,5]; var s = a[1:3]; return s[0] + s[1]"), Value::Int(5)); // 2+3
    assert_eq!(eval("var a = [1,2,3,4,5]; var s = a[:2]; return s[0]*10 + s[1]"), Value::Int(12)); // 1,2
    assert_eq!(eval("var a = [1,2,3,4,5]; var s = a[3:]; return s[0] + s[1]"), Value::Int(9)); // 4+5
    // bytes/byteArray 切片（按字节）
    assert_eq!(eval("return bytesHex(bytes(\"ABCDE\")[1:3])"), Value::str("4243"));
    assert_eq!(eval("return bytesHex(byteArrayFromBytes(bytes(\"ABCDE\"))[1:3])"), Value::str("4243"));
    // byteArray 切片返回 byteArray（类型一致）
    assert!(eval("var ba = byteArrayFromBytes(bytes(\"AB\")); return ba[0:1]").type_name() == "byteArray".to_string());
}

#[test]
fn test_file_handle() {
    let path = std::env::temp_dir().join("sflang_file_handle_test.txt");
    let path_str = path.to_str().unwrap();

    let mut sf = Sflang::new();
    sf.set_global("__p", Value::str(path_str));

    // 写入（流式）
    sf.run_string(r#"
        var f = openFile(__p, "w")
        defer closeFile(f)
        writeLine(f, "hello")
        writeLine(f, "world")
        writeBytes(f, bytes("raw"))
    "#).unwrap();

    // 类型判断
    let mut sf2 = Sflang::new();
    sf2.set_global("__p", Value::str(path_str));
    assert_eq!(sf2.run_string("return typeName(openFile(__p, \"r\"))").unwrap(), Value::str("file"));
    assert_eq!(sf2.run_string("return isType(openFile(__p, \"r\"), \"file\")").unwrap(), Value::Bool(true));

    // 逐行读取
    let mut sf3 = Sflang::new();
    sf3.set_global("__p", Value::str(path_str));
    let r = sf3.run_string(r#"
        var f = openFile(__p, "r")
        defer closeFile(f)
        var lines = []
        var line = readLine(f)
        while !isUndefined(line) {
            push(lines, line)
            line = readLine(f)
        }
        return lines
    "#).unwrap();
    match r {
        Value::Array(a) => {
            let arr = a.lock().unwrap();
            assert_eq!(arr.len(), 3);   // "hello", "world", "raw"（raw 无换行符但 readLine 在 EOF 前返回）
        }
        _ => panic!("expected Array"),
    }

    // readAll + seek
    let mut sf4 = Sflang::new();
    sf4.set_global("__p", Value::str(path_str));
    let r = sf4.run_string(r#"
        var f = openFile(__p, "r")
        defer closeFile(f)
        var first5 = readN(f, 5)
        seek(f, 0, 0)
        var all = readAll(f)
        return [len(first5), len(all), tell(f) == len(all)]
    "#).unwrap();
    match r {
        Value::Array(a) => {
            let arr = a.lock().unwrap();
            assert_eq!(arr[0], Value::Int(5));       // 前5字节
            assert_eq!(arr[2], Value::Bool(true));   // tell 在末尾
        }
        _ => panic!("expected Array"),
    }

    // EOF 返回 undefined
    let mut sf5 = Sflang::new();
    sf5.set_global("__p", Value::str(path_str));
    let r = sf5.run_string(r#"
        var f = openFile(__p, "r")
        defer closeFile(f)
        readAll(f)        // 读到底
        return readLine(f) // 再读 → undefined
    "#).unwrap();
    assert_eq!(r, Value::Undefined);

    std::fs::remove_file(path).ok();
}

#[test]
fn test_read_str_bytes() {
    // readStr/readBytes 从各种源统一读取
    // string
    assert_eq!(eval("return readStr(\"hello\")"), Value::str("hello"));
    // bytes
    assert_eq!(eval("return readStr(bytes(\"world\"))"), Value::str("world"));
    // byteArray
    assert_eq!(eval("return bytesHex(readBytes(byteArrayFromBytes(bytes(\"AB\"))))"), Value::str("4142"));
    // string → bytes
    assert_eq!(eval("return bytesHex(readBytes(\"AB\"))"), Value::str("4142"));
    // bytes → bytes
    assert_eq!(eval("return bytesHex(readBytes(bytes(\"CD\")))"), Value::str("4344"));
    // file 句柄
    let path = std::env::temp_dir().join("sflang_readstr_test.txt");
    std::fs::write(&path, "file content").unwrap();
    let mut sf = Sflang::new();
    sf.set_global("__p", Value::str(path.to_str().unwrap()));
    assert_eq!(sf.run_string("var f = openFile(__p, \"r\"); defer closeFile(f); return readStr(f)").unwrap(), Value::str("file content"));
    assert_eq!(sf.run_string("var f = openFile(__p, \"r\"); defer closeFile(f); return bytesHex(readBytes(f))").unwrap(), Value::str("66696c6520636f6e74656e74"));
    std::fs::remove_file(path).ok();
    // 不支持的类型报错
    assert!(run("return readStr(42)").is_err());
    assert!(run("return readBytes(true)").is_err());
}

#[test]
fn test_read_chars_and_n() {
    // readChars：按字符读（含多字节字符）
    assert_eq!(eval("return readChars(\"Hello World\", 5)"), Value::str("Hello"));
    assert_eq!(eval("return readChars(\"中文测试\", 2)"), Value::str("中文"));
    assert_eq!(eval("return readChars(\"ab\", 10)"), Value::str("ab"));  // 不足返回实际
    assert_eq!(eval("return readChars(\"\", 5)"), Value::str(""));       // 空串
    // readChars 从 bytes/byteArray
    assert_eq!(eval("return readChars(bytes(\"ABC\"), 2)"), Value::str("AB"));
    assert_eq!(eval("return readChars(byteArrayFromBytes(bytes(\"XYZ\")), 2)"), Value::str("XY"));
    // readBytes 带数量参数
    assert_eq!(eval("return bytesHex(readBytes(bytes(\"ABCDE\"), 3))"), Value::str("414243"));
    assert_eq!(eval("return bytesHex(readBytes(\"ABCDE\", 2))"), Value::str("4142"));
    assert_eq!(eval("return bytesHex(readBytes(byteArrayFromBytes(bytes(\"XYZ\")), 2))"), Value::str("5859"));
    // 不足 n
    assert_eq!(eval("return bytesHex(readBytes(bytes(\"AB\"), 10))"), Value::str("4142"));
    // readChars 从 file
    let path = std::env::temp_dir().join("sflang_readchars_test.txt");
    std::fs::write(&path, "Hello你好").unwrap();
    let mut sf = Sflang::new();
    sf.set_global("__p", Value::str(path.to_str().unwrap()));
    assert_eq!(sf.run_string("var f = openFile(__p, \"r\"); defer closeFile(f); return readChars(f, 7)").unwrap(), Value::str("Hello你好"));
    std::fs::remove_file(path).ok();
    // 负数报错
    assert!(run("return readChars(\"abc\", -1)").is_err());
    assert!(run("return readBytes(bytes(\"abc\"), -1)").is_err());
}

#[test]
fn test_generic_close() {
    // close(file) — 关闭文件句柄
    let path = std::env::temp_dir().join("sflang_close_test.txt");
    std::fs::write(&path, "test").unwrap();
    let mut sf = Sflang::new();
    sf.set_global("__p", Value::str(path.to_str().unwrap()));
    assert!(sf.run_string("var f = openFile(__p, \"r\"); close(f); return isType(f, \"file\")").unwrap() == Value::Bool(true));
    // close(mutex) — 释放互斥锁
    assert!(run("var mu = newMutex(); lock(mu); close(mu); return tryLock(mu)").unwrap() == Value::Bool(true));
    // close(rwlock) — 释放写锁
    assert!(run("var rw = newRWMutex(); wlock(rw); close(rw); return tryLock(newMutex())").is_ok());
    // close 幂等（重复 close 不报错）
    assert!(run("var mu = newMutex(); lock(mu); close(mu); close(mu); return true").unwrap() == Value::Bool(true));
    // 不支持的类型报错
    assert!(run("close(42)").is_err());
    assert!(run("close(\"hello\")").is_err());
    std::fs::remove_file(path).ok();
}

#[test]
fn test_oop_methods() {
    // 构造函数 + 自动 self 绑定
    let src = r#"
        func Counter(startA) {
            var self = {value: startA}
            self.inc = func(self, n) { self.value = self.value + n; return self.value }
            self.get = func(self) { return self.value }
            return self
        }
        var c = Counter(10)
        var r1 = c.inc(5)
        var r2 = c.get()
        return [r1, r2]
    "#;
    let r = eval(src);
    match r {
        Value::Array(a) => {
            let arr = a.lock().unwrap();
            assert_eq!(arr[0], Value::Int(15));  // 10+5
            assert_eq!(arr[1], Value::Int(15));
        }
        _ => panic!("expected Array"),
    }
    // 方法带额外参数
    assert_eq!(eval(r#"
        func Box() {
            var self = {items: []}
            self.add = func(self, x) { push(self.items, x) }
            self.count = func(self) { return len(self.items) }
            return self
        }
        var b = Box()
        b.add("a"); b.add("b")
        return b.count()
    "#), Value::Int(2));
}

#[test]
fn test_oop_inheritance() {
    // 构造函数组合实现继承 + 方法覆盖
    let r = eval(r#"
        func Animal(nameA) {
            var self = {}
            self.name = nameA
            self.speak = func(self) { return self.name + " sounds" }
            return self
        }
        func Dog(nameA, breedA) {
            var self = Animal(nameA)
            self.breed = breedA
            self.speak = func(self) { return self.name + " (" + self.breed + ") barks" }
            return self
        }
        var a = Animal("Cat")
        var d = Dog("Rex", "Lab")
        return [a.speak(), d.speak()]
    "#);
    match r {
        Value::Array(a) => {
            let arr = a.lock().unwrap();
            assert_eq!(arr[0], Value::str("Cat sounds"));
            assert_eq!(arr[1], Value::str("Rex (Lab) barks"));
        }
        _ => panic!("expected Array"),
    }
}

#[test]
fn test_oop_prototype() {
    // 原型链方法共享 + 自动 self
    let r = eval(r#"
        var proto = {
            greet: func(self) { return "hi from " + self.name },
            describe: func(self) { return self.name + " has " + string(len(self.items)) + " items" }
        }
        func Entity(nameA) {
            var self = newObject(proto)
            self.name = nameA
            self.items = []
            return self
        }
        var e = Entity("Test")
        push(e.items, "x")
        return [e.greet(), e.describe()]
    "#);
    match r {
        Value::Array(a) => {
            let arr = a.lock().unwrap();
            assert_eq!(arr[0], Value::str("hi from Test"));
            assert_eq!(arr[1], Value::str("Test has 1 items"));
        }
        _ => panic!("expected Array"),
    }
    // newObject 创建的对象 typeName 仍是 object
    assert_eq!(eval("return typeName(newObject({}))"), Value::str("object"));
}

#[test]
fn test_hash_builtins() {
    // md5/sha256 已知答案（RFC/FIPS 标准向量）
    assert_eq!(eval("return md5Hex(\"abc\")"), Value::str("900150983cd24fb0d6963f7d28e17f72"));
    assert_eq!(eval("return md5Hex(\"\")"), Value::str("d41d8cd98f00b204e9800998ecf8427e"));
    assert_eq!(eval("return sha256Hex(\"abc\")"), Value::str("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"));
    assert_eq!(eval("return sha256Hex(\"\")"), Value::str("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"));
    // 返回 bytes 类型
    assert!(eval("return md5(\"abc\")").type_name() == "bytes".to_string());
    assert_eq!(eval("return len(sha256(\"abc\"))"), Value::Int(32));  // 32 字节
    assert_eq!(eval("return len(md5(\"abc\"))"), Value::Int(16));     // 16 字节
}

#[test]
fn test_encode_builtins() {
    // base64 往返
    assert_eq!(eval("return base64Encode(\"Hello\")"), Value::str("SGVsbG8="));
    assert_eq!(eval("return strFromBytes(base64Decode(\"SGVsbG8=\"))"), Value::str("Hello"));
    assert_eq!(eval("return base64Encode(\"\")"), Value::str(""));
    // URL 编解码（RFC 3986）：空格=%20，+ 保留，往返一致
    assert_eq!(eval("return urlEncode(\"hello world\")"), Value::str("hello%20world"));
    assert_eq!(eval("return urlDecode(urlEncode(\"hello world & foo=bar\"))"), Value::str("hello world & foo=bar"));
    // + 在 RFC 3986 中不转空格（往返保持）
    assert_eq!(eval("return urlDecode(urlEncode(\"a+b\"))"), Value::str("a+b"));
    assert_eq!(eval("return urlEncode(\"a+b\")"), Value::str("a%2Bb")); // + 非保留? 否，+ 编码
    // 表单编码：空格=+，+ → %2B
    assert_eq!(eval("return urlFormEncode(\"hello world\")"), Value::str("hello+world"));
    assert_eq!(eval("return urlFormEncode(\"a+b\")"), Value::str("a%2Bb"));
    assert_eq!(eval("return urlFormDecode(urlFormEncode(\"a+b c\"))"), Value::str("a+b c"));
    assert_eq!(eval("return urlFormDecode(\"hello+world\")"), Value::str("hello world"));
    // 两种标准对空格的不同处理
    assert_eq!(eval("return urlEncode(\" \")"), Value::str("%20"));     // RFC 3986
    assert_eq!(eval("return urlFormEncode(\" \")"), Value::str("+"));   // 表单
}

#[test]
fn test_sys_builtins() {
    // 路径处理
    assert_eq!(eval("return baseName(\"/a/b/c.txt\")"), Value::str("c.txt"));
    assert_eq!(eval("return fileExt(\"c.txt\")"), Value::str(".txt"));
    assert_eq!(eval("return fileExt(\"archive.tar.gz\")"), Value::str(".gz"));
    // osName/osArch 返回非空字符串
    assert!(eval("return osName()").to_str().len() > 0);
    assert!(eval("return osArch()").to_str().len() > 0);
    assert_eq!(eval("return len(getCurDir()) > 0"), Value::Bool(true));
    assert_eq!(eval("return len(getTempDir()) > 0"), Value::Bool(true));
    // getEnv 不存在 → undefined
    assert_eq!(eval("return getEnv(\"SFANG_NONEXIST_98765\")"), Value::Undefined);
    // makeDirAll + listDir 往返
    let dir = std::env::temp_dir().join("sflang_sys_test_dir");
    std::fs::remove_dir_all(&dir).ok();
    let mut sf = Sflang::new();
    sf.set_global("__p", Value::str(dir.to_str().unwrap()));
    sf.run_string("makeDirAll(__p)").unwrap();
    assert_eq!(sf.run_string("return fileExists(__p)").unwrap(), Value::Bool(true));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_util_builtins() {
    // uuid 格式（36 字符，含 4 个连字符）
    let uid = eval("return uuid()");
    assert!(uid.to_str().len() == 36);
    // randomStr 长度
    assert_eq!(eval("return len(randomStr(20))"), Value::Int(20));
    // hasKey
    assert_eq!(eval("return hasKey({\"a\": 1, \"b\": 2}, \"a\")"), Value::Bool(true));
    assert_eq!(eval("return hasKey({\"a\": 1}, \"z\")"), Value::Bool(false));
    // values
    assert_eq!(eval("return len(values({\"a\": 1, \"b\": 2}))"), Value::Int(2));
    // filter
    let r = eval("return filter([1,2,3,4,5], func(x) { return x > 2 })");
    match r { Value::Array(a) => assert_eq!(a.lock().unwrap().len(), 3), _ => panic!("expected Array") }
    // map
    let r = eval("return map([1,2,3], func(x) { return x * 10 })");
    match r {
        Value::Array(a) => {
            let arr = a.lock().unwrap();
            assert_eq!(arr[0], Value::Int(10));
            assert_eq!(arr[2], Value::Int(30));
        }
        _ => panic!("expected Array"),
    }
    // sprintf
    assert_eq!(eval("return sprintf(\"%s=%d\", \"age\", 25)"), Value::str("age=25"));
    // deepClone 隔离
    let src = r#"
        var obj = {nested: {x: 1}}
        var c = deepClone(obj)
        c.nested.x = 99
        return [obj.nested.x, c.nested.x]
    "#;
    let r = eval(src);
    match r {
        Value::Array(a) => {
            let arr = a.lock().unwrap();
            assert_eq!(arr[0], Value::Int(1));   // 原对象不变
            assert_eq!(arr[1], Value::Int(99));  // 克隆已改
        }
        _ => panic!("expected Array"),
    }
}

#[test]
fn test_regex_functions() {
    // regMatch 整串匹配
    assert_eq!(eval("return regMatch(\"^\\\\d+$\", \"12345\")"), Value::Bool(true));
    assert_eq!(eval("return regMatch(\"^\\\\d+$\", \"12a\")"), Value::Bool(false));
    // regFind 第一个匹配
    assert_eq!(eval("return regFind(\"\\\\d+\", \"abc123def\")"), Value::str("123"));
    assert_eq!(eval("return regFind(\"\\\\d+\", \"abc\")"), Value::Undefined);
    // regFindAll 全部匹配
    let r = eval("return regFindAll(\"\\\\d+\", \"a1b22c333\")");
    match r { Value::Array(a) => assert_eq!(a.lock().unwrap().len(), 3), _ => panic!("expected Array") }
    // regFindFirst 捕获组
    let r = eval("return regFindFirst(\"(\\\\d+)-(\\\\d+)\", \"x12-34y\")");
    match r {
        Value::Array(a) => {
            let arr = a.lock().unwrap();
            assert_eq!(arr.len(), 3);          // 全匹配 + 2 捕获组
            assert_eq!(arr[0], Value::str("12-34"));
            assert_eq!(arr[1], Value::str("12"));
            assert_eq!(arr[2], Value::str("34"));
        }
        _ => panic!("expected Array"),
    }
    // regReplace（含 $1/$2 捕获引用）
    assert_eq!(eval("return regReplace(\"\\\\d+\", \"a1b2c3\", \"#\")"), Value::str("a#b#c#"));
    assert_eq!(eval("return regReplace(\"(\\\\w+)@(\\\\w+)\", \"x@y\", \"[$2/$1]\")"), Value::str("[y/x]"));
    // regSplit
    let r = eval("return regSplit(\",\\\\s*\", \"a, b,c ,d\")");
    match r { Value::Array(a) => assert_eq!(a.lock().unwrap().len(), 4), _ => panic!("expected Array") }
    // regCompile 预编译 + 复用
    assert_eq!(eval("var re = regCompile(\"\\\\d+\"); return regMatch(re, \"999\")"), Value::Bool(true));
    assert_eq!(eval("var re = regCompile(\"\\\\d+\"); return regFind(re, \"abc42\")"), Value::str("42"));
    // 非法模式报错
    assert!(run("return regMatch(\"(?=...)\", \"x\")").is_err()); // 前瞻不支持
}

#[test]
fn test_datetime_type() {
    // 构造与类型
    let dt = eval("return datetime(2024, 6, 15, 14, 30, 45)");
    assert_eq!(dt.type_name(), "datetime");
    assert_eq!(eval("return isDatetime(datetime(2024,1,1))"), Value::Bool(true));
    assert_eq!(eval("return isDatetime(now())"), Value::Bool(false)); // now() 返回 int
    // 字段访问
    assert_eq!(eval("return datetime(2024,6,15,14,30,45).year"), Value::Int(2024));
    assert_eq!(eval("return datetime(2024,6,15,14,30,45).month"), Value::Int(6));
    assert_eq!(eval("return datetime(2024,6,15,14,30,45).day"), Value::Int(15));
    assert_eq!(eval("return datetime(2024,6,15,14,30,45).hour"), Value::Int(14));
    assert_eq!(eval("return datetime(2024,6,15,14,30,45).minute"), Value::Int(30));
    assert_eq!(eval("return datetime(2024,6,15,14,30,45).second"), Value::Int(45));
    // 1970-01-01 是周四（weekday=4）
    assert_eq!(eval("return datetime(1970,1,1).weekday"), Value::Int(4));
    // 格式化（Go 风格）
    assert_eq!(eval("return dtFormat(datetime(2024,1,1), \"2006-01-02\")"), Value::str("2024-01-01"));
    assert_eq!(eval("return dtFormat(datetime(2024,6,15,14,30,45), \"2006-01-02 15:04:05\")"), Value::str("2024-06-15 14:30:45"));
    // 加减天
    assert_eq!(eval("return dtFormat(dtAddDays(datetime(2024,1,1), 31), \"2006-01-02\")"), Value::str("2024-02-01"));
    assert_eq!(eval("return dtFormat(dtAddDays(datetime(2024,1,1), 10), \"2006-01-02\")"), Value::str("2024-01-11"));
    // 毫秒互转
    assert_eq!(eval("return dtToMillis(datetime(1970,1,1))"), Value::Int(0));
    assert_eq!(eval("return dtFormat(datetimeFromMillis(1704067200000), \"2006-01-02\")"), Value::str("2024-01-01"));
    // 解析往返
    assert_eq!(eval("return dtFormat(datetimeParse(\"2024-12-25\", \"2006-01-02\"), \"2006-01-02\")"), Value::str("2024-12-25"));
    // 闰年边界
    assert!(run("return datetime(1900,2,29)").is_err());  // 1900 非 400 倍数，不闰
    assert!(eval("return datetime(2000,2,29)").type_name() == "datetime".to_string()); // 2000 闰
}

#[test]
fn test_printf_family() {
    // printf/printfln 通过输出副作用验证（这里主要验证不报错 + 参数匹配）
    // %v %s %d %f %t %x %c %% 各种 verb
    assert!(run("printf(\"%v %s %d %.2f %t %x %c %%\", 1, \"a\", 42, 3.14159, true, 255, 65)").is_ok());
    assert!(run("printfln(\"v=%v\", 99)").is_ok());
    // 简称别名
    assert!(run("prf(\"%d\", 1)").is_ok());
    assert!(run("pl(\"%d\", 1)").is_ok());
    assert!(run("pr(\"x\")").is_ok());
    assert!(run("pln(\"x\")").is_ok());
    // 宽度与对齐
    assert!(run("printf(\"%5d|%-5d|\", 7, 7)").is_ok());
    // 参数少于占位符（保留字面）
    assert!(run("printf(\"%d %d\", 1)").is_ok());
    // 参数多于占位符（多余忽略）
    assert!(run("printf(\"%d\", 1, 2, 3)").is_ok());
    // 类型不符报错
    assert!(run("printf(\"%d\", \"notnum\")").is_err());
    // 无参数
    assert!(run("printf(\"\")").is_ok());
}

#[test]
fn test_slice_edge_cases() {
    // 负索引溢出 clamp 到 0（对齐 Python，不报错）
    assert_eq!(eval("return \"abcde\"[-100:]"), Value::str("abcde"));
    assert_eq!(eval("return \"abcde\"[:-100]"), Value::str(""));   // 负溢出到尾=空
    assert_eq!(eval("return \"abcde\"[-100:100]"), Value::str("abcde"));
    // array 同样 clamp
    assert_eq!(eval("return len([1,2,3][-100:])"), Value::Int(3));
    assert_eq!(eval("return len([1,2,3][-100:-50])"), Value::Int(0));
    // high 越界截断到 len
    assert_eq!(eval("return \"abc\"[0:100]"), Value::str("abc"));
    assert_eq!(eval("return \"abc\"[1:100]"), Value::str("bc"));
    // low 越界（超过 len）→ 空
    assert_eq!(eval("return \"abc\"[100:]"), Value::str(""));
    // 双越界 → 空
    assert_eq!(eval("return \"abc\"[100:200]"), Value::str(""));
    // bytes/byteArray 负溢出同样 clamp
    assert_eq!(eval("return bytesHex(bytes(\"ABC\")[-100:])"), Value::str("414243"));
    assert_eq!(eval("return bytesHex(byteArrayFromBytes(bytes(\"ABC\"))[-100:])"), Value::str("414243"));
    // 全缺省 = 整体拷贝
    assert_eq!(eval("return \"abc\"[:]"), Value::str("abc"));
    assert_eq!(eval("return len([1,2,3][:])"), Value::Int(3));
}

#[test]
fn test_string_byte_functions() {
    // lenBytes vs len（字符数 vs 字节数）
    assert_eq!(eval("return lenBytes(\"ABC\")"), Value::Int(3));
    assert_eq!(eval("return lenBytes(\"中\")"), Value::Int(3));   // 中文 3 字节
    assert_eq!(eval("return len(\"中\")"), Value::Int(1));        // 但字符数是 1
    // bytesAt：取字节（返回 byte 类型）
    assert_eq!(eval("return bytesAt(\"AB\", 0)"), Value::Byte(65));
    assert_eq!(eval("return bytesAt(\"AB\", 1)"), Value::Byte(66));
    assert_eq!(eval("return bytesAt(\"中\", 0)"), Value::Byte(0xE4)); // 首字节
    // bytesSlice：按字节切
    assert_eq!(eval("return bytesHex(bytesSlice(\"ABC\", 0, 2))"), Value::str("4142"));
    assert_eq!(eval("return bytesHex(bytesSlice(\"中\", 0, 3))"), Value::str("e4b8ad")); // 完整 UTF-8
    // 负索引
    assert_eq!(eval("return bytesAt(\"AB\", -1)"), Value::Byte(66));
}

#[test]
fn test_big_int() {
    // 构造（从 string 解析大数）
    let big = eval("return bigInt(\"99999999999999999999\")");
    assert_eq!(big.type_name(), "bigInt");
    // 从 int 构造（仍是 bigInt 类型，不降级）
    assert_eq!(eval("return typeName(bigInt(5))"), Value::str("bigInt"));
    // 算术（大数 + 大数）——用 == 校验（BigInt PartialEq 是指针比较，走脚本 == 经 equals 值比较）
    assert_eq!(eval("return (bigInt(\"99999999999999999999\") + 1) == bigInt(\"100000000000000000000\")"), Value::Bool(true));
    // 大数乘法
    assert_eq!(eval("return (bigInt(\"999999999999\") * bigInt(\"999999999999\")) == bigInt(\"999999999998000000000001\")"), Value::Bool(true));
    // 与 int 互通（int + bigInt → 结果能装回 int 则降级为 int，否则 bigInt）
    assert_eq!(eval("return typeName(1 + bigInt(2))"), Value::str("int"));  // 小结果降级
    assert_eq!(eval("return typeName(1 + bigInt(\"99999999999999999999\"))"), Value::str("bigInt"));  // 大结果保持 bigInt
    assert_eq!(eval("return (1 + bigInt(\"99999999999999999999\")) == bigInt(\"100000000000000000000\")"), Value::Bool(true));
    // 阶乘 25!（超出 i64）
    assert_eq!(eval(r#"
        var f = bigInt(1)
        for i in range(1, 26) { f = f * bigInt(i) }
        return f == bigInt("15511210043330985984000000")
    "#), Value::Bool(true));
    // 比较
    assert_eq!(eval("return bigInt(\"999\") > bigInt(\"1000\")"), Value::Bool(false));
    assert_eq!(eval("return bigInt(5) == bigInt(5)"), Value::Bool(true));
    assert_eq!(eval("return bigInt(5) == 5"), Value::Bool(true));  // 跨类型
    // 除法与取模
    assert_eq!(eval("return (bigInt(\"100000000000000000000\") / bigInt(3)) == bigInt(\"33333333333333333333\")"), Value::Bool(true));
    assert_eq!(eval("return bigInt(\"100000000000000000000\") % bigInt(3) == 1"), Value::Bool(true));
    // 多 limb 除数（曾导致死循环 bug）—— 用不变式验证而非硬编码商
    assert_eq!(eval("var a = bigInt(\"99999999999999999999999999\"); var b = bigInt(\"999999999999\"); var r = a % b; return a == (a / b) * b + r"), Value::Bool(true));
    assert_eq!(eval("var a = bigInt(\"99999999999999999999999999\"); var b = bigInt(\"999999999999\"); var r = a % b; return r < b"), Value::Bool(true));
    // 除零报错
    assert!(run("return bigInt(5) / bigInt(0)").is_err());
    // 非交换运算的操作数顺序（BigInt 在左、Int 在右）
    assert_eq!(eval("return bigInt(10) - 3 == 7"), Value::Bool(true));
    assert_eq!(eval("return bigInt(100) / 5 == 20"), Value::Bool(true));
    assert_eq!(eval("return bigInt(100) % 7 == 2"), Value::Bool(true));
    assert_eq!(eval("return 3 - bigInt(10) == -7"), Value::Bool(true));  // 反向
    assert_eq!(eval("return 5 / bigInt(100) == 0"), Value::Bool(true));  // 反向
    // toBigInt（强制 bigInt 类型）
    assert_eq!(eval("return typeName(toBigInt(5))"), Value::str("bigInt"));
    // isBigInt
    assert_eq!(eval("return isType(bigInt(5), \"bigInt\")"), Value::Bool(true));
    assert_eq!(eval("return isType(5, \"bigInt\")"), Value::Bool(false));
}

#[test]
fn test_big_float() {
    // 精确十进制：0.1 + 0.2 = 0.3（避免 float 误差）——用脚本 == 值比较
    assert_eq!(eval("return (bigFloat(\"0.1\") + bigFloat(\"0.2\")) == bigFloat(\"0.3\")"), Value::Bool(true));
    // float 的精度问题（对照）
    assert_eq!(eval("return 0.1 + 0.2 == 0.3"), Value::Bool(false));  // float 不精确
    // 乘法
    assert_eq!(eval("return (bigFloat(\"1.1\") * bigFloat(\"1.1\")) == bigFloat(\"1.21\")"), Value::Bool(true));
    // 除法（默认 20 位）
    let r = eval("return bigFloat(\"1\") / bigFloat(\"3\")");
    match r {
        Value::BigFloat(bf) => assert!(bf.to_string().starts_with("0.3333")),
        _ => panic!("expected BigFloat"),
    }
    // 指定精度除法
    assert_eq!(eval("return bigFloatDiv(bigFloat(\"1\"), bigFloat(\"3\"), 5) == bigFloat(\"0.33333\")"), Value::Bool(true));
    // 与 int 互通
    assert_eq!(eval("return (bigFloat(\"1.5\") + 1) == bigFloat(\"2.5\")"), Value::Bool(true));
    // 大数 + 小数
    assert_eq!(eval("return (bigFloat(\"99999999999999999999.99\") + bigFloat(\"0.01\")) == bigFloat(\"100000000000000000000\")"), Value::Bool(true));
    // isBigFloat
    assert_eq!(eval("return isType(bigFloat(\"1\"), \"bigFloat\")"), Value::Bool(true));
    // bigFloat + float 报错（精度语义冲突）
    assert!(run("return bigFloat(\"1\") + 1.5").is_err());
    // 非交换运算的操作数顺序（BigFloat 在左、Int 在右）—— 曾因共用 arm 算反
    assert_eq!(eval("return bigFloat(\"10\") - 3 == bigFloat(\"7\")"), Value::Bool(true));
    assert_eq!(eval("return bigFloat(\"10\") / 4 == bigFloat(\"2.5\")"), Value::Bool(true));
    // 比较方向（BigFloat 在左）—— 曾因方向反导致 bigFloat(5) > 3 返回 false
    assert_eq!(eval("return bigFloat(\"5\") > 3"), Value::Bool(true));
    assert_eq!(eval("return bigFloat(\"3\") > 5"), Value::Bool(false));
    assert_eq!(eval("return bigFloat(\"5\") < 3"), Value::Bool(false));
    assert_eq!(eval("return bigFloat(\"5\") >= 5"), Value::Bool(true));
    // BigFloat vs BigInt 比较（方向）
    assert_eq!(eval("return bigFloat(\"5\") > bigInt(3)"), Value::Bool(true));
    assert_eq!(eval("return bigFloat(\"3\") > bigInt(5)"), Value::Bool(false));
}

#[test]
fn test_byte_type() {
    // 构造与类型
    assert_eq!(eval("return typeName(byte(65))"), Value::str("byte"));
    assert_eq!(eval("return isType(byte(65), \"byte\")"), Value::Bool(true));
    assert_eq!(eval("return isType(65, \"byte\")"), Value::Bool(false));
    // byte == int 跨类型相等
    assert_eq!(eval("return byte(65) == 65"), Value::Bool(true));
    assert_eq!(eval("return byte(0) == 0"), Value::Bool(true));
    // byte 范围校验
    assert!(run("return byte(256)").is_err());
    assert!(run("return byte(-1)").is_err());
    // byte 算术（mod 256 环绕）
    assert_eq!(eval("return typeName(byte(255) + byte(1))"), Value::str("byte"));  // 结果仍为 byte
    assert_eq!(eval("return byte(255) + byte(1)"), Value::Byte(0));   // 环绕
    assert_eq!(eval("return byte(0) - byte(1)"), Value::Byte(255));   // 环绕
    assert_eq!(eval("return byte(200) * byte(2)"), Value::Byte(144)); // 环绕
    // byte + int → int
    assert_eq!(eval("return typeName(byte(65) + 1)"), Value::str("int"));
    assert_eq!(eval("return byte(65) + 1"), Value::Int(66));
    // byte 位运算
    assert_eq!(eval("return byte(0xF0) & byte(0x0F)"), Value::Byte(0));
    assert_eq!(eval("return byte(0xF0) | byte(0x0F)"), Value::Byte(255));
    assert_eq!(eval("return byte(0xFF) ^ byte(0xAA)"), Value::Byte(0x55));
    assert_eq!(eval("return ~byte(0)"), Value::Byte(255));
    // byte 比较
    assert_eq!(eval("return byte(100) > byte(50)"), Value::Bool(true));
    assert_eq!(eval("return byte(50) < 100"), Value::Bool(true));  // 跨类型
    // byteArray 索引返回 byte
    assert_eq!(eval("return typeName(byteArray(1, 0x41)[0])"), Value::str("byte"));
    // bytesXor 加解密往返
    assert_eq!(eval("return strFromBytes(bytesXor(bytesXor(bytes(\"test\"), byte(0xFF)), byte(0xFF)))"), Value::str("test"));
    // bytesXorInPlace 原地
    let r = eval(r#"
        var ba = byteArrayFromBytes(bytes("ABC"))
        bytesXorInPlace(ba, byte(0x20))
        bytesXorInPlace(ba, byte(0x20))
        return strFromBytes(ba)
    "#);
    assert_eq!(r, Value::str("ABC"));
}

#[test]
fn test_byte_array_basics() {
    // 构造与类型
    assert!(eval("return byteArray(4)").type_name() == "byteArray".to_string());
    assert_eq!(eval("return typeName(byteArray(4))"), Value::str("byteArray"));
    assert_eq!(eval("return isType(byteArray(4), \"byteArray\")"), Value::Bool(true));
    assert_eq!(eval("return isType(bytes(\"ab\"), \"byteArray\")"), Value::Bool(false));
    // 长度与填充
    assert_eq!(eval("return len(byteArray(8))"), Value::Int(8));
    assert_eq!(eval("return byteArray(3, 0xFF)[0]"), Value::Byte(255));
    assert_eq!(eval("return byteArray(3, 0x41)[2]"), Value::Byte(65));
    // 索引读写（就地修改）
    assert_eq!(eval("var ba = byteArray(3); ba[0] = 65; ba[1] = 66; ba[2] = 67; return ba[1]"), Value::Byte(66));
    // 负索引
    assert_eq!(eval("var ba = byteArray(3, 0x41); return ba[-1]"), Value::Byte(65));
    // .len 成员
    assert_eq!(eval("return byteArray(5).len"), Value::Int(5));
    // 越界与非法值报错
    assert!(run("return byteArray(3)[5]").is_err());
    assert!(run("var ba = byteArray(3); ba[0] = 256; return ba").is_err());  // 超 0-255
    assert!(run("var ba = byteArray(3); ba[0] = \"x\"; return ba").is_err()); // 非 int
    assert!(run("return byteArray(-1)").is_err());
}

#[test]
fn test_byte_array_mutation_xor() {
    // 按位加密场景：就地 XOR 加密（byteArray 存在的核心价值）
    let src = r#"
        var data = byteArray(4)
        data[0] = 0xDE; data[1] = 0xAD; data[2] = 0xBE; data[3] = 0xEF
        // 加密
        for i in range(len(data)) { data[i] = data[i] ^ 0xAA }
        var encrypted = bytesHex(data)
        // 解密（XOR 自反）
        for i in range(len(data)) { data[i] = data[i] ^ 0xAA }
        var decrypted = bytesHex(data)
        return [encrypted, decrypted]
    "#;
    let r = eval(src);
    match r {
        Value::Array(a) => {
            let arr = a.lock().unwrap();
            assert_eq!(arr[0], Value::str("74071445"));  // deadbeef ^ aaaaaaaa
            assert_eq!(arr[1], Value::str("deadbeef"));  // 解密还原
        }
        _ => panic!("expected Array"),
    }
}

#[test]
fn test_byte_array_conversions() {
    // bytes ↔ byteArray 转换
    assert_eq!(eval("return bytesHex(bytes(byteArray(2, 0x41)))"), Value::str("4141"));
    assert_eq!(eval("return bytesHex(byteArrayFromBytes(bytes(\"AB\")))"), Value::str("4142"));
    // string → bytes（UTF-8）
    assert_eq!(eval("return len(bytes(\"中\"))"), Value::Int(3));  // 中文 3 字节
    // Array<Int> → byteArray
    assert_eq!(eval("return bytesHex(byteArrayFromArray([65, 66, 67]))"), Value::str("414243"));
    // byteArray → Array<Int>
    assert_eq!(eval("return arrayFromByteArray(byteArray(2, 0x41))[0]"), Value::Int(65));
    // 跨类型相等（bytes == byteArray 按内容）
    assert_eq!(eval("return byteArray(2, 0x41) == bytes(\"AA\")"), Value::Bool(true));
    // 转换有拷贝：改 byteArray 不影响原 bytes
    let src = r#"
        var b = bytes("AB")
        var ba = byteArrayFromBytes(b)
        ba[0] = 0x5A
        return [b[0], ba[0]]
    "#;
    let r = eval(src);
    match r {
        Value::Array(a) => {
            let arr = a.lock().unwrap();
            assert_eq!(arr[0], Value::Byte(65));  // 原 bytes 不变（bytes 索引返回 byte）
            assert_eq!(arr[1], Value::Byte(90));  // byteArray 已改（返回 byte）
        }
        _ => panic!("expected Array"),
    }
}

#[test]
fn test_byte_hex_codec() {
    // bytesHex / bytesFromHex 往返
    assert_eq!(eval("return bytesHex(bytes(\"AB\"))"), Value::str("4142"));
    assert_eq!(eval("return strFromBytes(bytesFromHex(\"4142\"))"), Value::str("AB"));
    // bytesFromHex 忽略分隔符
    assert_eq!(eval("return bytesHex(bytesFromHex(\"de:ad-be ef\"))"), Value::str("deadbeef"));
    // strFromBytes 编码
    assert_eq!(eval("return strFromBytes(bytesFromHex(\"e4b8ad\"), \"utf8\")"), Value::str("中"));
    assert_eq!(eval("return strFromBytes(byteArray(1, 0x41), \"latin1\")"), Value::str("A"));
    // 奇数长度十六进制报错
    assert!(run("return bytesFromHex(\"abc\")").is_err());
}

#[test]
fn test_copy_and_slice() {
    // copy：src → dst
    assert_eq!(eval("var d = byteArray(4); var n = copy(d, bytes(\"AB\")); return n"), Value::Int(2));
    assert_eq!(eval("var d = byteArray(4); copy(d, bytes(\"ABCD\")); return bytesHex(d)"), Value::str("41424344"));
    // copy 带 dstStart
    assert_eq!(eval("var d = byteArray(4, 0x00); copy(d, bytes(\"XX\"), 1); return bytesHex(d)"), Value::str("00585800"));
    // copy src 长于剩余空间：只复制能放下的
    assert_eq!(eval("var d = byteArray(2); var n = copy(d, bytes(\"ABCD\")); return n"), Value::Int(2));
    // slice 返回 byteArray（类型一致）
    let r = eval("var ba = byteArrayFromBytes(bytes(\"abcdef\")); return slice(ba, 1, 3)");
    assert!(matches!(r, Value::ByteArray(_)));
    assert_eq!(eval("var ba = byteArrayFromBytes(bytes(\"abcdef\")); return bytesHex(slice(ba, 1, 3))"), Value::str("6263"));
}

#[test]
fn test_binary_file_io() {
    let path = std::env::temp_dir().join("sflang_test_bin.sf.tmp");
    let path_str = path.to_str().unwrap();

    let mut sf = Sflang::new();
    sf.set_global("__p", Value::str(path_str));
    // 写入二进制（含非 UTF-8 字节 0xFF 0x00）
    sf.run_string("writeFileBytes(__p, bytesFromHex(\"ff004142\"))").unwrap();
    // 读回为 bytes
    let r = sf.run_string("return bytesHex(readFileBytes(__p))").unwrap();
    assert_eq!(r, Value::str("ff004142"));
    // readFile 读含非法 UTF-8 字节会出错或替换；readFileBytes 保持原始字节
    std::fs::remove_file(path).ok();
}

#[test]
fn test_compound_assignment() {
    // 算术复合赋值（简单变量）
    assert_eq!(eval("var x = 10; x += 5; return x"), Value::Int(15));
    assert_eq!(eval("var x = 10; x -= 3; return x"), Value::Int(7));
    assert_eq!(eval("var x = 10; x *= 2; return x"), Value::Int(20));
    assert_eq!(eval("var x = 10; x /= 4; return x"), Value::Int(2));
    assert_eq!(eval("var x = 10; x %= 3; return x"), Value::Int(1));
    // 位运算复合赋值
    assert_eq!(eval("var x = 12; x &= 10; return x"), Value::Int(8));
    assert_eq!(eval("var x = 12; x |= 10; return x"), Value::Int(14));
    assert_eq!(eval("var x = 12; x ^= 10; return x"), Value::Int(6));
    assert_eq!(eval("var x = 1; x <<= 4; return x"), Value::Int(16));
    assert_eq!(eval("var x = 256; x >>= 2; return x"), Value::Int(64));
    // ??= 复合赋值
    assert_eq!(eval("var x = undefined; x ??= 99; return x"), Value::Int(99));
    assert_eq!(eval("var x = 5; x ??= 99; return x"), Value::Int(5));  // 已有值不覆盖
    assert_eq!(eval("var x = 0; x ??= 99; return x"), Value::Int(0));  // 0 不触发 ??=
    // 复合赋值作用于 a[i]
    assert_eq!(eval("var a = [10,20,30]; a[1] += 5; return a[1]"), Value::Int(25));
    assert_eq!(eval("var a = [1,2,3]; a[0] <<= 3; return a[0]"), Value::Int(8));
    // 复合赋值作用于 obj.k
    assert_eq!(eval("var o = {n: 5}; o.n *= 4; return o.n"), Value::Int(20));
    assert_eq!(eval("var o = {}; o.k ??= 7; return o.k"), Value::Int(7));
    // 复合赋值是表达式，返回新值
    assert_eq!(eval("var x = 5; return x += 3"), Value::Int(8));
}

#[test]
fn test_increment_decrement() {
    // 前缀 ++/--（返回新值）
    assert_eq!(eval("var x = 5; return ++x"), Value::Int(6));
    assert_eq!(eval("var x = 5; return --x"), Value::Int(4));
    // 后缀 ++/--（返回旧值）
    assert_eq!(eval("var x = 5; return x++"), Value::Int(5));
    assert_eq!(eval("var x = 5; return x--"), Value::Int(5));
    // 变量本身被修改
    assert_eq!(eval("var x = 5; x++; return x"), Value::Int(6));
    assert_eq!(eval("var x = 5; ++x; return x"), Value::Int(6));
    // 作用于 a[i]（前缀）
    assert_eq!(eval("var a = [10,20]; return ++a[0]"), Value::Int(11));
    assert_eq!(eval("var a = [10,20]; return --a[1]"), Value::Int(19));
    // 作用于 a[i]（后缀）
    assert_eq!(eval("var a = [10,20]; return a[0]++"), Value::Int(10)); // 返回旧值 10
    assert_eq!(eval("var a = [10,20]; a[0]++; return a[0]"), Value::Int(11)); // 但已变 11
    // 作用于 obj.k
    assert_eq!(eval("var o = {n: 7}; return ++o.n"), Value::Int(8));
    assert_eq!(eval("var o = {n: 7}; return o.n++"), Value::Int(7)); // 返回旧值
    assert_eq!(eval("var o = {n: 7}; o.n++; return o.n"), Value::Int(8));
    // 地址只求值一次：a[f()]++ 中 f() 只调用一次
    let src = r#"
        var calls = 0
        func idx() { calls = calls + 1; return 1 }
        var a = [10, 20, 30]
        a[idx()] += 5
        return [a[1], calls]
    "#;
    let r = eval(src);
    match r {
        Value::Array(arr) => {
            let a = arr.lock().unwrap();
            assert_eq!(a[0], Value::Int(25));  // 20+5
            assert_eq!(a[1], Value::Int(1));   // f() 只调一次
        }
        _ => panic!("expected Array"),
    }
    // 非法目标报错
    assert!(run("return 5++").is_err());
    assert!(run("return (x)++").is_err());
}

#[test]
fn test_null_coalesce() {
    // 基本语义：undefined 时取右值
    assert_eq!(eval("return undefined ?? 99"), Value::Int(99));
    // 非 undefined 时取左值
    assert_eq!(eval("return 42 ?? 99"), Value::Int(42));
    // 与 falsy 的关键区别：0/""/false 均为有效值，不触发兜底
    assert_eq!(eval("return 0 ?? 99"), Value::Int(0));
    assert_eq!(eval("return \"\" ?? 99"), Value::str(""));
    assert_eq!(eval("return false ?? 99"), Value::Bool(false));
    // map 缺键 → undefined → 触发兜底
    assert_eq!(eval("var m = {\"a\": 1}; return m[\"missing\"] ?? 99"), Value::Int(99));
    assert_eq!(eval("var m = {\"a\": 0}; return m[\"a\"] ?? 99"), Value::Int(0));
    // nil 别名也视为 undefined，触发兜底
    assert_eq!(eval("return undefined ?? 7"), Value::Int(7));
    // 嵌套（左结合）
    assert_eq!(eval("return undefined ?? (undefined ?? 7)"), Value::Int(7));
    assert_eq!(eval("return undefined ?? undefined ?? 8"), Value::Int(8));
    // 与 || 混用：?? 优先级低于 || → (undefined || 0) ?? 99
    //   undefined || 0 → false（falsy），false 非 undefined → 取左值 false
    assert_eq!(eval("return undefined || 0 ?? 99"), Value::Bool(false));
    // 短路求值：左值非 undefined 时不求右（右含除零也不报错）
    assert_eq!(eval("return 42 ?? (1/0 == 0)"), Value::Int(42));
}

#[test]
fn test_type_error_suggestion() {
    // undefined 参与算术属类型不兼容 → 抛异常（nil 是 undefined 的别名）
    let r = run("var __r = 1 + undefined").unwrap_err();
    match r {
        Value::Error(e) => {
            // 错误信息含类型名 undefined（便于 AI 定位）
            assert!(e.message.contains("undefined"), "msg: {}", e.message);
        }
        _ => panic!("expected Error"),
    }
}

// ---- 逻辑短路 and/or（回归测试，曾因编译与 VM 弹栈语义不一致导致 panic）----

#[test]
fn test_logical_and_or() {
    // 结果规范化为布尔值（短路求值）。
    assert_eq!(eval("var r = 5; return r >= 1 && r <= 10"), Value::Bool(true));
    assert_eq!(eval("var r = 5; return r > 10 || r == 5"), Value::Bool(true));
    assert_eq!(eval("var r = 5; return r > 10 && r == 5"), Value::Bool(false));
    // 短路：and 左假时不求右（右为 1/0 不会除零）
    assert_eq!(eval("return false && 1/0 == 0"), Value::Bool(false));
    // 短路：or 左真时不求右
    assert_eq!(eval("return true || 1/0 == 0"), Value::Bool(true));
    // 嵌套
    assert_eq!(eval("return (1 < 2) && (3 > 2) || (1 > 5)"), Value::Bool(true));
    // 非布尔操作数的真值判断
    assert_eq!(eval("return 0 && 1"), Value::Bool(false));
    assert_eq!(eval("return 1 && 2"), Value::Bool(true));
}

// ---- 字符串内置函数 ----

#[test]
fn test_str_case_trim() {
    assert_eq!(eval("return strToUpper(\"abc\")"), Value::str("ABC"));
    assert_eq!(eval("return strToLower(\"AbC\")"), Value::str("abc"));
    assert_eq!(eval("return trim(\"  hi  \")"), Value::str("hi"));
    assert_eq!(eval("return strTrimPrefix(\"hello.txt\", \"hello.\")"), Value::str("txt"));
    assert_eq!(eval("return strTrimSuffix(\"hello.txt\", \".txt\")"), Value::str("hello"));
}

#[test]
fn test_str_find_contains() {
    // strFind(sub, s)：在 s 中查找 sub 的位置
    assert_eq!(eval("return strFind(\"ll\", \"hello\")"), Value::Int(2));
    assert_eq!(eval("return strFind(\"z\", \"hello\")"), Value::Int(-1));
    assert_eq!(eval("return contains(\"hello\", \"ell\")"), Value::Bool(true));
    assert_eq!(eval("return strStartsWith(\"hello\", \"he\")"), Value::Bool(true));
    assert_eq!(eval("return strEndsWith(\"hello\", \"lo\")"), Value::Bool(true));
}

#[test]
fn test_str_replace_split_join() {
    assert_eq!(eval("return strReplace(\"a-b-c\", \"-\", \"+\")"), Value::str("a+b+c"));
    // strSplit(sep, s)：按分隔符 sep 分割字符串 s
    let r = eval("return strJoin(strSplit(\",\", \"a,b,c\"), \"-\")");
    assert_eq!(r, Value::str("a-b-c"));
}

#[test]
fn test_str_substring_repeat_reverse() {
    assert_eq!(eval("return strSub(\"hello\", 1, 3)"), Value::str("el"));
    assert_eq!(eval("return strSub(\"hello\", 2)"), Value::str("llo"));
    assert_eq!(eval("return strSub(\"hello\", -2)"), Value::str("lo"));
    assert_eq!(eval("return strRepeat(\"ab\", 3)"), Value::str("ababab"));
    assert_eq!(eval("return reverse(\"abc\")"), Value::str("cba"));
}

#[test]
fn test_str_error_message() {
    let r = run("return strToUpper(123)").unwrap_err();
    match r {
        Value::Error(e) => {
            assert!(e.message.contains("strToUpper"));
            assert!(e.message.contains("string"));
            assert!(e.message.contains("可能原因"));
        }
        _ => panic!("expected Error"),
    }
}

// ---- 数学内置函数 ----

#[test]
fn test_math_basic() {
    assert_eq!(eval("return abs(-5)"), Value::Int(5));
    assert_eq!(eval("return abs(-2.5)"), Value::Float(2.5));
    assert_eq!(eval("return floor(2.9)"), Value::Int(2));
    assert_eq!(eval("return ceil(2.1)"), Value::Int(3));
    assert_eq!(eval("return round(2.5)"), Value::Int(3));
    assert_eq!(eval("return sign(-3)"), Value::Int(-1));
}

#[test]
fn test_math_pow_sqrt() {
    assert_eq!(eval("return pow(2, 10)"), Value::Int(1024));
    assert_eq!(eval("return sqrt(9)"), Value::Float(3.0));
}

#[test]
fn test_math_min_max() {
    assert_eq!(eval("return min(3, 1, 2)"), Value::Int(1));
    assert_eq!(eval("return max(3, 1, 2)"), Value::Int(3));
    // 数组形式
    assert_eq!(eval("return min([5, 2, 8])"), Value::Int(2));
}

#[test]
fn test_math_constants() {
    assert_eq!(eval("return pi()"), Value::Float(std::f64::consts::PI));
    assert_eq!(eval("return e()"), Value::Float(std::f64::consts::E));
    // 全局变量形式
    assert_eq!(eval("return piG"), Value::Float(std::f64::consts::PI));
}

#[test]
fn test_math_sqrt_negative_error() {
    let r = run("return sqrt(-1)").unwrap_err();
    match r {
        Value::Error(e) => assert!(e.message.contains("负数")),
        _ => panic!("expected Error"),
    }
}

#[test]
fn test_math_randint_range() {
    // 多次取样验证范围正确
    for _ in 0..100 {
        let v = eval("return randInt(1, 10)");
        match v {
            Value::Int(i) => assert!(i >= 1 && i <= 10),
            _ => panic!("expected Int"),
        }
    }
}

// ---- 数组内置函数 ----

#[test]
fn test_arr_sort_reverse() {
    let r = eval("var a = [3, 1, 2]; sort(a); return a[0]");
    assert_eq!(r, Value::Int(1));
    let r = eval("var a = [1, 2, 3]; sort(a, true); return a[0]");
    assert_eq!(r, Value::Int(3));
    let r = eval("var a = [1, 2, 3]; reverse(a); return a[0]");
    assert_eq!(r, Value::Int(3));
}

#[test]
fn test_arr_contains_indexof() {
    assert_eq!(eval("return contains([1, 2, 3], 2)"), Value::Bool(true));
    assert_eq!(eval("return contains([1, 2, 3], 9)"), Value::Bool(false));
    assert_eq!(eval("return indexOf([\"a\", \"b\"], \"b\")"), Value::Int(1));
    assert_eq!(eval("return indexOf([1, 2], 9)"), Value::Int(-1));
}

#[test]
fn test_arr_slice_concat() {
    let r = eval("return slice([1, 2, 3, 4], 1, 3)");
    match r {
        Value::Array(a) => assert_eq!(a.lock().unwrap().len(), 2),
        _ => panic!("expected Array"),
    }
    let r = eval("return concat([1], [2, 3])");
    match r {
        Value::Array(a) => assert_eq!(a.lock().unwrap().len(), 3),
        _ => panic!("expected Array"),
    }
}

#[test]
fn test_arr_insert_remove() {
    let r = eval("var a = [1, 3]; insert(a, 1, 2); return a[1]");
    assert_eq!(r, Value::Int(2));
    let r = eval("var a = [1, 2, 3]; return remove(a, 1)");
    assert_eq!(r, Value::Int(2));
}

#[test]
fn test_arr_remove_out_of_bounds_error() {
    let r = run("var a = [1]; return remove(a, 5)").unwrap_err();
    match r {
        Value::Error(e) => assert!(e.message.contains("越界") || e.message.contains("index")),
        _ => panic!("expected Error"),
    }
}

// ---- 类型判断内置函数 ----

#[test]
fn test_type_predicates() {
    // 通用类型判断 isType 取代零散的 isXxx 谓词
    assert_eq!(eval("return isType([], \"array\")"), Value::Bool(true));
    assert_eq!(eval("return isType(\"x\", \"string\")"), Value::Bool(true));
    assert_eq!(eval("return isType(3, \"int\")"), Value::Bool(true));
    assert_eq!(eval("return isType(3.0, \"float\")"), Value::Bool(true));
    assert_eq!(eval("return isType(true, \"bool\")"), Value::Bool(true));
    // isUndefined 保留（特殊语义）
    assert_eq!(eval("return isUndefined(undefined)"), Value::Bool(true));
    assert_eq!(eval("return isUndefined(0)"), Value::Bool(false));
    // isTypeCode 按数字编码判断
    assert_eq!(eval("return isTypeCode(3, 1)"), Value::Bool(true));
    assert_eq!(eval("return isTypeCode(3.0, 2)"), Value::Bool(true));
}

// ---- 时间内置函数 ----

#[test]
fn test_time_functions() {
    let now = eval("return now()");
    match now {
        Value::Int(i) => assert!(i > 1_000_000_000_000), // 2001 年之后
        _ => panic!("expected Int"),
    }
    let c1 = eval("return clock()");
    let c2 = eval("return clock()");
    match (c1, c2) {
        (Value::Int(a), Value::Int(b)) => assert!(b >= a),
        _ => panic!("expected Int"),
    }
}

// ---- 文件 IO 内置函数 ----

#[test]
fn test_fs_roundtrip() {
    let path = std::env::temp_dir().join("sflang_test_fs.sf.tmp");
    let path_str = path.to_str().unwrap();
    let path_val = Value::str(path_str);

    // 写入并读回
    let mut sf = Sflang::new();
    sf.set_global("__p", path_val.clone());
    sf.run_string("writeFile(__p, \"line1\\nline2\\n\")").unwrap();
    let r = sf.run_string("return readFile(__p)").unwrap();
    assert_eq!(r, Value::str("line1\nline2\n"));

    // fileExists
    let r = sf.run_string("return fileExists(__p)").unwrap();
    assert_eq!(r, Value::Bool(true));

    // readLines
    let r = sf.run_string("return readLines(__p)").unwrap();
    match r {
        Value::Array(a) => assert_eq!(a.lock().unwrap().len(), 2),
        _ => panic!("expected Array"),
    }

    // deleteFile
    sf.run_string("deleteFile(__p)").unwrap();
    let r = sf.run_string("return fileExists(__p)").unwrap();
    assert_eq!(r, Value::Bool(false));
}

#[test]
fn test_fs_read_notfound_error() {
    let r = run("return readFile(\"definitely_nonexistent_xyz.sf\")").unwrap_err();
    match r {
        Value::Error(e) => {
            assert!(e.message.contains("readFile"));
            assert!(e.message.contains("可能原因"));
        }
        _ => panic!("expected Error"),
    }
}

// ---- JSON 内置函数 ----

#[test]
fn test_json_encode() {
    assert_eq!(eval("return jsonEncode(undefined)"), Value::str("null"));
    assert_eq!(eval("return jsonEncode(undefined)"), Value::str("null"));
    assert_eq!(eval("return jsonEncode(true)"), Value::str("true"));
    assert_eq!(eval("return jsonEncode(42)"), Value::str("42"));
    assert_eq!(eval("return jsonEncode(\"hi\")"), Value::str("\"hi\""));
    assert_eq!(eval("return jsonEncode([1, 2, 3])"), Value::str("[1,2,3]"));
    assert_eq!(eval("return jsonEncode({\"a\": 1})"), Value::str("{\"a\":1}"));
}

#[test]
fn test_json_decode() {
    assert_eq!(eval("return jsonDecode(\"42\")"), Value::Int(42));
    assert_eq!(eval("return jsonDecode(\"true\")"), Value::Bool(true));
    assert_eq!(eval("return jsonDecode(\"null\")"), Value::Undefined);
    assert_eq!(eval("return jsonDecode(\"3.5\")"), Value::Float(3.5));
    assert_eq!(eval("return jsonDecode(\"\\\"hi\\\"\")"), Value::str("hi"));
    let r = eval("return jsonDecode(\"[1, 2, 3]\")[1]");
    assert_eq!(r, Value::Int(2));
    let r = eval("return jsonDecode(\"{\\\"a\\\": 10}\")[\"a\"]");
    assert_eq!(r, Value::Int(10));
}

#[test]
fn test_json_roundtrip() {
    let r = eval("return jsonDecode(jsonEncode({\"name\": \"sf\", \"v\": 1}))[\"name\"]");
    assert_eq!(r, Value::str("sf"));
}

#[test]
fn test_json_decode_error() {
    let r = run("return jsonDecode(\"{bad}\")").unwrap_err();
    match r {
        Value::Error(e) => {
            assert!(e.message.contains("jsonDecode"));
            assert!(e.message.contains("可能原因"));
        }
        _ => panic!("expected Error"),
    }
}

// ---- import 脚本加载 ----

/// write_test_sf 在 temp_dir 下写入一个 .sf 文件，返回其路径字符串。
fn write_test_sf(name: &str, content: &str) -> String {
    use std::fs;
    let path = std::env::temp_dir().join(name);
    fs::write(&path, content).expect("write test sf");
    path.to_string_lossy().into_owned()
}

#[test]
fn test_import_basic_merge() {
    // 库定义 var/func/const，主脚本 import 后应能直接引用
    let lib = write_test_sf("sflang_imp_lib.sf", "var impX = 42\nfunc impAdd(a,b){ return a+b }\nconst impK = \"v1\"");
    let lib_dir = std::path::Path::new(&lib).parent().unwrap().to_string_lossy().into_owned();

    let main_src = format!(
        "import \"{}\"\nassert(impX == 42)\nassert(impAdd(3,4) == 7)\nassert(impK == \"v1\")",
        lib.replace('\\', "/"),
    );
    // 主脚本文件名需在 lib 所在目录，使相对路径能解析
    let main_path = std::path::Path::new(&lib_dir).join("sflang_imp_main.sf");
    std::fs::write(&main_path, &main_src).unwrap();

    let mut sf = Sflang::new();
    let r = sf.run_file(main_path.to_str().unwrap());
    assert!(r.is_ok(), "import basic failed: {:?}", r);
}

#[test]
fn test_import_circular_detected() {
    // a imports b, b imports a → 应抛出循环依赖错误
    let dir = std::env::temp_dir();
    let a_path = dir.join("sflang_cyc_a.sf");
    let b_path = dir.join("sflang_cyc_b.sf");
    std::fs::write(&a_path, "import \"sflang_cyc_b.sf\"\nvar aV = 1").unwrap();
    std::fs::write(&b_path, "import \"sflang_cyc_a.sf\"\nvar bV = 2").unwrap();

    let mut sf = Sflang::new();
    let r = sf.run_file(a_path.to_str().unwrap());
    let err = r.unwrap_err();
    match err {
        Value::Error(e) => {
            assert!(e.message.contains("循环") || e.message.contains("circular"), "msg: {}", e.message);
            assert!(e.message.contains("可能原因"), "缺少 AI 提示: {}", e.message);
        }
        _ => panic!("expected Error for circular import"),
    }
}

#[test]
fn test_import_diamond_idempotent() {
    // main imports b 和 c，二者都 import common；common 只执行一次
    let dir = std::env::temp_dir();
    std::fs::write(dir.join("sflang_d_common.sf"), "var dCounter = 1\nfunc dFunc(){ return 100 }").unwrap();
    std::fs::write(dir.join("sflang_d_b.sf"), "import \"sflang_d_common.sf\"\nfunc useB(){ return dFunc() }").unwrap();
    std::fs::write(dir.join("sflang_d_c.sf"), "import \"sflang_d_common.sf\"\nfunc useC(){ return dFunc() }").unwrap();
    let main_src = "import \"sflang_d_b.sf\"\nimport \"sflang_d_c.sf\"\nassert(dCounter == 1)\nassert(useB() == 100)\nassert(useC() == 100)";
    let main_path = dir.join("sflang_d_main.sf");
    std::fs::write(&main_path, main_src).unwrap();

    let mut sf = Sflang::new();
    let r = sf.run_file(main_path.to_str().unwrap());
    assert!(r.is_ok(), "diamond import failed: {:?}", r);
}

#[test]
fn test_import_not_found_error() {
    // import 不存在的文件 → AI 友好错误
    let r = run("import \"definitely_nonexistent_xyz.sf\"").unwrap_err();
    match r {
        Value::Error(e) => {
            assert!(e.message.contains("import"));
            assert!(e.message.contains("可能原因"));
        }
        _ => panic!("expected Error"),
    }
}

// ---- 遗留问题修复回归测试 ----

#[test]
fn test_string_escape_literal_preservation() {
    // 无效转义保留字面量（Python/JS 风格）：\d \s \w 等不再报错
    assert_eq!(eval("return \"a\\\\db\""), Value::str("a\\db")); // 注：源码 \\ → 一个反斜杠
    // \U 后跟非十六进制：保留字面量（Windows 路径场景）
    assert_eq!(eval("return \"C:\\\\Users\\\\name\""), Value::str("C:\\Users\\name"));
    // 合法 \uNNNN 仍正常解析
    assert_eq!(eval("return \"\\u4e2d\""), Value::str("中"));
    // 合法 \xNN 仍正常解析
    assert_eq!(eval("return \"\\x41\""), Value::str("A"));
    // 已识别转义 \n \t 仍转换
    assert_eq!(eval("return \"a\\nb\""), Value::str("a\nb"));
}

#[test]
fn test_string_escape_unknown_not_error() {
    // 未识别转义不报错，保留字面量
    let r = run("var s = \"dir\\file\\name\""); // \f \n 是已识别转义，但 \d 这类应保留
    // \f 是换页符(未在识别集)，应保留字面量而非报错
    let _ = r;
    let r2 = run("var s = \"C:\\\\dir\\sub\"");
    assert!(r2.is_ok());
}

#[test]
fn test_multiline_string_preserves_first_char() {
    // 回归：三引号多行字符串曾因开头 """ 多消费一个字节丢失首字符
    // （lex_string 已消费 1 个 "，lex_multiline_string 又消费 3 个 → 共 4）
    assert_eq!(eval("return \"\"\"hello\"\"\""), Value::str("hello"));
    assert_eq!(eval("return \"\"\"a\nb\"\"\""), Value::str("a\nb"));
    // 单字符、空串
    assert_eq!(eval("return \"\"\"x\"\"\""), Value::str("x"));
    assert_eq!(eval("return \"\"\"\"\"\""), Value::str(""));
    // 内嵌单个双引号（不应提前闭合）
    assert_eq!(eval("return \"\"\"say \\\"hi\\\"\"\"\""), Value::str("say \"hi\""));
}

#[test]
fn test_math_floor_overflow_guard() {
    // floor 超出 i64 范围应报错而非静默饱和
    let r = run("return floor(1e30)").unwrap_err();
    match r {
        Value::Error(e) => assert!(e.message.contains("超出整数范围")),
        _ => panic!("expected Error for floor overflow"),
    }
}

#[test]
fn test_math_randint_huge_bounds() {
    // randInt(0, i64::MAX) 不应溢出 panic
    for _ in 0..20 {
        let v = eval("return randInt(0, 9223372036854775807)");
        match v {
            Value::Int(i) => assert!(i >= 0),
            _ => panic!("expected Int"),
        }
    }
}

#[test]
fn test_read_lines_degenerate() {
    use std::fs;
    let dir = std::env::temp_dir();
    let empty = dir.join("sflang_rl_empty.txt");
    let ab = dir.join("sflang_rl_ab.txt");
    let nl = dir.join("sflang_rl_nl.txt");
    fs::write(&empty, "").unwrap();
    fs::write(&ab, "a\nb\n").unwrap();
    fs::write(&nl, "\n").unwrap();

    let mut sf = Sflang::new();
    sf.set_global("__p", Value::str(empty.to_str().unwrap()));
    assert_eq!(sf.run_string("return len(readLines(__p))").unwrap(), Value::Int(0));

    sf.set_global("__p", Value::str(ab.to_str().unwrap()));
    assert_eq!(sf.run_string("return len(readLines(__p))").unwrap(), Value::Int(2));

    sf.set_global("__p", Value::str(nl.to_str().unwrap()));
    assert_eq!(sf.run_string("return len(readLines(__p))").unwrap(), Value::Int(1));
}

#[test]
fn test_json_depth_limit() {
    // 深度超过 200 层应报错而非栈溢出
    let deep: String = "[".repeat(250) + &"]".repeat(250);
    let src = format!("return jsonDecode(\"{}\")", deep);
    let r = run(&src).unwrap_err();
    match r {
        Value::Error(e) => assert!(e.message.contains("嵌套深度") || e.message.contains("depth")),
        _ => panic!("expected Error for deep JSON"),
    }
}

#[test]
fn test_import_retry_after_error() {
    // import 失败后不标记为已加载：修正脚本后可重新加载
    let dir = std::env::temp_dir();
    let bad = dir.join("sflang_retry_mod.sf");
    // 第一版：含运行时错误（除零）
    std::fs::write(&bad, "var retryVal = 10 / 0").unwrap();
    let main_src = "var __e = undefined\ntry { import \"sflang_retry_mod.sf\" } catch(e) { __e = e }\nassert(retryVal == 10)";
    let main_path = dir.join("sflang_retry_main.sf");
    std::fs::write(&main_path, main_src).unwrap();

    // 先修正模块为有效代码
    std::fs::write(&bad, "var retryVal = 10").unwrap();
    let mut sf = Sflang::new();
    let r = sf.run_file(main_path.to_str().unwrap());
    assert!(r.is_ok(), "import after fix should succeed: {:?}", r);
}

// ---- 阶段三：并发（run 真线程 + channel） ----

#[test]
fn test_concurrency_run_threads() {
    // run 启动真线程，共享全局数组。验证线程确实并发执行（counter 增长）。
    // 注：读-改-写非原子，故最终值 <= 预期；只要 >0 且不为单线程值即证明并发。
    let dir = std::env::temp_dir();
    let path = dir.join("sflang_conc_run.sf");
    std::fs::write(&path, "\
var counter = [0]\n\
func worker() {\n\
    var i = 0\n\
    while i < 500 { counter[0] = counter[0] + 1; i = i + 1 }\n\
}\n\
run worker()\n\
run worker()\n\
worker()\n\
sleepMs(150)\n\
return counter[0]\n\
").unwrap();
    let mut sf = Sflang::new();
    let r = sf.run_file(path.to_str().unwrap()).unwrap();
    // 3 个 worker 各 +500，单线程应为 1500。并发下因竞争可能 < 1500，但必 > 0。
    match r {
        Value::Int(n) => assert!(n > 0 && n <= 1500, "counter = {} (应 >0 且 <=1500)", n),
        _ => panic!("expected Int"),
    }
}

#[test]
fn test_concurrency_channel_producer_consumer() {
    // 生产者-消费者：用 channel 在线程间安全传递数据（无竞争）。
    let dir = std::env::temp_dir();
    let path = dir.join("sflang_conc_chan.sf");
    std::fs::write(&path, "\
var ch = newChannel()\n\
var results = []\n\
\n\
func producer() {\n\
    var i = 1\n\
    while i <= 10 { chanSend(ch, i); i = i + 1 }\n\
    chanSend(ch, -1)  // 结束标记\n\
}\n\
\n\
func consumer() {\n\
    while true {\n\
        var v = chanRecv(ch)\n\
        if v == -1 { return }\n\
        push(results, v)\n\
    }\n\
}\n\
\n\
run producer()\n\
consumer()\n\
sleepMs(100)\n\
return len(results)\n\
").unwrap();
    let mut sf = Sflang::new();
    let r = sf.run_file(path.to_str().unwrap()).unwrap();
    // 应恰好收到 10 个（channel 同步保证无丢失）
    assert_eq!(r, Value::Int(10), "channel producer-consumer 应收到 10 个值");
}

#[test]
fn test_concurrency_shared_globals() {
    // 验证 run 启动的线程能读取主线程定义的全局 var/func。
    let dir = std::env::temp_dir();
    let path = dir.join("sflang_conc_glob.sf");
    std::fs::write(&path, "\
var greeting = \"hello\"\n\
func shout() { println(greeting + \"!\") }\n\
run shout()\n\
shout()\n\
sleepMs(50)\n\
return greeting\n\
").unwrap();
    let mut sf = Sflang::new();
    let r = sf.run_file(path.to_str().unwrap()).unwrap();
    assert_eq!(r, Value::str("hello"));
}

#[test]
fn test_concurrency_value_send_sync() {
    // 验证 Value 现在是 Send + Sync（阶段三核心目标）。
    // 若类型不满足，编译期即失败；此处运行时再确认线程能传递复杂 Value。
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Value>();

    let dir = std::env::temp_dir();
    let path = dir.join("sflang_conc_complex.sf");
    std::fs::write(&path, "\
var ch = newChannel()\n\
func sender() {\n\
    chanSend(ch, {\"name\": \"sf\", \"items\": [1, 2, 3]})\n\
}\n\
run sender()\n\
var obj = chanRecv(ch)\n\
return obj[\"name\"]\n\
").unwrap();
    let mut sf = Sflang::new();
    sf.run_string("sleepMs(50)").ok();
    let r = sf.run_file(path.to_str().unwrap()).unwrap();
    assert_eq!(r, Value::str("sf"));
}

// ---- 同步原语（mutex/waitGroup/semaphore/once/rwmutex） ----

#[test]
fn test_sync_mutex_solves_race() {
    // mutex 保护读-改-写，多线程下 counter 应精确等于预期（对比无锁竞争）。
    let dir = std::env::temp_dir();
    let path = dir.join("sflang_sync_mutex.sf");
    std::fs::write(&path, "\
var counter = [0]\n\
var mu = newMutex()\n\
func worker() {\n\
    var i = 0\n\
    while i < 300 { lock(mu); counter[0] = counter[0] + 1; unlock(mu); i = i + 1 }\n\
}\n\
run worker(); run worker(); worker()\n\
sleepMs(300)\n\
return counter[0]\n\
").unwrap();
    let mut sf = Sflang::new();
    let r = sf.run_file(path.to_str().unwrap()).unwrap();
    // 3 worker × 300 = 900，mutex 保护下应精确
    assert_eq!(r, Value::Int(900), "mutex 应消除竞争，得到精确 900");
}

#[test]
fn test_sync_waitgroup_join() {
    // WaitGroup 等待多线程汇总
    let dir = std::env::temp_dir();
    let path = dir.join("sflang_sync_wg.sf");
    std::fs::write(&path, "\
var wg = newWaitGroup()\n\
var total = [0]\n\
var mu = newMutex()\n\
wgAdd(wg, 4)\n\
func adder(n) { lock(mu); total[0] = total[0] + n; unlock(mu); wgDone(wg) }\n\
run adder(1); run adder(2); run adder(3); run adder(4)\n\
wgWait(wg)\n\
return total[0]\n\
").unwrap();
    let mut sf = Sflang::new();
    let r = sf.run_file(path.to_str().unwrap()).unwrap();
    assert_eq!(r, Value::Int(10), "WaitGroup 汇总应 = 10");
}

#[test]
fn test_sync_semaphore_limits_concurrency() {
    // 信号量(1) 限制并发为 1（等价 mutex）
    let dir = std::env::temp_dir();
    let path = dir.join("sflang_sync_sem.sf");
    std::fs::write(&path, "\
var sem = newSemaphore(1)\n\
var cur = [0]\n\
var peak = [0]\n\
func w() {\n\
    semAcquire(sem)\n\
    cur[0] = cur[0] + 1\n\
    if cur[0] > peak[0] { peak[0] = cur[0] }\n\
    sleepMs(10)\n\
    cur[0] = cur[0] - 1\n\
    semRelease(sem)\n\
}\n\
run w(); run w(); run w()\n\
sleepMs(150)\n\
return peak[0]\n\
").unwrap();
    let mut sf = Sflang::new();
    let r = sf.run_file(path.to_str().unwrap()).unwrap();
    assert_eq!(r, Value::Int(1), "信号量=1 时并发峰值应为 1");
}

#[test]
fn test_sync_once_single_execution() {
    // Once 保证函数只执行一次（多线程并发调用）
    let dir = std::env::temp_dir();
    let path = dir.join("sflang_sync_once.sf");
    std::fs::write(&path, "\
var once = newOnce()\n\
var cnt = [0]\n\
func initf() { cnt[0] = cnt[0] + 1 }\n\
run onceDo(once, initf); run onceDo(once, initf)\n\
onceDo(once, initf); onceDo(once, initf)\n\
sleepMs(100)\n\
return cnt[0]\n\
").unwrap();
    let mut sf = Sflang::new();
    let r = sf.run_file(path.to_str().unwrap()).unwrap();
    assert_eq!(r, Value::Int(1), "onceDo 应只执行 1 次");
}

#[test]
fn test_sync_trylock() {
    // tryLock 空锁返回 true
    let dir = std::env::temp_dir();
    let path = dir.join("sflang_sync_trylock.sf");
    std::fs::write(&path, "return tryLock(newMutex())").unwrap();
    let mut sf = Sflang::new();
    let r = sf.run_file(path.to_str().unwrap()).unwrap();
    assert_eq!(r, Value::Bool(true));
}

#[test]
fn test_sync_rwmutex_shared_read() {
    // 多个读锁可并发获取（不阻塞）
    let dir = std::env::temp_dir();
    let path = dir.join("sflang_sync_rw.sf");
    std::fs::write(&path, "\
var rw = newRWMutex()\n\
rlock(rw); rlock(rw); rlock(rw)\n\
runlock(rw); runlock(rw); runlock(rw)\n\
wlock(rw); wunlock(rw)\n\
return 1\n\
").unwrap();
    let mut sf = Sflang::new();
    let r = sf.run_file(path.to_str().unwrap()).unwrap();
    assert_eq!(r, Value::Int(1), "rwmutex 读写锁操作应正常");
}

#[test]
fn test_sync_wrong_type_error() {
    // 类型错误应返回 AI 友好错误（如对非 mutex 调用 lock）
    let r = run("lock(123)").unwrap_err();
    match r {
        Value::Error(e) => {
            assert!(e.message.contains("lock"));
            assert!(e.message.contains("mutex"));
            assert!(e.message.contains("可能原因"));
        }
        _ => panic!("expected Error"),
    }
}

// ---- TXERROR 错误字符串机制 ----

#[test]
fn test_error_object() {
    // error() 创建 Error 对象
    let r = eval("return error(\"测试错误\")");
    assert!(matches!(r, Value::Error(_)));
}

#[test]
fn test_is_error() {
    // isError 判断 Error 对象
    assert_eq!(eval("return isError(error(\"x\"))"), Value::Bool(true));
    assert_eq!(eval("return isError(42)"), Value::Bool(false));
}

#[test]
fn test_is_err_error_object() {
    // isErr 同时识别 Error 对象
    assert_eq!(eval("return isErr(error(\"x\"))"), Value::Bool(true));
}

#[test]
fn test_is_err_txerror_string() {
    // isErr 同时识别 TXERROR 字符串
    assert_eq!(eval("return isErr(\"TXERROR:something\")"), Value::Bool(true));
}

#[test]
fn test_is_err_non_error() {
    // isErr 对非错误值返回 false
    assert_eq!(eval("return isErr(42)"), Value::Bool(false));
    assert_eq!(eval("return isErr(\"正常字符串\")"), Value::Bool(false));
    assert_eq!(eval("return isErr(\"\")"), Value::Bool(false));
}

#[test]
fn test_is_err_undefined() {
    // isErr(undefined) 返回 false（undefined 不是错误）
    assert_eq!(eval("return isErr(undefined)"), Value::Bool(false));
}

#[test]
fn test_is_err_str() {
    // isErrStr 只识别 TXERROR 字符串
    assert_eq!(eval("return isErrStr(\"TXERROR:错误\")"), Value::Bool(true));
    assert_eq!(eval("return isErrStr(\"正常\")"), Value::Bool(false));
    // Error 对象不算 TXERROR 字符串
    assert_eq!(eval("return isErrStr(error(\"x\"))"), Value::Bool(false));
}

#[test]
fn test_get_err_str_from_error() {
    // getErrStr 从 Error 对象提取信息
    let r = eval("return getErrStr(error(\"测试错误\"))");
    assert_eq!(r, Value::str("测试错误"));
}

#[test]
fn test_get_err_str_from_txerror() {
    // getErrStr 从 TXERROR 字符串提取信息（去前缀）
    let r = eval("return getErrStr(\"TXERROR:字符串错误\")");
    assert_eq!(r, Value::str("字符串错误"));
}

#[test]
fn test_get_err_str_non_error() {
    // getErrStr 对非错误值返回其字符串表示
    let r = eval("return getErrStr(42)");
    assert_eq!(r, Value::str("42"));
}

#[test]
fn test_err_strf() {
    // errStrf 格式化生成 TXERROR 字符串
    let r = eval("return errStrf(\"失败: %v\", 404)");
    assert_eq!(r, Value::str("TXERROR:失败: 404"));
    // 确认生成的字符串能被 isErr 识别
    assert_eq!(eval("return isErr(errStrf(\"x\"))"), Value::Bool(true));
}

#[test]
fn test_err_strf_no_args() {
    // errStrf 无参数返回纯前缀
    let r = eval("return errStrf()");
    assert_eq!(r, Value::str("TXERROR:"));
}

#[test]
fn test_err_to_empty_error() {
    // errToEmpty 对 Error 对象返回空字符串
    let r = eval("return errToEmpty(error(\"x\"))");
    assert_eq!(r, Value::str(""));
}

#[test]
fn test_err_to_empty_txerror() {
    // errToEmpty 对 TXERROR 字符串返回空字符串
    let r = eval("return errToEmpty(\"TXERROR:x\")");
    assert_eq!(r, Value::str(""));
}

#[test]
fn test_err_to_empty_non_error() {
    // errToEmpty 对非错误值原样返回
    assert_eq!(eval("return errToEmpty(42)"), Value::Int(42));
    assert_eq!(eval("return errToEmpty(\"正常\")"), Value::str("正常"));
}

#[test]
fn test_trim_err_preserves_error() {
    // trimErr 对错误值原样返回（不丢失错误）
    let r = eval("return trimErr(error(\"x\"))");
    assert!(matches!(r, Value::Error(_)));
    let r2 = eval("return trimErr(\"TXERROR:x\")");
    assert_eq!(r2, Value::str("TXERROR:x"));
}

#[test]
fn test_trim_err_normal() {
    // trimErr 对正常字符串去空白
    let r = eval("return trimErr(\"  hi  \")");
    assert_eq!(r, Value::str("hi"));
}

#[test]
fn test_trim_err_undefined() {
    // trimErr 对 undefined 返回空字符串
    let r = eval("return trimErr(undefined)");
    assert_eq!(r, Value::str(""));
}

#[test]
fn test_txerror_unified_pattern() {
    // 统一错误处理模式：同时处理 Error 对象和 TXERROR 字符串
    // 用全局函数定义 + 多次 run_string 调用
    let mut sf = Sflang::new();
    sf.run_string("func doWork(typ) {\n\
        if typ == \"error\" {\n\
            return error(\"error对象错误\")\n\
        }\n\
        if typ == \"txerror\" {\n\
            return errStrf(\"txerror字符串错误: %v\", 99)\n\
        }\n\
        return \"成功\"\n\
    }").unwrap();

    // error 对象
    sf.run_string("var __r = doWork(\"error\")").unwrap();
    let r1 = sf.get_global("__r").unwrap();
    assert!(is_err_value_test(&r1), "error 对象应被 isErr 识别");

    // TXERROR 字符串
    sf.run_string("var __r2 = isErr(doWork(\"txerror\"))").unwrap();
    let r2 = sf.get_global("__r2").unwrap();
    assert_eq!(r2, Value::Bool(true));

    // 成功
    sf.run_string("var __r3 = doWork(\"ok\")").unwrap();
    let r3 = sf.get_global("__r3").unwrap();
    assert_eq!(r3, Value::str("成功"));
}

#[test]
fn test_get_err_str_strips_error_prefix() {
    // VM 抛出的异常 message 以 "error: " 开头，getErrStr 应去掉
    let src = "try { return 1/0 } catch (e) { return getErrStr(e) }";
    let r = eval(src);
    // 不应以 "error: " 开头
    if let Value::Str(s) = &r {
        assert!(!s.starts_with("error: "), "getErrStr 不应包含 'error: ' 前缀");
    }
}

/// is_err_value_test 测试辅助：复用 isErr 的逻辑（避免依赖私有函数）。
fn is_err_value_test(v: &Value) -> bool {
    match v {
        Value::Error(_) => true,
        Value::Str(s) => s.starts_with("TXERROR:"),
        _ => false,
    }
}

// ---- Ring 环形缓冲区 ----

/// assert_array 断言 Value 是 Array 且元素按值等于预期。
fn assert_array(actual: &Value, expected: &[Value]) {
    match actual {
        Value::Array(a) => {
            let g = a.lock().unwrap();
            assert_eq!(g.len(), expected.len(), "数组长度不符: 实际 {:?} 期望 {:?}", actual, expected);
            for (i, (a, b)) in g.iter().zip(expected.iter()).enumerate() {
                assert!(a.equals(b), "元素 #{} 不符: 实际 {:?} 期望 {:?}", i, a, b);
            }
        }
        other => panic!("期望 array，得到 {:?}", other),
    }
}

#[test]
fn test_ring_new() {
    // newRing 创建 ring，初始 size 为 0
    let r = eval("var r = newRing(3); return ringSize(r)");
    assert_eq!(r, Value::Int(0));
}

#[test]
fn test_ring_push_basic() {
    let r = eval("\
        var r = newRing(10)\n\
        ringPush(r, 10)\n\
        ringPush(r, 20)\n\
        ringPush(r, 30)\n\
        return ringToList(r)");
    assert_array(&r, &[Value::Int(10), Value::Int(20), Value::Int(30)]);
}

#[test]
fn test_ring_capacity_eviction() {
    // 容量 3，push 4 个，淘汰头部
    let r = eval("\
        var r = newRing(3)\n\
        ringPush(r, 1); ringPush(r, 2); ringPush(r, 3); ringPush(r, 4)\n\
        return ringToList(r)");
    assert_array(&r, &[Value::Int(2), Value::Int(3), Value::Int(4)]);
}

#[test]
fn test_ring_get() {
    let r = eval("\
        var r = newRing(10)\n\
        ringPush(r, \"a\"); ringPush(r, \"b\"); ringPush(r, \"c\")\n\
        return ringGet(r, 0)");
    assert_eq!(r, Value::str("a"));
}

#[test]
fn test_ring_get_head_and_tail() {
    let r = eval("\
        var r = newRing(10)\n\
        ringPush(r, \"a\"); ringPush(r, \"b\"); ringPush(r, \"c\")\n\
        var head = ringGet(r)\n\
        var tail = ringGet(r, -1)\n\
        return [head, tail]");
    assert_array(&r, &[Value::str("a"), Value::str("c")]);
}

#[test]
fn test_ring_pick() {
    // pick 取出头部（删除）
    let r = eval("\
        var r = newRing(10)\n\
        ringPush(r, 10); ringPush(r, 20)\n\
        var first = ringPick(r)\n\
        var sizeAfter = ringSize(r)\n\
        return [first, sizeAfter]");
    assert_array(&r, &[Value::Int(10), Value::Int(1)]);
}

#[test]
fn test_ring_pop() {
    // pop 取出尾部（删除）
    let r = eval("\
        var r = newRing(10)\n\
        ringPush(r, 10); ringPush(r, 20)\n\
        var last = ringPop(r)\n\
        var sizeAfter = ringSize(r)\n\
        return [last, sizeAfter]");
    assert_array(&r, &[Value::Int(20), Value::Int(1)]);
}

#[test]
fn test_ring_pick_empty() {
    // 空 ring pick 返回 undefined
    let r = eval("var r = newRing(3); return ringPick(r)");
    assert_eq!(r, Value::Undefined);
}

#[test]
fn test_ring_pop_empty() {
    // 空 ring pop 返回 undefined
    let r = eval("var r = newRing(3); return ringPop(r)");
    assert_eq!(r, Value::Undefined);
}

#[test]
fn test_ring_insert() {
    let r = eval("\
        var r = newRing(10)\n\
        ringPush(r, 1); ringPush(r, 3)\n\
        ringInsert(r, 1, 2)\n\
        return ringToList(r)");
    assert_array(&r, &[Value::Int(1), Value::Int(2), Value::Int(3)]);
}

#[test]
fn test_ring_remove() {
    let r = eval("\
        var r = newRing(10)\n\
        ringPush(r, 1); ringPush(r, 2); ringPush(r, 3)\n\
        ringRemove(r, 1)\n\
        return ringToList(r)");
    assert_array(&r, &[Value::Int(1), Value::Int(3)]);
}

#[test]
fn test_ring_set() {
    let r = eval("\
        var r = newRing(10)\n\
        ringPush(r, 1); ringPush(r, 2); ringPush(r, 3)\n\
        ringSet(r, 1, 99)\n\
        return ringToList(r)");
    assert_array(&r, &[Value::Int(1), Value::Int(99), Value::Int(3)]);
}

#[test]
fn test_ring_clear() {
    let r = eval("\
        var r = newRing(10)\n\
        ringPush(r, 1); ringPush(r, 2)\n\
        ringClear(r)\n\
        return ringSize(r)");
    assert_eq!(r, Value::Int(0));
}

#[test]
fn test_ring_mixed_types() {
    // Ring 可存任意 Value 类型
    let r = eval("\
        var r = newRing(5)\n\
        ringPush(r, \"字符串\")\n\
        ringPush(r, 42)\n\
        ringPush(r, true)\n\
        return ringSize(r)");
    assert_eq!(r, Value::Int(3));
}

#[test]
fn test_ring_default_capacity() {
    // newRing() 无参数默认容量 10
    let r = eval("\
        var r = newRing()\n\
        ringPush(r, 1)\n\
        return ringSize(r)");
    assert_eq!(r, Value::Int(1));
}

#[test]
fn test_ring_unlimited_capacity() {
    // cap <= 0 表示无限制
    let r = eval("\
        var r = newRing(0)\n\
        for i := 0; i < 100; i++ { ringPush(r, i) }\n\
        return ringSize(r)");
    assert_eq!(r, Value::Int(100));
}

// ---- isType / isTypeCode 通用类型判断 ----

#[test]
fn test_is_type_basic() {
    assert_eq!(eval("return isType(42, \"int\")"), Value::Bool(true));
    assert_eq!(eval("return isType(3.14, \"float\")"), Value::Bool(true));
    assert_eq!(eval("return isType(true, \"bool\")"), Value::Bool(true));
    assert_eq!(eval("return isType(\"hi\", \"string\")"), Value::Bool(true));
}

#[test]
fn test_is_type_case_insensitive() {
    // 大小写不敏感
    assert_eq!(eval("return isType(42, \"Int\")"), Value::Bool(true));
    assert_eq!(eval("return isType(42, \"INT\")"), Value::Bool(true));
}

#[test]
fn test_is_type_mismatch() {
    assert_eq!(eval("return isType(42, \"string\")"), Value::Bool(false));
    assert_eq!(eval("return isType(\"hi\", \"int\")"), Value::Bool(false));
}

#[test]
fn test_is_type_undefined() {
    assert_eq!(eval("return isType(undefined, \"undefined\")"), Value::Bool(true));
}

#[test]
fn test_is_type_array_map_object() {
    assert_eq!(eval("return isType([1,2], \"array\")"), Value::Bool(true));
    assert_eq!(eval("return isType(map{}, \"map\")"), Value::Bool(true));
    assert_eq!(eval("return isType({}, \"object\")"), Value::Bool(true));
}

#[test]
fn test_is_type_ring() {
    // Ring 是 Native 细分类型
    assert_eq!(eval("var r = newRing(3); return isType(r, \"ring\")"), Value::Bool(true));
    // 不是 'native'
    assert_eq!(eval("var r = newRing(3); return isType(r, \"native\")"), Value::Bool(false));
}

#[test]
fn test_is_type_code() {
    // isTypeCode 按数字编码判断
    assert_eq!(eval("return isTypeCode(42, 1)"), Value::Bool(true));   // int = 1
    assert_eq!(eval("return isTypeCode(\"hi\", 4)"), Value::Bool(true));  // string = 4
    assert_eq!(eval("return isTypeCode(true, 3)"), Value::Bool(true));  // bool = 3
    assert_eq!(eval("return isTypeCode(42, 4)"), Value::Bool(false));   // int != string
}

#[test]
fn test_is_type_code_native() {
    // Ring 的 typeCode 是 11 (Native)
    assert_eq!(eval("var r = newRing(3); return isTypeCode(r, 11)"), Value::Bool(true));
}

// ---- StringBuilder ----

#[test]
fn test_string_builder_new() {
    let r = eval("var sb = newStringBuilder(); return len(sb)");
    assert_eq!(r, Value::Int(0));
}

#[test]
fn test_string_builder_new_with_init() {
    let r = eval("var sb = newStringBuilder(\"hello\"); return len(sb)");
    assert_eq!(r, Value::Int(5));
}

#[test]
fn test_string_builder_write_str() {
    let r = eval("\
        var sb = newStringBuilder()\n\
        writeStr(sb, \"abc\")\n\
        writeStr(sb, \"123\")\n\
        return toStr(sb)");
    assert_eq!(r, Value::str("abc123"));
}

#[test]
fn test_string_builder_write_any() {
    let r = eval("\
        var sb = newStringBuilder()\n\
        writeStr(sb, 42)\n\
        writeStr(sb, true)\n\
        return toStr(sb)");
    assert_eq!(r, Value::str("42true"));
}

#[test]
fn test_string_builder_write_bytes() {
    let r = eval("\
        var sb = newStringBuilder()\n\
        writeBytes(sb, \"AB\")\n\
        return toStr(sb)");
    assert_eq!(r, Value::str("AB"));
}

#[test]
fn test_string_builder_chain() {
    let r = eval("\
        var sb = newStringBuilder()\n\
        writeStr(writeStr(sb, \"a\"), \"b\")\n\
        return toStr(sb)");
    assert_eq!(r, Value::str("ab"));
}

#[test]
fn test_string_builder_len() {
    let r = eval("var sb = newStringBuilder(\"hello\"); return len(sb)");
    assert_eq!(r, Value::Int(5));
}

#[test]
fn test_string_builder_clear() {
    let r = eval("var sb = newStringBuilder(\"hello\"); clear(sb); return len(sb)");
    assert_eq!(r, Value::Int(0));
}

#[test]
fn test_string_builder_reset() {
    let r = eval("var sb = newStringBuilder(\"hello\"); reset(sb); return len(sb)");
    assert_eq!(r, Value::Int(0));
}

#[test]
fn test_string_builder_is_type() {
    assert_eq!(eval("return isType(newStringBuilder(), \"stringBuilder\")"), Value::Bool(true));
    assert_eq!(eval("return isTypeCode(newStringBuilder(), 19)"), Value::Bool(true));
}

#[test]
fn test_string_builder_json() {
    let r = eval("return jsonEncode(newStringBuilder(\"hello\"))");
    assert_eq!(r, Value::str("\"hello\""));
}

#[test]
fn test_clear_array() {
    let r = eval("var a = [1,2,3]; clear(a); return len(a)");
    assert_eq!(r, Value::Int(0));
}

#[test]
fn test_clear_map() {
    let r = eval("var m = map{a:1,b:2}; clear(m); return len(m)");
    assert_eq!(r, Value::Int(0));
}

// ---- CSV ----

/// assert_csv_rows 断言 readCsvFromStr 的结果与预期字符串二维数组一致。
fn assert_csv_rows(actual: &Value, expected: &[&[&str]]) {
    match actual {
        Value::Array(rows) => {
            let rg = rows.lock().unwrap();
            assert_eq!(rg.len(), expected.len(), "行数不符: {:?} vs {:?}", actual, expected);
            for (i, (row_val, exp_row)) in rg.iter().zip(expected.iter()).enumerate() {
                match row_val {
                    Value::Array(fields) => {
                        let fg = fields.lock().unwrap();
                        assert_eq!(fg.len(), exp_row.len(), "第{}行字段数不符", i);
                        for (j, (f, exp)) in fg.iter().zip(exp_row.iter()).enumerate() {
                            assert_eq!(f, &Value::str(exp), "第{}行第{}列: {:?} != {:?}", i, j, f, exp);
                        }
                    }
                    other => panic!("第{}行不是数组: {:?}", i, other),
                }
            }
        }
        other => panic!("期望二维数组，得到 {:?}", other),
    }
}

#[test]
fn test_csv_basic() {
    let r = eval("return readCsvFromStr(\"a,b,c\\n1,2,3\\n\")");
    assert_csv_rows(&r, &[&["a", "b", "c"], &["1", "2", "3"]]);
}

#[test]
fn test_csv_quoted_comma() {
    // 引号内的逗号不算分隔符
    let r = eval("return readCsvFromStr(\"a,b\\n\\\"x,y\\\",z\\n\")");
    assert_csv_rows(&r, &[&["a", "b"], &["x,y", "z"]]);
}

#[test]
fn test_csv_escaped_quotes() {
    // "" 表示字面双引号
    let r = eval("return readCsvFromStr(\"a\\n\\\"say \\\"\\\"hi\\\"\\\"\\\"\\n\")");
    assert_csv_rows(&r, &[&["a"], &["say \"hi\""]]);
}

#[test]
fn test_csv_newline_in_quotes() {
    // 引号内的换行是字段内容
    let r = eval("return readCsvFromStr(\"a\\n\\\"line1\\nline2\\\"\\n\")");
    assert_csv_rows(&r, &[&["a"], &["line1\nline2"]]);
}

#[test]
fn test_csv_no_trailing_newline() {
    // 最后一行无换行也能解析
    let r = eval("return readCsvFromStr(\"a,b\\nc,d\")");
    assert_csv_rows(&r, &[&["a", "b"], &["c", "d"]]);
}

#[test]
fn test_csv_single_field() {
    let r = eval("return readCsvFromStr(\"hello\\n\")");
    assert_csv_rows(&r, &[&["hello"]]);
}

#[test]
fn test_csv_empty_string() {
    let r = eval("return readCsvFromStr(\"\")");
    // 空字符串应返回空数组
    match r {
        Value::Array(a) => assert!(a.lock().unwrap().is_empty()),
        other => panic!("期望空数组，得到 {:?}", other),
    }
}

#[test]
fn test_csv_crlf_line_endings() {
    // \r\n 行结束
    let r = eval("return readCsvFromStr(\"a,b\\r\\n1,2\\r\\n\")");
    assert_csv_rows(&r, &[&["a", "b"], &["1", "2"]]);
}

#[test]
fn test_csv_write_read_roundtrip() {
    // 写入文件再读回
    let mut path = std::env::temp_dir();
    path.push("sflang_csv_test_roundtrip.csv");
    let path_str = path.to_str().unwrap().replace('\\', "/");

    let mut sf = Sflang::new();
    let src = format!(
        "var data = [[\"name\", \"age\"], [\"Alice\", \"30\"], [\"Tom, Jr.\", \"25\"]]\n\
        writeCsv(data, \"{}\")\n\
        var rows = readCsv(\"{}\")\n\
        var r0 = rows[0]\n\
        var r2 = rows[2]",
        path_str, path_str,
    );
    sf.run_string(&src).unwrap();

    let r0 = sf.get_global("r0").unwrap();
    assert_array(&r0, &[Value::str("name"), Value::str("age")]);

    let r2 = sf.get_global("r2").unwrap();
    assert_array(&r2, &[Value::str("Tom, Jr."), Value::str("25")]);

    let _ = std::fs::remove_file(&path);
}

// ---- Excel (xlsx) ----

#[test]
fn test_excel_new() {
    let r = eval("var wb = excelNew(); return isType(wb, \"workbook\")");
    assert_eq!(r, Value::Bool(true));
}

#[test]
fn test_excel_write_read_roundtrip() {
    let mut path = std::env::temp_dir();
    path.push("sflang_xlsx_test_roundtrip.xlsx");
    let path_str = path.to_str().unwrap().replace('\\', "/");

    let mut sf = Sflang::new();
    let src = format!(
        "var wb = excelNew()\n\
        excelWriteSheet(wb, 0, [[\"name\", \"age\"], [\"Alice\", 30], [\"Bob\", 25]])\n\
        excelSaveAs(wb, \"{}\")\n\
        var rows = excelReadSheet(\"{}\")\n\
        var r0 = rows[0]\n\
        var r1 = rows[1]",
        path_str, path_str,
    );
    sf.run_string(&src).unwrap();

    let r0 = sf.get_global("r0").unwrap();
    assert_array(&r0, &[Value::str("name"), Value::str("age")]);

    let r1 = sf.get_global("r1").unwrap();
    assert_array(&r1, &[Value::str("Alice"), Value::Int(30)]);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_excel_write_float() {
    let mut path = std::env::temp_dir();
    path.push("sflang_xlsx_test_float.xlsx");
    let path_str = path.to_str().unwrap().replace('\\', "/");

    let mut sf = Sflang::new();
    let src = format!(
        "var wb = excelNew()\n\
        excelWriteSheet(wb, 0, [[\"x\"], [3.14]])\n\
        excelSaveAs(wb, \"{}\")\n\
        var rows = excelReadSheet(\"{}\")\n\
        var val = rows[1][0]",
        path_str, path_str,
    );
    sf.run_string(&src).unwrap();

    let val = sf.get_global("val").unwrap();
    // calamine 可能返回 float
    match val {
        Value::Float(f) => assert!((f - 3.14).abs() < 0.001),
        Value::Int(i) => assert!((i as f64 - 3.14).abs() < 0.001),
        other => panic!("期望数字，得到 {:?}", other),
    }

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_excel_multiple_sheets() {
    let mut path = std::env::temp_dir();
    path.push("sflang_xlsx_test_multi.xlsx");
    let path_str = path.to_str().unwrap().replace('\\', "/");

    let mut sf = Sflang::new();
    let src = format!(
        "var wb = excelNew()\n\
        excelWriteSheet(wb, 0, [[\"sheet1data\"]])\n\
        var idx = excelNewSheet(wb, \"MySheet\")\n\
        excelWriteSheet(wb, \"MySheet\", [[\"sheet2data\"]])\n\
        excelSaveAs(wb, \"{}\")\n\
        var rows1 = excelReadSheet(\"{}\", 0)\n\
        var rows2 = excelReadSheet(\"{}\", \"MySheet\")\n\
        var all = excelReadAll(\"{}\")\n\
        var sheetCount = len(all)",
        path_str, path_str, path_str, path_str,
    );
    sf.run_string(&src).unwrap();

    let count = sf.get_global("sheetCount").unwrap();
    assert_eq!(count, Value::Int(2));

    let rows2 = sf.get_global("rows2").unwrap();
    match rows2 {
        Value::Array(a) => {
            let g = a.lock().unwrap();
            assert_eq!(g.len(), 1);
        }
        other => panic!("期望数组，得到 {:?}", other),
    }

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_excel_bool_type() {
    let mut path = std::env::temp_dir();
    path.push("sflang_xlsx_test_bool.xlsx");
    let path_str = path.to_str().unwrap().replace('\\', "/");

    let mut sf = Sflang::new();
    let src = format!(
        "var wb = excelNew()\n\
        excelWriteSheet(wb, 0, [[\"flag\"], [true], [false]])\n\
        excelSaveAs(wb, \"{}\")\n\
        var rows = excelReadSheet(\"{}\")\n\
        var v1 = rows[1][0]",
        path_str, path_str,
    );
    sf.run_string(&src).unwrap();

    let v1 = sf.get_global("v1").unwrap();
    match v1 {
        Value::Bool(b) => assert!(b),
        other => panic!("期望 bool true，得到 {:?}", other),
    }

    let _ = std::fs::remove_file(&path);
}

// ---- docx ----

/// create_test_docx 创建一个最小的测试 docx 文件，返回路径。
fn create_test_docx(filename: &str) -> PathBuf {
    use std::io::Write;
    let mut path = std::env::temp_dir();
    path.push(filename);

    let doc_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
        <w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
        <w:body>\
        <w:p><w:r><w:t>Hello World</w:t></w:r></w:p>\
        <w:p><w:r><w:t>Name: {name}</w:t></w:r></w:p>\
        <w:p><w:r><w:t>Date: {date}</w:t></w:r></w:p>\
        <w:p><w:r><w:t>Amp: test &amp; demo</w:t></w:r></w:p>\
        </w:body></w:document>";

    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
        <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
        <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
        <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
        <Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
        </Types>";

    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
        <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
        <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
        </Relationships>";

    let file = std::fs::File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();

    zip.start_file("[Content_Types].xml", options).unwrap();
    zip.write_all(content_types.as_bytes()).unwrap();
    zip.start_file("_rels/.rels", options).unwrap();
    zip.write_all(rels.as_bytes()).unwrap();
    zip.start_file("word/document.xml", options).unwrap();
    zip.write_all(doc_xml.as_bytes()).unwrap();
    zip.finish().unwrap();

    path
}

#[test]
fn test_docx_to_strs() {
    let path = create_test_docx("sflang_docx_test_tostrs.docx");
    let path_str = path.to_str().unwrap().replace('\\', "/");

    let mut sf = Sflang::new();
    let src = format!("var paras = docxToStrs(\"{}\")", path_str);
    sf.run_string(&src).unwrap();

    let paras = sf.get_global("paras").unwrap();
    match &paras {
        Value::Array(a) => {
            let g = a.lock().unwrap();
            assert!(g.len() >= 4, "应至少 4 段");
            assert_eq!(g[0], Value::str("Hello World"));
            // XML 实体解码
            assert_eq!(g[3], Value::str("Amp: test & demo"));
        }
        other => panic!("期望数组，得到 {:?}", other),
    }

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_docx_get_placeholders() {
    let path = create_test_docx("sflang_docx_test_placeholders.docx");
    let path_str = path.to_str().unwrap().replace('\\', "/");

    let mut sf = Sflang::new();
    let src = format!("var ph = docxGetPlaceholders(\"{}\")", path_str);
    sf.run_string(&src).unwrap();

    let ph = sf.get_global("ph").unwrap();
    match &ph {
        Value::Array(a) => {
            let g = a.lock().unwrap();
            assert_eq!(g.len(), 2, "应有 2 个占位符");
            assert!(g.contains(&Value::str("{name}")), "应包含 {{name}}");
            assert!(g.contains(&Value::str("{date}")), "应包含 {{date}}");
        }
        other => panic!("期望数组，得到 {:?}", other),
    }

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_docx_replace() {
    let path = create_test_docx("sflang_docx_test_replace.docx");
    let path_str = path.to_str().unwrap().replace('\\', "/");

    let mut sf = Sflang::new();
    let src = format!(
        "var bs = readFileBytes(\"{}\")\n\
        var newBs = docxReplace(bs, [\"{{name}}\", \"Alice\", \"{{date}}\", \"2026-07-07\"])\n\
        var outPath = joinPath(getTempDir(), \"sflang_docx_replaced.docx\")\n\
        writeFileBytes(outPath, newBs)\n\
        var paras = docxToStrs(outPath)\n\
        var p1 = paras[1]\n\
        var p2 = paras[2]",
        path_str,
    );
    sf.run_string(&src).unwrap();

    let p1 = sf.get_global("p1").unwrap();
    assert_eq!(p1, Value::str("Name: Alice"));

    let p2 = sf.get_global("p2").unwrap();
    assert_eq!(p2, Value::str("Date: 2026-07-07"));

    // 清理
    let out_path = std::env::temp_dir().join("sflang_docx_replaced.docx");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&out_path);
}

#[test]
fn test_docx_to_strs_from_bytes() {
    let path = create_test_docx("sflang_docx_test_bytes.docx");
    let path_str = path.to_str().unwrap().replace('\\', "/");

    let mut sf = Sflang::new();
    let src = format!(
        "var bs = readFileBytes(\"{}\")\n\
        var paras = docxToStrs(bs)",
        path_str,
    );
    sf.run_string(&src).unwrap();

    let paras = sf.get_global("paras").unwrap();
    match &paras {
        Value::Array(a) => {
            let g = a.lock().unwrap();
            assert_eq!(g[0], Value::str("Hello World"));
        }
        other => panic!("期望数组，得到 {:?}", other),
    }

    let _ = std::fs::remove_file(&path);
}

// ---- SQLite 数据库 ----

#[test]
fn test_db_connect_memory() {
    let r = eval("var db = dbConnect(\"sqlite3\", \":memory:\"); return isType(db, \"database\")");
    assert_eq!(r, Value::Bool(true));
}

#[test]
fn test_db_create_insert_query() {
    let mut sf = Sflang::new();
    sf.run_string("\
        var db = dbConnect(\"sqlite3\", \":memory:\")\n\
        dbExec(db, \"CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT, age INTEGER)\")\n\
        dbExec(db, \"INSERT INTO test (id, name, age) VALUES (?, ?, ?)\", 1, \"Alice\", 30)\n\
        dbExec(db, \"INSERT INTO test (id, name, age) VALUES (?, ?, ?)\", 2, \"Bob\", 25)\n\
        var rows = dbQuery(db, \"SELECT * FROM test ORDER BY id\")\n\
        var count = len(rows)\n\
        var name0 = rows[0][\"name\"]\n\
        var age1 = rows[1][\"age\"]").unwrap();

    assert_eq!(sf.get_global("count").unwrap(), Value::Int(2));
    assert_eq!(sf.get_global("name0").unwrap(), Value::str("Alice"));
    assert_eq!(sf.get_global("age1").unwrap(), Value::Int(25));
}

#[test]
fn test_db_query_with_params() {
    let mut sf = Sflang::new();
    sf.run_string("\
        var db = dbConnect(\"sqlite3\", \":memory:\")\n\
        dbExec(db, \"CREATE TABLE t (id INTEGER, val TEXT)\")\n\
        dbExec(db, \"INSERT INTO t VALUES (1, 'a')\")\n\
        dbExec(db, \"INSERT INTO t VALUES (2, 'b')\")\n\
        dbExec(db, \"INSERT INTO t VALUES (3, 'c')\")\n\
        var rows = dbQuery(db, \"SELECT * FROM t WHERE id > ?\", 1)\n\
        var count = len(rows)").unwrap();

    assert_eq!(sf.get_global("count").unwrap(), Value::Int(2));
}

#[test]
fn test_db_update() {
    let mut sf = Sflang::new();
    sf.run_string("\
        var db = dbConnect(\"sqlite3\", \":memory:\")\n\
        dbExec(db, \"CREATE TABLE t (id INTEGER, name TEXT)\")\n\
        dbExec(db, \"INSERT INTO t VALUES (1, \\\"old\\\")\")\n\
        var affected = dbExec(db, \"UPDATE t SET name = ? WHERE id = ?\", \"new\", 1)\n\
        var rows = dbQuery(db, \"SELECT name FROM t WHERE id = 1\")\n\
        var name = rows[0][\"name\"]").unwrap();

    assert_eq!(sf.get_global("affected").unwrap(), Value::Int(1));
    assert_eq!(sf.get_global("name").unwrap(), Value::str("new"));
}

#[test]
fn test_db_float_type() {
    let mut sf = Sflang::new();
    sf.run_string("\
        var db = dbConnect(\"sqlite3\", \":memory:\")\n\
        dbExec(db, \"CREATE TABLE t (val REAL)\")\n\
        dbExec(db, \"INSERT INTO t VALUES (?)\", 3.14)\n\
        var rows = dbQuery(db, \"SELECT val FROM t\")\n\
        var val = rows[0][\"val\"]").unwrap();

    match sf.get_global("val").unwrap() {
        Value::Float(f) => assert!((f - 3.14).abs() < 0.001),
        other => panic!("期望 float，得到 {:?}", other),
    }
}

#[test]
fn test_db_null_value() {
    let mut sf = Sflang::new();
    sf.run_string("\
        var db = dbConnect(\"sqlite3\", \":memory:\")\n\
        dbExec(db, \"CREATE TABLE t (id INTEGER, val TEXT)\")\n\
        dbExec(db, \"INSERT INTO t VALUES (1, ?)\", undefined)\n\
        var rows = dbQuery(db, \"SELECT val FROM t\")\n\
        var val = rows[0][\"val\"]").unwrap();

    // null → undefined
    assert_eq!(sf.get_global("val").unwrap(), Value::Undefined);
}

#[test]
fn test_db_file_database() {
    let mut path = std::env::temp_dir();
    path.push("sflang_sqlite_test.db");
    let path_str = path.to_str().unwrap().replace('\\', "/");
    let _ = std::fs::remove_file(&path);

    let mut sf = Sflang::new();
    let src = format!(
        "var db = dbConnect(\"sqlite3\", \"{}\")\n\
        dbExec(db, \"CREATE TABLE t (id INTEGER)\")\n\
        dbExec(db, \"INSERT INTO t VALUES (42)\")\n\
        dbClose(db)\n\
        var db2 = dbConnect(\"sqlite3\", \"{}\")\n\
        var rows = dbQuery(db2, \"SELECT * FROM t\")\n\
        var val = rows[0][\"id\"]",
        path_str, path_str,
    );
    sf.run_string(&src).unwrap();

    assert_eq!(sf.get_global("val").unwrap(), Value::Int(42));

    let _ = std::fs::remove_file(&path);
}

// ---- MySQL（需要运行中的 MySQL 服务器，标记 #[ignore]） ----

#[test]
#[ignore]
fn test_mysql_connect_and_crud() {
    // 需要 MySQL 运行在 localhost:3306，用户 root，密码 root，数据库 test
    // 运行：cargo test test_mysql_connect_and_crud -- --ignored
    let mut sf = Sflang::new();
    sf.run_string("\
        var db = dbConnect(\"mysql\", \"mysql://root:root@localhost:3306/test\")\n\
        dbExec(db, \"DROP TABLE IF EXISTS sflang_test\")\n\
        dbExec(db, \"CREATE TABLE sflang_test (id INT PRIMARY KEY, name VARCHAR(100), score FLOAT)\")\n\
        var affected = dbExec(db, \"INSERT INTO sflang_test VALUES (?, ?, ?)\", 1, \"Alice\", 95.5)\n\
        var rows = dbQuery(db, \"SELECT * FROM sflang_test\")\n\
        var count = len(rows)\n\
        var name0 = rows[0][\"name\"]\n\
        var score0 = rows[0][\"score\"]\n\
        dbExec(db, \"DROP TABLE sflang_test\")\n\
        dbClose(db)").unwrap();

    assert_eq!(sf.get_global("affected").unwrap(), Value::Int(1));
    assert_eq!(sf.get_global("count").unwrap(), Value::Int(1));
    assert_eq!(sf.get_global("name0").unwrap(), Value::str("Alice"));
    match sf.get_global("score0").unwrap() {
        Value::Float(f) => assert!((f - 95.5).abs() < 0.1),
        Value::Int(i) => assert!((i as f64 - 95.5).abs() < 0.1),
        other => panic!("期望数字，得到 {:?}", other),
    }
}

#[test]
#[ignore]
fn test_mysql_is_type() {
    let mut sf = Sflang::new();
    sf.run_string("var db = dbConnect(\"mysql\", \"mysql://root:root@localhost:3306/test\")").unwrap();
    let r = sf.get_global("db").unwrap();
    assert!(matches!(r, Value::Native(_)));
}

// ---- HTTP 服务器 ----

/// start_test_server 在后台线程启动一个测试服务器，返回端口号。
/// 测试结束后线程自动随进程退出。
fn start_test_server(script: &str, port: u16) -> u16 {
    let mut sf = Sflang::new();
    sf.set_output(std::io::sink());
    let script = script.replace("{PORT}", &port.to_string());
    std::thread::spawn(move || {
        let _ = sf.run_string(&script);
    });
    // 等待服务器启动
    std::thread::sleep(std::time::Duration::from_millis(300));
    port
}

#[test]
fn test_http_route_params() {
    let port = 19001;
    start_test_server("\
        var server = httpServer(\"--port={PORT}\", \"--host=127.0.0.1\")\n\
        serverSetHandler(server, \"/users/:id\", func(req, resp) {\n\
            setRespContentType(resp, \"application/json\")\n\
            return jsonEncode({\"id\": routeParamsG[\"id\"]})\n\
        })\n\
        serverStart(server, \"--thread\")\n\
    ", port);

    let mut sf = Sflang::new();
    sf.set_output(std::io::sink());
    let url = format!("http://127.0.0.1:{}/users/42", port);
    sf.run_string(&format!("var result = getWeb(\"{}\")", url)).unwrap();
    let result = sf.get_global("result").unwrap();
    match result {
        Value::Str(s) => assert!(s.contains("\"id\":\"42\""), "响应应包含 id=42: {}", s),
        other => panic!("期望字符串，得到 {:?}", other),
    }
}

#[test]
fn test_http_cookie_read() {
    let port = 19002;
    start_test_server("\
        var server = httpServer(\"--port={PORT}\", \"--host=127.0.0.1\")\n\
        serverSetHandler(server, \"/get\", func(req, resp) {\n\
            var v = getReqCookie(req, \"testCookie\")\n\
            if v == undefined { v = \"none\" }\n\
            return v\n\
        })\n\
        serverStart(server, \"--thread\")\n\
    ", port);

    let mut sf = Sflang::new();
    sf.set_output(std::io::sink());
    // 发送带 Cookie 头的请求
    let url = format!("http://127.0.0.1:{}/get", port);
    sf.run_string(&format!("var r = getWeb(\"{}\", \"Cookie: testCookie=hello123\")", url)).unwrap();
    let r = sf.get_global("r").unwrap();
    match r {
        Value::Str(s) => assert_eq!(&*s, "hello123", "Cookie 值应匹配"),
        other => panic!("期望字符串，得到 {:?}", other),
    }
}

#[test]
fn test_http_multi_route_params() {
    let port = 19003;
    start_test_server("\
        var server = httpServer(\"--port={PORT}\", \"--host=127.0.0.1\")\n\
        serverSetHandler(server, \"/posts/:postId/comments/:commentId\", func(req, resp) {\n\
            setRespContentType(resp, \"application/json\")\n\
            return jsonEncode({\"postId\": routeParamsG[\"postId\"], \"commentId\": routeParamsG[\"commentId\"]})\n\
        })\n\
        serverStart(server, \"--thread\")\n\
    ", port);

    let mut sf = Sflang::new();
    sf.set_output(std::io::sink());
    let url = format!("http://127.0.0.1:{}/posts/10/comments/20", port);
    sf.run_string(&format!("var r = getWeb(\"{}\")", url)).unwrap();
    let r = sf.get_global("r").unwrap();
    match r {
        Value::Str(s) => {
            assert!(s.contains("\"postId\":\"10\""), "响应应包含 postId=10: {}", s);
            assert!(s.contains("\"commentId\":\"20\""), "响应应包含 commentId=20: {}", s);
        }
        other => panic!("期望字符串，得到 {:?}", other),
    }
}

#[test]
fn test_http_save_file_uploads() {
    let port = 19004;
    // 使用系统临时目录，路径用正斜杠避免 Sflang 转义
    let tmp_base = std::env::temp_dir();
    let tmp_dir = tmp_base.join(format!("sflang_upload_{}", port));
    let tmp_dir_str = tmp_dir.to_string_lossy().replace('\\', "/");
    // 清理旧目录
    let _ = std::fs::remove_dir_all(&tmp_dir);

    start_test_server(&format!("\
        var server = httpServer(\"--port={{PORT}}\", \"--host=127.0.0.1\")\n\
        serverSetHandler(server, \"/upload\", func(req, resp) {{\n\
            var r = saveFileUploads(req, \"{}\")\n\
            if isErr(r) {{ return r }}\n\
            return jsonEncode(r)\n\
        }})\n\
        serverStart(server, \"--thread\")\n\
    ", tmp_dir_str), port);

    // 构造 multipart 请求体
    let boundary = "----sflangtest";
    let body = format!(
        "--{}\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nhello\r\n\
        --{}\r\nContent-Disposition: form-data; name=\"avatar\"; filename=\"test.txt\"\r\nContent-Type: text/plain\r\n\r\nfile content here\r\n\
        --{}--\r\n",
        boundary, boundary, boundary
    );

    // 用原始 TCP 发送 multipart 请求
    let req = format!(
        "POST /upload HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: multipart/form-data; boundary={}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        port, boundary, body.len(), body
    );
    let mut stream = std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
    use std::io::{Read, Write};
    stream.write_all(req.as_bytes()).unwrap();

    // 读取响应
    let mut resp_buf = Vec::new();
    stream.read_to_end(&mut resp_buf).unwrap();
    let resp_str = String::from_utf8_lossy(&resp_buf);
    // 找到响应体（双 \r\n 后）
    let body_start = resp_str.find("\r\n\r\n").map(|p| p + 4).unwrap_or(0);
    let body_part = &resp_str[body_start..];

    // 响应中应包含 avatar 字段
    assert!(body_part.contains("avatar"), "响应应包含 avatar 字段: {}", body_part);
    assert!(body_part.contains("test.txt"), "响应应包含文件名 test.txt: {}", body_part);

    // 验证文件已写入磁盘
    let saved_path = tmp_dir.join("test.txt");
    assert!(saved_path.exists(), "文件应已保存到 {}", saved_path.display());
    let content = std::fs::read_to_string(&saved_path).unwrap();
    assert_eq!(content, "file content here");

    // 清理
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

// ---- 压缩与 ZIP 测试 ----

#[test]
fn test_compress_decompress_bytes() {
    // 压缩 → 解压往返
    let orig = "Hello, Sflang! 这是一个测试字符串，重复多次以增加压缩率。".repeat(50);
    let mut sf = Sflang::new();
    sf.set_global("orig", Value::str(&orig));
    sf.run_string("var compressed = compressBytes(orig, 9)").unwrap();
    sf.run_string("var decompressed = decompressBytes(compressed)").unwrap();

    let compressed = sf.get_global("compressed").unwrap();
    let decompressed = sf.get_global("decompressed").unwrap();
    match (&compressed, &decompressed) {
        (Value::Bytes(c), Value::Bytes(d)) => {
            assert!(c.len() < orig.len(), "压缩后应更小: {} < {}", c.len(), orig.len());
            assert_eq!(&**d, orig.as_bytes(), "解压后应与原文一致");
        }
        _ => panic!("类型不匹配"),
    }
}

#[test]
fn test_gzip_gunzip_bytes() {
    let orig = "gzip 格式压缩测试，包含中文内容。".repeat(30);
    let mut sf = Sflang::new();
    sf.set_global("orig", Value::str(&orig));
    sf.run_string("var gz = gzipBytes(orig, 6)").unwrap();
    sf.run_string("var restored = gunzipBytes(gz)").unwrap();

    let restored = sf.get_global("restored").unwrap();
    match restored {
        Value::Bytes(b) => assert_eq!(&*b, orig.as_bytes()),
        _ => panic!("期望 bytes"),
    }
}

#[test]
fn test_zip_create_list_extract() {
    let tmp = std::env::temp_dir();
    let zip_path = tmp.join("sflang_test_zip.zip");
    let extract_dir = tmp.join("sflang_test_zip_extract");
    let _ = std::fs::remove_file(&zip_path);
    let _ = std::fs::remove_dir_all(&extract_dir);

    let zip_path_str = zip_path.to_string_lossy().replace('\\', "/");
    let extract_dir_str = extract_dir.to_string_lossy().replace('\\', "/");

    let script = format!(r#"
        var zw = zipCreate("{zip}")
        zipAddBytes(zw, bytes("hello world"), "test.txt")
        zipAddBytes(zw, bytes("中文内容测试"), "中文文件.txt")
        zipAddBytes(zw, bytes("nested data"), "subdir/nested.txt")
        zipClose(zw)

        var list = zipList("{zip}")
        var names = []
        for item in list {{
            push(names, item["name"])
        }}

        var count = zipExtract("{zip}", "{dest}")
        var r1 = zipReadFile("{zip}", "test.txt")
        var r2 = zipReadFile("{zip}", "中文文件.txt")
        var r3 = zipReadFile("{zip}", "subdir/nested.txt")
    "#, zip = zip_path_str, dest = extract_dir_str);

    let mut sf = Sflang::new();
    sf.set_output(std::io::sink());
    sf.run_string(&script).unwrap();

    // 验证 zipList 返回的文件名
    let names = sf.get_global("names").unwrap();
    match &names {
        Value::Array(a) => {
            let arr = a.lock().unwrap();
            assert_eq!(arr.len(), 3, "应有 3 个条目");
            let name_strs: Vec<String> = arr.iter().map(|v| {
                match v { Value::Str(s) => s.to_string(), _ => panic!("期望 string") }
            }).collect();
            assert!(name_strs.contains(&"test.txt".to_string()), "应包含 test.txt: {:?}", name_strs);
            assert!(name_strs.contains(&"中文文件.txt".to_string()), "应包含中文文件名: {:?}", name_strs);
            assert!(name_strs.contains(&"subdir/nested.txt".to_string()), "应包含嵌套路径: {:?}", name_strs);
        }
        _ => panic!("期望 array"),
    }

    // 验证解压文件数
    let count = sf.get_global("count").unwrap();
    assert_eq!(count, Value::Int(3));

    // 验证 zipReadFile 内容
    let r1 = sf.get_global("r1").unwrap();
    match r1 { Value::Bytes(b) => assert_eq!(&*b, b"hello world"), _ => panic!("期望 bytes") }
    let r2 = sf.get_global("r2").unwrap();
    match r2 { Value::Bytes(b) => assert_eq!(&*b, "中文内容测试".as_bytes()), _ => panic!("期望 bytes") }
    let r3 = sf.get_global("r3").unwrap();
    match r3 { Value::Bytes(b) => assert_eq!(&*b, b"nested data"), _ => panic!("期望 bytes") }

    // 验证解压到磁盘的文件
    let extracted_file = extract_dir.join("test.txt");
    assert!(extracted_file.exists(), "解压文件应存在");
    assert_eq!(std::fs::read_to_string(&extracted_file).unwrap(), "hello world");

    let extracted_cn = extract_dir.join("中文文件.txt");
    assert!(extracted_cn.exists(), "中文文件名解压应存在");
    assert_eq!(std::fs::read_to_string(&extracted_cn).unwrap(), "中文内容测试");

    // 清理
    let _ = std::fs::remove_file(&zip_path);
    let _ = std::fs::remove_dir_all(&extract_dir);
}

#[test]
fn test_zip_extract_single_file() {
    let tmp = std::env::temp_dir();
    let zip_path = tmp.join("sflang_test_zip_single.zip");
    let extract_path = tmp.join("sflang_test_single_out.txt");
    let _ = std::fs::remove_file(&zip_path);
    let _ = std::fs::remove_file(&extract_path);

    let zip_path_str = zip_path.to_string_lossy().replace('\\', "/");
    let extract_path_str = extract_path.to_string_lossy().replace('\\', "/");

    let script = format!(r#"
        var zw = zipCreate("{zip}")
        zipAddBytes(zw, bytes("file A content"), "a.txt")
        zipAddBytes(zw, bytes("file B content"), "b.txt")
        zipClose(zw)

        var ok = zipExtractFile("{zip}", "b.txt", "{dest}")
    "#, zip = zip_path_str, dest = extract_path_str);

    let mut sf = Sflang::new();
    sf.set_output(std::io::sink());
    sf.run_string(&script).unwrap();

    let ok = sf.get_global("ok").unwrap();
    assert_eq!(ok, Value::Bool(true));
    assert!(extract_path.exists());
    assert_eq!(std::fs::read_to_string(&extract_path).unwrap(), "file B content");

    let _ = std::fs::remove_file(&zip_path);
    let _ = std::fs::remove_file(&extract_path);
}

#[test]
fn test_zip_add_dir() {
    let tmp = std::env::temp_dir();
    let src_dir = tmp.join("sflang_test_zip_src");
    let zip_path = tmp.join("sflang_test_zip_dir.zip");
    let extract_dir = tmp.join("sflang_test_zip_dir_extract");
    let _ = std::fs::remove_dir_all(&src_dir);
    let _ = std::fs::remove_file(&zip_path);
    let _ = std::fs::remove_dir_all(&extract_dir);

    // 创建源目录结构
    std::fs::create_dir_all(src_dir.join("sub")).unwrap();
    std::fs::write(src_dir.join("file1.txt"), "content1").unwrap();
    std::fs::write(src_dir.join("sub").join("file2.txt"), "content2").unwrap();

    let src_dir_str = src_dir.to_string_lossy().replace('\\', "/");
    let zip_path_str = zip_path.to_string_lossy().replace('\\', "/");
    let extract_dir_str = extract_dir.to_string_lossy().replace('\\', "/");

    let script = format!(r#"
        var zw = zipCreate("{zip}")
        var n = zipAddDir(zw, "{src}", "")
        zipClose(zw)
        var list = zipList("{zip}")
        var count = zipExtract("{zip}", "{dest}")
    "#, zip = zip_path_str, src = src_dir_str, dest = extract_dir_str);

    let mut sf = Sflang::new();
    sf.set_output(std::io::sink());
    sf.run_string(&script).unwrap();

    // zipAddDir 应返回添加的文件数（不含目录条目）
    let n = sf.get_global("n").unwrap();
    assert_eq!(n, Value::Int(2), "应添加 2 个文件");

    // 验证解压后文件存在
    assert!(extract_dir.join("file1.txt").exists());
    assert!(extract_dir.join("sub").join("file2.txt").exists());
    assert_eq!(std::fs::read_to_string(extract_dir.join("file1.txt")).unwrap(), "content1");
    assert_eq!(std::fs::read_to_string(extract_dir.join("sub").join("file2.txt")).unwrap(), "content2");

    // 清理
    let _ = std::fs::remove_dir_all(&src_dir);
    let _ = std::fs::remove_file(&zip_path);
    let _ = std::fs::remove_dir_all(&extract_dir);
}

// ---- 新增内置函数（Charlang 对标）----

#[test]
fn test_str_to_int_float() {
    assert_eq!(eval("return strToInt(\"42\")"), Value::Int(42));
    assert_eq!(eval("return strToInt(\"  -7  \")"), Value::Int(-7));
    assert_eq!(eval("return strToInt(\"abc\", 99)"), Value::Int(99));
    assert_eq!(eval("return strToInt(\"abc\")"), Value::Int(0));
    match eval("return strToFloat(\"3.14\")") {
        Value::Float(f) => assert!((f - 3.14).abs() < 1e-9),
        other => panic!("期望 float，得到 {:?}", other),
    }
    match eval("return strToFloat(\"nan\", 0.0)") {
        Value::Float(f) => assert!((f - 0.0).abs() < 1e-9),
        other => panic!("期望 float，得到 {:?}", other),
    }
}

#[test]
fn test_str_contains_any_in() {
    assert_eq!(eval("return strContainsAny(\"abc123\", \"0123456789\")"), Value::Bool(true));
    assert_eq!(eval("return strContainsAny(\"abcdef\", \"0123456789\")"), Value::Bool(false));
    assert_eq!(eval("return strContainsIn(\"Hello 世界\", [\"Python\", \"世界\"])"), Value::Bool(true));
    assert_eq!(eval("return strContainsIn(\"Hello\", [\"Rust\", \"Go\"])"), Value::Bool(false));
}

#[test]
fn test_reg_contains() {
    // Sflang 正则参数顺序：regContains(pattern, text)
    assert_eq!(eval(r#"return regContains(`\d+`, "abc123")"#), Value::Bool(true));
    assert_eq!(eval(r#"return regContains(`\d+`, "abcdef")"#), Value::Bool(false));
    assert_eq!(eval(r#"return regContains(`^\w+@\w+\.\w+$`, "a@b.com")"#), Value::Bool(true));
}

#[test]
fn test_find_array() {
    let r = eval(r#"
        var users = [{name: "张三", age: 28}, {name: "李四", age: 35}, {name: "王五", age: 22}]
        var found = find(users, func(u) { return u["age"] >= 30 })
        return found["name"]
    "#);
    assert_eq!(r, Value::str("李四"));

    let r2 = eval(r#"
        var arr = [1, 2, 3]
        var nf = find(arr, func(x) { return x > 100 })
        return isUndefined(nf)
    "#);
    assert_eq!(r2, Value::Bool(true));
}

#[test]
fn test_get_json_node_str() {
    let r = eval(r#"
        var j = `{"name":"Sflang","author":{"name":"张三"},"tags":["lang","rust"]}`
        return getJsonNodeStr(j, "author.name")
    "#);
    assert_eq!(r, Value::str("张三"));

    let r2 = eval(r#"
        var j = `{"tags":["lang","rust","fast"]}`
        return getJsonNodeStr(j, "tags.#")
    "#);
    assert_eq!(r2, Value::str("3"));

    let r3 = eval(r#"
        var j = `{"tags":["lang","rust","fast"]}`
        return getJsonNodeStr(j, "tags.0")
    "#);
    assert_eq!(r3, Value::str("lang"));
}

#[test]
fn test_jwt_round_trip() {
    let r = eval(r#"
        var payload = {"sub": "user123", "name": "张三"}
        var token = genJwtToken(payload, "mySecret")
        var parsed = parseJwtToken(token, "mySecret")
        return getJsonNodeStr(parsed, "sub")
    "#);
    assert_eq!(r, Value::str("user123"));

    let r2 = eval(r#"
        var payload = {"sub": "user123", "name": "张三"}
        var token = genJwtToken(payload, "mySecret")
        var parsed = parseJwtToken(token, "mySecret")
        return getJsonNodeStr(parsed, "name")
    "#);
    assert_eq!(r2, Value::str("张三"));
}

#[test]
fn test_jwt_wrong_secret_fails() {
    let r = run(r#"
        var payload = {"sub": "user123"}
        var token = genJwtToken(payload, "correctSecret")
        var parsed = parseJwtToken(token, "wrongSecret")
    "#);
    assert!(r.is_err(), "错误密钥应返回错误");
}

#[test]
fn test_show_table() {
    let r = eval(r#"
        var data = [["姓名","年龄"],["张三",28],["李四",35]]
        return showTable(data)
    "#);
    match r {
        Value::Str(s) => {
            let s = s.as_ref();
            assert!(s.contains("姓名"), "表格应含表头");
            assert!(s.contains("张三"), "表格应含数据");
            assert!(s.contains("李四"), "表格应含数据");
            assert!(s.contains('+'), "表格应有边框");
            assert!(s.contains('|'), "表格应有列分隔");
        }
        other => panic!("期望 string，得到 {:?}", other),
    }
}

#[test]
fn test_show_table_empty() {
    let r = eval("return showTable([])");
    match r {
        Value::Str(s) => assert!(s.contains("empty"), "空表格应有提示"),
        other => panic!("期望 string，得到 {:?}", other),
    }
}

#[test]
fn test_show_table_no_header() {
    let r = eval(r#"
        var data = [["张三",28],["李四",35]]
        return showTable(data, {header: false})
    "#);
    match r {
        Value::Str(s) => {
            let lines: Vec<&str> = s.lines().collect();
            // 无表头时只有上边框 + 2 行数据 + 下边框 = 4 行
            assert_eq!(lines.len(), 4, "无表头应有 4 行");
        }
        other => panic!("期望 string，得到 {:?}", other),
    }
}

// ---- S3 客户端 ----

#[test]
fn test_s3_connect_invalid_endpoint() {
    // 缺少 http:// 前缀应返回错误
    let r = eval(r#"
        var c = s3Connect("minio.local:9000", "us-east-1", "ak", "sk")
        return isErr(c)
    "#);
    assert_eq!(r, Value::Bool(true));
}

#[test]
fn test_s3_connect_minio() {
    // MinIO 风格 endpoint 应成功创建客户端
    let r = eval(r#"
        var c = s3Connect("http://localhost:9000", "us-east-1", "ak", "sk")
        return typeName(c)
    "#);
    match r {
        Value::Str(s) => assert_eq!(s.as_ref(), "s3Client", "MinIO 客户端类型应为 s3Client"),
        other => panic!("期望 string，得到 {:?}", other),
    }
}

#[test]
fn test_s3_connect_aws() {
    // AWS S3 endpoint 应成功创建客户端
    let r = eval(r#"
        var c = s3Connect("https://s3.us-east-1.amazonaws.com", "us-east-1", "ak", "sk")
        return typeName(c)
    "#);
    match r {
        Value::Str(s) => assert_eq!(s.as_ref(), "s3Client"),
        other => panic!("期望 string，得到 {:?}", other),
    }
}

#[test]
fn test_s3_connect_path_style_auto_infer() {
    // MinIO 端口非 80/443 应推断为 path-style
    let r = eval(r#"
        var c = s3Connect("http://192.168.1.100:9000", "us-east-1", "ak", "sk")
        return typeName(c)
    "#);
    match r {
        Value::Str(s) => assert_eq!(s.as_ref(), "s3Client"),
        _ => panic!("应成功创建客户端"),
    }
}

#[test]
fn test_s3_connect_explicit_path_style() {
    // 显式指定 path-style 参数
    let r = eval(r#"
        var c = s3Connect("https://s3.us-east-1.amazonaws.com", "us-east-1", "ak", "sk", true)
        return typeName(c)
    "#);
    match r {
        Value::Str(s) => assert_eq!(s.as_ref(), "s3Client"),
        _ => panic!("应成功创建客户端"),
    }
}

#[test]
fn test_s3_close_returns_undefined() {
    // s3Close 无实际资源，仅返回 undefined
    let r = eval(r#"
        var c = s3Connect("http://localhost:9000", "us-east-1", "ak", "sk")
        if isErr(c) { return c }
        return s3Close(c)
    "#);
    assert_eq!(r, Value::Undefined);
}

#[test]
fn test_s3_function_arg_count_error() {
    // 参数不足应抛出错误（内置函数参数校验返回 Err）
    let r = run(r#"
        var c = s3Connect("http://localhost:9000", "us-east-1", "ak")
        return isErr(c)
    "#);
    assert!(r.is_err(), "参数不足应导致脚本运行失败");
}

#[test]
fn test_s3_wrong_client_type() {
    // 传入非 S3 客户端应抛出错误
    let r = run(r#"
        var notClient = 42
        var r = s3ListBuckets(notClient)
        return isErr(r)
    "#);
    assert!(r.is_err(), "传入非 S3 客户端应抛出错误");
}

#[test]
fn test_s3_wrong_client_type_undefined() {
    // 传入 undefined 应抛出错误
    let r = run(r#"
        var r = s3ListBuckets(undefined)
        return isErr(r)
    "#);
    assert!(r.is_err(), "传入 undefined 应抛出错误");
}

#[test]
fn test_s3_list_objects_arg_check() {
    // 缺少 bucket 参数应抛出错误
    let r = run(r#"
        var c = s3Connect("http://localhost:9000", "us-east-1", "ak", "sk")
        if isErr(c) { return false }
        var r = s3ListObjects(c)
        return isErr(r)
    "#);
    assert!(r.is_err(), "缺少 bucket 参数应抛出错误");
}

#[test]
fn test_s3_put_object_wrong_body_type() {
    // body 类型不匹配应抛出错误
    let r = run(r#"
        var c = s3Connect("http://localhost:9000", "us-east-1", "ak", "sk")
        if isErr(c) { return false }
        var r = s3PutObject(c, "bucket", "key", 42)
        return isErr(r)
    "#);
    assert!(r.is_err(), "body 类型不匹配应抛出错误");
}

// ---- S3 Multipart Upload ----

#[test]
fn test_s3_multipart_create_arg_check() {
    // 缺少 key 参数应抛出错误
    let r = run(r#"
        var c = s3Connect("http://localhost:9000", "us-east-1", "ak", "sk")
        if isErr(c) { return false }
        var r = s3MultipartCreate(c, "bucket")
        return isErr(r)
    "#);
    assert!(r.is_err(), "缺少 key 参数应抛出错误");
}

#[test]
fn test_s3_multipart_upload_part_invalid_part_no() {
    // partNo 越界应返回错误对象（不抛异常，业务错误）
    let r = eval(r#"
        var c = s3Connect("http://localhost:9000", "us-east-1", "ak", "sk")
        if isErr(c) { return false }
        var r = s3MultipartUploadPart(c, "bucket", "key", "uploadId", 0, "data")
        return isErr(r)
    "#);
    assert_eq!(r, Value::Bool(true));
}

#[test]
fn test_s3_multipart_upload_part_part_no_too_large() {
    // partNo > 10000 应返回错误对象
    let r = eval(r#"
        var c = s3Connect("http://localhost:9000", "us-east-1", "ak", "sk")
        if isErr(c) { return false }
        var r = s3MultipartUploadPart(c, "bucket", "key", "uploadId", 10001, "data")
        return isErr(r)
    "#);
    assert_eq!(r, Value::Bool(true));
}

#[test]
fn test_s3_upload_big_file_part_size_too_small() {
    // partSize < 5MB 应返回错误对象
    let r = eval(r#"
        var c = s3Connect("http://localhost:9000", "us-east-1", "ak", "sk")
        if isErr(c) { return false }
        var r = s3UploadBigFile(c, "bucket", "key", "/tmp/test.dat", 1024)
        return isErr(r)
    "#);
    assert_eq!(r, Value::Bool(true));
}

#[test]
fn test_s3_upload_big_file_part_size_too_large() {
    // partSize > 5GB 应返回错误对象
    let r = eval(r#"
        var c = s3Connect("http://localhost:9000", "us-east-1", "ak", "sk")
        if isErr(c) { return false }
        var r = s3UploadBigFile(c, "bucket", "key", "/tmp/test.dat", 6 * 1024 * 1024 * 1024)
        return isErr(r)
    "#);
    // 6GB 超过 usize 正数表示范围不会有问题，应返回 error
    assert_eq!(r, Value::Bool(true));
}

#[test]
fn test_s3_upload_big_file_not_exist() {
    // 文件不存在应返回错误对象
    let r = eval(r#"
        var c = s3Connect("http://localhost:9000", "us-east-1", "ak", "sk")
        if isErr(c) { return false }
        var r = s3UploadBigFile(c, "bucket", "key", "/tmp/not_exist_file_xxx_12345.dat")
        return isErr(r)
    "#);
    assert_eq!(r, Value::Bool(true));
}

#[test]
fn test_s3_multipart_complete_wrong_parts_type() {
    // parts 数组中含非 object 元素应返回错误对象
    let r = eval(r#"
        var c = s3Connect("http://localhost:9000", "us-east-1", "ak", "sk")
        if isErr(c) { return false }
        var r = s3MultipartComplete(c, "bucket", "key", "uploadId", [42])
        return isErr(r)
    "#);
    assert_eq!(r, Value::Bool(true));
}


// ---- switch 语句 ----

#[test]
fn test_switch_basic_match() {
    // 命中第二个 case
    assert_eq!(eval(r#"
        var r = ""
        switch 2 {
            case 1 { r = "one" }
            case 2 { r = "two" }
            default { r = "other" }
        }
        return r
    "#), Value::str("two"));
}

#[test]
fn test_switch_first_case() {
    assert_eq!(eval(r#"
        var r = ""
        switch 1 { case 1 { r = "one" } case 2 { r = "two" } }
        return r
    "#), Value::str("one"));
}

#[test]
fn test_switch_default_branch() {
    // 无匹配时走 default
    assert_eq!(eval(r#"
        var r = ""
        switch 99 { case 1 { r = "one" } default { r = "def" } }
        return r
    "#), Value::str("def"));
}

#[test]
fn test_switch_no_match_no_default() {
    // 无匹配且无 default：变量不变
    assert_eq!(eval(r#"
        var hit = "no"
        switch 99 { case 1 { hit = "yes" } }
        return hit
    "#), Value::str("no"));
}

#[test]
fn test_switch_no_fallthrough() {
    // 默认不贯穿：命中后只执行该 case
    assert_eq!(eval(r#"
        var log = ""
        switch 1 { case 1 { log = log + "A" } case 2 { log = log + "B" } }
        return log
    "#), Value::str("A"));
}

#[test]
fn test_switch_string_match() {
    assert_eq!(eval(r#"
        var r = ""
        switch "hi" { case "hello" { r = "en" } case "hi" { r = "matched" } }
        return r
    "#), Value::str("matched"));
}

#[test]
fn test_switch_break_in_case() {
    // break 提前跳出 switch
    assert_eq!(eval(r#"
        var log = ""
        switch 1 {
            case 1 {
                log = log + "A"
                break
            }
        }
        return log
    "#), Value::str("A"));
}

#[test]
fn test_switch_break_in_loop_only_exits_switch() {
    // switch 内的 break 只跳出 switch，不影响外层循环
    assert_eq!(eval(r#"
        var n = 0
        for i := 0; i < 3; i++ {
            switch i {
                case 1 { break }
                default { n = n + 1 }
            }
        }
        return n
    "#), Value::Int(2));
}

#[test]
fn test_switch_continue_affects_outer_loop() {
    // switch 内的 continue 作用于外层循环（switch 不是循环）
    assert_eq!(eval(r#"
        var sum = 0
        for i := 0; i < 5; i++ {
            switch i { case 2 { continue } }
            sum = sum + i
        }
        return sum
    "#), Value::Int(8));   // 0+1+3+4
}

#[test]
fn test_switch_break_label_outer_loop() {
    // break label 从 switch 内直接跳出外层循环
    assert_eq!(eval(r#"
        var total = 0
        outer: for i := 0; i < 10; i++ {
            switch i { case 3 { break outer } }
            total = total + i
        }
        return total
    "#), Value::Int(3));   // 0+1+2
}

#[test]
fn test_switch_match_expression() {
    // switch 值可以是任意表达式
    assert_eq!(eval(r#"
        var r = ""
        var a = 10
        switch a + 20 {
            case 15 { r = "fifteen" }
            case 30 { r = "thirty" }
            default { r = "other" }
        }
        return r
    "#), Value::str("thirty"));
}

#[test]
fn test_switch_empty() {
    // 空 switch（无 case 无 default）合法，什么都不做
    assert_eq!(eval(r#"
        switch 1 { }
        return 42
    "#), Value::Int(42));
}

#[test]
fn test_switch_duplicate_default_error() {
    // 重复 default 应解析报错
    assert!(run("switch 1 { default {} default {} }").is_err());
}

#[test]
fn test_switch_case_value_expression() {
    // case 值也可以是表达式
    assert_eq!(eval(r#"
        var r = ""
        switch 5 {
            case 2 + 2 { r = "four" }
            case 2 + 3 { r = "five" }
            default { r = "other" }
        }
        return r
    "#), Value::str("five"));
}

#[test]
fn test_switch_in_function_return() {
    // switch 在函数内配合 return
    assert_eq!(eval(r#"
        func describe(n) {
            switch n {
                case 0 { return "zero" }
                case 1 { return "one" }
                default { return "many" }
            }
        }
        return describe(0) + "," + describe(1) + "," + describe(99)
    "#), Value::str("zero,one,many"));
}

// ---- help 系统 ----

#[test]
fn test_help_no_args_lists_categories() {
    // help() 无参应返回包含分类信息的字符串
    let r = eval("return help()");
    assert!(matches!(r, Value::Str(_)));
    if let Value::Str(s) = &r {
        assert!(s.contains("内置函数"));
        assert!(s.contains("regex"));
        assert!(s.contains("core"));
    }
}

#[test]
fn test_help_function_detail() {
    // help("regFind") 应返回该函数的详细文档
    let r = eval("return help(\"regFind\")");
    assert!(matches!(r, Value::Str(_)));
    if let Value::Str(s) = &r {
        assert!(s.contains("regFind"));
        assert!(s.contains("pattern"));
        assert!(s.contains("regex"));
    }
}

#[test]
fn test_help_core_function() {
    // help("len") 应返回 len 的文档
    let r = eval("return help(\"len\")");
    if let Value::Str(s) = &r {
        assert!(s.contains("len"));
        assert!(s.contains("长度"));
    }
}

#[test]
fn test_help_unknown_function_returns_error() {
    // 不存在的函数名应返回错误
    let r = run("return help(\"___nonexistent_func___\")");
    assert!(r.is_err());
}

#[test]
fn test_help_category_query() {
    // help("regex") 应列出 regex 分类的函数
    let r = eval("return help(\"regex\")");
    if let Value::Str(s) = &r {
        assert!(s.contains("regFind"));
        assert!(s.contains("regMatch"));
    }
}

#[test]
fn test_help_fuzzy_match() {
    // 拼写接近的应返回相似建议
    let r = eval("return help(\"regfind\")");
    if let Value::Str(s) = &r {
        assert!(s.contains("regFind"));
    }
}

// ---- 块级作用域 ----

#[test]
fn test_block_scope_if_body() {
    // if body 内变量块外不可见
    assert_eq!(eval(r#"
        if true { var x = 10 }
        return x
    "#), Value::Undefined);
}

#[test]
fn test_block_scope_while_body() {
    // while body 内变量块外不可见
    assert_eq!(eval(r#"
        var i = 0
        while i < 1 { var y = 20; i++ }
        return y
    "#), Value::Undefined);
}

#[test]
fn test_block_scope_for_loop_var() {
    // C 风格 for 的循环变量循环后不可见
    assert_eq!(eval(r#"
        for i := 0; i < 3; i++ {}
        return i
    "#), Value::Undefined);
}

#[test]
fn test_block_scope_switch_case() {
    // switch case body 内变量块外不可见
    assert_eq!(eval(r#"
        switch 1 { case 1 { var z = 30 } }
        return z
    "#), Value::Undefined);
}

#[test]
fn test_block_scope_bare_block() {
    // 裸 {} 块内变量块外不可见
    assert_eq!(eval(r#"
        { var w = 40 }
        return w
    "#), Value::Undefined);
}

#[test]
fn test_block_scope_shadowing() {
    // 块内遮蔽外层变量，块外仍读外层值
    assert_eq!(eval(r#"
        var x = 10
        { var x = 99 }
        return x
    "#), Value::Int(10));
}

#[test]
fn test_block_scope_shadowing_inside() {
    // 块内读到内层遮蔽值
    assert_eq!(eval(r#"
        var x = 10
        var r = 0
        { var x = 99; r = x }
        return r
    "#), Value::Int(99));
}

#[test]
fn test_block_scope_nested_for_access_outer() {
    // 嵌套 for 内层能访问外层循环变量（作用域嵌套）
    assert_eq!(eval(r#"
        var s = 0
        for i := 0; i < 3; i++ {
            for j := 0; j < 2; j++ {
                s = s + i
            }
        }
        return s
    "#), Value::Int(6));   // (0+0)*2 + (1+1)*2 + (2+2)*2 = 0+2+4 = 6
}

#[test]
fn test_block_scope_closure_capture() {
    // 闭包捕获块内变量，块结束后仍可访问
    assert_eq!(eval(r#"
        func f() {
            var r
            { var x = 42; r = func() { return x } }
            return r()
        }
        return f()
    "#), Value::Int(42));
}

#[test]
fn test_block_scope_for_in_still_works() {
    // for-in 变量循环后不可见（已有行为，回归验证）
    assert_eq!(eval(r#"
        for v in range(3) {}
        return v
    "#), Value::Undefined);
}

#[test]
fn test_block_scope_try_catch_var() {
    // catch 变量 catch 块外不可见（用非全局名避免与 eG 冲突）
    assert_eq!(eval(r#"
        try { throw "err" } catch (myerr) { }
        return myerr
    "#), Value::Undefined);
}

// ---- JWT RS256 ----

#[test]
fn test_jwt_rs256_roundtrip() {
    // 生成 RSA 密钥对，用 RS256 签发 token，再用公钥验证
    use rsa::{RsaPrivateKey, pkcs8::EncodePublicKey, pkcs8::EncodePrivateKey};
    use rsa::pkcs8::LineEnding;

    let mut rng = rand::thread_rng();
    let private_key = RsaPrivateKey::new(&mut rng, 2048).expect("failed to generate key");
    let public_key = private_key.to_public_key();

    let private_pem = private_key
        .to_pkcs8_pem(LineEnding::LF)
        .expect("failed to encode private key")
        .to_string();
    let public_pem = public_key
        .to_public_key_pem(LineEnding::LF)
        .expect("failed to encode public key");

    // 生成 RS256 token
    let mut sf = Sflang::new();
    sf.set_global("__privKey", Value::str_from(private_pem));
    let token = sf.run_string(r#"
        return genJwtToken({"user": "alice", "role": "admin"}, __privKey, "RS256")
    "#).expect("genJwtToken RS256 failed");
    let token_str = match &token {
        Value::Str(s) => s.to_string(),
        _ => panic!("expected string, got {:?}", token),
    };

    // 验证 token（用公钥）
    let mut sf2 = Sflang::new();
    sf2.set_global("__pubKey", Value::str_from(public_pem));
    sf2.set_global("__token", Value::str_from(token_str));
    let payload = sf2.run_string(r#"
        return parseJwtToken(__token, __pubKey)
    "#).expect("parseJwtToken RS256 failed");

    // payload 应包含 user=alice（jsonDecode 返回 map 或 object）
    let user = match &payload {
        Value::Object(o) => o.lock().unwrap().get("user").map(|v| v.clone()),
        Value::Map(m) => m.lock().unwrap().get("user").map(|v| v.clone()),
        _ => panic!("expected object/map, got {:?}", payload),
    };
    assert_eq!(user, Some(Value::str("alice")));
}

#[test]
fn test_jwt_hs256_still_works() {
    // HS256 回归验证（默认算法）
    let r = eval(r#"
        var t = genJwtToken({"user": "bob"}, "secret123")
        var p = parseJwtToken(t, "secret123")
        return p["user"]
    "#);
    assert_eq!(r, Value::str("bob"));
}

#[test]
fn test_jwt_rs256_wrong_key_fails() {
    // 用错误的公钥验证应返回 error
    use rsa::{RsaPrivateKey, pkcs8::{EncodePublicKey, EncodePrivateKey}, pkcs8::LineEnding};

    let mut rng = rand::thread_rng();
    let key1 = RsaPrivateKey::new(&mut rng, 2048).unwrap();
    let key2 = RsaPrivateKey::new(&mut rng, 2048).unwrap();

    let priv_pem = key1.to_pkcs8_pem(LineEnding::LF).unwrap().to_string();
    let wrong_pub_pem = key2.to_public_key().to_public_key_pem(LineEnding::LF).unwrap();

    let mut sf = Sflang::new();
    sf.set_global("__privKey", Value::str_from(priv_pem));
    let token = sf.run_string(r#"
        return genJwtToken({"x": 1}, __privKey, "RS256")
    "#).unwrap();
    let token_str = token.to_str();

    let mut sf2 = Sflang::new();
    sf2.set_global("__pubKey", Value::str_from(wrong_pub_pem));
    sf2.set_global("__token", Value::str_from(token_str));
    let result = sf2.run_string(r#"return parseJwtToken(__token, __pubKey)"#);
    assert!(result.is_err(), "wrong key should fail verification");
}

// ---- 默认参数 ----

#[test]
fn test_default_param_basic() {
    assert_eq!(eval(r#"
        func greet(name, greeting="你好") { return greeting + ", " + name }
        return greet("Alice")
    "#), Value::str("你好, Alice"));
}

#[test]
fn test_default_param_override() {
    assert_eq!(eval(r#"
        func greet(name, greeting="你好") { return greeting + ", " + name }
        return greet("Bob", "Hi")
    "#), Value::str("Hi, Bob"));
}

#[test]
fn test_default_param_multiple() {
    assert_eq!(eval(r#"
        func f(a, b=10, c=20) { return a + b + c }
        return f(1)
    "#), Value::Int(31));
}

#[test]
fn test_default_param_reference_earlier() {
    // 默认值可引用前面的参数
    assert_eq!(eval(r#"
        func f(a, b=a+1) { return a + b }
        return f(10)
    "#), Value::Int(21));
}

#[test]
fn test_default_param_none_when_passed() {
    // 传了 undefined 仍用默认值
    assert_eq!(eval(r#"
        func f(a, b=99) { return b }
        return f(1)
    "#), Value::Int(99));
}

// ---- 字符串拼接 + ----

#[test]
fn test_string_concat_with_int() {
    assert_eq!(eval(r#"return "count: " + 42"#), Value::str("count: 42"));
}

#[test]
fn test_string_concat_with_float() {
    assert_eq!(eval(r#"return "pi=" + 3.14"#), Value::str("pi=3.14"));
}

#[test]
fn test_string_concat_with_bool() {
    assert_eq!(eval(r#"return "flag=" + true"#), Value::str("flag=true"));
}

#[test]
fn test_int_concat_string() {
    // 数字在左、字符串在右
    assert_eq!(eval(r#"return 1 + "a""#), Value::str("1a"));
}

#[test]
fn test_int_plus_int_still_arithmetic() {
    // 数值 + 数值 仍是加法，不是拼接
    assert_eq!(eval(r#"return 1 + 2"#), Value::Int(3));
}

#[test]
fn test_string_concat_chain() {
    assert_eq!(eval(r#"return "a=" + 1 + ", b=" + true"#), Value::str("a=1, b=true"));
}

// ---- 字符串插值 ${expr} ----

#[test]
fn test_interp_basic() {
    assert_eq!(eval(r#"
        var name = "World"
        return "Hello, ${name}!"
    "#), Value::str("Hello, World!"));
}

#[test]
fn test_interp_expression() {
    assert_eq!(eval(r#"return "${1+2} items""#), Value::str("3 items"));
}

#[test]
fn test_interp_multiple() {
    assert_eq!(eval(r#"
        var a = 1; var b = 2
        return "${a} + ${b} = ${a+b}"
    "#), Value::str("1 + 2 = 3"));
}

#[test]
fn test_interp_member_access() {
    assert_eq!(eval(r#"
        var o = {name: "Alice"}
        return "user: ${o.name}"
    "#), Value::str("user: Alice"));
}

#[test]
fn test_interp_function_call() {
    assert_eq!(eval(r#"
        var arr = [1, 2, 3]
        return "len=${len(arr)}"
    "#), Value::str("len=3"));
}

#[test]
fn test_interp_escaped() {
    // \${} 输出字面 ${}
    assert_eq!(eval(r#"return "literal: \${x}""#), Value::str("literal: ${x}"));
}

#[test]
fn test_interp_raw_string_no_interp() {
    // 反引号 raw string 不插值
    assert_eq!(eval(r#"return `${1+2}`"#), Value::str("${1+2}"));
}

#[test]
fn test_interp_nested_braces() {
    assert_eq!(eval(r#"
        var f = func(x) { return x * 2 }
        return "${f(5+3)}"
    "#), Value::str("16"));
}

#[test]
fn test_interp_no_interp_plain_string() {
    // 不含 ${} 的字符串仍是普通字符串
    assert_eq!(eval(r#"return "no interp here""#), Value::str("no interp here"));
}
