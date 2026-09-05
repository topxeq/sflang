//! builtins_ssh.rs — SSH 客户端内置函数（基于 russh + russh-sftp）
//!
//! 纯 Rust SSH 客户端，对标 Charlang 的 ssh* 函数。
//! 文件传输用 SFTP 子系统（原生协议，高效可靠）。
//! 支持密码认证和私钥认证。
//! 支持 PTY 交互式终端（sshShell* 系列）。
//!
//! 函数：
//!   sshRun      — 执行远程命令
//!   sshList     — 列出远程目录内容
//!   sshUpload   — SFTP 上传文件
//!   sshDownload — SFTP 下载文件
//!   sshMkdir    — 创建远程目录
//!   sshRemove   — 删除远程文件或目录
//!   sshMove     — 移动/重命名远程文件
//!   sshShell*   — PTY 交互式终端（Open/Write/Resize/Close/Keepalive）

use std::sync::Arc;

use crate::function::BuiltinDoc;
use crate::value::Value;
use crate::vm::VM;

// ---- SSH 函数文档 ----

static DOC_SSH_RUN: BuiltinDoc = BuiltinDoc {
    category: "ssh",
    signature: "sshRun(\"--host=...\", \"--user=...\", \"--password=...\", command, opts...) -> string",
    summary: "在远程主机执行 shell 命令，返回 stdout 输出（含 stderr）。",
    params: &[
        ("--host", "远程主机地址（必填）"),
        ("--user", "登录用户名（必填）"),
        ("--password", "密码认证（与 --key 二选一）"),
        ("--key", "私钥文件路径（与 --password 二选一）"),
        ("--keyPassphrase", "私钥口令（可选，私钥加密时）"),
        ("--port", "SSH 端口，默认 22"),
        ("--cmdTimeout", "命令超时秒数，默认 0（无超时）"),
        ("command", "要执行的 shell 命令（非 -- 开头的字符串参数）"),
    ],
    returns: "string：命令的标准输出（含合并的 stderr）；失败返回 error",
    examples: &[
        "sshRun(\"--host=10.0.0.1\", \"--user=root\", \"--password=secret\", \"uname -a\")",
        "sshRun(\"--host=10.0.0.1\", \"--user=app\", \"--key=/home/app/.ssh/id_rsa\", \"ls -la /var/log\")",
        "sshRun(\"--host=10.0.0.1\", \"--user=app\", \"--password=secret\", \"--cmdTimeout=10\", \"sleep 999\")",
    ],
    errors: &[
        "SSH 连接失败：网络不通 / 端口错误（返回 error 而非抛异常）",
        "认证失败：密码或私钥被拒绝",
        "缺少 --host / --user 参数；未提供 --password 或 --key",
        "命令超时（--cmdTimeout 到期）",
    ],
};

static DOC_SSH_LIST: BuiltinDoc = BuiltinDoc {
    category: "ssh",
    signature: "sshList(\"--host=...\", \"--user=...\", \"--password=...\", \"--remotePath=/dir\") -> array<string>",
    summary: "通过 SFTP 列出远程目录下的文件名（不含子目录递归）。",
    params: &[
        ("--host/--user/--password", "认证参数（同 sshRun）"),
        ("--remotePath", "要列出的远程目录路径，默认 /"),
    ],
    returns: "array<string>：目录下条目的文件名；失败返回 error",
    examples: &[
        "sshList(\"--host=10.0.0.1\", \"--user=root\", \"--password=secret\", \"--remotePath=/var/log\")  // → [\"syslog\",\"auth.log\"]",
        "sshList(\"--host=10.0.0.1\", \"--user=app\", \"--password=secret\", \"--remotePath=/home/app\")",
    ],
    errors: &[
        "SFTP 读取目录失败：路径不存在 / 权限不足",
        "认证 / 连接失败（同 sshRun）",
    ],
};

static DOC_SSH_UPLOAD: BuiltinDoc = BuiltinDoc {
    category: "ssh",
    signature: "sshUpload(\"--host=...\", \"--user=...\", \"--password=...\", \"--localPath=...\", \"--remotePath=...\" [, \"--append\"]) -> undefined",
    summary: "用 SFTP 将本地文件上传到远程主机。默认覆盖写入；指定 --append 时追加到远程文件末尾（文件不存在则创建）。",
    params: &[
        ("--host/--user/--password", "认证参数"),
        ("--localPath", "本地源文件路径（必填）"),
        ("--remotePath", "远程目标文件路径（必填）"),
        ("--append", "可选开关。追加模式：数据写入远程文件末尾而非覆盖"),
    ],
    returns: "undefined：上传成功；失败返回 error",
    examples: &[
        "sshUpload(\"--host=10.0.0.1\", \"--user=app\", \"--password=secret\", \"--localPath=./config.cfg\", \"--remotePath=/etc/app/config.cfg\")",
        "sshUpload(\"--host=10.0.0.1\", \"--user=root\", \"--key=/home/u/.ssh/id\", \"--localPath=C:\\\\data.zip\", \"--remotePath=/tmp/data.zip\")",
    ],
    errors: &[
        "读取本地文件失败：路径不存在 / 权限不足",
        "SFTP 创建 / 写入失败：远程路径父目录不存在 / 权限不足",
        "缺少 --localPath 或 --remotePath 参数",
    ],
};

static DOC_SSH_DOWNLOAD: BuiltinDoc = BuiltinDoc {
    category: "ssh",
    signature: "sshDownload(\"--host=...\", \"--user=...\", \"--password=...\", \"--remotePath=...\", \"--localPath=...\") -> undefined",
    summary: "用 SFTP 将远程文件下载到本地。",
    params: &[
        ("--host/--user/--password", "认证参数"),
        ("--remotePath", "远程源文件路径（必填）"),
        ("--localPath", "本地目标文件路径（必填）"),
    ],
    returns: "undefined：下载成功；失败返回 error",
    examples: &[
        "sshDownload(\"--host=10.0.0.1\", \"--user=app\", \"--password=secret\", \"--remotePath=/var/log/app.log\", \"--localPath=./app.log\")",
    ],
    errors: &[
        "SFTP 读取失败：远程文件不存在 / 权限不足",
        "写入本地文件失败：路径不可写 / 磁盘满",
        "缺少 --remotePath 或 --localPath 参数",
    ],
};

static DOC_SSH_MKDIR: BuiltinDoc = BuiltinDoc {
    category: "ssh",
    signature: "sshMkdir(\"--host=...\", \"--user=...\", \"--password=...\", \"--remotePath=/dir\") -> undefined",
    summary: "用 SFTP 在远程创建单个目录（父目录必须存在，非递归）。",
    params: &[
        ("--host/--user/--password", "认证参数"),
        ("--remotePath", "要创建的远程目录路径（必填）"),
    ],
    returns: "undefined：创建成功；失败返回 error",
    examples: &[
        "sshMkdir(\"--host=10.0.0.1\", \"--user=root\", \"--password=secret\", \"--remotePath=/opt/newapp\")",
    ],
    errors: &[
        "SFTP 创建目录失败：父目录不存在（如需递归用 sshEnsureMakeDirs）/ 权限不足 / 目录已存在",
        "缺少 --remotePath 参数",
    ],
};

