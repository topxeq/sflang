//! builtins_jwt.rs — JWT (JSON Web Token) 内置函数
//!
//! 设计要点：
//!   - 支持 HS256（HMAC-SHA256，对称）和 RS256（RSA-SHA256，非对称）
//!   - HS256 复用 crate::hash::hmac_sha256（纯标准库）
//!   - RS256 使用 rsa crate（已是传递依赖，不增加二进制体积）
//!   - 密钥以 PEM 格式字符串传入（RS256），便于脚本使用
//!   - 错误信息包含 AI 友好的可能原因提示
//!
//! 函数列表：
//!   genJwtToken(payload, key, [alg])  — 生成 JWT 令牌（alg 默认 HS256）
//!   parseJwtToken(token, key)         — 解析并验证 JWT 令牌（自动识别 alg）
//!
//! JWT 结构：header.payload.signature
//!   - header:  {"alg":"HS256|RS256","typ":"JWT"}
//!   - payload: 用户自定义数据（Map/Object）
//!   - signature(HS256): HMAC-SHA256(header.payload, secret)
//!   - signature(RS256): RSA-SHA256(header.payload, privateKey)

use crate::builtins_helpers as bh;
use crate::function::BuiltinDoc;
use crate::value::{Value, error_value};
use crate::vm::VM;

// ---- JWT 函数文档 ----

static DOC_GEN_JWT_TOKEN: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "genJwtToken(payload, key, alg?) -> string",
    summary: "生成 JWT 令牌（header.payload.signature），默认 HS256，可选 RS256。",
    params: &[
        ("payload", "map/object：自定义声明数据（自动 JSON 编码）"),
        ("key", "HS256：对称密钥字符串；RS256：PEM 格式 RSA 私钥字符串"),
        ("alg", "可选 \"HS256\"(默认) 或 \"RS256\""),
    ],
    returns: "string：标准 JWT 令牌（base64url 无填充编码）",
    examples: &[
        "genJwtToken({user: \"alice\"}, \"secret\")              → HS256 令牌",
        "genJwtToken({user: \"alice\"}, privateKeyPem, \"RS256\") → RS256 令牌",
    ],
    errors: &[
        "payload 必须是 map/object",
        "alg 仅支持 HS256 或 RS256",
        "RS256 私钥须为合法 PEM（PKCS#8 或 PKCS#1 格式）",
    ],
};

static DOC_PARSE_JWT_TOKEN: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "parseJwtToken(token, key) -> map",
    summary: "解析并验证 JWT 令牌（自动识别 HS256/RS256 算法），成功返回 payload map。",
    params: &[
        ("token", "string：标准 JWT 令牌（3 段 header.payload.signature）"),
        ("key", "HS256：对称密钥字符串；RS256：PEM 格式 RSA 公钥字符串"),
    ],
    returns: "map：令牌的 payload 声明（已验证签名）",
    examples: &[
        "tok := genJwtToken({user: \"alice\"}, \"secret\")",
        "parseJwtToken(tok, \"secret\")            → {user: \"alice\"}（HS256）",
        "parseJwtToken(tok, publicKeyPem)         → payload（RS256 用公钥验证）",
    ],
    errors: &[
        "令牌必须为 3 段（用 . 分隔）",
        "算法由 header 自动识别，须为 HS256 或 RS256",
        "签名验证失败（密钥错误/被篡改）返回 error",
    ],
};

/// register 注册 JWT 内置函数。
pub fn register(vm: &mut VM) {
    vm.register_builtin_doc("genJwtToken", bi_gen_jwt_token, &DOC_GEN_JWT_TOKEN);
    vm.register_builtin_doc("parseJwtToken", bi_parse_jwt_token, &DOC_PARSE_JWT_TOKEN);
}

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

/// map_to_json 将 Map/Object 转为 JSON 字符串（用 jsonEncode 确保格式正确）。
fn map_to_json(v: &Value) -> Result<String, Value> {
    match v {
        Value::Object(_) | Value::Map(_) => {
            // 复用 jsonEncode 逻辑（避免 inspect 格式 {key: val} 不是有效 JSON）
            let mut out = String::new();
            crate::builtins_json::encode_value(v, &mut out);
            Ok(out)
        }
        _ => Err(error_value(format!(
            "genJwtToken() payload 应为 map，得到 {} (可能原因：参数类型错误)",
            v.type_name_ex(),
        ))),
    }
}

/// parse_alg_from_header 从 JWT header 段解析 alg 字段。
/// 返回 "HS256" 或 "RS256"，无法识别时返回错误。
fn parse_alg_from_header(header_b64: &str) -> Result<String, Value> {
    let header_bytes = base64url_decode(header_b64).map_err(|e| error_value(format!(
        "parseJwtToken() header 解码失败: {} (可能原因：令牌格式损坏)", e,
    )))?;
    let header_str = String::from_utf8(header_bytes).map_err(|e| error_value(format!(
        "parseJwtToken() header UTF-8 解码失败: {} (可能原因：header 包含非 UTF-8 数据)", e,
    )))?;
    // 简化解析：查找 "alg":"XXX"
    let alg = if header_str.contains(r#""alg":"RS256""#) {
        "RS256".to_string()
    } else if header_str.contains(r#""alg":"HS256""#) {
        "HS256".to_string()
    } else {
        return Err(error_value(format!(
            "parseJwtToken() 不支持的算法，header={} (可能原因：令牌使用了非 HS256/RS256 算法)",
            header_str,
        )));
    };
    Ok(alg)
}

