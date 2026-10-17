//! des.rs — DES 块加密与 CTR 流模式（纯标准库实现）
//!
//! 用于 xxci (goconnectit) 协议的 DES-CTR 加密。
//! 标准 FIPS 46-3 算法，无第三方依赖。
//!
//! 提供：
//!   - DesBlock: 单块加密器（8字节块，u64+SP 复合盒实现，每块零堆分配）
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

// 8 个 S 盒（每个 4x16，行 = 首尾两位组成的外部位，列 = 中间 4 位）
// 全部数值已逐行核对 FIPS 46-3（另经 OpenSSL des-ecb 三百组随机向量交叉验证）
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
        13, 8, 10, 1, 3, 15, 4, 2, 11, 6, 7, 12, 0, 5, 14, 9,
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

/// DesBlock DES 单块加密器（u64 + SP 复合盒实现）。
///
/// 8 字节密钥，加密 8 字节块。算术与早期位向量实现逐位等价
/// （经 NIST 向量与 Go crypto/des 互通验证），但每块零堆分配：
///   - IP/FP 用 u64 表驱动置换
///   - 每轮 E 扩展 + 子密钥异或后，直接查 8 个 SP 复合盒
///     （S 盒输出已按 P 置换预放到 32 位半字的对应位置），
///     8 次 u32 查表异或即完成一轮 f 函数
pub struct DesBlock {
    /// round_keys 每轮 48 位子密钥（低 48 位有效，6 位一组对齐 SP 盒下标）。
    round_keys: [u64; 16],
    /// sp SP 复合盒：sp[i][6 位输入] = 第 i 个 S 盒输出经 P 置换后的 32 位半字。
    /// 与密钥无关，每个实例构造时生成（8x64x4B = 2KB，微秒级）。
    sp: [[u32; 64]; 8],
}

/// make_bit_contrib 构建位贡献表：CONTRIB[k][v] = 输入第 k 字节取值 v 时
/// 对整个置换输出的贡献（u64，按 out_bits 对齐 MSB）。
///
/// 用于 IP/FP/E 的快速置换：输出 = 逐输入字节查表后按位或，
/// 把逐位串行移位链变成 4~8 次独立查表（ILP 友好）。
fn make_bit_contrib(table: &[u8], out_bits: u32) -> Vec<[u64; 256]> {
    let in_bytes = (table.len() + 7) / 8;
    let mut c = vec![[0u64; 256]; in_bytes];
    for k in 0..in_bytes {
        for v in 0..256usize {
            let mut acc = 0u64;
            for (j, &t) in table.iter().enumerate() {
                let pos = t as usize - 1; // 全局源位（0-indexed，MSB first）
                if pos / 8 != k {
                    continue;
                }
                let bit = (v >> (7 - (pos % 8))) & 1;
                acc |= (bit as u64) << (out_bits - 1 - j as u32);
            }
            c[k][v] = acc;
        }
    }
    c
}

// IP/FP/E 的位贡献表（与密钥无关，进程内构建一次）
static IP_CONTRIB: std::sync::LazyLock<Vec<[u64; 256]>> =
    std::sync::LazyLock::new(|| make_bit_contrib(&IP, 64));
static FP_CONTRIB: std::sync::LazyLock<Vec<[u64; 256]>> =
    std::sync::LazyLock::new(|| make_bit_contrib(&FP, 64));
static E_CONTRIB: std::sync::LazyLock<Vec<[u64; 256]>> =
    std::sync::LazyLock::new(|| make_bit_contrib(&E, 48));

/// make_sp_boxes 由 S 盒 + P 置换生成 8 个 SP 复合盒。
fn make_sp_boxes() -> [[u32; 64]; 8] {
    let mut sp = [[0u32; 64]; 8];
    for i in 0..8 {
        for v in 0..64usize {
            // 6 位输入 v：MSB first —— 行 = 首尾两位，列 = 中间 4 位（与位向量版一致）
            let row = (((v >> 5) & 1) << 1) | (v & 1);
            let col = (v >> 1) & 0xF;
            let val = S_BOXES[i][row * 16 + col] as u32;
            // 该 chunk 的 4 位输出在 32 位半字中的位置：第 i 组占 (31-4i)..(28-4i)
            let word = val << (28 - 4 * i);
            // 应用 P 置换（1-indexed，1 = 32 位字最高位）
            let mut out = 0u32;
            for (k, &p) in P.iter().enumerate() {
                let bit = (word >> (32 - p as u32)) & 1;
                out |= bit << (31 - k as u32);
            }
            sp[i][v] = out;
        }
    }
    sp
}

