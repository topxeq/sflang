//! builtins_bytes.rs — 字节序列内置函数（bytes / byteArray）
//!
//! 设计要点（来自 AGENTS.md 与 byteArray 设计讨论）：
//!   - bytes：不可变字节序列（Arc<Vec<u8>>），用于只读场景（读取、哈希、传输）
//!   - byteArray：可变字节序列（Arc<Mutex<Vec<u8>>>），用于就地修改（按位加密、协议改包）
//!   - 两者转换有拷贝，保证修改互不影响（类似 Python bytes/bytearray、Rust &[u8]/Vec<u8>）
//!   - 仅依赖 Rust 标准库
//!   - 错误信息 AI 友好：附函数名、期望类型、可能原因
//!
//! 函数列表：
//!   构造/转换：
//!     byteArray(n) / byteArray(n, fill)  — 创建 n 字节（默认填 0 或指定值）
//!     bytes(v)                           — 转 bytes（string→UTF8字节；byteArray→拷贝；Array<Int>→字节）
//!     byteArrayFromBytes(b)              — bytes → byteArray（拷贝）
//!     byteArrayFromArray(arr)            — Array<Int> → byteArray
//!     arrayFromByteArray(ba)             — byteArray → Array<Int>（每字节一个 Int）
//!     strFromBytes(b, "utf8"|"latin1"|"hex") — bytes → string（指定解码）
//!   操作：
//!     copy(dst, src) / copy(dst, src, dstStart) — 批量复制（类似 Go copy），返回复制字节数
//!     bytesHex(b)                       — bytes/byteArray → 十六进制字符串
//!     bytesFromHex(s)                   — 十六进制字符串 → bytes

use std::sync::{Arc, Mutex};

use crate::builtins_helpers as bh;
use crate::function::BuiltinDoc;
use crate::value::Value;
use crate::vm::VM;

// ---- 字节序列函数文档 ----

static DOC_BYTE_ARRAY: BuiltinDoc = BuiltinDoc {
    category: "bytes",
    signature: "byteArray(n, fill?) -> byteArray",
    summary: "创建 n 字节的可变 byteArray，默认全填 0 或指定填充值。",
    params: &[
        ("n", "字节数（int，非负）"),
        ("fill", "可选 int 0-255：填充值，默认 0"),
    ],
    returns: "byteArray：长度为 n 的可变字节序列",
    examples: &[
        "byteArray(4)         → [0x00, 0x00, 0x00, 0x00]",
        "byteArray(3, 255)    → [0xFF, 0xFF, 0xFF]",
    ],
    errors: &["n 不能为负；fill 须在 0-255 范围内"],
};

static DOC_BYTES: BuiltinDoc = BuiltinDoc {
    category: "bytes",
    signature: "bytes(v) -> bytes",
    summary: "转为不可变 bytes：string→UTF-8 字节、byteArray→只读快照、array<int>→字节。",
    params: &[("v", "string（UTF-8 字节）/ byteArray（拷贝快照）/ array<int>（每个元素 0-255）")],
    returns: "bytes：不可变字节序列",
    examples: &[
        "bytes(\"AB\")           → [0x41, 0x42]",
        "bytes([65, 66])       → [0x41, 0x42]",
    ],
    errors: &[
        "array<int> 模式下每个元素须为 0-255 的 int",
        "参数应为 string/byteArray/array<int>，其他类型报错",
    ],
};

static DOC_BYTE_ARRAY_FROM_BYTES: BuiltinDoc = BuiltinDoc {
    category: "bytes",
    signature: "byteArrayFromBytes(v) -> byteArray",
    summary: "从 bytes 创建可变 byteArray（拷贝）；也接受 byteArray/string。",
    params: &[("v", "bytes / byteArray / string（UTF-8 字节）")],
    returns: "byteArray：内容相同的可变副本",
    examples: &[
        "byteArrayFromBytes(bytes(\"AB\"))   → [0x41, 0x42]",
        "byteArrayFromBytes(\"AB\")          → [0x41, 0x42]",
    ],
    errors: &["参数应为 bytes/byteArray/string"],
};