/// rs256_sign 用 RSA 私钥（PEM 格式）对数据签名。
fn rs256_sign(signing_input: &[u8], private_key_pem: &str) -> Result<Vec<u8>, Value> {
    use rsa::pkcs1::DecodeRsaPrivateKey;
    use rsa::pkcs8::DecodePrivateKey;
    use rsa::signature::{SignatureEncoding, Signer};
    use rsa::sha2::Sha256;
    use rsa::pkcs1v15::SigningKey;

    // 尝试 PKCS#8 格式，再尝试 PKCS#1 格式
    let private_key = rsa::RsaPrivateKey::from_pkcs8_pem(private_key_pem)
        .or_else(|_| rsa::RsaPrivateKey::from_pkcs1_pem(private_key_pem))
        .map_err(|e| error_value(format!(
            "genJwtToken() RS256 私钥解析失败: {} (可能原因：密钥不是有效的 PEM 格式 RSA 私钥；应为 PKCS#8 或 PKCS#1 格式)",
            e,
        )))?;

    let signing_key = SigningKey::<Sha256>::new(private_key);
    let signature = signing_key.sign(signing_input);
    Ok(signature.to_vec())
}

/// rs256_verify 用 RSA 公钥（PEM 格式）验证签名。
fn rs256_verify(signing_input: &[u8], signature: &[u8], public_key_pem: &str) -> Result<(), Value> {
    use rsa::pkcs1::DecodeRsaPublicKey;
    use rsa::pkcs8::DecodePublicKey;
    use rsa::signature::Verifier;
    use rsa::sha2::Sha256;
    use rsa::pkcs1v15::{VerifyingKey, Signature};

    // 尝试 SPKI 格式（SubjectPublicKeyInfo），再尝试 PKCS#1 格式
    let public_key = rsa::RsaPublicKey::from_public_key_pem(public_key_pem)
        .or_else(|_| rsa::RsaPublicKey::from_pkcs1_pem(public_key_pem))
        .map_err(|e| error_value(format!(
            "parseJwtToken() RS256 公钥解析失败: {} (可能原因：密钥不是有效的 PEM 格式 RSA 公钥)",
            e,
        )))?;

    let verifying_key = VerifyingKey::<Sha256>::new(public_key);
    let sig = Signature::try_from(signature).map_err(|_| error_value(
        "parseJwtToken() RS256 签名格式错误 (可能原因：签名长度不正确)".to_string(),
    ))?;

    Verifier::verify(&verifying_key, signing_input, &sig).map_err(|_| error_value(
        "parseJwtToken() RS256 签名验证失败 (可能原因：公钥不匹配或令牌被篡改)".to_string(),
    ))
}

/// bi_gen_jwt_token 生成 JWT 令牌（HS256 或 RS256）。
///
/// 用法：
///   token := genJwtToken({user: "alice"}, "secret")              // HS256（默认）
///   token := genJwtToken({user: "alice"}, privateKeyPem, "RS256") // RS256
fn bi_gen_jwt_token(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 1, "genJwtToken")?;
    let payload = map_to_json(&args[0])?;
    let key = bh::as_str(args, 1, "genJwtToken")?;
    let alg = args.get(2).map(|v| v.to_str()).unwrap_or_else(|| "HS256".to_string());

    let header_json = match alg.as_str() {
        "HS256" => r#"{"alg":"HS256","typ":"JWT"}"#,
        "RS256" => r#"{"alg":"RS256","typ":"JWT"}"#,
        other => return Err(error_value(format!(
            "genJwtToken() 不支持的算法 '{}' (可能原因：算法名应为 HS256 或 RS256)", other,
        ))),
    };

    let header_b64 = base64url_encode(header_json.as_bytes());
    let payload_b64 = base64url_encode(payload.as_bytes());
    let signing_input = format!("{}.{}", header_b64, payload_b64);

    let signature = match alg.as_str() {
        "HS256" => crate::hash::hmac_sha256(key.as_bytes(), signing_input.as_bytes()),
        "RS256" => rs256_sign(signing_input.as_bytes(), key)?,
        _ => unreachable!(),
    };
    let sig_b64 = base64url_encode(&signature);

    Ok(Value::str_from(format!("{}.{}", signing_input, sig_b64)))
}

/// bi_parse_jwt_token 解析并验证 JWT 令牌（自动识别 HS256/RS256）。
///
/// 用法：
///   payload := parseJwtToken(token, "secret")        // HS256：secret 为对称密钥
///   payload := parseJwtToken(token, publicKeyPem)     // RS256：公钥为 PEM 格式
/// 验证失败返回 error。成功返回 payload（已解析的 Map）。
fn bi_parse_jwt_token(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 1, "parseJwtToken")?;
    let token = bh::as_str(args, 0, "parseJwtToken")?;
    let key = bh::as_str(args, 1, "parseJwtToken")?;

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

    // 从 header 自动识别算法
    let alg = parse_alg_from_header(header_b64)?;

    // 验证签名
    let signing_input = format!("{}.{}", header_b64, payload_b64);
    match alg.as_str() {
        "HS256" => {
            let expected_sig = crate::hash::hmac_sha256(key.as_bytes(), signing_input.as_bytes());
            let expected_sig_b64 = base64url_encode(&expected_sig);
            if expected_sig_b64 != sig_b64 {
                return Err(error_value(
                    "parseJwtToken() HS256 签名验证失败 (可能原因：密钥错误或令牌被篡改)".to_string(),
                ));
            }
        }
        "RS256" => {
            let sig_bytes = base64url_decode(sig_b64).map_err(|e| error_value(format!(
                "parseJwtToken() 签名解码失败: {} (可能原因：令牌格式损坏)", e,
            )))?;
            rs256_verify(signing_input.as_bytes(), &sig_bytes, key)?;
        }
        _ => unreachable!(),
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
