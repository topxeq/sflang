//! builtins_tls.rs — TLS 证书检查内置函数
//!
//! 函数列表：
//!   tlsGetCertInfo(host [, port [, timeoutMs]]) -> object
//!     连接 HTTPS 服务完成 TLS 握手，返回叶证书（站点证书）的详细信息，
//!     包括有效期（notBefore/notAfter）、主题/颁发者、序列号、剩余天数、
//!     是否已过期等。已过期证书同样可以获取（本函数不对证书做信任校验）。
//!
//! 实现要点：
//!   - TLS 客户端基于 rustls（项目已有依赖，与 HTTPS 服务器共用 ring provider）
//!   - 证书校验采用"全部接受"模式：本函数的目的是读取并检查证书本身，
//!     不能让校验失败（例如证书已过期）阻止获取证书信息；
//!     是否过期由返回值 expired 字段（或脚本自行比较 notAfter）判断
//!   - X.509 字段解析（有效期/主题/颁发者/序列号/签名算法）用本文件内
//!     纯标准库实现的轻量 DER 解析完成，不引入新的第三方依赖
//!     （遵循 AGENTS.md"尽量只用 std"的原则）
//!   - 阻塞式网络 IO，用 register_builtin_doc_blocking 注册，
//!     在 run 并发任务中会正确让出调度器线程
//!
//! 典型用法：
//!   info := tlsGetCertInfo("example.com")
//!   pln(info.notAfter, "剩余", info.daysRemaining, "天")
//!   if info.expired { pln("证书已过期") }

use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};

use crate::builtins_helpers as bh;
use crate::datetime::DateTime;
use crate::function::BuiltinDoc;
use crate::object_map;
use crate::value::{error_value, Value};
use crate::vm::VM;

// ===========================================================================
// 文档
// ===========================================================================

static DOC_TLS_GET_CERT_INFO: BuiltinDoc = BuiltinDoc {
    category: "tls",
    signature: "tlsGetCertInfo(host [, port [, timeoutMs]]) -> object",
    summary: "连接 HTTPS 服务完成 TLS 握手，返回站点叶证书信息（含到期时间、是否已过期）。不做证书信任校验，因此已过期/自签名证书同样能取到信息。",
    params: &[
        ("host", "主机名，如 \"example.com\"；也接受 \"example.com:8443\" 或完整 https:// URL（自动提取主机与端口）"),
        ("port", "可选端口号，默认 443"),
        ("timeoutMs", "可选超时毫秒（DNS/连接/握手每次 IO 分别限时），默认 10000"),
    ],
    returns: "object：host, port, subject, issuer, serial, notBefore, notAfter（UTC 时间字符串）, notBeforeTs, notAfterTs（Unix 秒）, daysRemaining（int，负数表示已过期天数）, expired（bool）, selfSigned（bool，主题与颁发者相同）, tlsVersion, chainCount",
    examples: &[
        "info := tlsGetCertInfo(\"example.com\")",
        "pln(info.notAfter, \" 剩余\", info.daysRemaining, \"天\")",
        "info := tlsGetCertInfo(\"https://example.com:8443\", 8443, 5000)",
    ],
    errors: &[
        "tlsGetCertInfo() 连接 'xxx:443' 失败（可能原因：域名无法解析、网络不通、服务器 443 端口未开放 TLS、防火墙拦截、公司内网代理限制、超时过短）",
        "tlsGetCertInfo() TLS 握手失败（可能原因：目标端口不是 TLS 服务（误用了普通 HTTP 端口）、服务器不支持任何现代 TLS 版本、中间设备拦截 TLS、SNI 不匹配导致服务器拒绝）",
        "tlsGetCertInfo() 服务器未提供证书（可能原因：端口上是普通 TCP/HTTP 服务而非 HTTPS）",
        "tlsGetCertInfo() 解析证书失败（可能原因：服务器返回了非 X.509 数据、DER 编码异常）",
        "tlsGetCertInfo() 参数 host 含非法字符（可能原因：传入了带路径的 URL 且格式异常、包含空格或全角字符）",
    ],
};

// ===========================================================================
// 内置函数入口
// ===========================================================================

