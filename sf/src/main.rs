// sf — Sflang 解释器主程序入口
//
// 用法：
//   sf                       启动 REPL
//   sf <script.sf> [args...] 执行脚本文件，argsG 为参数数组
//   sf -e "<code>"           执行代码字符串
//   sf --build <script.sf>   编译脚本为独立可执行文件
//   sf -h | --help | help    显示帮助
//   sf -v | --version        显示版本
//
// 自包含模式：当 sf 自身尾部嵌入了脚本时，直接执行嵌入的脚本。
//
// 设计要点（AGENTS.md）：
//   - 主程序名 sf（Windows 下 sf.exe）
//   - 无执行目标时启动 REPL
//   - 支持命令行参数（argsG 全局变量）
//   - 错误信息充分（AI 友好）
//   - 能编译脚本为单一文件的可执行文件

use std::io::{self, BufRead, Write, Read, Seek};
use std::process::ExitCode;

use sflang::value::Value;
use sflang::Sflang;

/// 嵌入脚本标记。追加到 exe 末尾：[脚本内容][脚本长度u64 LE][SFLANG_PACK]
const PACK_MAGIC: &[u8] = b"SFLANG_PACK";
const PACK_MAGIC_LEN: usize = 11;
const PACK_TRAILER_LEN: usize = PACK_MAGIC_LEN + 8; // magic + u64 长度

/// main 入口：解析命令行，分发到 REPL / 脚本执行 / 代码执行 / 打包。
fn main() -> ExitCode {
    // 优先检测：自身是否嵌入了脚本（自包含模式）
    if let Some(script) = read_embedded_script() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        return run_string(&script, "<embedded>", args);
    }

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        // 无参数：启动 REPL
        return run_repl();
    }
    match args[1].as_str() {
        "-h" | "--help" | "help" => {
            print_help();
            ExitCode::SUCCESS
        }
        "-e" | "--eval" => {
            if args.len() < 3 {
                eprintln!("错误：-e 需要一个代码参数");
                eprintln!("用法：sf -e \"<code>\"");
                return ExitCode::from(1);
            }
            let code = &args[2];
            let script_args: Vec<String> = args[3..].to_vec();
            run_string(code, "<-e>", script_args)
        }
        "--build" | "-b" => {
            // sf --build <script.sf> [--output path]
            if args.len() < 3 {
                eprintln!("错误：--build 需要一个脚本文件参数");
                eprintln!("用法：sf --build <script.sf> [--output <输出路径>]");
                return ExitCode::from(1);
            }
            let script_path = &args[2];
            // 解析 --output 参数
            let mut output_path = {
                let base = script_path.trim_end_matches(".sf");
                if cfg!(windows) {
                    format!("{}.exe", base)
                } else {
                    base.to_string()
                }
            };
            let mut i = 3;
            while i < args.len() {
                if (args[i] == "--output" || args[i] == "-o") && i + 1 < args.len() {
                    output_path = args[i + 1].clone();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            build_executable(script_path, &output_path)
        }
        "-v" | "--version" => {
            println!("sf 0.1.0 (Sflang, Rust implementation)");
            ExitCode::SUCCESS
        }
        s => {
            // 视为脚本文件
            let script_args: Vec<String> = args[2..].to_vec();
            run_file(s, script_args)
        }
    }
}

/// read_embedded_script 检测自身可执行文件尾部是否嵌入了脚本。
///
/// 格式：[脚本UTF-8字节][脚本长度 u64 LE][SFLANG_PACK]
/// 返回 None 表示不是自包含 exe。
fn read_embedded_script() -> Option<String> {
    let exe_path = std::env::current_exe().ok()?;
    let mut file = std::fs::File::open(&exe_path).ok()?;
    let file_len = file.metadata().ok()?.len() as usize;
    if file_len < PACK_TRAILER_LEN {
        return None;
    }

    // 读取尾部 PACK_TRAILER_LEN 字节
    file.seek(io::SeekFrom::Start((file_len - PACK_TRAILER_LEN) as u64)).ok()?;
    let mut trailer = vec![0u8; PACK_TRAILER_LEN];
    file.read_exact(&mut trailer).ok()?;

    // 检查 magic
    let magic = &trailer[8..];
    if magic != PACK_MAGIC {
        return None;
    }

    // 读取脚本长度
    let script_len = u64::from_le_bytes(trailer[..8].try_into().ok()?) as usize;
    if script_len == 0 || script_len > file_len - PACK_TRAILER_LEN {
        return None;
    }

    // 读取脚本内容
    let script_start = file_len - PACK_TRAILER_LEN - script_len;
    file.seek(io::SeekFrom::Start(script_start as u64)).ok()?;
    let mut script_bytes = vec![0u8; script_len];
    file.read_exact(&mut script_bytes).ok()?;

    String::from_utf8(script_bytes).ok()
}

/// build_executable 将脚本打包为独立可执行文件。
///
/// 原理：复制当前 sf.exe → 在末尾追加 [脚本内容][脚本长度u64 LE][SFLANG_PACK]
fn build_executable(script_path: &str, output_path: &str) -> ExitCode {
    // 1. 读取脚本
    let script = match std::fs::read_to_string(script_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("错误：读取脚本 '{}' 失败: {}", script_path, e);
            return ExitCode::from(1);
        }
    };

    // 2. 获取当前 exe 路径（sf.exe 自身）
    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("错误：无法确定当前可执行文件路径: {}", e);
            return ExitCode::from(1);
        }
    };

    // 3. 读取 sf.exe 全部内容
    let exe_data = match std::fs::read(&exe_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("错误：读取 '{}' 失败: {}", exe_path.display(), e);
            return ExitCode::from(1);
        }
    };

    // 4. 构建输出：exe + 脚本 + 长度 + magic
    let script_bytes = script.as_bytes();
    let script_len = script_bytes.len() as u64;

    let mut output = Vec::with_capacity(exe_data.len() + script_bytes.len() + PACK_TRAILER_LEN);
    output.extend_from_slice(&exe_data);
    output.extend_from_slice(script_bytes);
    output.extend_from_slice(&script_len.to_le_bytes());
    output.extend_from_slice(PACK_MAGIC);

    // 5. 写入输出文件
    if let Err(e) = std::fs::write(output_path, &output) {
        eprintln!("错误：写入 '{}' 失败: {}", output_path, e);
        return ExitCode::from(1);
    }

    // 6. 在非 Windows 上设置可执行权限
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = std::fs::metadata(output_path) {
            let mut perms = metadata.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(output_path, perms);
        }
    }

    let size_kb = output.len() / 1024;
    println!("已生成可执行文件: {} ({} KB)", output_path, size_kb);
    println!("嵌入脚本: {} ({} 字节)", script_path, script_bytes.len());
    ExitCode::SUCCESS
}

