//! opcode.rs — 字节码操作码与 Code 结构定义
//!
//! 设计要点：
//!   - 1 字节操作码 + 操作数（u16 大端序或 u8）
//!   - Code 包含指令序列、常量池、名字池、行号表
//!   - 含 NumLocals（局部变量槽位数）和 FreeSources（闭包捕获来源）
//!
//! 字节码格式（每条指令 1-3 字节）：
//!   OpXxx           → 1 字节（无操作数）
//!   OpXxx <u16>     → 3 字节（u16 大端序）
//!   OpXxx <u8>      → 2 字节（仅 OpCall）

use std::sync::Arc;

use crate::function::Function;
use crate::value::Value;

/// Opcode 操作码类型（1 字节）。
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum Opcode {
    // ---- 栈操作 ----
    /// OpNull 压入 undefined（未定义值）。
    Null = 0,
    /// OpConst 压入常量。u16 常量索引。
    Const = 1,
    /// OpPop 弹出栈顶（表达式语句）。
    Pop = 2,
    /// OpDup 复制栈顶。
    Dup = 3,

    // ---- 变量读写 ----
    /// OpLoadName 读取变量（动态查找：env 链 + globals 回退）。
    LoadName = 10,
    /// OpStoreName 声明变量（顶层 var 走 StoreGlobal，函数内走 StoreLocal）。
    StoreName = 11,
    /// OpAssignName 赋值变量（动态查找，回退到全局）。
    AssignName = 12,
    /// OpLoadGlobal 读取全局变量。u16 名字索引。
    LoadGlobal = 13,
    /// OpStoreGlobal 写入全局变量。u16 名字索引。
    StoreGlobal = 14,
    /// OpLoadLocal 读取当前帧局部变量。u16 槽位索引。
    LoadLocal = 20,
    /// OpStoreLocal 写入当前帧局部变量。u16 槽位索引。
    StoreLocal = 21,
    /// OpLoadFree 读取闭包捕获的自由变量。u16 自由变量索引。
    LoadFree = 22,
    /// OpStoreFree 写入闭包捕获的自由变量。u16 自由变量索引。
    StoreFree = 23,

    // ---- 算术与比较 ----
    /// OpAdd 加法（数值加或字符串拼接）。
    Add = 30,
    /// OpSub 减法。
    Sub = 31,
    /// OpMul 乘法。
    Mul = 32,
    /// OpDiv 除法。
    Div = 33,
    /// OpMod 取模。
    Mod = 34,
    /// OpNeg 一元负。
    Neg = 35,
    /// OpEq 相等。
    Eq = 36,
    /// OpNeq 不等。
    Neq = 37,
    /// OpLT 小于。
    LT = 38,
    /// OpLE 小于等于。
    LE = 39,
    /// OpGT 大于。
    GT = 40,
    /// OpGE 大于等于。
    GE = 41,
    /// OpNot 逻辑非。
    Not = 42,
    /// OpBitAnd 按位与（仅整数）。
    BitAnd = 43,
    /// OpBitOr 按位或（仅整数）。
    BitOr = 44,
    /// OpBitXor 按位异或（仅整数）。
    BitXor = 45,
    /// OpBitShl 左移（仅整数）。
    BitShl = 46,
    /// OpBitShr 右移（仅整数）。
    BitShr = 47,
    /// OpBitNot 按位取反 ~（仅整数，一元）。
    BitNot = 48,

    // ---- 控制流 ----
    /// OpJump 无条件跳转。u16 目标地址。
    Jump = 50,
    /// OpJumpIfFalse 栈顶为假则跳转。u16 目标地址。
    JumpIfFalse = 51,
    /// OpJumpIfTrue 栈顶为真则跳转。u16 目标地址。
    JumpIfTrue = 52,
    /// OpJumpIfNotUndefined 弹出栈顶，若该值不是 undefined 则跳转。u16 目标地址。
    /// 用于 `??` 空合并运算符的短路编译（判定条件是"是否为 undefined"，与 truthy 无关）。
    JumpIfNotUndefined = 53,
    /// OpCompoundIndex 复合索引赋值 a[i] op= v。u8 flag（运算+前后缀）。
    /// 栈：[v, obj, idx] → [new]。地址只求值一次（obj/idx 各弹一次）。
    CompoundIndex = 54,
    /// OpCompoundMember 复合成员赋值 obj.k op= v。u8 name_idx, u8 flag。
    /// 栈：[v, obj] → [new]。
    CompoundMember = 55,
    /// OpIncDecIndex 索引自增自减 a[i]++ / ++a[i]。u8 flag。
    /// 栈：[obj, idx] → [result]（前缀返回新值，后缀返回旧值）。
    IncDecIndex = 56,
    /// OpIncDecMember 成员自增自减 obj.k++ / ++obj.k。u8 name_idx, u8 flag。
    /// 栈：[obj] → [result]。
    IncDecMember = 57,
    /// OpSlice 切片 a[low:high]。栈：[obj, low, high] → [result]。
    /// low/high 缺省时压 undefined（表示到边界）。无操作数。
    Slice = 58,
    /// OpMethodCall 方法调用 obj.name(args)，自动注入 obj 作为隐式 self。
    MethodCall = 59,
    /// OpSpreadCall 带展开的调用。u8 argc, u8 spread_mask。
    SpreadCall = 64,
    /// OpBuildOrdMap 构造有序映射。u16 键值对数量。
    BuildOrdMap = 65,
    /// OpRef 取引用：弹出值，包装为 Arc<Mutex<Value>> 压栈。
    Ref = 66,
    /// OpDeref 解引用：弹出引用包装，读取内部值压栈。
    Deref = 67,
    /// OpSetDeref 引用赋值 *p = v。栈：[..., v, ref] → [..., v]。
    /// 弹出 ref 和 v（v 在底），写入 ref = v，保留 v 在栈。
    SetDeref = 68,

    // ---- 函数调用 ----
    /// OpCall 调用函数。u8 实参数量。
    Call = 60,
    /// OpReturn 返回（栈顶为返回值）。
    Return = 61,
    /// OpReturnVoid 返回 undefined。
    ReturnVoid = 62,
    /// OpClosure 创建闭包。u16 常量索引（Function 模板）。
    Closure = 63,

    // ---- 容器构造 ----
    /// OpBuildArray 构造数组。u16 元素数量（栈顶为最后元素）。
    BuildArray = 70,
    /// OpBuildMap 构造对象。u16 键值对数量。
    BuildMap = 71,
    /// OpIndexGet 读取索引：a[i]。
    IndexGet = 72,
    /// OpIndexSet 设置索引：a[i] = v（v 留栈顶）。
    IndexSet = 73,
    /// OpGetMember 读取成员：a.name。u16 名字索引。
    GetMember = 74,
    /// OpSetMember 设置成员：a.name = v（v 留栈顶）。u16 名字索引。
    SetMember = 75,

    // ---- 异常处理 ----
    /// OpPushTry 压入 try 上下文。u16 catchIP, u16 finallyIP。
    PushTry = 80,
    /// OpPopTry 弹出 try 上下文（try/catch 块正常结束）。
    PopTry = 81,
    /// OpThrow 抛出异常（栈顶为异常值）。
    Throw = 82,
    /// OpExitFinally finally 块结束，恢复挂起的控制流。
    ExitFinally = 83,

    // ---- defer ----
    /// OpDefer 注册 defer 调用。u8 实参数量。
    Defer = 90,

    // ---- 并发 ----
    /// OpRun 启动新线程（run 关键字）。u8 实参数量。
    Run = 100,

    // ---- 导入 ----
    /// OpImport 导入并执行脚本。u16 名字索引（文件名）。
    Import = 110,
}

