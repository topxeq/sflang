//! des_fix_test.rs — DES S 盒修复的集成测试（通过公开 API 验证）
//!
//! 背景：src/des.rs 中 S2 第 4 行曾损坏为
//! `13, 8, 10, 1, 3, 15, 4, 2, 11, 7, 12, 5, 6, 10, 9, 14`
//! （10 出现两次、0 缺失）。按 FIPS 46-3 修复为
//! `13, 8, 10, 1, 3, 15, 4, 2, 11, 6, 7, 12, 0, 5, 14, 9`。
//!
//! 本文件通过 `sflang::des::DesBlock` 公开接口验证：
//!   1. ECB 单块已知答案（全部经 OpenSSL des-ecb 独立验证，且均能区分损坏实现）
//!   2. 加解密往返一致性

use sflang::des::DesBlock;

/// hex_to_block 将 16 个十六进制字符解析为 8 字节数组
fn hex_to_block(s: &str) -> [u8; 8] {
    assert_eq!(s.len(), 16, "十六进制串长度必须为 16: {}", s);
    let mut out = [0u8; 8];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
            .unwrap_or_else(|e| panic!("非法十六进制串 {}: {}", s, e));
    }
    out
}

/// ECB 已知答案测试：4 组标准向量，全部命中 S2 第 4 行损坏区域
/// （旧损坏实现会分别给出注释中的错误密文，因此这些向量具备回归防护能力）
#[test]
fn test_des_ecb_standard_vectors() {
    // (key, plain, cipher, 旧损坏实现给出的错误密文)
    let vectors: [(&str, &str, &str, &str); 4] = [
        // OpenSSL destest.c 弱密钥向量；旧损坏实现输出 A9CC9C8606F577D5
        (
            "0101010101010101",
            "95F8A5E531312243",
            "139EB07C6E568642",
            "A9CC9C8606F577D5",
        ),
        // 《The DES Algorithm Illustrated》算例；旧损坏实现输出 85E813440F0AF005
        (
            "133457799BBCDFF1",
            "0123456789ABCDEF",
            "85E813540F0AB405",
            "85E813440F0AF005",
        ),
        // 零密钥零明文经典向量；旧损坏实现输出 30C0A138E0346AD0
        (
            "0000000000000000",
            "0000000000000000",
            "8CA64DE9C1B123A7",
            "30C0A138E0346AD0",
        ),
        // NIST SP 800-67 附录 B ECB 向量；旧损坏实现输出 A3EA548B280F6434
        (
            "7CA110454A1A6E57",
            "01A1D6D039776742",
            "690F5B0D9A26939B",
            "A3EA548B280F6434",
        ),
    ];

    for (i, (key_hex, pt_hex, ct_hex, buggy_hex)) in vectors.iter().enumerate() {
        let key = hex_to_block(key_hex);
        let pt = hex_to_block(pt_hex);
        let expected_ct = hex_to_block(ct_hex);

        let des = DesBlock::new(&key);
        let ct = des.encrypt_block(&pt);
        assert_eq!(
            ct, expected_ct,
            "向量 {} 加密结果与 FIPS 46-3 标准答案不符",
            i + 1
        );

        // 与旧损坏实现的输出必须不同，确保该向量能捕获 S2 第 4 行损坏
        assert_ne!(
            ct,
            hex_to_block(buggy_hex),
            "向量 {} 与旧损坏实现输出相同，失去回归防护能力",
            i + 1
        );

        // 解密恢复明文
        assert_eq!(des.decrypt_block(&ct), pt, "向量 {} 解密未恢复明文", i + 1);
    }
}

/// 多组密钥/明文下加解密往返一致性（含全 0、全 FF 等边界值）
#[test]
fn test_des_roundtrip_various() {
    let cases: [(&str, &str); 6] = [
        ("0123456789ABCDEF", "4E6F772069732074"),
        ("0000000000000000", "0000000000000000"),
        ("FFFFFFFFFFFFFFFF", "FFFFFFFFFFFFFFFF"),
        ("F0E1D2C3B4A59687", "1122334455667788"),
        ("DEADBEEFCAFEBABE", "0123456789ABCDEF"),
        ("0101010101010101", "95F8A5E531312243"),
    ];
    for (i, (key_hex, pt_hex)) in cases.iter().enumerate() {
        let des = DesBlock::new(&hex_to_block(key_hex));
        let pt = hex_to_block(pt_hex);
        let ct = des.encrypt_block(&pt);
        assert_eq!(des.decrypt_block(&ct), pt, "用例 {} 往返不一致", i + 1);
    }
}
