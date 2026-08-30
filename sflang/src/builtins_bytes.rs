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
//!   字节序整数互转（对标 xie）：
//!     bytesToData(data, "-endian=B|L")  — 字节序列 → 无符号整数（int|bigInt）
//!     dataToBytes(v, "-endian=B|L", "-size=N") — 无符号整数 → 定长字节序列

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
    // 字节序整数互转（对标 xie 的 bytesToData/dataToBytes）
    vm.register_builtin_doc("bytesToData", bi_bytes_to_data, &DOC_BYTES_TO_DATA);
    vm.register_builtin_doc("dataToBytes", bi_data_to_bytes, &DOC_DATA_TO_BYTES);
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

// ---- 字节序整数互转（对标 xie 的 bytesToData/dataToBytes）----

static DOC_BYTES_TO_DATA: BuiltinDoc = BuiltinDoc {
    category: "bytes",
    signature: "bytesToData(data[, \"-endian=B|L\"]) -> int|bigInt",
    summary: "将字节序列按指定字节序解读为无符号整数（1-16 字节）。",
    params: &[
        ("data", "bytes/byteArray，长度 1-16 字节"),
        ("-endian=B|L", "可选。B=大端（默认，网络序），L=小端"),
    ],
    returns: "int（能放入 i64 时）或 bigInt（更大时）；空输入报错",
    examples: &[
        "bytesToData(bytesFromHex(\"00000000000007FF\"), \"-endian=B\")   // 2047",
        "bytesToData(bytesFromHex(\"FF01\"), \"-endian=L\")               // 511",
    ],
    errors: &[
        "空字节序列或超过 16 字节报错",
        "参数应为 bytes/byteArray，其他类型报错",
        "-endian= 只接受 B/L 开头的值",
    ],
};

static DOC_DATA_TO_BYTES: BuiltinDoc = BuiltinDoc {
    category: "bytes",
    signature: "dataToBytes(v[, \"-endian=B|L\"[, \"-size=N\"]]) -> bytes",
    summary: "将非负整数转为定长字节序列（默认 8 字节，对标 uint64）。",
    params: &[
        ("v", "非负 int 或 bigInt"),
        ("-endian=B|L", "可选。B=大端（默认，网络序），L=小端"),
        ("-size=N", "可选。输出字节数 1-16，默认 8"),
    ],
    returns: "bytes：定长字节序列（高位补 0）",
    examples: &[
        "dataToBytes(2047, \"-endian=B\")                    // 8 字节大端 00000000000007FF",
        "dataToBytes(2047, \"-endian=B\", \"-size=2\")        // 2 字节大端 07FF",
    ],
    errors: &[
        "负数报错；值超出 N 字节可表示范围报错",
        "size 不在 1-16 范围报错",
        "参数应为 int/bigInt，其他类型报错",
    ],
};

/// parse_endian_opt 从可选参数中解析 -endian=B|L（默认大端）。
///
/// 未能识别的其他选项忽略（与其他内置函数的开关风格一致）。
fn parse_endian_opt(args: &[Value], fn_name: &str) -> Result<bool, Value> {
    for opt in &args[1..] {
        let s = opt.to_str();
        if let Some(val) = s.strip_prefix("-endian=") {
            let c = val.chars().next().unwrap_or(' ').to_ascii_uppercase();
            if c == 'B' {
                return Ok(true);
            }
            if c == 'L' {
                return Ok(false);
            }
            return Err(crate::value::error_value(format!(
                "{}() 无法识别的字节序 '{}' (可能原因：-endian= 只接受 B(大端) 或 L(小端)，不区分大小写)",
                fn_name, val,
            )));
        }
    }
    Ok(true)
}

/// parse_size_opt 从可选参数中解析 -size=N（默认 8，范围 1-16）。
fn parse_size_opt(args: &[Value], fn_name: &str) -> Result<usize, Value> {
    for opt in &args[1..] {
        let s = opt.to_str();
        if let Some(val) = s.strip_prefix("-size=") {
            let n: i64 = val.trim().parse().map_err(|_| {
                crate::value::error_value(format!(
                    "{}() 无法解析字节数 '{}' (可能原因：-size= 后应为 1-16 的整数)",
                    fn_name, val,
                ))
            })?;
            if n < 1 || n > 16 {
                return Err(crate::value::error_value(format!(
                    "{}() 字节数 {} 超出范围 (需 1-16；可能原因：超过 16 字节请改用 bigInt 的十进制字符串处理)",
                    fn_name, n,
                )));
            }
            return Ok(n as usize);
        }
    }
    Ok(8)
}

/// u128_max_for_size 计算 N 字节无符号整数可表示的最大值（N 已保证 1-16）。
fn u128_max_for_size(size: usize) -> u128 {
    if size >= 16 {
        u128::MAX
    } else {
        (1u128 << (8 * size)) - 1
    }
}

