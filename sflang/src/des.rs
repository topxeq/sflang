//! des.rs — DES 块加密与 CTR 流模式（纯标准库实现）
//!
//! 用于 xxci (goconnectit) 协议的 DES-CTR 加密。
//! 标准 FIPS 46-3 算法，无第三方依赖。
//!
//! 提供：
//!   - DesBlock: 单块加密器（8字节块）
//!   - CtrStream: CTR 模式流加密器（与 Go crypto/cipher.NewCTR 行为一致）

// IP 初始置换表（1-indexed）
const IP: [u8; 64] = [
    58, 50, 42, 34, 26, 18, 10, 2,
    60, 52, 44, 36, 28, 20, 12, 4,
    62, 54, 46, 38, 30, 22, 14, 6,
    64, 56, 48, 40, 32, 24, 16, 8,
    57, 49, 41, 33, 25, 17, 9, 1,
    59, 51, 43, 35, 27, 19, 11, 3,
    61, 53, 45, 37, 29, 21, 13, 5,
    63, 55, 47, 39, 31, 23, 15, 7,
];

// FP 逆初始置换表（IP^-1）
const FP: [u8; 64] = [
    40, 8, 48, 16, 56, 24, 64, 32,
    39, 7, 47, 15, 55, 23, 63, 31,
    38, 6, 46, 14, 54, 22, 62, 30,
    37, 5, 45, 13, 53, 21, 61, 29,
    36, 4, 44, 12, 52, 20, 60, 28,
    35, 3, 43, 11, 51, 19, 59, 27,
    34, 2, 42, 10, 50, 18, 58, 26,
    33, 1, 41, 9, 49, 17, 57, 25,
];

// E 扩展置换表（32 -> 48）
const E: [u8; 48] = [
    32, 1, 2, 3, 4, 5,
    4, 5, 6, 7, 8, 9,
    8, 9, 10, 11, 12, 13,
    12, 13, 14, 15, 16, 17,
    16, 17, 18, 19, 20, 21,
    20, 21, 22, 23, 24, 25,
    24, 25, 26, 27, 28, 29,
    28, 29, 30, 31, 32, 1,
];

// P 置换表（32 -> 32）
const P: [u8; 32] = [
    16, 7, 20, 21,
    29, 12, 28, 17,
    1, 15, 23, 26,
    5, 18, 31, 10,
    2, 8, 24, 14,
    32, 27, 3, 9,
    19, 13, 30, 6,
    22, 11, 4, 25,
];

// PC-1 密钥置换表（64 -> 56）
const PC1: [u8; 56] = [
    57, 49, 41, 33, 25, 17, 9,
    1, 58, 50, 42, 34, 26, 18,
    10, 2, 59, 51, 43, 35, 27,
    19, 11, 3, 60, 52, 44, 36,
    63, 55, 47, 39, 31, 23, 15,
    7, 62, 54, 46, 38, 30, 22,
    14, 6, 61, 53, 45, 37, 29,
    21, 13, 5, 28, 20, 12, 4,
];

// PC-2 密钥压缩置换表（56 -> 48）
const PC2: [u8; 48] = [
    14, 17, 11, 24, 1, 5,
    3, 28, 15, 6, 21, 10,
    23, 19, 12, 4, 26, 8,
    16, 7, 27, 20, 13, 2,
    41, 52, 31, 37, 47, 55,
    30, 40, 51, 45, 33, 48,
    44, 49, 39, 56, 34, 53,
    46, 42, 50, 36, 29, 32,
];

