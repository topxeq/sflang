//! Value 定义了 Sflang 所有数据类型的统一表示。
//!
//! 设计要点（来自 AGENTS.md）：
//!   - 所有类型实现相同的 Object trait（typeCode/typeName/inspect/is_truthy/equals）
//!   - 基于 Rust 原生类型实现（i64/f64/bool/String/Vec/HashMap）
//!   - 类型编码固定（不使用枚举默认值，便于版本兼容）
//!   - 引用类型用 Arc 共享，可变容器用 Mutex（支持多线程 run）
//!   - 数值类型（Int/Float/Bool）直接内联在 enum 中，零堆分配（关键性能特性）
//!
//! 性能设计：Value 是 tagged enum，整数/浮点/布尔运算零装箱，
//! 这是 Sflang 性能接近 Rust 原生的根本保证（对比 Go 接口装箱）。
//!
//! 并发设计（阶段三）：所有引用类型用 Arc 共享，可变容器用 Mutex。
//! Value 实现 Send + Sync，使 run 关键字可真正多线程执行。
//! 关键安全约束：inspect/equals 等递归函数必须在持锁期间克隆出数据后
//! 立即释放锁，再递归——避免持锁访问嵌套容器导致死锁。

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

// 重导出子模块类型，方便外部使用
pub use crate::bigint::BigInt;
pub use crate::bigfloat::BigFloat;
pub use crate::datetime::DateTime;
pub use crate::ord_map::OrdMap;
pub use crate::function::{Builtin, BuiltinFn, Function};
pub use crate::object_map::Map;
pub use crate::http_lite::{SfHttpRequest, SfHttpResponse, SfWebSocket};

/// TypeCode 类型编码。
///
/// 固定数字编码（不使用从 0 自增），便于版本兼容与序列化。
/// 与 Go 版本保持一致（继承版本兼容性）。
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeCode {
    /// Undefined 空值（未定义/缺省/无返回值统一语义），类型码固定为 0。
    ///
    /// 语义对齐 Charlang 的 `undefined`：读取未定义变量、map 缺键、函数无返回值
    /// 等场景均得到此值。值为 falsy，仅与自身相等。
    Undefined = 0,
    Int = 1,
    Float = 2,
    Bool = 3,
    String = 4,
    Bytes = 5,
    Array = 6,
    Object = 7,
    Function = 8,
    Builtin = 9,
    Error = 10,
    Native = 11,
    /// ByteArray 可变字节序列（就地修改，类似 Go []byte / Python bytearray）。
    ByteArray = 12,
    /// BigInt 任意精度整数（自实现，不依赖第三方库）。超出 i64 范围时使用。
    BigInt = 13,
    /// BigFloat 任意精度十进制浮点（定点表示，精确避免 float 误差）。
    BigFloat = 14,
    /// DateTime 日期时间（纯标准库实现，毫秒+时区）。
    DateTime = 15,
    /// File 文件句柄（流式读写、随机访问，纯标准库 std::fs::File）。
    File = 16,
    /// Byte 字节（0-255），用于字节级操作（加密、协议解析等）。
    Byte = 17,
    /// Map 有序映射（插入顺序保持，纯数据容器，无原型链）。
    Map = 18,
    /// StringBuilder 高效字符串构建器（可变，大量拼接用）。
    StringBuilder = 19,
    /// HttpReq HTTP 请求对象（服务器模式下注入 requestG）。
    HttpReq = 20,
    /// HttpResp HTTP 响应对象（服务器模式下注入 responseG）。
    HttpResp = 21,
    /// WebSocket 连接对象（服务端升级或客户端连接）。
    WebSocket = 22,
}

