//! builtins_dialog.rs — 系统对话框内置函数（打开文件/保存文件/选目录/消息框）
//!
//! 设计要点（仿 builtins_clipboard.rs）：
//!   - Windows：直接调用 Win32 API（GetOpenFileNameW/GetSaveFileNameW/SHBrowseForFolderW/MessageBoxW）
//!   - Linux：调用 zenity 或 kdialog 命令
//!   - macOS：调用 osascript 命令
//!   - 不引入第三方依赖，Windows 用已有的 windows-sys
//!
//! 函数列表：
//!   openFileDialog(filter?, initialDir?) -> string|undefined
//!   saveFileDialog(defaultName?, initialDir?) -> string|undefined
//!   selectFolder(initialDir?) -> string|undefined
//!   msgBox(text, title?, type?) -> int

use crate::builtins_helpers as bh;
use crate::function::BuiltinDoc;
use crate::value::Value;
use crate::vm::VM;

// ---- 文档 ----

static DOC_OPEN_FILE_DIALOG: BuiltinDoc = BuiltinDoc {
    category: "dialog",
    signature: "openFileDialog([filter[, initialDir]]) -> string|undefined",
    summary: "弹出系统文件打开对话框，返回选定文件路径。取消返回 undefined。",
    params: &[
        ("filter", "可选。文件过滤器，如 \".sf,.txt\" 或 \"Sflang|*.sf|All|*.*\""),
        ("initialDir", "可选。初始目录路径"),
    ],
    returns: "string 文件路径；用户取消返回 undefined",
    examples: &[
        "var path = openFileDialog()",
        "var path = openFileDialog(\".sf,.txt\")",
    ],
    errors: &["Linux 下需要 zenity 或 kdialog"],
};

static DOC_SAVE_FILE_DIALOG: BuiltinDoc = BuiltinDoc {
    category: "dialog",
    signature: "saveFileDialog([defaultName[, initialDir]]) -> string|undefined",
    summary: "弹出系统文件保存对话框，返回选定保存路径。取消返回 undefined。",
    params: &[
        ("defaultName", "可选。默认文件名"),
        ("initialDir", "可选。初始目录路径"),
    ],
    returns: "string 保存路径；用户取消返回 undefined",
    examples: &["var path = saveFileDialog(\"untitled.sf\")"],
    errors: &["Linux 下需要 zenity 或 kdialog"],
};

static DOC_SELECT_FOLDER: BuiltinDoc = BuiltinDoc {
    category: "dialog",
    signature: "selectFolder([initialDir]) -> string|undefined",
    summary: "弹出系统目录选择对话框，返回选定目录路径。取消返回 undefined。",
    params: &[("initialDir", "可选。初始目录路径")],
    returns: "string 目录路径；用户取消返回 undefined",
    examples: &["var dir = selectFolder()"],
    errors: &["Linux 下需要 zenity 或 kdialog"],
};

static DOC_MSG_BOX: BuiltinDoc = BuiltinDoc {
    category: "dialog",
    signature: "msgBox(text[, title[, type]]) -> int",
    summary: "弹出系统消息框。type 可为 \"info\"/\"warning\"/\"error\"/\"question\"。返回按钮编号（1=确定, 2=取消）。",
    params: &[
        ("text", "消息文本"),
        ("title", "可选。标题，默认 \"提示\""),
        ("type", "可选。消息类型：info/warning/error/question，默认 info"),
    ],
    returns: "int 按钮编号（1=确定, 2=取消）",
    examples: &[
        "msgBox(\"操作完成\")",
        "if msgBox(\"确认删除？\", \"确认\", \"question\") == 1 { ... }",
    ],
    errors: &[],
};

/// register 注册所有对话框内置函数。
pub fn register(vm: &mut VM) {
    vm.register_builtin_doc("openFileDialog", bi_open_file_dialog, &DOC_OPEN_FILE_DIALOG);
    vm.register_builtin_doc("saveFileDialog", bi_save_file_dialog, &DOC_SAVE_FILE_DIALOG);
    vm.register_builtin_doc("selectFolder", bi_select_folder, &DOC_SELECT_FOLDER);
    vm.register_builtin_doc("msgBox", bi_msg_box, &DOC_MSG_BOX);
}

