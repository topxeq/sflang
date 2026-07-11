//! builtins_ssh.rs — SSH 客户端内置函数（基于 russh + russh-sftp）
//!
//! 纯 Rust SSH 客户端，对标 Charlang 的 ssh* 函数。
//! 文件传输用 SFTP 子系统（原生协议，高效可靠）。
//! 支持密码认证和私钥认证。
//!
//! 函数：
//!   sshRun      — 执行远程命令
//!   sshList     — 列出远程目录内容
//!   sshUpload   — SFTP 上传文件
//!   sshDownload — SFTP 下载文件
//!   sshMkdir    — 创建远程目录
//!   sshRemove   — 删除远程文件或目录
//!   sshMove     — 移动/重命名远程文件

use std::sync::Arc;

use crate::value::Value;
use crate::vm::VM;

pub fn register(vm: &mut VM) {
    vm.register_builtin("sshRun", bi_ssh_run);
    vm.register_builtin("sshList", bi_ssh_list);
    vm.register_builtin("sshUpload", ssh_upload_impl);
    vm.register_builtin("sshDownload", ssh_download_impl);
    vm.register_builtin("sshMkdir", bi_ssh_mkdir);
    vm.register_builtin("sshRemove", bi_ssh_remove);
    vm.register_builtin("sshMove", bi_ssh_move);
    vm.register_builtin("sshSync", bi_ssh_sync);
    vm.register_builtin("sshCreateFile", bi_ssh_create_file);
    vm.register_builtin("sshUploadBytes", bi_ssh_upload_bytes);
    vm.register_builtin("sshDownloadBytes", bi_ssh_download_bytes);
}

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

fn get_command(args: &[Value]) -> String {
    for arg in args {
        if let Value::Str(s) = arg {
            if !s.starts_with("--") && !s.starts_with("-h=") && !s.starts_with("-p=") && !s.starts_with("-u=") && !s.starts_with("-pass=") {
                return s.to_string();
            }
        }
    }
    String::new()
}

struct SshParams {
    host: String,
    port: u16,
    user: String,
    password: String,
    key_path: String,
    key_passphrase: String,
    /// 命令超时（秒），0 = 无超时。
    cmd_timeout: u64,
}

fn parse_ssh_params(args: &[Value]) -> Result<SshParams, Value> {
    let p = SshParams {
        host: get_switch(args, "host", ""),
        port: get_switch(args, "port", "22").parse().unwrap_or(22),
        user: get_switch(args, "user", ""),
        password: get_switch(args, "password", ""),
        key_path: get_switch(args, "key", ""),
        key_passphrase: get_switch(args, "keyPassphrase", ""),
        cmd_timeout: get_switch(args, "cmdTimeout", "0").parse().unwrap_or(0),
    };
    if p.host.is_empty() || p.user.is_empty() {
        return Err(crate::value::error_value("SSH 需要 --host 和 --user 参数"));
    }
    if p.password.is_empty() && p.key_path.is_empty() {
        return Err(crate::value::error_value("SSH 需要 --password 或 --key 认证参数"));
    }
    Ok(p)
}

struct SshHandler;

