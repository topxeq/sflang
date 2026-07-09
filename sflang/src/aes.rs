//! aes.rs — AES 加密自实现（纯标准库，无第三方依赖）
//!
//! 支持 AES-128/192/256，CBC 模式，PKCS7 填充。
//! 密钥长度决定 AES 变体：16字节=128, 24字节=192, 32字节=256。
//!
//! 加密格式：[16字节随机IV][加密数据]
//!
//! 参考标准：FIPS 197 (AES), RFC 3602 (CBC), RFC 5652 (PKCS7)

/// AES 加密的 S-Box（替换表）。
const SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

/// AES 解密的逆 S-Box。
const INV_SBOX: [u8; 256] = [
    0x52, 0x09, 0x6a, 0xd5, 0x30, 0x36, 0xa5, 0x38, 0xbf, 0x40, 0xa3, 0x9e, 0x81, 0xf3, 0xd7, 0xfb,
    0x7c, 0xe3, 0x39, 0x82, 0x9b, 0x2f, 0xff, 0x87, 0x34, 0x8e, 0x43, 0x44, 0xc4, 0xde, 0xe9, 0xcb,
    0x54, 0x7b, 0x94, 0x32, 0xa6, 0xc2, 0x23, 0x3d, 0xee, 0x4c, 0x95, 0x0b, 0x42, 0xfa, 0xc3, 0x4e,
    0x08, 0x2e, 0xa1, 0x66, 0x28, 0xd9, 0x24, 0xb2, 0x76, 0x5b, 0xa2, 0x49, 0x6d, 0x8b, 0xd1, 0x25,
    0x72, 0xf8, 0xf6, 0x64, 0x86, 0x68, 0x98, 0x16, 0xd4, 0xa4, 0x5c, 0xcc, 0x5d, 0x65, 0xb6, 0x92,
    0x6c, 0x70, 0x48, 0x50, 0xfd, 0xed, 0xb9, 0xda, 0x5e, 0x15, 0x46, 0x57, 0xa7, 0x8d, 0x9d, 0x84,
    0x90, 0xd8, 0xab, 0x00, 0x8c, 0xbc, 0xd3, 0x0a, 0xf7, 0xe4, 0x58, 0x05, 0xb8, 0xb3, 0x45, 0x06,
    0xd0, 0x2c, 0x1e, 0x8f, 0xca, 0x3f, 0x0f, 0x02, 0xc1, 0xaf, 0xbd, 0x03, 0x01, 0x13, 0x8a, 0x6b,
    0x3a, 0x91, 0x11, 0x41, 0x4f, 0x67, 0xdc, 0xea, 0x97, 0xf2, 0xcf, 0xce, 0xf0, 0xb4, 0xe6, 0x73,
    0x96, 0xac, 0x74, 0x22, 0xe7, 0xad, 0x35, 0x85, 0xe2, 0xf9, 0x37, 0xe8, 0x1c, 0x75, 0xdf, 0x6e,
    0x47, 0xf1, 0x1a, 0x71, 0x1d, 0x29, 0xc5, 0x89, 0x6f, 0xb7, 0x62, 0x0e, 0xaa, 0x18, 0xbe, 0x1b,
    0xfc, 0x56, 0x3e, 0x4b, 0xc6, 0xd2, 0x79, 0x20, 0x9a, 0xdb, 0xc0, 0xfe, 0x78, 0xcd, 0x5a, 0xf4,
    0x1f, 0xdd, 0xa8, 0x33, 0x88, 0x07, 0xc7, 0x31, 0xb1, 0x12, 0x10, 0x59, 0x27, 0x80, 0xec, 0x5f,
    0x60, 0x51, 0x7f, 0xa9, 0x19, 0xb5, 0x4a, 0x0d, 0x2d, 0xe5, 0x7a, 0x9f, 0x93, 0xc9, 0x9c, 0xef,
    0xa0, 0xe0, 0x3b, 0x4d, 0xae, 0x2a, 0xf5, 0xb0, 0xc8, 0xeb, 0xbb, 0x3c, 0x83, 0x53, 0x99, 0x61,
    0x17, 0x2b, 0x04, 0x7e, 0xba, 0x77, 0xd6, 0x26, 0xe1, 0x69, 0x14, 0x63, 0x55, 0x21, 0x0c, 0x7d,
];

