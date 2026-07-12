//! builtins_tcp.rs — TCP socket server/client 内置函数
//!
//! 设计要点：
//!   - 纯标准库实现（std::net::TcpListener / TcpStream）
//!   - 连接对象用 Value::Native 包装，支持跨线程共享（Arc<TcpStream> 需 Mutex 保护）
//!   - 服务器 accept 在独立线程循环执行，通过回调函数处理每个连接
//!   - 客户端 connect 返回连接对象，支持 read/write/close
//!   - 全部 IO 操作有超时选项（用 set_read_timeout/set_write_timeout）
//!   - 错误信息 AI 友好，包含可能原因
//!
//! 函数列表：
//!   tcpListen(addr, handler)       — 启动 TCP 服务器，handler(conn) 处理每个连接
//!   tcpConnect(host, port)         — 连接到 TCP 服务器，返回 conn 对象
//!   tcpRead(conn, n)               — 从连接读取 n 字节
//!   tcpReadLine(conn)              — 读取一行（以 \n 结尾）
//!   tcpWrite(conn, data)           — 写数据到连接
//!   tcpWriteLine(conn, data)       — 写一行（追加 \n）
//!   tcpClose(conn)                 — 关闭连接
//!   tcpSetTimeout(conn, readMs, writeMs) — 设置读写超时
//!   tcpRemoteAddr(conn)            — 获取对端地址
//!   tcpLocalAddr(conn)             — 获取本端地址
//!   tcpStopServer(server)          — 停止 TCP 服务器

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::builtins_helpers as bh;
use crate::value::{error_value, Value};
use crate::vm::VM;

/// register 注册所有 TCP 内置函数。
pub fn register(vm: &mut VM) {
    vm.register_builtin("tcpListen", bi_tcp_listen);
    vm.register_builtin("tcpConnect", bi_tcp_connect);
    vm.register_builtin("tcpRead", bi_tcp_read);
    vm.register_builtin("tcpReadLine", bi_tcp_read_line);
    vm.register_builtin("tcpWrite", bi_tcp_write);
    vm.register_builtin("tcpWriteLine", bi_tcp_write_line);
    vm.register_builtin("tcpClose", bi_tcp_close);
    vm.register_builtin("tcpSetTimeout", bi_tcp_set_timeout);
    vm.register_builtin("tcpRemoteAddr", bi_tcp_remote_addr);
    vm.register_builtin("tcpLocalAddr", bi_tcp_local_addr);
    vm.register_builtin("tcpStopServer", bi_tcp_stop_server);
    vm.register_builtin("tcpPipe", bi_tcp_pipe);
}

// ============ 类型定义 ============

/// TcpConn Sflang 的 TCP 连接对象。
///
/// 用 Mutex<TcpStream> 保护，使 TcpStream 可跨线程共享。
/// 脚本层通过 Value::Native(Arc<TcpConn>) 引用。
pub struct TcpConn {
    /// stream 底层 TCP 流。
    pub stream: Mutex<TcpStream>,
}

/// TcpServer Sflang 的 TCP 服务器对象。
///
/// 持有 listener 和停止标志。accept 循环在独立线程中执行，
/// 检测 stop_flag 后退出。
pub struct TcpServer {
    /// stop_flag 停止标志，true 时 accept 循环退出。
    pub stop_flag: Arc<AtomicBool>,
}

// ============ 辅助函数 ============

/// conn_downcast 从 Value 中提取 TcpConn 引用。
pub fn conn_downcast<'a>(v: &'a Value, fn_name: &str) -> Result<&'a Arc<TcpConn>, Value> {
    match v {
        Value::Native(n) => n.downcast_ref::<Arc<TcpConn>>().ok_or_else(|| {
            error_value(format!(
                "{}() 参数不是 TCP 连接 (可能原因：传入错误类型或 undefined，应先用 tcpConnect 或 tcpListen 回调获取)",
                fn_name,
            ))
        }),
        Value::Undefined => Err(error_value(format!(
            "{}() 参数为 undefined (可能原因：变量未初始化)", fn_name,
        ))),
        other => Err(error_value(format!(
            "{}() 参数应为 TCP 连接，得到 {} (可能原因：参数顺序错误)",
            fn_name, other.type_name(),
        ))),
    }
}

