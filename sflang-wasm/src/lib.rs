// sflang-wasm — Sflang 的浏览器 WASM 封装
// ==================================
// 以 cdylib 产出 sflang.wasm，供官网 Sflang 专区 Playground 在浏览器内直接执行
// Sflang 脚本。刻意不依赖 wasm-bindgen：ABI 为手写 extern "C" + 共享内存缓冲区，
// JS glue 见 playground 页（零新增第三方依赖）。
//
// 内存协议（单实例、单线程，适合 Playground 场景）：
//   sf_in_ptr()      → *mut u8   源代码写入缓冲（64KB，JS 用 TextEncoder 写入）
//   sf_entropy_ptr() → *mut u8   熵缓冲（64KB，JS 用 crypto.getRandomValues 预填，
//                                 每次 run 前补满；Rust 侧用后清零防密钥流复用）
//   sf_run(code_len) → i64       >=0 成功，返回值 = 输出长度；<0 失败，绝对值 = 输出长度
//                                （输出含错误信息与调用栈，UTF-8）
//   sf_out_ptr()     → *const u8 输出读取指针
//
// 限制（一期）：脚本触发的 Rust panic 会使 wasm 实例中止（wasm 仅支持 panic=abort），
// JS glue 在异常后重建实例兜底。

use std::io::Write;

/// IN_BUF 源代码输入缓冲（64KB，JS 侧写入）。
static mut IN_BUF: [u8; 65536] = [0u8; 65536];

/// OUT_BUF 输出缓冲（脚本 stdout/stderr 捕获 + 末尾表达式回显）。
static mut OUT_BUF: Vec<u8> = Vec::new();

/// PANIC_BUF 最近一次 Rust panic 的信息（wasm 无 stderr，经此暴露给 JS 显示）。
static mut PANIC_BUF: Vec<u8> = Vec::new();

/// PANIC_HOOK_SET 确保 panic hook 只安装一次。
static PANIC_HOOK_SET: std::sync::Once = std::sync::Once::new();

/// js_log JS 侧导入：panic hook 在实例中止前经此把诊断信息同步交给 JS
/// （wasm 为 panic=abort，abort 后内存即分离，无法事后读取）。
#[link(wasm_import_module = "env")]
extern "C" {
    fn js_log(ptr: *const u8, len: usize);
}

/// ENTROPY 熵缓冲（JS 预填；Rust 侧顺序取用，用后清零）。
static mut ENTROPY: [u8; 65536] = [0u8; 65536];

/// entropy_pos 熵缓冲已取用量。
static mut ENTROPY_POS: usize = 0;

/// wasm_getrandom getrandom 自定义后端：wasm 无 OS 熵源，从 JS 预填的熵缓冲顺序取。
/// 每次运行前 JS glue 用 crypto.getRandomValues 重新补满缓冲。
fn wasm_getrandom(buf: &mut [u8]) -> Result<(), getrandom::Error> {
    unsafe {
        if ENTROPY_POS + buf.len() > ENTROPY.len() {
            // 熵耗尽：提示 JS 补充（正常脚本每次 run 前补满，不应触达）
            eprintln!("[sflang-wasm] 熵缓冲耗尽：请由 JS glue 在每次运行前补满熵缓冲");
            return Err(getrandom::Error::UNSUPPORTED);
        }
        let start = ENTROPY_POS;
        buf.copy_from_slice(&ENTROPY[start..start + buf.len()]);
        ENTROPY_POS += buf.len();
        // 已用字节清零：防止不同调用复用同一段"密钥流"
        for b in &mut ENTROPY[start..start + buf.len()] {
            *b = 0;
        }
        Ok(())
    }
}

getrandom::register_custom_getrandom!(wasm_getrandom);

/// OutSink 把脚本输出（pln/print 等）追加到 OUT_BUF。
struct OutSink;

impl Write for OutSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        unsafe { OUT_BUF.extend_from_slice(buf) };
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// sf_in_ptr 返回源代码输入缓冲指针（JS 侧写入代码用）。
#[no_mangle]
pub extern "C" fn sf_in_ptr() -> *mut u8 {
    unsafe { IN_BUF.as_mut_ptr() }
}