/// AES 轮常数（用于密钥扩展）。
const RCON: [u8; 11] = [
    0x00, 0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36,
];

/// xtime GF(2^8) 乘法辅助函数（保留用于参考，gmul 已替代）。
#[allow(dead_code)]
fn xtime(b: u8) -> u8 {
    (b << 1) ^ if b & 0x80 != 0 { 0x1b } else { 0x00 }
}

/// GF(2^8) 乘法。
fn gmul(mut a: u8, mut b: u8) -> u8 {
    let mut p: u8 = 0;
    for _ in 0..8 {
        if b & 1 != 0 { p ^= a; }
        let hi = a & 0x80;
        a <<= 1;
        if hi != 0 { a ^= 0x1b; }
        b >>= 1;
    }
    p
}

/// key_expansion 密钥扩展，生成轮密钥。
///
/// 返回展开后的密钥字（每字 4 字节，共 Nb*(Nr+1) 字 = 4*(Nr+1)*4 字节）。
fn key_expansion(key: &[u8]) -> Vec<u8> {
    let nk = key.len() / 4;  // 密钥字数：4/6/8
    let nr = nk + 6;          // 轮数：10/12/14
    let total_words = 4 * (nr + 1);
    let mut w = vec![0u8; total_words * 4];

    // 复制原始密钥
    w[..key.len()].copy_from_slice(key);

    for i in nk..total_words {
        let mut temp = [w[(i - 1) * 4], w[(i - 1) * 4 + 1], w[(i - 1) * 4 + 2], w[(i - 1) * 4 + 3]];

        if i % nk == 0 {
            // RotWord
            temp = [temp[1], temp[2], temp[3], temp[0]];
            // SubWord
            for t in temp.iter_mut() { *t = SBOX[*t as usize]; }
            // Rcon
            temp[0] ^= RCON[i / nk];
        } else if nk > 6 && i % nk == 4 {
            // SubWord
            for t in temp.iter_mut() { *t = SBOX[*t as usize]; }
        }

        for j in 0..4 {
            w[i * 4 + j] = w[(i - nk) * 4 + j] ^ temp[j];
        }
    }

    w
}

/// sub_bytes 字节替换。
fn sub_bytes(state: &mut [u8; 16]) {
    for b in state.iter_mut() { *b = SBOX[*b as usize]; }
}

/// inv_sub_bytes 逆字节替换。
fn inv_sub_bytes(state: &mut [u8; 16]) {
    for b in state.iter_mut() { *b = INV_SBOX[*b as usize]; }
}

/// shift_rows 行移位。
fn shift_rows(state: &mut [u8; 16]) {
    // 行1: 不移位
    // 行2: 左移1
    let t = state[1]; state[1] = state[5]; state[5] = state[9]; state[9] = state[13]; state[13] = t;
    // 行3: 左移2
    let t1 = state[2]; let t2 = state[6];
    state[2] = state[10]; state[6] = state[14]; state[10] = t1; state[14] = t2;
    // 行4: 左移3
    let t = state[3]; state[3] = state[15]; state[15] = state[11]; state[11] = state[7]; state[7] = t;
}

/// inv_shift_rows 逆行移位。
fn inv_shift_rows(state: &mut [u8; 16]) {
    // 行2: 右移1
    let t = state[13]; state[13] = state[9]; state[9] = state[5]; state[5] = state[1]; state[1] = t;
    // 行3: 右移2
    let t1 = state[10]; let t2 = state[14];
    state[10] = state[2]; state[14] = state[6]; state[2] = t1; state[6] = t2;
    // 行4: 右移3
    let t = state[7]; state[7] = state[11]; state[11] = state[15]; state[15] = state[3]; state[3] = t;
}

/// mix_columns 列混淆。
fn mix_columns(state: &mut [u8; 16]) {
    for c in 0..4 {
        let s0 = state[c * 4];
        let s1 = state[c * 4 + 1];
        let s2 = state[c * 4 + 2];
        let s3 = state[c * 4 + 3];
        state[c * 4]     = gmul(s0, 2) ^ gmul(s1, 3) ^ s2 ^ s3;
        state[c * 4 + 1] = s0 ^ gmul(s1, 2) ^ gmul(s2, 3) ^ s3;
        state[c * 4 + 2] = s0 ^ s1 ^ gmul(s2, 2) ^ gmul(s3, 3);
        state[c * 4 + 3] = gmul(s0, 3) ^ s1 ^ s2 ^ gmul(s3, 2);
    }
}

