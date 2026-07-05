//! ast.rs — 抽象语法树节点定义（enum 风格）
//!
//! 设计要点：
//!   - 用 enum 表示 Expr/Stmt，支持 match 模式匹配，无需 downcast
//!   - 每个节点记录 Token 位置（用于错误信息）
//!   - 函数字面量支持默认参数、可变参数
//!   - 支持 for-in 语法

use crate::token::Token;

/// Expr 表达式（enum 风格，便于 match 与 clone）。
#[derive(Debug, Clone)]
pub enum Expr {
    /// IntLit 整数字面量。
    IntLit { tok: Token, value: i64 },
    /// FloatLit 浮点字面量。
    FloatLit { tok: Token, value: f64 },
    /// StringLit 字符串字面量。
    StringLit { tok: Token, value: String },
    /// BoolLit 布尔字面量。
    BoolLit { tok: Token, value: bool },
    /// UndefinedLit undefined 字面量（关键字 `undefined`，兼容旧脚本的 `nil` 别名）。
    UndefinedLit { tok: Token },
    /// Ident 标识符引用。
    Ident { tok: Token, name: String },
    /// ArrayLit 数组字面量。
    ArrayLit { tok: Token, elems: Vec<Expr> },
    /// MapLit 对象字面量。
    MapLit { tok: Token, pairs: Vec<(String, Expr)> },
    /// OrdMapLit 有序映射字面量 map{"k": v}。
    OrdMapLit { tok: Token, pairs: Vec<(String, Expr)> },
    /// BinaryExpr 二元运算。
    BinaryExpr { tok: Token, op: BinaryOp, left: Box<Expr>, right: Box<Expr> },
    /// UnaryExpr 一元运算。
    UnaryExpr { tok: Token, op: UnaryOp, operand: Box<Expr> },
    /// IndexExpr 索引访问 a[i]。
    IndexExpr { tok: Token, obj: Box<Expr>, index: Box<Expr> },
    /// SliceExpr 切片 a[low:high]，low/high 可缺省（None 表示到边界）。
    /// string/array 按字符/元素切片；bytes/byteArray 按字节切片。
    SliceExpr { tok: Token, obj: Box<Expr>, low: Option<Box<Expr>>, high: Option<Box<Expr>> },
    /// MemberExpr 成员访问 a.name。
    MemberExpr { tok: Token, obj: Box<Expr>, name: String },
    /// CallExpr 函数调用 f(args...)。
    CallExpr { tok: Token, callee: Box<Expr>, args: Vec<Expr> },
    /// FuncLit 函数字面量。
    FuncLit { tok: Token, func: FuncLit },
    /// Assign 赋值表达式（求值为被赋的值）。
    Assign { tok: Token, target: AssignTarget, value: Box<Expr> },
    /// Ternary 三元条件表达式 `cond ? then : else_`（右结合）。
    /// 与 if/else 表达式语义等价，是语法糖；else_ 递归允许链式 `a?b:c?d:e`。
    Ternary { tok: Token, cond: Box<Expr>, then: Box<Expr>, else_: Box<Expr> },
    /// IncDec 自增/自减 `++target` / `target++`（含 a[i]++、obj.k++）。
    /// prefix=true 为前缀（返回新值），false 为后缀（返回旧值）。
    IncDec { tok: Token, target: AssignTarget, op: IncDecOp, prefix: bool },
    /// CompoundAssign 复合赋值 `target op= value`（如 a[i] += 1）。
    CompoundAssign { tok: Token, target: AssignTarget, op: BinaryOp, value: Box<Expr> },
    /// Spread 展开表达式 `...expr`（用于可变参数调用的展开：f(...arr)）。
    Spread { tok: Token, expr: Box<Expr> },
}

impl Expr {
    /// token 返回节点的代表 Token。
    pub fn token(&self) -> &Token {
        match self {
            Expr::IntLit { tok, .. } => tok,
            Expr::FloatLit { tok, .. } => tok,
            Expr::StringLit { tok, .. } => tok,
            Expr::BoolLit { tok, .. } => tok,
            Expr::UndefinedLit { tok } => tok,
            Expr::Ident { tok, .. } => tok,
            Expr::ArrayLit { tok, .. } => tok,
            Expr::MapLit { tok, .. } => tok,
            Expr::OrdMapLit { tok, .. } => tok,
            Expr::BinaryExpr { tok, .. } => tok,
            Expr::UnaryExpr { tok, .. } => tok,
            Expr::IndexExpr { tok, .. } => tok,
            Expr::SliceExpr { tok, .. } => tok,
            Expr::MemberExpr { tok, .. } => tok,
            Expr::CallExpr { tok, .. } => tok,
            Expr::FuncLit { tok, .. } => tok,
            Expr::Assign { tok, .. } => tok,
            Expr::Ternary { tok, .. } => tok,
            Expr::IncDec { tok, .. } => tok,
            Expr::CompoundAssign { tok, .. } => tok,
            Expr::Spread { tok, .. } => tok,
        }
    }
}

