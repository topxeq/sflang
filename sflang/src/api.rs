//! api.rs — 嵌入式 API
//!
//! 提供 Sflang 作为第三方库被其他 Rust 程序调用的简洁接口。
//!
//! 用法：
//! ```ignore
//! use sflang::Sflang;
//!
//! let mut sf = Sflang::new();
//! let result = sf.run_string("1 + 2").unwrap();
//! ```

use std::sync::Arc;

use crate::compiler::compile;
use crate::lexer::tokenize;
use crate::parser::parse_program;
use crate::value::Value;
use crate::vm::VM;

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

    /// compile_source 编译源码为 Code。
    pub fn compile_source(src: &str, file: &str) -> Result<Arc<crate::opcode::Code>, String> {
        let tokens = tokenize(src, file).map_err(|e| format!("lex error: {}", e))?;
        let prog = parse_program(tokens, file).map_err(|e| format!("parse error: {}", e))?;
        let code = compile(&prog).map_err(|e| format!("compile error: {}", e))?;
        Ok(Arc::new(code))
    }

    /// run_string 编译并执行源码字符串。
    pub fn run_string(&mut self, src: &str) -> Result<Value, Value> {
        let code = Self::compile_source(src, "<string>").map_err(|e| {
            crate::value::error_value(e)
        })?;
        self.vm.run(code)
    }

    /// vm_run_code 执行预编译的 Code。
    pub fn vm_run_code(&mut self, code: Arc<crate::opcode::Code>) -> Result<Value, Value> {
        self.vm.run(code)
    }

    /// run_file 编译并执行脚本文件。
    pub fn run_file(&mut self, path: &str) -> Result<Value, Value> {
        let src = std::fs::read_to_string(path).map_err(|e| {
            crate::value::error_value(format!("read file failed: {}", e))
        })?;
        let code = Self::compile_source(&src, path).map_err(|e| {
            crate::value::error_value(e)
        })?;
        self.vm.run(code)
    }

    /// call_func 调用已定义的全局函数。
    pub fn call_func(&mut self, name: &str, args: &[Value]) -> Result<Value, Value> {
        let _f = self.vm.get_global(name).ok_or_else(|| {
            crate::value::error_value(format!("function not found: {}", name))
        })?;
        // 通过构造调用栈执行
        // 简化：编译 "name(args)" 并执行
        let arg_exprs: Vec<String> = args.iter().enumerate()
            .map(|(i, _)| format!("__arg{}", i))
            .collect();
        let call_src = if arg_exprs.is_empty() {
            format!("{}()", name)
        } else {
            format!("{}({})", name, arg_exprs.join(", "))
        };
        // 设置参数为全局变量
        for (i, arg) in args.iter().enumerate() {
            self.vm.set_global(&format!("__arg{}", i), arg.clone());
        }
        self.run_string(&call_src)
    }
}

impl Default for Sflang {
    fn default() -> Self {
        Self::new()
    }
}
