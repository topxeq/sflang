//! crypto_fix_test.rs — 加解密/编解码修复的回归测试
//!
//! 覆盖（通过脚本层运行断言，参考 tests/api_test.rs 的用法）：
//!   - aesDecrypt / aesDecryptStr 失败路径返回可捕获错误（脚本 try-catch 能捕获），
//!     且未捕获时使 VM 返回 Err；
//!   - base64UrlDecode 非法字符返回 error、base64UrlEncode/Decode 严格往返一致；
//!   - base64Decode 对 '=' 出现在中间位置（如 "AB==CDEF"）返回 error；
//!   - HMAC-SHA256 RFC 4231 case 1/2 官方向量（经脚本 bytesHex/hmacSha256Hex 断言）；
//!   - htmlDecode("&#0;") 不产生 NUL（替换为 U+FFFD）。

use std::sync::Arc;

use sflang::value::Value;
use sflang::Sflang;

// ---- 辅助函数 ----

/// eval 求值代码块并返回结果（用 IIFE 包裹，src 内需显式 return）。
fn eval(src: &str) -> Value {
    let mut sf = Sflang::new();
    let wrapped = format!("func __f() {{ {} }} var __r = __f()", src);
    sf.run_string(&wrapped).expect("eval failed");
    sf.get_global("__r").expect("__r not set")
}

/// run 执行代码并返回整体结果（未捕获的错误 → Err）。
fn run(src: &str) -> Result<Value, Value> {
    let mut sf = Sflang::new();
    sf.run_string(src)
}

/// make_failing_ciphertext 构造一段在该密钥下解密必然失败（PKCS7 校验不过）的数据。
///
/// 格式为 aesDecrypt 的输入格式 [16 字节 IV][密文]。
/// 构造方法：逐次篡改密文字节并用底层 sflang::aes::aes_cbc_decrypt 验证，
/// 直到解密失败为止（随机明文通过 PKCS7 校验的概率约 1/256，几次内即失败），
/// 因此测试是确定性的，不会因随机 IV/密钥而偶发通过。
fn make_failing_ciphertext() -> Vec<u8> {
    let key = b"0123456789abcdef";
    let iv = [0x07u8; 16];
    let mut ct = [0x11u8; 16];
    loop {
        if sflang::aes::aes_cbc_decrypt(&ct, key, &iv).is_err() {
            let mut data = Vec::with_capacity(32);
            data.extend_from_slice(&iv);
            data.extend_from_slice(&ct);
            return data;
        }
        ct[0] = ct[0].wrapping_add(1);
    }
}

// ---- aesDecrypt / aesDecryptStr 错误可捕获 ----

