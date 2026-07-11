//! builtins_json.rs — JSON 编解码内置函数
//!
//! 设计要点（来自 AGENTS.md）：
//!   - 仅依赖 Rust 标准库（手写递归编解码器，不引入第三方）
//!   - 支持 undefined / bool / number(int/float) / string / array / object
//!     （注：JSON 协议的 null 与 Sflang 的 undefined 互转；JSON 的 "null" 字面量字符串不变）
//!   - 错误信息含字节偏移与可能原因，便于 AI 定位
//!
//! 函数列表：
//!   jsonEncode(v) — Value → JSON 字符串
//!   jsonDecode(s) — JSON 字符串 → Value

use std::sync::{Arc, Mutex};

use crate::builtins_helpers as bh;
use crate::value::Value;
use crate::vm::VM;

/// register 注册所有 JSON 内置函数到 VM。
pub fn register(vm: &mut VM) {
    vm.register_builtin("jsonEncode", bi_json_encode);
    vm.register_builtin("jsonDecode", bi_json_decode);
    vm.register_builtin("toJson", bi_json_encode);
    vm.register_builtin("fromJson", bi_json_decode);
    vm.register_builtin("getJsonNodeStr", bi_get_json_node_str);
    vm.register_builtin("getJsonNode", bi_get_json_node);
    vm.register_builtin("formatJson", bi_format_json);
    vm.register_builtin("compactJson", bi_compact_json);
    vm.register_builtin("getJsonNodeStrs", bi_get_json_node_strs);
}

// ---- 编码（Value → JSON 字符串）----

/// encode_value 递归编码 Value 到 JSON 字符串。
fn encode_value(v: &Value, out: &mut String) {
    match v {
        Value::Undefined => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => out.push_str(&i.to_string()),
        Value::Byte(b) => out.push_str(&b.to_string()),
        Value::Float(f) => {
            if f.is_nan() || f.is_infinite() {
                out.push_str("null"); // JSON 无 NaN/Infinity
            } else {
                out.push_str(&f.to_string());
            }
        }
        Value::Str(s) => encode_string(s.as_ref(), out),
        Value::Bytes(b) => {
            // 字节按数字数组编码
            out.push('[');
            for (i, byte) in b.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&byte.to_string());
            }
            out.push(']');
        }
        Value::ByteArray(b) => {
            // 可变字节序列同样按数字数组编码
            let snap = b.lock().unwrap().clone();
            out.push('[');
            for (i, byte) in snap.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&byte.to_string());
            }
            out.push(']');
        }
        Value::Array(a) => {
            out.push('[');
            // 克隆快照后释放锁，再递归（避免持锁死锁）
            let snapshot: Vec<Value> = a.lock().unwrap().clone();
            for (i, elem) in snapshot.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                encode_value(elem, out);
            }
            out.push(']');
        }
        Value::Object(o) => {
            out.push('{');
            let snapshot: Vec<(String, Value)> = o.lock().unwrap().snapshot();
            let mut first = true;
            for (k, val) in snapshot.iter() {
                if !first { out.push(','); }
                first = false;
                encode_string(k, out);
                out.push(':');
                encode_value(val, out);
            }
            out.push('}');
        }
        Value::Map(m) => {
            out.push('{');
            let snapshot: Vec<(String, Value)> = m.lock().unwrap().snapshot();
            let mut first = true;
            for (k, val) in snapshot.iter() {
                if !first { out.push(','); }
                first = false;
                encode_string(k, out);
                out.push(':');
                encode_value(val, out);
            }
            out.push('}');
        }
        Value::StringBuilder(sb) => encode_string(&sb.lock().unwrap(), out),
        Value::Func(_) | Value::Builtin(_) => {
            // 函数无 JSON 表示，编码为 null
            out.push_str("null");
        }
        Value::Error(e) => encode_string(&e.message, out),
        Value::Native(_) => out.push_str("null"),
        Value::BigInt(b) => out.push_str(&b.to_string_decimal()),
        // BigFloat 编码为 JSON 数字（字符串形式，JSON 无大数类型）
        Value::BigFloat(b) => out.push_str(&b.to_string()),
        // DateTime 编码为 ISO 8601 字符串（JSON 无日期类型）
        Value::DateTime(dt) => {
            encode_string(&dt.format("2006-01-02T15:04:05"), out);
        }
        // File 句柄无 JSON 表示，编码为 null
        Value::File(_) => out.push_str("null"),
        // HttpReq/HttpResp/WebSocket 无 JSON 表示，编码为 null
        Value::HttpReq(_) | Value::HttpResp(_) | Value::WebSocket(_) => out.push_str("null"),
    }
}