static DOC_BYTE_ARRAY_FROM_ARRAY: BuiltinDoc = BuiltinDoc {
    category: "bytes",
    signature: "byteArrayFromArray(arr) -> byteArray",
    summary: "从 array<int> 创建可变 byteArray（每个元素作为一字节）。",
    params: &[("arr", "array<int>：每个元素须为 0-255 的 int")],
    returns: "byteArray：元素值组成的可变字节序列",
    examples: &[
        "byteArrayFromArray([65, 66, 67])  → [0x41, 0x42, 0x43]",
    ],
    errors: &["数组元素须为 0-255 的 int，越界报错并附元素索引"],
};

static DOC_ARRAY_FROM_BYTE_ARRAY: BuiltinDoc = BuiltinDoc {
    category: "bytes",
    signature: "arrayFromByteArray(b) -> array<int>",
    summary: "将 byteArray/bytes 转为 array<int>（每字节一个 int 0-255）。",
    params: &[("b", "byteArray 或 bytes")],
    returns: "array<int>：每字节一个 int（0-255）",
    examples: &[
        "arrayFromByteArray(bytes(\"AB\"))  → [65, 66]",
    ],
    errors: &["参数应为 byteArray 或 bytes"],
};

static DOC_STR_FROM_BYTES: BuiltinDoc = BuiltinDoc {
    category: "bytes",
    signature: "strFromBytes(b, encoding?) -> string",
    summary: "将字节序列按指定编码解码为字符串（默认 utf8）。",
    params: &[
        ("b", "bytes 或 byteArray"),
        ("encoding", "可选 \"utf8\"(默认) / \"latin1\" / \"hex\""),
    ],
    returns: "string：解码后的字符串",
    examples: &[
        "strFromBytes(bytes(\"你好\"))          → \"你好\"",
        "strFromBytes(bytes([0x41,0x42]), \"latin1\") → \"AB\"",
        "strFromBytes(bytes([0x41]), \"hex\")   → \"41\"",
    ],
    errors: &[
        "utf8 非法字节会被替换为 U+FFFD（不报错）",
        "encoding 仅支持 utf8/latin1/hex",
    ],
};

static DOC_COPY: BuiltinDoc = BuiltinDoc {
    category: "bytes",
    signature: "copy(dst, src, dstStart?) -> int",
    summary: "批量复制字节到 byteArray（类似 Go copy），返回实际复制字节数。",
    params: &[
        ("dst", "目标 byteArray（可变，原地修改）"),
        ("src", "源 bytes/byteArray/string"),
        ("dstStart", "可选 int：dst 写入起始位置，默认 0"),
    ],
    returns: "int：实际复制字节数 = min(len(src), len(dst) - dstStart)",
    examples: &[
        "dst := byteArray(4); copy(dst, bytes(\"AB\"))   → 2（dst=[0x41,0x42,0,0]）",
        "dst := byteArray(4); copy(dst, bytes(\"AB\"), 2) → 2（dst=[0,0,0x41,0x42]）",
    ],
    errors: &[
        "dst 必须是 byteArray（参数顺序：dst 在前、src 在后）",
        "dstStart 不能为负，且不能超过 dst 长度",
    ],
};

static DOC_BYTES_HEX: BuiltinDoc = BuiltinDoc {
    category: "bytes",
    signature: "bytesHex(b) -> string",
    summary: "将 bytes/byteArray 转为小写十六进制字符串。",
    params: &[("b", "bytes 或 byteArray")],
    returns: "string：每字节两位小写十六进制",
    examples: &[
        "bytesHex(bytes(\"AB\"))  → \"4142\"",
        "bytesHex(bytes([255]))  → \"ff\"",
    ],
    errors: &["参数应为 bytes 或 byteArray"],
};

static DOC_BYTES_FROM_HEX: BuiltinDoc = BuiltinDoc {
    category: "bytes",
    signature: "bytesFromHex(hex) -> bytes",
    summary: "将十六进制字符串转为 bytes（自动忽略空格/冒号/横线等分隔符）。",
    params: &[("hex", "十六进制字符串；有效字符需成对（偶数个）")],
    returns: "bytes：解码后的字节序列",
    examples: &[
        "bytesFromHex(\"4142\")       → bytes([0x41, 0x42])",
        "bytesFromHex(\"41:42\")      → bytes([0x41, 0x42])（忽略冒号）",
    ],
    errors: &["有效十六进制字符数必须为偶数"],
};

