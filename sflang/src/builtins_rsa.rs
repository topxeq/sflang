//! builtins_rsa.rs — RSA 加密内置函数（基于 rsa crate 的 BigUint 大整数运算）
//!
//! 设计要点（对标 xxlang/hlbr 的 rsaEncryptRaw / rsaEncrypt）：
//!   - rsaEncryptRaw：原始 RSA（无填充）。明文字节按 JSEncrypt 等前端库的
//!     小端约定反转后作大整数，计算 m^e mod n，输出最小宽度小写 hex——
//!     匹配服务端 JS 代码 `new RSAKey(exponent, "", modulus).encrypt(password)`
//!     的加密约定（解密方持有私钥，反转还原明文）
//!   - rsaEncrypt：标准 PKCS#1 v1.5 填充，输出定宽（等于密钥字节数）小写 hex，
//!     适配大多数标准 RSA 服务端
//!   - 模数/指数均为 hex 字符串（通常来自服务端公钥接口；奇数长度自动左补 0）
//!
//! 函数列表：
//!   rsaEncryptRaw(plaintext, hexModulus, hexExponent) — 无填充原始 RSA（反转字节序）
//!   rsaEncrypt(plaintext, hexModulus, hexExponent)    — PKCS#1 v1.5 填充 RSA

use crate::builtins_helpers as bh;
use crate::function::BuiltinDoc;
use crate::value::Value;
use crate::vm::VM;
use rsa::BigUint;

static DOC_RSA_ENCRYPT_RAW: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "rsaEncryptRaw(plaintext, hexModulus, hexExponent) -> string",
    summary: "无填充原始 RSA 加密：明文字节反转后作大整数算 m^e mod n，输出最小宽度小写 hex。",
    params: &[
        ("plaintext", "明文字符串（UTF-8 字节参与运算）"),
        ("hexModulus", "模数 n 的 hex 字符串（奇数长度自动左补 0）"),
        ("hexExponent", "指数 e 的 hex 字符串"),
    ],
    returns: "string 密文的 hex 编码（无前导零，最小宽度）",
    examples: &[
        // 对标前端 JSEncrypt 风格的服务端约定：
        "rsaEncryptRaw(password, moduleHex, \"10001\")",
    ],
    errors: &[
        "hex 含非法字符报错（仅允许 0-9a-fA-F）",
        "空模数/空指数报错；明文反转后必须小于模数",
    ],
};

static DOC_RSA_ENCRYPT: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "rsaEncrypt(plaintext, hexModulus, hexExponent) -> string",
    summary: "PKCS#1 v1.5 填充的 RSA 加密，输出定宽（密钥字节数）小写 hex。",
    params: &[
        ("plaintext", "明文字符串（UTF-8 字节，长度不得超过 密钥字节数-11）"),
        ("hexModulus", "模数 n 的 hex 字符串"),
        ("hexExponent", "指数 e 的 hex 字符串（须为不超过 u64 的整数，常见为 10001）"),
    ],
    returns: "string 密文的 hex 编码（定宽，前导零保留）",
    examples: &["rsaEncrypt(password, moduleHex, \"10001\")"],
    errors: &[
        "hex 含非法字符报错",
        "指数超过 u64 范围报错（标准公钥指数 65537 合法）",
        "明文过长（超过 密钥字节数-11）报错",
    ],
};

/// parse_hex_biguint 将 hex 字符串严格解析为大整数（对标 Go hex.DecodeString 语义）。
///
/// 允许首尾空白；奇数长度左补一个 '0'；含非法字符返回 AI 友好错误。
fn parse_hex_biguint(s: &str, fn_name: &str, what: &str) -> Result<BigUint, Value> {
    let cleaned = s.trim();
    if cleaned.is_empty() {
        return Err(crate::value::error_value(format!(
            "{}() {} 为空 (可能原因：服务端公钥接口返回了空字段或字段名拼写错误)",
            fn_name, what,
        )));
    }
    if !cleaned.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(crate::value::error_value(format!(
            "{}() {} 含非法 hex 字符 (可能原因：字段值不是纯 hex；有效字符为 0-9a-fA-F，实际值前缀: {:.32})",
            fn_name, what, cleaned,
        )));
    }
    // 奇数长度左补 0（与 hlbr 的 padHex 一致），再按大端字节组装大整数
    let padded = if cleaned.len() % 2 != 0 {
        format!("0{}", cleaned)
    } else {
        cleaned.to_string()
    };
    let mut bytes = Vec::with_capacity(padded.len() / 2);
    let raw = padded.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        let hi = (raw[i] as char).to_digit(16).unwrap() as u8;
        let lo = (raw[i + 1] as char).to_digit(16).unwrap() as u8;
        bytes.push((hi << 4) | lo);
        i += 2;
    }
    Ok(BigUint::from_bytes_be(&bytes))
}