#[async_trait::async_trait]
impl russh::client::Handler for SshHandler {
    type Error = russh::Error;
    async fn check_server_key(&mut self, _: &russh::keys::key::PublicKey) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// do_ssh 建立 SSH 连接 + 认证，在 tokio runtime 中运行异步操作。
fn do_ssh<F, Fut, R>(params: &SshParams, op: F) -> Result<R, String>
where
    F: FnOnce(russh::client::Handle<SshHandler>) -> Fut + Send,
    Fut: std::future::Future<Output = Result<R, String>> + Send,
    R: Send,
{
    let runtime = tokio::runtime::Runtime::new().map_err(|e| format!("创建 tokio runtime 失败: {}", e))?;
    let config = Arc::new(russh::client::Config::default());
    let addr = format!("{}:{}", params.host, params.port);

    runtime.block_on(async {
        let mut handle = russh::client::connect(config, addr, SshHandler)
            .await
            .map_err(|e| format!("SSH 连接失败: {} (可能原因：网络不通)", e))?;

        let auth_ok = if !params.key_path.is_empty() {
            let key_pair = russh::keys::load_secret_key(
                &params.key_path,
                if params.key_passphrase.is_empty() { None } else { Some(&params.key_passphrase) },
            ).map_err(|e| format!("SSH 加载私钥失败: {}", e))?;
            handle.authenticate_publickey(&params.user, Arc::new(key_pair))
                .await.map_err(|e| format!("SSH 密钥认证失败: {}", e))?
        } else {
            handle.authenticate_password(&params.user, &params.password)
                .await.map_err(|e| format!("SSH 认证失败: {}", e))?
        };

        if !auth_ok {
            return Err("SSH 认证失败: 凭据被拒绝".to_string());
        }

        op(handle).await
    })
}

/// 在 channel 上执行远程命令，返回输出。支持超时。
async fn exec_cmd(handle: &russh::client::Handle<SshHandler>, command: &str, timeout_secs: u64) -> Result<String, String> {
    let mut channel = handle.channel_open_session().await
        .map_err(|e| format!("SSH 打开通道失败: {}", e))?;
    channel.exec(true, command).await
        .map_err(|e| format!("SSH exec 失败: {}", e))?;

    let read_fut = async {
        let mut output = Vec::new();
        use tokio::io::AsyncReadExt;
        let mut reader = channel.make_reader();
        reader.read_to_end(&mut output).await
            .map_err(|e| format!("SSH 读取输出失败: {}", e))?;
        Ok::<Vec<u8>, String>(output)
    };

    let output = if timeout_secs > 0 {
        tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), read_fut)
            .await
            .map_err(|_| format!("SSH 命令超时 ({}秒)", timeout_secs))?
    } else {
        read_fut.await
    }?;

    Ok(String::from_utf8_lossy(&output).into_owned())
}

/// 建立 SFTP 会话。
async fn sftp_open(handle: &russh::client::Handle<SshHandler>) -> Result<russh_sftp::client::SftpSession, String> {
    let channel = handle.channel_open_session().await
        .map_err(|e| format!("SFTP 打开通道失败: {}", e))?;
    channel.request_subsystem(true, "sftp").await
        .map_err(|e| format!("SFTP 子系统失败: {}", e))?;
    russh_sftp::client::SftpSession::new(channel.into_stream())
        .await.map_err(|e| format!("SFTP 会话失败: {}", e))
}

// ---- 内置函数 ----

fn bi_ssh_run(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ssh_params(args)?;
    let command = get_command(args);
    if command.is_empty() {
        return Ok(crate::value::error_value("sshRun() 需要命令参数"));
    }

    match do_ssh(&params, |handle| async move {
        let result = exec_cmd(&handle, &command, params.cmd_timeout).await;
        let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        result
    }) {
        Ok(output) => Ok(Value::str_from(output)),
        Err(e) => Ok(crate::value::error_value(e)),
    }
}

fn bi_ssh_list(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ssh_params(args)?;
    let remote_path = get_switch(args, "remotePath", "/");

    match do_ssh(&params, |handle| async move {
        let sftp = sftp_open(&handle).await?;
        let mut entries = Vec::new();
        let dir = sftp.read_dir(&remote_path).await
            .map_err(|e| format!("SFTP 读取目录失败: {}", e))?;
        for entry in dir {
            entries.push(entry.file_name());
        }
        let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        Ok::<Vec<String>, String>(entries)
    }) {
        Ok(files) => {
            let result: Vec<Value> = files.into_iter().map(Value::str_from).collect();
            Ok(Value::Array(Arc::new(std::sync::Mutex::new(result))))
        }
        Err(e) => Ok(crate::value::error_value(e)),
    }
}

pub fn ssh_upload_impl(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ssh_params(args)?;
    let local_path = get_switch(args, "localPath", "");
    let remote_path = get_switch(args, "remotePath", "");
    if local_path.is_empty() || remote_path.is_empty() {
        return Ok(crate::value::error_value("sshUpload() 需要 --localPath 和 --remotePath 参数"));
    }

    let file_data = std::fs::read(&local_path).map_err(|e| {
        crate::value::error_value(format!("sshUpload() 读取本地文件 '{}' 失败: {}", local_path, e))
    })?;

    match do_ssh(&params, |handle| async move {
        let sftp = sftp_open(&handle).await?;
        let mut file = sftp.create(&remote_path).await
            .map_err(|e| format!("SFTP 创建文件失败: {}", e))?;
        use tokio::io::AsyncWriteExt;
        file.write_all(&file_data).await
            .map_err(|e| format!("SFTP 写入失败: {}", e))?;
        file.flush().await.ok();
        let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        Ok::<(), String>(())
    }) {
        Ok(()) => Ok(Value::Undefined),
        Err(e) => Ok(crate::value::error_value(e)),
    }
}