/// run_repl 启动交互式 REPL。
fn run_repl() -> ExitCode {
    println!("Sflang REPL 0.1.0");
    let mut sf = Sflang::new();
    sf.set_output(sflang::ConsoleWriter::stdout());
    // REPL 模式不设置 argsG/scriptPathG
    let stdin = io::stdin();
    let mut buf = String::new();
    let mut multiline = String::new();
    loop {
        // 提示符
        if multiline.is_empty() {
            print!("sf> ");
        } else {
            print!("...> ");
        }
        io::stdout().flush().ok();
        buf.clear();
        match stdin.lock().read_line(&mut buf) {
            Ok(0) => {
                // EOF
                println!();
                break;
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("读取输入失败：{}", e);
                return ExitCode::from(1);
            }
        }
        let line = buf.trim_end_matches(['\n', '\r']);
        // 处理点命令
        if multiline.is_empty() {
            let trimmed = line.trim();
            if trimmed == ".exit" || trimmed == ".quit" {
                break;
            }
            if trimmed == ".help" {
                print_repl_help();
                continue;
            }
        }
        // 多行：以 \ 结尾或括号不匹配时累积
        let line_with_nl = format!("{}\n", line);
        multiline.push_str(&line_with_nl);
        // 简单的多行判定：括号是否平衡
        if !is_balanced(&multiline) {
            // 继续读下一行
            continue;
        }
        // 执行
        let src = std::mem::take(&mut multiline);
        match sf.run_string(&src) {
            Ok(v) => {
                // 非空结果打印（顶层表达式求值）
                if !matches!(v, Value::Undefined) {
                    println!("{}", v.inspect());
                }
            }
            Err(e) => {
                eprintln!("{}", format_error(&e));
            }
        }
    }
    ExitCode::SUCCESS
}

