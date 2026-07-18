//! txde.rs — TXDE 加密系列（对标 Charlang/tkc 的 TXTE/TXDEE/TXDEF/TXDEM）
//!
//! 注意：这些是 Charlang 自定义的混淆/加密格式，不是标准加密算法。
//! 实现目的是与 Charlang 的已有加密数据兼容。
//!
//! 格式说明：
//!   TXTE — 文本加法混淆 + hex 编码输出
//!   TXDEE — 4字节盐帧数据混淆
//!   TXDEF — 默认格式，key 派生盐长度，可选 //TXDEF# 头
//!   TXDEM — 双密码流密码，32字节种子，magic bytes {0x74,0x04,0x05}

use std::sync::Arc;

use crate::function::BuiltinDoc;
use crate::value::Value;

// ---- 辅助 ----

/// sum_bytes 计算字节切片的 u8 和（对标 tkc SumBytes）。
fn sum_bytes(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |acc, &b| acc.wrapping_add(b))
}

/// random_byte 生成一个伪随机字节。
fn random_byte() -> u8 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0) as u64;
    // 用纳秒时间做简单随机
    let seed = nanos.wrapping_mul(2654435761u64);
    (seed >> 56) as u8
}

/// random_bytes 生成多个伪随机字节。
fn random_bytes(n: usize) -> Vec<u8> {
    let mut result = Vec::with_capacity(n);
    for _ in 0..n {
        result.push(random_byte());
        // 微小延迟让纳秒变化
        std::thread::sleep(std::time::Duration::from_nanos(1));
    }
    result
}

/// hex_encode 大写 hex 编码。
fn hex_encode_upper(data: &[u8]) -> String {
    data.iter().map(|b| format!("{:02X}", b)).collect()
}

/// hex_decode hex 解码（忽略大小写和空白）。
fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    let clean: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
    if clean.len() % 2 != 0 { return None; }
    let mut result = Vec::with_capacity(clean.len() / 2);
    let bytes = clean.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = (bytes[i] as char).to_digit(16)?;
        let lo = (bytes[i + 1] as char).to_digit(16)?;
        result.push(((hi << 4) | lo) as u8);
        i += 2;
    }
    Some(result)
}

// ---- TXTE ----

/// encrypt_string_txte 文本加密（加法混淆 + hex 输出）。
pub fn encrypt_string_txte(text: &str, code: &str) -> String {
    if text.is_empty() { return String::new(); }
    let code = if code.is_empty() { "topxeq" } else { code };
    let src = text.as_bytes();
    let code_bytes = code.as_bytes();
    let code_len = code_bytes.len();

    let mut out = Vec::with_capacity(src.len());
    for (i, &b) in src.iter().enumerate() {
        out.push(b.wrapping_add(code_bytes[i % code_len]).wrapping_add((i + 1) as u8));
    }
    hex_encode_upper(&out)
}

