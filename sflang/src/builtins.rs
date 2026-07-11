//! builtins.rs — 内置函数
//!
//! 设计要点：
//!   - 提供丰富的内置函数，覆盖常见编程任务
//!   - 错误信息包含可能原因（AI 友好）
//!   - 类型校验给出清晰提示

use std::sync::{Arc, Mutex};

use crate::value::Value;
use crate::vm::VM;

/// register 注册所有内置函数到 VM。
pub fn register(vm: &mut VM) {
    vm.register_builtin("println", bi_println);
    vm.register_builtin("print", bi_print);
    // 打印函数简称（别名）
    vm.register_builtin("pln", bi_println);
    vm.register_builtin("pr", bi_print);
    // 格式化打印（Go 风格占位符 %v %d %s %f %t %x %%）
    vm.register_builtin("printf", bi_printf);
    vm.register_builtin("prf", bi_printf);
    vm.register_builtin("printfln", bi_printfln);
    vm.register_builtin("pl", bi_printfln);
    vm.register_builtin("len", bi_len);
    vm.register_builtin("keys", bi_keys);
    vm.register_builtin("push", bi_push);
    vm.register_builtin("pop", bi_pop);
    vm.register_builtin("typeCode", bi_type_code);
    vm.register_builtin("typeName", bi_type_name);
    vm.register_builtin("string", bi_string);
    vm.register_builtin("int", bi_int);
    vm.register_builtin("float", bi_float);
    vm.register_builtin("range", bi_range);
    vm.register_builtin("assert", bi_assert);
    vm.register_builtin("sleep", bi_sleep);
    vm.register_builtin("sleepMs", bi_sleep_ms);
    vm.register_builtin("newStringBuilder", bi_new_string_builder);
    vm.register_builtin("clear", bi_clear);
    vm.register_builtin("reset", bi_reset);
    // ---- 实用函数（对标 charlang 常见编程任务）----
    vm.register_builtin("uuid", bi_uuid);
    vm.register_builtin("randomStr", bi_random_str);
    vm.register_builtin("values", bi_values);
    vm.register_builtin("hasKey", bi_has_key);
    vm.register_builtin("deepClone", bi_deep_clone);
    vm.register_builtin("newObject", bi_new_object);
    vm.register_builtin("filter", bi_filter);
    vm.register_builtin("map", bi_map);
    vm.register_builtin("find", bi_find);
    vm.register_builtin("sprintf", bi_sprintf);
    vm.register_builtin("spr", bi_sprintf);
    vm.register_builtin("fpr", bi_printf);
    vm.register_builtin("adjustFloat", bi_adjust_float);
    vm.register_builtin("pass", bi_pass);
    vm.register_builtin("plt", bi_plt);
    vm.register_builtin("getParam", bi_get_param);
    vm.register_builtin("getSwitch", bi_get_switch);
    vm.register_builtin("getAllSwitches", bi_get_all_switches);
    vm.register_builtin("ifSwitchExists", bi_if_switch_exists);
    vm.register_builtin("toStr", bi_string);
    vm.register_builtin("toInt", bi_int);
    vm.register_builtin("toFloat", bi_float);
    vm.register_builtin("compile", bi_compile);
    vm.register_builtin("runCode", bi_run_code);
    vm.register_builtin("newRef", bi_new_ref);
    vm.register_builtin("getValueByRef", bi_get_value_by_ref);
    vm.register_builtin("setValueByRef", bi_set_value_by_ref);
    // byte 构造与字节操作
    vm.register_builtin("byte", bi_byte);
    vm.register_builtin("newMap", bi_new_map);
    vm.register_builtin("entries", bi_entries);
    vm.register_builtin("dataKeys", bi_data_keys);
    vm.register_builtin("dataValues", bi_data_values);
    vm.register_builtin("bytesXor", bi_bytes_xor);
    vm.register_builtin("bytesXorInPlace", bi_bytes_xor_in_place);
    // 类型判断：isUndefined 保留（特殊语义：缺参返回 true，链式判空）
    vm.register_builtin("isUndefined", bi_is_undefined);
    // 错误处理：isError/isErr 保留（错误判断，非纯类型判断）
    vm.register_builtin("error", bi_error);
    vm.register_builtin("isError", bi_is_error);
    // ---- TXERROR 错误字符串机制（对标 Charlang isErrX/getErrStrX 等）----
    vm.register_builtin("isErr", bi_is_err);
    vm.register_builtin("isErrX", bi_is_err);      // Charlang 兼容别名
    vm.register_builtin("isErrStr", bi_is_err_str);
    vm.register_builtin("getErrStr", bi_get_err_str);
    vm.register_builtin("getErrStrX", bi_get_err_str);  // Charlang 兼容别名
    vm.register_builtin("errStrf", bi_err_strf);
    vm.register_builtin("errf", bi_err_strf);       // Charlang 兼容别名
    vm.register_builtin("errToEmpty", bi_err_to_empty);
    vm.register_builtin("checkErr", bi_check_err);
    vm.register_builtin("checkErrX", bi_check_err); // Charlang 兼容别名
    vm.register_builtin("trimErr", bi_trim_err);
    // ---- undefined 配套内置函数（对标 Charlang 的 nilToEmpty 等）----
    vm.register_builtin("undefToEmpty", bi_undef_to_empty);
    vm.register_builtin("default", bi_default);
    vm.register_builtin("defaultUndef", bi_default_undef);
    vm.register_builtin("explainUndef", bi_explain_undef);
    // ---- 通用类型判断（取代零散的 isXxx 谓词）----
    vm.register_builtin("isType", bi_is_type);
    vm.register_builtin("isTypeCode", bi_is_type_code);
    // ---- 调试与反射 ----
    vm.register_builtin("dumpVar", bi_dump_var);
    vm.register_builtin("globals", bi_globals);
    // ---- 成员反射 ----
    vm.register_builtin("getMember", bi_get_member);
    vm.register_builtin("setMember", bi_set_member);
    vm.register_builtin("callMethod", bi_call_method);
    // ---- 格式化辅助 ----
    vm.register_builtin("toKMG", bi_to_kmg);
    vm.register_builtin("showTable", bi_show_table);
}

/// bi_println 打印并换行。
fn bi_println(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = args.iter().map(|v| v.to_str()).collect::<Vec<_>>().join(" ");
    let out = _vm.output_handle();
    writeln!(out.lock().unwrap(), "{}", s).map_err(|e| crate::value::error_value(e.to_string()))?;
    Ok(Value::Undefined)
}

/// bi_print 打印不换行。
fn bi_print(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = args.iter().map(|v| v.to_str()).collect::<Vec<_>>().join(" ");
    let out = _vm.output_handle();
    write!(out.lock().unwrap(), "{}", s).map_err(|e| crate::value::error_value(e.to_string()))?;
    Ok(Value::Undefined)
}

/// bi_printf 格式化打印（不换行）。
///
/// Go 风格占位符：
///   %v  任意值（用 to_str 表示）
///   %d  整数（int/bigInt，截断小数）
///   %s  字符串
///   %f  浮点
///   %t  布尔（true/false）
///   %x  十六进制（整数）
///   %c  码点 → 单字符
///   %%  字面百分号
/// 宽度/精度：支持 %5d、%-5s、%.2f（对标 Go fmt）。
/// 占位符多于参数：保留原样；参数多于占位符：多余参数忽略。
fn bi_printf(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = sprintf(args)?;
    let out = _vm.output_handle();
    write!(out.lock().unwrap(), "{}", s).map_err(|e| crate::value::error_value(e.to_string()))?;
    Ok(Value::Undefined)
}

/// bi_printfln 格式化打印并换行。语义 = printf + "\n"。
fn bi_printfln(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = sprintf(args)?;
    let out = _vm.output_handle();
    writeln!(out.lock().unwrap(), "{}", s).map_err(|e| crate::value::error_value(e.to_string()))?;
    Ok(Value::Undefined)
}

