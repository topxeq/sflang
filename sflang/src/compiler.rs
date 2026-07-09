//! compiler.rs — AST → 字节码编译器
//!
//! 设计要点：
//!   - 变量四分类：local（slot 数组）/free（闭包捕获）/global（globals）/dynamic（回退）
//!   - 函数内 var 走 OpStoreLocal，顶层 var 走 OpStoreGlobal
//!   - 闭包捕获：编译期标记 captured，运行时 OpClosure 提取到 free_vars
//!   - 循环 break/continue 用跳转回填
//!   - try/catch/finally 用 PushTry/PopTry/ExitFinally
//!
//! 闭包实现：
//!   - 子编译器克隆父作用域链用于 free_vars 解析
//!   - 运行时 OpClosure 按需创建 box（被捕获的 local 装箱共享）
//!   - 一旦 local 被装箱，后续读写自动走 box，实现 live 共享

use std::sync::Arc;

use crate::ast::*;
use crate::function::Function;
use crate::opcode::{Code, FreeSource, Opcode};
use crate::value::Value;

/// CompileError 编译错误。
#[derive(Debug, Clone)]
pub struct CompileError {
    /// msg 错误信息。
    pub msg: String,
    /// line 源码行号（0 表示未知）。
    pub line: u32,
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "compile error at line {}: {}", self.line, self.msg)
    }
}

impl std::error::Error for CompileError {}

/// Compiler 编译器。
pub struct Compiler {
    /// code 当前正在构建的 Code（顶层或函数体）。
    code: Code,
    /// file 源码文件名。
    file: String,
    /// loops 循环上下文栈（break/continue 回填）。
    loops: Vec<LoopCtx>,
    /// scopes 作用域栈。空表示顶层（用 globals）。
    scopes: Vec<Scope>,
    /// func_local_count 当前函数局部变量总数（用于 slot 分配，函数级唯一）。
    func_local_count: usize,
}

/// LoopCtx 循环上下文（break/continue 跳转回填）。
struct LoopCtx {
    /// break_jumps 待回填的 break 跳转偏移列表。
    break_jumps: Vec<usize>,
    /// continue_jumps 待回填的 continue 跳转偏移列表。
    continue_jumps: Vec<usize>,
}

/// Scope 一层作用域（函数作用域或块作用域）。
#[derive(Clone)]
struct Scope {
    /// slots 名字→局部槽位索引。
    slots: std::collections::HashMap<String, usize>,
    /// captured 被内层函数捕获的变量名集合（仅用于调试）。
    captured: std::collections::HashSet<String>,
    /// free_vars 本函数捕获的外层变量。
    free_vars: Vec<FreeVarEntry>,
    /// is_function 是否为函数体作用域（pop 时设置 num_locals/free_sources）。
    is_function: bool,
}

/// FreeVarEntry 自由变量条目。
#[derive(Clone)]
struct FreeVarEntry {
    /// name 变量名。
    name: String,
    /// is_local true=从外层 local 捕获，false=从外层 free_var 捕获。
    is_local: bool,
    /// index 外层 local slot 或 free_var index。
    index: usize,
}

/// VarKind 变量解析结果。
enum VarKind {
    /// Local 局部变量（slot 索引）。
    Local(usize),
    /// Free 自由变量（捕获索引）。
    Free(usize),
    /// Global 全局变量（回退）。
    Global,
}

impl Compiler {
    /// new 创建编译器。
    pub fn new(file: &str, func_name: &str) -> Self {
        Compiler {
            code: Code::new(func_name, file),
            file: file.to_string(),
            loops: Vec::new(),
            scopes: Vec::new(),
            func_local_count: 0,
        }
    }

    /// compile_program 编译整个程序为顶层 Code。
    pub fn compile_program(prog: &Program) -> Result<Code, CompileError> {
        let mut c = Compiler::new(&prog.file, "<script>");
        c.compile_stmts(&prog.stmts)?;
        c.code.emit(Opcode::ReturnVoid);
        c.code.num_locals = c.func_local_count;
        Ok(c.code)
    }

    fn compile_stmts(&mut self, stmts: &[Stmt]) -> Result<(), CompileError> {
        for s in stmts {
            self.compile_stmt(s)?;
        }
        Ok(())
    }

    fn set_line(&mut self, line: u32) {
        self.code.set_line(line);
    }

    fn err(&self, tok_line: u32, msg: impl Into<String>) -> CompileError {
        CompileError { msg: msg.into(), line: tok_line }
    }

    // ---- 作用域管理 ----

    fn push_scope(&mut self, is_function: bool) {
        self.scopes.push(Scope {
            slots: std::collections::HashMap::new(),
            captured: std::collections::HashSet::new(),
            free_vars: Vec::new(),
            is_function,
        });
    }

    fn pop_scope(&mut self) -> Scope {
        self.scopes.pop().expect("scope stack underflow")
    }

    /// declare_local 在当前作用域声明局部变量，返回函数级槽位索引。
    /// slot 索引相对于整个函数（非 per-scope），块作用域共享函数的 slot 空间。
    fn declare_local(&mut self, name: &str) -> usize {
        if self.scopes.is_empty() {
            // 顶层无 scope：返回 usize::MAX 表示走 global
            return usize::MAX;
        }
        // 先检查是否已存在
        if let Some(&slot) = self.scopes.last().unwrap().slots.get(name) {
            return slot;
        }
        // 分配新 slot（函数级唯一）
        let slot = self.func_local_count;
        self.func_local_count += 1;
        self.scopes.last_mut().unwrap().slots.insert(name.to_string(), slot);
        slot
    }

    /// resolve_local 在当前函数作用域内查找局部变量。
    /// 沿当前函数的块作用域向上查找（但不超过函数边界）。
    fn resolve_local(&self, name: &str) -> Option<usize> {
        // 从最内层向外查找，遇到函数作用域边界停止
        for s in self.scopes.iter().rev() {
            if let Some(&slot) = s.slots.get(name) {
                return Some(slot);
            }
            if s.is_function {
                break;
            }
        }
        None
    }

