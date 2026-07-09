//! builtins_ssh.rs — SSH 客户端内置函数（基于 russh）
//!
//! 纯 Rust SSH 客户端，对标 Charlang 的 ssh* 函数。
//! 用 tokio block_on 桥接异步 russh 为同步 API。
//!
//! 函数：
//!   sshRun("--host=...", "--port=22", "--user=...", "--password=...", "command")
//!       — 执行远程命令，返回输出字符串
//!   sshList("--host=...", "--user=...", "--password=...", "--remotePath=...")
//!       — 列出远程目录内容（每行一个文件）
//!   sshUpload("--host=...", "--user=...", "--password=...", "--localPath=...", "--remotePath=...")
//!       — 上传本地文件到远程
//!   sshDownload("--host=...", "--user=...", "--password=...", "--remotePath=...", "--localPath=...")
//!       — 下载远程文件到本地

use std::sync::Arc;

use crate::builtins_helpers as bh;
use crate::value::Value;
use crate::vm::VM;

/// register 注册 SSH 内置函数。
pub fn register(vm: &mut VM) {
    vm.register_builtin("sshRun", bi_ssh_run);
    vm.register_builtin("sshList", bi_ssh_list);
    vm.register_builtin("sshUpload", bi_ssh_upload);
    vm.register_builtin("sshDownload", bi_ssh_download);
}

/// get_switch 从参数列表中解析 --key=value 格式的开关。
fn get_switch(args: &[Value], key: &str, default: &str) -> String {
    let prefix = format!("--{}=", key);
    let prefix_short = format!("-{}=", key);
    for arg in args {
        if let Value::Str(s) = arg {
            if let Some(rest) = s.strip_prefix(&prefix).or_else(|| s.strip_prefix(&prefix_short)) {
                return rest.to_string();
            }
        }
    }
    default.to_string()
}

/// SshHandler russh 的空 Handler 实现。
struct SshHandler;

#[async_trait::async_trait]
impl russh::client::Handler for SshHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::key::PublicKey,
    ) -> Result<bool, Self::Error> {
        // 接受所有服务器密钥（简化，不验证指纹）
        Ok(true)
    }
}

/// ssh_connect 建立 SSH 连接并返回 Handle。
///
/// 内部用 tokio runtime 桥接。
fn ssh_connect(
    runtime: &tokio::runtime::Runtime,
    host: &str,
    port: u16,
    user: &str,
    password: &str,
) -> Result<russh::client::Handle<SshHandler>, String> {
    let config = Arc::new(russh::client::Config::default());
    let addr = format!("{}:{}", host, port);

    runtime.block_on(async {
        let mut handle = russh::client::connect(config, addr, SshHandler)
            .await
            .map_err(|e| format!("SSH 连接失败: {} (可能原因：网络不通、端口错误)", e))?;

        let auth_ok = handle
            .authenticate_password(user, password)
            .await
            .map_err(|e| format!("SSH 认证失败: {} (可能原因：用户名或密码错误)", e))?;

        if !auth_ok {
            return Err("SSH 认证失败: 密码被拒绝".to_string());
        }

        Ok(handle)
    })
}

/// ssh_exec 在已连接的 SSH session 上执行命令，返回输出。
fn ssh_exec(
    runtime: &tokio::runtime::Runtime,
    handle: &mut russh::client::Handle<SshHandler>,
    command: &str,
) -> Result<String, String> {
    runtime.block_on(async {
        let mut channel = handle
            .channel_open_session()
            .await
            .map_err(|e| format!("SSH 打开通道失败: {}", e))?;

        channel.exec(true, command).await.map_err(|e| format!("SSH exec 失败: {}", e))?;

        // 读取输出
        let mut output = Vec::new();
        use tokio::io::AsyncReadExt;
        let mut reader = channel.make_reader();
        reader.read_to_end(&mut output).await.map_err(|e| format!("SSH 读取输出失败: {}", e))?;

        Ok(String::from_utf8_lossy(&output).into_owned())
    })
}

// ---- 内置函数 ----