/// decrypt_string_txte 文本解密。
pub fn decrypt_string_txte(hex_str: &str, code: &str) -> String {
    if hex_str.is_empty() { return String::new(); }
    let code = if code.is_empty() { "topxeq" } else { code };
    let src = match hex_decode(hex_str) { Some(v) => v, None => return String::new() };
    let code_bytes = code.as_bytes();
    let code_len = code_bytes.len();

    let mut out = Vec::with_capacity(src.len());
    for (i, &b) in src.iter().enumerate() {
        out.push(b.wrapping_sub(code_bytes[i % code_len]).wrapping_sub((i + 1) as u8));
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---- TXDEE ----

/// encrypt_data_txdee 数据加密（4字节盐帧）。
pub fn encrypt_data_txdee(src: &[u8], code: &str) -> Vec<u8> {
    if src.is_empty() { return src.to_vec(); }
    let code = if code.is_empty() { "topxeq" } else { code };
    let code_bytes = code.as_bytes();
    let code_len = code_bytes.len();
    let data_len = src.len();

    let mut buf = vec![0u8; data_len + 4];
    buf[0] = random_byte();
    buf[1] = random_byte();

    for (i, &b) in src.iter().enumerate() {
        buf[2 + i] = b.wrapping_add(code_bytes[i % code_len]).wrapping_add((i + 1) as u8).wrapping_add(buf[1]);
    }

    buf[data_len + 2] = random_byte();
    buf[data_len + 3] = random_byte();
    buf
}

/// decrypt_data_txdee 数据解密。
pub fn decrypt_data_txdee(src: &[u8], code: &str) -> Option<Vec<u8>> {
    if src.len() < 4 { return None; }
    let code = if code.is_empty() { "topxeq" } else { code };
    let code_bytes = code.as_bytes();
    let code_len = code_bytes.len();
    let data_len = src.len() - 4;

    let mut buf = Vec::with_capacity(data_len);
    for i in 0..data_len {
        buf.push(src[2 + i].wrapping_sub(code_bytes[i % code_len]).wrapping_sub((i + 1) as u8).wrapping_sub(src[1]));
    }
    Some(buf)
}

// ---- TXDEF ----

const TXDEF_HEAD: &[u8] = b"//TXDEF#";

/// encrypt_data_txdef 数据加密（默认格式，key 派生盐长度）。
///
/// code 可以是密码字符串。add_head 为 true 时添加 //TXDEF# 头。
pub fn encrypt_data_txdef(src: &[u8], code: &str, add_head: bool) -> Vec<u8> {
    if src.is_empty() { return src.to_vec(); }
    let code = if code.is_empty() { "topxeq" } else { code };
    let code_bytes = code.as_bytes();
    let code_len = code_bytes.len();

    let sum = sum_bytes(code_bytes) as usize;
    let add_len = (sum % 5) + 2;
    let enc_index = sum % add_len;

    let head_len = if add_head { TXDEF_HEAD.len() } else { 0 };
    let data_len = src.len();

    let mut buf = vec![0u8; data_len + add_len + head_len];

    // 头
    if add_head {
        buf[..head_len].copy_from_slice(TXDEF_HEAD);
    }

    // 盐
    for i in 0..add_len {
        buf[i + head_len] = random_byte();
    }

    // 加密数据
    for (i, &b) in src.iter().enumerate() {
        buf[add_len + i + head_len] = b
            .wrapping_add(code_bytes[i % code_len])
            .wrapping_add((i + 1) as u8)
            .wrapping_add(buf[enc_index + head_len]);
    }

    buf
}

/// decrypt_data_txdef 数据解密。
pub fn decrypt_data_txdef(src: &[u8], code: &str) -> Option<Vec<u8>> {
    let code = if code.is_empty() { "topxeq" } else { code };
    let code_bytes = code.as_bytes();
    let code_len = code_bytes.len();

    let sum = sum_bytes(code_bytes) as usize;
    let add_len = (sum % 5) + 2;
    let enc_index = sum % add_len;

    // 去除 //TXDEF# 头
    let data = if src.starts_with(TXDEF_HEAD) { &src[TXDEF_HEAD.len()..] } else { src };
    if data.len() < add_len { return None; }

    let actual_data_len = data.len() - add_len;
    let mut buf = Vec::with_capacity(actual_data_len);

    for i in 0..actual_data_len {
        buf.push(data[add_len + i]
            .wrapping_sub(code_bytes[i % code_len])
            .wrapping_sub((i + 1) as u8)
            .wrapping_sub(data[enc_index]));
    }
    Some(buf)
}

/// is_encrypted_txdef 检测是否以 //TXDEF# 开头。
pub fn is_encrypted_txdef(data: &[u8]) -> bool {
    data.starts_with(TXDEF_HEAD)
}

// ---- TXDEM ----

const TXDEM_SEED_LEN: usize = 32;
const TXDEM_MAGIC: [u8; 3] = [0x74, 0x04, 0x05];

/// txdem_ext_key 密钥扩展。
fn txdem_ext_key(code: &str) -> [u8; TXDEM_SEED_LEN] {
    let code_bytes = code.as_bytes();
    let code_len = code_bytes.len();
    let mut k = [0u8; TXDEM_SEED_LEN];

    if code_len == 0 {
        for i in 0..TXDEM_SEED_LEN {
            k[i] = ((i * 7) as u8).wrapping_add(0x5A);
        }
    } else {
        for i in 0..TXDEM_SEED_LEN {
            k[i] = code_bytes[i % code_len].wrapping_add(i as u8).wrapping_add(0x5A);
        }
    }
    for i in 0..TXDEM_SEED_LEN {
        k[i] = (k[i] ^ k[(i + 7) % TXDEM_SEED_LEN]).wrapping_add(0x3C);
    }
    k
}

/// txdem_mask_seed 用密码掩码种子。
fn txdem_mask_seed(seed: &[u8; TXDEM_SEED_LEN], code: &str) -> [u8; TXDEM_SEED_LEN] {
    let k = txdem_ext_key(code);
    let mut r = [0u8; TXDEM_SEED_LEN];
    for i in 0..TXDEM_SEED_LEN {
        r[i] = seed[i] ^ k[i];
    }
    r
}

/// txdem_stream 生成密钥流。
fn txdem_stream(seed: &[u8; TXDEM_SEED_LEN], length: usize) -> Vec<u8> {
    let mut s = [0u8; TXDEM_SEED_LEN];
    s.copy_from_slice(seed);
    let mut out = Vec::with_capacity(length);
    for i in 0..length {
        s[i % TXDEM_SEED_LEN] = s[i % TXDEM_SEED_LEN]
            .wrapping_add(s[(i + 13) % TXDEM_SEED_LEN])
            ^ (i as u8)
            .wrapping_add(0x7B);
        out.push(s[i % TXDEM_SEED_LEN]);
    }
    out
}

/// txdem_seed_checksum 种子校验和。
fn txdem_seed_checksum(seed: &[u8; TXDEM_SEED_LEN]) -> u8 {
    seed.iter().fold(0u8, |acc, &b| acc ^ b)
}

/// encrypt_data_txdem 数据加密（双密码流密码）。
///
/// code_a 和 code_b 是两个密码，任一正确都可解密。
pub fn encrypt_data_txdem(src: &[u8], code_a: &str, code_b: &str) -> Vec<u8> {
    if src.is_empty() { return src.to_vec(); }
    let code_a = if code_a.is_empty() { "topxeq" } else { code_a };
    let code_b = if code_b.is_empty() { "txdem" } else { code_b };

    // 随机种子
    let mut seed = [0u8; TXDEM_SEED_LEN];
    let rand_bytes = random_bytes(TXDEM_SEED_LEN);
    seed.copy_from_slice(&rand_bytes);

    let enc_sa = txdem_mask_seed(&seed, code_a);
    let enc_sb = txdem_mask_seed(&seed, code_b);
    let stream = txdem_stream(&seed, src.len());
    let checksum = txdem_seed_checksum(&seed);

    let mut buf = Vec::with_capacity(TXDEM_SEED_LEN * 2 + 1 + src.len());
    buf.extend_from_slice(&enc_sa);
    buf.extend_from_slice(&enc_sb);
    buf.push(checksum);
    for (i, &b) in src.iter().enumerate() {
        buf.push(b.wrapping_add(stream[i]));
    }
    buf
}

/// decrypt_data_txdem 数据解密（需要 code_a 或 code_b 之一）。
pub fn decrypt_data_txdem(src: &[u8], code_a: &str) -> Option<Vec<u8>> {
    let code_a = if code_a.is_empty() { "topxeq" } else { code_a };

    // 去除 magic bytes
    let data = if src.len() >= 3 && src[0] == TXDEM_MAGIC[0] && src[1] == TXDEM_MAGIC[1] && src[2] == TXDEM_MAGIC[2] {
        &src[3..]
    } else {
        src
    };

    if data.len() < TXDEM_SEED_LEN * 2 + 1 { return None; }

    let enc_sa = &data[..TXDEM_SEED_LEN];
    let enc_sb = &data[TXDEM_SEED_LEN..TXDEM_SEED_LEN * 2];
    let stored_checksum = data[TXDEM_SEED_LEN * 2];

    let k = txdem_ext_key(code_a);

    // 尝试 code_a
    let mut seed = [0u8; TXDEM_SEED_LEN];
    for i in 0..TXDEM_SEED_LEN {
        seed[i] = enc_sa[i] ^ k[i];
    }

    // 验证校验和，失败则尝试 code_b 槽位
    if txdem_seed_checksum(&seed) != stored_checksum {
        for i in 0..TXDEM_SEED_LEN {
            seed[i] = enc_sb[i] ^ k[i];
        }
        if txdem_seed_checksum(&seed) != stored_checksum {
            return None; // 两个密码都不对
        }
    }

    let enc_data = &data[TXDEM_SEED_LEN * 2 + 1..];
    let stream = txdem_stream(&seed, enc_data.len());

    let mut buf = Vec::with_capacity(enc_data.len());
    for (i, &b) in enc_data.iter().enumerate() {
        buf.push(b.wrapping_sub(stream[i]));
    }
    Some(buf)
}

/// is_encrypted_txdem 检测 TXDEM magic bytes。
pub fn is_encrypted_txdem(data: &[u8]) -> bool {
    data.len() >= 3 && data[0] == TXDEM_MAGIC[0] && data[1] == TXDEM_MAGIC[1] && data[2] == TXDEM_MAGIC[2]
}

// ---- 注册内置函数 ----

// ---- TXTE ----

static DOC_ENCRYPT_TEXT_BY_TXTE: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "encryptTextByTXTE(text[, code]) -> string",
    summary: "用 TXTE 算法加密文本（加法混淆 + hex 编码输出）。",
    params: &[
        ("text", "要加密的字符串"),
        ("code", "可选。密码字符串，省略时默认 \"topxeq\""),
    ],
    returns: "string 加密后的 hex 字符串（空输入返回空串）",
    examples: &[
        "var s = encryptTextByTXTE(\"hello\")            // 默认密码",
        "var s2 = encryptTextByTXTE(\"hello\", \"mykey\")    // 指定密码",
    ],
    errors: &[],
};

static DOC_DECRYPT_TEXT_BY_TXTE: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "decryptTextByTXTE(hex[, code]) -> string",
    summary: "用 TXTE 算法解密文本（输入 hex 字符串，需与加密时同一密码）。",
    params: &[
        ("hex", "encryptTextByTXTE 输出的 hex 字符串"),
        ("code", "可选。密码字符串，省略时默认 \"topxeq\""),
    ],
    returns: "string 解密后的原文（空输入或 hex 解码失败返回空串）",
    examples: &[
        "decryptTextByTXTE(encryptTextByTXTE(\"hi\"))     // \"hi\"",
    ],
    errors: &["密码不匹配时返回乱码（不报错）"],
};

