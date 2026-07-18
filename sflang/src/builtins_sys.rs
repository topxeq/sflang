//! builtins_sys.rs — 系统/环境/路径内置函数（纯标准库）
//!
//! 设计要点：
//!   - 环境变量：getEnv/setEnv
//!   - 系统信息：osName/getCurDir/getTempDir/getHomeDir
//!   - 路径处理：joinPath/dirName/baseName/fileExt/absPath
//!   - 目录操作：makeDir/makeDirAll/listDir
//!   - 全部基于 std::env / std::path / std::fs
//!
//! 函数列表：
//!   getEnv(name)        — 读取环境变量（无则 undefined）
//!   setEnv(name, val)   — 设置环境变量
//!   osName()            — 操作系统名（"windows"/"linux"/"macos"）
//!   osArch()            — CPU 架构（"amd64"/"arm64" 等）
//!   getCurDir()         — 当前工作目录
//!   getTempDir()        — 系统临时目录
//!   getHomeDir()        — 用户主目录（无则 undefined）
//!   joinPath(...)       — 拼接多个路径段
//!   dirName(p)          — 路径的目录部分
//!   baseName(p)         — 路径的文件名部分
//!   fileExt(p)          — 文件扩展名（含点，如 ".txt"）
//!   absPath(p)          — 转绝对路径
//!   makeDir(p)          — 创建单层目录
//!   makeDirAll(p)       — 递归创建目录
//!   listDir(p)          — 列出目录下的条目名（array<string>）

use std::sync::Arc;

use crate::builtins_helpers as bh;
use crate::function::BuiltinDoc;
use crate::value::Value;
use crate::vm::VM;

// ---- 系统/环境/路径函数文档 ----

static DOC_GET_ENV: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "getEnv(name) -> string|undefined",
    summary: "读取环境变量。不存在（或未设置）返回 undefined。",
    params: &[("name", "环境变量名（区分大小写）")],
    returns: "string 变量值；未设置返回 undefined",
    examples: &[
        "getEnv(\"PATH\")        → \"/usr/bin:...\"",
        "var home = getEnv(\"HOME\") ?? \"/tmp\"",
    ],
    errors: &[
        "环境变量名在 Unix 区分大小写；未设置返回 undefined（非 error）",
    ],
};

static DOC_SET_ENV: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "setEnv(name, val) -> undefined",
    summary: "设置环境变量（仅当前进程及其子进程可见，不持久化到系统）。",
    params: &[
        ("name", "环境变量名"),
        ("val", "变量值（string）"),
    ],
    returns: "undefined",
    examples: &[
        "setEnv(\"MY_VAR\", \"hello\")",
        "systemCmd(\"echo $MY_VAR\")  // 子进程可见",
    ],
    errors: &[
        "参数顺序：name 在前，val 在后",
        "仅影响当前进程；退出后不保留（不写入注册表/shell 配置）",
    ],
};

static DOC_OS_NAME: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "osName() -> string",
    summary: "返回操作系统名：windows / linux / macos（未知为 unknown）。",
    params: &[],
    returns: "string 编译期确定的 OS 名称",
    examples: &[
        "osName()   → \"linux\"",
        "if (osName() == \"windows\") { ... }",
    ],
    errors: &[],
};

static DOC_OS_ARCH: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "osArch() -> string",
    summary: "返回 CPU 架构名：amd64 / arm64 / 386（其他回退到 std::env::consts::ARCH）。",
    params: &[],
    returns: "string 编译期确定的架构名称",
    examples: &[
        "osArch()   → \"amd64\"",
    ],
    errors: &[],
};

static DOC_GET_CUR_DIR: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "getCurDir() -> string",
    summary: "返回当前工作目录的绝对路径。",
    params: &[],
    returns: "string 当前工作目录",
    examples: &[
        "getCurDir()   → \"/home/user/project\"",
        "println(getCurDir())",
    ],
    errors: &[
        "目录被删除或权限不足返回 error",
    ],
};

static DOC_CHANGE_DIR: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "changeDir(path) -> undefined",
    summary: "改变当前工作目录（影响后续相对路径解析）。等同于 shell 的 cd。",
    params: &[("path", "目标目录路径")],
    returns: "undefined",
    examples: &[
        "changeDir(\"/tmp\")",
        "changeDir(\"..\")",
    ],
    errors: &[
        "目录不存在或权限不足返回 error",
        "影响整个进程的相对路径基准；子线程也会受影响",
    ],
};

