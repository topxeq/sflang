//! builtins_csv.rs — CSV 读写内置函数
//!
//! 按 RFC 4180 规范解析/生成 CSV。
//! 纯标准库实现，无需第三方依赖。
//!
//! 函数：
//!   readCsv(path[, delim])          — 从文件读取，返回二维数组（全字符串）
//!   readCsvFromStr(s[, delim])      — 从字符串读取
//!   csvEncode(rows[, delim])        — 把二维数组编码为 CSV 文本
//!   writeCsv(data, path[, delim])   — 把二维数组写入文件
//!
//! 统一约定：
//!   - 分隔符可选，默认逗号；传入时必须是单个字符（如 "|"、"\t"），
//!     且不能是引号或换行符；
//!   - 解析时自动剥离行首 UTF-8 BOM（\uFEFF），兼容带 BOM 的文件；
//!   - 生成的 CSV 为 UTF-8 无 BOM、LF 行尾；需要 BOM 时可在脚本层
//!     用 writeFile(path, "\uFEFF" + csvEncode(rows)) 自行拼接。

use std::sync::{Arc, Mutex};

use crate::builtins_helpers as bh;
use crate::value::Value;
use crate::function::BuiltinDoc;

static DOC_READCSV: BuiltinDoc = BuiltinDoc {
    category: "csv",
    signature: "readCsv(path[, delim]) -> array",
    summary: "从文件读取 CSV 为二维数组（全字符串），自动剥离 UTF-8 BOM。",
    params: &[("path", "CSV 文件路径"), ("delim", "可选。分隔符，默认逗号；单个字符，如 \"|\"、\"\\t\"")],
    returns: "array 行列数据",
    examples: &["var rows = readCsv(\"data.csv\")", "var rows = readCsv(\"data.tsv\", \"\\t\")"],
    errors: &["文件不存在或编码非 UTF-8 返回 error", "delim 不是单个字符或为引号/换行返回 error"],
};

static DOC_CSVREAD: BuiltinDoc = BuiltinDoc {
    category: "csv",
    signature: "readCsv(path[, delim]) -> array",
    summary: "readCsv 的别名。",
    params: &[("path", "CSV 文件路径"), ("delim", "可选。分隔符")],
    returns: "array",
    examples: &["csvRead(\"data.csv\")"],
    errors: &[],
};

static DOC_READCSVFROMSTR: BuiltinDoc = BuiltinDoc {
    category: "csv",
    signature: "readCsvFromStr(s[, delim]) -> array",
    summary: "从字符串读取 CSV，自动剥离行首 UTF-8 BOM。",
    params: &[("s", "CSV 格式字符串"), ("delim", "可选。分隔符，默认逗号；单个字符")],
    returns: "array",
    examples: &["var rows = readCsvFromStr(\"a,b\\n1,2\\n\")"],
    errors: &["delim 不是单个字符或为引号/换行返回 error"],
};

static DOC_WRITECSV: BuiltinDoc = BuiltinDoc {
    category: "csv",
    signature: "writeCsv(data, path[, delim]) -> undefined",
    summary: "将二维数组写入 CSV 文件（UTF-8 无 BOM、LF 行尾，自动转义）。",
    params: &[
        ("data", "二维数组（每个元素自动 toStr 转换）"),
        ("path", "输出路径"),
        ("delim", "可选。分隔符，默认逗号；单个字符，如 \"|\"、\"\\t\""),
    ],
    returns: "undefined",
    examples: &["writeCsv(rows, \"out.csv\")", "writeCsv(rows, \"out.tsv\", \"\\t\")"],
    errors: &["data 不是二维数组、path 类型错误、delim 非法返回 error"],
};

static DOC_CSVWRITE: BuiltinDoc = BuiltinDoc {
    category: "csv",
    signature: "writeCsv(data, path[, delim]) -> undefined",
    summary: "writeCsv 的别名。",
    params: &[
        ("data", "二维数组"),
        ("path", "输出路径"),
        ("delim", "可选。分隔符"),
    ],
    returns: "undefined",
    examples: &["csvWrite(rows, \"out.csv\")"],
    errors: &[],
};

static DOC_CSVENCODE: BuiltinDoc = BuiltinDoc {
    category: "csv",
    signature: "csvEncode(rows[, delim]) -> string",
    summary: "将二维数组编码为 CSV 文本（UTF-8 无 BOM、LF 行尾，自动转义）。",
    params: &[
        ("rows", "二维数组（每个元素自动 toStr 转换）"),
        ("delim", "可选。分隔符，默认逗号；单个字符，如 \"|\"、\"\\t\""),
    ],
    returns: "string CSV 文本，每行以 \\n 结束",
    examples: &[
        "var s = csvEncode([[\"name\",\"age\"], [\"Alice\",30]])  // \"name,age\\nAlice,30\\n\"",
        "writeFile(\"out.csv\", \"\\uFEFF\" + csvEncode(rows))  // 写带 BOM 的 CSV（Excel 友好）",
    ],
    errors: &["rows 不是二维数组、delim 非法返回 error"],
};