// ---- TXDEE ----

static DOC_ENCRYPT_DATA_BY_TXDEE: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "encryptDataByTXDEE(data[, code]) -> bytes",
    summary: "用 TXDEE 算法加密数据（4 字节随机盐帧混淆）。返回二进制 bytes。",
    params: &[
        ("data", "要加密的数据（string/bytes/byteArray）"),
        ("code", "可选。密码字符串，省略时默认 \"topxeq\""),
    ],
    returns: "bytes 加密后的二进制数据（长度 = 原长度 + 4）",
    examples: &[
        "var b = encryptDataByTXDEE(\"data\")",
        "var b2 = encryptDataByTXDEE(\"data\", \"pw\")",
    ],
    errors: &["参数类型应为 string/bytes/byteArray"],
};

static DOC_DECRYPT_DATA_BY_TXDEE: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "decryptDataByTXDEE(data[, code]) -> bytes|error",
    summary: "用 TXDEE 算法解密数据（需与加密时同一密码）。",
    params: &[
        ("data", "encryptDataByTXDEE 输出的 bytes/string/byteArray"),
        ("code", "可选。密码字符串，省略时默认 \"topxeq\""),
    ],
    returns: "bytes 解密后的原始数据；解密失败返回 error 值",
    examples: &[
        "decryptDataByTXDEE(encryptDataByTXDEE(\"x\"))   // 还原 \"x\" 的 bytes",
    ],
    errors: &["密码错误或数据损坏时返回 error 值（数据长度 < 4）"],
};

