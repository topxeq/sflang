//! builtins_le.rs — 文本行编辑器内置函数（对标 Charlang le 系列）
//!
//! 内部维护一个全局行数组（Vec<String>），所有操作围绕它进行。
//! 纯标准库实现，无需第三方依赖。
//!
//! 支持的 SSH 操作：leLoadFromSsh / leSaveToSsh 可直接通过 SSH 读写远程文件。

use std::sync::{Arc, Mutex};

use crate::value::Value;
use crate::vm::VM;

/// map! 宏快速构造 Object。
macro_rules! map {
    ($($k:expr => $v:expr),* $(,)?) => {{
        let mut m = crate::object_map::Map::new();
        $(m.set($k.to_string(), $v);)*
        Value::Object(Arc::new(Mutex::new(m)))
    }};
}

/// LE_LINES 全局行数组（线程安全）。
static LE_LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// register 注册所有 le 内置函数。
pub fn register(vm: &mut VM) {
    vm.register_builtin("leLoadFromStr", bi_le_load_from_str);
    vm.register_builtin("leLoadFromFile", bi_le_load_from_file);
    vm.register_builtin("leLoadFromSsh", bi_le_load_from_ssh);
    vm.register_builtin("leSaveToStr", bi_le_save_to_str);
    vm.register_builtin("leSaveToFile", bi_le_save_to_file);
    vm.register_builtin("leSaveToSsh", bi_le_save_to_ssh);
    vm.register_builtin("leToStr", bi_le_save_to_str); // 别名
    vm.register_builtin("lePrint", bi_le_print);
    vm.register_builtin("leInfo", bi_le_info);
    vm.register_builtin("leClear", bi_le_clear);
    vm.register_builtin("leAppendLine", bi_le_append_line);
    vm.register_builtin("leAppendFromFile", bi_le_append_from_file);
    vm.register_builtin("leAppendFromStr", bi_le_append_from_str);
    vm.register_builtin("leAppendToFile", bi_le_append_to_file);
    vm.register_builtin("leInsertLine", bi_le_insert_line);
    vm.register_builtin("leRemoveLine", bi_le_remove_line);
    vm.register_builtin("leRemoveLines", bi_le_remove_lines);
    vm.register_builtin("leSetLine", bi_le_set_line);
    vm.register_builtin("leGetLine", bi_le_get_line);
    vm.register_builtin("leGetList", bi_le_get_list);
    vm.register_builtin("leSetLines", bi_le_set_lines);
    vm.register_builtin("leViewLine", bi_le_view_line);
    vm.register_builtin("leViewLines", bi_le_view_lines);
    vm.register_builtin("leViewAll", bi_le_view_all);
    vm.register_builtin("leSort", bi_le_sort);
    vm.register_builtin("leFind", bi_le_find);
    vm.register_builtin("leFindAll", bi_le_find_all);
    vm.register_builtin("leFindLines", bi_le_find_lines);
    vm.register_builtin("leReplace", bi_le_replace);
}

// ---- 辅助 ----

fn get_switch(args: &[Value], key: &str, default: &str) -> String {
    let prefix1 = format!("--{}=", key);
    let prefix2 = format!("-{}=", key);
    for arg in args {
        if let Value::Str(s) = arg {
            if let Some(rest) = s.strip_prefix(&prefix1).or_else(|| s.strip_prefix(&prefix2)) {
                return rest.to_string();
            }
        }
    }
    default.to_string()
}

fn has_switch(args: &[Value], key: &str) -> bool {
    let short = format!("-{}", key);
    args.iter().any(|arg| {
        if let Value::Str(s) = arg { &**s == short }
        else { false }
    })
}

fn split_lines(text: &str) -> Vec<String> {
    // 保持空行，去掉每行的 \r
    text.lines().map(|l| l.trim_end_matches('\r').to_string()).collect()
}

// ---- 加载/保存 ----

fn bi_le_load_from_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let text = args.get(0).map(|v| v.to_str()).unwrap_or_default();
    let mut lines = LE_LINES.lock().unwrap();
    *lines = split_lines(&text);
    Ok(Value::Undefined)
}

fn bi_le_load_from_file(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let path = args.get(0).map(|v| v.to_str()).unwrap_or_default();
    let content = std::fs::read_to_string(&path).map_err(|e| {
        crate::value::error_value(format!("leLoadFromFile() 读取 '{}' 失败: {}", path, e))
    })?;
    let mut lines = LE_LINES.lock().unwrap();
    *lines = split_lines(&content);
    Ok(Value::Undefined)
}

