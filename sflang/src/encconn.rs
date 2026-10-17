//! encconn.rs — xxci 加密连接层（对标 goconnectit 的连接加密，供多个模块共用）
//!
//! 从 builtins_xxci.rs 拆出的公共部分，使用方：
//!   - builtins_xxci.rs — xxciServer/xxciClient/xxciWrapConn 内置函数
//!   - builtins_tcp.rs  — TcpConn 的可选加密层（包装后 tcpRead/tcpWrite/tcpPipe 自动走加密通道）
//!
//! 协议与 Go 版 goconnectit 完全兼容（相同密码和加密方法可互通）：
//!   - des:   DES-CTR，连接建立时双方各生成随机 IV 并发给对方（写 IV 在前、读 IV 在后）
//!   - txdef: 每帧 = 2 字节大端长度 + addLen(2~6) 字节随机头 + 加密数据
//!   - txdee: 每帧 = 2 字节大端长度 + 2 字节随机头 + 加密数据 + 2 字节随机尾
//!   - txde:  纯流式逐字节加密，无额外帧开销
//!
//! 设计要点：
//!   - 纯标准库实现，DES 用自研的 des.rs（CtrStream）
//!   - 随机源基于 OS 熵（std RandomState 由操作系统播种），对齐 Go 版 crypto/rand 的质量
//!   - EncryptedReader/EncryptedWriter 各持有独立的 TcpStream 克隆（try_clone），
//!     读写两个方向可并发，互不阻塞

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use crate::des::CtrStream;

// ============ 辅助函数 ============

/// sum_bytes 计算字节切片的 u8 回绕和。
///
/// 用于 TXDEF 的随机头长度派生（与 Go 版 sumBytes 一致）。
pub fn sum_bytes(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |acc, &b| acc.wrapping_add(b))
}

/// random_byte 生成一个随机字节（OS 熵源）。
///
/// 纯标准库方案：std 的 RandomState 内部由操作系统熵播种
/// （Windows 走 BCryptGenRandom，Linux 走 getrandom//dev/urandom），
/// 借助其 Hasher 产出随机数；再混入高精度时间与进程内计数器，
/// 保证同一线程内连续调用也互不相同。
pub fn random_byte() -> u8 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    /// counter 进程内单调计数器，区分同线程的连续调用。
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let nanos = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);

    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(nanos);
    hasher.write_u64(c);
    hasher.write_u64(std::process::id() as u64);
    hasher.finish() as u8
}

/// random_bytes 填充 buf 为随机字节（逐字节调用 random_byte）。
pub fn random_bytes(buf: &mut [u8]) {
    for b in buf.iter_mut() {
        *b = random_byte();
    }
}

/// dial_with_timeout 带超时的 TCP 拨号。
///
/// 域名先解析（解析阶段无超时控制），再对每个候选地址尝试带超时连接，
/// 任一成功即返回。对标 Go 版 net.DialTimeout 的"目标不可达时最终失败
/// 而不是永久挂起"行为。
pub fn dial_with_timeout(addr: &str, timeout: Duration) -> std::io::Result<TcpStream> {
    let addrs = addr.to_socket_addrs()?;
    let mut last_err = None;
    for sa in addrs {
        match TcpStream::connect_timeout(&sa, timeout) {
            Ok(s) => return Ok(s),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("地址无候选: {}", addr))
    }))
}

// ============ 加密读写器 ============

/// EncryptedReader 加密读取器（读方向）。
///
/// 每种加密方法有独立的读取逻辑。
/// 通过 try_clone 持有独立的 TcpStream，支持与写方向并发。
pub enum EncryptedReader {
    /// DES-CTR 流式读取
    Des {
        /// stream CTR 解密流（对端 IV 初始化）。
        stream: CtrStream,
        /// conn 底层连接（独立克隆句柄）。
        conn: TcpStream,
    },
    /// TXDEF 分块读取
    Txdef {
        /// conn 底层连接（独立克隆句柄）。
        conn: TcpStream,
        /// code 密码字节序列。
        code: Vec<u8>,
        /// add_len 每帧随机头字节数（2~6）。
        add_len: usize,
        /// enc_index 密钥字节在随机头中的位置。
        enc_index: usize,
        /// read_buf 当前帧解密后的剩余数据缓存。
        read_buf: Vec<u8>,
        /// read_idx 缓存读取位置。
        read_idx: usize,
    },
    /// TXDEE 分块读取
    Txdee {
        /// conn 底层连接（独立克隆句柄）。
        conn: TcpStream,
        /// code 密码字节序列。
        code: Vec<u8>,
        /// read_buf 当前帧解密后的剩余数据缓存。
        read_buf: Vec<u8>,
        /// read_idx 缓存读取位置。
        read_idx: usize,
    },
    /// TXDE 流式读取
    Txde {
        /// conn 底层连接（独立克隆句柄）。
        conn: TcpStream,
        /// code 密码字节序列。
        code: Vec<u8>,
        /// idx 密码流当前位置。
        idx: usize,
    },
}

