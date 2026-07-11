//! builtins_xml.rs — XML 解析与格式化内置函数（纯标准库）
//!
//! 设计要点：
//!   - 手写轻量级 XML 解析器，支持元素/属性/文本/CDATA/注释/自闭合
//!   - 不支持 DTD/ENTITY/XSD 等复杂特性（保持简单）
//!   - XML 节点用 Map 表示：{name, attrs, children, text}
//!     children 为子节点数组，text 为直接文本内容
//!   - 错误信息含偏移与可能原因，便于 AI 定位
//!
//! 函数列表：
//!   fromXml(s)          — XML 字符串 → Map 节点
//!   xmlGetNodeStr(node) — 提取节点文本（递归拼接所有文本）
//!   formatXml(node)     — Map 节点 → XML 字符串（带缩进）

use std::sync::Arc;

use crate::builtins_helpers as bh;
use crate::object_map::new_map;
use crate::value::{Value, error_value};
use crate::vm::VM;

/// register 注册 XML 内置函数。
pub fn register(vm: &mut VM) {
    vm.register_builtin("fromXml", bi_from_xml);
    vm.register_builtin("xmlGetNodeStr", bi_xml_get_node_str);
    vm.register_builtin("formatXml", bi_format_xml);
}

// ===========================================================================
// XML 解析器
// ===========================================================================

/// Parser XML 解析器内部状态。
struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

