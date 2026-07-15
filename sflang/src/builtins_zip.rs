//! builtins_zip.rs — 压缩与 ZIP 文件处理内置函数
//!
//! 提供两类能力：
//!
//! 1. 内部数据压缩解压（基于 flate2）：
//!    - compressBytes / decompressBytes  — raw deflate
//!    - gzipBytes / gunzipBytes          — gzip 格式
//!
//! 2. ZIP 文件处理（基于 zip crate）：
//!    - zipCreate / zipAddFile / zipAddBytes / zipAddDir / zipClose  — 创建
//!    - zipList / zipExtract / zipExtractFile / zipReadFile          — 读取
//!
//! # 中文文件名兼容
//!
//! - 写入时：直接传 UTF-8 字符串，zip crate 自动设置 EFS 标志位（bit 11）
//! - 读取时：优先按 UTF-8 解码；若非 UTF-8 则尝试 GBK 解码（兼容 Windows 旧工具）
//! - zipList 返回的文件名始终为正确解码的 UTF-8 字符串

use std::io::{Read, Write};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};

use crate::function::BuiltinDoc;
use crate::value::{Value, error_value};
use crate::vm::VM;

// ===========================================================================
// Native 包装类型：ZipWriter
// ===========================================================================

/// ZipWriterState zip 写入器内部状态。
///
/// 包装 zip::ZipWriter，用 Mutex 保护单线程访问。
/// 通过 Value::Native(Arc<ZipWriterState>) 暴露给脚本。
pub struct ZipWriterState {
    /// writer 内部 zip 写入器（基于 Cursor<Vec<u8>> 的内存写入器）。
    /// 用 Option 包装以便 zipClose 时 take 出来 finish（finish 需要 self 所有权）。
    writer: Mutex<Option<zip::ZipWriter<std::io::Cursor<Vec<u8>>>>>,
    /// path 输出文件路径。
    path: String,
    /// closed 是否已关闭（finish 后不能再写入）。
    closed: AtomicBool,
}

// ===========================================================================
// 注册内置函数
// ===========================================================================

// ---- 压缩与 ZIP 函数文档 ----

static DOC_COMPRESS_BYTES: BuiltinDoc = BuiltinDoc {
    category: "zip",
    signature: "compressBytes(data, level?) -> bytes",
    summary: "用 zlib (deflate + zlib 头) 压缩数据。",
    params: &[
        ("data", "要压缩的数据：bytes / byteArray / string"),
        ("level", "可选：压缩级别 1-9（默认 6），越高压缩率越大但越慢"),
    ],
    returns: "bytes：压缩后的数据（zlib 格式，含 2 字节头和 4 字节校验）",
    examples: &[
        "var c = compressBytes(\"hello world\")",
        "var c = compressBytes(fileReadBytes(\"./data.bin\"), 9)  // 最高压缩率",
    ],
    errors: &[
        "data 类型必须为 bytes/byteArray/string",
        "level 自动 clamp 到 1-9",
        "对应解压函数为 decompressBytes（与 gzipBytes 的 gzip 格式不同）",
    ],
};

static DOC_DECOMPRESS_BYTES: BuiltinDoc = BuiltinDoc {
    category: "zip",
    signature: "decompressBytes(data) -> bytes",
    summary: "解压 zlib (deflate + zlib 头) 压缩的数据。",
    params: &[
        ("data", "compressBytes 返回的压缩数据：bytes / byteArray"),
    ],
    returns: "bytes：解压后的原始数据；失败返回 error",
    examples: &[
        "var data = decompressBytes(compressBytes(\"hello\"))  // → b\"hello\"",
    ],
    errors: &[
        "解压失败：数据不是有效的 zlib 格式（被截断 / 头损坏）",
        "data 类型必须为 bytes/byteArray",
        "对应 gzip 格式请用 gunzipBytes",
    ],
};

static DOC_GZIP_BYTES: BuiltinDoc = BuiltinDoc {
    category: "zip",
    signature: "gzipBytes(data, level?) -> bytes",
    summary: "用 gzip 格式压缩数据（含 gzip 文件头，兼容 .gz 文件）。",
    params: &[
        ("data", "要压缩的数据：bytes / byteArray / string"),
        ("level", "可选：压缩级别 1-9（默认 6）"),
    ],
    returns: "bytes：gzip 格式压缩数据（含文件头，可写入 .gz 文件）",
    examples: &[
        "var g = gzipBytes(\"hello world\")",
        "fileWriteBytes(\"./data.gz\", gzipBytes(fileReadBytes(\"./data\")))",
    ],
    errors: &[
        "与 compressBytes 区别：gzipBytes 含 gzip 文件头（10 字节），compressBytes 是 zlib 头（2 字节）",
        "level 自动 clamp 到 1-9",
        "对应解压函数为 gunzipBytes",
    ],
};