// 8 个 S 盒（每个 4x16）
const S_BOXES: [[u8; 64]; 8] = [
    // S1
    [
        14, 4, 13, 1, 2, 15, 11, 8, 3, 10, 6, 12, 5, 9, 0, 7,
        0, 15, 7, 4, 14, 2, 13, 1, 10, 6, 12, 11, 9, 5, 3, 8,
        4, 1, 14, 8, 13, 6, 2, 11, 15, 12, 9, 7, 3, 10, 5, 0,
        15, 12, 8, 2, 4, 9, 1, 7, 5, 11, 3, 14, 10, 0, 6, 13,
    ],
    // S2
    [
        15, 1, 8, 14, 6, 11, 3, 4, 9, 7, 2, 13, 12, 0, 5, 10,
        3, 13, 4, 7, 15, 2, 8, 14, 12, 0, 1, 10, 6, 9, 11, 5,
        0, 14, 7, 11, 10, 4, 13, 1, 5, 8, 12, 6, 9, 3, 2, 15,
        13, 8, 10, 1, 3, 15, 4, 2, 11, 7, 12, 5, 6, 10, 9, 14,
    ],
    // S3
    [
        10, 0, 9, 14, 6, 3, 15, 5, 1, 13, 12, 7, 11, 4, 2, 8,
        13, 7, 0, 9, 3, 4, 6, 10, 2, 8, 5, 14, 12, 11, 15, 1,
        13, 6, 4, 9, 8, 15, 3, 0, 11, 1, 2, 12, 5, 10, 14, 7,
        1, 10, 13, 0, 6, 9, 8, 7, 4, 15, 14, 3, 11, 5, 2, 12,
    ],
    // S4
    [
        7, 13, 14, 3, 0, 6, 9, 10, 1, 2, 8, 5, 11, 12, 4, 15,
        13, 8, 11, 5, 6, 15, 0, 3, 4, 7, 2, 12, 1, 10, 14, 9,
        10, 6, 9, 0, 12, 11, 7, 13, 15, 1, 3, 14, 5, 2, 8, 4,
        3, 15, 0, 6, 10, 1, 13, 8, 9, 4, 5, 11, 12, 7, 2, 14,
    ],
    // S5
    [
        2, 12, 4, 1, 7, 10, 11, 6, 8, 5, 3, 15, 13, 0, 14, 9,
        14, 11, 2, 12, 4, 7, 13, 1, 5, 0, 15, 10, 3, 9, 8, 6,
        4, 2, 1, 11, 10, 13, 7, 8, 15, 9, 12, 5, 6, 3, 0, 14,
        11, 8, 12, 7, 1, 14, 2, 13, 6, 15, 0, 9, 10, 4, 5, 3,
    ],
    // S6
    [
        12, 1, 10, 15, 9, 2, 6, 8, 0, 13, 3, 4, 14, 7, 5, 11,
        10, 15, 4, 2, 7, 12, 9, 5, 6, 1, 13, 14, 0, 11, 3, 8,
        9, 14, 15, 5, 2, 8, 12, 3, 7, 0, 4, 10, 1, 13, 11, 6,
        4, 3, 2, 12, 9, 5, 15, 10, 11, 14, 1, 7, 6, 0, 8, 13,
    ],
    // S7
    [
        4, 11, 2, 14, 15, 0, 8, 13, 3, 12, 9, 7, 5, 10, 6, 1,
        13, 0, 11, 7, 4, 9, 1, 10, 14, 3, 5, 12, 2, 15, 8, 6,
        1, 4, 11, 13, 12, 3, 7, 14, 10, 15, 6, 8, 0, 5, 9, 2,
        6, 11, 13, 8, 1, 4, 10, 7, 9, 5, 0, 15, 14, 2, 3, 12,
    ],
    // S8
    [
        13, 2, 8, 4, 6, 15, 11, 1, 10, 9, 3, 14, 5, 0, 12, 7,
        1, 15, 13, 8, 10, 3, 7, 4, 12, 5, 6, 11, 0, 14, 9, 2,
        7, 11, 4, 1, 9, 12, 14, 2, 0, 6, 10, 13, 15, 3, 5, 8,
        2, 1, 14, 7, 4, 10, 8, 13, 15, 12, 9, 0, 3, 5, 6, 11,
    ],
];

// 每轮循环左移位数
const SHIFTS: [u8; 16] = [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1];

// ---- 辅助函数 ----

/// bytes_to_bits 将字节数组转为位数组（MSB first）
fn bytes_to_bits(data: &[u8]) -> Vec<u8> {
    let mut bits = Vec::with_capacity(data.len() * 8);
    for &byte in data {
        for i in (0..8).rev() {
            bits.push((byte >> i) & 1);
        }
    }
    bits
}