/// sprintf 格式化核心：args[0] 为格式串，args[1..] 为占位符实参。
///
/// 解析 %[flags][width][.precision]verb，按 verb 取下一个参数格式化。
/// 未识别 verb 按字面输出。参数耗尽后剩余占位符按字面输出。
fn sprintf(args: &[Value]) -> Result<String, Value> {
    if args.is_empty() {
        return Ok(String::new());
    }
    let fmt = match &args[0] {
        Value::Str(s) => s.to_string(),
        v => v.to_str(),
    };
    let rest = &args[1..];
    let mut out = String::with_capacity(fmt.len() + 8);
    let mut arg_idx = 0usize;
    let bytes = fmt.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            // 正确处理多字节 UTF-8：取完整字符而非单字节
            // 找到当前字符的 UTF-8 边界
            let ch_len = utf8_char_len(bytes[i]);
            let end = (i + ch_len).min(bytes.len());
            if let Ok(s) = std::str::from_utf8(&bytes[i..end]) {
                out.push_str(s);
            } else {
                out.push(bytes[i] as char); // 回退
            }
            i = end;
            continue;
        }
        // 遇到 %，解析格式说明
        i += 1;
        if i >= bytes.len() {
            out.push('%');
            break;
        }
        if bytes[i] == b'%' {
            out.push('%');
            i += 1;
            continue;
        }
        // 解析 flags（- 0 + 空格）
        let mut flags = String::new();
        while i < bytes.len() && matches!(bytes[i], b'-' | b'0' | b'+' | b' ') {
            flags.push(bytes[i] as char);
            i += 1;
        }
        // 解析 width
        let mut width = String::new();
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            width.push(bytes[i] as char);
            i += 1;
        }
        // 解析 .precision
        let mut precision: Option<String> = None;
        if i < bytes.len() && bytes[i] == b'.' {
            i += 1;
            let mut prec = String::new();
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                prec.push(bytes[i] as char);
                i += 1;
            }
            precision = Some(prec);
        }
        // 解析 verb
        if i >= bytes.len() {
            // 格式说明未闭合，按字面输出已解析部分
            out.push('%');
            out.push_str(&flags);
            out.push_str(&width);
            if let Some(p) = precision { out.push('.'); out.push_str(&p); }
            break;
        }
        let verb = bytes[i] as char;
        i += 1;
        // 取参数
        let arg = rest.get(arg_idx);
        if arg.is_none() {
            // 参数耗尽：占位符按字面输出
            out.push('%');
            out.push_str(&flags);
            out.push_str(&width);
            if let Some(p) = precision { out.push('.'); out.push_str(&p); }
            out.push(verb);
            continue;
        }
        arg_idx += 1;
        let arg = arg.unwrap();
        let formatted = format_value(verb, arg, &flags, &width, precision.as_deref())?;
        out.push_str(&formatted);
    }
    Ok(out)
}

/// format_value 按 verb 格式化单个值，应用 width/precision/flags。
fn format_value(verb: char, v: &Value, flags: &str, width: &str, precision: Option<&str>) -> Result<String, Value> {
    let body: String = match verb {
        'v' => v.to_str(),
        's' => v.to_str(),
        'd' => match v {
            Value::Int(x) => x.to_string(),
            Value::BigInt(b) => b.to_string_decimal(),
            Value::Float(f) => (*f as i64).to_string(),
            _ => return Err(crate::value::error_value(format!(
                "printf %d 需要整数，得到 {} (可能原因：类型不匹配)", v.type_name(),
            ))),
        },
        'f' | 'g' | 'e' => match v {
            Value::Float(f) => format_float(verb, *f, precision),
            Value::Int(x) => format_float(verb, *x as f64, precision),
            _ => return Err(crate::value::error_value(format!(
                "printf %f 需要数值，得到 {} (可能原因：类型不匹配)", v.type_name(),
            ))),
        },
        't' => match v {
            Value::Bool(b) => b.to_string(),
            _ => v.is_truthy().to_string(),
        },
        'T' => v.type_name().to_string(),
        'x' => match v {
            Value::Int(x) => format!("{:x}", x),
            Value::BigInt(b) => {
                // 十六进制（绝对值 + 符号）
                let mag: String = b.to_string_decimal().chars().filter(|c| c.is_ascii_digit()).collect();
                let n = mag.parse::<u128>().unwrap_or(0);
                format!("{:x}", n)
            }
            _ => return Err(crate::value::error_value(format!(
                "printf %x 需要整数，得到 {}", v.type_name(),
            ))),
        },
        'c' => match v {
            Value::Int(code) => {
                match char::from_u32(*code as u32) {
                    Some(c) => c.to_string(),
                    None => return Err(crate::value::error_value(format!(
                        "printf %c 码点 {} 无效", code,
                    ))),
                }
            }
            _ => return Err(crate::value::error_value(format!(
                "printf %c 需要 int 码点，得到 {}", v.type_name(),
            ))),
        },
        _ => {
            // 未识别 verb：原样输出（Go 风格 %!verb）
            let prec_str = precision.map(|p| format!(".{}", p)).unwrap_or_default();
            return Ok(format!("%{}{}{}{}", flags, width, prec_str, verb));
        }
    };
    Ok(apply_width(body, flags, width, verb == 's' || verb == 'v'))
}

/// format_float 按精度格式化浮点（%.2f 等）。
fn format_float(verb: char, f: f64, precision: Option<&str>) -> String {
    let prec: usize = precision.and_then(|p| p.parse().ok()).unwrap_or(6);
    match verb {
        'f' => format!("{:.*}", prec, f),
        'g' => format!("{}", f),
        'e' => format!("{:.*e}", prec, f),
        _ => format!("{}", f),
    }
}

/// apply_width 应用宽度与对齐（- 左对齐，否则右对齐，0 填充对数值）。
fn apply_width(body: String, flags: &str, width: &str, is_string_like: bool) -> String {
    let w: usize = match width.parse() { Ok(n) => n, Err(_) => return body };
    if w == 0 || body.chars().count() >= w {
        return body;
    }
    let pad = w - body.chars().count();
    let fill = if flags.contains('0') && !is_string_like { '0' } else { ' ' };
    if flags.contains('-') {
        format!("{}{}", body, " ".repeat(pad))
    } else {
        format!("{}{}", fill.to_string().repeat(pad), body)
    }
}

/// bi_len 返回长度。
fn bi_len(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("len() 需要至少 1 个参数 (可能原因：忘记传参)"));
    }
    let n = match &args[0] {
        Value::Str(s) => s.chars().count() as i64,
        Value::Bytes(b) => b.len() as i64,
        Value::ByteArray(b) => b.lock().unwrap().len() as i64,
        Value::Array(a) => a.lock().unwrap().len() as i64,
        Value::Object(o) => o.lock().unwrap().len() as i64,
        Value::Map(m) => m.lock().unwrap().len() as i64,
        Value::StringBuilder(sb) => sb.lock().unwrap().chars().count() as i64,
        v => return Err(crate::value::error_value(format!("len() 不支持类型 {} (可能原因：参数类型错误)", v.type_name()))),
    };
    Ok(Value::Int(n))
}

/// bi_keys 返回所有键。
fn bi_keys(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("keys() 需要至少 1 个参数"));
    }
    let keys: Vec<Value> = match &args[0] {
        Value::Object(o) => o.lock().unwrap().keys().into_iter().map(|k| Value::str(&k)).collect(),
        Value::Map(m) => m.lock().unwrap().keys().into_iter().map(|k| Value::str(&k)).collect(),
        Value::Array(a) => {
            a.lock().unwrap().iter().enumerate().map(|(i, _)| Value::Int(i as i64)).collect()
        }
        Value::Str(s) => s.chars().enumerate().map(|(i, _)| Value::Int(i as i64)).collect(),
        v => return Err(crate::value::error_value(format!("keys() 不支持类型 {}", v.type_name()))),
    };
    Ok(Value::Array(Arc::new(Mutex::new(keys))))
}

/// bi_push 追加元素到数组。
fn bi_push(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.len() < 2 {
        return Err(crate::value::error_value("push() 需要 2 个参数 (array, value)"));
    }
    match &args[0] {
        Value::Array(a) => {
            a.lock().unwrap().push(args[1].clone());
            Ok(args[0].clone())
        }
        v => Err(crate::value::error_value(format!("push() 第一个参数必须是数组，得到 {} (可能原因：参数顺序错误；正确顺序 push(arr, value))", v.type_name()))),
    }
}

/// bi_pop 弹出数组末尾元素。
fn bi_pop(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("pop() 需要 1 个参数"));
    }
    match &args[0] {
        Value::Array(a) => {
            let mut arr = a.lock().unwrap();
            arr.pop().ok_or_else(|| crate::value::error_value("pop() on empty array"))
        }
        v => Err(crate::value::error_value(format!("pop() 不支持类型 {}", v.type_name()))),
    }
}

/// bi_type_code 返回类型编码。
fn bi_type_code(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("typeCode() 需要 1 个参数"));
    }
    Ok(Value::Int(args[0].type_code() as i64))
}

/// bi_type_name 返回类型名。
fn bi_type_name(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("typeName() 需要 1 个参数"));
    }
    Ok(Value::str(args[0].type_name()))
}

