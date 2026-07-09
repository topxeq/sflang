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
    vm.register_builtin("guiNewWindow", bi_gui_new_window);
    vm.register_builtin("guiSetHtml", bi_gui_set_html);
    vm.register_builtin("guiSetUrl", bi_gui_set_url);
    vm.register_builtin("guiSetHandler", bi_gui_set_handler);
    vm.register_builtin("guiShow", bi_gui_show);
    vm.register_builtin("guiEval", bi_gui_eval);
    vm.register_builtin("guiSetTitle", bi_gui_set_title);
    vm.register_builtin("guiClose", bi_gui_close);
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
        event_loop::{ControlFlow, EventLoop},
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

    // 创建事件循环和窗口
    let event_loop = EventLoop::new();

    let window = WindowBuilder::new()
        .with_title(&title)
        .with_inner_size(tao::dpi::LogicalSize::new(width, height))
        .build(&event_loop)
        .map_err(|e| crate::value::error_value(format!("guiShow() 创建窗口失败: {}", e)))?;

    // 构建 WebView
    let mut builder = WebViewBuilder::new();

    // IPC handler：收到 JS postMessage 时调用 Sflang handler
    let handler_clone = handler.clone();
    let win_for_ipc = win_arc.clone();
    builder = builder.with_ipc_handler(move |request: wry::http::Request<String>| {
        let msg = request.body().to_string();

        // VM 重入：调用 handler 函数
        if let Some(ref handler_fn) = handler_clone {
            let stored_ptr = {
                let p = GUI_VM.lock().unwrap();
                *p
            };
            if stored_ptr != 0 {
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
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

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
    });

    // 清理全局 VM 指针
    {
        let mut ptr = GUI_VM.lock().unwrap();
        *ptr = 0;
    }

    Ok(Value::Undefined)
}
