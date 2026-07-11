//! builtins_clipboard.rs — 系统剪贴板读写内置函数
//!
//! 设计要点：
//!   - Windows：直接调用 Win32 API（OpenClipboard/GetClipboardData/SetClipboardData）
//!   - Linux：调用 xclip 或 xsel 命令（无桌面环境时返回错误）
//!   - macOS：调用 pbcopy/pbpaste 命令
//!   - 不引入第三方依赖，Windows 用已有的 windows-sys
//!
//! 函数列表：
//!   getClipText()  — 读取系统剪贴板文本（无文本则返回空字符串）
//!   setClipText(s) — 写入文本到系统剪贴板

use crate::builtins_helpers as bh;
use crate::value::Value;
use crate::vm::VM;

/// register 注册剪贴板内置函数。
pub fn register(vm: &mut VM) {
    vm.register_builtin("getClipText", bi_get_clip_text);
    vm.register_builtin("setClipText", bi_set_clip_text);
}

// ===========================================================================
// Windows 实现（Win32 API）
// ===========================================================================

#[cfg(windows)]
mod platform_clipboard {
    use crate::value::{Value, error_value};

    // CF_UNICODETEXT 常量值（Win32 剪贴板格式）
    const CF_UNICODETEXT: u32 = 13;

    /// read 读取剪贴板文本。
    pub fn read() -> Result<String, Value> {
        unsafe {
            use windows_sys::Win32::System::DataExchange::{
                OpenClipboard, CloseClipboard, GetClipboardData, IsClipboardFormatAvailable,
            };
            use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};

            if OpenClipboard(std::ptr::null_mut()) == 0 {
                return Err(error_value(
                    "getClipText() 打开剪贴板失败 (可能原因：另一程序占用剪贴板)".to_string(),
                ));
            }
            // 用闭包确保 CloseClipboard 被调用
            let result = (|| {
                if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
                    return Ok(String::new());
                }
                let handle = GetClipboardData(CF_UNICODETEXT);
                if handle.is_null() {
                    return Ok(String::new());
                }
                let ptr = GlobalLock(handle) as *const u16;
                if ptr.is_null() {
                    return Ok(String::new());
                }
                // 计算长度（UTF-16LE，以 0 结尾）
                let mut len = 0;
                while *ptr.add(len) != 0 {
                    len += 1;
                }
                let slice = std::slice::from_raw_parts(ptr, len);
                let s = String::from_utf16_lossy(slice);
                GlobalUnlock(handle);
                Ok(s)
            })();
            CloseClipboard();
            result
        }
    }

    /// write 写入文本到剪贴板。
    pub fn write(text: &str) -> Result<(), Value> {
        unsafe {
            use windows_sys::Win32::System::DataExchange::{
                OpenClipboard, CloseClipboard, EmptyClipboard, SetClipboardData,
            };
            use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

            if OpenClipboard(std::ptr::null_mut()) == 0 {
                return Err(error_value(
                    "setClipText() 打开剪贴板失败 (可能原因：另一程序占用剪贴板)".to_string(),
                ));
            }
            let result = (|| {
                EmptyClipboard();
                // 编码为 UTF-16LE（含终止符）
                let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0u16)).collect();
                let byte_len = utf16.len() * 2;
                let handle = GlobalAlloc(GMEM_MOVEABLE, byte_len);
                if handle.is_null() {
                    return Err(error_value("setClipText() 分配内存失败".to_string()));
                }
                let ptr = GlobalLock(handle) as *mut u16;
                if ptr.is_null() {
                    return Err(error_value("setClipText() 锁定内存失败".to_string()));
                }
                std::ptr::copy_nonoverlapping(utf16.as_ptr(), ptr, utf16.len());
                GlobalUnlock(handle);
                if SetClipboardData(CF_UNICODETEXT, handle).is_null() {
                    return Err(error_value("setClipText() 设置剪贴板数据失败".to_string()));
                }
                Ok(())
            })();
            CloseClipboard();
            result
        }
    }
}

// ===========================================================================
// Linux/macOS 实现（命令行工具）
// ===========================================================================

#[cfg(not(windows))]
mod platform_clipboard {
    use crate::value::{Value, error_value};

    /// read 读取剪贴板文本。
    pub fn read() -> Result<String, Value> {
        if cfg!(target_os = "macos") {
            run_command("pbpaste", &[])
        } else {
            // Linux：优先 xclip，其次 xsel
            run_command("xclip", &["-selection", "clipboard", "-o"])
                .or_else(|_| run_command("xsel", &["--clipboard", "--output"]))
        }
    }

    /// write 写入文本到剪贴板。
    pub fn write(text: &str) -> Result<(), Value> {
        if cfg!(target_os = "macos") {
            run_command_input("pbcopy", &[], text)
        } else {
            run_command_input("xclip", &["-selection", "clipboard"], text)
                .or_else(|_| run_command_input("xsel", &["--clipboard", "--input"], text))
        }
    }

    /// run_command 运行命令并返回 stdout。
    fn run_command(cmd: &str, args: &[&str]) -> Result<String, Value> {
        let output = std::process::Command::new(cmd)
            .args(args)
            .output()
            .map_err(|e| error_value(format!(
                "执行 {} 失败: {} (可能原因：未安装 {} 命令)", cmd, e, cmd,
            )))?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// run_command_input 运行命令，通过 stdin 传入文本。
    fn run_command_input(cmd: &str, args: &[&str], input: &str) -> Result<(), Value> {
        use std::io::Write;
        let mut child = std::process::Command::new(cmd)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| error_value(format!(
                "执行 {} 失败: {} (可能原因：未安装 {} 命令)", cmd, e, cmd,
            )))?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(input.as_bytes());
        }
        let status = child.wait().map_err(|e| error_value(format!(
            "等待 {} 退出失败: {}", cmd, e,
        )))?;
        if status.success() {
            Ok(())
        } else {
            Err(error_value(format!(
                "{} 退出码非零 (可能原因：命令执行错误)", cmd,
            )))
        }
    }
}

// ===========================================================================
// 内置函数
// ===========================================================================

/// bi_get_clip_text 读取系统剪贴板文本。
fn bi_get_clip_text(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    let text = platform_clipboard::read()?;
    Ok(Value::str_from(text))
}

/// bi_set_clip_text 写入文本到系统剪贴板。
fn bi_set_clip_text(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let text = bh::as_str(args, 0, "setClipText")?;
    platform_clipboard::write(text)?;
    Ok(Value::Undefined)
}