/// bi_tls_get_cert_info 实现 tlsGetCertInfo(host [, port [, timeoutMs]])。
///
/// 错误风格与 getWeb 等网络类内置函数一致：
///   - 参数个数/类型错误（编程错误）→ Err(error_value)（自动抛出，需 try-catch）
///   - 运行时失败（DNS/连接/握手/解析失败等）→ Ok(错误对象)（惰性，用 isError 检查）
///
/// 流程：解析主机名 → TCP 连接 → rustls 握手（接受任意证书）→
/// 取对端证书链叶证书 → DER 解析关键字段 → 组装返回对象。
fn bi_tls_get_cert_info(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "tlsGetCertInfo")?;
    let host_raw = bh::as_str(args, 0, "tlsGetCertInfo")?;

    // 可选端口参数（默认 443）
    let port_arg = bh::opt_arg(args, 1).map(|_| bh::as_int(args, 1, "tlsGetCertInfo")).transpose()?;
    // 可选超时参数（默认 10 秒）
    let timeout_arg = bh::opt_arg(args, 2).map(|_| bh::as_int(args, 2, "tlsGetCertInfo")).transpose()?;

    // 可选参数取值范围检查（编程错误 → 抛出）
    let port = match port_arg {
        Some(p) => {
            if !(1..=65535).contains(&p) {
                return Err(error_value(format!(
                    "tlsGetCertInfo() 端口 {} 非法（可能原因：端口必须在 1-65535 之间）", p
                )));
            }
            p as u16
        }
        None => 443,
    };
    let timeout_ms = match timeout_arg {
        Some(t) => {
            if !(1..=300_000).contains(&t) {
                return Err(error_value(format!(
                    "tlsGetCertInfo() 超时 {}ms 非法（可能原因：超时必须在 1-300000 毫秒之间）", t
                )));
            }
            t as u64
        }
        None => 10_000,
    };

    // 解析 host 参数：容忍完整 URL / host:port 两种写法（数据性失败 → 错误对象）
    let (host, host_port) = match split_host_port(host_raw) {
        Ok(r) => r,
        Err(msg) => return Ok(error_value(msg)),
    };
    let port = match host_port {
        Some(p) => p,
        None => port,
    };

    // 完成 TLS 握手，取回证书链（运行时失败 → 错误对象）
    let (chain, tls_version) = match tls_fetch_chain(&host, port, timeout_ms) {
        Ok(r) => r,
        Err(e) => return Ok(e),
    };
    let leaf = match chain.first() {
        Some(c) => c,
        None => {
            return Ok(error_value(format!(
                "tlsGetCertInfo(\"{}\", {}) 服务器未提供证书（可能原因：端口上是普通 TCP/HTTP 服务而非 HTTPS）",
                host, port
            )));
        }
    };

    // 解析叶证书关键字段（DER 解析失败 → 错误对象，附可能原因）
    let info = match parse_leaf_cert(leaf.as_ref()) {
        Ok(r) => r,
        Err(e) => {
            return Ok(error_value(format!(
                "tlsGetCertInfo(\"{}\", {}) 解析证书失败: {}（可能原因：服务器返回了非 X.509 数据、DER 编码异常）",
                host, port, e
            )));
        }
    };

    // 计算剩余天数（负数表示已过期多少天）
    let now_sec = DateTime::now().to_millis() / 1000;
    let days_remaining = (info.not_after_ts - now_sec).div_euclid(86400);

    // 组装返回对象
    let obj = object_map::new_map();
    {
        let mut m = obj.lock().unwrap();
        m.set("host".to_string(), Value::str_from(host.clone()));
        m.set("port".to_string(), Value::Int(port as i64));
        m.set("subject".to_string(), Value::str_from(info.subject.clone()));
        m.set("issuer".to_string(), Value::str_from(info.issuer.clone()));
        m.set("serial".to_string(), Value::str_from(info.serial.clone()));
        m.set("sigAlg".to_string(), Value::str_from(info.sig_alg.clone()));
        m.set("notBefore".to_string(), Value::str_from(fmt_utc(info.not_before_ts)));
        m.set("notAfter".to_string(), Value::str_from(fmt_utc(info.not_after_ts)));
        m.set("notBeforeTs".to_string(), Value::Int(info.not_before_ts));
        m.set("notAfterTs".to_string(), Value::Int(info.not_after_ts));
        m.set("daysRemaining".to_string(), Value::Int(days_remaining));
        m.set("expired".to_string(), Value::Bool(days_remaining < 0));
        m.set("selfSigned".to_string(), Value::Bool(info.subject == info.issuer));
        m.set("tlsVersion".to_string(), Value::str_from(tls_version));
        m.set("chainCount".to_string(), Value::Int(chain.len() as i64));
    }
    Ok(Value::Object(obj))
}

