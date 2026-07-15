//! builtins_regex.rs — 正则表达式内置函数
//!
//! 设计要点：
//!   - 基于 rust-lang 官方 regex crate（线性时间保证，防 ReDoS）
//!   - 不支持前后向断言、反向引用（regex crate 限制；对标 Go regexp）
//!   - 函数命名对标 charlang（reg 前缀，符合 AGENTS.md 分类原则）
//!   - 支持 string 模式或 regCompile 预编译对象两种入参
//!   - 错误信息 AI 友好：附 pattern 与原因
//!
//! 函数列表：
//!   regMatch(pattern, s)        — 整串是否完全匹配
//!   regFind(pattern, s)         — 第一个匹配子串（无则 undefined）
//!   regFindAll(pattern, s)      — 全部匹配子串（array<string>）
//!   regFindFirst(pattern, s)    — 第一个匹配 + 捕获组（array<string>，无则 undefined）
//!   regReplace(pattern, s, repl)— 替换全部匹配（repl 可含 $1/$2 捕获引用）
//!   regSplit(pattern, s)        — 按模式分割
//!   regCompile(pattern)         — 预编译正则（返回可复用的 regex 对象）

use std::sync::Arc;

use regex::Regex;

use crate::builtins_helpers as bh;
use crate::function::BuiltinDoc;
use crate::value::Value;
use crate::vm::VM;

// ---- 正则函数文档 ----

static DOC_REG_MATCH: BuiltinDoc = BuiltinDoc {
    category: "regex",
    signature: "regMatch(pattern, s) -> bool",
    summary: "判断整串 s 是否完全匹配 pattern（^...$ 语义）。",
    params: &[
        ("pattern", "正则表达式（string）或 regCompile 预编译对象"),
        ("s", "被匹配的字符串"),
    ],
    returns: "bool：整串完全匹配返回 true",
    examples: &[
        "regMatch(\"\\\\d+\", \"12345\")      → true",
        "regMatch(\"\\\\d+\", \"12a\")         → false（非整串数字）",
    ],
    errors: &[
        "参数顺序：pattern 在前，s 在后",
        "pattern 用字符串时反斜杠需双重转义（\"\\\\\\\\d+\" 表示 \\d+）",
    ],
};

static DOC_REG_FIND: BuiltinDoc = BuiltinDoc {
    category: "regex",
    signature: "regFind(pattern, s) -> string|undefined",
    summary: "在 s 中查找第一个匹配 pattern 的子串。",
    params: &[
        ("pattern", "正则表达式（string 或 regCompile 对象）"),
        ("s", "被搜索的字符串"),
    ],
    returns: "string 第一个匹配子串；未匹配返回 undefined",
    examples: &[
        "regFind(\"\\\\d+\", \"a12b34\")        → \"12\"",
        "regFind(\"xyz\", \"abc\")             → undefined",
    ],
    errors: &[
        "参数顺序：pattern 在前，s 在后（最易搞反）",
        "pattern 用字符串时反斜杠需双重转义",
    ],
};

static DOC_REG_FIND_ALL: BuiltinDoc = BuiltinDoc {
    category: "regex",
    signature: "regFindAll(pattern, s) -> array<string>",
    summary: "查找 s 中所有匹配 pattern 的子串。",
    params: &[
        ("pattern", "正则表达式（string 或 regCompile 对象）"),
        ("s", "被搜索的字符串"),
    ],
    returns: "array<string> 所有匹配子串；无匹配返回空数组 []",
    examples: &[
        "regFindAll(\"\\\\d+\", \"a1b22c333\")   → [\"1\", \"22\", \"333\"]",
    ],
    errors: &["参数顺序：pattern 在前，s 在后"],
};

static DOC_REG_REPLACE: BuiltinDoc = BuiltinDoc {
    category: "regex",
    signature: "regReplace(pattern, s, repl) -> string",
    summary: "将 s 中所有匹配 pattern 的子串替换为 repl（repl 可含 $1/$2 捕获组引用）。",
    params: &[
        ("pattern", "正则表达式（string 或 regCompile 对象）"),
        ("s", "原字符串"),
        ("repl", "替换串，可含 $1 $2 ${name} 引用捕获组"),
    ],
    returns: "string 替换后的字符串",
    examples: &[
        "regReplace(\"\\\\d+\", \"a1b2\", \"#\")   → \"a#b#\"",
        "regReplace(\"(\\\\w)(\\\\d)\", \"a1b2\", \"$2$1\")  → \"1a2b\"",
    ],
    errors: &["参数顺序：pattern, s, repl"],
};

