//! builtins_str.rs — 字符串处理内置函数
//!
//! 设计要点（来自 AGENTS.md）：
//!   - 提供常见字符串操作（大小写、裁剪、查找、替换、分割、连接等）
//!   - 错误信息 AI 友好（复用 builtins_helpers 的统一格式）
//!   - 索引语义基于"字符"（Unicode scalar），与 len() 一致
//!
//! 函数列表：
//!   upper lower trim trimStart trimEnd
//!   contains startsWith endsWith find replace split join
//!   substring repeat reverse

use std::sync::{Arc, Mutex};

use crate::builtins_helpers as bh;
use crate::function::BuiltinDoc;
use crate::value::Value;
use crate::vm::VM;

// ---- 字符串函数文档 ----

static DOC_STR_TO_UPPER: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strToUpper(s) -> string",
    summary: "将字符串转为大写（Unicode 感知）。",
    params: &[("s", "原字符串")],
    returns: "string 大写形式",
    examples: &["strToUpper(\"hello\")  → \"HELLO\"", "strToUpper(\"你好\")    → \"你好\""],
    errors: &[],
};

static DOC_STR_TO_LOWER: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strToLower(s) -> string",
    summary: "将字符串转为小写（Unicode 感知）。",
    params: &[("s", "原字符串")],
    returns: "string 小写形式",
    examples: &["strToLower(\"HELLO\")  → \"hello\""],
    errors: &[],
};

static DOC_STR_TRIM: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "trim(s) -> string",
    summary: "去除字符串首尾空白字符（空格/制表/换行）。别名 strTrim。",
    params: &[("s", "原字符串；undefined 视为空串")],
    returns: "string 去除首尾空白后的字符串",
    examples: &["trim(\"  hi  \")  → \"hi\""],
    errors: &[],
};

static DOC_STR_REPLACE: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strReplace(s, old, new) / strReplace(s, old1, new1, old2, new2, ...) -> string",
    summary: "替换字符串中的子串（全部替换）。支持多对替换依次执行。",
    params: &[
        ("s", "原字符串"),
        ("old", "要替换的子串"),
        ("new", "替换为的子串"),
    ],
    returns: "string 替换后的字符串",
    examples: &[
        "strReplace(\"a-b-c\", \"-\", \"+\")        → \"a+b+c\"",
        "strReplace(\"abc\", \"a\", \"x\", \"b\", \"y\")  → \"xyc\"",
    ],
    errors: &[],
};

static DOC_STR_SPLIT: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strSplit(sep, s) -> array<string>",
    summary: "按分隔符 sep 分割字符串 s。",
    params: &[
        ("sep", "分隔符字符串（非正则）"),
        ("s", "被分割的字符串"),
    ],
    returns: "array<string> 分割后的片段",
    examples: &[
        "strSplit(\",\", \"a,b,c\")    → [\"a\", \"b\", \"c\"]",
        "strSplit(\"\", \"abc\")       → [\"a\", \"b\", \"c\"]（空分隔符按字符分割）",
    ],
    errors: &["参数顺序：sep 在前，s 在后"],
};

static DOC_STR_JOIN: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strJoin(sep, arr) -> string",
    summary: "用分隔符 sep 连接字符串数组的各元素。",
    params: &[
        ("sep", "分隔符字符串"),
        ("arr", "待连接的字符串数组（非字符串元素自动转换）"),
    ],
    returns: "string 连接后的字符串",
    examples: &[
        "strJoin(\",\", [\"a\",\"b\",\"c\"])   → \"a,b,c\"",
        "strJoin(\"\", [\"a\",\"b\"])         → \"ab\"",
    ],
    errors: &["参数顺序：sep 在前，arr 在后"],
};

static DOC_STR_SUB: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strSub(s, start[, end]) -> string",
    summary: "截取子串。索引基于字符（Unicode scalar），负数表示从末尾倒数。",
    params: &[
        ("s", "原字符串"),
        ("start", "起始字符索引（含），负数从末尾倒数"),
        ("end", "可选。结束字符索引（不含）；省略则到末尾"),
    ],
    returns: "string 截取的子串",
    examples: &[
        "strSub(\"hello\", 1, 3)    → \"el\"",
        "strSub(\"hello\", 2)       → \"llo\"",
        "strSub(\"hello\", -2)      → \"lo\"",
    ],
    errors: &[],
};

static DOC_STR_REPEAT: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strRepeat(s, n) -> string",
    summary: "将字符串重复 n 次。",
    params: &[
        ("s", "原字符串"),
        ("n", "重复次数（int，≥0）"),
    ],
    returns: "string 重复后的字符串",
    examples: &["strRepeat(\"ab\", 3)  → \"ababab\""],
    errors: &[],
};

static DOC_STR_STARTS_WITH: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strStartsWith(s, prefix) -> bool",
    summary: "判断字符串 s 是否以 prefix 开头。",
    params: &[
        ("s", "原字符串"),
        ("prefix", "前缀字符串"),
    ],
    returns: "bool",
    examples: &["strStartsWith(\"hello\", \"he\")  → true"],
    errors: &[],
};

static DOC_STR_ENDS_WITH: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strEndsWith(s, suffix) -> bool",
    summary: "判断字符串 s 是否以 suffix 结尾。",
    params: &[
        ("s", "原字符串"),
        ("suffix", "后缀字符串"),
    ],
    returns: "bool",
    examples: &["strEndsWith(\"hello\", \"lo\")  → true"],
    errors: &[],
};

static DOC_STR_FIND: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strFind(sub, s) -> int",
    summary: "查找子串 sub 在 s 中首次出现的字符索引。",
    params: &[
        ("sub", "要查找的子串"),
        ("s", "被搜索的字符串"),
    ],
    returns: "int 首次出现的索引（0-based）；未找到返回 -1",
    examples: &[
        "strFind(\"lo\", \"hello\")   → 3",
        "strFind(\"x\", \"hello\")    → -1",
    ],
    errors: &["参数顺序：sub 在前，s 在后"],
};