impl Opcode {
    /// 从 u8 转换为 Opcode（无效值返回 None）。
    pub fn from_u8(v: u8) -> Option<Opcode> {
        // 简单方式：用 match 覆盖所有合法值
        // 注：不用 unsafe transmute，避免未定义行为
        match v {
            0 => Some(Opcode::Null),
            1 => Some(Opcode::Const),
            2 => Some(Opcode::Pop),
            3 => Some(Opcode::Dup),
            10 => Some(Opcode::LoadName),
            11 => Some(Opcode::StoreName),
            12 => Some(Opcode::AssignName),
            13 => Some(Opcode::LoadGlobal),
            14 => Some(Opcode::StoreGlobal),
            20 => Some(Opcode::LoadLocal),
            21 => Some(Opcode::StoreLocal),
            22 => Some(Opcode::LoadFree),
            23 => Some(Opcode::StoreFree),
            30 => Some(Opcode::Add),
            31 => Some(Opcode::Sub),
            32 => Some(Opcode::Mul),
            33 => Some(Opcode::Div),
            34 => Some(Opcode::Mod),
            35 => Some(Opcode::Neg),
            36 => Some(Opcode::Eq),
            37 => Some(Opcode::Neq),
            38 => Some(Opcode::LT),
            39 => Some(Opcode::LE),
            40 => Some(Opcode::GT),
            41 => Some(Opcode::GE),
            42 => Some(Opcode::Not),
            43 => Some(Opcode::BitAnd),
            44 => Some(Opcode::BitOr),
            45 => Some(Opcode::BitXor),
            46 => Some(Opcode::BitShl),
            47 => Some(Opcode::BitShr),
            48 => Some(Opcode::BitNot),
            50 => Some(Opcode::Jump),
            51 => Some(Opcode::JumpIfFalse),
            52 => Some(Opcode::JumpIfTrue),
            53 => Some(Opcode::JumpIfNotUndefined),
            54 => Some(Opcode::CompoundIndex),
            55 => Some(Opcode::CompoundMember),
            56 => Some(Opcode::IncDecIndex),
            57 => Some(Opcode::IncDecMember),
            58 => Some(Opcode::Slice),
            59 => Some(Opcode::MethodCall),
            64 => Some(Opcode::SpreadCall),
            65 => Some(Opcode::BuildOrdMap),
            66 => Some(Opcode::Ref),
            67 => Some(Opcode::Deref),
            68 => Some(Opcode::SetDeref),
            60 => Some(Opcode::Call),
            61 => Some(Opcode::Return),
            62 => Some(Opcode::ReturnVoid),
            63 => Some(Opcode::Closure),
            70 => Some(Opcode::BuildArray),
            71 => Some(Opcode::BuildMap),
            72 => Some(Opcode::IndexGet),
            73 => Some(Opcode::IndexSet),
            74 => Some(Opcode::GetMember),
            75 => Some(Opcode::SetMember),
            80 => Some(Opcode::PushTry),
            81 => Some(Opcode::PopTry),
            82 => Some(Opcode::Throw),
            83 => Some(Opcode::ExitFinally),
            90 => Some(Opcode::Defer),
            100 => Some(Opcode::Run),
            110 => Some(Opcode::Import),
            _ => None,
        }
    }
}

