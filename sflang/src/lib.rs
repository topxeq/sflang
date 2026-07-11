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
pub mod builtins;
pub mod builtins_helpers;
pub mod builtins_str;
pub mod builtins_math;
pub mod builtins_arr;
pub mod builtins_time;
pub mod builtins_fs;
pub mod builtins_json;
pub mod builtins_bytes;
pub mod builtins_bigint;
pub mod builtins_regex;
pub mod builtins_encode;
pub mod builtins_hash;
pub mod builtins_sys;
pub mod concurrency;
pub mod ring;
pub mod builtins_ring;
pub mod builtins_csv;
pub mod builtins_xlsx;
pub mod builtins_docx;
pub mod builtins_db;
pub mod aes;
pub mod builtins_aes;
pub mod txde;
pub mod builtins_gui;
pub mod builtins_ssh;
pub mod builtins_le;
pub mod builtins_email;
pub mod builtins_ftp;
pub mod http_lite;
pub mod builtins_http;
pub mod api;

// 重导出常用类型
pub use value::{Value, TypeCode, SfError, error_value, err_to_value};
pub use object_map::{Map, new_map, new_map_with_proto};
pub use function::{Function, Builtin, BuiltinFn};
pub use console_writer::ConsoleWriter;
pub use opcode::{Opcode, Code, FreeSource};
pub use lexer::tokenize;
pub use parser::{parse_program, ParseError};
pub use compiler::{compile, CompileError};
pub use vm::VM;
pub use api::Sflang;
