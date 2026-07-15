//! builtins_xxci.rs — 加密代理协议（对标 github.com/topxeq/goconnectit，简称 xxci）
//!
//! 设计要点：
//!   - 纯标准库实现，DES 加密用自实现的 des.rs
//!   - 支持 4 种加密方法：des, txdef, txdee, txde
//!   - 服务器模式：监听端口，接受加密连接，解密后处理 SOCKS5 代理
//!   - 客户端模式：本地 SOCKS5 代理，通过加密连接转发到服务器
//!   - 协议与 Go 版 goconnectit 完全兼容（相同密码和加密方法可互通）
//!
//! 函数列表：
//!   xxciServer(addr, password, encryptMethod) — 启动服务器
//!   xxciServerStop(server)                    — 停止服务器
//!   xxciClient(serverAddr, localAddr, password, encryptMethod) — 启动客户端
//!   xxciClientStop(client)                    — 停止客户端

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::builtins_helpers as bh;
use crate::des::CtrStream;
use crate::function::BuiltinDoc;
use crate::value::{error_value, Value};
use crate::vm::VM;

static DOC_XXCI_SERVER: BuiltinDoc = BuiltinDoc {
    category: "xxci",
    signature: "xxciServer(addr, password, encryptMethod) -> server",
    summary: "启动加密代理服务器，接收加密 SOCKS5 连接（与 Go 版 goconnectit 协议兼容）。",
    params: &[
        ("addr", "监听地址，如 \"0.0.0.0:8888\""),
        ("password", "加密密码，空串时使用默认 \"topxeq\""),
        ("encryptMethod", "加密方法：\"des\" / \"txdef\" / \"txdee\" / \"txde\""),
    ],
    returns: "XxciServer 对象，传给 xxciServerStop 停止",
    examples: &["s := xxciServer(\"0.0.0.0:8888\", \"mypassword\", \"txde\")  // 启动 txde 加密的服务器"],
    errors: &[
        "xxciServer() 绑定 'xxx' 失败（可能原因：地址被占用或权限不足）",
        "不支持的加密方法（可能原因：拼写错误，支持 des/txdef/txdee/txde）",
    ],
};

static DOC_XXCI_SERVER_STOP: BuiltinDoc = BuiltinDoc {
    category: "xxci",
    signature: "xxciServerStop(server) -> undefined",
    summary: "停止 xxciServer 启动的服务器。",
    params: &[("server", "xxciServer 返回的对象")],
    returns: "undefined",
    examples: &["xxciServerStop(s)  // 停止服务器"],
    errors: &[
        "xxciServerStop() 参数不是服务器对象（可能原因：传入了错误类型，应使用 xxciServer 的返回值）",
        "xxciServerStop() 参数应为服务器，得到 X（可能原因：参数类型不匹配）",
    ],
};

static DOC_XXCI_CLIENT: BuiltinDoc = BuiltinDoc {
    category: "xxci",
    signature: "xxciClient(serverAddr, localAddr, password, encryptMethod) -> client",
    summary: "启动加密代理客户端，本地暴露 SOCKS5 代理，经加密通道转发到 xxciServer。",
    params: &[
        ("serverAddr", "远程 xxciServer 地址，如 \"your-host:8888\""),
        ("localAddr", "本地 SOCKS5 监听地址，如 \"127.0.0.1:1080\""),
        ("password", "加密密码，需与服务器一致"),
        ("encryptMethod", "加密方法，需与服务器一致：des/txdef/txdee/txde"),
    ],
    returns: "XxciClient 对象，传给 xxciClientStop 停止",
    examples: &["c := xxciClient(\"my-server:8888\", \"127.0.0.1:1080\", \"mypassword\", \"txde\")  // 本地 1080 提供加密代理"],
    errors: &[
        "xxciClient() 绑定 'xxx' 失败（可能原因：地址被占用或权限不足）",
        "[xxciClient] 连接服务器 xxx 失败（可能原因：服务器未启动或网络不通）",
    ],
};

static DOC_XXCI_CLIENT_STOP: BuiltinDoc = BuiltinDoc {
    category: "xxci",
    signature: "xxciClientStop(client) -> undefined",
    summary: "停止 xxciClient 启动的客户端。",
    params: &[("client", "xxciClient 返回的对象")],
    returns: "undefined",
    examples: &["xxciClientStop(c)  // 停止客户端"],
    errors: &[
        "xxciClientStop() 参数不是客户端对象（可能原因：传入了错误类型，应使用 xxciClient 的返回值）",
        "xxciClientStop() 参数应为客户端，得到 X（可能原因：参数类型不匹配）",
    ],
};