impl TypeCode {
    /// 返回类型名字符串。
    pub fn name(self) -> &'static str {
        match self {
            TypeCode::Undefined => "undefined",
            TypeCode::Int => "int",
            TypeCode::Float => "float",
            TypeCode::Bool => "bool",
            TypeCode::String => "string",
            TypeCode::Bytes => "bytes",
            TypeCode::Array => "array",
            TypeCode::Object => "object",
            TypeCode::Function => "function",
            TypeCode::Builtin => "builtin",
            TypeCode::Error => "error",
            TypeCode::Native => "native",
            TypeCode::ByteArray => "byteArray",
            TypeCode::BigInt => "bigInt",
            TypeCode::BigFloat => "bigFloat",
            TypeCode::DateTime => "datetime",
            TypeCode::File => "file",
            TypeCode::Byte => "byte",
            TypeCode::Map => "map",
            TypeCode::StringBuilder => "stringBuilder",
            TypeCode::HttpReq => "httpReq",
            TypeCode::HttpResp => "httpResp",
            TypeCode::WebSocket => "webSocket",
        }
    }
}

/// Value Sflang 的统一值类型（tagged enum）。
///
/// 核心设计：
///   - Int/Float/Bool 直接内联，零堆分配（性能关键）
///   - Str/Bytes 用 Arc 共享不可变数据，clone 廉价
///   - Array/Object 用 Arc<Mutex<>> 实现可变共享容器（线程安全）
///   - Func/Builtin/Error 用 Arc 共享
///   - Native 用 Arc<dyn Any + Send + Sync>，约束宿主嵌入值线程安全
///
/// 性能特征：
///   - 数值运算（Add/Sub/...）：零分配，直接操作 enum 内联字段
///   - 字符串 clone：仅 Arc 指针复制，非深拷贝
///   - 数组/对象 clone：仅 Arc 指针复制
///
/// 并发安全：Value 实现 Send + Sync（阶段三）。
#[derive(Clone)]
pub enum Value {
    /// Undefined 空值，全局唯一语义（无实例区分）。
    ///
    /// 表示"没有值"：未定义的变量、map 缺失的键、无返回值的函数等。
    /// 与 Charlang 的 `undefined` 同义。参与运算（算术/比较/取成员/索引）
    /// 会抛异常（类型不兼容），仅在逻辑运算中按 falsy 参与。
    Undefined,
    /// Int 64位整数，基于 Rust i64。
    Int(i64),
    /// Float 64位浮点，基于 Rust f64。
    Float(f64),
    /// Bool 布尔值。
    Bool(bool),
    /// Str UTF-8 字符串，用 Arc<str> 共享。
    Str(Arc<str>),
    /// Bytes 字节序列，用 Arc<Vec<u8>> 共享。
    Bytes(Arc<Vec<u8>>),
    /// ByteArray 可变字节序列，用 Arc<Mutex<Vec<u8>>> 共享（就地修改）。
    ///
    /// 与 Bytes（不可变）分工：读取/接收用 bytes，需要就地修改（如按位加密、
    /// 协议改包）用 byteArray。两者转换有拷贝，保证修改互不影响。
    /// 索引读写均按字节（0-255），超出范围或非 0-255 整数报错。
    ByteArray(Arc<Mutex<Vec<u8>>>),
    /// Array 可变数组，用 Arc<Mutex<Vec<Value>>> 共享。
    Array(Arc<Mutex<Vec<Value>>>),
    /// Object 对象/映射，用 Arc<Mutex<Map>> 共享，支持原型链。
    Object(Arc<Mutex<Map>>),
    /// Func 用户自定义函数。
    Func(Arc<Function>),
    /// Builtin 内置函数。
    Builtin(Builtin),
    /// Error 错误值（可抛出/捕获）。
    Error(Arc<SfError>),
    /// Native 原生值，用于宿主嵌入任意 Rust 值（须 Send + Sync）。
    Native(Arc<dyn std::any::Any + Send + Sync>),
    /// BigInt 任意精度整数（Arc 共享，不可变；运算返回新值）。
    ///
    /// 超出 i64 范围的大整数运算。与 Int 自动互通（int + bigInt → bigInt）。
    BigInt(Arc<BigInt>),
    /// BigFloat 任意精度十进制浮点（定点表示，精确）。
    ///
    /// 值 = mantissa / 10^scale。避免 float 的 0.1+0.2 精度问题。
    BigFloat(Arc<BigFloat>),
    /// DateTime 日期时间（毫秒+时区，不可变；运算返回新值）。
    DateTime(Arc<DateTime>),
    /// File 文件句柄（流式读写、随机访问）。Arc<Mutex<File>> 共享。
    File(Arc<Mutex<std::fs::File>>),
    /// Byte 字节值（0-255），用于字节级操作。
    Byte(u8),
    /// Map 有序映射（插入顺序，纯数据容器）。
    Map(Arc<Mutex<OrdMap>>),
    /// StringBuilder 高效字符串构建器（可变，用于大量拼接）。
    ///
    /// 对标 Go strings.Builder。底层 Rust String，O(1) 追加、O(n) 构建。
    /// 通过通用的 writeStr/writeBytes/len/toStr/clear/reset 操作。
    StringBuilder(Arc<Mutex<String>>),
    /// HttpReq HTTP 请求对象（服务器模式下注入 requestG）。
    /// 用 Arc 共享，inner 用 Mutex 保护（支持跨线程 run）。
    HttpReq(Arc<SfHttpRequest>),
    /// HttpResp HTTP 响应对象（服务器模式下注入 responseG）。
    /// 用 Arc 共享，inner 用 Mutex 保护。
    HttpResp(Arc<SfHttpResponse>),
    /// WebSocket 连接对象（服务端升级或客户端连接）。
    /// 用 Arc 共享，inner 用 Mutex 保护（支持跨线程安全访问）。
    WebSocket(Arc<SfWebSocket>),
}

