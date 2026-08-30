//! builtins.rs 已确认 bug 修复的回归测试。
//!
//! 覆盖：
//!   - uuid() 随机种子（进程内/跨 VM 实例不重复，v4 格式）
//!   - adjustFloat/toKMG 负精度/超大精度返回 error 而非 panic
//!   - sleep(inf/NaN/超大) 返回 error
//!   - range() 溢出不再死循环 + 元素数量上限
//!   - deepClone 支持 map、保留 object 原型链、深度上限
//!   - values() 支持 map、array 返回快照拷贝
//!   - getParam 负索引返回 error
//!   - sprintf 巨大 width / randomStr 巨大 n 返回 error

use sflang::value::Value;
use sflang::Sflang;

// ---- 辅助函数（用法与 tests/api_test.rs 一致）----

/// eval 求值代码块并返回结果（用 IIFE 包裹，src 内需显式 return）。
fn eval(src: &str) -> Value {
    let mut sf = Sflang::new();
    let wrapped = format!("func __f() {{ {} }} var __r = __f()", src);
    sf.run_string(&wrapped).expect("eval failed");
    sf.get_global("__r").expect("__r not set")
}

/// run 执行代码，返回运行结果（错误时为 Err）。
fn run(src: &str) -> Result<Value, Value> {
    let mut sf = Sflang::new();
    sf.run_string(src)
}

/// err_msg 取出错误信息字符串（断言辅助）。
fn err_msg(r: Result<Value, Value>) -> String {
    match r {
        Err(Value::Error(e)) => e.message.clone(),
        Err(other) => panic!("expected Value::Error, got {:?}", other),
        Ok(v) => panic!("expected error, got ok value: {}", v.inspect()),
    }
}

// ---- uuid() 随机种子修复 ----

#[test]
fn test_uuid_unique_and_v4_format() {
    // 同一 VM 内两次调用不同
    let a = eval("return uuid()").to_str();
    let b = eval("return uuid()").to_str();
    assert_ne!(a, b, "同一进程内两次 uuid() 不应重复: {} vs {}", a, b);

    // 36 字符，v4 版本位（第 13 个十六进制位 = '4'），RFC 4122 变体位（第 17 位为 8/9/a/b）
    let s = a.as_str();
    assert_eq!(s.len(), 36, "uuid 应为 36 字符: {}", s);
    let parts: Vec<&str> = s.split('-').collect();
    assert_eq!(
        parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
        vec![8, 4, 4, 4, 12],
        "uuid 分段应为 8-4-4-4-12: {}",
        s
    );
    assert_eq!(&s[14..15], "4", "uuid 版本位应为 4: {}", s);
    assert!(
        matches!(&s[19..20], "8" | "9" | "a" | "b"),
        "uuid 变体位应为 8/9/a/b: {}",
        s
    );
}

