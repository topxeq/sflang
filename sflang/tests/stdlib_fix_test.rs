//! stdlib_fix_test.rs — 标准库 bug 修复回归测试
//!
//! 覆盖 builtins_math / builtins_str / builtins_bigint / builtins_json 中
//! 已确认并修复的缺陷（零参 panic、溢出回绕、深嵌套栈溢出、死循环、
//! 转义顺序、代理对丢失、大整数降级等）。
//!
//! 约定：可捕获错误经 error_value 返回（未捕获时 run_string 返回 Err），
//! 本测试用 run(...).is_err() 断言。

use sflang::value::Value;
use sflang::Sflang;

// ---- 辅助函数（与 api_test.rs 相同的求值/执行模式）----

/// eval 求值代码块并返回结果（用 IIFE 包裹，src 内需显式 return）。
fn eval(src: &str) -> Value {
    let mut sf = Sflang::new();
    let wrapped = format!("func __f() {{ {} }} var __r = __f()", src);
    sf.run_string(&wrapped).expect("eval failed");
    sf.get_global("__r").expect("__r not set")
}

/// run 执行代码，返回 Result（用于断言可捕获错误）。
fn run(src: &str) -> Result<Value, Value> {
    let mut sf = Sflang::new();
    sf.run_string(src)
}

// ---- builtins_math：abs ----

#[test]
fn test_abs_zero_args_error() {
    // 零参不再 panic，返回 error
    assert!(run("var r = abs()").is_err());
    // 正常路径不受影响
    assert_eq!(eval("return abs(-5)"), Value::Int(5));
    assert_eq!(eval("return abs(5)"), Value::Int(5));
    assert_eq!(eval("return abs(-3.5)"), Value::Float(3.5));
}

#[test]
fn test_abs_i64_min_error() {
    // i64::MIN 的绝对值 2^63 无法用 int 表示，返回 error（提示转 bigInt）
    let mut sf = Sflang::new();
    sf.set_global("__m", Value::Int(i64::MIN));
    let r = sf.run_string("var __r = abs(__m)");
    match r {
        Err(Value::Error(e)) => {
            assert!(e.message.contains("bigInt"), "msg: {}", e.message);
        }
        other => panic!("expected error, got {:?}", other.map(|v| v.to_str())),
    }
    // 次小负数仍正常
    let mut sf2 = Sflang::new();
    sf2.set_global("__m2", Value::Int(i64::MIN + 1));
    sf2.run_string("var __r2 = abs(__m2)").unwrap();
    assert_eq!(sf2.get_global("__r2").unwrap(), Value::Int(i64::MAX));
}

// ---- builtins_math：randInt 大跨度 ----

#[test]
fn test_rand_int_wide_range_no_wraparound() {
    // lo 为负、hi 为 i64::MAX：结果必须落在区间内（不因 lo + r 回绕溢出）
    let src = r#"
        var okT = true
        for i in range(0, 50) {
            var v = randInt(-1, 9223372036854775807)
            if v < -1 || v > 9223372036854775807 { okT = false }
        }
        return okT
    "#;
    assert_eq!(eval(src), Value::Bool(true));
    // 常规区间仍正确
    let v = eval("return randInt(1, 6)");
    match v {
        Value::Int(i) => assert!((1..=6).contains(&i)),
        other => panic!("expected int, got {}", other.to_str()),
    }
}

// ---- builtins_math：flexEval 深嵌套 ----