/// bits_to_bytes 将位数组转为字节数组
fn bits_to_bytes(bits: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(bits.len() / 8);
    for chunk in bits.chunks(8) {
        let mut byte = 0u8;
        for i in 0..8 {
            byte |= chunk[i] << (7 - i);
        }
        bytes.push(byte);
    }
    bytes
}

/// permute 按置换表对位进行重排（表中值 1-indexed）
fn permute(input: &[u8], table: &[u8]) -> Vec<u8> {
    table.iter().map(|&pos| input[pos as usize - 1]).collect()
}

/// left_rotate 将位数组循环左移 n 位
fn left_rotate(bits: &mut Vec<u8>, n: usize) {
    for _ in 0..n {
        let first = bits.remove(0);
        bits.push(first);
    }
}

// ---- DES 块加密器 ----

/// DesBlock DES 单块加密器。
///
/// 8 字节密钥，加密 8 字节块。
pub struct DesBlock {
    // subkeys 16 轮子密钥（每轮 48 位）
    subkeys: Vec<Vec<u8>>,
}

impl DesBlock {
    /// new 从 8 字节密钥创建 DES 加密器。
    pub fn new(key: &[u8; 8]) -> Self {
        let key_bits = bytes_to_bits(key);
        // PC-1: 64 -> 56
        let permuted = permute(&key_bits, &PC1);
        // 分成左右各 28 位
        let mut left = permuted[..28].to_vec();
        let mut right = permuted[28..].to_vec();
        let mut subkeys = Vec::with_capacity(16);
        for round in 0..16 {
            left_rotate(&mut left, SHIFTS[round] as usize);
            left_rotate(&mut right, SHIFTS[round] as usize);
            let mut combined = Vec::with_capacity(56);
            combined.extend_from_slice(&left);
            combined.extend_from_slice(&right);
            // PC-2: 56 -> 48
            subkeys.push(permute(&combined, &PC2));
        }
        DesBlock { subkeys }
    }

    /// encrypt_block 加密单个 8 字节块。
    pub fn encrypt_block(&self, input: &[u8; 8]) -> [u8; 8] {
        self.process_block(input, false)
    }

    /// decrypt_block 解密单个 8 字节块。
    pub fn decrypt_block(&self, input: &[u8; 8]) -> [u8; 8] {
        self.process_block(input, true)
    }

    /// process_block 处理单个块（encrypt 为 true 时逆向使用子密钥）。
    fn process_block(&self, input: &[u8; 8], decrypt: bool) -> [u8; 8] {
        let bits = bytes_to_bits(input);
        // IP 初始置换
        let permuted = permute(&bits, &IP);
        let mut left = permuted[..32].to_vec();
        let mut right = permuted[32..].to_vec();

        for round in 0..16 {
            let old_right = right.clone();
            // f 函数
            let f_result = self.f_function(&right, if decrypt { 15 - round } else { round });
            // XOR
            for i in 0..32 {
                right[i] = left[i] ^ f_result[i];
            }
            left = old_right;
        }

        // 合并 R16 + L16（注意顺序）
        let mut combined = Vec::with_capacity(64);
        combined.extend_from_slice(&right); // R16 在前
        combined.extend_from_slice(&left); // L16 在后
        // FP 逆初始置换
        let final_bits = permute(&combined, &FP);
        let bytes = bits_to_bytes(&final_bits);
        let mut output = [0u8; 8];
        output.copy_from_slice(&bytes);
        output
    }

    /// f_function Feistel 轮函数。
    ///
    /// expand R(32) -> 48, XOR with subkey, S-box substitution, P permutation
    fn f_function(&self, r: &[u8], round: usize) -> Vec<u8> {
        // E 扩展: 32 -> 48
        let expanded = permute(r, &E);
        // XOR with subkey
        let subkey = &self.subkeys[round];
        let mut xored = Vec::with_capacity(48);
        for i in 0..48 {
            xored.push(expanded[i] ^ subkey[i]);
        }
        // S 盒替换: 48 -> 32
        let mut s_output = Vec::with_capacity(32);
        for i in 0..8 {
            let chunk = &xored[i * 6..(i + 1) * 6];
            // 行 = (chunk[0] << 1) | chunk[5]
            let row = ((chunk[0] as usize) << 1) | (chunk[5] as usize);
            // 列 = chunk[1..5] 组成的 4 位
            let col = ((chunk[1] as usize) << 3)
                | ((chunk[2] as usize) << 2)
                | ((chunk[3] as usize) << 1)
                | (chunk[4] as usize);
            let val = S_BOXES[i][row * 16 + col];
            // 输出 4 位
            for j in (0..4).rev() {
                s_output.push((val >> j) & 1);
            }
        }
        // P 置换: 32 -> 32
        permute(&s_output, &P)
    }
}