/// register 注册所有 goconnectit 内置函数。
pub fn register(vm: &mut VM) {
    vm.register_builtin_doc("xxciServer", bi_xxci_server, &DOC_XXCI_SERVER);
    vm.register_builtin_doc("xxciServerStop", bi_xxci_server_stop, &DOC_XXCI_SERVER_STOP);
    vm.register_builtin_doc("xxciClient", bi_xxci_client, &DOC_XXCI_CLIENT);
    vm.register_builtin_doc("xxciClientStop", bi_xxci_client_stop, &DOC_XXCI_CLIENT_STOP);
}

// ============ 类型定义 ============

/// XxciServer 服务器对象。
pub struct XxciServer {
    /// stop_flag 停止标志。
    pub stop_flag: Arc<AtomicBool>,
}

/// XxciClient 客户端对象。
pub struct XxciClient {
    /// stop_flag 停止标志。
    pub stop_flag: Arc<AtomicBool>,
}

// ============ 辅助函数 ============

/// sum_bytes 计算字节切片的 u8 和。
fn sum_bytes(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |acc, &b| acc.wrapping_add(b))
}

/// random_byte 生成一个伪随机字节。
///
/// 使用 static 计数器确保连续调用返回不同值。
fn random_byte() -> u8 {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    ((nanos.wrapping_mul(2654435761)).wrapping_add(c.wrapping_mul(0x9E3779B9))) as u8
}

// ============ 加密连接 ============

/// EncryptedReader 加密读取器（读方向）。
///
/// 每种加密方法有独立的读取逻辑。
/// 通过 try_clone 持有独立的 TcpStream，支持与写方向并发。
enum EncryptedReader {
    /// DES-CTR 流式读取
    Des {
        stream: CtrStream,
        conn: TcpStream,
    },
    /// TXDEF 分块读取
    Txdef {
        conn: TcpStream,
        code: Vec<u8>,
        add_len: usize,
        enc_index: usize,
        read_buf: Vec<u8>,
        read_idx: usize,
    },
    /// TXDEE 分块读取
    Txdee {
        conn: TcpStream,
        code: Vec<u8>,
        read_buf: Vec<u8>,
        read_idx: usize,
    },
    /// TXDE 流式读取
    Txde {
        conn: TcpStream,
        code: Vec<u8>,
        idx: usize,
    },
}

/// EncryptedWriter 加密写入器（写方向）。
enum EncryptedWriter {
    /// DES-CTR 流式写入
    Des {
        stream: CtrStream,
        conn: TcpStream,
    },
    /// TXDEF 分块写入
    Txdef {
        conn: TcpStream,
        code: Vec<u8>,
        add_len: usize,
        enc_index: usize,
    },
    /// TXDEE 分块写入
    Txdee {
        conn: TcpStream,
        code: Vec<u8>,
    },
    /// TXDE 流式写入
    Txde {
        conn: TcpStream,
        code: Vec<u8>,
        idx: usize,
    },
}

impl Read for EncryptedReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            EncryptedReader::Des { stream, conn } => {
                // 直接从连接读取，然后解密
                let n = conn.read(buf)?;
                if n == 0 {
                    return Ok(0);
                }
                // CTR 模式：原地解密（dst = src XOR keystream）
                let tmp = buf[..n].to_vec();
                stream.xor_key_stream(&mut buf[..n], &tmp);
                Ok(n)
            }
            EncryptedReader::Txdef { conn, code, add_len, enc_index, read_buf, read_idx } => {
                // 如果缓冲区有剩余数据，先返回
                if *read_idx < read_buf.len() {
                    let n = std::cmp::min(buf.len(), read_buf.len() - *read_idx);
                    buf[..n].copy_from_slice(&read_buf[*read_idx..*read_idx + n]);
                    *read_idx += n;
                    return Ok(n);
                }
                // 读取 2 字节长度
                let mut len_buf = [0u8; 2];
                conn.read_exact(&mut len_buf)?;
                let data_len = ((len_buf[0] as usize) << 8) | (len_buf[1] as usize);
                if data_len < *add_len {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("TXDEF 数据长度 {} 小于 add_len {}", data_len, add_len),
                    ));
                }
                let mut full_buf = vec![0u8; data_len];
                conn.read_exact(&mut full_buf)?;
                let key_byte = full_buf[*enc_index];
                let actual_len = data_len - *add_len;
                let mut decrypted = vec![0u8; actual_len];
                for i in 0..actual_len {
                    decrypted[i] = full_buf[*add_len + i]
                        .wrapping_sub(code[i % code.len()])
                        .wrapping_sub((i + 1) as u8)
                        .wrapping_sub(key_byte);
                }
                // 缓存解密数据
                let n = std::cmp::min(buf.len(), actual_len);
                buf[..n].copy_from_slice(&decrypted[..n]);
                *read_buf = decrypted;
                *read_idx = n;
                Ok(n)
            }
            EncryptedReader::Txdee { conn, code, read_buf, read_idx } => {
                if *read_idx < read_buf.len() {
                    let n = std::cmp::min(buf.len(), read_buf.len() - *read_idx);
                    buf[..n].copy_from_slice(&read_buf[*read_idx..*read_idx + n]);
                    *read_idx += n;
                    return Ok(n);
                }
                let mut len_buf = [0u8; 2];
                conn.read_exact(&mut len_buf)?;
                let data_len = ((len_buf[0] as usize) << 8) | (len_buf[1] as usize);
                if data_len < 4 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("TXDEE 数据长度 {} 小于 4", data_len),
                    ));
                }
                let mut full_buf = vec![0u8; data_len];
                conn.read_exact(&mut full_buf)?;
                let key_byte = full_buf[1];
                let actual_len = data_len - 4;
                let mut decrypted = vec![0u8; actual_len];
                for i in 0..actual_len {
                    decrypted[i] = full_buf[2 + i]
                        .wrapping_sub(code[i % code.len()])
                        .wrapping_sub((i + 1) as u8)
                        .wrapping_sub(key_byte);
                }
                let n = std::cmp::min(buf.len(), actual_len);
                buf[..n].copy_from_slice(&decrypted[..n]);
                *read_buf = decrypted;
                *read_idx = n;
                Ok(n)
            }
            EncryptedReader::Txde { conn, code, idx } => {
                let n = conn.read(buf)?;
                if n == 0 {
                    return Ok(0);
                }
                for i in 0..n {
                    buf[i] = buf[i]
                        .wrapping_sub(code[*idx % code.len()])
                        .wrapping_sub((*idx + 1) as u8);
                    *idx += 1;
                }
                Ok(n)
            }
        }
    }
}

