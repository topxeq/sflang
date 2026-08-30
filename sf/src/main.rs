// sf — Sflang 解释器主程序入口
//
// 用法：
//   sf                       启动 REPL
//   sf <script.sf> [args...] 执行脚本文件，argsG 为参数数组
//   sf -e "<code>"           执行代码字符串
//   sf --remote <url>        从 URL 下载并执行脚本
//   sf --cloud <name>        从云端执行脚本（基础 URL 配置于 ~/.sf/cloud.cfg）
//   sf -server [options]     启动 HTTP 应用服务器
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
use std::path::PathBuf;
use std::process::ExitCode;

use sflang::value::Value;
use sflang::Sflang;

/// 嵌入脚本标记。追加到 exe 末尾：[脚本内容][脚本长度u64 LE][SFLANG_PACK]
const PACK_MAGIC: &[u8] = b"SFLANG_PACK";
const PACK_MAGIC_LEN: usize = 11;
const PACK_TRAILER_LEN: usize = PACK_MAGIC_LEN + 8; // magic + u64 长度

/// main 入口：在大栈线程中执行主逻辑。
///
/// VM 的函数调用通过 Rust 递归实现（run_frame → do_call → run_frame），
/// 每层函数调用消耗一个 OS 栈帧。默认线程栈（Windows 1MB 主线程 / 8MB 子线程）
/// 在递归约 200-300 层时就会溢出，远早于 max_call_depth 的逻辑保护。
///
/// 此处在 32MB 栈的子线程中执行主逻辑，使 max_call_depth=500 能真正可达。
/// run 启动的并发子线程仍用默认 8MB 栈（避免高并发时地址空间膨胀）。
fn main() -> ExitCode {
    // 用大栈线程执行，避免深递归时 OS 栈溢出
    let result = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024) // 32MB，足够 500 层递归
        .spawn(real_main)
        .expect("failed to spawn main thread");
    result.join().unwrap_or(ExitCode::from(1))
}