static DOC_GUNZIP_BYTES: BuiltinDoc = BuiltinDoc {
    category: "zip",
    signature: "gunzipBytes(data) -> bytes",
    summary: "解压 gzip 格式数据（兼容 .gz 文件内容）。",
    params: &[
        ("data", "gzip 格式压缩数据：bytes / byteArray"),
    ],
    returns: "bytes：解压后的原始数据；失败返回 error",
    examples: &[
        "var data = gunzipBytes(gzipBytes(\"hello\"))  // → b\"hello\"",
        "var data = gunzipBytes(fileReadBytes(\"./data.gz\"))  // 读取 .gz 文件",
    ],
    errors: &[
        "解压失败：数据不是有效的 gzip 格式（头损坏 / 不是 gzip）",
        "data 类型必须为 bytes/byteArray",
    ],
};

static DOC_ZIP_CREATE: BuiltinDoc = BuiltinDoc {
    category: "zip",
    signature: "zipCreate(zipPath) -> zipWriter",
    summary: "创建 ZIP 文件写入器（后续用 zipAddFile/zipAddBytes/zipAddDir 添加内容，最后 zipClose 落盘）。",
    params: &[
        ("zipPath", "要生成的 ZIP 文件路径"),
    ],
    returns: "zipWriter 写入器对象（传入后续 zip* 函数）",
    examples: &[
        "var zw = zipCreate(\"./out.zip\")",
        "zipAddFile(zw, \"./a.txt\", \"a.txt\")",
        "zipAddBytes(zw, \"hello\", \"b.txt\")",
        "zipClose(zw)  // 落盘",
    ],
    errors: &[
        "zipPath 必须为 string",
        "实际写入在 zipClose 时发生；未 close 不会生成文件",
    ],
};

static DOC_ZIP_ADD_FILE: BuiltinDoc = BuiltinDoc {
    category: "zip",
    signature: "zipAddFile(zipWriter, srcPath, entryName) -> undefined",
    summary: "将本地文件添加到 ZIP（entryName 为 zip 内路径，支持中文）。",
    params: &[
        ("zipWriter", "zipCreate 返回的写入器"),
        ("srcPath", "本地源文件路径"),
        ("entryName", "zip 内的文件名（路径，支持中文 UTF-8）"),
    ],
    returns: "undefined：添加成功；失败返回 error",
    examples: &[
        "zipAddFile(zw, \"./report.pdf\", \"reports/report.pdf\")",
        "zipAddFile(zw, \"./配置.txt\", \"配置.txt\")  // 中文文件名",
    ],
    errors: &[
        "打开源文件失败：路径不存在 / 权限不足",
        "zipWriter 已关闭（zipClose 后不能再写）",
        "使用 Deflated 压缩方法",
    ],
};

static DOC_ZIP_ADD_BYTES: BuiltinDoc = BuiltinDoc {
    category: "zip",
    signature: "zipAddBytes(zipWriter, data, entryName) -> undefined",
    summary: "将内存数据（bytes/byteArray/string）添加到 ZIP。",
    params: &[
        ("zipWriter", "zipCreate 返回的写入器"),
        ("data", "要写入的数据：bytes / byteArray / string"),
        ("entryName", "zip 内的文件名（支持中文 UTF-8）"),
    ],
    returns: "undefined：添加成功；失败返回 error",
    examples: &[
        "zipAddBytes(zw, \"hello world\", \"hello.txt\")",
        "zipAddBytes(zw, fileReadBytes(\"./data.bin\"), \"data.bin\")",
    ],
    errors: &[
        "data 类型必须为 bytes/byteArray/string",
        "zipWriter 已关闭后写入返回 error",
    ],
};

static DOC_ZIP_ADD_DIR: BuiltinDoc = BuiltinDoc {
    category: "zip",
    signature: "zipAddDir(zipWriter, dirPath, basePath?) -> int",
    summary: "递归添加整个目录到 ZIP（保留相对路径结构）。",
    params: &[
        ("zipWriter", "zipCreate 返回的写入器"),
        ("dirPath", "要添加的本地目录路径"),
        ("basePath", "可选：目录在 zip 内的根路径前缀（如 \"\" 或 \"subdir/\"，默认空）"),
    ],
    returns: "int：实际添加的文件数（不含目录条目）",
    examples: &[
        "var n = zipAddDir(zw, \"./project\", \"\")           // zip 内保留 project 下的相对路径",
        "var n = zipAddDir(zw, \"C:\\\\www\", \"web/\")         // zip 内路径加 web/ 前缀",
    ],
    errors: &[
        "目录不存在返回 error",
        "zipWriter 已关闭后写入返回 error",
        "目录条目用 Stored（不压缩），文件用 Deflated",
    ],
};

