//! lexer.rs — 词法分析器
//!
//! 设计要点：
//!   - 将源码字符串扫描为 Token 序列
//!   - 支持多行字符串（三引号 """..."""）
//!   - 支持反引号 raw string（`...`，不转义）
//!   - 支持数字字面量：十进制、0x 十六进制、0o 八进制、0b 二进制、浮点、科学计数法
//!   - 支持行注释 # 和 //
//!   - UTF-8 编码
//!   - 错误信息包含行号、列号、可能原因（AI 友好）

use crate::token::{lookup_keyword, Token, TokenKind};

/// Lexer 词法分析器。
pub struct Lexer {
    /// src 源码（UTF-8 字节）。
    src: Vec<u8>,
    /// pos 当前位置（字节偏移）。
    pos: usize,
    /// line 当前行号（1-based）。
    line: u32,
    /// col 当前列号（1-based）。
    col: u32,
    /// file 文件名（用于错误信息）。
    #[allow(dead_code)]
    file: String,
}

/// LexError 词法错误。
#[derive(Debug, Clone)]
pub struct LexError {
    pub msg: String,
    pub line: u32,
    pub col: u32,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.msg)
    }
}

impl std::error::Error for LexError {}

impl Lexer {
    /// new 创建词法分析器。
    pub fn new(src: &str, file: &str) -> Self {
        Lexer {
            src: src.as_bytes().to_vec(),
            pos: 0,
            line: 1,
            col: 1,
            file: file.to_string(),
        }
    }

