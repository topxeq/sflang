//! builtins_proxy.rs — SOCKS/HTTP 二合一代理与端口透传
//!
//! 设计要点：
//!   - 纯标准库实现，复用 builtins_tcp 的 TcpConn 和 pipe_connections
//!   - SOCKS5 (RFC 1928)：支持 CONNECT 命令，无认证模式
//!   - HTTP 代理：支持 CONNECT 方法（HTTPS 隧道）和 GET/POST 转发（HTTP 明文）
//!   - 自动协议识别：读取首字节判断 SOCKS5 (0x05) 或 HTTP（C/G/P/D/H/O/T 开头）
//!   - 端口透传：监听端口，每个连接转发到固定目标地址
//!   - 支持 handler 回调做访问控制
//!
//! 函数列表：
//!   proxyListen(addr, handler)        — 启动 SOCKS/HTTP 二合一代理
//!   proxyStop(server)                 — 停止代理服务器
//!   portForward(listenAddr, targetAddr) — 启动端口转发
//!   portForwardStop(server)          — 停止端口转发

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::builtins_helpers as bh;
use crate::builtins_tcp::{pipe_connections, TcpConn};
use crate::function::BuiltinDoc;
use crate::value::{error_value, Value};
use crate::vm::VM;

static DOC_PROXY_LISTEN: BuiltinDoc = BuiltinDoc {
    category: "proxy",
    signature: "proxyListen(addr [, handler]) -> proxyServer",
    summary: "启动 SOCKS5/HTTP 二合一代理服务器，自动识别客户端协议。",
    params: &[
        ("addr", "监听地址，如 \"0.0.0.0:1080\" 或 \"127.0.0.1:1080\""),
        ("handler", "可选 func(targetAddr, proto, clientAddr) -> bool，返回 true 允许、false 拒绝连接；省略则允许全部"),
    ],
    returns: "proxyServer 对象，传给 proxyStop 停止",
    examples: &[
        "s := proxyListen(\"127.0.0.1:1080\")  // 启动代理，允许所有连接",
        "s := proxyListen(\"0.0.0.0:1080\", func(t, p, c) { return !contains(t, \"blocked.host\") })  // 自定义访问控制",
    ],
    errors: &[
        "proxyListen() 绑定 'xxx' 失败（可能原因：地址被占用或权限不足）",
        "proxyListen() 第 2 个参数应为 function，得到 X（可能原因：参数顺序错误）",
        "协议处理错误：未知协议 / 不支持的 SOCKS5 命令（仅支持 CONNECT=1）",
    ],
};

static DOC_PROXY_STOP: BuiltinDoc = BuiltinDoc {
    category: "proxy",
    signature: "proxyStop(server) -> undefined",
    summary: "停止 proxyListen 启动的代理服务器。",
    params: &[("server", "proxyListen 返回的 proxyServer 对象")],
    returns: "undefined",
    examples: &["proxyStop(s)  // 停止代理"],
    errors: &[
        "proxyStop() 参数不是代理服务器（可能原因：传入了错误类型，应使用 proxyListen 的返回值）",
        "proxyStop() 参数应为代理服务器，得到 X（可能原因：参数类型不匹配）",
    ],
};

static DOC_PORT_FORWARD: BuiltinDoc = BuiltinDoc {
    category: "proxy",
    signature: "portForward(listenAddr, targetAddr) -> forwardServer",
    summary: "启动端口转发，将监听端口收到的连接全部透传到目标地址。",
    params: &[
        ("listenAddr", "监听地址，如 \"0.0.0.0:8080\""),
        ("targetAddr", "目标地址，如 \"127.0.0.1:80\""),
    ],
    returns: "forwardServer 对象，传给 portForwardStop 停止",
    examples: &["pf := portForward(\"0.0.0.0:8080\", \"127.0.0.1:80\")  // 转发 8080 到本地 80 端口"],
    errors: &[
        "portForward() 绑定 'xxx' 失败（可能原因：地址被占用或权限不足）",
        "连接目标 xxx 失败（目标未启动或防火墙拦截，仅记录日志不中断转发器）",
    ],
};

