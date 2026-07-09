//! builtins_bytes.rs — 字节序列内置函数（bytes / byteArray）
//!
//! 设计要点（来自 AGENTS.md 与 byteArray 设计讨论）：
//!   - bytes：不可变字节序列（Arc<Vec<u8>>），用于只读场景（读取、哈希、传输）
//!   - byteArray：可变字节序列（Arc<Mutex<Vec<u8>>>），用于就地修改（按位加密、协议改包）
//!   - 两者转换有拷贝，保证修改互不影响（类似 Python bytes/bytearray、Rust &[u8]/Vec<u8>）
//!   - 仅依赖 Rust 标准库
//!   - 错误信息 AI 友好：附函数名、期望类型、可能原因
//!
//! 函数列表：
//!   构造/转换：
//!     byteArray(n) / byteArray(n, fill)  — 创建 n 字节（默认填 0 或指定值）
//!     bytes(v)                           — 转 bytes（string→UTF8字节；byteArray→拷贝；Array<Int>→字节）
//!     byteArrayFromBytes(b)              — bytes → byteArray（拷贝）
//!     byteArrayFromArray(arr)            — Array<Int> → byteArray
//!     arrayFromByteArray(ba)             — byteArray → Array<Int>（每字节一个 Int）
//!     strFromBytes(b, "utf8"|"latin1"|"hex") — bytes → string（指定解码）
//!   操作：
//!     copy(dst, src) / copy(dst, src, dstStart) — 批量复制（类似 Go copy），返回复制字节数
//!     bytesHex(b)                       — bytes/byteArray → 十六进制字符串
//!     bytesFromHex(s)                   — 十六进制字符串 → bytes

use std::sync::{Arc, Mutex};

use crate::builtins_helpers as bh;
use crate::value::Value;
use crate::vm::VM;

/// register 注册所有字节序列内置函数到 VM。
pub fn register(vm: &mut VM) {
    vm.register_builtin("byteArray", bi_byte_array);
    vm.register_builtin("bytes", bi_bytes);
    vm.register_builtin("byteArrayFromBytes", bi_byte_array_from_bytes);
    vm.register_builtin("byteArrayFromArray", bi_byte_array_from_array);
    vm.register_builtin("arrayFromByteArray", bi_array_from_byte_array);
    vm.register_builtin("strFromBytes", bi_str_from_bytes);
    vm.register_builtin("copy", bi_copy);
    vm.register_builtin("bytesHex", bi_bytes_hex);
    vm.register_builtin("bytesFromHex", bi_bytes_from_hex);
    // hex 别名（对标 Charlang，接受 string/bytes/byteArray）
    vm.register_builtin("hexEncode", bi_hex_encode);
    vm.register_builtin("hexDecode", bi_bytes_from_hex);
    vm.register_builtin("hexToStr", bi_hex_to_str);
}

/// byte_val 将 Int 值转为 u8，越界或非整数返回错误。
fn byte_val(v: &Value, fn_name: &str) -> Result<u8, Value> {
    match v {
        Value::Int(x) => {
            if *x < 0 || *x > 255 {
                return Err(crate::value::error_value(format!(
                    "{}() 字节值超出范围: {} (需 0-255；可能原因：传入了非字节整数)",
                    fn_name, x,
                )));
            }
            Ok(*x as u8)
        }
        _ => Err(crate::value::error_value(format!(
            "{}() 需要 int 字节值 (0-255)，得到 {} (可能原因：类型不匹配)",
            fn_name, v.type_name(),
        ))),
    }
}

/// bi_byte_array 创建可变字节序列。
///
/// 用法：
///   byteArray(n)        — n 字节，全填 0
///   byteArray(n, fill)  — n 字节，全填 fill（0-255）
fn bi_byte_array(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let n = bh::as_int(args, 0, "byteArray")?;
    if n < 0 {
        return Err(crate::value::error_value(
            "byteArray() 长度不能为负 (可能原因：参数错误)",
        ));
    }
    let fill = if args.len() >= 2 {
        byte_val(&args[1], "byteArray")?
    } else {
        0u8
    };
    let buf = vec![fill; n as usize];
    Ok(Value::ByteArray(Arc::new(Mutex::new(buf))))
}

/// bi_bytes 转为不可变 bytes。
///
/// 支持来源：
///   string        — UTF-8 编码字节
///   byteArray     — 拷贝出只读快照
///   Array<Int>    — 每个 Int 作为一个字节（0-255）
fn bi_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "bytes")?;
    match &args[0] {
        Value::Str(s) => Ok(Value::Bytes(Arc::new(s.as_bytes().to_vec()))),
        Value::Bytes(b) => Ok(Value::Bytes(b.clone())), // 已是 bytes，原样返回
        Value::ByteArray(b) => {
            // 拷贝出只读快照
            let snap = b.lock().unwrap().clone();
            Ok(Value::Bytes(Arc::new(snap)))
        }
        Value::Array(a) => {
            // Array<Int> → bytes
            let arr = a.lock().unwrap();
            let mut buf = Vec::with_capacity(arr.len());
            for (i, v) in arr.iter().enumerate() {
                buf.push(byte_val(v, "bytes").map_err(|e| {
                    // 附加元素索引信息
                    match &e {
                        Value::Error(er) => crate::value::error_value(format!(
                            "{} [元素 #{}]", er.message, i,
                        )),
                        _ => e,
                    }
                })?);
            }
            Ok(Value::Bytes(Arc::new(buf)))
        }
        v => Err(crate::value::error_value(format!(
            "bytes() 不支持类型 {} (可能原因：参数应为 string/byteArray/array<int>)",
            v.type_name(),
        ))),
    }
}