/// bi_ssh_run 执行远程命令。
///
/// 用法：sshRun("--host=...", "--port=22", "--user=...", "--password=...", "ls -la")
/// 或：  sshRun("--host=...", "--user=...", "--password=...", "ls -la")（端口默认 22）
///
/// 最后一个非 -- 开头的参数是命令。
fn bi_ssh_run(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let host = get_switch(args, "host", "");
    let port: u16 = get_switch(args, "port", "22").parse().unwrap_or(22);
    let user = get_switch(args, "user", "");
    let password = get_switch(args, "password", "");

    // 找命令：最后一个非 -- 开头的参数
    let mut command = String::new();
    for arg in args {
        if let Value::Str(s) = arg {
            if !s.starts_with("--") && !s.starts_with("-host") && !s.starts_with("-port") && !s.starts_with("-user") && !s.starts_with("-password") {
                command = s.to_string();
            }
        }
    }

    if host.is_empty() || user.is_empty() {
        return Ok(crate::value::error_value(
            "sshRun() 需要 --host 和 --user 参数 (格式: sshRun(\"--host=x\", \"--user=y\", \"--password=z\", \"command\"))",
        ));
    }

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return Ok(crate::value::error_value(format!("创建 tokio runtime 失败: {}", e))),
    };

    let mut handle = match ssh_connect(&runtime, &host, port, &user, &password) {
        Ok(h) => h,
        Err(e) => return Ok(crate::value::error_value(e)),
    };

    match ssh_exec(&runtime, &mut handle, &command) {
        Ok(output) => {
            let _ = runtime.block_on(handle.disconnect(russh::Disconnect::ByApplication, "", "en"));
            Ok(Value::str_from(output))
        }
        Err(e) => Ok(crate::value::error_value(e)),
    }
}

/// bi_ssh_list 列出远程目录内容。
///
/// 用法：sshList("--host=...", "--user=...", "--password=...", "--remotePath=/tmp")
fn bi_ssh_list(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let host = get_switch(args, "host", "");
    let port: u16 = get_switch(args, "port", "22").parse().unwrap_or(22);
    let user = get_switch(args, "user", "");
    let password = get_switch(args, "password", "");
    let remote_path = get_switch(args, "remotePath", "/");

    if host.is_empty() || user.is_empty() {
        return Ok(crate::value::error_value(
            "sshList() 需要 --host 和 --user 参数",
        ));
    }

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return Ok(crate::value::error_value(format!("创建 tokio runtime 失败: {}", e))),
    };

    let mut handle = match ssh_connect(&runtime, &host, port, &user, &password) {
        Ok(h) => h,
        Err(e) => return Ok(crate::value::error_value(e)),
    };

    let command = format!("ls -1 {}", remote_path);
    match ssh_exec(&runtime, &mut handle, &command) {
        Ok(output) => {
            let _ = runtime.block_on(handle.disconnect(russh::Disconnect::ByApplication, "", "en"));
            // 按行分割返回数组
            let files: Vec<Value> = output.lines().map(|l| Value::str(l.trim())).collect();
            Ok(Value::Array(Arc::new(std::sync::Mutex::new(files))))
        }
        Err(e) => Ok(crate::value::error_value(e)),
    }
}