static DOC_PORT_FORWARD_STOP: BuiltinDoc = BuiltinDoc {
    category: "proxy",
    signature: "portForwardStop(server) -> undefined",
    summary: "停止 portForward 启动的端口转发。",
    params: &[("server", "portForward 返回的 forwardServer 对象")],
    returns: "undefined",
    examples: &["portForwardStop(pf)  // 停止转发"],
    errors: &[
        "portForwardStop() 参数不是转发服务器（可能原因：传入了错误类型，应使用 portForward 的返回值）",
        "portForwardStop() 参数应为转发服务器，得到 X（可能原因：参数类型不匹配）",
    ],
};

/// register 注册所有代理相关内置函数。
pub fn register(vm: &mut VM) {
    vm.register_builtin_doc("proxyListen", bi_proxy_listen, &DOC_PROXY_LISTEN);
    vm.register_builtin_doc("proxyStop", bi_proxy_stop, &DOC_PROXY_STOP);
    vm.register_builtin_doc("portForward", bi_port_forward, &DOC_PORT_FORWARD);
    vm.register_builtin_doc("portForwardStop", bi_port_forward_stop, &DOC_PORT_FORWARD_STOP);
}

// ============ 类型定义 ============

/// ProxyServer 代理服务器对象。
pub struct ProxyServer {
    /// stop_flag 停止标志，true 时 accept 循环退出。
    pub stop_flag: Arc<AtomicBool>,
}

/// ForwardServer 端口转发服务器对象。
pub struct ForwardServer {
    /// stop_flag 停止标志，true 时 accept 循环退出。
    pub stop_flag: Arc<AtomicBool>,
}

// ============ SOCKS/HTTP 代理 ============

/// bi_proxy_listen 启动 SOCKS/HTTP 二合一代理服务器。
///
/// 用法：
///   proxyListen(addr)                       — 启动代理，允许所有连接
///   proxyListen(addr, handler)              — 启动代理，handler 做访问控制
///
/// addr: "0.0.0.0:1080" 或 "127.0.0.1:1080"
/// handler: func(targetAddr, proto, clientAddr) { return true/false }
///   targetAddr: "host:port" 目标地址
///   proto: "socks5" 或 "http"
///   clientAddr: "ip:port" 客户端地址
///   返回 true 允许连接，false 拒绝
///
/// 同时支持 SOCKS5 和 HTTP CONNECT 代理协议。
/// 客户端可用其中任一种协议连接，服务器自动识别。
fn bi_proxy_listen(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let addr = bh::as_str(args, 0, "proxyListen")?;
    // handler 可选
    let handler = if args.len() > 1 {
        match &args[1] {
            Value::Func(_) | Value::Builtin(_) => Some(args[1].clone()),
            Value::Undefined => None,
            other => return Err(error_value(format!(
                "proxyListen() 第 2 个参数应为 function，得到 {} (可能原因：参数顺序错误)",
                other.type_name(),
            ))),
        }
    } else {
        None
    };

    let listener = TcpListener::bind(addr).map_err(|e| {
        error_value(format!(
            "proxyListen() 绑定 '{}' 失败: {} (可能原因：地址被占用或权限不足)",
            addr, e,
        ))
    })?;

    listener.set_nonblocking(true).map_err(|e| {
        error_value(format!(
            "proxyListen() 设置非阻塞模式失败: {}", e,
        ))
    })?;

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();
    let globals = vm.globals_handle();
    let out = vm.output_handle();

    std::thread::spawn(move || {
        while !stop_clone.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, client_addr)) => {
                    // 恢复阻塞模式
                    let _ = stream.set_nonblocking(false);
                    let conn = Arc::new(TcpConn {
                        stream: Mutex::new(stream),
                    });
                    let handler_clone = handler.clone();
                    let globals_clone = globals.clone();
                    let out_clone = out.clone();

                    std::thread::spawn(move || {
                        let mut vm = VM::new();
                        vm.set_globals_handle(globals_clone);
                        vm.set_output_handle(out_clone);
                        if let Err(e) = handle_proxy_conn(&conn, &handler_clone, &mut vm, &client_addr) {
                            let _ = writeln!(
                                vm.output_handle().lock().unwrap(),
                                "[proxy] {} : {}",
                                client_addr, e,
                            );
                        }
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => {
                    let _ = writeln!(
                        out.lock().unwrap(),
                        "[proxy] accept 错误: {} (服务器将停止)", e,
                    );
                    break;
                }
            }
        }
    });

    Ok(Value::Native(Arc::new(Arc::new(ProxyServer { stop_flag }))))
}