/// bi_byte_array_from_bytes 从不可变 bytes 创建可变 byteArray（拷贝）。
fn bi_byte_array_from_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "byteArrayFromBytes")?;
    match &args[0] {
        Value::Bytes(b) => Ok(Value::ByteArray(Arc::new(Mutex::new(b.as_ref().to_vec())))),
        Value::ByteArray(b) => {
            // byteArray → byteArray：拷贝一份新的（语义上互不影响）
            let snap = b.lock().unwrap().clone();
            Ok(Value::ByteArray(Arc::new(Mutex::new(snap))))
        }
        Value::Str(s) => {
            // string 也支持：UTF-8 字节
            Ok(Value::ByteArray(Arc::new(Mutex::new(s.as_bytes().to_vec()))))
        }
        v => Err(crate::value::error_value(format!(
            "byteArrayFromBytes() 不支持类型 {} (可能原因：参数应为 bytes/byteArray/string)",
            v.type_name(),
        ))),
    }
}

/// bi_byte_array_from_array 从 Array<Int> 创建可变 byteArray。
fn bi_byte_array_from_array(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let arr = bh::as_array(args, 0, "byteArrayFromArray")?;
    let guard = arr.lock().unwrap();
    let mut buf = Vec::with_capacity(guard.len());
    for (i, v) in guard.iter().enumerate() {
        buf.push(byte_val(v, "byteArrayFromArray").map_err(|e| match e {
            Value::Error(er) => crate::value::error_value(format!("{} [元素 #{}]", er.message, i)),
            _ => e,
        })?);
    }
    Ok(Value::ByteArray(Arc::new(Mutex::new(buf))))
}

/// bi_array_from_byte_array 将 byteArray 转为 Array<Int>（每字节一个 Int）。
///
/// 也接受 bytes（不可变）作为输入。
fn bi_array_from_byte_array(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "arrayFromByteArray")?;
    let bytes_vec: Vec<u8> = match &args[0] {
        Value::ByteArray(b) => b.lock().unwrap().clone(),
        Value::Bytes(b) => b.as_ref().to_vec(),
        v => return Err(crate::value::error_value(format!(
            "arrayFromByteArray() 不支持类型 {} (可能原因：参数应为 byteArray/bytes)",
            v.type_name(),
        ))),
    };
    let arr: Vec<Value> = bytes_vec.into_iter().map(|x| Value::Int(x as i64)).collect();
    Ok(Value::Array(Arc::new(Mutex::new(arr))))
}

/// bi_str_from_bytes 将字节序列解码为字符串。
///
/// 用法：strFromBytes(b, encoding)
///   encoding: "utf8"（默认，非法字节替换为 U+FFFD）/ "latin1"（每字节一个码点）/ "hex"（十六进制文本）
fn bi_str_from_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "strFromBytes")?;
    let enc = if args.len() >= 2 {
        bh::as_str(args, 1, "strFromBytes")?.to_string()
    } else {
        "utf8".to_string()
    };
    let bytes_vec: Vec<u8> = match &args[0] {
        Value::Bytes(b) => b.as_ref().to_vec(),
        Value::ByteArray(b) => b.lock().unwrap().clone(),
        v => return Err(crate::value::error_value(format!(
            "strFromBytes() 不支持类型 {} (可能原因：参数应为 bytes/byteArray)",
            v.type_name(),
        ))),
    };
    let s = match enc.as_str() {
        "utf8" | "utf-8" => String::from_utf8_lossy(&bytes_vec).into_owned(),
        "latin1" | "iso-8859-1" => {
            // 每字节直接映射为码点 0-255
            bytes_vec.iter().map(|&b| b as char).collect()
        }
        "hex" => bytes_vec.iter().map(|b| format!("{:02x}", b)).collect(),
        _ => return Err(crate::value::error_value(format!(
            "strFromBytes() 不支持的编码 '{}' (可能原因：编码名错误；支持 utf8/latin1/hex)",
            enc,
        ))),
    };
    Ok(Value::str_from(s))
}

