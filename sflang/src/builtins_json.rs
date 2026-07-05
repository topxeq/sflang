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
use crate::object_map::Map;
use crate::value::Value;
use crate::vm::VM;

/// register 注册所有 JSON 内置函数到 VM。
pub fn register(vm: &mut VM) {
    vm.register_builtin("jsonEncode", bi_json_encode);
    vm.register_builtin("jsonDecode", bi_json_decode);
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
struct Decoder<'a> {
    bytes: &'a [u8],
    pos: usize,
    /// depth 当前嵌套深度（对象/数组每层 +1）。
    depth: usize,
}

impl<'a> Decoder<'a> {
    fn new(s: &'a str) -> Self {
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
    fn parse_value(&mut self) -> Result<Value, Value> {
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

    /// parse_object 解析对象。
    fn parse_object(&mut self) -> Result<Value, Value> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.err(&format!("嵌套深度超过 {} 层", MAX_DEPTH)));
        }
        self.pos += 1; // 消费 '{'
        let mut map = Map::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(Value::Object(Arc::new(Mutex::new(map))));
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
        Ok(Value::Object(Arc::new(Mutex::new(map))))
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