fn bi_le_load_from_ssh(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    // leLoadFromSsh("--host=...", "--user=...", "--password=...", "--remotePath=...")
    let remote_path = get_switch(args, "remotePath", "");
    if remote_path.is_empty() {
        return Ok(crate::value::error_value("leLoadFromSsh() 需要 --remotePath 参数"));
    }
    // 用 sshDownload 读取远程文件内容
    let dl_path = std::env::temp_dir().join("sflang_le_ssh_tmp.txt");
    let dl_path_str = dl_path.to_string_lossy().replace('\\', "/");

    // 构造 sshDownload 参数
    let mut ssh_args: Vec<Value> = Vec::new();
    for a in args {
        if let Value::Str(s) = a {
            if s.starts_with("--remotePath") {
                ssh_args.push(Value::str_from(format!("--remotePath={}", remote_path)));
            } else {
                ssh_args.push(a.clone());
            }
        }
    }
    ssh_args.push(Value::str_from(format!("--localPath={}", dl_path_str)));

    let result = crate::builtins_ssh::ssh_download_impl(vm, &ssh_args)?;
    if matches!(result, Value::Error(_)) {
        return Ok(result);
    }

    let content = std::fs::read_to_string(&dl_path).unwrap_or_default();
    let _ = std::fs::remove_file(&dl_path);

    let mut lines = LE_LINES.lock().unwrap();
    *lines = split_lines(&content);
    Ok(Value::Undefined)
}

fn bi_le_save_to_str(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    let lines = LE_LINES.lock().unwrap();
    let text = lines.join("\n");
    Ok(Value::str_from(text))
}

fn bi_le_save_to_file(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let path = args.get(0).map(|v| v.to_str()).unwrap_or_default();
    let lines = LE_LINES.lock().unwrap();
    let text = lines.join("\n");
    std::fs::write(&path, text.as_bytes()).map_err(|e| {
        crate::value::error_value(format!("leSaveToFile() 写入 '{}' 失败: {}", path, e))
    })?;
    Ok(Value::Undefined)
}

fn bi_le_save_to_ssh(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    // leSaveToSsh("--host=...", "--user=...", "--password=...", "--remotePath=...")
    let remote_path = get_switch(args, "remotePath", "");
    if remote_path.is_empty() {
        return Ok(crate::value::error_value("leSaveToSsh() 需要 --remotePath 参数"));
    }

    // 保存到临时文件再上传
    let tmp = std::env::temp_dir().join("sflang_le_ssh_upload.txt");
    let tmp_str = tmp.to_string_lossy().replace('\\', "/");

    {
        let lines = LE_LINES.lock().unwrap();
        let text = lines.join("\n");
        let _ = std::fs::write(&tmp, text.as_bytes());
    }

    let mut ssh_args: Vec<Value> = Vec::new();
    for a in args {
        ssh_args.push(a.clone());
    }
    ssh_args.push(Value::str_from(format!("--localPath={}", tmp_str)));

    let result = crate::builtins_ssh::ssh_upload_impl(vm, &ssh_args)?;
    let _ = std::fs::remove_file(&tmp);
    Ok(result)
}

// ---- 信息/显示 ----

fn bi_le_info(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    let lines = LE_LINES.lock().unwrap();
    let info = map!{
        "lineCount" => Value::Int(lines.len() as i64),
        "charCount" => Value::Int(lines.iter().map(|l| l.chars().count() as i64).sum()),
        "byteCount" => Value::Int(lines.iter().map(|l| l.len() as i64).sum()),
    };
    Ok(info)
}

fn bi_le_print(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let line_number = has_switch(args, "lineNumber");
    let with_len = has_switch(args, "withLen");
    let lines = LE_LINES.lock().unwrap();
    let out = _vm.output_handle();
    for (i, line) in lines.iter().enumerate() {
        let mut prefix = String::new();
        if line_number {
            prefix.push_str(&format!("{:4}: ", i));
        }
        if with_len {
            prefix.push_str(&format!("({:4}) ", line.len()));
        }
        writeln!(out.lock().unwrap(), "{}{}", prefix, line)
            .map_err(|e| crate::value::error_value(e.to_string()))?;
    }
    Ok(Value::Undefined)
}

fn bi_le_clear(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    LE_LINES.lock().unwrap().clear();
    Ok(Value::Undefined)
}

// ---- 行操作 ----

fn bi_le_append_line(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let line = args.get(0).map(|v| v.to_str()).unwrap_or_default();
    LE_LINES.lock().unwrap().push(line);
    Ok(Value::Undefined)
}

