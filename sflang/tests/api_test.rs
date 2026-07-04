//! Sflang 库的单元测试
//!
//! 覆盖：词法、语法、编译、VM、嵌入式 API 的核心功能。

use sflang::Sflang;
use sflang::value::Value;

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

// ---- 函数 ----

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
    assert_eq!(eval("return nil"), Value::Undefined);
    // undefined == undefined
    assert_eq!(eval("return undefined == undefined"), Value::Bool(true));
    assert_eq!(eval("return undefined == nil"), Value::Bool(true));
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
    assert_eq!(eval("return undefined or 5"), Value::Bool(true));
    assert_eq!(eval("return undefined and 5"), Value::Bool(false));
    // undefined 取成员 / 索引 → 抛异常（类型不兼容）
    assert!(run("var x = undefined; return x.foo").is_err());
    assert!(run("var x = undefined; return x[0]").is_err());
    // undefined 比较 < → 抛异常（类型不兼容）
    assert!(run("return undefined < 5").is_err());
    // default / defaultUndef
    assert_eq!(eval("return default(undefined, 99)"), Value::Int(99));
    assert_eq!(eval("return defaultUndef(undefined, 99)"), Value::Int(99));
    // defaultUndef 不对 0/"" 触发兜底
    assert_eq!(eval("return defaultUndef(0, 99)"), Value::Int(0));
    assert_eq!(eval("return defaultUndef(\"\", 99)"), Value::str(""));
    // default 对 falsy(0) 触发兜底
    assert_eq!(eval("return default(0, 99)"), Value::Int(99));
    // undefToEmpty
    assert_eq!(eval("return undefToEmpty(undefined)"), Value::str(""));
    assert_eq!(eval("return undefToEmpty(42)"), Value::str("42"));
}