static DOC_REG_SPLIT: BuiltinDoc = BuiltinDoc {
    category: "regex",
    signature: "regSplit(pattern, s) -> array<string>",
    summary: "按 pattern 分割字符串 s。",
    params: &[
        ("pattern", "分隔正则（string 或 regCompile 对象）"),
        ("s", "被分割的字符串"),
    ],
    returns: "array<string> 分割后的片段",
    examples: &[
        "regSplit(\",\", \"a,b,c\")   → [\"a\", \"b\", \"c\"]",
        "regSplit(\"\\\\s+\", \"a  b c\") → [\"a\", \"b\", \"c\"]",
    ],
    errors: &["参数顺序：pattern 在前，s 在后"],
};

static DOC_REG_COMPILE: BuiltinDoc = BuiltinDoc {
    category: "regex",
    signature: "regCompile(pattern) -> regex|error",
    summary: "预编译正则表达式，返回可复用的 regex 对象（避免重复编译开销）。",
    params: &[("pattern", "正则表达式字符串")],
    returns: "regex 预编译对象（传给 regFind/regMatch 等的 pattern 参数）；编译失败返回 error",
    examples: &[
        "var re = regCompile(\"\\\\d+\")",
        "regFind(re, \"a12b\")        → \"12\"",
        "regMatch(re, \"123\")        → true",
    ],
    errors: &["无效的正则语法会返回 error 对象（用 isErr 检查）"],
};

static DOC_REGFINDFIRST: BuiltinDoc = BuiltinDoc {
    category: "regex",
    signature: "regFindFirst(pattern, s) -> array<string>|undefined",
    summary: "第一个匹配 + 捕获组。",
    params: &[("pattern", "正则"), ("s", "字符串")],
    returns: "array<string> [完整匹配, 捕获组1, ...]；无匹配 undefined",
    examples: &[],
    errors: &[],
};

static DOC_REGQUOTE: BuiltinDoc = BuiltinDoc {
    category: "regex",
    signature: "regQuote(s) -> string",
    summary: "转义字符串中的正则元字符。",
    params: &[("s", "字面文本")],
    returns: "string 可安全用作正则",
    examples: &["regQuote(\"1.5\")  // 1\\.5"],
    errors: &[],
};

static DOC_REGCOUNT: BuiltinDoc = BuiltinDoc {
    category: "regex",
    signature: "regCount(pattern, s) -> int",
    summary: "统计匹配次数。",
    params: &[("pattern", "正则"), ("s", "字符串")],
    returns: "int 匹配数",
    examples: &[],
    errors: &[],
};

static DOC_REGCONTAINS: BuiltinDoc = BuiltinDoc {
    category: "regex",
    signature: "regContains(pattern, s) -> bool",
    summary: "regMatch 的别名（是否整串匹配）。",
    params: &[("pattern", "正则"), ("s", "字符串")],
    returns: "bool",
    examples: &[],
    errors: &[],
};

static DOC_REGFINDALLINDEX: BuiltinDoc = BuiltinDoc {
    category: "regex",
    signature: "regFindAllIndex(pattern, s) -> array",
    summary: "全部匹配的起止位置。",
    params: &[("pattern", "正则"), ("s", "字符串")],
    returns: "array [start, end] 对",
    examples: &[],
    errors: &[],
};

static DOC_REGFINDALLGROUPS: BuiltinDoc = BuiltinDoc {
    category: "regex",
    signature: "regFindAllGroups(pattern, s) -> array<array<string>>",
    summary: "全部匹配及捕获组。",
    params: &[("pattern", "正则"), ("s", "字符串")],
    returns: "array<array<string>>",
    examples: &[],
    errors: &[],
};

static DOC_REGCONTAINSIN: BuiltinDoc = BuiltinDoc {
    category: "regex",
    signature: "regContainsIn(pattern, s) -> bool",
    summary: "判断是否包含匹配（非整串）。",
    params: &[("pattern", "正则"), ("s", "字符串")],
    returns: "bool",
    examples: &[],
    errors: &[],
};

/// register 注册所有正则内置函数到 VM。
pub fn register(vm: &mut VM) {
    vm.register_builtin_doc("regMatch", bi_reg_match, &DOC_REG_MATCH);
    vm.register_builtin_doc("regFind", bi_reg_find, &DOC_REG_FIND);
    vm.register_builtin_doc("regFindAll", bi_reg_find_all, &DOC_REG_FIND_ALL);
    vm.register_builtin_doc("regFindFirst", bi_reg_find_first, &DOC_REGFINDFIRST);
    vm.register_builtin_doc("regReplace", bi_reg_replace, &DOC_REG_REPLACE);
    vm.register_builtin_doc("regSplit", bi_reg_split, &DOC_REG_SPLIT);
    vm.register_builtin_doc("regCompile", bi_reg_compile, &DOC_REG_COMPILE);
    vm.register_builtin_doc("regQuote", bi_reg_quote, &DOC_REGQUOTE);
    vm.register_builtin_doc("regCount", bi_reg_count, &DOC_REGCOUNT);
    vm.register_builtin_doc("regContains", bi_reg_match, &DOC_REGCONTAINS);  // regMatch 的语义化别名
    vm.register_builtin_doc("regFindAllIndex", bi_reg_find_all_index, &DOC_REGFINDALLINDEX);
    vm.register_builtin_doc("regFindAllGroups", bi_reg_find_all_groups, &DOC_REGFINDALLGROUPS);
    vm.register_builtin_doc("regContainsIn", bi_reg_contains_in, &DOC_REGCONTAINSIN);
}

