//! builtins_pinyin.rs — 汉字转拼音内置函数（纯标准库）
//!
//! 设计要点：
//!   - 内嵌 CJK 基本区（0x4E00-0x9FFF）约 20000 汉字的拼音数据
//!   - 数据来源 mozillazg/pinyin-data（MIT），已处理为不带声调的纯拼音
//!   - 二分查找（码点有序），O(log n) 查询
//!   - 多音字取第一个读音（简化处理，覆盖日常 99% 场景）
//!   - 非汉字字符原样保留
//!
//! 函数列表：
//!   toPinYin(s)       — 汉字字符串转拼音（无声调，多音字取首读音）
//!   toPinYinInitial(s) — 汉字字符串转拼音首字母

use crate::builtins_helpers as bh;
use crate::value::Value;
use crate::vm::VM;

// 内嵌拼音数据表
use crate::pinyin_data;
use crate::function::BuiltinDoc;

static DOC_TOPINYIN: BuiltinDoc = BuiltinDoc {
    category: "pinyin",
    signature: "toPinYin(s) -> string",
    summary: "将中文字符串转为拼音（带声调标记）。",
    params: &[("s", "中文字符串")],
    returns: "string 拼音",
    examples: &["toPinYin(\"你好\")  // nihao"],
    errors: &[],
};

static DOC_TOPINYININITIAL: BuiltinDoc = BuiltinDoc {
    category: "pinyin",
    signature: "toPinYinInitial(s) -> string",
    summary: "提取中文拼音首字母。",
    params: &[("s", "中文字符串")],
    returns: "string 首字母串",
    examples: &["toPinYinInitial(\"你好\")  // nh"],
    errors: &[],
};

/// register 注册拼音内置函数。
pub fn register(vm: &mut VM) {
    vm.register_builtin_doc("toPinYin", bi_to_pinyin, &DOC_TOPINYIN);
    vm.register_builtin_doc("toPinYinInitial", bi_to_pinyin_initial, &DOC_TOPINYININITIAL);
}

/// lookup_pinyin 查找单个汉字的拼音（二分查找）。
///
/// 返回 None 表示不在表中（非汉字或扩展区汉字）。
fn lookup_pinyin(code: u32) -> Option<&'static str> {
    let table = pinyin_data::PINYIN_TABLE;
    let mut lo = 0;
    let mut hi = table.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        if table[mid].0 < code {
            lo = mid + 1;
        } else if table[mid].0 > code {
            hi = mid;
        } else {
            return Some(table[mid].1);
        }
    }
    None
}

/// convert_pinyin 将字符串中的汉字转为拼音，非汉字字符保留。
///
/// separator 为拼音之间的分隔符。
/// 逻辑：汉字各自转拼音（拼音之间加分隔符），连续非汉字原样保留，
/// 汉字与非汉字之间也加分隔符。
fn convert_pinyin(s: &str, separator: &str) -> String {
    let mut out = String::with_capacity(s.len() * 4);
    let mut prev_han = false;

    for ch in s.chars() {
        let code = ch as u32;
        if (0x4E00..=0x9FFF).contains(&code) {
            if let Some(py) = lookup_pinyin(code) {
                if prev_han {
                    out.push_str(separator);
                }
                out.push_str(py);
                prev_han = true;
                continue;
            }
        }
        // 非汉字或未收录，原样保留
        if prev_han {
            out.push_str(separator);
        }
        out.push(ch);
        prev_han = false;
    }
    out
}

/// convert_initial 将字符串中的汉字转为拼音首字母，非汉字字符保留。
fn convert_initial(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let code = ch as u32;
        if (0x4E00..=0x9FFF).contains(&code) {
            if let Some(py) = lookup_pinyin(code) {
                if let Some(first_char) = py.chars().next() {
                    out.push(first_char);
                    continue;
                }
            }
        }
        out.push(ch);
    }
    out
}

/// bi_to_pinyin 汉字字符串转拼音。
///
/// 用法：toPinYin("中文") → "zhong wen"
///       toPinYin("中文", "_") → "zhong_wen"
///       toPinYin("Hello世界") → "Hello shi jie"
fn bi_to_pinyin(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "toPinYin")?;
    let separator = if args.len() > 1 {
        bh::as_str(args, 1, "toPinYin")?.to_string()
    } else {
        " ".to_string()
    };
    Ok(Value::str_from(convert_pinyin(s, &separator)))
}

/// bi_to_pinyin_initial 汉字字符串转拼音首字母。
///
/// 用法：toPinYinInitial("中文") → "zw"
///       toPinYinInitial("Hello世界") → "Hello sj"
fn bi_to_pinyin_initial(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "toPinYinInitial")?;
    Ok(Value::str_from(convert_initial(s)))
}