static DOC_ENCRYPT_TEXT_BY_TXDEE: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "encryptTextByTXDEE(text[, code]) -> string",
    summary: "用 TXDEE 加密文本，输出 hex 字符串（对 TXDEE 二进制结果做 hex 编码）。",
    params: &[
        ("text", "要加密的字符串"),
        ("code", "可选。密码字符串，省略时默认 \"topxeq\""),
    ],
    returns: "string hex 编码的密文",
    examples: &[
        "var s = encryptTextByTXDEE(\"hello\", \"pw\")",
    ],
    errors: &[],
};

static DOC_DECRYPT_TEXT_BY_TXDEE: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "decryptTextByTXDEE(hex[, code]) -> string",
    summary: "解密 encryptTextByTXDEE 生成的 hex 字符串（兼容 740404 前缀）。",
    params: &[
        ("hex", "encryptTextByTXDEE 输出的 hex 字符串"),
        ("code", "可选。密码字符串，省略时默认 \"topxeq\""),
    ],
    returns: "string 解密后的原文；失败返回 error 值",
    examples: &[
        "decryptTextByTXDEE(encryptTextByTXDEE(\"hi\", \"pw\"), \"pw\")   // \"hi\"",
    ],
    errors: &[
        "hex 解码失败返回 error",
        "解密失败（密码错误/数据损坏）返回 error",
    ],
};

// ---- TXDEF ----

static DOC_ENCRYPT_DATA: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "encryptData(data[, code[, \"-addHead\"]]) -> bytes",
    summary: "用 TXDEF 默认算法加密数据（key 派生盐长度）。可选添加 //TXDEF# 头。",
    params: &[
        ("data", "要加密的数据（string/bytes/byteArray）"),
        ("code", "可选。密码字符串，省略时默认 \"topxeq\""),
        ("-addHead", "可选。字符串字面量 \"-addHead\"，添加 //TXDEF# 头"),
    ],
    returns: "bytes 加密后的数据",
    examples: &[
        "var b = encryptData(\"data\")",
        "var b2 = encryptData(\"data\", \"pw\", \"-addHead\")   // 带 //TXDEF# 头",
    ],
    errors: &["参数类型应为 string/bytes/byteArray"],
};