/// server_downcast 从 Value 中提取 TcpServer 引用。
fn server_downcast<'a>(v: &'a Value, fn_name: &str) -> Result<&'a Arc<TcpServer>, Value> {
    match v {
        Value::Native(n) => n.downcast_ref::<Arc<TcpServer>>().ok_or_else(|| {
            error_value(format!(
                "{}() 参数不是 TCP 服务器 (可能原因：传入错误类型，应先用 tcpListen 获取)",
                fn_name,
            ))
        }),
        Value::Undefined => Err(error_value(format!(
            "{}() 参数为 undefined (可能原因：变量未初始化)", fn_name,
        ))),
        other => Err(error_value(format!(
            "{}() 参数应为 TCP 服务器，得到 {}", fn_name, other.type_name(),
        ))),
    }
}

/// to_bytes 将参数转为字节 Vec（接受 string/bytes/byteArray）。
fn to_bytes(v: &Value, fn_name: &str) -> Result<Vec<u8>, Value> {
    match v {
        Value::Str(s) => Ok(s.as_bytes().to_vec()),
        Value::Bytes(b) => Ok(b.as_ref().to_vec()),
        Value::ByteArray(b) => Ok(b.lock().unwrap().clone()),
        other => Err(error_value(format!(
            "{}() 数据参数应为 string/bytes/byteArray，得到 {} (可能原因：参数类型不匹配)",
            fn_name, other.type_name(),
        ))),
    }
}

// ============ TCP 服务器 ============

/// bi_tcp_listen 启动 TCP 服务器。
///
/// 用法：
///   tcpListen(addr, handler)         — 启动并立即返回 server 对象
///   tcpListen(addr, handler, backlog) — 指定 backlog
///
/// addr: "0.0.0.0:8080" 或 ":8080"
/// handler: func(conn) { ... } 每个连接在独立线程中调用
///
/// 返回 TcpServer 对象，可用 tcpStopServer 停止。
/// 连接在独立线程中处理，handler 接收 conn 对象。
fn bi_tcp_listen(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let addr = bh::as_str(args, 0, "tcpListen")?;
    bh::require_arg(args, 1, "tcpListen")?;
    let handler = args[1].clone();

    // 绑定地址
    let listener = TcpListener::bind(addr).map_err(|e| {
        error_value(format!(
            "tcpListen() 绑定 '{}' 失败: {} (可能原因：地址被占用或权限不足)",
            addr, e,
        ))
    })?;

    // 设置非阻塞模式，以便通过 stop_flag 退出 accept 循环
    listener.set_nonblocking(true).map_err(|e| {
        error_value(format!(
            "tcpListen() 设置非阻塞模式失败: {} (可能原因：系统不支持非阻塞 IO)", e,
        ))
    })?;

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();

    // 共享 VM 的全局变量和输出句柄
    let globals = vm.globals_handle();
    let out = vm.output_handle();

    // accept 循环线程
    std::thread::spawn(move || {
        let handler = handler;
        while !stop_clone.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, addr)) => {
                    // 将连接恢复为阻塞模式（listener 是非阻塞的，accept 出的 stream 也继承）
                    let _ = stream.set_nonblocking(false);
                    // 为每个连接创建 TcpConn 并在新线程调用 handler
                    let conn = Arc::new(TcpConn {
                        stream: Mutex::new(stream),
                    });
                    let conn_val = Value::Native(Arc::new(conn));
                    let globals_clone = globals.clone();
                    let out_clone = out.clone();
                    let handler_clone = handler.clone();

                    std::thread::spawn(move || {
                        let mut vm = VM::new();
                        vm.set_globals_handle(globals_clone);
                        vm.set_output_handle(out_clone);
                        // 调用 handler(conn)
                        if let Err(e) = vm.call_function_value(handler_clone, vec![conn_val]) {
                            let msg = match &e {
                                Value::Error(se) => se.message.clone(),
                                other => other.to_str(),
                            };
                            let _ = writeln!(
                                vm.output_handle().lock().unwrap(),
                                "[tcpListen 连接处理异常] 来自 {} : {}",
                                addr, msg
                            );
                        }
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // 非阻塞模式下无连接，短暂休眠避免空转
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => {
                    // 其他错误（如 listener 关闭）
                    let _ = writeln!(
                        out.lock().unwrap(),
                        "[tcpListen] accept 错误: {} (服务器将停止)", e,
                    );
                    break;
                }
            }
        }
    });

    Ok(Value::Native(Arc::new(Arc::new(TcpServer { stop_flag }))))
}

// ============ TCP 客户端 ============

