//! vm.rs — 字节码虚拟机
//!
//! 设计要点：
//!   - 基于栈的 VM，递归式函数调用（每帧独立 locals 数组）
//!   - try/catch/finally：throw 查找 try 栈；finally 通过 pending 恢复
//!   - defer：注册到当前帧，return 时逆序执行
//!   - run 关键字：启动新线程（共享全局，独立 VM 状态）
//!   - 局部变量用 slot 数组，闭包用 box 共享

use std::sync::{Arc, Mutex};

use crate::compiler::compile;
use crate::function::{Builtin, Function};
use crate::lexer::tokenize;
use crate::opcode::{Code, Opcode};
use crate::parser::parse_program;
use crate::value::{error_value, Value, SfError};

/// flow_kind 控制流类型。
#[derive(Clone, Copy, PartialEq, Eq)]
enum FlowKind {
    /// Normal 正常执行（无控制流跳转）。
    Normal,
    /// Return 返回（值在 FlowResult.value）。
    Return,
    /// Throw 抛出异常（值在 FlowResult.value）。
    Throw,
}

/// flow_result 帧执行结果。
struct FlowResult {
    /// value 控制流携带的值（返回值或异常值）。
    value: Value,
    /// kind 控制流类型。
    kind: FlowKind,
}

/// try_entry try 上下文（编译期 PushTry 创建）。
struct TryEntry {
    /// catch_ip catch 块入口（-1 表示无 catch）。
    catch_ip: i32,
    /// finally_ip finally 块入口（-1 表示无 finally）。
    finally_ip: i32,
}

/// pending_entry 挂起的控制流（用于 finally 恢复）。
///
/// 当 try 块中出现 return/throw 时，若存在 finally，
/// 控制流被挂起，先执行 finally，finally 结束（ExitFinally）时恢复。
struct PendingEntry {
    /// is_throw true=throw，false=return。
    is_throw: bool,
    /// value 挂起的值。
    value: Value,
}

/// defer_entry defer 调用。
struct DeferEntry {
    /// callee 被调用的函数。
    callee: Value,
    /// args 实参列表。
    args: Vec<Value>,
}

/// Frame 调用帧。
struct Frame {
    /// code 本帧执行的字节码。
    code: Arc<Code>,
    /// ip 指令指针。
    ip: usize,
    /// locals 局部变量数组（含参数）。
    locals: Vec<Value>,
    /// boxes 被捕获的 local（box 共享）。惰性分配。
    boxes: std::collections::HashMap<usize, Arc<Mutex<Value>>>,
    /// free_vars 闭包捕获的自由变量（box 共享，跨线程可变）。
    free_vars: Vec<Arc<Mutex<Value>>>,
    /// defers 已注册的 defer 调用（按注册顺序，return 时逆序执行）。
    defers: Vec<DeferEntry>,
    /// try_stack try 上下文栈。
    try_stack: Vec<TryEntry>,
    /// pendings 挂起的控制流（与 try_stack 配对，ExitFinally 时弹出）。
    pendings: Vec<PendingEntry>,
}

impl Frame {
    fn new(code: Arc<Code>, free_vars: Vec<Arc<Mutex<Value>>>) -> Self {
        let locals = vec![Value::Undefined; code.num_locals];
        Frame {
            code,
            ip: 0,
            locals,
            boxes: std::collections::HashMap::new(),
            free_vars,
            defers: Vec::new(),
            try_stack: Vec::new(),
            pendings: Vec::new(),
        }
    }
}

/// VM 虚拟机。
pub struct VM {
    /// stack 操作数栈。
    stack: Vec<Value>,
    /// globals 全局变量（跨线程共享，run 启动的线程与本 VM 共享同一份）。
    globals: Arc<Mutex<std::collections::HashMap<String, Value>>>,
    /// builtins 内置函数表。
    builtins: std::collections::HashMap<String, Builtin>,
    /// out 标准输出（跨线程共享）。
    out: Arc<Mutex<dyn std::io::Write + Send>>,
    /// max_call_depth 最大调用深度。
    max_call_depth: usize,
    /// depth 当前调用深度。
    depth: usize,
    /// import_stack 正在加载的脚本绝对路径栈（环检测，防循环 import）。
    import_stack: Vec<String>,
    /// imported_modules 已成功加载的模块规范化路径（模块缓存，保证幂等）。
    imported_modules: Vec<String>,
}

impl VM {
    /// new 创建虚拟机并注册内置函数。
    ///
    /// 在此统一注册所有内置函数模块（核心/字符串/数学/数组/时间/文件/JSON/并发），
    /// 保证 VM::new 与 Sflang::new 入口的内置函数集完全一致。
    pub fn new() -> Self {
        let mut vm = VM {
            stack: Vec::with_capacity(1024),
            globals: Arc::new(Mutex::new(std::collections::HashMap::new())),
            builtins: std::collections::HashMap::new(),
            out: Arc::new(Mutex::new(std::io::sink())),
            max_call_depth: 1000,
            depth: 0,
            import_stack: Vec::new(),
            imported_modules: Vec::new(),
        };
        crate::builtins::register(&mut vm);
        crate::builtins_str::register(&mut vm);
        crate::builtins_math::register(&mut vm);
        crate::builtins_arr::register(&mut vm);
        crate::builtins_time::register(&mut vm);
        crate::builtins_fs::register(&mut vm);
        crate::builtins_json::register(&mut vm);
        crate::builtins_bytes::register(&mut vm);
        crate::builtins_bigint::register(&mut vm);
        crate::builtins_regex::register(&mut vm);
        crate::builtins_encode::register(&mut vm);
        crate::builtins_hash::register(&mut vm);
        crate::builtins_sys::register(&mut vm);
        crate::concurrency::register(&mut vm);
        crate::builtins_ring::register(&mut vm);
        crate::builtins_csv::register(&mut vm);
        crate::builtins_xlsx::register(&mut vm);
        crate::builtins_docx::register(&mut vm);
        crate::builtins_db::register(&mut vm);
        crate::builtins_aes::register(&mut vm);
        crate::txde::register(&mut vm);
        crate::builtins_gui::register(&mut vm);
        crate::builtins_ssh::register(&mut vm);
        crate::builtins_le::register(&mut vm);
        crate::builtins_email::register(&mut vm);
        crate::builtins_ftp::register(&mut vm);
        // 预定义数学常量全局变量
        vm.set_global("piG", Value::Float(std::f64::consts::PI));
        vm.set_global("eG", Value::Float(std::f64::consts::E));
        vm
    }

    /// set_output 设置标准输出（须 Send 以支持跨线程共享）。
    pub fn set_output(&mut self, w: impl std::io::Write + Send + 'static) {
        self.out = Arc::new(Mutex::new(w));
    }

    /// set_output_handle 直接设置 Arc<Mutex<dyn Write + Send>> 句柄（用于线程间共享）。
    pub fn set_output_handle(&mut self, out: Arc<Mutex<dyn std::io::Write + Send>>) {
        self.out = out;
    }

    /// output_handle 获取输出句柄（供内置函数使用）。
    pub fn output_handle(&self) -> Arc<Mutex<dyn std::io::Write + Send>> {
        self.out.clone()
    }

    /// set_global 设置全局变量（线程安全，加锁）。
    pub fn set_global(&mut self, name: &str, val: Value) {
        self.globals.lock().unwrap().insert(name.to_string(), val);
    }

    /// get_global 读取全局变量（线程安全，加锁）。
    pub fn get_global(&self, name: &str) -> Option<Value> {
        self.globals.lock().unwrap().get(name).cloned()
    }

    /// register_builtin 注册内置函数。
    pub fn register_builtin(&mut self, name: &'static str, func: crate::function::BuiltinFn) {
        self.builtins.insert(name.to_string(), Builtin::new(name, func));
    }

    /// globals_handle 获取全局变量的共享句柄（Arc<Mutex<HashMap>>）。
    ///
    /// 用于 run 启动子线程时共享同一份全局环境（而非克隆快照），
    /// 使主线程与子线程的 var/func 定义互通。
    pub fn globals_handle(&self) -> Arc<Mutex<std::collections::HashMap<String, Value>>> {
        self.globals.clone()
    }

    /// set_globals_handle 设置全局变量的共享句柄（用于子线程接入主线程的全局环境）。
    pub fn set_globals_handle(&mut self, globals: Arc<Mutex<std::collections::HashMap<String, Value>>>) {
        self.globals = globals;
    }

    /// run 执行顶层 Code。
    pub fn run(&mut self, code: Arc<Code>) -> Result<Value, Value> {
        let frame = Frame::new(code, Vec::new());
        let res = self.run_frame(frame);
        match res.kind {
            FlowKind::Throw => Err(res.value),
            _ => Ok(res.value),
        }
    }