/// bi_proxy_stop 停止代理服务器。
fn bi_proxy_stop(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    match &args[0] {
        Value::Native(n) => {
            if let Some(s) = n.downcast_ref::<Arc<ProxyServer>>() {
                s.stop_flag.store(true, Ordering::Relaxed);
                Ok(Value::Undefined)
            } else {
                Err(error_value(
                    "proxyStop() 参数不是代理服务器 (可能原因：传入了错误类型，应使用 proxyListen 的返回值)",
                ))
            }
        }
        other => Err(error_value(format!(
            "proxyStop() 参数应为代理服务器，得到 {} (可能原因：参数类型不匹配)",
            other.type_name(),
        ))),
    }
}

// ============ 端口转发 ============

/// bi_port_forward 启动端口转发。
///
/// 用法：
///   portForward(listenAddr, targetAddr) — 启动端口转发
///
/// listenAddr: "0.0.0.0:8080" 监听地址
/// targetAddr: "127.0.0.1:80" 目标地址
///
/// 每个新连接自动转发到 targetAddr，双向数据透传。
fn bi_port_forward(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let listen_addr = bh::as_str(args, 0, "portForward")?.to_string();
    let target_addr = bh::as_str(args, 1, "portForward")?.to_string();

    let listener = TcpListener::bind(&listen_addr).map_err(|e| {
        error_value(format!(
            "portForward() 绑定 '{}' 失败: {} (可能原因：地址被占用或权限不足)",
            listen_addr, e,
        ))
    })?;

    listener.set_nonblocking(true).map_err(|e| {
        error_value(format!(
            "portForward() 设置非阻塞模式失败: {}", e,
        ))
    })?;

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();
    let out = vm.output_handle();

    std::thread::spawn(move || {
        while !stop_clone.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, client_addr)) => {
                    let _ = stream.set_nonblocking(false);
                    let conn = Arc::new(TcpConn {
                        stream: Mutex::new(stream),
                    });
                    let target = target_addr.clone();
                    let out_clone = out.clone();

                    std::thread::spawn(move || {
                        match TcpStream::connect(&target) {
                            Ok(target_stream) => {
                                let target_conn = Arc::new(TcpConn {
                                    stream: Mutex::new(target_stream),
                                });
                                pipe_connections(&conn, &target_conn);
                            }
                            Err(e) => {
                                let _ = writeln!(
                                    out_clone.lock().unwrap(),
                                    "[portForward] {} -> {} 连接失败: {}",
                                    client_addr, target, e,
                                );
                            }
                        }
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => {
                    let _ = writeln!(
                        out.lock().unwrap(),
                        "[portForward] accept 错误: {} (服务器将停止)", e,
                    );
                    break;
                }
            }
        }
    });

    Ok(Value::Native(Arc::new(Arc::new(ForwardServer { stop_flag }))))
}

/// bi_port_forward_stop 停止端口转发。
fn bi_port_forward_stop(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    match &args[0] {
        Value::Native(n) => {
            if let Some(s) = n.downcast_ref::<Arc<ForwardServer>>() {
                s.stop_flag.store(true, Ordering::Relaxed);
                Ok(Value::Undefined)
            } else {
                Err(error_value(
                    "portForwardStop() 参数不是转发服务器 (可能原因：传入了错误类型，应使用 portForward 的返回值)",
                ))
            }
        }
        other => Err(error_value(format!(
            "portForwardStop() 参数应为转发服务器，得到 {} (可能原因：参数类型不匹配)",
            other.type_name(),
        ))),
    }
}

// ============ 协议处理 ============

/// handle_proxy_conn 处理单个代理连接，自动识别 SOCKS5 或 HTTP 协议。
fn handle_proxy_conn(
    conn: &Arc<TcpConn>,
    handler: &Option<Value>,
    vm: &mut VM,
    client_addr: &std::net::SocketAddr,
) -> Result<(), String> {
    let mut stream = conn.stream.lock().unwrap();

    // 读取首字节判断协议
    let mut first_byte = [0u8; 1];
    stream.read_exact(&mut first_byte).map_err(|e| format!("读取首字节失败: {}", e))?;

    match first_byte[0] {
        0x05 => {
            drop(stream);
            handle_socks5(conn, handler, vm, client_addr)
        }
        b'C' | b'G' | b'P' | b'H' | b'O' | b'D' | b'T' => {
            drop(stream);
            handle_http(conn, first_byte[0], handler, vm, client_addr)
        }
        _ => Err(format!("未知协议: 首字节 0x{:02x}", first_byte[0])),
    }
}