static DOC_ENCRYPT_BYTES: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "encryptBytes(data[, code[, \"-addHead\"]]) -> bytes",
    summary: "encryptData 的别名：用 TXDEF 算法加密数据。",
    params: &[
        ("data", "要加密的数据（string/bytes/byteArray）"),
        ("code", "可选。密码字符串"),
        ("-addHead", "可选。字符串 \"-addHead\"，添加 //TXDEF# 头"),
    ],
    returns: "bytes 加密后的数据",
    examples: &["var b = encryptBytes(\"data\", \"pw\")"],
    errors: &[],
};

static DOC_DECRYPT_DATA: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "decryptData(data[, code]) -> bytes|error",
    summary: "用 TXDEF 算法解密数据（自动识别并去除 //TXDEF# 头）。",
    params: &[
        ("data", "encryptData 输出的 bytes/string/byteArray"),
        ("code", "可选。密码字符串，省略时默认 \"topxeq\""),
    ],
    returns: "bytes 解密后的原始数据；失败返回 error 值",
    examples: &[
        "decryptData(encryptData(\"x\"))   // 还原 \"x\" 的 bytes",
    ],
    errors: &["密码错误或数据损坏返回 error"],
};

static DOC_DECRYPT_BYTES: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "decryptBytes(data[, code]) -> bytes|error",
    summary: "decryptData 的别名：用 TXDEF 算法解密数据。",
    params: &[
        ("data", "encryptData/encryptBytes 的输出"),
        ("code", "可选。密码字符串"),
    ],
    returns: "bytes 解密后的数据；失败返回 error 值",
    examples: &["decryptBytes(encryptBytes(\"x\"))"],
    errors: &[],
};

static DOC_ENCRYPT_TEXT: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "encryptText(text[, code]) -> string",
    summary: "用 TXDEF 加密文本，输出 hex 字符串（不带 //TXDEF# 头）。",
    params: &[
        ("text", "要加密的字符串"),
        ("code", "可选。密码字符串，省略时默认 \"topxeq\""),
    ],
    returns: "string hex 编码的密文",
    examples: &[
        "var s = encryptText(\"hello\", \"pw\")",
    ],
    errors: &[],
};

static DOC_ENCRYPT_STR: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "encryptStr(text[, code]) -> string",
    summary: "encryptText 的别名：用 TXDEF 加密文本输出 hex 字符串。",
    params: &[
        ("text", "要加密的字符串"),
        ("code", "可选。密码字符串"),
    ],
    returns: "string hex 编码的密文",
    examples: &["encryptStr(\"hello\", \"pw\")"],
    errors: &[],
};

static DOC_DECRYPT_TEXT: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "decryptText(hex[, code]) -> string|error",
    summary: "解密 encryptText 生成的 hex 字符串（兼容 //TXDEF# 和 740404 前缀）。",
    params: &[
        ("hex", "encryptText 输出的 hex 字符串"),
        ("code", "可选。密码字符串，省略时默认 \"topxeq\""),
    ],
    returns: "string 解密后的原文；失败返回 error 值",
    examples: &[
        "decryptText(encryptText(\"hi\", \"pw\"), \"pw\")   // \"hi\"",
    ],
    errors: &["hex 解码失败或解密失败返回 error"],
};

static DOC_DECRYPT_STR: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "decryptStr(hex[, code]) -> string|error",
    summary: "decryptText 的别名：解密 TXDEF 文本密文。",
    params: &[
        ("hex", "encryptText/encryptStr 输出的 hex 字符串"),
        ("code", "可选。密码字符串"),
    ],
    returns: "string 解密后的原文；失败返回 error 值",
    examples: &["decryptStr(encryptStr(\"hi\"))"],
    errors: &[],
};

static DOC_IS_ENCRYPTED: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "isEncrypted(v) -> bool",
    summary: "检测值是否为已加密格式：string 检查 //TXDEF# 或 //TXDEM# 前缀，bytes 检查 magic bytes。",
    params: &[("v", "string 或 bytes")],
    returns: "bool：识别为加密格式返回 true",
    examples: &[
        "isEncrypted(\"//TXDEF#xxxx\")        // true",
        "isEncrypted(encryptData(\"x\"))        // 视情况",
    ],
    errors: &["非 string/bytes 类型返回 false"],
};

// ---- TXDEM ----