    /// call_function_value 调用一个函数值（Func 或 Builtin），返回其结果。
    ///
    /// 供内置函数调用用户函数（如 onceDo 执行一次性回调、sort 自定义比较器等）。
    /// 内部构造临时帧，复用 do_call 机制，错误转为 Result。
    pub fn call_function_value(&mut self, callee: Value, args: Vec<Value>) -> Result<Value, Value> {
        let argc = args.len();
        self.push(callee);
        for a in args {
            self.push(a);
        }
        let mut tmp_frame = Frame::new(Arc::new(Code::new("<call>", "<call>")), Vec::new());
        let res = self.do_call(&mut tmp_frame, argc);
        match res.kind {
            FlowKind::Throw => Err(res.value),
            _ => Ok(self.pop()),
        }
    }

    fn push(&mut self, v: Value) {
        self.stack.push(v);
    }
    fn pop(&mut self) -> Value {
        self.stack.pop().expect("stack underflow")
    }

    fn peek(&self) -> &Value {
        self.stack.last().expect("stack empty")
    }

    /// run_frame 执行一帧。
    fn run_frame(&mut self, mut frame: Frame) -> FlowResult {
        let code = frame.code.clone();
        let insts = code.insts.clone();
        while frame.ip < insts.len() {
            let op_byte = insts[frame.ip];
            let op = match Opcode::from_u8(op_byte) {
                Some(o) => o,
                None => {
                    let ip = frame.ip;
                    return self.handle_throw(frame, error_value(format!("invalid opcode: 0x{:02x} at ip={}", op_byte, ip)));
                }
            };
            match op {
                Opcode::Null => { self.push(Value::Undefined); frame.ip += 1; }
                Opcode::Const => {
                    let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    self.push(code.constants[idx].clone());
                }
                Opcode::Pop => { self.pop(); frame.ip += 1; }
                Opcode::Dup => { let v = self.peek().clone(); self.push(v); frame.ip += 1; }
                Opcode::LoadName => {
                    let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    let name = &code.names[idx];
                    // 名字解析：globals → builtins → undefined（宽容策略，对齐 Charlang）。
                    // 读取未定义变量不再抛错，而是返回 undefined；AI/用户可用
                    // explainUndef(name) 主动诊断为何得到 undefined。
                    let resolved: Value = {
                        let globals = self.globals.lock().unwrap();
                        if let Some(v) = globals.get(name) {
                            v.clone()
                        } else if let Some(b) = self.builtins.get(name) {
                            Value::Builtin(b.clone())
                        } else {
                            // 未定义：返回 undefined（不抛错）
                            Value::Undefined
                        }
                    };
                    self.push(resolved);
                }
                Opcode::StoreName => {
                    let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    let name = code.names[idx].clone();
                    let v = self.pop();
                    self.globals.lock().unwrap().insert(name, v);
                }
                Opcode::AssignName => {
                    let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    let name = code.names[idx].clone();
                    let v = self.pop();
                    // 简化：直接写全局（无论是否存在）
                    self.globals.lock().unwrap().insert(name, v);
                }
                Opcode::LoadGlobal => {
                    let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    let name = &code.names[idx];
                    // 同 LoadName：未定义的全局返回 undefined（宽容策略）。
                    let resolved: Value = {
                        let globals = self.globals.lock().unwrap();
                        if let Some(v) = globals.get(name) {
                            v.clone()
                        } else if let Some(b) = self.builtins.get(name) {
                            Value::Builtin(b.clone())
                        } else {
                            Value::Undefined
                        }
                    };
                    self.push(resolved);
                }
                Opcode::StoreGlobal => {
                    let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    let name = code.names[idx].clone();
                    let v = self.pop();
                    self.globals.lock().unwrap().insert(name, v);
                }
                Opcode::LoadLocal => {
                    let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    if let Some(b) = frame.boxes.get(&idx) {
                        self.push(b.lock().unwrap().clone());
                    } else {
                        self.push(frame.locals[idx].clone());
                    }
                }
                Opcode::StoreLocal => {
                    let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    let v = self.pop();
                    if let Some(b) = frame.boxes.get(&idx) {
                        *b.lock().unwrap() = v;
                    } else {
                        frame.locals[idx] = v;
                    }
                }
                Opcode::LoadFree => {
                    let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    self.push(frame.free_vars[idx].lock().unwrap().clone());
                }
                Opcode::StoreFree => {
                    let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    *frame.free_vars[idx].lock().unwrap() = self.pop();
                }
                Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div | Opcode::Mod
                | Opcode::BitAnd | Opcode::BitOr | Opcode::BitXor | Opcode::BitShl | Opcode::BitShr => {
                    let b = self.pop();
                    let a = self.pop();
                    match arith_op(op, a.clone(), b.clone()) {
                        Ok(r) => self.push(r),
                        Err(e) => {
                            let line = frame.code.get_line(frame.ip);
                            let detail = format!("{} (行 {}: {} {:?} {} [{}] 和 {} [{}])",
                                e, line, "运算", op, a.type_name(), a.inspect(), b.type_name(), b.inspect());
                            return self.handle_throw(frame, error_value(detail));
                        }
                    }
                    frame.ip += 1;
                }
                Opcode::Neg => {
                    let a = self.pop();
                    match a {
                        Value::Int(i) => self.push(Value::Int(-i)),
                        Value::Float(f) => self.push(Value::Float(-f)),
                        _ => {
                            return self.handle_throw(frame, error_value(format!("cannot negate {}", a.type_name())));
                        }
                    }
                    frame.ip += 1;
                }
                Opcode::BitNot => {
                    // 按位取反 ~（整数或字节）
                    let a = self.pop();
                    match a {
                        Value::Int(i) => self.push(Value::Int(!i)),
                        Value::Byte(b) => self.push(Value::Byte(!b)),
                        _ => {
                            return self.handle_throw(frame, error_value(format!(
                                "cannot bitwise-not {} (可能原因：~ 仅支持整数/字节)", a.type_name(),
                            )));
                        }
                    }
                    frame.ip += 1;
                }
                Opcode::Eq => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(Value::Bool(a.equals(&b)));
                    frame.ip += 1;
                }
                Opcode::Neq => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(Value::Bool(!a.equals(&b)));
                    frame.ip += 1;
                }
                Opcode::LT | Opcode::LE | Opcode::GT | Opcode::GE => {
                    let b = self.pop();
                    let a = self.pop();
                    match cmp_op(op, a, b) {
                        Ok(r) => self.push(r),
                        Err(e) => {
                            return self.handle_throw(frame, error_value(e));
                        }
                    }
                    frame.ip += 1;
                }
                Opcode::Not => {
                    let a = self.pop();
                    self.push(Value::Bool(!a.is_truthy()));
                    frame.ip += 1;
                }
                Opcode::Jump => {
                    let target = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip = target;
                }
                Opcode::JumpIfFalse => {
                    let cond = self.pop();
                    let target = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    if !cond.is_truthy() {
                        frame.ip = target;
                    }
                }
                Opcode::JumpIfTrue => {
                    let cond = self.pop();
                    let target = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    if cond.is_truthy() {
                        frame.ip = target;
                    }
                }
                Opcode::JumpIfNotUndefined => {
                    // 弹出栈顶，仅当该值不是 undefined 时跳转（用于 ?? 短路）
                    let v = self.pop();
                    let target = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    if !matches!(v, Value::Undefined) {
                        frame.ip = target;
                    }
                }
                Opcode::CompoundIndex => {
                    // a[i] op= v：栈 [v, obj, idx] → [new]，地址只求值一次
                    let flag = insts[frame.ip + 1];
                    frame.ip += 2;
                    let idx = self.pop();
                    let obj = self.pop();
                    let v = self.pop();
                    match self.compound_index(&obj, &idx, v, flag) {
                        Ok(r) => self.push(r),
                        Err(e) => return self.handle_throw(frame, error_value(e)),
                    }
                }
                Opcode::CompoundMember => {
                    // obj.k op= v：栈 [v, obj] → [new]
                    let name_idx = insts[frame.ip + 1] as usize;
                    let flag = insts[frame.ip + 2];
                    frame.ip += 3;
                    let name = code.names[name_idx].clone();
                    let obj = self.pop();
                    let v = self.pop();
                    match self.compound_member(&obj, &name, v, flag) {
                        Ok(r) => self.push(r),
                        Err(e) => return self.handle_throw(frame, error_value(e)),
                    }
                }
                Opcode::IncDecIndex => {
                    // a[i]++ / ++a[i]：栈 [obj, idx] → [result]
                    let flag = insts[frame.ip + 1];
                    frame.ip += 2;
                    let idx = self.pop();
                    let obj = self.pop();
                    // IncDec 复用 CompoundIndex 逻辑：v=1，op 为 Add(Inc)/Sub(Dec)
                    let op_flag = if flag & 0x80 != 0 { 0x80 } else { 0 }; // 保留后缀位
                    let base = if flag & 0x01 == 0 { 0 } else { 1 }; // 0=Add(Inc), 1=Sub(Dec)
                    let cf = op_flag | base;
                    match self.compound_index(&obj, &idx, Value::Int(1), cf) {
                        Ok(new) => {
                            // 前缀返回新值，后缀返回旧值（new-1 或 new+1 反推）
                            if flag & 0x80 != 0 {
                                // 后缀：还原旧值
                                let old = if base == 0 { new.clone() } else { new.clone() };
                                let old = match old {
                                    Value::Int(i) => Value::Int(if base == 0 { i - 1 } else { i + 1 }),
                                    other => other, // 非 int（理论上不会，因 ++ 要求数值）
                                };
                                self.push(old);
                            } else {
                                self.push(new);
                            }
                        }
                        Err(e) => return self.handle_throw(frame, error_value(e)),
                    }
                }
                Opcode::IncDecMember => {
                    // obj.k++ / ++obj.k：栈 [obj] → [result]
                    let name_idx = insts[frame.ip + 1] as usize;
                    let flag = insts[frame.ip + 2];
                    frame.ip += 3;
                    let name = code.names[name_idx].clone();
                    let obj = self.pop();
                    let op_flag = if flag & 0x80 != 0 { 0x80 } else { 0 };
                    let base = if flag & 0x01 == 0 { 0 } else { 1 };
                    let cf = op_flag | base;
                    match self.compound_member(&obj, &name, Value::Int(1), cf) {
                        Ok(new) => {
                            if flag & 0x80 != 0 {
                                let old = match new {
                                    Value::Int(i) => Value::Int(if base == 0 { i - 1 } else { i + 1 }),
                                    other => other,
                                };
                                self.push(old);
                            } else {
                                self.push(new);
                            }
                        }
                        Err(e) => return self.handle_throw(frame, error_value(e)),
                    }
                }
                Opcode::Slice => {
                    // 切片 a[low:high]：栈 [obj, low, high] → [result]
                    // low/high 缺省为 undefined（表示到边界）
                    frame.ip += 1;
                    let high = self.pop();
                    let low = self.pop();
                    let obj = self.pop();
                    let lo: Option<i64> = match low {
                        Value::Undefined => None,
                        Value::Int(i) => Some(i),
                        v => return self.handle_throw(frame, error_value(format!(
                            "切片下界需为 int 或缺省，得到 {} (可能原因：语法错误)", v.type_name(),
                        ))),
                    };
                    let hi: Option<i64> = match high {
                        Value::Undefined => None,
                        Value::Int(i) => Some(i),
                        v => return self.handle_throw(frame, error_value(format!(
                            "切片上界需为 int 或缺省，得到 {} (可能原因：语法错误)", v.type_name(),
                        ))),
                    };
                    match slice_value(&obj, lo, hi) {
                        Ok(v) => self.push(v),
                        Err(e) => return self.handle_throw(frame, error_value(e)),
                    }
                }
                Opcode::MethodCall => {
                    // 方法调用 obj.name(args)，自动注入 obj 作为隐式 self（首参）
                    // 操作数：name_idx, argc。栈：[obj, arg1, ..., argN]
                    let name_idx = insts[frame.ip + 1] as usize;
                    let argc = insts[frame.ip + 2] as usize;
                    frame.ip += 3;
                    let name = code.names[name_idx].clone();
                    // 弹出 N 个参数 + obj（参数在上，obj 在底）
                    let mut args = Vec::with_capacity(argc);
                    for _ in 0..argc {
                        args.push(self.pop());
                    }
                    args.reverse(); // 恢复 arg1..argN 顺序
                    let obj = self.pop();
                    // 从 obj 读取方法（沿原型链）
                    let method = match member_get(&obj, &name) {
                        Ok(v) => v,
                        Err(e) => return self.handle_throw(frame, error_value(e)),
                    };
                    // 重排栈为 do_call 期望的 [callee=method, self=obj, arg1, ..., argN]
                    self.push(method);
                    self.push(obj); // 隐式 self
                    for a in args {
                        self.push(a);
                    }
                    // 调用：argc = N + 1（含隐式 self）
                    let res = self.do_call(&mut frame, argc + 1);
                    if res.kind != FlowKind::Normal {
                        return self.handle_throw(frame, res.value);
                    }
                }
                Opcode::SpreadCall => {
                    // 带展开的调用：u8 argc, u8 spread_mask
                    // 栈：[callee, arg0, arg1, ...]（标记为 spread 的 arg 是 array）
                    let argc = insts[frame.ip + 1] as usize;
                    let spread_mask = insts[frame.ip + 2];
                    frame.ip += 3;
                    // 弹出所有参数，展开标记为 spread 的数组
                    let mut all_args: Vec<Value> = Vec::new();
                    // 从后往前弹（栈顶是最后一个参数）
                    for i in (0..argc).rev() {
                        let v = self.pop();
                        if spread_mask & (1 << i) != 0 {
                            // 展开数组：插入到 all_args 前面（保持顺序）
                            match &v {
                                Value::Array(a) => {
                                    let elements = a.lock().unwrap().clone();
                                    for e in elements.into_iter().rev() {
                                        all_args.insert(0, e);
                                    }
                                }
                                _ => {
                                    // 非数组无法展开，报错
                                    return self.handle_throw(frame, error_value(format!(
                                        "无法展开非数组类型 {} (可能原因：... 只能用于数组)", v.type_name(),
                                    )));
                                }
                            }
                        } else {
                            all_args.insert(0, v);
                        }
                    }
                    let callee = self.pop();
                    // 重新压栈：callee + 展开后的参数
                    self.push(callee);
                    for a in &all_args {
                        self.push(a.clone());
                    }
                    let total_argc = all_args.len();
                    let res = self.do_call(&mut frame, total_argc);
                    if res.kind != FlowKind::Normal {
                        return self.handle_throw(frame, res.value);
                    }
                }
                Opcode::Call => {
                    let argc = insts[frame.ip + 1] as usize;
                    frame.ip += 2;
                    let res = self.do_call(&mut frame, argc);
                    if res.kind != FlowKind::Normal {
                        // Throw 需要在当前 frame 的 try_stack 中查找 catch/finally
                        return self.handle_throw(frame, res.value);
                    }
                }
                Opcode::Return => {
                    let v = self.pop();
                    return self.finish_return(frame, v);
                }
                Opcode::ReturnVoid => {
                    return self.finish_return(frame, Value::Undefined);
                }
                Opcode::Closure => {
                    let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    let tmpl = match &code.constants[idx] {
                        Value::Func(f) => f.clone(),
                        _ => {
                            return FlowResult {
                                value: error_value("closure: constant is not a function"),
                                kind: FlowKind::Throw,
                            };
                        }
                    };
                    // 提取 free_vars
                    let mut free_vars = Vec::with_capacity(tmpl.body.free_sources.len());
                    for src in &tmpl.body.free_sources {
                        if src.is_local {
                            if !frame.boxes.contains_key(&src.index) {
                                let b = Arc::new(Mutex::new(frame.locals[src.index].clone()));
                                frame.boxes.insert(src.index, b);
                            }
                            free_vars.push(frame.boxes.get(&src.index).unwrap().clone());
                        } else {
                            free_vars.push(frame.free_vars[src.index].clone());
                        }
                    }
                    let func = Function::new_closure(
                        tmpl.name.clone(),
                        tmpl.params.clone(),
                        tmpl.body.clone(),
                        free_vars,
                        tmpl.variadic,
                    );
                    self.push(Value::Func(Arc::new(func)));
                }
                Opcode::BuildArray => {
                    let n = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    let stack_len = self.stack.len();
                    let elems: Vec<Value> = self.stack[stack_len - n..].to_vec();
                    self.stack.truncate(stack_len - n);
                    self.push(Value::Array(Arc::new(Mutex::new(elems))));
                }
                Opcode::BuildMap => {
                    let n = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    let mut map = crate::object_map::Map::new();
                    for _ in 0..n {
                        let v = self.pop();
                        let k = self.pop();
                        match k {
                            Value::Str(s) => map.set((*s).to_string(), v),
                            _ => {
                                return self.handle_throw(frame, error_value(format!("map key must be string, got {}", k.type_name())));
                            }
                        }
                    }
                    self.push(Value::Object(Arc::new(Mutex::new(map))));
                }
                Opcode::BuildOrdMap => {
                    let n = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    // 栈顶为最后一对，弹出后逆序存放，再反转保持插入顺序
                    let mut temp: Vec<(String, Value)> = Vec::with_capacity(n);
                    for _ in 0..n {
                        let v = self.pop();
                        let k = self.pop();
                        match k {
                            Value::Str(s) => temp.push(((*s).to_string(), v)),
                            _ => {
                                return self.handle_throw(frame, error_value(format!("map key must be string, got {}", k.type_name())));
                            }
                        }
                    }
                    temp.reverse();  // 恢复插入顺序
                    let mut map = crate::ord_map::OrdMap::new();
                    for (k, v) in temp {
                        map.set(k, v);
                    }
                    self.push(Value::Map(Arc::new(Mutex::new(map))));
                }
                Opcode::IndexGet => {
                    frame.ip += 1;
                    let idx = self.pop();
                    let obj = self.pop();
                    match index_get(&obj, &idx) {
                        Ok(v) => self.push(v),
                        Err(e) => return self.handle_throw(frame, error_value(e)),
                    }
                }
                Opcode::IndexSet => {
                    frame.ip += 1;
                    // 栈形如：[..., v, a, i]（由 compiler 的 Assign Index 路径产生）
                    // 但实际上 IndexSet 用于语句 a[i] = v（非赋值表达式）
                    // 编译器当前未发射此情况——Assign Index 用的是 IndexSet 但栈形如 [v, v, a, i]
                    // 此处统一处理：弹 i, a, v（v 在底）
                    let i = self.pop();
                    let a = self.pop();
                    let v = self.pop();
                    match index_set(&a, &i, v) {
                        Ok(_) => {}
                        Err(e) => return self.handle_throw(frame, error_value(e)),
                    }
                }
                Opcode::GetMember => {
                    let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    let name = code.names[idx].clone();
                    let obj = self.pop();
                    match member_get(&obj, &name) {
                        Ok(v) => self.push(v),
                        Err(e) => return self.handle_throw(frame, error_value(e)),
                    }
                }
                Opcode::SetMember => {
                    let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    let name = code.names[idx].clone();
                    // 栈：[..., v, a]（v 在下，a 在上）
                    let a = self.pop();
                    let v = self.pop();
                    match member_set(&a, &name, v) {
                        Ok(_) => {}
                        Err(e) => return self.handle_throw(frame, error_value(e)),
                    }
                }
                Opcode::PushTry => {
                    let catch_ip = Code::read_u16(&insts, frame.ip + 1) as i32;
                    let finally_ip = Code::read_u16(&insts, frame.ip + 3) as i32;
                    frame.ip += 5;
                    frame.try_stack.push(TryEntry { catch_ip, finally_ip });
                }
                Opcode::PopTry => {
                    frame.ip += 1;
                    frame.try_stack.pop();
                }
                Opcode::Throw => {
                    frame.ip += 1;
                    let v = self.pop();
                    return self.handle_throw(frame, v);
                }
                Opcode::ExitFinally => {
                    frame.ip += 1;
                    // finally 块结束，恢复挂起的控制流
                    if let Some(p) = frame.pendings.pop() {
                        if p.is_throw {
                            return self.handle_throw(frame, p.value);
                        } else {
                            return self.finish_return(frame, p.value);
                        }
                    }
                    // 无挂起：正常继续
                }
                Opcode::Defer => {
                    let argc = insts[frame.ip + 1] as usize;
                    frame.ip += 2;
                    // 栈：[callee, arg0, arg1, ...]
                    let stack_len = self.stack.len();
                    let callee = self.stack[stack_len - argc - 1].clone();
                    let args: Vec<Value> = self.stack[stack_len - argc..].to_vec();
                    self.stack.truncate(stack_len - argc - 1);
                    frame.defers.push(DeferEntry { callee, args });
                }
                Opcode::Run => {
                    let argc = insts[frame.ip + 1] as usize;
                    frame.ip += 2;
                    // 启动新线程执行调用
                    let stack_len = self.stack.len();
                    let callee = self.stack[stack_len - argc - 1].clone();
                    let args: Vec<Value> = self.stack[stack_len - argc..].to_vec();
                    self.stack.truncate(stack_len - argc - 1);
                    self.spawn_thread(callee, args);
                }
                Opcode::Import => {
                    let idx = Code::read_u16(&insts, frame.ip + 1) as usize;
                    frame.ip += 3;
                    let path = code.names[idx].clone();
                    let cur_file = code.file.clone();
                    match self.do_import(&path, &cur_file) {
                        Ok(()) => {
                            self.push(Value::Undefined);
                        }
                        Err(err_val) => {
                            return self.handle_throw(frame, err_val);
                        }
                    }
                }
                Opcode::Ref => {
                    // &expr：创建引用包装
                    // 对基本类型（Int/Float/Bool/String/Byte）：创建 Mutex<Value> 拷贝
                    // 对引用类型（Array/Object/Map）：已经是 Arc<Mutex>，直接包装 Value
                    // 无论哪种，*p = v 都能修改引用内的值
                    frame.ip += 1;
                    let v = self.pop();
                    self.push(Value::Native(std::sync::Arc::new(std::sync::Arc::new(std::sync::Mutex::new(v)))));
                }
                Opcode::Deref => {
                    // *expr：弹出引用包装，读取内部值
                    frame.ip += 1;
                    let v = self.pop();
                    match deref_value(&v) {
                        Ok(inner) => self.push(inner),
                        Err(e) => return self.handle_throw(frame, error_value(e)),
                    }
                }
                Opcode::SetDeref => {
                    // *p = v：栈 [v, ref]，弹 ref 和 v，写入
                    frame.ip += 1;
                    let ref_val = self.pop();
                    let new_val = self.pop();
                    match set_deref_value(&ref_val, new_val) {
                        Ok(()) => {
                            // 保留 v 在栈（赋值表达式返回被赋值）
                            // 但 v 已经被弹出了……需要重新压
                            // 实际上编译器在 SetDeref 前留了一份 v 在栈底
                            // 栈原来是 [v, v_copy, ref] → 弹 ref + v_copy → [v]
                            // 所以此处不需要额外压
                        }
                        Err(e) => return self.handle_throw(frame, error_value(e)),
                    }
                }
            }
        }
        // 函数自然结束（无 return）：返回 undefined
        self.finish_return(frame, Value::Undefined)
    }

    /// do_call 执行函数调用。
    fn do_call(&mut self, _caller_frame: &mut Frame, argc: usize) -> FlowResult {
        let stack_len = self.stack.len();
        let callee = self.stack[stack_len - argc - 1].clone();
        let args: Vec<Value> = self.stack[stack_len - argc..].to_vec();
        self.stack.truncate(stack_len - argc - 1);

        match &callee {
            Value::Builtin(b) => {
                match (b.func)(self, &args) {
                    Ok(v) => {
                        self.push(v);
                        FlowResult { value: Value::Undefined, kind: FlowKind::Normal }
                    }
                    Err(e) => FlowResult { value: e, kind: FlowKind::Throw }
                }
            }
            Value::Func(f) => {
                if self.depth >= self.max_call_depth {
                    return FlowResult {
                        value: error_value(format!("max call depth exceeded ({}); 可能原因：递归过深", self.max_call_depth)),
                        kind: FlowKind::Throw,
                    };
                }
                self.depth += 1;
                let mut new_frame = Frame::new(f.body.clone(), f.free_vars.clone());
                self.bind_params(f, &args, &mut new_frame);
                let res = self.run_frame(new_frame);
                self.depth -= 1;
                match res.kind {
                    FlowKind::Return => {
                        self.push(res.value);
                        FlowResult { value: Value::Undefined, kind: FlowKind::Normal }
                    }
                    FlowKind::Throw => res,
                    FlowKind::Normal => {
                        // 函数自然结束：push undefined 作为返回值
                        self.push(Value::Undefined);
                        FlowResult { value: Value::Undefined, kind: FlowKind::Normal }
                    }
                }
            }
            _ => {
                FlowResult {
                    value: error_value(format!("not callable: {} (可能原因：调用了非函数值；请检查变量是否为函数)", callee.type_name())),
                    kind: FlowKind::Throw,
                }
            }
        }
    }

    /// bind_params 绑定形参与实参。
    fn bind_params(&self, fn_def: &Function, args: &[Value], frame: &mut Frame) {
        let n = fn_def.params.len();
        if fn_def.variadic {
            for i in 0..n.saturating_sub(1) {
                frame.locals[i] = args.get(i).cloned().unwrap_or(Value::Undefined);
            }
            if n > 0 {
                let rest: Vec<Value> = if args.len() >= n - 1 {
                    args[n - 1..].to_vec()
                } else {
                    Vec::new()
                };
                frame.locals[n - 1] = Value::Array(Arc::new(Mutex::new(rest)));
            }
        } else {
            for i in 0..n {
                frame.locals[i] = args.get(i).cloned().unwrap_or(Value::Undefined);
            }
        }
    }

    /// handle_throw 处理 throw：查找 try 栈，决定跳 catch/finally 或穿透。
    fn handle_throw(&mut self, mut frame: Frame, val: Value) -> FlowResult {
        // 先增强错误信息（追加行号）
        let val = self.enhance_error_with_line(&frame, val);

        // 查找最近的有 catch 或 finally 的 try
        while let Some(te) = frame.try_stack.pop() {
            if te.catch_ip >= 0 && te.catch_ip < te.finally_ip {
                frame.ip = te.catch_ip as usize;
                self.push(val);
                return self.run_frame(frame);
            } else if te.finally_ip >= 0 {
                frame.pendings.push(PendingEntry { is_throw: true, value: val });
                frame.ip = te.finally_ip as usize;
                return self.run_frame(frame);
            }
        }
        // 无 try 处理：穿透返回
        FlowResult { value: val, kind: FlowKind::Throw }
    }

    /// enhance_error_with_line 为未捕获的错误值追加行号信息。
    ///
    /// 只处理 Error 类型，跳过用户主动 throw 的非 Error 值。
    /// 如果错误消息已包含 "行 "（行号标记），不重复追加。
    fn enhance_error_with_line(&self, frame: &Frame, val: Value) -> Value {
        match &val {
            Value::Error(e) => {
                if e.message.contains(" (行 ") {
                    return val;
                }
                // 当前 ip 的行号可能是 0（未设置 set_line），往前找最近的非零行号
                let mut line = 0u32;
                let ip = frame.ip;
                if ip > 0 && ip <= frame.code.lines.len() {
                    // 向前搜索最近的有行号的指令
                    for i in (0..ip.min(frame.code.lines.len())).rev() {
                        if frame.code.lines[i] > 0 {
                            line = frame.code.lines[i];
                            break;
                        }
                    }
                }
                if line > 0 {
                    Value::Error(Arc::new(SfError::new(format!(
                        "{} (行 {})", e.message, line
                    ))))
                } else {
                    val
                }
            }
            _ => val,
        }
    }

    /// finish_return 处理 return（含 defer/finally）。
    fn finish_return(&mut self, mut frame: Frame, val: Value) -> FlowResult {
        // 1. 执行 defers（逆序）
        let defers = std::mem::take(&mut frame.defers);
        for d in defers.into_iter().rev() {
            // 调用 defer 函数（忽略返回值）
            self.push(d.callee);
            for a in &d.args {
                self.push(a.clone());
            }
            let res = self.do_call(&mut frame, d.args.len());
            if res.kind == FlowKind::Throw {
                // defer 抛出异常：覆盖原 return 值
                return self.handle_throw(frame, res.value);
            }
            // do_call 会把返回值压栈，defer 忽略返回值，需要弹出
            if res.kind == FlowKind::Normal {
                self.pop();
            }
        }
        // 2. 处理 finally：若当前帧有未消费的 try 栈
        // 简化：return 时检查 try_stack，若有 finally 入口则挂起
        while let Some(te) = frame.try_stack.pop() {
            if te.finally_ip >= 0 {
                frame.pendings.push(PendingEntry { is_throw: false, value: val.clone() });
                frame.ip = te.finally_ip as usize;
                return self.run_frame(frame);
            }
            // 无 finally 的 try：跳过
        }
        // 3. 正常返回
        FlowResult { value: val, kind: FlowKind::Return }
    }

    /// do_import 加载并执行一个 Sflang 脚本，将其顶层 var/func 合并到当前全局环境。
    ///
    /// 实现要点：
    ///   - 路径解析：相对路径基于当前脚本（cur_file）所在目录；绝对路径直接使用
    ///   - 环检测：用规范化绝对路径的栈防止循环 import（A import B import A）
    ///   - 全局合并：目标脚本的顶层声明写入同一 self.globals，调用方即可引用
    ///   - 模块缓存：已加载的模块不重复执行（同一路径只生效一次），避免副作用重复
    ///
    /// 参数：
    ///   - path: import 语句中的路径字面量
    ///   - cur_file: 当前正在执行的脚本文件名（用于解析相对路径基准）
    pub fn do_import(&mut self, path: &str, cur_file: &str) -> Result<(), Value> {
        // 1. 解析路径：相对路径基于当前脚本目录
        let resolved = resolve_import_path(path, cur_file);

        // 2. 规范化绝对路径（用于环检测与模块缓存）
        let canonical = std::fs::canonicalize(&resolved)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| resolved.clone());

        // 3. 模块缓存：已加载过则跳过（幂等，避免重复执行副作用）
        if self.imported_modules.contains(&canonical) {
            return Ok(());
        }

        // 4. 环检测：路径已在加载栈中 → 循环 import
        if self.import_stack.iter().any(|p| p == &canonical) {
            return Err(error_value(format!(
                "import 循环依赖：'{}' (可能原因：A import B，B 又 import A；请重构消除环)",
                canonical,
            )));
        }

        // 5. 读取文件
        let src = std::fs::read_to_string(&resolved).map_err(|e| {
            let hint = match e.kind() {
                std::io::ErrorKind::NotFound => "模块文件不存在（检查路径或当前工作目录）",
                std::io::ErrorKind::PermissionDenied => "权限不足",
                _ => "路径非法或被占用",
            };
            error_value(format!(
                "import 失败：无法读取 '{}' - {} (可能原因：{})",
                path, e, hint,
            ))
        })?;

        // 6. 编译：lex → parse → compile
        let tokens = tokenize(&src, &canonical).map_err(|e| {
            error_value(format!("import '{}' 词法错误: {}", path, e))
        })?;
        let prog = parse_program(tokens, &canonical).map_err(|e| {
            error_value(format!("import '{}' 语法错误: {}", path, e))
        })?;
        let sub_code = compile(&prog).map_err(|e| {
            error_value(format!("import '{}' 编译错误: {}", path, e))
        })?;

        // 7. 执行：压入加载栈，递归执行子脚本（共享 self.globals）
        self.import_stack.push(canonical.clone());
        let result = self.run(Arc::new(sub_code));
        self.import_stack.pop();
        // 仅在执行成功时标记为已加载（幂等）。
        // 失败时不标记：这样错误恢复/重试流程可重新加载（修正后的脚本会重新执行）。
        match &result {
            Ok(_) => {
                self.imported_modules.push(canonical);
                Ok(())
            }
            Err(e) => Err(e.clone()),
        }
    }

    /// compound_index 执行 a[i] op= v，返回新值（op 由 flag 编码）。
    /// flag 低 4 位为运算类型索引，与 compound_op 解码对应。
    fn compound_index(&self, obj: &Value, idx: &Value, v: Value, flag: u8) -> Result<Value, String> {
        // 读取旧值
        let old = index_get(obj, idx)?;
        // ??= 特殊：仅当 old 为 undefined 才赋值（返回新值），否则返回 old（不赋值）
        if flag & 0x0f == 0x05 {
            if matches!(old, Value::Undefined) {
                index_set(obj, idx, v.clone())?;
                return Ok(v);
            }
            return Ok(old);
        }
        let op = compound_op(flag & 0x0f)?;
        let new = arith_op(op, old, v)?;
        index_set(obj, idx, new.clone())?;
        Ok(new)
    }

    /// compound_member 执行 obj.k op= v，返回新值。
    fn compound_member(&self, obj: &Value, name: &str, v: Value, flag: u8) -> Result<Value, String> {
        let old = member_get(obj, name)?;
        if flag & 0x0f == 0x05 {
            // ??=
            if matches!(old, Value::Undefined) {
                member_set(obj, name, v.clone())?;
                return Ok(v);
            }
            return Ok(old);
        }
        let op = compound_op(flag & 0x0f)?;
        let new = arith_op(op, old, v)?;
        member_set(obj, name, new.clone())?;
        Ok(new)
    }

    /// spawn_thread 启动新 OS 线程执行函数调用（阶段三：真并发）。
    ///
    /// 设计：
    ///   - 用 std::thread::spawn 真正多线程执行（Value 现为 Arc/Mutex，Send + Sync 安全）
    ///   - 子线程构造独立 VM（独立操作数栈/帧/调用深度），不与主线程共享栈
    ///   - 共享 self.globals（Arc<Mutex<HashMap>>）与 self.out（Arc<Mutex<dyn Write+Send>>），
    ///     使主线程与子线程的 var/func 定义互通、输出汇聚同一处
    ///   - callee 与 args 所有权转移到子线程（不再被主线程访问）
    ///   - 异常在子线程内吞掉（打印到输出），不影响主线程；如需收集可用 channel
    fn spawn_thread(&self, callee: Value, args: Vec<Value>) {
        let globals = self.globals.clone();
        let out = self.out.clone();
        std::thread::spawn(move || {
            let mut vm = VM::new();
            vm.set_globals_handle(globals);
            vm.set_output_handle(out);
            vm.push(callee);
            for a in &args {
                vm.push(a.clone());
            }
            let mut tmp_frame = Frame::new(Arc::new(Code::new("<run>", "<run>")), Vec::new());
            let res = vm.do_call(&mut tmp_frame, args.len());
            // 子线程内异常：打印提示，不传播（避免 panic）
            if res.kind == FlowKind::Throw {
                if let Value::Error(e) = &res.value {
                    let _ = writeln!(vm.output_handle().lock().unwrap(), "[run 线程异常] {}", e.message);
                }
            }
        });
    }
}

