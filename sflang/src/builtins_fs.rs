//! builtins_fs.rs — 文件 IO 内置函数
//!
//! 设计要点（来自 AGENTS.md）：
//!   - 仅依赖 Rust 标准库（std::fs / std::path）
//!   - 跨平台：使用 std::path::Path 处理路径分隔符
//!   - 错误信息 AI 友好：附 io::Error 原因与可能原因（不存在/权限/路径）
//!
//! 函数列表：
//!   readFile(path)          — 读取整个文件为字符串
//!   writeFile(path, text)   — 覆盖写入字符串
//!   appendFile(path, text)  — 追加写入字符串
//!   fileExists(path)        — 判断路径是否存在
//!   deleteFile(path)        — 删除文件
//!   readLines(path)         — 按行读取为数组（保留行内容，去换行符）

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::builtins_helpers as bh;
use crate::value::Value;
use crate::vm::VM;

/// register 注册所有文件 IO 内置函数到 VM。
pub fn register(vm: &mut VM) {
    vm.register_builtin("readFile", bi_read_file);
    vm.register_builtin("writeFile", bi_write_file);
    vm.register_builtin("appendFile", bi_append_file);
    vm.register_builtin("fileExists", bi_file_exists);
    vm.register_builtin("deleteFile", bi_delete_file);
    vm.register_builtin("readLines", bi_read_lines);
    // 二进制 IO（读取/写入原始字节，不经过 UTF-8 解码）
    vm.register_builtin("readFileBytes", bi_read_file_bytes);
    vm.register_builtin("writeFileBytes", bi_write_file_bytes);
}

/// io_err 将 std::io::Error 转为 AI 友好错误值，附加常见原因提示。
fn io_err(fn_name: &str, path: &str, e: std::io::Error) -> Value {
    let hint = match e.kind() {
        std::io::ErrorKind::NotFound => "文件不存在（检查路径或当前工作目录）",
        std::io::ErrorKind::PermissionDenied => "权限不足",
        std::io::ErrorKind::AlreadyExists => "文件已存在",
        _ => "路径非法或被占用",
    };
    crate::value::error_value(format!(
        "{}() 失败: 路径 '{}' - {} (可能原因：{})",
        fn_name, path, e, hint,
    ))
}

/// bi_read_file 读取整个文件为 UTF-8 字符串。
fn bi_read_file(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let path = bh::as_str(args, 0, "readFile")?;
    let content = fs::read_to_string(Path::new(path)).map_err(|e| io_err("readFile", path, e))?;
    Ok(Value::str_from(content))
}

/// bi_write_file 覆盖写入字符串到文件（不存在则创建）。
fn bi_write_file(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let path = bh::as_str(args, 0, "writeFile")?;
    let content = bh::as_str(args, 1, "writeFile")?;
    fs::write(Path::new(path), content).map_err(|e| io_err("writeFile", path, e))?;
    Ok(Value::Undefined)
}

/// bi_append_file 追加写入字符串到文件（不存在则创建）。
fn bi_append_file(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let path = bh::as_str(args, 0, "appendFile")?;
    let content = bh::as_str(args, 1, "appendFile")?;
    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(Path::new(path))
        .map_err(|e| io_err("appendFile", path, e))?;
    f.write_all(content.as_bytes()).map_err(|e| io_err("appendFile", path, e))?;
    Ok(Value::Undefined)
}

/// bi_file_exists 判断路径是否存在。
fn bi_file_exists(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let path = bh::as_str(args, 0, "fileExists")?;
    Ok(Value::Bool(Path::new(path).exists()))
}

/// bi_delete_file 删除文件。返回是否删除成功（不存在视为失败并报错）。
fn bi_delete_file(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let path = bh::as_str(args, 0, "deleteFile")?;
    fs::remove_file(Path::new(path)).map_err(|e| io_err("deleteFile", path, e))?;
    Ok(Value::Undefined)
}

/// bi_read_lines 按行读取文件为数组（去掉每行末尾换行符）。
///
/// 兼容 \n 与 \r\n。语义：每个以换行结尾的为完整行；末尾无换行的残行也算一行；
/// 末尾换行之后的空内容不算作行。故空文件 → 空数组，"a\n" → ["a"]，"a" → ["a"]，
/// "a\nb\n" → ["a","b"]，"\n" → [""]（一个空行）。
fn bi_read_lines(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let path = bh::as_str(args, 0, "readLines")?;
    let content = fs::read_to_string(Path::new(path)).map_err(|e| io_err("readLines", path, e))?;
    let lines = split_lines(&content);
    Ok(Value::Array(Arc::new(Mutex::new(lines))))
}

/// split_lines 按行切分（兼容 \n 与 \r\n），返回 Value::Str 数组。
///
/// 语义同标准库 lines()：以换行分隔；末尾换行不产生额外的空行；
/// 但纯粹的空字符串（空文件）返回空数组。
fn split_lines(content: &str) -> Vec<Value> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for c in content.chars() {
        if c == '\n' {
            // 去掉行尾可能的 \r（Windows 风格），\r\n 中的 \r 已被加入 cur，此处剥离
            if cur.ends_with('\r') {
                cur.pop();
            }
            lines.push(Value::str_from(std::mem::take(&mut cur)));
        } else {
            cur.push(c);
        }
    }
    // 末尾残行（无换行结尾）：仅当非空时作为最后一行加入
    // （末尾换行后的空残行不算行，故 cur 为空时跳过）
    if !cur.is_empty() {
        lines.push(Value::str_from(cur));
    }
    lines
}

/// bi_read_file_bytes 读取整个文件为不可变 bytes（不经过 UTF-8 解码，适合二进制文件）。
///
/// 与 readFile 区别：readFile 返回 string（要求 UTF-8），readFileBytes 返回 bytes，
/// 可读取图片/加密数据/任意二进制。需要修改时用 byteArrayFromBytes 转换。
fn bi_read_file_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let path = bh::as_str(args, 0, "readFileBytes")?;
    let data = fs::read(Path::new(path)).map_err(|e| io_err("readFileBytes", path, e))?;
    Ok(Value::Bytes(Arc::new(data)))
}

/// bi_write_file_bytes 将字节序列写入文件（覆盖）。
///
/// 接受 bytes 或 byteArray 作为数据源。
fn bi_write_file_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let path = bh::as_str(args, 0, "writeFileBytes")?;
    bh::require_arg(args, 1, "writeFileBytes")?;
    let data: Vec<u8> = match &args[1] {
        Value::Bytes(b) => b.as_ref().to_vec(),
        Value::ByteArray(b) => b.lock().unwrap().clone(),
        Value::Str(s) => s.as_bytes().to_vec(),
        v => return Err(crate::value::error_value(format!(
            "writeFileBytes() 数据应为 bytes/byteArray/string，得到 {} (可能原因：类型不匹配)",
            v.type_name(),
        ))),
    };
    fs::write(Path::new(path), data).map_err(|e| io_err("writeFileBytes", path, e))?;
    Ok(Value::Undefined)
}
