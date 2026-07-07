//! builtins_xlsx.rs — Excel (xlsx) 读写内置函数
//!
//! 读取用 calamine（支持 xlsx/xls/xlsb/ods），
//! 写入用 rust_xlsxwriter（纯 Rust，生成 xlsx）。
//!
//! 函数（对标 Charlang）：
//!   excelNew()                          — 创建空工作簿
//!   excelOpen(path)                     — 打开已有 xlsx（用于后续 writeSheet）
//!   excelSaveAs(wb, path)               — 保存工作簿
//!   excelReadSheet(path, sheetIndex)    — 从文件读取指定 sheet → 二维数组
//!   excelReadAll(path)                  — 读取所有 sheets → map{名: 二维数组}
//!   excelWriteSheet(wb, sheetIndex, data) — 写二维数组到工作簿
//!   excelNewSheet(wb, name)             — 新建 sheet，返回索引

use std::sync::{Arc, Mutex};

use calamine::Reader;

use crate::builtins_helpers as bh;
use crate::value::Value;

/// Workbook Sflang 中的 Excel 工作簿对象（可写）。
///
/// 包装 rust_xlsxwriter::Workbook，用 Arc<Mutex<>> 实现线程安全共享。
pub type Workbook = rust_xlsxwriter::Workbook;

/// register 注册所有 Excel 内置函数。
pub fn register(vm: &mut crate::vm::VM) {
    vm.register_builtin("excelNew", bi_excel_new);
    vm.register_builtin("excelOpen", bi_excel_open);
    vm.register_builtin("excelSaveAs", bi_excel_save_as);
    vm.register_builtin("excelReadSheet", bi_excel_read_sheet);
    vm.register_builtin("excelReadAll", bi_excel_read_all);
    vm.register_builtin("excelWriteSheet", bi_excel_write_sheet);
    vm.register_builtin("excelNewSheet", bi_excel_new_sheet);
}

// ---- 辅助函数 ----

/// workbook_value 将 Workbook 包装为 Value::Native。
fn workbook_value(wb: Workbook) -> Value {
    Value::Native(Arc::new(Arc::new(Mutex::new(wb))))
}

/// workbook_downcast 从 Value 中提取 Workbook 引用。
fn workbook_downcast<'a>(v: &'a Value, fn_name: &str) -> Result<&'a Arc<Mutex<Workbook>>, Value> {
    match v {
        Value::Native(n) => n.downcast_ref::<Arc<Mutex<Workbook>>>().ok_or_else(|| {
            crate::value::error_value(format!(
                "{}() 参数不是 workbook (可能原因：未用 excelNew/excelOpen 创建)",
                fn_name,
            ))
        }),
        Value::Undefined => Err(crate::value::error_value(format!(
            "{}() 参数为 undefined (可能原因：变量未初始化)", fn_name,
        ))),
        other => Err(crate::value::error_value(format!(
            "{}() 参数应为 workbook，得到 {} (可能原因：参数顺序错误)",
            fn_name, other.type_name(),
        ))),
    }
}

/// calamine_data_to_value 将 calamine Data 转为 Value。
fn data_to_value(data: &calamine::Data) -> Value {
    use calamine::Data;
    match data {
        Data::Int(i) => Value::Int(*i),
        Data::Float(f) => Value::Float(*f),
        Data::String(s) => Value::str_from(s.clone()),
        Data::Bool(b) => Value::Bool(*b),
        Data::DateTime(dt) => Value::str_from(dt.to_string()),
        Data::DateTimeIso(s) => Value::str_from(s.clone()),
        Data::DurationIso(s) => Value::str_from(s.clone()),
        Data::Empty => Value::str(""),
        Data::Error(e) => Value::str_from(format!("#ERROR:{:?}", e)),
    }
}

/// range_to_array 将 calamine Range 转为二维数组 Value。
fn range_to_array(range: &calamine::Range<calamine::Data>) -> Value {
    let rows: Vec<Value> = range
        .rows()
        .map(|row| {
            let fields: Vec<Value> = row.iter().map(data_to_value).collect();
            Value::Array(Arc::new(Mutex::new(fields)))
        })
        .collect();
    Value::Array(Arc::new(Mutex::new(rows)))
}

// ---- 内置函数 ----

