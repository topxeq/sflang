//! builtins_hash.rs — 哈希与 HMAC 内置函数（基于自实现的 hash.rs）
//!
//! 函数列表：
//!   md5(b)          — MD5 哈希（16 字节 → bytes）
//!   sha1(b)         — SHA-1 哈希（20 字节 → bytes）
//!   sha256(b)       — SHA-256 哈希（32 字节 → bytes）
//!   md5Hex(b)       — MD5 哈希的十六进制字符串
//!   sha1Hex(b)      — SHA-1 哈希的十六进制字符串
//!   sha256Hex(b)    — SHA-256 哈希的十六进制字符串
//!   hmacSha256(k,m) — HMAC-SHA256（32 字节 → bytes）
//!   hmacSha256Hex(k,m) — HMAC-SHA256 的十六进制字符串
//!   getOtpCode(secret, timestamp?) — 生成 TOTP 6 位验证码
//!   checkOtpCode(secret, code, timestamp?) — 验证 TOTP 码

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::builtins_helpers as bh;
use crate::function::BuiltinDoc;
use crate::value::Value;
use crate::vm::VM;

// ---- 哈希函数文档 ----

static DOC_MD5: BuiltinDoc = BuiltinDoc {
    category: "hash",
    signature: "md5(data) -> bytes",
    summary: "计算 MD5 哈希，返回 16 字节 bytes。",
    params: &[("data", "string/bytes/byteArray：待哈希的数据（string 取 UTF-8 字节）")],
    returns: "bytes：16 字节 MD5 摘要",
    examples: &[
        "md5(\"abc\")          → 16 字节 bytes",
        "bytesHex(md5(\"abc\")) → \"900150983cd24fb0d6963f7d28e17f72\"",
    ],
    errors: &["MD5 不再安全，勿用于密码存储；仅用于校验和/去重"],
};

static DOC_SHA1: BuiltinDoc = BuiltinDoc {
    category: "hash",
    signature: "sha1(data) -> bytes",
    summary: "计算 SHA-1 哈希，返回 20 字节 bytes。",
    params: &[("data", "string/bytes/byteArray：待哈希的数据（string 取 UTF-8 字节）")],
    returns: "bytes：20 字节 SHA-1 摘要",
    examples: &[
        "sha1(\"abc\")          → 20 字节 bytes",
        "bytesHex(sha1(\"abc\")) → \"a9993e364706816aba3e25717850c26c9cd0d89d\"",
    ],
    errors: &["SHA-1 已不推荐用于安全场景；新代码建议用 sha256"],
};

static DOC_SHA256: BuiltinDoc = BuiltinDoc {
    category: "hash",
    signature: "sha256(data) -> bytes",
    summary: "计算 SHA-256 哈希，返回 32 字节 bytes。",
    params: &[("data", "string/bytes/byteArray：待哈希的数据（string 取 UTF-8 字节）")],
    returns: "bytes：32 字节 SHA-256 摘要",
    examples: &[
        "sha256(\"abc\")          → 32 字节 bytes",
        "bytesHex(sha256(\"abc\")) → \"ba7816bf...ad2347\"（64 位十六进制）",
    ],
    errors: &["参数须为 string/bytes/byteArray"],
};

static DOC_MD5_HEX: BuiltinDoc = BuiltinDoc {
    category: "hash",
    signature: "md5Hex(data) -> string",
    summary: "计算 MD5 哈希并返回 32 字符小写十六进制字符串。",
    params: &[("data", "string/bytes/byteArray：待哈希的数据（string 取 UTF-8 字节）")],
    returns: "string：32 字符小写十六进制摘要",
    examples: &[
        "md5Hex(\"abc\") → \"900150983cd24fb0d6963f7d28e17f72\"",
    ],
    errors: &["等价于 bytesHex(md5(data))；MD5 非安全算法"],
};

static DOC_SHA1_HEX: BuiltinDoc = BuiltinDoc {
    category: "hash",
    signature: "sha1Hex(data) -> string",
    summary: "计算 SHA-1 哈希并返回 40 字符小写十六进制字符串。",
    params: &[("data", "string/bytes/byteArray：待哈希的数据（string 取 UTF-8 字节）")],
    returns: "string：40 字符小写十六进制摘要",
    examples: &[
        "sha1Hex(\"abc\") → \"a9993e364706816aba3e25717850c26c9cd0d89d\"",
    ],
    errors: &["等价于 bytesHex(sha1(data))"],
};