// ---- CTR 流模式 ----

/// CtrStream CTR 模式流加密器。
///
/// 与 Go crypto/cipher.NewCTR 行为一致：
/// 计数器从 IV 开始，每次加密一个块后递增（大端序，从最后一个字节开始）。
pub struct CtrStream {
    block: DesBlock,
    counter: [u8; 8],
    keystream: [u8; 8],
    pos: usize,
}

impl CtrStream {
    /// new 创建 CTR 流。
    ///
    /// key: 8 字节 DES 密钥
    /// iv: 8 字节初始向量
    pub fn new(key: &[u8; 8], iv: &[u8; 8]) -> Self {
        CtrStream {
            block: DesBlock::new(key),
            counter: *iv,
            keystream: [0; 8],
            pos: 8,
        }
    }

    /// xor_key_stream 将密钥流与 src 异或写入 dst。
    ///
    /// dst 和 src 长度必须相同。
    pub fn xor_key_stream(&mut self, dst: &mut [u8], src: &[u8]) {
        for i in 0..src.len() {
            if self.pos >= 8 {
                self.keystream = self.block.encrypt_block(&self.counter);
                // 递增计数器（大端序，从最后一个字节开始）
                for j in (0..8).rev() {
                    self.counter[j] = self.counter[j].wrapping_add(1);
                    if self.counter[j] != 0 {
                        break;
                    }
                }
                self.pos = 0;
            }
            dst[i] = src[i] ^ self.keystream[self.pos];
            self.pos += 1;
        }
    }
}

// ---- 测试 ----

#[cfg(test)]
mod tests {
    use super::*;

    /// NIST DES 测试向量
    #[test]
    fn test_des_block() {
        // 密钥 = 0x0123456789ABCDEF
        let key = [0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF];
        // 明文 = 0x4E6F772069732074
        let pt = [0x4E, 0x6F, 0x77, 0x20, 0x69, 0x73, 0x20, 0x74];
        // 密文 = 0x3FA40E8A984D4815
        let expected_ct = [0x3F, 0xA4, 0x0E, 0x8A, 0x98, 0x4D, 0x48, 0x15];

        let block = DesBlock::new(&key);
        let ct = block.encrypt_block(&pt);
        assert_eq!(ct, expected_ct, "DES 加密结果不匹配");

        let pt2 = block.decrypt_block(&ct);
        assert_eq!(pt2, pt, "DES 解密结果不匹配");
    }

    #[test]
    fn test_ctr_stream() {
        // CTR 模式：加密后解密应恢复原文
        let key = [0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF];
        let iv = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01];
        let plaintext = b"Hello, DES-CTR encryption test!";

        let mut enc = CtrStream::new(&key, &iv);
        let mut ciphertext = vec![0u8; plaintext.len()];
        enc.xor_key_stream(&mut ciphertext, plaintext);

        let mut dec = CtrStream::new(&key, &iv);
        let mut recovered = vec![0u8; ciphertext.len()];
        dec.xor_key_stream(&mut recovered, &ciphertext);

        assert_eq!(&recovered[..], &plaintext[..], "CTR 解密结果不匹配");
    }

    #[test]
    fn test_ctr_multiple_blocks() {
        // 测试超过一个块的数据
        let key = [0xFF; 8];
        let iv = [0xAA; 8];
        let plaintext = vec![0x42u8; 100]; // 12.5 个块

        let mut enc = CtrStream::new(&key, &iv);
        let mut ciphertext = vec![0u8; 100];
        enc.xor_key_stream(&mut ciphertext, &plaintext);

        let mut dec = CtrStream::new(&key, &iv);
        let mut recovered = vec![0u8; 100];
        dec.xor_key_stream(&mut recovered, &ciphertext);

        assert_eq!(&recovered[..], &plaintext[..]);
    }
}
