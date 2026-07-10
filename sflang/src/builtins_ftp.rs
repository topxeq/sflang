//! builtins_ftp.rs - FTP 客户端内置函数（基于 suppaftp）
//!
//! 纯 Rust FTP/FTPS 客户端，对标 Charlang 的 ftp* 函数。
//!
//! 函数：
//!   ftpList         - 列目录
//!   ftpUpload       - 上传文件
//!   ftpDownload     - 下载文件到本地
//!   ftpDownloadBytes- 下载到 bytes
//!   ftpCreateDir    - 创建远程目录
//!   ftpRemoveFile   - 删除远程文件
//!   ftpSize         - 获取文件大小

use std::sync::Arc;

use crate::value::Value;
use crate::vm::VM;

pub fn register(vm: &mut VM) {
    vm.register_builtin("ftpList", bi_ftp_list);
    vm.register_builtin("ftpUpload", bi_ftp_upload);
    vm.register_builtin("ftpDownload", bi_ftp_download);
    vm.register_builtin("ftpDownloadBytes", bi_ftp_download_bytes);
    vm.register_builtin("ftpCreateDir", bi_ftp_create_dir);
    vm.register_builtin("ftpRemoveFile", bi_ftp_remove_file);
    vm.register_builtin("ftpSize", bi_ftp_size);
    vm.register_builtin("ftpCreateFile", bi_ftp_create_file);
    vm.register_builtin("ftpUploadBytes", bi_ftp_upload_bytes);
}

fn get_switch(args: &[Value], key: &str, default: &str) -> String {
    let p1 = format!("--{}=", key);
    let p2 = format!("-{}=", key);
    for arg in args {
        if let Value::Str(s) = arg {
            if let Some(rest) = s.strip_prefix(&p1).or_else(|| s.strip_prefix(&p2)) {
                return rest.to_string();
            }
        }
    }
    default.to_string()
}

fn has_switch(args: &[Value], key: &str) -> bool {
    let p1 = format!("--{}", key);
    let p2 = format!("-{}", key);
    args.iter().any(|arg| {
        if let Value::Str(s) = arg { &**s == p1 || &**s == p2 }
        else { false }
    })
}

struct FtpParams {
    host: String,
    port: u16,
    user: String,
    password: String,
}

fn parse_ftp_params(args: &[Value]) -> Result<FtpParams, Value> {
    let p = FtpParams {
        host: get_switch(args, "host", ""),
        port: get_switch(args, "port", "21").parse().unwrap_or(21),
        user: get_switch(args, "user", "anonymous"),
        password: get_switch(args, "password", ""),
    };
    if p.host.is_empty() {
        return Err(crate::value::error_value("FTP 需要 --host 参数"));
    }
    let _ = has_switch; // 保留 has_switch 供将来 --tls 使用
    Ok(p)
}

fn ftp_connect(params: &FtpParams) -> Result<suppaftp::FtpStream, Value> {
    let addr = format!("{}:{}", params.host, params.port);
    let mut ftp = suppaftp::FtpStream::connect(&addr)
        .map_err(|e| crate::value::error_value(format!(
            "FTP 连接 {} 失败: {} (可能原因：网络不通或端口错误)", addr, e,
        )))?;

    ftp.login(&params.user, &params.password)
        .map_err(|e| crate::value::error_value(format!(
            "FTP 登录失败: {} (可能原因：用户名或密码错误)", e,
        )))?;

    Ok(ftp)
}

fn bi_ftp_list(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ftp_params(args)?;
    let remote_path = get_switch(args, "remotePath", "/");

    let mut ftp = ftp_connect(&params)?;

    let entries = ftp.list(Some(&remote_path))
        .map_err(|e| crate::value::error_value(format!(
            "FTP 列目录失败: {} (路径: {})", e, remote_path,
        )))?;

    let result: Vec<Value> = entries.into_iter()
        .map(|s| Value::str_from(s.trim().to_string()))
        .filter(|v| if let Value::Str(s) = v { !s.is_empty() } else { true })
        .collect();

    ftp.quit().ok();
    Ok(Value::Array(Arc::new(std::sync::Mutex::new(result))))
}

fn bi_ftp_upload(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ftp_params(args)?;
    let local_path = get_switch(args, "localPath", "");
    let remote_path = get_switch(args, "remotePath", "");

    if local_path.is_empty() || remote_path.is_empty() {
        return Err(crate::value::error_value("ftpUpload() 需要 --localPath 和 --remotePath 参数"));
    }

    let data = std::fs::read(&local_path)
        .map_err(|e| crate::value::error_value(format!(
            "ftpUpload() 读取本地文件 '{}' 失败: {}", local_path, e,
        )))?;

    let mut ftp = ftp_connect(&params)?;

    ftp.put_file(&remote_path, &mut std::io::Cursor::new(data))
        .map_err(|e| crate::value::error_value(format!(
            "FTP 上传失败: {} (远程路径: {})", e, remote_path,
        )))?;

    ftp.quit().ok();
    Ok(Value::Undefined)
}