/// is_balanced 简单括号匹配判断（用于 REPL 多行输入）。
fn is_balanced(s: &str) -> bool {
    let mut depth_paren = 0i32;
    let mut depth_brace = 0i32;
    let mut depth_bracket = 0i32;
    let mut in_str = false;
    let mut in_raw = false;
    let mut in_line_comment = false;
    let mut prev = '\0';
    for ch in s.chars() {
        if in_line_comment {
            if ch == '\n' {
                in_line_comment = false;
            }
            prev = ch;
            continue;
        }
        if in_str {
            if ch == '"' && prev != '\\' {
                in_str = false;
            }
            prev = ch;
            continue;
        }
        if in_raw {
            if ch == '`' {
                in_raw = false;
            }
            prev = ch;
            continue;
        }
        match ch {
            '#' if depth_paren == 0 && depth_brace == 0 && depth_bracket == 0 => {
                in_line_comment = true;
            }
            '"' => in_str = true,
            '`' => in_raw = true,
            '(' => depth_paren += 1,
            ')' => depth_paren -= 1,
            '{' => depth_brace += 1,
            '}' => depth_brace -= 1,
            '[' => depth_bracket += 1,
            ']' => depth_bracket -= 1,
            '\\' => {
                prev = ch;
                continue;
            }
            _ => {}
        }
        prev = ch;
    }
    // 行末 \ 视为续行
    if s.ends_with("\\\n") || s.ends_with("\\") {
        return false;
    }
    depth_paren == 0 && depth_brace == 0 && depth_bracket == 0 && !in_str && !in_raw
}

/// run_file 执行脚本文件。
fn run_file(path: &str, script_args: Vec<String>) -> ExitCode {
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("读取文件失败：{} ({})", path, e);
            eprintln!("可能原因：文件不存在；路径错误；权限不足");
            return ExitCode::from(1);
        }
    };
    run_string(&src, path, script_args)
}

/// run_string 执行代码字符串，设置 argsG/scriptPathG 全局变量。
fn run_string(src: &str, file: &str, script_args: Vec<String>) -> ExitCode {
    let mut sf = Sflang::new();
    sf.set_output(sflang::ConsoleWriter::stdout());
    // 设置预定义全局变量
    let args_val = Value::Array(std::sync::Arc::new(std::sync::Mutex::new(
        script_args.iter().map(|s| Value::str(s)).collect(),
    )));
    sf.set_global("argsG", args_val);
    sf.set_global("scriptPathG", Value::str(file));
    // 编译并执行
    let code = match Sflang::compile_source(src, file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("编译错误：{}", e);
            return ExitCode::from(1);
        }
    };
    match sf.vm_run_code(code) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{}", format_error(&e));
            ExitCode::from(1)
        }
    }
}

/// format_error 格式化错误输出（AI 友好）。
fn format_error(e: &Value) -> String {
    match e {
        Value::Error(err) => {
            if err.stack.is_empty() {
                format!("错误：{}", err.message)
            } else {
                format!("错误：{}\n调用栈：\n  {}", err.message, err.stack.join("\n  "))
            }
        }
        _ => format!("错误：{}", e.inspect()),
    }
}

/// print_help 打印主程序帮助。
fn print_help() {
    println!("sf - Sflang 解释器");
    println!();
    println!("用法：");
    println!("  sf                       启动 REPL（交互式环境）");
    println!("  sf <script.sf> [args...] 执行脚本文件，参数存入 argsG");
    println!("  sf -e \"<code>\"           执行代码字符串");
    println!("  sf --build <script.sf>   编译脚本为独立可执行文件");
    println!("      [--output <路径>]    指定输出路径");
    println!("  sf -h | --help | help    显示此帮助");
    println!("  sf -v | --version        显示版本");
    println!();
    println!("预定义全局变量：");
    println!("  piG, eG       数学常量");
    println!("  argsG         命令行参数数组（脚本/REPL 可用）");
    println!("  scriptPathG   脚本路径");
    println!();
    println!("注释：// 行注释、/* */ 块注释");
    println!("逻辑：&& || !（无 and/or/not 关键字）");
    println!("空值：undefined（无 nil）");
    println!();
    println!("19 种类型：int float bool byte string bytes byteArray");
    println!("  array object map function builtin error native");
    println!("  bigInt bigFloat datetime file undefined");
    println!();
    println!("脚本示例：");
    println!("  println(\"Hello, Sflang!\")");
    println!("  for i in range(1, 10) {{");
    println!("      println(i)");
    println!("  }}");
}

/// print_repl_help 打印 REPL 帮助。
fn print_repl_help() {
    println!("REPL 命令：");
    println!("  .exit / .quit  退出 REPL");
    println!("  .help          显示此帮助");
    println!();
    println!("多行输入：括号未闭合或行末 \\ 时自动续行");
    println!("顶层表达式求值会自动打印结果");
    println!();
    println!("注释：// 和 /* */");
    println!("逻辑：&& || !");
    println!("空值：undefined");
    println!("字符串：\"双引号\" `反引号` \"\"\"三引号\"\"\"");
}