static DOC_BYTESAT: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "bytesAt(b, idx) -> int",
    summary: "获取字节值（0-255）。",
    params: &[("b", "bytes/string"), ("idx", "字节索引")],
    returns: "int",
    examples: &[],
    errors: &[],
};

static DOC_BYTESGBTOUTF8STR: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "bytesGbToUtf8Str(b) -> string",
    summary: "GBK 字节转 UTF-8。",
    params: &[("b", "GBK bytes")],
    returns: "string",
    examples: &[],
    errors: &[],
};

static DOC_BYTESSLICE: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "bytesSlice(b, start[, end]) -> bytes",
    summary: "按字节切片。",
    params: &[("b", "bytes/string"), ("start", "起始"), ("end", "可选")],
    returns: "bytes",
    examples: &[],
    errors: &[],
};

static DOC_CHARFROMCODE: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "charFromCode(code) -> string",
    summary: "Unicode 码点转字符。",
    params: &[("code", "码点")],
    returns: "string",
    examples: &[],
    errors: &[],
};

static DOC_CODEOF: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "codeOf(s) -> int",
    summary: "字符转 Unicode 码点。",
    params: &[("s", "单字符")],
    returns: "int",
    examples: &[],
    errors: &[],
};

static DOC_ISUTF8: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "isUtf8(b) -> bool",
    summary: "是否有效 UTF-8。",
    params: &[("b", "bytes")],
    returns: "bool",
    examples: &[],
    errors: &[],
};

static DOC_LENBYTES: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "lenBytes(b) -> int",
    summary: "返回字节长度。",
    params: &[("b", "bytes/string")],
    returns: "int",
    examples: &[],
    errors: &[],
};

static DOC_REVERSEMAP: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "reverseMap(m) -> object",
    summary: "反转 map 键值。",
    params: &[("m", "object/map")],
    returns: "object",
    examples: &[],
    errors: &[],
};

static DOC_SIMPLESTRTOMAP: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "simpleStrToMap(s[, sep1[, sep2]]) -> object",
    summary: "解析 key=val 为 map。",
    params: &[("s", "字符串"), ("sep1", "可选"), ("sep2", "可选")],
    returns: "object",
    examples: &[],
    errors: &[],
};

static DOC_STRCONTAINSANY: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strContainsAny(s, chars) -> bool",
    summary: "是否包含任意指定字符。",
    params: &[("s", "字符串"), ("chars", "字符集")],
    returns: "bool",
    examples: &[],
    errors: &[],
};

static DOC_STRCONTAINSIN: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strContainsIn(s, arr) -> bool",
    summary: "是否包含数组中任意子串。",
    params: &[("s", "字符串"), ("arr", "子串数组")],
    returns: "bool",
    examples: &[],
    errors: &[],
};

static DOC_STRCOUNT: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strCount(s, sub) -> int",
    summary: "统计子串出现次数。",
    params: &[("s", "字符串"), ("sub", "子串")],
    returns: "int",
    examples: &[],
    errors: &[],
};

static DOC_STRFINDDIFFPOS: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strFindDiffPos(a, b) -> int",
    summary: "首个不同位置。",
    params: &[("a", "字符串1"), ("b", "字符串2")],
    returns: "int；相同返回 -1",
    examples: &[],
    errors: &[],
};

static DOC_STRLIMIT: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strLimit(s, maxLen[, suffix]) -> string",
    summary: "截断到 maxLen 字符。",
    params: &[("s", "字符串"), ("maxLen", "最大长度"), ("suffix", "可选。默认 ...")],
    returns: "string",
    examples: &[],
    errors: &[],
};

static DOC_STRPAD: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strPad(s, len[, pad[, align]]) -> string",
    summary: "填充到指定长度。",
    params: &[("s", "字符串"), ("len", "目标长度"), ("pad", "可选"), ("align", "可选。left/right/center")],
    returns: "string",
    examples: &[],
    errors: &[],
};

static DOC_STRQUOTE: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strQuote(s) -> string",
    summary: "用双引号包裹并转义。",
    params: &[("s", "字符串")],
    returns: "string",
    examples: &[],
    errors: &[],
};

static DOC_STRREMOVEBOMHEAD: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strRemoveBomHead(s) -> string",
    summary: "去除 UTF-8 BOM。",
    params: &[("s", "字符串")],
    returns: "string",
    examples: &[],
    errors: &[],
};

static DOC_STRREPLACEN: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strReplaceN(s, old, new, n) -> string",
    summary: "替换前 n 个匹配。",
    params: &[("s", "字符串"), ("old", "旧子串"), ("new", "新子串"), ("n", "次数")],
    returns: "string",
    examples: &[],
    errors: &[],
};

static DOC_STRSPLITLINES: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strSplitLines(s) -> array<string>",
    summary: "按行分割。",
    params: &[("s", "多行文本")],
    returns: "array<string>",
    examples: &[],
    errors: &[],
};

static DOC_STRSPLITN: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strSplitN(sep, s, n) -> array<string>",
    summary: "分割，最多 n 段。",
    params: &[("sep", "分隔符"), ("s", "字符串"), ("n", "最大段数")],
    returns: "array<string>",
    examples: &[],
    errors: &[],
};

static DOC_STRSUBBYTES: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strSubBytes(s, start[, end]) -> string",
    summary: "按字节截取子串。",
    params: &[("s", "字符串"), ("start", "字节起始"), ("end", "可选")],
    returns: "string",
    examples: &[],
    errors: &[],
};

static DOC_STRTOFLOAT: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strToFloat(s) -> float|error",
    summary: "字符串转浮点。",
    params: &[("s", "数字字符串")],
    returns: "float；失败 error",
    examples: &[],
    errors: &[],
};

static DOC_STRTOGBKBYTES: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strToGbkBytes(s) -> bytes",
    summary: "UTF-8 转 GBK 字节。",
    params: &[("s", "字符串")],
    returns: "bytes",
    examples: &[],
    errors: &[],
};

