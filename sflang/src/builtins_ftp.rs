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

use crate::function::BuiltinDoc;
use crate::value::Value;
use crate::vm::VM;

// ---- FTP 函数文档 ----

static DOC_FTP_LIST: BuiltinDoc = BuiltinDoc {
    category: "ftp",
    signature: "ftpList(\"--host=...\", \"--user=...\", \"--password=...\", \"--remotePath=/dir\") -> array<string>",
    summary: "列出 FTP 远程目录内容（返回原始 LIST 命令输出行）。",
    params: &[
        ("--host", "FTP 服务器地址（必填）"),
        ("--port", "FTP 端口，默认 21"),
        ("--user", "登录用户名，默认 anonymous"),
        ("--password", "登录密码（匿名可留空）"),
        ("--remotePath", "要列出的远程目录，默认 /"),
    ],
    returns: "array<string>：LIST 命令返回的每行（含权限、大小、文件名等原始格式）；失败返回 error",
    examples: &[
        "ftpList(\"--host=ftp.example.com\", \"--user=u\", \"--password=p\", \"--remotePath=/pub\")",
        "// → [\"-rw-r--r-- 1 owner group 1024 Jan 01 file.txt\", \"drwxr-xr-x 2 owner group 4096 Jan 01 subdir\"]",
        "ftpList(\"--host=ftp.example.com\", \"--remotePath=/incoming\")  // 匿名登录",
    ],
    errors: &[
        "FTP 连接失败：网络不通 / 端口错误",
        "FTP 登录失败：用户名或密码错误",
        "列目录失败：路径不存在 / 权限不足",
    ],
};

static DOC_FTP_UPLOAD: BuiltinDoc = BuiltinDoc {
    category: "ftp",
    signature: "ftpUpload(\"--host=...\", \"--user=...\", \"--password=...\", \"--localPath=...\", \"--remotePath=...\") -> undefined",
    summary: "将本地文件上传到 FTP 远程主机。",
    params: &[
        ("--host/--port/--user/--password", "连接与认证参数"),
        ("--localPath", "本地源文件路径（必填）"),
        ("--remotePath", "远程目标文件路径（必填）"),
    ],
    returns: "undefined：上传成功；失败返回 error",
    examples: &[
        "ftpUpload(\"--host=ftp.example.com\", \"--user=u\", \"--password=p\", \"--localPath=./report.pdf\", \"--remotePath=/pub/report.pdf\")",
    ],
    errors: &[
        "读取本地文件失败：路径不存在 / 权限不足",
        "FTP 上传失败：远程父目录不存在 / 权限不足 / 磁盘满",
        "缺少 --localPath 或 --remotePath 参数",
    ],
};

static DOC_FTP_DOWNLOAD: BuiltinDoc = BuiltinDoc {
    category: "ftp",
    signature: "ftpDownload(\"--host=...\", \"--user=...\", \"--password=...\", \"--remotePath=...\", \"--localPath=...\") -> undefined",
    summary: "将 FTP 远程文件下载到本地磁盘。",
    params: &[
        ("--host/--port/--user/--password", "连接与认证参数"),
        ("--remotePath", "远程源文件路径（必填）"),
        ("--localPath", "本地目标文件路径（必填）"),
    ],
    returns: "undefined：下载成功；失败返回 error",
    examples: &[
        "ftpDownload(\"--host=ftp.example.com\", \"--user=u\", \"--password=p\", \"--remotePath=/pub/data.csv\", \"--localPath=./data.csv\")",
    ],
    errors: &[
        "FTP 下载失败：远程文件不存在 / 权限不足",
        "写入本地文件失败：路径不可写 / 磁盘满",
        "缺少 --remotePath 或 --localPath 参数",
    ],
};

static DOC_FTP_DOWNLOAD_BYTES: BuiltinDoc = BuiltinDoc {
    category: "ftp",
    signature: "ftpDownloadBytes(\"--host=...\", \"--user=...\", \"--password=...\", \"--remotePath=/f\") -> bytes",
    summary: "将 FTP 远程文件下载到内存 bytes（不落本地磁盘）。",
    params: &[
        ("--host/--port/--user/--password", "连接与认证参数"),
        ("--remotePath", "远程源文件路径（必填）"),
    ],
    returns: "bytes：文件全部内容；失败返回 error",
    examples: &[
        "var data = ftpDownloadBytes(\"--host=ftp.example.com\", \"--user=u\", \"--password=p\", \"--remotePath=/pub/data.bin\")",
        "fileWriteBytes(\"./local.bin\", ftpDownloadBytes(\"--host=h\", \"--user=u\", \"--password=p\", \"--remotePath=/tmp/x\"))",
    ],
    errors: &[
        "FTP 下载失败：远程文件不存在 / 权限不足",
        "缺少 --remotePath 参数",
    ],
};

static DOC_FTP_CREATE_DIR: BuiltinDoc = BuiltinDoc {
    category: "ftp",
    signature: "ftpCreateDir(\"--host=...\", \"--user=...\", \"--password=...\", \"--remotePath=/dir\") -> undefined",
    summary: "在 FTP 远程主机创建单个目录（非递归，父目录须存在）。",
    params: &[
        ("--host/--port/--user/--password", "连接与认证参数"),
        ("--remotePath", "要创建的远程目录路径（必填）"),
    ],
    returns: "undefined：创建成功；失败返回 error",
    examples: &[
        "ftpCreateDir(\"--host=ftp.example.com\", \"--user=u\", \"--password=p\", \"--remotePath=/pub/newdir\")",
    ],
    errors: &[
        "FTP 创建目录失败：父目录不存在 / 权限不足 / 目录已存在",
        "缺少 --remotePath 参数",
    ],
};