static DOC_JOIN_PATH: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "joinPath(seg1, seg2, ...) -> string",
    summary: "用平台路径分隔符拼接多个路径段，自动处理冗余分隔符。",
    params: &[("segN", "路径段（可变参数，每段为 string）")],
    returns: "string 拼接后的路径",
    examples: &[
        "joinPath(\"a\", \"b\", \"c.txt\")   → \"a/b/c.txt\"",
        "joinPath(\"/usr\", \"local/bin\")  → \"/usr/local/bin\"",
    ],
    errors: &[
        "所有参数须为 string，非 string 会报错",
    ],
};

static DOC_MAKE_DIR: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "makeDir(path) -> undefined",
    summary: "创建单层目录（父目录须存在；目录已存在则报错）。递归建目录请用 makeDirAll。",
    params: &[("path", "要创建的目录路径")],
    returns: "undefined",
    examples: &[
        "makeDir(\"newdir\")",
        "makeDir(\"a/b/c\")  // a/b 须已存在，否则报错",
    ],
    errors: &[
        "目录已存在、父目录缺失、权限不足均返回 error",
        "递归创建多层请用 makeDirAll",
    ],
};

static DOC_LIST_DIR: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "listDir(path) -> array<string>",
    summary: "列出目录下的条目名（不递归，仅返回文件名不含路径前缀）。",
    params: &[("path", "目录路径")],
    returns: "array<string> 条目名数组；目录不存在或非目录返回 error",
    examples: &[
        "listDir(\".\")   → [\"a.txt\", \"sub\", \"b.rs\"]",
        "for (name in listDir(dir)) { ... }",
    ],
    errors: &[
        "路径不存在或不是目录返回 error",
        "返回的是名称（不含父路径），需完整路径请用 joinPath 拼接",
    ],
};

static DOC_SYSTEM_CMD: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "systemCmd(cmd[, arg1, arg2, ...]) -> string",
    summary: "执行 shell 命令（Windows 用 cmd /C，Unix 用 sh -c），返回 stdout 字符串。",
    params: &[
        ("cmd", "命令字符串（如 \"ls\" 或 \"dir\"）"),
        ("argN", "可选追加参数（string，附加到命令后）"),
    ],
    returns: "string 命令的 stdout 输出；命令失败（含 stderr）返回 error",
    examples: &[
        "systemCmd(\"echo hello\")   → \"hello\\n\"",
        "systemCmd(\"ping\", \"127.0.0.1\")",
        "var out = systemCmd(\"git status\")",
    ],
    errors: &[
        "命令不存在或权限不足返回 error",
        "退出码非 0 且有 stderr 时合并拼成 error 信息",
        "存在命令注入风险，切勿将不可信输入直接拼接为 cmd",
    ],
};

static DOC_GET_INPUT: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "getInput([prompt]) -> string|undefined",
    summary: "从标准输入读一行（去掉末尾换行）。可选 prompt 作为提示先打印到 stdout。",
    params: &[("prompt", "可选提示字符串，先打印（不换行）")],
    returns: "string 读到的行；EOF（Ctrl+D / Ctrl+Z）返回 undefined",
    examples: &[
        "name := getInput(\"请输入姓名: \")",
        "line := getInput()",
    ],
    errors: &[
        "stdin 被关闭或重定向导致读取失败返回 error",
        "EOF 返回 undefined（须判空），不会报错",
    ],
};

static DOC_EXIT: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "exit([code]) -> !",
    summary: "立即退出进程。可选 code 为退出码（int，默认 0）。",
    params: &[("code", "可选退出码（int，默认 0）")],
    returns: "不返回（进程直接终止）",
    examples: &[
        "exit(1)",
        "exit()   // 等同 exit(0)",
    ],
    errors: &[
        "不会触发 defer；调用后进程立即终止",
    ],
};