impl Write for EncryptedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            EncryptedWriter::Des { stream, conn } => {
                // CTR 模式：加密后写入
                let mut encrypted = vec![0u8; buf.len()];
                stream.xor_key_stream(&mut encrypted, buf);
                conn.write_all(&encrypted)?;
                Ok(buf.len())
            }
            EncryptedWriter::Txdef { conn, code, add_len, enc_index } => {
                let data_len = buf.len();
                let mut random_bytes = vec![0u8; *add_len];
                for i in 0..*add_len {
                    random_bytes[i] = random_byte();
                }
                let key_byte = random_bytes[*enc_index];
                let total_len = *add_len + data_len;
                let mut out = Vec::with_capacity(2 + total_len);
                out.push((total_len >> 8) as u8);
                out.push(total_len as u8);
                out.extend_from_slice(&random_bytes);
                for i in 0..data_len {
                    out.push(
                        buf[i]
                            .wrapping_add(code[i % code.len()])
                            .wrapping_add((i + 1) as u8)
                            .wrapping_add(key_byte),
                    );
                }
                conn.write_all(&out)?;
                Ok(data_len)
            }
            EncryptedWriter::Txdee { conn, code } => {
                let data_len = buf.len();
                let key_byte1 = random_byte();
                let key_byte2 = random_byte();
                let trail1 = random_byte();
                let trail2 = random_byte();
                let total_len = 2 + data_len + 2;
                let mut out = Vec::with_capacity(2 + total_len);
                out.push((total_len >> 8) as u8);
                out.push(total_len as u8);
                out.push(key_byte1);
                out.push(key_byte2);
                for i in 0..data_len {
                    out.push(
                        buf[i]
                            .wrapping_add(code[i % code.len()])
                            .wrapping_add((i + 1) as u8)
                            .wrapping_add(key_byte2),
                    );
                }
                out.push(trail1);
                out.push(trail2);
                conn.write_all(&out)?;
                Ok(data_len)
            }
            EncryptedWriter::Txde { conn, code, idx } => {
                let mut encrypted = vec![0u8; buf.len()];
                for i in 0..buf.len() {
                    encrypted[i] = buf[i]
                        .wrapping_add(code[*idx % code.len()])
                        .wrapping_add((*idx + 1) as u8);
                    *idx += 1;
                }
                conn.write_all(&encrypted)?;
                Ok(buf.len())
            }
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            EncryptedWriter::Des { conn, .. } => conn.flush(),
            EncryptedWriter::Txdef { conn, .. } => conn.flush(),
            EncryptedWriter::Txdee { conn, .. } => conn.flush(),
            EncryptedWriter::Txde { conn, .. } => conn.flush(),
        }
    }
}