/// real_main 实际的入口逻辑：解析命令行，分发到 REPL / 脚本执行 / 代码执行 / 打包。
fn real_main() -> ExitCode {
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
        "-server" | "--server" => {
            // 启动 HTTP 应用服务器
            let server_args: Vec<String> = args[1..].to_vec();
            let code = sflang::builtins_http::run_server_cli(&server_args);
            // i32 → u8 转换：负数/超 255 的值按惯例收敛为 1，避免静默截断
            u8::try_from(code).map(ExitCode::from).unwrap_or(ExitCode::from(1))
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
            // 默认输出：脚本名去一次 .sf 后缀（用 file_stem，避免 trim_end_matches
            // 把 test.sf.sf 削成 test），Windows 加 .exe
            let mut output_path = {
                let p = std::path::Path::new(script_path);
                let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("sflang_out");
                if cfg!(windows) {
                    format!("{}.exe", stem)
                } else {
                    stem.to_string()
                }
            };
            let mut i = 3;
            while i < args.len() {
                if args[i] == "--output" || args[i] == "-o" {
                    if i + 1 < args.len() {
                        output_path = args[i + 1].clone();
                        i += 2;
                    } else {
                        eprintln!("错误：--output 需要一个路径参数");
                        return ExitCode::from(1);
                    }
                } else {
                    i += 1;
                }
            }
            build_executable(script_path, &output_path)
        }
        "--remote" | "-remote" => {
            // 从 URL 下载脚本并执行
            if args.len() < 3 {
                eprintln!("错误：--remote 需要一个 URL 参数");
                eprintln!("用法：sf --remote https://example.com/scripts/basic.sf");
                return ExitCode::from(1);
            }
            let script_args: Vec<String> = args[3..].to_vec();
            run_remote(&args[2], script_args)
        }
        "--cloud" | "-cloud" => {
            // 从云端执行：基础 URL 配置于 ~/.sf/cloud.cfg
            if args.len() < 3 {
                eprintln!("错误：--cloud 需要一个脚本名参数");
                eprintln!("用法：sf --cloud basic.sf");
                eprintln!("说明：需先在用户目录 .sf 下创建 cloud.cfg，内容为云端基础 URL，");
                eprintln!("      例如 {} 下的 cloud.cfg 内容为 https://script.example.com/ ，", sf_home_dir().display());
                eprintln!("      则 sf --cloud basic.sf 等同于 sf --remote https://script.example.com/basic.sf");
                return ExitCode::from(1);
            }
            let name = args[2].trim();
            if name.is_empty() {
                eprintln!("错误：--cloud 的脚本名不能为空");
                return ExitCode::from(1);
            }
            let script_args: Vec<String> = args[3..].to_vec();
            match load_cloud_base_url() {
                Ok(base) => {
                    let url = join_cloud_url(&base, name);
                    run_remote(&url, script_args)
                }
                Err(msg) => {
                    eprintln!("{}", msg);
                    ExitCode::from(1)
                }
            }
        }
        "-v" | "--version" => {
            println!("sf {} (Sflang, Rust implementation)", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        "--list-builtins" | "-lb" => {
            // 列出所有内置函数（按分类）
            // 可选第二参数筛选分类：sf --list-builtins regex
            let filter = args.get(2).map(|s| s.as_str());
            list_builtins(filter)
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
    // 0. 安全检查：输出路径不得与脚本同一路径（否则写回会销毁源脚本）
    let canon_out = std::fs::canonicalize(output_path).ok()
        .or_else(|| std::path::Path::new(output_path).canonicalize().ok());
    let canon_src = std::fs::canonicalize(script_path).ok();
    if let (Some(a), Some(b)) = (&canon_out, &canon_src) {
        if a == b {
            eprintln!("错误：输出路径与脚本路径相同（{}），拒绝打包以免覆盖源脚本", output_path);
            return ExitCode::from(1);
        }
    }
    // 输出路径已存在且是目录也直接拒绝
    if std::path::Path::new(output_path).is_dir() {
        eprintln!("错误：输出路径 '{}' 是一个目录", output_path);
        return ExitCode::from(1);
    }

    // 1. 读取脚本
    let script = match std::fs::read_to_string(script_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("错误：读取脚本 '{}' 失败: {}", script_path, e);
            return ExitCode::from(1);
        }
    };
    if script.trim().is_empty() {
        eprintln!("错误：脚本 '{}' 内容为空，拒绝打包（空脚本的 exe 无法自识别，会退化为 REPL）", script_path);
        return ExitCode::from(1);
    }

    // 1.5 打包前先校验脚本可编译（语法错误尽早暴露给打包者，而不是最终用户）
    if let Err(e) = sflang::api::Sflang::compile_source(&script, script_path) {
        eprintln!("错误：脚本编译失败，未打包。{}", e);
        return ExitCode::from(1);
    }

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

    let size_kb = (output.len() + 1023) / 1024; // 向上取整，避免 1023KB 显示为 0
    println!("已生成可执行文件: {} ({} KB)", output_path, size_kb);
    println!("嵌入脚本: {} ({} 字节)", script_path, script_bytes.len());
    ExitCode::SUCCESS
}

/// run_repl 启动交互式 REPL。
fn run_repl() -> ExitCode {
    println!("Sflang REPL {}（.help 查看帮助；exit 或 Ctrl-D 退出）", env!("CARGO_PKG_VERSION"));
    let mut sf = Sflang::new();
    sf.set_output(sflang::ConsoleWriter::stdout());
    // REPL 模式设置空 argsG（帮助文档承诺 argsG 在 REPL 可用；此处无脚本参数）
    sf.set_global("argsG", Value::Array(std::sync::Arc::new(std::sync::Mutex::new(Vec::new()))));
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
            // 退出命令：除点命令外，接受裸 exit/quit/q（容忍结尾分号）。
            // 这些裸词在脚本语义里只会求值为 undefined 而静默无输出，单独成行时
            // 按退出意图处理（对齐 Python/node 等常见 REPL 习惯，避免用户被"卡住"）。
            let bare = trimmed.trim_end_matches(';').trim_end();
            if trimmed == ".exit"
                || trimmed == ".quit"
                || bare == "exit"
                || bare == "quit"
                || bare == "q"
            {
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

/// is_balanced 判断 REPL 输入是否完整（用于多行累积）。
///
/// 复用真正的词法器而不是手写状态机（手写版不识别 //、/\* \*/ 注释与
/// \\\\ 双反斜杠转义，注释里的括号会被误计数导致 REPL 卡死）：
///   - 词法错误为"未闭合"类（字符串/注释/插值）→ 继续读下一行
///   - 词法成功 → 按 Token 统计括号配对，未配对则继续读
///   - 其他词法错误（非法字符等）→ 视为完整（提交执行，把错误显示给用户）
/// 行末 \ 也视为续行。
fn is_balanced(s: &str) -> bool {
    // 行末 \ 视为续行
    if s.ends_with("\\\n") || s.ends_with("\\") {
        return false;
    }
    match sflang::lexer::tokenize(s, "<repl>") {
        Err(e) => {
            // 未闭合类错误 → 继续读；其他错误 → 交给执行阶段显示
            let m = e.msg;
            !(m.contains("unterminated") || m.contains("未闭合"))
        }
        Ok(tokens) => {
            let (mut paren, mut brace, mut bracket) = (0i32, 0i32, 0i32);
            for t in &tokens {
                use sflang::token::TokenKind;
                match t.kind {
                    TokenKind::LParen => paren += 1,
                    TokenKind::RParen => paren -= 1,
                    TokenKind::LBrace => brace += 1,
                    TokenKind::RBrace => brace -= 1,
                    TokenKind::LBracket => bracket += 1,
                    TokenKind::RBracket => bracket -= 1,
                    _ => {}
                }
            }
            paren == 0 && brace == 0 && bracket == 0
        }
    }
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

// ---- 云端脚本（--cloud / --remote） ----
//
// 配置约定（对标 Charlang 的 ~/.char/cloud.cfg，sflang 使用用户目录下的 .sf）：
//   ~/.sf/cloud.cfg 内容为云端基础 URL，如 `https://script.example.com/`，
//   则 `sf --cloud basic.sf` 等同于 `sf --remote https://script.example.com/basic.sf`。

/// sf_home_dir 返回 sflang 配置目录：用户主目录下的 `.sf`。
///
/// Windows 下如 `C:\Users\<用户>\.sf`，Linux 下如 `/home/<用户>/.sf`。
/// 不自动创建目录；由需要写入的一方负责创建。
fn sf_home_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
        .unwrap_or_else(|| ".".to_string());
    PathBuf::from(home).join(".sf")
}

/// load_cloud_base_url 读取云端基础 URL 配置（~/.sf/cloud.cfg）。
///
/// 失败时返回带修复指引的错误信息（AI 友好）。
fn load_cloud_base_url() -> Result<String, String> {
    let cfg_path = sf_home_dir().join("cloud.cfg");
    if !cfg_path.exists() {
        // 顺带提示目录是否存在，帮助定位问题
        let dir = sf_home_dir();
        let dir_state = if dir.exists() { "已存在" } else { "不存在" };
        return Err(format!(
            "错误：未找到云端配置文件 {}\n\
             可能原因：尚未创建该文件；配置目录{}；路径拼写错误\n\
             解决方法：创建文件 {} ，内容为一行云端基础 URL，例如：\n\
             \x20   https://script.example.com/\n\
             然后即可运行：sf --cloud basic.sf",
            cfg_path.display(), dir_state, cfg_path.display(),
        ));
    }
    let content = std::fs::read_to_string(&cfg_path).map_err(|e| {
        format!(
            "错误：读取云端配置文件 {} 失败: {}\n可能原因：权限不足；文件被占用；编码不是 UTF-8",
            cfg_path.display(), e,
        )
    })?;
    match parse_cfg_content(&content) {
        Some(url) => Ok(url),
        None => Err(format!(
            "错误：云端配置文件 {} 中没有有效内容（全部为空行或注释）\n\
             解决方法：文件内容应为一行基础 URL，例如 https://script.example.com/",
            cfg_path.display(),
        )),
    }
}

/// parse_cfg_content 从配置文件内容中提取有效值。
///
/// 规则：取第一个非空行并去除首尾空白；以 `#` 开头的行视为注释跳过；
/// 忽略 UTF-8 BOM。不做行内注释截断，避免破坏 URL 中的 `//` 与 `#`。
/// 无有效内容返回 None。
fn parse_cfg_content(content: &str) -> Option<String> {
    let content = content.trim_start_matches('\u{feff}');
    for line in content.lines() {
        let val = line.trim();
        if val.is_empty() || val.starts_with('#') {
            continue;
        }
        return Some(val.to_string());
    }
    None
}

/// join_cloud_url 拼接云端基础 URL 与脚本名。
///
/// base 末尾多余的 `/` 与 name 开头的 `/` 不会产生双斜杠。
fn join_cloud_url(base: &str, name: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), name.trim_start_matches('/'))
}

/// run_remote 从 URL 下载脚本并执行（--remote 与 --cloud 的公共路径）。
///
/// scriptPathG 设为完整 URL，便于脚本内引用自身来源。
fn run_remote(url: &str, script_args: Vec<String>) -> ExitCode {
    let resp = match sflang::http_lite::http_get(url, &[], 30) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("下载脚本失败：{} ({})", url, e);
            eprintln!("可能原因：URL 格式错误；网络不通；DNS 解析失败；TLS 证书验证失败；服务器超时");
            return ExitCode::from(1);
        }
    };
    if resp.status < 200 || resp.status >= 400 {
        eprintln!("下载脚本失败：{} (HTTP {})", url, resp.status);
        eprintln!("可能原因：脚本不存在（404）；服务器错误（5xx）；需要认证（401/403）");
        return ExitCode::from(1);
    }
    let src = match String::from_utf8(resp.body) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("下载的脚本不是有效的 UTF-8 文本：{} ({})", url, e);
            return ExitCode::from(1);
        }
    };
    run_string(&src, url, script_args)
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
    println!("  sf --remote <url>        从 URL 下载并执行脚本");
    println!("  sf --cloud <脚本名>      从云端执行脚本（基础 URL 配置于 ~/.sf/cloud.cfg）");
    println!("      示例：cloud.cfg 内容为 https://script.example.com/ 时，");
    println!("            sf --cloud basic.sf 等同于 sf --remote https://script.example.com/basic.sf");
    println!("  sf -server [options]     启动 HTTP 应用服务器");
    println!("      --port=80             HTTP 服务端口（默认 80）");
    println!("      --sslPort=443         HTTPS 服务端口（指定 --certDir 且证书存在时启用，默认 443）");
    println!("      --certDir=.           证书目录（需含 server.crt + server.key）");
    println!("      --host=0.0.0.0        监听地址");
    println!("      --dir=./scripts       脚本根目录");
    println!("      --webDir=./web        静态文件目录");
    println!("      --adminToken=sflang   管理端点令牌");
    println!("      --verbose             打印请求日志");
    println!("  sf --build <script.sf>   编译脚本为独立可执行文件");
    println!("      [--output <路径>]    指定输出路径");
    println!("  sf -h | --help | help    显示此帮助");
    println!("  sf -v | --version        显示版本");
    println!("  sf --list-builtins [分类] 列出所有内置函数（可按分类筛选）");
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

