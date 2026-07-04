//! builtins_encode.rs — 编解码内置函数（base64 / URL 编码，纯标准库）
//!
//! 设计要点：
//!   - base64：数据传输、嵌入数据、JWT 等常用编码
//!   - URL：处理 URL 参数、表单数据
//!   - 全部纯标准库实现（base64 手写，URL 用 percent-encoding 逻辑）
//!
//! 函数列表：
//!   base64Encode(b)      — bytes/byteArray/string → base64 字符串
//!   base64Decode(s)      — base64 字符串 → bytes
//!   urlEncode(s)         — URL 编码（百分号编码）
//!   urlDecode(s)         — URL 解码

use std::sync::Arc;

use crate::builtins_helpers as bh;
use crate::value::Value;
use crate::vm::VM;

/// register 注册编解码内置函数。
pub fn register(vm: &mut VM) {
    vm.register_builtin("base64Encode", bi_base64_encode);
    vm.register_builtin("base64Decode", bi_base64_decode);
    vm.register_builtin("urlEncode", bi_url_encode);
    vm.register_builtin("urlDecode", bi_url_decode);
    vm.register_builtin("urlFormEncode", bi_url_form_encode);
    vm.register_builtin("urlFormDecode", bi_url_form_decode);
}

/// to_bytes 将参数转为字节 Vec（接受 string/bytes/byteArray）。
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

const B64_TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// bi_base64_encode 编码为 base64 字符串。
fn bi_base64_encode(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "base64Encode")?;
    let data = to_bytes(&args[0])?;
    let mut out = Vec::with_capacity((data.len() + 2) / 3 * 4);
    let mut i = 0;
    while i + 3 <= data.len() {
        let n = ((data[i] as u32) << 16) | ((data[i+1] as u32) << 8) | (data[i+2] as u32);
        out.push(B64_TABLE[((n >> 18) & 0x3F) as usize]);
        out.push(B64_TABLE[((n >> 12) & 0x3F) as usize]);
        out.push(B64_TABLE[((n >> 6) & 0x3F) as usize]);
        out.push(B64_TABLE[(n & 0x3F) as usize]);
        i += 3;
    }
    let rem = data.len() - i;
    if rem == 1 {
        let n = (data[i] as u32) << 16;
        out.push(B64_TABLE[((n >> 18) & 0x3F) as usize]);
        out.push(B64_TABLE[((n >> 12) & 0x3F) as usize]);
        out.push(b'=');
        out.push(b'=');
    } else if rem == 2 {
        let n = ((data[i] as u32) << 16) | ((data[i+1] as u32) << 8);
        out.push(B64_TABLE[((n >> 18) & 0x3F) as usize]);
        out.push(B64_TABLE[((n >> 12) & 0x3F) as usize]);
        out.push(B64_TABLE[((n >> 6) & 0x3F) as usize]);
        out.push(b'=');
    }
    Ok(Value::str_from(String::from_utf8_lossy(&out).into_owned()))
}