/// BinaryOp 二元运算符。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add, Sub, Mul, Div, Mod,
    Eq, Neq, LT, LE, GT, GE,
    And, Or,
    /// NullCoal 空合并 ??：左值为 undefined 时取右值（否则取左值）。
    /// 与 falsy 无关：0/""/false 均视为有效值，不触发兜底。
    NullCoal,
    /// BitAnd 按位与 &（仅整数）。
    BitAnd,
    /// BitOr 按位或 |（仅整数）。
    BitOr,
    /// BitXor 按位异或 ^（仅整数）。
    BitXor,
    /// BitShl 左移 <<（仅整数）。
    BitShl,
    /// BitShr 右移 >>（仅整数）。
    BitShr,
}

/// UnaryOp 一元运算符。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg, Not,
    /// BitNot 按位取反 ~（仅整数）。
    BitNot,
}

/// IncDecOp 自增/自减类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncDecOp {
    /// Inc 自增 ++。
    Inc,
    /// Dec 自减 --。
    Dec,
}

/// AssignTarget 赋值目标。
#[derive(Debug, Clone)]
pub enum AssignTarget {
    /// Name 变量赋值。
    Name(String),
    /// Index 索引赋值 a[i] = v。
    Index { obj: Box<Expr>, index: Box<Expr> },
    /// Member 成员赋值 a.name = v。
    Member { obj: Box<Expr>, name: String },
}

/// FuncLit 函数字面量。
#[derive(Debug, Clone)]
pub struct FuncLit {
    /// tok 代表 Token。
    pub tok: Token,
    /// name 函数名（匿名函数为空）。
    pub name: String,
    /// params 形参名列表。
    pub params: Vec<String>,
    /// defaults 默认参数值（与 params 等长，无默认值为 None）。
    pub defaults: Vec<Option<Expr>>,
    /// variadic 是否可变参数。
    pub variadic: bool,
    /// body 函数体。
    pub body: Block,
}

/// Stmt 语句（enum 风格）。
#[derive(Debug, Clone)]
pub enum Stmt {
    /// ExprStmt 表达式语句。
    ExprStmt { tok: Token, expr: Expr },
    /// VarDecl var 声明。
    VarDecl { tok: Token, name: String, value: Option<Expr> },
    /// FuncDecl 函数声明。
    FuncDecl { tok: Token, func: FuncLit },
    /// IfStmt if/elif/else。
    IfStmt {
        tok: Token,
        cond: Expr,
        then: Block,
        elif_conds: Vec<Expr>,
        elif_bodies: Vec<Block>,
        else_block: Option<Block>,
    },
    /// WhileStmt while 循环。
    WhileStmt { tok: Token, cond: Expr, body: Block },
    /// ForStmt C 风格 for 循环。
    ForStmt {
        tok: Token,
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        post: Option<Box<Stmt>>,
        body: Block,
    },
    /// ForInStmt for-in 循环。
    ForInStmt {
        tok: Token,
        index_var: Option<String>,
        var: String,
        iter: Expr,
        body: Block,
    },
    /// BreakStmt break。
    BreakStmt { tok: Token },
    /// ContinueStmt continue。
    ContinueStmt { tok: Token },
    /// ReturnStmt return。
    ReturnStmt { tok: Token, value: Option<Expr> },
    /// TryStmt try/catch/finally。
    TryStmt {
        tok: Token,
        try_block: Block,
        catch_var: Option<String>,
        catch_block: Option<Block>,
        finally_block: Option<Block>,
    },
    /// DeferStmt defer。
    DeferStmt { tok: Token, call: Expr },
    /// RunStmt run（启动新线程）。
    RunStmt { tok: Token, call: Expr },
    /// ThrowStmt throw。
    ThrowStmt { tok: Token, expr: Expr },
    /// ImportStmt import（加载并执行另一脚本，合并其顶层定义到当前全局环境）。
    ImportStmt { tok: Token, path: String },
    /// Block 块语句。
    Block { tok: Token, stmts: Vec<Stmt> },
}

impl Stmt {
    /// token 返回节点的代表 Token。
    pub fn token(&self) -> &Token {
        match self {
            Stmt::ExprStmt { tok, .. } => tok,
            Stmt::VarDecl { tok, .. } => tok,
            Stmt::FuncDecl { tok, .. } => tok,
            Stmt::IfStmt { tok, .. } => tok,
            Stmt::WhileStmt { tok, .. } => tok,
            Stmt::ForStmt { tok, .. } => tok,
            Stmt::ForInStmt { tok, .. } => tok,
            Stmt::BreakStmt { tok } => tok,
            Stmt::ContinueStmt { tok } => tok,
            Stmt::ReturnStmt { tok, .. } => tok,
            Stmt::TryStmt { tok, .. } => tok,
            Stmt::DeferStmt { tok, .. } => tok,
            Stmt::RunStmt { tok, .. } => tok,
            Stmt::ThrowStmt { tok, .. } => tok,
            Stmt::ImportStmt { tok, .. } => tok,
            Stmt::Block { tok, .. } => tok,
        }
    }
}

/// Block 块语句。
#[derive(Debug, Clone)]
pub struct Block {
    /// tok 代表 Token。
    pub tok: Token,
    /// stmts 语句列表。
    pub stmts: Vec<Stmt>,
}

/// Program 整个程序。
#[derive(Debug)]
pub struct Program {
    /// file 文件名。
    pub file: String,
    /// stmts 顶层语句列表。
    pub stmts: Vec<Stmt>,
}