/// bi_tcp_connect 连接到 TCP 服务器。
///
/// 用法：
///   tcpConnect(host, port)           — 连接，默认无超时
///   tcpConnect(host, port, timeoutMs) — 指定连接超时
///
/// host: 主机名或 IP
/// port: 端口号
/// 返回 TcpConn 对象。
fn bi_tcp_connect(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let host = bh::as_str(args, 0, "tcpConnect")?;
    let port = bh::as_int(args, 1, "tcpConnect")?;
    let addr = format!("{}:{}", host, port);

    let stream = if args.len() > 2 {
        // 带超时连接
        let timeout_ms = bh::as_int(args, 2, "tcpConnect")?;
        let timeout = Duration::from_millis(timeout_ms as u64);
        let addrs = std::net::ToSocketAddrs::to_socket_addrs(&addr).map_err(|e| {
            error_value(format!(
                "tcpConnect() 解析地址 '{}' 失败: {} (可能原因：主机名无法解析)",
                addr, e,
            ))
        })?;
        let mut last_err = None;
        let mut connected = None;
        for sa in addrs {
            match TcpStream::connect_timeout(&sa, timeout) {
                Ok(s) => { connected = Some(s); break; }
                Err(e) => { last_err = Some(e); }
            }
        }
        match connected {
            Some(s) => s,
            None => return Err(error_value(format!(
                "tcpConnect() 连接 '{}' 失败: {} (可能原因：目标不可达或超时 {}ms)",
                addr, last_err.map(|e| e.to_string()).unwrap_or_default(), timeout_ms,
            ))),
        }
    } else {
        // 无超时连接
        TcpStream::connect(&addr).map_err(|e| {
            error_value(format!(
                "tcpConnect() 连接 '{}' 失败: {} (可能原因：目标未启动或防火墙拦截)",
                addr, e,
            ))
        })?
    };

    // 禁用 Nagle 算法，降低延迟
    let _ = stream.set_nodelay(true);

    let conn = Arc::new(TcpConn {
        stream: Mutex::new(stream),
    });
    Ok(Value::Native(Arc::new(conn)))
}

// ============ 读写操作 ============

/// bi_tcp_read 从连接读取指定长度字节。
///
/// 用法：
///   tcpRead(conn, n) — 读取 n 字节，返回 bytes
///
/// 如果连接关闭或读到 EOF，返回的 bytes 可能少于 n。
/// 完全无数据可读时返回 undefined。
fn bi_tcp_read(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let conn = conn_downcast(&args[0], "tcpRead")?;
    let n = bh::as_int(args, 1, "tcpRead")? as usize;
    if n == 0 {
        return Ok(Value::Bytes(Arc::new(Vec::new())));
    }
    let mut buf = vec![0u8; n];
    let mut guard = conn.stream.lock().unwrap();
    let read = guard.read(&mut buf).map_err(|e| {
        error_value(format!(
            "tcpRead() 读取失败: {} (可能原因：连接已关闭或网络中断)", e,
        ))
    })?;
    if read == 0 {
        return Ok(Value::Undefined);
    }
    buf.truncate(read);
    Ok(Value::Bytes(Arc::new(buf)))
}

/// bi_tcp_read_line 读取一行（以 \n 结尾）。
///
/// 用法：
///   tcpReadLine(conn) — 读取一行，返回 string（不含尾 \n）
///
/// 如果连接关闭返回 undefined。
fn bi_tcp_read_line(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let conn = conn_downcast(&args[0], "tcpReadLine")?;
    let mut guard = conn.stream.lock().unwrap();
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match guard.read(&mut byte) {
            Ok(0) => {
                // EOF
                if buf.is_empty() {
                    return Ok(Value::Undefined);
                }
                break;
            }
            Ok(_) => {
                if byte[0] == b'\n' {
                    break;
                }
                buf.push(byte[0]);
            }
            Err(e) => {
                return Err(error_value(format!(
                    "tcpReadLine() 读取失败: {} (可能原因：连接已关闭或网络中断)", e,
                )));
            }
        }
    }
    // 去除尾 \r（如有）
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    let s = String::from_utf8_lossy(&buf).into_owned();
    Ok(Value::str_from(s))
}

