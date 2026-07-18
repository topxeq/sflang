//! builtins_gui.rs — GUI 内置函数（基于 wry/tao WebView）
//!
//! 对标 Charlang 的 WebView2 GUI 支持。
//! 采用"协作式移交"模式：guiShow 阻塞 VM 线程进入事件循环，
//! JS 通过 window.ipc.postMessage() 发消息，IPC handler 在同一线程
//! 安全重入执行 Sflang 的 handler 函数。
//!
//! 函数：
//!   guiNewWindow(switches...)  — 创建窗口配置对象
//!   guiSetHtml(win, html)      — 设置 HTML 内容
//!   guiSetUrl(win, url)        — 加载 URL
//!   guiSetHandler(win, func)   — 设置 IPC handler
//!   guiShow(win)               — 显示窗口，进入事件循环（阻塞）
//!   guiEval(win, jsCode)       — 在 WebView 中排队执行 JS
//!   guiSetTitle(win, title)    — 设置窗口标题
//!   guiClose(win)              — 请求关闭窗口（在 handler 中调用）

use std::sync::{Arc, Mutex};

use crate::builtins_helpers as bh;
use crate::value::Value;
use crate::vm::VM;
use crate::function::BuiltinDoc;

// ===================== GUI 函数文档 =====================

static DOC_GUI_NEW_WINDOW: BuiltinDoc = BuiltinDoc {
    category: "gui",
    signature: "guiNewWindow(switches...) -> window",
    summary: "创建 GUI 窗口配置对象（不显示）。",
    params: &[
        ("...switches", "--key=value 形式的开关，支持 --title/--width/--height"),
    ],
    returns: "window 窗口对象，传给 guiSetHtml/guiSetUrl/guiSetHandler/guiShow 等",
    examples: &[
        "var w = guiNewWindow(\"--title=My App\", \"--width=1024\", \"--height=768\")",
    ],
    errors: &[
        "switches 为可变参数，未传时使用默认值 title=Sflang width=800 height=600",
    ],
};

static DOC_GUI_SET_HTML: BuiltinDoc = BuiltinDoc {
    category: "gui",
    signature: "guiSetHtml(win, html) -> window",
    summary: "设置窗口要加载的 HTML 内容。",
    params: &[
        ("win", "guiNewWindow 返回的窗口对象"),
        ("html", "HTML 字符串"),
    ],
    returns: "window 返回传入的窗口对象（便于链式调用）",
    examples: &["guiSetHtml(w, \"<h1>Hello</h1>\")"],
    errors: &[
        "win 必须是 guiNewWindow 创建的 window 对象",
        "html 与 url 互斥，guiShow 优先使用 html",
    ],
};

static DOC_GUI_SET_URL: BuiltinDoc = BuiltinDoc {
    category: "gui",
    signature: "guiSetUrl(win, url) -> window",
    summary: "设置窗口要加载的 URL。",
    params: &[
        ("win", "guiNewWindow 返回的窗口对象"),
        ("url", "URL 字符串（http/https/file 等）"),
    ],
    returns: "window 返回传入的窗口对象（便于链式调用）",
    examples: &["guiSetUrl(w, \"https://example.com\")"],
    errors: &[
        "win 必须是 guiNewWindow 创建的 window 对象",
        "html 与 url 互斥，guiShow 优先使用 html",
    ],
};

static DOC_GUI_SET_HANDLER: BuiltinDoc = BuiltinDoc {
    category: "gui",
    signature: "guiSetHandler(win, func) -> window",
    summary: "设置 IPC 消息处理函数。",
    params: &[
        ("win", "guiNewWindow 返回的窗口对象"),
        ("func", "处理函数，接收 JS 通过 window.ipc.postMessage(msg) 发来的字符串"),
    ],
    returns: "window 返回传入的窗口对象（便于链式调用）",
    examples: &[
        "guiSetHandler(w, func(msg) { println(msg) })",
    ],
    errors: &[
        "func 必须是可调用对象（函数/闭包），接收单个字符串参数",
        "handler 在 guiShow 事件循环线程内同步执行，可安全重入 VM",
    ],
};

static DOC_GUI_SHOW: BuiltinDoc = BuiltinDoc {
    category: "gui",
    signature: "guiShow(win) -> !",
    summary: "显示窗口并进入事件循环（阻塞当前线程）。",
    params: &[("win", "guiNewWindow 返回的窗口对象")],
    returns: "永不返回（事件循环阻塞，直到窗口关闭）",
    examples: &[
        "guiShow(w)  // 阻塞直到用户关闭窗口",
    ],
    errors: &[
        "win 必须是 guiNewWindow 创建的 window 对象",
        "创建窗口/WebView 失败会抛错（如平台不支持）",
        "未设置 html/url/handler 时显示默认占位 HTML",
    ],
};