    /// resolve_free 查找自由变量（捕获外层变量）。
    /// 在外层函数作用域中查找，自动补中间层的 free_var。
    fn resolve_free(&mut self, name: &str) -> Option<usize> {
        if self.scopes.is_empty() {
            return None;
        }
        // 找到当前函数作用域的位置
        let cur_func_idx = self.find_cur_function_scope()?;
        // 先查已有 free_vars
        let existing = self.scopes[cur_func_idx].free_vars.iter().position(|fv| fv.name == name);
        if let Some(i) = existing {
            return Some(i);
        }
        // 沿外层函数作用域查找
        let mut func_iter_idx = cur_func_idx;
        while func_iter_idx > 0 {
            func_iter_idx = self.find_outer_function_scope(func_iter_idx)?;
            // 在该外层函数的所有块作用域中查找（从内到外）
            // 简化：直接在该函数作用域查
            if let Some(&slot) = self.scopes[func_iter_idx].slots.get(name) {
                self.scopes[func_iter_idx].captured.insert(name.to_string());
                // 给所有中间函数层（func_iter_idx+1..=cur_func_idx）补 free_var
                let mut last_is_local = true;
                let mut last_index = slot;
                let mut mid = func_iter_idx + 1;
                while mid <= cur_func_idx {
                    // 跳过非函数块作用域（块作用域共享父函数的 free_vars）
                    if self.scopes[mid].is_function || mid == cur_func_idx {
                        let fv_idx = self.scopes[mid].free_vars.len();
                        self.scopes[mid].free_vars.push(FreeVarEntry {
                            name: name.to_string(),
                            is_local: last_is_local,
                            index: last_index,
                        });
                        last_is_local = false;
                        last_index = fv_idx;
                    }
                    mid += 1;
                }
                return Some(last_index);
            }
        }
        None
    }

    /// find_cur_function_scope 找到当前所在函数作用域的索引。
    fn find_cur_function_scope(&self) -> Option<usize> {
        for (i, s) in self.scopes.iter().enumerate().rev() {
            if s.is_function {
                return Some(i);
            }
        }
        None
    }

    /// find_outer_function_scope 找到指定函数作用域之外最近的函数作用域。
    fn find_outer_function_scope(&self, start: usize) -> Option<usize> {
        if start == 0 {
            return None;
        }
        for i in (0..start).rev() {
            if self.scopes[i].is_function {
                return Some(i);
            }
        }
        None
    }

    fn resolve_var(&mut self, name: &str) -> VarKind {
        if let Some(slot) = self.resolve_local(name) {
            return VarKind::Local(slot);
        }
        if let Some(idx) = self.resolve_free(name) {
            return VarKind::Free(idx);
        }
        VarKind::Global
    }

    fn emit_load_var(&mut self, name: &str) {
        match self.resolve_var(name) {
            VarKind::Local(slot) => { self.code.emit_u16(Opcode::LoadLocal, slot as u16); }
            VarKind::Free(idx) => { self.code.emit_u16(Opcode::LoadFree, idx as u16); }
            VarKind::Global => {
                let idx = self.code.add_name(name);
                self.code.emit_u16(Opcode::LoadName, idx as u16);
            }
        }
    }

    /// emit_store_var 处理 var 声明：函数内为 local，顶层为 global。
    fn emit_store_var(&mut self, name: &str) {
        if self.scopes.is_empty() {
            let idx = self.code.add_name(name);
            self.code.emit_u16(Opcode::StoreGlobal, idx as u16);
            return;
        }
        let slot = self.declare_local(name);
        self.code.emit_u16(Opcode::StoreLocal, slot as u16);
    }

    /// emit_assign_var 处理赋值（不声明新变量）。
    fn emit_assign_var(&mut self, name: &str) {
        match self.resolve_var(name) {
            VarKind::Local(slot) => { self.code.emit_u16(Opcode::StoreLocal, slot as u16); }
            VarKind::Free(idx) => { self.code.emit_u16(Opcode::StoreFree, idx as u16); }
            VarKind::Global => {
                let idx = self.code.add_name(name);
                self.code.emit_u16(Opcode::AssignName, idx as u16);
            }
        }
    }

    // ---- 语句编译 ----

    fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), CompileError> {
        match stmt {
            Stmt::ExprStmt { expr, tok } => {
                self.set_line(tok.line);
                self.compile_expr(expr)?;
                self.code.emit(Opcode::Pop);
            }
            Stmt::VarDecl { name, value, tok } => {
                self.set_line(tok.line);
                match value {
                    Some(e) => self.compile_expr(e)?,
                    None => { self.code.emit(Opcode::Null); }
                }
                self.emit_store_var(name);
            }
            Stmt::FuncDecl { func, tok } => {
                self.set_line(tok.line);
                // 先声明 name（让函数体内能引用自身实现递归）
                // declare_local 在编译期分配 slot，运行时 Closure 后才赋值
                // 函数体内引用 name 会走 resolve_free（捕获外层 local，运行时通过 box 共享）
                let slot = self.declare_local(&func.name);
                let is_global = slot == usize::MAX;
                let f = self.compile_func_lit(func)?;
                let idx = self.code.add_const(Value::Func(Arc::new(f)));
                self.code.emit_u16(Opcode::Closure, idx as u16);
                // 存到已声明的 slot（不重新声明）
                if is_global {
                    let name_idx = self.code.add_name(&func.name);
                    self.code.emit_u16(Opcode::StoreGlobal, name_idx as u16);
                } else {
                    self.code.emit_u16(Opcode::StoreLocal, slot as u16);
                }
            }
            Stmt::IfStmt { cond, then, elif_conds, elif_bodies, else_block, tok } => {
                self.set_line(tok.line);
                self.compile_expr(cond)?;
                let jump_to_next = self.code.emit_u16(Opcode::JumpIfFalse, 0);
                self.compile_block(then)?;
                let mut end_jumps = vec![self.code.emit_u16(Opcode::Jump, 0)];
                self.code.patch_u16(jump_to_next, self.code.insts.len() as u16);
                for (ec, eb) in elif_conds.iter().zip(elif_bodies.iter()) {
                    self.compile_expr(ec)?;
                    let jf = self.code.emit_u16(Opcode::JumpIfFalse, 0);
                    self.compile_block(eb)?;
                    end_jumps.push(self.code.emit_u16(Opcode::Jump, 0));
                    self.code.patch_u16(jf, self.code.insts.len() as u16);
                }
                if let Some(eb) = else_block {
                    self.compile_block(eb)?;
                }
                let end = self.code.insts.len() as u16;
                for j in end_jumps {
                    self.code.patch_u16(j, end);
                }
            }
            Stmt::WhileStmt { cond, body, tok } => {
                self.set_line(tok.line);
                let start = self.code.insts.len();
                self.loops.push(LoopCtx { break_jumps: vec![], continue_jumps: vec![] });
                self.compile_expr(cond)?;
                let j_end = self.code.emit_u16(Opcode::JumpIfFalse, 0);
                self.compile_block(body)?;
                self.code.emit_u16(Opcode::Jump, start as u16);
                let end = self.code.insts.len();
                self.code.patch_u16(j_end, end as u16);
                let lc = self.loops.pop().unwrap();
                for j in lc.break_jumps { self.code.patch_u16(j, end as u16); }
                for j in lc.continue_jumps { self.code.patch_u16(j, start as u16); }
            }
            Stmt::ForStmt { init, cond, post, body, tok } => {
                self.set_line(tok.line);
                // for (init; cond; post) body
                // 等价于：{ init; while cond { body; post } }
                if let Some(s) = init {
                    self.compile_stmt(s)?;
                }
                let start = self.code.insts.len();
                self.loops.push(LoopCtx { break_jumps: vec![], continue_jumps: vec![] });
                if let Some(c) = cond {
                    self.compile_expr(c)?;
                    let j_end = self.code.emit_u16(Opcode::JumpIfFalse, 0);
                    // 注：j_end 在循环结束 patch
                    self.compile_block(body)?;
                    // continue 跳到 post
                    let continue_target = self.code.insts.len();
                    if let Some(p) = post {
                        self.compile_stmt(p)?;
                    }
                    self.code.emit_u16(Opcode::Jump, start as u16);
                    let end = self.code.insts.len();
                    self.code.patch_u16(j_end, end as u16);
                    let lc = self.loops.pop().unwrap();
                    for j in lc.break_jumps { self.code.patch_u16(j, end as u16); }
                    for j in lc.continue_jumps { self.code.patch_u16(j, continue_target as u16); }
                } else {
                    // 无 cond：无限循环
                    self.compile_block(body)?;
                    let continue_target = self.code.insts.len();
                    if let Some(p) = post {
                        self.compile_stmt(p)?;
                    }
                    self.code.emit_u16(Opcode::Jump, start as u16);
                    // 无退出点（除非 break）
                    let lc = self.loops.pop().unwrap();
                    // break 跳到这里（实际上无 cond 的 for 只能 break 退出）
                    let end = self.code.insts.len();
                    for j in lc.break_jumps { self.code.patch_u16(j, end as u16); }
                    for j in lc.continue_jumps { self.code.patch_u16(j, continue_target as u16); }
                }
            }
            Stmt::ForInStmt { index_var, var, iter, body, tok } => {
                self.set_line(tok.line);
                // for-in 编译为（在块作用域内，所有变量都是 local）：
                //   eval iter -> __iter
                //   __idx = 0
                //   start:
                //   __idx < len(__iter) ? 否则跳 end
                //   var = __iter[__idx]
                //   [index_var = __idx]
                //   body
                //   continue_target:
                //   __idx = __idx + 1
                //   jump start
                //   end:
                self.push_scope(false);
                let iter_slot = self.declare_local("__forin_iter");
                let idx_slot = self.declare_local("__forin_idx");
                let var_slot = self.declare_local(var);
                let index_slot_opt = if let Some(iv) = index_var {
                    Some(self.declare_local(iv))
                } else {
                    None
                };

                // 编译 iter，留栈
                self.compile_expr(iter)?;
                // 存到 __iter
                self.code.emit_u16(Opcode::StoreLocal, iter_slot as u16);
                // __keys = keys(__iter) —— 统一用 keys() 获取索引数组
                //   object → string 键数组；array → [0,1,2...]；string → [0,1,2...]
                // 这样后续用 __keys[__idx] 取 key，再用 __iter[key] 取 value
                // 解决了 object 不能用 int 索引的问题
                let keys_slot = self.declare_local("__forin_keys");
                let key_slot = self.declare_local("__forin_key");
                let keys_name_idx = self.code.add_name("keys");
                self.code.emit_u16(Opcode::LoadName, keys_name_idx as u16);
                self.code.emit_u16(Opcode::LoadLocal, iter_slot as u16);
                self.code.emit_u8(Opcode::Call, 1);
                self.code.emit_u16(Opcode::StoreLocal, keys_slot as u16);
                // __idx = 0
                let zero_idx = self.code.add_const(Value::Int(0));
                self.code.emit_u16(Opcode::Const, zero_idx as u16);
                self.code.emit_u16(Opcode::StoreLocal, idx_slot as u16);

                let start = self.code.insts.len();
                self.loops.push(LoopCtx { break_jumps: vec![], continue_jumps: vec![] });

                // 压入 __idx（LT 的左操作数先压）
                self.code.emit_u16(Opcode::LoadLocal, idx_slot as u16);
                // 计算 len(__keys)
                let len_name_idx = self.code.add_name("len");
                self.code.emit_u16(Opcode::LoadName, len_name_idx as u16);
                self.code.emit_u16(Opcode::LoadLocal, keys_slot as u16);
                self.code.emit_u8(Opcode::Call, 1);
                // 栈：[idx, len]
                self.code.emit(Opcode::LT);
                let j_end = self.code.emit_u16(Opcode::JumpIfFalse, 0);

                // __key = __keys[__idx]
                self.code.emit_u16(Opcode::LoadLocal, keys_slot as u16);
                self.code.emit_u16(Opcode::LoadLocal, idx_slot as u16);
                self.code.emit(Opcode::IndexGet);
                self.code.emit_u16(Opcode::StoreLocal, key_slot as u16);

                // var = __iter[__key]（object 用 string 键，array/string 用 int 键）
                self.code.emit_u16(Opcode::LoadLocal, iter_slot as u16);
                self.code.emit_u16(Opcode::LoadLocal, key_slot as u16);
                self.code.emit(Opcode::IndexGet);
                self.code.emit_u16(Opcode::StoreLocal, var_slot as u16);

                // index_var = __key（如果有）—— object 时是 string 键，array 时是 int 索引
                if let Some(slot) = index_slot_opt {
                    self.code.emit_u16(Opcode::LoadLocal, key_slot as u16);
                    self.code.emit_u16(Opcode::StoreLocal, slot as u16);
                }

                // body
                self.compile_block(body)?;

                // continue_target: __idx = __idx + 1
                let continue_target = self.code.insts.len();
                self.code.emit_u16(Opcode::LoadLocal, idx_slot as u16);
                let one_idx = self.code.add_const(Value::Int(1));
                self.code.emit_u16(Opcode::Const, one_idx as u16);
                self.code.emit(Opcode::Add);
                self.code.emit_u16(Opcode::StoreLocal, idx_slot as u16);

                // jump start
                self.code.emit_u16(Opcode::Jump, start as u16);

                // end:
                let end = self.code.insts.len();
                self.code.patch_u16(j_end, end as u16);
                let lc = self.loops.pop().unwrap();
                for j in lc.break_jumps { self.code.patch_u16(j, end as u16); }
                for j in lc.continue_jumps { self.code.patch_u16(j, continue_target as u16); }
                self.pop_scope();
            }
            Stmt::BreakStmt { tok } => {
                self.set_line(tok.line);
                if let Some(lc) = self.loops.last_mut() {
                    let j = self.code.emit_u16(Opcode::Jump, 0);
                    lc.break_jumps.push(j);
                } else {
                    return Err(self.err(tok.line, "break 不在循环内"));
                }
            }
            Stmt::ContinueStmt { tok } => {
                self.set_line(tok.line);
                if let Some(lc) = self.loops.last_mut() {
                    let j = self.code.emit_u16(Opcode::Jump, 0);
                    lc.continue_jumps.push(j);
                } else {
                    return Err(self.err(tok.line, "continue 不在循环内"));
                }
            }
            Stmt::ReturnStmt { value, tok } => {
                self.set_line(tok.line);
                match value {
                    Some(e) => self.compile_expr(e)?,
                    None => { self.code.emit(Opcode::Null); }
                }
                self.code.emit(Opcode::Return);
            }
            Stmt::TryStmt { try_block, catch_var, catch_block, finally_block, tok } => {
                self.set_line(tok.line);
                // 编译 try/catch/finally 结构：
                //   PushTry catch_ip finally_ip
                //   <try block>
                //   PopTry
                //   Jump end
                //   catch_ip: <catch block>  // 栈顶已是异常值
                //   Jump end (or finally)
                //   finally_ip: <finally block>
                //   ExitFinally
                //   end:
                // 用块作用域包裹，确保 catch_var 等变量是 local
                self.push_scope(false);
                let push_try_off = self.code.emit_push_try(0, 0);
                self.compile_block(try_block)?;
                self.code.emit(Opcode::PopTry);
                let jump_after_try = self.code.emit_u16(Opcode::Jump, 0);
                // catch 块
                let catch_ip = self.code.insts.len() as u16;
                if let (Some(var), Some(block)) = (catch_var, catch_block) {
                    // 异常值在栈顶，存入 catch_var
                    let slot = self.declare_local(var);
                    self.code.emit_u16(Opcode::StoreLocal, slot as u16);
                    self.compile_block(block)?;
                    // catch 块结束，跳到 finally 或 end
                    let jump_after_catch = self.code.emit_u16(Opcode::Jump, 0);
                    // finally 块
                    let finally_ip = self.code.insts.len() as u16;
                    if let Some(fb) = finally_block {
                        // patch jump_after_catch -> 跳过 finally 到 end
                        // 实际上 catch 末尾应跳到 end，不应进入 finally
                        // 但若 finally 存在，正常路径也要执行 finally
                        // 简化处理：catch 末尾跳到 finally 入口
                        self.code.patch_u16(jump_after_catch, finally_ip);
                        self.compile_block(fb)?;
                        self.code.emit(Opcode::ExitFinally);
                        // end:
                        let end = self.code.insts.len() as u16;
                        self.code.patch_u16(jump_after_try, end);
                        self.code.patch_push_try(push_try_off, catch_ip, finally_ip);
                    } else {
                        // 无 finally：finally_ip 设为 end
                        let end = self.code.insts.len() as u16;
                        self.code.patch_u16(jump_after_catch, end);
                        self.code.patch_u16(jump_after_try, end);
                        self.code.patch_push_try(push_try_off, catch_ip, end);
                    }
                } else {
                    // 无 catch：catch_ip 和 finally_ip 都指向 end（无 catch 标记为 catch_ip >= finally_ip）
                    let finally_ip = self.code.insts.len() as u16;
                    if let Some(fb) = finally_block {
                        // try 正常完成应跳到 finally（不是 end）
                        self.code.patch_u16(jump_after_try, finally_ip);
                        self.compile_block(fb)?;
                        self.code.emit(Opcode::ExitFinally);
                        let end = self.code.insts.len() as u16;
                        // catch_ip = end（>= finally_ip 表示无 catch），finally_ip 正常
                        self.code.patch_push_try(push_try_off, end, finally_ip);
                    } else {
                        // 既无 catch 也无 finally：实际上 try 无意义
                        let end = self.code.insts.len() as u16;
                        self.code.patch_u16(jump_after_try, end);
                        self.code.patch_push_try(push_try_off, end, end);
                    }
                }
                self.pop_scope();
            }
            Stmt::DeferStmt { call, tok } => {
                self.set_line(tok.line);
                // defer 调用：编译 call 表达式，但用 OpDefer 注册
                // 编译 callee 和 args，再用 OpDefer
                if let Expr::CallExpr { callee, args, .. } = call {
                    self.compile_expr(callee)?;
                    for a in args {
                        self.compile_expr(a)?;
                    }
                    self.code.emit_u8(Opcode::Defer, args.len() as u8);
                } else {
                    return Err(self.err(tok.line, "defer 后必须是函数调用"));
                }
            }
            Stmt::RunStmt { call, tok } => {
                self.set_line(tok.line);
                // run 关键字：编译 call 表达式，用 OpRun 启动新线程
                if let Expr::CallExpr { callee, args, .. } = call {
                    self.compile_expr(callee)?;
                    for a in args {
                        self.compile_expr(a)?;
                    }
                    self.code.emit_u8(Opcode::Run, args.len() as u8);
                } else {
                    return Err(self.err(tok.line, "run 后必须是函数调用"));
                }
            }
            Stmt::ThrowStmt { expr, tok } => {
                self.set_line(tok.line);
                self.compile_expr(expr)?;
                self.code.emit(Opcode::Throw);
            }
            Stmt::ImportStmt { path, tok } => {
                self.set_line(tok.line);
                // import "path"：路径入名字池，发射 OpImport <u16 name_idx>
                // VM 负责加载、编译、执行目标脚本，顶层定义合并到当前全局环境
                let idx = self.code.add_name(path);
                self.code.emit_u16(Opcode::Import, idx as u16);
            }
            Stmt::Block { stmts, tok } => {
                self.set_line(tok.line);
                // 块作用域：push scope（非函数），编译语句，pop
                // 注：当前简化实现不引入块作用域，所有变量在函数作用域
                // 这样可避免变量遮蔽问题，且性能更好
                self.compile_stmts(stmts)?;
            }
        }
        Ok(())
    }

    fn compile_block(&mut self, block: &Block) -> Result<(), CompileError> {
        self.compile_stmts(&block.stmts)
    }

    // ---- 表达式编译 ----

    fn compile_expr(&mut self, expr: &Expr) -> Result<(), CompileError> {
        match expr {
            Expr::IntLit { value, tok } => {
                self.set_line(tok.line);
                let idx = self.code.add_const(Value::Int(*value));
                self.code.emit_u16(Opcode::Const, idx as u16);
            }
            Expr::FloatLit { value, tok } => {
                self.set_line(tok.line);
                let idx = self.code.add_const(Value::Float(*value));
                self.code.emit_u16(Opcode::Const, idx as u16);
            }
            Expr::StringLit { value, tok } => {
                self.set_line(tok.line);
                let idx = self.code.add_const(Value::str(value));
                self.code.emit_u16(Opcode::Const, idx as u16);
            }
            Expr::BoolLit { value, tok } => {
                self.set_line(tok.line);
                let idx = self.code.add_const(Value::Bool(*value));
                self.code.emit_u16(Opcode::Const, idx as u16);
            }
            Expr::UndefinedLit { tok } => {
                self.set_line(tok.line);
                self.code.emit(Opcode::Null);
            }
            Expr::Ident { name, tok } => {
                self.set_line(tok.line);
                self.emit_load_var(name);
            }
            Expr::ArrayLit { elems, tok } => {
                self.set_line(tok.line);
                for e in elems {
                    self.compile_expr(e)?;
                }
                self.code.emit_u16(Opcode::BuildArray, elems.len() as u16);
            }
            Expr::MapLit { pairs, tok } => {
                self.set_line(tok.line);
                for (k, v) in pairs {
                    // 键作为字符串常量压栈，值压栈
                    let idx = self.code.add_const(Value::str(k));
                    self.code.emit_u16(Opcode::Const, idx as u16);
                    self.compile_expr(v)?;
                }
                self.code.emit_u16(Opcode::BuildMap, pairs.len() as u16);
            }
            Expr::OrdMapLit { pairs, tok } => {
                self.set_line(tok.line);
                for (k, v) in pairs {
                    let idx = self.code.add_const(Value::str(k));
                    self.code.emit_u16(Opcode::Const, idx as u16);
                    self.compile_expr(v)?;
                }
                self.code.emit_u16(Opcode::BuildOrdMap, pairs.len() as u16);
            }
            // 三元条件表达式 cond ? then : else_（语法糖，编译为跳转）
            //   eval cond          ; [c]
            //   JumpIfFalse L_else ; 弹 c，假则跳 L_else
            //   eval then          ; [t]
            //   Jump L_end
            //   L_else: eval else_ ; [e]
            //   L_end:             ; 结果在栈顶
            Expr::Ternary { cond, then, else_, tok } => {
                self.set_line(tok.line);
                self.compile_expr(cond)?;
                let j_else = self.code.emit_u16(Opcode::JumpIfFalse, 0);
                self.compile_expr(then)?;
                let j_end = self.code.emit_u16(Opcode::Jump, 0);
                let l_else = self.code.insts.len() as u16;
                self.code.patch_u16(j_else, l_else);
                self.compile_expr(else_)?;
                let l_end = self.code.insts.len() as u16;
                self.code.patch_u16(j_end, l_end);
            }
            // 自增自减 ++ / --（前缀返回新值，后缀返回旧值）
            Expr::IncDec { target, op, prefix, tok } => {
                self.set_line(tok.line);
                let prefix = *prefix;
                match target {
                    AssignTarget::Name(name) => {
                        // 简单变量：读-算-写（变量读无副作用，求值两次安全）
                        // 后缀需先 Dup 旧值保留，再算再写；前缀直接算写
                        self.emit_load_var(name);
                        if !prefix {
                            // 后缀：保留旧值副本到栈底
                            // 顺序：load old; push 1; <add|sub>; store var（store 会 pop）
                            // 但要先留 old。用：load old; const 1; op; <new on top>; swap? 无 swap
                            // 改：load old; load old(再读一次); push1; op; store(写新); 留 old
                            self.emit_load_var(name); // [old, old]
                            let one_idx = self.code.add_const(Value::Int(1));
                            self.code.emit_u16(Opcode::Const, one_idx as u16); // [old, old, 1]
                            match op {
                                IncDecOp::Inc => { self.code.emit(Opcode::Add); }
                                IncDecOp::Dec => { self.code.emit(Opcode::Sub); }
                            }
                            // [old, new]
                            self.emit_assign_var(name); // 写 new，pop new -> [old]
                            // 栈留 old（后缀返回旧值）
                        } else {
                            // 前缀：load old; push1; op; dup new; store new; 留 new
                            let one_idx = self.code.add_const(Value::Int(1));
                            self.code.emit_u16(Opcode::Const, one_idx as u16);
                            match op {
                                IncDecOp::Inc => { self.code.emit(Opcode::Add); }
                                IncDecOp::Dec => { self.code.emit(Opcode::Sub); }
                            }
                            self.code.emit(Opcode::Dup); // [new, new]
                            self.emit_assign_var(name); // 写 new，pop -> [new]
                        }
                    }
                    AssignTarget::Index { obj, index } => {
                        // a[i]++：地址只求值一次，用 IncDecIndex
                        self.compile_expr(obj)?;
                        self.compile_expr(index)?;
                        // flag: bit0=op(0=inc,1=dec), bit7=前缀(0)/后缀(1)
                        let flag: u8 = match op {
                            IncDecOp::Inc => if prefix { 0x00 } else { 0x80 },
                            IncDecOp::Dec => if prefix { 0x01 } else { 0x81 },
                        };
                        self.code.emit_u8(Opcode::IncDecIndex, flag);
                    }
                    AssignTarget::Member { obj, name } => {
                        // obj.k++：地址只求值一次，用 IncDecMember
                        self.compile_expr(obj)?;
                        let name_idx = self.code.add_name(name);
                        let flag: u8 = match op {
                            IncDecOp::Inc => if prefix { 0x00 } else { 0x80 },
                            IncDecOp::Dec => if prefix { 0x01 } else { 0x81 },
                        };
                        self.code.emit_u8_u8(Opcode::IncDecMember, name_idx as u8, flag);
                    }
                    AssignTarget::Deref { .. } => {
                        return Err(self.err(tok.line, "*p 的 ++/-- 暂不支持，请用 *p = *p + 1"));
                    }
                }
            }
            // 复合赋值 op=（target op= value）
            Expr::CompoundAssign { target, op, value, tok } => {
                self.set_line(tok.line);
                match target {
                    AssignTarget::Name(name) => {
                        // 简单变量：load old; <按 op 处理>; store（返回新值）
                        self.emit_load_var(name);
                        let opcode = binary_op_to_opcode(*op);
                        match opcode {
                            Some(opc) => {
                                // 算术/位运算：load old; eval value; <op>; dup; store
                                self.compile_expr(value)?;
                                self.code.emit(opc);
                                self.code.emit(Opcode::Dup);
                                self.emit_assign_var(name);
                            }
                            None => {
                                // NullCoal(??=)：load old; dup; JumpIfNotUndefined end; pop; eval value; end; dup; store
                                self.code.emit(Opcode::Dup);
                                let j_end = self.code.emit_u16(Opcode::JumpIfNotUndefined, 0);
                                self.code.emit(Opcode::Pop);
                                self.compile_expr(value)?;
                                let end = self.code.insts.len() as u16;
                                self.code.patch_u16(j_end, end);
                                self.code.emit(Opcode::Dup);
                                self.emit_assign_var(name);
                            }
                        }
                    }
                    AssignTarget::Index { obj, index } => {
                        // a[i] op= v：地址只求值一次，用 CompoundIndex
                        // 栈序：[v, obj, idx]
                        self.compile_expr(value)?;
                        self.compile_expr(obj)?;
                        self.compile_expr(index)?;
                        let flag = binary_op_to_flag(*op);
                        self.code.emit_u8(Opcode::CompoundIndex, flag);
                    }
                    AssignTarget::Member { obj, name } => {
                        // obj.k op= v：地址只求值一次，用 CompoundMember
                        // 栈序：[v, obj]
                        self.compile_expr(value)?;
                        self.compile_expr(obj)?;
                        let name_idx = self.code.add_name(name);
                        let flag = binary_op_to_flag(*op);
                        self.code.emit_u8_u8(Opcode::CompoundMember, name_idx as u8, flag);
                    }
                    AssignTarget::Deref { .. } => {
                        return Err(self.err(tok.line, "*p 的复合赋值暂不支持，请用 *p = *p + v"));
                    }
                }
            }
            Expr::BinaryExpr { op, left, right, tok } => {
                self.set_line(tok.line);
                match op {
                    BinaryOp::NullCoal => {
                        // 空合并 ?? ：短路编译（与 And/Or 的结构平行）
                        //   eval left          ; [a]
                        //   Dup                ; [a, a]
                        //   JumpIfNotUndefined L_end  ; 弹出顶 a，非 undefined 则跳 L_end，留 [a]
                        //   Pop                ; a 是 undefined，丢弃 -> []
                        //   eval right         ; [b]
                        //   L_end:             ; 结果在栈顶
                        // 关键：判定条件是"是否为 undefined"，与 truthy 无关（0/""/false 视为有效值）。
                        self.compile_expr(left)?;
                        self.code.emit(Opcode::Dup);
                        let j_end = self.code.emit_u16(Opcode::JumpIfNotUndefined, 0);
                        self.code.emit(Opcode::Pop);
                        self.compile_expr(right)?;
                        let end = self.code.insts.len() as u16;
                        self.code.patch_u16(j_end, end);
                    }
                    BinaryOp::And => {
                        // 短路逻辑与，结果规范化为布尔值：
                        //   left 假 → false；left 真时取 right 的真假。
                        // VM 的 JumpIfFalse 会弹出条件值，故每个分支独立压入布尔结果。
                        let true_idx = self.code.add_const(Value::Bool(true));
                        let false_idx = self.code.add_const(Value::Bool(false));
                        self.compile_expr(left)?;
                        let j_false1 = self.code.emit_u16(Opcode::JumpIfFalse, 0);
                        // left 真：求 right，再判真假
                        self.compile_expr(right)?;
                        let j_false2 = self.code.emit_u16(Opcode::JumpIfFalse, 0);
                        // right 真 → 压 true
                        self.code.emit_u16(Opcode::Const, true_idx as u16);
                        let j_end1 = self.code.emit_u16(Opcode::Jump, 0);
                        // 任意一边假 → 压 false
                        let l_false = self.code.insts.len() as u16;
                        self.code.patch_u16(j_false1, l_false);
                        self.code.patch_u16(j_false2, l_false);
                        self.code.emit_u16(Opcode::Const, false_idx as u16);
                        self.code.patch_u16(j_end1, self.code.insts.len() as u16);
                    }
                    BinaryOp::Or => {
                        // 短路逻辑或，结果规范化为布尔值：
                        //   left 真 → true；left 假时取 right 的真假。
                        let true_idx = self.code.add_const(Value::Bool(true));
                        let false_idx = self.code.add_const(Value::Bool(false));
                        self.compile_expr(left)?;
                        let j_true1 = self.code.emit_u16(Opcode::JumpIfTrue, 0);
                        // left 假：求 right，再判真假
                        self.compile_expr(right)?;
                        let j_true2 = self.code.emit_u16(Opcode::JumpIfTrue, 0);
                        // right 假 → 压 false
                        self.code.emit_u16(Opcode::Const, false_idx as u16);
                        let j_end1 = self.code.emit_u16(Opcode::Jump, 0);
                        // 任意一边真 → 压 true
                        let l_true = self.code.insts.len() as u16;
                        self.code.patch_u16(j_true1, l_true);
                        self.code.patch_u16(j_true2, l_true);
                        self.code.emit_u16(Opcode::Const, true_idx as u16);
                        self.code.patch_u16(j_end1, self.code.insts.len() as u16);
                    }
                    _ => {
                        self.compile_expr(left)?;
                        self.compile_expr(right)?;
                        let opcode = match op {
                            BinaryOp::Add => Opcode::Add,
                            BinaryOp::Sub => Opcode::Sub,
                            BinaryOp::Mul => Opcode::Mul,
                            BinaryOp::Div => Opcode::Div,
                            BinaryOp::Mod => Opcode::Mod,
                            BinaryOp::Eq => Opcode::Eq,
                            BinaryOp::Neq => Opcode::Neq,
                            BinaryOp::LT => Opcode::LT,
                            BinaryOp::LE => Opcode::LE,
                            BinaryOp::GT => Opcode::GT,
                            BinaryOp::GE => Opcode::GE,
                            BinaryOp::BitAnd => Opcode::BitAnd,
                            BinaryOp::BitOr => Opcode::BitOr,
                            BinaryOp::BitXor => Opcode::BitXor,
                            BinaryOp::BitShl => Opcode::BitShl,
                            BinaryOp::BitShr => Opcode::BitShr,
                            BinaryOp::And | BinaryOp::Or | BinaryOp::NullCoal => unreachable!(),
                        };
                        self.code.emit(opcode);
                        self.set_line(tok.line);
                    }
                }
            }
            Expr::UnaryExpr { op, operand, tok } => {
                self.set_line(tok.line);
                self.compile_expr(operand)?;
                match op {
                    UnaryOp::Neg => { self.code.emit(Opcode::Neg); }
                    UnaryOp::Not => { self.code.emit(Opcode::Not); }
                    UnaryOp::BitNot => { self.code.emit(Opcode::BitNot); }
                }
            }
            Expr::IndexExpr { obj, index, tok } => {
                self.set_line(tok.line);
                self.compile_expr(obj)?;
                self.compile_expr(index)?;
                self.code.emit(Opcode::IndexGet);
            }
            Expr::SliceExpr { obj, low, high, tok } => {
                // 切片 a[low:high]：栈布局 [obj, low, high]（缺省压 undefined）
                self.set_line(tok.line);
                self.compile_expr(obj)?;
                match low {
                    Some(e) => self.compile_expr(e)?,
                    None => { self.code.emit(Opcode::Null); }
                }
                match high {
                    Some(e) => self.compile_expr(e)?,
                    None => { self.code.emit(Opcode::Null); }
                }
                self.code.emit(Opcode::Slice);
            }
            Expr::MemberExpr { obj, name, tok } => {
                self.set_line(tok.line);
                self.compile_expr(obj)?;
                let idx = self.code.add_name(name);
                self.code.emit_u16(Opcode::GetMember, idx as u16);
            }
            Expr::CallExpr { callee, args, tok } => {
                self.set_line(tok.line);
                // 方法调用检测：callee 是 MemberExpr → obj.name(args)
                if let Expr::MemberExpr { obj, name, .. } = callee.as_ref() {
                    self.compile_expr(obj)?;
                    for a in args {
                        self.compile_expr(a)?;
                    }
                    if args.len() > 254 {
                        return Err(self.err(tok.line, format!("方法参数过多（{} > 254，含隐式 self）", args.len())));
                    }
                    let name_idx = self.code.add_name(name);
                    self.code.emit_u8_u8(Opcode::MethodCall, name_idx as u8, args.len() as u8);
                    return Ok(());
                }
                // 检测是否有 Spread 参数
                let has_spread = args.iter().any(|a| matches!(a, Expr::Spread { .. }));
                if has_spread {
                    // 带展开的调用：编译 callee + 所有参数（Spread 编译为内部表达式），发 SpreadCall
                    self.compile_expr(callee)?;
                    let mut spread_mask: u8 = 0;
                    for (i, a) in args.iter().enumerate() {
                        match a {
                            Expr::Spread { expr, .. } => {
                                self.compile_expr(expr)?;
                                spread_mask |= 1 << i;
                            }
                            other => { self.compile_expr(other)?; }
                        }
                    }
                    self.code.emit_u8_u8(Opcode::SpreadCall, args.len() as u8, spread_mask);
                    return Ok(());
                }
                // 普通调用
                self.compile_expr(callee)?;
                for a in args {
                    self.compile_expr(a)?;
                }
                if args.len() > 255 {
                    return Err(self.err(tok.line, format!("参数过多（{} > 255）", args.len())));
                }
                self.code.emit_u8(Opcode::Call, args.len() as u8);
            }
            Expr::FuncLit { func, tok } => {
                self.set_line(tok.line);
                let f = self.compile_func_lit(func)?;
                let idx = self.code.add_const(Value::Func(Arc::new(f)));
                self.code.emit_u16(Opcode::Closure, idx as u16);
            }
            Expr::Assign { target, value, tok } => {
                self.set_line(tok.line);
                // 编译 value，留栈（赋值表达式求值为被赋的值）
                self.compile_expr(value)?;
                // 复制一份用于赋值（避免消费 value）
                self.code.emit(Opcode::Dup);
                match target {
                    AssignTarget::Name(name) => {
                        self.emit_assign_var(name);
                    }
                    AssignTarget::Index { obj, index } => {
                        // a[i] = v：栈形如 [..., v, v]
                        // 需要变成 [..., v, a, i, v] 然后 IndexSet（v 留栈）
                        // 实际上 IndexSet 设计为：弹出 v、i、a，再压入 v
                        // 当前栈：[v_dup]（v_dup 用于保留结果）
                        // 重新排列：先存 v_dup 到临时，编译 a 和 i，再压 v_dup，最后 IndexSet
                        // 简化：用栈操作
                        // 当前栈：[..., v_dup]（v 已被 Dup 复制）
                        // 步骤：编译 obj（压 a），编译 index（压 i），交换让 v 在顶
                        // 但栈操作复杂，改用：先编译 obj 和 index，再 Dup value，再 IndexSet
                        // 重新设计：编译 value 后不 Dup，先编译 obj 和 index，再 LoadGlobal value 临时变量
                        // 太复杂。简化：把 v 暂存到栈底，编译 obj/index，再调换顺序
                        // 这里用临时方案：把 v 弹出到临时槽，编译 obj+index，再压 v，IndexSet
                        // 由于难以用临时变量（作用域问题），改用以下顺序：
                        //   eval value -> stack: [v]
                        //   eval obj -> stack: [v, a]
                        //   eval index -> stack: [v, a, i]
                        //   rotate3 -> stack: [a, i, v]  (但无 rotate 指令)
                        // 用最简方案：把 value 留在栈底，编译 obj 和 index 在上面，
                        // 然后 IndexSet 时需要 [a, i, v] 顺序——但当前是 [v, a, i]
                        // 改用：先 Dup value，编译 obj 和 index，Pop+IndexSet
                        // 实际上当前栈 [v_dup]，我们需要 [a, i, v] 顺序
                        // 用以下：Dup value 后 [v, v]，编译 obj [v, v, a]，编译 index [v, v, a, i]
                        // 然后用 IndexSet 时弹出 v, a, i 留下 v
                        // 但 IndexSet 实现的栈顺序需要明确
                        // 设计 IndexSet：弹出 v, i, a，执行 a[i] = v，压入 v
                        // 当前栈 [v, v, a, i]：弹 i [v, v, a]，弹 a [v, v]，弹 v [v]
                        // 执行 a[i] = v，压 v [v]
                        // 最终栈 [v]——正确！
                        // 所以这里需要 Dup value（已 Dup 过一次，再 Dup 一次）
                        // 当前栈：[v_dup]，再编译 obj [v_dup, a]，编译 index [v_dup, a, i]
                        // 但 IndexSet 需要 v 在顶，所以需要换序
                        // 简化：重新设计，不在此处 Dup
                        // 重新写：
                        // 实际上当前代码已经 Dup 过了：栈 [v, v_dup]
                        // 调整策略：把 v_dup 弹出用于 IndexSet，留 v 作为结果
                        // 步骤：v 已压栈并 Dup -> [v, v2]
                        // 编译 obj -> [v, v2, a]
                        // 编译 index -> [v, v2, a, i]
                        // 需要 IndexSet 读取 [a, i, v2] 顺序——但栈是 [v, v2, a, i]
                        // 改 IndexSet 实现：弹出 i, a, v，执行 a[i] = v，不压入（因为 v 已在栈底）
                        // 这样栈 [v, v2, a, i] -> 弹 i -> [v, v2, a] -> 弹 a -> [v, v2] -> 弹 v -> [v] -> 执行
                        // 结果栈 [v]——正确！
                        // 所以 IndexSet 不应压回 v，因为 v 已在栈中
                        // 但这样 Name 赋值的 Dup 就不一致了
                        // 改用统一策略：所有赋值表达式都用：
                        //   eval value -> [v]
                        //   Dup -> [v, v]
                        //   for Name: emit_assign_var (StoreLocal/StoreFree 会 pop 一个 v) -> [v]
                        //   for Index: eval obj -> [v, v, a], eval index -> [v, v, a, i], IndexSet pops i,a,v -> [v]
                        //   for Member: 同 Index
                        // 所以 IndexSet 设计为：弹 i, 弹 a, 弹 v，执行 a[i] = v，不压回
                        self.compile_expr(obj)?;
                        self.compile_expr(index)?;
                        self.code.emit(Opcode::IndexSet);
                    }
                    AssignTarget::Member { obj, name } => {
                        self.compile_expr(obj)?;
                        let idx = self.code.add_name(name);
                        self.code.emit_u16(Opcode::SetMember, idx as u16);
                    }
                    AssignTarget::Deref { expr } => {
                        // *p = v：当前栈 [v, v_dup]
                        // 弹 v_dup（Assign 统一先 Dup 了）
                        self.code.emit(Opcode::Pop);
                        // 再 Dup v 给 SetDeref 用
                        self.code.emit(Opcode::Dup);
                        // 编译 ref 表达式
                        self.compile_expr(expr)?;
                        // 栈：[v, v_copy, ref]
                        // SetDeref：弹 ref 和 v_copy，写入 ref = v_copy，留 v
                        self.code.emit(Opcode::SetDeref);
                    }
                }
            }
            // Spread 不应独立出现（只在 CallExpr 内处理），报错
            Expr::Spread { tok, .. } => {
                return Err(self.err(tok.line, "... 展开只能在函数调用参数中使用"));
            }
            Expr::Ref { expr, tok } => {
                // &expr：编译内部表达式，发 Ref opcode
                self.set_line(tok.line);
                self.compile_expr(expr)?;
                self.code.emit(Opcode::Ref);
            }
            Expr::Deref { expr, tok } => {
                // *expr：编译内部表达式，发 Deref opcode
                self.set_line(tok.line);
                self.compile_expr(expr)?;
                self.code.emit(Opcode::Deref);
            }
        }
        Ok(())
    }
    /// 创建子编译器，克隆父作用域链用于 free_vars 解析。
    fn compile_func_lit(&mut self, func: &FuncLit) -> Result<Function, CompileError> {
        let mut sub = Compiler::new(&self.file, &func.name);
        // 克隆父作用域链，让子编译器能解析 free_vars
        sub.scopes = self.scopes.clone();
        // 子编译器重置 func_local_count（新函数从 0 开始分配 slot）
        sub.func_local_count = 0;

        // 压入本函数作用域
        sub.push_scope(true);
        // 声明参数（占用 slot 0..n-1）
        for p in &func.params {
            sub.declare_local(p);
        }
        // 编译函数体
        sub.compile_block(&func.body)?;
        sub.code.emit(Opcode::ReturnVoid);
        // pop 函数作用域，设置 num_locals 和 free_sources
        let s = sub.pop_scope();
        sub.code.num_locals = sub.func_local_count;
        sub.code.free_sources = s.free_vars.iter().map(|fv| FreeSource {
            is_local: fv.is_local,
            index: fv.index,
        }).collect();

        Ok(Function::new_closure(
            func.name.clone(),
            func.params.clone(),
            Arc::new(sub.code),
            Vec::new(),
            func.variadic,
        ))
    }
}