/// get_regex 从参数获取正则：支持 string（现场编译）或 regCompile 预编译对象。
/// 返回编译好的 Regex 引用（编译失败返回错误 Value）。
fn get_regex<'a>(arg: &'a Value) -> Result<std::borrow::Cow<'a, Regex>, Value> {
    match arg {
        Value::Str(s) => {
            let re = Regex::new(s).map_err(|e| crate::value::error_value(format!(
                "正则编译失败: '{}' - {} (可能原因：模式语法错误；注：不支持前后向断言 (?=...) 和反向引用 \\1)",
                s, e,
            )))?;
            Ok(std::borrow::Cow::Owned(re))
        }
        Value::Native(n) => {
            // regCompile 预编译对象（Arc<Regex> 包装）
            match n.downcast_ref::<Arc<Regex>>() {
                Some(re) => Ok(std::borrow::Cow::Borrowed(re)),
                None => Err(crate::value::error_value(format!(
                    "参数不是有效的预编译正则对象 (可能原因：传入了其他 native 值)",
                ))),
            }
        }
        v => Err(crate::value::error_value(format!(
            "正则模式应为 string 或 regCompile 结果，得到 {} (可能原因：参数类型错误)",
            v.type_name(),
        ))),
    }
}

/// bi_reg_match 整串是否完全匹配。
fn bi_reg_match(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let re = get_regex(&args[0])?;
    let s = bh::as_str(args, 1, "regMatch")?;
    Ok(Value::Bool(re.is_match(s)))
}

/// bi_reg_find 找第一个匹配子串。无匹配返回 undefined。
fn bi_reg_find(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let re = get_regex(&args[0])?;
    let s = bh::as_str(args, 1, "regFind")?;
    match re.find(s) {
        Some(m) => Ok(Value::str_from(m.as_str().to_string())),
        None => Ok(Value::Undefined),
    }
}

/// bi_reg_find_all 找全部匹配子串，返回 array<string>（无则空数组）。
fn bi_reg_find_all(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let re = get_regex(&args[0])?;
    let s = bh::as_str(args, 1, "regFindAll")?;
    let matches: Vec<Value> = re.find_iter(s).map(|m| Value::str_from(m.as_str().to_string())).collect();
    Ok(Value::Array(Arc::new(std::sync::Mutex::new(matches))))
}

/// bi_reg_find_first 第一个匹配 + 捕获组。
///
/// 返回 array<string>：[0]=完整匹配，[1]=捕获组1，[2]=捕获组2...
/// 无匹配返回 undefined。
fn bi_reg_find_first(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let re = get_regex(&args[0])?;
    let s = bh::as_str(args, 1, "regFindFirst")?;
    match re.captures(s) {
        Some(caps) => {
            let parts: Vec<Value> = caps.iter()
                .map(|c| match c {
                    Some(m) => Value::str_from(m.as_str().to_string()),
                    None => Value::Undefined, // 可选捕获组未匹配
                })
                .collect();
            Ok(Value::Array(Arc::new(std::sync::Mutex::new(parts))))
        }
        None => Ok(Value::Undefined),
    }
}

/// bi_reg_replace 替换全部匹配。
///
/// repl 可含 $1/$2/${name} 捕获引用（regex crate 原生支持）。
fn bi_reg_replace(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let re = get_regex(&args[0])?;
    let s = bh::as_str(args, 1, "regReplace")?;
    let repl = bh::as_str(args, 2, "regReplace")?;
    let result = re.replace_all(s, repl);
    Ok(Value::str_from(result.into_owned()))
}

/// bi_reg_split 按模式分割，返回 array<string>。
fn bi_reg_split(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let re = get_regex(&args[0])?;
    let s = bh::as_str(args, 1, "regSplit")?;
    let parts: Vec<Value> = re.split(s).map(|p| Value::str_from(p.to_string())).collect();
    Ok(Value::Array(Arc::new(std::sync::Mutex::new(parts))))
}

