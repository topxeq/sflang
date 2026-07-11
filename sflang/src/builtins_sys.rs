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
use crate::value::Value;
use crate::vm::VM;

/// register 注册系统/路径内置函数。
pub fn register(vm: &mut VM) {
    vm.register_builtin("getEnv", bi_get_env);
    vm.register_builtin("setEnv", bi_set_env);
    vm.register_builtin("osName", bi_os_name);
    vm.register_builtin("osArch", bi_os_arch);
    vm.register_builtin("getCurDir", bi_get_cur_dir);
    vm.register_builtin("getTempDir", bi_get_temp_dir);
    vm.register_builtin("getHomeDir", bi_get_home_dir);
    vm.register_builtin("joinPath", bi_join_path);
    vm.register_builtin("dirName", bi_dir_name);
    vm.register_builtin("baseName", bi_base_name);
    vm.register_builtin("fileExt", bi_file_ext);
    vm.register_builtin("absPath", bi_abs_path);
    vm.register_builtin("makeDir", bi_make_dir);
    vm.register_builtin("makeDirAll", bi_make_dir_all);
    vm.register_builtin("listDir", bi_list_dir);
    vm.register_builtin("removeDir", bi_remove_dir);
    vm.register_builtin("rename", bi_rename);
    vm.register_builtin("systemCmd", bi_system_cmd);
    vm.register_builtin("exit", bi_exit);
    vm.register_builtin("getOsArgs", bi_get_os_args);
    vm.register_builtin("getInput", bi_get_input);
    vm.register_builtin("getChar", bi_get_char);
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

    // 构建命令
    let mut command = if cfg!(windows) {
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

    // 追加额外参数（用于简单命令如 systemCmd("ping", "127.0.0.1")）
    for i in 1..args.len() {
        if let Value::Str(s) = &args[i] {
            command.arg(s.as_ref());
        }
    }

    let output = command.output().map_err(|e| crate::value::error_value(format!(
        "systemCmd() 执行失败: {} (可能原因：命令不存在或权限不足)", e,
    )))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() && !stderr.is_empty() {
        // 命令失败但有 stderr 输出，拼到一起
        let combined = format!("{}{}", stdout, stderr);
        return Ok(crate::value::error_value(format!(
            "systemCmd() 命令失败 (exit {}): {}", output.status.code().unwrap_or(-1), combined.trim(),
        )));
    }

    Ok(Value::str_from(stdout.into_owned()))
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
