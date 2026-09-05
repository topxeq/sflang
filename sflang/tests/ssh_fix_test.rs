//! builtins_ssh.rs `--append` 追加上传的回归测试。
//!
//! 覆盖：
//!   - sshUploadBytes 携带 --append / -append 标志时参数管道完整
//!     （bytes 参数仍可被识别，缺失 bytes 的错误路径不受影响）
//!   - sshUpload（文件上传）同样接受 --append 标志
//!   - 连接不可达主机时返回 error 对象（而非解释器级错误）
//!
//! 注：SFTP 追加写入的端到端行为依赖真实 SSH 服务器，测试环境不可用，
//! 此处以「参数解析 + 连接错误路径」做回归保护；追加写入逻辑
//! （查远程文件大小 + CREATE|WRITE 打开 + seek 到末尾）由代码审查与
//! 手工连接测试保障。

use sflang::Sflang;
use sflang::value::Value;

/// eval 求值代码并返回结果（用 IIFE 包裹，src 内需显式 return）。
fn eval(src: &str) -> Value {
    let mut sf = Sflang::new();
    let wrapped = format!("func __f() {{ {} }} var __r = __f()", src);
    sf.run_string(&wrapped).expect("eval failed");
    sf.get_global("__r").expect("__r not set")
}

/// assert_err_contains 断言求值结果为 error 对象且信息包含 expect 片段。
fn assert_err_contains(src: &str, expect: &str) {
    match eval(src) {
        Value::Error(e) => assert!(
            e.message.contains(expect),
            "错误信息应包含 '{}'，实际: {}",
            expect,
            e.message
        ),
        other => panic!("应返回 error 对象，得到 {}", other.type_name()),
    }
}

// ---- --append 标志的参数管道 ----

#[test]
fn test_ssh_upload_bytes_append_flag_reaches_connection() {
    // 127.0.0.1:1 不可达，预期在「SSH 连接失败」处返回 error 对象。
    // 若 --append 标志破坏了参数管道（如把 bytes 参数误判为开关），
    // 会先报 "需要 bytes/byteArray 参数"，本测试即失败。
    assert_err_contains(
        r#"return sshUploadBytes(strToUtf8("line\n"), "-host=127.0.0.1", "-port=1", "-user=u", "-password=p", "-remotePath=/tmp/sflang_append_test.txt", "--append")"#,
        "SSH 连接失败",
    );
}

#[test]
fn test_ssh_upload_bytes_append_short_flag() {
    // -append 单横杠写法（与 charlang 迁移脚本的参数习惯一致）
    assert_err_contains(
        r#"return sshUploadBytes(strToUtf8("line\n"), "-host=127.0.0.1", "-port=1", "-user=u", "-password=p", "-remotePath=/tmp/sflang_append_test.txt", "-append")"#,
        "SSH 连接失败",
    );
}

#[test]
fn test_ssh_upload_bytes_accepts_string_data() {
    // string 数据直接按 UTF-8 编码上传，无须 strToUtf8 显式转换；
    // 若 string 未被识别为数据，会报 "需要 bytes/byteArray/string 数据参数"
    assert_err_contains(
        r#"return sshUploadBytes(getNowStr() + "\n", "-host=127.0.0.1", "-port=1", "-user=u", "-password=p", "-remotePath=/tmp/sflang_append_test.txt", "--append")"#,
        "SSH 连接失败",
    );
}

#[test]
fn test_ssh_upload_bytes_missing_bytes_error_unaffected() {
    // 缺 bytes 参数的错误路径不受 --append 标志影响
    assert_err_contains(
        r#"return sshUploadBytes("-host=127.0.0.1", "-user=u", "-password=p", "-remotePath=/tmp/x", "--append")"#,
        "bytes/byteArray",
    );
}

#[test]
fn test_ssh_upload_append_flag_reaches_local_read() {
    // sshUpload（文件上传）同样接受 --append；
    // 本地文件不存在时在读取阶段返回 error（先于 SSH 连接）
    assert_err_contains(
        r#"return sshUpload("-host=127.0.0.1", "-user=u", "-password=p", "-localPath=./__no_such_file__.bin", "-remotePath=/tmp/x", "--append")"#,
        "读取本地文件",
    );
}