/// register 注册 CSV 内置函数。
pub fn register(vm: &mut crate::vm::VM) {
    vm.register_builtin_doc("readCsv", bi_read_csv, &DOC_READCSV);
    vm.register_builtin_doc("csvRead", bi_read_csv, &DOC_CSVREAD); // Charlang 兼容别名
    vm.register_builtin_doc("readCsvFromStr", bi_read_csv_from_str, &DOC_READCSVFROMSTR);
    vm.register_builtin_doc("writeCsv", bi_write_csv, &DOC_WRITECSV);
    vm.register_builtin_doc("csvWrite", bi_write_csv, &DOC_CSVWRITE); // Charlang 兼容别名
    vm.register_builtin_doc("csvEncode", bi_csv_encode, &DOC_CSVENCODE);
}

// ---- RFC 4180 CSV 解析器 ----

/// csv_parse 按 RFC 4180 解析 CSV 文本（逗号分隔），返回二维字符串数组。
///
/// 规则：
/// - 逗号分隔字段
/// - 双引号包裹的字段中，逗号和换行不算分隔符
/// - 引号内 "" 表示一个字面的双引号
/// - 引号外的 \r\n 或 \n 为行分隔符
/// - 最后一行可以无换行符
/// - 行首 UTF-8 BOM（\uFEFF）自动剥离
/// csv_parse 解析 CSV 文本为二维字符串数组（RFC 4180）。
/// 公开供 builtins_db.rs 的 csv 数据库连接复用。
pub fn csv_parse(text: &str) -> Vec<Vec<String>> {
    csv_parse_with(text, ',')
}