impl Default for VM {
    fn default() -> Self {
        Self::new()
    }
}

/// compound_op 将复合赋值的 flag（低 4 位）映射为对应的算术 opcode。
/// 编码：0=Add 1=Sub 2=Mul 3=Div 4=Mod 5=NullCoal(??=, 调用处特判) 6=BitAnd 7=BitOr 8=BitXor 9=Shl 10=Shr
fn compound_op(flag: u8) -> Result<Opcode, String> {
    match flag {
        0 => Ok(Opcode::Add),
        1 => Ok(Opcode::Sub),
        2 => Ok(Opcode::Mul),
        3 => Ok(Opcode::Div),
        4 => Ok(Opcode::Mod),
        // 5 = ??=，已在 compound_index/member 处特判，不应到达此处
        6 => Ok(Opcode::BitAnd),
        7 => Ok(Opcode::BitOr),
        8 => Ok(Opcode::BitXor),
        9 => Ok(Opcode::BitShl),
        10 => Ok(Opcode::BitShr),
        other => Err(format!("invalid compound op flag: {}", other)),
    }
}

/// arith_op 算术与位运算。
fn arith_op(op: Opcode, a: Value, b: Value) -> Result<Value, String> {
    // 字符串拼接（仅 +）
    if op == Opcode::Add {
        if let (Value::Str(s1), Value::Str(s2)) = (&a, &b) {
            let mut s = String::with_capacity(s1.len() + s2.len());
            s.push_str(s1);
            s.push_str(s2);
            return Ok(Value::Str(Arc::from(s.as_str())));
        }
    }
    // 位运算：仅整数参与（Float 报错，类型不兼容）
    match op {
        Opcode::BitAnd | Opcode::BitOr | Opcode::BitXor | Opcode::BitShl | Opcode::BitShr => {
            return bit_op(op, &a, &b);
        }
        _ => {}
    }
    // ---- byte 运算 ----
    // Byte op Byte → Byte（算术 mod 256 环绕；位运算结果必在 0-255）
    match (&a, &b) {
        (Value::Byte(x), Value::Byte(y)) => {
            let r: u8 = match op {
                Opcode::Add => x.wrapping_add(*y),
                Opcode::Sub => x.wrapping_sub(*y),
                Opcode::Mul => x.wrapping_mul(*y),
                // 除法/取模结果可能不在 byte 范围语义内，提升为 int
                Opcode::Div => {
                    if *y == 0 { return Err("division by zero (除零错误)".into()); }
                    return Ok(Value::Int((*x / *y) as i64));
                }
                Opcode::Mod => {
                    if *y == 0 { return Err("modulo by zero".into()); }
                    return Ok(Value::Int((*x % *y) as i64));
                }
                _ => unreachable!(),
            };
            return Ok(Value::Byte(r));
        }
        // Byte + Int → Int（byte 提升为 int）
        (Value::Byte(x), Value::Int(y)) => {
            return arith_op(op, Value::Int(*x as i64), Value::Int(*y));
        }
        (Value::Int(x), Value::Byte(y)) => {
            return arith_op(op, Value::Int(*x), Value::Int(*y as i64));
        }
        // Byte + Float → Float（byte 提升为 float）
        (Value::Byte(x), Value::Float(y)) => {
            return arith_op(op, Value::Float(*x as f64), Value::Float(*y));
        }
        (Value::Float(x), Value::Byte(y)) => {
            return arith_op(op, Value::Float(*x), Value::Float(*y as f64));
        }
        _ => {}
    }
    // ---- 原有数值运算 ----
    match (&a, &b) {
        (Value::Int(x), Value::Int(y)) => {
            let r = match op {
                Opcode::Add => x.wrapping_add(*y),
                Opcode::Sub => x.wrapping_sub(*y),
                Opcode::Mul => x.wrapping_mul(*y),
                Opcode::Div => {
                    if *y == 0 { return Err("division by zero (除零错误；可能原因：除数为 0)".into()); }
                    x.wrapping_div(*y)
                }
                Opcode::Mod => {
                    if *y == 0 { return Err("modulo by zero".into()); }
                    x.wrapping_rem(*y)
                }
                _ => unreachable!(),
            };
            Ok(Value::Int(r))
        }
        (Value::Float(x), Value::Float(y)) => {
            let r = match op {
                Opcode::Add => x + y,
                Opcode::Sub => x - y,
                Opcode::Mul => x * y,
                Opcode::Div => x / y,
                Opcode::Mod => x % y,
                _ => unreachable!(),
            };
            Ok(Value::Float(r))
        }
        (Value::Int(x), Value::Float(y)) => arith_op(op, Value::Float(*x as f64), Value::Float(*y)),
        (Value::Float(x), Value::Int(y)) => arith_op(op, Value::Float(*x), Value::Float(*y as f64)),
        // ---- BigInt 互通 ----
        // 注意：非交换运算（- / %）必须保持操作数顺序，故拆分为独立 arm
        (Value::BigInt(a), Value::BigInt(b)) => big_arith(op, a, b),
        (Value::Int(x), Value::BigInt(b)) => {
            let a_bi = std::sync::Arc::new(crate::bigint::BigInt::from_i64(*x));
            big_arith(op, &a_bi, b)
        }
        (Value::BigInt(b), Value::Int(x)) => {
            let b_bi = std::sync::Arc::new(crate::bigint::BigInt::from_i64(*x));
            big_arith(op, b, &b_bi)
        }
        // ---- BigFloat 互通 ----
        (Value::BigFloat(a), Value::BigFloat(b)) => bigfloat_arith(op, a, b),
        (Value::BigInt(a), Value::BigFloat(b)) => {
            let a_bf = std::sync::Arc::new(crate::bigfloat::BigFloat::from_bigint((**a).clone()));
            bigfloat_arith(op, &a_bf, b)
        }
        (Value::BigFloat(b), Value::BigInt(a)) => {
            let a_bf = std::sync::Arc::new(crate::bigfloat::BigFloat::from_bigint((**a).clone()));
            bigfloat_arith(op, b, &a_bf)
        }
        (Value::Int(x), Value::BigFloat(b)) => {
            let a_bf = std::sync::Arc::new(crate::bigfloat::BigFloat::from_i64(*x));
            bigfloat_arith(op, &a_bf, b)
        }
        (Value::BigFloat(b), Value::Int(x)) => {
            let b_bf = std::sync::Arc::new(crate::bigfloat::BigFloat::from_i64(*x));
            bigfloat_arith(op, b, &b_bf)
        }
        // BigInt/BigFloat 与 Float 混算：报错（精度语义冲突，需用户显式转换）
        (Value::Float(_), Value::BigInt(_)) | (Value::BigInt(_), Value::Float(_))
        | (Value::Float(_), Value::BigFloat(_)) | (Value::BigFloat(_), Value::Float(_)) => {
            Err(format!("cannot {:?} {} and {} (可能原因：大数(bigInt/bigFloat)不与 float 直接混算，请先转换)", op, a.type_name(), b.type_name()))
        }
        _ => Err(format!("cannot {:?} {} and {} (可能原因：类型不匹配；算术运算要求数值或字符串)", op, a.type_name(), b.type_name())),
    }
}

