//! builtins_aes.rs — AES 加解密内置函数
//!
//! 基于 aes.rs 自实现（纯标准库）。
//! 支持 AES-128/192/256（密钥长度决定），CBC 模式，PKCS7 填充。
//!
//! 函数：
//!   aesEncrypt(data, key)     — AES-CBC 加密，返回 [IV(16字节)][密文]
//!   aesDecrypt(data, key)     — AES-CBC 解密
//!   aesEncryptStr(text, key)  — 便捷：字符串加密 → base64 输出
//!   aesDecryptStr(b64, key)   — 便捷：base64 输入 → 字符串解密

use std::sync::Arc;

use crate::builtins_helpers as bh;
use crate::value::Value;
use crate::vm::VM;

/// register 注册 AES 内置函数。
pub fn register(vm: &mut VM) {
    vm.register_builtin("aesEncrypt", bi_aes_encrypt);
    vm.register_builtin("aesDecrypt", bi_aes_decrypt);
    vm.register_builtin("aesEncryptStr", bi_aes_encrypt_str);
    vm.register_builtin("aesDecryptStr", bi_aes_decrypt_str);
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

/// 生成随机 16 字节 IV。
fn random_iv() -> [u8; 16] {
    let mut iv = [0u8; 16];
    // 用当前时间纳秒做简单随机源（非密码学安全，但足够区分）
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    // 用简单的 LCG 生成 16 字节
    let mut seed: u64 = nanos as u64;
    for b in iv.iter_mut() {
        seed = seed.wrapping_mul(6364136223846793005u64).wrapping_add(1442695040888963407u64);
        *b = (seed >> 33) as u8;
    }
    iv
}

/// bi_aes_encrypt AES-CBC 加密。
///
/// 用法：aesEncrypt(data, key) → bytes
/// data: string/bytes/byteArray（明文）
/// key: string/bytes，长度 16/24/32（对应 AES-128/192/256）
/// 返回：[16字节IV][密文] 的 bytes
fn bi_aes_encrypt(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "aesEncrypt")?;
    bh::require_arg(args, 1, "aesEncrypt")?;
    let data = to_bytes(&args[0])?;
    let key = to_bytes(&args[1])?;
    let iv = random_iv();
    let encrypted = crate::aes::aes_cbc_encrypt(&data, &key, &iv).map_err(|e| {
        crate::value::error_value(format!("aesEncrypt() 失败: {}", e))
    })?;
    // 输出：IV + 密文
    let mut result = Vec::with_capacity(16 + encrypted.len());
    result.extend_from_slice(&iv);
    result.extend_from_slice(&encrypted);
    Ok(Value::Bytes(Arc::new(result)))
}

/// bi_aes_decrypt AES-CBC 解密。
///
/// 用法：aesDecrypt(data, key) → bytes
/// data: [16字节IV][密文] 的 bytes/byteArray
fn bi_aes_decrypt(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "aesDecrypt")?;
    bh::require_arg(args, 1, "aesDecrypt")?;
    let data = to_bytes(&args[0])?;
    let key = to_bytes(&args[1])?;
    if data.len() < 16 {
        return Ok(crate::value::error_value("aesDecrypt() 数据太短（至少需要 16 字节 IV）"));
    }
    let mut iv = [0u8; 16];
    iv.copy_from_slice(&data[..16]);
    let ciphertext = &data[16..];
    match crate::aes::aes_cbc_decrypt(ciphertext, &key, &iv) {
        Ok(plaintext) => Ok(Value::Bytes(Arc::new(plaintext))),
        Err(e) => Ok(crate::value::error_value(format!("aesDecrypt() 解密失败: {}", e))),
    }
}

