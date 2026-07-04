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

/// Builtin 内置函数。
#[derive(Clone)]
pub struct Builtin {
    /// name 函数名。
    pub name: &'static str,
    /// fn Go 实现的函数。
    pub func: BuiltinFn,
}

impl Builtin {
    /// new 创建内置函数。
    pub fn new(name: &'static str, func: BuiltinFn) -> Self {
        Builtin { name, func }
    }
}