/// bit_op 位运算（仅整数 i64；Float/其他类型报错）。
fn bit_op(op: Opcode, a: &Value, b: &Value) -> Result<Value, String> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => {
            let r = match op {
                Opcode::BitAnd => x & y,
                Opcode::BitOr => x | y,
                Opcode::BitXor => x ^ y,
                Opcode::BitShl => x.wrapping_shl(*y as u32),
                Opcode::BitShr => x.wrapping_shr(*y as u32),
                _ => unreachable!(),
            };
            Ok(Value::Int(r))
        }
        // Byte op Byte 位运算 → Byte（& | ^ 结果必在 0-255；移位提升为 int）
        (Value::Byte(x), Value::Byte(y)) => {
            match op {
                Opcode::BitAnd => Ok(Value::Byte(x & y)),
                Opcode::BitOr => Ok(Value::Byte(x | y)),
                Opcode::BitXor => Ok(Value::Byte(x ^ y)),
                // 移位可能超出 byte 范围，提升为 int
                Opcode::BitShl => Ok(Value::Int((*x as i64).wrapping_shl(*y as u32))),
                Opcode::BitShr => Ok(Value::Int((*x as i64).wrapping_shr(*y as u32))),
                _ => unreachable!(),
            }
        }
        // Byte op Int / Int op Byte 位运算 → Int（byte 提升）
        (Value::Byte(x), Value::Int(y)) => bit_op(op, &Value::Int(*x as i64), &Value::Int(*y)),
        (Value::Int(x), Value::Byte(y)) => bit_op(op, &Value::Int(*x), &Value::Int(*y as i64)),
        _ => Err(format!(
            "cannot {:?} {} and {} (可能原因：位运算仅支持整数/字节；浮点/其他类型不兼容)",
            op, a.type_name(), b.type_name(),
        )),
    }
}