/// u128_to_bytes 将值按字节序展开为定长字节序列（v 已保证不超出 size 字节）。
fn u128_to_bytes(v: u128, size: usize, big_endian: bool) -> Vec<u8> {
    let mut buf = vec![0u8; size];
    for (i, slot) in buf.iter_mut().enumerate() {
        // 无论字节序，先按小端填充，大端最后整体翻转
        *slot = (v >> (8 * i)) as u8;
    }
    if big_endian {
        buf.reverse();
    }
    buf
}

/// bi_bytes_to_data 将字节序列按指定字节序解读为无符号整数。
///
/// 用法：bytesToData(data[, "-endian=B|L"])
/// 长度须为 1-16 字节；结果能放入 i64 返回 int，否则返回 bigInt。
fn bi_bytes_to_data(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "bytesToData")?;
    let data: Vec<u8> = match &args[0] {
        Value::Bytes(b) => b.as_ref().to_vec(),
        Value::ByteArray(b) => b.lock().unwrap().clone(),
        v => return Err(crate::value::error_value(format!(
            "bytesToData() 不支持类型 {} (需要 bytes/byteArray；可能原因：传入了 string，可先用 bytesFromHex 转换)",
            v.type_name(),
        ))),
    };
    let big_endian = parse_endian_opt(args, "bytesToData")?;
    if data.is_empty() || data.len() > 16 {
        return Err(crate::value::error_value(format!(
            "bytesToData() 字节长度 {} 超出范围 (需 1-16 字节；可能原因：输入为空或不是定长整数编码)",
            data.len(),
        )));
    }
    let mut v: u128 = 0;
    if big_endian {
        for &b in &data {
            v = (v << 8) | b as u128;
        }
    } else {
        for (i, &b) in data.iter().enumerate() {
            v |= (b as u128) << (8 * i);
        }
    }
    if v <= i64::MAX as u128 {
        Ok(Value::Int(v as i64))
    } else {
        // 超出 i64 的值以 bigInt 返回，保证无符号值不丢失精度
        let bi = crate::bigint::BigInt::from_str_decimal(&v.to_string())
            .map_err(crate::value::error_value)?;
        Ok(Value::BigInt(Arc::new(bi)))
    }
}

/// bi_data_to_bytes 将非负整数转为定长字节序列。
///
/// 用法：dataToBytes(v[, "-endian=B|L"[, "-size=N"]])
/// 默认 8 字节（对标 uint64）；负数或超出范围返回 error。
fn bi_data_to_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "dataToBytes")?;
    let big_endian = parse_endian_opt(args, "dataToBytes")?;
    let size = parse_size_opt(args, "dataToBytes")?;
    match &args[0] {
        Value::Int(x) => {
            if *x < 0 {
                return Err(crate::value::error_value(format!(
                    "dataToBytes() 不支持负数 {} (可能原因：本函数按无符号整数编码；有符号编码请自行按字节拆分)",
                    x,
                )));
            }
            let v = *x as u128;
            let max = u128_max_for_size(size);
            if v > max {
                return Err(crate::value::error_value(format!(
                    "dataToBytes() 值 {} 超出 {} 字节可表示范围 (最大 2^{}) (可能原因：-size= 太小，可增大或省略用默认 8 字节)",
                    x, size, 8 * size,
                )));
            }
            Ok(Value::Bytes(Arc::new(u128_to_bytes(v, size, big_endian))))
        }
        Value::BigInt(b) => {
            use std::cmp::Ordering;
            if b.cmp(&crate::bigint::BigInt::zero()) == Ordering::Less {
                return Err(crate::value::error_value(
                    "dataToBytes() 不支持负数 (可能原因：本函数按无符号整数编码)".to_string(),
                ));
            }
            // 反复除以 256 取余，得到低位在前的字节序列
            let base = crate::bigint::BigInt::from_i64(256);
            let mut cur = (**b).clone();
            let mut buf: Vec<u8> = Vec::new();
            while !cur.is_zero() {
                let (q, r) = cur.divmod(&base).map_err(crate::value::error_value)?;
                buf.push(r.to_i64().unwrap_or(0) as u8);
                cur = q;
            }
            if buf.len() > size {
                return Err(crate::value::error_value(format!(
                    "dataToBytes() 值超出 {} 字节可表示范围 (至少需要 {} 字节) (可能原因：-size= 太小，可增大或省略用默认 8 字节)",
                    size, buf.len(),
                )));
            }
            while buf.len() < size {
                buf.push(0);
            }
            if big_endian {
                buf.reverse();
            }
            Ok(Value::Bytes(Arc::new(buf)))
        }
        v => Err(crate::value::error_value(format!(
            "dataToBytes() 不支持类型 {} (需要 int/bigInt；可能原因：传入了 float/string，请先取整或转整数)",
            v.type_name(),
        ))),
    }
}