/// create_encrypted_conn 创建加密连接。
///
/// 根据 encrypt_method 创建对应的加密读写器。
/// 对于 DES，会先交换 IV（先写后读）。
fn create_encrypted_conn(
    mut conn: TcpStream,
    password: &str,
    encrypt_method: &str,
) -> Result<(EncryptedReader, EncryptedWriter), String> {
    let code = if password.is_empty() {
        b"topxeq".to_vec()
    } else {
        password.as_bytes().to_vec()
    };

    match encrypt_method {
        "des" => {
            // DES 密钥：密码前 8 字节，不足补 0
            let mut des_key = [0u8; 8];
            let pw_bytes = password.as_bytes();
            let copy_len = std::cmp::min(pw_bytes.len(), 8);
            des_key[..copy_len].copy_from_slice(&pw_bytes[..copy_len]);

            // 生成写 IV
            let iv_write: [u8; 8] = [
                random_byte(),
                random_byte(),
                random_byte(),
                random_byte(),
                random_byte(),
                random_byte(),
                random_byte(),
                random_byte(),
            ];

            // 写入 IV
            conn.write_all(&iv_write).map_err(|e| format!("写入 DES IV 失败: {}", e))?;

            // 读取对端 IV
            let mut iv_read = [0u8; 8];
            conn.read_exact(&mut iv_read).map_err(|e| format!("读取 DES IV 失败: {}", e))?;

            // try_clone 给读写各一份
            let read_conn = conn.try_clone().map_err(|e| format!("try_clone 失败: {}", e))?;
            let write_conn = conn.try_clone().map_err(|e| format!("try_clone 失败: {}", e))?;

            let reader = EncryptedReader::Des {
                stream: CtrStream::new(&des_key, &iv_read),
                conn: read_conn,
            };
            let writer = EncryptedWriter::Des {
                stream: CtrStream::new(&des_key, &iv_write),
                conn: write_conn,
            };
            Ok((reader, writer))
        }
        "txdef" => {
            let sum = sum_bytes(&code) as usize;
            let add_len = (sum % 5) + 2;
            let enc_index = sum % add_len;
            let read_conn = conn.try_clone().map_err(|e| format!("try_clone 失败: {}", e))?;
            let write_conn = conn.try_clone().map_err(|e| format!("try_clone 失败: {}", e))?;
            Ok((
                EncryptedReader::Txdef {
                    conn: read_conn,
                    code: code.clone(),
                    add_len,
                    enc_index,
                    read_buf: Vec::new(),
                    read_idx: 0,
                },
                EncryptedWriter::Txdef {
                    conn: write_conn,
                    code,
                    add_len,
                    enc_index,
                },
            ))
        }
        "txdee" => {
            let read_conn = conn.try_clone().map_err(|e| format!("try_clone 失败: {}", e))?;
            let write_conn = conn.try_clone().map_err(|e| format!("try_clone 失败: {}", e))?;
            Ok((
                EncryptedReader::Txdee {
                    conn: read_conn,
                    code: code.clone(),
                    read_buf: Vec::new(),
                    read_idx: 0,
                },
                EncryptedWriter::Txdee {
                    conn: write_conn,
                    code,
                },
            ))
        }
        "txde" => {
            let read_conn = conn.try_clone().map_err(|e| format!("try_clone 失败: {}", e))?;
            let write_conn = conn.try_clone().map_err(|e| format!("try_clone 失败: {}", e))?;
            Ok((
                EncryptedReader::Txde {
                    conn: read_conn,
                    code: code.clone(),
                    idx: 0,
                },
                EncryptedWriter::Txde {
                    conn: write_conn,
                    code,
                    idx: 0,
                },
            ))
        }
        other => Err(format!(
            "不支持的加密方法: {} (可能原因：拼写错误，支持 des/txdef/txdee/txde)",
            other,
        )),
    }
}

// ============ 服务器模式 ============