/// b64_decode_char 将 base64 字符解码为 6 位值。
fn b64_val(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some((c - b'A') as u32),
        b'a'..=b'z' => Some((c - b'a' + 26) as u32),
        b'0'..=b'9' => Some((c - b'0' + 52) as u32),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// bi_base64_decode 解码 base64 字符串为 bytes。
fn bi_base64_decode(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "base64Decode")?;
    let cleaned: Vec<u8> = s.bytes().filter(|&c| c != b'\n' && c != b'\r' && c != b' ' && c != b'\t').collect();
    if cleaned.len() % 4 != 0 {
        return Err(crate::value::error_value(format!(
            "base64Decode() 输入长度 {} 不是 4 的倍数 (可能原因：base64 数据损坏)", cleaned.len(),
        )));
    }
    let mut out = Vec::with_capacity(cleaned.len() / 4 * 3);
    let mut i = 0;
    while i < cleaned.len() {
        let c0 = cleaned[i]; let c1 = cleaned[i+1]; let c2 = cleaned[i+2]; let c3 = cleaned[i+3];
        let v0 = b64_val(c0).ok_or_else(|| crate::value::error_value(format!(
            "base64Decode() 非法字符 '{}' (可能原因：不是有效 base64)", c0 as char,
        )))?;
        let v1 = b64_val(c1).ok_or_else(|| crate::value::error_value(format!(
            "base64Decode() 非法字符 '{}'", c1 as char,
        )))?;
        let n = (v0 << 18) | (v1 << 12);
        if c2 == b'=' {
            out.push((n >> 16) as u8);
        } else {
            let v2 = b64_val(c2).ok_or_else(|| crate::value::error_value("base64Decode() 非法字符"))?;
            let n = n | (v2 << 6);
            if c3 == b'=' {
                out.push((n >> 16) as u8);
                out.push((n >> 8) as u8);
            } else {
                let v3 = b64_val(c3).ok_or_else(|| crate::value::error_value("base64Decode() 非法字符"))?;
                let n = n | v3;
                out.push((n >> 16) as u8);
                out.push((n >> 8) as u8);
                out.push(n as u8);
            }
        }
        i += 4;
    }
    Ok(Value::Bytes(Arc::new(out)))
}

/// url_unreserved 判断字符是否为 URL 非保留字符（不编码）。
fn url_unreserved(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c == b'.' || c == b'~'
}

/// bi_url_encode URL 编码（RFC 3986 百分号编码）。
///
/// 空格 → %20（非 +）；+ 保留不编码；非保留字符（字母数字 -_.~）外全部 %XX。
/// 适用于 URL 的 path/query/fragment 部分。
fn bi_url_encode(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "urlEncode")?;
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        if url_unreserved(byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{:02X}", byte));
        }
    }
    Ok(Value::str_from(out))
}

/// bi_url_decode URL 解码（RFC 3986 百分号解码）。
///
/// 仅解码 %XX；+ 保持原样（不转空格）。与 urlEncode 严格往返。
fn bi_url_decode(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "urlDecode")?;
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i+1] as char).to_digit(16);
            let lo = (bytes[i+2] as char).to_digit(16);
            match (hi, lo) {
                (Some(h), Some(l)) => {
                    out.push((h * 16 + l) as u8);
                    i += 3;
                }
                _ => { out.push(bytes[i]); i += 1; }
            }
        } else {
            // RFC 3986：+ 不转空格，原样保留
            out.push(bytes[i]);
            i += 1;
        }
    }
    Ok(Value::str_from(String::from_utf8_lossy(&out).into_owned()))
}

/// bi_url_form_encode 表单编码（application/x-www-form-urlencoded）。
///
/// 空格 → +（非 %20）；+ → %2B；其余非保留字符外 %XX。
/// 适用于 HTML 表单提交、query string 中的表单参数。
fn bi_url_form_encode(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "urlFormEncode")?;
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        if byte == b' ' {
            out.push('+');
        } else if byte == b'+' {
            out.push_str("%2B");
        } else if url_unreserved(byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{:02X}", byte));
        }
    }
    Ok(Value::str_from(out))
}

/// bi_url_form_decode 表单解码。
///
/// + → 空格；%XX 解码。与 urlFormEncode 严格往返。
fn bi_url_form_decode(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "urlFormDecode")?;
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i+1] as char).to_digit(16);
            let lo = (bytes[i+2] as char).to_digit(16);
            match (hi, lo) {
                (Some(h), Some(l)) => {
                    out.push((h * 16 + l) as u8);
                    i += 3;
                }
                _ => { out.push(bytes[i]); i += 1; }
            }
        } else if bytes[i] == b'+' {
            // 表单：+ → 空格
            out.push(b' ');
            i += 1;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Ok(Value::str_from(String::from_utf8_lossy(&out).into_owned()))
}