#[test]
fn test_uuid_differs_across_vm_instances() {
    // 跨 VM 实例（独立的 Sflang 对象）首值也应不同（进程级计数器持续推进）
    let mut sf1 = Sflang::new();
    let u1 = sf1.run_string("return uuid()").unwrap().to_str();
    let mut sf2 = Sflang::new();
    let u2 = sf2.run_string("return uuid()").unwrap().to_str();
    assert_ne!(u1, u2, "跨 VM 实例的 uuid() 不应相同: {} vs {}", u1, u2);

    // 同一实例内连续多次调用也互不相同
    let r = sf1
        .run_string(r#"return [uuid(), uuid(), uuid(), uuid()]"#)
        .unwrap();
    let mut seen: Vec<String> = Vec::new();
    if let Value::Array(a) = r {
        for v in a.lock().unwrap().iter() {
            let s = v.to_str();
            assert!(!seen.contains(&s), "uuid 重复: {}", s);
            seen.push(s);
        }
    } else {
        panic!("expected Array");
    }
}

// ---- adjustFloat / toKMG 精度校验 ----

#[test]
fn test_adjust_float_precision_bounds() {
    // 负精度：修复前 as usize 回绕成巨大值导致 format 分配超大缓冲（OOM/panic）
    let r = run("return adjustFloat(1.5, -1)");
    let msg = err_msg(r);
    assert!(msg.contains("0..1000"), "错误信息应含范围提示: {}", msg);

    // 超大精度同样返回 error
    let r = run("return adjustFloat(1.5, 2000)");
    assert!(r.is_err(), "精度 2000 应返回 error");

    // 正常用法不受影响
    assert_eq!(eval("return adjustFloat(3.14159, 2)"), Value::Float(3.14));
    assert_eq!(eval("return adjustFloat(0.1 + 0.2)"), Value::Float(0.3));
    // 边界值合法
    assert!(run("return adjustFloat(1.5, 1000)").is_ok());
}

#[test]
fn test_to_kmg_precision_bounds() {
    // toKMG 同病：负小数位返回 error 而非 panic
    let r = run("return toKMG(1536, -1)");
    let msg = err_msg(r);
    assert!(msg.contains("0..1000"), "错误信息应含范围提示: {}", msg);

    let r = run("return toKMG(1536, 5000)");
    assert!(r.is_err(), "小数位 5000 应返回 error");

    // 正常用法不受影响
    assert_eq!(eval("return toKMG(1536)"), Value::str("1.50K"));
    assert_eq!(eval("return toKMG(1048576, 1)"), Value::str("1.0M"));
}

// ---- sleep 时长校验 ----

#[test]
fn test_sleep_invalid_duration_returns_error() {
    // inf：修复前 Duration::from_secs_f64(inf) 直接 panic
    let r = run("return sleep(float(\"inf\"))");
    let msg = err_msg(r);
    assert!(msg.contains("非法") || msg.contains("31536000"), "错误信息: {}", msg);

    // NaN / 负数 / 超过一年上限
    assert!(run("return sleep(float(\"NaN\"))").is_err());
    assert!(run("return sleep(-0.5)").is_err());
    assert!(run("return sleep(1e300)").is_err());

    // 合法时长不受影响（0 秒立即返回）
    assert_eq!(run("return sleep(0)").unwrap(), Value::Undefined);
}

// ---- range() 溢出与数量上限 ----

#[test]
fn test_range_overflow_no_infinite_loop() {
    // 正向上溢：i64::MAX 附近 + step，checked_add 溢出应安全 break（不再死循环/OOM）
    let r = run("var a = range(9223372036854775800, 9223372036854775807); return len(a)");
    assert_eq!(r.unwrap(), Value::Int(7));

    // 负向下溢：i64::MIN 附近 - step，同样安全 break
    let r = run("var a = range(-9223372036854775805, -9223372036854775807, -5); return len(a)");
    assert_eq!(r.unwrap(), Value::Int(1));

    // 常规用法不受影响
    assert_eq!(eval("return len(range(1, 10))"), Value::Int(9));
    assert_eq!(eval("return len(range(5, 0, -1))"), Value::Int(5));
}

#[test]
fn test_range_element_limit() {
    // 元素数量超过 100 万上限返回 error
    let r = run("return range(0, 1000001)");
    let msg = err_msg(r);
    assert!(
        msg.contains("元素过多") || msg.contains("1000000"),
        "错误信息应含上限提示: {}",
        msg
    );

    // 恰好在上限内（100 万）可用
    assert_eq!(
        run("var a = range(0, 1000000); return len(a)").unwrap(),
        Value::Int(1_000_000)
    );
}

// ---- deepClone 修复 ----

#[test]
fn test_deep_clone_map_isolation() {
    // map 深拷贝：修改副本的嵌套 map 不影响原 map
    // 修复前只克隆 Arc 指针，"副本"与原 map 共享底层数据
    let src = r#"
        var m = map{"a": map{"x": 1}}
        var c = deepClone(m)
        c["a"]["x"] = 99
        return [m["a"]["x"], c["a"]["x"]]
    "#;
    let r = eval(src);
    match r {
        Value::Array(a) => {
            let arr = a.lock().unwrap();
            assert_eq!(arr[0], Value::Int(1), "原 map 不应被修改");
            assert_eq!(arr[1], Value::Int(99), "副本应已修改");
        }
        _ => panic!("expected Array"),
    }
}

#[test]
fn test_deep_clone_preserves_proto() {
    // 深拷贝保留 object 原型链：克隆体仍能调用原型方法
    // 修复前用 Map::new() 重建导致原型方法丢失
    let src = r#"
        var proto = {greet: func(self) { return "hi " + self.name }}
        var obj = newObject(proto)
        obj.name = "bob"
        var c = deepClone(obj)
        return [c.greet(), c.name]
    "#;
    let r = eval(src);
    match r {
        Value::Array(a) => {
            let arr = a.lock().unwrap();
            assert_eq!(arr[0], Value::str("hi bob"), "克隆体应能调用原型方法");
            assert_eq!(arr[1], Value::str("bob"), "自身成员应被克隆");
        }
        _ => panic!("expected Array"),
    }
}

#[test]
fn test_deep_clone_depth_limit() {
    // 构造 250 层嵌套 map，超过 200 层深度上限应返回 error（不栈溢出）
    let src = r#"
        var root = map{}
        var cur = root
        for i in range(0, 250) {
            cur["v"] = map{}
            cur = cur["v"]
        }
        return deepClone(root)
    "#;
    let r = run(src);
    let msg = err_msg(r);
    assert!(
        msg.contains("嵌套深度") || msg.contains("200"),
        "错误信息应含深度提示: {}",
        msg
    );

    // 正常深度的克隆不受影响（object 嵌套 map，map 用索引访问）
    let r = eval(
        r#"
        var o = {inner: map{n: [1, 2, map{deep: true}]}}
        var c = deepClone(o)
        return c.inner["n"][2]["deep"]
    "#,
    );
    assert_eq!(r, Value::Bool(true));
}

// ---- values() 支持 map / array 快照 ----

#[test]
fn test_values_supports_map() {
    // 有序 map：返回值的快照数组
    assert_eq!(eval("return len(values(map{\"a\":1, \"b\":2}))"), Value::Int(2));
    assert_eq!(eval("return values(map{\"a\": 7})[0]"), Value::Int(7));

    // object 用法保持不变
    assert_eq!(eval("return len(values({\"a\":1, \"b\":2}))"), Value::Int(2));

    // 非法类型错误信息与文档统一（object 或 map）
    let msg = err_msg(run("return values(42)"));
    assert!(msg.contains("object 或 map"), "错误信息: {}", msg);
}

#[test]
fn test_values_array_returns_snapshot() {
    // 修复前 values(arr) 只克隆 Arc（别名）：修改返回数组会影响原数组
    let src = r#"
        var a = [1, 2]
        var vs = values(a)
        push(vs, 3)
        return [len(a), len(vs)]
    "#;
    let r = eval(src);
    match r {
        Value::Array(a) => {
            let arr = a.lock().unwrap();
            assert_eq!(arr[0], Value::Int(2), "原数组不应被 push 影响（应为快照）");
            assert_eq!(arr[1], Value::Int(3));
        }
        _ => panic!("expected Array"),
    }
}

// ---- getParam 负索引 ----

#[test]
fn test_get_param_negative_index_returns_error() {
    // 负索引：修复前 as usize 回绕，静默变成"越界返回默认值"
    let r = run("return getParam([\"a\", \"b\"], -1)");
    let msg = err_msg(r);
    assert!(msg.contains("负"), "错误信息应含负数提示: {}", msg);

    // 正常用法不受影响
    assert_eq!(eval("return getParam([\"a\", \"b\"], 0)"), Value::str("a"));
    assert_eq!(
        eval("return getParam([\"a\"], 5, \"d\")"),
        Value::str("d"),
        "越界应返回默认值"
    );
}

// ---- sprintf 巨大 width / randomStr 巨大 n ----

#[test]
fn test_sprintf_huge_width_returns_error() {
    // 修复前 "%2000000d" 会分配 2MB 填充字符串；更大值直接 OOM
    let r = run("return sprintf(\"%2000000d\", 1)");
    let msg = err_msg(r);
    assert!(
        msg.contains("宽度") || msg.contains("1000000"),
        "错误信息应含宽度上限提示: {}",
        msg
    );

    // 正常宽度不受影响
    assert_eq!(eval("return len(sprintf(\"%20d\", 1))"), Value::Int(20));
    assert_eq!(eval("return sprintf(\"%-5s|\", \"ab\")"), Value::str("ab   |"));
}

#[test]
fn test_random_str_huge_length_returns_error() {
    let r = run("return randomStr(2000000)");
    let msg = err_msg(r);
    assert!(
        msg.contains("2000000") || msg.contains("上限"),
        "错误信息应含上限提示: {}",
        msg
    );

    // 正常长度不受影响
    assert_eq!(eval("return len(randomStr(10))"), Value::Int(10));
    assert!(run("return randomStr(1000000)").is_ok(), "恰在上限内应可用");
}
