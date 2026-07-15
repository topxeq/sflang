//! function.rs — 用户自定义函数与内置函数类型
//!
//! 设计要点：
//!   - Function 函数体已编译为字节码（Code），运行时由 VM 解释执行
//!   - 闭包通过 free_vars 持有捕获的变量（box 共享）
//!   - Builtin 用函数指针实现，签名统一
//!
//! 并发设计（阶段三）：Function 用 Arc 共享（body 为 Arc<Code>），
//! free_vars 用 Arc<Mutex<Value>> 实现跨线程可变共享。Function 因此 Send + Sync。

use std::sync::{Arc, Mutex};

use crate::opcode::Code;
use crate::value::Value;

/// Function 用户自定义函数。
///
/// 函数体已编译为字节码（Code），运行时由 VM 解释执行。
/// 闭包通过 free_vars 持有捕获的变量（box 共享，可跨层修改）。
pub struct Function {
    /// name 函数名（匿名函数为空字符串）。
    pub name: String,
    /// params 形参名列表。
    pub params: Vec<String>,
    /// body 函数体的已编译字节码。
    pub body: Arc<Code>,
    /// free_vars 闭包捕获的自由变量（box 共享，跨线程可变）。
    /// 非闭包函数为空。
    pub free_vars: Vec<Arc<Mutex<Value>>>,
    /// variadic 是否可变参数（最后一个形参接收剩余参数为数组）。
    pub variadic: bool,
}

impl Function {
    /// new 创建函数（非闭包）。
    pub fn new(name: String, params: Vec<String>, body: Arc<Code>) -> Self {
        Function {
            name,
            params,
            body,
            free_vars: Vec::new(),
            variadic: false,
        }
    }

    /// new_closure 创建闭包。
    pub fn new_closure(
        name: String,
        params: Vec<String>,
        body: Arc<Code>,
        free_vars: Vec<Arc<Mutex<Value>>>,
        variadic: bool,
    ) -> Self {
        Function {
            name,
            params,
            body,
            free_vars,
            variadic,
        }
    }
}

/// BuiltinFn 内置函数签名。
///
/// vm 为虚拟机实例（可用于访问输出、全局环境等），args 为实参列表。
/// 返回结果与错误（错误非 None 时在 VM 中转为抛出）。
pub type BuiltinFn = fn(vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value>;

/// BuiltinDoc 内置函数的文档元数据（静态，用于 help 系统）。
///
/// 每个字段都是 &'static，零运行时分配。通过 register_builtin_doc 注册到 VM，
/// 脚本侧用 help("funcName") 查询。未补全文档的函数 doc 为 None。
///
/// 设计目标：让 AI 和人类能通过 help() 自省内置函数的签名、参数、返回值、
/// 示例与常见错误，无需查阅外部文档。
pub struct BuiltinDoc {
    /// category 分类标识（如 "regex"、"string"、"array"）。用于 help() 分类列表。
    pub category: &'static str,
    /// signature 函数签名（如 "regFind(pattern, s) -> string|undefined"）。
    pub signature: &'static str,
    /// summary 一句话功能简介。
    pub summary: &'static str,
    /// params 参数说明列表：(参数名, 说明)。可变参数用 "..." 标注。
    pub params: &'static [(&'static str, &'static str)],
    /// returns 返回值说明。
    pub returns: &'static str,
    /// examples 示例代码片段（每条一行，含预期结果注释）。
    pub examples: &'static [&'static str],
    /// errors 常见错误提示（AI 友好，帮助定位参数顺序、类型等问题）。
    pub errors: &'static [&'static str],
}

/// Builtin 内置函数。
#[derive(Clone)]
pub struct Builtin {
    /// name 函数名。
    pub name: &'static str,
    /// fn Go 实现的函数。
    pub func: BuiltinFn,
    /// doc 文档元数据（可选；未补全的函数为 None）。
    pub doc: Option<&'static BuiltinDoc>,
}

impl Builtin {
    /// new 创建内置函数（无文档）。
    pub fn new(name: &'static str, func: BuiltinFn) -> Self {
        Builtin { name, func, doc: None }
    }

    /// new_with_doc 创建带文档的内置函数。
    pub fn new_with_doc(name: &'static str, func: BuiltinFn, doc: &'static BuiltinDoc) -> Self {
        Builtin { name, func, doc: Some(doc) }
    }
}
