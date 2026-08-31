//! sm.rs — 国密算法自实现（SM3 哈希 + SM4 分组密码，纯标准库）
//!
//! 设计要点（AGENTS.md：算法自己写，不依赖第三方）：
//!   - SM3：256 位密码杂凑，GB/T 32905-2016。结构与 SHA-256 同族
//!     （512 位分组、64 轮、Merkle–Damgård），输出 32 字节。
//!   - SM4：128 位分组密码，GB/T 32907-2016。128 位密钥、32 轮、
//!     加解密同构（轮密钥逆序）。提供 CBC 模式 + PKCS7 填充。
//!   - 全部返回 bytes；hex/base64 等编码由上层内置函数完成。
//!
//! 算法来源：公开国标（GB/T 32905-2016 / GB/T 32907-2016），实现参照规范伪代码，
//! 并以标准附录测试向量验证（见文件末尾单元测试）。

// ============ SM3 密码杂凑 ============

/// SM3 初始值 IV（GB/T 32905-2016 §5.3.1）。
const SM3_IV: [u32; 8] = [
    0x7380166f, 0x4914b2b9, 0x172442d7, 0xda8a0600,
    0xa96f30bc, 0x163138aa, 0xe38dee4d, 0xb0fb0e4e,
];

/// sm3 布尔函数 FF：j ∈ [0,16) 为异或，[16,64) 为多数函数。
#[inline]
fn sm3_ff(j: usize, x: u32, y: u32, z: u32) -> u32 {
    if j < 16 { x ^ y ^ z } else { (x & y) | (x & z) | (y & z) }
}

/// sm3 布尔函数 GG：j ∈ [0,16) 为异或，[16,64) 为选择函数。
#[inline]
fn sm3_gg(j: usize, x: u32, y: u32, z: u32) -> u32 {
    if j < 16 { x ^ y ^ z } else { (x & y) | (!x & z) }
}

/// sm3 置换函数 P0（压缩函数用）。
#[inline]
fn sm3_p0(x: u32) -> u32 {
    x ^ x.rotate_left(9) ^ x.rotate_left(17)
}

/// sm3 置换函数 P1（消息扩展用）。
#[inline]
fn sm3_p1(x: u32) -> u32 {
    x ^ x.rotate_left(15) ^ x.rotate_left(23)
}

/// sm3 计算数据的 SM3 杂凑值，返回 32 字节 Vec<u8>。
pub fn sm3(data: &[u8]) -> Vec<u8> {
    let mut v = SM3_IV;

    // 填充：追加 0x80、补零至 56 字节（模 64）、追加 64 位大端比特长度（与 SHA-256 同族）
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    // 逐 512 位分组迭代压缩
    for chunk in msg.chunks(64) {
        // 消息扩展：W[0..16] 为分组大端字，W[16..68] 按标准递推
        let mut w = [0u32; 68];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        for i in 16..68 {
            w[i] = sm3_p1(w[i - 16] ^ w[i - 9] ^ w[i - 3].rotate_left(15))
                ^ w[i - 13].rotate_left(7)
                ^ w[i - 6];
        }

        // 压缩函数 64 轮
        let (mut a, mut b, mut c, mut d) = (v[0], v[1], v[2], v[3]);
        let (mut e, mut f, mut g, mut h) = (v[4], v[5], v[6], v[7]);
        for j in 0..64 {
            let t: u32 = if j < 16 { 0x79cc4519 } else { 0x7a879d8a };
            let ss1 = (a.rotate_left(12)
                .wrapping_add(e)
                .wrapping_add(t.rotate_left((j as u32) % 32)))
            .rotate_left(7);
            let ss2 = ss1 ^ a.rotate_left(12);
            let tt1 = sm3_ff(j, a, b, c)
                .wrapping_add(d)
                .wrapping_add(ss2)
                .wrapping_add(w[j] ^ w[j + 4]);
            let tt2 = sm3_gg(j, e, f, g)
                .wrapping_add(h)
                .wrapping_add(ss1)
                .wrapping_add(w[j]);
            d = c;
            c = b.rotate_left(9);
            b = a;
            a = tt1;
            h = g;
            g = f.rotate_left(19);
            f = e;
            e = sm3_p0(tt2);
        }
        v[0] ^= a;
        v[1] ^= b;
        v[2] ^= c;
        v[3] ^= d;
        v[4] ^= e;
        v[5] ^= f;
        v[6] ^= g;
        v[7] ^= h;
    }

    v.iter().flat_map(|x| x.to_be_bytes()).collect()
}

