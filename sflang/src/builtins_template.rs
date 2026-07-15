//! builtins_template.rs — 模板与渲染内置函数
//!
//! 设计要点（来自 AGENTS.md）：
//!   - 仅依赖 Rust 标准库（不引入第三方 markdown 库）
//!   - renderMarkdown: 简单 Markdown → HTML，覆盖常见语法
//!   - replaceHtmlByMap: 模板占位符替换，便于动态生成内容
//!   - 不要求完整 Markdown 规范实现，仅覆盖常见且实用的语法
//!
//! 函数列表：
//!   renderMarkdown(md)               — Markdown 转 HTML
//!   replaceHtmlByMap(template, data) — {{key}} 占位符替换

use crate::builtins_helpers as bh;
use crate::value::Value;
use crate::vm::VM;
use crate::function::BuiltinDoc;

static DOC_RENDERMARKDOWN: BuiltinDoc = BuiltinDoc {
    category: "template",
    signature: "renderMarkdown(md) -> string",
    summary: "将 Markdown 转为 HTML。",
    params: &[("md", "Markdown 文本")],
    returns: "string HTML",
    examples: &["renderMarkdown(mdText)"],
    errors: &[],
};

static DOC_REPLACEHTMLBYMAP: BuiltinDoc = BuiltinDoc {
    category: "template",
    signature: "replaceHtmlByMap(tmpl, m) -> string",
    summary: "用 map 替换模板中的 {{key}} 占位符。",
    params: &[("tmpl", "含 {{key}} 占位符的模板"), ("m", "键值映射（object/map）")],
    returns: "string 替换后的字符串",
    examples: &["replaceHtmlByMap(tmpl, {\"name\": \"Alice\"})  // Hi Alice"],
    errors: &[],
};

/// register 注册所有模板内置函数到 VM。
pub fn register(vm: &mut VM) {
    vm.register_builtin_doc("renderMarkdown", bi_render_markdown, &DOC_RENDERMARKDOWN);
    vm.register_builtin_doc("replaceHtmlByMap", bi_replace_html_by_map, &DOC_REPLACEHTMLBYMAP);
}

// ---- HTML 转义辅助 ----

/// html_escape 转义 HTML 中的特殊字符（避免 XSS 与显示错误）。
///
/// & < > " ' 均转义为对应实体。用于普通文本节点的安全输出。
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

// ---- 内联格式解析（粗体/斜体/代码/链接）----
//
// 处理顺序很关键：先处理代码（避免内部内容被再次解析），
// 再处理链接（避免 URL 中的特殊字符被错误解析），最后处理粗体/斜体。
// 使用简化的扫描器，逐字符查找标记。

/// render_inline 渲染内联格式：粗体 **、斜体 *、代码 `、链接 [text](url)。
///
/// 简化实现：不支持嵌套，按顺序查找标记对。
/// 代码块中的内容不再次解析（避免 URL 中的 * 等被误解析）。
fn render_inline(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        // 代码 `code`
        if c == '`' {
            if let Some(end) = find_char(&chars, i + 1, '`') {
                let code: String = chars[i + 1..end].iter().collect();
                // 代码块中的 HTML 字符串仍需转义（防 XSS）
                out.push_str("<code>");
                out.push_str(&html_escape(&code));
                out.push_str("</code>");
                i = end + 1;
                continue;
            }
        }

        // 粗体 **text**
        if c == '*' && i + 1 < chars.len() && chars[i + 1] == '*' {
            if let Some(end) = find_substr(&chars, i + 2, '*', '*') {
                let inner: String = chars[i + 2..end].iter().collect();
                out.push_str("<strong>");
                // 内部递归处理（支持链接等嵌套，但不支持粗体嵌套）
                out.push_str(&render_inline(&inner));
                out.push_str("</strong>");
                i = end + 2;
                continue;
            }
        }

        // 链接 [text](url)
        if c == '[' {
            if let Some(text_end) = find_char(&chars, i + 1, ']') {
                if text_end + 1 < chars.len() && chars[text_end + 1] == '(' {
                    if let Some(url_end) = find_char(&chars, text_end + 2, ')') {
                        let link_text: String = chars[i + 1..text_end].iter().collect();
                        let url: String = chars[text_end + 2..url_end].iter().collect();
                        // 转义 URL 中的危险字符（防 XSS：javascript: 等）
                        let safe_url = sanitize_url(&url);
                        out.push_str("<a href=\"");
                        out.push_str(&safe_url);
                        out.push_str("\">");
                        out.push_str(&html_escape(&link_text));
                        out.push_str("</a>");
                        i = url_end + 1;
                        continue;
                    }
                }
            }
        }

        // 斜体 *text*（单个星号）
        if c == '*' {
            if let Some(end) = find_single_char(&chars, i + 1, '*') {
                let inner: String = chars[i + 1..end].iter().collect();
                out.push_str("<em>");
                out.push_str(&render_inline(&inner));
                out.push_str("</em>");
                i = end + 1;
                continue;
            }
        }

        // 普通字符（转义后输出）
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
        i += 1;
    }

    out
}