static DOC_STRTOINT: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strToInt(s) -> int|error",
    summary: "字符串转整数。",
    params: &[("s", "数字字符串")],
    returns: "int；失败 error",
    examples: &[],
    errors: &[],
};

static DOC_STRTOUTF8: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strToUtf8(b) -> string",
    summary: "字节转 UTF-8 字符串。",
    params: &[("b", "bytes")],
    returns: "string",
    examples: &[],
    errors: &[],
};

static DOC_STRTRIMLEFT: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strTrimLeft(s, cutset) -> string",
    summary: "去除左侧指定字符集。",
    params: &[("s", "字符串"), ("cutset", "字符集")],
    returns: "string",
    examples: &[],
    errors: &[],
};

static DOC_STRTRIMPREFIX: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strTrimPrefix(s, prefix) -> string",
    summary: "去除头部子串（如有）。",
    params: &[("s", "字符串"), ("prefix", "前缀")],
    returns: "string",
    examples: &[],
    errors: &[],
};

static DOC_STRTRIMRIGHT: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strTrimRight(s, cutset) -> string",
    summary: "去除右侧指定字符集。",
    params: &[("s", "字符串"), ("cutset", "字符集")],
    returns: "string",
    examples: &[],
    errors: &[],
};

static DOC_STRTRIMSUFFIX: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strTrimSuffix(s, suffix) -> string",
    summary: "去除尾部子串（如有）。",
    params: &[("s", "字符串"), ("suffix", "后缀")],
    returns: "string",
    examples: &[],
    errors: &[],
};

static DOC_STRUNQUOTE: BuiltinDoc = BuiltinDoc {
    category: "string",
    signature: "strUnquote(s) -> string",
    summary: "去除引号并反转义。",
    params: &[("s", "带引号字符串")],
    returns: "string",
    examples: &[],
    errors: &[],
};

/// register 注册所有字符串内置函数到 VM。
///
/// 注：contains / reverse 与数组模块重名，由数组模块注册为多态版本
/// （同时支持 string 与 array），此处不重复注册。
pub fn register(vm: &mut VM) {
    // 字符串专有函数（加 str 前缀，对标 Charlang）
    vm.register_builtin_doc("strToUpper", bi_upper, &DOC_STR_TO_UPPER);
    vm.register_builtin_doc("strToLower", bi_lower, &DOC_STR_TO_LOWER);
    vm.register_builtin_doc("strTrim", bi_trim, &DOC_STR_TRIM);
    vm.register_builtin_doc("trim", bi_trim, &DOC_STR_TRIM);
    vm.register_builtin_doc("strTrimPrefix", bi_trim_start, &DOC_STRTRIMPREFIX); // 去头部子串
    vm.register_builtin_doc("strTrimSuffix", bi_trim_end, &DOC_STRTRIMSUFFIX);   // 去尾部子串
    vm.register_builtin_doc("strStartsWith", bi_starts_with, &DOC_STR_STARTS_WITH);
    vm.register_builtin_doc("strEndsWith", bi_ends_with, &DOC_STR_ENDS_WITH);
    vm.register_builtin_doc("strFind", bi_find, &DOC_STR_FIND);
    vm.register_builtin_doc("strReplace", bi_str_replace, &DOC_STR_REPLACE);
    vm.register_builtin_doc("strSplit", bi_split, &DOC_STR_SPLIT);
    vm.register_builtin_doc("strJoin", bi_join, &DOC_STR_JOIN);
    vm.register_builtin_doc("strSub", bi_substring, &DOC_STR_SUB);
    vm.register_builtin_doc("strSubBytes", bi_str_sub_bytes, &DOC_STRSUBBYTES);
    vm.register_builtin_doc("strRepeat", bi_repeat, &DOC_STR_REPEAT);
    // 按字符集裁剪
    vm.register_builtin_doc("strTrimLeft", bi_str_trim_left, &DOC_STRTRIMLEFT);
    vm.register_builtin_doc("strTrimRight", bi_str_trim_right, &DOC_STRTRIMRIGHT);
    // 其他字符串函数
    vm.register_builtin_doc("strCount", bi_str_count, &DOC_STRCOUNT);
    vm.register_builtin_doc("strLimit", bi_limit_str, &DOC_STRLIMIT);
    vm.register_builtin_doc("strPad", bi_str_pad, &DOC_STRPAD);
    vm.register_builtin_doc("strSplitN", bi_str_split_n, &DOC_STRSPLITN);
    vm.register_builtin_doc("strReplaceN", bi_str_replace_n, &DOC_STRREPLACEN);
    vm.register_builtin_doc("strSplitLines", bi_str_split_lines, &DOC_STRSPLITLINES);
    vm.register_builtin_doc("strQuote", bi_str_quote, &DOC_STRQUOTE);
    vm.register_builtin_doc("strUnquote", bi_str_unquote, &DOC_STRUNQUOTE);
    // string 字节级访问
    vm.register_builtin_doc("bytesSlice", bi_bytes_slice, &DOC_BYTESSLICE);
    vm.register_builtin_doc("bytesAt", bi_bytes_at, &DOC_BYTESAT);
    vm.register_builtin_doc("lenBytes", bi_len_bytes, &DOC_LENBYTES);
    // 码点 ↔ 字符转换
    vm.register_builtin_doc("charFromCode", bi_char_from_code, &DOC_CHARFROMCODE);
    vm.register_builtin_doc("codeOf", bi_code_of, &DOC_CODEOF);
    // contains / reverse 由 builtins_arr 多态实现（同时支持 string 与 array）
    // 对标 Charlang 补充
    vm.register_builtin_doc("strToInt", bi_str_to_int, &DOC_STRTOINT);
    vm.register_builtin_doc("strToFloat", bi_str_to_float, &DOC_STRTOFLOAT);
    vm.register_builtin_doc("strContainsAny", bi_str_contains_any, &DOC_STRCONTAINSANY);
    vm.register_builtin_doc("strContainsIn", bi_str_contains_in, &DOC_STRCONTAINSIN);
    // 编码与字符串分析
    vm.register_builtin_doc("strFindDiffPos", bi_str_find_diff_pos, &DOC_STRFINDDIFFPOS);
    vm.register_builtin_doc("strRemoveBomHead", bi_str_remove_bom_head, &DOC_STRREMOVEBOMHEAD);
    vm.register_builtin_doc("strToUtf8", bi_str_to_utf8, &DOC_STRTOUTF8);
    vm.register_builtin_doc("bytesGbToUtf8Str", bi_bytes_gb_to_utf8_str, &DOC_BYTESGBTOUTF8STR);
    vm.register_builtin_doc("strToGbkBytes", bi_str_to_gbk_bytes, &DOC_STRTOGBKBYTES);
    vm.register_builtin_doc("isUtf8", bi_is_utf8, &DOC_ISUTF8);
    vm.register_builtin_doc("simpleStrToMap", bi_simple_str_to_map, &DOC_SIMPLESTRTOMAP);
    vm.register_builtin_doc("reverseMap", bi_reverse_map, &DOC_REVERSEMAP);
}

