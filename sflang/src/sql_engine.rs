//! sql_engine.rs — 内置 SQL 子集引擎（用于 csv/excel 数据库）
//!
//! 支持 SELECT 子集：SELECT 列 FROM 表 [WHERE 条件] [GROUP BY 列] [ORDER BY 列] [LIMIT n] [OFFSET n]
//! 聚合函数：COUNT(*) / COUNT(col) / SUM(col) / AVG(col) / MIN(col) / MAX(col)
//! WHERE 支持：= != < > <= >= LIKE AND OR NOT
//! 列引用：列名 或 列序号（0, 1, 2...）
//!
//! 纯标准库实现，无第三方依赖。解析为 AST 后在内存表上执行。

use crate::value::Value;

// ============ 内存表 ============

/// MemTable 内存表（csv/excel 导入后）。
#[derive(Debug, Clone)]
pub struct MemTable {
    /// name 表名（csv 文件名或 excel sheet 名）。
    pub name: String,
    /// columns 列名（来自首行）。
    pub columns: Vec<String>,
    /// rows 数据行（已类型推断）。
    pub rows: Vec<Vec<Value>>,
}

impl MemTable {
    /// resolve_col 将列名或列序号解析为列索引。
    /// 先按列名精确匹配，找不到再尝试纯数字序号。
    pub fn resolve_col(&self, name: &str) -> Option<usize> {
        // 先按列名匹配
        if let Some(idx) = self.columns.iter().position(|c| c == name) {
            return Some(idx);
        }
        // 再按列序号（纯数字）
        if let Ok(n) = name.parse::<usize>() {
            if n < self.columns.len() {
                return Some(n);
            }
        }
        None
    }
}

// ============ 词法分析 ============

/// SqlToken SQL token 类型。
#[derive(Debug, Clone, PartialEq)]
enum SqlToken {
    // 关键字
    Select, From, Where, Order, Group, By, Limit, Offset, Asc, Desc,
    And, Or, Not, Like, As,
    Count, Sum, Avg, Min, Max,
    // 字面量
    Ident(String),
    Number(f64),
    StringLit(String),
    // 运算符
    Eq, Neq, Lt, Gt, Lte, Gte,
    // 标点
    Star, Comma, LParen, RParen,
    Eof,
}

/// SqlLexer SQL 词法分析器。
struct SqlLexer {
    chars: Vec<char>,
    pos: usize,
}

impl SqlLexer {
    fn new(sql: &str) -> Self {
        SqlLexer { chars: sql.chars().collect(), pos: 0 }
    }

    fn peek(&self) -> Option<char> { self.chars.get(self.pos).copied() }
    fn advance(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() { self.pos += 1; }
        c
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() { self.advance(); } else { break; }
        }
    }

    /// tokenize 将 SQL 字符串转为 token 序列。
    fn tokenize(&mut self) -> Result<Vec<SqlToken>, String> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace();
            match self.peek() {
                None => { tokens.push(SqlToken::Eof); break; }
                Some(c) => {
                    match c {
                        '*' => { self.advance(); tokens.push(SqlToken::Star); }
                        ',' => { self.advance(); tokens.push(SqlToken::Comma); }
                        '(' => { self.advance(); tokens.push(SqlToken::LParen); }
                        ')' => { self.advance(); tokens.push(SqlToken::RParen); }
                        '=' => { self.advance(); tokens.push(SqlToken::Eq); }
                        '<' => {
                            self.advance();
                            if self.peek() == Some('=') { self.advance(); tokens.push(SqlToken::Lte); }
                            else if self.peek() == Some('>') { self.advance(); tokens.push(SqlToken::Neq); }
                            else { tokens.push(SqlToken::Lt); }
                        }
                        '>' => {
                            self.advance();
                            if self.peek() == Some('=') { self.advance(); tokens.push(SqlToken::Gte); }
                            else { tokens.push(SqlToken::Gt); }
                        }
                        '!' => {
                            self.advance();
                            if self.peek() == Some('=') { self.advance(); tokens.push(SqlToken::Neq); }
                            else { return Err("SQL 语法错误：'!' 后应为 '='".into()); }
                        }
                        '\'' => {
                            self.advance(); // 消费开头 '
                            let mut s = String::new();
                            loop {
                                match self.peek() {
                                    None => return Err("SQL 语法错误：字符串未闭合".into()),
                                    Some('\'') => {
                                        // 检查是否为转义的 '' → 字面量 '
                                        self.advance();
                                        if self.peek() == Some('\'') {
                                            s.push('\'');
                                            self.advance();
                                        } else {
                                            break;
                                        }
                                    }
                                    Some(ch) => { s.push(ch); self.advance(); }
                                }
                            }
                            tokens.push(SqlToken::StringLit(s));
                        }
                        '0'..='9' | '.' => {
                            let mut num = String::new();
                            let mut has_dot = c == '.';
                            num.push(c);
                            self.advance();
                            while let Some(ch) = self.peek() {
                                if ch.is_ascii_digit() {
                                    num.push(ch);
                                    self.advance();
                                } else if ch == '.' && !has_dot {
                                    has_dot = true;
                                    num.push(ch);
                                    self.advance();
                                } else {
                                    break;
                                }
                            }
                            let n: f64 = num.parse().map_err(|_| format!("SQL 语法错误：无效数字 '{}'", num))?;
                            tokens.push(SqlToken::Number(n));
                        }
                        _ if c.is_alphabetic() || c == '_' => {
                            let mut word = String::new();
                            while let Some(ch) = self.peek() {
                                if ch.is_alphanumeric() || ch == '_' || ch == '.' {
                                    word.push(ch);
                                    self.advance();
                                } else {
                                    break;
                                }
                            }
                            // 关键字匹配（大小写不敏感）
                            let upper = word.to_uppercase();
                            let token = match upper.as_str() {
                                "SELECT" => SqlToken::Select,
                                "FROM" => SqlToken::From,
                                "WHERE" => SqlToken::Where,
                                "ORDER" => SqlToken::Order,
                                "GROUP" => SqlToken::Group,
                                "BY" => SqlToken::By,
                                "LIMIT" => SqlToken::Limit,
                                "OFFSET" => SqlToken::Offset,
                                "ASC" => SqlToken::Asc,
                                "DESC" => SqlToken::Desc,
                                "AND" => SqlToken::And,
                                "OR" => SqlToken::Or,
                                "NOT" => SqlToken::Not,
                                "LIKE" => SqlToken::Like,
                                "AS" => SqlToken::As,
                                "COUNT" => SqlToken::Count,
                                "SUM" => SqlToken::Sum,
                                "AVG" => SqlToken::Avg,
                                "MIN" => SqlToken::Min,
                                "MAX" => SqlToken::Max,
                                _ => SqlToken::Ident(word),
                            };
                            tokens.push(token);
                        }
                        _ => return Err(format!("SQL 语法错误：意外字符 '{}'", c)),
                    }
                }
            }
        }
        Ok(tokens)
    }
}