#[test]
fn test_aes_decrypt_too_short_is_catchable() {
    // 数据不足 16 字节 IV 段：应抛出可被 try-catch 捕获的错误
    let r = eval(r#"
        var caught = false
        try {
            aesDecrypt(bytesFromHex("00112233"), "0123456789abcdef")
            caught = "unexpected-success"
        } catch (e) {
            caught = true
        }
        return caught
    "#);
    assert_eq!(r, Value::Bool(true));
}

#[test]
fn test_aes_decrypt_corrupted_is_catchable() {
    // 损坏数据：16 字节 IV + 5 字节密文（长度非 16 的倍数，必然解密失败）
    let r = eval(r#"
        var caught = false
        try {
            aesDecrypt(bytesFromHex("00000000000000000000000000000000aabbccddee"), "0123456789abcdef")
            caught = "unexpected-success"
        } catch (e) {
            caught = true
        }
        return caught
    "#);
    assert_eq!(r, Value::Bool(true));
}

#[test]
fn test_aes_decrypt_wrong_key_is_catchable() {
    // 错误密钥：密文在 Rust 侧确定性构造为"该密钥下 PKCS7 必然校验失败"
    let mut sf = Sflang::new();
    sf.set_global("__data", Value::Bytes(Arc::new(make_failing_ciphertext())));
    let r = sf.run_string(r#"
        func __f() {
            var caught = false
            try {
                aesDecrypt(__data, "0123456789abcdef")
                caught = "unexpected-success"
            } catch (e) {
                caught = true
            }
            return caught
        }
        var __r = __f()
    "#).expect("run failed");
    assert_eq!(sf.get_global("__r").unwrap(), Value::Bool(true));
}

#[test]
fn test_aes_decrypt_uncaught_returns_err() {
    // 未捕获时：VM 整体返回 Err，且错误信息含函数名（AI 友好）
    let r = run(r#"var x = aesDecrypt(bytesFromHex("00112233"), "0123456789abcdef")"#);
    match r {
        Err(Value::Error(e)) => assert!(e.message.contains("aesDecrypt"), "msg: {}", e.message),
        other => panic!("expected Err(Error), got {:?}", other),
    }
}

#[test]
fn test_aes_decrypt_str_errors_are_catchable() {
    // 非法 base64 输入
    let r = eval(r#"
        var caught = false
        try {
            aesDecryptStr("!!!not-base64!!!", "0123456789abcdef")
            caught = "unexpected-success"
        } catch (e) {
            caught = true
        }
        return caught
    "#);
    assert_eq!(r, Value::Bool(true));
    // 合法 base64 但解码后不足 16 字节（"AAAA" → 3 字节）
    let r = eval(r#"
        var caught = false
        try {
            aesDecryptStr("AAAA", "0123456789abcdef")
            caught = "unexpected-success"
        } catch (e) {
            caught = true
        }
        return caught
    "#);
    assert_eq!(r, Value::Bool(true));
}

#[test]
fn test_aes_str_roundtrip_still_works() {
    // 正常路径回归：加密→解密往返（修复不应破坏原有功能）
    let r = eval(r#"
        var ct = aesEncryptStr("hello 中文 world", "0123456789abcdef")
        return aesDecryptStr(ct, "0123456789abcdef")
    "#);
    assert_eq!(r, Value::str("hello 中文 world"));
    // bytes 版本往返
    let r = eval(r#"
        var ct = aesEncrypt("payload-123", "0123456789abcdef0123456789abcdef")
        return strFromBytes(aesDecrypt(ct, "0123456789abcdef0123456789abcdef"))
    "#);
    assert_eq!(r, Value::str("payload-123"));
}

// ---- base64UrlDecode 严格化 ----

#[test]
fn test_base64url_decode_invalid_char_is_error() {
    // "AA!A"：'!' 为非法字符，应报错（而非静默按 0 处理）
    let r = run(r#"var y = base64UrlDecode("AA!A")"#);
    match r {
        Err(Value::Error(e)) => assert!(e.message.contains("base64UrlDecode"), "msg: {}", e.message),
        other => panic!("expected Err(Error), got {:?}", other),
    }
    // 脚本层 try-catch 同样可捕获
    let r = eval(r#"
        var caught = false
        try {
            base64UrlDecode("AA!A")
            caught = "unexpected-success"
        } catch (e) {
            caught = true
        }
        return caught
    "#);
    assert_eq!(r, Value::Bool(true));
    // 标准变体的 + / 亦视为非法（应使用 base64Decode）
    assert!(run(r#"var y = base64UrlDecode("a+b/")"#).is_err());
}

#[test]
fn test_base64url_decode_truncated_is_error() {
    // 去除空白/填充后长度余 1：无法解出整字节，应报错（而非静默丢字符）
    assert!(run(r#"var y = base64UrlDecode("AAAAB")"#).is_err());
}

#[test]
fn test_base64url_roundtrip() {
    // 覆盖长度 mod 3 的三种余数（0/1/2）与空串
    let r = eval(r#"
        var ok = true
        var cases = ["", "de", "dead", "deadbe", "deadbeef", "00ff10"]
        for c in cases {
            if bytesHex(base64UrlDecode(base64UrlEncode(bytesFromHex(c)))) != c { ok = false }
        }
        return ok
    "#);
    assert_eq!(r, Value::Bool(true));
    // 带填充的 URL-safe 输入（兼容有 = padding 的变体）
    let r = eval(r#"return bytesHex(base64UrlDecode("-_8"))"#);
    assert_eq!(r, Value::str("fbff"));
}

// ---- base64Decode 填充位置校验 ----

#[test]
fn test_base64_decode_padding_position_is_error() {
    // "AB==CDEF"：'=' 出现在中间位置，应报错（旧实现会静默解码成功）
    let r = run(r#"var y = base64Decode("AB==CDEF")"#);
    match r {
        Err(Value::Error(e)) => assert!(e.message.contains("base64Decode"), "msg: {}", e.message),
        other => panic!("expected Err(Error), got {:?}", other),
    }
    // 正常输入不受影响
    assert_eq!(eval(r#"return strFromBytes(base64Decode("YWJj"))"#), Value::str("abc"));
    assert_eq!(eval(r#"return strFromBytes(base64Decode("YQ=="))"#), Value::str("a"));
    assert_eq!(eval(r#"return strFromBytes(base64Decode("YWI="))"#), Value::str("ab"));
    // 往返一致
    let r = eval(r#"
        var ok = true
        var cases = ["", "a", "ab", "abc", "hello world 中文"]
        for c in cases {
            if strFromBytes(base64Decode(base64Encode(c))) != c { ok = false }
        }
        return ok
    "#);
    assert_eq!(r, Value::Bool(true));
}

// ---- HMAC 官方向量（RFC 4231）经脚本层断言 ----

#[test]
fn test_hmac_sha256_rfc4231_case1() {
    // RFC 4231 Test Case 1：Key = 0x0b x20，Data = "Hi There"
    let r = eval(r#"return bytesHex(hmacSha256(bytesFromHex("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b"), "Hi There"))"#);
    assert_eq!(r, Value::str("b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"));
}

#[test]
fn test_hmac_sha256_rfc4231_case2() {
    // RFC 4231 Test Case 2：Key = "Jefe"，Data = "what do ya want for nothing?"
    let r = eval(r#"return hmacSha256Hex("Jefe", "what do ya want for nothing?")"#);
    assert_eq!(r, Value::str("5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"));
}

#[test]
fn test_hmac_sha256_doc_example() {
    // 文档示例：完整句子的 Wikipedia HMAC 向量（消息须一字不差）
    let r = eval(r#"return hmacSha256Hex("key", "The quick brown fox jumps over the lazy dog")"#);
    assert_eq!(r, Value::str("f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"));
}

// ---- htmlDecode 不产生 NUL ----

#[test]
fn test_html_decode_zero_codepoint_replaced() {
    // &#0; 应替换为 U+FFFD，而不是 NUL
    let r = eval(r#"return htmlDecode("&#0;") == charFromCode(0xFFFD)"#);
    assert_eq!(r, Value::Bool(true));
    // 首字节为 U+FFFD 的 UTF-8 首字节 0xEF（二次确认不含 NUL 字节）
    let r = eval(r#"return bytesAt(htmlDecode("&#0;"), 0) == byte(0xEF)"#);
    assert_eq!(r, Value::Bool(true));
    // 长度为 1 个字符（NUL 未被保留）
    let r = eval(r#"return len(htmlDecode("&#0;"))"#);
    assert_eq!(r, Value::Int(1));
    // 非法代理区码点同样替换为 U+FFFD
    let r = eval(r#"return htmlDecode("&#xD800;") == charFromCode(0xFFFD)"#);
    assert_eq!(r, Value::Bool(true));
    // 正常数字实体不受影响
    let r = eval(r#"return htmlDecode("&#65;")"#);
    assert_eq!(r, Value::str("A"));
    // encode/decode 往返
    let r = eval(r#"return htmlDecode(htmlEncode("<a & b>")) == "<a & b>""#);
    assert_eq!(r, Value::Bool(true));
}
