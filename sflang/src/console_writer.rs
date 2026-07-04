//! console_writer.rs — 跨平台控制台输出（Windows 自动编码转换）
//!
//! 设计要点：
//!   - Windows 下控制台默认使用系统代码页（中文系统为 GBK/936），
//!     而 Sflang 内部统一用 UTF-8。直接写 UTF-8 字节到控制台会乱码。
//!   - 本模块检测输出目标是否为控制台（Console），若是则将 UTF-8 转为
//!     系统代码页再输出（对标 Go 的行为）；若为文件/管道则保持 UTF-8。
//!   - Linux/Mac 终端默认 UTF-8，无需转换，直接输出。

use std::io::{self, Write};

/// ConsoleWriter 自动检测控制台并做编码转换的输出封装。
pub struct ConsoleWriter {
    /// is_console 是否输出到控制台（Windows 下需编码转换）。
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
                let converted = utf8_to_console_encoding(buf);
                let mut stdout = io::stdout().lock();
                stdout.write_all(&converted)?;
                stdout.flush()?;
                return Ok(buf.len());
            }
        }
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
    use std::io;
    use std::os::windows::io::AsRawHandle;

    /// 检测 stdout 是否为控制台句柄。
    pub fn is_stdout_console() -> bool {
        use windows_sys::Win32::System::Console::GetConsoleMode;
        let handle = io::stdout().as_raw_handle();
        unsafe {
            let mut mode = 0u32;
            GetConsoleMode(handle, &mut mode) != 0
        }
    }

    /// 将 UTF-8 字节转为控制台当前代码页编码。
    pub fn utf8_to_console_encoding(utf8: &[u8]) -> Vec<u8> {
        let s = match std::str::from_utf8(utf8) {
            Ok(s) => s,
            Err(_) => return utf8.to_vec(),
        };
        let cp = get_console_output_cp();
        if cp == 65001 {
            return utf8.to_vec();  // 已是 UTF-8 代码页
        }
        string_to_codepage(s, cp)
    }

    fn get_console_output_cp() -> u32 {
        use windows_sys::Win32::System::Console::GetConsoleOutputCP;
        unsafe { GetConsoleOutputCP() }
    }

    fn string_to_codepage(s: &str, cp: u32) -> Vec<u8> {
        use windows_sys::Win32::Globalization::WideCharToMultiByte;
        let wide: Vec<u16> = s.encode_utf16().collect();
        if wide.is_empty() { return Vec::new(); }
        let needed = unsafe {
            WideCharToMultiByte(
                cp, 0,
                wide.as_ptr(), wide.len() as i32,
                std::ptr::null_mut(), 0,
                std::ptr::null(), std::ptr::null_mut(),
            )
        };
        if needed <= 0 { return s.as_bytes().to_vec(); }
        let mut buf = vec![0u8; needed as usize];
        let written = unsafe {
            WideCharToMultiByte(
                cp, 0,
                wide.as_ptr(), wide.len() as i32,
                buf.as_mut_ptr(), needed,
                std::ptr::null(), std::ptr::null_mut(),
            )
        };
        if written > 0 {
            buf.truncate(written as usize);
            buf
        } else {
            s.as_bytes().to_vec()
        }
    }
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