/// bi_string 转字符串。
fn bi_string(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Ok(Value::str(""));
    }
    Ok(Value::str_from(args[0].to_str()))
}

/// bi_int 转整数。
fn bi_int(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("int() 需要 1 个参数"));
    }
    match &args[0] {
        Value::Int(_) => Ok(args[0].clone()),
        Value::Float(f) => Ok(Value::Int(*f as i64)),
        Value::Bool(b) => Ok(Value::Int(if *b { 1 } else { 0 })),
        Value::Byte(b) => Ok(Value::Int(*b as i64)),
        Value::Str(s) => s.parse::<i64>().map(Value::Int).map_err(|_| {
            crate::value::error_value(format!("int() 无法解析 '{}' (可能原因：字符串不是有效整数)", s))
        }),
        Value::BigInt(b) => {
            // BigInt -> Int，超出 i64 范围则报错
            match b.to_i64() {
                Some(v) => Ok(Value::Int(v)),
                None => Err(crate::value::error_value(format!(
                    "int() BigInt 超出 i64 范围: {} (可能原因：数值过大，请保持使用 bigInt 类型)",
                    b
                ))),
            }
        }
        v => Err(crate::value::error_value(format!("int() 不支持类型 {}", v.type_name()))),
    }
}

/// bi_float 转浮点。
fn bi_float(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("float() 需要 1 个参数"));
    }
    match &args[0] {
        Value::Int(i) => Ok(Value::Float(*i as f64)),
        Value::Float(_) => Ok(args[0].clone()),
        Value::Bool(b) => Ok(Value::Float(if *b { 1.0 } else { 0.0 })),
        Value::Byte(b) => Ok(Value::Float(*b as f64)),
        Value::BigInt(b) => {
            match b.to_i64() {
                Some(v) => Ok(Value::Float(v as f64)),
                None => Err(crate::value::error_value(format!(
                    "float() BigInt 超出 i64 范围: {} (可能原因：数值过大，无法精确转为 f64)", b
                ))),
            }
        }
        Value::BigFloat(b) => {
            // bigFloat -> f64，通过字符串中转尽量保留精度
            let s = format!("{}", b);
            Ok(Value::Float(s.parse::<f64>().unwrap_or(0.0)))
        }
        Value::Str(s) => s.parse::<f64>().map(Value::Float).map_err(|_| {
            crate::value::error_value(format!("float() 无法解析 '{}'", s))
        }),
        v => Err(crate::value::error_value(format!("float() 不支持类型 {}", v.type_name()))),
    }
}

/// bi_range 生成范围数组。
fn bi_range(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let (start, end, step) = match args.len() {
        1 => (0, args[0].to_int().ok_or_else(|| crate::value::error_value("range() 参数需为整数"))?, 1i64),
        2 => (
            args[0].to_int().ok_or_else(|| crate::value::error_value("range() 参数需为整数"))?,
            args[1].to_int().ok_or_else(|| crate::value::error_value("range() 参数需为整数"))?,
            1,
        ),
        3 => (
            args[0].to_int().ok_or_else(|| crate::value::error_value("range() 参数需为整数"))?,
            args[1].to_int().ok_or_else(|| crate::value::error_value("range() 参数需为整数"))?,
            args[2].to_int().ok_or_else(|| crate::value::error_value("range() 参数需为整数"))?,
        ),
        _ => return Err(crate::value::error_value("range() 需要 1-3 个参数")),
    };
    if step == 0 {
        return Err(crate::value::error_value("range() step 不能为 0"));
    }
    let mut v = Vec::new();
    if step > 0 {
        let mut i = start;
        while i < end {
            v.push(Value::Int(i));
            i += step;
        }
    } else {
        let mut i = start;
        while i > end {
            v.push(Value::Int(i));
            i += step;
        }
    }
    Ok(Value::Array(Arc::new(Mutex::new(v))))
}

/// bi_assert 断言。
fn bi_assert(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("assert() 需要至少 1 个参数"));
    }
    if !args[0].is_truthy() {
        let msg = if args.len() > 1 {
            args[1].to_str()
        } else {
            format!("assertion failed: value is falsy ({})", args[0].inspect())
        };
        return Err(crate::value::error_value(msg));
    }
    Ok(Value::Undefined)
}

/// bi_sleep 睡眠（毫秒）。
/// bi_sleep 睡眠指定秒数（支持小数）。
///
/// 用法：sleep(1.5) — 睡眠 1.5 秒
fn bi_sleep(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("sleep() 需要 1 个参数 (秒)"));
    }
    let secs = args[0].to_f64().ok_or_else(|| crate::value::error_value("sleep() 参数需为数字"))?;
    let dur = std::time::Duration::from_secs_f64(secs.max(0.0));
    std::thread::sleep(dur);
    Ok(Value::Undefined)
}

/// bi_sleep_ms 睡眠指定毫秒数（整数）。
///
/// 用法：sleepMs(500) — 睡眠 500 毫秒
fn bi_sleep_ms(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("sleepMs() 需要 1 个参数 (毫秒)"));
    }
    let ms = args[0].to_int().ok_or_else(|| crate::value::error_value("sleepMs() 参数需为整数"))?;
    std::thread::sleep(std::time::Duration::from_millis(ms.max(0) as u64));
    Ok(Value::Undefined)
}

// ---- byte 构造 ----

/// bi_byte 构造 byte 值（0-255）。
///
/// byte(65) → Byte(65)。超出 0-255 报错。
fn bi_byte(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let v = bh::as_int(args, 0, "byte")?;
    if v < 0 || v > 255 {
        return Err(crate::value::error_value(format!(
            "byte() 值 {} 超出范围 (0-255; 可能原因：传入了非字节整数)", v,
        )));
    }
    Ok(Value::Byte(v as u8))
}

/// bi_new_map 创建空有序 Map。
fn bi_new_map(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Map(std::sync::Arc::new(std::sync::Mutex::new(crate::ord_map::OrdMap::new()))))
}

/// bi_new_string_builder 创建 StringBuilder（高效字符串构建器）。
///
/// 用法：
///   newStringBuilder()       — 空 builder
///   newStringBuilder("初始")  — 带初始内容
///
/// 通过通用函数操作：writeStr/writeBytes/len/toStr/clear/reset。
fn bi_new_string_builder(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let initial = if args.is_empty() {
        String::new()
    } else {
        args[0].to_str()
    };
    Ok(Value::StringBuilder(std::sync::Arc::new(std::sync::Mutex::new(initial))))
}

/// bi_clear 清空容器内容（不释放内存）。
///
/// 支持：stringBuilder、array、byteArray、map、ring。
/// 用法：clear(sb)
fn bi_clear(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "clear")?;
    match &args[0] {
        Value::StringBuilder(sb) => sb.lock().unwrap().clear(),
        Value::Array(a) => a.lock().unwrap().clear(),
        Value::ByteArray(b) => b.lock().unwrap().clear(),
        Value::Map(m) => m.lock().unwrap().clear(),
        Value::Native(n) => {
            // ring
            if let Some(r) = n.downcast_ref::<std::sync::Arc<std::sync::Mutex<crate::ring::Ring>>>() {
                r.lock().unwrap().clear();
            } else {
                return Err(crate::value::error_value(format!(
                    "clear() 不支持此 native 类型 (可能原因：不是 ring)",
                )));
            }
        }
        other => return Err(crate::value::error_value(format!(
            "clear() 不支持类型 {} (可能原因：参数应为 stringBuilder/array/byteArray/map/ring)", other.type_name(),
        ))),
    }
    Ok(Value::Undefined)
}

/// bi_reset 清空容器并释放内存（对 stringBuilder 效果最明显）。
///
/// 支持：stringBuilder、array、byteArray、map。
/// 用法：reset(sb)
fn bi_reset(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "reset")?;
    match &args[0] {
        Value::StringBuilder(sb) => {
            // 清空并 shrink_to_fit 释放内存
            let mut guard = sb.lock().unwrap();
            guard.clear();
            guard.shrink_to_fit();
        }
        Value::Array(a) => {
            let mut guard = a.lock().unwrap();
            guard.clear();
            guard.shrink_to_fit();
        }
        Value::ByteArray(b) => {
            let mut guard = b.lock().unwrap();
            guard.clear();
            guard.shrink_to_fit();
        }
        other => return Err(crate::value::error_value(format!(
            "reset() 不支持类型 {} (可能原因：参数应为 stringBuilder/array/byteArray)", other.type_name(),
        ))),
    }
    Ok(Value::Undefined)
}