static DOC_SHA256_HEX: BuiltinDoc = BuiltinDoc {
    category: "hash",
    signature: "sha256Hex(data) -> string",
    summary: "计算 SHA-256 哈希并返回 64 字符小写十六进制字符串。",
    params: &[("data", "string/bytes/byteArray：待哈希的数据（string 取 UTF-8 字节）")],
    returns: "string：64 字符小写十六进制摘要",
    examples: &[
        "sha256Hex(\"abc\") → \"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\"",
    ],
    errors: &["等价于 bytesHex(sha256(data))"],
};

static DOC_HMAC_SHA256: BuiltinDoc = BuiltinDoc {
    category: "hash",
    signature: "hmacSha256(key, message) -> bytes",
    summary: "计算 HMAC-SHA256（RFC 2104），返回 32 字节 bytes。",
    params: &[
        ("key", "string/bytes/byteArray：HMAC 密钥"),
        ("message", "string/bytes/byteArray：待认证的消息"),
    ],
    returns: "bytes：32 字节 HMAC-SHA256 摘要",
    examples: &[
        "bytesHex(hmacSha256(\"key\", \"The quick brown fox\")) → \"f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8\"",
    ],
    errors: &["参数顺序为 key 在前、message 在后"],
};

static DOC_HMAC_SHA256_HEX: BuiltinDoc = BuiltinDoc {
    category: "hash",
    signature: "hmacSha256Hex(key, message) -> string",
    summary: "计算 HMAC-SHA256 并返回 64 字符小写十六进制字符串。",
    params: &[
        ("key", "string/bytes/byteArray：HMAC 密钥"),
        ("message", "string/bytes/byteArray：待认证的消息"),
    ],
    returns: "string：64 字符小写十六进制 HMAC-SHA256 摘要",
    examples: &[
        "hmacSha256Hex(\"key\", \"The quick brown fox\") → \"f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8\"",
    ],
    errors: &["参数顺序为 key 在前、message 在后"],
};

static DOC_GET_OTP_CODE: BuiltinDoc = BuiltinDoc {
    category: "hash",
    signature: "getOtpCode(secret, timestamp?) -> string",
    summary: "生成 TOTP（RFC 6238）6 位验证码，左侧补零。",
    params: &[
        ("secret", "Base32 编码的密钥字符串（字母表 A-Z, 2-7）"),
        ("timestamp", "可选 int：Unix 秒；省略则用当前系统时间"),
    ],
    returns: "string：6 位数字验证码（左侧补零）",
    examples: &[
        "getOtpCode(\"JBSWY3DPEHPK3PXP\", 1234567890) → 固定 6 位码",
        "getOtpCode(\"JBSWY3DPEHPK3PXP\")            → 用当前时间的 6 位码",
    ],
    errors: &[
        "secret 必须是合法 Base32（A-Z, 2-7），空格和 = 填充会被忽略",
        "解码后密钥为空时返回 error",
    ],
};

static DOC_CHECK_OTP_CODE: BuiltinDoc = BuiltinDoc {
    category: "hash",
    signature: "checkOtpCode(secret, code, timestamp?) -> bool",
    summary: "验证 TOTP 码是否匹配（比较前会 trim code），返回 bool。",
    params: &[
        ("secret", "Base32 编码的密钥字符串（字母表 A-Z, 2-7）"),
        ("code", "用户输入的 6 位字符串（前后空白会被 trim）"),
        ("timestamp", "可选 int：Unix 秒；省略则用当前系统时间"),
    ],
    returns: "bool：code 与该时间窗口的预期值一致返回 true",
    examples: &[
        "checkOtpCode(\"JBSWY3DPEHPK3PXP\", getOtpCode(\"JBSWY3DPEHPK3PXP\")) → true",
    ],
    errors: &[
        "TOTP 有 30 秒时间窗口，时钟漂移可能误判",
        "secret 必须是合法 Base32，否则返回 error",
    ],
};