static DOC_ENCRYPT_DATA_BY_TXDEM: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "encryptDataByTXDEM(data[, codeA[, codeB]]) -> bytes",
    summary: "用 TXDEM 双密码流密码加密数据（任一密码正确均可解密）。",
    params: &[
        ("data", "要加密的数据（string/bytes/byteArray）"),
        ("codeA", "可选。密码 A，省略时默认 \"topxeq\""),
        ("codeB", "可选。密码 B，省略时默认 \"txdem\""),
    ],
    returns: "bytes 加密后的数据（含双盐槽 + 校验和 + 密文）",
    examples: &[
        "var b = encryptDataByTXDEM(\"data\", \"pw1\", \"pw2\")",
    ],
    errors: &["参数类型应为 string/bytes/byteArray"],
};

static DOC_DECRYPT_DATA_BY_TXDEM: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "decryptDataByTXDEM(data[, codeA]) -> bytes|error",
    summary: "用 TXDEM 解密数据。提供 codeA 或 codeB 任一即可（自动尝试两个盐槽）。",
    params: &[
        ("data", "encryptDataByTXDEM 的输出（string/bytes/byteArray）"),
        ("codeA", "可选。密码 A 或 B，省略时默认 \"topxeq\""),
    ],
    returns: "bytes 解密后的数据；两个密码都不匹配返回 error 值",
    examples: &[
        "decryptDataByTXDEM(encryptDataByTXDEM(\"x\", \"pw1\", \"pw2\"), \"pw1\")  // 还原 \"x\"",
    ],
    errors: &["两个密码都不正确时返回 error 值"],
};

static DOC_ENCRYPT_TEXT_BY_TXDEM: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "encryptTextByTXDEM(text[, codeA[, codeB]]) -> string",
    summary: "用 TXDEM 双密码流密码加密文本，输出 hex 字符串。",
    params: &[
        ("text", "要加密的字符串"),
        ("codeA", "可选。密码 A，省略时默认 \"topxeq\""),
        ("codeB", "可选。密码 B，省略时默认 \"txdem\""),
    ],
    returns: "string hex 编码的密文",
    examples: &[
        "var s = encryptTextByTXDEM(\"hello\", \"pw1\", \"pw2\")",
    ],
    errors: &[],
};

static DOC_DECRYPT_TEXT_BY_TXDEM: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "decryptTextByTXDEM(hex[, codeA]) -> string|error",
    summary: "解密 encryptTextByTXDEM 的 hex 字符串（兼容 //TXDEM# 和 740405 前缀）。",
    params: &[
        ("hex", "encryptTextByTXDEM 输出的 hex 字符串"),
        ("codeA", "可选。密码 A 或 B，省略时默认 \"topxeq\""),
    ],
    returns: "string 解密后的原文；失败返回 error 值",
    examples: &[
        "decryptTextByTXDEM(encryptTextByTXDEM(\"hi\", \"pw\"), \"pw\")   // \"hi\"",
    ],
    errors: &[
        "hex 解码失败返回 error",
        "密码不正确或数据损坏返回 error",
    ],
};

/// register 注册所有 TXDE 内置函数。
pub fn register(vm: &mut crate::vm::VM) {
    // TXTE
    vm.register_builtin_doc("encryptTextByTXTE", bi_encrypt_str_txte, &DOC_ENCRYPT_TEXT_BY_TXTE);
    vm.register_builtin_doc("decryptTextByTXTE", bi_decrypt_str_txte, &DOC_DECRYPT_TEXT_BY_TXTE);
    // TXDEE
    vm.register_builtin_doc("encryptDataByTXDEE", bi_encrypt_data_txdee, &DOC_ENCRYPT_DATA_BY_TXDEE);
    vm.register_builtin_doc("decryptDataByTXDEE", bi_decrypt_data_txdee, &DOC_DECRYPT_DATA_BY_TXDEE);
    vm.register_builtin_doc("encryptTextByTXDEE", bi_encrypt_str_txdee, &DOC_ENCRYPT_TEXT_BY_TXDEE);
    vm.register_builtin_doc("decryptTextByTXDEE", bi_decrypt_str_txdee, &DOC_DECRYPT_TEXT_BY_TXDEE);
    // TXDEF
    vm.register_builtin_doc("encryptData", bi_encrypt_data_txdef, &DOC_ENCRYPT_DATA);
    vm.register_builtin_doc("encryptBytes", bi_encrypt_data_txdef, &DOC_ENCRYPT_BYTES);
    vm.register_builtin_doc("decryptData", bi_decrypt_data_txdef, &DOC_DECRYPT_DATA);
    vm.register_builtin_doc("decryptBytes", bi_decrypt_data_txdef, &DOC_DECRYPT_BYTES);
    vm.register_builtin_doc("encryptText", bi_encrypt_str_txdef, &DOC_ENCRYPT_TEXT);
    vm.register_builtin_doc("encryptStr", bi_encrypt_str_txdef, &DOC_ENCRYPT_STR);
    vm.register_builtin_doc("decryptText", bi_decrypt_str_txdef, &DOC_DECRYPT_TEXT);
    vm.register_builtin_doc("decryptStr", bi_decrypt_str_txdef, &DOC_DECRYPT_STR);
    vm.register_builtin_doc("isEncrypted", bi_is_encrypted, &DOC_IS_ENCRYPTED);
    // TXDEM
    vm.register_builtin_doc("encryptDataByTXDEM", bi_encrypt_data_txdem, &DOC_ENCRYPT_DATA_BY_TXDEM);
    vm.register_builtin_doc("decryptDataByTXDEM", bi_decrypt_data_txdem, &DOC_DECRYPT_DATA_BY_TXDEM);
    vm.register_builtin_doc("encryptTextByTXDEM", bi_encrypt_str_txdem, &DOC_ENCRYPT_TEXT_BY_TXDEM);
    vm.register_builtin_doc("decryptTextByTXDEM", bi_decrypt_str_txdem, &DOC_DECRYPT_TEXT_BY_TXDEM);
}