/// bi_ssh_upload 上传文件。
///
/// 用法：sshUpload("--host=...", "--user=...", "--password=...", "--localPath=...", "--remotePath=...")
/// 通过 SFTP 或 scp 命令实现。这里用 cat > remotePath 方式（简单可靠）。
fn bi_ssh_upload(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let host = get_switch(args, "host", "");
    let port: u16 = get_switch(args, "port", "22").parse().unwrap_or(22);
    let user = get_switch(args, "user", "");
    let password = get_switch(args, "password", "");
    let local_path = get_switch(args, "localPath", "");
    let remote_path = get_switch(args, "remotePath", "");

    if host.is_empty() || user.is_empty() || local_path.is_empty() || remote_path.is_empty() {
        return Ok(crate::value::error_value(
            "sshUpload() 需要 --host, --user, --password, --localPath, --remotePath 参数",
        ));
    }

    // 读取本地文件
    let file_data = match std::fs::read(&local_path) {
        Ok(d) => d,
        Err(e) => return Ok(crate::value::error_value(format!(
            "sshUpload() 读取本地文件 '{}' 失败: {}", local_path, e,
        ))),
    };

    // base64 编码文件内容，用 echo | base64 -d 方式上传
    let b64: String = {
        let mut out = Vec::with_capacity((file_data.len() + 2) / 3 * 4);
        let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut i = 0;
        while i + 3 <= file_data.len() {
            let n = ((file_data[i] as u32) << 16) | ((file_data[i+1] as u32) << 8) | (file_data[i+2] as u32);
            out.push(table[((n >> 18) & 0x3F) as usize]);
            out.push(table[((n >> 12) & 0x3F) as usize]);
            out.push(table[((n >> 6) & 0x3F) as usize]);
            out.push(table[(n & 0x3F) as usize]);
            i += 3;
        }
        let rem = file_data.len() - i;
        if rem == 1 {
            let n = (file_data[i] as u32) << 16;
            out.push(table[((n >> 18) & 0x3F) as usize]);
            out.push(table[((n >> 12) & 0x3F) as usize]);
            out.push(b'='); out.push(b'=');
        } else if rem == 2 {
            let n = ((file_data[i] as u32) << 16) | ((file_data[i+1] as u32) << 8);
            out.push(table[((n >> 18) & 0x3F) as usize]);
            out.push(table[((n >> 12) & 0x3F) as usize]);
            out.push(table[((n >> 6) & 0x3F) as usize]);
            out.push(b'=');
        }
        String::from_utf8_lossy(&out).into_owned()
    };

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return Ok(crate::value::error_value(format!("创建 tokio runtime 失败: {}", e))),
    };

    let mut handle = match ssh_connect(&runtime, &host, port, &user, &password) {
        Ok(h) => h,
        Err(e) => return Ok(crate::value::error_value(e)),
    };

    let command = format!("echo {} | base64 -d > {}", b64, remote_path);
    match ssh_exec(&runtime, &mut handle, &command) {
        Ok(_) => {
            let _ = runtime.block_on(handle.disconnect(russh::Disconnect::ByApplication, "", "en"));
            Ok(Value::Undefined)
        }
        Err(e) => Ok(crate::value::error_value(e)),
    }
}

/// bi_ssh_download 下载文件。
///
/// 用法：sshDownload("--host=...", "--user=...", "--password=...", "--remotePath=...", "--localPath=...")
fn bi_ssh_download(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let host = get_switch(args, "host", "");
    let port: u16 = get_switch(args, "port", "22").parse().unwrap_or(22);
    let user = get_switch(args, "user", "");
    let password = get_switch(args, "password", "");
    let remote_path = get_switch(args, "remotePath", "");
    let local_path = get_switch(args, "localPath", "");

    if host.is_empty() || user.is_empty() || remote_path.is_empty() || local_path.is_empty() {
        return Ok(crate::value::error_value(
            "sshDownload() 需要 --host, --user, --password, --remotePath, --localPath 参数",
        ));
    }

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return Ok(crate::value::error_value(format!("创建 tokio runtime 失败: {}", e))),
    };

    let mut handle = match ssh_connect(&runtime, &host, port, &user, &password) {
        Ok(h) => h,
        Err(e) => return Ok(crate::value::error_value(e)),
    };

    // 用 cat remotepath 下载
    let command = format!("cat {}", remote_path);
    match ssh_exec(&runtime, &mut handle, &command) {
        Ok(output) => {
            let _ = runtime.block_on(handle.disconnect(russh::Disconnect::ByApplication, "", "en"));
            match std::fs::write(&local_path, output.as_bytes()) {
                Ok(()) => Ok(Value::Undefined),
                Err(e) => Ok(crate::value::error_value(format!(
                    "sshDownload() 写入本地文件 '{}' 失败: {}", local_path, e,
                ))),
            }
        }
        Err(e) => Ok(crate::value::error_value(e)),
    }
}