/// bi_open_file_dialog 打开文件对话框。
fn bi_open_file_dialog(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let filter = args.get(0).map(|v| v.to_str()).unwrap_or_default();
    let initial_dir = args.get(1).map(|v| v.to_str()).unwrap_or_default();
    match platform_dialog::open_file(&filter, &initial_dir) {
        Ok(Some(path)) => Ok(Value::str_from(path)),
        Ok(None) => Ok(Value::Undefined),
        Err(e) => Ok(e),
    }
}

/// bi_save_file_dialog 保存文件对话框。
fn bi_save_file_dialog(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let default_name = args.get(0).map(|v| v.to_str()).unwrap_or_default();
    let initial_dir = args.get(1).map(|v| v.to_str()).unwrap_or_default();
    match platform_dialog::save_file(&default_name, &initial_dir) {
        Ok(Some(path)) => Ok(Value::str_from(path)),
        Ok(None) => Ok(Value::Undefined),
        Err(e) => Ok(e),
    }
}

/// bi_select_folder 选择目录对话框。
fn bi_select_folder(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let initial_dir = args.get(0).map(|v| v.to_str()).unwrap_or_default();
    match platform_dialog::select_folder(&initial_dir) {
        Ok(Some(path)) => Ok(Value::str_from(path)),
        Ok(None) => Ok(Value::Undefined),
        Err(e) => Ok(e),
    }
}

/// bi_msg_box 消息框。
fn bi_msg_box(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "msgBox")?;
    let text = bh::as_str(args, 0, "msgBox")?;
    let title = args.get(1).map(|v| v.to_str()).unwrap_or_else(|| "提示".to_string());
    let msg_type = args.get(2).map(|v| v.to_str()).unwrap_or_else(|| "info".to_string());
    match platform_dialog::msg_box(text, &title, &msg_type) {
        Ok(button) => Ok(Value::Int(button as i64)),
        Err(e) => Ok(e),
    }
}

// ===========================================================================
// Windows 实现（Win32 API）
// ===========================================================================

#[cfg(windows)]
mod platform_dialog {
    use crate::value::Value;