static DOC_LOOK_PATH: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "lookPath(name) -> string|undefined",
    summary: "在 PATH 环境变量中查找可执行文件路径（Windows 自动尝试 PATHEXT 中的扩展名）。",
    params: &[("name", "可执行文件名（如 \"go\"、\"python\"）或含分隔符的路径")],
    returns: "string 找到的完整路径；未找到返回 undefined",
    examples: &[
        "lookPath(\"go\")      → \"/usr/local/go/bin/go\"",
        "lookPath(\"python\") → undefined（未安装时）",
        "var p = lookPath(\"git\")",
    ],
    errors: &[
        "未找到返回 undefined（非 error）",
        "name 含 / 或 \\ 时直接检查该路径是否为文件",
    ],
};

static DOC_GETTEMPDIR: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "getTempDir() -> string",
    summary: "返回系统临时目录。",
    params: &[],
    returns: "string",
    examples: &[],
    errors: &[],
};

static DOC_GETHOMEDIR: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "getHomeDir() -> string",
    summary: "返回用户主目录。",
    params: &[],
    returns: "string",
    examples: &[],
    errors: &[],
};

static DOC_DIRNAME: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "dirName(path) -> string",
    summary: "返回路径的目录部分。",
    params: &[("path", "文件路径")],
    returns: "string",
    examples: &[],
    errors: &[],
};

static DOC_BASENAME: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "baseName(path) -> string",
    summary: "返回路径的文件名部分。",
    params: &[("path", "文件路径")],
    returns: "string",
    examples: &[],
    errors: &[],
};

static DOC_FILEEXT: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "fileExt(path) -> string",
    summary: "返回文件扩展名（含点）。",
    params: &[("path", "文件路径")],
    returns: "string",
    examples: &[],
    errors: &[],
};

static DOC_ABSPATH: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "absPath(path) -> string",
    summary: "返回绝对路径。",
    params: &[("path", "相对或绝对路径")],
    returns: "string 绝对路径",
    examples: &[],
    errors: &[],
};

static DOC_MAKEDIRALL: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "makeDirAll(path) -> error|undefined",
    summary: "递归创建目录。",
    params: &[("path", "目录路径")],
    returns: "undefined；失败返回 error",
    examples: &[],
    errors: &[],
};

static DOC_REMOVEDIR: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "removeDir(path) -> error|undefined",
    summary: "删除目录。",
    params: &[("path", "目录路径")],
    returns: "undefined",
    examples: &[],
    errors: &[],
};

static DOC_RENAME: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "rename(old, new) -> error|undefined",
    summary: "重命名/移动文件或目录。",
    params: &[("old", "旧路径"), ("new", "新路径")],
    returns: "undefined",
    examples: &[],
    errors: &[],
};

static DOC_GETOSARGS: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "getOsArgs() -> array<string>",
    summary: "返回命令行参数（含程序名）。",
    params: &[],
    returns: "array<string>",
    examples: &[],
    errors: &[],
};

static DOC_GETCHAR: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "getChar() -> string",
    summary: "读取单个按键（无需回车）。",
    params: &[],
    returns: "string 单字符",
    examples: &[],
    errors: &[],
};

static DOC_GETINPUTPASSWORD: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "getInputPassword([prompt]) -> string",
    summary: "读取密码输入（不回显）。",
    params: &[("prompt", "可选。提示信息")],
    returns: "string",
    examples: &[],
    errors: &[],
};

static DOC_SYSTEMCMDDETACHED: BuiltinDoc = BuiltinDoc {
    category: "system",
    signature: "systemCmdDetached(cmd) -> undefined",
    summary: "后台执行命令（不等待）。",
    params: &[("cmd", "命令字符串")],
    returns: "undefined",
    examples: &[],
    errors: &[],
};