// ============ AST ============

/// CmpOp 比较运算符。
#[derive(Debug, Clone, Copy)]
enum CmpOp { Eq, Neq, Lt, Gt, Lte, Gte }

/// AggFunc 聚合函数类型。
#[derive(Debug, Clone, Copy, PartialEq)]
enum AggFunc { Count, Sum, Avg, Min, Max }

/// SelCol SELECT 子句的列表达式。
#[derive(Debug, Clone)]
enum SelCol {
    /// Star 通配符 *。
    Star,
    /// Column 列名或列序号。output_name 为输出列名（AS 别名或原列名）。
    Column { name: String, output_name: String },
    /// Agg 聚合函数。
    Agg { func: AggFunc, arg: Box<SelCol>, output_name: String },
}

/// WhereExpr WHERE 条件表达式。
#[derive(Debug, Clone)]
enum WhereExpr {
    /// Compare 列与常量比较。
    Compare { col: String, op: CmpOp, val: Value },
    /// Like 列与模式匹配（% 通配）。
    Like { col: String, pattern: String, negate: bool },
    /// And 逻辑与。
    And(Box<WhereExpr>, Box<WhereExpr>),
    /// Or 逻辑或。
    Or(Box<WhereExpr>, Box<WhereExpr>),
    /// Not 逻辑非。
    Not(Box<WhereExpr>),
}

/// SelectStmt 解析后的 SELECT 语句。
#[derive(Debug, Clone)]
struct SelectStmt {
    pub columns: Vec<SelCol>,
    pub from_table: String,
    pub where_clause: Option<WhereExpr>,
    pub group_by: Vec<String>,
    pub order_by: Vec<(String, bool)>, // (列名, is_desc)
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

// ============ 语法分析 ============

/// SqlParser SQL 语法分析器。
struct SqlParser {
    tokens: Vec<SqlToken>,
    pos: usize,
}

impl SqlParser {
    fn new(tokens: Vec<SqlToken>) -> Self {
        SqlParser { tokens, pos: 0 }
    }

    fn peek(&self) -> &SqlToken { &self.tokens[self.pos] }
    fn advance(&mut self) -> SqlToken {
        let t = self.tokens[self.pos].clone();
        if self.pos + 1 < self.tokens.len() { self.pos += 1; }
        t
    }
    fn expect(&mut self, expected: &SqlToken, what: &str) -> Result<(), String> {
        if self.peek() == expected {
            self.advance();
            Ok(())
        } else {
            Err(format!("SQL 语法错误：期望 {}，得到 {:?}", what, self.peek()))
        }
    }