pub fn ssh_download_impl(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ssh_params(args)?;
    let remote_path = get_switch(args, "remotePath", "");
    let local_path = get_switch(args, "localPath", "");
    if remote_path.is_empty() || local_path.is_empty() {
        return Ok(crate::value::error_value("sshDownload() 需要 --remotePath 和 --localPath 参数"));
    }

    match do_ssh(&params, |handle| async move {
        let sftp = sftp_open(&handle).await?;
        let data = sftp.read(&remote_path).await
            .map_err(|e| format!("SFTP 读取失败: {}", e))?;
        let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        Ok::<Vec<u8>, String>(data)
    }) {
        Ok(data) => {
            std::fs::write(&local_path, &data).map_err(|e| {
                crate::value::error_value(format!("sshDownload() 写入本地 '{}' 失败: {}", local_path, e))
            })?;
            Ok(Value::Undefined)
        }
        Err(e) => Ok(crate::value::error_value(e)),
    }
}

fn bi_ssh_mkdir(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ssh_params(args)?;
    let remote_path = get_switch(args, "remotePath", "");
    if remote_path.is_empty() {
        return Ok(crate::value::error_value("sshMkdir() 需要 --remotePath 参数"));
    }

    match do_ssh(&params, |handle| async move {
        let sftp = sftp_open(&handle).await?;
        sftp.create_dir(&remote_path).await
            .map_err(|e| format!("SFTP 创建目录失败: {}", e))?;
        let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        Ok::<(), String>(())
    }) {
        Ok(()) => Ok(Value::Undefined),
        Err(e) => Ok(crate::value::error_value(e)),
    }
}

fn bi_ssh_remove(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ssh_params(args)?;
    let remote_path = get_switch(args, "remotePath", "");
    if remote_path.is_empty() {
        return Ok(crate::value::error_value("sshRemove() 需要 --remotePath 参数"));
    }

    match do_ssh(&params, |handle| async move {
        let sftp = sftp_open(&handle).await?;
        // 先试删文件，失败再删目录
        if sftp.remove_file(&remote_path).await.is_err() {
            sftp.remove_dir(&remote_path).await
                .map_err(|e| format!("SFTP 删除失败: {}", e))?;
        }
        let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        Ok::<(), String>(())
    }) {
        Ok(()) => Ok(Value::Undefined),
        Err(e) => Ok(crate::value::error_value(e)),
    }
}

fn bi_ssh_move(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ssh_params(args)?;
    let remote_path = get_switch(args, "remotePath", "");
    let target_path = get_switch(args, "targetPath", "");
    if remote_path.is_empty() || target_path.is_empty() {
        return Ok(crate::value::error_value("sshMove() 需要 --remotePath 和 --targetPath 参数"));
    }

    match do_ssh(&params, |handle| async move {
        let sftp = sftp_open(&handle).await?;
        sftp.rename(&remote_path, &target_path).await
            .map_err(|e| format!("SFTP 移动失败: {}", e))?;
        let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        Ok::<(), String>(())
    }) {
        Ok(()) => Ok(Value::Undefined),
        Err(e) => Ok(crate::value::error_value(e)),
    }
}

/// has_switch 检查布尔开关是否存在。
fn has_switch(args: &[Value], key: &str) -> bool {
    let full = format!("--{}", key);
    let short = format!("-{}", key);
    args.iter().any(|arg| {
        if let Value::Str(s) = arg { &**s == full || &**s == short }
        else { false }
    })
}

/// bi_ssh_sync 目录同步。
fn bi_ssh_sync(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ssh_params(args)?;
    let local_path = get_switch(args, "localPath", "");
    let remote_path = get_switch(args, "remotePath", "");
    let direction = get_switch(args, "direction", "push");
    let recursive = has_switch(args, "recursive");
    let delete_extra = has_switch(args, "delete");
    let dry_run = has_switch(args, "dryRun");

    if local_path.is_empty() || remote_path.is_empty() {
        return Ok(crate::value::error_value("sshSync() 需要 --localPath 和 --remotePath 参数"));
    }

    match do_ssh(&params, |handle| async move {
        let sftp = sftp_open(&handle).await?;
        let mut log = Vec::new();
        match direction.as_str() {
            "push" => sync_push(&sftp, &local_path, &remote_path, recursive, delete_extra, dry_run, &mut log).await?,
            "pull" => sync_pull(&sftp, &local_path, &remote_path, recursive, delete_extra, dry_run, &mut log).await?,
            _ => return Err("--direction 只支持 push 或 pull".to_string()),
        }
        let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        Ok::<Vec<String>, String>(log)
    }) {
        Ok(log) => {
            let result: Vec<Value> = log.into_iter().map(Value::str_from).collect();
            Ok(Value::Array(Arc::new(std::sync::Mutex::new(result))))
        }
        Err(e) => Ok(crate::value::error_value(e)),
    }
}