/// bi_tcp_write 写数据到连接。
///
/// 用法：
///   tcpWrite(conn, data) — data 为 string/bytes/byteArray
///
/// 返回写入的字节数。
fn bi_tcp_write(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let conn = conn_downcast(&args[0], "tcpWrite")?;
    let data = to_bytes(&args[1], "tcpWrite")?;
    let mut guard = conn.stream.lock().unwrap();
    let written = guard.write(&data).map_err(|e| {
        error_value(format!(
            "tcpWrite() 写入失败: {} (可能原因：连接已关闭或对端拒绝)", e,
        ))
    })?;
    // 刷新确保数据立即发送
    let _ = guard.flush();
    Ok(Value::Int(written as i64))
}

/// bi_tcp_write_line 写一行数据（追加 \n）。
///
/// 用法：
///   tcpWriteLine(conn, data) — data 为 string/bytes/byteArray
fn bi_tcp_write_line(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let conn = conn_downcast(&args[0], "tcpWriteLine")?;
    let mut data = to_bytes(&args[1], "tcpWriteLine")?;
    data.push(b'\n');
    let mut guard = conn.stream.lock().unwrap();
    let written = guard.write(&data).map_err(|e| {
        error_value(format!(
            "tcpWriteLine() 写入失败: {} (可能原因：连接已关闭或对端拒绝)", e,
        ))
    })?;
    let _ = guard.flush();
    Ok(Value::Int(written as i64))
}

// ============ 连接管理 ============

/// bi_tcp_close 关闭连接。
///
/// 用法：
///   tcpClose(conn)        — 双向关闭（读写都关闭）
///   tcpClose(conn, "read")  — 仅关闭读端
///   tcpClose(conn, "write") — 仅关闭写端
fn bi_tcp_close(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let conn = conn_downcast(&args[0], "tcpClose")?;
    let how = if args.len() > 1 {
        let s = bh::as_str(args, 1, "tcpClose")?;
        match s {
            "read" => Shutdown::Read,
            "write" => Shutdown::Write,
            "both" => Shutdown::Both,
            other => return Err(error_value(format!(
                "tcpClose() 第二参数应为 'read'/'write'/'both'，得到 '{}' (可能原因：拼写错误)", other,
            ))),
        }
    } else {
        Shutdown::Both
    };
    let guard = conn.stream.lock().unwrap();
    guard.shutdown(how).map_err(|e| {
        error_value(format!(
            "tcpClose() 关闭失败: {} (可能原因：连接已关闭)", e,
        ))
    })?;
    Ok(Value::Undefined)
}

/// bi_tcp_set_timeout 设置读写超时。
///
/// 用法：
///   tcpSetTimeout(conn, readMs, writeMs)
///
/// readMs/writeMs 为 0 表示无超时。
fn bi_tcp_set_timeout(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let conn = conn_downcast(&args[0], "tcpSetTimeout")?;
    let read_ms = bh::as_int(args, 1, "tcpSetTimeout")?;
    let write_ms = bh::as_int(args, 2, "tcpSetTimeout")?;
    let guard = conn.stream.lock().unwrap();
    if read_ms > 0 {
        guard.set_read_timeout(Some(Duration::from_millis(read_ms as u64))).map_err(|e| {
            error_value(format!("tcpSetTimeout() 设置读超时失败: {}", e))
        })?;
    } else {
        guard.set_read_timeout(None).ok();
    }
    if write_ms > 0 {
        guard.set_write_timeout(Some(Duration::from_millis(write_ms as u64))).map_err(|e| {
            error_value(format!("tcpSetTimeout() 设置写超时失败: {}", e))
        })?;
    } else {
        guard.set_write_timeout(None).ok();
    }
    Ok(Value::Undefined)
}

/// bi_tcp_remote_addr 获取对端地址。
///
/// 用法：
///   tcpRemoteAddr(conn) — 返回 "ip:port" 字符串
fn bi_tcp_remote_addr(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let conn = conn_downcast(&args[0], "tcpRemoteAddr")?;
    let guard = conn.stream.lock().unwrap();
    match guard.peer_addr() {
        Ok(addr) => Ok(Value::str_from(addr.to_string())),
        Err(e) => Err(error_value(format!(
            "tcpRemoteAddr() 获取对端地址失败: {} (可能原因：连接已关闭)", e,
        ))),
    }
}