static DOC_HEX_ENCODE: BuiltinDoc = BuiltinDoc {
    category: "bytes",
    signature: "hexEncode(v) -> string",
    summary: "将 string/bytes/byteArray 编码为小写十六进制字符串。",
    params: &[("v", "string（UTF-8 字节）/ bytes / byteArray")],
    returns: "string：每字节两位小写十六进制",
    examples: &[
        "hexEncode(\"AB\")   → \"4142\"",
        "hexEncode([0xFF])  → \"ff\"",
    ],
    errors: &["参数应为 string/bytes/byteArray"],
};

static DOC_HEX_DECODE: BuiltinDoc = BuiltinDoc {
    category: "bytes",
    signature: "hexDecode(hex) -> bytes",
    summary: "十六进制字符串解码为 bytes（hexDecode 是 bytesFromHex 的语义化别名）。",
    params: &[("hex", "十六进制字符串；有效字符需成对（偶数个）")],
    returns: "bytes：解码后的字节序列",
    examples: &[
        "hexDecode(\"4142\")  → bytes([0x41, 0x42])",
    ],
    errors: &["有效十六进制字符数必须为偶数"],
};

static DOC_HEX_TO_STR: BuiltinDoc = BuiltinDoc {
    category: "bytes",
    signature: "hexToStr(hex) -> string",
    summary: "将十六进制字符串解码为 UTF-8 字符串（先解码字节再按 UTF-8 解释）。",
    params: &[("hex", "十六进制字符串，前后空白会被忽略")],
    returns: "string：解码后的字符串（非法 UTF-8 字节替换为 U+FFFD）",
    examples: &[
        "hexToStr(\"4142\")      → \"AB\"",
        "hexToStr(\"e4bda0e5a5bd\") → \"你好\"",
    ],
    errors: &["hex 字符串长度必须为偶数；非法 hex 字符报错"],
};

/// register 注册所有字节序列内置函数到 VM。
pub fn register(vm: &mut VM) {
    vm.register_builtin_doc("byteArray", bi_byte_array, &DOC_BYTE_ARRAY);
    vm.register_builtin_doc("bytes", bi_bytes, &DOC_BYTES);
    vm.register_builtin_doc("byteArrayFromBytes", bi_byte_array_from_bytes, &DOC_BYTE_ARRAY_FROM_BYTES);
    vm.register_builtin_doc("byteArrayFromArray", bi_byte_array_from_array, &DOC_BYTE_ARRAY_FROM_ARRAY);
    vm.register_builtin_doc("arrayFromByteArray", bi_array_from_byte_array, &DOC_ARRAY_FROM_BYTE_ARRAY);
    vm.register_builtin_doc("strFromBytes", bi_str_from_bytes, &DOC_STR_FROM_BYTES);
    vm.register_builtin_doc("copy", bi_copy, &DOC_COPY);
    vm.register_builtin_doc("bytesHex", bi_bytes_hex, &DOC_BYTES_HEX);
    vm.register_builtin_doc("bytesFromHex", bi_bytes_from_hex, &DOC_BYTES_FROM_HEX);
    // hex 别名（对标 Charlang，接受 string/bytes/byteArray）
    vm.register_builtin_doc("hexEncode", bi_hex_encode, &DOC_HEX_ENCODE);
    vm.register_builtin_doc("hexDecode", bi_bytes_from_hex, &DOC_HEX_DECODE);
    vm.register_builtin_doc("hexToStr", bi_hex_to_str, &DOC_HEX_TO_STR);
}