fn s_owned(t: String) -> Value {
    Value::str_from(t)
}

/// bi_upper 转大写。
fn bi_upper(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(s_owned(bh::as_str(args, 0, "strToUpper")?.to_uppercase()))
}

/// bi_lower 转小写。
fn bi_lower(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(s_owned(bh::as_str(args, 0, "strToLower")?.to_lowercase()))
}

/// bi_trim 去除两端空白，同时将 undefined 转为空字符串（跨类型，对标 Charlang trim）。
///
/// 这是常用的判空模式：trim(map["missing"]) → "" 而非报错。
fn bi_trim(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = match args.get(0) {
        Some(Value::Str(s)) => s.to_string(),
        Some(Value::Undefined) | None => String::new(),
        Some(v) => v.to_str(),
    };
    Ok(s_owned(s.trim().to_string()))
}

/// bi_trim_start 去除头部子串（Go TrimPrefix 语义，非去空白）。
///
/// strTrimPrefix("hello.txt", "hello.") → "txt"
/// strTrimPrefix("abc", "xyz") → "abc"（无匹配则原样返回）
fn bi_trim_start(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "strTrimPrefix")?;
    let prefix = bh::as_str(args, 1, "strTrimPrefix")?;
    if let Some(rest) = s.strip_prefix(prefix) {
        Ok(s_owned(rest.to_string()))
    } else {
        Ok(s_owned(s.to_string()))
    }
}

/// bi_trim_end 去除尾部子串（Go TrimSuffix 语义，非去空白）。
///
/// strTrimSuffix("hello.txt", ".txt") → "hello"
fn bi_trim_end(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "strTrimSuffix")?;
    let suffix = bh::as_str(args, 1, "strTrimSuffix")?;
    if let Some(rest) = s.strip_suffix(suffix) {
        Ok(s_owned(rest.to_string()))
    } else {
        Ok(s_owned(s.to_string()))
    }
}

/// bi_contains 判断字符串是否包含子串（pub(crate) 供数组模块多态分发）。
pub(crate) fn bi_contains_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let h = bh::as_str(args, 0, "contains")?;
    let n = bh::as_str(args, 1, "contains")?;
    Ok(Value::Bool(h.contains(n)))
}

/// bi_starts_with 判断前缀。
fn bi_starts_with(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let h = bh::as_str(args, 0, "startsWith")?;
    let n = bh::as_str(args, 1, "startsWith")?;
    Ok(Value::Bool(h.starts_with(n)))
}

/// bi_ends_with 判断后缀。
fn bi_ends_with(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let h = bh::as_str(args, 0, "endsWith")?;
    let n = bh::as_str(args, 1, "endsWith")?;
    Ok(Value::Bool(h.ends_with(n)))
}

/// bi_find 查找子串，返回首个匹配的字符索引；未找到返回 -1。
///
/// 注意：索引基于字符（与 len() 一致），非字节偏移。
fn bi_find(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    // strFind(sub, s)：在 s 中查找 sub
    let sub = bh::as_str(args, 0, "strFind")?;
    let s = bh::as_str(args, 1, "strFind")?;
    match s.find(sub) {
        // find 返回字节偏移，需转换为字符索引。
        Some(byte_off) => {
            let char_idx = s[..byte_off].chars().count() as i64;
            Ok(Value::Int(char_idx))
        }
        None => Ok(Value::Int(-1)),
    }
}

/// bi_str_replace 替换子串，支持多对替换。
///
/// 用法：
///   strReplace(s, old, new)                      — 替换所有 old → new
///   strReplace(s, old1, new1, old2, new2, ...)   — 多对替换（依次执行）
fn bi_str_replace(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.len() < 3 {
        return Err(crate::value::error_value("strReplace() 需要至少 3 个参数 (s, old, new)"));
    }
    let mut result = bh::as_str(args, 0, "strReplace")?.to_string();
    // 按对处理 (old, new)
    let mut i = 1;
    while i + 1 < args.len() {
        let old = bh::as_str(args, i, "strReplace")?;
        let new = bh::as_str(args, i + 1, "strReplace")?;
        if !old.is_empty() {
            result = result.replace(old, new);
        }
        i += 2;
    }
    Ok(s_owned(result))
}

/// bi_split 按分隔符切分为字符串数组。
fn bi_split(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    // strSplit(sep, s)：按分隔符 sep 分割字符串 s
    let sep = bh::as_str(args, 0, "strSplit")?;
    let src = bh::as_str(args, 1, "strSplit")?;
    let parts: Vec<Value> = if sep.is_empty() {
        // 空分隔符：按字符切分
        src.chars().map(|c| Value::str_from(c.to_string())).collect()
    } else {
        src.split(sep).map(|p| Value::str_from(p.to_string())).collect()
    };
    Ok(Value::Array(Arc::new(Mutex::new(parts))))
}

