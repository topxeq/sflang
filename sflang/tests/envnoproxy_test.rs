//! 环境代理行为测试（NO_PROXY 排除）。
//!
//! 独立进程：HTTP_PROXY 指向死端口 + NO_PROXY 排除本机 → 应绕过代理直连成功。

use sflang::api::Sflang;
use sflang::value::Value;

#[test]
fn test_http_no_proxy_bypass() {
    let port = 19023;
    let mut sf = Sflang::new();
    sf.set_output(std::io::sink());

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

    // 两个变量都在第一次 getWeb 之前设置好
    std::env::set_var("HTTP_PROXY", "http://127.0.0.1:1");
    std::env::set_var("NO_PROXY", "127.0.0.1,localhost");

    sf.run_string(&format!("var r1 = getWeb(\"http://127.0.0.1:{}/\")", port)).unwrap();
    match sf.get_global("r1").unwrap() {
        Value::Str(s) => assert_eq!(s.as_ref(), "direct-ok", "NO_PROXY 排除的主机应绕过代理直连"),
        other => panic!("NO_PROXY 排除后应直连成功，得到 {:?}", other.type_name()),
    }
    std::env::remove_var("HTTP_PROXY");
    std::env::remove_var("NO_PROXY");
}