/// handle_socks5 处理 SOCKS5 协议 (RFC 1928)。
///
/// 仅支持 CONNECT 命令 (CMD=0x01)，无认证模式 (METHOD=0x00)。
fn handle_socks5(
    conn: &Arc<TcpConn>,
    handler: &Option<Value>,
    vm: &mut VM,
    client_addr: &std::net::SocketAddr,
) -> Result<(), String> {
    let mut stream = conn.stream.lock().unwrap();

    // 1. 认证方法协商: VER(已由 handle_proxy_conn 消费) NMETHODS(1) METHODS(var)
    let mut nmethods_buf = [0u8; 1];
    stream.read_exact(&mut nmethods_buf).map_err(|e| format!("读取 SOCKS5 NMETHODS 失败: {}", e))?;
    let nmethods = nmethods_buf[0] as usize;
    if nmethods == 0 {
        return Err("SOCKS5 NMETHODS 为 0".into());
    }
    let mut methods = vec![0u8; nmethods];
    stream.read_exact(&mut methods).map_err(|e| format!("读取认证方法失败: {}", e))?;

    // 选择无认证 (0x00)
    stream.write_all(&[0x05, 0x00]).map_err(|e| format!("写入方法选择失败: {}", e))?;
    stream.flush().ok();

    // 2. 读取请求: VER(1) CMD(1) RSV(1) ATYP(1) DST.ADDR(var) DST.PORT(2)
    let mut req = [0u8; 4];
    stream.read_exact(&mut req).map_err(|e| format!("读取 SOCKS5 请求失败: {}", e))?;
    if req[0] != 0x05 {
        return Err(format!("SOCKS5 版本不匹配: {}", req[0]));
    }
    if req[1] != 0x01 {
        // 不支持的命令
        stream.write_all(&[0x05, 0x07, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).ok();
        return Err(format!("不支持的 SOCKS5 命令: {} (仅支持 CONNECT=1)", req[1]));
    }

    // 解析目标地址
    let target_addr = match req[3] {
        0x01 => {
            // IPv4
            let mut addr = [0u8; 4];
            stream.read_exact(&mut addr).map_err(|e| format!("读取 IPv4 地址失败: {}", e))?;
            let mut port = [0u8; 2];
            stream.read_exact(&mut port).map_err(|e| format!("读取端口失败: {}", e))?;
            format!("{}.{}.{}.{}:{}", addr[0], addr[1], addr[2], addr[3],
                u16::from_be_bytes(port))
        }
        0x03 => {
            // 域名
            let mut len_buf = [0u8; 1];
            stream.read_exact(&mut len_buf).map_err(|e| format!("读取域名长度失败: {}", e))?;
            let len = len_buf[0] as usize;
            let mut domain = vec![0u8; len];
            stream.read_exact(&mut domain).map_err(|e| format!("读取域名失败: {}", e))?;
            let mut port = [0u8; 2];
            stream.read_exact(&mut port).map_err(|e| format!("读取端口失败: {}", e))?;
            let domain = String::from_utf8_lossy(&domain);
            format!("{}:{}", domain, u16::from_be_bytes(port))
        }
        0x04 => {
            // IPv6
            let mut addr = [0u8; 16];
            stream.read_exact(&mut addr).map_err(|e| format!("读取 IPv6 地址失败: {}", e))?;
            let mut port = [0u8; 2];
            stream.read_exact(&mut port).map_err(|e| format!("读取端口失败: {}", e))?;
            let mut parts = Vec::new();
            for i in (0..16).step_by(2) {
                parts.push(format!("{:x}", u16::from_be_bytes([addr[i], addr[i+1]])));
            }
            format!("[{}]:{}", parts.join(":"), u16::from_be_bytes(port))
        }
        other => {
            stream.write_all(&[0x05, 0x08, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).ok();
            return Err(format!("不支持的地址类型: {}", other));
        }
    };

    drop(stream);

    // 3. 调用 handler 做访问控制
    let allowed = check_access(handler, &target_addr, "socks5", client_addr, vm);
    if !allowed {
        let mut stream = conn.stream.lock().unwrap();
        // REP = 0x02 (connection not allowed by ruleset)
        stream.write_all(&[0x05, 0x02, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).ok();
        return Ok(());
    }

    // 4. 连接目标
    let target = match TcpStream::connect(&target_addr) {
        Ok(s) => s,
        Err(e) => {
            let mut stream = conn.stream.lock().unwrap();
            // REP = 0x05 (connection refused)
            stream.write_all(&[0x05, 0x05, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).ok();
            return Err(format!("连接目标 {} 失败: {}", target_addr, e));
        }
    };

    // 5. 回复成功
    {
        let mut stream = conn.stream.lock().unwrap();
        stream.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .map_err(|e| format!("写入成功回复失败: {}", e))?;
        stream.flush().ok();
    }

    // 6. 双向转发
    let target_conn = Arc::new(TcpConn { stream: Mutex::new(target) });
    pipe_connections(conn, &target_conn);
    Ok(())
}

/// handle_http 处理 HTTP 代理协议。
///
/// 支持 CONNECT 方法（HTTPS 隧道）和 GET/POST 转发（HTTP 明文代理）。
fn handle_http(
    conn: &Arc<TcpConn>,
    first_byte: u8,
    handler: &Option<Value>,
    vm: &mut VM,
    client_addr: &std::net::SocketAddr,
) -> Result<(), String> {
    let mut stream = conn.stream.lock().unwrap();

    // 读取完整 HTTP 请求头（以 \r\n\r\n 结尾）
    let mut buf = vec![first_byte];
    loop {
        let mut tmp = [0u8; 1];
        stream.read_exact(&mut tmp).map_err(|e| format!("读取 HTTP 请求失败: {}", e))?;
        buf.push(tmp[0]);
        if buf.len() >= 4 && &buf[buf.len()-4..] == b"\r\n\r\n" {
            break;
        }
        if buf.len() > 16384 {
            return Err("HTTP 请求头过大 (>16KB)".into());
        }
    }

    let request = String::from_utf8_lossy(&buf);
    let first_line = request.lines().next().ok_or("空请求")?;
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(format!("无效的 HTTP 请求行: {}", first_line));
    }

    let method = parts[0];
    let target = parts[1];

    if method == "CONNECT" {
        // HTTPS 隧道代理
        let target_addr = target.to_string();
        drop(stream);

        let allowed = check_access(handler, &target_addr, "http", client_addr, vm);
        if !allowed {
            let mut stream = conn.stream.lock().unwrap();
            stream.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").ok();
            return Ok(());
        }

        let target_stream = match TcpStream::connect(&target_addr) {
            Ok(s) => s,
            Err(_) => {
                let mut stream = conn.stream.lock().unwrap();
                stream.write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n").ok();
                return Err(format!("连接目标 {} 失败", target_addr));
            }
        };

        // 回复成功
        {
            let mut stream = conn.stream.lock().unwrap();
            stream.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .map_err(|e| format!("写入代理回复失败: {}", e))?;
            stream.flush().ok();
        }

        // 双向转发
        let target_conn = Arc::new(TcpConn { stream: Mutex::new(target_stream) });
        pipe_connections(conn, &target_conn);
        Ok(())
    } else {
        // HTTP 明文代理 (GET/POST/PUT/DELETE 等)
        // target 格式: http://host:port/path
        let (host, port, path) = parse_http_url(target)?;
        let target_addr = format!("{}:{}", host, port);

        drop(stream);

        let allowed = check_access(handler, &target_addr, "http", client_addr, vm);
        if !allowed {
            let mut stream = conn.stream.lock().unwrap();
            stream.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").ok();
            return Ok(());
        }

        let target_stream = match TcpStream::connect(&target_addr) {
            Ok(s) => s,
            Err(_) => {
                let mut stream = conn.stream.lock().unwrap();
                stream.write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n").ok();
                return Err(format!("连接目标 {} 失败", target_addr));
            }
        };

        // 重写请求行：去掉代理 URL 前缀，只保留 path
        // 原始: GET http://host:port/path HTTP/1.1
        // 重写: GET /path HTTP/1.1
        let mut new_request = format!("{} {} HTTP/1.1\r\n", method, path);
        // 保留原始头部，去掉 Proxy-Connection
        for line in request.lines().skip(1) {
            if line.to_lowercase().starts_with("proxy-connection:") {
                continue;
            }
            new_request.push_str(line);
            new_request.push_str("\r\n");
        }
        new_request.push_str("\r\n");

        // 发送重写后的请求
        {
            let mut target_guard = &target_stream;
            target_guard.write_all(new_request.as_bytes())
                .map_err(|e| format!("写入目标失败: {}", e))?;
            target_guard.flush().ok();
        }

        // 双向转发
        let target_conn = Arc::new(TcpConn { stream: Mutex::new(target_stream) });
        pipe_connections(conn, &target_conn);
        Ok(())
    }
}

/// check_access 调用 handler 回调做访问控制。
///
/// handler 为 None 时允许所有连接。
/// handler(targetAddr, proto, clientAddr) 返回的值 is_truthy 为 true 则允许。
fn check_access(
    handler: &Option<Value>,
    target_addr: &str,
    proto: &str,
    client_addr: &std::net::SocketAddr,
    vm: &mut VM,
) -> bool {
    let handler = match handler {
        Some(h) => h,
        None => return true,
    };

    let args = vec![
        Value::str_from(target_addr.to_string()),
        Value::str_from(proto.to_string()),
        Value::str_from(client_addr.to_string()),
    ];

    match vm.call_function_value(handler.clone(), args) {
        Ok(v) => v.is_truthy(),
        Err(_) => false,
    }
}

/// parse_http_url 解析 HTTP 代理 URL。
///
/// 输入: http://host:port/path 或 http://host/path
/// 输出: (host, port, path)
fn parse_http_url(url: &str) -> Result<(String, u16, String), String> {
    let without_scheme = if let Some(s) = url.strip_prefix("http://") {
        s
    } else if let Some(s) = url.strip_prefix("https://") {
        s
    } else {
        // 不带 scheme 的 URL，直接当 host:port/path 处理
        url
    };

    let (host_port, path) = match without_scheme.find('/') {
        Some(i) => (&without_scheme[..i], &without_scheme[i..]),
        None => (without_scheme, "/"),
    };

    let (host, port) = match host_port.find(':') {
        Some(i) => {
            let h = &host_port[..i];
            let p: u16 = host_port[i+1..].parse().map_err(|_| format!("无效端口: {}", &host_port[i+1..]))?;
            (h.to_string(), p)
        }
        None => (host_port.to_string(), 80),
    };

    Ok((host, port, path.to_string()))
}

// ============ 测试 ============

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Sflang;
    use std::time::Duration;

    /// 辅助：启动回显 TCP 服务器
    fn start_echo_server() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(mut s) => {
                        std::thread::spawn(move || {
                            let mut buf = [0u8; 1024];
                            loop {
                                match s.read(&mut buf) {
                                    Ok(0) | Err(_) => break,
                                    Ok(n) => {
                                        let _ = s.write_all(&buf[..n]);
                                        let _ = s.flush();
                                    }
                                }
                            }
                        });
                    }
                    Err(_) => break,
                }
            }
        });
        port
    }

    #[test]
    fn test_port_forward_basic() {
        let echo_port = start_echo_server();
        std::thread::sleep(Duration::from_millis(50));

        let mut sf = Sflang::new();
        // 只验证 portForward 能编译和注册
        let _ = sf.run_string(&format!(r#"
            pf := portForward("127.0.0.1:0", "127.0.0.1:{}")
            sleepMs(100)
            portForwardStop(pf)
        "#, echo_port));
        // 不报错即通过
    }

    #[test]
    fn test_parse_http_url() {
        let (host, port, path) = parse_http_url("http://example.com:8080/path").unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(port, 8080);
        assert_eq!(path, "/path");

        let (host, port, path) = parse_http_url("http://example.com/path").unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(port, 80);
        assert_eq!(path, "/path");

        let (host, port, path) = parse_http_url("http://example.com").unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(port, 80);
        assert_eq!(path, "/");
    }
}