static DOC_FTP_REMOVE_FILE: BuiltinDoc = BuiltinDoc {
    category: "ftp",
    signature: "ftpRemoveFile(\"--host=...\", \"--user=...\", \"--password=...\", \"--remotePath=/f\") -> undefined",
    summary: "删除 FTP 远程文件（DELE 命令，仅文件不能删目录）。",
    params: &[
        ("--host/--port/--user/--password", "连接与认证参数"),
        ("--remotePath", "要删除的远程文件路径（必填）"),
    ],
    returns: "undefined：删除成功；失败返回 error",
    examples: &[
        "ftpRemoveFile(\"--host=ftp.example.com\", \"--user=u\", \"--password=p\", \"--remotePath=/pub/old.log\")",
    ],
    errors: &[
        "FTP 删除失败：文件不存在 / 权限不足 / 路径是目录（用 RMD 删目录需自行扩展）",
        "缺少 --remotePath 参数",
    ],
};

static DOC_FTP_SIZE: BuiltinDoc = BuiltinDoc {
    category: "ftp",
    signature: "ftpSize(\"--host=...\", \"--user=...\", \"--password=...\", \"--remotePath=/f\") -> int",
    summary: "获取 FTP 远程文件大小（字节，SIZE 命令）。",
    params: &[
        ("--host/--port/--user/--password", "连接与认证参数"),
        ("--remotePath", "远程文件路径（必填）"),
    ],
    returns: "int：文件字节数；失败返回 error",
    examples: &[
        "var sz = ftpSize(\"--host=ftp.example.com\", \"--user=u\", \"--password=p\", \"--remotePath=/pub/data.tar.gz\")  // → 1048576",
    ],
    errors: &[
        "FTP 获取大小失败：文件不存在 / 权限不足 / 路径是目录",
        "部分老 FTP 服务器不支持 SIZE 命令",
        "缺少 --remotePath 参数",
    ],
};

static DOC_FTP_CREATE_FILE: BuiltinDoc = BuiltinDoc {
    category: "ftp",
    signature: "ftpCreateFile(\"--host=...\", \"--user=...\", \"--password=...\", \"--remotePath=/f\", \"--content=...\") -> undefined",
    summary: "在 FTP 远程主机创建文件并写入指定内容。",
    params: &[
        ("--host/--port/--user/--password", "连接与认证参数"),
        ("--remotePath", "远程目标文件路径（必填）"),
        ("--content", "文件内容字符串（默认空串，创建空文件）"),
    ],
    returns: "undefined：创建成功；失败返回 error",
    examples: &[
        "ftpCreateFile(\"--host=ftp.example.com\", \"--user=u\", \"--password=p\", \"--remotePath=/pub/readme.txt\", \"--content=Hello FTP\")",
        "ftpCreateFile(\"--host=h\", \"--user=u\", \"--password=p\", \"--remotePath=/pub/empty\", \"--content=\")  // 空文件",
    ],
    errors: &[
        "FTP 创建文件失败：远程父目录不存在 / 权限不足 / 磁盘满",
        "缺少 --remotePath 参数",
    ],
};

static DOC_FTP_UPLOAD_BYTES: BuiltinDoc = BuiltinDoc {
    category: "ftp",
    signature: "ftpUploadBytes(\"--host=...\", \"--user=...\", \"--password=...\", \"--remotePath=/f\", dataBytes) -> undefined",
    summary: "将 bytes/byteArray 内存数据上传到 FTP 远程文件。",
    params: &[
        ("--host/--port/--user/--password", "连接与认证参数"),
        ("--remotePath", "远程目标文件路径（必填）"),
        ("dataBytes", "要上传的 bytes 或 byteArray（最后一个非 -- 开头的参数）"),
    ],
    returns: "undefined：上传成功；失败返回 error",
    examples: &[
        "ftpUploadBytes(\"--host=ftp.example.com\", \"--user=u\", \"--password=p\", \"--remotePath=/pub/data.bin\", fileReadBytes(\"./local.bin\"))",
    ],
    errors: &[
        "缺少 bytes/byteArray 参数（最后一个非 -- 开头的参数）",
        "FTP 上传失败：远程路径无效 / 权限不足",
        "缺少 --remotePath 参数",
    ],
};

pub fn register(vm: &mut VM) {
    vm.register_builtin_doc("ftpList", bi_ftp_list, &DOC_FTP_LIST);
    vm.register_builtin_doc("ftpUpload", bi_ftp_upload, &DOC_FTP_UPLOAD);
    vm.register_builtin_doc("ftpDownload", bi_ftp_download, &DOC_FTP_DOWNLOAD);
    vm.register_builtin_doc("ftpDownloadBytes", bi_ftp_download_bytes, &DOC_FTP_DOWNLOAD_BYTES);
    vm.register_builtin_doc("ftpCreateDir", bi_ftp_create_dir, &DOC_FTP_CREATE_DIR);
    vm.register_builtin_doc("ftpRemoveFile", bi_ftp_remove_file, &DOC_FTP_REMOVE_FILE);
    vm.register_builtin_doc("ftpSize", bi_ftp_size, &DOC_FTP_SIZE);
    vm.register_builtin_doc("ftpCreateFile", bi_ftp_create_file, &DOC_FTP_CREATE_FILE);
    vm.register_builtin_doc("ftpUploadBytes", bi_ftp_upload_bytes, &DOC_FTP_UPLOAD_BYTES);
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