/// XmlNode 内存中的 XML 节点表示。
struct XmlNode {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<XmlNode>,
    text: String,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Parser { bytes: s.as_bytes(), pos: 0 }
    }

    /// peek 查看当前字节。
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    /// starts_with 检查当前位置是否以指定字符串开头。
    fn starts_with(&self, s: &str) -> bool {
        self.bytes[self.pos..].starts_with(s.as_bytes())
    }

    /// skip_ws 跳过空白字符。
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    /// parse_document 解析整个 XML 文档，返回根节点。
    fn parse_document(&mut self) -> Result<XmlNode, String> {
        self.skip_ws();
        // 跳过 XML 声明 <?xml ...?>
        if self.starts_with("<?xml") {
            self.skip_pi()?;
            self.skip_ws();
        }
        // 跳过注释和处理指令
        loop {
            self.skip_ws();
            if self.starts_with("<!--") {
                self.skip_comment()?;
            } else if self.starts_with("<?") {
                self.skip_pi()?;
            } else {
                break;
            }
        }
        // 解析根元素
        let root = self.parse_element()?;
        Ok(root)
    }

    /// skip_pi 跳过处理指令 <?...?>
    fn skip_pi(&mut self) -> Result<(), String> {
        if !self.starts_with("<?") {
            return Err(format!("期望 <? 开始处理指令，位置 {}", self.pos));
        }
        self.pos += 2;
        while self.pos < self.bytes.len() {
            if self.starts_with("?>") {
                self.pos += 2;
                return Ok(());
            }
            self.pos += 1;
        }
        Err("处理指令未闭合 (缺少 ?>)".to_string())
    }

    /// skip_comment 跳过注释 <!--...-->
    fn skip_comment(&mut self) -> Result<(), String> {
        if !self.starts_with("<!--") {
            return Err(format!("期望 <!-- 开始注释，位置 {}", self.pos));
        }
        self.pos += 4;
        while self.pos < self.bytes.len() {
            if self.starts_with("-->") {
                self.pos += 3;
                return Ok(());
            }
            self.pos += 1;
        }
        Err("注释未闭合 (缺少 -->)".to_string())
    }

    /// parse_element 解析一个元素 <tag ...>...</tag> 或 <tag .../>
    fn parse_element(&mut self) -> Result<XmlNode, String> {
        if self.peek() != Some(b'<') {
            return Err(format!(
                "期望 '<' 开始元素，位置 {} 得到 {:?} (可能原因：XML 格式错误或标签未闭合)",
                self.pos,
                self.peek().map(|c| c as char),
            ));
        }
        self.pos += 1; // 消费 '<'
        let name = self.parse_name()?;
        // 解析属性
        let mut attrs = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'>') => {
                    self.pos += 1;
                    break;
                }
                Some(b'/') => {
                    // 自闭合 <tag/>
                    if self.bytes.get(self.pos + 1) == Some(&b'>') {
                        self.pos += 2;
                        return Ok(XmlNode {
                            name,
                            attrs,
                            children: Vec::new(),
                            text: String::new(),
                        });
                    } else {
                        return Err(format!(
                            "期望 '/>' 自闭合，位置 {} (可能原因：属性值未用引号括起)",
                            self.pos,
                        ));
                    }
                }
                Some(_) => {
                    let attr_name = self.parse_name()?;
                    self.skip_ws();
                    if self.peek() != Some(b'=') {
                        return Err(format!(
                            "期望 '=' 在属性名后，位置 {} (可能原因：属性值缺少 = 或引号未闭合)",
                            self.pos,
                        ));
                    }
                    self.pos += 1; // 消费 '='
                    self.skip_ws();
                    let attr_val = self.parse_attr_value()?;
                    attrs.push((attr_name, attr_val));
                }
                None => return Err("XML 意外结束于属性解析".to_string()),
            }
        }
        // 解析子节点和文本
        let mut children = Vec::new();
        let mut text = String::new();
        loop {
            if self.pos >= self.bytes.len() {
                return Err(format!(
                    "元素 <{}> 未闭合 (缺少 </{}>) (可能原因：开始标签未匹配结束标签)",
                    name, name,
                ));
            }
            if self.starts_with("</") {
                // 结束标签
                self.pos += 2;
                let end_name = self.parse_name()?;
                self.skip_ws();
                if self.peek() != Some(b'>') {
                    return Err(format!("结束标签 {} 后期望 '>'，位置 {}", end_name, self.pos));
                }
                self.pos += 1;
                if end_name != name {
                    return Err(format!(
                        "标签不匹配：开始 <{}> 结束 </{}> (可能原因：标签嵌套错误)",
                        name, end_name,
                    ));
                }
                break;
            } else if self.starts_with("<!--") {
                self.skip_comment()?;
            } else if self.starts_with("<?") {
                self.skip_pi()?;
            } else if self.starts_with("<![CDATA[") {
                let cdata = self.parse_cdata()?;
                text.push_str(&cdata);
            } else if self.peek() == Some(b'<') {
                // 子元素
                let child = self.parse_element()?;
                children.push(child);
            } else {
                // 文本内容
                let chunk = self.parse_text()?;
                text.push_str(&chunk);
            }
        }
        Ok(XmlNode {
            name,
            attrs,
            children,
            text,
        })
    }

    /// parse_name 解析标签名或属性名（字母/数字/下划线/冒号/连字符）。
    fn parse_name(&mut self) -> Result<String, String> {
        let start = self.pos;
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b':' | b'-' | b'.' => {
                    self.pos += 1;
                }
                _ => break,
            }
        }
        if self.pos == start {
            return Err(format!(
                "期望标签/属性名，位置 {} (可能原因：标签名包含非法字符或缺少空格分隔)",
                start,
            ));
        }
        Ok(String::from_utf8_lossy(&self.bytes[start..self.pos]).to_string())
    }

    /// parse_attr_value 解析属性值（单引号或双引号括起）。
    fn parse_attr_value(&mut self) -> Result<String, String> {
        let quote = match self.peek() {
            Some(b'"') => b'"',
            Some(b'\'') => b'\'',
            _ => {
                return Err(format!(
                    "属性值需用引号括起，位置 {} (可能原因：忘记用 \" 或 ' 括起属性值)",
                    self.pos,
                ));
            }
        };
        self.pos += 1; // 消费引号
        let start = self.pos;
        while self.pos < self.bytes.len() {
            if self.bytes[self.pos] == quote {
                let val = String::from_utf8_lossy(&self.bytes[start..self.pos]).to_string();
                self.pos += 1;
                return Ok(decode_xml_entities(&val));
            }
            self.pos += 1;
        }
        Err(format!("属性值未闭合，起始位置 {} (缺少匹配的引号)", start))
    }

    /// parse_cdata 解析 CDATA 段 <![CDATA[...]]>
    fn parse_cdata(&mut self) -> Result<String, String> {
        if !self.starts_with("<![CDATA[") {
            return Err(format!("期望 <![CDATA[，位置 {}", self.pos));
        }
        self.pos += 9; // 消费 "<![CDATA["
        let start = self.pos;
        while self.pos < self.bytes.len() {
            if self.starts_with("]]>") {
                let val = String::from_utf8_lossy(&self.bytes[start..self.pos]).to_string();
                self.pos += 3;
                return Ok(val);
            }
            self.pos += 1;
        }
        Err("CDATA 段未闭合 (缺少 ]]>".to_string())
    }

    /// parse_text 解析文本内容（直到遇到 '<'），解码 XML 实体。
    fn parse_text(&mut self) -> Result<String, String> {
        let start = self.pos;
        while self.pos < self.bytes.len() && self.bytes[self.pos] != b'<' {
            self.pos += 1;
        }
        let raw = String::from_utf8_lossy(&self.bytes[start..self.pos]).to_string();
        Ok(decode_xml_entities(&raw))
    }
}