// ===========================================================================
// TLS 连接与握手
// ===========================================================================

/// AcceptAllVerifier 接受任意服务端证书的校验器。
///
/// 用途说明：tlsGetCertInfo 的目标是"读取证书信息"而非"建立可信连接"，
/// 因此证书是否可信、是否过期都不应阻断信息获取。
#[derive(Debug)]
struct AcceptAllVerifier;

impl ServerCertVerifier for AcceptAllVerifier {
    /// verify_server_cert 接受任意服务端证书链。
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    /// verify_tls12_signature 接受任意 TLS 1.2 签名。
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    /// verify_tls13_signature 接受任意 TLS 1.3 签名。
    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    /// supported_verify_schemes 声明支持全部常见签名方案（避免握手协商失败）。
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::ECDSA_NISTP521_SHA512,
            SignatureScheme::ED25519,
        ]
    }
}

/// tls_fetch_chain 连接 host:port 完成 TLS 握手，返回（证书链, 协商的 TLS 版本名）。
///
/// 连接与握手每次 IO 操作限时 timeout_ms；DNS 解析依次尝试所有地址（最多前 4 个）。
fn tls_fetch_chain(host: &str, port: u16, timeout_ms: u64) -> Result<(Vec<CertificateDer<'static>>, String), Value> {
    // 安装 ring crypto provider（幂等；与 HTTPS 服务器/ureq 客户端共用同一 provider）
    {
        use std::sync::OnceLock;
        static PROVIDER_INSTALLED: OnceLock<()> = OnceLock::new();
        PROVIDER_INSTALLED.get_or_init(|| {
            let _ = rustls::crypto::ring::default_provider().install_default();
        });
    }

    // 构造客户端配置：dangerous 校验器接受任意证书（见 AcceptAllVerifier 说明）
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| error_value(format!("tlsGetCertInfo() 构建 TLS 配置失败: {}（可能原因：TLS 协议版本特性未启用）", e)))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAllVerifier))
        .with_no_client_auth();

    // SNI 名称（同时兼容域名与 IP 地址写法）
    let server_name = ServerName::try_from(host.to_string()).map_err(|_| {
        error_value(format!(
            "tlsGetCertInfo(\"{}\") 主机名非法（可能原因：包含空格/全角字符等非法字符；IP 地址需为合法的 IPv4/IPv6 形式）",
            host
        ))
    })?;

    // DNS 解析（域名 → socket 地址列表）
    let addrs: Vec<_> = (host, port)
        .to_socket_addrs()
        .map_err(|e| {
            error_value(format!(
                "tlsGetCertInfo(\"{}\", {}) DNS 解析失败: {}（可能原因：域名不存在/已失效、拼写错误、DNS 服务器不可用、公司内网无法解析外部域名）",
                host, port, e
            ))
        })?
        .take(4)
        .collect();
    if addrs.is_empty() {
        return Err(error_value(format!(
            "tlsGetCertInfo(\"{}\", {}) DNS 解析结果为空（可能原因：域名不存在、拼写错误）",
            host, port
        )));
    }

    // 依次尝试连接（记录最后一个错误用于报错信息）
    let mut last_err = String::new();
    let mut tcp = None;
    for addr in &addrs {
        match TcpStream::connect_timeout(addr, Duration::from_millis(timeout_ms)) {
            Ok(s) => {
                tcp = Some(s);
                break;
            }
            Err(e) => last_err = format!("{}: {}", addr, e),
        }
    }
    let mut tcp = tcp.ok_or_else(|| {
        error_value(format!(
            "tlsGetCertInfo(\"{}\", {}) 连接失败（{}）（可能原因：网络不通、服务器 {} 端口未开放、防火墙拦截、超时 {}ms 过短）",
            host, port, last_err, port, timeout_ms
        ))
    })?;

    // 读写超时（每次 IO 分别限时；握手一般 1-2 个来回）
    let io_timeout = Duration::from_millis(timeout_ms);
    let _ = tcp.set_read_timeout(Some(io_timeout));
    let _ = tcp.set_write_timeout(Some(io_timeout));

    // 建立 TLS 连接并完成握手（握手在 complete_io 循环中推进）
    let mut conn = rustls::ClientConnection::new(Arc::new(config), server_name).map_err(|e| {
        error_value(format!("tlsGetCertInfo() 创建 TLS 连接失败: {}（可能原因：主机名/SNI 非法）", e))
    })?;

    let mut rounds = 0;
    while conn.is_handshaking() {
        rounds += 1;
        // 防御性上限：正常握手 1-3 轮完成，超过视为握手停滞
        if rounds > 20 {
            return Err(error_value(format!(
                "tlsGetCertInfo(\"{}\", {}) TLS 握手停滞（可能原因：中间设备拦截 TLS 流量、服务器行为异常）",
                host, port
            )));
        }
        match conn.complete_io(&mut tcp) {
            Ok((0, 0)) => {
                return Err(error_value(format!(
                    "tlsGetCertInfo(\"{}\", {}) TLS 握手无进展（可能原因：中间设备拦截 TLS 流量、服务器提前断开）",
                    host, port
                )));
            }
            Ok(_) => {}
            Err(e) => {
                let hint = if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut {
                    "，连接/握手超时"
                } else {
                    ""
                };
                return Err(error_value(format!(
                    "tlsGetCertInfo(\"{}\", {}) TLS 握手失败: {}{}（可能原因：目标端口不是 TLS 服务（误用了普通 HTTP 端口）、服务器不支持现代 TLS 版本、中间设备拦截 TLS、SNI 不匹配导致服务器拒绝、超时 {}ms 过短）",
                    host, port, e, hint, timeout_ms
                )));
            }
        }
    }

    // 读取协商的 TLS 版本名（便于排查老旧服务器）
    let tls_version = match conn.protocol_version() {
        Some(rustls::ProtocolVersion::TLSv1_3) => "TLS1.3".to_string(),
        Some(rustls::ProtocolVersion::TLSv1_2) => "TLS1.2".to_string(),
        Some(v) => format!("{:?}", v),
        None => "unknown".to_string(),
    };

    // 取出对端证书链（叶证书在首位）
    let chain = match conn.peer_certificates() {
        Some(certs) if !certs.is_empty() => certs.iter().map(|c| c.clone().into_owned()).collect(),
        _ => {
            return Err(error_value(format!(
                "tlsGetCertInfo(\"{}\", {}) 服务器未提供证书（可能原因：端口上是普通 TCP/HTTP 服务而非 HTTPS）",
                host, port
            )));
        }
    };

    Ok((chain, tls_version))
}