static DOC_ZIP_CLOSE: BuiltinDoc = BuiltinDoc {
    category: "zip",
    signature: "zipClose(zipWriter) -> bool",
    summary: "完成 ZIP 并写入磁盘（必须调用，否则不会生成文件）。",
    params: &[
        ("zipWriter", "zipCreate 返回的写入器"),
    ],
    returns: "bool：true 写入成功；失败返回 error",
    examples: &[
        "var zw = zipCreate(\"./out.zip\")",
        "zipAddFile(zw, \"./a.txt\", \"a.txt\")",
        "zipClose(zw)  // 此时才真正写入 out.zip",
    ],
    errors: &[
        "重复 close 返回 error（已关闭）",
        "写入磁盘失败：路径不可写 / 磁盘满",
        "未调用 close 的写入器其内容会丢失（GC 时丢弃）",
    ],
};

static DOC_ZIP_LIST: BuiltinDoc = BuiltinDoc {
    category: "zip",
    signature: "zipList(zipPath) -> array<object>",
    summary: "列出 ZIP 文件中所有条目（文件名自动解码 UTF-8/GBK）。",
    params: &[
        ("zipPath", "ZIP 文件路径"),
    ],
    returns: "array<object>：每个元素 {name:string, size:int, compressedSize:int, isDir:bool}",
    examples: &[
        "var entries = zipList(\"./out.zip\")  // → [{\"name\":\"a.txt\",\"size\":100,\"compressedSize\":45,\"isDir\":false}]",
        "for (var e in zipList(\"./out.zip\")) { println(e[\"name\"], e[\"size\"]) }",
    ],
    errors: &[
        "打开文件失败：路径不存在 / 权限不足",
        "读取 zip 失败：不是有效的 zip 文件",
        "中文文件名优先 UTF-8 解码，失败回退 GBK（兼容 Windows 旧工具）",
    ],
};

static DOC_ZIP_EXTRACT: BuiltinDoc = BuiltinDoc {
    category: "zip",
    signature: "zipExtract(zipPath, destDir) -> int",
    summary: "解压整个 ZIP 到指定目录（自动创建目录，含 zip slip 安全防护）。",
    params: &[
        ("zipPath", "ZIP 文件路径"),
        ("destDir", "目标目录（不存在则自动创建）"),
    ],
    returns: "int：解压出的文件数（不含目录条目）",
    examples: &[
        "var n = zipExtract(\"./out.zip\", \"./unzipped\")  // → 3",
    ],
    errors: &[
        "zip slip 安全检查：条目含 .. 或绝对路径时拒绝解压（防路径穿越攻击）",
        "打开 / 读取 zip 失败；创建目录 / 文件失败",
        "中文文件名自动 UTF-8/GBK 解码",
    ],
};

static DOC_ZIP_EXTRACT_FILE: BuiltinDoc = BuiltinDoc {
    category: "zip",
    signature: "zipExtractFile(zipPath, entryName, destPath) -> bool",
    summary: "解压 ZIP 中的单个文件到指定路径。",
    params: &[
        ("zipPath", "ZIP 文件路径"),
        ("entryName", "zip 内的条目名（支持中文，需精确匹配）"),
        ("destPath", "本地目标文件路径"),
    ],
    returns: "bool：true 解压成功；未找到条目或失败返回 error",
    examples: &[
        "zipExtractFile(\"./out.zip\", \"reports/report.pdf\", \"./restored.pdf\")  // → true",
    ],
    errors: &[
        "未找到条目：文件名不匹配或编码不一致（中文需 UTF-8）",
        "打开 / 创建文件失败",
        "与 zipExtract 区别：本函数只解压指定单个条目",
    ],
};

static DOC_ZIP_READ_FILE: BuiltinDoc = BuiltinDoc {
    category: "zip",
    signature: "zipReadFile(zipPath, entryName) -> bytes",
    summary: "读取 ZIP 中单个文件的内容到内存 bytes（不解压到磁盘）。",
    params: &[
        ("zipPath", "ZIP 文件路径"),
        ("entryName", "zip 内的条目名（支持中文）"),
    ],
    returns: "bytes：条目解压后的原始内容；未找到或失败返回 error",
    examples: &[
        "var data = zipReadFile(\"./out.zip\", \"a.txt\")",
        "println(toStr(data))  // 文本文件可转字符串查看",
    ],
    errors: &[
        "未找到条目：文件名不匹配或编码不一致",
        "打开 zip / 读取条目失败",
        "适合不需要落盘的小文件读取",
    ],
};

/// register 注册所有压缩与 ZIP 相关内置函数。
pub fn register(vm: &mut VM) {
    // 数据压缩
    vm.register_builtin_doc("compressBytes", bi_compress_bytes, &DOC_COMPRESS_BYTES);
    vm.register_builtin_doc("decompressBytes", bi_decompress_bytes, &DOC_DECOMPRESS_BYTES);
    vm.register_builtin_doc("gzipBytes", bi_gzip_bytes, &DOC_GZIP_BYTES);
    vm.register_builtin_doc("gunzipBytes", bi_gunzip_bytes, &DOC_GUNZIP_BYTES);

    // ZIP 文件处理
    vm.register_builtin_doc("zipCreate", bi_zip_create, &DOC_ZIP_CREATE);
    vm.register_builtin_doc("zipAddFile", bi_zip_add_file, &DOC_ZIP_ADD_FILE);
    vm.register_builtin_doc("zipAddBytes", bi_zip_add_bytes, &DOC_ZIP_ADD_BYTES);
    vm.register_builtin_doc("zipAddDir", bi_zip_add_dir, &DOC_ZIP_ADD_DIR);
    vm.register_builtin_doc("zipClose", bi_zip_close, &DOC_ZIP_CLOSE);
    vm.register_builtin_doc("zipList", bi_zip_list, &DOC_ZIP_LIST);
    vm.register_builtin_doc("zipExtract", bi_zip_extract, &DOC_ZIP_EXTRACT);
    vm.register_builtin_doc("zipExtractFile", bi_zip_extract_file, &DOC_ZIP_EXTRACT_FILE);
    vm.register_builtin_doc("zipReadFile", bi_zip_read_file, &DOC_ZIP_READ_FILE);
}

