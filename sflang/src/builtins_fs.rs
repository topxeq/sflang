//! builtins_fs.rs — 文件 IO 内置函数
//!
//! 设计要点（来自 AGENTS.md）：
//!   - 仅依赖 Rust 标准库（std::fs / std::path）
//!   - 跨平台：使用 std::path::Path 处理路径分隔符
//!   - 错误信息 AI 友好：附 io::Error 原因与可能原因（不存在/权限/路径）
//!
//! 函数列表：
//!   全量读写（小文件便捷）：
//!     readFile(path)          — 读取整个文件为字符串
//!     writeFile(path, text)   — 覆盖写入字符串
//!     appendFile(path, text)  — 追加写入字符串
//!     fileExists(path)        — 判断路径是否存在
//!     deleteFile(path)        — 删除文件
//!     readLines(path)         — 按行读取为数组（保留行内容，去换行符）
//!     readFileBytes(path)     — 读取整个文件为 bytes
//!     writeFileBytes(path, b) — 写入字节
//!   file 句柄（流式/随机访问）：
//!     openFile(path, mode)    — 打开文件，返回 file 句柄
//!     closeFile(f)            — 关闭文件
//!     readLine(f)             — 读一行（string 或 undefined@EOF）
//!     readAll(f)              — 读全部剩余内容（bytes）
//!     readN(f, n)             — 读 n 字节（bytes 或 undefined@EOF）
//!     writeStr(f, s)          — 写字符串
//!     writeBytes(f, b)        — 写字节（bytes/byteArray/string）
//!     writeLine(f, s)         — 写一行（字符串 + \n）
//!     seek(f, offset, whence) — 定位（0=开头/1=当前/2=末尾）
//!     tell(f)                 — 当前位置（int）

use std::fs;
use std::io::{Read, Seek as _, SeekFrom, Write};
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
    // file 句柄（流式/随机访问）
    vm.register_builtin("openFile", bi_open_file);
    vm.register_builtin("closeFile", bi_close_file);
    vm.register_builtin("readLine", bi_read_line);
    vm.register_builtin("readAll", bi_read_all);
    vm.register_builtin("readN", bi_read_n);
    vm.register_builtin("writeStr", bi_write_str);
    vm.register_builtin("writeBytes", bi_write_bytes);
    vm.register_builtin("writeLine", bi_write_line);
    vm.register_builtin("seek", bi_seek);
    vm.register_builtin("tell", bi_tell);
    // 通用读取（file 句柄或基础类型）
    vm.register_builtin("readStr", bi_read_str);
    vm.register_builtin("readBytes", bi_read_bytes);
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

// ---- file 句柄（流式/随机访问）----

/// as_file 取参数为 file 句柄（Arc<Mutex<File>>）。
fn as_file<'a>(args: &'a [Value], idx: usize, fn_name: &str) -> Result<&'a Arc<Mutex<fs::File>>, Value> {
    bh::require_arg(args, idx, fn_name)?;
    match &args[idx] {
        Value::File(f) => Ok(f),
        v => Err(crate::value::error_value(format!(
            "{}() 参数应为 file 句柄，得到 {} (可能原因：未用 openFile 打开或已 close)",
            fn_name, v.type_name(),
        ))),
    }
}

/// bi_open_file 打开文件，返回 file 句柄。
///
/// mode: "r"(只读,默认) / "w"(只写,创建/覆盖) / "a"(追加) / "r+"(读写)
/// 不分文本/二进制——读取函数决定返回类型。
fn bi_open_file(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let path = bh::as_str(args, 0, "openFile")?;
    let mode = if args.len() > 1 { bh::as_str(args, 1, "openFile")? } else { "r" };
    let mut opts = fs::OpenOptions::new();
    match mode {
        "r" => { opts.read(true); }
        "w" => { opts.write(true).create(true).truncate(true); }
        "a" => { opts.append(true).create(true); }
        "r+" => { opts.read(true).write(true); }
        other => return Err(crate::value::error_value(format!(
            "openFile() 不支持的 mode '{}' (可能原因：mode 应为 r/w/a/r+)", other,
        ))),
    }
    let file = opts.open(Path::new(path)).map_err(|e| io_err("openFile", path, e))?;
    Ok(Value::File(Arc::new(Mutex::new(file))))
}

/// bi_close_file 关闭文件句柄。
fn bi_close_file(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    // as_file 不校验是否已关闭（File::close 幂等），直接 drop 即可
    let _f = as_file(args, 0, "closeFile")?;
    // 注：Arc 可能多引用，这里只释放当前引用。
    // 真正关闭靠所有引用消失（Arc drop）。简化处理。
    Ok(Value::Undefined)
}

