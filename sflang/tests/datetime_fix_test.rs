//! datetime.rs / builtins_time.rs 已确认 bug 的回归测试
//!
//! 覆盖：
//!   1. 1970 前时间戳（负 Unix 毫秒）的各字段换算（div_euclid/rem_euclid 修复）
//!   2. datetimeParse 中文等多字节字面字符不 panic 且正确解析
//!   3. 带符号时区（±HHMM）的格式化/解析往返
//!   4. 毫秒布局（.000/.999）的格式化/解析往返
//!   5. 解析后输入未完全消费时报错（提示含剩余内容）
//!   6. dtAddDays/dtAddSeconds/dtAddMillis/timeAddDate 巨大参数溢出返回 error
//!   7. from_components 年份越界（0 / 10000）返回 error

use sflang::Sflang;
use sflang::value::Value;

/// eval 求值代码并返回结果（用 IIFE 包裹，src 内需显式 return）。
fn eval(src: &str) -> Value {
    let mut sf = Sflang::new();
    let wrapped = format!("func __f() {{ {} }} var __r = __f()", src);
    sf.run_string(&wrapped).expect("eval failed");
    sf.get_global("__r").expect("__r not set")
}

/// run 执行代码，返回结果或错误（用于校验 error 路径）。
fn run(src: &str) -> Result<Value, Value> {
    let mut sf = Sflang::new();
    sf.run_string(src)
}

// ---- 1. 1970 前时间戳组件 ----

