//! http_lite.rs — 纯标准库 HTTP/1.1 服务器
//!
//! 基于 std::net::TcpListener 实现的轻量 HTTP/1.1 解析器与响应写入器。
//! 不依赖任何第三方库，满足 AGENTS.md "尽量只用 std" 的原则。
//!
//! 能力范围（第一阶段）：
//!   - 请求行 + headers 解析（\r\n 分隔）
//!   - Content-Length / Transfer-Encoding: chunked body 读取
//!   - 响应写入：状态行 + headers + body
//!   - keep-alive 连接复用
//!
//! 不实现（后续增强）：
//!   - HTTP/2、压缩、管道化
//!
//! 注：TLS/HTTPS 与 WebSocket 已在上层 builtins_http.rs 中通过 rustls / tungstenite 实现，
//! 本模块仅负责纯文本 HTTP/1.1 解析与响应写入（供 HTTP 服务器与上层 TLS 封装复用）。

use std::io::{self, BufRead, BufReader};
use std::net::{TcpListener, TcpStream};

// ---------------------------------------------------------------------------
// HttpRequest
// ---------------------------------------------------------------------------

/// HttpRequest 解析后的 HTTP 请求。
#[derive(Debug, Clone)]
pub struct HttpRequest {
    /// method 请求方法（GET/POST/PUT/DELETE 等）。
    pub method: String,
    /// uri 完整 URI（含查询串）。
    pub uri: String,
    /// path 路径部分（不含查询串）。
    pub path: String,
    /// query 查询串（不含 ?）。
    pub query: String,
    /// version HTTP 版本（如 "HTTP/1.1"）。
    pub version: String,
    /// headers 请求头（名称小写化，保留原始值）。
    pub headers: Vec<(String, String)>,
    /// body 请求体。
    pub body: Vec<u8>,
    /// remote_addr 远端地址字符串。
    pub remote_addr: String,
}

impl HttpRequest {
    /// new 构造空请求。
    pub fn new() -> Self {
        HttpRequest {
            method: String::new(),
            uri: String::new(),
            path: String::new(),
            query: String::new(),
            version: String::new(),
            headers: Vec::new(),
            body: Vec::new(),
            remote_addr: String::new(),
        }
    }

    /// get_header 获取指定 header 的值（大小写不敏感）。
    ///
    /// header 名称已小写化存储，查找时也用小写。
    pub fn get_header(&self, name: &str) -> Option<&str> {
        let name_lower = name.to_lowercase();
        for (k, v) in &self.headers {
            if k == &name_lower {
                return Some(v.as_str());
            }
        }
        None
    }

    /// parse_query 解析查询串为键值对列表。
    ///
    /// 支持百分号编码解码（%XX）。同一 key 出现多次时保留全部。
    pub fn parse_query(&self) -> Vec<(String, String)> {
        if self.query.is_empty() {
            return Vec::new();
        }
        url_decode_pairs(&self.query)
    }
}

// ---------------------------------------------------------------------------
// HttpResponse
// ---------------------------------------------------------------------------

/// HttpResponse 待发送的 HTTP 响应。
#[derive(Debug, Clone)]
pub struct HttpResponse {
    /// status 状态码（如 200、404）。
    pub status: u16,
    /// headers 响应头列表。
    pub headers: Vec<(String, String)>,
    /// body 响应体。
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// new 构造默认响应（200，空体）。
    pub fn new() -> Self {
        HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    /// set_header 设置响应头（追加，不替换同名）。
    pub fn set_header(&mut self, key: String, value: String) {
        self.headers.push((key, value));
    }

    /// get_header 获取响应头（首次匹配）。
    pub fn get_header(&self, name: &str) -> Option<&str> {
        for (k, v) in &self.headers {
            if k.eq_ignore_ascii_case(name) {
                return Some(v.as_str());
            }
        }
        None
    }

    /// write_body 追加数据到响应体。
    pub fn write_body(&mut self, data: &[u8]) {
        self.body.extend_from_slice(data);
    }

    /// content_type 返回 Content-Type 头的值（若已设）。
    pub fn content_type(&self) -> Option<&str> {
        self.get_header("Content-Type")
    }
}

// ---------------------------------------------------------------------------
// HTTP 状态码描述
// ---------------------------------------------------------------------------

/// status_text 返回状态码的标准描述文本。
pub fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Unknown",
    }
}

// ---------------------------------------------------------------------------
// 请求解析
// ---------------------------------------------------------------------------

/// HttpError HTTP 解析/处理错误。
#[derive(Debug)]
pub enum HttpError {
    /// Io IO 错误。
    Io(io::Error),
    /// Parse 解析错误。
    Parse(String),
    /// TooLarge 请求体过大。
    TooLarge,
}