/// bi_join 将数组元素用分隔符连接成字符串。
fn bi_join(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let arr = bh::as_array(args, 0, "join")?;
    let sep = bh::as_str(args, 1, "join")?;
    let elems = arr.lock().unwrap();
    let joined = elems.iter().map(|v| v.to_str()).collect::<Vec<_>>().join(sep);
    Ok(s_owned(joined))
}

/// bi_substring 取子串 [start, end)（字符索引，含 start 不含 end）。
///
/// end 省略时取到末尾。负数索引按"距末端"解释（-1 表示最后一个字符）。
fn bi_substring(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let src = bh::as_str(args, 0, "substring")?;
    let chars: Vec<char> = src.chars().collect();
    let len = chars.len() as i64;
    let mut start = bh::as_int(args, 1, "substring")?;
    let mut end = if args.len() > 2 {
        bh::as_int(args, 2, "substring")?
    } else {
        len
    };
    // 负数索引转换为距末端的正索引
    if start < 0 {
        start += len;
    }
    if end < 0 {
        end += len;
    }
    if start < 0 {
        start = 0;
    }
    if end > len {
        end = len;
    }
    if start >= end {
        return Ok(Value::str(""));
    }
    let result: String = chars[(start as usize)..(end as usize)].iter().collect();
    Ok(s_owned(result))
}

/// bi_repeat 重复字符串 n 次。
fn bi_repeat(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let src = bh::as_str(args, 0, "repeat")?;
    let n = bh::as_int(args, 1, "repeat")?;
    if n < 0 {
        return Err(crate::value::error_value(
            "repeat() 次数不能为负数 (可能原因：参数顺序错误；正确顺序 repeat(str, n))",
        ));
    }
    Ok(s_owned(src.repeat(n as usize)))
}

/// bi_reverse_str 反转字符串（按字符，非字节）（pub(crate) 供数组模块多态分发）。
pub(crate) fn bi_reverse_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let src = bh::as_str(args, 0, "reverse")?;
    let rev: String = src.chars().rev().collect();
    Ok(s_owned(rev))
}

// ---- string 字节级访问（与按字符的 s[i]/s[i:j] 互补）----

/// bi_bytes_slice 按 UTF-8 字节切片 string，返回不可变 bytes。
///
/// 用于协议解析、手动 UTF-8 处理等需要字节级访问的场景。
/// 注：可能切断多字节字符（与按字符的 s[i:j] 切片不同）。
fn bi_bytes_slice(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "bytesSlice")?;
    let bytes = s.as_bytes();
    let n = bytes.len() as i64;
    let start = bh::as_int(args, 1, "bytesSlice")?;
    let mut start = if start < 0 { start + n } else { start };
    let end = if args.len() > 2 {
        let mut e = bh::as_int(args, 2, "bytesSlice")?;
        if e < 0 { e += n; }
        e
    } else {
        n
    };
    if start < 0 { start = 0; }
    let end = if end > n { n } else { end };
    if start >= end {
        return Ok(Value::Bytes(std::sync::Arc::new(Vec::new())));
    }
    let part = bytes[(start as usize)..(end as usize)].to_vec();
    Ok(Value::Bytes(std::sync::Arc::new(part)))
}

/// bi_bytes_at 取 string 第 i 字节（0-255），返回 int。
///
/// 越界报错。负索引支持。
fn bi_bytes_at(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "bytesAt")?;
    let bytes = s.as_bytes();
    let n = bytes.len() as i64;
    let mut i = bh::as_int(args, 1, "bytesAt")?;
    if i < 0 { i += n; }
    if i < 0 || i >= n {
        return Err(crate::value::error_value(format!(
            "bytesAt() 索引 {} 越界 (len={}); 可能原因：索引超出字节数", i, n,
        )));
    }
    Ok(Value::Byte(bytes[i as usize]))
}

/// bi_len_bytes 返回 string 的 UTF-8 字节数。
///
/// 区别于 len(s)（字符数）：len("中")=1，lenBytes("中")=3。
fn bi_len_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "lenBytes")?;
    Ok(Value::Int(s.as_bytes().len() as i64))
}

/// bi_char_from_code 将 Unicode 码点（int）转为单字符 string。
///
/// 与 s[i] 配对：charFromCode(s[i]) 得到原字符。
/// 非法码点（代理区 0xD800-0xDFFF 或 > 0x10FFFF）报错。
fn bi_char_from_code(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let code = bh::as_int(args, 0, "charFromCode")?;
    if code < 0 || code > 0x10FFFF {
        return Err(crate::value::error_value(format!(
            "charFromCode() 码点 {} 超出有效范围 (0-1114111); 可能原因：传入了负数或过大值",
            code,
        )));
    }
    // 排除 UTF-16 代理区（0xD800-0xDFFF，不是合法 Unicode 码点）
    if (0xD800..=0xDFFF).contains(&code) {
        return Err(crate::value::error_value(format!(
            "charFromCode() 码点 {} 在代理区 (D800-DFFF)，不是合法字符; 可能原因：传入了代理区码点",
            code,
        )));
    }
    match char::from_u32(code as u32) {
        Some(c) => Ok(Value::str_from(c.to_string())),
        None => Err(crate::value::error_value(format!(
            "charFromCode() 码点 {} 无法转为字符; 可能原因：非法码点", code,
        ))),
    }
}

/// bi_code_of 返回单字符 string 的 Unicode 码点（int）。
///
/// 与 charFromCode 互逆。要求 string 长度恰为 1 字符。
fn bi_code_of(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "codeOf")?;
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Ok(Value::Int(c as u32 as i64)),
        _ => Err(crate::value::error_value(
            "codeOf() 参数需为恰好 1 个字符的 string (可能原因：传入空串或多字符 string)",
        )),
    }
}

// ---- 新增字符串函数（对标 Charlang）----