/// EncryptedWriter 加密写入器（写方向）。
pub enum EncryptedWriter {
    /// DES-CTR 流式写入
    Des {
        /// stream CTR 加密流（本端随机 IV 初始化）。
        stream: CtrStream,
        /// conn 底层连接（独立克隆句柄）。
        conn: TcpStream,
    },
    /// TXDEF 分块写入
    Txdef {
        /// conn 底层连接（独立克隆句柄）。
        conn: TcpStream,
        /// code 密码字节序列。
        code: Vec<u8>,
        /// add_len 每帧随机头字节数（2~6）。
        add_len: usize,
        /// enc_index 密钥字节在随机头中的位置。
        enc_index: usize,
    },
    /// TXDEE 分块写入
    Txdee {
        /// conn 底层连接（独立克隆句柄）。
        conn: TcpStream,
        /// code 密码字节序列。
        code: Vec<u8>,
    },
    /// TXDE 流式写入
    Txde {
        /// conn 底层连接（独立克隆句柄）。
        conn: TcpStream,
        /// code 密码字节序列。
        code: Vec<u8>,
        /// idx 密码流当前位置。
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
                let mut random_bytes_buf = vec![0u8; *add_len];
                random_bytes(&mut random_bytes_buf);
                let key_byte = random_bytes_buf[*enc_index];
                let total_len = *add_len + data_len;
                let mut out = Vec::with_capacity(2 + total_len);
                out.push((total_len >> 8) as u8);
                out.push(total_len as u8);
                out.extend_from_slice(&random_bytes_buf);
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
                let mut rnd = [0u8; 4];
                random_bytes(&mut rnd);
                let (key_byte1, key_byte2) = (rnd[0], rnd[1]);
                let (trail1, trail2) = (rnd[2], rnd[3]);
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

/// EncryptedIO 加密读写层，包装在 TcpConn 中使用。
///
/// reader/writer 各自加互斥锁保护有状态的密码流；
/// Arc 共享使 ConnReadHandle/ConnWriteHandle 能独立持有读写两端。
pub struct EncryptedIO {
    /// reader 解密读端（有状态，须互斥）。
    pub reader: Arc<Mutex<EncryptedReader>>,
    /// writer 加密写端（有状态，须互斥）。
    pub writer: Arc<Mutex<EncryptedWriter>>,
}

/// create_encrypted_conn 创建加密连接读写层。
///
/// 根据 encrypt_method 创建对应的加密读写器。
/// 对于 DES，会先交换 IV（先写本端 IV、后读对端 IV）。
/// 注意：调用后原始 conn 的加密集状态由返回的读写器持有，
/// 不应再用原始句柄直接读写，否则会破坏密码流对齐。
pub fn create_encrypted_conn(
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

            // 生成写 IV（OS 熵随机）
            let mut iv_write = [0u8; 8];
            random_bytes(&mut iv_write);

            // 写入 IV
            conn.write_all(&iv_write).map_err(|e| format!("写入 DES IV 失败: {}", e))?;
            let _ = conn.flush();

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

// ============ 统一读写句柄（供 tcpPipe 等批量转发使用） ============

/// ConnReadHandle 统一读句柄：明文为独立 TcpStream 克隆，加密为共享解密 reader。
pub enum ConnReadHandle {
    /// Plain 明文读（独立克隆句柄，避免长时间持锁）。
    Plain(TcpStream),
    /// Enc 加密读（共享 reader，内部互斥）。
    Enc(Arc<Mutex<EncryptedReader>>),
}

impl Read for ConnReadHandle {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            ConnReadHandle::Plain(s) => s.read(buf),
            ConnReadHandle::Enc(r) => r.lock().unwrap().read(buf),
        }
    }
}

/// ConnWriteHandle 统一写句柄：明文为独立 TcpStream 克隆，加密为共享加密 writer。
pub enum ConnWriteHandle {
    /// Plain 明文写（独立克隆句柄，避免长时间持锁）。
    Plain(TcpStream),
    /// Enc 加密写（共享 writer，内部互斥；write 语义为写全部）。
    Enc(Arc<Mutex<EncryptedWriter>>),
}

impl Write for ConnWriteHandle {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            ConnWriteHandle::Plain(s) => {
                // 明文保持单次 write 语义，与原 pipe 实现一致（配合 write_all 使用）
                s.write(buf)
            }
            ConnWriteHandle::Enc(w) => {
                // 加密写端内部已实现为写全部（整帧加密后发出）
                w.lock().unwrap().write(buf)
            }
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            ConnWriteHandle::Plain(s) => s.flush(),
            ConnWriteHandle::Enc(w) => w.lock().unwrap().flush(),
        }
    }
}

// ============ 测试 ============

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_random_byte_varies() {
        // 连续取 64 个字节，应至少出现 8 种不同值（OS 熵源下几乎必然远多于此）
        let vals: Vec<u8> = (0..64).map(|_| random_byte()).collect();
        let distinct: std::collections::HashSet<u8> = vals.iter().copied().collect();
        assert!(distinct.len() >= 8, "64 个随机字节仅 {} 种取值，随机性不足", distinct.len());
    }

    #[test]
    fn test_sum_bytes_matches_go() {
        // Go 版 sumBytes("topxeq")：逐字节 u8 回绕和
        assert_eq!(sum_bytes(b"topxeq"), b"topxeq".iter().fold(0u8, |a, &b| a.wrapping_add(b)));
        assert_eq!(sum_bytes(&[0xFF, 0x01]), 0x00);
    }

    #[test]
    fn test_dial_with_timeout_refused() {
        // 绑定后立即释放端口，连接应快速失败而不是挂起
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let r = dial_with_timeout(&format!("127.0.0.1:{}", port), Duration::from_millis(1000));
        assert!(r.is_err(), "对已释放端口连接应失败");
    }
}