/// split_host_port 解析 host 参数，返回（主机名, 可选端口）。
///
/// 容忍三种写法：
///   - "example.com"
///   - "example.com:8443"
///   - "https://example.com[:8443][/path]"（scheme 非法时报错提示）
fn split_host_port(s: &str) -> Result<(String, Option<u16>), String> {
    let mut rest = s.trim();

    // 处理 URL scheme：只接受 https://（或无 scheme）；http:// 明确报错提示
    if let Some(pos) = rest.find("://") {
        let scheme = rest[..pos].to_lowercase();
        if scheme != "https" {
            return Err(format!(
                "tlsGetCertInfo(\"{}\") 参数 host 的协议为 \"{}\"（可能原因：本函数检查 HTTPS 证书，请使用 https:// 开头或直接写主机名，如 \"example.com\"）",
                s, scheme
            ));
        }
        rest = &rest[pos + 3..];
    }

    // 去掉路径部分
    if let Some(pos) = rest.find('/') {
        rest = &rest[..pos];
    }

    if rest.is_empty() {
        return Err(format!(
            "tlsGetCertInfo(\"{}\") 参数 host 为空（可能原因：URL 中缺少主机名部分）",
            s
        ));
    }

    // IPv6 写法：[::1]:8443
    if rest.starts_with('[') {
        let end = rest.find(']').ok_or_else(|| {
            format!("tlsGetCertInfo(\"{}\") IPv6 地址缺少右括号（可能原因：应为 [::1]:8443 形式）", s)
        })?;
        let host = rest[1..end].to_string();
        let port_part = rest.get(end + 1..).unwrap_or("");
        let port = parse_port_str(port_part).ok_or_else(|| {
            format!("tlsGetCertInfo(\"{}\") 端口 \"{}\" 非法（可能原因：端口应为 1-65535 的数字）", s, port_part)
        })?;
        return Ok((host, Some(port)));
    }

    // 普通域名/IP:port 写法（只按最后一个 ':' 拆分）
    if let Some(pos) = rest.rfind(':') {
        let (h, p) = rest.split_at(pos);
        let port = parse_port_str(&p[1..]).ok_or_else(|| {
            format!("tlsGetCertInfo(\"{}\") 端口 \"{}\" 非法（可能原因：端口应为 1-65535 的数字；IPv6 地址需用方括号括起）", s, &p[1..])
        })?;
        return Ok((h.to_string(), Some(port)));
    }

    Ok((rest.to_string(), None))
}

