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
use crate::value::Value;
use crate::vm::VM;

/// register 注册所有字符串内置函数到 VM。
///
/// 注：contains / reverse 与数组模块重名，由数组模块注册为多态版本
/// （同时支持 string 与 array），此处不重复注册。
pub fn register(vm: &mut VM) {
    vm.register_builtin("upper", bi_upper);
    vm.register_builtin("lower", bi_lower);
    vm.register_builtin("trim", bi_trim);
    vm.register_builtin("trimStart", bi_trim_start);
    vm.register_builtin("trimEnd", bi_trim_end);
    vm.register_builtin("startsWith", bi_starts_with);
    vm.register_builtin("endsWith", bi_ends_with);
    vm.register_builtin("find", bi_find);
    vm.register_builtin("replace", bi_replace);
    vm.register_builtin("split", bi_split);
    vm.register_builtin("join", bi_join);
    vm.register_builtin("substring", bi_substring);
    vm.register_builtin("repeat", bi_repeat);
    // string 字节级访问（与按字符的 s[i]/s[i:j] 互补，用于 UTF-8 手动处理/协议解析）
    vm.register_builtin("bytesSlice", bi_bytes_slice);
    vm.register_builtin("bytesAt", bi_bytes_at);
    vm.register_builtin("lenBytes", bi_len_bytes);
    // 码点 ↔ 字符转换（与 s[i] 返回码点 int 配对）
    vm.register_builtin("charFromCode", bi_char_from_code);
    vm.register_builtin("codeOf", bi_code_of);
    // contains / reverse 由 builtins_arr 多态实现（同时支持 string 与 array）
}

fn s_owned(t: String) -> Value {
    Value::str_from(t)
}

/// bi_upper 转大写。
fn bi_upper(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(s_owned(bh::as_str(args, 0, "upper")?.to_uppercase()))
}

/// bi_lower 转小写。
fn bi_lower(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(s_owned(bh::as_str(args, 0, "lower")?.to_lowercase()))
}

/// bi_trim 去除两端空白。
fn bi_trim(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(s_owned(bh::as_str(args, 0, "trim")?.trim().to_string()))
}

/// bi_trim_start 去除前端空白。
fn bi_trim_start(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(s_owned(bh::as_str(args, 0, "trimStart")?.trim_start().to_string()))
}

/// bi_trim_end 去除末端空白。
fn bi_trim_end(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(s_owned(bh::as_str(args, 0, "trimEnd")?.trim_end().to_string()))
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
    let h = bh::as_str(args, 0, "find")?;
    let n = bh::as_str(args, 1, "find")?;
    match h.find(n) {
        // find 返回字节偏移，需转换为字符索引。
        Some(byte_off) => {
            let char_idx = h[..byte_off].chars().count() as i64;
            Ok(Value::Int(char_idx))
        }
        None => Ok(Value::Int(-1)),
    }
}

/// bi_replace 替换所有匹配子串。
fn bi_replace(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let src = bh::as_str(args, 0, "replace")?;
    let from = bh::as_str(args, 1, "replace")?;
    let to = bh::as_str(args, 2, "replace")?;
    if from.is_empty() {
        // 空模式替换会 panic 或无意义，直接返回原串。
        return Ok(s_owned(src.to_string()));
    }
    Ok(s_owned(src.replace(from, to)))
}

/// bi_split 按分隔符切分为字符串数组。
fn bi_split(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let src = bh::as_str(args, 0, "split")?;
    let sep = bh::as_str(args, 1, "split")?;
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
    Ok(Value::Int(bytes[i as usize] as i64))
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