/// list_builtins 列出所有内置函数（按分类），可选按分类筛选。
/// 用法：sf --list-builtins [分类名]
fn list_builtins(filter: Option<&str>) -> ExitCode {
    let mut sf = sflang::Sflang::new();
    let vm = sf.vm_mut();
    let cats = vm.builtin_categories();
    let total = vm.builtin_names().len();

    if let Some(cat_filter) = filter {
        // 筛选指定分类（大小写不敏感）
        let lower = cat_filter.to_lowercase();
        let found: Vec<_> = cats
            .iter()
            .filter(|(c, _)| c.to_lowercase() == lower)
            .collect();
        if found.is_empty() {
            eprintln!("未找到分类 '{}'。可用分类：", cat_filter);
            let all_cats: Vec<&str> = cats.iter().map(|(c, _)| *c).collect();
            eprintln!("  {}", all_cats.join(", "));
            eprintln!("提示：也可在脚本内用 help(\"{}\") 查询。", cat_filter);
            return ExitCode::from(1);
        }
        for (cat, names) in &found {
            println!("== {}（{} 个函数）==", cat, names.len());
            // 每行最多 4 个，带简介
            for name in names.iter() {
                if let Some(doc) = vm.builtin_doc(name) {
                    println!("  {} — {}", name, doc.summary);
                } else {
                    println!("  {}", name);
                }
            }
        }
    } else {
        // 列出全部分类
        println!("Sflang 内置函数（共 {} 个，按分类列出）：", total);
        println!("用 sf --list-builtins <分类> 筛选，或在脚本内 help(\"函数名\") 查看详情。");
        println!();
        for (cat, names) in &cats {
            println!("== {}（{}）==", cat, names.len());
            for chunk in names.chunks(6) {
                println!("  {}", chunk.join(", "));
            }
            println!();
        }
    }
    ExitCode::SUCCESS
}