/// bi_tcp_local_addr 获取本端地址。
///
/// 用法：
///   tcpLocalAddr(conn) — 返回 "ip:port" 字符串
fn bi_tcp_local_addr(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let conn = conn_downcast(&args[0], "tcpLocalAddr")?;
    let guard = conn.stream.lock().unwrap();
    match guard.local_addr() {
        Ok(addr) => Ok(Value::str_from(addr.to_string())),
        Err(e) => Err(error_value(format!(
            "tcpLocalAddr() 获取本端地址失败: {} (可能原因：连接已关闭)", e,
        ))),
    }
}

// ============ 服务器管理 ============

/// bi_tcp_stop_server 停止 TCP 服务器。
///
/// 用法：
///   tcpStopServer(server) — 停止 accept 循环
///
/// 注意：已在处理的连接不受影响，会继续执行直到完成。
fn bi_tcp_stop_server(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let server = server_downcast(&args[0], "tcpStopServer")?;
    server.stop_flag.store(true, Ordering::Relaxed);
    Ok(Value::Undefined)
}

// ============ 双向转发 ============

/// pipe_connections 双向转发两个连接的数据，直到任一方关闭。
///
/// 内部启动两个线程分别转发两个方向的数据，阻塞至双方都结束。
/// 供 tcpPipe 内置函数和 builtins_proxy 模块复用。
///
/// 实现要点：用 try_clone 给每个方向的读写各克隆一份独立的 TcpStream，
/// 避免在阻塞 read 时持有 Mutex 锁导致另一方向无法写入（经典死锁）。
pub fn pipe_connections(conn1: &Arc<TcpConn>, conn2: &Arc<TcpConn>) {
    // 克隆出 4 个独立的流句柄，每个方向读写互不锁
    let (mut read1, mut write2) = {
        let s1 = conn1.stream.lock().unwrap();
        let s2 = conn2.stream.lock().unwrap();
        let r1 = s1.try_clone().expect("pipe_connections: try_clone failed");
        let w2 = s2.try_clone().expect("pipe_connections: try_clone failed");
        (r1, w2)
    };
    let (mut read2, mut write1) = {
        let s1 = conn1.stream.lock().unwrap();
        let s2 = conn2.stream.lock().unwrap();
        let r2 = s2.try_clone().expect("pipe_connections: try_clone failed");
        let w1 = s1.try_clone().expect("pipe_connections: try_clone failed");
        (r2, w1)
    };

    let (tx, rx) = std::sync::mpsc::channel();
    let tx2 = tx.clone();

    // 方向 1: conn1 -> conn2
    let h1 = std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match read1.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if write2.write_all(&buf[..n]).is_err() {
                        break;
                    }
                    let _ = write2.flush();
                }
            }
        }
        let _ = tx.send(());
    });

    // 方向 2: conn2 -> conn1
    let h2 = std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match read2.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if write1.write_all(&buf[..n]).is_err() {
                        break;
                    }
                    let _ = write1.flush();
                }
            }
        }
        let _ = tx2.send(());
    });

    // 等待任一方向结束，然后关闭两个连接以中断另一方向
    rx.recv().ok();
    let _ = conn1.stream.lock().unwrap().shutdown(Shutdown::Both);
    let _ = conn2.stream.lock().unwrap().shutdown(Shutdown::Both);
    h1.join().unwrap();
    h2.join().unwrap();
}

/// bi_tcp_pipe 双向转发两个连接的数据。
///
/// 用法：
///   tcpPipe(conn1, conn2) — 阻塞直到任一方关闭
///
/// 通常用于代理转发场景。调用后阻塞当前线程，
/// 数据在两个连接间双向流动，直到一方断开。
fn bi_tcp_pipe(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let conn1 = conn_downcast(&args[0], "tcpPipe")?;
    let conn2 = conn_downcast(&args[1], "tcpPipe")?;
    pipe_connections(conn1, conn2);
    Ok(Value::Undefined)
}