/// handle_server_connection 处理服务器端单个连接。
///
/// 1. 创建加密流
/// 2. 读取加密的 SOCKS5 请求
/// 3. 解析 SOCKS5 协议，回复方法选择
/// 4. 读取加密的 SOCKS5 CONNECT 请求
/// 5. 连接目标地址
/// 6. 回复连接成功
/// 7. 双向转发
fn handle_server_connection(stream: TcpStream, password: &str, encrypt_method: &str, out: Arc<std::sync::Mutex<dyn std::io::Write + Send>>) {
    let client_addr = stream.peer_addr().map(|a| a.to_string()).unwrap_or_default();

    // 克隆一份用于后续 shutdown 中断
    let encrypted_shutdown = stream.try_clone().expect("try_clone for shutdown failed");

    let (mut reader, mut writer) = match create_encrypted_conn(stream, password, encrypt_method) {
        Ok(rw) => rw,
        Err(e) => {
            let _ = writeln!(out.lock().unwrap(), "[xxciServer] {} 创建加密流失败: {}", client_addr, e);
            return;
        }
    };

    let mut buf = [0u8; 512];

    // 1. 读取 SOCKS5 初始请求
    let n = match reader.read(&mut buf) {
        Ok(n) if n >= 3 => n,
        Ok(_) => {
            let _ = writeln!(out.lock().unwrap(), "[xxciServer] {} SOCKS5 请求过短", client_addr);
            return;
        }
        Err(e) => {
            let _ = writeln!(out.lock().unwrap(), "[xxciServer] {} 读取失败: {}", client_addr, e);
            return;
        }
    };

    if buf[0] != 0x05 {
        let _ = writeln!(out.lock().unwrap(), "[xxciServer] {} 不支持的 SOCKS 版本: {}", client_addr, buf[0]);
        return;
    }

    // 校验方法数与读取长度匹配
    let n_methods = buf[1] as usize;
    if n < 2 + n_methods {
        let _ = writeln!(out.lock().unwrap(), "[xxciServer] {} SOCKS5 方法数不匹配", client_addr);
        return;
    }

    // 回复方法选择（无认证）
    if writer.write_all(&[0x05, 0x00]).is_err() {
        return;
    }
    let _ = writer.flush();

    // 2. 读取 SOCKS5 CONNECT 请求
    let n = match reader.read(&mut buf) {
        Ok(n) if n >= 4 => n,
        Ok(_) => {
            let _ = writeln!(out.lock().unwrap(), "[xxciServer] {} CONNECT 请求过短", client_addr);
            return;
        }
        Err(e) => {
            let _ = writeln!(out.lock().unwrap(), "[xxciServer] {} 读取 CONNECT 失败: {}", client_addr, e);
            return;
        }
    };

    if buf[0] != 0x05 || buf[1] != 0x01 {
        let _ = writeln!(out.lock().unwrap(), "[xxciServer] {} 不支持的命令: {}", client_addr, buf[1]);
        return;
    }

    // 解析目标地址
    let target_addr = match buf[3] {
        0x01 => {
            // IPv4
            if n < 10 {
                let _ = writeln!(out.lock().unwrap(), "[xxciServer] {} IPv4 地址不完整", client_addr);
                return;
            }
            format!("{}.{}.{}.{}:{}", buf[4], buf[5], buf[6], buf[7],
                ((buf[8] as u16) << 8) | (buf[9] as u16))
        }
        0x03 => {
            // 域名
            if n < 7 {
                let _ = writeln!(out.lock().unwrap(), "[xxciServer] {} 域名不完整", client_addr);
                return;
            }
            let domain_len = buf[4] as usize;
            if n < 5 + domain_len + 2 {
                let _ = writeln!(out.lock().unwrap(), "[xxciServer] {} 域名长度不匹配", client_addr);
                return;
            }
            let domain = String::from_utf8_lossy(&buf[5..5 + domain_len]);
            let port = ((buf[5 + domain_len] as u16) << 8) | (buf[5 + domain_len + 1] as u16);
            format!("{}:{}", domain, port)
        }
        0x04 => {
            // IPv6
            if n < 22 {
                let _ = writeln!(out.lock().unwrap(), "[xxciServer] {} IPv6 地址不完整", client_addr);
                return;
            }
            let mut parts = Vec::new();
            for i in (4..20).step_by(2) {
                parts.push(format!("{:x}", ((buf[i] as u16) << 8) | (buf[i + 1] as u16)));
            }
            let port = ((buf[20] as u16) << 8) | (buf[21] as u16);
            format!("[{}]:{}", parts.join(":"), port)
        }
        other => {
            let _ = writeln!(out.lock().unwrap(), "[xxciServer] {} 不支持的地址类型: {}", client_addr, other);
            return;
        }
    };

    // 3. 连接目标
    let remote_conn = match TcpStream::connect(&target_addr) {
        Ok(c) => c,
        Err(e) => {
            let _ = writeln!(out.lock().unwrap(), "[xxciServer] {} -> {} 连接失败: {}", client_addr, target_addr, e);
            let _ = writer.write_all(&[0x05, 0x05, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
            return;
        }
    };

    // 4. 回复连接成功
    if writer.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).is_err() {
        return;
    }
    let _ = writer.flush();

    // 5. 双向转发：加密连接 <-> 目标连接
    pipe_encrypted_plain(reader, writer, remote_conn, encrypted_shutdown);
}

/// pipe_encrypted_plain 双向转发加密连接与明文连接。
///
/// 线程1：从加密 reader 读（解密），写到明文连接
/// 线程2：从明文连接读，写到加密 writer（加密）
///
/// 任一方向结束时，关闭两个连接以中断另一方向，避免死锁。
fn pipe_encrypted_plain(
    mut reader: EncryptedReader,
    mut writer: EncryptedWriter,
    plain_conn: TcpStream,
    encrypted_shutdown: TcpStream,
) {
    let mut plain_read = plain_conn.try_clone().unwrap();
    let mut plain_write = plain_conn.try_clone().unwrap();

    let (tx, rx) = std::sync::mpsc::channel();
    let tx2 = tx.clone();

    // 方向1: 加密 reader -> 明文
    let h1 = std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if plain_write.write_all(&buf[..n]).is_err() {
                        break;
                    }
                    let _ = plain_write.flush();
                }
            }
        }
        let _ = tx.send(());
    });

    // 方向2: 明文 -> 加密 writer
    let h2 = std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match plain_read.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if writer.write_all(&buf[..n]).is_err() {
                        break;
                    }
                    let _ = writer.flush();
                }
            }
        }
        let _ = tx2.send(());
    });

    // 等待任一方向结束，然后关闭两个连接以中断另一方向
    rx.recv().ok();
    let _ = plain_conn.shutdown(Shutdown::Both);
    let _ = encrypted_shutdown.shutdown(Shutdown::Both);
    h1.join().ok();
    h2.join().ok();
}