fn list_local_dir(path: &str) -> Result<Vec<(String, bool)>, String> {
    let entries = std::fs::read_dir(path).map_err(|e| format!("读取本地目录 '{}' 失败: {}", path, e))?;
    let mut result = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("读取条目失败: {}", e))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        result.push((name, is_dir));
    }
    Ok(result)
}

async fn list_remote_dir(sftp: &russh_sftp::client::SftpSession, path: &str) -> Result<Vec<(String, bool)>, String> {
    let dir = sftp.read_dir(path).await
        .map_err(|e| format!("SFTP 读取目录 '{}' 失败: {}", path, e))?;
    let mut result = Vec::new();
    for entry in dir {
        let name = entry.file_name();
        let is_dir = entry.file_type().is_dir();
        result.push((name, is_dir));
    }
    Ok(result)
}

/// to_native_path 转为本地路径格式（Windows 加反斜杠）。
fn to_native_path(p: &str) -> String {
    if cfg!(windows) { p.replace('/', "\\") } else { p.to_string() }
}

/// join_path_unix 用 / 拼接路径（远程路径用 Unix 格式）。
fn join_unix(base: &str, name: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), name)
}

async fn sync_push(
    sftp: &russh_sftp::client::SftpSession, local: &str, remote: &str,
    recursive: bool, delete_extra: bool, dry_run: bool, log: &mut Vec<String>,
) -> Result<(), String> {
    if !dry_run { let _ = sftp.create_dir(remote).await; }
    let local_files = list_local_dir(local)?;
    let remote_files = list_remote_dir(sftp, remote).await.unwrap_or_default();
    for (name, is_dir) in &local_files {
        let lp = join_unix(local, name);
        let rp = join_unix(remote, name);
        if *is_dir && recursive {
            log.push(format!("DIR  → {}", rp));
            Box::pin(sync_push(sftp, &lp, &rp, recursive, delete_extra, dry_run, log)).await?;
        } else if !*is_dir {
            if dry_run { log.push(format!("PUT  {} → {}", lp, rp)); }
            else {
                let data = std::fs::read(to_native_path(&lp)).map_err(|e| format!("读取 '{}' 失败: {}", lp, e))?;
                let mut f = sftp.create(&rp).await.map_err(|e| format!("SFTP 创建 '{}' 失败: {}", rp, e))?;
                use tokio::io::AsyncWriteExt;
                f.write_all(&data).await.map_err(|e| format!("写入 '{}' 失败: {}", rp, e))?;
                f.flush().await.ok();
                log.push(format!("PUT  {} → {} ({} bytes)", lp, rp, data.len()));
            }
        }
    }
    if delete_extra {
        let local_names: std::collections::HashSet<&str> = local_files.iter().map(|(n,_)| n.as_str()).collect();
        for (name,_) in &remote_files {
            if !local_names.contains(name.as_str()) {
                let rp = join_unix(remote, name);
                if dry_run { log.push(format!("DEL  {}", rp)); }
                else { let _ = sftp.remove_file(&rp).await; log.push(format!("DEL  {}", rp)); }
            }
        }
    }
    Ok(())
}

async fn sync_pull(
    sftp: &russh_sftp::client::SftpSession, local: &str, remote: &str,
    recursive: bool, delete_extra: bool, dry_run: bool, log: &mut Vec<String>,
) -> Result<(), String> {
    if !dry_run { std::fs::create_dir_all(to_native_path(local)).map_err(|e| format!("创建本地目录 '{}' 失败: {}", local, e))?; }
    let remote_files = list_remote_dir(sftp, remote).await?;
    let local_files = list_local_dir(&to_native_path(local)).unwrap_or_default();
    for (name, is_dir) in &remote_files {
        let rp = join_unix(remote, name);
        let lp = join_unix(local, name);
        if *is_dir && recursive {
            log.push(format!("DIR  ← {}", rp));
            Box::pin(sync_pull(sftp, &lp, &rp, recursive, delete_extra, dry_run, log)).await?;
        } else if !*is_dir {
            if dry_run { log.push(format!("GET  {} → {}", rp, lp)); }
            else {
                let data = sftp.read(&rp).await.map_err(|e| format!("SFTP 读取 '{}' 失败: {}", rp, e))?;
                std::fs::write(to_native_path(&lp), &data).map_err(|e| format!("写入 '{}' 失败: {}", lp, e))?;
                log.push(format!("GET  {} → {} ({} bytes)", rp, lp, data.len()));
            }
        }
    }
    if delete_extra {
        let remote_names: std::collections::HashSet<&str> = remote_files.iter().map(|(n,_)| n.as_str()).collect();
        for (name,_) in &local_files {
            if !remote_names.contains(name.as_str()) {
                let lp = join_unix(local, name);
                if dry_run { log.push(format!("DEL  {}", lp)); }
                else { let _ = std::fs::remove_file(to_native_path(&lp)); log.push(format!("DEL  {}", lp)); }
            }
        }
    }
    Ok(())
}