// ============ 测试 ============

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Sflang;
    use std::time::Duration;

    fn eval(src: &str) -> Value {
        let mut sf = Sflang::new();
        let wrapped = format!("func __f() {{ {} }} var __r = __f()", src);
        sf.run_string(&wrapped).expect("eval failed");
        sf.get_global("__r").expect("__r not set")
    }

    /// 辅助：启动一个回显服务器，返回端口
    fn start_echo_server(port: u16) -> u16 {
        let addr = format!("127.0.0.1:{}", port);
        let listener = TcpListener::bind(&addr).unwrap();
        let actual_port = listener.local_addr().unwrap().port();
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
        actual_port
    }

    #[test]
    fn test_tcp_connect_and_read_write() {
        let port = start_echo_server(0);
        std::thread::sleep(Duration::from_millis(50));

        let result = eval(&format!(r#"
            var conn = tcpConnect("127.0.0.1", {})
            tcpWrite(conn, "hello")
            var data = tcpRead(conn, 5)
            tcpClose(conn)
            return data
        "#, port));

        match result {
            Value::Bytes(b) => assert_eq!(b.as_ref(), b"hello"),
            other => panic!("期望 bytes，得到 {:?}", other),
        }
    }

    #[test]
    fn test_tcp_read_line() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if let Ok(mut s) = stream {
                    std::thread::spawn(move || {
                        let _ = s.write_all(b"line1\nline2\n");
                        let _ = s.flush();
                        std::thread::sleep(Duration::from_millis(100));
                    });
                    break;
                }
            }
        });
        std::thread::sleep(Duration::from_millis(50));

        let result = eval(&format!(r#"
            var conn = tcpConnect("127.0.0.1", {})
            var line1 = tcpReadLine(conn)
            var line2 = tcpReadLine(conn)
            tcpClose(conn)
            return [line1, line2]
        "#, port));

        match result {
            Value::Array(a) => {
                let guard = a.lock().unwrap();
                assert_eq!(guard[0].to_str(), "line1");
                assert_eq!(guard[1].to_str(), "line2");
            }
            other => panic!("期望 array，得到 {:?}", other),
        }
    }

    #[test]
    fn test_tcp_remote_local_addr() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if let Ok(s) = stream {
                    let _ = s;
                    std::thread::sleep(Duration::from_millis(200));
                    break;
                }
            }
        });
        std::thread::sleep(Duration::from_millis(50));

        let result = eval(&format!(r#"
            var conn = tcpConnect("127.0.0.1", {})
            var remote = tcpRemoteAddr(conn)
            var local = tcpLocalAddr(conn)
            tcpClose(conn)
            return [remote, local]
        "#, port));

        match result {
            Value::Array(a) => {
                let guard = a.lock().unwrap();
                let remote = guard[0].to_str();
                let local = guard[1].to_str();
                assert!(remote.contains("127.0.0.1"), "remote 应含 127.0.0.1，得到 {}", remote);
                assert!(local.contains("127.0.0.1"), "local 应含 127.0.0.1，得到 {}", local);
            }
            other => panic!("期望 array，得到 {:?}", other),
        }
    }

    #[test]
    fn test_tcp_listen_with_handler() {
        let result = eval(r#"
            var server = tcpListen("127.0.0.1:0", func(conn) {
                var line = tcpReadLine(conn)
                tcpWriteLine(conn, "echo: " + line)
                tcpClose(conn)
            })
            return server
        "#);

        // 验证返回的是 server 对象
        assert!(matches!(result, Value::Native(_)));
    }

    #[test]
    fn test_tcp_connect_refused() {
        // 绑定一个端口然后立即释放，确保连接被拒绝
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let mut sf = Sflang::new();
        let src = format!(r#"
            var conn = tcpConnect("127.0.0.1", {}, 1000)
        "#, port);
        let result = sf.run_string(&src);

        // 应该返回错误
        match result {
            Err(Value::Error(e)) => assert!(e.message.contains("连接") || e.message.contains("connect"), "错误信息应提到连接失败"),
            Err(other) => panic!("期望 error，得到 {:?}", other),
            Ok(_) => panic!("期望连接失败但成功了"),
        }
    }

    #[test]
    fn test_tcp_write_line_and_read_line_roundtrip() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if let Ok(mut s) = stream {
                    std::thread::spawn(move || {
                        use std::io::{BufRead, BufReader, Write};
                        let mut reader = BufReader::new(s.try_clone().unwrap());
                        let mut line = String::new();
                        loop {
                            line.clear();
                            if reader.read_line(&mut line).unwrap_or(0) == 0 { break; }
                            let resp = format!("resp:{}", line);
                            let _ = s.write_all(resp.as_bytes());
                            let _ = s.flush();
                        }
                    });
                    break;
                }
            }
        });
        std::thread::sleep(Duration::from_millis(50));

        let result = eval(&format!(r#"
            var conn = tcpConnect("127.0.0.1", {})
            tcpWriteLine(conn, "ping")
            var resp = tcpReadLine(conn)
            tcpClose(conn)
            return resp
        "#, port));

        assert_eq!(result.to_str(), "resp:ping");
    }
}