/// compile 便捷函数：编译程序。
pub fn compile(prog: &Program) -> Result<Code, CompileError> {
    Compiler::compile_program(prog)
}

/// binary_op_to_opcode 将普通二元运算（算术/位运算）映射为 opcode。
/// NullCoal 返回 None（需短路编译，不能简单发 opcode）。
fn binary_op_to_opcode(op: BinaryOp) -> Option<Opcode> {
    match op {
        BinaryOp::Add => Some(Opcode::Add),
        BinaryOp::Sub => Some(Opcode::Sub),
        BinaryOp::Mul => Some(Opcode::Mul),
        BinaryOp::Div => Some(Opcode::Div),
        BinaryOp::Mod => Some(Opcode::Mod),
        BinaryOp::BitAnd => Some(Opcode::BitAnd),
        BinaryOp::BitOr => Some(Opcode::BitOr),
        BinaryOp::BitXor => Some(Opcode::BitXor),
        BinaryOp::BitShl => Some(Opcode::BitShl),
        BinaryOp::BitShr => Some(Opcode::BitShr),
        BinaryOp::NullCoal => None,
        // 比较/逻辑不参与复合赋值，不应到达此处
        _ => None,
    }
}

/// binary_op_to_flag 将复合赋值的运算映射为 flag 字节（低 4 位）。
/// 编码与 vm::compound_op 对应：0=Add 1=Sub 2=Mul 3=Div 4=Mod 5=?? 6=& 7=| 8=^ 9=<< 10=>>
fn binary_op_to_flag(op: BinaryOp) -> u8 {
    match op {
        BinaryOp::Add => 0,
        BinaryOp::Sub => 1,
        BinaryOp::Mul => 2,
        BinaryOp::Div => 3,
        BinaryOp::Mod => 4,
        BinaryOp::NullCoal => 5,
        BinaryOp::BitAnd => 6,
        BinaryOp::BitOr => 7,
        BinaryOp::BitXor => 8,
        BinaryOp::BitShl => 9,
        BinaryOp::BitShr => 10,
        _ => 0,
    }
}