/// encode_string 编码字符串字面量（带转义）。
fn encode_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// bi_json_encode 将 Value 编码为 JSON 字符串。
fn bi_json_encode(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "jsonEncode")?;
    let mut out = String::new();
    encode_value(&args[0], &mut out);
    Ok(Value::str_from(out))
}

// ---- 解码（JSON 字符串 → Value）----

/// MAX_DEPTH JSON 解析最大嵌套深度（防恶意深层嵌套导致栈溢出）。
const MAX_DEPTH: usize = 200;

/// Decoder 递归下降 JSON 解析器，跟踪字节偏移用于错误定位。
pub struct Decoder<'a> {
    bytes: &'a [u8],
    pos: usize,
    /// depth 当前嵌套深度（对象/数组每层 +1）。
    depth: usize,
}

impl<'a> Decoder<'a> {
    pub fn new(s: &'a str) -> Self {
        Decoder { bytes: s.as_bytes(), pos: 0, depth: 0 }
    }

    /// err 生成带偏移信息的错误值。
    fn err(&self, msg: &str) -> Value {
        crate::value::error_value(format!(
            "jsonDecode() 在位置 {} 处失败: {} (可能原因：JSON 格式错误，如缺引号/逗号/括号不匹配)",
            self.pos, msg,
        ))
    }