/// bi_entries 返回对象的非函数键值对（过滤方法），每对为 [key, value]。
///
/// 用法：entries(obj) → [["k1", v1], ["k2", v2], ...]
/// 也支持 Map（不过滤，Map 本来就是纯数据）。
fn bi_entries(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "entries")?;
    let pairs: Vec<(String, Value)> = match &args[0] {
        Value::Object(o) => {
            o.lock().unwrap().snapshot().into_iter()
                .filter(|(_, v)| !matches!(v, Value::Func(_) | Value::Builtin(_)))
                .collect()
        }
        Value::Map(m) => m.lock().unwrap().snapshot(),
        _ => return Err(crate::value::error_value(format!(
            "entries() 需要 object 或 map，得到 {}", args[0].type_name(),
        ))),
    };
    let result: Vec<Value> = pairs.into_iter().map(|(k, v)| {
        Value::Array(std::sync::Arc::new(std::sync::Mutex::new(vec![Value::str(&k), v])))
    }).collect();
    Ok(Value::Array(std::sync::Arc::new(std::sync::Mutex::new(result))))
}

/// bi_data_keys 返回对象的非函数键（过滤方法）。
fn bi_data_keys(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "dataKeys")?;
    let keys: Vec<Value> = match &args[0] {
        Value::Object(o) => {
            o.lock().unwrap().snapshot().into_iter()
                .filter(|(_, v)| !matches!(v, Value::Func(_) | Value::Builtin(_)))
                .map(|(k, _)| Value::str(&k))
                .collect()
        }
        Value::Map(m) => m.lock().unwrap().keys().into_iter().map(|k| Value::str(&k)).collect(),
        _ => return Err(crate::value::error_value(format!(
            "dataKeys() 需要 object 或 map，得到 {}", args[0].type_name(),
        ))),
    };
    Ok(Value::Array(std::sync::Arc::new(std::sync::Mutex::new(keys))))
}

/// bi_data_values 返回对象的非函数值（过滤方法）。
fn bi_data_values(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "dataValues")?;
    let vals: Vec<Value> = match &args[0] {
        Value::Object(o) => {
            o.lock().unwrap().snapshot().into_iter()
                .filter(|(_, v)| !matches!(v, Value::Func(_) | Value::Builtin(_)))
                .map(|(_, v)| v)
                .collect()
        }
        Value::Map(m) => m.lock().unwrap().values(),
        _ => return Err(crate::value::error_value(format!(
            "dataValues() 需要 object 或 map，得到 {}", args[0].type_name(),
        ))),
    };
    Ok(Value::Array(std::sync::Arc::new(std::sync::Mutex::new(vals))))
}

/// bi_bytes_xor 批量 XOR：data 的每个字节与 key 的对应字节异或。
///
/// data 可以是 bytes 或 byteArray。key 可以是 bytes/byteArray/int(byte)。
/// 返回新的 bytes（不可变）。适合高效加密/解密。
fn bi_bytes_xor(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "bytesXor")?;
    bh::require_arg(args, 1, "bytesXor")?;
    let data = to_byte_vec(&args[0]).map_err(crate::value::error_value)?;
    let key = to_byte_vec(&args[1]).map_err(crate::value::error_value)?;
    if key.is_empty() {
        return Err(crate::value::error_value("bytesXor() key 不能为空"));
    }
    let result: Vec<u8> = data.iter().enumerate()
        .map(|(i, &b)| b ^ key[i % key.len()])
        .collect();
    Ok(Value::Bytes(std::sync::Arc::new(result)))
}

/// bi_bytes_xor_in_place 原地 XOR（修改 byteArray，不创建新对象）。
fn bi_bytes_xor_in_place(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "bytesXorInPlace")?;
    bh::require_arg(args, 1, "bytesXorInPlace")?;
    let key = to_byte_vec(&args[1]).map_err(crate::value::error_value)?;
    if key.is_empty() {
        return Err(crate::value::error_value("bytesXorInPlace() key 不能为空"));
    }
    match &args[0] {
        Value::ByteArray(b) => {
            let mut guard = b.lock().map_err(|e| crate::value::error_value(format!("锁异常: {}", e)))?;
            for (i, byte) in guard.iter_mut().enumerate() {
                *byte ^= key[i % key.len()];
            }
            Ok(args[0].clone())
        }
        _ => Err(crate::value::error_value("bytesXorInPlace() 第一个参数须为 byteArray")),
    }
}

/// to_byte_vec 将 Value 转为字节 Vec（bytes/byteArray/string/int）。
fn to_byte_vec(v: &Value) -> Result<Vec<u8>, String> {
    match v {
        Value::Bytes(b) => Ok(b.as_ref().to_vec()),
        Value::ByteArray(b) => Ok(b.lock().unwrap().clone()),
        Value::Str(s) => Ok(s.as_bytes().to_vec()),
        Value::Int(x) => {
            if *x < 0 || *x > 255 { return Err(format!("值 {} 超出字节范围 0-255", x)); }
            Ok(vec![*x as u8])
        }
        Value::Byte(x) => Ok(vec![*x]),
        _ => Err(format!("无法将 {} 转为字节", v.type_name())),
    }
}

/// bi_is_undefined 判断是否为 undefined（含旧称 nil）。
///
/// 缺参时返回 true（便于链式判空：`isUndefined(m["maybe"])`）。
/// 这是特殊保留的类型判断函数（缺参返回 true 的语义不同于 isType）。
fn bi_is_undefined(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(args.get(0), Some(Value::Undefined) | None)))
}

/// bi_error 创建一个错误值。
///
/// 用法：error(msg) → Error 值
/// 错误值是普通值（不抛出），用于返回错误结果；配合 isError 判断。
/// 这符合 Sflang "一般返回错误对象为主" 的设计原则。
fn bi_error(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let msg = bh::as_str(args, 0, "error")?;
    Ok(crate::value::error_value(msg))
}

/// bi_is_error 判断是否为错误值。
fn bi_is_error(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(args.get(0), Some(Value::Error(_)))))
}

// ---- TXERROR 错误字符串机制（对标 Charlang） ----
//
// Sflang 同时支持两种错误表示：
//   1. Error 对象（Value::Error）— 推荐方式，结构化
//   2. "TXERROR:xxx" 字符串 — 字符串形式的错误，便于跨边界传递
//
// 配套函数统一处理两种形式：
//   - isErr(v):       判断 v 是否为 Error 对象或 "TXERROR:" 开头的字符串
//   - isErrStr(v):    判断 v 是否为 "TXERROR:" 开头的字符串
//   - getErrStr(v):   提取错误信息字符串（Error 取 message，TXERROR 字符串去前缀）
//   - errStrf(fmt, args...): 格式化生成 "TXERROR:" 前缀的错误字符串
//   - errf(fmt, args...):    同 errStrf（别名）
//   - checkErr(v, ...):      若 v 是错误则打印并退出进程
//   - checkErrX(v, ...):     checkErr 的别名
//   - errToEmpty(v):         若 v 是错误则转为空字符串，否则原样返回
//   - trimErr(v, ...):       若 v 是错误则原样返回，否则去空白（错误不静默丢失）

/// TXERROR 前缀常量。
const TXERROR_PREFIX: &str = "TXERROR:";

/// is_err_value 内部辅助：判断 Value 是否为"错误样"值（Error 对象或 TXERROR 字符串）。
fn is_err_value(v: &Value) -> bool {
    match v {
        Value::Error(_) => true,
        Value::Str(s) => s.starts_with(TXERROR_PREFIX),
        _ => false,
    }
}

/// get_err_str 内部辅助：从错误样值提取错误信息字符串。
/// - Error 对象 → message（若已是 "error: xxx" 形式则去掉 "error: " 前缀）
/// - TXERROR 字符串 → 去掉 "TXERROR:" 前缀后的内容
/// - 非错误 → 值的字符串表示
fn extract_err_str(v: &Value) -> String {
    match v {
        Value::Error(e) => {
            // Error 对象的 message 可能以 "error: " 开头（VM 抛出时），去掉保持一致
            let msg = &e.message;
            if let Some(rest) = msg.strip_prefix("error: ") {
                rest.to_string()
            } else {
                msg.clone()
            }
        }
        Value::Str(s) => {
            // TXERROR:xxx → xxx
            s.strip_prefix(TXERROR_PREFIX).map(|r| r.to_string()).unwrap_or_else(|| s.to_string())
        }
        _ => v.to_str(),
    }
}

/// bi_is_err 判断是否为错误样值（Error 对象或 TXERROR 字符串）。
///
/// 这是统一判断函数，同时识别两种错误形式。
/// 别名：isErrX（与 Charlang 完全一致）。
fn bi_is_err(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(args.get(0).map(is_err_value).unwrap_or(false)))
}

