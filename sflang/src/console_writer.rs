//! console_writer.rs — 跨平台控制台输出（Windows 用 WriteConsoleW 直接写 UTF-16）
//!
//! 设计要点：
//!   - Windows 下控制台默认使用系统代码页（中文系统为 GBK/936），
//!     而 Sflang 内部统一用 UTF-8。
//!   - Rust 1.70+ 的 std::io::stdout() 在 Windows 上会自动启用 UTF-8 控制台模式，
//!     但写入非 UTF-8 字节会报错；且旧版 Windows（如 Win10 早期）不一定支持 UTF-8 模式。
//!   - 最可靠方案：检测是否为控制台，若是则用 Windows API WriteConsoleW 直接写 UTF-16，
//!     绕过 Rust stdout 层的编码限制（对标 Go 的 WriteConsoleW 策略）。
//!   - 非 Windows 或非控制台（文件/管道）：走正常 stdout，输出 UTF-8。

use std::io::{self, Write};

/// ConsoleWriter 自动检测控制台并选择写入方式的输出封装。
pub struct ConsoleWriter {
    /// is_console 是否输出到控制台。
    is_console: bool,
}

impl ConsoleWriter {
    /// stdout 创建指向标准输出的 ConsoleWriter。
    pub fn stdout() -> Self {
        ConsoleWriter {
            is_console: is_stdout_console(),
        }
    }
}

impl Write for ConsoleWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        #[cfg(windows)]
        {
            if self.is_console {
                return write_console_w(buf);
            }
        }
        // 非控制台（文件/管道）或非 Windows：直接输出 UTF-8
        let mut stdout = io::stdout().lock();
        stdout.write(buf)?;
        stdout.flush()?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stdout().flush()
    }
}

// ---- Windows 平台 ----

#[cfg(windows)]
mod win {
    /// 检测 stdout 是否为控制台句柄。
    pub fn is_stdout_console() -> bool {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::Console::GetConsoleMode;
        let handle = io::stdout().as_raw_handle();
        unsafe {
            let mut mode = 0u32;
            GetConsoleMode(handle, &mut mode) != 0
        }
    }

    /// write_console_w 用 WriteConsoleW 直接写 UTF-16 到控制台。
    ///
    /// 绕过 Rust stdout 的编码限制。UTF-8 → UTF-16 → WriteConsoleW。
    pub fn write_console_w(utf8: &[u8]) -> std::io::Result<usize> {
        use std::io::Write;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::Console::WriteConsoleW;
        // UTF-8 → UTF-16
        let s = match std::str::from_utf8(utf8) {
            Ok(s) => s,
            Err(_) => return Ok(utf8.len()),  // 非 UTF-8，丢弃避免崩溃
        };
        let wide: Vec<u16> = s.encode_utf16().collect();
        if wide.is_empty() { return Ok(utf8.len()); }
        let handle = io::stdout().as_raw_handle();
        let mut written: u32 = 0;
        let result = unsafe {
            WriteConsoleW(
                handle,
                wide.as_ptr(),
                wide.len() as u32,
                &mut written,
                std::ptr::null(),
            )
        };
        if result == 0 {
            // WriteConsoleW 失败，回退到普通 stdout
            let mut stdout = io::stdout().lock();
            stdout.write_all(utf8)?;
            stdout.flush()?;
        }
        Ok(utf8.len())
    }

    use std::io;
}

// ---- 非 Windows 平台 ----

#[cfg(not(windows))]
mod unix {
    pub fn is_stdout_console() -> bool { false }
}

#[cfg(windows)]
use win::*;
#[cfg(not(windows))]
use unix::*;