/// byte_val 将 Int 值转为 u8，越界或非整数返回错误。
fn byte_val(v: &Value, fn_name: &str) -> Result<u8, Value> {
    match v {
        Value::Int(x) => {
            if *x < 0 || *x > 255 {
                return Err(crate::value::error_value(format!(
                    "{}() 字节值超出范围: {} (需 0-255；可能原因：传入了非字节整数)",
                    fn_name, x,
                )));
            }
            Ok(*x as u8)
        }
        _ => Err(crate::value::error_value(format!(
            "{}() 需要 int 字节值 (0-255)，得到 {} (可能原因：类型不匹配)",
            fn_name, v.type_name(),
        ))),
    }
}

/// bi_byte_array 创建可变字节序列。
///
/// 用法：
///   byteArray(n)        — n 字节，全填 0
///   byteArray(n, fill)  — n 字节，全填 fill（0-255）
fn bi_byte_array(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let n = bh::as_int(args, 0, "byteArray")?;
    if n < 0 {
        return Err(crate::value::error_value(
            "byteArray() 长度不能为负 (可能原因：参数错误)",
        ));
    }
    let fill = if args.len() >= 2 {
        byte_val(&args[1], "byteArray")?
    } else {
        0u8
    };
    let buf = vec![fill; n as usize];
    Ok(Value::ByteArray(Arc::new(Mutex::new(buf))))
}

/// bi_bytes 转为不可变 bytes。
///
/// 支持来源：
///   string        — UTF-8 编码字节
///   byteArray     — 拷贝出只读快照
///   Array<Int>    — 每个 Int 作为一个字节（0-255）
fn bi_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "bytes")?;
    match &args[0] {
        Value::Str(s) => Ok(Value::Bytes(Arc::new(s.as_bytes().to_vec()))),
        Value::Bytes(b) => Ok(Value::Bytes(b.clone())), // 已是 bytes，原样返回
        Value::ByteArray(b) => {
            // 拷贝出只读快照
            let snap = b.lock().unwrap().clone();
            Ok(Value::Bytes(Arc::new(snap)))
        }
        Value::Array(a) => {
            // Array<Int> → bytes
            let arr = a.lock().unwrap();
            let mut buf = Vec::with_capacity(arr.len());
            for (i, v) in arr.iter().enumerate() {
                buf.push(byte_val(v, "bytes").map_err(|e| {
                    // 附加元素索引信息
                    match &e {
                        Value::Error(er) => crate::value::error_value(format!(
                            "{} [元素 #{}]", er.message, i,
                        )),
                        _ => e,
                    }
                })?);
            }
            Ok(Value::Bytes(Arc::new(buf)))
        }
        v => Err(crate::value::error_value(format!(
            "bytes() 不支持类型 {} (可能原因：参数应为 string/byteArray/array<int>)",
            v.type_name(),
        ))),
    }
}

/// bi_byte_array_from_bytes 从不可变 bytes 创建可变 byteArray（拷贝）。
fn bi_byte_array_from_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "byteArrayFromBytes")?;
    match &args[0] {
        Value::Bytes(b) => Ok(Value::ByteArray(Arc::new(Mutex::new(b.as_ref().to_vec())))),
        Value::ByteArray(b) => {
            // byteArray → byteArray：拷贝一份新的（语义上互不影响）
            let snap = b.lock().unwrap().clone();
            Ok(Value::ByteArray(Arc::new(Mutex::new(snap))))
        }
        Value::Str(s) => {
            // string 也支持：UTF-8 字节
            Ok(Value::ByteArray(Arc::new(Mutex::new(s.as_bytes().to_vec()))))
        }
        v => Err(crate::value::error_value(format!(
            "byteArrayFromBytes() 不支持类型 {} (可能原因：参数应为 bytes/byteArray/string)",
            v.type_name(),
        ))),
    }
}

/// bi_byte_array_from_array 从 Array<Int> 创建可变 byteArray。
fn bi_byte_array_from_array(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let arr = bh::as_array(args, 0, "byteArrayFromArray")?;
    let guard = arr.lock().unwrap();
    let mut buf = Vec::with_capacity(guard.len());
    for (i, v) in guard.iter().enumerate() {
        buf.push(byte_val(v, "byteArrayFromArray").map_err(|e| match e {
            Value::Error(er) => crate::value::error_value(format!("{} [元素 #{}]", er.message, i)),
            _ => e,
        })?);
    }
    Ok(Value::ByteArray(Arc::new(Mutex::new(buf))))
}