pub fn register(vm: &mut VM) {
    vm.register_builtin_doc("md5", bi_md5, &DOC_MD5);
    vm.register_builtin_doc("sha1", bi_sha1, &DOC_SHA1);
    vm.register_builtin_doc("sha256", bi_sha256, &DOC_SHA256);
    vm.register_builtin_doc("md5Hex", bi_md5_hex, &DOC_MD5_HEX);
    vm.register_builtin_doc("sha1Hex", bi_sha1_hex, &DOC_SHA1_HEX);
    vm.register_builtin_doc("sha256Hex", bi_sha256_hex, &DOC_SHA256_HEX);
    vm.register_builtin_doc("hmacSha256", bi_hmac_sha256, &DOC_HMAC_SHA256);
    vm.register_builtin_doc("hmacSha256Hex", bi_hmac_sha256_hex, &DOC_HMAC_SHA256_HEX);
    vm.register_builtin_doc("getOtpCode", bi_get_otp_code, &DOC_GET_OTP_CODE);
    vm.register_builtin_doc("checkOtpCode", bi_check_otp_code, &DOC_CHECK_OTP_CODE);
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

fn bi_sha1_hex(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "sha1Hex")?;
    let data = to_bytes(&args[0])?;
    let hex: String = crate::hash::sha1(&data).iter().map(|b| format!("{:02x}", b)).collect();
    Ok(Value::str_from(hex))
}

fn bi_sha256_hex(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "sha256Hex")?;
    let data = to_bytes(&args[0])?;
    let hex: String = crate::hash::sha256(&data).iter().map(|b| format!("{:02x}", b)).collect();
    Ok(Value::str_from(hex))
}

/// hmac_sha256 自实现 HMAC-SHA256（RFC 2104）。
///
/// HMAC(K, m) = H((K' ⊕ opad) ‖ H((K' ⊕ ipad) ‖ m))
/// K' = K 补零到 block_size（64字节），超过则 H(K)
fn hmac_sha256(key: &[u8], message: &[u8]) -> Vec<u8> {
    crate::hash::hmac_sha256(key, message)
}

fn bi_hmac_sha256(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "hmacSha256")?;
    bh::require_arg(args, 1, "hmacSha256")?;
    let key = to_bytes(&args[0])?;
    let message = to_bytes(&args[1])?;
    Ok(Value::Bytes(Arc::new(hmac_sha256(&key, &message))))
}

fn bi_hmac_sha256_hex(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "hmacSha256Hex")?;
    bh::require_arg(args, 1, "hmacSha256Hex")?;
    let key = to_bytes(&args[0])?;
    let message = to_bytes(&args[1])?;
    let mac = hmac_sha256(&key, &message);
    let hex: String = mac.iter().map(|b| format!("{:02x}", b)).collect();
    Ok(Value::str_from(hex))
}

// ---- TOTP（RFC 6238）实现 ----
//
// 算法概述：
//   1. Base32 解码 secret 为字节
//   2. time counter = timestamp / 30
//   3. HMAC-SHA1(key=decoded_secret, msg=counter_as_8_bytes_big_endian)
//   4. offset = last_nibble & 0x0F
//   5. code = (hmac[offset..offset+4] as u32 & 0x7FFFFFFF) % 1000000
//   6. 返回 6 位字符串，左侧补零

/// base32_decode 解码 Base32 字符串（RFC 4648）。
///
/// 标准字母表 A-Z2-7（不区分大小写），= 为填充字符。
/// 忽略输入中的空格和 = 填充，便于处理用户输入的密钥（如 "JBSWY3DPEHPK3PXP"）。
/// 解码失败返回 None。
fn base32_decode(input: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

    // 将字符转为 5 位值
    let char_to_val = |c: u8| -> Option<u8> {
        let c = if c >= b'a' && c <= b'z' { c - 32 } else { c };
        ALPHABET.iter().position(|&a| a == c).map(|p| p as u8)
    };

    // 先过滤掉空白和填充字符
    let filtered: Vec<u8> = input.bytes()
        .filter(|&b| b != b' ' && b != b'=' && b != b'\n' && b != b'\r' && b != b'\t')
        .collect();

    if filtered.is_empty() {
        return Some(Vec::new());
    }

    let mut buffer: u32 = 0;
    let mut bits_left: u32 = 0;
    let mut output: Vec<u8> = Vec::with_capacity(filtered.len() * 5 / 8);

    for &b in &filtered {
        let val = char_to_val(b)?;
        buffer = (buffer << 5) | (val as u32);
        bits_left += 5;
        if bits_left >= 8 {
            bits_left -= 8;
            output.push((buffer >> bits_left) as u8);
            buffer &= (1u32 << bits_left) - 1;
        }
    }

    Some(output)
}