/// decode_xml_entities 解码 XML 实体（&amp; &lt; &gt; &quot; &apos; &#NN; &#xNN;）。
fn decode_xml_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '&' {
            // 查找 ';'
            let semi = s[i..].find(';');
            if let Some(rel) = semi {
                let entity = &s[i + 1..i + rel];
                if entity == "amp" {
                    out.push('&');
                } else if entity == "lt" {
                    out.push('<');
                } else if entity == "gt" {
                    out.push('>');
                } else if entity == "quot" {
                    out.push('"');
                } else if entity == "apos" {
                    out.push('\'');
                } else if let Some(hex) = entity.strip_prefix("#x") {
                    if let Ok(n) = u32::from_str_radix(hex, 16) {
                        if let Some(ch) = char::from_u32(n) {
                            out.push(ch);
                        }
                    }
                } else if let Some(dec) = entity.strip_prefix('#') {
                    if let Ok(n) = dec.parse::<u32>() {
                        if let Some(ch) = char::from_u32(n) {
                            out.push(ch);
                        }
                    }
                } else {
                    // 未知实体，原样保留
                    out.push('&');
                    out.push_str(entity);
                    out.push(';');
                }
                // 消费到分号
                while let Some((_, c2)) = chars.next() {
                    if c2 == ';' {
                        break;
                    }
                }
            } else {
                out.push('&');
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// encode_xml_entities 编码 XML 特殊字符为实体。
fn encode_xml_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// node_to_value 将 XmlNode 转换为 Sflang Value（Map 类型）。
fn node_to_value(node: &XmlNode) -> Value {
    let m = new_map();
    {
        let mut guard = m.lock().unwrap();
        guard.set("name".to_string(), Value::str_from(node.name.clone()));
        // attrs 用 Map 表示
        let attrs_map = new_map();
        {
            let mut am = attrs_map.lock().unwrap();
            for (k, v) in &node.attrs {
                am.set(k.clone(), Value::str_from(v.clone()));
            }
        }
        guard.set("attrs".to_string(), Value::Object(attrs_map));
        // children 用 Array 表示
        let children: Vec<Value> = node.children.iter().map(node_to_value).collect();
        guard.set(
            "children".to_string(),
            Value::Array(Arc::new(std::sync::Mutex::new(children))),
        );
        guard.set("text".to_string(), Value::str_from(node.text.clone()));
    }
    Value::Object(m)
}

/// value_to_node 将 Sflang Value（Map/Object）转换回 XmlNode。
fn value_to_node(v: &Value, fn_name: &str) -> Result<XmlNode, Value> {
    // 支持 Object(Map) 和 Map(OrdMap) 两种容器
    let (name_val, attrs_val, children_val, text_val) = match v {
        Value::Object(m) => {
            let guard = m.lock().unwrap();
            (
                guard.get("name"),
                guard.get("attrs"),
                guard.get("children"),
                guard.get("text"),
            )
        }
        Value::Map(m) => {
            let guard = m.lock().unwrap();
            (
                guard.get("name"),
                guard.get("attrs"),
                guard.get("children"),
                guard.get("text"),
            )
        }
        _ => {
            return Err(error_value(format!(
                "{}() 参数应为 xml 节点 (map)，得到 {} (可能原因：传入了非 fromXml 的结果)",
                fn_name,
                v.type_name_ex(),
            )));
        }
    };
    let name = match name_val {
        Some(Value::Str(s)) => s.to_string(),
        _ => return Err(error_value(format!(
            "{}() 节点缺少 name 字段 (可能原因：传入的不是 fromXml 结果)",
            fn_name,
        ))),
    };
    let mut attrs = Vec::new();
    match attrs_val {
        Some(Value::Object(am)) => {
            let am_guard = am.lock().unwrap();
            for (k, v) in am_guard.data.iter() {
                let val = match &v {
                    Value::Str(s) => s.to_string(),
                    _ => v.to_str(),
                };
                attrs.push((k.clone(), val));
            }
        }
        Some(Value::Map(am)) => {
            let am_guard = am.lock().unwrap();
            for (k, v) in am_guard.snapshot() {
                let val = match &v {
                    Value::Str(s) => s.to_string(),
                    _ => v.to_str(),
                };
                attrs.push((k, val));
            }
        }
        _ => {}
    }
    let mut children = Vec::new();
    if let Some(Value::Array(arr)) = children_val {
        let arr_guard = arr.lock().unwrap();
        for child in arr_guard.iter() {
            children.push(value_to_node(child, fn_name)?);
        }
    }
    let text = match text_val {
        Some(Value::Str(s)) => s.to_string(),
        _ => String::new(),
    };
    Ok(XmlNode {
        name,
        attrs,
        children,
        text,
    })
}

/// collect_text 递归收集节点及其子节点的所有文本。
fn collect_text(node: &XmlNode, out: &mut String) {
    if !node.text.is_empty() {
        out.push_str(&node.text);
    }
    for child in &node.children {
        collect_text(child, out);
    }
}

/// format_node 将 XmlNode 格式化为带缩进的 XML 字符串。
fn format_node(node: &XmlNode, indent: usize, out: &mut String) {
    let pad = "  ".repeat(indent);
    out.push_str(&pad);
    out.push('<');
    out.push_str(&node.name);
    for (k, v) in &node.attrs {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        out.push_str(&encode_xml_entities(v));
        out.push('"');
    }
    if node.children.is_empty() && node.text.is_empty() {
        out.push_str(" />\n");
        return;
    }
    out.push('>');
    if !node.text.is_empty() {
        out.push_str(&encode_xml_entities(&node.text));
    }
    if !node.children.is_empty() {
        out.push('\n');
        for child in &node.children {
            format_node(child, indent + 1, out);
        }
        out.push_str(&pad);
    }
    out.push_str("</");
    out.push_str(&node.name);
    out.push_str(">\n");
}

// ===========================================================================
// 内置函数
// ===========================================================================

/// bi_from_xml 解析 XML 字符串为 Map 节点。
fn bi_from_xml(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "fromXml")?;
    let mut parser = Parser::new(s);
    match parser.parse_document() {
        Ok(root) => Ok(node_to_value(&root)),
        Err(e) => Err(error_value(format!(
            "fromXml() 解析失败: {} (可能原因：XML 格式错误、标签未闭合或编码不一致)",
            e,
        ))),
    }
}

/// bi_xml_get_node_str 提取 XML 节点的文本内容（递归拼接所有文本）。
fn bi_xml_get_node_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let node = value_to_node(&args[0], "xmlGetNodeStr")?;
    let mut text = String::new();
    collect_text(&node, &mut text);
    Ok(Value::str_from(text))
}

/// bi_format_xml 将 Map 节点格式化为 XML 字符串（带缩进）。
fn bi_format_xml(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let node = value_to_node(&args[0], "formatXml")?;
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    format_node(&node, 0, &mut out);
    // 去掉末尾换行
    if out.ends_with('\n') {
        out.pop();
    }
    Ok(Value::str_from(out))
}
