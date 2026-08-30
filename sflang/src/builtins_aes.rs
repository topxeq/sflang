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

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::builtins_helpers as bh;
use crate::function::BuiltinDoc;
use crate::value::Value;
use crate::vm::VM;

// ---- AES 函数文档 ----

static DOC_AES_ENCRYPT: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "aesEncrypt(data, key) -> bytes",
    summary: "AES-CBC 加密（PKCS7 填充），返回 [16 字节 IV][密文] 的 bytes。",
    params: &[
        ("data", "string/bytes/byteArray：明文"),
        ("key", "string/bytes：密钥，长度 16/24/32（对应 AES-128/192/256）"),
    ],
    returns: "bytes：[16 字节随机 IV][密文]，每次调用 IV 不同",
    examples: &[
        "ct := aesEncrypt(\"hello\", \"0123456789abcdef\")  → 32 字节 bytes（16 IV + 16 密文）",
    ],
    errors: &[
        "key 长度必须为 16/24/32（对应 AES-128/192/256）",
        "输出含随机 IV，每次结果不同；用 aesDecrypt 解密",
    ],
};

static DOC_AES_DECRYPT: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "aesDecrypt(data, key) -> bytes",
    summary: "AES-CBC 解密，输入须为 [16 字节 IV][密文] 格式。",
    params: &[
        ("data", "string/bytes/byteArray：[16 字节 IV][密文]（即 aesEncrypt 的输出）"),
        ("key", "string/bytes：密钥（须与加密时相同，长度 16/24/32）"),
    ],
    returns: "bytes：解密后的明文字节；密钥错误或数据损坏返回 error",
    examples: &[
        "pt := aesDecrypt(aesEncrypt(\"hello\", \"0123456789abcdef\"), \"0123456789abcdef\") → bytes(\"hello\")",
    ],
    errors: &[
        "data 至少 16 字节（IV 段），否则返回 error",
        "key 与加密时不一致或填充损坏时返回 error（不 panic）",
    ],
};

static DOC_AES_ENCRYPT_STR: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "aesEncryptStr(text, key) -> string",
    summary: "便捷字符串加密：UTF-8 字节加密后输出 base64 字符串。",
    params: &[
        ("text", "string：明文（UTF-8 编码）"),
        ("key", "string/bytes：密钥，长度 16/24/32"),
    ],
    returns: "string：标准 base64（含 IV），可直接传输/存储",
    examples: &[
        "ct := aesEncryptStr(\"hello\", \"0123456789abcdef\")  → base64 字符串",
    ],
    errors: &[
        "输出含随机 IV，每次结果不同",
        "等价于 base64(aesEncrypt(text, key))",
    ],
};

static DOC_AES_DECRYPT_STR: BuiltinDoc = BuiltinDoc {
    category: "crypto",
    signature: "aesDecryptStr(base64, key) -> string",
    summary: "便捷字符串解密：输入 aesEncryptStr 产生的 base64，返回 UTF-8 字符串。",
    params: &[
        ("base64", "string：aesEncryptStr 的输出（base64）"),
        ("key", "string/bytes：密钥（须与加密时相同）"),
    ],
    returns: "string：解密后的明文（按 UTF-8 解释）；失败返回 error",
    examples: &[
        "aesDecryptStr(aesEncryptStr(\"hello\", \"0123456789abcdef\"), \"0123456789abcdef\") → \"hello\"",
    ],
    errors: &[
        "base64 解码后数据短于 16 字节返回 error",
        "密钥错误或填充损坏返回 error（不 panic）",
    ],
};

/// register 注册 AES 内置函数。
pub fn register(vm: &mut VM) {
    vm.register_builtin_doc("aesEncrypt", bi_aes_encrypt, &DOC_AES_ENCRYPT);
    vm.register_builtin_doc("aesDecrypt", bi_aes_decrypt, &DOC_AES_DECRYPT);
    vm.register_builtin_doc("aesEncryptStr", bi_aes_encrypt_str, &DOC_AES_ENCRYPT_STR);
    vm.register_builtin_doc("aesDecryptStr", bi_aes_decrypt_str, &DOC_AES_DECRYPT_STR);
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

/// splitmix64 均匀混合函数：把线性计数器值打散成分布良好的伪随机数。
fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// IV_SEED 进程级随机种子：Once 保证进程内只初始化一次。
static IV_SEED: AtomicU64 = AtomicU64::new(0);
static IV_INIT: std::sync::Once = std::sync::Once::new();

/// IV_COUNTER 调用计数器：每次生成 IV 递增，保证进程内取值不重复（线程安全）。
static IV_COUNTER: AtomicU64 = AtomicU64::new(0);

/// iv_collect_entropy 收集多路熵源并混合为一个 u64 种子。
///
/// 熵源：
///   1. SystemTime 的秒 + 纳秒（启动时刻几乎不可能重复）；
///   2. 进程 id（区分同机多进程）；
///   3. 当前线程 id 的哈希（区分并发初始化场景）；
///   4. RandomState 哈希一个固定值（std 内部使用操作系统提供的随机种子，
///      是主要熵源；其余源用于兜底与增强）。
fn iv_collect_entropy() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hash, Hasher};

    let mut seed: u64 = 0;

    // 熵源 1：系统时间（秒 + 纳秒）
    if let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        seed ^= splitmix64(now.as_nanos() as u64);
        seed = splitmix64(seed ^ now.as_secs());
    }

    // 熵源 2：进程 id
    seed ^= splitmix64(std::process::id() as u64);

    // 熵源 3：当前线程 id（ThreadId 无数值 API，经哈希混合）
    {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        std::thread::current().id().hash(&mut h);
        seed ^= h.finish();
    }

    // 熵源 4：RandomState 的 OS 随机种子（哈希任意固定值即可，随机性来自 RandomState 本身）
    {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(0x5F1A_CA11_2026_0827);
        seed ^= h.finish();
    }

    splitmix64(seed)
}