/// register 注册系统/路径内置函数。
pub fn register(vm: &mut VM) {
    vm.register_builtin_doc("getEnv", bi_get_env, &DOC_GET_ENV);
    vm.register_builtin_doc("setEnv", bi_set_env, &DOC_SET_ENV);
    vm.register_builtin_doc("osName", bi_os_name, &DOC_OS_NAME);
    vm.register_builtin_doc("osArch", bi_os_arch, &DOC_OS_ARCH);
    vm.register_builtin_doc("getCurDir", bi_get_cur_dir, &DOC_GET_CUR_DIR);
    vm.register_builtin_doc("getTempDir", bi_get_temp_dir, &DOC_GETTEMPDIR);
    vm.register_builtin_doc("getHomeDir", bi_get_home_dir, &DOC_GETHOMEDIR);
    vm.register_builtin_doc("joinPath", bi_join_path, &DOC_JOIN_PATH);
    vm.register_builtin_doc("dirName", bi_dir_name, &DOC_DIRNAME);
    vm.register_builtin_doc("baseName", bi_base_name, &DOC_BASENAME);
    vm.register_builtin_doc("fileExt", bi_file_ext, &DOC_FILEEXT);
    vm.register_builtin_doc("absPath", bi_abs_path, &DOC_ABSPATH);
    vm.register_builtin_doc("makeDir", bi_make_dir, &DOC_MAKE_DIR);
    vm.register_builtin_doc("makeDirAll", bi_make_dir_all, &DOC_MAKEDIRALL);
    vm.register_builtin_doc("listDir", bi_list_dir, &DOC_LIST_DIR);
    vm.register_builtin_doc("removeDir", bi_remove_dir, &DOC_REMOVEDIR);
    vm.register_builtin_doc("rename", bi_rename, &DOC_RENAME);
    vm.register_builtin_doc("systemCmd", bi_system_cmd, &DOC_SYSTEM_CMD);
    vm.register_builtin_doc("exit", bi_exit, &DOC_EXIT);
    vm.register_builtin_doc("getOsArgs", bi_get_os_args, &DOC_GETOSARGS);
    vm.register_builtin_doc("getInput", bi_get_input, &DOC_GET_INPUT);
    vm.register_builtin_doc("getChar", bi_get_char, &DOC_GETCHAR);
    vm.register_builtin_doc("changeDir", bi_change_dir, &DOC_CHANGE_DIR);
    vm.register_builtin_doc("lookPath", bi_look_path, &DOC_LOOK_PATH);
    vm.register_builtin_doc("getInputPassword", bi_get_input_password, &DOC_GETINPUTPASSWORD);
    vm.register_builtin_doc("systemCmdDetached", bi_system_cmd_detached, &DOC_SYSTEMCMDDETACHED);
}

/// bi_get_env 读取环境变量，无则返回 undefined。
fn bi_get_env(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let name = bh::as_str(args, 0, "getEnv")?;
    match std::env::var(name) {
        Ok(v) => Ok(Value::str_from(v)),
        Err(_) => Ok(Value::Undefined),
    }
}

/// bi_set_env 设置环境变量。
fn bi_set_env(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let name = bh::as_str(args, 0, "setEnv")?;
    let val = bh::as_str(args, 1, "setEnv")?;
    std::env::set_var(name, val);
    Ok(Value::Undefined)
}

/// bi_os_name 返回操作系统名。
fn bi_os_name(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    let name = if cfg!(target_os = "windows") { "windows" }
        else if cfg!(target_os = "linux") { "linux" }
        else if cfg!(target_os = "macos") { "macos" }
        else { "unknown" };
    Ok(Value::str(name))
}

/// bi_os_arch 返回 CPU 架构名。
fn bi_os_arch(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    let arch = if cfg!(target_arch = "x86_64") { "amd64" }
        else if cfg!(target_arch = "aarch64") { "arm64" }
        else if cfg!(target_arch = "x86") { "386" }
        else { std::env::consts::ARCH };
    Ok(Value::str(arch))
}

/// bi_get_cur_dir 返回当前工作目录。
fn bi_get_cur_dir(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    let dir = std::env::current_dir().map_err(|e| crate::value::error_value(
        format!("getCurDir() 失败: {} (可能原因：权限或目录被删)", e),
    ))?;
    Ok(Value::str_from(dir.to_string_lossy().into_owned()))
}

/// bi_get_temp_dir 返回系统临时目录。
fn bi_get_temp_dir(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    let dir = std::env::temp_dir();
    Ok(Value::str_from(dir.to_string_lossy().into_owned()))
}

/// bi_get_home_dir 返回用户主目录（无则 undefined）。
fn bi_get_home_dir(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    match std::env::var(if cfg!(target_os = "windows") { "USERPROFILE" } else { "HOME" }) {
        Ok(v) => Ok(Value::str_from(v)),
        Err(_) => Ok(Value::Undefined),
    }
}

/// bi_join_path 拼接多个路径段。
fn bi_join_path(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use std::path::PathBuf;
    let mut path = PathBuf::new();
    for (i, arg) in args.iter().enumerate() {
        let s = bh::as_str(args, i, "joinPath")?;
        path.push(s);
        let _ = arg; // 避免 unused
    }
    Ok(Value::str_from(path.to_string_lossy().into_owned()))
}