impl From<io::Error> for HttpError {
    fn from(e: io::Error) -> Self {
        HttpError::Io(e)
    }
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::Io(e) => write!(f, "IO error: {}", e),
            HttpError::Parse(s) => write!(f, "parse error: {}", s),
            HttpError::TooLarge => write!(f, "request body too large"),
        }
    }
}

/// 默认最大请求体大小（10 MB）。
const MAX_BODY_SIZE: usize = 10 * 1024 * 1024;

/// parse_request 从 BufReader 解析一个 HTTP 请求。
///
/// 读取请求行、headers、body（按 Content-Length 或 chunked）。
/// remote_addr 需要调用方在外部设置。
pub fn parse_request<R: BufRead>(reader: &mut R) -> Result<HttpRequest, HttpError> {
    // 1. 读取请求行
    let mut request_line = String::new();
    let n = reader.read_line(&mut request_line)?;
    if n == 0 {
        return Err(HttpError::Parse("connection closed".to_string()));
    }
    let request_line = request_line.trim_end_matches("\r\n").trim_end_matches('\n');

    let parts: Vec<&str> = request_line.splitn(3, ' ').collect();
    if parts.len() < 3 {
        return Err(HttpError::Parse(format!("malformed request line: {}", request_line)));
    }
    let method = parts[0].to_uppercase();
    let uri = parts[1].to_string();
    let version = parts[2].to_string();

    // 分离 path 与 query
    let (path, query) = match uri.find('?') {
        Some(pos) => (uri[..pos].to_string(), uri[pos + 1..].to_string()),
        None => (uri.clone(), String::new()),
    };

    // 2. 读取 headers
    let mut headers: Vec<(String, String)> = Vec::new();
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Err(HttpError::Parse("unexpected EOF in headers".to_string()));
        }
        let line = line.trim_end_matches("\r\n").trim_end_matches('\n');
        if line.is_empty() {
            break; // 空行 = headers 结束
        }
        if let Some(colon_pos) = line.find(':') {
            let key = line[..colon_pos].trim().to_lowercase();
            let value = line[colon_pos + 1..].trim().to_string();
            headers.push((key, value));
        }
    }

    // 3. 读取 body
    let body = read_body(reader, &headers)?;

    Ok(HttpRequest {
        method,
        uri,
        path,
        query,
        version,
        headers,
        body,
        remote_addr: String::new(),
    })
}

/// read_body 按 Content-Length 或 chunked 编码读取请求体。
fn read_body<R: BufRead>(reader: &mut R, headers: &[(String, String)]) -> Result<Vec<u8>, HttpError> {
    // 检查 Transfer-Encoding: chunked
    let is_chunked = headers.iter().any(|(k, v)| {
        k == "transfer-encoding" && v.to_lowercase().contains("chunked")
    });

    if is_chunked {
        return read_chunked_body(reader);
    }

    // 按 Content-Length 读取
    let content_length: Option<usize> = headers.iter().find_map(|(k, v)| {
        if k == "content-length" {
            v.parse().ok()
        } else {
            None
        }
    });

    match content_length {
        Some(len) if len > MAX_BODY_SIZE => Err(HttpError::TooLarge),
        Some(len) if len > 0 => {
            let mut body = vec![0u8; len];
            reader.read_exact(&mut body)?;
            Ok(body)
        }
        _ => Ok(Vec::new()),
    }
}

/// read_chunked_body 读取 chunked 编码的请求体。
fn read_chunked_body<R: BufRead>(reader: &mut R) -> Result<Vec<u8>, HttpError> {
    let mut body = Vec::new();
    loop {
        let mut size_line = String::new();
        let n = reader.read_line(&mut size_line)?;
        if n == 0 {
            return Err(HttpError::Parse("unexpected EOF in chunked body".to_string()));
        }
        let size_str = size_line.trim_end_matches("\r\n").trim_end_matches('\n').trim();
        let chunk_size = usize::from_str_radix(size_str, 16)
            .map_err(|_| HttpError::Parse(format!("invalid chunk size: {}", size_str)))?;

        if chunk_size == 0 {
            // 读取尾部 \r\n（trailer 结束标记）
            let mut trailer = String::new();
            let _ = reader.read_line(&mut trailer);
            break;
        }

        if body.len() + chunk_size > MAX_BODY_SIZE {
            return Err(HttpError::TooLarge);
        }

        let mut chunk = vec![0u8; chunk_size];
        reader.read_exact(&mut chunk)?;
        body.extend_from_slice(&chunk);

        // 读取 chunk 后的 \r\n
        let mut crlf = String::new();
        let _ = reader.read_line(&mut crlf);
    }
    Ok(body)
}

// ---------------------------------------------------------------------------
// 响应写入
// ---------------------------------------------------------------------------