use crate::builtins_helpers as bh;
use crate::vm::VM;

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

// ---- TXTE 内置函数 ----

fn bi_encrypt_str_txte(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let text = bh::as_str(args, 0, "encryptTextByTXTE")?;
    let code = if args.len() > 1 { bh::as_str(args, 1, "encryptTextByTXTE")? } else { "" };
    Ok(Value::str_from(encrypt_string_txte(text, code)))
}

fn bi_decrypt_str_txte(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let hex = bh::as_str(args, 0, "decryptTextByTXTE")?;
    let code = if args.len() > 1 { bh::as_str(args, 1, "decryptTextByTXTE")? } else { "" };
    Ok(Value::str_from(decrypt_string_txte(hex, code)))
}

// ---- TXDEE 内置函数 ----

fn bi_encrypt_data_txdee(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "encryptDataByTXDEE")?;
    let data = to_bytes(&args[0])?;
    let code = if args.len() > 1 { bh::as_str(args, 1, "encryptDataByTXDEE")? } else { "" };
    Ok(Value::Bytes(Arc::new(encrypt_data_txdee(&data, code))))
}

fn bi_decrypt_data_txdee(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "decryptDataByTXDEE")?;
    let data = to_bytes(&args[0])?;
    let code = if args.len() > 1 { bh::as_str(args, 1, "decryptDataByTXDEE")? } else { "" };
    match decrypt_data_txdee(&data, code) {
        Some(v) => Ok(Value::Bytes(Arc::new(v))),
        None => Ok(crate::value::error_value("decryptDataByTXDEE() 解密失败（可能原因：密码错误或数据损坏）")),
    }
}

fn bi_encrypt_str_txdee(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let text = bh::as_str(args, 0, "encryptTextByTXDEE")?;
    let code = if args.len() > 1 { bh::as_str(args, 1, "encryptTextByTXDEE")? } else { "" };
    let encrypted = encrypt_data_txdee(text.as_bytes(), code);
    Ok(Value::str_from(hex_encode_upper(&encrypted)))
}

fn bi_decrypt_str_txdee(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let hex_str = bh::as_str(args, 0, "decryptTextByTXDEE")?;
    let code = if args.len() > 1 { bh::as_str(args, 1, "decryptTextByTXDEE")? } else { "" };
    // 去除可能的 "740404" 前缀
    let actual = if hex_str.starts_with("740404") { &hex_str[6..] } else { hex_str };
    match hex_decode(actual) {
        Some(data) => match decrypt_data_txdee(&data, code) {
            Some(v) => Ok(Value::str_from(String::from_utf8_lossy(&v).into_owned())),
            None => Ok(crate::value::error_value("decryptTextByTXDEE() 解密失败")),
        },
        None => Ok(crate::value::error_value("decryptTextByTXDEE() hex 解码失败")),
    }
}

// ---- TXDEF 内置函数 ----

fn bi_encrypt_data_txdef(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "encryptData")?;
    let data = to_bytes(&args[0])?;
    let code = if args.len() > 1 { bh::as_str(args, 1, "encryptData")? } else { "" };
    let add_head = args.iter().any(|a| matches!(a, Value::Str(s) if &**s == "-addHead"));
    Ok(Value::Bytes(Arc::new(encrypt_data_txdef(&data, code, add_head))))
}

