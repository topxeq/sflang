//! 环境代理行为测试（HTTP_PROXY 生效）。
//!
//! 单独成文件：cargo test 以文件为单位各起独立进程，set_var 的环境变量
//! 不会泄漏到其他测试文件的进程。环境代理设置进程级缓存（OnceLock），
//! 因此必须在第一次 getWeb 之前设置好环境变量。

use sflang::api::Sflang;
use sflang::value::Value;

#[test]
fn test_http_env_proxy_honored() {
    let port = 19022;
    let mut sf = Sflang::new();
    sf.set_output(std::io::sink());

    // 先起服务器，但绝不先发任何 getWeb（避免 ENV_PROXY 缓存为无代理状态）
    std::thread::spawn(move || {
        let mut srv = Sflang::new();
        srv.set_output(std::io::sink());
        let _ = srv.run_string(&format!(
            "var server = httpServer(\"--port={}\", \"--host=127.0.0.1\")\n\
             serverSetHandler(server, \"/\", func(req, resp) {{ return \"direct-ok\" }})\n\
             serverStart(server)",
            port
        ));
    });
    std::thread::sleep(std::time::Duration::from_millis(300));

    // 环境代理指向必然拒绝连接的端口
    std::env::set_var("HTTP_PROXY", "http://127.0.0.1:1");

    sf.run_string(&format!("var r1 = getWeb(\"http://127.0.0.1:{}/\")", port)).unwrap();
    match sf.get_global("r1").unwrap() {
        Value::Error(_) => {} // 走死端口代理连接被拒，证明环境代理被采纳
        other => panic!("HTTP_PROXY 指向死端口时请求应失败，得到 {:?}", other.type_name()),
    }
    std::env::remove_var("HTTP_PROXY");
}