/// csv_parse_with 按指定分隔符解析 CSV 文本，返回二维字符串数组。
///
/// 规则同 csv_parse，仅分隔符不同；行首 UTF-8 BOM 同样自动剥离。
pub fn csv_parse_with(text: &str, delim: char) -> Vec<Vec<String>> {
    // 剥离行首 UTF-8 BOM，兼容带 BOM 的文件（如 Excel 导出的 CSV）
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);

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
        if ch == delim {
            current_row.push(std::mem::take(&mut current_field));
            i += 1;
        } else {
            match ch {
                '"' => {
                    in_quotes = true;
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
pub fn csv_escape(field: &str) -> String {
    csv_escape_with(field, ',')
}

/// csv_escape_with 按指定分隔符转义单个字段（必要时加引号）。
///
/// 需要加引号的情况：包含分隔符、双引号、换行、\r。
fn csv_escape_with(field: &str, delim: char) -> String {
    let needs_quote = field.contains(delim)
        || field.contains('"')
        || field.contains('\n')
        || field.contains('\r');
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

/// parse_delim_arg 解析可选的分隔符参数（args[idx]）。
///
/// 缺省返回逗号；传入时必须是单个字符，且不能是双引号或换行符
/// （这些字符参与引号/行结束的语法，作分隔符会破坏解析）。
fn parse_delim_arg(args: &[Value], idx: usize, fn_name: &str) -> Result<char, Value> {
    if args.len() <= idx {
        return Ok(',');
    }
    let s = bh::as_str(args, idx, fn_name)?;
    let mut chars = s.chars();
    let delim = match (chars.next(), chars.next()) {
        (Some(c), None) => c,
        _ => {
            return Err(crate::value::error_value(format!(
                "{}() 分隔符参数应为单个字符（如 \"|\"、\"\\t\"），得到 {:?} (可能原因：分隔符为空串或长度大于 1)",
                fn_name, s,
            )))
        }
    };
    if delim == '"' || delim == '\n' || delim == '\r' {
        return Err(crate::value::error_value(format!(
            "{}() 分隔符不能是引号（\"）或换行符（\\n、\\r），得到 {:?} (可能原因：这些字符是 CSV 语法保留字符)",
            fn_name, delim,
        )));
    }
    Ok(delim)
}

/// csv_rows_value_to_text 把二维数组 Value 编码为 CSV 文本。
///
/// 每个元素自动 toStr() 转换；非二维数组返回带原因的 error。
fn csv_rows_value_to_text(rows_val: &Value, fn_name: &str, delim: char) -> Result<String, Value> {
    let rows = match rows_val {
        Value::Array(a) => a.lock().unwrap().clone(),
        other => return Err(crate::value::error_value(format!(
            "{}() 第一个参数应为二维数组，得到 {} (可能原因：参数类型错误)", fn_name, other.type_name(),
        ))),
    };

    let delim_str = delim.to_string();
    let mut out = String::new();
    for row_val in &rows {
        match row_val {
            Value::Array(row) => {
                let fields: Vec<String> = row
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|v| csv_escape_with(&v.to_str(), delim))
                    .collect();
                out.push_str(&fields.join(&delim_str));
                out.push('\n');
            }
            other => return Err(crate::value::error_value(format!(
                "{}() 每行应为数组，得到 {} (可能原因：数据不是二维数组)", fn_name, other.type_name(),
            ))),
        }
    }

    Ok(out)
}

/// bi_read_csv 从文件路径读取 CSV，返回二维数组。
///
/// 用法：readCsv("data.csv") → [["name","age"], ["Alice","30"], ...]
/// 可选第二参数指定分隔符（单个字符）。
fn bi_read_csv(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    let path = bh::as_str(args, 0, "readCsv")?;
    let delim = parse_delim_arg(args, 1, "readCsv")?;
    let content = std::fs::read_to_string(path).map_err(|e| {
        crate::value::error_value(format!(
            "readCsv() 读取文件 '{}' 失败: {} (可能原因：文件不存在或编码非 UTF-8)",
            path, e,
        ))
    })?;
    let rows = csv_parse_with(&content, delim);
    Ok(csv_rows_to_value(rows))
}

/// bi_read_csv_from_str 从字符串解析 CSV，返回二维数组。
///
/// 用法：readCsvFromStr("a,b\n1,2\n") → [[...], ...]
/// 可选第二参数指定分隔符（单个字符）。
fn bi_read_csv_from_str(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    let text = bh::as_str(args, 0, "readCsvFromStr")?;
    let delim = parse_delim_arg(args, 1, "readCsvFromStr")?;
    let rows = csv_parse_with(text, delim);
    Ok(csv_rows_to_value(rows))
}

/// bi_csv_encode 把二维数组编码为 CSV 文本并返回。
///
/// 用法：csvEncode(rows) → "name,age\nAlice,30\n"
/// 与 readCsvFromStr 互为逆操作；可选第二参数指定分隔符（单个字符）。
/// 输出为 UTF-8 无 BOM、LF 行尾；需要 BOM 时在脚本层自行拼接 "\uFEFF"。
fn bi_csv_encode(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "csvEncode")?;
    let delim = parse_delim_arg(args, 1, "csvEncode")?;
    let text = csv_rows_value_to_text(&args[0], "csvEncode", delim)?;
    Ok(Value::str_from(text))
}

/// bi_write_csv 把二维数组写入 CSV 文件。
///
/// 用法：writeCsv(data, path) / writeCsv(data, path, "\t")
/// data 为二维数组，每个元素自动 toStr() 转换。
/// 输出为 UTF-8 无 BOM、LF 行尾。
fn bi_write_csv(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "writeCsv")?;
    let path = bh::as_str(args, 1, "writeCsv")?;
    let delim = parse_delim_arg(args, 2, "writeCsv")?;

    let text = csv_rows_value_to_text(&args[0], "writeCsv", delim)?;

    std::fs::write(path, text.as_bytes()).map_err(|e| {
        crate::value::error_value(format!(
            "writeCsv() 写入文件 '{}' 失败: {} (可能原因：路径无效或权限不足)",
            path, e,
        ))
    })?;

    Ok(Value::Undefined)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// csv_parse 基本解析（逗号）
    #[test]
    fn test_csv_parse_basic() {
        let rows = csv_parse("a,b,c\n1,2,3\n");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["a", "b", "c"]);
        assert_eq!(rows[1], vec!["1", "2", "3"]);
    }

    /// csv_parse 剥离行首 BOM
    #[test]
    fn test_csv_parse_strips_bom() {
        let rows = csv_parse("\u{FEFF}a,b\n1,2\n");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["a", "b"]);
        assert_eq!(rows[1], vec!["1", "2"]);
    }

    /// csv_parse_with 自定义分隔符
    #[test]
    fn test_csv_parse_with_delim() {
        let rows = csv_parse_with("a|b\n1|2\n", '|');
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["a", "b"]);
        assert_eq!(rows[1], vec!["1", "2"]);
    }

    /// csv_parse_with 制表符分隔（TSV）
    #[test]
    fn test_csv_parse_with_tab() {
        let rows = csv_parse_with("a\tb\n1\t2\n", '\t');
        assert_eq!(rows[0], vec!["a", "b"]);
    }

    /// csv_escape_with 含分隔符的字段加引号
    #[test]
    fn test_csv_escape_with_delim() {
        assert_eq!(csv_escape_with("x|y", '|'), "\"x|y\"");
        assert_eq!(csv_escape_with("x,y", '|'), "x,y");
        assert_eq!(csv_escape_with("x,y", ','), "\"x,y\"");
    }
}