/// FreeSource 闭包捕获来源。
///
/// 描述一个自由变量从何处捕获：
///   - IsLocal=true：从父帧的局部变量捕获（slot 索引）
///   - IsLocal=false：从父函数的自由变量捕获（跨层传递，free_idx 索引）
#[derive(Debug, Clone, Copy)]
pub struct FreeSource {
    /// is_local true=从父帧 local 捕获，false=从父函数 free_var 捕获。
    pub is_local: bool,
    /// index 对应的索引（local slot 或 free_var index）。
    pub index: usize,
}

/// Code 编译后的字节码单元。
///
/// 一个 Code 对应一个函数体（或顶层脚本）。
/// 包含指令序列、常量池、名字池、行号表等。
pub struct Code {
    /// name 代码单元名（函数名或 "<script>"）。
    pub name: String,
    /// file 源码文件名。
    pub file: String,
    /// insts 指令序列（字节码）。
    pub insts: Vec<u8>,
    /// constants 常量池（数值/字符串/Function 模板等）。
    pub constants: Vec<Value>,
    /// names 名字池（变量名/成员名等）。
    pub names: Vec<String>,
    /// lines 行号表（与 insts 等长，每条指令对应的源码行号）。
    pub lines: Vec<u32>,
    /// num_locals 局部变量槽位数量（含参数）。
    pub num_locals: usize,
    /// free_sources 本函数捕获的外层变量来源列表（按捕获顺序）。
    pub free_sources: Vec<FreeSource>,
}

impl Code {
    /// new 创建空 Code。
    pub fn new(name: impl Into<String>, file: impl Into<String>) -> Self {
        Code {
            name: name.into(),
            file: file.into(),
            insts: Vec::new(),
            constants: Vec::new(),
            names: Vec::new(),
            lines: Vec::new(),
            num_locals: 0,
            free_sources: Vec::new(),
        }
    }

    /// emit 追加无操作数指令，返回偏移量。
    pub fn emit(&mut self, op: Opcode) -> usize {
        let off = self.insts.len();
        self.insts.push(op as u8);
        self.lines.push(0); // 行号由 set_line 设置
        off
    }

    /// emit_u8 追加 1 字节操作数指令。
    pub fn emit_u8(&mut self, op: Opcode, arg: u8) -> usize {
        let off = self.insts.len();
        self.insts.push(op as u8);
        self.insts.push(arg);
        self.lines.push(0);
        self.lines.push(0);
        off
    }

