//! builtins_bigint.rs — 任意精度数值内置函数（bigInt / bigFloat）
//!
//! 设计要点：
//!   - bigInt/bigFloat 作为 Value 类型变体，可直接用 + - * / 运算符（见 vm.rs arith_op）
//!   - 本模块提供构造、转换、判断、高精度除法等函数
//!   - 字面量暂用构造函数（bigInt("...") / bigFloat("...")），不加重缀语法
//!
//! 函数列表：
//!   构造/转换：
//!     bigInt(v)              — 从 int 或解析 string 构造
//!     bigFloat(v)            — 从 int/float/string 构造（float 会精度受限）
//!     bigFloat(s, scale)     — 指定 scale 构造
//!     toBigInt(x)            — int/bigInt 转 bigInt
//!     toBigFloat(x)          — int/bigInt/bigFloat 转 bigFloat
//!   判断：
//!     isBigInt(x) / isBigFloat(x)
//!   高精度除法：
//!     bigFloatDiv(a, b, prec) — 指定结果小数位数的除法

use std::sync::Arc;

use crate::bigint::BigInt;
use crate::bigfloat::BigFloat;
use crate::value::Value;
use crate::vm::VM;

/// register 注册所有大数内置函数到 VM。
pub fn register(vm: &mut VM) {
    vm.register_builtin("bigInt", bi_big_int);
    vm.register_builtin("bigFloat", bi_big_float);
    vm.register_builtin("toBigInt", bi_to_big_int);
    vm.register_builtin("toBigFloat", bi_to_big_float);
    vm.register_builtin("isBigInt", bi_is_big_int);
    vm.register_builtin("isBigFloat", bi_is_big_float);
    vm.register_builtin("bigFloatDiv", bi_big_float_div);
}

/// bi_big_int 构造 bigInt。
///
/// 用法：
///   bigInt(int)        — 从 int 提升
///   bigInt(string)     — 解析十进制字符串（支持任意位数）
fn bi_big_int(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    crate::builtins_helpers::require_arg(args, 0, "bigInt")?;
    let bi = match &args[0] {
        Value::Int(x) => BigInt::from_i64(*x),
        Value::BigInt(b) => (**b).clone(),
        Value::Str(s) => BigInt::from_str_decimal(s).map_err(crate::value::error_value)?,
        Value::Float(f) => BigInt::from_i64(*f as i64),
        v => return Err(crate::value::error_value(format!(
            "bigInt() 不支持类型 {} (可能原因：参数应为 int/string/bigInt)", v.type_name(),
        ))),
    };
    // 构造函数：始终返回 BigInt 类型（用户显式要求 bigInt，不降级）
    Ok(Value::BigInt(Arc::new(bi)))
}

/// bi_big_float 构造 bigFloat。
///
/// 用法：
///   bigFloat(int)      — 整数转 bigFloat（scale=0）
///   bigFloat(bigInt)   — bigInt 转 bigFloat
///   bigFloat(string)   — 解析十进制（如 "3.14"）
///   bigFloat(string, scale) — 指定小数位数（此时字符串应为纯整数，自动补小数点）
fn bi_big_float(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    crate::builtins_helpers::require_arg(args, 0, "bigFloat")?;
    let bf = match &args[0] {
        Value::Int(x) => BigFloat::from_i64(*x),
        Value::BigInt(b) => BigFloat::from_bigint((**b).clone()),
        Value::BigFloat(b) => (**b).clone(),
        Value::Str(s) => {
            if args.len() >= 2 {
                // 指定 scale：字符串视为纯整数尾数
                let scale = crate::builtins_helpers::as_int(args, 1, "bigFloat")? as u32;
                let mantissa = BigInt::from_str_decimal(s).map_err(crate::value::error_value)?;
                BigFloat { mantissa, scale }
            } else {
                BigFloat::from_str_decimal(s).map_err(crate::value::error_value)?
            }
        }
        Value::Float(f) => {
            // float → bigFloat：用字符串中转（尽量保留十进制）
            BigFloat::from_str_decimal(&format!("{}", f)).map_err(crate::value::error_value)?
        }
        v => return Err(crate::value::error_value(format!(
            "bigFloat() 不支持类型 {} (可能原因：参数应为 int/string/bigInt/bigFloat)", v.type_name(),
        ))),
    };
    Ok(Value::BigFloat(Arc::new(bf)))
}