/// inv_mix_columns 逆列混淆。
fn inv_mix_columns(state: &mut [u8; 16]) {
    for c in 0..4 {
        let s0 = state[c * 4];
        let s1 = state[c * 4 + 1];
        let s2 = state[c * 4 + 2];
        let s3 = state[c * 4 + 3];
        state[c * 4]     = gmul(s0, 0x0e) ^ gmul(s1, 0x0b) ^ gmul(s2, 0x0d) ^ gmul(s3, 0x09);
        state[c * 4 + 1] = gmul(s0, 0x09) ^ gmul(s1, 0x0e) ^ gmul(s2, 0x0b) ^ gmul(s3, 0x0d);
        state[c * 4 + 2] = gmul(s0, 0x0d) ^ gmul(s1, 0x09) ^ gmul(s2, 0x0e) ^ gmul(s3, 0x0b);
        state[c * 4 + 3] = gmul(s0, 0x0b) ^ gmul(s1, 0x0d) ^ gmul(s2, 0x09) ^ gmul(s3, 0x0e);
    }
}

/// add_round_key 轮密钥加。
fn add_round_key(state: &mut [u8; 16], round_key: &[u8]) {
    for i in 0..16 { state[i] ^= round_key[i]; }
}

/// encrypt_block 加密单个 16 字节块。
fn encrypt_block(block: &[u8; 16], expanded_key: &[u8], nr: usize) -> [u8; 16] {
    let mut state = *block;
    add_round_key(&mut state, &expanded_key[0..16]);

    for round in 1..nr {
        sub_bytes(&mut state);
        shift_rows(&mut state);
        mix_columns(&mut state);
        add_round_key(&mut state, &expanded_key[round * 16..round * 16 + 16]);
    }

    // 最后一轮不 mix_columns
    sub_bytes(&mut state);
    shift_rows(&mut state);
    add_round_key(&mut state, &expanded_key[nr * 16..nr * 16 + 16]);

    state
}

/// decrypt_block 解密单个 16 字节块。
fn decrypt_block(block: &[u8; 16], expanded_key: &[u8], nr: usize) -> [u8; 16] {
    let mut state = *block;
    add_round_key(&mut state, &expanded_key[nr * 16..nr * 16 + 16]);

    for round in (1..nr).rev() {
        inv_shift_rows(&mut state);
        inv_sub_bytes(&mut state);
        add_round_key(&mut state, &expanded_key[round * 16..round * 16 + 16]);
        inv_mix_columns(&mut state);
    }

    inv_shift_rows(&mut state);
    inv_sub_bytes(&mut state);
    add_round_key(&mut state, &expanded_key[0..16]);

    state
}

/// pkcs7_pad PKCS7 填充。
fn pkcs7_pad(data: &[u8]) -> Vec<u8> {
    let pad_len = 16 - (data.len() % 16);
    let mut padded = data.to_vec();
    padded.extend(std::iter::repeat(pad_len as u8).take(pad_len));
    padded
}

/// pkcs7_unpad PKCS7 去填充。
fn pkcs7_unpad(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.is_empty() || data.len() % 16 != 0 {
        return Err("数据长度不是 16 的倍数".to_string());
    }
    let pad_len = *data.last().unwrap() as usize;
    if pad_len == 0 || pad_len > 16 {
        return Err("无效的 PKCS7 填充".to_string());
    }
    // 验证填充
    for i in 0..pad_len {
        if data[data.len() - 1 - i] != pad_len as u8 {
            return Err("无效的 PKCS7 填充".to_string());
        }
    }
    Ok(data[..data.len() - pad_len].to_vec())
}

