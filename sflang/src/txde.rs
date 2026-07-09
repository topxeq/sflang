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

/// register 注册所有 TXDE 内置函数。
pub fn register(vm: &mut crate::vm::VM) {
    // TXTE
    vm.register_builtin("encryptTextByTXTE", bi_encrypt_str_txte);
    vm.register_builtin("decryptTextByTXTE", bi_decrypt_str_txte);
    // TXDEE
    vm.register_builtin("encryptDataByTXDEE", bi_encrypt_data_txdee);
    vm.register_builtin("decryptDataByTXDEE", bi_decrypt_data_txdee);
    vm.register_builtin("encryptTextByTXDEE", bi_encrypt_str_txdee);
    vm.register_builtin("decryptTextByTXDEE", bi_decrypt_str_txdee);
    // TXDEF
    vm.register_builtin("encryptData", bi_encrypt_data_txdef);
    vm.register_builtin("encryptBytes", bi_encrypt_data_txdef);
    vm.register_builtin("decryptData", bi_decrypt_data_txdef);
    vm.register_builtin("decryptBytes", bi_decrypt_data_txdef);
    vm.register_builtin("encryptText", bi_encrypt_str_txdef);
    vm.register_builtin("encryptStr", bi_encrypt_str_txdef);
    vm.register_builtin("decryptText", bi_decrypt_str_txdef);
    vm.register_builtin("decryptStr", bi_decrypt_str_txdef);
    vm.register_builtin("isEncrypted", bi_is_encrypted);
    // TXDEM
    vm.register_builtin("encryptDataByTXDEM", bi_encrypt_data_txdem);
    vm.register_builtin("decryptDataByTXDEM", bi_decrypt_data_txdem);
    vm.register_builtin("encryptTextByTXDEM", bi_encrypt_str_txdem);
    vm.register_builtin("decryptTextByTXDEM", bi_decrypt_str_txdem);
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