// ============ 客户端模式 ============

/// handle_client_connection 处理客户端单个连接。
///
/// 1. 连接远程服务器
/// 2. 创建加密流
/// 3. 转发 SOCKS5 握手（2次请求-回复）
/// 4. 双向转发
fn handle_client_connection(
    mut local_conn: TcpStream,
    server_addr: &str,
    password: &str,
    encrypt_method: &str,
    out: Arc<std::sync::Mutex<dyn std::io::Write + Send>>,
) {
    // 连接服务器
    let server_conn = match TcpStream::connect(server_addr) {
        Ok(c) => c,
        Err(e) => {
            let _ = writeln!(out.lock().unwrap(), "[xxciClient] 连接服务器 {} 失败: {}", server_addr, e);
            return;
        }
    };

    // 克隆一份用于后续 shutdown 中断
    let encrypted_shutdown = server_conn.try_clone().expect("try_clone for shutdown failed");

    let (mut reader, mut writer) = match create_encrypted_conn(server_conn, password, encrypt_method) {
        Ok(rw) => rw,
        Err(e) => {
            let _ = writeln!(out.lock().unwrap(), "[xxciClient] 创建加密流失败: {}", e);
            return;
        }
    };

    let mut buf = [0u8; 512];

    // 1. 读取本地 SOCKS5 初始请求
    let n = match local_conn.read(&mut buf) {
        Ok(n) if n > 0 => n,
        _ => return,
    };

    // 2. 加密转发到服务器
    if writer.write_all(&buf[..n]).is_err() {
        return;
    }
    let _ = writer.flush();

    // 3. 读取服务器加密回复
    let n = match reader.read(&mut buf) {
        Ok(n) if n > 0 => n,
        _ => return,
    };

    // 4. 解密返回本地
    if local_conn.write_all(&buf[..n]).is_err() {
        return;
    }
    let _ = local_conn.flush();

    // 5. 读取本地 SOCKS5 CONNECT 请求
    let n = match local_conn.read(&mut buf) {
        Ok(n) if n > 0 => n,
        _ => return,
    };

    // 6. 加密转发到服务器
    if writer.write_all(&buf[..n]).is_err() {
        return;
    }
    let _ = writer.flush();

    // 7. 读取服务器加密回复
    let n = match reader.read(&mut buf) {
        Ok(n) if n > 0 => n,
        _ => return,
    };

    // 8. 解密返回本地
    if local_conn.write_all(&buf[..n]).is_err() {
        return;
    }
    let _ = local_conn.flush();

    // 9. 双向转发
    pipe_encrypted_plain(reader, writer, local_conn, encrypted_shutdown);
}

// ============ 内置函数 ============

/// bi_xxci_server 启动 goconnectit 服务器。
///
/// 用法：
///   xxciServer(addr, password, encryptMethod)
///
/// addr: "0.0.0.0:8888" 监听地址
/// password: 加密密码
/// encryptMethod: "des" / "txdef" / "txdee" / "txde"
fn bi_xxci_server(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let addr = bh::as_str(args, 0, "xxciServer")?.to_string();
    let password = bh::as_str(args, 1, "xxciServer")?.to_string();
    let encrypt_method = bh::as_str(args, 2, "xxciServer")?.to_string();

    let listener = TcpListener::bind(&addr).map_err(|e| {
        error_value(format!(
            "xxciServer() 绑定 '{}' 失败: {} (可能原因：地址被占用或权限不足)",
            addr, e,
        ))
    })?;

    listener.set_nonblocking(true).map_err(|e| {
        error_value(format!("xxciServer() 设置非阻塞模式失败: {}", e))
    })?;

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();
    let out = vm.output_handle();
    let pw = password.clone();
    let em = encrypt_method.clone();
    let listen_addr = addr.clone();

    std::thread::spawn(move || {
        let _ = writeln!(out.lock().unwrap(), "[xxciServer] 服务器启动: {} (加密: {})", listen_addr, em);
        while !stop_clone.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, client_addr)) => {
                    let _ = stream.set_nonblocking(false);
                    let out_clone = out.clone();
                    let pw_clone = pw.clone();
                    let em_clone = em.clone();
                    std::thread::spawn(move || {
                        handle_server_connection(stream, &pw_clone, &em_clone, out_clone);
                    });
                    let _ = client_addr; // 避免未使用警告
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(e) => {
                    let _ = writeln!(out.lock().unwrap(), "[xxciServer] accept 错误: {}", e);
                    break;
                }
            }
        }
    });

    Ok(Value::Native(Arc::new(Arc::new(XxciServer { stop_flag }))))
}