    /// peek 查看当前字节（不消费）。
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    /// skip_ws 跳过空白。
    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    /// parse_value 解析一个 JSON 值。
    pub fn parse_value(&mut self) -> Result<Value, Value> {
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b'"') => self.parse_string().map(Value::str_from),
            Some(b't') | Some(b'f') => self.parse_bool(),
            Some(b'n') => self.parse_null(),
            Some(b) if b == b'-' || b.is_ascii_digit() => self.parse_number(),
            Some(_) => Err(self.err("意外的字符")),
            None => Err(self.err("意外的输入结束")),
        }
    }

    /// parse_object 解析对象为有序 Map（保持 JSON 键的原始顺序）。
    fn parse_object(&mut self) -> Result<Value, Value> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.err(&format!("嵌套深度超过 {} 层", MAX_DEPTH)));
        }
        self.pos += 1; // 消费 '{'
        let mut map = crate::ord_map::OrdMap::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(Value::Map(Arc::new(Mutex::new(map))));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(self.err("对象键必须是字符串"));
            }
            let key = self.parse_string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(self.err("对象键后缺少冒号 ':'"));
            }
            self.pos += 1; // 消费 ':'
            let val = self.parse_value()?;
            map.set(key, val);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                    continue;
                }
                Some(b'}') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(self.err("对象中缺少 ',' 或 '}'")),
            }
        }
        self.depth -= 1;
        Ok(Value::Map(Arc::new(Mutex::new(map))))
    }

    /// parse_array 解析数组。
    fn parse_array(&mut self) -> Result<Value, Value> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.err(&format!("嵌套深度超过 {} 层", MAX_DEPTH)));
        }
        self.pos += 1; // 消费 '['
        let mut arr = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(Value::Array(Arc::new(Mutex::new(arr))));
        }
        loop {
            let val = self.parse_value()?;
            arr.push(val);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                    continue;
                }
                Some(b']') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(self.err("数组中缺少 ',' 或 ']'")),
            }
        }
        self.depth -= 1;
        Ok(Value::Array(Arc::new(Mutex::new(arr))))
    }

    /// parse_string 解析字符串字面量，返回解转义后的 String。
    fn parse_string(&mut self) -> Result<String, Value> {
        self.pos += 1; // 消费开头 '"'
        let mut s = String::new();
        loop {
            match self.peek() {
                None => return Err(self.err("字符串未闭合（缺少结束引号）")),
                Some(b'"') => {
                    self.pos += 1;
                    break;
                }
                Some(b'\\') => {
                    self.pos += 1;
                    match self.peek() {
                        Some(b'"') => s.push('"'),
                        Some(b'\\') => s.push('\\'),
                        Some(b'/') => s.push('/'),
                        Some(b'n') => s.push('\n'),
                        Some(b't') => s.push('\t'),
                        Some(b'r') => s.push('\r'),
                        Some(b'b') => s.push('\u{08}'),
                        Some(b'f') => s.push('\u{0C}'),
                        Some(b'u') => {
                            // \uXXXX
                            if self.pos + 4 >= self.bytes.len() {
                                return Err(self.err("\\u 转义不完整"));
                            }
                            let hex = std::str::from_utf8(&self.bytes[self.pos + 1..self.pos + 5])
                                .map_err(|_| self.err("\\u 转义非 UTF-8"))?;
                            let code = u32::from_str_radix(hex, 16)
                                .map_err(|_| self.err("\\u 转义非十六进制"))?;
                            self.pos += 4;
                            if let Some(ch) = char::from_u32(code) {
                                s.push(ch);
                            }
                        }
                        Some(_) => return Err(self.err("无效的转义字符")),
                        None => return Err(self.err("转义序列不完整")),
                    }
                    self.pos += 1;
                }
                Some(_) => {
                    // 普通字符：需按 UTF-8 边界消费（一个 char 可能多字节）
                    let rest = &self.bytes[self.pos..];
                    let ch = std::str::from_utf8(rest)
                        .ok()
                        .and_then(|t| t.chars().next());
                    match ch {
                        Some(c) => {
                            s.push(c);
                            self.pos += c.len_utf8();
                        }
                        None => return Err(self.err("非 UTF-8 字符")),
                    }
                }
            }
        }
        Ok(s)
    }

    /// parse_bool 解析 true/false。
    fn parse_bool(&mut self) -> Result<Value, Value> {
        if self.bytes[self.pos..].starts_with(b"true") {
            self.pos += 4;
            Ok(Value::Bool(true))
        } else if self.bytes[self.pos..].starts_with(b"false") {
            self.pos += 5;
            Ok(Value::Bool(false))
        } else {
            Err(self.err("无效的布尔字面量"))
        }
    }

    /// parse_null 解析 null。
    fn parse_null(&mut self) -> Result<Value, Value> {
        if self.bytes[self.pos..].starts_with(b"null") {
            self.pos += 4;
            Ok(Value::Undefined)
        } else {
            Err(self.err("无效的 null 字面量"))
        }
    }

    /// parse_number 解析数字（整数或浮点）。
    fn parse_number(&mut self) -> Result<Value, Value> {
        let start = self.pos;
        // 可选负号
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let mut is_float = false;
        while let Some(b) = self.peek() {
            match b {
                b'0'..=b'9' => self.pos += 1,
                b'.' | b'e' | b'E' | b'+' | b'-' => {
                    is_float = true;
                    self.pos += 1;
                }
                _ => break,
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| self.err("数字非 UTF-8"))?;
        if is_float {
            text.parse::<f64>()
                .map(Value::Float)
                .map_err(|_| self.err("无效的浮点数"))
        } else {
            text.parse::<i64>()
                .map(Value::Int)
                // 极大整数降级为 float
                .or_else(|_| text.parse::<f64>().map(Value::Float))
                .map_err(|_| self.err("无效的数字"))
        }
    }
}

/// bi_json_decode 将 JSON 字符串解码为 Value。
fn bi_json_decode(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "jsonDecode")?;
    let mut dec = Decoder::new(s);
    let v = dec.parse_value()?;
    dec.skip_ws();
    if dec.pos != dec.bytes.len() {
        return Err(dec.err("JSON 末尾存在多余字符"));
    }
    Ok(v)
}

// ---- JSON 路径查询（gjson 风格）----

/// query_json_node 按点分路径查询 JSON 节点，返回拥有的 Value。
///
/// 路径语法：
///   "name"           → 顶层字段
///   "user.name"      → 嵌套字段
///   "users.0.name"   → 数组索引 + 嵌套
///   "items.#"        → 数组长度
///
/// 不存在返回 undefined。
fn query_json_node(root: &Value, path: &str) -> Value {
    if path.is_empty() {
        return root.clone();
    }
    let mut current = root.clone();
    for part in path.split('.') {
        if part.is_empty() {
            continue;
        }
        if part == "#" {
            if let Value::Array(arr) = &current {
                let len = arr.lock().unwrap().len();
                return Value::Int(len as i64);
            }
            return Value::Undefined;
        }
        // 计算下一节点，独立作用域避免借用冲突
        let next: Option<Value> = if let Ok(idx) = part.parse::<i64>() {
            match &current {
                Value::Array(arr) => {
                    let guard = arr.lock().unwrap();
                    let len = guard.len() as i64;
                    let actual = if idx < 0 { idx + len } else { idx };
                    if actual < 0 || actual >= len { None }
                    else { Some(guard[actual as usize].clone()) }
                }
                _ => None,
            }
        } else if let Value::Object(o) = &current {
            let guard = o.lock().unwrap();
            guard.get(part)
        } else if let Value::Map(m) = &current {
            let guard = m.lock().unwrap();
            guard.get(part)
        } else {
            None
        };
        match next {
            Some(v) => current = v,
            None => return Value::Undefined,
        }
    }
    current
}