/// find_char 查找下一个目标字符的位置（无嵌套，简单查找）。
fn find_char(chars: &[char], start: usize, target: char) -> Option<usize> {
    chars[start..].iter().position(|&c| c == target).map(|p| p + start)
}

/// find_substr 查找连续两个字符的位置（用于查找 **）。
fn find_substr(chars: &[char], start: usize, a: char, b: char) -> Option<usize> {
    let mut i = start;
    while i + 1 < chars.len() {
        if chars[i] == a && chars[i + 1] == b {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// find_single_char 查找单个星号（避免匹配到 ** 粗体标记）。
///
/// 当遇到连续两个星号时跳过（视为粗体标记，不应被斜体匹配）。
fn find_single_char(chars: &[char], start: usize, target: char) -> Option<usize> {
    let mut i = start;
    while i < chars.len() {
        if chars[i] == target {
            // 检查是否为 ** （粗体标记），若是则跳过
            if i + 1 < chars.len() && chars[i + 1] == target {
                i += 2;
                continue;
            }
            return Some(i);
        }
        i += 1;
    }
    None
}

/// sanitize_url 简单过滤 URL，移除危险的协议（javascript:、data: 等）。
///
/// 仅允许 http://、https://、mailto:、#anchor、/path、relative 等安全形式。
fn sanitize_url(url: &str) -> String {
    let trimmed = url.trim();
    let lower = trimmed.to_lowercase();
    // 仅允许白名单协议或相对路径
    let safe = lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || lower.starts_with('/')
        || lower.starts_with('#')
        || !lower.contains(':'); // 相对路径，无协议
    if safe {
        // 转义 URL 中的 " 防止逃逸属性
        trimmed.replace('"', "&quot;")
    } else {
        // 不安全协议降级为 #
        "#".to_string()
    }
}

// ---- Markdown 块级解析 ----

/// bi_render_markdown 将 Markdown 转为 HTML。
///
/// 用法：renderMarkdown(md) → string
///
/// 支持的语法（简单实现，不追求完整规范）：
///   - 标题 # ## ### → h1 h2 h3（最多 6 级）
///   - 粗体 **text** → strong
///   - 斜体 *text* → em
///   - 代码 `code` → code
///   - 代码块 ```lang\ncode\n``` → pre code（lang 作为 class）
///   - 链接 [text](url) → a
///   - 无序列表 - item 或 * item → ul li
///   - 有序列表 1. item → ol li
///   - 段落（空行分隔）
///   - 换行：单行末尾两个空格 → <br>，或行内换行按段落处理
fn bi_render_markdown(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let md = bh::as_str(args, 0, "renderMarkdown")?;

    let mut out = String::with_capacity(md.len() * 2);
    let lines: Vec<&str> = md.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // 跳过空行
        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        // 代码块 ```lang
        if trimmed.starts_with("```") {
            let lang = trimmed[3..].trim();
            let mut code_content = String::new();
            i += 1;
            while i < lines.len() && !lines[i].trim().starts_with("```") {
                code_content.push_str(lines[i]);
                code_content.push('\n');
                i += 1;
            }
            // 跳过结束的 ```
            if i < lines.len() {
                i += 1;
            }
            // 去掉末尾多余的 \n
            if code_content.ends_with('\n') {
                code_content.pop();
            }
            out.push_str("<pre><code");
            if !lang.is_empty() {
                out.push_str(" class=\"language-");
                out.push_str(&html_escape(lang));
                out.push('"');
            }
            out.push('>');
            out.push_str(&html_escape(&code_content));
            out.push_str("</code></pre>\n");
            continue;
        }

        // 标题 # ## ###
        if let Some(rest) = trimmed.strip_prefix('#') {
            let level = 1 + rest.chars().take_while(|&c| c == '#').count();
            let content = rest[level - 1..].trim();
            if !content.is_empty() || level <= 6 {
                let level = level.min(6);
                out.push_str(&format!("<h{}>", level));
                out.push_str(&render_inline(content));
                out.push_str(&format!("</h{}>\n", level));
                i += 1;
                continue;
            }
        }

        // 无序列表（- 或 * 开头）
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            out.push_str("<ul>\n");
            while i < lines.len() {
                let li = lines[i].trim();
                if let Some(item) = li.strip_prefix("- ").or_else(|| li.strip_prefix("* ")) {
                    out.push_str("<li>");
                    out.push_str(&render_inline(item));
                    out.push_str("</li>\n");
                    i += 1;
                } else {
                    break;
                }
            }
            out.push_str("</ul>\n");
            continue;
        }

        // 有序列表（1. 2. 等数字加点开头）
        if is_ordered_list_item(trimmed) {
            out.push_str("<ol>\n");
            while i < lines.len() {
                let li = lines[i].trim();
                if let Some(item) = strip_ordered_prefix(li) {
                    out.push_str("<li>");
                    out.push_str(&render_inline(item));
                    out.push_str("</li>\n");
                    i += 1;
                } else {
                    break;
                }
            }
            out.push_str("</ol>\n");
            continue;
        }

        // 段落：连续的非空行组成一个段落
        let mut para = String::new();
        while i < lines.len() {
            let l = lines[i];
            let t = l.trim();
            if t.is_empty() {
                break;
            }
            // 段落以这些标记结束时停止
            if t.starts_with('#')
                || t.starts_with("```")
                || t.starts_with("- ")
                || t.starts_with("* ")
                || is_ordered_list_item(t) {
                break;
            }
            // 末尾两个空格 → <br>
            let content = l.trim_end();
            if content.ends_with("  ") {
                let stripped = &content[..content.len() - 2];
                if !para.is_empty() {
                    para.push(' ');
                }
                para.push_str(&render_inline(stripped));
                para.push_str("<br>\n");
            } else {
                if !para.is_empty() {
                    para.push(' ');
                }
                para.push_str(&render_inline(content));
            }
            i += 1;
        }
        if !para.is_empty() {
            out.push_str("<p>");
            out.push_str(&para.trim_end());
            out.push_str("</p>\n");
        }
    }

    // 去掉末尾多余换行
    if out.ends_with('\n') {
        out.pop();
    }
    Ok(Value::str_from(out))
}

/// is_ordered_list_item 判断是否为有序列表项（如 "1. xxx"）。
fn is_ordered_list_item(s: &str) -> bool {
    let mut chars = s.chars().peekable();
    let mut digit_count = 0;
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            digit_count += 1;
            chars.next();
        } else {
            break;
        }
    }
    if digit_count == 0 {
        return false;
    }
    match chars.next() {
        Some('.') => match chars.next() {
            Some(' ') => true,
            _ => false,
        },
        _ => false,
    }
}