fn bi_decrypt_data_txdef(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "decryptData")?;
    let data = to_bytes(&args[0])?;
    let code = if args.len() > 1 { bh::as_str(args, 1, "decryptData")? } else { "" };
    match decrypt_data_txdef(&data, code) {
        Some(v) => Ok(Value::Bytes(Arc::new(v))),
        None => Ok(crate::value::error_value("decryptData() 解密失败（可能原因：密码错误或数据损坏）")),
    }
}

fn bi_encrypt_str_txdef(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let text = bh::as_str(args, 0, "encryptText")?;
    let code = if args.len() > 1 { bh::as_str(args, 1, "encryptText")? } else { "" };
    let encrypted = encrypt_data_txdef(text.as_bytes(), code, false);
    Ok(Value::str_from(hex_encode_upper(&encrypted)))
}

fn bi_decrypt_str_txdef(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let hex_str = bh::as_str(args, 0, "decryptText")?;
    let code = if args.len() > 1 { bh::as_str(args, 1, "decryptText")? } else { "" };
    // 去除可能的 "740404" 或 "//TXDEF#" 前缀
    let actual = if hex_str.starts_with("740404") {
        &hex_str[6..]
    } else if hex_str.starts_with("//TXDEF#") {
        &hex_str[8..]
    } else {
        hex_str
    };
    match hex_decode(actual) {
        Some(data) => match decrypt_data_txdef(&data, code) {
            Some(v) => Ok(Value::str_from(String::from_utf8_lossy(&v).into_owned())),
            None => Ok(crate::value::error_value("decryptText() 解密失败")),
        },
        None => Ok(crate::value::error_value("decryptText() hex 解码失败")),
    }
}

fn bi_is_encrypted(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "isEncrypted")?;
    let result = match &args[0] {
        Value::Str(s) => s.starts_with("//TXDEF#") || s.starts_with("//TXDEM#"),
        Value::Bytes(b) => is_encrypted_txdef(b) || is_encrypted_txdem(b),
        _ => false,
    };
    Ok(Value::Bool(result))
}

// ---- TXDEM 内置函数 ----

fn bi_encrypt_data_txdem(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "encryptDataByTXDEM")?;
    let data = to_bytes(&args[0])?;
    let code_a = if args.len() > 1 { bh::as_str(args, 1, "encryptDataByTXDEM")? } else { "" };
    let code_b = if args.len() > 2 { bh::as_str(args, 2, "encryptDataByTXDEM")? } else { "" };
    Ok(Value::Bytes(Arc::new(encrypt_data_txdem(&data, code_a, code_b))))
}

fn bi_decrypt_data_txdem(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "decryptDataByTXDEM")?;
    let data = to_bytes(&args[0])?;
    let code_a = if args.len() > 1 { bh::as_str(args, 1, "decryptDataByTXDEM")? } else { "" };
    match decrypt_data_txdem(&data, code_a) {
        Some(v) => Ok(Value::Bytes(Arc::new(v))),
        None => Ok(crate::value::error_value("decryptDataByTXDEM() 解密失败（可能原因：密码错误或数据损坏）")),
    }
}

fn bi_encrypt_str_txdem(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let text = bh::as_str(args, 0, "encryptTextByTXDEM")?;
    let code_a = if args.len() > 1 { bh::as_str(args, 1, "encryptTextByTXDEM")? } else { "" };
    let code_b = if args.len() > 2 { bh::as_str(args, 2, "encryptTextByTXDEM")? } else { "" };
    let encrypted = encrypt_data_txdem(text.as_bytes(), code_a, code_b);
    Ok(Value::str_from(hex_encode_upper(&encrypted)))
}

fn bi_decrypt_str_txdem(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let hex_str = bh::as_str(args, 0, "decryptTextByTXDEM")?;
    let code_a = if args.len() > 1 { bh::as_str(args, 1, "decryptTextByTXDEM")? } else { "" };
    let actual = if hex_str.starts_with("740405") {
        &hex_str[6..]
    } else if hex_str.starts_with("//TXDEM#") {
        &hex_str[8..]
    } else {
        hex_str
    };
    match hex_decode(actual) {
        Some(data) => match decrypt_data_txdem(&data, code_a) {
            Some(v) => Ok(Value::str_from(String::from_utf8_lossy(&v).into_owned())),
            None => Ok(crate::value::error_value("decryptTextByTXDEM() 解密失败")),
        },
        None => Ok(crate::value::error_value("decryptTextByTXDEM() hex 解码失败")),
    }
}