/// bi_get_json_node 按 JSON 路径查询，返回原始 Value。
///
/// 用法：getJsonNode(data, "user.name") → "张三"
///       getJsonNode(data, "users.0") → {id: 1, ...}
///       getJsonNode(data, "users.#") → 3（数组长度）
fn bi_get_json_node(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 1, "getJsonNode")?;
    let path = bh::as_str(args, 1, "getJsonNode")?;
    Ok(query_json_node(&args[0], path))
}

/// bi_get_json_node_str 按 JSON 路径查询，返回字符串表示。
///
/// 用法：getJsonNodeStr(jsonStr, "user.name") → "张三"
///       getJsonNodeStr(jsonStr, "users.#") → "3"
///
/// 第一个参数可以是 JSON 字符串或已解析的 Value。
fn bi_get_json_node_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 1, "getJsonNodeStr")?;
    let path = bh::as_str(args, 1, "getJsonNodeStr")?;
    // 第一个参数可以是 JSON 字符串或已解析的 Value
    let parsed;
    let root = match &args[0] {
        Value::Str(s) => {
            let mut dec = Decoder::new(s);
            let v = dec.parse_value().map_err(|e| {
                crate::value::error_value(format!(
                    "getJsonNodeStr() JSON 解析失败: {} (可能原因：JSON 格式错误)", e.to_str(),
                ))
            })?;
            parsed = v;
            &parsed
        }
        other => other,
    };
    let result = query_json_node(root, path);
    Ok(Value::str_from(result.to_str()))
}

// ---- 格式化/紧凑输出 ----

/// push_indent 向输出追加 level 层缩进，每层用 indent_str 个空格。
fn push_indent(out: &mut String, level: usize, indent_str: &str) {
    for _ in 0..level {
        out.push_str(indent_str);
    }
}

/// pretty_encode 递归编码 Value 为带换行和缩进的美化 JSON。
///
/// 与 encode_value 的区别：对象/数组元素各占一行，按 indent 层级缩进。
/// 标量类型（数字/字符串/布尔/undefined 等）复用 encode_value 的紧凑输出。
fn pretty_encode(v: &Value, out: &mut String, indent: usize, indent_str: &str) {
    match v {
        Value::Array(a) => {
            // 克隆快照后释放锁，再递归避免持锁死锁
            let snapshot: Vec<Value> = a.lock().unwrap().clone();
            if snapshot.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, elem) in snapshot.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push('\n');
                push_indent(out, indent + 1, indent_str);
                pretty_encode(elem, out, indent + 1, indent_str);
            }
            out.push('\n');
            push_indent(out, indent, indent_str);
            out.push(']');
        }
        Value::Object(o) => {
            let snapshot: Vec<(String, Value)> = o.lock().unwrap().snapshot();
            if snapshot.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (k, val)) in snapshot.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push('\n');
                push_indent(out, indent + 1, indent_str);
                encode_string(k, out);
                out.push_str(": ");
                pretty_encode(val, out, indent + 1, indent_str);
            }
            out.push('\n');
            push_indent(out, indent, indent_str);
            out.push('}');
        }
        Value::Map(m) => {
            let snapshot: Vec<(String, Value)> = m.lock().unwrap().snapshot();
            if snapshot.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (k, val)) in snapshot.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push('\n');
                push_indent(out, indent + 1, indent_str);
                encode_string(k, out);
                out.push_str(": ");
                pretty_encode(val, out, indent + 1, indent_str);
            }
            out.push('\n');
            push_indent(out, indent, indent_str);
            out.push('}');
        }
        // 标量类型复用紧凑编码
        _ => encode_value(v, out),
    }
}