/// bi_ssh_create_file 在远程创建文件（带内容）。
///
/// 用法：sshCreateFile("--host=...", "--user=...", "--password=...",
///                    "--remotePath=/tmp/config.txt", "--content=文件内容")
fn bi_ssh_create_file(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ssh_params(args)?;
    let remote_path = get_switch(args, "remotePath", "");
    let content = get_switch(args, "content", "");

    if remote_path.is_empty() {
        return Ok(crate::value::error_value("sshCreateFile() 需要 --remotePath 参数"));
    }

    let content_bytes = content.into_bytes();

    match do_ssh(&params, |handle| async move {
        let sftp = sftp_open(&handle).await?;
        let mut file = sftp.create(&remote_path).await
            .map_err(|e| format!("SFTP 创建文件失败: {}", e))?;
        use tokio::io::AsyncWriteExt;
        file.write_all(&content_bytes).await
            .map_err(|e| format!("SFTP 写入失败: {}", e))?;
        file.flush().await.ok();
        let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        Ok::<(), String>(())
    }) {
        Ok(()) => Ok(Value::Undefined),
        Err(e) => Ok(crate::value::error_value(e)),
    }
}

/// bi_ssh_upload_bytes 用 SFTP 上传 bytes/byteArray 到远程。
///
/// 用法：sshUploadBytes("--host=...", "--user=...", "--password=...",
///                    "--remotePath=/tmp/data.bin", dataBytes)
/// 最后一个参数是要上传的 bytes/byteArray。
fn bi_ssh_upload_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ssh_params(args)?;
    let remote_path = get_switch(args, "remotePath", "");

    if remote_path.is_empty() {
        return Ok(crate::value::error_value("sshUploadBytes() 需要 --remotePath 参数"));
    }

    // 找 bytes 参数（最后一个非 -- 开头的参数）
    let data = match args.iter().rev().find(|arg| matches!(arg, Value::Bytes(_) | Value::ByteArray(_))) {
        Some(Value::Bytes(b)) => b.as_ref().to_vec(),
        Some(Value::ByteArray(b)) => b.lock().unwrap().clone(),
        _ => return Ok(crate::value::error_value("sshUploadBytes() 需要 bytes/byteArray 参数")),
    };

    match do_ssh(&params, |handle| async move {
        let sftp = sftp_open(&handle).await?;
        let mut file = sftp.create(&remote_path).await
            .map_err(|e| format!("SFTP 创建文件失败: {}", e))?;
        use tokio::io::AsyncWriteExt;
        file.write_all(&data).await
            .map_err(|e| format!("SFTP 写入失败: {}", e))?;
        file.flush().await.ok();
        let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        Ok::<(), String>(())
    }) {
        Ok(()) => Ok(Value::Undefined),
        Err(e) => Ok(crate::value::error_value(e)),
    }
}

/// bi_ssh_download_bytes 用 SFTP 下载远程文件到 bytes。
///
/// 用法：sshDownloadBytes("--host=...", "--user=...", "--password=...",
///                      "--remotePath=/tmp/data.bin")
/// 返回 bytes。
fn bi_ssh_download_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ssh_params(args)?;
    let remote_path = get_switch(args, "remotePath", "");

    if remote_path.is_empty() {
        return Ok(crate::value::error_value("sshDownloadBytes() 需要 --remotePath 参数"));
    }

    match do_ssh(&params, |handle| async move {
        let sftp = sftp_open(&handle).await?;
        let data = sftp.read(&remote_path).await
            .map_err(|e| format!("SFTP 读取失败: {}", e))?;
        let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        Ok::<Vec<u8>, String>(data)
    }) {
        Ok(data) => Ok(Value::Bytes(Arc::new(data))),
        Err(e) => Ok(crate::value::error_value(e)),
    }
}