/// hmac_sm3 计算 HMAC-SM3（RFC 2104 构造，SM3 分组 64 字节），返回 32 字节。
///
/// 用于需要对国密体系做消息认证的场景；结构与 hmac_sha256 完全一致。
pub fn hmac_sm3(key: &[u8], message: &[u8]) -> Vec<u8> {
    const BLOCK_SIZE: usize = 64;

    // 密钥长于分组时先做 SM3 杂凑
    let key_processed: Vec<u8> = if key.len() > BLOCK_SIZE {
        sm3(key)
    } else {
        key.to_vec()
    };

    let mut k_padded = [0u8; BLOCK_SIZE];
    k_padded[..key_processed.len()].copy_from_slice(&key_processed);

    let mut ipad = [0u8; BLOCK_SIZE];
    let mut opad = [0u8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        ipad[i] = k_padded[i] ^ 0x36;
        opad[i] = k_padded[i] ^ 0x5c;
    }

    let mut inner_input = Vec::with_capacity(BLOCK_SIZE + message.len());
    inner_input.extend_from_slice(&ipad);
    inner_input.extend_from_slice(message);
    let inner_hash = sm3(&inner_input);

    let mut outer_input = Vec::with_capacity(BLOCK_SIZE + inner_hash.len());
    outer_input.extend_from_slice(&opad);
    outer_input.extend_from_slice(&inner_hash);
    sm3(&outer_input)
}

// ============ SM4 分组密码 ============

/// SM4 S 盒（GB/T 32907-2016 §6.2），按行优先 16×16。
const SM4_SBOX: [u8; 256] = [
    0xd6, 0x90, 0xe9, 0xfe, 0xcc, 0xe1, 0x3d, 0xb7, 0x16, 0xb6, 0x14, 0xc2, 0x28, 0xfb, 0x2c, 0x05,
    0x2b, 0x67, 0x9a, 0x76, 0x2a, 0xbe, 0x04, 0xc3, 0xaa, 0x44, 0x13, 0x26, 0x49, 0x86, 0x06, 0x99,
    0x9c, 0x42, 0x50, 0xf4, 0x91, 0xef, 0x98, 0x7a, 0x33, 0x54, 0x0b, 0x43, 0xed, 0xcf, 0xac, 0x62,
    0xe4, 0xb3, 0x1c, 0xa9, 0xc9, 0x08, 0xe8, 0x95, 0x80, 0xdf, 0x94, 0xfa, 0x75, 0x8f, 0x3f, 0xa6,
    0x47, 0x07, 0xa7, 0xfc, 0xf3, 0x73, 0x17, 0xba, 0x83, 0x59, 0x3c, 0x19, 0xe6, 0x85, 0x4f, 0xa8,
    0x68, 0x6b, 0x81, 0xb2, 0x71, 0x64, 0xda, 0x8b, 0xf8, 0xeb, 0x0f, 0x4b, 0x70, 0x56, 0x9d, 0x35,
    0x1e, 0x24, 0x0e, 0x5e, 0x63, 0x58, 0xd1, 0xa2, 0x25, 0x22, 0x7c, 0x3b, 0x01, 0x21, 0x78, 0x87,
    0xd4, 0x00, 0x46, 0x57, 0x9f, 0xd3, 0x27, 0x52, 0x4c, 0x36, 0x02, 0xe7, 0xa0, 0xc4, 0xc8, 0x9e,
    0xea, 0xbf, 0x8a, 0xd2, 0x40, 0xc7, 0x38, 0xb5, 0xa3, 0xf7, 0xf2, 0xce, 0xf9, 0x61, 0x15, 0xa1,
    0xe0, 0xae, 0x5d, 0xa4, 0x9b, 0x34, 0x1a, 0x55, 0xad, 0x93, 0x32, 0x30, 0xf5, 0x8c, 0xb1, 0xe3,
    0x1d, 0xf6, 0xe2, 0x2e, 0x82, 0x66, 0xca, 0x60, 0xc0, 0x29, 0x23, 0xab, 0x0d, 0x53, 0x4e, 0x6f,
    0xd5, 0xdb, 0x37, 0x45, 0xde, 0xfd, 0x8e, 0x2f, 0x03, 0xff, 0x6a, 0x72, 0x6d, 0x6c, 0x5b, 0x51,
    0x8d, 0x1b, 0xaf, 0x92, 0xbb, 0xdd, 0xbc, 0x7f, 0x11, 0xd9, 0x5c, 0x41, 0x1f, 0x10, 0x5a, 0xd8,
    0x0a, 0xc1, 0x31, 0x88, 0xa5, 0xcd, 0x7b, 0xbd, 0x2d, 0x74, 0xd0, 0x12, 0xb8, 0xe5, 0xb4, 0xb0,
    0x89, 0x69, 0x97, 0x4a, 0x0c, 0x96, 0x77, 0x7e, 0x65, 0xb9, 0xf1, 0x09, 0xc5, 0x6e, 0xc6, 0x84,
    0x18, 0xf0, 0x7d, 0xec, 0x3a, 0xdc, 0x4d, 0x20, 0x79, 0xee, 0x5f, 0x3e, 0xd7, 0xcb, 0x39, 0x48,
];