/// bi_excel_new 创建空工作簿。
///
/// 用法：excelNew() → workbook
fn bi_excel_new(_vm: &mut crate::vm::VM, _args: &[Value]) -> Result<Value, Value> {
    Ok(workbook_value(rust_xlsxwriter::Workbook::new()))
}

/// bi_excel_open 打开已有 xlsx 文件（用于后续 writeSheet/SaveAs）。
///
/// 注意：rust_xlsxwriter 只能写不能读已有文件。此函数实际是创建新工作簿。
/// 读取已有文件请用 excelReadSheet。
///
/// 用法：excelOpen(path) → workbook
fn bi_excel_open(_vm: &mut crate::vm::VM, _args: &[Value]) -> Result<Value, Value> {
    // rust_xlsxwriter 不支持打开已有文件编辑，返回新工作簿
    // 用户如需读取已有文件，应使用 excelReadSheet
    Ok(workbook_value(rust_xlsxwriter::Workbook::new()))
}

/// bi_excel_save_as 保存工作簿到 xlsx 文件。
///
/// 用法：excelSaveAs(wb, path)
fn bi_excel_save_as(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "excelSaveAs")?;
    let path = bh::as_str(args, 1, "excelSaveAs")?;
    let wb = workbook_downcast(&args[0], "excelSaveAs")?;
    wb.lock()
        .unwrap()
        .save(path)
        .map_err(|e| crate::value::error_value(format!(
            "excelSaveAs() 保存 '{}' 失败: {} (可能原因：路径无效或权限不足)", path, e,
        )))?;
    Ok(Value::Undefined)
}

/// bi_excel_read_sheet 从文件读取指定 sheet 为二维数组。
///
/// 用法：
///   excelReadSheet(path)        — 默认读取第一个 sheet
///   excelReadSheet(path, index) — 按索引读取（0-based）
///   excelReadSheet(path, name)  — 按名称读取
fn bi_excel_read_sheet(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    let path = bh::as_str(args, 0, "excelReadSheet")?;

    let mut workbook: calamine::Sheets<std::io::BufReader<std::fs::File>> =
        calamine::open_workbook_auto(path).map_err(|e| {
            crate::value::error_value(format!(
                "excelReadSheet() 打开 '{}' 失败: {} (可能原因：文件不存在或不是有效 Excel 文件)",
                path, e,
            ))
        })?;

    let sheet_names = workbook.sheet_names();
    if sheet_names.is_empty() {
        return Err(crate::value::error_value("excelReadSheet() 文件中没有工作表"));
    }

    // 确定目标 sheet
    let target_name = if args.len() > 1 {
        match &args[1] {
            Value::Int(i) => {
                let idx = *i as usize;
                if idx >= sheet_names.len() {
                    return Err(crate::value::error_value(format!(
                        "excelReadSheet() sheet 索引 {} 超出范围 (共 {} 个 sheet)", i, sheet_names.len(),
                    )));
                }
                sheet_names[idx].clone()
            }
            Value::Str(s) => (**s).to_string(),
            other => return Err(crate::value::error_value(format!(
                "excelReadSheet() sheet 参数应为 int 或 string，得到 {}", other.type_name(),
            ))),
        }
    } else {
        sheet_names[0].clone()
    };

    let range = workbook.worksheet_range(&target_name).map_err(|e| {
        crate::value::error_value(format!(
            "excelReadSheet() 读取 sheet '{}' 失败: {}", target_name, e,
        ))
    })?;

    Ok(range_to_array(&range))
}

/// bi_excel_read_all 读取所有 sheets，返回 map{sheet名: 二维数组}。
///
/// 用法：excelReadAll(path) → map
fn bi_excel_read_all(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    let path = bh::as_str(args, 0, "excelReadAll")?;

    let mut workbook: calamine::Sheets<std::io::BufReader<std::fs::File>> =
        calamine::open_workbook_auto(path).map_err(|e| {
            crate::value::error_value(format!(
                "excelReadAll() 打开 '{}' 失败: {}", path, e,
            ))
        })?;

    let sheets = workbook.worksheets();
    let mut result = crate::ord_map::OrdMap::new();
    for (name, range) in sheets {
        result.set(name, range_to_array(&range));
    }

    Ok(Value::Map(Arc::new(Mutex::new(result))))
}