/// generate_totp 生成 TOTP 6 位验证码。
///
/// secret 为已解码的字节切片，timestamp 为 Unix 秒。
/// 返回 6 位字符串（左侧补零）。
fn generate_totp(secret: &[u8], timestamp: i64) -> String {
    // 时间步长 30 秒
    let counter: u64 = (timestamp as u64) / 30;

    // counter 转为 8 字节大端序
    let counter_bytes: [u8; 8] = counter.to_be_bytes();

    // HMAC-SHA1
    let hmac: Vec<u8> = crate::hash::hmac_sha1(secret, &counter_bytes);

    // Dynamic truncation
    // offset = hmac 最后一字节的低 4 位
    let offset: usize = (hmac[hmac.len() - 1] & 0x0F) as usize;
    // 取 4 字节并去掉最高位（& 0x7FFFFFFF）
    let truncated: u32 = ((hmac[offset] as u32) << 24)
        | ((hmac[offset + 1] as u32) << 16)
        | ((hmac[offset + 2] as u32) << 8)
        | (hmac[offset + 3] as u32);
    let code_num: u32 = (truncated & 0x7FFFFFFF) % 1_000_000;

    // 左侧补零到 6 位
    format!("{:06}", code_num)
}

/// bi_get_otp_code 生成 TOTP 6 位验证码。
///
/// 用法：
///   getOtpCode(secret)              — 用当前时间
///   getOtpCode(secret, timestamp)   — 指定 Unix 时间戳（秒）
///
/// secret 为 Base32 编码的密钥字符串（如 "JBSWY3DPEHPK3PXP"）。
/// 返回 6 位数字字符串，左侧补零。
fn bi_get_otp_code(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let secret_str = bh::as_str(args, 0, "getOtpCode")?;

    // 时间戳：可选，默认当前
    let timestamp: i64 = match args.get(1) {
        Some(_) => bh::as_int(args, 1, "getOtpCode")?,
        None => SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .map_err(|e| crate::value::error_value(format!(
                "getOtpCode() 获取系统时间失败: {} (可能原因：系统时钟异常)", e,
            )))?,
    };

    // Base32 解码
    let secret_bytes = base32_decode(secret_str).ok_or_else(|| crate::value::error_value(format!(
        "getOtpCode() Base32 解码失败: '{}' (可能原因：包含非 Base32 字符；标准字母表为 A-Z, 2-7)",
        secret_str,
    )))?;

    if secret_bytes.is_empty() {
        return Err(crate::value::error_value(
            "getOtpCode() 解码后密钥为空 (可能原因：secret 字符串仅含填充/空白字符)",
        ));
    }

    Ok(Value::str_from(generate_totp(&secret_bytes, timestamp)))
}

/// bi_check_otp_code 验证 TOTP 码。
///
/// 用法：
///   checkOtpCode(secret, code)              — 用当前时间验证
///   checkOtpCode(secret, code, timestamp)   — 指定 Unix 时间戳验证
///
/// secret 为 Base32 密钥，code 为 6 位字符串。
/// 返回 bool。
fn bi_check_otp_code(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let secret_str = bh::as_str(args, 0, "checkOtpCode")?;
    let code = bh::as_str(args, 1, "checkOtpCode")?;

    let timestamp: i64 = match args.get(2) {
        Some(_) => bh::as_int(args, 2, "checkOtpCode")?,
        None => SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .map_err(|e| crate::value::error_value(format!(
                "checkOtpCode() 获取系统时间失败: {} (可能原因：系统时钟异常)", e,
            )))?,
    };

    // Base32 解码
    let secret_bytes = base32_decode(secret_str).ok_or_else(|| crate::value::error_value(format!(
        "checkOtpCode() Base32 解码失败: '{}' (可能原因：包含非 Base32 字符；标准字母表为 A-Z, 2-7)",
        secret_str,
    )))?;

    if secret_bytes.is_empty() {
        return Err(crate::value::error_value(
            "checkOtpCode() 解码后密钥为空 (可能原因：secret 字符串仅含填充/空白字符)",
        ));
    }

    // 生成当前 OTP
    let expected = generate_totp(&secret_bytes, timestamp);

    // 比较时大小写不敏感无意义，但为防 AI 误传前后空白，先 trim
    let code_trimmed = code.trim();
    Ok(Value::Bool(expected == code_trimmed))
}