static DOC_GUI_EVAL: BuiltinDoc = BuiltinDoc {
    category: "gui",
    signature: "guiEval(win, jsCode) -> undefined",
    summary: "在 WebView 中排队执行 JavaScript 代码。",
    params: &[
        ("win", "guiNewWindow 返回的窗口对象"),
        ("jsCode", "要执行的 JavaScript 代码字符串"),
    ],
    returns: "undefined（JS 执行结果不回传到 Sflang）",
    examples: &[
        "guiEval(w, \"document.body.style.background='red'\")",
    ],
    errors: &[
        "必须在 guiShow 之前或 handler 内调用，事件循环每 20ms 刷新队列",
        "JS 代码错误不会抛回 Sflang，仅在 WebView 控制台报错",
    ],
};

static DOC_GUI_SET_TITLE: BuiltinDoc = BuiltinDoc {
    category: "gui",
    signature: "guiSetTitle(win, title) -> window",
    summary: "设置窗口标题。",
    params: &[
        ("win", "guiNewWindow 返回的窗口对象"),
        ("title", "标题字符串"),
    ],
    returns: "window 返回传入的窗口对象（便于链式调用）",
    examples: &["guiSetTitle(w, \"My App\")"],
    errors: &["win 必须是 guiNewWindow 创建的 window 对象"],
};

static DOC_GUI_CLOSE: BuiltinDoc = BuiltinDoc {
    category: "gui",
    signature: "guiClose(win) -> undefined",
    summary: "请求关闭窗口（用于在 handler 中程序化关闭）。",
    params: &[("win", "guiNewWindow 返回的窗口对象")],
    returns: "undefined（事件循环检测到标志后退出）",
    examples: &[
        "guiSetHandler(w, func(msg) { if (msg == \"quit\") { guiClose(w) } })",
    ],
    errors: &[
        "win 必须是 guiNewWindow 创建的 window 对象",
        "仅在 guiShow 事件循环运行期间有效",
    ],
};

/// GuiWindow 窗口配置（可变，通过 Mutex 保护）。
pub struct GuiWindow {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub html: Option<String>,
    pub url: Option<String>,
    pub handler: Option<Value>,
    pub eval_queue: Vec<String>,
    pub close_requested: bool,
}

fn gui_value(win: GuiWindow) -> Value {
    Value::Native(Arc::new(Arc::new(Mutex::new(win))))
}

fn gui_downcast<'a>(v: &'a Value, fn_name: &str) -> Result<&'a Arc<Mutex<GuiWindow>>, Value> {
    match v {
        Value::Native(n) => n.downcast_ref::<Arc<Mutex<GuiWindow>>>().ok_or_else(|| {
            crate::value::error_value(format!(
                "{}() 参数不是 window 对象 (可能原因：未用 guiNewWindow 创建)", fn_name,
            ))
        }),
        other => Err(crate::value::error_value(format!(
            "{}() 参数应为 window 对象，得到 {}", fn_name, other.type_name(),
        ))),
    }
}

pub fn register(vm: &mut VM) {
    vm.register_builtin_doc("guiNewWindow", bi_gui_new_window, &DOC_GUI_NEW_WINDOW);
    vm.register_builtin_doc("guiSetHtml", bi_gui_set_html, &DOC_GUI_SET_HTML);
    vm.register_builtin_doc("guiSetUrl", bi_gui_set_url, &DOC_GUI_SET_URL);
    vm.register_builtin_doc("guiSetHandler", bi_gui_set_handler, &DOC_GUI_SET_HANDLER);
    vm.register_builtin_doc("guiShow", bi_gui_show, &DOC_GUI_SHOW);
    vm.register_builtin_doc("guiEval", bi_gui_eval, &DOC_GUI_EVAL);
    vm.register_builtin_doc("guiSetTitle", bi_gui_set_title, &DOC_GUI_SET_TITLE);
    vm.register_builtin_doc("guiClose", bi_gui_close, &DOC_GUI_CLOSE);
}

/// 全局 VM 指针（用于 IPC handler 中 VM 重入）。
/// 用 usize 避免 Send 约束。安全性：guiShow 是同步阻塞的。
static GUI_VM: Mutex<usize> = Mutex::new(0);