/// bi_is_err_str 判断是否为 TXERROR 字符串。
fn bi_is_err_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(
        args.get(0),
        Some(Value::Str(s)) if s.starts_with(TXERROR_PREFIX)
    )))
}

/// bi_get_err_str 提取错误信息字符串。
///
/// 用法：getErrStr(v) → 字符串
/// - Error 对象 → message（去 "error: " 前缀）
/// - TXERROR 字符串 → 去 "TXERROR:" 前缀
/// - 其他 → to_str
fn bi_get_err_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    match args.get(0) {
        Some(v) => Ok(Value::str_from(extract_err_str(v))),
        None => Ok(Value::str_from(String::new())),
    }
}

/// bi_err_strf 格式化生成 TXERROR 错误字符串。
///
/// 用法：errStrf(format, args...) → "TXERROR:" + sprintf(format, args...)
/// 这是创建字符串形式错误的便捷方式。
fn bi_err_strf(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Ok(Value::str_from(TXERROR_PREFIX.to_string()));
    }
    let formatted = sprintf(args)?;
    Ok(Value::str_from(format!("{}{}", TXERROR_PREFIX, formatted)))
}

/// bi_err_to_empty 若 v 是错误样值则转为空字符串，否则原样返回。
///
/// 用于安全地处理可能为错误的值：错误时得到空串，非错误时保留原值。
fn bi_err_to_empty(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    match args.get(0) {
        Some(v) if is_err_value(v) => Ok(Value::str_from(String::new())),
        Some(v) => Ok(v.clone()),
        None => Ok(Value::str_from(String::new())),
    }
}

/// bi_check_err 若 v 是错误样值则打印错误信息并退出进程（退出码 1）。
///
/// 用法：checkErr(v) 或 checkErr(v, "-format=自定义格式 %v\n")
/// 默认格式："Error: %v\n"
/// 非错误时原样返回 v。
///
/// 对标 Charlang checkErrX/checkErr。
fn bi_check_err(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let v = match args.get(0) {
        Some(v) => v,
        None => return Err(crate::value::error_value("checkErr() 至少需要 1 个参数")),
    };
    if is_err_value(v) {
        // 解析可选的 -format= 参数
        let default_fmt = "Error: %v\n";
        let mut fmt = default_fmt.to_string();
        for i in 1..args.len() {
            if let Value::Str(s) = &args[i] {
                if let Some(rest) = s.strip_prefix("-format=") {
                    fmt = rest.to_string();
                }
            }
        }
        let err_msg = extract_err_str(v);
        let formatted = sprintf(&[Value::str_from(fmt), Value::str_from(err_msg)])?;
        // 打印到 stderr 并退出
        eprint!("{}", formatted);
        std::process::exit(1);
    }
    Ok(v.clone())
}

/// bi_trim_err 若 v 是错误样值则原样返回（不静默丢失错误），否则去空白。
///
/// 用法：trimErr(v) 或 trimErr(v, cutset...)
/// 这是对 trim 的安全增强：避免 trim 意外吞掉错误信息。
fn bi_trim_err(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let v = match args.get(0) {
        Some(v) => v,
        None => return Err(crate::value::error_value("trimErr() 至少需要 1 个参数")),
    };
    // 错误样值原样返回（不丢失错误）
    if is_err_value(v) {
        return Ok(v.clone());
    }
    // undefined 转空字符串
    if matches!(v, Value::Undefined) {
        return Ok(Value::str_from(String::new()));
    }
    let s = bh::as_str(args, 0, "trimErr")?;
    // 收集 cutset 字符
    let cutsets: Vec<&str> = args[1..].iter().filter_map(|a| match a {
        Value::Str(s) => Some(&**s),
        _ => None,
    }).collect();
    let trimmed = if cutsets.is_empty() {
        s.trim().to_string()
    } else {
        let chars: Vec<char> = cutsets.iter().flat_map(|c| c.chars()).collect();
        s.trim_matches(|c| chars.contains(&c)).to_string()
    };
    Ok(Value::str_from(trimmed))
}

// ---- undefined 配套内置函数 ----
//
// 设计目标（对标 Charlang 的 nilToEmpty/trim，并为 AI 友好补强）：
//   - undefToEmpty: undefined → 空字符串，其余 → to_str()
//   - default(x, d): x 为 falsy 时返回 d（宽松兜底，含 0/""）
//   - defaultUndef(x, d): 仅 x 为 undefined 时返回 d（严格空合并，0/"" 不触发）
//   - explainUndef(name): 返回某名字为何为 undefined 的诊断字符串（AI 定位用）

/// bi_undef_to_empty 将 undefined 转为空字符串，其余值转为 to_str。
///
/// 用途：把"可能为 undefined"的值安全接入字符串处理。
/// 等价 Charlang 的 nilToEmpty。
fn bi_undef_to_empty(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    match args.get(0) {
        Some(Value::Undefined) | None => Ok(Value::str("")),
        Some(v) => Ok(Value::str_from(v.to_str())),
    }
}

/// bi_default 宽松兜底：x 为 falsy（undefined/0/""/空容器）时返回 d，否则返回 x。
///
/// 注意：0 和 "" 也会触发兜底（与 Python 的 `or` 一致）。若只想对 undefined 兜底，
/// 请用 defaultUndef。d 缺省时按 undefined 处理。
fn bi_default(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let x = args.get(0).cloned().unwrap_or(Value::Undefined);
    let d = args.get(1).cloned().unwrap_or(Value::Undefined);
    if x.is_truthy() {
        Ok(x)
    } else {
        Ok(d)
    }
}

/// bi_default_undef 严格空合并：仅当 x 为 undefined 时返回 d，否则返回 x。
///
/// 对应其他语言的 `??` 运算符：0/""/空数组 都视为有效值，不触发兜底。
fn bi_default_undef(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let x = args.get(0).cloned().unwrap_or(Value::Undefined);
    let d = args.get(1).cloned().unwrap_or(Value::Undefined);
    if matches!(x, Value::Undefined) {
        Ok(d)
    } else {
        Ok(x)
    }
}

/// bi_explain_undef 返回某名字"为何为 undefined"的诊断字符串（AI 友好）。
///
/// 由于本实现读取未定义变量直接返回 undefined（不抛错），脚本难以察觉拼写错误。
/// 此函数让 AI/用户主动诊断：返回包含名字、是否为预定义全局、相似已声明变量等
/// 信息的提示。缺省或非字符串参数时给出通用说明。
fn bi_explain_undef(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let name = match args.get(0) {
        Some(Value::Str(s)) => s.as_ref(),
        _ => return Ok(Value::str("explainUndef 需传入变量名（字符串）。读取未定义变量在 Sflang 中返回 undefined（非错误）。可用 isUndefined(x) 判空，default(x, d) 或 defaultUndef(x, d) 提供默认值。")),
    };
    // 检查该名字是否已绑定（全局或内置）
    let bound = vm.get_global(name).is_some();
    if bound {
        return Ok(Value::str_from(format!(
            "'{}' 当前已绑定（非 undefined）。若仍得到 undefined，请检查是否读到了 map 缺键或函数无返回值的情形。",
            name,
        )));
    }
    // 预定义全局名单（与 VM::new / sf/main 设置的一致）
    let predefined = ["piG", "eG", "argsG", "scriptPathG"];
    let is_predefined = predefined.contains(&name);
    // 收集相似名字：取全局中编辑距离最近的前 3 个（简单实现，避免大改依赖）
    let globals = vm.globals_handle();
    let g = globals.lock().unwrap();
    let mut similar: Vec<(String, usize)> = g.keys()
        .map(|k| (k.clone(), lev(name, k)))
        .filter(|(_, d)| *d <= name.len().max(1) / 2 + 1)
        .collect();
    similar.sort_by_key(|(_, d)| *d);
    let hints: Vec<String> = similar.into_iter().take(3).map(|(k, _)| k).collect();
    let mut msg = format!(
        "'{}' 未定义（读取返回 undefined）。{}",
        name,
        if is_predefined { "它是预定义全局变量，但当前未赋值（如在 REPL 中 argsG/scriptPathG 未设置）。" } else { "可能原因：变量未声明、拼写错误，或为 map 缺键/函数无返回值。" },
    );
    if !hints.is_empty() {
        msg.push_str(&format!(" 作用域内相似名字：{}。", hints.join(", ")));
    }
    msg.push_str(" 可用 isUndefined(x) 判空；default(x, d) / defaultUndef(x, d) 提供默认值。");
    Ok(Value::str_from(msg))
}

// ---- 通用类型判断（取代零散的 isXxx 谓词） ----