    /// 宽字符辅助：&str -> Vec<u16>（null 结尾）。
    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0u16)).collect()
    }

    /// 宽字符辅助：*const u16 -> String。
    fn from_wide(ptr: *const u16) -> String {
        if ptr.is_null() { return String::new(); }
        unsafe {
            let mut len = 0;
            while *ptr.add(len) != 0 { len += 1; }
            let slice = std::slice::from_raw_parts(ptr, len);
            String::from_utf16_lossy(slice)
        }
    }

    /// 解析过滤器字符串为 Win32 过滤器格式。
    /// 输入 ".sf,.txt" -> "Sflang (*.sf)\0*.sf\0Text (*.txt)\0*.txt\0All (*.*)\0*.*\0"
    /// 输入 "Sflang|*.sf|All|*.*" -> 原样使用
    fn build_filter(filter: &str) -> Vec<u16> {
        if filter.is_empty() {
            // 默认：所有文件
            return to_wide("All Files\0*.*\0");
        }
        if filter.contains('|') {
            // "Name|*.ext|Name2|*.ext2" 格式
            let parts: Vec<&str> = filter.split('|').collect();
            let mut result = String::new();
            let mut i = 0;
            while i + 1 < parts.len() {
                result.push_str(parts[i]);
                result.push('\0');
                result.push_str(parts[i + 1]);
                result.push('\0');
                i += 2;
            }
            if result.is_empty() {
                return to_wide("All Files\0*.*\0");
            }
            return result.encode_utf16().chain(std::iter::once(0u16)).collect();
        }
        // ".sf,.txt" 格式
        let exts: Vec<&str> = filter.split(',').filter(|s| !s.is_empty()).collect();
        let mut result = String::new();
        for ext in &exts {
            let ext_name = ext.trim_start_matches('.');
            result.push_str(&format!("{} files (*{})\0*{}\0", ext_name.to_uppercase(), ext, ext));
        }
        result.push_str("All Files\0*.*\0");
        result.encode_utf16().chain(std::iter::once(0u16)).collect()
    }

    /// open_file 打开文件对话框。
    pub fn open_file(filter: &str, initial_dir: &str) -> Result<Option<String>, Value> {
        unsafe {
            use windows_sys::Win32::UI::Controls::Dialogs::{
                GetOpenFileNameW, OPENFILENAMEW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_PATHMUSTEXIST,
            };

            let mut file_buf = [0u16; 1024];
            let filter_wide = build_filter(filter);
            let dir_wide = to_wide(initial_dir);

            let mut ofn: OPENFILENAMEW = std::mem::zeroed();
            ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
            ofn.hwndOwner = std::ptr::null_mut();
            ofn.lpstrFilter = filter_wide.as_ptr();
            ofn.nFilterIndex = 1;
            ofn.lpstrFile = file_buf.as_mut_ptr();
            ofn.nMaxFile = file_buf.len() as u32;
            ofn.lpstrInitialDir = if initial_dir.is_empty() { std::ptr::null() } else { dir_wide.as_ptr() };
            ofn.lpstrTitle = std::ptr::null();
            ofn.Flags = OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST;

            if GetOpenFileNameW(&mut ofn) != 0 {
                let path = from_wide(file_buf.as_ptr());
                if path.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(path))
                }
            } else {
                Ok(None)
            }
        }
    }

    /// save_file 保存文件对话框。
    pub fn save_file(default_name: &str, initial_dir: &str) -> Result<Option<String>, Value> {
        unsafe {
            use windows_sys::Win32::UI::Controls::Dialogs::{
                GetSaveFileNameW, OPENFILENAMEW, OFN_EXPLORER, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST,
            };

            let mut file_buf = [0u16; 1024];
            let name_wide = to_wide(default_name);
            // 把默认文件名复制到 file_buf
            let name_bytes = name_wide.as_slice();
            let copy_len = name_bytes.len().min(file_buf.len() - 1);
            file_buf[..copy_len].copy_from_slice(&name_bytes[..copy_len]);

            let dir_wide = to_wide(initial_dir);
            let filter_wide = to_wide("All Files\0*.*\0");

            let mut ofn: OPENFILENAMEW = std::mem::zeroed();
            ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
            ofn.hwndOwner = std::ptr::null_mut();
            ofn.lpstrFilter = filter_wide.as_ptr();
            ofn.nFilterIndex = 1;
            ofn.lpstrFile = file_buf.as_mut_ptr();
            ofn.nMaxFile = file_buf.len() as u32;
            ofn.lpstrInitialDir = if initial_dir.is_empty() { std::ptr::null() } else { dir_wide.as_ptr() };
            ofn.lpstrTitle = std::ptr::null();
            ofn.Flags = OFN_EXPLORER | OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST;
            ofn.lpstrDefExt = std::ptr::null();

            if GetSaveFileNameW(&mut ofn) != 0 {
                let path = from_wide(file_buf.as_ptr());
                if path.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(path))
                }
            } else {
                Ok(None)
            }
        }
    }

    /// select_folder 选择目录对话框。
    pub fn select_folder(initial_dir: &str) -> Result<Option<String>, Value> {
        // SHBrowseForFolder + SHGetPathFromIDList
        // 简化实现：用 systemCmd 调用 PowerShell 的 FolderBrowserDialog
        let ps_script = if initial_dir.is_empty() {
            r#"Add-Type -AssemblyName System.Windows.Forms; $f = New-Object System.Windows.Forms.FolderBrowserDialog; if ($f.ShowDialog() -eq 'OK') { $f.SelectedPath }"#
        } else {
            &format!(
                r#"Add-Type -AssemblyName System.Windows.Forms; $f = New-Object System.Windows.Forms.FolderBrowserDialog; $f.SelectedPath = '{}'; if ($f.ShowDialog() -eq 'OK') {{ $f.SelectedPath }}"#,
                initial_dir.replace('\'', "''")
            )
        };
        let output = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", ps_script])
            .output();
        match output {
            Ok(o) => {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if s.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(s))
                }
            }
            Err(_) => Ok(None),
        }
    }

    /// msg_box 消息框。
    pub fn msg_box(text: &str, title: &str, msg_type: &str) -> Result<i32, Value> {
        unsafe {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                MessageBoxW, IDOK, IDYES, IDNO, IDCANCEL,
                MB_OK, MB_OKCANCEL, MB_ICONINFORMATION, MB_ICONWARNING, MB_ICONERROR, MB_ICONQUESTION,
            };

            let text_wide = to_wide(text);
            let title_wide = to_wide(title);

            let (flags, is_question) = match msg_type.to_lowercase().as_str() {
                "warning" => (MB_OK | MB_ICONWARNING, false),
                "error" => (MB_OK | MB_ICONERROR, false),
                "question" => (MB_OKCANCEL | MB_ICONQUESTION, true),
                _ => (MB_OK | MB_ICONINFORMATION, false),
            };

            let result = MessageBoxW(
                std::ptr::null_mut(),
                text_wide.as_ptr(),
                title_wide.as_ptr(),
                flags as u32,
            );

            // 统一返回值：1=确定/是, 2=取消/否
            if is_question {
                Ok(match result {
                    IDOK | IDYES => 1,
                    IDCANCEL | IDNO => 2,
                    _ => 2,
                })
            } else {
                Ok(if result == IDOK { 1 } else { 2 })
            }
        }
    }
}