/// big_arith BigInt 算术（加/减/乘/除/模）。
///
/// 结果若能装回 i64 则降级为 Int（避免小结果仍用 BigInt）；否则保持 BigInt。
fn big_arith(op: Opcode, a: &std::sync::Arc<crate::bigint::BigInt>, b: &std::sync::Arc<crate::bigint::BigInt>) -> Result<Value, String> {
    use crate::bigint::BigInt;
    let result: BigInt = match op {
        Opcode::Add => a.add(b),
        Opcode::Sub => a.sub(b),
        Opcode::Mul => a.mul(b),
        Opcode::Div => {
            let (q, _r) = a.divmod(b)?;
            q
        }
        Opcode::Mod => {
            let (_q, r) = a.divmod(b)?;
            r
        }
        _ => return Err(format!("bigInt 不支持运算 {:?}", op)),
    };
    // 能装回 i64 则降级为 Int（小结果用更高效的 Int 表示）
    match result.to_i64() {
        Some(i) => Ok(Value::Int(i)),
        None => Ok(Value::BigInt(std::sync::Arc::new(result))),
    }
}

/// bigfloat_arith BigFloat 算术。
///
/// 除法默认 20 位小数（可用 bigFloatDiv 指定更高精度）。
fn bigfloat_arith(op: Opcode, a: &std::sync::Arc<crate::bigfloat::BigFloat>, b: &std::sync::Arc<crate::bigfloat::BigFloat>) -> Result<Value, String> {
    use crate::bigfloat::BigFloat;
    let result: BigFloat = match op {
        Opcode::Add => a.add(b),
        Opcode::Sub => a.sub(b),
        Opcode::Mul => a.mul(b),
        Opcode::Div => a.div_default(b)?,
        Opcode::Mod => return Err("bigFloat 不支持取模 % (可能原因：浮点无整数取模语义)".into()),
        _ => return Err(format!("bigFloat 不支持运算 {:?}", op)),
    };
    Ok(Value::BigFloat(std::sync::Arc::new(result)))
}