// ===========================================================================
// 辅助函数
// ===========================================================================

/// extract_zip_writer 从 Value 中提取 ZipWriterState 引用。
fn extract_zip_writer<'a>(v: &'a Value) -> Result<&'a Arc<ZipWriterState>, Value> {
    match v {
        Value::Native(n) => n.downcast_ref::<Arc<ZipWriterState>>().ok_or_else(|| {
            error_value(format!(
                "参数应为 zipWriter 对象 (可能原因：传入了其他 native 类型 '{}')",
                v.type_name_ex()
            ))
        }),
        _ => Err(error_value(format!(
            "参数应为 zipWriter 对象，得到 {} (可能原因：参数顺序错误)",
            v.type_name()
        ))),
    }
}

/// decode_zip_name 解码 zip 文件名（兼容 UTF-8 和 GBK）。
///
/// zip crate 读取时：若 EFS 标志位为 1，按 UTF-8 解码；
/// 否则按 CP437 解码（不含中文）。
/// 此函数从原始字节出发，优先 UTF-8，失败则尝试 GBK。
fn decode_zip_name(raw: &[u8]) -> String {
    // 优先按 UTF-8 解码
    if let Ok(s) = std::str::from_utf8(raw) {
        return s.to_string();
    }
    // 非 UTF-8，尝试 GBK 解码（兼容 Windows 旧工具生成的 zip）
    let (decoded, _, had_errors) = encoding_rs::GBK.decode(raw);
    if had_errors {
        // GBK 也失败，用 lossy UTF-8 兜底
        String::from_utf8_lossy(raw).into_owned()
    } else {
        decoded.into_owned()
    }
}

// ===========================================================================
// 数据压缩内置函数
// ===========================================================================

/// bi_compress_bytes 使用 deflate 算法压缩数据。
///
/// 用法：`compressBytes(data, level)`
/// data: bytes/byteArray/string
/// level: 1-9（默认 6），越高压缩率越大但越慢
/// 返回: bytes（raw deflate 格式，不含 gzip 头）
fn bi_compress_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let data: Vec<u8> = match args.get(0) {
        Some(Value::Bytes(b)) => b.to_vec(),
        Some(Value::ByteArray(b)) => b.lock().unwrap().clone(),
        Some(Value::Str(s)) => s.as_bytes().to_vec(),
        Some(v) => return Err(error_value(format!(
            "compressBytes() 第 1 个参数应为 bytes/byteArray/string，得到 {}",
            v.type_name()
        ))),
        None => return Err(error_value("compressBytes() 需要至少 1 个参数 (data)")),
    };

    let level: u32 = match args.get(1) {
        Some(Value::Int(n)) => *n as u32,
        _ => 6,
    };
    let level = level.clamp(1, 9);

    use flate2::write::ZlibEncoder;
    use flate2::Compression;

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(level));
    encoder.write_all(&data).map_err(|e| error_value(format!(
        "compressBytes() 压缩失败: {}", e
    )))?;
    let compressed = encoder.finish().map_err(|e| error_value(format!(
        "compressBytes() 完成压缩失败: {}", e
    )))?;

    Ok(Value::Bytes(Arc::new(compressed)))
}

/// bi_decompress_bytes 解压 deflate 压缩的数据。
///
/// 用法：`decompressBytes(data)`
/// data: bytes/byteArray
/// 返回: bytes
fn bi_decompress_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let data: Vec<u8> = match args.get(0) {
        Some(Value::Bytes(b)) => b.to_vec(),
        Some(Value::ByteArray(b)) => b.lock().unwrap().clone(),
        Some(v) => return Err(error_value(format!(
            "decompressBytes() 第 1 个参数应为 bytes/byteArray，得到 {}",
            v.type_name()
        ))),
        None => return Err(error_value("decompressBytes() 需要至少 1 个参数 (data)")),
    };

    use flate2::read::ZlibDecoder;

    let mut decoder = ZlibDecoder::new(&data[..]);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed).map_err(|e| error_value(format!(
        "decompressBytes() 解压失败: {} (可能原因：数据不是有效的 deflate 格式)",
        e
    )))?;

    Ok(Value::Bytes(Arc::new(decompressed)))
}