/// bi_format_json 美化 JSON 输出（带换行和缩进）。
///
/// 用法：formatJson(v)          — 默认 2 空格缩进
///       formatJson(v, 4)       — 4 空格缩进
fn bi_format_json(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "formatJson")?;
    // 缩进空格数，默认 2
    let spaces = if args.len() > 1 {
        let n = bh::as_int(args, 1, "formatJson")?;
        if n < 0 {
            return Err(crate::value::error_value(
                "formatJson() 缩进空格数不能为负 (可能原因：参数传错或为负值)".to_string(),
            ));
        }
        n as usize
    } else {
        2
    };
    let indent_str = " ".repeat(spaces);
    let mut out = String::new();
    pretty_encode(&args[0], &mut out, 0, &indent_str);
    Ok(Value::str_from(out))
}

/// bi_compact_json 紧凑 JSON 输出（无空格无换行）。
///
/// 与 jsonEncode 等价，直接复用 encode_value。
fn bi_compact_json(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "compactJson")?;
    let mut out = String::new();
    encode_value(&args[0], &mut out);
    Ok(Value::str_from(out))
}

// ---- JSON 路径批量查询（通配符）----

/// collect_json_strs 按路径分段递归收集所有匹配节点的字符串值。
///
/// 路径分段用 '.' 分隔。支持：
///   "*" 或 "#" — 通配符，遍历数组所有元素或对象所有值
///   数字       — 数组索引（支持负数，-1 为末尾）
///   其他       — 对象键名
fn collect_json_strs(root: &Value, parts: &[&str], out: &mut Vec<String>) {
    if parts.is_empty() {
        out.push(root.to_str());
        return;
    }
    let part = parts[0];
    let rest = &parts[1..];
    if part.is_empty() {
        // 空段（连续点号或首尾点号），跳过
        collect_json_strs(root, rest, out);
        return;
    }
    if part == "*" || part == "#" {
        // 通配符：遍历数组所有元素或对象所有值
        match root {
            Value::Array(arr) => {
                let snap = arr.lock().unwrap().clone();
                for elem in snap.iter() {
                    collect_json_strs(elem, rest, out);
                }
            }
            Value::Object(o) => {
                let snap = o.lock().unwrap().snapshot();
                for (_, v) in snap.iter() {
                    collect_json_strs(v, rest, out);
                }
            }
            Value::Map(m) => {
                let snap = m.lock().unwrap().snapshot();
                for (_, v) in snap.iter() {
                    collect_json_strs(v, rest, out);
                }
            }
            // 非容器类型无法遍历，跳过
            _ => {}
        }
        return;
    }
    // 数组索引
    if let Ok(idx) = part.parse::<i64>() {
        if let Value::Array(arr) = root {
            let guard = arr.lock().unwrap();
            let len = guard.len() as i64;
            let actual = if idx < 0 { idx + len } else { idx };
            if actual >= 0 && actual < len {
                collect_json_strs(&guard[actual as usize], rest, out);
            }
        }
        return;
    }
    // 对象键
    let next: Option<Value> = match root {
        Value::Object(o) => o.lock().unwrap().get(part),
        Value::Map(m) => m.lock().unwrap().get(part),
        _ => None,
    };
    match next {
        Some(v) => collect_json_strs(&v, rest, out),
        None => {} // 键不存在，跳过
    }
}

/// bi_get_json_node_strs 按 JSON 路径查询所有匹配节点的字符串值数组。
///
/// 用法：getJsonNodeStrs(json, "items.*.name") → ["a", "b"]
///       getJsonNodeStrs(json, "items.#.name") → ["a", "b"]
/// 支持 * 和 # 作为通配符表示所有元素。第一个参数可为 JSON 字符串或已解析 Value。
fn bi_get_json_node_strs(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 1, "getJsonNodeStrs")?;
    let path = bh::as_str(args, 1, "getJsonNodeStrs")?;
    // 第一个参数可以是 JSON 字符串或已解析的 Value
    let parsed;
    let root = match &args[0] {
        Value::Str(s) => {
            let mut dec = Decoder::new(s);
            let v = dec.parse_value().map_err(|e| {
                crate::value::error_value(format!(
                    "getJsonNodeStrs() JSON 解析失败: {} (可能原因：JSON 格式错误)", e.to_str(),
                ))
            })?;
            parsed = v;
            &parsed
        }
        other => other,
    };
    let parts: Vec<&str> = path.split('.').collect();
    let mut result = Vec::new();
    collect_json_strs(root, &parts, &mut result);
    let arr: Vec<Value> = result.into_iter().map(Value::str_from).collect();
    Ok(Value::Array(std::sync::Arc::new(std::sync::Mutex::new(arr))))
}