/// bi_array_from_byte_array 将 byteArray 转为 Array<Int>（每字节一个 Int）。
///
/// 也接受 bytes（不可变）作为输入。
fn bi_array_from_byte_array(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "arrayFromByteArray")?;
    let bytes_vec: Vec<u8> = match &args[0] {
        Value::ByteArray(b) => b.lock().unwrap().clone(),
        Value::Bytes(b) => b.as_ref().to_vec(),
        v => return Err(crate::value::error_value(format!(
            "arrayFromByteArray() 不支持类型 {} (可能原因：参数应为 byteArray/bytes)",
            v.type_name(),
        ))),
    };
    let arr: Vec<Value> = bytes_vec.into_iter().map(|x| Value::Int(x as i64)).collect();
    Ok(Value::Array(Arc::new(Mutex::new(arr))))
}

/// bi_str_from_bytes 将字节序列解码为字符串。
///
/// 用法：strFromBytes(b, encoding)
///   encoding: "utf8"（默认，非法字节替换为 U+FFFD）/ "latin1"（每字节一个码点）/ "hex"（十六进制文本）
fn bi_str_from_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "strFromBytes")?;
    let enc = if args.len() >= 2 {
        bh::as_str(args, 1, "strFromBytes")?.to_string()
    } else {
        "utf8".to_string()
    };
    let bytes_vec: Vec<u8> = match &args[0] {
        Value::Bytes(b) => b.as_ref().to_vec(),
        Value::ByteArray(b) => b.lock().unwrap().clone(),
        v => return Err(crate::value::error_value(format!(
            "strFromBytes() 不支持类型 {} (可能原因：参数应为 bytes/byteArray)",
            v.type_name(),
        ))),
    };
    let s = match enc.as_str() {
        "utf8" | "utf-8" => String::from_utf8_lossy(&bytes_vec).into_owned(),
        "latin1" | "iso-8859-1" => {
            // 每字节直接映射为码点 0-255
            bytes_vec.iter().map(|&b| b as char).collect()
        }
        "hex" => bytes_vec.iter().map(|b| format!("{:02x}", b)).collect(),
        _ => return Err(crate::value::error_value(format!(
            "strFromBytes() 不支持的编码 '{}' (可能原因：编码名错误；支持 utf8/latin1/hex)",
            enc,
        ))),
    };
    Ok(Value::str_from(s))
}

/// bi_copy 批量复制字节（类似 Go 的 copy）。
///
/// 用法：
///   copy(dst, src)              — 从 src 复制到 dst 开头，返回复制字节数
///   copy(dst, src, dstStart)    — 从 dst 的 dstStart 位置开始写入
///
/// dst 必须是 byteArray（可变）；src 可以是 bytes/byteArray/string。
/// 复制字节数 = min(len(src), len(dst) - dstStart)。
fn bi_copy(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.len() < 2 {
        return Err(crate::value::error_value("copy() 需要至少 2 个参数 (dst, src)"));
    }
    let dst_start = if args.len() >= 3 {
        bh::as_int(args, 2, "copy")?
    } else {
        0
    };
    if dst_start < 0 {
        return Err(crate::value::error_value("copy() dstStart 不能为负"));
    }
    // dst 必须是 byteArray
    let dst_arc = match &args[0] {
        Value::ByteArray(b) => b.clone(),
        v => return Err(crate::value::error_value(format!(
            "copy() 目标必须是 byteArray，得到 {} (可能原因：参数顺序错误；dst 应在前)",
            v.type_name(),
        ))),
    };
    // src：bytes/byteArray/string
    let src_vec: Vec<u8> = match &args[1] {
        Value::Bytes(b) => b.as_ref().to_vec(),
        Value::ByteArray(b) => b.lock().unwrap().clone(),
        Value::Str(s) => s.as_bytes().to_vec(),
        v => return Err(crate::value::error_value(format!(
            "copy() 源应为 bytes/byteArray/string，得到 {} (可能原因：类型不匹配)",
            v.type_name(),
        ))),
    };
    let mut dst = dst_arc.lock().unwrap();
    let dst_len = dst.len();
    if (dst_start as usize) > dst_len {
        return Err(crate::value::error_value(format!(
            "copy() dstStart {} 超出目标长度 {} (可能原因：起始位置越界)",
            dst_start, dst_len,
        )));
    }
    let avail = dst_len - dst_start as usize;
    let n = src_vec.len().min(avail);
    dst[dst_start as usize..dst_start as usize + n].copy_from_slice(&src_vec[..n]);
    Ok(Value::Int(n as i64))
}