impl PartialEq for Value {
    /// PartialEq 按值比较（Int/Float/Bool/Str/Undefined 按值；引用类型按 Arc 指针）。
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Undefined, Value::Undefined) => true,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a.as_ref() == b.as_ref(),
            (Value::Bytes(a), Value::Bytes(b)) => a.as_ref() == b.as_ref(),
            // ByteArray 按指针比较（身份相等，与 Array/Object 一致）
            (Value::ByteArray(a), Value::ByteArray(b)) => Arc::ptr_eq(a, b),
            // 引用类型按指针比较（身份相等）
            (Value::Array(a), Value::Array(b)) => Arc::ptr_eq(a, b),
            (Value::Object(a), Value::Object(b)) => Arc::ptr_eq(a, b),
            (Value::Func(a), Value::Func(b)) => Arc::ptr_eq(a, b),
            (Value::Error(a), Value::Error(b)) => Arc::ptr_eq(a, b),
            (Value::BigInt(a), Value::BigInt(b)) => Arc::ptr_eq(a, b),
            (Value::BigFloat(a), Value::BigFloat(b)) => Arc::ptr_eq(a, b),
            (Value::DateTime(a), Value::DateTime(b)) => Arc::ptr_eq(a, b),
            (Value::File(a), Value::File(b)) => Arc::ptr_eq(a, b),
            (Value::Byte(a), Value::Byte(b)) => a == b,
            (Value::Map(a), Value::Map(b)) => Arc::ptr_eq(a, b),
            (Value::StringBuilder(a), Value::StringBuilder(b)) => Arc::ptr_eq(a, b),
            (Value::HttpReq(a), Value::HttpReq(b)) => Arc::ptr_eq(a, b),
            (Value::HttpResp(a), Value::HttpResp(b)) => Arc::ptr_eq(a, b),
            (Value::WebSocket(a), Value::WebSocket(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl std::fmt::Debug for Value {
    /// Debug 调试输出（用 inspect 格式）。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.inspect())
    }
}

impl Value {
    /// type_code 返回类型的固定数字编码。
    pub fn type_code(&self) -> TypeCode {
        match self {
            Value::Undefined => TypeCode::Undefined,
            Value::Int(_) => TypeCode::Int,
            Value::Float(_) => TypeCode::Float,
            Value::Bool(_) => TypeCode::Bool,
            Value::Str(_) => TypeCode::String,
            Value::Bytes(_) => TypeCode::Bytes,
            Value::ByteArray(_) => TypeCode::ByteArray,
            Value::Array(_) => TypeCode::Array,
            Value::Object(_) => TypeCode::Object,
            Value::Func(_) => TypeCode::Function,
            Value::Builtin(_) => TypeCode::Builtin,
            Value::Error(_) => TypeCode::Error,
            Value::Native(_) => TypeCode::Native,
            Value::BigInt(_) => TypeCode::BigInt,
            Value::BigFloat(_) => TypeCode::BigFloat,
            Value::DateTime(_) => TypeCode::DateTime,
            Value::File(_) => TypeCode::File,
            Value::Byte(_) => TypeCode::Byte,
            Value::Map(_) => TypeCode::Map,
            Value::StringBuilder(_) => TypeCode::StringBuilder,
            Value::HttpReq(_) => TypeCode::HttpReq,
            Value::HttpResp(_) => TypeCode::HttpResp,
            Value::WebSocket(_) => TypeCode::WebSocket,
        }
    }

    /// type_name 返回类型的字符串名称。
    pub fn type_name(&self) -> &'static str {
        self.type_code().name()
    }

    /// type_name_ex 返回细化的类型名称。
    ///
    /// 对于 Native 类型，尝试 downcast 识别具体的包装类型（如 ring、channel、mutex 等）。
    /// 非 Native 类型与 type_name 相同。
    pub fn type_name_ex(&self) -> String {
        match self {
            Value::Native(n) => {
                // 尝试识别各种 Native 包装类型
                if n.downcast_ref::<std::sync::Arc<std::sync::Mutex<crate::ring::Ring>>>().is_some() {
                    return "ring".to_string();
                }
                if n.downcast_ref::<std::sync::Arc<crate::opcode::Code>>().is_some() {
                    return "code".to_string();
                }
                if n.downcast_ref::<std::sync::Arc<std::sync::Mutex<crate::value::Value>>>().is_some() {
                    return "ref".to_string();
                }
                if n.downcast_ref::<std::sync::Arc<crate::concurrency::Channel>>().is_some() {
                    return "channel".to_string();
                }
                if n.downcast_ref::<std::sync::Arc<crate::concurrency::MutexT>>().is_some() {
                    return "mutex".to_string();
                }
                if n.downcast_ref::<std::sync::Arc<crate::concurrency::RWMutexT>>().is_some() {
                    return "rwmutex".to_string();
                }
                if n.downcast_ref::<std::sync::Arc<crate::concurrency::WaitGroupT>>().is_some() {
                    return "waitGroup".to_string();
                }
                if n.downcast_ref::<std::sync::Arc<crate::concurrency::SemaphoreT>>().is_some() {
                    return "semaphore".to_string();
                }
                if n.downcast_ref::<std::sync::Arc<regex::Regex>>().is_some() {
                    return "regex".to_string();
                }
                if n.downcast_ref::<std::sync::Arc<std::sync::Mutex<rust_xlsxwriter::Workbook>>>().is_some() {
                    return "workbook".to_string();
                }
                if n.downcast_ref::<crate::builtins_db::DatabaseConn>().is_some() {
                    return "database".to_string();
                }
                if n.downcast_ref::<std::sync::Arc<crate::builtins_http::SfHttpServer>>().is_some() {
                    return "httpServer".to_string();
                }
                "native".to_string()
            }
            other => other.type_code().name().to_string(),
        }
    }

    /// is_truthy 返回值的布尔真值（用于条件判断）。
    ///
    /// 语义：undefined/false/0/0.0/空字符串/空数组/空对象 为假，其余为真。
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Undefined => false,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::Bool(b) => *b,
            Value::Str(s) => !s.is_empty(),
            Value::Bytes(b) => !b.is_empty(),
            Value::ByteArray(b) => !b.lock().unwrap().is_empty(),
            Value::Array(a) => !a.lock().unwrap().is_empty(),
            Value::Object(o) => !o.lock().unwrap().is_empty(),
            Value::Func(_) | Value::Builtin(_) | Value::Error(_) | Value::Native(_) => true,
            Value::BigInt(b) => !b.is_zero(),
            Value::BigFloat(b) => !b.is_zero(),
            Value::DateTime(_) => true,
            Value::File(_) => true,
            Value::Byte(b) => *b != 0,
            Value::Map(m) => !m.lock().unwrap().is_empty(),
            Value::StringBuilder(sb) => !sb.lock().unwrap().is_empty(),
            Value::HttpReq(_) => true,
            Value::HttpResp(_) => true,
            Value::WebSocket(_) => true,
        }
    }

    /// inspect 返回值的可读字符串表示（用于打印与错误信息）。
    ///
    /// 并发安全：对数组/对象先克隆出数据快照并释放锁，再递归 inspect，
    /// 避免持锁访问嵌套容器导致死锁。
    pub fn inspect(&self) -> String {
        match self {
            Value::Undefined => "undefined".to_string(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => {
                if f.is_nan() {
                    "nan".to_string()
                } else if f.is_infinite() {
                    if *f > 0.0 { "inf".to_string() } else { "-inf".to_string() }
                } else {
                    format!("{}", f)
                }
            }
            Value::Bool(b) => b.to_string(),
            Value::Str(s) => (*s).to_string(),
            Value::Bytes(b) => format!("bytes({})", b.len()),
            Value::ByteArray(b) => {
                // 显示长度 + 前 16 字节十六进制（便于调试）
                let guard = b.lock().unwrap();
                let n = guard.len();
                let head: String = guard.iter().take(16)
                    .map(|x| format!("{:02x}", x)).collect::<Vec<_>>().join(" ");
                if n <= 16 {
                    format!("byteArray({}: {})", n, head)
                } else {
                    format!("byteArray({}: {} ...)", n, head)
                }
            }
            Value::Array(a) => {
                // 克隆快照后立即释放锁，再递归（避免持锁死锁）
                let snapshot: Vec<Value> = a.lock().unwrap().clone();
                let items: Vec<String> = snapshot.iter().map(repr_value).collect();
                format!("[{}]", items.join(", "))
            }
            Value::Object(o) => {
                // 克隆 (key, value) 快照后释放锁，再递归
                let snapshot: Vec<(String, Value)> = o.lock().unwrap().snapshot();
                if snapshot.is_empty() {
                    return "{}".to_string();
                }
                let items: Vec<String> = snapshot
                    .iter()
                    .map(|(k, v)| format!("{}: {}", quote_str(k), repr_value(v)))
                    .collect();
                format!("{{{}}}", items.join(", "))
            }
            Value::Func(f) => {
                let name = if f.name.is_empty() { "<anonymous>".to_string() } else { f.name.clone() };
                format!("<func {}({})>", name, f.params.join(", "))
            }
            Value::Builtin(b) => format!("<builtin {}>", b.name),
            Value::Error(e) => {
                if e.stack.is_empty() {
                    format!("error: {}", e.message)
                } else {
                    format!("error: {}\n{}", e.message, e.stack.join("\n"))
                }
            }
            Value::Native(n) => format!("<native {:?}>", Arc::as_ptr(n).addr()),
            Value::BigInt(b) => b.to_string_decimal(),
            Value::BigFloat(b) => b.to_string(),
            Value::DateTime(dt) => dt.inspect(),
            Value::File(_) => "<file>".to_string(),
            Value::Byte(b) => b.to_string(),
            Value::Map(m) => {
                let snapshot: Vec<(String, Value)> = m.lock().unwrap().snapshot();
                if snapshot.is_empty() {
                    return "map{}".to_string();
                }
                let items: Vec<String> = snapshot
                    .iter()
                    .map(|(k, v)| format!("{}: {}", quote_str(k), repr_value(v)))
                    .collect();
                format!("map{{{}}}", items.join(", "))
            }
            Value::StringBuilder(sb) => {
                let s = sb.lock().unwrap().clone();
                if s.is_empty() {
                    "(stringBuilder)".to_string()
                } else {
                    format!("(stringBuilder){}", s)
                }
            }
            Value::HttpReq(_) => "<httpReq>".to_string(),
            Value::HttpResp(_) => "<httpResp>".to_string(),
            Value::WebSocket(_) => "<webSocket>".to_string(),
        }
    }

    /// equals 判断值相等（值语义；引用类型按内容/引用相等）。
    ///
    /// Int/Float 跨数值类型可比较（1 == 1.0 为 true）。
    /// 引用类型（Str/Bytes/Array/Object）按内容比较。
    /// Func/Builtin/Error/Error 按引用相等（Arc 指针）。
    ///
    /// 并发安全：数组/对象比较时先克隆快照释放锁，再递归 equals。
    pub fn equals(&self, other: &Value) -> bool {
        use Value::*;
        match (self, other) {
            (Undefined, Undefined) => true,
            (Int(a), Int(b)) => a == b,
            (Float(a), Float(b)) => a == b,
            (Int(a), Float(b)) | (Float(b), Int(a)) => (*a as f64) == *b,
            (Bool(a), Bool(b)) => a == b,
            (Str(a), Str(b)) => **a == **b,
            (Bytes(a), Bytes(b)) => **a == **b,
            // ByteArray 与 ByteArray/Bytes 按内容比较（跨类型可比，便于等值判断）
            (ByteArray(a), ByteArray(b)) => {
                let sa = a.lock().unwrap().clone();
                let sb = b.lock().unwrap().clone();
                sa == sb
            }
            (Bytes(a), ByteArray(b)) | (ByteArray(b), Bytes(a)) => {
                **a == *b.lock().unwrap()
            }
            (Array(a), Array(b)) => {
                // 分别克隆快照（避免同时持有两个锁），再递归比较
                let sa = a.lock().unwrap().clone();
                let sb = b.lock().unwrap().clone();
                sa.len() == sb.len() && sa.iter().zip(sb.iter()).all(|(x, y)| x.equals(y))
            }
            (Object(a), Object(b)) => {
                let sa: Vec<(String, Value)> = a.lock().unwrap().snapshot();
                let sb: Vec<(String, Value)> = b.lock().unwrap().snapshot();
                sa.len() == sb.len()
                    && sa.iter().all(|(k, v)| {
                        sb.iter().any(|(bk, bv)| bk == k && v.equals(bv))
                    })
            }
            (Func(a), Func(b)) => Arc::ptr_eq(a, b),
            (Builtin(a), Builtin(b)) => a.name == b.name,
            // Byte：自身按值比；与 Int/Float 跨类型按值可比
            (Byte(a), Byte(b)) => a == b,
            (Byte(a), Int(b)) | (Int(b), Byte(a)) => *a as i64 == *b,
            (Byte(a), Float(b)) | (Float(b), Byte(a)) => (*a as f64) == *b,
            // Map 按内容比较
            (Map(a), Map(b)) => {
                let sa = a.lock().unwrap().snapshot();
                let sb = b.lock().unwrap().snapshot();
                sa.len() == sb.len()
                    && sa.iter().all(|(k, v)|
                        sb.iter().any(|(bk, bv)| bk == k && v.equals(bv)))
            }
            (Error(a), Error(b)) => a.message == b.message,
            // BigInt：自身按值比；与 Int 跨类型可比
            (BigInt(a), BigInt(b)) => a.cmp(b) == std::cmp::Ordering::Equal,
            (Int(a), BigInt(b)) | (BigInt(b), Int(a)) => {
                match b.to_i64() { Some(bi) => *a == bi, None => false }
            }
            // BigFloat：自身按值比；与 Int 跨类型可比
            (BigFloat(a), BigFloat(b)) => a.cmp(b) == std::cmp::Ordering::Equal,
            // DateTime 按时间点比较（UTC 毫秒相等即相等，忽略时区显示差异）
            (DateTime(a), DateTime(b)) => a.millis == b.millis,
            (Int(a), BigFloat(b)) | (BigFloat(b), Int(a)) => {
                crate::bigfloat::BigFloat::from_i64(*a).cmp(b) == std::cmp::Ordering::Equal
            }
            // HttpReq/HttpResp 按指针相等
            (HttpReq(a), HttpReq(b)) => Arc::ptr_eq(a, b),
            (HttpResp(a), HttpResp(b)) => Arc::ptr_eq(a, b),
            (WebSocket(a), WebSocket(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// is_number 判断是否为数值类型。
    pub fn is_number(&self) -> bool {
        matches!(self, Value::Int(_) | Value::Float(_) | Value::Byte(_))
    }

    /// to_f64 转为 f64（非数值返回 None）。
    pub fn to_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            Value::Byte(b) => Some(*b as f64),
            _ => None,
        }
    }

    /// to_int 转为 i64（数值类型转换，非数值返回 None）。
    pub fn to_int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) => Some(*f as i64),
            Value::Byte(b) => Some(*b as i64),
            _ => None,
        }
    }

    /// to_str 转为字符串（用于字符串拼接、打印等）。
    ///
    /// undefined → "undefined"，Str 直接返回内容，其他用 inspect。
    /// ByteArray/Bytes 不是文本，走 inspect（显示十六进制），避免隐式 UTF-8 解码失败。
    pub fn to_str(&self) -> String {
        match self {
            Value::Undefined => "undefined".to_string(),
            Value::Str(s) => (*s).to_string(),
            Value::StringBuilder(sb) => sb.lock().unwrap().clone(),
            _ => self.inspect(),
        }
    }

    /// undefined 返回 Undefined 值（便捷构造）。
    pub fn undefined() -> Value {
        Value::Undefined
    }

    /// int 从 i64 构造 Int 值。
    pub fn int(i: i64) -> Value {
        Value::Int(i)
    }

    /// byte 从 u8 构造 Byte 值。
    pub fn byte(b: u8) -> Value {
        Value::Byte(b)
    }

    /// float 从 f64 构造 Float 值。
    pub fn float(f: f64) -> Value {
        Value::Float(f)
    }

    /// boolean 从 bool 构造 Bool 值。
    pub fn boolean(b: bool) -> Value {
        Value::Bool(b)
    }

    /// str 从 &str 构造 Str 值。
    pub fn str(s: &str) -> Value {
        Value::Str(Arc::from(s))
    }

    /// str_from 从 String 构造 Str 值。
    pub fn str_from(s: String) -> Value {
        Value::Str(Arc::from(s.as_str()))
    }
}