/// parse_port_str 解析端口字符串（要求非空且为 1-65535 的数字）。
fn parse_port_str(s: &str) -> Option<u16> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<u16>().ok().filter(|p| *p >= 1)
}

// ===========================================================================
// 轻量 X.509/DER 解析（纯标准库，不引入第三方依赖）
// ===========================================================================

/// LeafCertInfo 叶证书解析结果。
struct LeafCertInfo {
    /// subject 主题（DN 字符串，如 "CN=example.com, C=CN"）。
    subject: String,
    /// issuer 颁发者（DN 字符串）。
    issuer: String,
    /// serial 序列号（大写十六进制）。
    serial: String,
    /// sig_alg 签名算法名（如 "sha256WithRSAEncryption"，未知 OID 时显示 OID）。
    sig_alg: String,
    /// not_before_ts 有效期起点（Unix 秒，UTC）。
    not_before_ts: i64,
    /// not_after_ts 有效期终点（Unix 秒，UTC）。
    not_after_ts: i64,
}

/// DerTlv 单个 DER TLV（tag-length-value）条目。
///
/// content 指向原始数据切片中的值部分（不含 tag 与 length 字节）。
struct DerTlv<'a> {
    /// tag DER 标签字节（如 0x30 SEQUENCE、0x02 INTEGER、0x17 UTCTime）。
    tag: u8,
    /// content 值部分字节。
    content: &'a [u8],
}

/// der_read 从 data 开头读取一个 TLV，返回（条目, 剩余数据）。
///
/// 只支持 DER 定长编码（不允许 indefinite length）；数据不完整或长度越界返回 None。
fn der_read(data: &[u8]) -> Option<(DerTlv<'_>, &[u8])> {
    if data.len() < 2 {
        return None;
    }
    let tag = data[0];
    let len_byte = data[1];
    let (content_len, header_len) = if len_byte < 0x80 {
        // 短形式：长度直接是 len_byte
        (len_byte as usize, 2)
    } else {
        // 长形式：低 7 位是长度字节数（DER 不允许 0x80 即 indefinite length）
        let n = (len_byte & 0x7F) as usize;
        if n == 0 || n > 4 || data.len() < 2 + n {
            return None;
        }
        let mut len = 0usize;
        for &b in &data[2..2 + n] {
            len = (len << 8) | b as usize;
        }
        (len, 2 + n)
    };
    if data.len() < header_len + content_len {
        return None;
    }
    let content = &data[header_len..header_len + content_len];
    Some((DerTlv { tag, content }, &data[header_len + content_len..]))
}

/// der_children 把 SEQUENCE/SET 的内容解析为子 TLV 列表（严格顺序解析）。
fn der_children(content: &[u8]) -> Option<Vec<DerTlv<'_>>> {
    let mut rest = content;
    let mut out = Vec::new();
    // 防御性上限：单个证书的顶层子项不会超过 64 个
    while !rest.is_empty() {
        if out.len() >= 64 {
            return None;
        }
        let (tlv, next) = der_read(rest)?;
        out.push(tlv);
        rest = next;
    }
    Some(out)
}