impl DesBlock {
    /// new 从 8 字节密钥创建 DES 加密器。
    pub fn new(key: &[u8; 8]) -> Self {
        // 密钥调度沿用位向量实现（每连接仅一次，性能不敏感）
        let key_bits = bytes_to_bits(key);
        // PC-1: 64 -> 56
        let permuted = permute(&key_bits, &PC1);
        // 分成左右各 28 位
        let mut left = permuted[..28].to_vec();
        let mut right = permuted[28..].to_vec();
        let mut round_keys = [0u64; 16];
        for (round, rk) in round_keys.iter_mut().enumerate() {
            left_rotate(&mut left, SHIFTS[round] as usize);
            left_rotate(&mut right, SHIFTS[round] as usize);
            let mut combined = Vec::with_capacity(56);
            combined.extend_from_slice(&left);
            combined.extend_from_slice(&right);
            // PC-2: 56 -> 48，打包为 u64 低 48 位（MSB first）
            let pc2_bits = permute(&combined, &PC2);
            let mut packed = 0u64;
            for (j, &b) in pc2_bits.iter().enumerate() {
                packed |= (b as u64) << (47 - j as u32);
            }
            *rk = packed;
        }
        DesBlock {
            round_keys,
            sp: make_sp_boxes(),
        }
    }

    /// encrypt_block 加密单个 8 字节块。
    pub fn encrypt_block(&self, input: &[u8; 8]) -> [u8; 8] {
        // IP 初始置换（位贡献表：8 次查表按位或）
        let ic = &*IP_CONTRIB;
        let mut pre = 0u64;
        for k in 0..8 {
            pre |= ic[k][input[k] as usize];
        }
        let mut left = (pre >> 32) as u32;
        let mut right = pre as u32;
        for round in 0..16 {
            let old_right = right;
            let f = self.feistel(right, round);
            right = left ^ f;
            left = old_right;
        }
        // 合并 R16 + L16（R16 在前），FP 逆初始置换（位贡献表）
        let combined = ((right as u64) << 32) | (left as u64);
        let cb = combined.to_be_bytes();
        let fc = &*FP_CONTRIB;
        let mut out = 0u64;
        for k in 0..8 {
            out |= fc[k][cb[k] as usize];
        }
        out.to_be_bytes()
    }

    /// decrypt_block 解密单个 8 字节块（子密钥逆序使用）。
    pub fn decrypt_block(&self, input: &[u8; 8]) -> [u8; 8] {
        let ic = &*IP_CONTRIB;
        let mut pre = 0u64;
        for k in 0..8 {
            pre |= ic[k][input[k] as usize];
        }
        let mut left = (pre >> 32) as u32;
        let mut right = pre as u32;
        for round in 0..16 {
            let old_right = right;
            let f = self.feistel(right, 15 - round);
            right = left ^ f;
            left = old_right;
        }
        let combined = ((right as u64) << 32) | (left as u64);
        let cb = combined.to_be_bytes();
        let fc = &*FP_CONTRIB;
        let mut out = 0u64;
        for k in 0..8 {
            out |= fc[k][cb[k] as usize];
        }
        out.to_be_bytes()
    }

