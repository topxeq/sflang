//! token.rs — 词法 Token 定义
//!
//! Token 类型与 Go 版本保持一致，便于脚本兼容。

/// TokenKind Token 类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    // 字面量
    Int,
    Float,
    String,
    RawString,
    Bytes,
    Ident,

    // 关键字
    Var,
    Const,
    Func,
    Return,
    If,
    Elif,
    Else,
    While,
    For,
    In,
    Break,
    Continue,
    Try,
    Catch,
    Finally,
    Defer,
    Run,
    Throw,
    Import,
    True,
    False,
    Undefined,
    And,
    Or,
    Not,

    // 运算符
    Assign,        // =
    Plus,          // +
    Plus2,         // ++（自增）
    PlusAssign,    // +=
    Minus,         // -
    Minus2,        // --（自减）
    MinusAssign,   // -=
    Star,          // *
    StarAssign,    // *=
    Slash,         // /
    SlashAssign,   // /=
    Percent,       // %
    PercentAssign, // %=
    NullCoal,      // ??
    NullCoalAssign,// ??=
    Question,      // ?
    Eq,            // ==
    Neq,           // !=
    LT,            // <
    LE,            // <=
    GT,            // >
    GE,            // >=
    AndAnd,        // &&（逻辑与）
    OrOr,          // ||（逻辑或）
    Amp,           // &（按位与）
    AmpAssign,     // &=
    Pipe,          // |（按位或）
    PipeAssign,    // |=
    Caret,         // ^（按位异或）
    CaretAssign,   // ^=
    Tilde,         // ~（按位取反，一元）
    Shl,           // <<（左移）
    ShlAssign,     // <<=
    Shr,           // >>（右移）
    ShrAssign,     // >>=

    // 分隔符
    LParen,        // (
    RParen,        // )
    LBrace,        // {
    RBrace,        // }
    LBracket,      // [
    RBracket,      // ]
    Comma,         // ,
    Semicolon,     // ;
    Colon,         // :
    Dot,           // .
    Ellipsis,      // ...（可变参数标记 / 展开调用）
    Arrow,         // ->

    EOF,
}

impl TokenKind {
    /// is_keyword 判断是否为关键字。
    pub fn is_keyword(self) -> bool {
        matches!(
            self,
            TokenKind::Var
                | TokenKind::Const
                | TokenKind::Func
                | TokenKind::Return
                | TokenKind::If
                | TokenKind::Elif
                | TokenKind::Else
                | TokenKind::While
                | TokenKind::For
                | TokenKind::In
                | TokenKind::Break
                | TokenKind::Continue
                | TokenKind::Try
                | TokenKind::Catch
                | TokenKind::Finally
                | TokenKind::Defer
                | TokenKind::Run
                | TokenKind::Throw
                | TokenKind::Import
                | TokenKind::True
                | TokenKind::False
                | TokenKind::Undefined
        )
    }
}

/// Token 词法单元。
#[derive(Debug, Clone)]
pub struct Token {
    /// kind Token 类型。
    pub kind: TokenKind,
    /// value 字面值（字符串形式；数字待解析）。
    pub value: String,
    /// line 源码行号（1-based）。
    pub line: u32,
    /// col 源码列号（1-based）。
    pub col: u32,
}

impl Token {
    pub fn new(kind: TokenKind, value: String, line: u32, col: u32) -> Self {
        Token { kind, value, line, col }
    }
}

/// 从字符串查找关键字（非关键字返回 None）。
pub fn lookup_keyword(s: &str) -> Option<TokenKind> {
    match s {
        "var" => Some(TokenKind::Var),
        "const" => Some(TokenKind::Const),
        "func" => Some(TokenKind::Func),
        "return" => Some(TokenKind::Return),
        "if" => Some(TokenKind::If),
        "elif" => Some(TokenKind::Elif),
        "else" => Some(TokenKind::Else),
        "while" => Some(TokenKind::While),
        "for" => Some(TokenKind::For),
        "in" => Some(TokenKind::In),
        "break" => Some(TokenKind::Break),
        "continue" => Some(TokenKind::Continue),
        "try" => Some(TokenKind::Try),
        "catch" => Some(TokenKind::Catch),
        "finally" => Some(TokenKind::Finally),
        "defer" => Some(TokenKind::Defer),
        "run" => Some(TokenKind::Run),
        "throw" => Some(TokenKind::Throw),
        "import" => Some(TokenKind::Import),
        "true" => Some(TokenKind::True),
        "false" => Some(TokenKind::False),
        "undefined" => Some(TokenKind::Undefined),
        _ => None,
    }
}