fn bi_le_append_from_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let text = args.get(0).map(|v| v.to_str()).unwrap_or_default();
    let mut lines = LE_LINES.lock().unwrap();
    lines.extend(split_lines(&text));
    Ok(Value::Undefined)
}

fn bi_le_append_from_file(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let path = args.get(0).map(|v| v.to_str()).unwrap_or_default();
    let content = std::fs::read_to_string(&path).map_err(|e| {
        crate::value::error_value(format!("leAppendFromFile() 读取 '{}' 失败: {}", path, e))
    })?;
    let mut lines = LE_LINES.lock().unwrap();
    lines.extend(split_lines(&content));
    Ok(Value::Undefined)
}

fn bi_le_append_to_file(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let path = args.get(0).map(|v| v.to_str()).unwrap_or_default();
    let lines = LE_LINES.lock().unwrap();
    let text = lines.join("\n");
    // 追加模式
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().append(true).create(true).open(&path)
        .map_err(|e| crate::value::error_value(format!("leAppendToFile() 打开 '{}' 失败: {}", path, e)))?;
    writeln!(file, "{}", text).map_err(|e| {
        crate::value::error_value(format!("leAppendToFile() 写入失败: {}", e))
    })?;
    Ok(Value::Undefined)
}

fn bi_le_insert_line(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let idx = args.get(0).and_then(|v| v.to_int()).unwrap_or(0) as usize;
    let line = args.get(1).map(|v| v.to_str()).unwrap_or_default();
    let mut lines = LE_LINES.lock().unwrap();
    let pos = idx.min(lines.len());
    lines.insert(pos, line);
    Ok(Value::Undefined)
}

fn bi_le_remove_line(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let idx = args.get(0).and_then(|v| v.to_int()).unwrap_or(0) as usize;
    let mut lines = LE_LINES.lock().unwrap();
    if idx < lines.len() {
        lines.remove(idx);
    }
    Ok(Value::Undefined)
}

fn bi_le_remove_lines(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let start = args.get(0).and_then(|v| v.to_int()).unwrap_or(0) as usize;
    let count = args.get(1).and_then(|v| v.to_int()).unwrap_or(1) as usize;
    let mut lines = LE_LINES.lock().unwrap();
    let end = (start + count).min(lines.len());
    if start < lines.len() {
        lines.drain(start..end);
    }
    Ok(Value::Undefined)
}

fn bi_le_set_line(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let idx = args.get(0).and_then(|v| v.to_int()).unwrap_or(0) as usize;
    let text = args.get(1).map(|v| v.to_str()).unwrap_or_default();
    let mut lines = LE_LINES.lock().unwrap();
    if idx < lines.len() {
        lines[idx] = text;
    } else {
        // 超出范围时追加
        while lines.len() < idx {
            lines.push(String::new());
        }
        lines.push(text);
    }
    Ok(Value::Undefined)
}

fn bi_le_get_line(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let idx = args.get(0).and_then(|v| v.to_int()).unwrap_or(0) as usize;
    let lines = LE_LINES.lock().unwrap();
    Ok(Value::str(lines.get(idx).map(|s| s.as_str()).unwrap_or("")))
}

fn bi_le_get_list(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let lines = LE_LINES.lock().unwrap();
    // 支持 leGetList() 全部 或 leGetList(start, end) 范围
    let (start, end) = match (args.get(0).and_then(|v| v.to_int()), args.get(1).and_then(|v| v.to_int())) {
        (Some(s), Some(e)) => (s as usize, e.min(lines.len() as i64) as usize),
        _ => (0, lines.len()),
    };
    let start = start.min(lines.len());
    let end = end.min(lines.len());
    let result: Vec<Value> = lines[start..end].iter().map(|s| Value::str(s)).collect();
    Ok(Value::Array(Arc::new(Mutex::new(result))))
}

fn bi_le_set_lines(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let arr = match args.get(0) {
        Some(Value::Array(a)) => a.lock().unwrap().clone(),
        _ => return Ok(crate::value::error_value("leSetLines() 需要数组参数")),
    };
    let new_lines: Vec<String> = arr.iter().map(|v| v.to_str()).collect();
    let mut lines = LE_LINES.lock().unwrap();
    *lines = new_lines;
    Ok(Value::Undefined)
}

// ---- 查看 ----

fn bi_le_view_line(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let idx = args.get(0).and_then(|v| v.to_int()).unwrap_or(0) as usize;
    let lines = LE_LINES.lock().unwrap();
    Ok(Value::str(lines.get(idx).map(|s| s.as_str()).unwrap_or("")))
}