/// bi_str_trim_left 去除左侧指定的字符集（cutset）。
///
/// 与 strTrimStart 不同：strTrimStart 去空白，strTrimLeft 去指定字符集。
/// 例如 strTrimLeft("123abc", "0123456789") → "abc"
fn bi_str_trim_left(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "strTrimLeft")?;
    let cutset = bh::as_str(args, 1, "strTrimLeft")?;
    let cutset_chars: std::collections::HashSet<char> = cutset.chars().collect();
    let trimmed: &str = s.trim_start_matches(|c| cutset_chars.contains(&c));
    Ok(s_owned(trimmed.to_string()))
}

/// bi_str_trim_right 去除右侧指定的字符集（cutset）。
fn bi_str_trim_right(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "strTrimRight")?;
    let cutset = bh::as_str(args, 1, "strTrimRight")?;
    let cutset_chars: std::collections::HashSet<char> = cutset.chars().collect();
    let trimmed: &str = s.trim_end_matches(|c| cutset_chars.contains(&c));
    Ok(s_owned(trimmed.to_string()))
}

/// bi_limit_str 截断字符串到指定长度，超出部分用后缀替代。
///
/// 用法：limitStr(s, maxLen) 或 limitStr(s, maxLen, suffix)
/// 默认 suffix = "..."（省略号）。
/// 按字符计算长度（非字节），不切断多字节字符。
///
/// 示例：
///   limitStr("Hello World", 5)        → "He..."（截断到 5 字符，加省略号）
///   limitStr("Hello World", 5, "...")  → "He..."（同上，显式指定后缀）
///   limitStr("Hi", 10)                → "Hi"（未超长，原样返回）
///   limitStr("中文测试", 3)             → "中..."（按字符截断）
fn bi_limit_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "limitStr")?;
    let max_len = bh::as_int(args, 1, "limitStr")? as usize;
    let suffix = if args.len() > 2 { bh::as_str(args, 2, "limitStr")?.to_string() } else { "...".to_string() };
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_len {
        return Ok(s_owned(s.to_string()));
    }
    let suffix_len = suffix.chars().count();
    let take = if max_len > suffix_len { max_len - suffix_len } else { 0 };
    let result: String = chars[..take].iter().collect::<String>() + &suffix;
    Ok(s_owned(result))
}

/// bi_str_count 统计子串出现次数。
fn bi_str_count(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "strCount")?;
    let sub = bh::as_str(args, 1, "strCount")?;
    if sub.is_empty() {
        return Ok(Value::Int(0));
    }
    Ok(Value::Int(s.matches(sub).count() as i64))
}

/// bi_str_pad 字符串填充到指定长度。
///
/// 用法：
///   strPad(s, len)                — 左填充 "0" 到 len 个字符
///   strPad(s, len, fill)          — 左填充指定字符
///   strPad(s, len, fill, true)    — 右填充（第 4 参数 true=右填充，false/省略=左填充）
///
/// 示例：
///   strPad("42", 5)           → "00042"（左补零）
///   strPad("42", 5, " ")      → "   42"（左补空格）
///   strPad("42", 5, " ", true) → "42   "（右补空格）
fn bi_str_pad(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "strPad")?;
    let target_len = bh::as_int(args, 1, "strPad")? as usize;
    let fill = if args.len() > 2 { bh::as_str(args, 2, "strPad")?.to_string() } else { "0".to_string() };
    let right = if args.len() > 3 { args[3].is_truthy() } else { false };
    let cur_len = s.chars().count();
    if cur_len >= target_len || fill.is_empty() {
        return Ok(s_owned(s.to_string()));
    }
    let need = target_len - cur_len;
    let fill_chars: Vec<char> = fill.chars().collect();
    let mut padding = String::new();
    for i in 0..need {
        padding.push(fill_chars[i % fill_chars.len()]);
    }
    if right {
        Ok(s_owned(format!("{}{}", s, padding)))
    } else {
        Ok(s_owned(format!("{}{}", padding, s)))
    }
}

/// bi_str_split_n 按分隔符分割，限制最多 n 段。
fn bi_str_split_n(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let src = bh::as_str(args, 0, "strSplitN")?;
    let sep = bh::as_str(args, 1, "strSplitN")?;
    let n = bh::as_int(args, 2, "strSplitN")? as usize;
    if n <= 0 || sep.is_empty() {
        return Ok(Value::Array(Arc::new(Mutex::new(vec![s_owned(src.to_string())]))));
    }
    let parts: Vec<Value> = src.splitn(n, sep).map(|p| s_owned(p.to_string())).collect();
    Ok(Value::Array(Arc::new(Mutex::new(parts))))
}

/// bi_str_replace_n 替换前 n 个匹配（n=-1 或省略表示全部）。
fn bi_str_replace_n(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let src = bh::as_str(args, 0, "strReplaceN")?;
    let old = bh::as_str(args, 1, "strReplaceN")?;
    let new = bh::as_str(args, 2, "strReplaceN")?;
    let count = if args.len() > 3 {
        bh::as_int(args, 3, "strReplaceN")?
    } else {
        -1
    };
    if old.is_empty() {
        return Ok(s_owned(src.to_string()));
    }
    if count < 0 {
        return Ok(s_owned(src.replace(old, new)));
    }
    Ok(s_owned(src.replacen(old, new, count as usize)))
}

/// bi_str_split_lines 按行分割（兼容 \n 和 \r\n）。
fn bi_str_split_lines(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let src = bh::as_str(args, 0, "strSplitLines")?;
    let lines: Vec<Value> = src.lines().map(|l| s_owned(l.to_string())).collect();
    Ok(Value::Array(Arc::new(Mutex::new(lines))))
}

/// bi_str_quote 给字符串加双引号并转义特殊字符。
fn bi_str_quote(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "strQuote")?;
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\t', "\\t");
    Ok(s_owned(format!("\"{}\"", escaped)))
}