/// bytes_to_hex 字节序列转小写 hex（与 Go hex.EncodeToString 一致）。
fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// bi_rsa_encrypt_raw 无填充原始 RSA 加密（明文字节反转，JSEncrypt 小端约定）。
///
/// 步骤与 hlbr.RSAEncryptHexRaw 逐一对齐：
///   1. hex 解析模数 n 与指数 e（奇数长度左补 0）
///   2. 明文 UTF-8 字节整体反转
///   3. 反转后的字节按大端解读为大整数 m
///   4. 计算 c = m^e mod n
///   5. 输出 c 的最小宽度大端字节的小写 hex（无前导零填充）
fn bi_rsa_encrypt_raw(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let plaintext = bh::as_str(args, 0, "rsaEncryptRaw")?;
    let n = parse_hex_biguint(bh::as_str(args, 1, "rsaEncryptRaw")?, "rsaEncryptRaw", "模数(hexModulus)")?;
    let e = parse_hex_biguint(bh::as_str(args, 2, "rsaEncryptRaw")?, "rsaEncryptRaw", "指数(hexExponent)")?;
    if n.bits() < 2 {
        return Err(crate::value::error_value(
            "rsaEncryptRaw() 模数过小 (可能原因：modulus 不是 RSA 公钥模数)".to_string(),
        ));
    }
    // 明文字节反转（JSEncrypt 小端字节序约定）
    let mut pt_bytes = plaintext.as_bytes().to_vec();
    pt_bytes.reverse();
    let m = BigUint::from_bytes_be(&pt_bytes);
    if m >= n {
        return Err(crate::value::error_value(format!(
            "rsaEncryptRaw() 明文反转后不小于模数 (明文 {} 字节 vs 模数 {} 字节；可能原因：模数位数与约定不符)",
            pt_bytes.len(),
            n.to_bytes_be().len(),
        )));
    }
    // 原始 RSA：c = m^e mod n
    let c = m.modpow(&e, &n);
    Ok(Value::str_from(bytes_to_hex(&c.to_bytes_be())))
}

/// bi_rsa_encrypt 标准 PKCS#1 v1.5 填充 RSA 加密。
///
/// 输出为定宽密文（等于模数字节数）的小写 hex；明文长度不得超过 密钥字节数-11。
fn bi_rsa_encrypt(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let plaintext = bh::as_str(args, 0, "rsaEncrypt")?;
    let n = parse_hex_biguint(bh::as_str(args, 1, "rsaEncrypt")?, "rsaEncrypt", "模数(hexModulus)")?;
    let e = parse_hex_biguint(bh::as_str(args, 2, "rsaEncrypt")?, "rsaEncrypt", "指数(hexExponent)")?;
    // RsaPublicKey 要求指数为 u64（标准公钥指数 65537 均满足）
    let e_u64 = match e.to_bytes_be().try_into() {
        Ok(b) => u64::from_be_bytes(b),
        Err(_) => {
            return Err(crate::value::error_value(
                "rsaEncrypt() 指数超出 u64 范围 (可能原因：误把模数传给了指数参数)".to_string(),
            ));
        }
    };
    let pub_key = rsa::RsaPublicKey::new(n, BigUint::from(e_u64)).map_err(|err| {
        crate::value::error_value(format!("rsaEncrypt() 公钥非法: {}", err))
    })?;
    let ct = pub_key
        .encrypt(&mut rsa::rand_core::OsRng, rsa::Pkcs1v15Encrypt, plaintext.as_bytes())
        .map_err(|err| {
            crate::value::error_value(format!(
                "rsaEncrypt() 加密失败: {} (可能原因：明文过长，须不超过 密钥字节数-11)",
                err,
            ))
        })?;
    Ok(Value::str_from(bytes_to_hex(&ct)))
}

/// register 注册 RSA 加密内置函数到 VM。
pub fn register(vm: &mut VM) {
    vm.register_builtin_doc("rsaEncryptRaw", bi_rsa_encrypt_raw, &DOC_RSA_ENCRYPT_RAW);
    vm.register_builtin_doc("rsaEncrypt", bi_rsa_encrypt, &DOC_RSA_ENCRYPT);
}