/// bi_dir_name 返回路径的目录部分。
fn bi_dir_name(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let p = bh::as_str(args, 0, "dirName")?;
    let parent = std::path::Path::new(p).parent()
        .map(|x| x.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(Value::str_from(parent))
}

/// bi_base_name 返回路径的文件名部分。
fn bi_base_name(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let p = bh::as_str(args, 0, "baseName")?;
    let name = std::path::Path::new(p).file_name()
        .map(|x| x.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(Value::str_from(name))
}

/// bi_file_ext 返回文件扩展名（含点）。
fn bi_file_ext(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let p = bh::as_str(args, 0, "fileExt")?;
    let ext = std::path::Path::new(p).extension()
        .map(|x| format!(".{}", x.to_string_lossy()))
        .unwrap_or_default();
    Ok(Value::str_from(ext))
}

/// bi_abs_path 转绝对路径。
fn bi_abs_path(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let p = bh::as_str(args, 0, "absPath")?;
    let abs = std::fs::canonicalize(p)
        .map(|x| x.to_string_lossy().into_owned())
        .unwrap_or_else(|_| p.to_string());
    Ok(Value::str_from(abs))
}

/// bi_make_dir 创建单层目录。
fn bi_make_dir(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let p = bh::as_str(args, 0, "makeDir")?;
    std::fs::create_dir(p).map_err(|e| crate::value::error_value(format!(
        "makeDir() 失败: '{}' - {} (可能原因：目录已存在或权限不足)", p, e,
    )))?;
    Ok(Value::Undefined)
}

/// bi_make_dir_all 递归创建目录。
fn bi_make_dir_all(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let p = bh::as_str(args, 0, "makeDirAll")?;
    std::fs::create_dir_all(p).map_err(|e| crate::value::error_value(format!(
        "makeDirAll() 失败: '{}' - {}", p, e,
    )))?;
    Ok(Value::Undefined)
}

/// bi_list_dir 列出目录下的条目名（不递归）。
fn bi_list_dir(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let p = bh::as_str(args, 0, "listDir")?;
    let entries = std::fs::read_dir(p).map_err(|e| crate::value::error_value(format!(
        "listDir() 失败: '{}' - {} (可能原因：不是目录或不存在)", p, e,
    )))?;
    let mut names = Vec::new();
    for entry in entries {
        if let Ok(e) = entry {
            names.push(Value::str_from(e.file_name().to_string_lossy().into_owned()));
        }
    }
    Ok(Value::Array(Arc::new(std::sync::Mutex::new(names))))
}

/// bi_remove_dir 删除目录（仅空目录）。
fn bi_remove_dir(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let p = bh::as_str(args, 0, "removeDir")?;
    std::fs::remove_dir(p).map_err(|e| crate::value::error_value(format!(
        "removeDir() 失败: '{}' - {} (可能原因：目录非空或不存在)", p, e,
    )))?;
    Ok(Value::Undefined)
}

/// bi_rename 重命名/移动文件或目录。
fn bi_rename(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let from = bh::as_str(args, 0, "rename")?;
    let to = bh::as_str(args, 1, "rename")?;
    std::fs::rename(from, to).map_err(|e| crate::value::error_value(format!(
        "rename() 失败: '{}' → '{}' - {}", from, to, e,
    )))?;
    Ok(Value::Undefined)
}

/// bi_system_cmd 执行系统命令，返回 stdout 输出。
///
/// 用法：systemCmd("dir") 或 systemCmd("ls", "-la")
/// 返回命令的 stdout 输出字符串。
fn bi_system_cmd(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let cmd = bh::as_str(args, 0, "systemCmd")?;

    // 构建命令：
    // - 有额外参数时，直接用第一个参数作为可执行程序（避免 cmd /C 的路径解析问题）
    // - 只有单个字符串参数时，通过 shell（cmd /C 或 sh -c）执行（支持管道、重定向等）
    let mut command = if args.len() > 1 {
        // 直接执行模式：systemCmd("program", "arg1", "arg2", ...)
        std::process::Command::new(cmd)
    } else if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.arg("/C");
        c.arg(cmd);
        c
    } else {
        let mut c = std::process::Command::new("sh");
        c.arg("-c");
        c.arg(cmd);
        c
    };

    // 追加额外参数
    for i in 1..args.len() {
        if let Value::Str(s) = &args[i] {
            command.arg(s.as_ref());
        }
    }

    let output = command.output().map_err(|e| crate::value::error_value(format!(
        "systemCmd() 执行失败: {} (可能原因：命令不存在或权限不足)", e,
    )))?;

    // Windows 中文系统 cmd.exe 输出可能是 GBK(CP936) 编码。
    // 先尝试 UTF-8，失败则用 GBK 解码（encoding_rs 已是依赖）。
    let decode = |bytes: &[u8]| -> String {
        match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            Err(_) => {
                // 非 UTF-8，尝试 GBK 解码
                let (cow, _, _) = encoding_rs::GBK.decode(bytes);
                cow.into_owned()
            }
        }
    };
    let stdout = decode(&output.stdout);
    let stderr = decode(&output.stderr);

    if !output.status.success() && !stderr.is_empty() {
        // 命令失败但有 stderr 输出，拼到一起
        let combined = format!("{}{}", stdout, stderr);
        return Ok(crate::value::error_value(format!(
            "systemCmd() 命令失败 (exit {}): {}", output.status.code().unwrap_or(-1), combined.trim(),
        )));
    }

    Ok(Value::str_from(stdout))
}