/// bi_gzip_bytes 使用 gzip 格式压缩数据。
///
/// 用法：`gzipBytes(data, level)`
/// data: bytes/byteArray/string
/// level: 1-9（默认 6）
/// 返回: bytes（gzip 格式，含文件头）
fn bi_gzip_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let data: Vec<u8> = match args.get(0) {
        Some(Value::Bytes(b)) => b.to_vec(),
        Some(Value::ByteArray(b)) => b.lock().unwrap().clone(),
        Some(Value::Str(s)) => s.as_bytes().to_vec(),
        Some(v) => return Err(error_value(format!(
            "gzipBytes() 第 1 个参数应为 bytes/byteArray/string，得到 {}",
            v.type_name()
        ))),
        None => return Err(error_value("gzipBytes() 需要至少 1 个参数 (data)")),
    };

    let level: u32 = match args.get(1) {
        Some(Value::Int(n)) => *n as u32,
        _ => 6,
    };
    let level = level.clamp(1, 9);

    use flate2::write::GzEncoder;
    use flate2::Compression;

    let mut encoder = GzEncoder::new(Vec::new(), Compression::new(level));
    encoder.write_all(&data).map_err(|e| error_value(format!(
        "gzipBytes() 压缩失败: {}", e
    )))?;
    let compressed = encoder.finish().map_err(|e| error_value(format!(
        "gzipBytes() 完成压缩失败: {}", e
    )))?;

    Ok(Value::Bytes(Arc::new(compressed)))
}

/// bi_gunzip_bytes 解压 gzip 格式数据。
///
/// 用法：`gunzipBytes(data)`
/// data: bytes/byteArray
/// 返回: bytes
fn bi_gunzip_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let data: Vec<u8> = match args.get(0) {
        Some(Value::Bytes(b)) => b.to_vec(),
        Some(Value::ByteArray(b)) => b.lock().unwrap().clone(),
        Some(v) => return Err(error_value(format!(
            "gunzipBytes() 第 1 个参数应为 bytes/byteArray，得到 {}",
            v.type_name()
        ))),
        None => return Err(error_value("gunzipBytes() 需要至少 1 个参数 (data)")),
    };

    use flate2::read::GzDecoder;

    let mut decoder = GzDecoder::new(&data[..]);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed).map_err(|e| error_value(format!(
        "gunzipBytes() 解压失败: {} (可能原因：数据不是有效的 gzip 格式)",
        e
    )))?;

    Ok(Value::Bytes(Arc::new(decompressed)))
}

// ===========================================================================
// ZIP 文件处理 — 创建
// ===========================================================================

/// bi_zip_create 创建 ZIP 文件写入器。
///
/// 用法：`zipCreate(zipPath)` → zipWriter 对象
/// 后续用 zipAddFile / zipAddBytes / zipAddDir 添加内容
/// 最后用 zipClose 完成
fn bi_zip_create(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let path = match args.get(0) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "zipCreate() 第 1 个参数应为 string (zip 文件路径)，得到 {}",
            v.type_name()
        ))),
        None => return Err(error_value("zipCreate() 需要至少 1 个参数 (zipPath)")),
    };

    let state = Arc::new(ZipWriterState {
        writer: Mutex::new(Some(zip::ZipWriter::new(std::io::Cursor::new(Vec::new())))),
        path,
        closed: AtomicBool::new(false),
    });

    Ok(Value::Native(Arc::new(state)))
}

/// bi_zip_add_file 添加文件到 ZIP。
///
/// 用法：`zipAddFile(zipWriter, srcPath, entryName)`
/// entryName 是 zip 内的路径名（支持中文）
/// 返回: undefined 或 error
fn bi_zip_add_file(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let state = extract_zip_writer(&args[0])?.clone();
    let src_path = match args.get(1) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "zipAddFile() 第 2 个参数应为 string (源文件路径)，得到 {}",
            v.type_name()
        ))),
        None => return Err(error_value("zipAddFile() 需要至少 3 个参数")),
    };
    let entry_name = match args.get(2) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "zipAddFile() 第 3 个参数应为 string (zip 内文件名)，得到 {}",
            v.type_name()
        ))),
        None => return Err(error_value("zipAddFile() 需要至少 3 个参数")),
    };

    if state.closed.load(Ordering::SeqCst) {
        return Err(error_value("zipAddFile() zipWriter 已关闭，不能再写入"));
    }

    let file = std::fs::File::open(&src_path).map_err(|e| error_value(format!(
        "zipAddFile() 打开文件失败: {} (文件: {})", e, src_path
    )))?;
    let mut reader = std::io::BufReader::new(file);

    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let mut writer = state.writer.lock().unwrap();
    let writer = writer.as_mut().unwrap();
    writer.start_file(&entry_name, options).map_err(|e| error_value(format!(
        "zipAddFile() 创建 zip 条目失败: {}", e
    )))?;
    std::io::copy(&mut reader, &mut *writer).map_err(|e| error_value(format!(
        "zipAddFile() 写入文件失败: {}", e
    )))?;

    Ok(Value::Undefined)
}