static DOC_SSH_REMOVE: BuiltinDoc = BuiltinDoc {
    category: "ssh",
    signature: "sshRemove(\"--host=...\", \"--user=...\", \"--password=...\", \"--remotePath=/path\") -> undefined",
    summary: "删除远程文件或目录（自动尝试先删文件再删目录）。",
    params: &[
        ("--host/--user/--password", "认证参数"),
        ("--remotePath", "要删除的远程文件或目录路径（必填）"),
    ],
    returns: "undefined：删除成功；失败返回 error",
    examples: &[
        "sshRemove(\"--host=10.0.0.1\", \"--user=root\", \"--password=secret\", \"--remotePath=/tmp/old.log\")",
        "sshRemove(\"--host=10.0.0.1\", \"--user=root\", \"--password=secret\", \"--remotePath=/opt/emptydir\")",
    ],
    errors: &[
        "SFTP 删除失败：路径不存在 / 权限不足 / 目录非空",
        "内部先尝试 remove_file 再 remove_dir，两者都失败才报错",
    ],
};

static DOC_SSH_MOVE: BuiltinDoc = BuiltinDoc {
    category: "ssh",
    signature: "sshMove(\"--host=...\", \"--user=...\", \"--password=...\", \"--remotePath=/a\", \"--targetPath=/b\") -> undefined",
    summary: "移动或重命名远程文件 / 目录（SFTP rename）。",
    params: &[
        ("--host/--user/--password", "认证参数"),
        ("--remotePath", "源路径（必填）"),
        ("--targetPath", "目标路径（必填）"),
    ],
    returns: "undefined：移动成功；失败返回 error",
    examples: &[
        "sshMove(\"--host=10.0.0.1\", \"--user=root\", \"--password=secret\", \"--remotePath=/tmp/a.log\", \"--targetPath=/var/log/a.log\")",
        "sshMove(\"--host=10.0.0.1\", \"--user=u\", \"--password=p\", \"--remotePath=/old/name\", \"--targetPath=/new/name\")  // 重命名",
    ],
    errors: &[
        "SFTP 移动失败：源路径不存在 / 目标路径已存在 / 跨文件系统 / 权限不足",
        "缺少 --remotePath 或 --targetPath 参数",
    ],
};

static DOC_SSH_SYNC: BuiltinDoc = BuiltinDoc {
    category: "ssh",
    signature: "sshSync(\"--host=...\", \"--localPath=...\", \"--remotePath=...\", \"--direction=push|pull\", opts...) -> array<string>",
    summary: "在本地与远程之间同步目录（push 本地→远程，pull 远程→本地）。",
    params: &[
        ("--host/--user/--password", "认证参数"),
        ("--localPath", "本地目录（必填）"),
        ("--remotePath", "远程目录（必填）"),
        ("--direction", "方向：push（默认）或 pull"),
        ("--recursive", "开关：递归同步子目录"),
        ("--delete", "开关：删除目标侧多余的文件（镜像同步）"),
        ("--dryRun", "开关：只输出将执行的操作，不实际写入"),
    ],
    returns: "array<string>：同步操作日志（如 PUT local → remote (N bytes)、DEL path）",
    examples: &[
        "sshSync(\"--host=10.0.0.1\", \"--user=app\", \"--password=secret\", \"--localPath=./www\", \"--remotePath=/var/www\", \"--direction=push\", \"--recursive\", \"--delete\")",
        "sshSync(\"--host=h\", \"--user=u\", \"--password=p\", \"--localPath=./bak\", \"--remotePath=/data\", \"--direction=pull\", \"--dryRun\")  // 预演",
    ],
    errors: &[
        "--direction 只支持 push 或 pull，其他值返回 error",
        "读取本地 / 远程目录失败：路径不存在 / 权限不足",
        "缺少 --localPath 或 --remotePath 参数",
    ],
};

static DOC_SSH_CREATE_FILE: BuiltinDoc = BuiltinDoc {
    category: "ssh",
    signature: "sshCreateFile(\"--host=...\", \"--user=...\", \"--password=...\", \"--remotePath=/f\", \"--content=...\") -> undefined",
    summary: "在远程创建文件并写入指定内容（通过 SFTP）。",
    params: &[
        ("--host/--user/--password", "认证参数"),
        ("--remotePath", "远程目标文件路径（必填）"),
        ("--content", "文件内容字符串（默认空串）"),
    ],
    returns: "undefined：创建成功；失败返回 error",
    examples: &[
        "sshCreateFile(\"--host=10.0.0.1\", \"--user=root\", \"--password=secret\", \"--remotePath=/etc/myapp.conf\", \"--content=port=8080\\nlog=info\")",
        "sshCreateFile(\"--host=h\", \"--user=u\", \"--password=p\", \"--remotePath=/tmp/empty.txt\", \"--content=\")  // 创建空文件",
    ],
    errors: &[
        "SFTP 创建 / 写入失败：远程父目录不存在 / 权限不足",
        "缺少 --remotePath 参数",
    ],
};

static DOC_SSH_UPLOAD_BYTES: BuiltinDoc = BuiltinDoc {
    category: "ssh",
    signature: "sshUploadBytes(data, \"--host=...\", \"--user=...\", \"--password=...\", \"--remotePath=/f\" [, \"--append\"]) -> undefined",
    summary: "用 SFTP 将内存数据上传到远程文件。数据可为 bytes/byteArray/string（string 按 UTF-8 编码）。默认覆盖写入；指定 --append 时追加到文件末尾（文件不存在则创建）。",
    params: &[
        ("--host/--user/--password", "认证参数"),
        ("--remotePath", "远程目标文件路径（必填）"),
        ("--append", "可选开关。追加模式：数据写入远程文件末尾而非覆盖"),
        ("data", "要上传的 bytes/byteArray/string（最后一个非开关参数；以 - 开头的字符串视为开关参数）"),
    ],
    returns: "undefined：上传成功；失败返回 error",
    examples: &[
        "sshUploadBytes(\"--host=10.0.0.1\", \"--user=app\", \"--password=secret\", \"--remotePath=/tmp/data.bin\", fileReadBytes(\"./local.bin\"))",
        "sshUploadBytes(getNowStr() + \"\\n\", \"--host=h\", \"--user=u\", \"--password=p\", \"--remotePath=/var/log/app.log\", \"--append\")  // 直接上传字符串并追加一行",
    ],
    errors: &[
        "缺少数据参数（bytes/byteArray/string，最后一个非开关参数）",
        "SFTP 创建 / 写入失败：远程路径无效 / 权限不足",
        "缺少 --remotePath 参数",
    ],
};