/// cmp_op 比较运算。
/// cmp_apply 将 Ordering + Opcode 转为布尔比较结果。
fn cmp_apply(op: Opcode, ord: std::cmp::Ordering) -> bool {
    use std::cmp::Ordering::*;
    match op {
        Opcode::LT => ord == Less,
        Opcode::LE => ord != Greater,
        Opcode::GT => ord == Greater,
        Opcode::GE => ord != Less,
        _ => unreachable!(),
    }
}

fn cmp_op(op: Opcode, a: Value, b: Value) -> Result<Value, String> {
    let r = match (&a, &b) {
        (Value::Int(x), Value::Int(y)) => match op {
            Opcode::LT => x < y,
            Opcode::LE => x <= y,
            Opcode::GT => x > y,
            Opcode::GE => x >= y,
            _ => unreachable!(),
        },
        // Byte 比较（Byte-Byte / Byte-Int / Int-Byte，跨类型按值）
        (Value::Byte(x), Value::Byte(y)) => cmp_apply(op, (*x as i64).cmp(&(*y as i64))),
        (Value::Byte(x), Value::Int(y)) => cmp_apply(op, (*x as i64).cmp(y)),
        (Value::Int(x), Value::Byte(y)) => cmp_apply(op, x.cmp(&(*y as i64))),
        (Value::Float(x), Value::Float(y)) => match op {
            Opcode::LT => x < y,
            Opcode::LE => x <= y,
            Opcode::GT => x > y,
            Opcode::GE => x >= y,
            _ => unreachable!(),
        },
        (Value::Int(x), Value::Float(y)) => cmp_apply(op, (*x as f64).partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)),
        (Value::Float(x), Value::Int(y)) => cmp_apply(op, x.partial_cmp(&(*y as f64)).unwrap_or(std::cmp::Ordering::Equal)),
        // ---- BigInt/BigFloat 跨类型比较 ----
        (Value::BigInt(a), Value::BigInt(b)) => cmp_apply(op, a.cmp(b)),
        (Value::Int(x), Value::BigInt(b)) => {
            cmp_apply(op, crate::bigint::BigInt::from_i64(*x).cmp(b))
        }
        (Value::BigInt(b), Value::Int(x)) => {
            cmp_apply(op, b.cmp(&crate::bigint::BigInt::from_i64(*x)))
        }
        (Value::BigFloat(a), Value::BigFloat(b)) => cmp_apply(op, a.cmp(b)),
        (Value::Int(x), Value::BigFloat(b)) => {
            cmp_apply(op, crate::bigfloat::BigFloat::from_i64(*x).cmp(b))
        }
        (Value::BigFloat(b), Value::Int(x)) => {
            // b OP x，需比较 b 与 x（不是 x 与 b）
            cmp_apply(op, b.cmp(&crate::bigfloat::BigFloat::from_i64(*x)))
        }
        (Value::BigInt(a), Value::BigFloat(b)) => {
            cmp_apply(op, crate::bigfloat::BigFloat::from_bigint((**a).clone()).cmp(b))
        }
        (Value::BigFloat(b), Value::BigInt(a)) => {
            // b OP a，需比较 b 与 a（不是 a 与 b）
            cmp_apply(op, b.cmp(&crate::bigfloat::BigFloat::from_bigint((**a).clone())))
        }
        (Value::Str(x), Value::Str(y)) => match op {
            Opcode::LT => x < y,
            Opcode::LE => x <= y,
            Opcode::GT => x > y,
            Opcode::GE => x >= y,
            _ => unreachable!(),
        },
        _ => return Err(format!("cannot compare {} and {} (可能原因：类型不匹配)", a.type_name(), b.type_name())),
    };
    Ok(Value::Bool(r))
}