/// parse_leaf_cert 解析叶证书（DER 编码）中的关键字段。
fn parse_leaf_cert(der: &[u8]) -> Result<LeafCertInfo, String> {
    // Certificate ::= SEQUENCE { tbsCertificate, signatureAlgorithm, signatureValue }
    let (cert_seq, _) = der_read(der).ok_or("数据长度不足或 DER 编码不完整")?;
    if cert_seq.tag != 0x30 {
        return Err(format!("顶层不是 SEQUENCE（tag=0x{:02X}）", cert_seq.tag));
    }
    // tbsCertificate 为第一个子项
    let tbs = der_children(cert_seq.content)
        .and_then(|mut ch| if ch.is_empty() { None } else { Some(ch.remove(0)) })
        .ok_or("证书缺少 tbsCertificate")?;
    if tbs.tag != 0x30 {
        return Err("tbsCertificate 不是 SEQUENCE".to_string());
    }

    // TBSCertificate ::= [0] version?, serialNumber, signature, issuer, validity, subject, ...
    // 按位置依次识别各字段（[0] version 为可选的上下文标签，跳过）
    let tbs_items = der_children(tbs.content).ok_or("tbsCertificate 编码非法")?;
    let mut serial = String::new();
    let mut sig_alg = String::new();
    let mut issuer = String::new();
    let mut subject = String::new();
    let mut not_before = None;
    let mut not_after = None;
    let mut seq_index = 0; // 已识别的通用 SEQUENCE 计数（signature/issuer/validity/subject）

    for item in tbs_items.iter() {
        match item.tag {
            0xA0 => {} // [0] EXPLICIT version，跳过
            0x02 => {
                // serialNumber INTEGER → 大写十六进制（去掉符号补位的前导 0x00）
                let bytes = item.content;
                let trimmed = if bytes.len() > 1 && bytes[0] == 0 { &bytes[1..] } else { bytes };
                // 序列号最长 20 字节（RFC 5280），这里放宽显示前 32 字节
                let shown = &trimmed[..trimmed.len().min(32)];
                serial = shown.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join("");
                if trimmed.len() > 32 {
                    serial.push('…');
                }
            }
            0x30 => {
                seq_index += 1;
                match seq_index {
                    1 => sig_alg = oid_seq_to_name(item.content),
                    2 => issuer = parse_dn(item.content),
                    3 => {
                        // validity ::= SEQUENCE { notBefore, notAfter }
                        let times = der_children(item.content).ok_or("validity 编码非法")?;
                        if times.len() != 2 {
                            return Err(format!("validity 应包含 2 个时间，实际 {}", times.len()));
                        }
                        not_before = Some(parse_x509_time(times[0].tag, times[0].content)?);
                        not_after = Some(parse_x509_time(times[1].tag, times[1].content)?);
                    }
                    4 => subject = parse_dn(item.content),
                    _ => break, // 后面的 subjectPublicKeyInfo 等不再需要
                }
            }
            _ => {}
        }
    }

    let not_before_ts = not_before.ok_or("未找到 notBefore 字段")?;
    let not_after_ts = not_after.ok_or("未找到 notAfter 字段")?;

    Ok(LeafCertInfo {
        subject,
        issuer,
        serial,
        sig_alg,
        not_before_ts,
        not_after_ts,
    })
}

/// parse_x509_time 解析 X.509 时间值，返回 Unix 秒（UTC）。
///
/// UTCTime（tag 0x17）："YYMMDDHHMMSSZ"，YY >= 50 视为 19xx，否则 20xx；
/// GeneralizedTime（tag 0x18）："YYYYMMDDHHMMSSZ"（容忍小数秒 ".fff"）。
fn parse_x509_time(tag: u8, content: &[u8]) -> Result<i64, String> {
    let s = std::str::from_utf8(content).map_err(|_| "时间字段不是 ASCII")?;
    // 去掉结尾的 'Z'（X.509 要求 UTC 时间恒为 Z 结尾）与小数秒部分
    let digits = s.trim_end_matches('Z');
    let digits = digits.split('.').next().unwrap_or(digits);

    let (year, rest) = match tag {
        0x17 => {
            // UTCTime：2 位年份
            if digits.len() < 10 {
                return Err(format!("UTCTime \"{}\" 长度不足", s));
            }
            let yy: i32 = digits[..2].parse().map_err(|_| format!("UTCTime \"{}\" 年份非法", s))?;
            let year = if yy >= 50 { 1900 + yy } else { 2000 + yy };
            (year, &digits[2..])
        }
        0x18 => {
            // GeneralizedTime：4 位年份
            if digits.len() < 12 {
                return Err(format!("GeneralizedTime \"{}\" 长度不足", s));
            }
            let year: i32 = digits[..4].parse().map_err(|_| format!("GeneralizedTime \"{}\" 年份非法", s))?;
            (year, &digits[4..])
        }
        other => return Err(format!("时间字段 tag=0x{:02X} 不是 UTCTime/GeneralizedTime", other)),
    };

    // 剩余部分固定为 MMDDHHMMSS 共 10 位
    if rest.len() < 10 || !rest[..10].bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("时间 \"{}\" 格式非法", s));
    }
    let month: i32 = rest[0..2].parse().map_err(|_| "月份非法".to_string())?;
    let day: i32 = rest[2..4].parse().map_err(|_| "日期非法".to_string())?;
    let hour: i32 = rest[4..6].parse().map_err(|_| "小时非法".to_string())?;
    let min: i32 = rest[6..8].parse().map_err(|_| "分钟非法".to_string())?;
    let sec: i32 = rest[8..10].parse().map_err(|_| "秒非法".to_string())?;

    DateTime::from_components(year, month, day, hour, min, sec, 0, 0)
        .map(|dt| dt.to_millis() / 1000)
        .ok_or_else(|| format!("时间 {}-{:02}-{:02} {:02}:{:02}:{:02} 超出合法范围", year, month, day, hour, min, sec))
}