/// bi_exit 退出程序。
///
/// 用法：exit() 或 exit(0) 或 exit(1)
fn bi_exit(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let code = args.get(0).and_then(|v| v.to_int()).unwrap_or(0) as i32;
    std::process::exit(code);
}

/// bi_get_os_args 返回完整的命令行参数（含程序名）。
fn bi_get_os_args(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    let full_args: Vec<Value> = std::env::args().map(Value::str_from).collect();
    Ok(Value::Array(Arc::new(std::sync::Mutex::new(full_args))))
}

/// bi_get_input 从标准输入读取一行文本（去除末尾换行）。
///
/// 用法：line := getInput()
///       name := getInput("请输入姓名: ")  // 带提示
/// EOF（Ctrl+D / Ctrl+Z）返回 undefined。
fn bi_get_input(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use std::io::Write;
    // 可选提示
    if !args.is_empty() {
        let prompt = bh::as_str(args, 0, "getInput")?;
        if !prompt.is_empty() {
            print!("{}", prompt);
            let _ = std::io::stdout().flush();
        }
    }
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) => Ok(Value::Undefined),  // EOF
        Ok(_) => {
            // 去除末尾换行（\n 或 \r\n）
            while line.ends_with('\n') || line.ends_with('\r') {
                line.pop();
            }
            Ok(Value::str_from(line))
        }
        Err(e) => Err(crate::value::error_value(format!(
            "getInput() 读取输入失败: {} (可能原因：stdin 被关闭或重定向)", e,
        ))),
    }
}

/// bi_get_char 从标准输入读取单个字符（无回车，需按键即返回）。
///
/// 用法：ch := getChar()
/// EOF 返回 undefined。
fn bi_get_char(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    use std::io::Read;
    let mut buf = [0u8; 1];
    match std::io::stdin().read(&mut buf) {
        Ok(0) => Ok(Value::Undefined),
        Ok(_) => {
            // 处理 UTF-8 多字节字符
            let first = buf[0];
            let need = if first < 0x80 { 1 }
                       else if first < 0xC0 { 1 }  // 无效 UTF-8 前导，按 1 字节
                       else if first < 0xE0 { 2 }
                       else if first < 0xF0 { 3 }
                       else { 4 };
            if need == 1 {
                // ASCII 或无效字节，尝试转 char
                Ok(Value::str_from((first as char).to_string()))
            } else {
                // 读取剩余字节
                let mut full = vec![first];
                for _ in 1..need {
                    let mut b = [0u8; 1];
                    match std::io::stdin().read(&mut b) {
                        Ok(0) => break,
                        Ok(_) => full.push(b[0]),
                        Err(e) => return Err(crate::value::error_value(format!(
                            "getChar() 读取多字节字符失败: {}", e,
                        ))),
                    }
                }
                Ok(Value::str_from(String::from_utf8_lossy(&full).to_string()))
            }
        }
        Err(e) => Err(crate::value::error_value(format!(
            "getChar() 读取字符失败: {} (可能原因：stdin 被关闭)", e,
        ))),
    }
}

