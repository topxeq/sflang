//! builtins_helpers.rs — 内置函数参数校验微工具
//!
//! 设计要点（来自 AGENTS.md）：
//!   - 错误信息 AI 友好：包含函数名、期望类型、实际类型、可能原因
//!   - 消除各内置函数重复的参数校验样板代码
//!
//! 这些函数返回 `Result<..., Value>`，其中 `Err` 是 `error_value(...)`，
//! 可被内置函数用 `?` 直接传播。

use crate::value::{error_value, Value};
use crate::value::TypeCode;

/// err_type 构造"类型不符"错误值（AI 友好）。
///
/// 统一格式：`<fn_name>() 第 <idx+1> 个参数应为 <expect>，得到 <actual> (可能原因：<hint>)`
pub fn err_type(fn_name: &str, idx: usize, expect: &str, actual: TypeCode, hint: &str) -> Value {
    error_value(format!(
        "{}() 第 {} 个参数应为 {}，得到 {} (可能原因：{})",
        fn_name,
        idx + 1,
        expect,
        actual.name(),
        hint,
    ))
}

/// err_argc 构造"参数个数不足"错误值（AI 友好）。
pub fn err_argc(fn_name: &str, expect_min: usize, actual: usize) -> Value {
    error_value(format!(
        "{}() 需要至少 {} 个参数，实际 {} 个 (可能原因：参数缺失或括号未闭合)",
        fn_name, expect_min, actual,
    ))
}

/// require_arg 要求至少有 `idx+1` 个参数，否则返回错误值。
pub fn require_arg(args: &[Value], idx: usize, fn_name: &str) -> Result<(), Value> {
    if args.len() <= idx {
        Err(err_argc(fn_name, idx + 1, args.len()))
    } else {
        Ok(())
    }
}

/// as_str 取第 idx 个参数为字符串引用。
///
/// 失败时返回类型错误（期望 string）。
pub fn as_str<'a>(args: &'a [Value], idx: usize, fn_name: &str) -> Result<&'a str, Value> {
    require_arg(args, idx, fn_name)?;
    match &args[idx] {
        Value::Str(s) => Ok(s.as_ref()),
        v => Err(err_type(
            fn_name,
            idx,
            "string",
            v.type_code(),
            "参数顺序错误或忘记 string() 转换",
        )),
    }
}

/// as_int 取第 idx 个参数为 i64（Int 直接返回，Float 截断）。
pub fn as_int(args: &[Value], idx: usize, fn_name: &str) -> Result<i64, Value> {
    require_arg(args, idx, fn_name)?;
    let v = &args[idx];
    match v {
        Value::Int(i) => Ok(*i),
        Value::Float(f) => Ok(*f as i64),
        _ => Err(err_type(
            fn_name,
            idx,
            "int",
            v.type_code(),
            "需为整数（浮点会被截断）",
        )),
    }
}

/// as_float 取第 idx 个参数为 f64（Int 自动提升）。
pub fn as_float(args: &[Value], idx: usize, fn_name: &str) -> Result<f64, Value> {
    require_arg(args, idx, fn_name)?;
    let v = &args[idx];
    match v.to_f64() {
        Some(f) => Ok(f),
        None => Err(err_type(
            fn_name,
            idx,
            "number",
            v.type_code(),
            "需为 int 或 float",
        )),
    }
}

/// as_array 取第 idx 个参数为数组的 Arc<Mutex<Vec<Value>>> 引用。
pub fn as_array<'a>(
    args: &'a [Value],
    idx: usize,
    fn_name: &str,
) -> Result<&'a std::sync::Arc<std::sync::Mutex<Vec<Value>>>, Value> {
    require_arg(args, idx, fn_name)?;
    match &args[idx] {
        Value::Array(a) => Ok(a),
        v => Err(err_type(
            fn_name,
            idx,
            "array",
            v.type_code(),
            "用 [] 字面量或 range() 创建数组",
        )),
    }
}

/// as_object 取第 idx 个参数为 Map 的 Arc<Mutex<Map>> 引用。
pub fn as_object<'a>(
    args: &'a [Value],
    idx: usize,
    fn_name: &str,
) -> Result<&'a std::sync::Arc<std::sync::Mutex<crate::object_map::Map>>, Value> {
    require_arg(args, idx, fn_name)?;
    match &args[idx] {
        Value::Object(o) => Ok(o),
        v => Err(err_type(
            fn_name,
            idx,
            "object",
            v.type_code(),
            "用 {} 字面量创建对象",
        )),
    }
}

/// opt_arg 取可选第 idx 个参数；不存在时返回 None（不报错）。
#[allow(dead_code)]
pub fn opt_arg(args: &[Value], idx: usize) -> Option<&Value> {
    args.get(idx)
}