/// write_response 将 HttpResponse 写入到输出流。
///
/// 自动补全 Date、Content-Length、Connection 头。
/// 根据状态码决定是否写入 body（204/304 无 body）。
pub fn write_response<W: std::io::Write>(stream: &mut W, resp: &HttpResponse) -> io::Result<()> {
    let status_text = status_text(resp.status);

    // 状态行
    let status_line = format!("HTTP/1.1 {} {}\r\n", resp.status, status_text);
    stream.write_all(status_line.as_bytes())?;

    // Date 头（必需，RFC 7231）
    let date = http_date_now();
    stream.write_all(format!("Date: {}\r\n", date).as_bytes())?;

    // Connection: keep-alive（默认）
    let has_connection = resp.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("connection"));
    if !has_connection {
        stream.write_all(b"Connection: keep-alive\r\n")?;
    }

    // Content-Length（若未手动设）
    let has_content_length = resp.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-length"));
    let has_transfer_encoding = resp.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("transfer-encoding"));
    if !has_content_length && !has_transfer_encoding {
        stream.write_all(format!("Content-Length: {}\r\n", resp.body.len()).as_bytes())?;
    }

    // Content-Type（若未手动设且有 body）
    let has_content_type = resp.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
    if !has_content_type && !resp.body.is_empty() {
        // 探测：如果 body 是有效 UTF-8，用 text/plain；否则 application/octet-stream
        let ct = if std::str::from_utf8(&resp.body).is_ok() {
            "text/plain; charset=utf-8"
        } else {
            "application/octet-stream"
        };
        stream.write_all(format!("Content-Type: {}\r\n", ct).as_bytes())?;
    }

    // 用户自定义 headers
    for (key, value) in &resp.headers {
        stream.write_all(format!("{}: {}\r\n", key, value).as_bytes())?;
    }

    // 空行分隔 headers 与 body
    stream.write_all(b"\r\n")?;

    // Body（204/304 不应有 body）
    if resp.status != 204 && resp.status != 304 && !resp.body.is_empty() {
        stream.write_all(&resp.body)?;
    }

    stream.flush()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 服务器主循环
// ---------------------------------------------------------------------------

/// HttpHandler 请求处理函数签名。
///
/// 接收 HttpRequest，返回 HttpResponse。
pub trait HttpHandler: Send + Sync {
    /// handle 处理一个 HTTP 请求。
    fn handle(&self, req: HttpRequest) -> HttpResponse;
}

/// serve_forever 启动 HTTP 服务器主循环。
///
/// 在指定地址监听 TCP 连接，每个连接在新线程中处理。
/// 支持连接复用（keep-alive），在同一线程中循环读取请求。
///
/// # 参数
/// - `listener`: 已绑定的 TcpListener
/// - `handler`: 请求处理器
/// - `verbose`: 是否打印请求日志
pub fn serve_forever(listener: TcpListener, handler: &dyn HttpHandler, verbose: bool) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                // 注意：不能直接 move handler，因为它是 trait object 引用
                // 此函数在调用方线程中同步执行，handler 在此线程中使用
                handle_connection(stream, handler, verbose);
            }
            Err(e) => {
                eprintln!("accept error: {}", e);
            }
        }
    }
}