/// bi_is_type 通用类型判断：按类型名字符串判断。
///
/// 用法：isType(v, "string") → bool
///
/// 支持的类型名（与 type_name_ex 一致）：
///   基础类型：undefined, int, float, bool, string, bytes, byteArray, array,
///             object, function, builtin, error, native, bigInt, bigFloat,
///             datetime, file, byte, map
///   Native 细分：ring, channel, mutex, rwmutex, waitGroup, semaphore, code, ref, regex
///
/// 这取代零散的 isInt/isString/isArray 等谓词，统一为一个入口。
fn bi_is_type(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "isType")?;
    let type_name = bh::as_str(args, 1, "isType")?;
    // 用 type_name_ex 获取细化类型名，做大小写不敏感比较
    let actual = args[0].type_name_ex();
    let result = actual.eq_ignore_ascii_case(type_name);
    Ok(Value::Bool(result))
}

/// bi_is_type_code 通用类型判断：按类型数字编码判断。
///
/// 用法：isTypeCode(v, 4) → bool   // 4 = string
///
/// 数字编码与 TypeCode 枚举一致（0-18，详见 typeCode(v)）。
/// 对于 Native 细分类型（ring 等），编码均为 11（Native），需用 isType 按名字判断。
fn bi_is_type_code(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "isTypeCode")?;
    let code = bh::as_int(args, 1, "isTypeCode")?;
    let actual = args[0].type_code() as i64;
    Ok(Value::Bool(actual == code))
}

/// utf8_char_len 根据 UTF-8 首字节返回字符长度。
fn utf8_char_len(b: u8) -> usize {
    if b < 0x80 { 1 }
    else if b < 0xC0 { 1 }
    else if b < 0xE0 { 2 }
    else if b < 0xF0 { 3 }
    else { 4 }
}

/// lev 计算两字符串的 Levenshtein 编辑距离（用于相似名字提示）。
fn lev(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    if m == 0 { return n; }
    if n == 0 { return m; }
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut cur = vec![0usize; n + 1];
    for i in 1..=m {
        cur[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[n]
}

// ---- 实用函数 ----

/// bi_uuid 生成 UUID v4 字符串（如 "550e8400-e29b-41d4-a716-446655440000"）。
///
/// 用随机数填充（randInt 已有的 xorshift），版本位设为 4，变体位设为 RFC 4122。
fn bi_uuid(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEED: AtomicU64 = AtomicU64::new(0x1234_5678_9ABC_DEF0);
    let next = || {
        let mut s = SEED.load(Ordering::Relaxed);
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        SEED.store(s, Ordering::Relaxed);
        s
    };
    // 生成 16 字节
    let mut bytes = [0u8; 16];
    for chunk in bytes.chunks_mut(8) {
        let n = next();
        for (i, b) in chunk.iter_mut().enumerate() {
            *b = (n >> (i * 8)) as u8;
        }
    }
    // 版本位（byte 6 高 4 位 = 4），变体位（byte 8 高 2 位 = 10）
    bytes[6] = (bytes[6] & 0x0F) | 0x40;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    Ok(Value::str_from(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32],
    )))
}

/// bi_random_str 生成长度为 n 的随机字母数字字符串。
fn bi_random_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let n = bh::as_int(args, 0, "randomStr")?;
    if n < 0 {
        return Err(crate::value::error_value("randomStr() 长度不能为负"));
    }
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut out = String::with_capacity(n as usize);
    for _ in 0..n {
        let r = crate::builtins_math::next_rand() as usize % CHARS.len();
        out.push(CHARS[r] as char);
    }
    Ok(Value::str_from(out))
}

/// bi_values 返回 object 的所有值（array）。
fn bi_values(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "values")?;
    match &args[0] {
        Value::Object(o) => {
            let vals: Vec<Value> = o.lock().unwrap().snapshot().into_iter().map(|(_, v)| v).collect();
            Ok(Value::Array(std::sync::Arc::new(std::sync::Mutex::new(vals))))
        }
        Value::Array(a) => Ok(Value::Array(a.clone())), // 数组的 values 即自身
        _ => Err(crate::value::error_value(format!(
            "values() 需要 object 或 array，得到 {}", args[0].type_name(),
        ))),
    }
}

/// bi_has_key 判断 object 是否包含某键。
fn bi_has_key(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let key = bh::as_str(args, 1, "hasKey")?;
    match &args[0] {
        Value::Object(o) => Ok(Value::Bool(o.lock().unwrap().has(key))),
        Value::Map(m) => Ok(Value::Bool(m.lock().unwrap().has(key))),
        v => Err(crate::value::error_value(format!(
            "hasKey() 第 1 个参数应为 object 或 map，得到 {}", v.type_name(),
        ))),
    }
}

/// bi_deep_clone 深拷贝值（递归复制 array/object/byteArray）。
fn bi_deep_clone(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "deepClone")?;
    Ok(deep_clone_value(&args[0]))
}

/// bi_new_object 创建以 proto 为原型的空 object（暴露原型链到脚本层，用于方法共享）。
fn bi_new_object(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let proto = bh::as_object(args, 0, "newObject")?;
    Ok(Value::Object(crate::object_map::new_map_with_proto(proto.clone())))
}

/// deep_clone_value 递归克隆（内部辅助）。
fn deep_clone_value(v: &Value) -> Value {
    match v {
        Value::Array(a) => {
            let cloned: Vec<Value> = a.lock().unwrap().iter().map(deep_clone_value).collect();
            Value::Array(std::sync::Arc::new(std::sync::Mutex::new(cloned)))
        }
        Value::Object(o) => {
            let snap = o.lock().unwrap().snapshot();
            let mut new_map = crate::object_map::Map::new();
            for (k, val) in snap {
                new_map.set(k, deep_clone_value(&val));
            }
            Value::Object(std::sync::Arc::new(std::sync::Mutex::new(new_map)))
        }
        Value::ByteArray(b) => {
            let cloned = b.lock().unwrap().clone();
            Value::ByteArray(std::sync::Arc::new(std::sync::Mutex::new(cloned)))
        }
        other => other.clone(), // 不可变值（int/string/bytes 等）直接 clone
    }
}

/// bi_filter 用谓词函数过滤数组，返回新数组。
///
/// filter(arr, fn) → 仅保留 fn(x) 为 truthy 的元素。
fn bi_filter(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let arr = bh::as_array(args, 0, "filter")?;
    bh::require_arg(args, 1, "filter")?;
    let pred = args[1].clone();
    let snap = arr.lock().unwrap().clone();
    let mut result = Vec::new();
    for item in snap {
        let keep = vm.call_function_value(pred.clone(), vec![item.clone()])?;
        if keep.is_truthy() {
            result.push(item);
        }
    }
    Ok(Value::Array(std::sync::Arc::new(std::sync::Mutex::new(result))))
}

/// bi_map 用函数映射数组的每个元素，返回新数组。
///
/// map(arr, fn) → [fn(a[0]), fn(a[1]), ...]
fn bi_map(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let arr = bh::as_array(args, 0, "map")?;
    bh::require_arg(args, 1, "map")?;
    let f = args[1].clone();
    let snap = arr.lock().unwrap().clone();
    let mut result = Vec::with_capacity(snap.len());
    for item in snap {
        let mapped = vm.call_function_value(f.clone(), vec![item])?;
        result.push(mapped);
    }
    Ok(Value::Array(std::sync::Arc::new(std::sync::Mutex::new(result))))
}

/// bi_find 查找数组中第一个满足条件的元素，返回该元素或 undefined。
///
/// find(arr, fn) → 第一个 fn(x) 为真的元素，无则 undefined
fn bi_find(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let arr = bh::as_array(args, 0, "find")?;
    bh::require_arg(args, 1, "find")?;
    let pred = args[1].clone();
    let snap = arr.lock().unwrap().clone();
    for item in snap {
        let matched = vm.call_function_value(pred.clone(), vec![item.clone()])?;
        if matched.is_truthy() {
            return Ok(item);
        }
    }
    Ok(Value::Undefined)
}

/// bi_sprintf 格式化字符串（同 printf 的格式，但返回字符串而非打印）。
fn bi_sprintf(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = sprintf(args)?;
    Ok(Value::str_from(s))
}

/// bi_adjust_float 消除浮点计算精度误差。
fn bi_adjust_float(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let x = bh::as_float(args, 0, "adjustFloat")?;
    let prec = if args.len() > 1 {
        bh::as_int(args, 1, "adjustFloat")? as usize
    } else {
        10
    };
    let formatted = format!("{:.*}", prec, x);
    let result = formatted.parse::<f64>().map_err(|_| crate::value::error_value(
        format!("adjustFloat() 解析失败: {}", formatted),
    ))?;
    Ok(Value::Float(result))
}