#[test]
fn test_flex_eval_deep_nesting_error() {
    // 1001 层括号超过深度上限 1000，返回 error 而非栈溢出
    assert!(run(r#"return flexEval(strRepeat("(", 1001) + "1" + strRepeat(")", 1001))"#).is_err());
    // 未闭合的深层括号同样报错（缺右括号）
    assert!(run(r#"return flexEval(strRepeat("(", 1001) + "1")"#).is_err());
    // 上限内正常求值
    assert_eq!(eval(r#"return flexEval(strRepeat("(", 999) + "7" + strRepeat(")", 999))"#), Value::Int(7));
    assert_eq!(eval(r#"return flexEval("1+2*3")"#), Value::Int(7));
    // 非整数值结果保持 float（整数值结果按设计装回 int）
    assert_eq!(eval(r#"return flexEval("1.5*3")"#), Value::Float(4.5));
}

// ---- builtins_math：min/max 类型与输入一致 ----

#[test]
fn test_min_max_float_preserved() {
    // 任一输入为 float 时结果保持 float
    assert_eq!(eval("return min(2.5, 3.0)"), Value::Float(2.5));
    assert_eq!(eval("return min(2.0, 3)"), Value::Float(2.0));
    assert_eq!(eval("return max(2, 3.0)"), Value::Float(3.0));
    assert_eq!(eval("return max(1, 2, 0.5)"), Value::Float(2.0));
    // 纯 int 输入仍返回 int
    assert_eq!(eval("return min(2, 3)"), Value::Int(2));
    assert_eq!(eval("return max([5, 2, 8])"), Value::Int(8));
    // 数组形式：混合类型保持 float
    assert_eq!(eval("return min([2.0, 3])"), Value::Float(2.0));
    assert_eq!(eval("return max([1, 2.5])"), Value::Float(2.5));
}

// ---- builtins_math：floor 边界（2^63 恰好越界）----

#[test]
fn test_floor_exact_2p63_boundary() {
    // f64 == 2^63 时 as i64 会饱和为 i64::MAX（差 1），应报错
    assert!(run("var r = floor(9223372036854775808.0)").is_err());
    // 2^63 以内的整数值浮点仍可正常转 int
    assert_eq!(eval("return floor(9.2e18)"), Value::Int(9200000000000000000));
}

// ---- builtins_str：isUtf8 / bytesGbToUtf8Str 零参 ----

#[test]
fn test_is_utf8_and_gb_zero_args_error() {
    // 零参不再 panic，返回 error
    assert!(run("var r = isUtf8()").is_err());
    assert!(run("var r = bytesGbToUtf8Str()").is_err());
    // 正常路径
    assert_eq!(eval("return isUtf8(\"hello\")"), Value::Bool(true));
}

// ---- builtins_str：strPad ----

#[test]
fn test_str_pad_negative_and_limit() {
    // 负长度返回 error（此前 as usize 变巨大值导致死循环）
    assert!(run("var r = strPad(\"42\", -1)").is_err());
    // 上限 1_000_000
    assert!(run("var r = strPad(\"42\", 1000001)").is_err());
    assert!(run("var r = strPad(\"42\", 9223372036854775807)").is_err());
    // 正常路径
    assert_eq!(eval("return strPad(\"42\", 5)"), Value::str("00042"));
    assert_eq!(eval("return strPad(\"42\", 5, \" \")"), Value::str("   42"));
    assert_eq!(eval("return strPad(\"42\", 5, \" \", true)"), Value::str("42   "));
    // 目标长度不大于当前长度：原样返回
    assert_eq!(eval("return strPad(\"abcdef\", 3)"), Value::str("abcdef"));
}

// ---- builtins_str：strSplitN 负数 ----

#[test]
fn test_str_split_n_negative() {
    // n <= 0 返回 [原串] 单元素数组（此前 -1 as usize 变巨大值导致限制失效）
    let r = eval("return strSplitN(\"a,b,c\", \",\", -1)");
    match r {
        Value::Array(a) => {
            let g = a.lock().unwrap();
            assert_eq!(g.len(), 1);
            assert_eq!(g[0], Value::str("a,b,c"));
        }
        other => panic!("expected array, got {}", other.to_str()),
    }
    // 正常 n 仍生效
    let r = eval("return strSplitN(\"a,b,c,d\", \",\", 2)");
    match r {
        Value::Array(a) => {
            let g = a.lock().unwrap();
            assert_eq!(g.len(), 2);
            assert_eq!(g[1], Value::str("b,c,d"));
        }
        other => panic!("expected array, got {}", other.to_str()),
    }
}

// ---- builtins_str：strRepeat 上限 ----

#[test]
fn test_str_repeat_size_limit() {
    // 结果总字节超过 1<<30 返回 error（此前直接 OOM）
    assert!(run("var r = strRepeat(\"a\", 1073741825)").is_err());
    assert!(run("var r = strRepeat(\"ab\", 536870913)").is_err());
    // 负数仍报错
    assert!(run("var r = strRepeat(\"a\", -1)").is_err());
    // 正常路径
    assert_eq!(eval("return strRepeat(\"ab\", 3)"), Value::str("ababab"));
    assert_eq!(eval("return len(strRepeat(\"a\", 1000))"), Value::Int(1000));
}

// ---- builtins_str：strQuote / strUnquote 往返 ----

#[test]
fn test_str_unquote_roundtrip() {
    // 场景 1：字面反斜杠 + n（raw 字符串 `a\nb` 含 4 个字符：a \ n b）
    // 此前链式 replace 会把第二个 \ 与 n 误组成 "\n" 变成换行，往返损坏
    let original = "a\\nb"; // Rust 字符串：a \ n b（4 字符）
    let r = eval("return strUnquote(strQuote(`a\\nb`))");
    assert_eq!(r, Value::str(original));
    // 引号内 "\n"（反斜杠 + n）是合法转义，解为真实换行
    assert_eq!(eval("return strUnquote(`\"a\\nb\"`)"), Value::str("a\nb"));
    // 引号内 "\\n"（双反斜杠 + n）解为字面反斜杠 + n，与 strQuote 往返一致
    assert_eq!(eval("return strUnquote(`\"a\\\\nb\"`)"), Value::str(original));
    // 场景 2：真实换行/制表/引号/反斜杠转义往返无损
    assert_eq!(eval("return strUnquote(strQuote(\"a\\nb\"))"), Value::str("a\nb"));
    assert_eq!(eval("return strUnquote(strQuote(\"a\\tb\"))"), Value::str("a\tb"));
    assert_eq!(eval("return strUnquote(strQuote(\"a\\\"b\"))"), Value::str("a\"b"));
    assert_eq!(eval("return strUnquote(strQuote(\"a\\\\b\"))"), Value::str("a\\b"));
    // 场景 3：未知转义保留原样（\x 不是已知转义）
    assert_eq!(eval("return strUnquote(`\"a\\xb\"`)"), Value::str("a\\xb"));
    // 场景 4：无引号输入原样返回
    assert_eq!(eval("return strUnquote(\"plain\")"), Value::str("plain"));
}

// ---- builtins_str：strLimit ----

#[test]
fn test_str_limit_negative_and_suffix() {
    // 负 maxLen 返回 error
    assert!(run("var r = strLimit(\"abc\", -1)").is_err());
    // 未超长原样返回
    assert_eq!(eval("return strLimit(\"Hi\", 10)"), Value::str("Hi"));
    // 超长截断：结果总长恰为 maxLen
    assert_eq!(eval("return strLimit(\"Hello World\", 5)"), Value::str("He..."));
    // maxLen 小于默认后缀长度（3）时后缀被截短，结果不超过 maxLen
    let r = eval("return strLimit(\"abcdef\", 2)");
    assert!(matches!(&r, Value::Str(s) if s.chars().count() == 2), "got {}", r.to_str());
    assert_eq!(r, Value::str(".."));
    // 自定义后缀
    assert_eq!(eval("return strLimit(\"Hello World\", 6, \"~~\")"), Value::str("Hell~~"));
    // maxLen 为 0：空串
    assert_eq!(eval("return strLimit(\"abc\", 0)"), Value::str(""));
}

// ---- builtins_str：strReplace 奇数附加参数 ----

#[test]
fn test_str_replace_unpaired_args_error() {
    // 附加参数必须成对，落单返回 error（此前静默忽略）
    assert!(run(r#"var r = strReplace("a-b-c", "-", "+", "x")"#).is_err());
    // 成对的多组替换正常
    assert_eq!(eval(r#"return strReplace("abc", "a", "x", "b", "y")"#), Value::str("xyc"));
    assert_eq!(eval(r#"return strReplace("a-b-c", "-", "+")"#), Value::str("a+b+c"));
}

// ---- builtins_str：simpleStrToMap 可选分隔符 ----

#[test]
fn test_simple_str_to_map_default_seps() {
    // sep1/sep2 缺省为 "," 与 "="
    assert_eq!(eval(r#"return simpleStrToMap("a=1,b=2")["a"]"#), Value::str("1"));
    assert_eq!(eval(r#"return simpleStrToMap("a=1,b=2")["b"]"#), Value::str("2"));
    // 显式指定等价
    assert_eq!(eval(r#"return simpleStrToMap("a=1,b=2", ",", "=")["a"]"#), Value::str("1"));
    // 自定义分隔符
    assert_eq!(eval(r#"return simpleStrToMap("x:1;y:2", ";", ":")["y"]"#), Value::str("2"));
    // 空串返回空 map
    assert_eq!(eval(r#"return len(simpleStrToMap(""))"#), Value::Int(0));
}

// ---- builtins_str：错误信息函数名统一（抽查行为不受影响）----

#[test]
fn test_str_fn_names_in_errors() {
    // strSub / strJoin 行为正常（内部错误信息已统一为注册名）
    assert_eq!(eval("return strSub(\"hello\", 1, 3)"), Value::str("el"));
    assert_eq!(eval("return strSub(\"hello\", -2)"), Value::str("lo"));
    assert_eq!(eval(r#"return strJoin(["a","b"], ",")"#), Value::str("a,b"));
    // 错误信息中包含注册名 strSub（而非 substring）
    let r = run("var r = strSub(1, 2)");
    match r {
        Err(Value::Error(e)) => assert!(e.message.contains("strSub"), "msg: {}", e.message),
        other => panic!("expected error, got {:?}", other.map(|v| v.to_str())),
    }
}

// ---- builtins_bigint：prec/scale 校验 ----

#[test]
fn test_big_float_div_precision_bounds() {
    // 负精度返回 error（此前 as u32 变巨大值导致挂死）
    assert!(run(r#"var r = bigFloatDiv(bigFloat("1"), bigFloat("3"), -5)"#).is_err());
    // 超上限返回 error
    assert!(run(r#"var r = bigFloatDiv(bigFloat("1"), bigFloat("3"), 10001)"#).is_err());
    // 边界内正常
    assert_eq!(
        eval(r#"return bigFloatDiv(bigFloat("1"), bigFloat("3"), 5) == bigFloat("0.33333")"#),
        Value::Bool(true)
    );
}

#[test]
fn test_big_float_scale_bounds() {
    // bigFloat(s, scale) 的 scale 校验 0..=10000
    assert!(run(r#"var r = bigFloat("123", -2)"#).is_err());
    assert!(run(r#"var r = bigFloat("123", 10001)"#).is_err());
    // 正常：123 × 10^-2 = 1.23
    assert_eq!(
        eval(r#"return bigFloat("123", 2) == bigFloat("1.23")"#),
        Value::Bool(true)
    );
}

// ---- builtins_bigint：bigInt(float) 走字符串路径 ----

#[test]
fn test_big_int_from_float_no_saturation() {
    // 1e30 此前经 as i64 饱和为 i64::MAX，现在经字符串路径得到精确值
    assert_eq!(
        eval(r#"return bigInt(1e30) == bigInt("1000000000000000000000000000000")"#),
        Value::Bool(true)
    );
    // 整数值 float 正常
    assert_eq!(eval(r#"return bigInt(3.0) == bigInt(3)"#), Value::Bool(true));
    // 含小数部分的 float 返回 error（无法精确表示为整数）
    assert!(run(r#"var r = bigInt(2.5)"#).is_err());
}

// ---- builtins_json：编码深度/循环引用防护 ----

#[test]
fn test_json_encode_self_reference_error() {
    // 自引用数组返回 error（此前无限递归栈溢出）
    assert!(run("var a = []; push(a, a); return jsonEncode(a)").is_err());
    assert!(run("var a = []; push(a, a); return formatJson(a)").is_err());
    assert!(run("var a = []; push(a, a); return compactJson(a)").is_err());
    // 正常编码不受影响
    assert_eq!(eval("return jsonEncode([1,2,3])"), Value::str("[1,2,3]"));
    assert_eq!(eval("return jsonEncode(undefined)"), Value::str("null"));
}

#[test]
fn test_json_encode_deep_nesting_guard() {
    // 250 层嵌套超过上限 200，返回 error
    let deep = "var a = [1]; for i in range(0, 250) { a = [a] }; return jsonEncode(a)";
    assert!(run(deep).is_err());
    // 150 层在限内，正常
    let ok_src = "var a = [1]; for i in range(0, 150) { a = [a] }; return len(jsonEncode(a)) > 0";
    assert_eq!(eval(ok_src), Value::Bool(true));
}

// ---- builtins_json：\uXXXX 代理对 ----

#[test]
fn test_json_decode_surrogate_pair() {
    // 高代理 + 低代理合成一个 emoji（U+1F600），得到 1 个字符
    let r = eval(r#"return jsonDecode(`"\uD83D\uDE00"`)"#);
    match &r {
        Value::Str(s) => {
            assert_eq!(s.chars().count(), 1, "s: {:?}", s);
            assert_eq!(s.chars().next().unwrap() as u32, 0x1F600);
        }
        other => panic!("expected string, got {}", other.to_str()),
    }
    // 普通 \uXXXX 仍正确
    assert_eq!(eval(r#"return jsonDecode(`"\u4e2d"`)"#), Value::str("中"));
    // 单独的高代理 / 低代理 / 高代理后非低代理：解析错误而非静默丢弃
    assert!(run(r#"return jsonDecode(`"\uD800"`)"#).is_err());
    assert!(run(r#"return jsonDecode(`"\uDC00"`)"#).is_err());
    assert!(run(r#"return jsonDecode(`"\uD800A"`)"#).is_err());
    assert!(run(r#"return jsonDecode(`"\uD800\uD800"`)"#).is_err());
}

// ---- builtins_json：大整数解码为 bigInt ----

#[test]
fn test_json_decode_big_int_not_degraded() {
    // 超出 i64 的整数解码为 bigInt（不再静默降级 float 丢精度）
    assert_eq!(
        eval(r#"return typeName(jsonDecode("99999999999999999999"))"#),
        Value::str("bigInt")
    );
    // 负大整数同样
    assert_eq!(
        eval(r#"return typeName(jsonDecode("-99999999999999999999"))"#),
        Value::str("bigInt")
    );
    // 数值正确（可与 bigInt 比较往返）
    assert_eq!(
        eval(r#"return jsonDecode("99999999999999999999") == bigInt("99999999999999999999")"#),
        Value::Bool(true)
    );
    // i64 范围内仍为 int；浮点形式仍为 float
    assert_eq!(eval(r#"return typeName(jsonDecode("42"))"#), Value::str("int"));
    assert_eq!(eval(r#"return typeName(jsonDecode("1.5"))"#), Value::str("float"));
}

// 说明：datetimeParse 相关修复不在本批任务范围内，故不在此测试。