/// handle_connection 处理一个 TCP 连接（支持 keep-alive）。
///
/// 在当前线程中循环读取请求直到对端关闭或出错。
fn handle_connection(stream: TcpStream, handler: &dyn HttpHandler, verbose: bool) {
    let peer = stream.peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_default();

    // 尝试 clone stream（用于 reader/writer 分离）
    let writer_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            if verbose {
                eprintln!("clone stream failed from {}: {}", peer, e);
            }
            return;
        }
    };
    let mut reader = BufReader::new(stream);
    let mut writer = writer_stream;

    // keep-alive 最大请求数
    let mut requests_left = 100;

    loop {
        if requests_left == 0 {
            break;
        }
        requests_left -= 1;

        // 设置读取超时（避免 keep-alive 连接永远挂起）
        let _ = writer.set_read_timeout(Some(std::time::Duration::from_secs(30)));

        let mut req = match parse_request(&mut reader) {
            Ok(r) => r,
            Err(HttpError::Io(ref e)) if e.kind() == io::ErrorKind::TimedOut => {
                // 读取超时，正常关闭 keep-alive
                break;
            }
            Err(HttpError::Io(ref e)) if e.kind() == io::ErrorKind::ConnectionReset => {
                break;
            }
            Err(HttpError::Parse(msg)) => {
                if msg == "connection closed" {
                    break;
                }
                if verbose {
                    eprintln!("parse error from {}: {}", peer, msg);
                }
                // 返回 400
                let mut resp = HttpResponse::new();
                resp.status = 400;
                resp.set_header("Content-Type".to_string(), "text/plain; charset=utf-8".to_string());
                resp.write_body(format!("Bad Request: {}", msg).as_bytes());
                let _ = write_response(&mut writer, &resp);
                break;
            }
            Err(e) => {
                if verbose {
                    eprintln!("error from {}: {}", peer, e);
                }
                break;
            }
        };

        req.remote_addr = peer.clone();

        if verbose {
            eprintln!("{} {} {}", req.method, req.uri, req.remote_addr);
        }

        let resp = handler.handle(req);

        // 检查 Connection: close
        let should_close = resp.headers.iter().any(|(k, v)| {
            k.eq_ignore_ascii_case("connection") && v.eq_ignore_ascii_case("close")
        });

        let _ = write_response(&mut writer, &resp);

        if should_close {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// URL 解码工具
// ---------------------------------------------------------------------------

/// url_decode_pairs 将 URL 编码的查询串解析为键值对列表。
///
/// 格式：key1=value1&key2=value2，支持百分号编码。
pub fn url_decode_pairs(query: &str) -> Vec<(String, String)> {
    let mut result = Vec::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = match pair.find('=') {
            Some(pos) => (&pair[..pos], &pair[pos + 1..]),
            None => (pair, ""),
        };
        result.push((url_decode_component(key), url_decode_component(value)));
    }
    result
}

/// url_decode_component 解码单个 URL 编码组件。
///
/// 将 %XX 转为对应字节，+ 转为空格。
pub fn url_decode_component(s: &str) -> String {
    let mut result = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                result.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = hex_val(bytes[i + 1]);
                let lo = hex_val(bytes[i + 2]);
                if let (Some(h), Some(l)) = (hi, lo) {
                    result.push(h << 4 | l);
                    i += 3;
                } else {
                    result.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                result.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&result).into_owned()
}

/// hex_val 将一个十六进制字符转为数值。
fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// MIME 类型
// ---------------------------------------------------------------------------

/// guess_mime_type 根据文件扩展名猜测 MIME 类型。
///
/// 用于静态文件服务。
pub fn guess_mime_type(path: &str) -> &'static str {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "xml" => "text/xml; charset=utf-8",
        "txt" | "log" => "text/plain; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "md" => "text/markdown; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "bmp" => "image/bmp",
        "webp" => "image/webp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "eot" => "application/vnd.ms-fontobject",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" | "tgz" => "application/gzip",
        "tar" => "application/x-tar",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "wav" => "audio/wav",
        "avi" => "video/x-msvideo",
        "webm" => "video/webm",
        "ogg" => "audio/ogg",
        "wasm" => "application/wasm",
        "map" => "application/json",
        _ => "application/octet-stream",
    }
}

// ---------------------------------------------------------------------------
// HTTP 日期格式
// ---------------------------------------------------------------------------

/// http_date_now 返回当前时间的 HTTP 日期字符串（RFC 1123 格式）。
///
/// 示例："Fri, 10 Jul 2026 12:34:56 GMT"
fn http_date_now() -> String {
    // 简单实现：用 SystemTime 获取 Unix 时间戳，手动转换
    // 避免引入 chrono 第三方库
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    http_date_from_timestamp(secs)
}

