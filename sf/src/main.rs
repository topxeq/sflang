// sf — Sflang 解释器主程序入口
//
// 用法：
//   sf                       启动 REPL
//   sf <script.sf> [args...] 执行脚本文件，argsG 为参数数组
//   sf -e "<code>"           执行代码字符串
//   sf -h | --help | help    显示帮助
//
// 设计要点（AGENTS.md）：
//   - 主程序名 sf（Windows 下 sf.exe）
//   - 无执行目标时启动 REPL
//   - 支持命令行参数（argsG 全局变量）
//   - 错误信息充分（AI 友好）

use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use sflang::value::Value;
use sflang::Sflang;

/// main 入口：解析命令行，分发到 REPL / 脚本执行 / 代码执行。
fn main() -> ExitCode {
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
    println!("sf - Sflang 解释器 (Rust 实现)");
    println!();
    println!("用法：");
    println!("  sf                       启动 REPL（交互式环境）");
    println!("  sf <script.sf> [args...] 执行脚本文件，参数存入 argsG");
    println!("  sf -e \"<code>\"           执行代码字符串");
    println!("  sf -h | --help | help    显示此帮助");
    println!("  sf -v | --version        显示版本");
    println!();
    println!("预定义全局变量：");
    println!("  argsG        命令行参数数组");
    println!("  scriptPathG  脚本路径（REPL 模式为空）");
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
}