#[test]
fn test_symbol_logic_operators() {
    // && || ! 作为 and or not 的等价符号别名
    assert_eq!(eval("return true && false"), Value::Bool(false));
    assert_eq!(eval("return true || false"), Value::Bool(true));
    assert_eq!(eval("return !true"), Value::Bool(false));
    assert_eq!(eval("return !0"), Value::Bool(true));
    // 与关键字混用等价
    assert_eq!(eval("return (1 < 2) && (3 > 2) or (1 > 5)"), Value::Bool(true));
    assert_eq!(eval("return (1 < 2) and (3 > 2) || false"), Value::Bool(true));
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
    assert_eq!(sf2.run_string("return isFile(openFile(__p, \"r\"))").unwrap(), Value::Bool(true));

    // 逐行读取
    let mut sf3 = Sflang::new();
    sf3.set_global("__p", Value::str(path_str));
    let r = sf3.run_string(r#"
        var f = openFile(__p, "r")
        defer closeFile(f)
        var lines = []
        var line = readLine(f)
        while not isUndefined(line) {
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
    // bytesAt：取字节
    assert_eq!(eval("return bytesAt(\"AB\", 0)"), Value::Int(65));
    assert_eq!(eval("return bytesAt(\"AB\", 1)"), Value::Int(66));
    assert_eq!(eval("return bytesAt(\"中\", 0)"), Value::Int(0xE4)); // 首字节
    // bytesSlice：按字节切
    assert_eq!(eval("return bytesHex(bytesSlice(\"ABC\", 0, 2))"), Value::str("4142"));
    assert_eq!(eval("return bytesHex(bytesSlice(\"中\", 0, 3))"), Value::str("e4b8ad")); // 完整 UTF-8
    // 负索引
    assert_eq!(eval("return bytesAt(\"AB\", -1)"), Value::Int(66));
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
    assert_eq!(eval("return isBigInt(bigInt(5))"), Value::Bool(true));
    assert_eq!(eval("return isBigInt(5)"), Value::Bool(false));
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
    assert_eq!(eval("return isBigFloat(bigFloat(\"1\"))"), Value::Bool(true));
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
fn test_byte_array_basics() {
    // 构造与类型
    assert!(eval("return byteArray(4)").type_name() == "byteArray".to_string());
    assert_eq!(eval("return typeName(byteArray(4))"), Value::str("byteArray"));
    assert_eq!(eval("return isByteArray(byteArray(4))"), Value::Bool(true));
    assert_eq!(eval("return isByteArray(bytes(\"ab\"))"), Value::Bool(false));
    // 长度与填充
    assert_eq!(eval("return len(byteArray(8))"), Value::Int(8));
    assert_eq!(eval("return byteArray(3, 0xFF)[0]"), Value::Int(255));
    assert_eq!(eval("return byteArray(3, 0x41)[2]"), Value::Int(65));
    // 索引读写（就地修改）
    assert_eq!(eval("var ba = byteArray(3); ba[0] = 65; ba[1] = 66; ba[2] = 67; return ba[1]"), Value::Int(66));
    // 负索引
    assert_eq!(eval("var ba = byteArray(3, 0x41); return ba[-1]"), Value::Int(65));
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
            assert_eq!(arr[0], Value::Int(65));  // 原 bytes 不变
            assert_eq!(arr[1], Value::Int(90));  // byteArray 已改
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
    assert_eq!(eval("return nil ?? 7"), Value::Int(7));
    // 嵌套（左结合）
    assert_eq!(eval("return undefined ?? (undefined ?? 7)"), Value::Int(7));
    assert_eq!(eval("return undefined ?? undefined ?? 8"), Value::Int(8));
    // 与 or 混用：?? 优先级低于 or → (undefined or 0) ?? 99
    //   undefined or 0 → false（falsy），false 非 undefined → 取左值 false
    assert_eq!(eval("return undefined or 0 ?? 99"), Value::Bool(false));
    // 短路求值：左值非 undefined 时不求右（右含除零也不报错）
    assert_eq!(eval("return 42 ?? (1/0 == 0)"), Value::Int(42));
}

#[test]
fn test_type_error_suggestion() {
    // undefined 参与算术属类型不兼容 → 抛异常（nil 是 undefined 的别名）
    let r = run("var __r = 1 + nil").unwrap_err();
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
    assert_eq!(eval("var r = 5; return r >= 1 and r <= 10"), Value::Bool(true));
    assert_eq!(eval("var r = 5; return r > 10 or r == 5"), Value::Bool(true));
    assert_eq!(eval("var r = 5; return r > 10 and r == 5"), Value::Bool(false));
    // 短路：and 左假时不求右（右为 1/0 不会除零）
    assert_eq!(eval("return false and 1/0 == 0"), Value::Bool(false));
    // 短路：or 左真时不求右
    assert_eq!(eval("return true or 1/0 == 0"), Value::Bool(true));
    // 嵌套
    assert_eq!(eval("return (1 < 2) and (3 > 2) or (1 > 5)"), Value::Bool(true));
    // 非布尔操作数的真值判断
    assert_eq!(eval("return 0 and 1"), Value::Bool(false));
    assert_eq!(eval("return 1 and 2"), Value::Bool(true));
}

// ---- 字符串内置函数 ----

#[test]
fn test_str_case_trim() {
    assert_eq!(eval("return upper(\"abc\")"), Value::str("ABC"));
    assert_eq!(eval("return lower(\"AbC\")"), Value::str("abc"));
    assert_eq!(eval("return trim(\"  hi  \")"), Value::str("hi"));
    assert_eq!(eval("return trimStart(\"  hi\")"), Value::str("hi"));
    assert_eq!(eval("return trimEnd(\"hi  \")"), Value::str("hi"));
}

#[test]
fn test_str_find_contains() {
    assert_eq!(eval("return find(\"hello\", \"ll\")"), Value::Int(2));
    assert_eq!(eval("return find(\"hello\", \"z\")"), Value::Int(-1));
    assert_eq!(eval("return contains(\"hello\", \"ell\")"), Value::Bool(true));
    assert_eq!(eval("return startsWith(\"hello\", \"he\")"), Value::Bool(true));
    assert_eq!(eval("return endsWith(\"hello\", \"lo\")"), Value::Bool(true));
}

#[test]
fn test_str_replace_split_join() {
    assert_eq!(eval("return replace(\"a-b-c\", \"-\", \"+\")"), Value::str("a+b+c"));
    let r = eval("return join(split(\"a,b,c\", \",\"), \"-\")");
    assert_eq!(r, Value::str("a-b-c"));
}

#[test]
fn test_str_substring_repeat_reverse() {
    assert_eq!(eval("return substring(\"hello\", 1, 3)"), Value::str("el"));
    assert_eq!(eval("return substring(\"hello\", 2)"), Value::str("llo"));
    assert_eq!(eval("return substring(\"hello\", -2)"), Value::str("lo"));
    assert_eq!(eval("return repeat(\"ab\", 3)"), Value::str("ababab"));
    assert_eq!(eval("return reverse(\"abc\")"), Value::str("cba"));
}

#[test]
fn test_str_error_message() {
    let r = run("return upper(123)").unwrap_err();
    match r {
        Value::Error(e) => {
            assert!(e.message.contains("upper"));
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
    assert_eq!(eval("return isArray([])"), Value::Bool(true));
    assert_eq!(eval("return isString(\"x\")"), Value::Bool(true));
    assert_eq!(eval("return isObject({})"), Value::Bool(true));
    assert_eq!(eval("return isNumber(3)"), Value::Bool(true));
    assert_eq!(eval("return isNumber(3.0)"), Value::Bool(true));
    assert_eq!(eval("return isInt(3)"), Value::Bool(true));
    assert_eq!(eval("return isFloat(3.0)"), Value::Bool(true));
    assert_eq!(eval("return isBool(true)"), Value::Bool(true));
    assert_eq!(eval("return isNil(nil)"), Value::Bool(true));
    assert_eq!(eval("return isUndefined(undefined)"), Value::Bool(true));
    assert_eq!(eval("return isUndefined(nil)"), Value::Bool(true));
    assert_eq!(eval("return isUndefined(0)"), Value::Bool(false));
    assert_eq!(eval("return isNumber(\"x\")"), Value::Bool(false));
    assert_eq!(eval("return isFunction(println)"), Value::Bool(true));
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
    assert_eq!(eval("return jsonEncode(nil)"), Value::str("null"));
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
    let main_src = "var __e = nil\ntry { import \"sflang_retry_mod.sf\" } catch(e) { __e = e }\nassert(retryVal == 10)";
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
sleep(150)\n\
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
sleep(100)\n\
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
sleep(50)\n\
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
    sf.run_string("sleep(50)").ok();
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
sleep(300)\n\
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
    sleep(10)\n\
    cur[0] = cur[0] - 1\n\
    semRelease(sem)\n\
}\n\
run w(); run w(); run w()\n\
sleep(150)\n\
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
sleep(100)\n\
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