    /// feistel 轮函数：E 扩展（位贡献表）-> 异或子密钥 -> 8 次 SP 盒查表异或。
    fn feistel(&self, r: u32, round: usize) -> u32 {
        let rb = r.to_be_bytes();
        let ec = &*E_CONTRIB;
        let mut e = 0u64;
        for k in 0..4 {
            e |= ec[k][rb[k] as usize];
        }
        let e = e ^ self.round_keys[round];
        // 8 个 6 位分组各查一个 SP 盒，异或合成 32 位输出
        let mut out = 0u32;
        for (i, box_row) in self.sp.iter().enumerate() {
            out |= box_row[((e >> (42 - 6 * i)) & 0x3F) as usize];
        }
        out
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

    /// S 盒表完整性防护：8 个 S 盒共 32 行，每行排序后必须恰好为 0..=15。
    ///
    /// 历史教训：S2 第 4 行曾损坏为
    /// `13, 8, 10, 1, 3, 15, 4, 2, 11, 7, 12, 5, 6, 10, 9, 14`
    /// （10 出现两次、0 缺失，正确值应为 `13, 8, 10, 1, 3, 15, 4, 2, 11, 6, 7, 12, 0, 5, 14, 9`）。
    /// 该测试可在编译期测试阶段拦截此类"重复值/缺值"型损坏。
    #[test]
    fn test_sboxes_rows_are_permutations() {
        for (box_idx, sbox) in S_BOXES.iter().enumerate() {
            for row in 0..4 {
                let mut vals: [u8; 16] = sbox[row * 16..row * 16 + 16].try_into().unwrap();
                vals.sort_unstable();
                let expected: [u8; 16] = core::array::from_fn(|i| i as u8);
                assert_eq!(
                    vals, expected,
                    "S{} 第 {} 行不是 0..=15 的排列，S 盒表已损坏",
                    box_idx + 1,
                    row + 1
                );
            }
        }
    }

    /// ECB 单块加密/解密已知答案测试（FIPS 46-3 / 公开标准向量，均经 OpenSSL 独立验证）。
    ///
    /// 注意：以下 4 组向量在旧的损坏 S2 第 4 行下全部会产生不同密文（向量均命中损坏的
    /// 第 10~16 列区域），因此可防止该类损坏再次悄悄混入：
    ///   - 向量 1 旧实现输出 A9CC9C8606F577D5
    ///   - 向量 2 旧实现输出 85E813440F0AF005
    ///   - 向量 3 旧实现输出 30C0A138E0346AD0
    ///   - 向量 4 旧实现输出 A3EA548B280F6434
    #[test]
    fn test_ecb_known_answers() {
        // 向量出处：(key_hex, plain_hex, cipher_hex, 来源说明, 旧损坏实现的错误输出)
        let vectors: [(&str, &str, &str, &str, &str); 4] = [
            // OpenSSL destest.c 中的经典弱密钥验证向量
            (
                "0101010101010101",
                "95F8A5E531312243",
                "139EB07C6E568642",
                "OpenSSL destest.c 弱密钥向量",
                "A9CC9C8606F577D5",
            ),
            // 《The DES Algorithm Illustrated》逐步算例
            (
                "133457799BBCDFF1",
                "0123456789ABCDEF",
                "85E813540F0AB405",
                "教材逐步算例",
                "85E813440F0AF005",
            ),
            // 零密钥 + 零明文经典向量
            (
                "0000000000000000",
                "0000000000000000",
                "8CA64DE9C1B123A7",
                "零密钥零明文向量",
                "30C0A138E0346AD0",
            ),
            // NIST SP 800-67 附录 B 的单块 ECB 向量
            (
                "7CA110454A1A6E57",
                "01A1D6D039776742",
                "690F5B0D9A26939B",
                "NIST SP 800-67 ECB 向量",
                "A3EA548B280F6434",
            ),
        ];

        for (i, (key_hex, pt_hex, ct_hex, src, buggy_ct)) in vectors.iter().enumerate() {
            let key: [u8; 8] = hex_to_block(key_hex);
            let pt: [u8; 8] = hex_to_block(pt_hex);
            let expected_ct: [u8; 8] = hex_to_block(ct_hex);

            let des = DesBlock::new(&key);
            let ct = des.encrypt_block(&pt);
            assert_eq!(
                ct, expected_ct,
                "向量 {} ({}): 加密结果与 FIPS 46-3 标准答案不符（若此前通过则 S 盒可能再次损坏）",
                i + 1, src
            );

            // 解密必须恢复明文
            let pt2 = des.decrypt_block(&ct);
            assert_eq!(pt2, pt, "向量 {} ({}): 解密未恢复明文", i + 1, src);

            // 确认该向量能区分损坏实现：旧损坏 S2 行下算出的错误密文不应等于标准答案
            let buggy: [u8; 8] = hex_to_block(buggy_ct);
            assert_ne!(
                ct, buggy,
                "向量 {} 缺乏区分能力，无法防护 S2 第 4 行损坏",
                i + 1
            );
        }
    }

    /// hex_to_block 将 16 个十六进制字符解析为 8 字节数组（仅测试用）。
    fn hex_to_block(s: &str) -> [u8; 8] {
        assert_eq!(s.len(), 16, "十六进制串长度必须为 16: {}", s);
        let mut out = [0u8; 8];
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
                .unwrap_or_else(|e| panic!("非法十六进制串 {}: {}", s, e));
        }
        out
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

    /// test_des_block_roundtrip_random 随机数据加解密回环（多密钥）。
    #[test]
    fn test_des_block_roundtrip_random() {
        // 简单 LCG 生成确定性伪随机数据
        let mut seed: u64 = 0x2026_1017;
        let mut next = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as u8
        };
        for _key_i in 0..4 {
            let key = [next(), next(), next(), next(), next(), next(), next(), next()];
            let block = DesBlock::new(&key);
            for _blk_i in 0..64 {
                let pt = [next(), next(), next(), next(), next(), next(), next(), next()];
                let ct = block.encrypt_block(&pt);
                assert_ne!(ct, [0u8; 8]);
                assert_eq!(block.decrypt_block(&ct), pt, "加解密回环失败");
            }
        }
    }

    /// test_ctr_throughput CTR 吞吐量基准（防止回退到慢速实现）。
    ///
    /// 4 MB 加解密：release 下阈值 1 秒（SP 盒版实测远快于此；
    /// 早期位向量实现 release 约 1 MB/s，4MB 需 4 秒，会被拦截）。
    /// debug 无优化运行放宽到 30 秒。
    #[test]
    fn test_ctr_throughput() {
        let key = [0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
        let iv = [0u8; 8];
        let mut stream = CtrStream::new(&key, &iv);
        let data = vec![0xABu8; 4 << 20]; // 4 MB
        let mut out = vec![0u8; data.len()];
        let t0 = std::time::Instant::now();
        stream.xor_key_stream(&mut out, &data);
        let el = t0.elapsed();
        let limit_ms: u128 = if cfg!(debug_assertions) { 30_000 } else { 1_000 };
        assert!(
            el.as_millis() < limit_ms,
            "DES-CTR 4MB 耗时 {}ms，性能回退（阈值 {}ms）", el.as_millis(), limit_ms,
        );
    }
}