/// bi_str_unquote 去除字符串的双引号并解转义。
fn bi_str_unquote(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "strUnquote")?;
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        let inner = &s[1..s.len()-1];
        let unescaped = inner
            .replace("\\n", "\n")
            .replace("\\t", "\t")
            .replace("\\\"", "\"")
            .replace("\\\\", "\\");
        Ok(s_owned(unescaped))
    } else {
        Ok(s_owned(s.to_string()))
    }
}

/// bi_str_sub_bytes 按字节截取子串（UTF-8 字节索引）。
///
/// 与 strSub（按字符）不同，strSubBytes 按 UTF-8 字节偏移截取。
/// 可能切断多字节字符（类似 Go 的 s[start:end]），适合协议解析等场景。
///
/// 用法：strSubBytes(s, start) 或 strSubBytes(s, start, end)
fn bi_str_sub_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let src = bh::as_str(args, 0, "strSubBytes")?;
    let bytes = src.as_bytes();
    let len = bytes.len() as i64;
    let mut start = bh::as_int(args, 1, "strSubBytes")?;
    let mut end = if args.len() > 2 {
        bh::as_int(args, 2, "strSubBytes")?
    } else {
        len
    };
    if start < 0 { start += len; }
    if end < 0 { end += len; }
    if start < 0 { start = 0; }
    if end > len { end = len; }
    if start >= end {
        return Ok(s_owned(String::new()));
    }
    let slice = &bytes[start as usize..end as usize];
    Ok(s_owned(String::from_utf8_lossy(slice).into_owned()))
}

// ---- 对标 Charlang 补充 ----

/// bi_str_to_int 字符串转整数，失败返回默认值（不报错）。
///
/// 用法：strToInt("42", 0) → 42
///       strToInt("abc", -1) → -1
fn bi_str_to_int(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "strToInt")?;
    let default = if args.len() > 1 {
        bh::as_int(args, 1, "strToInt")?
    } else {
        0
    };
    match s.trim().parse::<i64>() {
        Ok(n) => Ok(Value::Int(n)),
        Err(_) => Ok(Value::Int(default)),
    }
}

/// bi_str_to_float 字符串转浮点，失败返回默认值（不报错）。
///
/// 用法：strToFloat("3.14", 0.0) → 3.14
///       strToFloat("abc", 0.0) → 0.0
fn bi_str_to_float(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "strToFloat")?;
    let default = if args.len() > 1 {
        bh::as_float(args, 1, "strToFloat")?
    } else {
        0.0
    };
    match s.trim().parse::<f64>() {
        // 过滤 NaN/Infinity（通常不是期望的有限数字）
        Ok(n) if n.is_finite() => Ok(Value::Float(n)),
        _ => Ok(Value::Float(default)),
    }
}

/// bi_str_contains_any 检查字符串是否包含字符集中的任意字符。
///
/// 用法：strContainsAny("hello", "aeiou") → true（包含 e/o）
///       strContainsAny("xyz", "aeiou") → false
fn bi_str_contains_any(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "strContainsAny")?;
    let chars = bh::as_str(args, 1, "strContainsAny")?;
    let char_set: std::collections::HashSet<char> = chars.chars().collect();
    Ok(Value::Bool(s.chars().any(|c| char_set.contains(&c))))
}

/// bi_str_contains_in 检查字符串是否包含多个子串中的任意一个。
///
/// 用法：strContainsIn("hello world", ["world", "python"]) → true
///       strContainsIn("hello", ["foo", "bar"]) → false
fn bi_str_contains_in(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "strContainsIn")?;
    let subs = bh::as_array(args, 1, "strContainsIn")?;
    let guard = subs.lock().unwrap();
    for sub in guard.iter() {
        let sub_str = sub.to_str();
        if s.contains(&sub_str) {
            return Ok(Value::Bool(true));
        }
    }
    Ok(Value::Bool(false))
}

// ---- 编码与字符串分析 ----

/// bi_str_find_diff_pos 找两个字符串第一个不同字符的位置（按 Unicode 字符计数）。
///
/// 用法：strFindDiffPos(s1, s2) → int
/// 完全相同返回 -1。较短字符串耗尽时返回其长度（即"位置 i 处一个有字符，另一个已结束"）。
///
/// 示例：
///   strFindDiffPos("abc", "abd") → 2
///   strFindDiffPos("abc", "abc") → -1
///   strFindDiffPos("ab",  "abc") → 2
fn bi_str_find_diff_pos(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s1 = bh::as_str(args, 0, "strFindDiffPos")?;
    let s2 = bh::as_str(args, 1, "strFindDiffPos")?;
    let c1: Vec<char> = s1.chars().collect();
    let c2: Vec<char> = s2.chars().collect();
    let min_len = c1.len().min(c2.len());
    for i in 0..min_len {
        if c1[i] != c2[i] {
            return Ok(Value::Int(i as i64));
        }
    }
    // 公共前缀完全相同：若长度一致视为相等，否则较短字符串结束位置即差异点
    if c1.len() == c2.len() {
        Ok(Value::Int(-1))
    } else {
        Ok(Value::Int(min_len as i64))
    }
}

/// bi_str_remove_bom_head 去除字符串开头的 UTF-8 BOM（\xEF\xBB\xBF），如果有的话。
///
/// BOM 是 U+FEFF 字符的 UTF-8 编码三字节序列。返回新字符串（无 BOM 则原样返回）。
///
/// 示例：
///   strRemoveBomHead("\u{FEFF}hello") → "hello"
///   strRemoveBomHead("hello")         → "hello"
fn bi_str_remove_bom_head(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "strRemoveBomHead")?;
    // U+FEFF 即 UTF-8 BOM 字符
    if let Some(rest) = s.strip_prefix('\u{FEFF}') {
        Ok(s_owned(rest.to_string()))
    } else {
        Ok(s_owned(s.to_string()))
    }
}