/// bi_to_big_int 转为 bigInt（始终返回 BigInt 类型，不降级为 int）。
fn bi_to_big_int(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    crate::builtins_helpers::require_arg(args, 0, "toBigInt")?;
    let bi = match &args[0] {
        Value::Int(x) => BigInt::from_i64(*x),
        Value::BigInt(b) => (**b).clone(),
        Value::Str(s) => BigInt::from_str_decimal(s).map_err(crate::value::error_value)?,
        v => return Err(crate::value::error_value(format!(
            "toBigInt() 不支持类型 {} (可能原因：参数应为 int/string/bigInt)", v.type_name(),
        ))),
    };
    Ok(Value::BigInt(Arc::new(bi)))
}

/// bi_to_big_float 转为 bigFloat。
fn bi_to_big_float(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    crate::builtins_helpers::require_arg(args, 0, "toBigFloat")?;
    let bf = match &args[0] {
        Value::Int(x) => BigFloat::from_i64(*x),
        Value::BigInt(b) => BigFloat::from_bigint((**b).clone()),
        Value::BigFloat(b) => (**b).clone(),
        Value::Str(s) => BigFloat::from_str_decimal(s).map_err(crate::value::error_value)?,
        Value::Float(f) => BigFloat::from_str_decimal(&format!("{}", f)).map_err(crate::value::error_value)?,
        v => return Err(crate::value::error_value(format!(
            "toBigFloat() 不支持类型 {}", v.type_name(),
        ))),
    };
    Ok(Value::BigFloat(Arc::new(bf)))
}

/// bi_is_big_int 判断是否为 bigInt。
fn bi_is_big_int(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(args.get(0), Some(Value::BigInt(_)))))
}

/// bi_is_big_float 判断是否为 bigFloat。
fn bi_is_big_float(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(args.get(0), Some(Value::BigFloat(_)))))
}

/// bi_big_float_div 高精度除法：a / b，保留 prec 位小数。
///
/// 用法：bigFloatDiv(a, b) 或 bigFloatDiv(a, b, prec)（默认 20 位）
fn bi_big_float_div(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    crate::builtins_helpers::require_arg(args, 0, "bigFloatDiv")?;
    crate::builtins_helpers::require_arg(args, 1, "bigFloatDiv")?;
    let prec = if args.len() >= 3 {
        crate::builtins_helpers::as_int(args, 2, "bigFloatDiv")? as u32
    } else {
        20
    };
    let a = value_to_bigfloat(&args[0])?;
    let b = value_to_bigfloat(&args[1])?;
    let r = a.div(&b, prec).map_err(crate::value::error_value)?;
    Ok(Value::BigFloat(Arc::new(r)))
}

/// value_to_bigfloat 将数值类 Value 转为 BigFloat（内部辅助）。
fn value_to_bigfloat(v: &Value) -> Result<BigFloat, Value> {
    match v {
        Value::Int(x) => Ok(BigFloat::from_i64(*x)),
        Value::BigInt(b) => Ok(BigFloat::from_bigint((**b).clone())),
        Value::BigFloat(b) => Ok((**b).clone()),
        Value::Float(f) => BigFloat::from_str_decimal(&format!("{}", f)).map_err(crate::value::error_value),
        _ => Err(crate::value::error_value(format!(
            "需要数值类型 (int/bigInt/bigFloat)，得到 {}", v.type_name(),
        ))),
    }
}