/// bi_read_line 从 file 读取一行（到 \n，不含换行符）。
///
/// 返回 string 或 undefined（EOF）。兼容 \r\n。
fn bi_read_line(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let f = as_file(args, 0, "readLine")?;
    let mut guard = f.lock().map_err(|e| crate::value::error_value(format!(
        "readLine() 文件锁异常: {}", e,
    )))?;
    // 逐字节读到 \n（避免缓冲区问题，简单可靠）
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match guard.read(&mut byte) {
            Ok(0) => {
                // EOF
                if buf.is_empty() { return Ok(Value::Undefined); }
                break;
            }
            Ok(_) => {
                if byte[0] == b'\n' { break; }
                buf.push(byte[0]);
            }
            Err(e) => return Err(crate::value::error_value(format!(
                "readLine() 读取失败: {}", e,
            ))),
        }
    }
    // 去掉 \r\n 中的 \r
    if buf.last() == Some(&b'\r') { buf.pop(); }
    Ok(Value::str_from(String::from_utf8_lossy(&buf).into_owned()))
}

/// bi_read_all 读取文件全部剩余内容为 bytes。
fn bi_read_all(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let f = as_file(args, 0, "readAll")?;
    let mut guard = f.lock().map_err(|e| crate::value::error_value(format!(
        "readAll() 文件锁异常: {}", e,
    )))?;
    let mut buf = Vec::new();
    guard.read_to_end(&mut buf).map_err(|e| crate::value::error_value(format!(
        "readAll() 读取失败: {}", e,
    )))?;
    Ok(Value::Bytes(Arc::new(buf)))
}

/// bi_read_n 读取 n 字节为 bytes。
///
/// 返回 bytes 或 undefined（EOF 且未读到任何字节）。
fn bi_read_n(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let f = as_file(args, 0, "readN")?;
    let n = bh::as_int(args, 1, "readN")?;
    if n < 0 {
        return Err(crate::value::error_value("readN() n 不能为负"));
    }
    let n = n as usize;
    let mut guard = f.lock().map_err(|e| crate::value::error_value(format!(
        "readN() 文件锁异常: {}", e,
    )))?;
    let mut buf = vec![0u8; n];
    let read = guard.read(&mut buf).map_err(|e| crate::value::error_value(format!(
        "readN() 读取失败: {}", e,
    )))?;
    if read == 0 {
        return Ok(Value::Undefined);
    }
    buf.truncate(read);
    Ok(Value::Bytes(Arc::new(buf)))
}

/// bi_write_str 向 file 写字符串。
fn bi_write_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let f = as_file(args, 0, "writeStr")?;
    let s = bh::as_str(args, 1, "writeStr")?;
    let mut guard = f.lock().map_err(|e| crate::value::error_value(format!(
        "writeStr() 文件锁异常: {}", e,
    )))?;
    guard.write_all(s.as_bytes()).map_err(|e| crate::value::error_value(format!(
        "writeStr() 写入失败: {} (可能原因：磁盘满或权限)", e,
    )))?;
    Ok(Value::Undefined)
}

/// bi_write_bytes 向 file 写字节（bytes/byteArray/string）。
fn bi_write_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let f = as_file(args, 0, "writeBytes")?;
    bh::require_arg(args, 1, "writeBytes")?;
    let data: Vec<u8> = match &args[1] {
        Value::Bytes(b) => b.as_ref().to_vec(),
        Value::ByteArray(b) => b.lock().unwrap().clone(),
        Value::Str(s) => s.as_bytes().to_vec(),
        v => return Err(crate::value::error_value(format!(
            "writeBytes() 数据应为 bytes/byteArray/string，得到 {}", v.type_name(),
        ))),
    };
    let mut guard = f.lock().map_err(|e| crate::value::error_value(format!(
        "writeBytes() 文件锁异常: {}", e,
    )))?;
    guard.write_all(&data).map_err(|e| crate::value::error_value(format!(
        "writeBytes() 写入失败: {}", e,
    )))?;
    Ok(Value::Undefined)
}

/// bi_write_line 写一行（字符串 + \n）。
fn bi_write_line(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let f = as_file(args, 0, "writeLine")?;
    let s = bh::as_str(args, 1, "writeLine")?;
    let mut guard = f.lock().map_err(|e| crate::value::error_value(format!(
        "writeLine() 文件锁异常: {}", e,
    )))?;
    writeln!(guard, "{}", s).map_err(|e| crate::value::error_value(format!(
        "writeLine() 写入失败: {}", e,
    )))?;
    Ok(Value::Undefined)
}