/// print_repl_help 打印 REPL 帮助。
fn print_repl_help() {
    println!("REPL 命令：");
    println!("  .exit / .quit / exit / quit / q   退出 REPL（Ctrl-D 同效）");
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

// ---- 单元测试（纯函数部分） ----

#[cfg(test)]
mod tests {
    use super::*;

    /// parse_cfg_content：普通 URL、注释、BOM、空白
    #[test]
    fn test_parse_cfg_content() {
        assert_eq!(parse_cfg_content("https://script.example.com/\n"), Some("https://script.example.com/".to_string()));
        // 带末尾 CR（Windows 换行）
        assert_eq!(parse_cfg_content("https://a.com/\r\n"), Some("https://a.com/".to_string()));
        // # 整行注释跳过，不影响 URL 中的 // 与 #
        assert_eq!(parse_cfg_content("# 注释\nhttps://a.com/path#frag\n"), Some("https://a.com/path#frag".to_string()));
        assert_eq!(parse_cfg_content("# 仅注释\n\n"), None);
        assert_eq!(parse_cfg_content(""), None);
        // UTF-8 BOM
        assert_eq!(parse_cfg_content("\u{feff}https://a.com/\n"), Some("https://a.com/".to_string()));
    }

    /// join_cloud_url：斜杠拼接不重复
    #[test]
    fn test_join_cloud_url() {
        assert_eq!(join_cloud_url("https://a.com/", "basic.sf"), "https://a.com/basic.sf");
        assert_eq!(join_cloud_url("https://a.com", "basic.sf"), "https://a.com/basic.sf");
        assert_eq!(join_cloud_url("https://a.com//", "/basic.sf"), "https://a.com/basic.sf");
        assert_eq!(join_cloud_url("https://a.com/scripts/", "demo/basic.sf"), "https://a.com/scripts/demo/basic.sf");
    }
}