/// SM4 系统参数 FK（密钥扩展用）。
const SM4_FK: [u32; 4] = [0xa3b1bac6, 0x56aa3350, 0x677d9197, 0xb27022dc];

/// sm4 非线性变换 τ：逐字节 S 盒替换。
#[inline]
fn sm4_tau(word: u32) -> u32 {
    let b = word.to_be_bytes();
    u32::from_be_bytes([
        SM4_SBOX[b[0] as usize],
        SM4_SBOX[b[1] as usize],
        SM4_SBOX[b[2] as usize],
        SM4_SBOX[b[3] as usize],
    ])
}

/// sm4 加密轮线性变换 L。
#[inline]
fn sm4_l_enc(b: u32) -> u32 {
    b ^ b.rotate_left(2) ^ b.rotate_left(10) ^ b.rotate_left(18) ^ b.rotate_left(24)
}

/// sm4 密钥扩展线性变换 L'。
#[inline]
fn sm4_l_key(b: u32) -> u32 {
    b ^ b.rotate_left(13) ^ b.rotate_left(23)
}

/// sm4 固定参数 CK：按标准公式 CK[i] 各字节 = (4i+j)×7 mod 256 生成。
fn sm4_ck() -> [u32; 32] {
    let mut ck = [0u32; 32];
    for i in 0..32 {
        let b: [u8; 4] = std::array::from_fn(|j| (((i * 4 + j) as u32 * 7) % 256) as u8);
        ck[i] = u32::from_be_bytes(b);
    }
    ck
}

/// sm4_expand_keys 由 16 字节主密钥扩展出 32 个轮密钥。
pub fn sm4_expand_keys(key: &[u8; 16]) -> [u32; 32] {
    let mut k = [0u32; 4];
    for i in 0..4 {
        k[i] = u32::from_be_bytes([key[4 * i], key[4 * i + 1], key[4 * i + 2], key[4 * i + 3]]) ^ SM4_FK[i];
    }
    let ck = sm4_ck();
    let mut rk = [0u32; 32];
    for i in 0..32 {
        let x = k[1] ^ k[2] ^ k[3] ^ ck[i];
        let nk = k[0] ^ sm4_l_key(sm4_tau(x));
        k[0] = k[1];
        k[1] = k[2];
        k[2] = k[3];
        k[3] = nk;
        rk[i] = nk;
    }
    rk
}