/// aes_cbc_encrypt AES-CBC 加密。
///
/// key 长度必须是 16/24/32 字节。iv 长度必须 16 字节。
/// 返回加密后的数据（长度与输入对齐到 16 字节）。
pub fn aes_cbc_encrypt(data: &[u8], key: &[u8], iv: &[u8; 16]) -> Result<Vec<u8>, String> {
    if key.len() != 16 && key.len() != 24 && key.len() != 32 {
        return Err(format!("密钥长度必须为 16/24/32 字节，实际 {}", key.len()));
    }
    let nr = key.len() / 4 + 6;
    let expanded = key_expansion(key);
    let padded = pkcs7_pad(data);

    let mut result = Vec::with_capacity(padded.len());
    let mut prev_block = *iv;

    for chunk in padded.chunks(16) {
        let mut block = [0u8; 16];
        // XOR with previous ciphertext (or IV)
        for i in 0..16 { block[i] = chunk[i] ^ prev_block[i]; }
        let encrypted = encrypt_block(&block, &expanded, nr);
        result.extend_from_slice(&encrypted);
        prev_block = encrypted;
    }

    Ok(result)
}

/// aes_cbc_decrypt AES-CBC 解密。
///
/// key 长度必须是 16/24/32 字节。iv 长度必须 16 字节。
pub fn aes_cbc_decrypt(data: &[u8], key: &[u8], iv: &[u8; 16]) -> Result<Vec<u8>, String> {
    if key.len() != 16 && key.len() != 24 && key.len() != 32 {
        return Err(format!("密钥长度必须为 16/24/32 字节，实际 {}", key.len()));
    }
    if data.is_empty() || data.len() % 16 != 0 {
        return Err("密文长度必须是 16 的倍数".to_string());
    }
    let nr = key.len() / 4 + 6;
    let expanded = key_expansion(key);

    let mut result = Vec::with_capacity(data.len());
    let mut prev_block = *iv;

    for chunk in data.chunks(16) {
        let mut block = [0u8; 16];
        block.copy_from_slice(chunk);
        let decrypted = decrypt_block(&block, &expanded, nr);
        // XOR with previous ciphertext (or IV)
        for i in 0..16 { result.push(decrypted[i] ^ prev_block[i]); }
        prev_block = block;
    }

    pkcs7_unpad(&result)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NIST FIPS-197 Appendix B 测试向量（AES-128）
    /// key: 000102030405060708090a0b0c0d0e0f
    /// plaintext: 00112233445566778899aabbccddeeff
    /// ciphertext: 69c4e0d86a7b0430d8cdb78070b4c55a
    #[test]
    fn test_aes128_encrypt() {
        let key = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
            0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
        ];
        let plaintext = [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
            0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
        ];
        let expected = [
            0x69, 0xc4, 0xe0, 0xd8, 0x6a, 0x7b, 0x04, 0x30,
            0xd8, 0xcd, 0xb7, 0x80, 0x70, 0xb4, 0xc5, 0x5a,
        ];

        let expanded = key_expansion(&key);
        let result = encrypt_block(&plaintext, &expanded, 10);
        assert_eq!(result, expected);

        // 解密验证
        let decrypted = decrypt_block(&expected, &expanded, 10);
        assert_eq!(decrypted, plaintext);
    }

    /// NIST FIPS-197 Appendix C.3 测试向量（AES-256）
    #[test]
    fn test_aes256_encrypt() {
        let key = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
            0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
            0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
            0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
        ];
        let plaintext = [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
            0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
        ];
        let expected = [
            0x8e, 0xa2, 0xb7, 0xca, 0x51, 0x67, 0x45, 0xbf,
            0xea, 0xfc, 0x49, 0x90, 0x4b, 0x49, 0x60, 0x89,
        ];

        let expanded = key_expansion(&key);
        let result = encrypt_block(&plaintext, &expanded, 14);
        assert_eq!(result, expected);

        let decrypted = decrypt_block(&expected, &expanded, 14);
        assert_eq!(decrypted, plaintext);
    }

    /// CBC 模式加解密回环测试
    #[test]
    fn test_cbc_roundtrip() {
        let key = [0u8; 32]; // AES-256，全零密钥
        let iv = [0u8; 16];
        let data = b"Hello, AES CBC encryption test!";

        let encrypted = aes_cbc_encrypt(data, &key, &iv).unwrap();
        let decrypted = aes_cbc_decrypt(&encrypted, &key, &iv).unwrap();
        assert_eq!(decrypted, data);
    }
}