/// bi_reg_compile 预编译正则，返回可复用的 regex 对象（native 包装）。
///
/// 同一 pattern 多次使用时预编译可避免重复解析，提速显著。
fn bi_reg_compile(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let pattern = bh::as_str(args, 0, "regCompile")?;
    let re = Regex::new(pattern).map_err(|e| crate::value::error_value(format!(
        "regCompile() 编译失败: '{}' - {} (可能原因：模式语法错误；不支持前后向断言)",
        pattern, e,
    )))?;
    // 用 Arc<Regex> 包装为 Native 值
    Ok(Value::Native(Arc::new(Arc::new(re))))
}

/// bi_reg_quote 转义字符串中的正则特殊字符。
///
/// 将用户输入安全嵌入正则模式串。特殊字符 . * + ? ( ) [ ] { } ^ $ | \ 被加反斜杠前缀。
fn bi_reg_quote(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "regQuote")?;
    let mut out = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        match c {
            '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '^' | '$' | '|' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            other => out.push(other),
        }
    }
    Ok(Value::str_from(out))
}

/// bi_reg_count 统计正则匹配次数。
///
/// 用法：regCount(pattern, s)
fn bi_reg_count(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let re = get_regex(&args[0])?;
    let s = bh::as_str(args, 1, "regCount")?;
    Ok(Value::Int(re.find_iter(s).count() as i64))
}

/// bi_reg_find_all_index 找全部匹配的 [起始, 结束] 位置数组。
///
/// 用法：regFindAllIndex(pattern, text) → array<[start, end]>
/// 位置基于 UTF-8 字节偏移（与 regex crate 原生一致）。
/// 无匹配返回空数组。
///
/// 示例：
///   regFindAllIndex(\d+, "a12b34") → [[1, 3], [4, 6]]
fn bi_reg_find_all_index(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let re = get_regex(&args[0])?;
    let s = bh::as_str(args, 1, "regFindAllIndex")?;
    let mut result: Vec<Value> = Vec::new();
    for m in re.find_iter(s) {
        // 每个 Match 带 start()/end() 字节偏移
        let pair = vec![
            Value::Int(m.start() as i64),
            Value::Int(m.end() as i64),
        ];
        result.push(Value::Array(Arc::new(std::sync::Mutex::new(pair))));
    }
    Ok(Value::Array(Arc::new(std::sync::Mutex::new(result))))
}

/// bi_reg_find_all_groups 找全部匹配及其捕获组。
///
/// 用法：regFindAllGroups(pattern, text) → array<array<string>>
/// 每个内层数组 [0]=完整匹配, [1..]=捕获组。
/// 未匹配的可选捕获组用 undefined 填充（与 regFindFirst 一致）。
/// 无匹配返回空数组。
///
/// 示例：
///   regFindAllGroups((\w)(\d), "a1b2") → [["a1", "a", "1"], ["b2", "b", "2"]]
fn bi_reg_find_all_groups(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let re = get_regex(&args[0])?;
    let s = bh::as_str(args, 1, "regFindAllGroups")?;
    let mut result: Vec<Value> = Vec::new();
    for caps in re.captures_iter(s) {
        let group: Vec<Value> = caps.iter()
            .map(|c| match c {
                Some(m) => Value::str_from(m.as_str().to_string()),
                None => Value::Undefined, // 可选捕获组未匹配
            })
            .collect();
        result.push(Value::Array(Arc::new(std::sync::Mutex::new(group))));
    }
    Ok(Value::Array(Arc::new(std::sync::Mutex::new(result))))
}

/// bi_reg_contains_in 判断文本是否匹配 patterns 数组中任意一个正则。
///
/// 用法：regContainsIn(text, patterns) → bool
/// patterns 是 array<string>。任一匹配即返回 true，全部不匹配返回 false。
///
/// 示例：
///   regContainsIn("hello world", ["\\d+", "world"]) → true
///   regContainsIn("hello", ["\\d+", "\\s+"])         → false
fn bi_reg_contains_in(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let text = bh::as_str(args, 0, "regContainsIn")?;
    let patterns = bh::as_array(args, 1, "regContainsIn")?;
    let guard = patterns.lock().unwrap();
    for (i, p) in guard.iter().enumerate() {
        let pat = match p {
            Value::Str(s) => s.as_ref(),
            v => return Err(crate::value::error_value(format!(
                "regContainsIn() patterns 数组元素应为 string，第 {} 个为 {} (可能原因：数组元素类型不一致)",
                i + 1, v.type_name(),
            ))),
        };
        let re = Regex::new(pat).map_err(|e| crate::value::error_value(format!(
            "regContainsIn() 第 {} 个正则编译失败: '{}' - {} (可能原因：模式语法错误；注：不支持前后向断言)",
            i + 1, pat, e,
        )))?;
        if re.is_match(text) {
            return Ok(Value::Bool(true));
        }
    }
    Ok(Value::Bool(false))
}