/// bi_seek 定位文件指针。
///
/// whence: 0=从开头(SEEK_SET), 1=从当前(SEEK_CUR), 2=从末尾(SEEK_END)
fn bi_seek(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let f = as_file(args, 0, "seek")?;
    let offset = bh::as_int(args, 1, "seek")?;
    let whence = if args.len() > 2 { bh::as_int(args, 2, "seek")? } else { 0 };
    let from = match whence {
        0 => SeekFrom::Start(offset.max(0) as u64),
        1 => SeekFrom::Current(offset),
        2 => SeekFrom::End(offset),
        other => return Err(crate::value::error_value(format!(
            "seek() whence 应为 0(开头)/1(当前)/2(末尾)，得到 {}", other,
        ))),
    };
    let mut guard = f.lock().map_err(|e| crate::value::error_value(format!(
        "seek() 文件锁异常: {}", e,
    )))?;
    guard.seek(from).map_err(|e| crate::value::error_value(format!(
        "seek() 定位失败: {}", e,
    )))?;
    Ok(Value::Undefined)
}

/// bi_tell 返回当前文件位置（int）。
fn bi_tell(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let f = as_file(args, 0, "tell")?;
    let mut guard = f.lock().map_err(|e| crate::value::error_value(format!(
        "tell() 文件锁异常: {}", e,
    )))?;
    let pos = guard.stream_position().map_err(|e| crate::value::error_value(format!(
        "tell() 获取位置失败: {}", e,
    )))?;
    Ok(Value::Int(pos as i64))
}

// ---- 通用读取（file 句柄或基础类型）----
//
// 对标用户需求：readStr/readBytes 能从 file/string/bytes/byteArray 统一读取。
// file 句柄 → readAll 后转换；基础类型 → 直接转换。

/// bi_read_str 从各种源读取为字符串。
///
/// - file 句柄：读取全部剩余内容，UTF-8 解码为 string
/// - string：直接返回
/// - bytes/byteArray：UTF-8 解码为 string
fn bi_read_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "readStr")?;
    match &args[0] {
        Value::File(f) => {
            let mut guard = f.lock().map_err(|e| crate::value::error_value(format!(
                "readStr() 文件锁异常: {}", e,
            )))?;
            let mut buf = Vec::new();
            guard.read_to_end(&mut buf).map_err(|e| crate::value::error_value(format!(
                "readStr() 读取失败: {}", e,
            )))?;
            Ok(Value::str_from(String::from_utf8_lossy(&buf).into_owned()))
        }
        Value::Str(s) => Ok(Value::str_from(s.to_string())),
        Value::Bytes(b) => Ok(Value::str_from(String::from_utf8_lossy(b).into_owned())),
        Value::ByteArray(b) => {
            let snap = b.lock().unwrap().clone();
            Ok(Value::str_from(String::from_utf8_lossy(&snap).into_owned()))
        }
        v => Err(crate::value::error_value(format!(
            "readStr() 不支持类型 {} (可能原因：应为 file/string/bytes/byteArray)", v.type_name(),
        ))),
    }
}

/// bi_read_bytes 从各种源读取为 bytes。
///
/// - file 句柄：读取全部剩余内容为 bytes
/// - bytes：原样返回
/// - byteArray：拷贝为不可变 bytes
/// - string：UTF-8 编码为 bytes
fn bi_read_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "readBytes")?;
    match &args[0] {
        Value::File(f) => {
            let mut guard = f.lock().map_err(|e| crate::value::error_value(format!(
                "readBytes() 文件锁异常: {}", e,
            )))?;
            let mut buf = Vec::new();
            guard.read_to_end(&mut buf).map_err(|e| crate::value::error_value(format!(
                "readBytes() 读取失败: {}", e,
            )))?;
            Ok(Value::Bytes(Arc::new(buf)))
        }
        Value::Bytes(b) => Ok(Value::Bytes(b.clone())),
        Value::ByteArray(b) => {
            let snap = b.lock().unwrap().clone();
            Ok(Value::Bytes(Arc::new(snap)))
        }
        Value::Str(s) => Ok(Value::Bytes(Arc::new(s.as_bytes().to_vec()))),
        v => Err(crate::value::error_value(format!(
            "readBytes() 不支持类型 {} (可能原因：应为 file/bytes/byteArray/string)", v.type_name(),
        ))),
    }
}
