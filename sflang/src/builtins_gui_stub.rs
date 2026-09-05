//! builtins_gui_stub.rs — GUI 内置函数的非 Windows 平台桩实现。
//!
//! Sflang 的 GUI 能力（wry/tao WebView）当前仅在 Windows 平台提供（WebView2）。
//! 本模块在非 Windows 平台编译时替代 builtins_gui：注册全部同名内置函数，
//! 调用时返回明确的错误对象——相比"未知函数"，这让脚本和 AI 能直接明白
//! 是平台限制而非函数名写错。
//!
//! 若未来扩展其他平台 GUI 支持，删除本模块并放开 lib.rs / vm.rs 中的
//! target_os 条件编译即可。

use crate::function::BuiltinDoc;
use crate::value::Value;
use crate::vm::VM;

/// 平台不支持错误的标准消息模板。
const MSG: &str = "{}() GUI 功能当前仅支持 Windows 平台 (当前平台不可用；如需 GUI 编程请在 Windows 上运行，或在非 Windows 平台改用命令行/服务器方式)";

/// 生成桩函数：接受任意参数，返回平台不支持错误对象。
macro_rules! gui_stub {
    ($fn_name:ident) => {
        fn $fn_name(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
            Ok(crate::value::error_value(
                MSG.replace("{}", stringify!($fn_name)),
            ))
        }
    };
}

gui_stub!(bi_gui_new_window);
gui_stub!(bi_gui_set_html);
gui_stub!(bi_gui_set_url);
gui_stub!(bi_gui_set_handler);
gui_stub!(bi_gui_show);
gui_stub!(bi_gui_eval);
gui_stub!(bi_gui_set_title);
gui_stub!(bi_gui_close);

/// 为桩函数生成统一的文档（签名与 Windows 版一致，summary 标注平台限制）。
macro_rules! gui_stub_doc {
    ($doc:ident, $sig:expr, $params:expr) => {
        static $doc: BuiltinDoc = BuiltinDoc {
            category: "gui",
            signature: $sig,
            summary: "GUI 功能当前仅支持 Windows 平台；在非 Windows 平台调用返回 error 对象。",
            params: $params,
            returns: "error 对象（平台不支持）",
            examples: &[],
            errors: &["非 Windows 平台调用返回平台不支持错误"],
        };
    };
}

gui_stub_doc!(
    DOC_GUI_NEW_WINDOW,
    "guiNewWindow([title] [, html|\"--url=...\"]) -> window|error",
    &[
        ("title", "窗口标题"),
        ("html", "可选。初始 HTML 内容"),
        ("--url", "可选。初始 URL（与 html 互斥）"),
    ]
);
gui_stub_doc!(
    DOC_GUI_SET_HTML,
    "guiSetHtml(win, html) -> window|error",
    &[("win", "窗口对象"), ("html", "HTML 内容")]
);
gui_stub_doc!(
    DOC_GUI_SET_URL,
    "guiSetUrl(win, url) -> window|error",
    &[("win", "窗口对象"), ("url", "目标 URL")]
);
gui_stub_doc!(
    DOC_GUI_SET_HANDLER,
    "guiSetHandler(win, handler) -> window|error",
    &[("win", "窗口对象"), ("handler", "IPC 消息处理函数")]
);
gui_stub_doc!(
    DOC_GUI_SHOW,
    "guiShow(win) -> undefined|error",
    &[("win", "窗口对象；阻塞进入事件循环直至窗口关闭")]
);
gui_stub_doc!(
    DOC_GUI_EVAL,
    "guiEval(win, jsCode) -> undefined|error",
    &[("win", "窗口对象"), ("jsCode", "在 WebView 中执行的 JS 代码")]
);
gui_stub_doc!(
    DOC_GUI_SET_TITLE,
    "guiSetTitle(win, title) -> window|error",
    &[("win", "窗口对象"), ("title", "新窗口标题")]
);
gui_stub_doc!(
    DOC_GUI_CLOSE,
    "guiClose(win) -> undefined|error",
    &[("win", "窗口对象；关闭窗口并退出事件循环")]
);

/// register 注册全部 GUI 桩函数到 VM（函数名与 Windows 版 builtins_gui 一致）。
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