/// get_switch 从参数列表中解析 --key=value 格式的开关。
fn get_switch(args: &[Value], key: &str, default: &str) -> String {
    let prefix = format!("--{}=", key);
    let prefix_short = format!("-{}=", key);
    for arg in args {
        if let Value::Str(s) = arg {
            if let Some(rest) = s.strip_prefix(&prefix).or_else(|| s.strip_prefix(&prefix_short)) {
                return rest.to_string();
            }
        }
    }
    default.to_string()
}

// ---- 内置函数 ----

fn bi_gui_new_window(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let win = GuiWindow {
        title: get_switch(args, "title", "Sflang"),
        width: get_switch(args, "width", "800").parse().unwrap_or(800),
        height: get_switch(args, "height", "600").parse().unwrap_or(600),
        html: None,
        url: None,
        handler: None,
        eval_queue: Vec::new(),
        close_requested: false,
    };
    Ok(gui_value(win))
}

fn bi_gui_set_html(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "guiSetHtml")?;
    bh::require_arg(args, 1, "guiSetHtml")?;
    let win = gui_downcast(&args[0], "guiSetHtml")?;
    let html = bh::as_str(args, 1, "guiSetHtml")?;
    win.lock().unwrap().html = Some(html.to_string());
    Ok(args[0].clone())
}

fn bi_gui_set_url(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "guiSetUrl")?;
    bh::require_arg(args, 1, "guiSetUrl")?;
    let win = gui_downcast(&args[0], "guiSetUrl")?;
    let url = bh::as_str(args, 1, "guiSetUrl")?;
    win.lock().unwrap().url = Some(url.to_string());
    Ok(args[0].clone())
}

fn bi_gui_set_handler(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "guiSetHandler")?;
    bh::require_arg(args, 1, "guiSetHandler")?;
    let win = gui_downcast(&args[0], "guiSetHandler")?;
    win.lock().unwrap().handler = Some(args[1].clone());
    Ok(args[0].clone())
}

fn bi_gui_set_title(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "guiSetTitle")?;
    bh::require_arg(args, 1, "guiSetTitle")?;
    let win = gui_downcast(&args[0], "guiSetTitle")?;
    let title = bh::as_str(args, 1, "guiSetTitle")?;
    win.lock().unwrap().title = title.to_string();
    Ok(args[0].clone())
}

fn bi_gui_eval(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "guiEval")?;
    bh::require_arg(args, 1, "guiEval")?;
    let win = gui_downcast(&args[0], "guiEval")?;
    let js = bh::as_str(args, 1, "guiEval")?;
    win.lock().unwrap().eval_queue.push(js.to_string());
    Ok(Value::Undefined)
}

/// bi_gui_close 请求关闭窗口（在 IPC handler 中调用）。
///
/// 设置 close_requested 标志，事件循环检测到后退出。
/// 用法：guiClose(win)
fn bi_gui_close(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "guiClose")?;
    let win = gui_downcast(&args[0], "guiClose")?;
    win.lock().unwrap().close_requested = true;
    Ok(Value::Undefined)
}