    /// emit_u8_u8 追加 2 字节操作数指令（用于 CompoundMember/IncDecMember）。
    pub fn emit_u8_u8(&mut self, op: Opcode, arg1: u8, arg2: u8) -> usize {
        let off = self.insts.len();
        self.insts.push(op as u8);
        self.insts.push(arg1);
        self.insts.push(arg2);
        self.lines.push(0);
        self.lines.push(0);
        self.lines.push(0);
        off
    }

    /// emit_u16 追加 2 字节操作数指令（大端序）。
    pub fn emit_u16(&mut self, op: Opcode, arg: u16) -> usize {
        let off = self.insts.len();
        self.insts.push(op as u8);
        self.insts.push((arg >> 8) as u8);
        self.insts.push(arg as u8);
        self.lines.push(0);
        self.lines.push(0);
        self.lines.push(0);
        off
    }

    /// emit_push_try 追加 PushTry 指令（2 个 u16 操作数）。
    /// 返回偏移量，用于后续 patch。
    pub fn emit_push_try(&mut self, catch_ip: u16, finally_ip: u16) -> usize {
        let off = self.insts.len();
        self.insts.push(Opcode::PushTry as u8);
        self.insts.push((catch_ip >> 8) as u8);
        self.insts.push(catch_ip as u8);
        self.insts.push((finally_ip >> 8) as u8);
        self.insts.push(finally_ip as u8);
        for _ in 0..5 {
            self.lines.push(0);
        }
        off
    }

    /// patch_u16 回填 u16 操作数。
    pub fn patch_u16(&mut self, off: usize, val: u16) {
        self.insts[off + 1] = (val >> 8) as u8;
        self.insts[off + 2] = val as u8;
    }

    /// patch_push_try 回填 PushTry 的 catch_ip 和 finally_ip。
    pub fn patch_push_try(&mut self, off: usize, catch_ip: u16, finally_ip: u16) {
        self.insts[off + 1] = (catch_ip >> 8) as u8;
        self.insts[off + 2] = catch_ip as u8;
        self.insts[off + 3] = (finally_ip >> 8) as u8;
        self.insts[off + 4] = finally_ip as u8;
    }

    /// set_line 设置当前最后一条指令的行号。
    pub fn set_line(&mut self, line: u32) {
        if let Some(last) = self.lines.last_mut() {
            *last = line;
        }
    }

    /// get_line 获取指定指令位置的源码行号。
    pub fn get_line(&self, ip: usize) -> u32 {
        if ip < self.lines.len() {
            self.lines[ip]
        } else {
            0
        }
    }

    /// add_const 添加常量（自动去重），返回索引。
    ///
    /// 去重用 PartialEq（严格类型+值匹配），不用 equals（跨数值类型判等）。
    /// 否则 Int(7) 和 Float(7.0) 会被误判为同一常量，导致类型被"污染"。
    pub fn add_const(&mut self, v: Value) -> usize {
        for (i, c) in self.constants.iter().enumerate() {
            if *c == v {
                return i;
            }
        }
        let i = self.constants.len();
        self.constants.push(v);
        i
    }

    /// add_name 添加名字（不自动去重，简化逻辑），返回索引。
    /// 注：名字重复不影响正确性，仅多占少量内存。
    pub fn add_name(&mut self, name: impl Into<String>) -> usize {
        let name = name.into();
        // 去重查找
        for (i, n) in self.names.iter().enumerate() {
            if *n == name {
                return i;
            }
        }
        let i = self.names.len();
        self.names.push(name);
        i
    }

    /// read_u16 从指令字节读取 u16（大端序）。
    pub fn read_u16(insts: &[u8], offset: usize) -> u16 {
        ((insts[offset] as u16) << 8) | (insts[offset + 1] as u16)
    }
}

/// 从 Value 获取 Function 模板的辅助函数。
/// 仅对 Value::Func 有效，其他返回 None。
pub fn as_function(v: &Value) -> Option<&Function> {
    match v {
        Value::Func(f) => Some(f.as_ref()),
        _ => None,
    }
}

/// 从 Value 获取 Function 模板的 Arc 引用。
pub fn as_function_rc(v: &Value) -> Option<Arc<Function>> {
    match v {
        Value::Func(f) => Some(Arc::clone(f)),
        _ => None,
    }
}