/// sm4_crypt_block 对单个 16 字节分组执行 SM4 轮函数（加/解密共用，轮密钥逆序即解密）。
fn sm4_crypt_block(block: &mut [u8; 16], rk: &[u32; 32]) {
    let mut x = [0u32; 4];
    for i in 0..4 {
        x[i] = u32::from_be_bytes([block[4 * i], block[4 * i + 1], block[4 * i + 2], block[4 * i + 3]]);
    }
    for i in 0..32 {
        let y = x[0] ^ sm4_l_enc(sm4_tau(x[1] ^ x[2] ^ x[3] ^ rk[i]));
        x[0] = x[1];
        x[1] = x[2];
        x[2] = x[3];
        x[3] = y;
    }
    // 反序变换 R：输出 x3,x2,x1,x0
    for i in 0..4 {
        block[4 * i..4 * i + 4].copy_from_slice(&x[3 - i].to_be_bytes());
    }
}

/// sm4_cbc_encrypt SM4-CBC 加密（PKCS7 填充）。
///
/// key 必须为 16 字节（SM4 密钥固定 128 位）；iv 为 16 字节。
pub fn sm4_cbc_encrypt(data: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Vec<u8> {
    let rk = sm4_expand_keys(key);
    // PKCS7 填充：补 N 个值为 N 的字节（N ∈ [1,16]）
    let pad = 16 - (data.len() % 16);
    let mut buf = Vec::with_capacity(data.len() + pad);
    buf.extend_from_slice(data);
    buf.resize(data.len() + pad, pad as u8);

    let mut prev = *iv;
    for block in buf.chunks_mut(16) {
        for (b, p) in block.iter_mut().zip(prev.iter()) {
            *b ^= p;
        }
        sm4_crypt_block(block.try_into().unwrap(), &rk);
        prev.copy_from_slice(block);
    }
    buf
}

/// sm4_cbc_decrypt SM4-CBC 解密（去 PKCS7 填充）。
///
/// data 长度须为 16 的倍数且非零；填充非法返回 Err。
pub fn sm4_cbc_decrypt(data: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Result<Vec<u8>, String> {
    if data.is_empty() || data.len() % 16 != 0 {
        return Err(format!(
            "密文长度 {} 非法（须为 16 的倍数且非零）",
            data.len()
        ));
    }
    let mut rk = sm4_expand_keys(key);
    rk.reverse(); // 解密 = 轮密钥逆序

    let mut prev = *iv;
    let mut out = Vec::with_capacity(data.len());
    for block in data.chunks(16) {
        let mut b: [u8; 16] = block.try_into().unwrap();
        let cipher_prev = b;
        sm4_crypt_block(&mut b, &rk);
        for (byte, p) in b.iter_mut().zip(prev.iter()) {
            *byte ^= p;
        }
        out.extend_from_slice(&b);
        prev = cipher_prev;
    }

    // 去 PKCS7 填充
    let pad = *out.last().ok_or("密文为空")? as usize;
    if pad == 0 || pad > 16 || pad > out.len() || out[out.len() - pad..].iter().any(|&b| b as usize != pad) {
        return Err(format!(
            "PKCS7 填充非法（尾部字节 {pad}）(可能原因：密钥不正确或密文被篡改)"
        ));
    }
    out.truncate(out.len() - pad);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// bytes 转 hex 便于与标准向量比对。
    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }

    #[test]
    fn test_sm3_standard_vectors() {
        // GB/T 32905-2016 附录 A 示例 1（与 IETF draft-oscca-cfrg-sm3 一致；
        // 并经 charlang SM3Encrypt、pip gmssl 交叉验证一致）
        assert_eq!(hex(&sm3(b"abc")), "66c7f0f462eeedd9d1f2d46bdc10e4e24167c4875cf2f7a2297da02b8f4ba8e0");
        // 空串（填充边界）
        assert_eq!(hex(&sm3(b"")), "1ab21d8355cfa17f8e61194831e81a8f22bec8c728fefb747ed035eb5082aa2b");
        // 附录 A 示例 2："abcd" 重复 16 次（64 字节，恰好一个分组）
        let msg2: Vec<u8> = b"abcd".repeat(16);
        assert_eq!(hex(&sm3(&msg2)), "debe9ff92275b8a138604889c18e5a4d6fdb70e5387e5765293dcba39c0c5732");
        // 多分组（覆盖长度字段跨分组）
        let long: Vec<u8> = vec![0x61; 200];
        assert_eq!(sm3(&long).len(), 32);
    }

    #[test]
    fn test_hmac_sm3_structure() {
        // HMAC-SM3 结构性验证：与手工两段式构造一致（opad||SM3(ipad||msg)）
        let key = b"key";
        let msg = b"The quick brown fox jumps over the lazy dog";
        let manual = {
            let mut k = [0u8; 64];
            k[..key.len()].copy_from_slice(key);
            let mut inner = Vec::new();
            inner.extend(k.iter().map(|b| b ^ 0x36));
            inner.extend_from_slice(msg);
            let mut outer = Vec::new();
            outer.extend(k.iter().map(|b| b ^ 0x5c));
            outer.extend_from_slice(&sm3(&inner));
            sm3(&outer)
        };
        assert_eq!(hmac_sm3(key, msg), manual);
        // 长密钥（超过 64 字节分组）路径：先杂凑后使用
        let long_key = vec![0x5a; 100];
        assert_eq!(hmac_sm3(&long_key, b"x").len(), 32);
    }

    #[test]
    fn test_sm4_standard_vector() {
        // GB/T 32907-2016 附录 A 示例（单次加密）：官方百万次迭代示例的第 1 轮值
        let key: [u8; 16] = [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
            0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10,
        ];
        let mut block = key;
        sm4_crypt_block(&mut block, &sm4_expand_keys(&key));
        assert_eq!(hex(&block), "681edf34d206965e86b3e94f536e4246");
    }

    #[test]
    fn test_sm4_million_iterations() {
        // GB/T 32907-2016 附录 B：同一分组连续加密 1,000,000 次的标准结果
        let key: [u8; 16] = [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
            0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10,
        ];
        let rk = sm4_expand_keys(&key);
        let mut block = key;
        for _ in 0..1_000_000 {
            sm4_crypt_block(&mut block, &rk);
        }
        assert_eq!(hex(&block), "595298c7c6fd271f0402f804c33d3f66");
    }

    #[test]
    fn test_sm4_cbc_roundtrip_and_padding() {
        let key = [7u8; 16];
        let iv = [9u8; 16];
        // 空数据：应产出恰好一个填充分组
        let ct = sm4_cbc_encrypt(b"", &key, &iv);
        assert_eq!(ct.len(), 16);
        assert_eq!(sm4_cbc_decrypt(&ct, &key, &iv).unwrap(), Vec::<u8>::new());
        // 恰好 16 字节整分组：PKCS7 仍补一整组
        let data16 = vec![0x42u8; 16];
        let ct = sm4_cbc_encrypt(&data16, &key, &iv);
        assert_eq!(ct.len(), 32);
        assert_eq!(sm4_cbc_decrypt(&ct, &key, &iv).unwrap(), data16);
        // 常规长度往返（含多字节 UTF-8 内容）
        let data = "国密 SM4 测试 🎉".as_bytes().to_vec();
        let ct = sm4_cbc_encrypt(&data, &key, &iv);
        assert_eq!(sm4_cbc_decrypt(&ct, &key, &iv).unwrap(), data);
        // 密钥错误：解密失败（PKCS7 校验）
        let wrong = [8u8; 16];
        assert!(sm4_cbc_decrypt(&ct, &wrong, &iv).is_err());
        // 密文长度非 16 倍数：直接报错
        assert!(sm4_cbc_decrypt(&ct[..ct.len() - 1], &key, &iv).is_err());
    }
}