/// bi_gui_show 显示窗口，进入事件循环（阻塞）。
fn bi_gui_show(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use tao::{
        event::{Event, WindowEvent},
        event_loop::ControlFlow,
        window::WindowBuilder,
    };
    use wry::WebViewBuilder;

    bh::require_arg(args, 0, "guiShow")?;
    let win = gui_downcast(&args[0], "guiShow")?;
    let win_guard = win.lock().unwrap();

    let title = win_guard.title.clone();
    let width = win_guard.width;
    let height = win_guard.height;
    let html = win_guard.html.clone();
    let url = win_guard.url.clone();
    let handler = win_guard.handler.clone();
    let win_arc = win.clone();

    drop(win_guard);

    // 设置全局 VM 指针
    let vm_ptr = vm as *mut VM as usize;
    {
        let mut ptr = GUI_VM.lock().unwrap();
        *ptr = vm_ptr;
    }

    // 创建事件循环和窗口。
    // 主程序在 32MB 栈子线程中运行（防递归溢出），tao 默认要求主线程创建 EventLoop。
    // 用 any_thread 模式允许在子线程创建（Windows 平台）。
    let event_loop = {
        let mut builder = tao::event_loop::EventLoopBuilder::new();
        #[cfg(target_os = "windows")]
        {
            use tao::platform::windows::EventLoopBuilderExtWindows;
            builder.with_any_thread(true);
        }
        builder.build()
    };

    let window = WindowBuilder::new()
        .with_title(&title)
        .with_inner_size(tao::dpi::LogicalSize::new(width, height))
        .build(&event_loop)
        .map_err(|e| crate::value::error_value(format!("guiShow() 创建窗口失败: {}", e)))?;

    // 构建 WebView
    let mut builder = WebViewBuilder::new();

    // IPC handler：收到 JS postMessage 时调用 Sflang handler
    //
    // 安全性说明（unsafe 的合理性）：
    //   - GUI 事件循环（EventLoop::run）阻塞当前线程，IPC 回调由 wry 在同一线程派发。
    //   - 因此 VM 重入始终在 GUI 线程（即调用 guiShow 的线程），与 VM 同线程，无数据竞争。
    //   - GUI_VM 全局裸指针在 guiShow 进入事件循环前设置、退出后清除，生命周期覆盖事件循环。
    //   - 这是 wry/tao 的标准重入模式（事件循环必须在主线程，回调无并发）。
    let handler_clone = handler.clone();
    let win_for_ipc = win_arc.clone();
    builder = builder.with_ipc_handler(move |request: wry::http::Request<String>| {
        let msg = request.body().to_string();

        // VM 重入：调用 handler 函数（同线程，安全）
        if let Some(ref handler_fn) = handler_clone {
            let stored_ptr = {
                let p = GUI_VM.lock().unwrap();
                *p
            };
            if stored_ptr != 0 {
                // SAFETY: IPC 回调在 GUI 事件循环线程派发，与 VM 同线程，无并发访问。
                // GUI_VM 指针在 guiShow 事件循环运行期间有效。
                let vm = unsafe { &mut *(stored_ptr as *mut VM) };
                match vm.call_function_value(handler_fn.clone(), vec![Value::str_from(msg)]) {
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("GUI handler 错误: {}", e.inspect());
                    }
                }
            }
        }
        // 标记 eval_queue 已处理（让事件循环知道有 eval 要执行）
        let _ = &win_for_ipc;
    });

    if let Some(ref html) = html {
        builder = builder.with_html(html);
    } else if let Some(ref url) = url {
        builder = builder.with_url(url);
    } else {
        builder = builder.with_html("<html><body><h1>Sflang GUI</h1></body></html>");
    }

    let webview = builder.build(&window)
        .map_err(|e| crate::value::error_value(format!("guiShow() 创建 WebView 失败: {}", e)))?;

    // 进入事件循环（阻塞）
    // 用短间隔 WaitUntil 确保及时检查 close_requested 和 eval_queue
    event_loop.run(move |event, _, control_flow| {
        use std::time::{Duration, Instant};
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(20));

        // 检查是否有排队的 eval 命令
        {
            let mut g = win_arc.lock().unwrap();
            if !g.eval_queue.is_empty() {
                let cmds = std::mem::take(&mut g.eval_queue);
                for js in cmds {
                    let _ = webview.evaluate_script(&js);
                }
            }
            if g.close_requested {
                *control_flow = ControlFlow::Exit;
            }
        }

        // 检查后台异步任务结果，通过 guiEval 回调通知前端
        let async_results = crate::builtins_async::drain_async_results();
        if !async_results.is_empty() {
            for r in async_results {
                // 构造 JS 回调：window.onAsyncResult(id, result, isError)
                let val_str = if r.is_error {
                    format!("\"{}\"", r.result.to_str().replace('\\', "\\\\").replace('"', "\\\""))
                } else {
                    match &r.result {
                        Value::Str(s) => format!("\"{}\"", s.to_string().replace('\\', "\\\\").replace('"', "\\\"")),
                        Value::Int(n) => n.to_string(),
                        Value::Float(f) => f.to_string(),
                        Value::Bool(b) => b.to_string(),
                        Value::Undefined => "undefined".to_string(),
                        _ => format!("\"{}\"", r.result.to_str().replace('\\', "\\\\").replace('"', "\\\"")),
                    }
                };
                let js = format!(
                    "if(window.onAsyncResult)window.onAsyncResult({},{},{});",
                    r.id, val_str, r.is_error
                );
                let _ = webview.evaluate_script(&js);
            }
        }

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }
            Event::WindowEvent {
                event: WindowEvent::Destroyed,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }
    })
    // event_loop.run() 的签名为 -> !，不会返回
    // GUI_VM 指针的清理由进程退出时自动完成
}