/// sf_entropy_ptr 返回熵缓冲指针（JS 侧用 crypto.getRandomValues 预填）。
#[no_mangle]
pub extern "C" fn sf_entropy_ptr() -> *mut u8 {
    unsafe { ENTROPY.as_mut_ptr() }
}

/// sf_out_ptr 返回输出缓冲指针（JS 侧读取结果用）。
#[no_mangle]
pub extern "C" fn sf_out_ptr() -> *const u8 {
    unsafe { OUT_BUF.as_ptr() }
}

/// sf_panic_ptr / sf_panic_len 返回最近一次 Rust panic 的信息（wasm 无 stderr，
/// panic 信息经此暴露，供 JS 在捕获 RuntimeError 后展示，定位 wasm 内部错误）。
#[no_mangle]
pub extern "C" fn sf_panic_ptr() -> *const u8 {
    unsafe { PANIC_BUF.as_ptr() }
}

#[no_mangle]
pub extern "C" fn sf_panic_len() -> usize {
    unsafe { PANIC_BUF.len() }
}

/// install_panic_hook 安装 panic hook：把 panic 信息写入 PANIC_BUF（仅首次调用生效），
/// 并立即经 js_log 导入交给 JS——wasm 为 panic=abort，hook 返回后实例即中止、
/// 内存分离，信息只能在 hook 执行期间同步传出。
fn install_panic_hook() {
    PANIC_HOOK_SET.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            let msg = format!("[wasm panic] {}", info);
            unsafe {
                PANIC_BUF = msg.clone().into_bytes();
                js_log(PANIC_BUF.as_ptr(), PANIC_BUF.len());
            }
        }));
    });
}

/// sf_run 编译并执行 IN_BUF 中的源代码（长度 code_len 字节，UTF-8）。
/// 返回：>=0 成功（输出长度）；<0 失败（绝对值为输出长度，内容为错误详情）。
/// 顶层脚本结束后泵空全部 run 任务（wasm 单线程模型，scheduler::pump_all）。
#[no_mangle]
pub extern "C" fn sf_run(code_len: usize) -> i64 {
    install_panic_hook();
    // 保持诊断导出不被链接器回收（未被引用的 no_mangle 函数会被 gc-sections 移除）
    std::hint::black_box((sf_panic_ptr as usize, sf_panic_len as usize));
    unsafe {
        OUT_BUF.clear();
        ENTROPY_POS = 0;
    }
    let src = unsafe {
        std::str::from_utf8(&IN_BUF[..code_len.min(IN_BUF.len())])
    };
    let Ok(src) = src else {
        return write_err("源代码不是有效的 UTF-8 文本");
    };

    let mut sf = sflang::Sflang::new();
    sf.set_output(OutSink);
    let result = sf.run_string(src);

    // 排空 fire-and-forget 任务（run 启动的后台体在 wasm 单线程泵里执行）
    sflang::scheduler::pump_all();

    match result {
        Ok(v) => {
            // 末尾表达式回显（与 REPL 语义一致；undefined 不显示）
            let show = !matches!(v, sflang::Value::Undefined);
            if show {
                let _ = writeln!(OutSink, "\n=> {}", v.inspect());
            }
            unsafe { OUT_BUF.len() as i64 }
        }
        Err(e) => match e {
            sflang::Value::Error(err) => {
                let mut msg = format!("错误：{}", err.message);
                if !err.stack.is_empty() {
                    msg.push_str("\n调用栈：\n  ");
                    msg.push_str(&err.stack.join("\n  "));
                }
                write_err(&msg)
            }
            other => write_err(&format!("错误：{}", other.inspect())),
        },
    }
}

/// write_err 把错误信息写入输出缓冲并返回负数结果码。
fn write_err(msg: &str) -> i64 {
    let n = unsafe {
        OUT_BUF.extend_from_slice(msg.as_bytes());
        OUT_BUF.len()
    };
    -(n as i64)
}