/// repr_value 返回值的"表示"字符串：字符串带引号，其他用 inspect。
/// 用于容器（数组/对象）打印时区分字符串与标识符。
fn repr_value(v: &Value) -> String {
    match v {
        Value::Str(s) => quote_str(s),
        _ => v.inspect(),
    }
}

/// quote_str 用双引号包裹字符串并转义特殊字符。
fn quote_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// SfError 错误值，用于 try-catch。
///
/// 既作为 Value::Error 存在（可抛出/捕获），也实现 error trait（供 Rust 侧使用）。
#[derive(Debug, Clone)]
pub struct SfError {
    /// message 错误信息。
    pub message: String,
    /// stack 调用栈信息（可选，用于调试定位）。
    pub stack: Vec<String>,
}

impl SfError {
    /// new 创建一个错误值。
    pub fn new(msg: impl Into<String>) -> Self {
        SfError {
            message: msg.into(),
            stack: Vec::new(),
        }
    }

    /// with_stack 附加调用栈信息。
    pub fn with_stack(mut self, stack: Vec<String>) -> Self {
        self.stack = stack;
        self
    }
}

impl fmt::Display for SfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.stack.is_empty() {
            write!(f, "error: {}", self.message)
        } else {
            write!(f, "error: {}\n{}", self.message, self.stack.join("\n"))
        }
    }
}

impl std::error::Error for SfError {}

/// 从 Rust error 创建 Sflang Error Value。
pub fn err_to_value<E: std::error::Error>(e: E) -> Value {
    Value::Error(Arc::new(SfError::new(e.to_string())))
}

/// 从字符串创建 Sflang Error Value。
pub fn error_value(msg: impl Into<String>) -> Value {
    Value::Error(Arc::new(SfError::new(msg)))
}

// 引用类型容器模块（避免循环依赖，在 value.rs 内定义 Map）
// 注：ObjectMap 是 Map 的别名，便于将来扩展

/// ObjectMap 对象/映射的内部存储类型。
pub type ObjectMap = HashMap<String, Value>;