/// bi_zip_add_bytes 添加内存数据到 ZIP。
///
/// 用法：`zipAddBytes(zipWriter, data, entryName)`
/// data: bytes/byteArray/string
/// 返回: undefined 或 error
fn bi_zip_add_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let state = extract_zip_writer(&args[0])?.clone();
    let data: Vec<u8> = match args.get(1) {
        Some(Value::Bytes(b)) => b.to_vec(),
        Some(Value::ByteArray(b)) => b.lock().unwrap().clone(),
        Some(Value::Str(s)) => s.as_bytes().to_vec(),
        Some(v) => return Err(error_value(format!(
            "zipAddBytes() 第 2 个参数应为 bytes/byteArray/string，得到 {}",
            v.type_name()
        ))),
        None => return Err(error_value("zipAddBytes() 需要至少 3 个参数")),
    };
    let entry_name = match args.get(2) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "zipAddBytes() 第 3 个参数应为 string (zip 内文件名)，得到 {}",
            v.type_name()
        ))),
        None => return Err(error_value("zipAddBytes() 需要至少 3 个参数")),
    };

    if state.closed.load(Ordering::SeqCst) {
        return Err(error_value("zipAddBytes() zipWriter 已关闭，不能再写入"));
    }

    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let mut writer = state.writer.lock().unwrap();
    let writer = writer.as_mut().unwrap();
    writer.start_file(&entry_name, options).map_err(|e| error_value(format!(
        "zipAddBytes() 创建 zip 条目失败: {}", e
    )))?;
    writer.write_all(&data).map_err(|e| error_value(format!(
        "zipAddBytes() 写入数据失败: {}", e
    )))?;

    Ok(Value::Undefined)
}

/// bi_zip_add_dir 递归添加目录到 ZIP。
///
/// 用法：`zipAddDir(zipWriter, dirPath, basePath)`
/// basePath 为目录在 zip 内的根路径（如 "" 或 "subdir/"）
/// 返回: 添加的文件数量 (int)
fn bi_zip_add_dir(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let state = extract_zip_writer(&args[0])?.clone();
    let dir_path = match args.get(1) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "zipAddDir() 第 2 个参数应为 string (目录路径)，得到 {}",
            v.type_name()
        ))),
        None => return Err(error_value("zipAddDir() 需要至少 2 个参数")),
    };
    let base_path = match args.get(2) {
        Some(Value::Str(s)) => s.to_string(),
        _ => String::new(),
    };

    if state.closed.load(Ordering::SeqCst) {
        return Err(error_value("zipAddDir() zipWriter 已关闭，不能再写入"));
    }

    let dir = std::path::Path::new(&dir_path);
    if !dir.exists() {
        return Err(error_value(format!(
            "zipAddDir() 目录不存在: {}", dir_path
        )));
    }

    let mut count: i64 = 0;
    let entries = collect_dir_entries(dir);
    for entry in &entries {
        let rel = entry.strip_prefix(dir).unwrap_or(entry);
        // 构建 zip 内路径：base_path + 相对路径（用 / 分隔）
        let mut zip_name = base_path.clone();
        if !zip_name.is_empty() && !zip_name.ends_with('/') {
            zip_name.push('/');
        }
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        zip_name.push_str(&rel_str);

        if entry.is_dir() {
            // 目录条目以 / 结尾
            let dir_name = if zip_name.ends_with('/') {
                zip_name.clone()
            } else {
                format!("{}/", zip_name)
            };
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            let mut writer = state.writer.lock().unwrap();
            let writer = writer.as_mut().unwrap();
            writer.add_directory(&dir_name, options).map_err(|e| error_value(format!(
                "zipAddDir() 添加目录条目失败: {}", e
            )))?;
        } else {
            let file = std::fs::File::open(entry).map_err(|e| error_value(format!(
                "zipAddDir() 打开文件失败: {} (文件: {})", e, entry.display()
            )))?;
            let mut reader = std::io::BufReader::new(file);

            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            let mut writer = state.writer.lock().unwrap();
            let writer = writer.as_mut().unwrap();
            writer.start_file(&zip_name, options).map_err(|e| error_value(format!(
                "zipAddDir() 创建 zip 条目失败: {}", e
            )))?;
            std::io::copy(&mut reader, &mut *writer).map_err(|e| error_value(format!(
                "zipAddDir() 写入文件失败: {}", e
            )))?;
            count += 1;
        }
    }

    Ok(Value::Int(count))
}

/// collect_dir_entries 递归收集目录下的所有条目。
fn collect_dir_entries(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                result.push(path.clone());
                result.extend(collect_dir_entries(&path));
            } else {
                result.push(path);
            }
        }
    }
    result
}