    /// parse 解析完整 SELECT 语句。
    fn parse(&mut self) -> Result<SelectStmt, String> {
        self.expect(&SqlToken::Select, "SELECT")?;

        // 解析列列表
        let columns = self.parse_select_list()?;

        // FROM
        self.expect(&SqlToken::From, "FROM")?;
        let from_table = match self.advance() {
            SqlToken::Ident(s) => s,
            other => return Err(format!("SQL 语法错误：FROM 后应为表名，得到 {:?}", other)),
        };

        // 可选子句
        let mut where_clause = None;
        let mut group_by = Vec::new();
        let mut order_by = Vec::new();
        let mut limit = None;
        let mut offset = None;

        loop {
            match self.peek() {
                SqlToken::Where => {
                    self.advance();
                    where_clause = Some(self.parse_where_expr()?);
                }
                SqlToken::Group => {
                    self.advance();
                    self.expect(&SqlToken::By, "GROUP BY")?;
                    group_by = self.parse_col_list()?;
                }
                SqlToken::Order => {
                    self.advance();
                    self.expect(&SqlToken::By, "ORDER BY")?;
                    order_by = self.parse_order_list()?;
                }
                SqlToken::Limit => {
                    self.advance();
                    limit = Some(self.parse_number_as_usize()?);
                }
                SqlToken::Offset => {
                    self.advance();
                    offset = Some(self.parse_number_as_usize()?);
                }
                SqlToken::Eof => break,
                _ => break,
            }
        }

        Ok(SelectStmt { columns, from_table, where_clause, group_by, order_by, limit, offset })
    }

    /// parse_select_list 解析 SELECT 列列表。
    fn parse_select_list(&mut self) -> Result<Vec<SelCol>, String> {
        let mut cols = Vec::new();
        loop {
            cols.push(self.parse_select_col()?);
            if *self.peek() == SqlToken::Comma {
                self.advance();
            } else {
                break;
            }
        }
        Ok(cols)
    }

    /// parse_select_col 解析单个 SELECT 列表达式。
    fn parse_select_col(&mut self) -> Result<SelCol, String> {
        match self.peek().clone() {
            SqlToken::Star => {
                self.advance();
                Ok(SelCol::Star)
            }
            SqlToken::Count | SqlToken::Sum | SqlToken::Avg | SqlToken::Min | SqlToken::Max => {
                let func = match self.advance() {
                    SqlToken::Count => AggFunc::Count,
                    SqlToken::Sum => AggFunc::Sum,
                    SqlToken::Avg => AggFunc::Avg,
                    SqlToken::Min => AggFunc::Min,
                    SqlToken::Max => AggFunc::Max,
                    _ => unreachable!(),
                };
                self.expect(&SqlToken::LParen, "(")?;
                let arg = if *self.peek() == SqlToken::Star {
                    self.advance();
                    Box::new(SelCol::Star)
                } else {
                    let name = self.parse_col_name()?;
                    Box::new(SelCol::Column { name: name.clone(), output_name: name })
                };
                self.expect(&SqlToken::RParen, ")")?;

                // AS 别名（可选）
                let output_name = self.parse_optional_alias(&format!("{:?}", func));

                Ok(SelCol::Agg { func, arg, output_name })
            }
            SqlToken::Ident(name) => {
                self.advance();
                let output_name = self.parse_optional_alias(&name);
                Ok(SelCol::Column { name, output_name })
            }
            SqlToken::Number(n) => {
                // 数字列序号
                self.advance();
                let name = format!("{}", n as i64);
                let output_name = self.parse_optional_alias(&name);
                Ok(SelCol::Column { name, output_name })
            }
            other => Err(format!("SQL 语法错误：SELECT 列处意外 {:?}（应为列名、* 或聚合函数）", other)),
        }
    }

    /// parse_optional_alias 解析可选的 AS 别名。
    fn parse_optional_alias(&mut self, default: &str) -> String {
        if *self.peek() == SqlToken::As {
            self.advance();
            if let SqlToken::Ident(s) = self.peek().clone() {
                self.advance();
                return s;
            }
        } else if let SqlToken::Ident(s) = self.peek().clone() {
            // 隐式别名（无 AS 关键字）— 但要避免吞掉 FROM 等关键字
            let upper = s.to_uppercase();
            if !matches!(upper.as_str(), "FROM" | "WHERE" | "ORDER" | "GROUP" | "LIMIT" | "OFFSET" | "AND" | "OR") {
                self.advance();
                return s;
            }
        }
        default.to_string()
    }

    /// parse_col_name 解析列名（标识符或数字序号）。
    fn parse_col_name(&mut self) -> Result<String, String> {
        match self.advance() {
            SqlToken::Ident(s) => Ok(s),
            SqlToken::Number(n) => Ok(format!("{}", n as i64)),
            other => Err(format!("SQL 语法错误：期望列名，得到 {:?}", other)),
        }
    }

    /// parse_col_list 解析逗号分隔的列名列表。
    fn parse_col_list(&mut self) -> Result<Vec<String>, String> {
        let mut cols = Vec::new();
        loop {
            cols.push(self.parse_col_name()?);
            if *self.peek() == SqlToken::Comma { self.advance(); } else { break; }
        }
        Ok(cols)
    }

    /// parse_order_list 解析 ORDER BY 列列表（含 ASC/DESC）。
    fn parse_order_list(&mut self) -> Result<Vec<(String, bool)>, String> {
        let mut cols = Vec::new();
        loop {
            let name = self.parse_col_name()?;
            let desc = match self.peek() {
                SqlToken::Desc => { self.advance(); true }
                SqlToken::Asc => { self.advance(); false }
                _ => false,
            };
            cols.push((name, desc));
            if *self.peek() == SqlToken::Comma { self.advance(); } else { break; }
        }
        Ok(cols)
    }