    /// lex 扫描全部 Token。
    pub fn lex(&mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace_and_comments()?;
            if self.pos >= self.src.len() {
                break;
            }
            let tok = self.next_token()?;
            tokens.push(tok);
        }
        tokens.push(Token::new(TokenKind::EOF, String::new(), self.line, self.col));
        Ok(tokens)
    }

    /// peek_byte 查看当前字节（不消费）。
    fn peek_byte(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    /// peek_byte_at 查看偏移 n 处的字节。
    fn peek_byte_at(&self, n: usize) -> Option<u8> {
        self.src.get(self.pos + n).copied()
    }

    /// advance 消费当前字节并返回。
    fn advance(&mut self) -> Option<u8> {
        let b = self.peek_byte()?;
        self.pos += 1;
        if b == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(b)
    }

    /// skip_bytes 跳过 n 个字节（用于确认消费已窥探的字节，不更新行/列的换行统计除外）。
    fn skip_bytes(&mut self, n: usize) {
        for _ in 0..n {
            self.advance();
        }
    }

    /// process_escape 处理反斜杠后的转义字符（已消费 '\\'，本方法消费转义字符及其后续）。
    ///
    /// 统一单行/多行字符串的转义语义（容错策略，与 Python/JS 一致）：
    ///   - 已识别简单转义（n t r \\ " ' 0）：照常转换
    ///   - \xNN \uNNNN \UNNNNNNNN：仅当后跟恰好 N 个十六进制字符才解析为码点；
    ///     否则保留字面量 "\x"/"\u"/"\U"（避免 Windows 路径 "C:\Users" 报错）
    ///   - 未识别转义（\d \s \w 等）：保留字面量 "\X"
    ///
    /// 返回转义结果字符串（通常 1 个字符，字面量保留时为 2 个字符 "\"+"X"）。
    /// 输入结束（\ 后无字符）返回 LexError。
    fn process_escape(&mut self, line: u32, col: u32) -> Result<String, LexError> {
        let esc = self.advance().ok_or(LexError {
            msg: "unterminated escape sequence (转义序列未闭合；可能原因：字符串以反斜杠结尾)".into(),
            line,
            col,
        })?;
        let out = match esc {
            b'n' => "\n".to_string(),
            b't' => "\t".to_string(),
            b'r' => "\r".to_string(),
            b'\\' => "\\".to_string(),
            b'"' => "\"".to_string(),
            b'\'' => "'".to_string(),
            b'0' => "\0".to_string(),
            b'x' | b'u' | b'U' => {
                let n = match esc {
                    b'x' => 2,
                    b'u' => 4,
                    _ => 8,
                };
                if let Some(hex) = self.peek_hex(n) {
                    self.skip_bytes(n);
                    let code = u32::from_str_radix(&hex, 16).unwrap_or(0xFFFD);
                    char::from_u32(code).unwrap_or('\u{FFFD}').to_string()
                } else {
                    // 非合法十六进制转义：保留字面量
                    format!("\\{}", esc as char)
                }
            }
            // 未识别转义：保留字面量
            other => format!("\\{}", other as char),
        };
        Ok(out)
    }

    /// peek_hex 窥探当前位置起的 n 个字节是否全部为十六进制字符。
    ///
    /// 若全部为十六进制，返回由这些字符组成的小写字符串（不消费字节）；
    /// 否则返回 None（用于转义容错：非合法十六进制则保留字面量）。
    fn peek_hex(&self, n: usize) -> Option<String> {
        let mut hex = String::with_capacity(n);
        for i in 0..n {
            let b = self.peek_byte_at(i)?;
            let c = b as char;
            if c.is_ascii_hexdigit() {
                hex.push(c.to_ascii_lowercase());
            } else {
                return None;
            }
        }
        Some(hex)
    }

    /// skip_whitespace_and_comments 跳过空白与注释。
    fn skip_whitespace_and_comments(&mut self) -> Result<(), LexError> {
        loop {
            match self.peek_byte() {
                Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n') => {
                    self.advance();
                }
                Some(b'/') if self.peek_byte_at(1) == Some(b'/') => {
                    // 行注释 //
                    while let Some(b) = self.peek_byte() {
                        if b == b'\n' {
                            break;
                        }
                        self.advance();
                    }
                }
                Some(b'/') if self.peek_byte_at(1) == Some(b'*') => {
                    // 块注释 /* ... */（支持嵌套）
                    self.advance();  // 消费 /
                    self.advance();  // 消费 *
                    let mut depth = 1;
                    while depth > 0 {
                        match self.peek_byte() {
                            None => {
                                return Err(LexError {
                                    msg: "unterminated block comment (块注释未闭合；可能原因：缺少 */)".into(),
                                    line: self.line,
                                    col: self.col,
                                });
                            }
                            Some(b'/') if self.peek_byte_at(1) == Some(b'*') => {
                                self.advance();
                                self.advance();
                                depth += 1;  // 嵌套
                            }
                            Some(b'*') if self.peek_byte_at(1) == Some(b'/') => {
                                self.advance();
                                self.advance();
                                depth -= 1;
                            }
                            Some(_) => { self.advance(); }
                        }
                    }
                }
                _ => break,
            }
        }
        Ok(())
    }

    /// next_token 读取下一个 Token。
    fn next_token(&mut self) -> Result<Token, LexError> {
        let line = self.line;
        let col = self.col;
        let b = self.peek_byte().unwrap();

        // 标识符 / 关键字（字母或下划线开头）
        if is_ident_start(b) {
            return self.lex_ident(line, col);
        }
        // 数字
        if b.is_ascii_digit() {
            return self.lex_number(line, col);
        }
        // 字符串
        if b == b'"' {
            return self.lex_string(line, col);
        }
        // 多行字符串（"""）
        if b == b'"' && self.peek_byte_at(1) == Some(b'"') && self.peek_byte_at(2) == Some(b'"') {
            return self.lex_multiline_string(line, col);
        }
        // raw string（反引号）
        if b == b'`' {
            return self.lex_raw_string(line, col);
        }
        // 运算符与分隔符
        self.lex_operator(line, col)
    }

    /// lex_ident 读取标识符或关键字。
    fn lex_ident(&mut self, line: u32, col: u32) -> Result<Token, LexError> {
        let start = self.pos;
        while let Some(b) = self.peek_byte() {
            if is_ident_part(b) {
                self.advance();
            } else {
                break;
            }
        }
        let s = std::str::from_utf8(&self.src[start..self.pos])
            .map_err(|_| LexError {
                msg: "invalid UTF-8 in identifier".into(),
                line,
                col,
            })?
            .to_string();
        let kind = lookup_keyword(&s).unwrap_or(TokenKind::Ident);
        Ok(Token::new(kind, s, line, col))
    }

    /// lex_number 读取数字字面量。
    fn lex_number(&mut self, line: u32, col: u32) -> Result<Token, LexError> {
        let start = self.pos;

        // 检测 0x/0o/0b 前缀
        if self.peek_byte() == Some(b'0') {
            match self.peek_byte_at(1) {
                Some(b'x') | Some(b'X') => {
                    self.advance();
                    self.advance();
                    let hex_start = self.pos;
                    while let Some(b) = self.peek_byte() {
                        if b.is_ascii_hexdigit() || b == b'_' {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    if self.pos == hex_start {
                        return Err(LexError {
                            msg: "hex literal needs at least one digit after 0x (例如 0xFF；可能原因：忘记写数字)".into(),
                            line,
                            col,
                        });
                    }
                    let s = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
                    return Ok(Token::new(TokenKind::Int, s.to_string(), line, col));
                }
                Some(b'o') | Some(b'O') => {
                    self.advance();
                    self.advance();
                    let oct_start = self.pos;
                    while let Some(b) = self.peek_byte() {
                        if (b'0'..=b'7').contains(&b) || b == b'_' {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    if self.pos == oct_start {
                        return Err(LexError {
                            msg: "octal literal needs at least one digit after 0o (例如 0o77)".into(),
                            line,
                            col,
                        });
                    }
                    let s = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
                    return Ok(Token::new(TokenKind::Int, s.to_string(), line, col));
                }
                Some(b'b') | Some(b'B') => {
                    self.advance();
                    self.advance();
                    let bin_start = self.pos;
                    while let Some(b) = self.peek_byte() {
                        if b == b'0' || b == b'1' || b == b'_' {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    if self.pos == bin_start {
                        return Err(LexError {
                            msg: "binary literal needs at least one digit after 0b (例如 0b1010)".into(),
                            line,
                            col,
                        });
                    }
                    let s = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
                    return Ok(Token::new(TokenKind::Int, s.to_string(), line, col));
                }
                _ => {}
            }
        }

        // 十进制整数或浮点（支持下划线分隔符，如 1_000_000）
        let mut is_float = false;
        while let Some(b) = self.peek_byte() {
            if b.is_ascii_digit() || b == b'_' {
                self.advance();
            } else if b == b'.' && !is_float && self.peek_byte_at(1).map_or(false, |c| c.is_ascii_digit()) {
                is_float = true;
                self.advance();
            } else {
                break;
            }
        }
        // 科学计数法
        if matches!(self.peek_byte(), Some(b'e') | Some(b'E')) {
            is_float = true;
            self.advance();
            if matches!(self.peek_byte(), Some(b'+') | Some(b'-')) {
                self.advance();
            }
            while let Some(b) = self.peek_byte() {
                if b.is_ascii_digit() || b == b'_' {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        let s = std::str::from_utf8(&self.src[start..self.pos]).unwrap().to_string();
        let kind = if is_float { TokenKind::Float } else { TokenKind::Int };
        Ok(Token::new(kind, s, line, col))
    }

    /// lex_string 读取双引号字符串（单行，支持转义）。
    fn lex_string(&mut self, line: u32, col: u32) -> Result<Token, LexError> {
        self.advance(); // 消费开头 "

        // 检测多行字符串 """
        if self.peek_byte() == Some(b'"') && self.peek_byte_at(1) == Some(b'"') {
            return self.lex_multiline_string(line, col);
        }

        let mut s = String::new();
        loop {
            match self.peek_byte() {
                None => {
                    return Err(LexError {
                        msg: "unterminated string (字符串未闭合；可能原因：忘记写结束引号 \")".into(),
                        line,
                        col,
                    });
                }
                Some(b'"') => {
                    self.advance();
                    break;
                }
                Some(b'\\') => {
                    self.advance(); // 消费 '\\'
                    let escaped = self.process_escape(line, col)?;
                    s.push_str(&escaped);
                }
                Some(b'\n') => {
                    return Err(LexError {
                        msg: "unterminated string (单行字符串不能跨行；可能原因：想用多行字符串请用 \"\"\" 或反引号)".into(),
                        line,
                        col,
                    });
                }
                Some(b) => {
                    // 直接消费 UTF-8 字节
                    if b < 0x80 {
                        s.push(b as char);
                        self.advance();
                    } else {
                        // 多字节 UTF-8：找到完整字符
                        let char_start = self.pos;
                        let n = utf8_char_len(b);
                        for _ in 0..n {
                            self.advance();
                        }
                        s.push_str(std::str::from_utf8(&self.src[char_start..self.pos]).unwrap_or("\u{FFFD}"));
                    }
                }
            }
        }
        Ok(Token::new(TokenKind::String, s, line, col))
    }

    /// lex_multiline_string 读取三引号多行字符串。
    ///
    /// 注：调用方 lex_string 已消费开头的第 1 个 '"'，此处仅再消费剩余 2 个 '"'。
    fn lex_multiline_string(&mut self, line: u32, col: u32) -> Result<Token, LexError> {
        // 消费开头 """ 的后两个 "（第 1 个已由 lex_string 消费）
        self.advance();
        self.advance();

        let mut s = String::new();
        loop {
            if self.pos >= self.src.len() {
                return Err(LexError {
                    msg: "unterminated multiline string (多行字符串未闭合；可能原因：忘记写结束 \"\"\")".into(),
                    line,
                    col,
                });
            }
            // 检测结束 """
            if self.peek_byte() == Some(b'"')
                && self.peek_byte_at(1) == Some(b'"')
                && self.peek_byte_at(2) == Some(b'"')
            {
                self.advance();
                self.advance();
                self.advance();
                break;
            }
            // 转义处理（与单行字符串一致，复用 process_escape）
            if self.peek_byte() == Some(b'\\') {
                self.advance(); // 消费 '\\'
                let escaped = self.process_escape(line, col)?;
                s.push_str(&escaped);
            } else {
                let b = self.peek_byte().unwrap();
                if b < 0x80 {
                    s.push(b as char);
                    self.advance();
                } else {
                    let char_start = self.pos;
                    let n = utf8_char_len(b);
                    for _ in 0..n {
                        self.advance();
                    }
                    s.push_str(std::str::from_utf8(&self.src[char_start..self.pos]).unwrap_or("\u{FFFD}"));
                }
            }
        }
        Ok(Token::new(TokenKind::String, s, line, col))
    }

    /// lex_raw_string 读取反引号 raw string（不转义）。
    fn lex_raw_string(&mut self, line: u32, col: u32) -> Result<Token, LexError> {
        self.advance(); // 消费开头 `
        let start = self.pos;
        loop {
            match self.peek_byte() {
                None => {
                    return Err(LexError {
                        msg: "unterminated raw string (raw string 未闭合；可能原因：忘记写结束 `)".into(),
                        line,
                        col,
                    });
                }
                Some(b'`') => {
                    let s = std::str::from_utf8(&self.src[start..self.pos])
                        .map_err(|_| LexError {
                            msg: "invalid UTF-8 in raw string".into(),
                            line,
                            col,
                        })?
                        .to_string();
                    self.advance(); // 消费结束 `
                    return Ok(Token::new(TokenKind::RawString, s, line, col));
                }
                Some(_) => {
                    self.advance();
                }
            }
        }
    }

    /// lex_operator 读取运算符与分隔符。
    fn lex_operator(&mut self, line: u32, col: u32) -> Result<Token, LexError> {
        let b = self.peek_byte().unwrap();
        let kind = match b {
            b'=' => {
                if self.peek_byte_at(1) == Some(b'=') {
                    self.advance();
                    self.advance();
                    TokenKind::Eq
                } else {
                    self.advance();
                    TokenKind::Assign
                }
            }
            b'!' => {
                if self.peek_byte_at(1) == Some(b'=') {
                    self.advance();
                    self.advance();
                    TokenKind::Neq
                } else {
                    self.advance();
                    TokenKind::Not
                }
            }
            b'<' => {
                match self.peek_byte_at(1) {
                    Some(b'<') => {
                        if self.peek_byte_at(2) == Some(b'=') {
                            self.advance(); self.advance(); self.advance(); TokenKind::ShlAssign
                        } else {
                            self.advance(); self.advance(); TokenKind::Shl
                        }
                    }
                    Some(b'=') => { self.advance(); self.advance(); TokenKind::LE }
                    _ => { self.advance(); TokenKind::LT }
                }
            }
            b'>' => {
                match self.peek_byte_at(1) {
                    Some(b'>') => {
                        if self.peek_byte_at(2) == Some(b'=') {
                            self.advance(); self.advance(); self.advance(); TokenKind::ShrAssign
                        } else {
                            self.advance(); self.advance(); TokenKind::Shr
                        }
                    }
                    Some(b'=') => { self.advance(); self.advance(); TokenKind::GE }
                    _ => { self.advance(); TokenKind::GT }
                }
            }
            b'&' => {
                match self.peek_byte_at(1) {
                    Some(b'&') => { self.advance(); self.advance(); TokenKind::AndAnd }
                    Some(b'=') => { self.advance(); self.advance(); TokenKind::AmpAssign }
                    _ => { self.advance(); TokenKind::Amp }
                }
            }
            b'|' => {
                match self.peek_byte_at(1) {
                    Some(b'|') => { self.advance(); self.advance(); TokenKind::OrOr }
                    Some(b'=') => { self.advance(); self.advance(); TokenKind::PipeAssign }
                    _ => { self.advance(); TokenKind::Pipe }
                }
            }
            b'^' => {
                if self.peek_byte_at(1) == Some(b'=') { self.advance(); self.advance(); TokenKind::CaretAssign }
                else { self.advance(); TokenKind::Caret }
            }
            b'~' => { self.advance(); TokenKind::Tilde }
            b'?' => {
                // ?? / ??= / ?: 单 ?
                if self.peek_byte_at(1) == Some(b'?') {
                    if self.peek_byte_at(2) == Some(b'=') {
                        self.advance(); self.advance(); self.advance(); TokenKind::NullCoalAssign
                    } else {
                        self.advance(); self.advance(); TokenKind::NullCoal
                    }
                } else {
                    self.advance();
                    TokenKind::Question
                }
            }
            b'+' => {
                match self.peek_byte_at(1) {
                    Some(b'+') => { self.advance(); self.advance(); TokenKind::Plus2 }
                    Some(b'=') => { self.advance(); self.advance(); TokenKind::PlusAssign }
                    _ => { self.advance(); TokenKind::Plus }
                }
            }
            b'-' => {
                match self.peek_byte_at(1) {
                    Some(b'>') => { self.advance(); self.advance(); TokenKind::Arrow }
                    Some(b'-') => { self.advance(); self.advance(); TokenKind::Minus2 }
                    Some(b'=') => { self.advance(); self.advance(); TokenKind::MinusAssign }
                    _ => { self.advance(); TokenKind::Minus }
                }
            }
            b'*' => {
                if self.peek_byte_at(1) == Some(b'=') { self.advance(); self.advance(); TokenKind::StarAssign }
                else { self.advance(); TokenKind::Star }
            }
            b'/' => {
                if self.peek_byte_at(1) == Some(b'=') { self.advance(); self.advance(); TokenKind::SlashAssign }
                else { self.advance(); TokenKind::Slash }
            }
            b'%' => {
                if self.peek_byte_at(1) == Some(b'=') { self.advance(); self.advance(); TokenKind::PercentAssign }
                else { self.advance(); TokenKind::Percent }
            }
            b'(' => { self.advance(); TokenKind::LParen }
            b')' => { self.advance(); TokenKind::RParen }
            b'{' => { self.advance(); TokenKind::LBrace }
            b'}' => { self.advance(); TokenKind::RBrace }
            b'[' => { self.advance(); TokenKind::LBracket }
            b']' => { self.advance(); TokenKind::RBracket }
            b',' => { self.advance(); TokenKind::Comma }
            b';' => { self.advance(); TokenKind::Semicolon }
            b':' => { self.advance(); TokenKind::Colon }
            b'.' => {
                // ... → Ellipsis，否则 Dot
                if self.peek_byte_at(1) == Some(b'.') && self.peek_byte_at(2) == Some(b'.') {
                    self.advance(); self.advance(); self.advance();
                    TokenKind::Ellipsis
                } else {
                    self.advance(); TokenKind::Dot
                }
            }
            _ => {
                return Err(LexError {
                    msg: format!("unexpected character '{}' (0x{:02x}) (可能原因：非法字符；Sflang 不支持此字符)", b as char, b),
                    line,
                    col,
                });
            }
        };
        Ok(Token::new(kind, String::new(), line, col))
    }
}

/// is_ident_start 判断字节是否可作为标识符开头（字母或下划线，含 UTF-8 多字节）。
fn is_ident_start(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphabetic() || b >= 0x80
}

/// is_ident_part 判断字节是否可作为标识符部分（字母/数字/下划线，含 UTF-8 多字节）。
fn is_ident_part(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphanumeric() || b >= 0x80
}

/// utf8_char_len 根据 UTF-8 首字节返回字符长度。
fn utf8_char_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b < 0xC0 {
        1 // 无效首字节，按 1 处理
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else {
        4
    }
}

/// tokenize 便捷函数：扫描源码为 Token 序列。
pub fn tokenize(src: &str, file: &str) -> Result<Vec<Token>, LexError> {
    let mut lex = Lexer::new(src, file);
    lex.lex()
}