static DOC_SSH_DOWNLOAD_BYTES: BuiltinDoc = BuiltinDoc {
    category: "ssh",
    signature: "sshDownloadBytes(\"--host=...\", \"--user=...\", \"--password=...\", \"--remotePath=/f\") -> bytes",
    summary: "用 SFTP 下载远程文件到内存 bytes（不落本地磁盘）。",
    params: &[
        ("--host/--user/--password", "认证参数"),
        ("--remotePath", "远程源文件路径（必填）"),
    ],
    returns: "bytes：文件全部内容的字节串；失败返回 error",
    examples: &[
        "var data = sshDownloadBytes(\"--host=10.0.0.1\", \"--user=app\", \"--password=secret\", \"--remotePath=/tmp/data.bin\")",
        "fileWriteBytes(\"./local.bin\", sshDownloadBytes(\"--host=h\", \"--user=u\", \"--password=p\", \"--remotePath=/tmp/x\"))",
    ],
    errors: &[
        "SFTP 读取失败：远程文件不存在 / 权限不足",
        "缺少 --remotePath 参数",
    ],
};

static DOC_SSH_IF_FILE_EXISTS: BuiltinDoc = BuiltinDoc {
    category: "ssh",
    signature: "sshIfFileExists(\"--host=...\", \"--user=...\", \"--password=...\", \"--remotePath=/path\") -> bool",
    summary: "检查远程文件或目录是否存在（通过 SFTP stat）。",
    params: &[
        ("--host/--user/--password", "认证参数"),
        ("--remotePath", "要检查的远程路径（必填）"),
    ],
    returns: "bool：true 存在（文件或目录），false 不存在；连接失败返回 error",
    examples: &[
        "if (sshIfFileExists(\"--host=10.0.0.1\", \"--user=u\", \"--password=p\", \"--remotePath=/etc/myapp.conf\")) { ... }",
    ],
    errors: &[
        "连接 / 认证失败返回 error（与文件不存在的 false 区分）",
        "缺少 --remotePath 参数",
    ],
};

static DOC_SSH_GET_FILE_INFO: BuiltinDoc = BuiltinDoc {
    category: "ssh",
    signature: "sshGetFileInfo(\"--host=...\", \"--user=...\", \"--password=...\", \"--remotePath=/path\") -> map",
    summary: "获取远程文件信息（大小、修改时间、是否目录等）。",
    params: &[
        ("--host/--user/--password", "认证参数"),
        ("--remotePath", "远程文件或目录路径（必填）"),
    ],
    returns: "map：{size:int, mtime:int, atime:int, isDir:bool, isFile:bool, isSymlink:bool}；文件不存在返回 error",
    examples: &[
        "var info = sshGetFileInfo(\"--host=10.0.0.1\", \"--user=u\", \"--password=p\", \"--remotePath=/var/log/syslog\")",
        "if (info[\"isDir\"]) { ... }",
        "println(info[\"size\"])  // 文件字节数",
    ],
    errors: &[
        "SFTP 获取文件信息失败：文件不存在 / 权限不足",
        "size/mtime/atime 可能为 0（服务器未返回时）",
        "缺少 --remotePath 参数",
    ],
};

static DOC_SSH_ENSURE_MAKE_DIRS: BuiltinDoc = BuiltinDoc {
    category: "ssh",
    signature: "sshEnsureMakeDirs(\"--host=...\", \"--user=...\", \"--password=...\", \"--remotePath=/a/b/c\") -> undefined",
    summary: "递归创建远程目录（类似 mkdir -p），已存在的目录跳过。",
    params: &[
        ("--host/--user/--password", "认证参数"),
        ("--remotePath", "要递归创建的远程目录路径（必填）"),
    ],
    returns: "undefined：创建成功；失败返回 error",
    examples: &[
        "sshEnsureMakeDirs(\"--host=10.0.0.1\", \"--user=root\", \"--password=secret\", \"--remotePath=/opt/myapp/logs/2024\")",
        "sshEnsureMakeDirs(\"--host=h\", \"--user=u\", \"--password=p\", \"--remotePath=/a/b/c\")  // 逐级创建",
    ],
    errors: &[
        "与 sshMkdir 区别：本函数递归创建父目录（mkdir -p 语义）",
        "中间某级创建失败：权限不足 / 已存在同名文件（非目录）",
        "缺少 --remotePath 参数",
    ],
};

static DOC_SSH_JOIN_PATH: BuiltinDoc = BuiltinDoc {
    category: "ssh",
    signature: "sshJoinPath(base, sub) -> string",
    summary: "拼接远程路径（固定用 / 分隔符，自动处理重复或缺失的斜杠）。",
    params: &[
        ("base", "基础路径（字符串）"),
        ("sub", "要拼接的子路径（字符串）"),
    ],
    returns: "string：拼接后的路径（智能处理首尾斜杠）",
    examples: &[
        "sshJoinPath(\"/home/user\", \"data\")        // → \"/home/user/data\"",
        "sshJoinPath(\"/home/user/\", \"/data\")      // → \"/home/user/data\"",
        "sshJoinPath(\"/home/user\", \"sub/dir/\")    // → \"/home/user/sub/dir/\"",
    ],
    errors: &[
        "纯字符串操作，不需要 SSH 连接（速度极快）",
        "参数应为 string 类型，类型不符返回 error",
        "需要 2 个参数 (base, sub)",
    ],
};

static DOC_SSH_LIST_DETAIL: BuiltinDoc = BuiltinDoc {
    category: "ssh",
    signature: "sshListDetail(--host=..., --user=..., --password=..., --remotePath=...) -> array<map>",
    summary: "列出远程目录内容（含元信息：名称/大小/类型/修改时间）。SSH 文件浏览器使用。",
    params: &[
        ("--host", "SSH 主机地址"),
        ("--user", "用户名"),
        ("--password", "密码（或 --key 密钥路径）"),
        ("--remotePath", "远程目录路径，默认 /"),
        ("--port", "可选。端口，默认 22"),
    ],
    returns: "array<map{name,size,isDir,isFile,isSymlink,mtime}>；失败返回 error",
    examples: &[
        "var items = sshListDetail(\"--host=1.2.3.4\", \"--user=root\", \"--password=x\", \"--remotePath=/etc\")",
    ],
    errors: &["连接失败或目录不存在返回 error"],
};

