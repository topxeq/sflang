//! builtins_csv.rs — CSV 读写内置函数
//!
//! 按 RFC 4180 规范解析/生成 CSV。
//! 纯标准库实现，无需第三方依赖。
//!
//! 函数：
//!   readCsv(path)          — 从文件读取，返回二维数组（全字符串）
//!   readCsvFromStr(s)      — 从字符串读取
//!   writeCsv(data, path)   — 把二维数组写入文件

use std::sync::{Arc, Mutex};

use crate::builtins_helpers as bh;
use crate::value::Value;

/// register 注册 CSV 内置函数。
pub fn register(vm: &mut crate::vm::VM) {
    vm.register_builtin("readCsv", bi_read_csv);
    vm.register_builtin("csvRead", bi_read_csv); // Charlang 兼容别名
    vm.register_builtin("readCsvFromStr", bi_read_csv_from_str);
    vm.register_builtin("writeCsv", bi_write_csv);
    vm.register_builtin("csvWrite", bi_write_csv); // Charlang 兼容别名
}

// ---- RFC 4180 CSV 解析器 ----

/// csv_parse 按 RFC 4180 解析 CSV 文本，返回二维字符串数组。
///
/// 规则：
/// - 逗号分隔字段
/// - 双引号包裹的字段中，逗号和换行不算分隔符
/// - 引号内 "" 表示一个字面的双引号
/// - 引号外的 \r\n 或 \n 为行分隔符
/// - 最后一行可以无换行符
fn csv_parse(text: &str) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut current_field = String::new();
    let mut in_quotes = false;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];

        if in_quotes {
            if ch == '"' {
                // 检查是否为转义的双引号 ""
                if i + 1 < chars.len() && chars[i + 1] == '"' {
                    current_field.push('"');
                    i += 2;
                    continue;
                } else {
                    // 引号结束
                    in_quotes = false;
                    i += 1;
                    continue;
                }
            } else {
                current_field.push(ch);
                i += 1;
                continue;
            }
        }

        // 不在引号内
        match ch {
            '"' => {
                in_quotes = true;
                i += 1;
            }
            ',' => {
                current_row.push(std::mem::take(&mut current_field));
                i += 1;
            }
            '\r' => {
                // \r\n 或单独 \r 都算行结束
                current_row.push(std::mem::take(&mut current_field));
                rows.push(std::mem::take(&mut current_row));
                // 跳过可能的 \n
                if i + 1 < chars.len() && chars[i + 1] == '\n' {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            '\n' => {
                current_row.push(std::mem::take(&mut current_field));
                rows.push(std::mem::take(&mut current_row));
                i += 1;
            }
            _ => {
                current_field.push(ch);
                i += 1;
            }
        }
    }

    // 处理最后一行（无换行符结尾的情况）
    if !current_field.is_empty() || !current_row.is_empty() {
        current_row.push(current_field);
        rows.push(current_row);
    }

    rows
}

/// csv_escape 转义单个字段为 CSV 格式（必要时加引号）。
///
/// 需要加引号的情况：包含逗号、双引号、换行、\r。
fn csv_escape(field: &str) -> String {
    let needs_quote = field.contains(',') || field.contains('"') || field.contains('\n') || field.contains('\r');
    if needs_quote {
        // 双引号转义为 ""
        let escaped = field.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        field.to_string()
    }
}

/// csv_rows_to_value 把 Vec<Vec<String>> 转为 Value 二维数组。
fn csv_rows_to_value(rows: Vec<Vec<String>>) -> Value {
    let outer: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            let inner: Vec<Value> = row.into_iter().map(Value::str_from).collect();
            Value::Array(Arc::new(Mutex::new(inner)))
        })
        .collect();
    Value::Array(Arc::new(Mutex::new(outer)))
}

/// bi_read_csv 从文件路径读取 CSV，返回二维数组。
///
/// 用法：readCsv("data.csv") → [["name","age"], ["Alice","30"], ...]
fn bi_read_csv(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    let path = bh::as_str(args, 0, "readCsv")?;
    let content = std::fs::read_to_string(path).map_err(|e| {
        crate::value::error_value(format!(
            "readCsv() 读取文件 '{}' 失败: {} (可能原因：文件不存在或编码非 UTF-8)",
            path, e,
        ))
    })?;
    let rows = csv_parse(&content);
    Ok(csv_rows_to_value(rows))
}

/// bi_read_csv_from_str 从字符串解析 CSV，返回二维数组。
///
/// 用法：readCsvFromStr(s) → [[...], ...]
fn bi_read_csv_from_str(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    let text = bh::as_str(args, 0, "readCsvFromStr")?;
    let rows = csv_parse(text);
    Ok(csv_rows_to_value(rows))
}

/// bi_write_csv 把二维数组写入 CSV 文件。
///
/// 用法：writeCsv(data, path)
/// data 为二维数组，每个元素自动 toStr() 转换。
fn bi_write_csv(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "writeCsv")?;
    let path = bh::as_str(args, 1, "writeCsv")?;

    let rows = match &args[0] {
        Value::Array(a) => a.lock().unwrap().clone(),
        other => return Err(crate::value::error_value(format!(
            "writeCsv() 第一个参数应为二维数组，得到 {} (可能原因：参数类型错误)", other.type_name(),
        ))),
    };

    let mut out = String::new();
    for row_val in &rows {
        match row_val {
            Value::Array(row) => {
                let fields: Vec<String> = row.lock().unwrap().iter().map(|v| csv_escape(&v.to_str())).collect();
                out.push_str(&fields.join(","));
                out.push('\n');
            }
            other => return Err(crate::value::error_value(format!(
                "writeCsv() 每行应为数组，得到 {} (可能原因：数据不是二维数组)", other.type_name(),
            ))),
        }
    }

    std::fs::write(path, out.as_bytes()).map_err(|e| {
        crate::value::error_value(format!(
            "writeCsv() 写入文件 '{}' 失败: {} (可能原因：路径无效或权限不足)",
            path, e,
        ))
    })?;

    Ok(Value::Undefined)
}
