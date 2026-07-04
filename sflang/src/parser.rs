//! parser.rs — 语法分析器
//!
//! 设计要点：
//!   - 递归下降式解析
//!   - 运算符优先级：or < and < 比较 < +- < */ < 一元 < 后缀
//!   - 错误信息包含行号、可能原因（AI 友好）

use crate::ast::*;
use crate::token::{Token, TokenKind};

/// Parser 语法分析器。
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    file: String,
}

/// ParseError 语法错误。
#[derive(Debug, Clone)]
pub struct ParseError {
    pub msg: String,
    pub line: u32,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.line, self.msg)
    }
}

impl std::error::Error for ParseError {}

impl Parser {
    pub fn new(tokens: Vec<Token>, file: &str) -> Self {
        Parser { tokens, pos: 0, file: file.to_string() }
    }

    pub fn parse(&mut self) -> Result<Program, ParseError> {
        let mut stmts = Vec::new();
        while !self.at_end() {
            while self.check(TokenKind::Semicolon) {
                self.advance();
            }
            if self.at_end() {
                break;
            }
            stmts.push(self.parse_stmt()?);
        }
        Ok(Program { file: self.file.clone(), stmts })
    }

    fn peek(&self) -> &Token { &self.tokens[self.pos] }
    fn peek_at(&self, n: usize) -> &Token {
        let i = self.pos + n;
        if i < self.tokens.len() { &self.tokens[i] } else { self.peek() }
    }
    fn advance(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        if !self.at_end() { self.pos += 1; }
        t
    }
    fn check(&self, kind: TokenKind) -> bool { !self.at_end() && self.peek().kind == kind }
    fn at_end(&self) -> bool { self.peek().kind == TokenKind::EOF }
    fn match_token(&mut self, kind: TokenKind) -> bool {
        if self.check(kind) { self.advance(); true } else { false }
    }
    fn expect(&mut self, kind: TokenKind, what: &str) -> Result<Token, ParseError> {
        if self.check(kind) { Ok(self.advance()) } else {
            Err(ParseError {
                msg: format!("期望 {}，但得到 '{}' (可能原因：语法结构不完整)", what, self.peek().value),
                line: self.peek().line,
            })
        }
    }
    fn err(&self, msg: impl Into<String>) -> ParseError {
        ParseError { msg: msg.into(), line: self.peek().line }
    }

    // ---- 语句 ----

    fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        match self.peek().kind {
            TokenKind::Var => self.parse_var_decl(),
            TokenKind::Const => self.parse_const_decl(),
            TokenKind::Func => {
                if matches!(self.peek_at(1).kind, TokenKind::Ident) {
                    self.parse_func_decl()
                } else {
                    let expr = self.parse_expr()?;
                    self.consume_semicolon();
                    Ok(Stmt::ExprStmt { tok: expr.token().clone(), expr })
                }
            }
            TokenKind::If => self.parse_if(),
            TokenKind::While => self.parse_while(),
            TokenKind::For => self.parse_for(),
            TokenKind::Break => { let t = self.advance(); self.consume_semicolon(); Ok(Stmt::BreakStmt { tok: t }) }
            TokenKind::Continue => { let t = self.advance(); self.consume_semicolon(); Ok(Stmt::ContinueStmt { tok: t }) }
            TokenKind::Return => self.parse_return(),
            TokenKind::Try => self.parse_try(),
            TokenKind::Defer => self.parse_defer(),
            TokenKind::Run => self.parse_run(),
            TokenKind::Throw => self.parse_throw(),
            TokenKind::Import => self.parse_import(),
            TokenKind::LBrace => {
                let b = self.parse_block()?;
                Ok(Stmt::Block { tok: b.tok.clone(), stmts: b.stmts })
            }
            _ => {
                let expr = self.parse_expr()?;
                self.consume_semicolon();
                Ok(Stmt::ExprStmt { tok: expr.token().clone(), expr })
            }
        }
    }

    fn consume_semicolon(&mut self) { self.match_token(TokenKind::Semicolon); }

    fn parse_block(&mut self) -> Result<Block, ParseError> {
        let tok = self.expect(TokenKind::LBrace, "'{'")?;
        let mut stmts = Vec::new();
        while !self.check(TokenKind::RBrace) && !self.at_end() {
            while self.match_token(TokenKind::Semicolon) {}
            if self.check(TokenKind::RBrace) { break; }
            stmts.push(self.parse_stmt()?);
        }
        self.expect(TokenKind::RBrace, "'}'")?;
        Ok(Block { tok, stmts })
    }

    fn parse_var_decl(&mut self) -> Result<Stmt, ParseError> {
        let tok = self.advance();
        let name = self.expect(TokenKind::Ident, "变量名")?.value;
        let value = if self.match_token(TokenKind::Assign) { Some(self.parse_expr()?) } else { None };
        self.consume_semicolon();
        Ok(Stmt::VarDecl { tok, name, value })
    }

    fn parse_const_decl(&mut self) -> Result<Stmt, ParseError> {
        let tok = self.advance();
        let name = self.expect(TokenKind::Ident, "常量名")?.value;
        self.expect(TokenKind::Assign, "'=' (const 必须有初始值)")?;
        let value = self.parse_expr()?;
        self.consume_semicolon();
        Ok(Stmt::VarDecl { tok, name, value: Some(value) })
    }

    fn parse_func_decl(&mut self) -> Result<Stmt, ParseError> {
        let tok = self.advance();
        let name = self.expect(TokenKind::Ident, "函数名")?.value;
        let func = self.parse_func_lit_body(tok, Some(name))?;
        Ok(Stmt::FuncDecl { tok: func.tok.clone(), func })
    }

    fn parse_func_lit_body(&mut self, tok: Token, name: Option<String>) -> Result<FuncLit, ParseError> {
        self.expect(TokenKind::LParen, "'(' (函数参数列表)")?;
        let mut params = Vec::new();
        let mut defaults = Vec::new();
        let mut variadic = false;
        while !self.check(TokenKind::RParen) {
            if self.match_token(TokenKind::Ellipsis) { variadic = true; }
            let p = self.expect(TokenKind::Ident, "参数名")?.value;
            params.push(p.clone());
            if self.match_token(TokenKind::Assign) {
                defaults.push(Some(self.parse_expr()?));
            } else {
                defaults.push(None);
            }
            if !self.match_token(TokenKind::Comma) { break; }
        }
        self.expect(TokenKind::RParen, "')'")?;
        let body = self.parse_block()?;
        Ok(FuncLit { tok, name: name.unwrap_or_default(), params, defaults, variadic, body })
    }

    fn parse_if(&mut self) -> Result<Stmt, ParseError> {
        let tok = self.advance();
        let cond = self.parse_expr()?;
        let then = self.parse_block()?;
        let mut elif_conds = Vec::new();
        let mut elif_bodies = Vec::new();
        let mut else_block = None;
        while self.check(TokenKind::Elif) {
            self.advance();
            elif_conds.push(self.parse_expr()?);
            elif_bodies.push(self.parse_block()?);
        }
        if self.check(TokenKind::Else) {
            self.advance();
            if self.check(TokenKind::If) {
                let inner = self.parse_if()?;
                else_block = Some(Block { tok: tok.clone(), stmts: vec![inner] });
            } else {
                else_block = Some(self.parse_block()?);
            }
        }
        Ok(Stmt::IfStmt { tok, cond, then, elif_conds, elif_bodies, else_block })
    }

    fn parse_while(&mut self) -> Result<Stmt, ParseError> {
        let tok = self.advance();
        let cond = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::WhileStmt { tok, cond, body })
    }

    fn parse_for(&mut self) -> Result<Stmt, ParseError> {
        let tok = self.advance();
        // for-in: for v in iter / for i, v in iter
        if self.check(TokenKind::Ident) && self.peek_at(1).kind == TokenKind::In {
            let var = self.advance().value;
            self.advance();
            let iter = self.parse_expr()?;
            let body = self.parse_block()?;
            return Ok(Stmt::ForInStmt { tok, index_var: None, var, iter, body });
        }
        if self.check(TokenKind::Ident) && self.peek_at(1).kind == TokenKind::Comma
            && self.peek_at(2).kind == TokenKind::Ident && self.peek_at(3).kind == TokenKind::In
        {
            let idx = self.advance().value;
            self.advance();
            let var = self.advance().value;
            self.advance();
            let iter = self.parse_expr()?;
            let body = self.parse_block()?;
            return Ok(Stmt::ForInStmt { tok, index_var: Some(idx), var, iter, body });
        }
        // C 风格 for (init; cond; post)
        let has_paren = self.match_token(TokenKind::LParen);
        let init = if !self.check(TokenKind::Semicolon) {
            // parse_simple_stmt 会消费 init 后的 ;
            Some(Box::new(self.parse_simple_stmt()?))
        } else {
            // init 为空，手动消费第一个 ;
            if has_paren { self.expect(TokenKind::Semicolon, "';'")?; } else { self.match_token(TokenKind::Semicolon); }
            None
        };
        let cond = if !self.check(TokenKind::Semicolon) { Some(self.parse_expr()?) } else { None };
        self.expect(TokenKind::Semicolon, "';'")?;
        let post = if (has_paren && !self.check(TokenKind::RParen)) || (!has_paren && !self.check(TokenKind::LBrace)) {
            Some(Box::new(self.parse_simple_stmt()?))
        } else { None };
        if has_paren { self.expect(TokenKind::RParen, "')'")?; }
        let body = self.parse_block()?;
        Ok(Stmt::ForStmt { tok, init, cond, post, body })
    }

    fn parse_simple_stmt(&mut self) -> Result<Stmt, ParseError> {
        match self.peek().kind {
            TokenKind::Var => self.parse_var_decl(),
            TokenKind::Const => self.parse_const_decl(),
            _ => {
                let expr = self.parse_expr()?;
                Ok(Stmt::ExprStmt { tok: expr.token().clone(), expr })
            }
        }
    }

    fn parse_return(&mut self) -> Result<Stmt, ParseError> {
        let tok = self.advance();
        let value = if self.check(TokenKind::Semicolon) || self.check(TokenKind::RBrace) || self.at_end() {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.consume_semicolon();
        Ok(Stmt::ReturnStmt { tok, value })
    }

    fn parse_try(&mut self) -> Result<Stmt, ParseError> {
        let tok = self.advance();
        let try_block = self.parse_block()?;
        let mut catch_var = None;
        let mut catch_block = None;
        let mut finally_block = None;
        if self.check(TokenKind::Catch) {
            self.advance();
            if self.match_token(TokenKind::LParen) {
                catch_var = Some(self.expect(TokenKind::Ident, "catch 变量名")?.value);
                self.expect(TokenKind::RParen, "')'")?;
            }
            catch_block = Some(self.parse_block()?);
        }
        if self.check(TokenKind::Finally) {
            self.advance();
            finally_block = Some(self.parse_block()?);
        }
        Ok(Stmt::TryStmt { tok, try_block, catch_var, catch_block, finally_block })
    }

    fn parse_defer(&mut self) -> Result<Stmt, ParseError> {
        let tok = self.advance();
        let call = self.parse_expr()?;
        match &call {
            Expr::CallExpr { .. } => {}
            _ => return Err(self.err("defer 后必须是函数调用")),
        }
        Ok(Stmt::DeferStmt { tok, call })
    }

    fn parse_run(&mut self) -> Result<Stmt, ParseError> {
        let tok = self.advance();
        let call = self.parse_expr()?;
        match &call {
            Expr::CallExpr { .. } => {}
            _ => return Err(self.err("run 后必须是函数调用")),
        }
        Ok(Stmt::RunStmt { tok, call })
    }

    fn parse_throw(&mut self) -> Result<Stmt, ParseError> {
        let tok = self.advance();
        let expr = self.parse_expr()?;
        self.consume_semicolon();
        Ok(Stmt::ThrowStmt { tok, expr })
    }

    /// parse_import 解析 import 语句：`import "path"`。
    ///
    /// path 必须是字符串字面量（普通字符串或 raw string）。
    /// 加载并执行目标脚本，其顶层 var/func 合并到当前全局环境。
    fn parse_import(&mut self) -> Result<Stmt, ParseError> {
        let tok = self.advance();
        let path_tok = self.expect(TokenKind::String, "import 后必须是字符串路径")?;
        self.consume_semicolon();
        Ok(Stmt::ImportStmt { tok, path: path_tok.value })
    }

    // ---- 表达式（按优先级） ----

    fn parse_expr(&mut self) -> Result<Expr, ParseError> { self.parse_assign() }

    fn parse_assign(&mut self) -> Result<Expr, ParseError> {
        // 优先级链：assign < ternary < nullcoal < or < and < bitor < bitxor < bitand
        //            < 比较 < shift < +- < */ < 一元 < 后缀
        let expr = self.parse_ternary()?;
        // 普通赋值
        if self.check(TokenKind::Assign) {
            let tok = self.advance();
            let value = self.parse_assign()?;
            let target = expr_to_target(expr);
            return Ok(Expr::Assign { tok, target, value: Box::new(value) });
        }
        // 复合赋值 op= （+= -= *= /= %= ??= &= |= ^= <<= >>=）
        if let Some(op) = compound_assign_op(self.peek().kind) {
            let tok = self.advance();
            let value = self.parse_assign()?;
            let target = expr_to_target(expr);
            return Ok(Expr::CompoundAssign { tok, target, op, value: Box::new(value) });
        }
        Ok(expr)
    }

    /// parse_ternary 解析三元条件表达式 `cond ? then : else`（右结合）。
    ///
    /// 优先级介于赋值与空合并之间（对齐 C/JS：三元优先级低于 ??）。
    /// then 分支用 parse_assign 解析（允许 `c ? x = 1 : y`，虽然不常见）；
    /// else 分支递归 parse_ternary，实现链式 `a?b:c?d:e` → `a?b:(c?d:e)`。
    /// then 内部遇到 `:` 自然停止（parse_assign 不消费 Colon），由本方法 expect 消费。
    fn parse_ternary(&mut self) -> Result<Expr, ParseError> {
        let cond = self.parse_nullcoal()?;
        if self.check(TokenKind::Question) {
            let tok = self.advance();
            let then = self.parse_assign()?;
            self.expect(TokenKind::Colon, "':' (三元运算符的 else 分支)")?;
            let else_ = self.parse_ternary()?;
            return Ok(Expr::Ternary {
                tok,
                cond: Box::new(cond),
                then: Box::new(then),
                else_: Box::new(else_),
            });
        }
        Ok(cond)
    }

    /// parse_nullcoal 解析空合并表达式 `a ?? b ?? c`（左结合）。
    ///
    /// 优先级低于 or、高于赋值。左操作数由 parse_or 解析，右侧递归 parse_or。
    fn parse_nullcoal(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_or()?;
        while self.check(TokenKind::NullCoal) {
            let tok = self.advance();
            let right = self.parse_or()?;
            left = Expr::BinaryExpr { tok, op: BinaryOp::NullCoal, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_and()?;
        // 同时接受关键字 or 与符号 ||（二者等价）
        while self.check(TokenKind::Or) || self.check(TokenKind::OrOr) {
            let tok = self.advance();
            let right = self.parse_and()?;
            left = Expr::BinaryExpr { tok, op: BinaryOp::Or, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bitor()?;
        // 同时接受关键字 and 与符号 &&（二者等价）
        while self.check(TokenKind::And) || self.check(TokenKind::AndAnd) {
            let tok = self.advance();
            let right = self.parse_bitor()?;
            left = Expr::BinaryExpr { tok, op: BinaryOp::And, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    /// parse_bitor 按位或 |（优先级低于 and，对齐 C）。
    fn parse_bitor(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bitxor()?;
        while self.check(TokenKind::Pipe) {
            let tok = self.advance();
            let right = self.parse_bitxor()?;
            left = Expr::BinaryExpr { tok, op: BinaryOp::BitOr, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    /// parse_bitxor 按位异或 ^。
    fn parse_bitxor(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bitand()?;
        while self.check(TokenKind::Caret) {
            let tok = self.advance();
            let right = self.parse_bitand()?;
            left = Expr::BinaryExpr { tok, op: BinaryOp::BitXor, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    /// parse_bitand 按位与 &（优先级高于 ^ 和 |，但低于比较，对齐 C）。
    fn parse_bitand(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_equality()?;
        while self.check(TokenKind::Amp) {
            let tok = self.advance();
            let right = self.parse_equality()?;
            left = Expr::BinaryExpr { tok, op: BinaryOp::BitAnd, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_equality(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_comparison()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Eq => BinaryOp::Eq,
                TokenKind::Neq => BinaryOp::Neq,
                _ => break,
            };
            let tok = self.advance();
            let right = self.parse_comparison()?;
            left = Expr::BinaryExpr { tok, op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_shift()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::LT => BinaryOp::LT,
                TokenKind::LE => BinaryOp::LE,
                TokenKind::GT => BinaryOp::GT,
                TokenKind::GE => BinaryOp::GE,
                _ => break,
            };
            let tok = self.advance();
            let right = self.parse_shift()?;
            left = Expr::BinaryExpr { tok, op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    /// parse_shift 移位 << >>（优先级低于比较、高于加减，对齐 C）。
    fn parse_shift(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_additive()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Shl => BinaryOp::BitShl,
                TokenKind::Shr => BinaryOp::BitShr,
                _ => break,
            };
            let tok = self.advance();
            let right = self.parse_additive()?;
            left = Expr::BinaryExpr { tok, op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Sub,
                _ => break,
            };
            let tok = self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr::BinaryExpr { tok, op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                TokenKind::Percent => BinaryOp::Mod,
                _ => break,
            };
            let tok = self.advance();
            let right = self.parse_unary()?;
            left = Expr::BinaryExpr { tok, op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        match self.peek().kind {
            TokenKind::Minus => {
                let tok = self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr::UnaryExpr { tok, op: UnaryOp::Neg, operand: Box::new(operand) })
            }
            TokenKind::Not => {
                let tok = self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr::UnaryExpr { tok, op: UnaryOp::Not, operand: Box::new(operand) })
            }
            TokenKind::Tilde => {
                let tok = self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr::UnaryExpr { tok, op: UnaryOp::BitNot, operand: Box::new(operand) })
            }
            // 前缀 ++ / --
            TokenKind::Plus2 => {
                let tok = self.advance();
                let operand = self.parse_unary()?;
                let target = expr_to_target(operand);
                Ok(Expr::IncDec { tok, target, op: IncDecOp::Inc, prefix: true })
            }
            TokenKind::Minus2 => {
                let tok = self.advance();
                let operand = self.parse_unary()?;
                let target = expr_to_target(operand);
                Ok(Expr::IncDec { tok, target, op: IncDecOp::Dec, prefix: true })
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek().kind {
                TokenKind::LParen => {
                    let tok = self.advance();
                    let mut args = Vec::new();
                    while !self.check(TokenKind::RParen) {
                        // 支持 ...arr 展开调用
                        if self.check(TokenKind::Ellipsis) {
                            let spread_tok = self.advance();
                            let inner = self.parse_expr()?;
                            args.push(Expr::Spread { tok: spread_tok, expr: Box::new(inner) });
                        } else {
                            args.push(self.parse_expr()?);
                        }
                        if !self.match_token(TokenKind::Comma) { break; }
                    }
                    self.expect(TokenKind::RParen, "')'")?;
                    expr = Expr::CallExpr { tok, callee: Box::new(expr), args };
                }
                TokenKind::LBracket => {
                    let tok = self.advance();
                    // 区分切片 [low:high] 与单索引 [index]：
                    //   紧跟 : → 切片，low=None
                    //   解析表达式后遇 : → 切片，low=Some
                    //   解析表达式后遇 ] → 单索引
                    if self.check(TokenKind::Colon) {
                        // [:high] 或 [:]
                        self.advance(); // 消费 :
                        let high = if self.check(TokenKind::RBracket) {
                            None
                        } else {
                            Some(Box::new(self.parse_expr()?))
                        };
                        self.expect(TokenKind::RBracket, "']'")?;
                        expr = Expr::SliceExpr {
                            tok,
                            obj: Box::new(expr),
                            low: None,
                            high,
                        };
                    } else {
                        let first = self.parse_expr()?;
                        if self.check(TokenKind::Colon) {
                            // [low:high] 或 [low:]
                            self.advance(); // 消费 :
                            let high = if self.check(TokenKind::RBracket) {
                                None
                            } else {
                                Some(Box::new(self.parse_expr()?))
                            };
                            self.expect(TokenKind::RBracket, "']'")?;
                            expr = Expr::SliceExpr {
                                tok,
                                obj: Box::new(expr),
                                low: Some(Box::new(first)),
                                high,
                            };
                        } else {
                            // 单索引 [index]
                            self.expect(TokenKind::RBracket, "']'")?;
                            expr = Expr::IndexExpr {
                                tok,
                                obj: Box::new(expr),
                                index: Box::new(first),
                            };
                        }
                    }
                }
                TokenKind::Dot => {
                    let tok = self.advance();
                    let name = self.expect(TokenKind::Ident, "成员名")?.value;
                    expr = Expr::MemberExpr { tok, obj: Box::new(expr), name };
                }
                // 后缀 ++ / --（返回旧值）
                TokenKind::Plus2 => {
                    let tok = self.advance();
                    let target = expr_to_target(expr);
                    expr = Expr::IncDec { tok, target, op: IncDecOp::Inc, prefix: false };
                }
                TokenKind::Minus2 => {
                    let tok = self.advance();
                    let target = expr_to_target(expr);
                    expr = Expr::IncDec { tok, target, op: IncDecOp::Dec, prefix: false };
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let tok = self.peek().clone();
        match tok.kind {
            TokenKind::Int => {
                self.advance();
                let value = parse_int_literal(&tok.value).map_err(|_| ParseError {
                    msg: format!("invalid integer literal: {} (可能原因：数值超出 i64 范围)", tok.value),
                    line: tok.line,
                })?;
                Ok(Expr::IntLit { tok, value })
            }
            TokenKind::Float => {
                self.advance();
                // 剔除下划线分隔符（如 1_000.5）
                let cleaned: String = tok.value.chars().filter(|c| *c != '_').collect();
                let value = cleaned.parse::<f64>().map_err(|_| ParseError {
                    msg: format!("invalid float literal: {}", tok.value), line: tok.line,
                })?;
                Ok(Expr::FloatLit { tok, value })
            }
            TokenKind::String | TokenKind::RawString => {
                self.advance();
                let value = tok.value.clone();
                Ok(Expr::StringLit { tok, value })
            }
            TokenKind::True => { self.advance(); Ok(Expr::BoolLit { tok, value: true }) }
            TokenKind::False => { self.advance(); Ok(Expr::BoolLit { tok, value: false }) }
            TokenKind::Undefined => { self.advance(); Ok(Expr::UndefinedLit { tok }) }
            TokenKind::Ident => { self.advance(); let name = tok.value.clone(); Ok(Expr::Ident { tok, name }) }
            TokenKind::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(TokenKind::RParen, "')'")?;
                Ok(expr)
            }
            TokenKind::LBracket => {
                self.advance();
                let mut elems = Vec::new();
                while !self.check(TokenKind::RBracket) {
                    elems.push(self.parse_expr()?);
                    if !self.match_token(TokenKind::Comma) { break; }
                }
                self.expect(TokenKind::RBracket, "']'")?;
                Ok(Expr::ArrayLit { tok, elems })
            }
            TokenKind::LBrace => {
                self.advance();
                let mut pairs = Vec::new();
                while !self.check(TokenKind::RBrace) {
                    let key = match self.peek().kind {
                        TokenKind::Ident => self.advance().value,
                        TokenKind::String => self.advance().value,
                        _ => return Err(self.err("对象键必须是标识符或字符串")),
                    };
                    self.expect(TokenKind::Colon, "':'")?;
                    let val = self.parse_expr()?;
                    pairs.push((key, val));
                    if !self.match_token(TokenKind::Comma) { break; }
                }
                self.expect(TokenKind::RBrace, "'}'")?;
                Ok(Expr::MapLit { tok, pairs })
            }
            TokenKind::Func => {
                self.advance();
                let func = self.parse_func_lit_body(tok, None)?;
                Ok(Expr::FuncLit { tok: func.tok.clone(), func })
            }
            _ => Err(self.err(format!("意外的 Token: {:?} (可能原因：表达式语法错误)", tok.kind))),
        }
    }
}

/// expr_to_target 将表达式转为赋值目标。
fn expr_to_target(expr: Expr) -> AssignTarget {
    match expr {
        Expr::Ident { name, .. } => AssignTarget::Name(name),
        Expr::IndexExpr { obj, index, .. } => AssignTarget::Index { obj, index },
        Expr::MemberExpr { obj, name, .. } => AssignTarget::Member { obj, name },
        _ => AssignTarget::Name(String::new()),
    }
}

/// compound_assign_op 将复合赋值 token 映射为对应的二元运算符。
/// 非复合赋值 token 返回 None。
fn compound_assign_op(kind: TokenKind) -> Option<BinaryOp> {
    match kind {
        TokenKind::PlusAssign => Some(BinaryOp::Add),
        TokenKind::MinusAssign => Some(BinaryOp::Sub),
        TokenKind::StarAssign => Some(BinaryOp::Mul),
        TokenKind::SlashAssign => Some(BinaryOp::Div),
        TokenKind::PercentAssign => Some(BinaryOp::Mod),
        TokenKind::NullCoalAssign => Some(BinaryOp::NullCoal),
        TokenKind::AmpAssign => Some(BinaryOp::BitAnd),
        TokenKind::PipeAssign => Some(BinaryOp::BitOr),
        TokenKind::CaretAssign => Some(BinaryOp::BitXor),
        TokenKind::ShlAssign => Some(BinaryOp::BitShl),
        TokenKind::ShrAssign => Some(BinaryOp::BitShr),
        _ => None,
    }
}

/// parse_program 便捷函数。
pub fn parse_program(tokens: Vec<Token>, file: &str) -> Result<Program, ParseError> {
    let mut p = Parser::new(tokens, file);
    p.parse()
}

/// parse_int_literal 解析整数字面量，支持十进制、0x 十六进制、0o 八进制、0b 二进制。
///
/// lexer 产出的 Int token value 保留原始前缀（如 "0xFF"、"0b1010"），
/// 故此处按前缀分发到 from_str_radix。十进制直接用 i64::from_str。
/// 下划线分隔符（如 1_000）也被剔除以便书写大数。
fn parse_int_literal(s: &str) -> Result<i64, std::num::ParseIntError> {
    // 剔除下划线分隔符（数字字面量中的 _ 无意义，如 1_000_000）
    let cleaned: String = s.chars().filter(|c| *c != '_').collect();
    if let Some(rest) = cleaned.strip_prefix("0x").or_else(|| cleaned.strip_prefix("0X")) {
        return i64::from_str_radix(rest, 16);
    }
    if let Some(rest) = cleaned.strip_prefix("0o").or_else(|| cleaned.strip_prefix("0O")) {
        return i64::from_str_radix(rest, 8);
    }
    if let Some(rest) = cleaned.strip_prefix("0b").or_else(|| cleaned.strip_prefix("0B")) {
        return i64::from_str_radix(rest, 2);
    }
    cleaned.parse::<i64>()
}