/// 生成随机 16 字节 IV。
///
/// 旧实现仅用"当前纳秒"做 LCG 种子，可预测且同一纳秒内重复，熵严重不足。
/// 现改为多源熵混合：
///   - 进程内一次性初始化的种子（见 iv_collect_entropy：OS 随机 + 时间 + 进程/线程 id）；
///   - AtomicU64 计数器保证进程内每次调用取值不重复（线程安全）；
///   - 每次调用用新的 RandomState 哈希计数器（其密钥派生自 OS 种子）再混入；
///   - 最终经 splitmix64 展开为 16 字节。
///
/// 注意：纯标准库无法获得密码学级随机，此实现仍非 CSPRNG，
/// 但已混合操作系统熵，对 CBC 模式 IV 的常规用途足够。
fn random_iv() -> [u8; 16] {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    // 进程内一次性初始化种子（Once 保证并发下也只执行一次）
    IV_INIT.call_once(|| {
        IV_SEED.store(iv_collect_entropy(), Ordering::Relaxed);
    });

    // 每次调用：计数器递增（保证不重复），用新的 RandomState 哈希计数器
    // （其密钥派生自 OS 种子），再与进程级种子混合
    let counter = IV_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut h = RandomState::new().build_hasher();
    h.write_u64(counter);
    let mut state = IV_SEED.load(Ordering::Relaxed) ^ h.finish();

    let mut iv = [0u8; 16];
    for chunk in iv.chunks_mut(8) {
        let n = splitmix64(state).to_be_bytes();
        chunk.copy_from_slice(&n[..chunk.len()]);
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
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
        // 返回 Err（可被脚本 try-catch 捕获），与 aesEncrypt 的错误行为一致
        return Err(crate::value::error_value("aesDecrypt() 数据太短（至少需要 16 字节 IV）"));
    }
    let mut iv = [0u8; 16];
    iv.copy_from_slice(&data[..16]);
    let ciphertext = &data[16..];
    match crate::aes::aes_cbc_decrypt(ciphertext, &key, &iv) {
        Ok(plaintext) => Ok(Value::Bytes(Arc::new(plaintext))),
        // 返回 Err（可被脚本 try-catch 捕获），与 aesEncrypt 的错误行为一致
        Err(e) => Err(crate::value::error_value(format!("aesDecrypt() 解密失败: {}", e))),
    }
}

/// bi_aes_encrypt_str 便捷：字符串加密 → base64 输出。
///
/// 用法：aesEncryptStr(text, key) → base64 字符串
fn bi_aes_encrypt_str(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "aesEncryptStr")?;
    bh::require_arg(args, 1, "aesEncryptStr")?;
    let encrypted = bi_aes_encrypt(vm, args)?;
    // 转 base64（复用 builtins_encode 的统一实现，避免两处手写逻辑不一致）
    match &encrypted {
        Value::Bytes(b) => Ok(Value::str_from(crate::builtins_encode::base64_encode_bytes(b))),
        other => Ok(other.clone()), // 错误值直接返回
    }
}

/// bi_aes_decrypt_str 便捷：base64 输入 → 字符串解密。
///
/// 用法：aesDecryptStr(base64, key) → 字符串
fn bi_aes_decrypt_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "aesDecryptStr")?;
    bh::require_arg(args, 1, "aesDecryptStr")?;
    // base64 解码：复用 builtins_encode 的严格模式实现
    // （非法字符 / padding 位置错误 / 长度非 4 的倍数均报错，而非静默按 0 处理）
    let b64 = bh::as_str(args, 0, "aesDecryptStr")?;
    let data = crate::builtins_encode::base64_decode_strict(b64).map_err(|e| {
        crate::value::error_value(format!(
            "aesDecryptStr() base64 解码失败: {} (可能原因：输入不是 aesEncryptStr 产生的标准 base64)",
            e,
        ))
    })?;

    let key = to_bytes(&args[1])?;
    if data.len() < 16 {
        // 返回 Err（可被脚本 try-catch 捕获），与 aesEncrypt 的错误行为一致
        return Err(crate::value::error_value("aesDecryptStr() base64 解码后数据太短（至少需要 16 字节 IV）"));
    }
    let mut iv = [0u8; 16];
    iv.copy_from_slice(&data[..16]);
    let ciphertext = &data[16..];
    match crate::aes::aes_cbc_decrypt(ciphertext, &key, &iv) {
        Ok(plaintext) => Ok(Value::str_from(String::from_utf8_lossy(&plaintext).into_owned())),
        // 返回 Err（可被脚本 try-catch 捕获），与 aesEncrypt 的错误行为一致
        Err(e) => Err(crate::value::error_value(format!("aesDecryptStr() 解密失败: {}", e))),
    }
}