/// bi_pass 空操作占位符（对标 Charlang pass()）。
fn bi_pass(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Undefined)
}

/// bi_plt 打印类型+值（对标 Charlang plt）。
/// 输出格式：(类型名)值
fn bi_plt(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let out = _vm.output_handle();
    for v in args {
        writeln!(out.lock().unwrap(), "({}) {}", v.type_name(), v.inspect())
            .map_err(|e| crate::value::error_value(e.to_string()))?;
    }
    Ok(Value::Undefined)
}

/// bi_get_param 从 argsG 中取第 idx 个参数，不存在则返回默认值。
/// 用法：getParam(argsG, index) 或 getParam(argsG, index, default)
fn bi_get_param(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    if args.is_empty() {
        return Ok(Value::Undefined);
    }
    let arr = match &args[0] {
        Value::Array(a) => a,
        _ => return Ok(args.get(2).cloned().unwrap_or(Value::Undefined)),
    };
    let idx = if args.len() > 1 { bh::as_int(args, 1, "getParam")? as usize } else { 0 };
    let guard = arr.lock().unwrap();
    Ok(guard.get(idx).cloned().unwrap_or_else(|| {
        args.get(2).cloned().unwrap_or(Value::Undefined)
    }))
}

/// bi_get_switch 从参数数组中按 --key=value 或 -key=value 格式提取开关值。
///
/// 用法：
///   getSwitch(argsG, "--host=", "localhost")  → 匹配 --host=xxx 返回 xxx
///   getSwitch(argsG, "-port=", "22")          → 匹配 -port=xxx 返回 xxx
///
/// key 参数应包含前缀（- 或 --）和等号（=）。
/// 如果找不到匹配，返回 default。
fn bi_get_switch(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let arr = match args.get(0) {
        Some(Value::Array(a)) => a.lock().unwrap().clone(),
        _ => return Ok(args.get(2).cloned().unwrap_or(Value::Undefined)),
    };
    let key = args.get(1).map(|v| v.to_str()).unwrap_or_default();
    let default = args.get(2).cloned().unwrap_or(Value::Undefined);

    for arg in &arr {
        let s = arg.to_str();
        if s.starts_with(&key) {
            let val = &s[key.len()..];
            return Ok(Value::str(val));
        }
    }
    Ok(default)
}

/// bi_get_all_switches 从参数数组中提取所有匹配 --key=value 的值（可多个同名）。
///
/// 用法：getAllSwitches(argsG, "--attach=") → ["file1.pdf", "file2.xlsx"]
/// 返回所有匹配值的数组。无匹配时返回空数组。
fn bi_get_all_switches(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use std::sync::{Arc, Mutex};
    let arr = match args.get(0) {
        Some(Value::Array(a)) => a.lock().unwrap().clone(),
        _ => return Ok(Value::Array(Arc::new(Mutex::new(Vec::new())))),
    };
    let key = args.get(1).map(|v| v.to_str()).unwrap_or_default();

    let mut results: Vec<Value> = Vec::new();
    for arg in &arr {
        let s = arg.to_str();
        if s.starts_with(&key) {
            results.push(Value::str_from(s[key.len()..].to_string()));
        }
    }
    Ok(Value::Array(Arc::new(Mutex::new(results))))
}

/// bi_if_switch_exists 检查参数数组中是否存在某个开关（布尔型，无值）。
///
/// 用法：ifSwitchExists(argsG, "--verbose")  → true/false
///       ifSwitchExists(argsG, "-v")         → true/false
fn bi_if_switch_exists(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let arr = match args.get(0) {
        Some(Value::Array(a)) => a.lock().unwrap().clone(),
        _ => return Ok(Value::Bool(false)),
    };
    let key = args.get(1).map(|v| v.to_str()).unwrap_or_default();
    Ok(Value::Bool(arr.iter().any(|arg| arg.to_str() == key)))
}
fn bi_compile(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let src = bh::as_str(args, 0, "compile")?;
    let tokens = crate::lexer::tokenize(src, "<compile>")
        .map_err(|e| crate::value::error_value(format!("compile() 词法错误: {}", e)))?;
    let prog = crate::parser::parse_program(tokens, "<compile>")
        .map_err(|e| crate::value::error_value(format!("compile() 语法错误: {}", e)))?;
    let code = crate::compiler::compile(&prog)
        .map_err(|e| crate::value::error_value(format!("compile() 编译错误: {}", e)))?;
    Ok(Value::Native(std::sync::Arc::new(std::sync::Arc::new(code))))
}

/// bi_run_code 执行编译后的 Code 对象，返回结果。
fn bi_run_code(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use std::sync::Arc;
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "runCode")?;
    let code_arc: Arc<crate::opcode::Code> = match &args[0] {
        Value::Native(n) => {
            n.downcast_ref::<Arc<crate::opcode::Code>>()
                .ok_or_else(|| crate::value::error_value("runCode() 参数不是编译后的代码对象"))?
                .clone()
        }
        _ => return Err(crate::value::error_value("runCode() 参数应为 compile() 的返回值")),
    };
    vm.run(code_arc)
}

/// bi_new_ref 创建引用容器，包装一个初始值。
///
/// 用法：newRef(value) → 返回引用对象
/// 引用是独立可变容器，函数传参后可修改容器内的值。
fn bi_new_ref(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "newRef")?;
    Ok(Value::Native(std::sync::Arc::new(std::sync::Arc::new(
        std::sync::Mutex::new(args[0].clone()),
    ))))
}

/// bi_get_value_by_ref 读取引用容器内的值。
///
/// 用法：getValueByRef(ref) → 返回引用内的值
fn bi_get_value_by_ref(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "getValueByRef")?;
    match &args[0] {
        Value::Native(n) => {
            if let Some(cell) = n.downcast_ref::<std::sync::Arc<std::sync::Mutex<Value>>>() {
                Ok(cell.lock().unwrap().clone())
            } else {
                Err(crate::value::error_value("getValueByRef() 参数不是引用对象（用 newRef 创建）"))
            }
        }
        v => Err(crate::value::error_value(format!(
            "getValueByRef() 参数应为引用，得到 {}", v.type_name(),
        ))),
    }
}

/// bi_set_value_by_ref 设置引用容器内的值。
///
/// 用法：setValueByRef(ref, newValue) → 返回 undefined
fn bi_set_value_by_ref(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "setValueByRef")?;
    bh::require_arg(args, 1, "setValueByRef")?;
    match &args[0] {
        Value::Native(n) => {
            if let Some(cell) = n.downcast_ref::<std::sync::Arc<std::sync::Mutex<Value>>>() {
                *cell.lock().unwrap() = args[1].clone();
                Ok(Value::Undefined)
            } else {
                Err(crate::value::error_value("setValueByRef() 第一个参数不是引用对象（用 newRef 创建）"))
            }
        }
        v => Err(crate::value::error_value(format!(
            "setValueByRef() 第一个参数应为引用，得到 {}", v.type_name(),
        ))),
    }
}

/// bi_to_kmg 将数字转为带单位的易读字符串（K/M/G/T）。
///
/// 用法：toKMG(n) 或 toKMG(n, decimals)
/// 默认保留 2 位小数。1024 进制（KB = 1024 bytes）。
/// 例：toKMG(1536) → "1.50K"，toKMG(1048576) → "1.00M"
fn bi_to_kmg(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let n = bh::as_float(args, 0, "toKMG")?;
    let decimals = match args.get(1) {
        Some(Value::Int(d)) => *d as usize,
        _ => 2,
    };
    let units = ["", "K", "M", "G", "T", "P"];
    let mut size = n.abs();
    let mut idx = 0;
    while size >= 1024.0 && idx < units.len() - 1 {
        size /= 1024.0;
        idx += 1;
    }
    Ok(Value::str_from(format!("{:.*}{}", decimals, size, units[idx])))
}

/// bi_dump_var 转储变量详细信息，返回多行诊断字符串。
///
/// 输出包含：类型名、类型码、值摘要。
/// 用于调试与 AI 定位问题。
fn bi_dump_var(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "dumpVar")?;
    let v = &args[0];
    let info = format!(
        "type: {}\ntypeCode: {}\nvalue: {}",
        v.type_name_ex(),
        v.type_code() as u32,
        v.inspect(),
    );
    Ok(Value::str_from(info))
}