/// parse_dn 解析 X.509 Name（DN，Distinguished Name）为可读字符串。
///
/// Name ::= SEQUENCE OF SET OF SEQUENCE { oid, value }
/// 输出形如 "CN=example.com, O=Example Ltd, C=US"；解析失败时退化为十六进制串，
/// 保证本函数永不报错、永不 panic。
fn parse_dn(content: &[u8]) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(rdns) = der_children(content) {
        for rdn in rdns.iter() {
            // 每个 RDN 是 SET，取第一个属性
            if let Some(attrs) = der_children(rdn.content) {
                if let Some(attr) = attrs.into_iter().next() {
                    if let Some(kv) = der_children(attr.content) {
                        if kv.len() >= 2 {
                            let oid = oid_to_name(kv[0].content);
                            let value = der_string_value(kv[1].tag, kv[1].content);
                            parts.push(format!("{}={}", oid, value));
                        }
                    }
                }
            }
        }
    }
    if parts.is_empty() {
        // 退化路径：原始十六进制（截断显示），帮助 AI/用户至少看到原始数据
        let shown = &content[..content.len().min(24)];
        let hex = shown.iter().map(|b| format!("{:02X}", b)).collect::<String>();
        return format!("<未解析 DN: {}{}>", hex, if content.len() > 24 { "…" } else { "" });
    }
    parts.join(", ")
}

/// oid_to_name 把 DER 编码的 OID 解析为常见名称（如 "CN"），未收录时显示点分数字形式。
fn oid_to_name(der_oid: &[u8]) -> String {
    // 解码点分弧值：首字节编码前两弧，后续为 base-128
    if der_oid.is_empty() {
        return "<空OID>".to_string();
    }
    let mut arcs: Vec<u64> = Vec::new();
    let first = der_oid[0] as u64;
    if first < 40 {
        arcs.push(0);
        arcs.push(first);
    } else if first < 80 {
        arcs.push(1);
        arcs.push(first - 40);
    } else {
        arcs.push(2);
        arcs.push(first - 80);
    }
    let mut val: u64 = 0;
    let mut in_progress = false;
    for &b in &der_oid[1..] {
        val = (val << 7) | (b & 0x7F) as u64;
        if b & 0x80 == 0 {
            arcs.push(val);
            val = 0;
            in_progress = false;
        } else {
            in_progress = true;
        }
    }
    let _ = in_progress; // 截断的多字节弧按已有值处理，不再报错

    let dotted = arcs.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(".");

    // 常见 DN 属性 OID → 缩写
    match dotted.as_str() {
        "2.5.4.3" => "CN".to_string(),
        "2.5.4.6" => "C".to_string(),
        "2.5.4.7" => "L".to_string(),
        "2.5.4.8" => "ST".to_string(),
        "2.5.4.10" => "O".to_string(),
        "2.5.4.11" => "OU".to_string(),
        "2.5.4.4" => "SN".to_string(),
        "2.5.4.42" => "GN".to_string(),
        "2.5.4.5" => "serialNumber".to_string(),
        "2.5.4.9" => "street".to_string(),
        "2.5.4.17" => "postalCode".to_string(),
        "2.5.4.15" => "businessCategory".to_string(),
        "1.2.840.113549.1.9.1" => "emailAddress".to_string(),
        "0.9.2342.19200300.100.1.25" => "DC".to_string(),
        _ => dotted,
    }
}