/// bi_copy 批量复制字节（类似 Go 的 copy）。
///
/// 用法：
///   copy(dst, src)              — 从 src 复制到 dst 开头，返回复制字节数
///   copy(dst, src, dstStart)    — 从 dst 的 dstStart 位置开始写入
///
/// dst 必须是 byteArray（可变）；src 可以是 bytes/byteArray/string。
/// 复制字节数 = min(len(src), len(dst) - dstStart)。
fn bi_copy(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.len() < 2 {
        return Err(crate::value::error_value("copy() 需要至少 2 个参数 (dst, src)"));
    }
    let dst_start = if args.len() >= 3 {
        bh::as_int(args, 2, "copy")?
    } else {
        0
    };
    if dst_start < 0 {
        return Err(crate::value::error_value("copy() dstStart 不能为负"));
    }
    // dst 必须是 byteArray
    let dst_arc = match &args[0] {
        Value::ByteArray(b) => b.clone(),
        v => return Err(crate::value::error_value(format!(
            "copy() 目标必须是 byteArray，得到 {} (可能原因：参数顺序错误；dst 应在前)",
            v.type_name(),
        ))),
    };
    // src：bytes/byteArray/string
    let src_vec: Vec<u8> = match &args[1] {
        Value::Bytes(b) => b.as_ref().to_vec(),
        Value::ByteArray(b) => b.lock().unwrap().clone(),
        Value::Str(s) => s.as_bytes().to_vec(),
        v => return Err(crate::value::error_value(format!(
            "copy() 源应为 bytes/byteArray/string，得到 {} (可能原因：类型不匹配)",
            v.type_name(),
        ))),
    };
    let mut dst = dst_arc.lock().unwrap();
    let dst_len = dst.len();
    if (dst_start as usize) > dst_len {
        return Err(crate::value::error_value(format!(
            "copy() dstStart {} 超出目标长度 {} (可能原因：起始位置越界)",
            dst_start, dst_len,
        )));
    }
    let avail = dst_len - dst_start as usize;
    let n = src_vec.len().min(avail);
    dst[dst_start as usize..dst_start as usize + n].copy_from_slice(&src_vec[..n]);
    Ok(Value::Int(n as i64))
}

/// bi_bytes_hex 将字节序列转为十六进制字符串。
///
/// 接受 bytes 或 byteArray。
fn bi_bytes_hex(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "bytesHex")?;
    let hex: String = match &args[0] {
        Value::Bytes(b) => b.iter().map(|x| format!("{:02x}", x)).collect(),
        Value::ByteArray(b) => b.lock().unwrap().iter().map(|x| format!("{:02x}", x)).collect(),
        v => return Err(crate::value::error_value(format!(
            "bytesHex() 不支持类型 {} (可能原因：参数应为 bytes/byteArray)",
            v.type_name(),
        ))),
    };
    Ok(Value::str_from(hex))
}

/// bi_bytes_from_hex 将十六进制字符串转为 bytes。
///
/// 字符串可含空格/冒号（自动忽略），长度（有效十六进制字符）须为偶数。
fn bi_bytes_from_hex(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "bytesFromHex")?;
    // 过滤非十六进制字符（忽略空格、冒号、横线等分隔符）
    let cleaned: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if cleaned.len() % 2 != 0 {
        return Err(crate::value::error_value(format!(
            "bytesFromHex() 十六进制字符数为奇数 {} (可能原因：缺少一个字符；有效字符需成对)",
            cleaned.len(),
        )));
    }
    let mut buf = Vec::with_capacity(cleaned.len() / 2);
    let bytes = cleaned.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = (bytes[i] as char).to_digit(16).unwrap() as u8;
        let lo = (bytes[i + 1] as char).to_digit(16).unwrap() as u8;
        buf.push((hi << 4) | lo);
        i += 2;
    }
    Ok(Value::Bytes(Arc::new(buf)))
}

/// bi_hex_encode 将 string/bytes/byteArray 编码为 hex 字符串。
fn bi_hex_encode(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "hexEncode")?;
    let data: Vec<u8> = match &args[0] {
        Value::Str(s) => s.as_bytes().to_vec(),
        Value::Bytes(b) => b.as_ref().to_vec(),
        Value::ByteArray(b) => b.lock().unwrap().clone(),
        other => return Err(crate::value::error_value(format!(
            "hexEncode() 不支持类型 {} (需要 string/bytes/byteArray)", other.type_name(),
        ))),
    };
    let hex: String = data.iter().map(|b| format!("{:02x}", b)).collect();
    Ok(Value::str_from(hex))
}

/// bi_hex_to_str 将 hex 字符串解码为原始字符串。
fn bi_hex_to_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    // 复用 bytesFromHex 逻辑，再转字符串
    let hex = bh::as_str(args, 0, "hexToStr")?;
    let cleaned: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.len() % 2 != 0 {
        return Err(crate::value::error_value("hexToStr() hex 字符串长度必须为偶数"));
    }
    let bytes = cleaned.as_bytes();
    let mut buf = Vec::with_capacity(bytes.len() / 2);
    let mut i = 0;
    while i < bytes.len() {
        let hi = (bytes[i] as char).to_digit(16).ok_or_else(|| crate::value::error_value(
            format!("hexToStr() 非法 hex 字符 '{}'", bytes[i] as char),
        ))? as u8;
        let lo = (bytes[i + 1] as char).to_digit(16).ok_or_else(|| crate::value::error_value(
            format!("hexToStr() 非法 hex 字符 '{}'", bytes[i + 1] as char),
        ))? as u8;
        buf.push((hi << 4) | lo);
        i += 2;
    }
    Ok(Value::str_from(String::from_utf8_lossy(&buf).into_owned()))
}