/// bi_change_dir 改变当前工作目录。
///
/// 用法：changeDir("/tmp") 或 changeDir("..")
fn bi_change_dir(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let path = bh::as_str(args, 0, "changeDir")?;
    std::env::set_current_dir(path).map_err(|e| crate::value::error_value(format!(
        "changeDir() 失败: '{}' - {} (可能原因：目录不存在或权限不足)", path, e,
    )))?;
    Ok(Value::Undefined)
}

/// bi_look_path 在 PATH 中查找可执行文件路径。
///
/// 用法：lookPath("go") → "/usr/local/go/bin/go"
///       lookPath("python") → undefined（未找到）
/// Windows 下会自动尝试 PATHEXT 中的扩展名（.exe/.bat/.cmd 等）。
fn bi_look_path(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let name = bh::as_str(args, 0, "lookPath")?;
    // 若 name 已含路径分隔符，直接检查是否为文件
    if name.contains('/') || name.contains('\\') {
        let p = std::path::Path::new(name);
        if p.is_file() {
            return Ok(Value::str_from(p.to_string_lossy().into_owned()));
        }
        return Ok(Value::Undefined);
    }
    // 从 PATH 环境变量获取搜索目录列表
    let path_var = match std::env::var("PATH") {
        Ok(v) => v,
        Err(_) => return Ok(Value::Undefined),
    };
    // 路径分隔符：Windows 用 ';'，Unix 用 ':'
    let sep = if cfg!(windows) { ';' } else { ':' };
    // Windows 下 PATHEXT 提供可执行扩展名列表
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.BAT;.CMD;.COM".to_string())
            .split(';')
            .map(|s| s.to_string())
            .collect()
    } else {
        Vec::new()
    };
    for dir in path_var.split(sep) {
        if dir.is_empty() {
            continue;
        }
        let base = std::path::PathBuf::from(dir);
        if cfg!(windows) {
            // Windows：先试原名，再试各扩展名
            let cand = base.join(name);
            if cand.is_file() {
                return Ok(Value::str_from(cand.to_string_lossy().into_owned()));
            }
            for ext in &exts {
                let cand = base.join(format!("{}{}", name, ext));
                if cand.is_file() {
                    return Ok(Value::str_from(cand.to_string_lossy().into_owned()));
                }
            }
        } else {
            // Unix：直接拼接检查
            let cand = base.join(name);
            if cand.is_file() {
                return Ok(Value::str_from(cand.to_string_lossy().into_owned()));
            }
        }
    }
    Ok(Value::Undefined)
}

// ---- 密码输入（不回显）----

/// read_line_plain 读取一行（无回显控制），用于密码输入的回退。
fn read_line_plain(prompt: &str) -> Result<String, Value> {
    use std::io::Write;
    if !prompt.is_empty() {
        print!("{}", prompt);
        let _ = std::io::stdout().flush();
    }
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) => Ok(String::new()),
        Ok(_) => {
            while line.ends_with('\n') || line.ends_with('\r') {
                line.pop();
            }
            Ok(line)
        }
        Err(e) => Err(crate::value::error_value(format!(
            "getInputPassword() 读取失败: {} (可能原因：stdin 被关闭)", e,
        ))),
    }
}

/// read_password_windows 读取密码（关闭控制台回显），失败时退化为普通输入。
#[cfg(windows)]
fn read_password_windows(prompt: &str) -> Result<String, Value> {
    use std::io::Write;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Console::{
        GetConsoleMode, SetConsoleMode, ENABLE_ECHO_INPUT,
    };
    // 显示提示
    if !prompt.is_empty() {
        print!("{}", prompt);
        let _ = std::io::stdout().flush();
    }
    let stdin = std::io::stdin();
    let handle = stdin.as_raw_handle();
    // 获取当前控制台模式
    let mut old_mode: u32 = 0;
    let mode_ok = unsafe { GetConsoleMode(handle, &mut old_mode) } != 0;
    if !mode_ok {
        // 非控制台（如管道重定向），退化为普通输入
        return read_line_plain("");
    }
    // 关闭回显
    let new_mode = old_mode & !ENABLE_ECHO_INPUT;
    unsafe { SetConsoleMode(handle, new_mode) };
    // 读取一行
    let mut line = String::new();
    let read_result = stdin.read_line(&mut line);
    // 恢复回显
    unsafe { SetConsoleMode(handle, old_mode) };
    // 密码输入后换行，保持终端整洁
    println!();
    match read_result {
        Ok(0) => Ok(String::new()),
        Ok(_) => {
            while line.ends_with('\n') || line.ends_with('\r') {
                line.pop();
            }
            Ok(line)
        }
        Err(e) => Err(crate::value::error_value(format!(
            "getInputPassword() 读取失败: {} (可能原因：stdin 被关闭)", e,
        ))),
    }
}