/// bi_globals 列出所有全局变量名，返回 array<string>。
///
/// 用于反射与调试。
fn bi_globals(vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    let g = vm.globals_handle();
    let guard = g.lock().unwrap();
    let names: Vec<Value> = guard.keys().map(|k| Value::str_from(k.clone())).collect();
    Ok(Value::Array(std::sync::Arc::new(std::sync::Mutex::new(names))))
}

/// bi_show_table 将二维数组渲染为对齐的 ASCII 表格字符串。
///
/// 用法：
///   showTable(data)            — data 第一行作为表头
///   showTable(data, opts)      — opts 为 map，支持 header(默认true)、sep(默认"|")
///
/// data: Array of Array，每行元素会转为字符串显示
/// 返回：表格字符串（不直接打印，调用方可用 println 输出）
///
/// 例：
///   showTable([["姓名","年龄"],["张三",20],["李四",25]])
///   →
///   +------+----+
///   | 姓名 | 年龄 |
///   +------+----+
///   | 张三 | 20  |
///   | 李四 | 25  |
///   +------+----+
fn bi_show_table(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let arr = bh::as_array(args, 0, "showTable")?;
    let snapshot = arr.lock().unwrap().clone();

    if snapshot.is_empty() {
        return Ok(Value::str_from("(empty table)".to_string()));
    }

    // 解析可选第二参数 opts（map/object），支持 header 和 sep
    let mut header = true;
    let mut sep = "|".to_string();
    if args.len() > 1 {
        match &args[1] {
            Value::Map(m) => {
                let g = m.lock().unwrap();
                if let Some(Value::Bool(b)) = g.get("header") {
                    header = b;
                }
                if let Some(Value::Str(s)) = g.get("sep") {
                    sep = s.to_string();
                }
            }
            Value::Object(o) => {
                let g = o.lock().unwrap();
                if let Some(Value::Bool(b)) = g.data.get("header") {
                    header = *b;
                }
                if let Some(Value::Str(s)) = g.data.get("sep") {
                    sep = s.to_string();
                }
            }
            _ => {}
        }
    }

    // 把每行转为 Vec<String>，校验每行是数组
    let mut rows: Vec<Vec<String>> = Vec::with_capacity(snapshot.len());
    for (i, row) in snapshot.iter().enumerate() {
        match row {
            Value::Array(r) => {
                let cells: Vec<String> = r.lock().unwrap().iter().map(|v| v.to_str()).collect();
                rows.push(cells);
            }
            v => {
                return Err(crate::value::error_value(format!(
                    "showTable() 第 {} 行不是数组 (得到 {})，可能原因：每行必须是一维数组",
                    i, v.type_name(),
                )));
            }
        }
    }

    // 计算每列最大宽度
    let n_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if n_cols == 0 {
        return Ok(Value::str_from("(empty table)".to_string()));
    }
    let mut widths = vec![0usize; n_cols];
    for r in &rows {
        for (i, c) in r.iter().enumerate() {
            let w = c.chars().count();
            if w > widths[i] {
                widths[i] = w;
            }
        }
    }

    // 渲染：边框 + 数据行
    let pad = |s: &str, w: usize| -> String {
        let len = s.chars().count();
        if len >= w {
            s.to_string()
        } else {
            format!("{}{}", s, " ".repeat(w - len))
        }
    };

    let border = {
        let mut b = String::from("+");
        for &w in &widths {
            b.push_str(&"-".repeat(w + 2));
            b.push('+');
        }
        b
    };

    let mut out = String::new();
    out.push_str(&border);
    out.push('\n');

    for (idx, r) in rows.iter().enumerate() {
        if header && idx == 0 {
            // 表头行
            out.push_str(&sep);
            for (i, &w) in widths.iter().enumerate() {
                let cell = r.get(i).map(|s| s.as_str()).unwrap_or("");
                out.push(' ');
                out.push_str(&pad(cell, w));
                out.push(' ');
                out.push_str(&sep);
            }
            out.push('\n');
            out.push_str(&border);
            out.push('\n');
        } else {
            out.push_str(&sep);
            for (i, &w) in widths.iter().enumerate() {
                let cell = r.get(i).map(|s| s.as_str()).unwrap_or("");
                out.push(' ');
                out.push_str(&pad(cell, w));
                out.push(' ');
                out.push_str(&sep);
            }
            out.push('\n');
        }
    }
    out.push_str(&border);

    Ok(Value::str_from(out))
}

// ---- 成员反射函数 ----
//
// 设计要点：
//   - getMember: 反射式读取 Object/Map 的成员值（字符串 key）
//   - setMember: 反射式设置 Object/Map 的成员值（修改原对象）
//   - callMethod: 调用 Object 上的方法（沿原型链查找）
//   - 与 obj.key / obj.key = v / obj.method(args) 的区别：
//     内置函数接收动态 key 字符串，便于反射式编程

/// bi_get_member 获取对象/Map 的成员值。
///
/// 用法：getMember(obj, key) → value 或 undefined
///
/// obj 为 Object 或 Map，key 为字符串。
/// Object 沿原型链查找；Map 仅查自身（Map 本就是纯数据容器）。
/// 不存在时返回 undefined（不报错），便于链式判空。
fn bi_get_member(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let key = bh::as_str(args, 1, "getMember")?;
    match &args[0] {
        Value::Object(o) => {
            // 沿原型链查找
            Ok(o.lock().unwrap().get_proto(key).unwrap_or(Value::Undefined))
        }
        Value::Map(m) => {
            // Map 不支持原型链，仅查自身
            Ok(m.lock().unwrap().get(key).unwrap_or(Value::Undefined))
        }
        v => Err(crate::value::error_value(format!(
            "getMember() 第 1 个参数应为 object 或 map，得到 {} (可能原因：参数顺序错误，正确顺序 getMember(obj, key))",
            v.type_name(),
        ))),
    }
}

/// bi_set_member 设置对象/Map 的成员值（原地修改）。
///
/// 用法：setMember(obj, key, value) → undefined
///
/// obj 为 Object 或 Map，key 为字符串。
/// 仅写入自身（不沿原型链），与 obj.key = v 语义一致。
fn bi_set_member(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let key = bh::as_str(args, 1, "setMember")?;
    bh::require_arg(args, 2, "setMember")?;
    let val = args[2].clone();

    match &args[0] {
        Value::Object(o) => {
            o.lock().unwrap().set(key.to_string(), val);
            Ok(Value::Undefined)
        }
        Value::Map(m) => {
            m.lock().unwrap().set(key.to_string(), val);
            Ok(Value::Undefined)
        }
        v => Err(crate::value::error_value(format!(
            "setMember() 第 1 个参数应为 object 或 map，得到 {} (可能原因：参数顺序错误，正确顺序 setMember(obj, key, value))",
            v.type_name(),
        ))),
    }
}

/// bi_call_method 调用对象的方法（沿原型链查找）。
///
/// 用法：
///   callMethod(obj, methodName)              — 无参数调用
///   callMethod(obj, methodName, argsArray)    — 带参数调用
///
/// 先在 Object 上沿原型链查找 methodName，找到则调用。
/// 调用时 obj 作为隐式 self（第一个参数）传入，args 数组中的元素作为后续参数。
/// 如果对象没有该方法，返回错误。
///
/// 与 obj.method(args) 的区别：methodName 为动态字符串，便于反射式调用。
fn bi_call_method(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let obj = match args.get(0) {
        Some(v) => v.clone(),
        None => return Err(crate::value::error_value(
            "callMethod() 需要至少 2 个参数 (可能原因：参数缺失)",
        )),
    };
    let method_name = bh::as_str(args, 1, "callMethod")?;

    // 收集调用参数（obj 作为第一个 self 参数）
    let mut call_args: Vec<Value> = vec![obj.clone()];
    if let Some(args_val) = args.get(2) {
        match args_val {
            Value::Array(a) => {
                let items: Vec<Value> = a.lock().unwrap().clone();
                call_args.extend(items);
            }
            other => {
                // 非数组的单个值作为单个参数
                call_args.push(other.clone());
            }
        }
    }

    // 在 Object 上沿原型链查找方法
    let method = match &obj {
        Value::Object(o) => {
            match o.lock().unwrap().get_proto(method_name) {
                Some(v) => v,
                None => return Err(crate::value::error_value(format!(
                    "callMethod() 对象上找不到方法 '{}' (可能原因：方法名拼写错误或未在原型链上定义)",
                    method_name,
                ))),
            }
        }
        v => return Err(crate::value::error_value(format!(
            "callMethod() 第 1 个参数应为 object，得到 {} (可能原因：参数顺序错误，正确顺序 callMethod(obj, methodName, args?))",
            v.type_name(),
        ))),
    };

    // 调用方法（self 作为第一个参数）
    vm.call_function_value(method, call_args)
}