fn bi_ftp_download(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ftp_params(args)?;
    let local_path = get_switch(args, "localPath", "");
    let remote_path = get_switch(args, "remotePath", "");

    if local_path.is_empty() || remote_path.is_empty() {
        return Err(crate::value::error_value("ftpDownload() 需要 --localPath 和 --remotePath 参数"));
    }

    let mut ftp = ftp_connect(&params)?;

    let buf = ftp.retr_as_buffer(&remote_path)
        .map_err(|e| crate::value::error_value(format!(
            "FTP 下载失败: {} (远程路径: {})", e, remote_path,
        )))?;

    let data = buf.into_inner();

    ftp.quit().ok();

    std::fs::write(&local_path, &data)
        .map_err(|e| crate::value::error_value(format!(
            "ftpDownload() 写入本地文件 '{}' 失败: {}", local_path, e,
        )))?;

    Ok(Value::Undefined)
}

fn bi_ftp_download_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ftp_params(args)?;
    let remote_path = get_switch(args, "remotePath", "");

    if remote_path.is_empty() {
        return Err(crate::value::error_value("ftpDownloadBytes() 需要 --remotePath 参数"));
    }

    let mut ftp = ftp_connect(&params)?;

    let buf = ftp.retr_as_buffer(&remote_path)
        .map_err(|e| crate::value::error_value(format!(
            "FTP 下载失败: {} (远程路径: {})", e, remote_path,
        )))?;

    let data = buf.into_inner();

    ftp.quit().ok();
    Ok(Value::Bytes(Arc::new(data)))
}

fn bi_ftp_create_dir(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ftp_params(args)?;
    let remote_path = get_switch(args, "remotePath", "");

    if remote_path.is_empty() {
        return Err(crate::value::error_value("ftpCreateDir() 需要 --remotePath 参数"));
    }

    let mut ftp = ftp_connect(&params)?;

    ftp.mkdir(&remote_path)
        .map_err(|e| crate::value::error_value(format!(
            "FTP 创建目录失败: {} (路径: {})", e, remote_path,
        )))?;

    ftp.quit().ok();
    Ok(Value::Undefined)
}

fn bi_ftp_remove_file(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ftp_params(args)?;
    let remote_path = get_switch(args, "remotePath", "");

    if remote_path.is_empty() {
        return Err(crate::value::error_value("ftpRemoveFile() 需要 --remotePath 参数"));
    }

    let mut ftp = ftp_connect(&params)?;

    ftp.rm(&remote_path)
        .map_err(|e| crate::value::error_value(format!(
            "FTP 删除文件失败: {} (路径: {})", e, remote_path,
        )))?;

    ftp.quit().ok();
    Ok(Value::Undefined)
}

fn bi_ftp_size(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ftp_params(args)?;
    let remote_path = get_switch(args, "remotePath", "");

    if remote_path.is_empty() {
        return Err(crate::value::error_value("ftpSize() 需要 --remotePath 参数"));
    }

    let mut ftp = ftp_connect(&params)?;

    let size = ftp.size(&remote_path)
        .map_err(|e| crate::value::error_value(format!(
            "FTP 获取大小失败: {} (路径: {})", e, remote_path,
        )))?;

    ftp.quit().ok();
    Ok(Value::Int(size as i64))
}
/// bi_ftp_create_file 在远程创建文件（带内容）。
///
/// 用法：ftpCreateFile("--host=...", "--user=...", "--password=...",
///                    "--remotePath=/pub/readme.txt", "--content=文件内容")
fn bi_ftp_create_file(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ftp_params(args)?;
    let remote_path = get_switch(args, "remotePath", "");
    let content = get_switch(args, "content", "");

    if remote_path.is_empty() {
        return Err(crate::value::error_value("ftpCreateFile() 需要 --remotePath 参数"));
    }

    let mut ftp = ftp_connect(&params)?;

    ftp.put_file(&remote_path, &mut std::io::Cursor::new(content.into_bytes()))
        .map_err(|e| crate::value::error_value(format!(
            "FTP 创建文件失败: {} (路径: {})", e, remote_path,
        )))?;

    ftp.quit().ok();
    Ok(Value::Undefined)
}

/// bi_ftp_upload_bytes 上传 bytes/byteArray 到远程。
///
/// 用法：ftpUploadBytes("--host=...", "--user=...", "--password=...",
///                    "--remotePath=/pub/data.bin", dataBytes)
/// 最后一个参数是要上传的 bytes/byteArray。
fn bi_ftp_upload_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ftp_params(args)?;
    let remote_path = get_switch(args, "remotePath", "");

    if remote_path.is_empty() {
        return Err(crate::value::error_value("ftpUploadBytes() 需要 --remotePath 参数"));
    }

    // 找 bytes 参数
    let data = match args.iter().rev().find(|arg| matches!(arg, Value::Bytes(_) | Value::ByteArray(_))) {
        Some(Value::Bytes(b)) => b.as_ref().to_vec(),
        Some(Value::ByteArray(b)) => b.lock().unwrap().clone(),
        _ => return Err(crate::value::error_value("ftpUploadBytes() 需要 bytes/byteArray 参数")),
    };

    let mut ftp = ftp_connect(&params)?;

    ftp.put_file(&remote_path, &mut std::io::Cursor::new(data))
        .map_err(|e| crate::value::error_value(format!(
            "FTP 上传失败: {} (路径: {})", e, remote_path,
        )))?;

    ftp.quit().ok();
    Ok(Value::Undefined)
}