    /// parse_number_as_usize 解析数字为 usize。
    fn parse_number_as_usize(&mut self) -> Result<usize, String> {
        match self.advance() {
            SqlToken::Number(n) => Ok(n as usize),
            other => Err(format!("SQL 语法错误：期望数字，得到 {:?}", other)),
        }
    }

    // ---- WHERE 表达式解析（优先级：OR < AND < NOT < 比较/LIKE）----

    fn parse_where_expr(&mut self) -> Result<WhereExpr, String> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<WhereExpr, String> {
        let mut left = self.parse_and()?;
        while *self.peek() == SqlToken::Or {
            self.advance();
            let right = self.parse_and()?;
            left = WhereExpr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<WhereExpr, String> {
        let mut left = self.parse_not()?;
        while *self.peek() == SqlToken::And {
            self.advance();
            let right = self.parse_not()?;
            left = WhereExpr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<WhereExpr, String> {
        if *self.peek() == SqlToken::Not {
            self.advance();
            let inner = self.parse_not()?;
            return Ok(WhereExpr::Not(Box::new(inner)));
        }
        self.parse_predicate()
    }

    fn parse_predicate(&mut self) -> Result<WhereExpr, String> {
        // 可能用括号分组
        if *self.peek() == SqlToken::LParen {
            self.advance();
            let expr = self.parse_where_expr()?;
            self.expect(&SqlToken::RParen, ")")?;
            return Ok(expr);
        }

        let col = self.parse_col_name()?;

        match self.peek().clone() {
            SqlToken::Eq | SqlToken::Neq | SqlToken::Lt | SqlToken::Gt | SqlToken::Lte | SqlToken::Gte => {
                let op = match self.advance() {
                    SqlToken::Eq => CmpOp::Eq,
                    SqlToken::Neq => CmpOp::Neq,
                    SqlToken::Lt => CmpOp::Lt,
                    SqlToken::Gt => CmpOp::Gt,
                    SqlToken::Lte => CmpOp::Lte,
                    SqlToken::Gte => CmpOp::Gte,
                    _ => unreachable!(),
                };
                let val = self.parse_literal()?;
                Ok(WhereExpr::Compare { col, op, val })
            }
            SqlToken::Like => {
                self.advance();
                match self.advance() {
                    SqlToken::StringLit(pattern) => Ok(WhereExpr::Like { col, pattern, negate: false }),
                    other => Err(format!("SQL 语法错误：LIKE 后应为字符串，得到 {:?}", other)),
                }
            }
            SqlToken::Not => {
                self.advance();
                if *self.peek() == SqlToken::Like {
                    self.advance();
                    match self.advance() {
                        SqlToken::StringLit(pattern) => Ok(WhereExpr::Like { col, pattern, negate: true }),
                        other => Err(format!("SQL 语法错误：NOT LIKE 后应为字符串，得到 {:?}", other)),
                    }
                } else {
                    Err("SQL 语法错误：NOT 后应为 LIKE".into())
                }
            }
            other => Err(format!("SQL 语法错误：WHERE 条件处意外 {:?}（应为运算符 = != < > <= >= LIKE）", other)),
        }
    }

    /// parse_literal 解析字面量值。
    fn parse_literal(&mut self) -> Result<Value, String> {
        match self.advance() {
            SqlToken::Number(n) => {
                if n == n.trunc() && n.is_finite() && n.abs() < 9.2e18 {
                    Ok(Value::Int(n as i64))
                } else {
                    Ok(Value::Float(n))
                }
            }
            SqlToken::StringLit(s) => Ok(Value::str_from(s)),
            SqlToken::Ident(s) => {
                // true/false/null
                match s.to_uppercase().as_str() {
                    "TRUE" => Ok(Value::Bool(true)),
                    "FALSE" => Ok(Value::Bool(false)),
                    "NULL" => Ok(Value::Undefined),
                    _ => Ok(Value::str_from(s)),
                }
            }
            other => Err(format!("SQL 语法错误：期望值字面量，得到 {:?}", other)),
        }
    }
}

// ============ 执行引擎 ============

/// execute 执行 SELECT 语句，返回 (列名, 数据行)。
fn execute(stmt: &SelectStmt, tables: &[MemTable]) -> Result<(Vec<String>, Vec<Vec<Value>>), String> {
    // 1. FROM：找到表
    let table = tables.iter().find(|t| t.name.eq_ignore_ascii_case(&stmt.from_table))
        .ok_or_else(|| format!("SQL 错误：找不到表 '{}'（可用表：{}）",
            stmt.from_table, tables.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", ")))?;

    // 2. WHERE：过滤行
    let filtered_rows: Vec<&Vec<Value>> = match &stmt.where_clause {
        Some(expr) => table.rows.iter().filter(|row| eval_where(expr, row, table)).collect(),
        None => table.rows.iter().collect(),
    };

    // 3. 判断是否有聚合
    let has_agg = stmt.columns.iter().any(|c| matches!(c, SelCol::Agg { .. }));

    if has_agg || !stmt.group_by.is_empty() {
        // 聚合 + GROUP BY 路径
        execute_aggregate(stmt, table, &filtered_rows)
    } else {
        // 普通投影路径
        execute_projection(stmt, table, &filtered_rows)
    }
}

/// execute_projection 执行普通列投影（无聚合）。
fn execute_projection(stmt: &SelectStmt, table: &MemTable, rows: &[&Vec<Value>]) -> Result<(Vec<String>, Vec<Vec<Value>>), String> {
    // 确定输出列
    let mut out_cols: Vec<(String, usize)> = Vec::new(); // (output_name, col_index)
    let mut select_star = false;

    for col in &stmt.columns {
        match col {
            SelCol::Star => {
                select_star = true;
                for (i, name) in table.columns.iter().enumerate() {
                    out_cols.push((name.clone(), i));
                }
            }
            SelCol::Column { name, output_name } => {
                let idx = table.resolve_col(name)
                    .ok_or_else(|| format!("SQL 错误：找不到列 '{}'（可用列：{} 或列序号 0-{}）",
                        name, table.columns.join(", "), table.columns.len() - 1))?;
                out_cols.push((output_name.clone(), idx));
            }
            SelCol::Agg { .. } => {
                // 有聚合时不应走到这里，但防御性处理
                return Err("SQL 错误：聚合函数不能与普通列混用（除非有 GROUP BY）".into());
            }
        }
    }

    let col_names: Vec<String> = out_cols.iter().map(|(n, _)| n.clone()).collect();
    let _ = select_star;

    // ORDER BY：在投影前对原始行排序（ORDER BY 可引用不在 SELECT 中的列）
    // 用行索引排序，再按索引顺序投影，保持排序结果。
    let mut indices: Vec<usize> = (0..rows.len()).collect();
    if !stmt.order_by.is_empty() {
        order_indices(&mut indices, rows, table, &stmt.order_by)?;
    }

    // 投影（按排序后的索引顺序）
    let mut result: Vec<Vec<Value>> = Vec::new();
    for &idx in &indices {
        let row = rows[idx];
        let out_row: Vec<Value> = out_cols.iter().map(|(_, ci)| row.get(*ci).cloned().unwrap_or(Value::Undefined)).collect();
        result.push(out_row);
    }

    // LIMIT / OFFSET
    apply_limit_offset(&mut result, stmt.limit, stmt.offset);

    Ok((col_names, result))
}

/// execute_aggregate 执行聚合查询（含 GROUP BY）。
fn execute_aggregate(stmt: &SelectStmt, table: &MemTable, rows: &[&Vec<Value>]) -> Result<(Vec<String>, Vec<Vec<Value>>), String> {
    // 分组
    let groups: Vec<Vec<usize>> = if stmt.group_by.is_empty() {
        // 无 GROUP BY：全部行一组
        vec![(0..rows.len()).collect()]
    } else {
        // 按 GROUP BY 列分组
        let mut group_map: std::collections::HashMap<String, Vec<usize>> = std::collections::HashMap::new();
        let mut group_order: Vec<String> = Vec::new();

        for (row_idx, row) in rows.iter().enumerate() {
            let mut key_parts: Vec<String> = Vec::new();
            for gcol in &stmt.group_by {
                let idx = table.resolve_col(gcol)
                    .ok_or_else(|| format!("SQL 错误：GROUP BY 找不到列 '{}'", gcol))?;
                key_parts.push(row.get(idx).map(|v| v.to_str()).unwrap_or_default());
            }
            let key = key_parts.join("\x01");
            if !group_map.contains_key(&key) {
                group_order.push(key.clone());
            }
            group_map.entry(key).or_default().push(row_idx);
        }
        group_order.into_iter().filter_map(|k| group_map.get(&k).cloned()).collect()
    };

    // 确定输出列和聚合计算
    let mut out_col_names: Vec<String> = Vec::new();
    let mut out_specs: Vec<OutSpec> = Vec::new();

    for col in &stmt.columns {
        match col {
            SelCol::Star => return Err("SQL 错误：聚合查询中不支持 *（请明确列出列和聚合函数）".into()),
            SelCol::Column { name, output_name } => {
                let idx = table.resolve_col(name)
                    .ok_or_else(|| format!("SQL 错误：找不到列 '{}'", name))?;
                out_specs.push(OutSpec::GroupKey(idx));
                out_col_names.push(output_name.clone());
            }
            SelCol::Agg { func, arg, output_name } => {
                let arg_idx = match arg.as_ref() {
                    SelCol::Star => None, // COUNT(*)
                    SelCol::Column { name, .. } => {
                        Some(table.resolve_col(name)
                            .ok_or_else(|| format!("SQL 错误：聚合函数找不到列 '{}'", name))?)
                    }
                    _ => return Err("SQL 错误：聚合函数参数无效".into()),
                };
                out_specs.push(OutSpec::Agg(*func, arg_idx));
                out_col_names.push(output_name.clone());
            }
        }
    }

    // 计算每组的输出行
    let mut result: Vec<Vec<Value>> = Vec::new();
    for group in &groups {
        let mut out_row: Vec<Value> = Vec::new();
        for spec in &out_specs {
            let val = match spec {
                OutSpec::GroupKey(idx) => {
                    // 取组内第一行的该列
                    group.first()
                        .and_then(|&ri| rows[ri].get(*idx))
                        .cloned()
                        .unwrap_or(Value::Undefined)
                }
                OutSpec::Agg(func, arg_idx) => {
                    compute_aggregate(*func, *arg_idx, group, rows)?
                }
            };
            out_row.push(val);
        }
        result.push(out_row);
    }

    // ORDER BY（聚合后按输出列名排序）
    if !stmt.order_by.is_empty() {
        order_rows(&mut result, &out_col_names, table, &stmt.order_by)?;
    }

    apply_limit_offset(&mut result, stmt.limit, stmt.offset);

    Ok((out_col_names, result))
}

/// OutSpec 聚合查询的输出列规格。
enum OutSpec {
    GroupKey(usize),
    Agg(AggFunc, Option<usize>),
}

/// compute_aggregate 计算单个聚合值。
fn compute_aggregate(func: AggFunc, arg_idx: Option<usize>, group: &[usize], rows: &[&Vec<Value>]) -> Result<Value, String> {
    match func {
        AggFunc::Count => {
            if arg_idx.is_none() {
                // COUNT(*) = 行数
                Ok(Value::Int(group.len() as i64))
            } else {
                // COUNT(col) = 非 null 值数
                let idx = arg_idx.unwrap();
                let count = group.iter().filter(|&&ri| {
                    rows[ri].get(idx).map(|v| !matches!(v, Value::Undefined)).unwrap_or(false)
                }).count();
                Ok(Value::Int(count as i64))
            }
        }
        AggFunc::Sum => {
            let idx = arg_idx.ok_or("SUM 需要指定列")?;
            let mut sum = 0.0f64;
            let mut all_int = true;
            for &ri in group {
                if let Some(v) = rows[ri].get(idx) {
                    match v {
                        Value::Int(n) => sum += *n as f64,
                        Value::Float(f) => { sum += f; all_int = false; }
                        _ => {}
                    }
                }
            }
            if all_int { Ok(Value::Int(sum as i64)) } else { Ok(Value::Float(sum)) }
        }
        AggFunc::Avg => {
            let idx = arg_idx.ok_or("AVG 需要指定列")?;
            let mut sum = 0.0f64;
            let mut count = 0usize;
            for &ri in group {
                if let Some(v) = rows[ri].get(idx) {
                    match v.to_f64() {
                        Some(f) => { sum += f; count += 1; }
                        None => {}
                    }
                }
            }
            if count == 0 { Ok(Value::Undefined) } else { Ok(Value::Float(sum / count as f64)) }
        }
        AggFunc::Min => {
            let idx = arg_idx.ok_or("MIN 需要指定列")?;
            let mut min: Option<Value> = None;
            for &ri in group {
                if let Some(v) = rows[ri].get(idx) {
                    if matches!(v, Value::Undefined) { continue; }
                    match &min {
                        None => min = Some(v.clone()),
                        Some(cur) => if cmp_values(v, cur) == std::cmp::Ordering::Less { min = Some(v.clone()); }
                    }
                }
            }
            Ok(min.unwrap_or(Value::Undefined))
        }
        AggFunc::Max => {
            let idx = arg_idx.ok_or("MAX 需要指定列")?;
            let mut max: Option<Value> = None;
            for &ri in group {
                if let Some(v) = rows[ri].get(idx) {
                    if matches!(v, Value::Undefined) { continue; }
                    match &max {
                        None => max = Some(v.clone()),
                        Some(cur) => if cmp_values(v, cur) == std::cmp::Ordering::Greater { max = Some(v.clone()); }
                    }
                }
            }
            Ok(max.unwrap_or(Value::Undefined))
        }
    }
}

/// eval_where 递归求值 WHERE 表达式。
fn eval_where(expr: &WhereExpr, row: &Vec<Value>, table: &MemTable) -> bool {
    match expr {
        WhereExpr::Compare { col, op, val } => {
            let idx = match table.resolve_col(col) { Some(i) => i, None => return false };
            let cell = row.get(idx).unwrap_or(&Value::Undefined);
            eval_compare(cell, *op, val)
        }
        WhereExpr::Like { col, pattern, negate } => {
            let idx = match table.resolve_col(col) { Some(i) => i, None => return false };
            let cell = row.get(idx).unwrap_or(&Value::Undefined);
            let matched = like_match(&cell.to_str(), pattern);
            if *negate { !matched } else { matched }
        }
        WhereExpr::And(a, b) => eval_where(a, row, table) && eval_where(b, row, table),
        WhereExpr::Or(a, b) => eval_where(a, row, table) || eval_where(b, row, table),
        WhereExpr::Not(e) => !eval_where(e, row, table),
    }
}

/// eval_compare 比较两个值。
fn eval_compare(cell: &Value, op: CmpOp, val: &Value) -> bool {
    // undefined 与任何值比较都为 false（除了 != ）
    if matches!(cell, Value::Undefined) || matches!(val, Value::Undefined) {
        return matches!(op, CmpOp::Neq) && matches!(cell, Value::Undefined) != matches!(val, Value::Undefined);
    }
    match op {
        CmpOp::Eq => cell.equals(val),
        CmpOp::Neq => !cell.equals(val),
        _ => {
            let ord = cmp_values(cell, val);
            match op {
                CmpOp::Lt => ord == std::cmp::Ordering::Less,
                CmpOp::Gt => ord == std::cmp::Ordering::Greater,
                CmpOp::Lte => ord != std::cmp::Ordering::Greater,
                CmpOp::Gte => ord != std::cmp::Ordering::Less,
                _ => unreachable!(),
            }
        }
    }
}

/// cmp_values 通用值比较（数值用数值比，其他用字符串比）。
fn cmp_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    // 优先数值比较
    if let (Some(fa), Some(fb)) = (a.to_f64(), b.to_f64()) {
        return fa.partial_cmp(&fb).unwrap_or(Ordering::Equal);
    }
    // 回退到字符串比较
    a.to_str().cmp(&b.to_str())
}

/// like_match SQL LIKE 匹配（% 匹配任意字符序列）。
fn like_match(s: &str, pattern: &str) -> bool {
    // 将 SQL LIKE 模式转为正则式逻辑：简化为 % → .*，其他字符字面匹配
    // 不支持 _ 单字符通配（可后续加）
    let s_lower = s.to_lowercase();
    let p_lower = pattern.to_lowercase();
    like_match_inner(s_lower.as_bytes(), p_lower.as_bytes())
}

/// like_match_inner 递归 LIKE 匹配。
fn like_match_inner(s: &[u8], p: &[u8]) -> bool {
    let mut si = 0;
    let mut pi = 0;
    let mut star_pi = None;
    let mut star_si = 0;

    while si < s.len() {
        if pi < p.len() && p[pi] == b'%' {
            star_pi = Some(pi);
            star_si = si;
            pi += 1;
        } else if pi < p.len() && (p[pi] == s[si] || p[pi] == b'_') {
            si += 1;
            pi += 1;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            star_si += 1;
            si = star_si;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'%' { pi += 1; }
    pi == p.len()
}

/// order_indices 对原始行索引排序（ORDER BY 引用原始表的列，不受投影影响）。
fn order_indices(indices: &mut Vec<usize>, rows: &[&Vec<Value>], table: &MemTable, order_by: &[(String, bool)]) -> Result<(), String> {
    // 解析排序列为原表列索引
    let mut order_idxs: Vec<(usize, bool)> = Vec::new();
    for (col, desc) in order_by {
        let idx = table.resolve_col(col)
            .ok_or_else(|| format!("SQL 错误：ORDER BY 找不到列 '{}'", col))?;
        order_idxs.push((idx, *desc));
    }

    // 稳定排序（从最后一个排序列开始，逐列排）
    for (col_idx, desc) in order_idxs.iter().rev() {
        let col_idx = *col_idx;
        let desc = *desc;
        indices.sort_by(|&a, &b| {
            let va = rows[a].get(col_idx).unwrap_or(&Value::Undefined);
            let vb = rows[b].get(col_idx).unwrap_or(&Value::Undefined);
            let ord = cmp_values(va, vb);
            if desc { ord.reverse() } else { ord }
        });
    }
    Ok(())
}

/// order_rows 对结果行排序。
fn order_rows(rows: &mut Vec<Vec<Value>>, col_names: &[String], table: &MemTable, order_by: &[(String, bool)]) -> Result<(), String> {
    // 解析排序列的索引（先从输出列名找，再从原表列找）
    let mut order_idxs: Vec<(usize, bool)> = Vec::new();
    for (col, desc) in order_by {
        let idx = col_names.iter().position(|c| c == col)
            .or_else(|| table.resolve_col(col))
            .ok_or_else(|| format!("SQL 错误：ORDER BY 找不到列 '{}'", col))?;
        order_idxs.push((idx, *desc));
    }

    // 稳定排序（从最后一个排序列开始，逐列排）
    for (idx, desc) in order_idxs.iter().rev() {
        let idx = *idx;
        let desc = *desc;
        rows.sort_by(|a, b| {
            let va = a.get(idx).unwrap_or(&Value::Undefined);
            let vb = b.get(idx).unwrap_or(&Value::Undefined);
            let ord = cmp_values(va, vb);
            if desc { ord.reverse() } else { ord }
        });
    }
    Ok(())
}

/// apply_limit_offset 应用 LIMIT 和 OFFSET。
fn apply_limit_offset(rows: &mut Vec<Vec<Value>>, limit: Option<usize>, offset: Option<usize>) {
    if let Some(off) = offset {
        if off >= rows.len() {
            rows.clear();
            return;
        }
        rows.drain(0..off);
    }
    if let Some(lim) = limit {
        if rows.len() > lim {
            rows.truncate(lim);
        }
    }
}

// ============ 公共 API ============

/// parse_and_execute 解析 SQL 并执行，返回 (列名, 数据行)。
/// 这是 builtins_db.rs 调用的入口。
pub fn parse_and_execute(sql: &str, tables: &[MemTable]) -> Result<(Vec<String>, Vec<Vec<Value>>), String> {
    let tokens = SqlLexer::new(sql).tokenize()?;
    let stmt = SqlParser::new(tokens).parse()?;
    execute(&stmt, tables)
}

/// infer_cell CSV 单元格类型推断。
pub fn infer_cell(s: &str) -> Value {
    let t = s.trim();
    if t.is_empty() { return Value::str(""); }
    if let Ok(n) = t.parse::<i64>() { return Value::Int(n); }
    if let Ok(f) = t.parse::<f64>() {
        if f.is_finite() { return Value::Float(f); }
    }
    match t.to_lowercase().as_str() {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        "null" | "undefined" => return Value::Undefined,
        _ => {}
    }
    Value::str_from(s.to_string())
}

// ============ 单元测试 ============

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_table() -> MemTable {
        MemTable {
            name: "users".to_string(),
            columns: vec!["name".to_string(), "age".to_string(), "city".to_string()],
            rows: vec![
                vec![Value::str("Alice"), Value::Int(30), Value::str("Beijing")],
                vec![Value::str("Bob"), Value::Int(25), Value::str("Shanghai")],
                vec![Value::str("Carol"), Value::Int(35), Value::str("Beijing")],
                vec![Value::str("Dave"), Value::Int(20), Value::str("Shenzhen")],
            ],
        }
    }

    #[test]
    fn test_select_all() {
        let t = make_test_table();
        let (cols, rows) = parse_and_execute("SELECT * FROM users", &[t]).unwrap();
        assert_eq!(cols, vec!["name", "age", "city"]);
        assert_eq!(rows.len(), 4);
    }

    #[test]
    fn test_select_cols() {
        let t = make_test_table();
        let (cols, rows) = parse_and_execute("SELECT name, age FROM users", &[t]).unwrap();
        assert_eq!(cols, vec!["name", "age"]);
        assert_eq!(rows[0][0].to_str(), "Alice");
    }

    #[test]
    fn test_where() {
        let t = make_test_table();
        let (_, rows) = parse_and_execute("SELECT * FROM users WHERE age > 25", &[t]).unwrap();
        assert_eq!(rows.len(), 2); // Alice(30), Carol(35)
    }

    #[test]
    fn test_where_and() {
        let t = make_test_table();
        let (_, rows) = parse_and_execute("SELECT * FROM users WHERE age > 20 AND city = 'Beijing'", &[t]).unwrap();
        assert_eq!(rows.len(), 2); // Alice, Carol
    }

    #[test]
    fn test_order_by() {
        let t = make_test_table();
        let (_, rows) = parse_and_execute("SELECT name FROM users ORDER BY age DESC", &[t]).unwrap();
        assert_eq!(rows[0][0].to_str(), "Carol"); // age 35
        assert_eq!(rows[3][0].to_str(), "Dave");  // age 20
    }

    #[test]
    fn test_limit_offset() {
        let t = make_test_table();
        let (_, rows) = parse_and_execute("SELECT name FROM users ORDER BY name LIMIT 2 OFFSET 1", &[t]).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0].to_str(), "Bob");
    }

    #[test]
    fn test_count() {
        let t = make_test_table();
        let (cols, rows) = parse_and_execute("SELECT COUNT(*) AS cnt FROM users", &[t]).unwrap();
        assert_eq!(cols, vec!["cnt"]);
        assert_eq!(rows[0][0].to_int(), Some(4));
    }

    #[test]
    fn test_group_by() {
        let t = make_test_table();
        let (cols, rows) = parse_and_execute(
            "SELECT city, COUNT(*) AS cnt FROM users GROUP BY city", &[t]).unwrap();
        assert_eq!(cols, vec!["city", "cnt"]);
        assert_eq!(rows.len(), 3); // Beijing, Shanghai, Shenzhen
    }

    #[test]
    fn test_column_index() {
        let t = make_test_table();
        let (cols, rows) = parse_and_execute("SELECT 0, 1 FROM users WHERE 2 = 'Beijing'", &[t]).unwrap();
        assert_eq!(cols.len(), 2);
        assert_eq!(rows.len(), 2); // Beijing 有 2 人
        assert_eq!(rows[0][0].to_str(), "Alice"); // 列 0 = name
    }

    #[test]
    fn test_like() {
        let t = make_test_table();
        let (_, rows) = parse_and_execute("SELECT name FROM users WHERE name LIKE 'A%'", &[t]).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][0].to_str(), "Alice");
    }

    #[test]
    fn test_avg() {
        let t = make_test_table();
        let (_, rows) = parse_and_execute("SELECT AVG(age) AS avg_age FROM users", &[t]).unwrap();
        // (30+25+35+20)/4 = 27.5
        assert_eq!(rows[0][0].to_f64(), Some(27.5));
    }

    #[test]
    fn test_infer_cell() {
        assert_eq!(infer_cell("42").to_int(), Some(42));
        assert_eq!(infer_cell("3.14").to_f64(), Some(3.14));
        assert_eq!(infer_cell("true"), Value::Bool(true));
        assert_eq!(infer_cell("hello").to_str(), "hello");
        assert_eq!(infer_cell("").to_str(), "");
    }
}