/// index_get 索引读取 a[i]。
fn index_get(obj: &Value, idx: &Value) -> Result<Value, String> {
    match (obj, idx) {
        (Value::Array(a), Value::Int(i)) => {
            let arr = a.lock().unwrap();
            let n = arr.len() as i64;
            let i = if *i < 0 { *i + n } else { *i };
            if i < 0 || i >= n {
                return Err(format!("array index out of range: {} (len={}); 可能原因：索引越界", i, n));
            }
            Ok(arr[i as usize].clone())
        }
        (Value::Str(s), Value::Int(i)) => {
            // string[i] 返回第 i 个字符的 Unicode 码点（int），按字符索引（不切断多字节）。
            // 与 charFromCode(n) 配对：charFromCode(s[i]) == 原字符。
            let n = s.chars().count() as i64;
            let i = if *i < 0 { *i + n } else { *i };
            if i < 0 || i >= n {
                return Err(format!("string index out of range: {} (len={}); 可能原因：索引越界", i, n));
            }
            let code = s.chars().nth(i as usize).unwrap() as u32 as i64;
            Ok(Value::Int(code))
        }
        (Value::Object(o), Value::Str(k)) => {
            Ok(o.lock().unwrap().get_proto(k).unwrap_or(Value::Undefined))
        }
        (Value::Map(m), Value::Str(k)) => {
            Ok(m.lock().unwrap().get(k).unwrap_or(Value::Undefined))
        }
        (Value::Bytes(b), Value::Int(i)) => {
            let arr = b.as_ref();
            let n = arr.len() as i64;
            let i = if *i < 0 { *i + n } else { *i };
            if i < 0 || i >= n {
                return Err(format!("bytes index out of range: {}", i));
            }
            Ok(Value::Byte(arr[i as usize]))
        }
        (Value::ByteArray(b), Value::Int(i)) => {
            // 可变字节序列读：返回 Byte
            let arr = b.lock().unwrap();
            let n = arr.len() as i64;
            let i = if *i < 0 { *i + n } else { *i };
            if i < 0 || i >= n {
                return Err(format!("byteArray index out of range: {} (len={}); 可能原因：索引越界", i, n));
            }
            Ok(Value::Byte(arr[i as usize]))
        }
        _ => Err(format!("cannot index {} with {} (可能原因：类型不匹配；数组用整数索引，对象用字符串键)", obj.type_name(), idx.type_name())),
    }
}

/// index_set 索引设置 a[i] = v。
fn index_set(obj: &Value, idx: &Value, v: Value) -> Result<(), String> {
    match (obj, idx) {
        (Value::Array(a), Value::Int(i)) => {
            let mut arr = a.lock().unwrap();
            let n = arr.len() as i64;
            let i = if *i < 0 { *i + n } else { *i };
            if i < 0 || i >= n {
                return Err(format!("array index out of range: {} (len={})", i, n));
            }
            arr[i as usize] = v;
            Ok(())
        }
        (Value::Object(o), Value::Str(k)) => {
            o.lock().unwrap().set((*k).to_string(), v);
            Ok(())
        }
        (Value::Map(m), Value::Str(k)) => {
            m.lock().unwrap().set((*k).to_string(), v);
            Ok(())
        }
        (Value::ByteArray(b), Value::Int(i)) => {
            // 可变字节序列写：就地修改。值须为 Int 且 0-255。
            let byte_val = match v {
                Value::Byte(x) => x,
                Value::Int(x) => {
                    if x < 0 || x > 255 {
                        return Err(format!(
                            "byteArray 赋值超出字节范围: {} (需 0-255；可能原因：传入了非字节整数)",
                            x,
                        ));
                    }
                    x as u8
                }
                _ => return Err(format!(
                    "byteArray 赋值需要 byte 或 int 字节值 (0-255)，得到 {} (可能原因：类型不匹配)",
                    v.type_name(),
                )),
            };
            let mut arr = b.lock().unwrap();
            let n = arr.len() as i64;
            let i = if *i < 0 { *i + n } else { *i };
            if i < 0 || i >= n {
                return Err(format!("byteArray index out of range: {} (len={})", i, n));
            }
            arr[i as usize] = byte_val;
            Ok(())
        }
        _ => Err(format!("cannot set index on {} with {} (可能原因：类型不匹配)", obj.type_name(), idx.type_name())),
    }
}

