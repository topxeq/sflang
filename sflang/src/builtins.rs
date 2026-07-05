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
    // ---- 实用函数（对标 charlang 常见编程任务）----
    vm.register_builtin("uuid", bi_uuid);
    vm.register_builtin("randomStr", bi_random_str);
    vm.register_builtin("values", bi_values);
    vm.register_builtin("hasKey", bi_has_key);
    vm.register_builtin("deepClone", bi_deep_clone);
    vm.register_builtin("newObject", bi_new_object);
    vm.register_builtin("filter", bi_filter);
    vm.register_builtin("map", bi_map);
    vm.register_builtin("sprintf", bi_sprintf);
    vm.register_builtin("spr", bi_sprintf);
    vm.register_builtin("fpr", bi_printf);
    vm.register_builtin("adjustFloat", bi_adjust_float);
    // 类型判断谓词
    vm.register_builtin("isArray", bi_is_array);
    vm.register_builtin("isString", bi_is_string);
    vm.register_builtin("isObject", bi_is_object);
    vm.register_builtin("isByteArray", bi_is_byte_array);
    vm.register_builtin("isBytes", bi_is_bytes);
    vm.register_builtin("isFile", bi_is_file);
    vm.register_builtin("byte", bi_byte);
    vm.register_builtin("isByte", bi_is_byte);
    vm.register_builtin("newMap", bi_new_map);
    vm.register_builtin("isMap", bi_is_map);
    vm.register_builtin("entries", bi_entries);
    vm.register_builtin("dataKeys", bi_data_keys);
    vm.register_builtin("dataValues", bi_data_values);
    vm.register_builtin("bytesXor", bi_bytes_xor);
    vm.register_builtin("bytesXorInPlace", bi_bytes_xor_in_place);
    vm.register_builtin("isBigInt", bi_is_big_int_pred);
    vm.register_builtin("isBigFloat", bi_is_big_float_pred);
    vm.register_builtin("isNumber", bi_is_number);
    vm.register_builtin("isInt", bi_is_int);
    vm.register_builtin("isFloat", bi_is_float);
    vm.register_builtin("isBool", bi_is_bool);
    vm.register_builtin("isUndefined", bi_is_undefined);
    vm.register_builtin("isFunction", bi_is_function);
    // ---- undefined 配套内置函数（对标 Charlang 的 nilToEmpty 等）----
    vm.register_builtin("undefToEmpty", bi_undef_to_empty);
    vm.register_builtin("default", bi_default);
    vm.register_builtin("defaultUndef", bi_default_undef);
    vm.register_builtin("explainUndef", bi_explain_undef);
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
        Value::Str(s) => s.parse::<i64>().map(Value::Int).map_err(|_| {
            crate::value::error_value(format!("int() 无法解析 '{}' (可能原因：字符串不是有效整数)", s))
        }),
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
fn bi_sleep(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("sleep() 需要 1 个参数 (毫秒)"));
    }
    let ms = args[0].to_int().ok_or_else(|| crate::value::error_value("sleep() 参数需为整数"))?;
    std::thread::sleep(std::time::Duration::from_millis(ms as u64));
    Ok(Value::Undefined)
}

// ---- 类型判断谓词 ----
//
// 每个谓词接收 1 个参数，返回 Bool。
// 空 args 时返回 false（而非报错），便于链式判断。

/// bi_is_array 判断是否为数组。
fn bi_is_array(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(args.get(0), Some(Value::Array(_)))))
}

/// bi_is_string 判断是否为字符串。
fn bi_is_string(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(args.get(0), Some(Value::Str(_)))))
}

/// bi_is_object 判断是否为对象/映射。
fn bi_is_object(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(args.get(0), Some(Value::Object(_)))))
}

/// bi_is_byte_array 判断是否为可变字节序列 byteArray。
fn bi_is_byte_array(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(args.get(0), Some(Value::ByteArray(_)))))
}

/// bi_is_bytes 判断是否为不可变字节序列 bytes。
fn bi_is_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(args.get(0), Some(Value::Bytes(_)))))
}

/// bi_is_file 判断是否为文件句柄 file。
fn bi_is_file(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(args.get(0), Some(Value::File(_)))))
}

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

/// bi_is_byte 判断是否为 byte 类型。
fn bi_is_byte(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(args.get(0), Some(Value::Byte(_)))))
}

/// bi_new_map 创建空有序 Map。
fn bi_new_map(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Map(std::sync::Arc::new(std::sync::Mutex::new(crate::ord_map::OrdMap::new()))))
}

/// bi_is_map 判断是否为有序 Map 类型。
fn bi_is_map(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(args.get(0), Some(Value::Map(_)))))
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

/// bi_is_big_int_pred 判断是否为任意精度整数 bigInt。
fn bi_is_big_int_pred(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(args.get(0), Some(Value::BigInt(_)))))
}

/// bi_is_big_float_pred 判断是否为任意精度浮点 bigFloat。
fn bi_is_big_float_pred(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(args.get(0), Some(Value::BigFloat(_)))))
}

/// bi_is_number 判断是否为数字（Int 或 Float）。
fn bi_is_number(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(args.get(0).map(|v| v.is_number()).unwrap_or(false)))
}

/// bi_is_int 判断是否为整数。
fn bi_is_int(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(args.get(0), Some(Value::Int(_)))))
}

/// bi_is_float 判断是否为浮点。
fn bi_is_float(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(args.get(0), Some(Value::Float(_)))))
}

/// bi_is_bool 判断是否为布尔。
fn bi_is_bool(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(args.get(0), Some(Value::Bool(_)))))
}

/// bi_is_undefined 判断是否为 undefined（含旧称 nil）。
///
/// 缺参时返回 true（便于链式判空：`isUndefined(m["maybe"])`）。
fn bi_is_undefined(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(args.get(0), Some(Value::Undefined) | None)))
}

/// bi_is_function 判断是否为函数（用户函数或内置函数）。
fn bi_is_function(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(args.get(0), Some(Value::Func(_)) | Some(Value::Builtin(_)))))
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
    let o = bh::as_object(args, 0, "hasKey")?;
    let key = bh::as_str(args, 1, "hasKey")?;
    Ok(Value::Bool(o.lock().unwrap().has(key)))
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

/// bi_sprintf 格式化字符串（同 printf 的格式，但返回字符串而非打印）。
fn bi_sprintf(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = sprintf(args)?;
    Ok(Value::str_from(s))
}

/// bi_adjust_float 消除浮点计算精度误差（如 0.1+0.2=0.30000000000000004 → 0.3）。
///
/// 用法：adjustFloat(x) 或 adjustFloat(x, precision)
/// 默认 precision=10（保留 10 位小数，去除末尾精度噪声）。
/// 算法：将浮点数格式化为 precision 位小数的字符串，再解析回来。
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