/// read_password_unix 读取密码（用 stty 关闭回显），失败时退化为普通输入。
#[cfg(not(windows))]
fn read_password_unix(prompt: &str) -> Result<String, Value> {
    use std::io::Write;
    // 显示提示
    if !prompt.is_empty() {
        print!("{}", prompt);
        let _ = std::io::stdout().flush();
    }
    // 尝试用 stty 关闭回显
    let stty_ok = std::process::Command::new("stty")
        .arg("-echo")
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let mut line = String::new();
    let read_result = std::io::stdin().read_line(&mut line);
    // 恢复回显
    if stty_ok {
        let _ = std::process::Command::new("stty").arg("echo").status();
    }
    // 密码输入后换行，保持终端整洁
    println!();
    match read_result {
        Ok(0) => Ok(String::new()),
        Ok(_) => {
            while line.ends_with('\n') || line.ends_with('\r') {
                line.pop();
            }
            Ok(line)
        }
        Err(e) => Err(crate::value::error_value(format!(
            "getInputPassword() 读取失败: {} (可能原因：stdin 被关闭)", e,
        ))),
    }
}

/// bi_get_input_password 读取密码输入（不回显）。
///
/// 用法：pw := getInputPassword("请输入密码: ")
///       pw := getInputPassword()  // 无提示
/// Windows 用 windows-sys 控制台 API 关闭 ECHO，Linux 用 stty。
/// 若无法隐藏回显（如 stdin 被重定向），退化为普通输入。
fn bi_get_input_password(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let prompt = if !args.is_empty() {
        bh::as_str(args, 0, "getInputPassword")?
    } else {
        ""
    };
    // 用 cfg 条件编译选择对应平台的实现（不能用 if cfg! 因另一平台函数不存在）
    let pw = {
        #[cfg(windows)]
        { read_password_windows(prompt) }
        #[cfg(not(windows))]
        { read_password_unix(prompt) }
    }?;
    Ok(Value::str_from(pw))
}

/// bi_system_cmd_detached 分离执行系统命令（不等待完成）。
///
/// 用法：pid := systemCmdDetached("notepad")
///       pid := systemCmdDetached("ping", ["127.0.0.1", "-n", "3"])
/// 第二个参数可为字符串或字符串数组。返回子进程 PID（int）。
fn bi_system_cmd_detached(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let cmd = bh::as_str(args, 0, "systemCmdDetached")?;
    let mut command = std::process::Command::new(cmd);
    // 后续参数可为多个 string 或单个 string 数组
    if args.len() > 1 {
        match &args[1] {
            Value::Array(a) => {
                let snap = a.lock().unwrap().clone();
                for v in snap.iter() {
                    command.arg(v.to_str());
                }
            }
            Value::Str(_) => {
                // 多参数模式：args[1..] 每个都是单独的参数
                for i in 1..args.len() {
                    command.arg(bh::as_str(args, i, "systemCmdDetached")?);
                }
            }
            v => return Err(crate::value::error_value(format!(
                "systemCmdDetached() 参数应为 string 或 string 数组，得到 {} (可能原因：参数类型错误)",
                v.type_name(),
            ))),
        }
    }
    // Windows: 在新控制台窗口中启动（CREATE_NEW_CONSOLE）
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NEW_CONSOLE = 0x00000010
        command.creation_flags(0x00000010);
    }
    let child = command.spawn().map_err(|e| crate::value::error_value(format!(
        "systemCmdDetached() 启动失败: '{}' - {} (可能原因：命令不存在或权限不足)", cmd, e,
    )))?;
    Ok(Value::Int(child.id() as i64))
}
