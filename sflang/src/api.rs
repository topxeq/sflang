//! api.rs — 嵌入式 API
//!
//! 提供 Sflang 作为第三方库被其他 Rust 程序调用的简洁接口。
//!
//! panic 隔离：所有公开执行入口（run_string / run_file / vm_run_code / call_func）
//! 均用 catch_unwind 包裹 VM 执行，脚本触发的内部 panic 会转为
//! `Err(Error("内部错误（panic）: ..."))` 返回，而不是击穿嵌入宿主进程。
//! 不修改全局 panic hook（保留默认行为，unwind 前仍会在 stderr 打印一条信息）。
//!
//! 用法：
//! ```ignore
//! use sflang::Sflang;
//!
//! let mut sf = Sflang::new();
//! // 正常执行返回 Ok；即使 VM 内部发生 panic 也只会返回 Err，不会 panic
//! let result = sf.run_string("1 + 2").unwrap();
//! ```

use std::sync::Arc;

use crate::compiler::compile;
use crate::lexer::tokenize;
use crate::parser::parse_program;
use crate::value::Value;
use crate::vm::VM;

/// panic_to_error 将 catch_unwind 捕获的 panic payload 转为错误值。
///
/// panic! 的 payload 常见为 String 或 &str，分别 downcast 提取消息；
/// 其他类型（如自定义结构体）给出占位描述，保证仍能返回错误而非崩溃。
fn panic_to_error(payload: Box<dyn std::any::Any + Send>) -> Value {
    let msg = if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else {
        "未知类型的 panic（payload 无法转为字符串）".to_string()
    };
    crate::value::error_value(format!("内部错误（panic）: {}", msg))
}

/// guard_catch 包裹一次 VM 执行，捕获内部 panic 并转为 Err(错误值)。
///
/// - 用 AssertUnwindSafe 声明跨越 unwind 边界的安全性：VM 含可变状态，
///   panic 中途展开后实例状态可能不一致，宿主应视为本次执行失败（丢弃该结果）；
/// - 不设置全局 panic hook，保持进程默认行为，避免影响宿主自身的 panic 策略。
fn guard_catch<R>(f: impl FnOnce() -> Result<R, Value>) -> Result<R, Value> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(payload) => Err(panic_to_error(payload)),
    }
}

/// Sflang 嵌入式 API 入口。
pub struct Sflang {
    vm: VM,
}

impl Sflang {
    /// new 创建实例。
    ///
    /// 所有内置函数（含并发原语）已在 VM::new 中统一注册。
    pub fn new() -> Self {
        let vm = VM::new();
        Sflang { vm }
    }

    /// set_output 设置输出（须 Send 以支持跨线程共享）。
    pub fn set_output(&mut self, w: impl std::io::Write + Send + 'static) {
        self.vm.set_output(w);
    }

    /// set_global 设置全局变量（可传参）。
    pub fn set_global(&mut self, name: &str, val: Value) {
        self.vm.set_global(name, val);
    }

    /// get_global 读取全局变量（获取返回值）。
    pub fn get_global(&self, name: &str) -> Option<Value> {
        self.vm.get_global(name)
    }

    /// vm_mut 获取 VM 的可变引用（高级用途）。
    pub fn vm_mut(&mut self) -> &mut VM {
        &mut self.vm
    }

    /// compile_source 编译源码为 Code。
    pub fn compile_source(src: &str, file: &str) -> Result<Arc<crate::opcode::Code>, String> {
        let tokens = tokenize(src, file).map_err(|e| format!("lex error: {}", e))?;
        let prog = parse_program(tokens, file).map_err(|e| format!("parse error: {}", e))?;
        let code = compile(&prog).map_err(|e| format!("compile error: {}", e))?;
        Ok(Arc::new(code))
    }

    /// run_string 编译并执行源码字符串。
    ///
    /// VM 执行（含编译阶段）被 catch_unwind 包裹，内部 panic 转为 Err 返回。
    pub fn run_string(&mut self, src: &str) -> Result<Value, Value> {
        guard_catch(|| {
            let code = Self::compile_source(src, "<string>")
                .map_err(|e| crate::value::error_value(e))?;
            self.vm.run(code)
        })
    }

    /// vm_run_code 执行预编译的 Code。
    ///
    /// VM 执行被 catch_unwind 包裹，内部 panic 转为 Err 返回。
    pub fn vm_run_code(&mut self, code: Arc<crate::opcode::Code>) -> Result<Value, Value> {
        guard_catch(|| self.vm.run(code))
    }

    /// run_file 编译并执行脚本文件。
    ///
    /// VM 执行（含编译阶段）被 catch_unwind 包裹，内部 panic 转为 Err 返回。
    pub fn run_file(&mut self, path: &str) -> Result<Value, Value> {
        guard_catch(|| {
            let src = std::fs::read_to_string(path).map_err(|e| {
                crate::value::error_value(format!("read file failed: {}", e))
            })?;
            let code = Self::compile_source(&src, path).map_err(|e| {
                crate::value::error_value(e)
            })?;
            self.vm.run(code)
        })
    }

    /// call_func 调用已定义的全局函数。
    ///
    /// 直接通过 VM 调用栈执行（零编译开销，不污染全局命名空间）。
    /// name 为脚本中定义的全局函数名，args 为实参列表。
    /// VM 执行被 catch_unwind 包裹，内部 panic 转为 Err 返回。
    pub fn call_func(&mut self, name: &str, args: &[Value]) -> Result<Value, Value> {
        guard_catch(|| {
            let callee = self.vm.get_global(name).ok_or_else(|| {
                crate::value::error_value(format!("function not found: {}", name))
            })?;
            self.vm.call_function_value(callee, args.to_vec())
        })
    }
}

impl Default for Sflang {
    fn default() -> Self {
        Self::new()
    }
}
