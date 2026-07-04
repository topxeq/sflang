//! builtins_hash.rs — 哈希内置函数（基于自实现的 hash.rs）
//!
//! 函数列表：
//!   md5(b)     — MD5 哈希（16 字节 → bytes）
//!   sha1(b)    — SHA-1 哈希（20 字节 → bytes）
//!   sha256(b)  — SHA-256 哈希（32 字节 → bytes）
//!   md5Hex(b)  — MD5 哈希的十六进制字符串
//!   sha256Hex(b) — SHA-256 哈希的十六进制字符串

use std::sync::Arc;

use crate::builtins_helpers as bh;
use crate::value::Value;
use crate::vm::VM;

pub fn register(vm: &mut VM) {
    vm.register_builtin("md5", bi_md5);
    vm.register_builtin("sha1", bi_sha1);
    vm.register_builtin("sha256", bi_sha256);
    vm.register_builtin("md5Hex", bi_md5_hex);
    vm.register_builtin("sha256Hex", bi_sha256_hex);
}

/// to_bytes 将参数转为字节 Vec。
fn to_bytes(v: &Value) -> Result<Vec<u8>, Value> {
    match v {
        Value::Str(s) => Ok(s.as_bytes().to_vec()),
        Value::Bytes(b) => Ok(b.as_ref().to_vec()),
        Value::ByteArray(b) => Ok(b.lock().unwrap().clone()),
        _ => Err(crate::value::error_value(format!(
            "需要 string/bytes/byteArray，得到 {}", v.type_name(),
        ))),
    }
}

fn bi_md5(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "md5")?;
    let data = to_bytes(&args[0])?;
    Ok(Value::Bytes(Arc::new(crate::hash::md5(&data))))
}

fn bi_sha1(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "sha1")?;
    let data = to_bytes(&args[0])?;
    Ok(Value::Bytes(Arc::new(crate::hash::sha1(&data))))
}

fn bi_sha256(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "sha256")?;
    let data = to_bytes(&args[0])?;
    Ok(Value::Bytes(Arc::new(crate::hash::sha256(&data))))
}

fn bi_md5_hex(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "md5Hex")?;
    let data = to_bytes(&args[0])?;
    let hex: String = crate::hash::md5(&data).iter().map(|b| format!("{:02x}", b)).collect();
    Ok(Value::str_from(hex))
}

fn bi_sha256_hex(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "sha256Hex")?;
    let data = to_bytes(&args[0])?;
    let hex: String = crate::hash::sha256(&data).iter().map(|b| format!("{:02x}", b)).collect();
    Ok(Value::str_from(hex))
}