/// slice_value 切片 a[low:high]，按类型分发单位与返回类型。
///
/// - string：按字符切片（不切断多字节字符），返回 string
/// - array：按元素切片，返回 array
/// - bytes：按字节切片，返回 bytes
/// - byteArray：按字节切片，返回 byteArray（类型一致，便于后续就地修改）
///
/// low/high 缺省（None）表示到边界（0 / len）。支持负索引（从尾算）。
/// low >= high 返回空（与 Python/Go 一致）。
fn slice_value(obj: &Value, low: Option<i64>, high: Option<i64>) -> Result<Value, String> {
    /// norm 将 low/high 归一化为 [0, len] 内的 usize，支持负索引与缺省。
    ///
    /// 负索引溢出（如 -100 对长度 3）clamp 到 0（对齐 Python/JS 的宽容语义，
    /// 不报错——脚本语言不应因索引偏大而崩）。
    fn norm(v: Option<i64>, len: i64, which: &str) -> Result<usize, String> {
        match v {
            None => Ok(if which == "low" { 0 } else { len as usize }),
            Some(i) => {
                let i = if i < 0 { i + len } else { i };
                // 负索引溢出 clamp 到 0；上界超过 len 截断到 len（到尾）
                let clamped = if i < 0 { 0 } else if i > len { len } else { i };
                Ok(clamped as usize)
            }
        }
    }
    match obj {
        Value::Str(s) => {
            let chars: Vec<char> = s.chars().collect();
            let n = chars.len() as i64;
            let lo = norm(low, n, "low")?;
            let hi = norm(high, n, "high")?;
            if lo >= hi {
                return Ok(Value::str(""));
            }
            let part: String = chars[lo..hi].iter().collect();
            Ok(Value::str_from(part))
        }
        Value::Array(a) => {
            let guard = a.lock().unwrap();
            let n = guard.len() as i64;
            let lo = norm(low, n, "low")?;
            let hi = norm(high, n, "high")?;
            if lo >= hi {
                return Ok(Value::Array(Arc::new(Mutex::new(Vec::new()))));
            }
            let part = guard[lo..hi].to_vec();
            Ok(Value::Array(Arc::new(Mutex::new(part))))
        }
        Value::Bytes(b) => {
            let n = b.len() as i64;
            let lo = norm(low, n, "low")?;
            let hi = norm(high, n, "high")?;
            if lo >= hi {
                return Ok(Value::Bytes(Arc::new(Vec::new())));
            }
            let part = b[lo..hi].to_vec();
            Ok(Value::Bytes(Arc::new(part)))
        }
        Value::ByteArray(b) => {
            let guard = b.lock().unwrap();
            let n = guard.len() as i64;
            let lo = norm(low, n, "low")?;
            let hi = norm(high, n, "high")?;
            if lo >= hi {
                return Ok(Value::ByteArray(Arc::new(Mutex::new(Vec::new()))));
            }
            let part = guard[lo..hi].to_vec();
            Ok(Value::ByteArray(Arc::new(Mutex::new(part))))
        }
        _ => Err(format!("cannot slice {} (可能原因：仅 string/array/bytes/byteArray 支持切片)", obj.type_name())),
    }
}
/// member_get 成员读取 a.name（沿原型链）。
fn member_get(obj: &Value, name: &str) -> Result<Value, String> {
    match obj {
        Value::Object(o) => Ok(o.lock().unwrap().get_proto(name).unwrap_or(Value::Undefined)),
        Value::Array(a) => {
            // 数组内置成员：len
            match name {
                "len" => Ok(Value::Int(a.lock().unwrap().len() as i64)),
                _ => Err(format!("array has no member '{}' (可能原因：成员名错误；数组支持 .len)", name)),
            }
        }
        Value::Str(s) => {
            match name {
                "len" => Ok(Value::Int(s.chars().count() as i64)),
                _ => Err(format!("string has no member '{}' (可能原因：成员名错误；字符串支持 .len)", name)),
            }
        }
        Value::ByteArray(b) => {
            match name {
                "len" => Ok(Value::Int(b.lock().unwrap().len() as i64)),
                _ => Err(format!("byteArray has no member '{}' (可能原因：成员名错误；byteArray 支持 .len)", name)),
            }
        }
        Value::DateTime(dt) => {
            match name {
                "year" => Ok(Value::Int(dt.year() as i64)),
                "month" => Ok(Value::Int(dt.month() as i64)),
                "day" => Ok(Value::Int(dt.day() as i64)),
                "hour" => Ok(Value::Int(dt.hour() as i64)),
                "minute" => Ok(Value::Int(dt.minute() as i64)),
                "second" => Ok(Value::Int(dt.second() as i64)),
                "millis" => Ok(Value::Int(dt.millis_part() as i64)),
                "weekday" => Ok(Value::Int(dt.weekday() as i64)),
                "tzOffset" => Ok(Value::Int(dt.tz_offset as i64)),
                _ => Err(format!("datetime has no member '{}' (可能原因：成员名错误)", name)),
            }
        }
        Value::Map(m) => {
            match name {
                "len" => Ok(Value::Int(m.lock().unwrap().len() as i64)),
                _ => Err(format!("map has no member '{}' (可能原因：成员名错误；map 支持 .len)", name)),
            }
        }
        _ => Err(format!("cannot get member '{}' from {} (可能原因：类型不支持成员访问)", name, obj.type_name())),
    }
}

/// member_set 成员设置 a.name = v。
fn member_set(obj: &Value, name: &str, v: Value) -> Result<(), String> {
    match obj {
        Value::Object(o) => {
            o.lock().unwrap().set(name.to_string(), v);
            Ok(())
        }
        _ => Err(format!("cannot set member '{}' on {} (可能原因：仅 object 类型支持成员设置)", name, obj.type_name())),
    }
}

/// resolve_import_path 解析 import 路径为文件系统路径。
///
/// 规则：
///   - 绝对路径（如 `/x/y.sf` 或 `D:\x\y.sf`）直接使用
///   - 相对路径基于当前脚本（cur_file）所在目录解析
///   - cur_file 为 "<string>" / "<run>" 等占位符时，回退到当前工作目录
fn resolve_import_path(path: &str, cur_file: &str) -> String {
    use std::path::{Path, PathBuf};
    let p = Path::new(path);
    // 绝对路径直接使用
    if p.is_absolute() {
        return path.to_string();
    }
    // 相对路径：基于当前脚本目录
    let base = Path::new(cur_file).parent();
    match base {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(path).to_string_lossy().into_owned(),
        // cur_file 无目录部分（如 "<string>"）：用当前工作目录
        _ => {
            let cwd: PathBuf = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            cwd.join(path).to_string_lossy().into_owned()
        }
    }
}

/// deref_value 解引用：从 Native(Arc<Mutex<Value>>) 包装中读取内部值。
fn deref_value(v: &Value) -> Result<Value, String> {
    use std::sync::Arc;
    use std::sync::Mutex;
    match v {
        Value::Native(n) => {
            if let Some(cell) = n.downcast_ref::<Arc<Mutex<Value>>>() {
                Ok(cell.lock().unwrap().clone())
            } else {
                Err("cannot dereference non-ref value".into())
            }
        }
        _ => Err(format!("cannot dereference {} (可能原因：只有 & 创建的引用才能用 * 解引用)", v.type_name())),
    }
}

/// set_deref_value 引用赋值：写入 Native(Arc<Mutex<Value>>) 包装。
fn set_deref_value(ref_val: &Value, new_val: Value) -> Result<(), String> {
    use std::sync::Arc;
    use std::sync::Mutex;
    match ref_val {
        Value::Native(n) => {
            if let Some(cell) = n.downcast_ref::<Arc<Mutex<Value>>>() {
                *cell.lock().unwrap() = new_val;
                Ok(())
            } else {
                Err("cannot set deref: not a ref wrapper".into())
            }
        }
        _ => Err(format!("cannot set deref on {} (可能原因：只有 & 创建的引用才能赋值)", ref_val.type_name())),
    }
}