#[test]
fn test_pre_epoch_datetime_components() {
    // datetime(-1) = 1969-12-31 23:59:59.999 UTC
    // 修复前：截断除法得到 "00:00:00.-01"（毫秒为 -1、各组件错乱）
    assert_eq!(
        eval(r#"return dtFormat(datetime(-1), "2006-01-02 15:04:05.000")"#),
        Value::str("1969-12-31 23:59:59.999"),
    );
    assert_eq!(eval("return datetime(-1).year"), Value::Int(1969));
    assert_eq!(eval("return datetime(-1).month"), Value::Int(12));
    assert_eq!(eval("return datetime(-1).day"), Value::Int(31));
    assert_eq!(eval("return datetime(-1).hour"), Value::Int(23));
    assert_eq!(eval("return datetime(-1).minute"), Value::Int(59));
    assert_eq!(eval("return datetime(-1).second"), Value::Int(59));
    assert_eq!(eval("return datetime(-1).millis"), Value::Int(999));
    assert_eq!(eval("return datetime(-1).weekday"), Value::Int(3)); // 1969-12-31 周三
    // 整天负值：-86400000 = 1969-12-31 00:00:00.000
    assert_eq!(
        eval(r#"return dtFormat(datetime(-86400000), "2006-01-02 15:04:05.000")"#),
        Value::str("1969-12-31 00:00:00.000"),
    );
    // -1ms 加 1ms 回到 epoch
    assert_eq!(eval("return dtToMillis(dtAddMillis(datetime(-1), 1))"), Value::Int(0));
}

// ---- 2. 多字节字面字符布局 ----

#[test]
fn test_parse_multibyte_literal_layout() {
    // 修复前：字面字符按字节推进，含"年"等中文时触发 char boundary panic
    assert_eq!(
        eval(r#"return dtFormat(datetimeParse("2024年06月15日", "2006年01月02日"), "2006-01-02")"#),
        Value::str("2024-06-15"),
    );
    // 字面字符不匹配返回 error（而非 panic）
    assert!(run(r#"return datetimeParse("2024X06月15日", "2006年01月02日")"#).is_err());
}

// ---- 3. 带符号时区往返 ----

#[test]
fn test_signed_tz_roundtrip() {
    // +0530：格式化 → 解析 → 再格式化 全程一致
    let src = r#"
        var layout = "2006-01-02 15:04:05 -0700"
        var dt = datetime(2024, 6, 15, 14, 30, 45, 330)
        var s = dtFormat(dt, layout)
        var dt2 = datetimeParse(s, layout)
        return [s, dtToMillis(dt) == dtToMillis(dt2), dtFormat(dt2, layout) == s]
    "#;
    match eval(src) {
        Value::Array(a) => {
            let g = a.lock().unwrap();
            assert_eq!(g[0], Value::str("2024-06-15 14:30:45 +0530"));
            assert_eq!(g[1], Value::Bool(true));
            assert_eq!(g[2], Value::Bool(true));
        }
        _ => panic!("expected array"),
    }
    // -0500：修复前把 HHMM 整数直接当分钟且只读 2 位，往返必然失败
    let src2 = r#"
        var layout = "2006-01-02 15:04:05 -0700"
        var dt = datetime(2024, 6, 15, 14, 30, 45, -300)
        var s = dtFormat(dt, layout)
        var dt2 = datetimeParse(s, layout)
        return [s, dtToMillis(dt) == dtToMillis(dt2)]
    "#;
    match eval(src2) {
        Value::Array(a) => {
            let g = a.lock().unwrap();
            assert_eq!(g[0], Value::str("2024-06-15 14:30:45 -0500"));
            assert_eq!(g[1], Value::Bool(true));
        }
        _ => panic!("expected array"),
    }
    // 时区正确换算为 UTC：+0530 的本地 00:00 = 前一日 18:30 UTC
    assert_eq!(
        eval(r#"return dtToMillis(datetimeParse("2024-06-15 00:00:00 +0530", "2006-01-02 15:04:05 -0700")) == dtToMillis(datetime(2024, 6, 14, 18, 30, 0))"#),
        Value::Bool(true),
    );
    // -0500 的本地 12:00 = 同日 17:00 UTC
    assert_eq!(
        eval(r#"return dtToMillis(datetimeParse("2024-06-15 12:00:00 -0500", "2006-01-02 15:04:05 -0700")) == dtToMillis(datetime(2024, 6, 15, 17, 0, 0))"#),
        Value::Bool(true),
    );
}

// ---- 4. 毫秒布局往返 ----

#[test]
fn test_millis_layout_roundtrip() {
    // .000 固定 3 位（修复前未消费输入中的 '.'，往返错位）。
    // 注：datetimeFromMillis 现为本地时区，此处用 datetime(ms)（显式 UTC）
    // 保证布局往返测试与时区环境无关。
    let src = r#"
        var layout = "2006-01-02 15:04:05.000"
        var dt = datetime(1704067200123)
        var s = dtFormat(dt, layout)
        var dt2 = datetimeParse(s, layout)
        return [s, dtToMillis(dt2)]
    "#;
    match eval(src) {
        Value::Array(a) => {
            let g = a.lock().unwrap();
            assert_eq!(g[0], Value::str("2024-01-01 00:00:00.123"));
            assert_eq!(g[1], Value::Int(1704067200123));
        }
        _ => panic!("expected array"),
    }
    // .999 去尾零：".12" 解析为 120 毫秒（位数不足右侧补零）
    assert_eq!(
        eval(r#"return dtFormat(datetime(120), "2006-01-02 15:04:05.999")"#),
        Value::str("1970-01-01 00:00:00.12"),
    );
    assert_eq!(
        eval(r#"return dtToMillis(datetimeParse("1970-01-01 00:00:00.12", "2006-01-02 15:04:05.999"))"#),
        Value::Int(120),
    );
    // .999 毫秒为 0 时整体省略，解析须容忍无 '.' 输入
    assert_eq!(
        eval(r#"return dtToMillis(datetimeParse("1970-01-01 00:00:00", "2006-01-02 15:04:05.999"))"#),
        Value::Int(0),
    );
    // datetimeFromMillis（本地时区）经 -0700 占位符格式化+解析，时刻应严格不变：
    // 格式化输出自带本地偏移量，解析按该偏移还原，任意时区环境均成立。
    let src3 = r#"
        var layout = "2006-01-02 15:04:05.000 -0700"
        var dt = datetimeFromMillis(1704067200123)
        return dtToMillis(datetimeParse(dtFormat(dt, layout), layout)) == 1704067200123
    "#;
    assert_eq!(eval(src3), Value::Bool(true));
    // 完整布局 "2006-01-02 15:04:05.000 -0700" 往返
    let src2 = r#"
        var layout = "2006-01-02 15:04:05.000 -0700"
        var dt = datetime(1704067200123)
        var s = dtFormat(dt, layout)
        return [s, dtToMillis(datetimeParse(s, layout)) == dtToMillis(dt)]
    "#;
    match eval(src2) {
        Value::Array(a) => {
            let g = a.lock().unwrap();
            assert_eq!(g[0], Value::str("2024-01-01 00:00:00.123 +0000"));
            assert_eq!(g[1], Value::Bool(true));
        }
        _ => panic!("expected array"),
    }
}

// ---- 5. 多余字符报错 ----

#[test]
fn test_parse_extra_input_error() {
    let r = run(r#"return datetimeParse("2024-06-15xyz", "2006-01-02")"#);
    assert!(r.is_err(), "多余字符应返回 error");
    match r.unwrap_err() {
        Value::Error(er) => assert!(er.message.contains("xyz"), "错误信息应包含剩余内容: {}", er.message),
        _ => panic!("expected Error"),
    }
    // 布局只到日，输入带时间同样算多余
    assert!(run(r#"return datetimeParse("2024-06-15 12:00:00", "2006-01-02")"#).is_err());
    // 完全消费的输入正常
    assert_eq!(
        eval(r#"return dtFormat(datetimeParse("2024-06-15", "2006-01-02"), "2006-01-02")"#),
        Value::str("2024-06-15"),
    );
}

// ---- 6. 加法溢出返回 error ----

#[test]
fn test_dt_add_overflow_returns_error() {
    // i64::MAX 级别参数：修复前直接 panic（溢出），现在返回 error
    assert!(run("return dtAddDays(datetime(2024,1,1), 9223372036854775807)").is_err());
    assert!(run("return dtAddSeconds(datetime(2024,1,1), 9223372036854775807)").is_err());
    assert!(run("return dtAddMillis(datetime(2024,1,1), 9223372036854775807)").is_err());
    assert!(run("return timeAddDate(datetime(2024,1,1), 0, 0, 9223372036854775807)").is_err());
    // 错误信息包含溢出提示（便于 AI 定位）
    match run("return dtAddDays(datetime(2024,1,1), 9223372036854775807)").unwrap_err() {
        Value::Error(er) => assert!(er.message.contains("溢出"), "msg: {}", er.message),
        _ => panic!("expected Error"),
    }
    // 正常值不受影响
    assert_eq!(
        eval(r#"return dtFormat(dtAddDays(datetime(2024,1,1), 31), "2006-01-02")"#),
        Value::str("2024-02-01"),
    );
}

// ---- 7. 年份越界 ----

#[test]
fn test_year_out_of_range_error() {
    // 年份限 1-9999
    assert!(run("return datetime(0, 1, 1)").is_err());
    assert!(run("return datetime(10000, 1, 1)").is_err());
    assert!(run("return datetime(10000, 1, 1, 12, 0, 0)").is_err());
    assert!(run(r#"return datetimeParse("0000-01-01", "2006-01-02")"#).is_err());
    // 边界值 1 与 9999 合法
    assert_eq!(eval("return datetime(1, 1, 1).year"), Value::Int(1));
    assert_eq!(eval("return datetime(9999, 12, 31).year"), Value::Int(9999));
}

// ---- 8. getNowStr / nowDT 本地时区 ----

/// tz_hhmm 以时区偏移分钟数推导 Go 风格 +HHMM/-HHMM 字符串（如 480 → "+0800"）。
fn tz_hhmm(offset_min: i32) -> String {
    let sign = if offset_min >= 0 { '+' } else { '-' };
    let abs = offset_min.unsigned_abs();
    format!("{}{:02}{:02}", sign, abs / 60, abs % 60)
}

#[test]
fn test_getnowstr_and_nowdt_use_local_timezone() {
    // 历史问题：文档曾宣称 getNowStr/nowDT 恒为 UTC（本地时区偏移未实现）。
    // 实际 DateTime::now() 已接入 local_tz_offset_minutes，此测试锁定该行为，
    // 且断言方式与运行机器的时区无关。
    let offset = sflang::datetime::local_tz_offset_minutes();
    let expected_tz = tz_hhmm(offset);

    // getNowStr 带 -0700 布局应输出本机时区偏移（如 +0800），而非恒 +0000
    let s = eval(r#"return getNowStr("2006-01-02 -0700")"#).to_str();
    let tail = &s[s.len() - 5..];
    assert_eq!(tail, expected_tz, "getNowStr 应输出本地时区偏移 {}", expected_tz);

    // nowDT 的 tz_offset 应与系统时区偏移一致
    match eval("return nowDT()") {
        Value::DateTime(dt) => assert_eq!(dt.tz_offset, offset, "nowDT 的 tz_offset 应取系统时区"),
        other => panic!("nowDT() 应返回 datetime，得到 {:?}", other.type_name()),
    }
}