pub fn register(vm: &mut VM) {
    vm.register_builtin_doc("sshRun", bi_ssh_run, &DOC_SSH_RUN);
    vm.register_builtin_doc("sshList", bi_ssh_list, &DOC_SSH_LIST);
    vm.register_builtin_doc("sshUpload", ssh_upload_impl, &DOC_SSH_UPLOAD);
    vm.register_builtin_doc("sshDownload", ssh_download_impl, &DOC_SSH_DOWNLOAD);
    vm.register_builtin_doc("sshMkdir", bi_ssh_mkdir, &DOC_SSH_MKDIR);
    vm.register_builtin_doc("sshRemove", bi_ssh_remove, &DOC_SSH_REMOVE);
    vm.register_builtin_doc("sshMove", bi_ssh_move, &DOC_SSH_MOVE);
    vm.register_builtin_doc("sshSync", bi_ssh_sync, &DOC_SSH_SYNC);
    vm.register_builtin_doc("sshCreateFile", bi_ssh_create_file, &DOC_SSH_CREATE_FILE);
    vm.register_builtin_doc("sshUploadBytes", bi_ssh_upload_bytes, &DOC_SSH_UPLOAD_BYTES);
    vm.register_builtin_doc("sshDownloadBytes", bi_ssh_download_bytes, &DOC_SSH_DOWNLOAD_BYTES);
    vm.register_builtin_doc("sshIfFileExists", bi_ssh_if_file_exists, &DOC_SSH_IF_FILE_EXISTS);
    vm.register_builtin_doc("sshGetFileInfo", bi_ssh_get_file_info, &DOC_SSH_GET_FILE_INFO);
    vm.register_builtin_doc("sshEnsureMakeDirs", bi_ssh_ensure_make_dirs, &DOC_SSH_ENSURE_MAKE_DIRS);
    vm.register_builtin_doc("sshJoinPath", bi_ssh_join_path, &DOC_SSH_JOIN_PATH);
    vm.register_builtin_doc("sshListDetail", bi_ssh_list_detail, &DOC_SSH_LIST_DETAIL);
    // PTY 交互式终端
    vm.register_builtin_doc("sshShellOpen", bi_ssh_shell_open, &DOC_SSH_SHELL_OPEN);
    vm.register_builtin_doc("sshShellWrite", bi_ssh_shell_write, &DOC_SSH_SHELL_WRITE);
    vm.register_builtin_doc("sshShellResize", bi_ssh_shell_resize, &DOC_SSH_SHELL_RESIZE);
    vm.register_builtin_doc("sshShellClose", bi_ssh_shell_close, &DOC_SSH_SHELL_CLOSE);
    vm.register_builtin_doc("sshShellKeepalive", bi_ssh_shell_keepalive, &DOC_SSH_SHELL_KEEPALIVE);
    vm.register_builtin_doc("sshShellStreamId", bi_ssh_shell_stream_id, &DOC_SSH_SHELL_STREAM_ID);
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

/// sftp_write_file 将内存数据写入远程文件（供 sshUpload / sshUploadBytes 共用）。
///
/// append 为 false 时覆盖写入（文件不存在则创建，即 SFTP 的 CREATE|TRUNC|WRITE）。
/// append 为 true 时追加到文件末尾：先查询远程文件当前大小，再以 CREATE|WRITE
/// 方式打开（不截断）并 seek 到末尾写入。
///
/// 说明：SFTPv3 协议虽有 SSH_FXF_APPEND 标志，但 OpenSSH 等主流服务器会忽略它，
/// 因此追加不依赖该标志，用「查大小 + seek」方式保证跨服务器可移植。
async fn sftp_write_file(
    sftp: &russh_sftp::client::SftpSession,
    remote_path: &str,
    data: &[u8],
    append: bool,
) -> Result<(), String> {
    use tokio::io::{AsyncSeekExt, AsyncWriteExt};

    let mut file = if append {
        // 远程文件当前大小；文件不存在或查询失败按 0 处理（随后打开失败会给出确切错误）
        let offset = match sftp.metadata(remote_path).await {
            Ok(meta) => meta.size.unwrap_or(0),
            Err(_) => 0,
        };
        // CREATE|WRITE：不存在则创建，存在则保留原内容（区别于 create 的 TRUNCATE）
        let mut file = sftp.open_with_flags(
            remote_path,
            russh_sftp::protocol::OpenFlags::CREATE | russh_sftp::protocol::OpenFlags::WRITE,
        ).await.map_err(|e| format!("SFTP 打开文件失败: {}", e))?;
        // 定位到原文件末尾，实现追加写入
        file.seek(std::io::SeekFrom::Start(offset)).await
            .map_err(|e| format!("SFTP 定位文件末尾失败 (offset={}): {}", offset, e))?;
        file
    } else {
        sftp.create(remote_path).await
            .map_err(|e| format!("SFTP 创建文件失败: {}", e))?
    };

    file.write_all(data).await
        .map_err(|e| format!("SFTP 写入失败: {}", e))?;
    file.flush().await.ok();
    Ok(())
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

/// bi_ssh_list_detail 列出远程目录内容（含元信息）。
///
/// 用法：sshListDetail("--host=x", "--user=y", "--password=z", "--remotePath=/etc")
/// 返回 array<map{name, size, isDir, isFile, isSymlink, mtime}>
fn bi_ssh_list_detail(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ssh_params(args)?;
    let remote_path = get_switch(args, "remotePath", "/");

    match do_ssh(&params, |handle| async move {
        let sftp = sftp_open(&handle).await?;
        let mut entries: Vec<(String, u64, bool, bool, bool, i64)> = Vec::new();
        let dir = sftp.read_dir(&remote_path).await
            .map_err(|e| format!("SFTP 读取目录失败: {}", e))?;
        for entry in dir {
            let name = entry.file_name();
            let meta = entry.metadata();
            let is_dir = meta.file_type().is_dir();
            let is_file = meta.file_type().is_file();
            let is_symlink = meta.file_type().is_symlink();
            let size = meta.size.unwrap_or(0);
            let mtime = meta.mtime.unwrap_or(0) as i64;
            entries.push((name, size, is_dir, is_file, is_symlink, mtime));
        }
        let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        Ok::<Vec<(String, u64, bool, bool, bool, i64)>, String>(entries)
    }) {
        Ok(items) => {
            let result: Vec<Value> = items.into_iter().map(|(name, size, is_dir, is_file, is_symlink, mtime)| {
                let mut m = crate::ord_map::OrdMap::new();
                m.set("name".to_string(), Value::str_from(name));
                m.set("size".to_string(), Value::Int(size as i64));
                m.set("isDir".to_string(), Value::Bool(is_dir));
                m.set("isFile".to_string(), Value::Bool(is_file));
                m.set("isSymlink".to_string(), Value::Bool(is_symlink));
                m.set("mtime".to_string(), Value::Int(mtime));
                Value::Map(Arc::new(std::sync::Mutex::new(m)))
            }).collect();
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

    // --append：追加模式（数据写到远程文件末尾而非覆盖）
    let append = has_switch(args, "append");

    // 读取本地文件失败按约定返回 error 对象（而非抛出异常），与文档一致
    let file_data = match std::fs::read(&local_path) {
        Ok(data) => data,
        Err(e) => return Ok(crate::value::error_value(format!(
            "sshUpload() 读取本地文件 '{}' 失败: {}", local_path, e,
        ))),
    };

    match do_ssh(&params, |handle| async move {
        let sftp = sftp_open(&handle).await?;
        let result = sftp_write_file(&sftp, &remote_path, &file_data, append).await;
        let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        result
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

    // --append：追加模式（数据写到远程文件末尾而非覆盖）
    let append = has_switch(args, "append");

    // 找数据参数（最后一个满足条件的参数）：
    //   bytes / byteArray 原样上传；string 按 UTF-8 编码后上传。
    //   以 - 开头的字符串视为开关参数（如 --append），不作为数据。
    let data = match args.iter().rev().find(|arg| match arg {
        Value::Bytes(_) | Value::ByteArray(_) => true,
        Value::Str(s) => !s.starts_with('-'),
        _ => false,
    }) {
        Some(Value::Bytes(b)) => b.as_ref().to_vec(),
        Some(Value::ByteArray(b)) => b.lock().unwrap().clone(),
        Some(Value::Str(s)) => s.as_bytes().to_vec(),
        _ => return Ok(crate::value::error_value(
            "sshUploadBytes() 需要 bytes/byteArray/string 数据参数 (string 按 UTF-8 编码；以 - 开头的字符串视为开关参数，不会作为数据)",
        )),
    };

    match do_ssh(&params, |handle| async move {
        let sftp = sftp_open(&handle).await?;
        let result = sftp_write_file(&sftp, &remote_path, &data, append).await;
        let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        result
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

// ===========================================================================
// 文件信息与目录管理
// ===========================================================================

/// bi_ssh_if_file_exists 检查远程文件是否存在（用 SFTP stat）。
///
/// 用法：sshIfFileExists("--host=...", "--user=...", "--password=...", "--remotePath=/path")
/// 返回 bool：true 表示文件或目录存在，false 表示不存在
fn bi_ssh_if_file_exists(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ssh_params(args)?;
    let remote_path = get_switch(args, "remotePath", "");

    if remote_path.is_empty() {
        return Ok(crate::value::error_value("sshIfFileExists() 需要 --remotePath 参数"));
    }

    match do_ssh(&params, |handle| async move {
        let sftp = sftp_open(&handle).await?;
        // metadata 内部调用 SFTP stat，文件不存在时返回错误
        let exists = sftp.metadata(&remote_path).await.is_ok();
        let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        Ok::<bool, String>(exists)
    }) {
        Ok(exists) => Ok(Value::Bool(exists)),
        Err(e) => Ok(crate::value::error_value(e)),
    }
}

/// bi_ssh_get_file_info 获取远程文件信息（大小、修改时间、是否目录等）。
///
/// 用法：sshGetFileInfo("--host=...", "--user=...", "--password=...", "--remotePath=/path")
/// 返回 Map：{size: int, mtime: int, isDir: bool, isFile: bool, isSymlink: bool}
/// 文件不存在时返回 Error
fn bi_ssh_get_file_info(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ssh_params(args)?;
    let remote_path = get_switch(args, "remotePath", "");

    if remote_path.is_empty() {
        return Ok(crate::value::error_value("sshGetFileInfo() 需要 --remotePath 参数"));
    }

    match do_ssh(&params, |handle| async move {
        let sftp = sftp_open(&handle).await?;
        let meta = sftp.metadata(&remote_path).await
            .map_err(|e| format!("SFTP 获取文件信息失败: {} (可能原因：文件不存在、权限不足)", e))?;
        let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        Ok::<russh_sftp::protocol::FileAttributes, String>(meta)
    }) {
        Ok(meta) => {
            let mut map = crate::ord_map::OrdMap::new();
            // size 文件大小（字节），文件不存在时可能为 None
            map.set("size".to_string(), Value::Int(meta.size.unwrap_or(0) as i64));
            // mtime 修改时间（Unix 时间戳，秒）
            map.set("mtime".to_string(), Value::Int(meta.mtime.unwrap_or(0) as i64));
            // atime 访问时间（Unix 时间戳，秒）
            map.set("atime".to_string(), Value::Int(meta.atime.unwrap_or(0) as i64));
            // isDir 是否为目录
            map.set("isDir".to_string(), Value::Bool(meta.file_type().is_dir()));
            // isFile 是否为普通文件
            map.set("isFile".to_string(), Value::Bool(meta.file_type().is_file()));
            // isSymlink 是否为符号链接
            map.set("isSymlink".to_string(), Value::Bool(meta.file_type().is_symlink()));
            Ok(Value::Map(Arc::new(std::sync::Mutex::new(map))))
        }
        Err(e) => Ok(crate::value::error_value(e)),
    }
}

/// bi_ssh_ensure_make_dirs 递归创建远程目录（类似 mkdir -p）。
///
/// 用法：sshEnsureMakeDirs("--host=...", "--user=...", "--password=...", "--remotePath=/a/b/c")
/// 逐级检查并创建不存在的目录，已存在的目录跳过
fn bi_ssh_ensure_make_dirs(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ssh_params(args)?;
    let remote_path = get_switch(args, "remotePath", "");

    if remote_path.is_empty() {
        return Ok(crate::value::error_value("sshEnsureMakeDirs() 需要 --remotePath 参数"));
    }

    match do_ssh(&params, |handle| async move {
        let sftp = sftp_open(&handle).await?;

        // 将路径按 / 分割，逐级创建
        // 如 /a/b/c → ["", "a", "b", "c"]
        let parts: Vec<&str> = remote_path.split('/').collect();
        let mut current = String::new();

        for part in &parts {
            if part.is_empty() {
                // 开头的 / 或连续的 //，保持根路径
                continue;
            }
            // 拼接当前层级路径
            if current.is_empty() {
                current = format!("/{}", part);
            } else {
                current = format!("{}/{}", current, part);
            }

            // 检查当前层级是否存在
            let exists = sftp.metadata(&current).await.is_ok();
            if !exists {
                // 不存在则创建
                sftp.create_dir(&current).await
                    .map_err(|e| format!("SFTP 创建目录 '{}' 失败: {} (可能原因：权限不足、父目录不存在)", current, e))?;
            }
        }

        let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        Ok::<(), String>(())
    }) {
        Ok(()) => Ok(Value::Undefined),
        Err(e) => Ok(crate::value::error_value(e)),
    }
}

/// bi_ssh_join_path 拼接远程路径（固定用 / 分隔符）。
///
/// 用法：sshJoinPath("/home/user", "data") → "/home/user/data"
/// sshJoinPath("/home/user/", "/data") → "/home/user/data"
/// sshJoinPath("/home/user", "sub/dir/") → "/home/user/sub/dir/"
/// 纯字符串操作，不需要 SSH 连接
fn bi_ssh_join_path(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let base = match args.get(0) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(crate::value::error_value(format!(
            "sshJoinPath() 第 1 个参数应为 string (base 路径)，得到 {} (可能原因：参数顺序错误)",
            v.type_name()
        ))),
        None => return Err(crate::value::error_value("sshJoinPath() 需要 2 个参数 (base, sub)")),
    };
    let sub = match args.get(1) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(crate::value::error_value(format!(
            "sshJoinPath() 第 2 个参数应为 string (sub 路径)，得到 {}", v.type_name()
        ))),
        None => return Err(crate::value::error_value("sshJoinPath() 需要 2 个参数 (base, sub)")),
    };

    // 处理 base 末尾和 sub 开头的 /，避免重复
    let result = if sub.is_empty() {
        base
    } else if base.is_empty() {
        sub
    } else {
        let base_has_slash = base.ends_with('/');
        let sub_has_slash = sub.starts_with('/');
        if base_has_slash && sub_has_slash {
            // 两边都有 /，去掉 sub 开头的 /
            format!("{}{}", base, &sub[1..])
        } else if !base_has_slash && !sub_has_slash {
            // 两边都没有 /，补一个
            format!("{}/{}", base, sub)
        } else {
            // 一边有一边没有，直接拼接
            format!("{}{}", base, sub)
        }
    };

    Ok(Value::str_from(result))
}

// ============================================================================
// PTY 交互式终端（sshShell* 系列）
// ============================================================================
//
// 与 sshRun（一次性 exec）不同，PTY 是长连接 + 持续双向数据流：
//   - 用户键盘输入 → sshShellWrite → channel.make_writer → 远程 shell
//   - 远程输出 → Handler::data 回调 → push_stream_event → GUI 事件循环
//     → window.onStreamData(streamId, data, kind, extra) → xterm.js 渲染
//
// 关键设计：
//   1. SshSession 持久化 tokio runtime + russh handle（仿 builtins_db::DatabaseConn）
//   2. PtyHandler 实现 Handler trait 的 data/eof/exit_status 回调，把数据通过
//      push_stream_event 推送（绕开 make_reader，让 Channel 一直存活可随时 resize）
//   3. sshShellWrite 在持久化 runtime 上 block_on（短操作）
//   4. 保活线程周期触发 SSH 协议 keepalive 或执行空命令

// ---- PTY 函数文档 ----

static DOC_SSH_SHELL_OPEN: BuiltinDoc = BuiltinDoc {
    category: "ssh",
    signature: "sshShellOpen(--host=..., --user=..., --password=..., --cols=80, --rows=24, opts...) -> session",
    summary: "建立 SSH 连接 + 申请 PTY + 启动交互式 shell，返回会话对象。",
    params: &[
        ("--host/--user/--password", "认证参数（同 sshRun）"),
        ("--key", "私钥路径（与 --password 二选一）"),
        ("--keyPassphrase", "私钥口令"),
        ("--port", "SSH 端口，默认 22"),
        ("--cols", "终端列数，默认 80"),
        ("--rows", "终端行数，默认 24"),
    ],
    returns: "session 会话对象（用于 sshShellWrite/Resize/Close）；失败返回 error",
    examples: &[
        "sshShellOpen(\"--host=10.0.0.1\", \"--user=root\", \"--password=secret\", \"--cols=120\", \"--rows=40\")",
    ],
    errors: &[
        "SSH 连接失败：网络不通 / 端口错误（返回 error）",
        "认证失败：密码或私钥被拒绝",
        "PTY 申请失败：服务器不允许 PTY（如 SFTP-only 账户）",
    ],
};

static DOC_SSH_SHELL_WRITE: BuiltinDoc = BuiltinDoc {
    category: "ssh",
    signature: "sshShellWrite(session, data) -> undefined",
    summary: "向 PTY 写入用户输入（字节流或字符串）。",
    params: &[
        ("session", "sshShellOpen 返回的会话对象"),
        ("data", "要写入的数据：string 或 bytes"),
    ],
    returns: "undefined；失败返回 error",
    examples: &[
        "sshShellWrite(sess, \"ls -la\\r\")",
        "sshShellWrite(sess, bytes([3]))  // Ctrl+C",
    ],
    errors: &[
        "session 已关闭或无效",
        "网络写入失败",
    ],
};

static DOC_SSH_SHELL_RESIZE: BuiltinDoc = BuiltinDoc {
    category: "ssh",
    signature: "sshShellResize(session, cols, rows) -> undefined",
    summary: "调整远程 PTY 窗口大小（对应 xterm.js 的 onResize）。",
    params: &[
        ("session", "sshShellOpen 返回的会话对象"),
        ("cols", "新列数（int）"),
        ("rows", "新行数（int）"),
    ],
    returns: "undefined；失败返回 error",
    examples: &["sshShellResize(sess, 120, 40)"],
    errors: &[
        "session 已关闭或无效",
        "服务器不支持 window-change（罕见）",
    ],
};

static DOC_SSH_SHELL_CLOSE: BuiltinDoc = BuiltinDoc {
    category: "ssh",
    signature: "sshShellClose(session) -> undefined",
    summary: "关闭 PTY 会话（发送 EOF + disconnect），释放资源。",
    params: &[("session", "sshShellOpen 返回的会话对象")],
    returns: "undefined",
    examples: &["sshShellClose(sess)"],
    errors: &[],
};

static DOC_SSH_SHELL_KEEPALIVE: BuiltinDoc = BuiltinDoc {
    category: "ssh",
    signature: "sshShellKeepalive(session, --interval=30, --cmd=\"\") -> undefined",
    summary: "启动保活线程：默认 SSH 协议级 keepalive；--cmd 非空时额外周期执行该命令。",
    params: &[
        ("session", "sshShellOpen 返回的会话对象"),
        ("--interval", "保活间隔秒数，默认 30；≤0 表示禁用"),
        ("--cmd", "可选的空命令心跳，如 \"echo .\"；空则仅协议级 keepalive"),
    ],
    returns: "undefined",
    examples: &[
        "sshShellKeepalive(sess)                            // 默认 30s 协议级",
        "sshShellKeepalive(sess, \"--interval=60\")           // 60s",
        "sshShellKeepalive(sess, \"--cmd=echo .\")            // 额外执行空命令",
    ],
    errors: &[],
};

static DOC_SSH_SHELL_STREAM_ID: BuiltinDoc = BuiltinDoc {
    category: "ssh",
    signature: "sshShellStreamId(session) -> int",
    summary: "返回会话的流 ID（前端用此区分 onStreamData 的来源）。",
    params: &[("session", "sshShellOpen 返回的会话对象")],
    returns: "int 流 ID",
    examples: &["var sid = sshShellStreamId(sess)"],
    errors: &[],
};

// ---- SshSession 持久化会话对象 ----

/// SshSession PTY 会话，仿 builtins_db::DatabaseConn 持久化 tokio runtime。
///
/// 生命周期：sshShellOpen 创建 → sshShellWrite/Resize 多次调用 → sshShellClose 销毁。
/// 内部持有 russh handle + channel（用于 write/resize）和 runtime（驱动异步事件循环）。
pub struct SshSession {
    /// russh 客户端 handle（用于 disconnect 等）。
    /// Option 允许 close 时 take() 出来 disconnect。
    pub handle: std::sync::Mutex<Option<russh::client::Handle<PtyHandler>>>,
    /// PTY channel（用于 data 写入和 window_change）。
    /// 注意：russh 的 Channel 在 request_shell 后仍可用于 window_change/data 等。
    /// 但 PtyHandler 的 data 回调是 russh 事件循环触发的，与这个 Channel 实例独立。
    /// 这里保留 Channel 主要为了 resize（window_change）。
    pub channel: std::sync::Mutex<Option<russh::Channel<russh::client::Msg>>>,
    /// 持久化 tokio runtime（驱动 russh 事件循环）。
    pub runtime: tokio::runtime::Runtime,
    /// 流 ID（前端 onStreamData 用此区分）。
    pub stream_id: u64,
    /// 是否已关闭（避免重复 close）。
    pub closed: std::sync::atomic::AtomicBool,
}

impl SshSession {
    /// is_closed 检查会话是否已关闭。
    pub fn is_closed(&self) -> bool {
        self.closed.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// mark_closed 标记为已关闭。
    pub fn mark_closed(&self) {
        self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// PtyHandler russh Handler 实现，接收远端 PTY 数据并通过 push_stream_event 推送。
///
/// 与一次性 sshRun 的 SshHandler 不同，PTY 需要持续接收数据。
/// 这里实现 Handler trait 的 data/extended_data/channel_eof/exit_status 方法，
/// 把数据直接推到 STREAM_EVENTS 队列，由 GUI 事件循环 drain 到前端。
struct PtyHandler {
    /// 流 ID（推送事件时标识来源）。
    stream_id: u64,
}

#[async_trait::async_trait]
impl russh::client::Handler for PtyHandler {
    type Error = russh::Error;

    async fn check_server_key(&mut self, _: &russh::keys::key::PublicKey) -> Result<bool, Self::Error> {
        // 接受所有 server key（与 sshRun 一致；生产环境应改用 known_hosts）
        Ok(true)
    }

    /// data 远程 stdout 数据：UTF-8 lossy 转换后推送到流队列。
    async fn data(
        &mut self,
        _channel: russh::ChannelId,
        data: &[u8],
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        let s = String::from_utf8_lossy(data).into_owned();
        crate::builtins_async::push_stream_event(
            self.stream_id,
            Value::str_from(s),
            crate::builtins_async::StreamKind::Data,
        );
        Ok(())
    }

    /// extended_data 远程 stderr 数据：合流到同一 stream（终端约定 stderr 也显示）。
    async fn extended_data(
        &mut self,
        _channel: russh::ChannelId,
        _ext: u32,
        data: &[u8],
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        let s = String::from_utf8_lossy(data).into_owned();
        crate::builtins_async::push_stream_event(
            self.stream_id,
            Value::str_from(s),
            crate::builtins_async::StreamKind::Data,
        );
        Ok(())
    }

    /// channel_eof 远端 EOF（shell 正常退出）。
    async fn channel_eof(
        &mut self,
        _channel: russh::ChannelId,
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        crate::builtins_async::push_stream_event(
            self.stream_id,
            Value::Undefined,
            crate::builtins_async::StreamKind::Eof,
        );
        Ok(())
    }

    /// channel_close 远端关闭通道。
    async fn channel_close(
        &mut self,
        _channel: russh::ChannelId,
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        crate::builtins_async::push_stream_event(
            self.stream_id,
            Value::Undefined,
            crate::builtins_async::StreamKind::Eof,
        );
        Ok(())
    }

    /// exit_status 远端进程退出（携带退出码）。
    async fn exit_status(
        &mut self,
        _channel: russh::ChannelId,
        exit_status: u32,
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        crate::builtins_async::push_stream_event(
            self.stream_id,
            Value::Undefined,
            crate::builtins_async::StreamKind::Exit(exit_status),
        );
        Ok(())
    }
}

/// ssh_session_clone 从 Value 克隆 Arc<SshSession>。
///
/// 用于内置函数内部，把 Native 值还原为强类型 Arc 引用。
fn ssh_session_clone(v: &Value, fn_name: &str) -> Result<Arc<SshSession>, Value> {
    match v {
        Value::Native(n) => {
            // Arc<dyn Any + Send + Sync>::clone() 拿到 Arc<dyn Any+...>，
            // 再 downcast 回 Arc<SshSession>
            let n_clone: Arc<dyn std::any::Any + Send + Sync> = n.clone();
            match Arc::downcast::<SshSession>(n_clone) {
                Ok(s) => Ok(s),
                Err(_) => Err(crate::value::error_value(format!(
                    "{}() 参数应为 SSH 会话对象（由 sshShellOpen 返回）",
                    fn_name
                ))),
            }
        }
        other => Err(crate::value::error_value(format!(
            "{}() 参数应为 SSH 会话对象，得到 {}", fn_name, other.type_name()
        ))),
    }
}

/// bi_ssh_shell_open 建立 PTY 会话。
///
/// 流程：创建持久化 runtime → connect → 认证 → channel_open_session →
///       request_pty（xterm-256color，常用 modes）→ request_shell →
///       把 handle/channel_id 存入 SshSession。
/// PtyHandler 的 data 回调会自动推送流事件。
fn bi_ssh_shell_open(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let params = parse_ssh_params(args)?;
    let cols: u32 = get_switch(args, "cols", "80").parse().unwrap_or(80);
    let rows: u32 = get_switch(args, "rows", "24").parse().unwrap_or(24);
    let stream_id = crate::builtins_async::next_stream_id();

    // 创建持久化 runtime（PTY 长连接需要）
    let runtime = tokio::runtime::Runtime::new().map_err(|e| {
        crate::value::error_value(format!("sshShellOpen() 创建 tokio runtime 失败: {}", e))
    })?;

    let config = Arc::new(russh::client::Config::default());
    let addr = format!("{}:{}", params.host, params.port);

    // 在 runtime 内建立连接 + 认证 + 申请 PTY + 启动 shell
    // 注意：runtime 会被 SshSession 持有，不能 block_on 后 drop。
    // 用 runtime.block_on 完成初始化阶段，然后让 runtime 继续驱动后续事件。
    let (handle, channel) = runtime.block_on(async {
        // 1. 建立连接
        let mut handle = russh::client::connect(
            config,
            addr,
            PtyHandler { stream_id },
        )
        .await
        .map_err(|e| format!("SSH 连接失败: {} (可能原因：网络不通 / 端口错误)", e))?;

        // 2. 认证
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

        // 3. 打开 session channel
        let channel = handle.channel_open_session().await
            .map_err(|e| format!("SSH 打开通道失败: {}", e))?;

        // 4. 申请 PTY（xterm-256color，经典交互模式：ECHO + ISIG + ICANON + OPOST）
        let modes = vec![
            (russh::Pty::ECHO, 1),
            (russh::Pty::ISIG, 1),
            (russh::Pty::ICANON, 1),
            (russh::Pty::ECHOE, 1),
            (russh::Pty::ECHOCTL, 1),
            (russh::Pty::OPOST, 1),
            (russh::Pty::ONLCR, 1),
            (russh::Pty::ICRNL, 1),
            (russh::Pty::TTY_OP_ISPEED, 38400),
            (russh::Pty::TTY_OP_OSPEED, 38400),
        ];
        channel.request_pty(true, "xterm-256color", cols, rows, 0, 0, &modes)
            .await
            .map_err(|e| format!("SSH 申请 PTY 失败: {} (可能原因：服务器不允许 PTY)", e))?;

        // 5. 启动 shell
        channel.request_shell(true).await
            .map_err(|e| format!("SSH 启动 shell 失败: {}", e))?;

        Ok::<(russh::client::Handle<PtyHandler>, russh::Channel<russh::client::Msg>), String>((handle, channel))
    }).map_err(crate::value::error_value)?;

    // 构造 SshSession（runtime 继续 idle 运行，驱动 russh 事件循环）
    // session 是 Arc<SshSession>，直接转为 trait object（不再 Arc::new）
    let session: Arc<SshSession> = Arc::new(SshSession {
        handle: std::sync::Mutex::new(Some(handle)),
        channel: std::sync::Mutex::new(Some(channel)),
        runtime,
        stream_id,
        closed: std::sync::atomic::AtomicBool::new(false),
    });
    Ok(Value::Native(session as Arc<dyn std::any::Any + Send + Sync>))
}

/// bi_ssh_shell_write 向 PTY 写入用户输入。
fn bi_ssh_shell_write(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "sshShellWrite")?;
    bh::require_arg(args, 1, "sshShellWrite")?;
    let session_arc = ssh_session_clone(&args[0], "sshShellWrite")?;
    let data_bytes: Vec<u8> = match &args[1] {
        Value::Str(s) => s.as_bytes().to_vec(),
        Value::Bytes(b) => b.as_ref().to_vec(),
        Value::ByteArray(b) => b.lock().unwrap().clone(),
        other => return Err(crate::value::error_value(format!(
            "sshShellWrite() 第 2 个参数应为 string 或 bytes，得到 {}", other.type_name()
        ))),
    };

    if session_arc.is_closed() {
        return Ok(crate::value::error_value("sshShellWrite() 会话已关闭"));
    }

    let session = &*session_arc;

    // 在持久化 runtime 上 block_on 写入（通过 Channel.data）
    let result = session.runtime.block_on(async {
        let channel_lock = session.channel.lock().unwrap();
        let channel = match channel_lock.as_ref() {
            Some(c) => c,
            None => return Err("会话已关闭".to_string()),
        };
        // russh 0.46 的 Channel::data<R: AsyncRead>：用 Cursor 作为 AsyncRead
        let cursor = std::io::Cursor::new(data_bytes);
        channel.data(cursor).await
            .map_err(|_e| "SSH 写入失败（连接可能已断开）".to_string())?;
        Ok::<(), String>(())
    });

    match result {
        Ok(()) => Ok(Value::Undefined),
        Err(e) => Ok(crate::value::error_value(e)),
    }
}

/// bi_ssh_shell_resize 调整 PTY 窗口大小。
fn bi_ssh_shell_resize(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "sshShellResize")?;
    bh::require_arg(args, 1, "sshShellResize")?;
    bh::require_arg(args, 2, "sshShellResize")?;
    let session_arc = ssh_session_clone(&args[0], "sshShellResize")?;
    let cols: u32 = match &args[1] {
        Value::Int(n) => *n as u32,
        other => return Err(crate::value::error_value(format!(
            "sshShellResize() 第 2 个参数 cols 应为 int，得到 {}", other.type_name()
        ))),
    };
    let rows: u32 = match &args[2] {
        Value::Int(n) => *n as u32,
        other => return Err(crate::value::error_value(format!(
            "sshShellResize() 第 3 个参数 rows 应为 int，得到 {}", other.type_name()
        ))),
    };

    if session_arc.is_closed() {
        return Ok(crate::value::error_value("sshShellResize() 会话已关闭"));
    }

    let session = &*session_arc;

    let result = session.runtime.block_on(async {
        let channel_lock = session.channel.lock().unwrap();
        let channel = match channel_lock.as_ref() {
            Some(c) => c,
            None => return Err("会话已关闭".to_string()),
        };
        // russh 0.46 的 Channel::window_change(cols, rows, pix_w, pix_h)
        channel.window_change(cols, rows, 0, 0).await
            .map_err(|e| format!("SSH window_change 失败: {}", e))?;
        Ok::<(), String>(())
    });

    match result {
        Ok(()) => Ok(Value::Undefined),
        Err(e) => Ok(crate::value::error_value(e)),
    }
}

/// bi_ssh_shell_close 关闭 PTY 会话。
fn bi_ssh_shell_close(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "sshShellClose")?;
    let session_arc = ssh_session_clone(&args[0], "sshShellClose")?;

    if session_arc.is_closed() {
        return Ok(Value::Undefined);  // 已关闭，幂等
    }
    session_arc.mark_closed();

    let session = &*session_arc;
    // 取出 handle，发送 disconnect
    let handle_opt = session.handle.lock().unwrap().take();
    if let Some(handle) = handle_opt {
        let _ = session.runtime.block_on(async {
            let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        });
    }
    Ok(Value::Undefined)
}

/// bi_ssh_shell_keepalive 启动保活线程。
fn bi_ssh_shell_keepalive(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "sshShellKeepalive")?;
    let session_arc = ssh_session_clone(&args[0], "sshShellKeepalive")?;
    let interval: u64 = get_switch(args, "interval", "30").parse().unwrap_or(30);
    let cmd = get_switch(args, "cmd", "");

    if interval == 0 {
        return Ok(Value::Undefined);  // 禁用保活
    }

    let session_clone = session_arc.clone();
    let stream_id = session_arc.stream_id;

    // 保活线程：周期触发
    std::thread::spawn(move || {
        loop {
            // 等待 interval 秒（用短睡便于快速响应关闭）
            let total_ms = interval * 1000;
            let mut waited = 0u64;
            while waited < total_ms {
                std::thread::sleep(std::time::Duration::from_millis(200));
                waited += 200;
                if session_clone.is_closed() {
                    return;  // 会话已关闭，退出保活线程
                }
            }

            if session_clone.is_closed() {
                return;
            }

            // 协议级 keepalive：发送一个空的全局请求（want_reply=false）
            // russh 0.46 的 Handle 没有 explicit keepalive 方法，但 data() 空写或
            // 发送 ignore 包可以达到类似效果。这里用 --cmd 执行命令更可靠。
            let session = &*session_clone;
            if !cmd.is_empty() {
                // 执行空命令：通过新开一个 exec channel（不影响 PTY shell）
                let cmd_owned = cmd.clone();
                let result = session.runtime.block_on(async {
                    let handle_lock = session.handle.lock().unwrap();
                    let handle = match handle_lock.as_ref() {
                        Some(h) => h,
                        None => return Err("会话已关闭".to_string()),
                    };
                    // 开一个临时 channel 执行命令
                    let ch = match handle.channel_open_session().await {
                        Ok(c) => c,
                        Err(e) => return Err(format!("保活开通道失败: {}", e)),
                    };
                    if ch.exec(true, cmd_owned).await.is_err() {
                        return Err("保活 exec 失败".to_string());
                    }
                    // 不读输出，让 channel 自然结束
                    Ok::<(), String>(())
                });
                if result.is_err() {
                    // 保活失败，推送错误事件
                    crate::builtins_async::push_stream_event(
                        stream_id,
                        Value::str_from("保活失败，连接可能已断开".to_string()),
                        crate::builtins_async::StreamKind::Error,
                    );
                    return;
                }
            }
            // cmd 为空时不发任何东西（仅靠上面的周期检查判断连接活性；
            // 真正的 SSH 协议级 keepalive 需要底层支持，russh 0.46 未暴露 API）
        }
    });

    Ok(Value::Undefined)
}

/// bi_ssh_shell_stream_id 返回会话的流 ID。
fn bi_ssh_shell_stream_id(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "sshShellStreamId")?;
    let session_arc = ssh_session_clone(&args[0], "sshShellStreamId")?;
    Ok(Value::Int(session_arc.stream_id as i64))
}