/// oid_seq_to_name 解析 AlgorithmIdentifier（SEQUENCE { OID, params? }）为算法名。
fn oid_seq_to_name(content: &[u8]) -> String {
    if let Some(children) = der_children(content) {
        if let Some(first) = children.into_iter().next() {
            if first.tag == 0x06 {
                let dotted = oid_dotted(first.content);
                return match dotted.as_str() {
                    "1.2.840.113549.1.1.1" => "rsaEncryption".to_string(),
                    "1.2.840.113549.1.1.5" => "sha1WithRSAEncryption".to_string(),
                    "1.2.840.113549.1.1.10" => "rsassaPss".to_string(),
                    "1.2.840.113549.1.1.11" => "sha256WithRSAEncryption".to_string(),
                    "1.2.840.113549.1.1.12" => "sha384WithRSAEncryption".to_string(),
                    "1.2.840.113549.1.1.13" => "sha512WithRSAEncryption".to_string(),
                    "1.2.840.10045.2.1" => "ecPublicKey".to_string(),
                    "1.2.840.10045.4.1" => "ecdsa-with-SHA1".to_string(),
                    "1.2.840.10045.4.3.2" => "ecdsa-with-SHA256".to_string(),
                    "1.2.840.10045.4.3.3" => "ecdsa-with-SHA384".to_string(),
                    "1.2.840.10045.4.3.4" => "ecdsa-with-SHA512".to_string(),
                    "1.3.101.112" => "ed25519".to_string(),
                    other => other.to_string(),
                };
            }
        }
    }
    "<未知算法>".to_string()
}

/// oid_dotted 把 DER 编码 OID 解析为点分数字字符串。
fn oid_dotted(der_oid: &[u8]) -> String {
    if der_oid.is_empty() {
        return "<空OID>".to_string();
    }
    let mut arcs: Vec<u64> = Vec::new();
    let first = der_oid[0] as u64;
    if first < 40 {
        arcs.push(0);
        arcs.push(first);
    } else if first < 80 {
        arcs.push(1);
        arcs.push(first - 40);
    } else {
        arcs.push(2);
        arcs.push(first - 80);
    }
    let mut val: u64 = 0;
    for &b in &der_oid[1..] {
        val = (val << 7) | (b & 0x7F) as u64;
        if b & 0x80 == 0 {
            arcs.push(val);
            val = 0;
        }
    }
    arcs.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(".")
}

/// der_string_value 按字符串标签解码 DN 属性值。
///
/// 支持 UTF8String/PrintableString/IA5String/VisibleString/T61String/BMPString；
/// 其他标签退化为十六进制显示。
fn der_string_value(tag: u8, content: &[u8]) -> String {
    match tag {
        // UTF8String / PrintableString / IA5String / VisibleString：直接按 UTF-8（ASCII 兼容）
        0x0C | 0x13 | 0x16 | 0x1A => String::from_utf8_lossy(content).into_owned(),
        // T61String（Teletex，实际常见为 Latin-1）：逐字节映射到 Unicode
        0x14 => content.iter().map(|&b| b as char).collect(),
        // BMPString：UTF-16 大端
        0x1E => {
            let mut units = Vec::with_capacity(content.len() / 2);
            for pair in content.chunks_exact(2) {
                units.push(u16::from_be_bytes([pair[0], pair[1]]));
            }
            String::from_utf16_lossy(&units)
        }
        _ => {
            let hex = content.iter().map(|b| format!("{:02X}", b)).collect::<String>();
            format!("<0x{:02X}>{}", tag, hex)
        }
    }
}

/// fmt_utc 把 Unix 秒格式化为 "YYYY-MM-DD HH:MM:SS UTC" 显示串。
fn fmt_utc(ts: i64) -> String {
    DateTime::from_millis_utc(ts * 1000).format("2006-01-02 15:04:05 UTC")
}

// ===========================================================================
// 注册
// ===========================================================================

/// register 把 TLS 内置函数注册到 VM（阻塞式网络 IO，用 blocking 注册）。
pub fn register(vm: &mut VM) {
    vm.register_builtin_doc_blocking("tlsGetCertInfo", bi_tls_get_cert_info, &DOC_TLS_GET_CERT_INFO);
}