/// bi_aes_encrypt_str 便捷：字符串加密 → base64 输出。
///
/// 用法：aesEncryptStr(text, key) → base64 字符串
fn bi_aes_encrypt_str(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "aesEncryptStr")?;
    bh::require_arg(args, 1, "aesEncryptStr")?;
    let encrypted = bi_aes_encrypt(vm, args)?;
    // 转 base64
    if let Value::Bytes(b) = &encrypted {
        let mut out = Vec::with_capacity((b.len() + 2) / 3 * 4);
        let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut i = 0;
        while i + 3 <= b.len() {
            let n = ((b[i] as u32) << 16) | ((b[i + 1] as u32) << 8) | (b[i + 2] as u32);
            out.push(table[((n >> 18) & 0x3F) as usize]);
            out.push(table[((n >> 12) & 0x3F) as usize]);
            out.push(table[((n >> 6) & 0x3F) as usize]);
            out.push(table[(n & 0x3F) as usize]);
            i += 3;
        }
        let rem = b.len() - i;
        if rem == 1 {
            let n = (b[i] as u32) << 16;
            out.push(table[((n >> 18) & 0x3F) as usize]);
            out.push(table[((n >> 12) & 0x3F) as usize]);
            out.push(b'='); out.push(b'=');
        } else if rem == 2 {
            let n = ((b[i] as u32) << 16) | ((b[i + 1] as u32) << 8);
            out.push(table[((n >> 18) & 0x3F) as usize]);
            out.push(table[((n >> 12) & 0x3F) as usize]);
            out.push(table[((n >> 6) & 0x3F) as usize]);
            out.push(b'=');
        }
        Ok(Value::str_from(String::from_utf8_lossy(&out).into_owned()))
    } else {
        Ok(encrypted) // 错误值直接返回
    }
}

/// bi_aes_decrypt_str 便捷：base64 输入 → 字符串解密。
///
/// 用法：aesDecryptStr(base64, key) → 字符串
fn bi_aes_decrypt_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "aesDecryptStr")?;
    bh::require_arg(args, 1, "aesDecryptStr")?;
    // 先 base64 解码（复用已有的解码逻辑）
    let b64 = bh::as_str(args, 0, "aesDecryptStr")?;
    let clean: Vec<u8> = b64.bytes().filter(|&b| b != b'=' && b != b'\n' && b != b'\r' && b != b' ').collect();
    let mut data = Vec::with_capacity(clean.len() * 3 / 4);
    let b64_val = |c: u8| -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' => Some(62), b'/' => Some(63),
            _ => None,
        }
    };
    let mut i = 0;
    while i + 4 <= clean.len() {
        let v0 = b64_val(clean[i]).unwrap_or(0);
        let v1 = b64_val(clean[i + 1]).unwrap_or(0);
        let v2 = b64_val(clean[i + 2]);
        let v3 = b64_val(clean[i + 3]);
        let n = (v0 << 18) | (v1 << 12) | (v2.unwrap_or(0) << 6) | v3.unwrap_or(0);
        data.push((n >> 16) as u8);
        if v2.is_some() { data.push((n >> 8) as u8); }
        if v3.is_some() { data.push(n as u8); }
        i += 4;
    }
    // 处理剩余 2-3 字符
    let rem = clean.len() - i;
    if rem == 2 {
        let v0 = b64_val(clean[i]).unwrap_or(0);
        let v1 = b64_val(clean[i + 1]).unwrap_or(0);
        let n = (v0 << 18) | (v1 << 12);
        data.push((n >> 16) as u8);
    } else if rem == 3 {
        let v0 = b64_val(clean[i]).unwrap_or(0);
        let v1 = b64_val(clean[i + 1]).unwrap_or(0);
        let v2 = b64_val(clean[i + 2]).unwrap_or(0);
        let n = (v0 << 18) | (v1 << 12) | (v2 << 6);
        data.push((n >> 16) as u8);
        data.push((n >> 8) as u8);
    }

    let key = to_bytes(&args[1])?;
    if data.len() < 16 {
        return Ok(crate::value::error_value("aesDecryptStr() base64 解码后数据太短"));
    }
    let mut iv = [0u8; 16];
    iv.copy_from_slice(&data[..16]);
    let ciphertext = &data[16..];
    match crate::aes::aes_cbc_decrypt(ciphertext, &key, &iv) {
        Ok(plaintext) => Ok(Value::str_from(String::from_utf8_lossy(&plaintext).into_owned())),
        Err(e) => Ok(crate::value::error_value(format!("aesDecryptStr() 解密失败: {}", e))),
    }
}