/// bi_zip_close 完成 ZIP 文件并写入磁盘。
///
/// 用法：`zipClose(zipWriter)` → bool
fn bi_zip_close(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let state = extract_zip_writer(&args[0])?.clone();

    if state.closed.load(Ordering::SeqCst) {
        return Err(error_value("zipClose() zipWriter 已关闭"));
    }

    // 取出 writer（take 出 Option），finish 需要 self 所有权
    let writer = {
        let mut guard = state.writer.lock().unwrap();
        guard.take()
    };
    let writer = writer.ok_or_else(|| error_value("zipClose() zipWriter 已被取出"))?;
    let cursor = writer.finish().map_err(|e| error_value(format!(
        "zipClose() 完成 zip 失败: {}", e
    )))?;
    let data = cursor.into_inner();

    std::fs::write(&state.path, &data).map_err(|e| error_value(format!(
        "zipClose() 写入文件失败: {} (文件: {})", e, state.path
    )))?;

    state.closed.store(true, Ordering::SeqCst);

    Ok(Value::Bool(true))
}

// ===========================================================================
// ZIP 文件处理 — 读取
// ===========================================================================

/// bi_zip_list 列出 ZIP 文件中的所有条目。
///
/// 用法：`zipList(zipPath)` → array of {name, size, compressedSize, isDir}
/// 文件名自动解码（兼容 UTF-8 和 GBK）
fn bi_zip_list(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let zip_path = match args.get(0) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "zipList() 第 1 个参数应为 string (zip 文件路径)，得到 {}",
            v.type_name()
        ))),
        None => return Err(error_value("zipList() 需要至少 1 个参数")),
    };

    let file = std::fs::File::open(&zip_path).map_err(|e| error_value(format!(
        "zipList() 打开文件失败: {} (文件: {})", e, zip_path
    )))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| error_value(format!(
        "zipList() 读取 zip 失败: {} (可能原因：不是有效的 zip 文件)", e
    )))?;

    let mut result = Vec::new();
    for i in 0..archive.len() {
        let file = archive.by_index(i).map_err(|e| error_value(format!(
            "zipList() 读取条目 {} 失败: {}", i, e
        )))?;

        // 用原始字节解码，兼容 GBK
        let name = decode_zip_name(file.name_raw());
        let is_dir = file.is_dir();
        let size = file.size() as i64;
        let compressed = file.compressed_size() as i64;

        let obj = crate::object_map::new_map();
        {
            let mut m = obj.lock().unwrap();
            m.set("name".to_string(), Value::str(&name));
            m.set("size".to_string(), Value::Int(size));
            m.set("compressedSize".to_string(), Value::Int(compressed));
            m.set("isDir".to_string(), Value::Bool(is_dir));
        }
        result.push(Value::Object(obj));
    }

    Ok(Value::Array(Arc::new(Mutex::new(result))))
}

/// bi_zip_extract 解压整个 ZIP 到指定目录。
///
/// 用法：`zipExtract(zipPath, destDir)` → int (解压文件数)
/// 自动创建 destDir。中文文件名自动解码。
fn bi_zip_extract(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let zip_path = match args.get(0) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "zipExtract() 第 1 个参数应为 string，得到 {}", v.type_name()
        ))),
        None => return Err(error_value("zipExtract() 需要至少 2 个参数")),
    };
    let dest_dir = match args.get(1) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "zipExtract() 第 2 个参数应为 string，得到 {}", v.type_name()
        ))),
        None => return Err(error_value("zipExtract() 需要至少 2 个参数")),
    };

    let file = std::fs::File::open(&zip_path).map_err(|e| error_value(format!(
        "zipExtract() 打开文件失败: {} (文件: {})", e, zip_path
    )))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| error_value(format!(
        "zipExtract() 读取 zip 失败: {}", e
    )))?;

    let dest = std::path::PathBuf::from(&dest_dir);
    std::fs::create_dir_all(&dest).map_err(|e| error_value(format!(
        "zipExtract() 创建目录失败: {}", e
    )))?;

    let mut count: i64 = 0;
    for i in 0..archive.len() {
        let mut zip_file = archive.by_index(i).map_err(|e| error_value(format!(
            "zipExtract() 读取条目 {} 失败: {}", i, e
        )))?;

        // 用原始字节解码文件名
        let name = decode_zip_name(zip_file.name_raw());

        // 安全检查：防止 zip slip（路径穿越攻击）
        // 检查条目名不含 .. 且不是绝对路径
        let path_check = std::path::Path::new(&name);
        if path_check.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err(error_value(format!(
                "zipExtract() 安全检查失败: 条目 '{}' 包含 .. 路径穿越", name
            )));
        }
        // Windows 绝对路径（如 C:\）也禁止
        if name.contains(':') && (name.starts_with('/') || name.starts_with('\\') || name.len() > 1 && name.as_bytes()[1] == b':') {
            return Err(error_value(format!(
                "zipExtract() 安全检查失败: 条目 '{}' 是绝对路径", name
            )));
        }

        let out_path = dest.join(&name);

        if zip_file.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| error_value(format!(
                "zipExtract() 创建目录失败: {} (路径: {})", e, out_path.display()
            )))?;
        } else {
            // 确保父目录存在
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| error_value(format!(
                    "zipExtract() 创建父目录失败: {}", e
                )))?;
            }
            let mut out_file = std::fs::File::create(&out_path).map_err(|e| error_value(format!(
                "zipExtract() 创建文件失败: {} (路径: {})", e, out_path.display()
            )))?;
            std::io::copy(&mut zip_file, &mut out_file).map_err(|e| error_value(format!(
                "zipExtract() 写入文件失败: {}", e
            )))?;
            count += 1;
        }
    }

    Ok(Value::Int(count))
}