/// bi_excel_write_sheet 写二维数组到工作簿的指定 sheet。
///
/// 用法：
///   excelWriteSheet(wb, sheetIndex, data) — 写到指定 sheet（0-based）
///   excelWriteSheet(wb, sheetName, data)  — 写到指定名称的 sheet
///
/// data 为二维数组，每个元素自动按类型写入（int/float/string/bool）。
fn bi_excel_write_sheet(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "excelWriteSheet")?;
    bh::require_arg(args, 2, "excelWriteSheet")?;
    let wb = workbook_downcast(&args[0], "excelWriteSheet")?;

    let data = match &args[2] {
        Value::Array(a) => a.lock().unwrap().clone(),
        other => return Err(crate::value::error_value(format!(
            "excelWriteSheet() 第 3 个参数应为二维数组，得到 {}", other.type_name(),
        ))),
    };

    let mut guard = wb.lock().unwrap();

    // 获取目标 worksheet（按索引或名称）
    // 如果按索引且索引超出范围（新工作簿默认无 sheet），自动创建
    let current_count = guard.worksheets().len();
    let sheet_result = match &args[1] {
        Value::Int(i) => {
            let idx = *i as usize;
            if idx >= current_count {
                // 自动创建缺失的 sheet
                while guard.worksheets().len() <= idx {
                    guard.add_worksheet();
                }
            }
            guard.worksheet_from_index(idx)
        }
        Value::Str(s) => {
            // 按名称查找，不存在则创建
            if guard.worksheet_from_name(s.as_ref()).is_err() {
                let ws = guard.add_worksheet();
                let _ = ws.set_name(s.as_ref());
            }
            guard.worksheet_from_name(s.as_ref())
        }
        other => return Err(crate::value::error_value(format!(
            "excelWriteSheet() sheet 参数应为 int 或 string，得到 {}", other.type_name(),
        ))),
    };

    let sheet = sheet_result.map_err(|e| crate::value::error_value(format!(
        "excelWriteSheet() 获取工作表失败: {}", e,
    )))?;

    // 逐行逐列写入数据
    for (row_idx, row_val) in data.iter().enumerate() {
        match row_val {
            Value::Array(row) => {
                let fields = row.lock().unwrap();
                for (col_idx, cell) in fields.iter().enumerate() {
                    write_cell(sheet, row_idx as u32, col_idx as u16, cell)?;
                }
            }
            // 单个值（非数组）当作一行一个字段
            other => {
                write_cell(sheet, row_idx as u32, 0u16, other)?;
            }
        }
    }

    Ok(Value::Undefined)
}

/// write_cell 将一个 Value 写入 worksheet 的指定单元格。
fn write_cell(
    sheet: &mut rust_xlsxwriter::Worksheet,
    row: u32,
    col: u16,
    v: &Value,
) -> Result<(), Value> {
    let result = match v {
        Value::Int(i) => sheet.write_number(row, col, *i as f64),
        Value::Float(f) => sheet.write_number(row, col, *f),
        Value::Bool(b) => sheet.write_boolean(row, col, *b),
        Value::Byte(b) => sheet.write_number(row, col, *b as f64),
        Value::BigInt(b) => {
            // 大整数转字符串写入
            let s = b.to_string_decimal();
            sheet.write_string(row, col, &s)
        }
        Value::Str(s) => sheet.write_string(row, col, s.as_ref()),
        Value::Undefined | Value::Error(_) => {
            // undefined/error 写空字符串
            sheet.write_string(row, col, "")
        }
        // 其他类型（array/object/map 等）转 to_str
        other => sheet.write_string(row, col, &other.to_str()),
    };
    result.map_err(|e| crate::value::error_value(format!(
        "excelWriteSheet() 写入单元格 ({},{}) 失败: {}", row, col, e,
    )))?;
    Ok(())
}

/// bi_excel_new_sheet 在工作簿中新建 sheet。
///
/// 用法：excelNewSheet(wb, name) → int（新 sheet 的索引）
fn bi_excel_new_sheet(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "excelNewSheet")?;
    let name = bh::as_str(args, 1, "excelNewSheet")?;
    let wb = workbook_downcast(&args[0], "excelNewSheet")?;

    let mut guard = wb.lock().unwrap();
    let sheet = guard.add_worksheet();
    sheet.set_name(name).map_err(|e| crate::value::error_value(format!(
        "excelNewSheet() 设置名称 '{}' 失败: {}", name, e,
    )))?;

    // 返回新 sheet 的索引（最后一个）
    let index = guard.worksheets().len() as i64 - 1;
    Ok(Value::Int(index))
}