/// bi_xxci_server_stop 停止服务器。
fn bi_xxci_server_stop(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    match &args[0] {
        Value::Native(n) => {
            if let Some(s) = n.downcast_ref::<Arc<XxciServer>>() {
                s.stop_flag.store(true, Ordering::Relaxed);
                Ok(Value::Undefined)
            } else {
                Err(error_value(
                    "xxciServerStop() 参数不是服务器对象 (可能原因：传入了错误类型，应使用 xxciServer 的返回值)",
                ))
            }
        }
        other => Err(error_value(format!(
            "xxciServerStop() 参数应为服务器，得到 {} (可能原因：参数类型不匹配)",
            other.type_name(),
        ))),
    }
}

/// bi_xxci_client 启动 goconnectit 客户端。
///
/// 用法：
///   xxciClient(serverAddr, localAddr, password, encryptMethod)
///
/// serverAddr: "your-server:8888" 服务器地址
/// localAddr: "127.0.0.1:1080" 本地 SOCKS5 代理地址
/// password: 加密密码
/// encryptMethod: "des" / "txdef" / "txdee" / "txde"
fn bi_xxci_client(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let server_addr = bh::as_str(args, 0, "xxciClient")?.to_string();
    let local_addr = bh::as_str(args, 1, "xxciClient")?.to_string();
    let password = bh::as_str(args, 2, "xxciClient")?.to_string();
    let encrypt_method = bh::as_str(args, 3, "xxciClient")?.to_string();

    let listener = TcpListener::bind(&local_addr).map_err(|e| {
        error_value(format!(
            "xxciClient() 绑定 '{}' 失败: {} (可能原因：地址被占用或权限不足)",
            local_addr, e,
        ))
    })?;

    listener.set_nonblocking(true).map_err(|e| {
        error_value(format!("xxciClient() 设置非阻塞模式失败: {}", e))
    })?;

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();
    let out = vm.output_handle();
    let sa = server_addr.clone();
    let pw = password.clone();
    let em = encrypt_method.clone();
    let la = local_addr.clone();

    std::thread::spawn(move || {
        let _ = writeln!(out.lock().unwrap(), "[xxciClient] 客户端启动: {} -> {} (加密: {})", la, sa, em);
        while !stop_clone.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    let out_clone = out.clone();
                    let sa_clone = sa.clone();
                    let pw_clone = pw.clone();
                    let em_clone = em.clone();
                    std::thread::spawn(move || {
                        handle_client_connection(stream, &sa_clone, &pw_clone, &em_clone, out_clone);
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(e) => {
                    let _ = writeln!(out.lock().unwrap(), "[xxciClient] accept 错误: {}", e);
                    break;
                }
            }
        }
    });

    Ok(Value::Native(Arc::new(Arc::new(XxciClient { stop_flag }))))
}

/// bi_xxci_client_stop 停止客户端。
fn bi_xxci_client_stop(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    match &args[0] {
        Value::Native(n) => {
            if let Some(c) = n.downcast_ref::<Arc<XxciClient>>() {
                c.stop_flag.store(true, Ordering::Relaxed);
                Ok(Value::Undefined)
            } else {
                Err(error_value(
                    "xxciClientStop() 参数不是客户端对象 (可能原因：传入了错误类型，应使用 xxciClient 的返回值)",
                ))
            }
        }
        other => Err(error_value(format!(
            "xxciClientStop() 参数应为客户端，得到 {} (可能原因：参数类型不匹配)",
            other.type_name(),
        ))),
    }
}

// ============ 测试 ============