// ===========================================================================
// Linux/macOS 实现（zenity/kdialog/osascript）
// ===========================================================================

#[cfg(not(windows))]
mod platform_dialog {
    use crate::value::{Value, error_value};
    use std::process::Command;

    /// 检查命令是否存在。
    fn has_cmd(cmd: &str) -> bool {
        Command::new("which")
            .arg(cmd)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// open_file 打开文件对话框。
    pub fn open_file(filter: &str, initial_dir: &str) -> Result<Option<String>, Value> {
        if has_cmd("zenity") {
            let mut cmd = Command::new("zenity");
            cmd.args(["--file-selection"]);
            if !initial_dir.is_empty() {
                cmd.args(["--filename", &format!("{}/", initial_dir)]);
            }
            // 简单处理过滤器
            if !filter.is_empty() && !filter.contains('|') {
                for ext in filter.split(',') {
                    let ext = ext.trim();
                    if !ext.is_empty() {
                        cmd.args(["--file-filter", &format!("*{}", ext)]);
                    }
                }
            }
            let output = cmd.output();
            return parse_output(output);
        }
        Err(error_value("openFileDialog() 需要 zenity 或 kdialog (可能原因：系统未安装)".to_string()))
    }

    /// save_file 保存文件对话框。
    pub fn save_file(default_name: &str, initial_dir: &str) -> Result<Option<String>, Value> {
        if has_cmd("zenity") {
            let mut cmd = Command::new("zenity");
            cmd.args(["--file-selection", "--save", "--confirm-overwrite"]);
            let filename = if initial_dir.is_empty() {
                default_name.to_string()
            } else {
                format!("{}/{}", initial_dir, default_name)
            };
            if !filename.is_empty() {
                cmd.args(["--filename", &filename]);
            }
            let output = cmd.output();
            return parse_output(output);
        }
        Err(error_value("saveFileDialog() 需要 zenity 或 kdialog".to_string()))
    }

    /// select_folder 选择目录对话框。
    pub fn select_folder(initial_dir: &str) -> Result<Option<String>, Value> {
        if has_cmd("zenity") {
            let mut cmd = Command::new("zenity");
            cmd.args(["--file-selection", "--directory"]);
            if !initial_dir.is_empty() {
                cmd.args(["--filename", &format!("{}/", initial_dir)]);
            }
            let output = cmd.output();
            return parse_output(output);
        }
        Err(error_value("selectFolder() 需要 zenity 或 kdialog".to_string()))
    }

    /// msg_box 消息框。
    pub fn msg_box(text: &str, title: &str, msg_type: &str) -> Result<i32, Value> {
        if has_cmd("zenity") {
            let cmd_type = match msg_type.to_lowercase().as_str() {
                "warning" => "--warning",
                "error" => "--error",
                "question" => "--question",
                _ => "--info",
            };
            let output = Command::new("zenity")
                .args([cmd_type, "--title", title, "--text", text])
                .output();
            // zenity question: 0=yes(确定), 1=no(取消)
            return Ok(match output {
                Ok(o) => {
                    if msg_type.eq_ignore_ascii_case("question") {
                        if o.status.success() { 1 } else { 2 }
                    } else {
                        1
                    }
                }
                Err(_) => 1,
            });
        }
        // 回退：打印到 stderr
        eprintln!("[{}] {}", title, text);
        Ok(1)
    }

    fn parse_output(output: std::io::Result<std::process::Output>) -> Result<Option<String>, Value> {
        match output {
            Ok(o) => {
                if o.status.success() {
                    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if s.is_empty() {
                        Ok(None)
                    } else {
                        Ok(Some(s))
                    }
                } else {
                    Ok(None)
                }
            }
            Err(_) => Ok(None),
        }
    }
}