/// strip_ordered_prefix 去掉有序列表前缀（如 "1. xxx" → "xxx"），非列表项返回 None。
fn strip_ordered_prefix(s: &str) -> Option<&str> {
    let mut chars = s.chars();
    let mut digit_count = 0;
    while let Some(c) = chars.clone().next() {
        if c.is_ascii_digit() {
            digit_count += 1;
            chars.next();
        } else {
            break;
        }
    }
    if digit_count == 0 {
        return None;
    }
    let rest: &str = &s[digit_count..];
    if let Some(r) = rest.strip_prefix(". ") {
        Some(r)
    } else {
        None
    }
}

/// bi_replace_html_by_map 模板替换：将 template 中的 {{key}} 替换为 dataMap 中对应的值。
///
/// 用法：replaceHtmlByMap(template, dataMap) → string
///
/// template 为字符串模板，dataMap 为 Object 或 Map。
/// 占位符格式：{{key}}（双花括号包裹的键名）。
/// dataMap 中存在的 key 替换为对应值的 to_str()；不存在的 key 替换为空字符串。
///
/// 用途：动态生成 HTML、SQL、配置文件等文本内容。
fn bi_replace_html_by_map(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let template = bh::as_str(args, 0, "replaceHtmlByMap")?;
    bh::require_arg(args, 1, "replaceHtmlByMap")?;

    // 收集键值对快照（避免在替换循环中持续持锁）
    let pairs: Vec<(String, String)> = match &args[1] {
        Value::Object(o) => {
            o.lock().unwrap().snapshot().into_iter()
                .map(|(k, v)| (k, v.to_str()))
                .collect()
        }
        Value::Map(m) => {
            m.lock().unwrap().snapshot().into_iter()
                .map(|(k, v)| (k, v.to_str()))
                .collect()
        }
        v => return Err(crate::value::error_value(format!(
            "replaceHtmlByMap() 第 2 个参数应为 object 或 map，得到 {} (可能原因：参数顺序错误，正确顺序 replaceHtmlByMap(template, dataMap))",
            v.type_name(),
        ))),
    };

    let mut result = template.to_string();
    for (key, value) in &pairs {
        let placeholder = format!("{{{{{}}}}}", key);
        result = result.replace(&placeholder, value);
    }

    // 第二轮：将未匹配的占位符替换为空字符串（避免遗留 {{xxx}} 在输出中）
    // 使用简单扫描器查找 {{ ... }} 模式
    result = strip_unmatched_placeholders(&result);

    Ok(Value::str_from(result))
}

/// strip_unmatched_placeholders 将未在 dataMap 中存在的占位符 {{xxx}} 替换为空字符串。
///
/// 简单实现：查找 {{ 开头，到 }} 结尾的子串，替换为空。
fn strip_unmatched_placeholders(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;

    while i < chars.len() {
        if i + 1 < chars.len() && chars[i] == '{' && chars[i + 1] == '{' {
            // 查找匹配的 }}
            if let Some(close) = find_close_braces(&chars, i + 2) {
                // 跳过整个占位符（不输出任何内容）
                i = close + 2;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// find_close_braces 查找 }} 的位置，从 start 开始。
///
/// 返回第一个 } 的位置（即匹配的 }} 中第一个 }）。
fn find_close_braces(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start;
    while i + 1 < chars.len() {
        if chars[i] == '}' && chars[i + 1] == '}' {
            return Some(i);
        }
        i += 1;
    }
    None
}
