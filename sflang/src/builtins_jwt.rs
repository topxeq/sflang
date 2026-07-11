//! builtins_jwt.rs — JWT (JSON Web Token) 内置函数
//!
//! 设计要点：
//!   - 纯标准库实现，复用 crate::hash::hmac_sha256
//!   - 支持 HS256 算法（HMAC-SHA256），覆盖绝大多数 Web 场景
//!   - getJsonNodeStr 配合解析 payload 字段
//!   - 错误信息包含 AI 友好的可能原因提示
//!
//! 函数列表：
//!   genJwtToken(payload, secret)  — 生成 JWT 令牌
//!   parseJwtToken(token, secret) — 解析并验证 JWT 令牌
//!
//! JWT 结构：header.payload.signature
//!   - header:  {"alg":"HS256","typ":"JWT"}
//!   - payload: 用户自定义数据（Map/Object）
//!   - signature: HMAC-SHA256(header.payload, secret)

use crate::builtins_helpers as bh;
use crate::value::{Value, error_value};
use crate::vm::VM;

/// register 注册 JWT 内置函数。
pub fn register(vm: &mut VM) {
    vm.register_builtin("genJwtToken", bi_gen_jwt_token);
    vm.register_builtin("parseJwtToken", bi_parse_jwt_token);
}

/// JWT header（固定 {"alg":"HS256","typ":"JWT"}）
const JWT_HEADER: &str = r#"{"alg":"HS256","typ":"JWT"}"#;

/// base64url_encode 编码字节为无填充的 URL-safe base64 字符串。
fn base64url_encode(data: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = Vec::with_capacity((data.len() + 2) / 3 * 4);
    let mut i = 0;
    while i + 3 <= data.len() {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | (data[i + 2] as u32);
        out.push(TABLE[((n >> 18) & 0x3F) as usize]);
        out.push(TABLE[((n >> 12) & 0x3F) as usize]);
        out.push(TABLE[((n >> 6) & 0x3F) as usize]);
        out.push(TABLE[(n & 0x3F) as usize]);
        i += 3;
    }
    let rem = data.len() - i;
    if rem == 1 {
        let n = (data[i] as u32) << 16;
        out.push(TABLE[((n >> 18) & 0x3F) as usize]);
        out.push(TABLE[((n >> 12) & 0x3F) as usize]);
    } else if rem == 2 {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8);
        out.push(TABLE[((n >> 18) & 0x3F) as usize]);
        out.push(TABLE[((n >> 12) & 0x3F) as usize]);
        out.push(TABLE[((n >> 6) & 0x3F) as usize]);
    }
    String::from_utf8(out).unwrap()
}

/// base64url_decode 解码 URL-safe base64（支持有/无填充）。
fn base64url_decode(s: &str) -> Result<Vec<u8>, String> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits = 0;
    for &c in bytes {
        if c == b'=' { break; }
        let v = val(c).ok_or_else(|| format!("无效 base64url 字符: {}", c as char))?;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8 & 0xFF);
        }
    }
    Ok(out)
}

/// map_to_json 将 Map/Object 转为 JSON 字符串（简化版，仅支持平铺）。
fn map_to_json(v: &Value) -> Result<String, Value> {
    match v {
        Value::Object(_) | Value::Map(_) => {
            // 复用 jsonEncode 逻辑
            Ok(v.to_str())
        }
        _ => Err(error_value(format!(
            "genJwtToken() payload 应为 map，得到 {} (可能原因：参数类型错误)",
            v.type_name_ex(),
        ))),
    }
}

/// bi_gen_jwt_token 生成 JWT 令牌（HS256）。
///
/// 用法：token := genJwtToken({user: "alice", role: "admin"}, "secret")
fn bi_gen_jwt_token(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 1, "genJwtToken")?;
    let payload = map_to_json(&args[0])?;
    let secret = bh::as_str(args, 1, "genJwtToken")?;

    let header_b64 = base64url_encode(JWT_HEADER.as_bytes());
    let payload_b64 = base64url_encode(payload.as_bytes());
    let signing_input = format!("{}.{}", header_b64, payload_b64);
    let signature = crate::hash::hmac_sha256(secret.as_bytes(), signing_input.as_bytes());
    let sig_b64 = base64url_encode(&signature);

    Ok(Value::str_from(format!("{}.{}", signing_input, sig_b64)))
}

/// bi_parse_jwt_token 解析并验证 JWT 令牌（HS256）。
///
/// 用法：payload := parseJwtToken(token, "secret")
/// 验证失败抛异常。成功返回 payload（已解析的 Map）。
fn bi_parse_jwt_token(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 1, "parseJwtToken")?;
    let token = bh::as_str(args, 0, "parseJwtToken")?;
    let secret = bh::as_str(args, 1, "parseJwtToken")?;

    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(error_value(format!(
            "parseJwtToken() 令牌格式错误: 应为 3 段 (header.payload.signature)，得到 {} 段 (可能原因：令牌被截断或拼接错误)",
            parts.len(),
        )));
    }

    let header_b64 = parts[0];
    let payload_b64 = parts[1];
    let sig_b64 = parts[2];

    // 验证签名
    let signing_input = format!("{}.{}", header_b64, payload_b64);
    let expected_sig = crate::hash::hmac_sha256(secret.as_bytes(), signing_input.as_bytes());
    let expected_sig_b64 = base64url_encode(&expected_sig);

    if expected_sig_b64 != sig_b64 {
        return Err(error_value(
            "parseJwtToken() 签名验证失败 (可能原因：密钥错误或令牌被篡改)".to_string(),
        ));
    }

    // 解析 payload
    let payload_bytes = base64url_decode(payload_b64).map_err(|e| error_value(format!(
        "parseJwtToken() payload 解码失败: {} (可能原因：令牌格式损坏)", e,
    )))?;
    let payload_str = String::from_utf8(payload_bytes).map_err(|e| error_value(format!(
        "parseJwtToken() payload UTF-8 解码失败: {} (可能原因：payload 包含非 UTF-8 数据)", e,
    )))?;

    // 解析 JSON
    let mut dec = crate::builtins_json::Decoder::new(&payload_str);
    dec.parse_value().map_err(|e| error_value(format!(
        "parseJwtToken() payload JSON 解析失败: {} (可能原因：payload 不是有效 JSON)", e.to_str(),
    )))
}