fn bi_le_view_lines(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let start = args.get(0).and_then(|v| v.to_int()).unwrap_or(0) as usize;
    let end = args.get(1).and_then(|v| v.to_int()).unwrap_or(0) as usize;
    let lines = LE_LINES.lock().unwrap();
    let start = start.min(lines.len());
    let end = end.min(lines.len());
    let result: Vec<Value> = lines[start..end].iter().map(|s| Value::str(s)).collect();
    Ok(Value::Array(Arc::new(Mutex::new(result))))
}

fn bi_le_view_all(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    let lines = LE_LINES.lock().unwrap();
    let result: Vec<Value> = lines.iter().map(|s| Value::str(s)).collect();
    Ok(Value::Array(Arc::new(Mutex::new(result))))
}

// ---- 排序 ----

fn bi_le_sort(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let order = get_switch(args, "order", "asc");
    let mut lines = LE_LINES.lock().unwrap();
    if order == "desc" {
        lines.sort_by(|a, b| b.cmp(a));
    } else {
        lines.sort();
    }
    Ok(Value::Undefined)
}

// ---- 查找/替换 ----

fn bi_le_find(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    // leFind(pattern, groupIndex) → 返回匹配结果的数组
    let pattern = args.get(0).map(|v| v.to_str()).unwrap_or_default();
    let group = args.get(1).and_then(|v| v.to_int()).unwrap_or(0) as usize;
    let lines = LE_LINES.lock().unwrap();

    let re = regex::Regex::new(&pattern).map_err(|e| {
        crate::value::error_value(format!("leFind() 正则编译失败: {}", e))
    })?;

    let mut results: Vec<Value> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if let Some(caps) = re.captures(line) {
            let matched = caps.get(group).map(|m| m.as_str()).unwrap_or("");
            // 返回 {line: 行号, text: 匹配文本}
            let entry = map!{
                "line" => Value::Int(i as i64),
                "text" => Value::str(matched),
            };
            results.push(entry);
        }
    }
    Ok(Value::Array(Arc::new(Mutex::new(results))))
}

fn bi_le_find_all(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let pattern = args.get(0).map(|v| v.to_str()).unwrap_or_default();
    let group = args.get(1).and_then(|v| v.to_int()).unwrap_or(0) as usize;
    let lines = LE_LINES.lock().unwrap();

    let re = regex::Regex::new(&pattern).map_err(|e| {
        crate::value::error_value(format!("leFindAll() 正则编译失败: {}", e))
    })?;

    let mut results: Vec<Value> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        for caps in re.captures_iter(line) {
            let matched = caps.get(group).map(|m| m.as_str()).unwrap_or("");
            let entry = map!{
                "line" => Value::Int(i as i64),
                "text" => Value::str(matched),
            };
            results.push(entry);
        }
    }
    Ok(Value::Array(Arc::new(Mutex::new(results))))
}

fn bi_le_find_lines(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let pattern = args.get(0).map(|v| v.to_str()).unwrap_or_default();
    let print = has_switch(args, "print");
    let lines = LE_LINES.lock().unwrap();

    let re = regex::Regex::new(&pattern).map_err(|e| {
        crate::value::error_value(format!("leFindLines() 正则编译失败: {}", e))
    })?;

    let mut matching_lines: Vec<Value> = Vec::new();
    let mut output_lines: Vec<(usize, String)> = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        if re.is_match(line) {
            matching_lines.push(Value::Int(i as i64));
            output_lines.push((i, line.clone()));
        }
    }

    if print {
        let out = _vm.output_handle();
        for (i, line) in &output_lines {
            writeln!(out.lock().unwrap(), "  {}: {}", i, line)
                .map_err(|e| crate::value::error_value(e.to_string()))?;
        }
    }

    Ok(Value::Array(Arc::new(Mutex::new(matching_lines))))
}

fn bi_le_replace(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let pattern = args.get(0).map(|v| v.to_str()).unwrap_or_default();
    let replacement = args.get(1).map(|v| v.to_str()).unwrap_or_default();

    let re = regex::Regex::new(&pattern).map_err(|e| {
        crate::value::error_value(format!("leReplace() 正则编译失败: {}", e))
    })?;

    let mut lines = LE_LINES.lock().unwrap();
    for line in lines.iter_mut() {
        *line = re.replace_all(line, replacement.as_str()).into_owned();
    }
    Ok(Value::Undefined)
}