#[cfg(test)]
mod tests {
    use super::*;
    use crate::des::DesBlock;
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
    fn test_des_block_nist_vector() {
        let key = [0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF];
        let pt = [0x4E, 0x6F, 0x77, 0x20, 0x69, 0x73, 0x20, 0x74];
        let expected_ct = [0x3F, 0xA4, 0x0E, 0x8A, 0x98, 0x4D, 0x48, 0x15];

        let block = DesBlock::new(&key);
        let ct = block.encrypt_block(&pt);
        assert_eq!(ct, expected_ct);
    }

    #[test]
    fn test_txde_stream_encrypt_decrypt() {
        let code = "testpass";
        let data = b"Hello, goconnectit!";
        let code_bytes = code.as_bytes();

        // 加密
        let mut encrypted = vec![0u8; data.len()];
        for i in 0..data.len() {
            encrypted[i] = data[i]
                .wrapping_add(code_bytes[i % code_bytes.len()])
                .wrapping_add((i + 1) as u8);
        }

        // 解密
        let mut decrypted = vec![0u8; data.len()];
        for i in 0..data.len() {
            decrypted[i] = encrypted[i]
                .wrapping_sub(code_bytes[i % code_bytes.len()])
                .wrapping_sub((i + 1) as u8);
        }

        assert_eq!(&decrypted[..], data);
    }

    #[test]
    fn test_xxci_server_client_loopback() {
        let echo_port = start_echo_server();
        std::thread::sleep(Duration::from_millis(50));

        let mut sf = Sflang::new();
        // 启动 goconnectit 服务器（用 txde 加密，最简单）
        let src = format!(r#"
            server := xxciServer("127.0.0.1:18999", "testpass", "txde")
            sleepMs(100)
            client := xxciClient("127.0.0.1:18999", "127.0.0.1:19000", "testpass", "txde")
            sleepMs(200)
            // 通过客户端的本地 SOCKS5 代理连接回显服务器
            conn := tcpConnect("127.0.0.1", 19000, 3000)
            // SOCKS5 握手
            tcpWrite(conn, bytes([0x05, 0x01, 0x00]))
            method := tcpRead(conn, 2)
            // SOCKS5 CONNECT 127.0.0.1:{echo_port}
            portHi := {echo_port} / 256
            portLo := {echo_port} % 256
            tcpWrite(conn, bytes([0x05, 0x01, 0x00, 0x01, 0x7f, 0x00, 0x00, 0x01, portHi, portLo]))
            reply := tcpRead(conn, 10)
            // 发送数据
            tcpWriteLine(conn, "hello goconnectit")
            resp := tcpReadLine(conn)
            tcpClose(conn)
            xxciClientStop(client)
            xxciServerStop(server)
            return resp
        "#, echo_port = echo_port);
        let result = sf.run_string(&src);
        assert!(result.is_ok(), "goconnectit 环回测试失败: {:?}", result);
        let val = result.unwrap();
        assert!(val.to_str().contains("hello goconnectit"), "回显数据不匹配: {}", val.to_str());
    }

    #[test]
    fn test_xxci_all_encryption_methods() {
        let echo_port = start_echo_server();
        std::thread::sleep(Duration::from_millis(50));

        for method in &["des", "txdef", "txdee", "txde"] {
            let mut sf = Sflang::new();
            let server_port = match method {
                &"des" => 19101,
                &"txdef" => 19102,
                &"txdee" => 19103,
                &"txde" => 19104,
                _ => unreachable!(),
            };
            let client_port = match method {
                &"des" => 19201,
                &"txdef" => 19202,
                &"txdee" => 19203,
                &"txde" => 19204,
                _ => unreachable!(),
            };
            let src = format!(r#"
                server := xxciServer("127.0.0.1:{server_port}", "testpass", "{method}")
                sleepMs(100)
                client := xxciClient("127.0.0.1:{server_port}", "127.0.0.1:{client_port}", "testpass", "{method}")
                sleepMs(200)
                conn := tcpConnect("127.0.0.1", {client_port}, 3000)
                tcpWrite(conn, bytes([0x05, 0x01, 0x00]))
                tcpRead(conn, 2)
                portHi := {echo_port} / 256
                portLo := {echo_port} % 256
                tcpWrite(conn, bytes([0x05, 0x01, 0x00, 0x01, 0x7f, 0x00, 0x00, 0x01, portHi, portLo]))
                tcpRead(conn, 10)
                tcpWriteLine(conn, "hello {method}")
                resp := tcpReadLine(conn)
                tcpClose(conn)
                xxciClientStop(client)
                xxciServerStop(server)
                return resp
            "#, server_port = server_port, client_port = client_port, method = method, echo_port = echo_port);
            let result = sf.run_string(&src);
            assert!(result.is_ok(), "加密方法 {} 测试失败: {:?}", method, result);
            let val = result.unwrap();
            assert!(val.to_str().contains(method), "方法 {} 回显不匹配: {}", method, val.to_str());
        }
    }
}