/// bi_bytes_hex 将字节序列转为十六进制字符串。
///
/// 接受 bytes 或 byteArray。
fn bi_bytes_hex(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "bytesHex")?;
    let hex: String = match &args[0] {
        Value::Bytes(b) => b.iter().map(|x| format!("{:02x}", x)).collect(),
        Value::ByteArray(b) => b.lock().unwrap().iter().map(|x| format!("{:02x}", x)).collect(),
        v => return Err(crate::value::error_value(format!(
            "bytesHex() 不支持类型 {} (可能原因：参数应为 bytes/byteArray)",
            v.type_name(),
        ))),
    };
    Ok(Value::str_from(hex))
}

/// bi_bytes_from_hex 将十六进制字符串转为 bytes。
///
/// 字符串可含空格/冒号（自动忽略），长度（有效十六进制字符）须为偶数。
fn bi_bytes_from_hex(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = bh::as_str(args, 0, "bytesFromHex")?;
    // 过滤非十六进制字符（忽略空格、冒号、横线等分隔符）
    let cleaned: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if cleaned.len() % 2 != 0 {
        return Err(crate::value::error_value(format!(
            "bytesFromHex() 十六进制字符数为奇数 {} (可能原因：缺少一个字符；有效字符需成对)",
            cleaned.len(),
        )));
    }
    let mut buf = Vec::with_capacity(cleaned.len() / 2);
    let bytes = cleaned.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = (bytes[i] as char).to_digit(16).unwrap() as u8;
        let lo = (bytes[i + 1] as char).to_digit(16).unwrap() as u8;
        buf.push((hi << 4) | lo);
        i += 2;
    }
    Ok(Value::Bytes(Arc::new(buf)))
}

/// bi_hex_encode 将 string/bytes/byteArray 编码为 hex 字符串。
fn bi_hex_encode(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "hexEncode")?;
    let data: Vec<u8> = match &args[0] {
        Value::Str(s) => s.as_bytes().to_vec(),
        Value::Bytes(b) => b.as_ref().to_vec(),
        Value::ByteArray(b) => b.lock().unwrap().clone(),
        other => return Err(crate::value::error_value(format!(
            "hexEncode() 不支持类型 {} (需要 string/bytes/byteArray)", other.type_name(),
        ))),
    };
    let hex: String = data.iter().map(|b| format!("{:02x}", b)).collect();
    Ok(Value::str_from(hex))
}

/// bi_hex_to_str 将 hex 字符串解码为原始字符串。
fn bi_hex_to_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    // 复用 bytesFromHex 逻辑，再转字符串
    let hex = bh::as_str(args, 0, "hexToStr")?;
    let cleaned: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.len() % 2 != 0 {
        return Err(crate::value::error_value("hexToStr() hex 字符串长度必须为偶数"));
    }
    let bytes = cleaned.as_bytes();
    let mut buf = Vec::with_capacity(bytes.len() / 2);
    let mut i = 0;
    while i < bytes.len() {
        let hi = (bytes[i] as char).to_digit(16).ok_or_else(|| crate::value::error_value(
            format!("hexToStr() 非法 hex 字符 '{}'", bytes[i] as char),
        ))? as u8;
        let lo = (bytes[i + 1] as char).to_digit(16).ok_or_else(|| crate::value::error_value(
            format!("hexToStr() 非法 hex 字符 '{}'", bytes[i + 1] as char),
        ))? as u8;
        buf.push((hi << 4) | lo);
        i += 2;
    }
    Ok(Value::str_from(String::from_utf8_lossy(&buf).into_owned()))
}
