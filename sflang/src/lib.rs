//! Sflang 核心库入口
//!
//! 模块组织：
//!   - value: Value 类型定义（tagged enum）
//!   - object_map: Map（对象/映射）类型
//!   - function: Function/Builtin 函数类型
//!   - token: Token 类型定义
//!   - lexer: 词法分析
//!   - ast: 抽象语法树
//!   - parser: 语法分析
//!   - opcode: 字节码操作码与 Code 结构
//!   - compiler: AST → 字节码编译器
//!   - vm: 字节码虚拟机
//!   - builtins: 内置函数
//!   - concurrency: 并发原语（channel）
//!   - api: 嵌入式 API

pub mod value;
pub mod object_map;
pub mod function;
pub mod bigint;
pub mod bigfloat;
pub mod datetime;
pub mod hash;
pub mod ord_map;
pub mod console_writer;
pub mod token;
pub mod lexer;
pub mod ast;
pub mod parser;
pub mod opcode;
pub mod compiler;
pub mod vm;
pub mod clock;
pub mod scheduler;
pub mod builtins;
// http_session 依赖 Cookie 会话与网络栈，wasm（浏览器）目标裁掉。
#[cfg(not(target_arch = "wasm32"))]
pub mod http_session;
pub mod builtins_helpers;
pub mod builtins_str;
pub mod builtins_math;
pub mod builtins_arr;
pub mod builtins_time;
// 文件系统内置函数：wasm（浏览器）目标无文件系统，裁掉。
#[cfg(not(target_arch = "wasm32"))]
pub mod builtins_fs;
pub mod builtins_json;
pub mod builtins_bytes;
pub mod builtins_bigint;
pub mod builtins_regex;
pub mod builtins_encode;
pub mod builtins_hash;
// 系统/进程内置函数（env/exec/临时目录等）：wasm 目标裁掉。
#[cfg(not(target_arch = "wasm32"))]
pub mod builtins_sys;
pub mod concurrency;
pub mod ring;
pub mod builtins_ring;
pub mod builtins_csv;
pub mod builtins_xlsx;
// docx 依赖 zip，zip 默认特性含 C 依赖（bzip2/zstd）无法为 wasm 编译，整体裁掉。
#[cfg(not(target_arch = "wasm32"))]
pub mod builtins_docx;
// 数据库内置函数：驱动依赖 socket 或 C 源码（rusqlite bundled），wasm 目标裁掉。
#[cfg(not(target_arch = "wasm32"))]
pub mod builtins_db;
#[cfg(not(target_arch = "wasm32"))]
pub mod sql_engine;
pub mod aes;
pub mod builtins_aes;
pub mod txde;
// GUI 内置函数仅在 Windows 平台提供（基于 WebView2，wry/tao）。
// 其他平台编译 builtins_gui_stub：注册同名函数，调用时返回明确的平台不支持错误。
#[cfg(all(feature = "gui", target_os = "windows"))]
pub mod builtins_gui;
#[cfg(all(feature = "gui", not(target_os = "windows")))]
pub mod builtins_gui_stub;
// SSH 客户端内置函数（依赖 socket），wasm 目标裁掉。
#[cfg(not(target_arch = "wasm32"))]
pub mod builtins_ssh;
// le*（line-editor 辅助）内置函数使用 std::fs/env，wasm 目标裁掉。
#[cfg(not(target_arch = "wasm32"))]
pub mod builtins_le;
// 邮件发送内置函数（lettre，依赖 socket/TLS），wasm 目标裁掉。
#[cfg(not(target_arch = "wasm32"))]
pub mod builtins_email;
// FTP 内置函数（依赖 socket），wasm 目标裁掉。
#[cfg(not(target_arch = "wasm32"))]
pub mod builtins_ftp;
// http_lite 自研 HTTP 客户端（现为 ureq 封装，依赖 socket/TLS）。
// wasm（浏览器）目标裁掉；Value 的 HttpReq/HttpResp/WebSocket 变体在 value.rs 中以
// cfg 占位类型保持形状与 TypeCode 编码稳定（固定编码是兼容性承诺）。
#[cfg(not(target_arch = "wasm32"))]
pub mod http_lite;
#[cfg(not(target_arch = "wasm32"))]
pub mod builtins_http;
// zip* 内置函数（C 依赖），wasm 目标裁掉。
#[cfg(not(target_arch = "wasm32"))]
pub mod builtins_zip;
pub mod builtins_containers;
pub mod builtins_xml;
// 剪贴板/系统对话框：Windows API 之外为桩，wasm 目标直接裁掉。
#[cfg(not(target_arch = "wasm32"))]
pub mod builtins_clipboard;
#[cfg(not(target_arch = "wasm32"))]
pub mod builtins_dialog;
// runAsync 后台线程执行（为 GUI 事件循环设计），wasm 目标裁掉。
#[cfg(not(target_arch = "wasm32"))]
pub mod builtins_async;
pub mod builtins_test;
pub mod builtins_pinyin;
pub mod builtins_jwt;
pub mod builtins_rsa;
pub mod sm;
pub mod builtins_cfg;
pub mod builtins_template;
// TCP 内置函数（std::net），wasm 目标裁掉。
#[cfg(not(target_arch = "wasm32"))]
pub mod builtins_tcp;
// TLS 证书解析内置函数（rustls），wasm 目标裁掉。
#[cfg(not(target_arch = "wasm32"))]
pub mod builtins_tls;
// 代理内置函数（网络），wasm 目标裁掉。
#[cfg(not(target_arch = "wasm32"))]
pub mod builtins_proxy;
pub mod des;
// 加密连接（encconn）依赖 TLS/socket，wasm 目标裁掉。
#[cfg(not(target_arch = "wasm32"))]
pub mod encconn;
// xxci* 内置函数（外部服务集成，网络），wasm 目标裁掉。
#[cfg(not(target_arch = "wasm32"))]
pub mod builtins_xxci;
pub mod builtins_image;
pub mod builtins_image_gen;
pub mod builtins_seq;
// S3 内置函数（HTTP 签名请求，网络），wasm 目标裁掉。
#[cfg(not(target_arch = "wasm32"))]
pub mod builtins_s3;
pub mod pinyin_data;
pub mod api;

// 重导出常用类型
pub use value::{Value, TypeCode, SfError, error_value, err_to_value};
pub use object_map::{Map, new_map, new_map_with_proto};
pub use function::{Function, Builtin, BuiltinFn, BuiltinDoc};
pub use console_writer::ConsoleWriter;
pub use opcode::{Opcode, Code, FreeSource};
pub use lexer::tokenize;
pub use parser::{parse_program, ParseError};
pub use compiler::{compile, CompileError};
pub use vm::VM;
pub use api::Sflang;