/// bi_zip_extract_file 解压 ZIP 中的单个文件。
///
/// 用法：`zipExtractFile(zipPath, entryName, destPath)` → bool
/// entryName 支持中文
fn bi_zip_extract_file(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let zip_path = match args.get(0) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "zipExtractFile() 第 1 个参数应为 string，得到 {}", v.type_name()
        ))),
        None => return Err(error_value("zipExtractFile() 需要至少 3 个参数")),
    };
    let entry_name = match args.get(1) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "zipExtractFile() 第 2 个参数应为 string，得到 {}", v.type_name()
        ))),
        None => return Err(error_value("zipExtractFile() 需要至少 3 个参数")),
    };
    let dest_path = match args.get(2) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "zipExtractFile() 第 3 个参数应为 string，得到 {}", v.type_name()
        ))),
        None => return Err(error_value("zipExtractFile() 需要至少 3 个参数")),
    };

    let file = std::fs::File::open(&zip_path).map_err(|e| error_value(format!(
        "zipExtractFile() 打开文件失败: {}", e
    )))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| error_value(format!(
        "zipExtractFile() 读取 zip 失败: {}", e
    )))?;

    // 先查找匹配的条目索引
    let mut found_idx: Option<usize> = None;
    for i in 0..archive.len() {
        let zip_file = archive.by_index(i).map_err(|e| error_value(format!(
            "zipExtractFile() 读取条目失败: {}", e
        )))?;
        let name = decode_zip_name(zip_file.name_raw());
        if name == entry_name {
            found_idx = Some(i);
            break;  // zip_file 在此处 drop，释放 archive 的借用
        }
    }

    match found_idx {
        Some(idx) => {
            let mut zip_file = archive.by_index(idx).map_err(|e| error_value(format!(
                "zipExtractFile() 重新打开条目失败: {}", e
            )))?;
            let mut out_file = std::fs::File::create(&dest_path).map_err(|e| error_value(format!(
                "zipExtractFile() 创建文件失败: {}", e
            )))?;
            std::io::copy(&mut zip_file, &mut out_file).map_err(|e| error_value(format!(
                "zipExtractFile() 写入文件失败: {}", e
            )))?;
            Ok(Value::Bool(true))
        }
        None => Ok(error_value(format!(
            "zipExtractFile() 未找到条目 '{}' (可能原因：文件名不匹配或编码不一致)",
            entry_name
        ))),
    }
}

/// bi_zip_read_file 读取 ZIP 中的文件内容到 bytes。
///
/// 用法：`zipReadFile(zipPath, entryName)` → bytes
fn bi_zip_read_file(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let zip_path = match args.get(0) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "zipReadFile() 第 1 个参数应为 string，得到 {}", v.type_name()
        ))),
        None => return Err(error_value("zipReadFile() 需要至少 2 个参数")),
    };
    let entry_name = match args.get(1) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "zipReadFile() 第 2 个参数应为 string，得到 {}", v.type_name()
        ))),
        None => return Err(error_value("zipReadFile() 需要至少 2 个参数")),
    };

    let file = std::fs::File::open(&zip_path).map_err(|e| error_value(format!(
        "zipReadFile() 打开文件失败: {}", e
    )))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| error_value(format!(
        "zipReadFile() 读取 zip 失败: {}", e
    )))?;

    // 先查找匹配的条目索引
    let mut found_idx: Option<usize> = None;
    for i in 0..archive.len() {
        let zip_file = archive.by_index(i).map_err(|e| error_value(format!(
            "zipReadFile() 读取条目失败: {}", e
        )))?;
        let name = decode_zip_name(zip_file.name_raw());
        if name == entry_name {
            found_idx = Some(i);
            break;  // zip_file 在此处 drop，释放 archive 的借用
        }
    }

    match found_idx {
        Some(idx) => {
            let mut zip_file = archive.by_index(idx).map_err(|e| error_value(format!(
                "zipReadFile() 重新打开条目失败: {}", e
            )))?;
            let mut data = Vec::new();
            zip_file.read_to_end(&mut data).map_err(|e| error_value(format!(
                "zipReadFile() 读取内容失败: {}", e
            )))?;
            Ok(Value::Bytes(Arc::new(data)))
        }
        None => Ok(error_value(format!(
            "zipReadFile() 未找到条目 '{}' (可能原因：文件名不匹配或编码不一致)",
            entry_name
        ))),
    }
}