/// bi_str_to_utf8 将字符串转为 UTF-8 编码的 bytes（即 string.as_bytes()）。
///
/// 与 bytes(s) 等价，提供语义化命名。
///
/// 示例：
///   strToUtf8("中") → bytes(3)  （"中" 的 UTF-8 编码为 3 字节）
fn bi_str_to_utf8(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "strToUtf8")?;
    Ok(Value::Bytes(std::sync::Arc::new(s.as_bytes().to_vec())))
}

/// bytes_to_vec 将 string/bytes/byteArray 统一转为 Vec<u8>（内部辅助函数）。
///
/// 接受类型：
///   string    — UTF-8 编码字节
///   bytes     — 不可变字节序列（拷贝）
///   byteArray — 可变字节序列（拷贝）
fn bytes_to_vec(arg: &Value, fn_name: &str) -> Result<Vec<u8>, Value> {
    match arg {
        Value::Str(s) => Ok(s.as_bytes().to_vec()),
        Value::Bytes(b) => Ok(b.as_ref().to_vec()),
        Value::ByteArray(b) => Ok(b.lock().unwrap().clone()),
        v => Err(crate::value::error_value(format!(
            "{}() 参数应为 string/bytes/byteArray，得到 {} (可能原因：参数类型不匹配)",
            fn_name, v.type_name(),
        ))),
    }
}

/// bi_bytes_gb_to_utf8_str 将 GBK 编码的字节转为 UTF-8 字符串。
///
/// 参数接受 string/bytes/byteArray。用 encoding_rs::GBK.decode。
///
/// 示例：
///   bytesGbToUtf8Str(b) → string  （b 是 GBK 编码的字节序列）
fn bi_bytes_gb_to_utf8_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let bytes = bytes_to_vec(&args[0], "bytesGbToUtf8Str")?;
    // encoding_rs::GBK.decode 返回 (Cow<str>, &Encoding, bool)
    let (cow, _, _) = encoding_rs::GBK.decode(&bytes);
    Ok(s_owned(cow.into_owned()))
}

/// bi_str_to_gbk_bytes 将字符串编码为 GBK 字节。
///
/// 用 encoding_rs::GBK.encode。无法用 GBK 表示的字符会被替换为问号 '?'。
///
/// 示例：
///   strToGbkBytes("中文") → bytes  （GBK 编码的字节序列）
fn bi_str_to_gbk_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "strToGbkBytes")?;
    let (cow, _, _) = encoding_rs::GBK.encode(s);
    Ok(Value::Bytes(std::sync::Arc::new(cow.into_owned())))
}

/// bi_is_utf8 判断字节序列是否为有效 UTF-8。
///
/// 参数接受 string/bytes/byteArray。用 std::str::from_utf8 判断。
///
/// 示例：
///   isUtf8(b)        → bool  （b 是 bytes/byteArray/string）
///   isUtf8("hello")  → true
fn bi_is_utf8(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let bytes = bytes_to_vec(&args[0], "isUtf8")?;
    Ok(Value::Bool(std::str::from_utf8(&bytes).is_ok()))
}

/// bi_simple_str_to_map 简单字符串转 Map。
///
/// 用法：simpleStrToMap(s, pairSep, kvSep) → Map
/// 如 "a=1,b=2,c=3" → map{a: "1", b: "2", c: "3"}
/// 空字符串返回空 Map。键值都按字符串处理。
///
/// 示例：
///   simpleStrToMap("a=1,b=2", ",", "=") → map{a: "1", b: "2"}
///   simpleStrToMap("x:1;y:2", ";", ":")  → map{x: "1", y: "2"}
fn bi_simple_str_to_map(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "simpleStrToMap")?;
    let pair_sep = bh::as_str(args, 1, "simpleStrToMap")?;
    let kv_sep = bh::as_str(args, 2, "simpleStrToMap")?;
    let mut om = crate::ord_map::OrdMap::new();
    if s.is_empty() {
        return Ok(Value::Map(std::sync::Arc::new(std::sync::Mutex::new(om))));
    }
    // 空分隔符保护：split 在空串上会产出无限空段
    if pair_sep.is_empty() || kv_sep.is_empty() {
        return Err(crate::value::error_value(
            "simpleStrToMap() pairSep 与 kvSep 不能为空 (可能原因：分隔符参数顺序错误；正确顺序 simpleStrToMap(s, pairSep, kvSep))",
        ));
    }
    for pair in s.split(pair_sep) {
        // 用 splitn(2, kv_sep) 避免值中含 kvSep 时被切断
        let mut parts = pair.splitn(2, kv_sep);
        let key = match parts.next() {
            Some(k) => k.to_string(),
            None => continue,
        };
        let val = parts.next().unwrap_or("").to_string();
        om.set(key, Value::str_from(val));
    }
    Ok(Value::Map(std::sync::Arc::new(std::sync::Mutex::new(om))))
}

/// bi_reverse_map 反转 Map 的键值（值需能转为字符串才能作为键）。
///
/// 用法：reverseMap(m) → Map（新 Map，原 Map 不变）
/// 值通过 to_str() 转为字符串作为新键，原键（string）作为新值。
/// 若多个键映射到同一字符串值，后处理的覆盖前者（与 Map.set 语义一致）。
///
/// 示例：
///   reverseMap(map{a: "1", b: "2"}) → map{"1": "a", "2": "b"}
fn bi_reverse_map(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "reverseMap")?;
    let snapshot: Vec<(String, Value)> = match &args[0] {
        Value::Map(m) => m.lock().unwrap().snapshot(),
        v => return Err(crate::value::error_value(format!(
            "reverseMap() 参数应为 map，得到 {} (可能原因：参数类型不匹配；用 newMap() 创建 Map)",
            v.type_name(),
        ))),
    };
    let mut om = crate::ord_map::OrdMap::new();
    for (k, v) in snapshot {
        // 值转字符串作为新键；原键（string）作为新值
        let new_key = v.to_str();
        om.set(new_key, Value::str_from(k));
    }
    Ok(Value::Map(std::sync::Arc::new(std::sync::Mutex::new(om))))
}