/// http_date_from_timestamp 将 Unix 时间戳转为 HTTP 日期字符串。
fn http_date_from_timestamp(secs: u64) -> String {
    // 简化计算：从 1970-01-01 00:00:00 UTC 开始
    const DAY_SECS: u64 = 86400;

    let mut days = secs / DAY_SECS;
    let time_secs = (secs % DAY_SECS) as u32;
    let hour = time_secs / 3600;
    let minute = (time_secs % 3600) / 60;
    let second = time_secs % 60;

    // 年月日计算（简化版 Gregorian calendar）
    let mut year = 1970;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let month_days: [u32; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1;
    let mut day = days as u32 + 1;
    for (i, &md) in month_days.iter().enumerate() {
        if day <= md {
            month = (i + 1) as u32;
            break;
        }
        day -= md;
    }

    // 星期几：Zeller 公式简化版
    let weekday = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let w = weekday[days_since_sunday(secs) as usize % 7].to_string();

    let month_names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun",
                       "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let month_name = month_names[(month - 1) as usize];

    format!("{}, {:02} {} {} {:02}:{:02}:{:02} GMT", w, day, month_name, year, hour, minute, second)
}

/// is_leap_year 判断闰年。
fn is_leap_year(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// days_since_sunday 计算从 1970-01-01（周四）起经过的天数对应的星期偏移。
fn days_since_sunday(secs: u64) -> u64 {
    // 1970-01-01 是周四，偏移 4
    (secs / 86400 + 4) % 7
}

// ---------------------------------------------------------------------------
// 静态文件扩展名白名单
// ---------------------------------------------------------------------------

/// WEB_EXTS 静态文件可服务的扩展名白名单。
///
/// 不在白名单内的文件扩展名不会被直接服务（安全考虑）。
pub const WEB_EXTS: &[&str] = &[
    "html", "htm", "css", "js", "mjs", "json", "xml",
    "txt", "log", "csv", "md", "svg",
    "png", "jpg", "jpeg", "gif", "ico", "bmp", "webp",
    "woff", "woff2", "ttf", "otf", "eot",
    "pdf", "wasm", "map",
    "mp3", "mp4", "wav", "ogg", "webm", "avi",
];

/// is_web_ext 判断文件扩展名是否在白名单中。
pub fn is_web_ext(path: &str) -> bool {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    WEB_EXTS.contains(&ext.as_str())
}

// ---------------------------------------------------------------------------
// Sflang 层的请求/响应包装类型
// ---------------------------------------------------------------------------

/// SfHttpRequest Sflang 脚本层的 HTTP 请求对象。
///
/// 包装 http_lite::HttpRequest，作为 Value::HttpReq 的载荷。
/// 脚本通过 getReqMethod/getReqPath/getReqHeader 等内置函数访问。
pub struct SfHttpRequest {
    /// inner 原始 HTTP 请求。
    pub inner: std::sync::Mutex<HttpRequest>,
}

impl SfHttpRequest {
    /// new 从已解析的 HttpRequest 构造。
    pub fn new(req: HttpRequest) -> Self {
        SfHttpRequest {
            inner: std::sync::Mutex::new(req),
        }
    }
}

/// SfHttpResponse Sflang 脚本层的 HTTP 响应对象。
///
/// 包装 http_lite::HttpResponse，作为 Value::HttpResp 的载荷。
/// 脚本通过 writeResp/setRespHeader/setRespStatus 等内置函数操作。
pub struct SfHttpResponse {
    /// inner 原始 HTTP 响应。
    pub inner: std::sync::Mutex<HttpResponse>,
}

impl SfHttpResponse {
    /// new 构造默认响应（200，空体）。
    pub fn new() -> Self {
        SfHttpResponse {
            inner: std::sync::Mutex::new(HttpResponse::new()),
        }
    }
}

/// SfWebSocket Sflang 脚本层的 WebSocket 连接对象。
///
/// 包装 tungstenite WebSocket 连接，作为 Value::WebSocket 的载荷。
/// 支持服务端（从 HTTP 请求升级）和客户端（连接 URL）两种模式。
/// 脚本通过 wsReadMsg/wsWriteMsg/wsClose 等内置函数操作。
pub struct SfWebSocket {
    /// inner WebSocket 连接（基于 TcpStream）。
    /// 用 Mutex 保护，支持跨线程安全访问。
    pub inner: std::sync::Mutex<tungstenite::WebSocket<std::net::TcpStream>>,
}

impl SfWebSocket {
    /// new 从已建立的 tungstenite WebSocket 连接构造。
    pub fn new(ws: tungstenite::WebSocket<std::net::TcpStream>) -> Self {
        SfWebSocket {
            inner: std::sync::Mutex::new(ws),
        }
    }
}

/// parse_ws_url 解析 WebSocket URL。
///
/// 返回 (host, port, path)。仅支持 ws://（非 TLS）。
pub fn parse_ws_url(url: &str) -> Result<(String, String, String), String> {
    let rest = url.strip_prefix("ws://")
        .ok_or_else(|| format!("仅支持 ws:// 协议（当前: {}），wss:// 需后续 TLS 客户端支持", url))?;
    
    let (host_port, path) = match rest.find('/') {
        Some(pos) => (&rest[..pos], &rest[pos..]),
        None => (rest, "/"),
    };
    
    let (host, port) = match host_port.find(':') {
        Some(pos) => (host_port[..pos].to_string(), host_port[pos+1..].to_string()),
        None => (host_port.to_string(), "80".to_string()),
    };
    
    Ok((host, port, path.to_string()))
}

// ---------------------------------------------------------------------------
// HTTP 客户端
// ---------------------------------------------------------------------------

/// ParsedUrl 解析后的 URL。
#[derive(Debug, Clone)]
pub struct ParsedUrl {
    /// scheme 协议（"http" 或 "https"）。
    pub scheme: String,
    /// host 主机名。
    pub host: String,
    /// port 端口（字符串形式）。
    pub port: String,
    /// path 路径（含查询串，默认 "/"）。
    pub path: String,
}

impl ParsedUrl {
    /// port_num 返回端口号。
    pub fn port_num(&self) -> u16 {
        self.port.parse().unwrap_or(if self.scheme == "https" { 443 } else { 80 })
    }

    /// is_tls 是否为 HTTPS。
    pub fn is_tls(&self) -> bool {
        self.scheme == "https"
    }
}

/// parse_http_url 解析 HTTP/HTTPS URL。
///
/// 支持 `http://host:port/path?query` 和 `https://host/path` 格式。
/// 省略端口时 http 默认 80，https 默认 443。
pub fn parse_http_url(url: &str) -> Result<ParsedUrl, String> {
    let (scheme, rest) = if let Some(r) = url.strip_prefix("http://") {
        ("http", r)
    } else if let Some(r) = url.strip_prefix("https://") {
        ("https", r)
    } else {
        return Err(format!("URL 必须以 http:// 或 https:// 开头（当前: {}）", url));
    };

    let (host_port, path) = match rest.find('/') {
        Some(pos) => (&rest[..pos], &rest[pos..]),
        None => (rest, "/"),
    };

    let (host, port) = match host_port.find(':') {
        Some(pos) => (
            host_port[..pos].to_string(),
            host_port[pos + 1..].to_string(),
        ),
        None => (
            host_port.to_string(),
            if scheme == "https" { "443".to_string() } else { "80".to_string() },
        ),
    };

    Ok(ParsedUrl {
        scheme: scheme.to_string(),
        host,
        port,
        path: path.to_string(),
    })
}

/// ClientResponse HTTP 客户端响应。
#[derive(Debug, Clone)]
pub struct ClientResponse {
    /// status 状态码。
    pub status: u16,
    /// headers 响应头。
    pub headers: Vec<(String, String)>,
    /// body 响应体。
    pub body: Vec<u8>,
}

impl ClientResponse {
    /// get_header 获取响应头（大小写不敏感）。
    pub fn get_header(&self, name: &str) -> Option<&str> {
        for (k, v) in &self.headers {
            if k.eq_ignore_ascii_case(name) {
                return Some(v.as_str());
            }
        }
        None
    }
}

/// http_get 发送 HTTP GET 请求。
///
/// 支持 HTTP 和 HTTPS（HTTPS 使用 rustls）。
/// 自动跟随重定向（最多 10 次）。
///
/// # 参数
/// - `url`: 完整 URL
/// - `headers`: 自定义请求头列表（可选，`["Content-Type: text/plain", ...]`）
/// - `timeout_secs`: 超时秒数（0 = 不超时）
pub fn http_get(url: &str, headers: &[String], timeout_secs: u64) -> Result<ClientResponse, String> {
    http_request("GET", url, &[], "", headers, timeout_secs, 0, None)
}

/// http_post 发送 HTTP POST 请求。
///
/// # 参数
/// - `url`: 完整 URL
/// - `body`: 请求体
/// - `content_type`: Content-Type
/// - `headers`: 额外请求头
/// - `timeout_secs`: 超时秒数
pub fn http_post(url: &str, body: &[u8], content_type: &str, headers: &[String], timeout_secs: u64) -> Result<ClientResponse, String> {
    http_request("POST", url, body, content_type, headers, timeout_secs, 0, None)
}

/// http_request 发送 HTTP 请求（支持任意方法，用于 S3 等需要 PUT/DELETE/HEAD 的场景）。
///
/// 底层使用 ureq crate 实现，自动处理 TLS、重定向、chunked 编码、连接池复用。
/// `redirect_count` 参数保留以保持 API 兼容，实际由 ureq 内部处理重定向（最多 10 次）。
/// `proxy` 参数可选，格式如 "http://host:port"、"socks5://host:port"，None 表示不使用代理。
pub fn http_request(
    method: &str,
    url: &str,
    body: &[u8],
    content_type: &str,
    headers: &[String],
    timeout_secs: u64,
    _redirect_count: u32,
    proxy: Option<&str>,
) -> Result<ClientResponse, String> {
    do_http_request(method, url, body, content_type, headers, timeout_secs, proxy)
}

/// http_request_with_retry 发送 HTTP 请求，支持失败重试。
///
/// 仅在网络错误、超时、5xx、429 上重试；4xx（除 429）和 2xx/3xx 不重试。
/// 重试采用指数退避：200ms、400ms、800ms... 上限 5s。
/// `retries` 为 0 表示不重试（等价于 http_request）。
///
/// 统计：每次底层调用计入 total_requests/success_count/failure_count；
/// 每次实际重试计入 retry_attempts。
pub fn http_request_with_retry(
    method: &str,
    url: &str,
    body: &[u8],
    content_type: &str,
    headers: &[String],
    timeout_secs: u64,
    proxy: Option<&str>,
    retries: u32,
) -> Result<ClientResponse, String> {
    let mut last_err: Option<String> = None;
    for attempt in 0..=retries {
        if attempt > 0 {
            HTTP_STATS.retry_attempts.fetch_add(1, Ordering::Relaxed);
            // 指数退避：200ms * 2^(attempt-1)，上限 5s
            let backoff_ms = std::cmp::min(200u64 * (1u64 << (attempt - 1).min(5)), 5000);
            std::thread::sleep(std::time::Duration::from_millis(backoff_ms));
        }
        match do_http_request(method, url, body, content_type, headers, timeout_secs, proxy) {
            Ok(resp) => {
                // 5xx 和 429 可重试
                let retryable_status = resp.status == 429 || resp.status >= 500;
                if retryable_status && attempt < retries {
                    last_err = Some(format!("HTTP {} 状态码 {} (可重试)", method, resp.status));
                    continue;
                }
                return Ok(resp);
            }
            Err(e) => {
                last_err = Some(e);
                // 网络错误/超时：继续重试
                continue;
            }
        }
    }
    Err(last_err.unwrap_or_else(|| "重试次数已耗尽".to_string()))
}

/// do_http_request 实际执行一次 HTTP 请求（统计计入全局计数器）。
fn do_http_request(
    method: &str,
    url: &str,
    body: &[u8],
    content_type: &str,
    headers: &[String],
    timeout_secs: u64,
    proxy: Option<&str>,
) -> Result<ClientResponse, String> {
    HTTP_STATS.total_requests.fetch_add(1, Ordering::Relaxed);

    let agent = match get_agent(timeout_secs, proxy) {
        Ok(a) => a,
        Err(e) => {
            HTTP_STATS.failure_count.fetch_add(1, Ordering::Relaxed);
            return Err(e);
        }
    };
    let method_upper = method.to_ascii_uppercase();
    use std::io::Read;

    // ureq 3.x 用 typestate 区分 RequestBuilder<WithoutBody>（GET/HEAD/DELETE）和 RequestBuilder<WithBody>（POST/PUT/PATCH）
    // 两条路径分别处理，header 设置逻辑用泛型函数 apply_req_headers 复用
    let resp_result = match method_upper.as_str() {
        "GET" | "HEAD" | "DELETE" => {
            let req = match method_upper.as_str() {
                "GET" => agent.get(url),
                "HEAD" => agent.head(url),
                "DELETE" => agent.delete(url),
                _ => unreachable!(),
            };
            let req = apply_req_headers(req, content_type, headers);
            req.call()
        }
        "POST" | "PUT" | "PATCH" => {
            let req = match method_upper.as_str() {
                "POST" => agent.post(url),
                "PUT" => agent.put(url),
                "PATCH" => agent.patch(url),
                _ => unreachable!(),
            };
            let req = apply_req_headers(req, content_type, headers);
            // WithBody 类型用 send 发送 body（空 body 也用 send 保持类型一致）
            req.send(body)
        }
        other => {
            HTTP_STATS.failure_count.fetch_add(1, Ordering::Relaxed);
            return Err(format!(
                "不支持的 HTTP 方法: {} (可能原因：ureq Agent 仅提供 GET/POST/PUT/DELETE/HEAD/PATCH 便捷方法)",
                other
            ));
        }
    };

    let mut resp = match resp_result {
        Ok(r) => r,
        Err(e) => {
            HTTP_STATS.failure_count.fetch_add(1, Ordering::Relaxed);
            return Err(format!(
                "HTTP {} 请求失败: {} (可能原因：DNS 解析失败、网络不通、TLS 证书验证失败、服务器拒绝连接)",
                method, e
            ));
        }
    };

    // 提取响应
    let status = resp.status().as_u16();
    let mut headers_out: Vec<(String, String)> = Vec::new();
    for (k, v) in resp.headers().iter() {
        if let Ok(vs) = v.to_str() {
            headers_out.push((k.as_str().to_string(), vs.to_string()));
        }
    }

    // 读取响应体
    let mut body_buf = Vec::new();
    if let Err(e) = resp.body_mut().as_reader().read_to_end(&mut body_buf) {
        HTTP_STATS.failure_count.fetch_add(1, Ordering::Relaxed);
        return Err(format!("读取响应体失败: {}", e));
    }

    HTTP_STATS.success_count.fetch_add(1, Ordering::Relaxed);
    Ok(ClientResponse { status, headers: headers_out, body: body_buf })
}

// ---------------------------------------------------------------------------
// HTTP 统计
// ---------------------------------------------------------------------------

/// HttpStats HTTP 客户端统计快照。
#[derive(Debug, Clone, Default)]
pub struct HttpStats {
    /// total_requests 总请求数（含失败与重试中的每次底层调用）。
    pub total_requests: u64,
    /// success_count 成功响应数（HTTP 事务完成，不论状态码）。
    pub success_count: u64,
    /// failure_count 失败数（网络错误、TLS 失败、读取响应体失败等）。
    pub failure_count: u64,
    /// retry_attempts 实际发生的重试次数（不含首次请求）。
    pub retry_attempts: u64,
    /// agent_pool_size 当前 Agent 池大小（按 (timeout, proxy) 缓存的 Agent 数）。
    pub agent_pool_size: usize,
}

/// HTTP_STATS 全局 HTTP 客户端统计计数器。
static HTTP_STATS: HttpStatsCounters = HttpStatsCounters {
    total_requests: AtomicU64::new(0),
    success_count: AtomicU64::new(0),
    failure_count: AtomicU64::new(0),
    retry_attempts: AtomicU64::new(0),
};

struct HttpStatsCounters {
    total_requests: AtomicU64,
    success_count: AtomicU64,
    failure_count: AtomicU64,
    retry_attempts: AtomicU64,
}

use std::sync::atomic::{AtomicU64, Ordering};

/// get_http_stats 获取当前 HTTP 客户端统计快照（含 Agent 池大小）。
pub fn get_http_stats() -> HttpStats {
    let agent_pool_size = AGENTS.get().map(|m| m.lock().map(|g| g.len()).unwrap_or(0)).unwrap_or(0);
    HttpStats {
        total_requests: HTTP_STATS.total_requests.load(Ordering::Relaxed),
        success_count: HTTP_STATS.success_count.load(Ordering::Relaxed),
        failure_count: HTTP_STATS.failure_count.load(Ordering::Relaxed),
        retry_attempts: HTTP_STATS.retry_attempts.load(Ordering::Relaxed),
        agent_pool_size,
    }
}

/// reset_http_stats 重置 HTTP 客户端统计计数器（不影响 Agent 池）。
pub fn reset_http_stats() {
    HTTP_STATS.total_requests.store(0, Ordering::Relaxed);
    HTTP_STATS.success_count.store(0, Ordering::Relaxed);
    HTTP_STATS.failure_count.store(0, Ordering::Relaxed);
    HTTP_STATS.retry_attempts.store(0, Ordering::Relaxed);
}

/// apply_req_headers 在 RequestBuilder 上批量设置 Content-Type 和用户自定义 headers。
/// 泛型参数 B 兼容 ureq 的 typestate（WithoutBody / WithBody）。
fn apply_req_headers<B>(
    mut req: ureq::RequestBuilder<B>,
    content_type: &str,
    headers: &[String],
) -> ureq::RequestBuilder<B> {
    if !content_type.is_empty() {
        req = req.header("Content-Type", content_type);
    }
    // 强制 Accept-Encoding: identity，避免服务器返回压缩响应
    // （sflang 未启用 ureq 的 gzip/brotli 解压 feature，压缩响应会导致 getWeb 返回乱码）
    // 这也确保 S3 SigV4 签名场景下响应体不被压缩，XML 解析正常
    req = req.header("Accept-Encoding", "identity");
    // 用户自定义 headers，格式为 "Key: Value"
    for h in headers {
        if let Some((k, v)) = h.split_once(':') {
            req = req.header(k.trim(), v.trim());
        }
    }
    req
}

/// AGENTS 全局 Agent 池，按 (timeout_secs, proxy) 缓存复用 Agent。
/// 同一 (timeout, proxy) 的请求共享 Agent，从而共享连接池（keep-alive 复用）。
/// 不同 timeout 或 proxy 的请求使用不同 Agent，各自独立连接池。
static AGENTS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<(u64, String), ureq::Agent>>> =
    std::sync::OnceLock::new();

/// get_agent 获取（或创建）指定 (timeout, proxy) 的 ureq::Agent。
///
/// Agent 配置：
/// - http_status_as_error = false：4xx/5xx 不当作 Err，保持 sflang 习惯（返回状态码由调用方判断）
/// - max_redirects = 10：自动跟随重定向，最多 10 次
/// - user_agent = "Sflang/0.1"
/// - TLS 根证书 = PlatformVerifier：用 OS 系统证书校验
///   (Windows SChannel / Linux ca-certificates / macOS Keychain)
/// - timeout_global：按 timeout_secs 配置（0 = 不超时）
/// - proxy：可选代理，格式如 "http://host:port"、"socks5://host:port"
fn get_agent(timeout_secs: u64, proxy: Option<&str>) -> Result<ureq::Agent, String> {
    use std::time::Duration;
    use ureq::tls::{RootCerts, TlsConfig};

    let proxy_key = proxy.unwrap_or("").to_string();
    let cache_key = (timeout_secs, proxy_key);

    let agents = AGENTS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut guard = match agents.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };

    if let Some(a) = guard.get(&cache_key) {
        return Ok(a.clone());
    }

    let mut builder = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(10)
        .user_agent("Sflang/0.1")
        // 禁用 Accept-Encoding 头，避免 S3 SigV4 签名场景下服务器返回压缩响应
        // （未启用 gzip/brotli feature 时 ureq 不会自动解压，压缩响应会导致 S3 XML 解析失败）
        .accept_encoding(ureq::config::AutoHeaderValue::None)
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        );

    if timeout_secs > 0 {
        builder = builder.timeout_global(Some(Duration::from_secs(timeout_secs)));
    }

    // 配置代理（ureq 原生支持 http/https/socks5 协议）
    if let Some(p) = proxy {
        let proxy = ureq::Proxy::new(p).map_err(|e| {
            format!("代理配置失败: {} (可能原因：URL 格式错误，应为 http://host:port 或 socks5://host:port)", e)
        })?;
        builder = builder.proxy(Some(proxy));
    }

    let agent: ureq::Agent = builder.build().into();
    guard.insert(cache_key, agent.clone());
    Ok(agent)
}
