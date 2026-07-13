//! builtins_s3.rs — S3 兼容对象存储客户端内置函数
//!
//! 设计要点（AGENTS.md：尽量只用标准库）：
//!   - 零新增第三方依赖，复用 crate::hash（sha256/hmac_sha256）和 crate::http_lite（HTTP+TLS）
//!   - 自实现 AWS SigV4 签名（AWS S3 / MinIO / R2 / OSS / COS 兼容）
//!   - S3Client 用 Value::Native(Arc<S3Client>) 包装，与 TcpConn/DatabaseConn 风格一致
//!   - 错误返回 Value::Error，消息 AI 友好（含函数名、HTTP 状态、可能原因）
//!   - 上传/下载支持 string/bytes/byteArray 三种类型
//!
//! 函数列表（前缀 s3）：
//!   s3Connect(endpoint, region, ak, sk)                  — 创建客户端
//!   s3Close(client)                                      — 释放（无实际资源，仅规范）
//!   s3ListBuckets(client)                                — 列出所有 bucket
//!   s3CreateBucket(client, bucket)                       — 创建 bucket
//!   s3DeleteBucket(client, bucket)                       — 删除空 bucket
//!   s3BucketExists(client, bucket)                       — 检查 bucket 是否存在
//!   s3ListObjects(client, bucket, prefix?, maxKeys?)     — 列出对象
//!   s3PutObject(client, bucket, key, body)               — 写入对象（string/bytes/byteArray）
//!   s3GetObject(client, bucket, key)                      — 读取对象为 string（UTF-8）
//!   s3GetObjectBytes(client, bucket, key)                 — 读取对象为 bytes（二进制安全）
//!   s3UploadFile(client, bucket, key, localPath)         — 上传本地文件
//!   s3DownloadFile(client, bucket, key, localPath)       — 下载到本地文件
//!   s3DeleteObject(client, bucket, key)                   — 删除单个对象
//!   s3DeleteObjects(client, bucket, keys)                — 批量删除
//!   s3ObjectExists(client, bucket, key)                  — 检查对象是否存在
//!   s3ObjectSize(client, bucket, key)                     — 获取对象大小
//!   s3CopyObject(client, srcBucket, srcKey, dstBucket, dstKey) — 服务端拷贝
//!
//! Multipart Upload（大文件分片上传，单次 PUT 上限 5GB，Multipart 可达 5TB）：
//!   s3MultipartCreate(client, bucket, key, contentType?)          — 启动，返回 uploadId
//!   s3MultipartUploadPart(client, bucket, key, uploadId, partNo, data) — 上传分片，返回 etag
//!   s3MultipartComplete(client, bucket, key, uploadId, parts)    — 完成，parts: [{partNumber, etag}]
//!   s3MultipartAbort(client, bucket, key, uploadId)              — 取消
//!   s3UploadBigFile(client, bucket, key, localPath, partSize?)   — 便捷封装（自动分片）

use std::io::Read;
use std::sync::Arc;

use crate::builtins_helpers as bh;
use crate::hash;
use crate::http_lite;
use crate::value::{error_value, Value};
use crate::vm::VM;

/// register 注册所有 S3 内置函数。
pub fn register(vm: &mut VM) {
    vm.register_builtin("s3Connect", bi_s3_connect);
    vm.register_builtin("s3Close", bi_s3_close);
    vm.register_builtin("s3ListBuckets", bi_s3_list_buckets);
    vm.register_builtin("s3CreateBucket", bi_s3_create_bucket);
    vm.register_builtin("s3DeleteBucket", bi_s3_delete_bucket);
    vm.register_builtin("s3BucketExists", bi_s3_bucket_exists);
    vm.register_builtin("s3ListObjects", bi_s3_list_objects);
    vm.register_builtin("s3PutObject", bi_s3_put_object);
    vm.register_builtin("s3GetObject", bi_s3_get_object);
    vm.register_builtin("s3GetObjectBytes", bi_s3_get_object_bytes);
    vm.register_builtin("s3UploadFile", bi_s3_upload_file);
    vm.register_builtin("s3DownloadFile", bi_s3_download_file);
    vm.register_builtin("s3DeleteObject", bi_s3_delete_object);
    vm.register_builtin("s3DeleteObjects", bi_s3_delete_objects);
    vm.register_builtin("s3ObjectExists", bi_s3_object_exists);
    vm.register_builtin("s3ObjectSize", bi_s3_object_size);
    vm.register_builtin("s3CopyObject", bi_s3_copy_object);
    // Multipart Upload（大文件分片上传）
    vm.register_builtin("s3MultipartCreate", bi_s3_multipart_create);
    vm.register_builtin("s3MultipartUploadPart", bi_s3_multipart_upload_part);
    vm.register_builtin("s3MultipartComplete", bi_s3_multipart_complete);
    vm.register_builtin("s3MultipartAbort", bi_s3_multipart_abort);
    vm.register_builtin("s3UploadBigFile", bi_s3_upload_big_file);
}

// ============ S3Client 结构 ============

/// S3Client S3 兼容客户端。
///
/// 不可变配置，所有字段在创建时确定；线程安全（无内部状态）。
/// 脚本层通过 Value::Native(Arc<S3Client>) 引用。
#[derive(Debug, Clone)]
pub struct S3Client {
    /// endpoint 基础 URL，如 "https://s3.amazonaws.com" 或 "http://minio.local:9000"。
    pub endpoint: String,
    /// region 区域，如 "us-east-1"。
    pub region: String,
    /// ak Access Key。
    pub ak: String,
    /// sk Secret Key（仅内存中持有）。
    pub sk: String,
    /// path_style 是否使用 path-style URL（bucket 名在路径中而非子域名）。
    /// MinIO/R2/OSS/COS 通常需要 true；AWS 默认 false（virtual-hosted-style）。
    pub path_style: bool,
    /// use_https 是否使用 HTTPS（从 endpoint 推断）。
    pub use_https: bool,
    /// host 主机名（不含端口），从 endpoint 解析。
    pub host: String,
    /// port 端口号，从 endpoint 解析。
    pub port: u16,
    /// timeout_secs 请求超时秒数（0 = 不超时）。
    pub timeout_secs: u64,
}

impl S3Client {
    /// build_url 构造请求 URL（不含查询串）。
    ///
    /// path_style=true:  {endpoint}/{bucket}/{key}
    /// path_style=false: {scheme}://{bucket}.{host}/{key}（默认端口省略）
    fn build_url(&self, bucket: &str, key: &str, query: &str) -> String {
        let scheme = if self.use_https { "https" } else { "http" };
        let host_wp = self.host_with_port();
        let mut url = if self.path_style {
            let base = self.endpoint.trim_end_matches('/');
            if bucket.is_empty() {
                base.to_string()
            } else if key.is_empty() {
                format!("{}/{}", base, bucket)
            } else {
                format!("{}/{}/{}", base, bucket, key)
            }
        } else {
            // virtual-hosted-style
            if bucket.is_empty() {
                format!("{}://{}", scheme, host_wp)
            } else {
                format!("{}://{}.{}", scheme, bucket, host_wp)
            }
        };
        // virtual-hosted-style 下补充 key 路径
        if !self.path_style && !key.is_empty() {
            url.push('/');
            url.push_str(key);
        }
        if !query.is_empty() {
            url.push('?');
            url.push_str(query);
        }
        url
    }

    /// sign SigV4 签名，返回 (headers_for_request, authorization_header_value)。
    ///
    /// 输入：HTTP 方法、URL 路径（不含查询串，以 / 开头）、查询串（已 URL 编码）、
    ///       请求头（含 host/content-type/x-amz-* 等，需排序后参与签名）、body、ISO8601 时间。
    ///
    /// 返回：完整的 Authorization 头值（已含 Credential/SignedHeaders/Signature）。
    fn sigv4_sign(
        &self,
        method: &str,
        uri: &str,
        query: &str,
        headers: &mut Vec<(String, String)>,
        body: &[u8],
        datetime: &str,
    ) -> String {
        // 日期与短日期
        let date_short = &datetime[..8]; // yyyymmdd
        let payload_hash = hex_hash(&hash::sha256(body));

        // 确保必要头存在
        ensure_header(headers, "x-amz-date", datetime.to_string());
        ensure_header(headers, "x-amz-content-sha256", payload_hash.clone());
        if !self.ak.is_empty() {
            // 使用永久凭证，无需 session token；如需临时凭证可在此扩展
        }

        // 规范化头（按头名小写排序）
        let mut cano_headers: Vec<(String, String, String)> = headers
            .iter()
            .map(|(k, v)| (k.to_lowercase(), v.trim().to_string(), k.to_lowercase()))
            .collect();
        cano_headers.sort_by(|a, b| a.0.cmp(&b.0));
        let cano_headers_str: String = cano_headers
            .iter()
            .map(|(k, v, _)| format!("{}:{}\n", k, v))
            .collect();
        let signed_headers: String = cano_headers
            .iter()
            .map(|(k, _, _)| k.as_str())
            .collect::<Vec<_>>()
            .join(";");

        // 规范化查询串（按 key 排序）
        let cano_query = canonical_query_string(query);

        // 规范化 URI（S3 特殊：连续 / 不压缩）
        let cano_uri = canonical_uri(uri);

        // CanonicalRequest
        let cano_req = format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            method.to_uppercase(),
            cano_uri,
            cano_query,
            cano_headers_str,
            signed_headers,
            payload_hash,
        );

        // StringToSign
        let scope = format!("{}/{}/s3/aws4_request", date_short, self.region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}\n{}",
            datetime,
            scope,
            hex_hash(&hash::sha256(cano_req.as_bytes())),
        );

        // SigningKey
        let k_date = hash::hmac_sha256(format!("AWS4{}", self.sk).as_bytes(), date_short.as_bytes());
        let k_region = hash::hmac_sha256(&k_date, self.region.as_bytes());
        let k_service = hash::hmac_sha256(&k_region, b"s3");
        let k_signing = hash::hmac_sha256(&k_service, b"aws4_request");
        let signature = hex_hash(&hash::hmac_sha256(&k_signing, string_to_sign.as_bytes()));

        format!(
            "AWS4-HMAC-SHA256 Credential={}/{}/{}/s3/aws4_request, SignedHeaders={}, Signature={}",
            self.ak, date_short, self.region, signed_headers, signature,
        )
    }

    /// request 发起 S3 请求并返回响应。
    ///
    /// 自动处理 SigV4 签名、Host 头、超时。
    /// 返回 ClientResponse 或错误字符串（由调用方包装为 error_value）。
    fn request(
        &self,
        method: &str,
        bucket: &str,
        key: &str,
        query: &str,
        extra_headers: Vec<(String, String)>,
        body: Vec<u8>,
    ) -> Result<http_lite::ClientResponse, String> {
        // 解析 URL 与 path
        let (path, host_header) = if self.path_style {
            let p = if bucket.is_empty() {
                "/".to_string()
            } else if key.is_empty() {
                format!("/{}", bucket)
            } else {
                format!("/{}/{}", bucket, key)
            };
            (p, self.host_with_port())
        } else {
            let p = if key.is_empty() { "/".to_string() } else { format!("/{}", key) };
            let h = if bucket.is_empty() {
                self.host_with_port()
            } else {
                format!("{}.{}", bucket, self.host_with_port())
            };
            (p, h)
        };

        // 构造完整 URL
        let full_url = self.build_url(bucket, key, query);

        // 组装请求头（host 必填，参与签名）
        let mut headers: Vec<(String, String)> = Vec::new();
        headers.push(("host".to_string(), host_header));
        for (k, v) in extra_headers {
            headers.push((k, v));
        }

        // 当前时间（UTC，ISO8601 基本格式）
        let datetime = now_iso8601_basic();

        // 签名
        let auth = self.sigv4_sign(method, &path, query, &mut headers, &body, &datetime);

        // 转为 http_lite 接受的 "Key: Value" 字符串数组
        let mut header_strs: Vec<String> = headers
            .iter()
            .filter(|(k, _)| k.to_lowercase() != "host") // host 由 http_lite 自动加
            .map(|(k, v)| format!("{}: {}", k, v))
            .collect();
        header_strs.push(format!("Authorization: {}", auth));

        // content_type 推断（如未指定）
        let content_type = header_strs
            .iter()
            .find_map(|h| {
                if h.to_lowercase().starts_with("content-type:") {
                    Some(h.splitn(2, ':').nth(1).unwrap_or("").trim().to_string())
                } else {
                    None
                }
            })
            .unwrap_or_default();

        http_lite::http_request(method, &full_url, &body, &content_type, &header_strs, self.timeout_secs, 0)
    }

    /// host_with_port 返回 "host:port" 形式（如非默认端口）。
    fn host_with_port(&self) -> String {
        let default = if self.use_https { 443 } else { 80 };
        if self.port == default {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

// ============ 辅助函数 ============

/// client_downcast 从 Value 中提取 S3Client 引用。
fn client_downcast<'a>(v: &'a Value, fn_name: &str) -> Result<&'a Arc<S3Client>, Value> {
    match v {
        Value::Native(n) => n.downcast_ref::<Arc<S3Client>>().ok_or_else(|| {
            error_value(format!(
                "{}() 参数不是 S3 客户端 (可能原因：传入错误类型或 undefined，应先用 s3Connect 创建)",
                fn_name,
            ))
        }),
        Value::Undefined => Err(error_value(format!(
            "{}() 参数为 undefined (可能原因：变量未初始化)", fn_name,
        ))),
        other => Err(error_value(format!(
            "{}() 参数应为 S3 客户端，得到 {} (可能原因：参数顺序错误)",
            fn_name, other.type_name(),
        ))),
    }
}

/// to_bytes_v 将 Value 转为字节 Vec（接受 string/bytes/byteArray）。
fn to_bytes_v(v: &Value, fn_name: &str) -> Result<Vec<u8>, Value> {
    match v {
        Value::Str(s) => Ok(s.as_bytes().to_vec()),
        Value::Bytes(b) => Ok(b.as_ref().to_vec()),
        Value::ByteArray(b) => Ok(b.lock().unwrap().clone()),
        other => Err(error_value(format!(
            "{}() 数据参数应为 string/bytes/byteArray，得到 {} (可能原因：参数类型不匹配)",
            fn_name, other.type_name(),
        ))),
    }
}

/// hex_hash 将字节数组转为小写十六进制字符串。
fn hex_hash(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// ensure_header 确保请求头列表中存在指定头（不存在则插入，存在则覆盖）。
fn ensure_header(headers: &mut Vec<(String, String)>, name: &str, value: String) {
    // 先删后插（不区分大小写）
    let lower = name.to_lowercase();
    headers.retain(|(k, _)| k.to_lowercase() != lower);
    headers.push((name.to_string(), value));
}

/// canonical_uri 规范化 URI（S3 特殊：连续 / 不压缩）。
fn canonical_uri(uri: &str) -> String {
    if uri.is_empty() {
        "/".to_string()
    } else {
        // S3 每个路径段需 URL 编码（保留 /）
        uri.split('/')
            .map(|seg| {
                if seg.is_empty() {
                    String::new()
                } else {
                    url_encode(seg)
                }
            })
            .collect::<Vec<_>>()
            .join("/")
    }
}

/// canonical_query_string 规范化查询串（按 key 排序，已 URL 编码）。
fn canonical_query_string(query: &str) -> String {
    if query.is_empty() {
        return String::new();
    }
    let mut pairs: Vec<(String, String)> = query
        .split('&')
        .filter(|s| !s.is_empty())
        .map(|s| {
            if let Some(pos) = s.find('=') {
                (s[..pos].to_string(), s[pos + 1..].to_string())
            } else {
                (s.to_string(), String::new())
            }
        })
        .collect();
    pairs.sort();
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("&")
}

/// url_encode RFC 3986 URL 编码（保留 / = & 等不编码，编码其他特殊字符）。
///
/// S3 SigV4 要求：A-Za-z0-9-_.~ 和 / 不编码，其余全部 %XX。
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// now_iso8601_basic 返回当前 UTC 时间的 ISO8601 基本格式（yyyyMMddTHHmmssZ）。
fn now_iso8601_basic() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    epoch_to_iso8601_basic(secs)
}

/// epoch_to_iso8601_basic 将 Unix 秒转为 ISO8601 基本格式字符串。
fn epoch_to_iso8601_basic(secs: u64) -> String {
    let days = secs / 86400;
    let rem = secs % 86400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, mo, d) = days_to_date(days);
    format!("{:04}{:02}{:02}T{:02}{:02}{:02}Z", y, mo, d, h, m, s)
}

/// days_to_date 将 Unix 纪元以来的天数转为 (年, 月, 日)（公历，UTC）。
fn days_to_date(days: u64) -> (u32, u32, u32) {
    // 从 1970-01-01 起算
    let mut year = 1970u32;
    let mut remaining = days;
    loop {
        let dy = if is_leap_year(year) { 366 } else { 365 };
        if remaining < dy {
            break;
        }
        remaining -= dy;
        year += 1;
    }
    let month_days: [u32; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 1u32;
    let mut day = remaining as u32 + 1;
    for &md in &month_days {
        if day <= md {
            break;
        }
        day -= md;
        month += 1;
    }
    (year, month, day)
}

/// is_leap_year 判断闰年（公历规则）。
fn is_leap_year(y: u32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

/// http_error 构造 HTTP 错误信息（AI 友好，含状态码与可能原因）。
fn http_error(fn_name: &str, status: u16, body: &[u8]) -> Value {
    let body_str = String::from_utf8_lossy(body);
    // 截取前 500 字符避免错误信息过长
    let body_preview = if body_str.len() > 500 {
        &body_str[..500]
    } else {
        &body_str
    };
    let hint = match status {
        401 | 403 => "可能原因：AK/SK 错误或过期、权限不足、签名错误、本地时钟偏差超过 15 分钟",
        404 => "可能原因：bucket 或 key 不存在、path-style 配置错误（MinIO 应为 true）",
        409 => "可能原因：bucket 已存在或非空（删除时）、并发冲突",
        500..=599 => "可能原因：S3 服务端错误，可重试",
        _ => "可能原因：请求格式错误、网络问题、endpoint 配置不当",
    };
    error_value(format!(
        "{}() 失败: HTTP {} (可能原因：{})\n响应: {}",
        fn_name, status, hint, body_preview,
    ))
}

// ============ XML 解析（最小子集，仅提取需要的字段） ============

/// extract_xml_values 从 XML 文本中提取指定标签的所有文本内容。
///
/// 简化实现：扫描 <tag>...</tag>，返回所有匹配的文本（不去重）。
/// 适用于 S3 ListAllMyBucketsResult / ListBucketResult 等 S3 响应。
fn extract_xml_values(xml: &str, tag: &str) -> Vec<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let mut result = Vec::new();
    let mut start = 0;
    while let Some(s) = xml[start..].find(&open) {
        let abs_start = start + s + open.len();
        if let Some(e) = xml[abs_start..].find(&close) {
            let text = xml[abs_start..abs_start + e].trim();
            result.push(text.to_string());
            start = abs_start + e + close.len();
        } else {
            break;
        }
    }
    result
}

// ============ 内置函数实现 ============

/// bi_s3_connect 创建 S3 客户端。
///
/// 用法：
///   s3Connect(endpoint, region, ak, sk)            — 默认 path-style 自动推断
///   s3Connect(endpoint, region, ak, sk, pathStyle) — 显式指定 path-style
///
/// endpoint 自动推断：含 IP 或非标准端口 → path-style=true（兼容 MinIO）；
/// 以 s3.amazonaws.com 或 s3.<region>.amazonaws.com 结尾 → path-style=false。
fn bi_s3_connect(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let endpoint = bh::as_str(args, 0, "s3Connect")?.to_string();
    let region = bh::as_str(args, 1, "s3Connect")?.to_string();
    let ak = bh::as_str(args, 2, "s3Connect")?.to_string();
    let sk = bh::as_str(args, 3, "s3Connect")?.to_string();

    // 解析 endpoint
    let parsed = match http_lite::parse_http_url(&endpoint) {
        Ok(p) => p,
        Err(e) => {
            return Ok(error_value(format!(
                "s3Connect() endpoint 解析失败: {} (可能原因：endpoint 需以 http:// 或 https:// 开头)", e,
            )));
        }
    };
    let use_https = parsed.is_tls();
    let port = parsed.port_num();
    let host = parsed.host.clone();

    // path-style 推断
    let auto_path_style = if let Some(style) = args.get(5).and_then(|v| match v {
        Value::Bool(b) => Some(*b),
        _ => None,
    }) {
        style
    } else {
        // 推断规则：非标准端口（非 80/443）或 IP 地址 → path-style
        let non_default_port = port != 80 && port != 443;
        let is_ip = host.chars().all(|c| c.is_ascii_digit() || c == '.');
        let is_aws = host.ends_with("amazonaws.com");
        non_default_port || is_ip || !is_aws
    };

    let client = Arc::new(S3Client {
        endpoint,
        region,
        ak,
        sk,
        path_style: auto_path_style,
        use_https,
        host,
        port,
        timeout_secs: 30,
    });
    Ok(Value::Native(Arc::new(client)))
}

/// bi_s3_close 关闭客户端（无实际资源，仅规范）。
fn bi_s3_close(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Undefined)
}

/// bi_s3_list_buckets 列出所有 bucket。
///
/// 返回 array，每个元素为 {name: string, createdAt: string}。
fn bi_s3_list_buckets(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let client = client_downcast(&args[0], "s3ListBuckets")?;

    match client.request("GET", "", "", "", vec![], Vec::new()) {
        Ok(resp) => {
            if resp.status != 200 {
                return Ok(http_error("s3ListBuckets", resp.status, &resp.body));
            }
            let xml = String::from_utf8_lossy(&resp.body);
            // S3 返回 <ListAllMyBucketsResult><Buckets><Bucket><Name>...</Name><CreationDate>...</CreationDate></Bucket>...</Buckets>
            let names = extract_xml_values(&xml, "Name");
            let dates = extract_xml_values(&xml, "CreationDate");
            let mut arr = Vec::new();
            for (i, name) in names.iter().enumerate() {
                let m = crate::object_map::new_map();
                {
                    let mut g = m.lock().unwrap();
                    g.data.insert("name".to_string(), Value::Str(Arc::from(name.as_str())));
                    let created = if i < dates.len() { dates[i].clone() } else { String::new() };
                    g.data.insert("createdAt".to_string(), Value::Str(Arc::from(created.as_str())));
                }
                arr.push(Value::Object(m));
            }
            Ok(Value::Array(Arc::new(std::sync::Mutex::new(arr))))
        }
        Err(e) => Ok(error_value(format!(
            "s3ListBuckets() 请求失败: {} (可能原因：endpoint 不可达、网络问题、AK/SK 错误)", e,
        ))),
    }
}

/// bi_s3_create_bucket 创建 bucket。
fn bi_s3_create_bucket(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let client = client_downcast(&args[0], "s3CreateBucket")?;
    let bucket = bh::as_str(args, 1, "s3CreateBucket")?;

    match client.request("PUT", bucket, "", "", vec![], Vec::new()) {
        Ok(resp) => {
            if resp.status == 200 || resp.status == 204 {
                Ok(Value::Bool(true))
            } else {
                Ok(http_error("s3CreateBucket", resp.status, &resp.body))
            }
        }
        Err(e) => Ok(error_value(format!(
            "s3CreateBucket() 请求失败: {} (可能原因：bucket 名不合法、已存在、权限不足)", e,
        ))),
    }
}

/// bi_s3_delete_bucket 删除空 bucket。
fn bi_s3_delete_bucket(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let client = client_downcast(&args[0], "s3DeleteBucket")?;
    let bucket = bh::as_str(args, 1, "s3DeleteBucket")?;

    match client.request("DELETE", bucket, "", "", vec![], Vec::new()) {
        Ok(resp) => {
            if resp.status == 200 || resp.status == 204 {
                Ok(Value::Bool(true))
            } else {
                Ok(http_error("s3DeleteBucket", resp.status, &resp.body))
            }
        }
        Err(e) => Ok(error_value(format!(
            "s3DeleteBucket() 请求失败: {} (可能原因：bucket 不存在或非空、权限不足)", e,
        ))),
    }
}

/// bi_s3_bucket_exists 检查 bucket 是否存在（HEAD 请求）。
fn bi_s3_bucket_exists(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let client = client_downcast(&args[0], "s3BucketExists")?;
    let bucket = bh::as_str(args, 1, "s3BucketExists")?;

    match client.request("HEAD", bucket, "", "", vec![], Vec::new()) {
        Ok(resp) => {
            // 200 = 存在，404 = 不存在，其他 = 错误
            if resp.status == 200 {
                Ok(Value::Bool(true))
            } else if resp.status == 404 {
                Ok(Value::Bool(false))
            } else {
                Ok(http_error("s3BucketExists", resp.status, &resp.body))
            }
        }
        Err(e) => Ok(error_value(format!(
            "s3BucketExists() 请求失败: {} (可能原因：endpoint 不可达、网络问题)", e,
        ))),
    }
}

/// bi_s3_list_objects 列出 bucket 内对象。
///
/// 用法：
///   s3ListObjects(client, bucket)                       — 默认 prefix="" maxKeys=1000
///   s3ListObjects(client, bucket, prefix)              — 指定前缀
///   s3ListObjects(client, bucket, prefix, maxKeys)     — 指定前缀和最大数量
///
/// 返回 array，每个元素为 {key, size, lastModified}。
fn bi_s3_list_objects(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let client = client_downcast(&args[0], "s3ListObjects")?;
    let bucket = bh::as_str(args, 1, "s3ListObjects")?;

    let prefix = args.get(2).and_then(|v| match v {
        Value::Str(s) => Some(s.as_ref().to_string()),
        _ => None,
    }).unwrap_or_default();

    let max_keys = args.get(3).and_then(|v| match v {
        Value::Int(i) => Some(*i as i64),
        Value::Float(f) => Some(*f as i64),
        _ => None,
    }).unwrap_or(1000);

    // 构造查询串（已 URL 编码）
    let mut query_parts = Vec::new();
    if !prefix.is_empty() {
        query_parts.push(format!("prefix={}", url_encode(&prefix)));
    }
    query_parts.push(format!("max-keys={}", max_keys));
    let query = query_parts.join("&");

    match client.request("GET", bucket, "", &query, vec![], Vec::new()) {
        Ok(resp) => {
            if resp.status != 200 {
                return Ok(http_error("s3ListObjects", resp.status, &resp.body));
            }
            let xml = String::from_utf8_lossy(&resp.body);
            let keys = extract_xml_values(&xml, "Key");
            let sizes = extract_xml_values(&xml, "Size");
            let dates = extract_xml_values(&xml, "LastModified");
            let mut arr = Vec::new();
            for (i, key) in keys.iter().enumerate() {
                let m = crate::object_map::new_map();
                {
                    let mut g = m.lock().unwrap();
                    g.data.insert("key".to_string(), Value::Str(Arc::from(key.as_str())));
                    let size: i64 = sizes.get(i).and_then(|s| s.parse().ok()).unwrap_or(0);
                    g.data.insert("size".to_string(), Value::Int(size));
                    let lm = dates.get(i).cloned().unwrap_or_default();
                    g.data.insert("lastModified".to_string(), Value::Str(Arc::from(lm.as_str())));
                }
                arr.push(Value::Object(m));
            }
            Ok(Value::Array(Arc::new(std::sync::Mutex::new(arr))))
        }
        Err(e) => Ok(error_value(format!(
            "s3ListObjects() 请求失败: {} (可能原因：bucket 不存在、权限不足、endpoint 不可达)", e,
        ))),
    }
}

/// bi_s3_put_object 写入对象（接受 string/bytes/byteArray）。
fn bi_s3_put_object(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let client = client_downcast(&args[0], "s3PutObject")?;
    let bucket = bh::as_str(args, 1, "s3PutObject")?;
    let key = bh::as_str(args, 2, "s3PutObject")?;
    bh::require_arg(args, 3, "s3PutObject")?;
    let body = to_bytes_v(&args[3], "s3PutObject")?;

    // 推断 Content-Type（默认 application/octet-stream）
    let content_type = guess_content_type(key);
    let extra = vec![("Content-Type".to_string(), content_type)];

    match client.request("PUT", bucket, key, "", extra, body) {
        Ok(resp) => {
            if resp.status == 200 || resp.status == 204 {
                Ok(Value::Bool(true))
            } else {
                Ok(http_error("s3PutObject", resp.status, &resp.body))
            }
        }
        Err(e) => Ok(error_value(format!(
            "s3PutObject() 上传失败: {} (可能原因：bucket 不存在、权限不足、key 不合法)", e,
        ))),
    }
}

/// bi_s3_get_object 读取对象为 string（UTF-8）。
fn bi_s3_get_object(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let client = client_downcast(&args[0], "s3GetObject")?;
    let bucket = bh::as_str(args, 1, "s3GetObject")?;
    let key = bh::as_str(args, 2, "s3GetObject")?;

    match client.request("GET", bucket, key, "", vec![], Vec::new()) {
        Ok(resp) => {
            if resp.status == 200 {
                let s = String::from_utf8_lossy(&resp.body).to_string();
                Ok(Value::Str(Arc::from(s.as_str())))
            } else {
                Ok(http_error("s3GetObject", resp.status, &resp.body))
            }
        }
        Err(e) => Ok(error_value(format!(
            "s3GetObject() 下载失败: {} (可能原因：bucket/key 不存在、权限不足)", e,
        ))),
    }
}

/// bi_s3_get_object_bytes 读取对象为 bytes（二进制安全）。
fn bi_s3_get_object_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let client = client_downcast(&args[0], "s3GetObjectBytes")?;
    let bucket = bh::as_str(args, 1, "s3GetObjectBytes")?;
    let key = bh::as_str(args, 2, "s3GetObjectBytes")?;

    match client.request("GET", bucket, key, "", vec![], Vec::new()) {
        Ok(resp) => {
            if resp.status == 200 {
                Ok(Value::Bytes(Arc::new(resp.body)))
            } else {
                Ok(http_error("s3GetObjectBytes", resp.status, &resp.body))
            }
        }
        Err(e) => Ok(error_value(format!(
            "s3GetObjectBytes() 下载失败: {} (可能原因：bucket/key 不存在、权限不足)", e,
        ))),
    }
}

/// bi_s3_upload_file 上传本地文件到 S3。
fn bi_s3_upload_file(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let client = client_downcast(&args[0], "s3UploadFile")?;
    let bucket = bh::as_str(args, 1, "s3UploadFile")?;
    let key = bh::as_str(args, 2, "s3UploadFile")?;
    let local_path = bh::as_str(args, 3, "s3UploadFile")?;

    let body = match std::fs::read(local_path) {
        Ok(b) => b,
        Err(e) => {
            return Ok(error_value(format!(
                "s3UploadFile() 读取本地文件 '{}' 失败: {} (可能原因：文件不存在、无读取权限)",
                local_path, e,
            )));
        }
    };

    let content_type = guess_content_type(key);
    let extra = vec![("Content-Type".to_string(), content_type)];

    match client.request("PUT", bucket, key, "", extra, body) {
        Ok(resp) => {
            if resp.status == 200 || resp.status == 204 {
                Ok(Value::Bool(true))
            } else {
                Ok(http_error("s3UploadFile", resp.status, &resp.body))
            }
        }
        Err(e) => Ok(error_value(format!(
            "s3UploadFile() 上传失败: {} (可能原因：bucket 不存在、权限不足、网络问题)", e,
        ))),
    }
}

/// bi_s3_download_file 下载 S3 对象到本地文件。
fn bi_s3_download_file(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let client = client_downcast(&args[0], "s3DownloadFile")?;
    let bucket = bh::as_str(args, 1, "s3DownloadFile")?;
    let key = bh::as_str(args, 2, "s3DownloadFile")?;
    let local_path = bh::as_str(args, 3, "s3DownloadFile")?;

    match client.request("GET", bucket, key, "", vec![], Vec::new()) {
        Ok(resp) => {
            if resp.status != 200 {
                return Ok(http_error("s3DownloadFile", resp.status, &resp.body));
            }
            match std::fs::write(local_path, &resp.body) {
                Ok(_) => Ok(Value::Bool(true)),
                Err(e) => Ok(error_value(format!(
                    "s3DownloadFile() 写入本地文件 '{}' 失败: {} (可能原因：路径不存在、无写入权限)",
                    local_path, e,
                ))),
            }
        }
        Err(e) => Ok(error_value(format!(
            "s3DownloadFile() 下载失败: {} (可能原因：bucket/key 不存在、网络问题)", e,
        ))),
    }
}

/// bi_s3_delete_object 删除单个对象。
fn bi_s3_delete_object(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let client = client_downcast(&args[0], "s3DeleteObject")?;
    let bucket = bh::as_str(args, 1, "s3DeleteObject")?;
    let key = bh::as_str(args, 2, "s3DeleteObject")?;

    match client.request("DELETE", bucket, key, "", vec![], Vec::new()) {
        Ok(resp) => {
            if resp.status == 200 || resp.status == 204 {
                Ok(Value::Bool(true))
            } else {
                Ok(http_error("s3DeleteObject", resp.status, &resp.body))
            }
        }
        Err(e) => Ok(error_value(format!(
            "s3DeleteObject() 删除失败: {} (可能原因：bucket/key 不存在、权限不足)", e,
        ))),
    }
}

/// bi_s3_delete_objects 批量删除对象。
///
/// 用法：s3DeleteObjects(client, bucket, keys)
/// keys: array<string>
///
/// 返回 {deleted: int, errors: array{key, msg}}
fn bi_s3_delete_objects(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let client = client_downcast(&args[0], "s3DeleteObjects")?;
    let bucket = bh::as_str(args, 1, "s3DeleteObjects")?;
    bh::require_arg(args, 2, "s3DeleteObjects")?;
    let keys_arc = bh::as_array(args, 2, "s3DeleteObjects")?;
    let keys = keys_arc.lock().unwrap();

    let mut deleted = 0i64;
    let mut errs: Vec<Value> = Vec::new();

    for v in keys.iter() {
        let key = match v {
            Value::Str(s) => s.to_string(),
            _ => {
                errs.push(Value::Str(Arc::from(format!("非字符串 key: {}", v.type_name()).as_str())));
                continue;
            }
        };
        match client.request("DELETE", bucket, &key, "", vec![], Vec::new()) {
            Ok(resp) => {
                if resp.status == 200 || resp.status == 204 {
                    deleted += 1;
                } else {
                    let m = crate::object_map::new_map();
                    {
                        let mut g = m.lock().unwrap();
                        g.data.insert("key".to_string(), Value::Str(Arc::from(key.as_str())));
                        g.data.insert("msg".to_string(), Value::Str(Arc::from(format!("HTTP {}", resp.status).as_str())));
                    }
                    errs.push(Value::Object(m));
                }
            }
            Err(e) => {
                let m = crate::object_map::new_map();
                {
                    let mut g = m.lock().unwrap();
                    g.data.insert("key".to_string(), Value::Str(Arc::from(key.as_str())));
                    g.data.insert("msg".to_string(), Value::Str(Arc::from(e.as_str())));
                }
                errs.push(Value::Object(m));
            }
        }
    }

    let m = crate::object_map::new_map();
    {
        let mut g = m.lock().unwrap();
        g.data.insert("deleted".to_string(), Value::Int(deleted));
        g.data.insert("errors".to_string(), Value::Array(Arc::new(std::sync::Mutex::new(errs))));
    }
    Ok(Value::Object(m))
}

/// bi_s3_object_exists 检查对象是否存在（HEAD 请求）。
fn bi_s3_object_exists(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let client = client_downcast(&args[0], "s3ObjectExists")?;
    let bucket = bh::as_str(args, 1, "s3ObjectExists")?;
    let key = bh::as_str(args, 2, "s3ObjectExists")?;

    match client.request("HEAD", bucket, key, "", vec![], Vec::new()) {
        Ok(resp) => {
            if resp.status == 200 {
                Ok(Value::Bool(true))
            } else if resp.status == 404 {
                Ok(Value::Bool(false))
            } else {
                Ok(http_error("s3ObjectExists", resp.status, &resp.body))
            }
        }
        Err(e) => Ok(error_value(format!(
            "s3ObjectExists() 请求失败: {} (可能原因：bucket 不存在、网络问题)", e,
        ))),
    }
}

/// bi_s3_object_size 获取对象大小（Content-Length）。
fn bi_s3_object_size(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let client = client_downcast(&args[0], "s3ObjectSize")?;
    let bucket = bh::as_str(args, 1, "s3ObjectSize")?;
    let key = bh::as_str(args, 2, "s3ObjectSize")?;

    match client.request("HEAD", bucket, key, "", vec![], Vec::new()) {
        Ok(resp) => {
            if resp.status == 200 {
                if let Some(len) = resp.get_header("content-length") {
                    if let Ok(n) = len.parse::<i64>() {
                        return Ok(Value::Int(n));
                    }
                }
                Ok(error_value("s3ObjectSize() 响应缺少 Content-Length 头 (可能原因：S3 服务端响应异常)"))
            } else if resp.status == 404 {
                Ok(error_value("s3ObjectSize() 对象不存在 (可能原因：bucket/key 不存在)"))
            } else {
                Ok(http_error("s3ObjectSize", resp.status, &resp.body))
            }
        }
        Err(e) => Ok(error_value(format!(
            "s3ObjectSize() 请求失败: {} (可能原因：bucket 不存在、网络问题)", e,
        ))),
    }
}

/// bi_s3_copy_object 服务端拷贝对象。
///
/// 用法：s3CopyObject(client, srcBucket, srcKey, dstBucket, dstKey)
/// 使用 x-amz-copy-source 头指定源对象。
fn bi_s3_copy_object(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let client = client_downcast(&args[0], "s3CopyObject")?;
    let src_bucket = bh::as_str(args, 1, "s3CopyObject")?;
    let src_key = bh::as_str(args, 2, "s3CopyObject")?;
    let dst_bucket = bh::as_str(args, 3, "s3CopyObject")?;
    let dst_key = bh::as_str(args, 4, "s3CopyObject")?;

    let copy_source = format!("{}/{}", src_bucket, src_key);
    let extra = vec![("x-amz-copy-source".to_string(), copy_source)];

    match client.request("PUT", dst_bucket, dst_key, "", extra, Vec::new()) {
        Ok(resp) => {
            if resp.status == 200 || resp.status == 204 {
                Ok(Value::Bool(true))
            } else {
                Ok(http_error("s3CopyObject", resp.status, &resp.body))
            }
        }
        Err(e) => Ok(error_value(format!(
            "s3CopyObject() 拷贝失败: {} (可能原因：源对象不存在、权限不足、目标 bucket 不存在)", e,
        ))),
    }
}

/// guess_content_type 根据扩展名推断 Content-Type。
fn guess_content_type(key: &str) -> String {
    let lower = key.to_lowercase();
    if lower.ends_with(".html") || lower.ends_with(".htm") {
        "text/html; charset=utf-8".to_string()
    } else if lower.ends_with(".txt") || lower.ends_with(".log") {
        "text/plain; charset=utf-8".to_string()
    } else if lower.ends_with(".json") {
        "application/json".to_string()
    } else if lower.ends_with(".xml") {
        "application/xml".to_string()
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg".to_string()
    } else if lower.ends_with(".png") {
        "image/png".to_string()
    } else if lower.ends_with(".gif") {
        "image/gif".to_string()
    } else if lower.ends_with(".pdf") {
        "application/pdf".to_string()
    } else if lower.ends_with(".zip") {
        "application/zip".to_string()
    } else if lower.ends_with(".gz") {
        "application/gzip".to_string()
    } else {
        "application/octet-stream".to_string()
    }
}

// ============ Multipart Upload（大文件分片上传） ============
//
// S3 Multipart Upload 流程：
//   1. CreateMultipartUpload  — POST /{key}?uploads 返回 UploadId
//   2. UploadPart            — PUT  /{key}?partNumber=N&uploadId=X 返回 ETag
//   3. CompleteMultipartUpload — POST /{key}?uploadId=X body=XML 列出所有 part
//   4. AbortMultipartUpload  — DELETE /{key}?uploadId=X
//
// 限制：
//   - 每个分片最小 5MB（最后一块除外）
//   - 每个分片最大 5GB
//   - 最多 10000 个分片
//   - 单对象最大 5TB

/// bi_s3_multipart_create 启动 multipart upload。
///
/// 用法：
///   s3MultipartCreate(client, bucket, key)             — 默认 Content-Type
///   s3MultipartCreate(client, bucket, key, contentType) — 指定 Content-Type
///
/// 返回 uploadId（字符串）或 error。
fn bi_s3_multipart_create(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let client = client_downcast(&args[0], "s3MultipartCreate")?;
    let bucket = bh::as_str(args, 1, "s3MultipartCreate")?;
    let key = bh::as_str(args, 2, "s3MultipartCreate")?;
    let content_type = args.get(3).and_then(|v| match v {
        Value::Str(s) => Some(s.to_string()),
        _ => None,
    }).unwrap_or_else(|| guess_content_type(key));

    let extra = vec![
        ("Content-Type".to_string(), content_type),
        ("x-amz-content-sha256".to_string(), hex_hash(&hash::sha256(&[]))),
    ];

    match client.request("POST", bucket, key, "uploads", extra, Vec::new()) {
        Ok(resp) => {
            if resp.status == 200 {
                let xml = String::from_utf8_lossy(&resp.body);
                let ids = extract_xml_values(&xml, "UploadId");
                if let Some(id) = ids.into_iter().next() {
                    Ok(Value::Str(Arc::from(id.as_str())))
                } else {
                    Ok(error_value(
                        "s3MultipartCreate() 响应中未找到 UploadId (可能原因：S3 服务端响应格式异常)",
                    ))
                }
            } else {
                Ok(http_error("s3MultipartCreate", resp.status, &resp.body))
            }
        }
        Err(e) => Ok(error_value(format!(
            "s3MultipartCreate() 请求失败: {} (可能原因：bucket 不存在、权限不足)", e,
        ))),
    }
}

/// bi_s3_multipart_upload_part 上传单个分片。
///
/// 用法：s3MultipartUploadPart(client, bucket, key, uploadId, partNo, data)
///   partNo: 分片编号（1-10000）
///   data: string/bytes/byteArray
///
/// 返回 ETag（字符串，含双引号）或 error。
fn bi_s3_multipart_upload_part(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let client = client_downcast(&args[0], "s3MultipartUploadPart")?;
    let bucket = bh::as_str(args, 1, "s3MultipartUploadPart")?;
    let key = bh::as_str(args, 2, "s3MultipartUploadPart")?;
    let upload_id = bh::as_str(args, 3, "s3MultipartUploadPart")?;
    let part_no = bh::as_int(args, 4, "s3MultipartUploadPart")?;
    bh::require_arg(args, 5, "s3MultipartUploadPart")?;
    let body = to_bytes_v(&args[5], "s3MultipartUploadPart")?;

    if part_no < 1 || part_no > 10000 {
        return Ok(error_value(format!(
            "s3MultipartUploadPart() partNo 应为 1-10000，得到 {} (可能原因：分片编号超限)", part_no,
        )));
    }

    // S3 规范：除最后一片外，每片至少 5MB
    // 注意：是否最后一片由 CompleteMultipartUpload 决定，这里只校验非最后一片的下限
    // 实际由用户决定，不强制 5MB（避免无法上传小于 5MB 的最后一片）

    let query = format!("partNumber={}&uploadId={}", part_no, url_encode(upload_id));
    let extra = vec![("Content-Length".to_string(), body.len().to_string())];

    match client.request("PUT", bucket, key, &query, extra, body) {
        Ok(resp) => {
            if resp.status == 200 {
                // ETag 在响应头中
                if let Some(etag) = resp.get_header("etag") {
                    Ok(Value::Str(Arc::from(etag)))
                } else {
                    Ok(error_value(
                        "s3MultipartUploadPart() 响应缺少 ETag 头 (可能原因：S3 服务端响应异常)",
                    ))
                }
            } else {
                Ok(http_error("s3MultipartUploadPart", resp.status, &resp.body))
            }
        }
        Err(e) => Ok(error_value(format!(
            "s3MultipartUploadPart() 上传分片失败: {} (可能原因：网络问题、uploadId 失效、权限不足)", e,
        ))),
    }
}

/// bi_s3_multipart_complete 完成 multipart upload。
///
/// 用法：s3MultipartComplete(client, bucket, key, uploadId, parts)
///   parts: array of {partNumber: int, etag: string}
///
/// 返回 true 或 error。
fn bi_s3_multipart_complete(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let client = client_downcast(&args[0], "s3MultipartComplete")?;
    let bucket = bh::as_str(args, 1, "s3MultipartComplete")?;
    let key = bh::as_str(args, 2, "s3MultipartComplete")?;
    let upload_id = bh::as_str(args, 3, "s3MultipartComplete")?;
    bh::require_arg(args, 4, "s3MultipartComplete")?;
    let parts_arc = bh::as_array(args, 4, "s3MultipartComplete")?;
    let parts = parts_arc.lock().unwrap();

    // 构造 CompleteMultipartUpload XML body
    // <CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>"..."</ETag></Part>...</CompleteMultipartUpload>
    let mut xml = String::from("<CompleteMultipartUpload>");
    for (i, p) in parts.iter().enumerate() {
        let part_number: i64;
        let etag: String;
        match p {
            Value::Object(m) => {
                let g = m.lock().unwrap();
                let pn = g.data.get("partNumber");
                let et = g.data.get("etag");
                match (pn, et) {
                    (Some(Value::Int(n)), Some(Value::Str(s))) => {
                        part_number = *n;
                        etag = s.to_string();
                    }
                    _ => {
                        return Ok(error_value(format!(
                            "s3MultipartComplete() parts[{}] 应含 partNumber(int) 和 etag(string) 字段", i,
                        )));
                    }
                }
            }
            _ => {
                return Ok(error_value(format!(
                    "s3MultipartComplete() parts[{}] 应为 object，得到 {} (可能原因：参数类型错误)", i, p.type_name(),
                )));
            }
        }
        xml.push_str(&format!(
            "<Part><PartNumber>{}</PartNumber><ETag>{}</ETag></Part>",
            part_number, etag,
        ));
    }
    xml.push_str("</CompleteMultipartUpload>");

    let query = format!("uploadId={}", url_encode(upload_id));
    let body = xml.into_bytes();
    let extra = vec![
        ("Content-Type".to_string(), "application/xml".to_string()),
        ("Content-Length".to_string(), body.len().to_string()),
    ];

    match client.request("POST", bucket, key, &query, extra, body) {
        Ok(resp) => {
            if resp.status == 200 {
                Ok(Value::Bool(true))
            } else {
                Ok(http_error("s3MultipartComplete", resp.status, &resp.body))
            }
        }
        Err(e) => Ok(error_value(format!(
            "s3MultipartComplete() 完成失败: {} (可能原因：uploadId 失效、part 列表不完整、权限不足)", e,
        ))),
    }
}

/// bi_s3_multipart_abort 取消 multipart upload。
///
/// 用法：s3MultipartAbort(client, bucket, key, uploadId)
/// 返回 true 或 error。
fn bi_s3_multipart_abort(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let client = client_downcast(&args[0], "s3MultipartAbort")?;
    let bucket = bh::as_str(args, 1, "s3MultipartAbort")?;
    let key = bh::as_str(args, 2, "s3MultipartAbort")?;
    let upload_id = bh::as_str(args, 3, "s3MultipartAbort")?;

    let query = format!("uploadId={}", url_encode(upload_id));

    match client.request("DELETE", bucket, key, &query, vec![], Vec::new()) {
        Ok(resp) => {
            if resp.status == 200 || resp.status == 204 {
                Ok(Value::Bool(true))
            } else {
                Ok(http_error("s3MultipartAbort", resp.status, &resp.body))
            }
        }
        Err(e) => Ok(error_value(format!(
            "s3MultipartAbort() 取消失败: {} (可能原因：uploadId 已失效或已完成、网络问题)", e,
        ))),
    }
}

/// bi_s3_upload_big_file 大文件分片上传（便捷封装）。
///
/// 用法：
///   s3UploadBigFile(client, bucket, key, localPath)              — 默认 5MB 分片
///   s3UploadBigFile(client, bucket, key, localPath, partSize)    — 指定分片大小（字节）
///
/// 内部流程：
///   1. 启动 multipart upload，获取 uploadId
///   2. 按分片大小读取文件，逐片上传，收集 ETag
///   3. 完成 multipart upload
///   4. 任一步骤失败时自动 abort
///
/// partSize 默认 5MB（5242880 字节），最小 5MB，最大 5GB。
/// 文件大小 < 5MB 时自动回退到单次 PUT（s3UploadFile 逻辑）。
fn bi_s3_upload_big_file(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let client = client_downcast(&args[0], "s3UploadBigFile")?;
    let bucket = bh::as_str(args, 1, "s3UploadBigFile")?;
    let key = bh::as_str(args, 2, "s3UploadBigFile")?;
    let local_path = bh::as_str(args, 3, "s3UploadBigFile")?;

    let part_size = args.get(4).and_then(|v| match v {
        Value::Int(i) => Some(*i as usize),
        Value::Float(f) => Some(*f as usize),
        _ => None,
    }).unwrap_or(5 * 1024 * 1024); // 默认 5MB

    // 校验分片大小
    let min_part: usize = 5 * 1024 * 1024;
    let max_part: usize = 5 * 1024 * 1024 * 1024;
    if part_size < min_part {
        return Ok(error_value(format!(
            "s3UploadBigFile() partSize 最小 5MB（{} 字节），得到 {} 字节 (可能原因：分片大小设置过小)",
            min_part, part_size,
        )));
    }
    if part_size > max_part {
        return Ok(error_value(format!(
            "s3UploadBigFile() partSize 最大 5GB（{} 字节），得到 {} 字节", max_part, part_size,
        )));
    }

    // 获取文件大小
    let file_size = match std::fs::metadata(local_path) {
        Ok(m) => m.len() as usize,
        Err(e) => {
            return Ok(error_value(format!(
                "s3UploadBigFile() 读取文件 '{}' 元数据失败: {} (可能原因：文件不存在、无权限)",
                local_path, e,
            )));
        }
    };

    // 小文件（< 5MB）直接走单次 PUT
    if file_size < min_part {
        return bi_s3_upload_file_internal(client, bucket, key, local_path);
    }

    // 计算分片数
    let total_parts = (file_size + part_size - 1) / part_size;
    if total_parts > 10000 {
        return Ok(error_value(format!(
            "s3UploadBigFile() 分片数 {} 超过 10000 上限 (可能原因：partSize 过小或文件过大，建议增大 partSize)",
            total_parts,
        )));
    }

    // 1. 启动 multipart upload
    let content_type = guess_content_type(key);
    let extra = vec![
        ("Content-Type".to_string(), content_type.clone()),
        ("x-amz-content-sha256".to_string(), hex_hash(&hash::sha256(&[]))),
    ];
    let upload_id = match client.request("POST", bucket, key, "uploads", extra, Vec::new()) {
        Ok(resp) => {
            if resp.status != 200 {
                return Ok(http_error("s3UploadBigFile", resp.status, &resp.body));
            }
            let xml = String::from_utf8_lossy(&resp.body);
            let ids = extract_xml_values(&xml, "UploadId");
            match ids.into_iter().next() {
                Some(id) => id,
                None => {
                    return Ok(error_value(
                        "s3UploadBigFile() 启动 multipart upload 失败：响应中未找到 UploadId",
                    ));
                }
            }
        }
        Err(e) => {
            return Ok(error_value(format!(
                "s3UploadBigFile() 启动 multipart upload 失败: {}", e,
            )));
        }
    };

    // 2. 打开文件，逐片上传
    let mut file = match std::fs::File::open(local_path) {
        Ok(f) => f,
        Err(e) => {
            // 自动 abort
            let _ = abort_multipart(client, bucket, key, &upload_id);
            return Ok(error_value(format!(
                "s3UploadBigFile() 打开文件 '{}' 失败: {}", local_path, e,
            )));
        }
    };

    let mut parts: Vec<Value> = Vec::with_capacity(total_parts);
    let mut buf = vec![0u8; part_size];

    for part_no in 1..=total_parts as i64 {
        // 读取一片
        let mut read_total = 0usize;
        let target = if part_no as usize == total_parts {
            file_size - (total_parts - 1) * part_size // 最后一片可能不足
        } else {
            part_size
        };
        while read_total < target {
            match file.read(&mut buf[read_total..target]) {
                Ok(0) => break, // EOF
                Ok(n) => read_total += n,
                Err(e) => {
                    let _ = abort_multipart(client, bucket, key, &upload_id);
                    return Ok(error_value(format!(
                        "s3UploadBigFile() 读取文件第 {} 片失败: {}", part_no, e,
                    )));
                }
            }
        }
        if read_total == 0 {
            break;
        }
        let part_data = buf[..read_total].to_vec();

        // 上传分片
        let query = format!("partNumber={}&uploadId={}", part_no, url_encode(&upload_id));
        let part_extra = vec![("Content-Length".to_string(), part_data.len().to_string())];

        match client.request("PUT", bucket, key, &query, part_extra, part_data) {
            Ok(resp) => {
                if resp.status != 200 {
                    let _ = abort_multipart(client, bucket, key, &upload_id);
                    return Ok(http_error("s3UploadBigFile", resp.status, &resp.body));
                }
                let etag = match resp.get_header("etag") {
                    Some(e) => e.to_string(),
                    None => {
                        let _ = abort_multipart(client, bucket, key, &upload_id);
                        return Ok(error_value(format!(
                            "s3UploadBigFile() 第 {} 片响应缺少 ETag", part_no,
                        )));
                    }
                };
                // 收集 part 信息
                let m = crate::object_map::new_map();
                {
                    let mut g = m.lock().unwrap();
                    g.data.insert("partNumber".to_string(), Value::Int(part_no));
                    g.data.insert("etag".to_string(), Value::Str(Arc::from(etag.as_str())));
                }
                parts.push(Value::Object(m));
            }
            Err(e) => {
                let _ = abort_multipart(client, bucket, key, &upload_id);
                return Ok(error_value(format!(
                    "s3UploadBigFile() 上传第 {} 片失败: {}", part_no, e,
                )));
            }
        }
    }

    // 3. 完成 multipart upload
    let complete_xml = build_complete_xml(&parts);
    let complete_body = complete_xml.into_bytes();
    let query = format!("uploadId={}", url_encode(&upload_id));
    let complete_extra = vec![
        ("Content-Type".to_string(), "application/xml".to_string()),
        ("Content-Length".to_string(), complete_body.len().to_string()),
    ];

    match client.request("POST", bucket, key, &query, complete_extra, complete_body) {
        Ok(resp) => {
            if resp.status == 200 {
                Ok(Value::Bool(true))
            } else {
                // complete 失败时 abort 已无意义（upload 已结束或将被服务端清理）
                Ok(http_error("s3UploadBigFile", resp.status, &resp.body))
            }
        }
        Err(e) => {
            let _ = abort_multipart(client, bucket, key, &upload_id);
            Ok(error_value(format!(
                "s3UploadBigFile() 完成 multipart upload 失败: {}", e,
            )))
        }
    }
}

/// abort_multipart 内部辅助函数：取消 multipart upload。
fn abort_multipart(client: &S3Client, bucket: &str, key: &str, upload_id: &str) {
    let query = format!("uploadId={}", url_encode(upload_id));
    let _ = client.request("DELETE", bucket, key, &query, vec![], Vec::new());
}

/// build_complete_xml 构造 CompleteMultipartUpload XML body。
fn build_complete_xml(parts: &[Value]) -> String {
    let mut xml = String::from("<CompleteMultipartUpload>");
    for p in parts {
        if let Value::Object(m) = p {
            let g = m.lock().unwrap();
            let pn = g.data.get("partNumber");
            let et = g.data.get("etag");
            if let (Some(Value::Int(n)), Some(Value::Str(s))) = (pn, et) {
                xml.push_str(&format!(
                    "<Part><PartNumber>{}</PartNumber><ETag>{}</ETag></Part>",
                    n, s,
                ));
            }
        }
    }
    xml.push_str("</CompleteMultipartUpload>");
    xml
}

/// bi_s3_upload_file_internal 内部上传小文件（s3UploadBigFile 回退用）。
fn bi_s3_upload_file_internal(client: &S3Client, bucket: &str, key: &str, local_path: &str) -> Result<Value, Value> {
    let body = match std::fs::read(local_path) {
        Ok(b) => b,
        Err(e) => {
            return Ok(error_value(format!(
                "s3UploadBigFile() 读取本地文件 '{}' 失败: {}", local_path, e,
            )));
        }
    };
    let content_type = guess_content_type(key);
    let extra = vec![("Content-Type".to_string(), content_type)];

    match client.request("PUT", bucket, key, "", extra, body) {
        Ok(resp) => {
            if resp.status == 200 || resp.status == 204 {
                Ok(Value::Bool(true))
            } else {
                Ok(http_error("s3UploadBigFile", resp.status, &resp.body))
            }
        }
        Err(e) => Ok(error_value(format!(
            "s3UploadBigFile() 上传失败: {}", e,
        ))),
    }
}

// ============ 单元测试 ============

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_hash() {
        assert_eq!(hex_hash(&[]), "");
        assert_eq!(hex_hash(&[0]), "00");
        assert_eq!(hex_hash(&[0xff]), "ff");
        assert_eq!(hex_hash(&[0x12, 0xab]), "12ab");
    }

    #[test]
    fn test_url_encode() {
        assert_eq!(url_encode("hello"), "hello");
        assert_eq!(url_encode("a/b"), "a/b");
        assert_eq!(url_encode("hello world"), "hello%20world");
        assert_eq!(url_encode("中文"), "%E4%B8%AD%E6%96%87");
        assert_eq!(url_encode("a+b=c"), "a%2Bb%3Dc");
    }

    #[test]
    fn test_canonical_query_string() {
        assert_eq!(canonical_query_string(""), "");
        assert_eq!(canonical_query_string("b=2&a=1"), "a=1&b=2");
        assert_eq!(canonical_query_string("max-keys=1000&prefix=logs"), "max-keys=1000&prefix=logs");
    }

    #[test]
    fn test_canonical_uri() {
        assert_eq!(canonical_uri(""), "/");
        assert_eq!(canonical_uri("/"), "/");
        assert_eq!(canonical_uri("/bucket/key"), "/bucket/key");
        assert_eq!(canonical_uri("/a b/c"), "/a%20b/c");
    }

    #[test]
    fn test_extract_xml_values() {
        let xml = r#"<Root><Item><Name>a</Name></Item><Item><Name>b</Name></Item></Root>"#;
        let names = extract_xml_values(xml, "Name");
        assert_eq!(names, vec!["a", "b"]);

        let empty = extract_xml_values("<Root></Root>", "Name");
        assert!(empty.is_empty());
    }

    #[test]
    fn test_ensure_header() {
        let mut h = vec![("Host".to_string(), "example.com".to_string())];
        ensure_header(&mut h, "host", "new.com".to_string());
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].0, "host");
        assert_eq!(h[0].1, "new.com");
    }

    #[test]
    fn test_epoch_to_iso8601_basic() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        assert_eq!(epoch_to_iso8601_basic(1704067200), "20240101T000000Z");
        // 1970-01-01 00:00:00 UTC = 0
        assert_eq!(epoch_to_iso8601_basic(0), "19700101T000000Z");
        // 2024-07-13 12:34:56 UTC = 1720874096
        assert_eq!(epoch_to_iso8601_basic(1720874096), "20240713T123456Z");
    }

    #[test]
    fn test_is_leap_year() {
        assert!(is_leap_year(2000));
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2023));
    }

    #[test]
    fn test_guess_content_type() {
        assert_eq!(guess_content_type("a.html"), "text/html; charset=utf-8");
        assert_eq!(guess_content_type("a.txt"), "text/plain; charset=utf-8");
        assert_eq!(guess_content_type("a.json"), "application/json");
        assert_eq!(guess_content_type("a.png"), "image/png");
        assert_eq!(guess_content_type("a.unknown"), "application/octet-stream");
        assert_eq!(guess_content_type("noext"), "application/octet-stream");
    }

    #[test]
    fn test_to_bytes_v() {
        let s = Value::Str(Arc::from("hello"));
        assert_eq!(to_bytes_v(&s, "test").unwrap(), b"hello");

        let b = Value::Bytes(Arc::new(b"world".to_vec()));
        assert_eq!(to_bytes_v(&b, "test").unwrap(), b"world");

        let i = Value::Int(42);
        assert!(to_bytes_v(&i, "test").is_err());
    }

    #[test]
    fn test_client_downcast_type_check() {
        // 非 Native 类型应失败
        let v = Value::Int(42);
        assert!(client_downcast(&v, "test").is_err());

        let v = Value::Undefined;
        assert!(client_downcast(&v, "test").is_err());

        // 错误信息应包含函数名
        let v = Value::Int(42);
        let err = client_downcast(&v, "s3Test").unwrap_err();
        match err {
            Value::Error(e) => {
                assert!(e.message.contains("s3Test"));
            }
            _ => panic!("应为 Error 类型"),
        }
    }

    #[test]
    fn test_s3_client_build_url_path_style() {
        let client = S3Client {
            endpoint: "http://localhost:9000".to_string(),
            region: "us-east-1".to_string(),
            ak: "".to_string(),
            sk: "".to_string(),
            path_style: true,
            use_https: false,
            host: "localhost".to_string(),
            port: 9000,
            timeout_secs: 30,
        };
        assert_eq!(client.build_url("bucket", "key", ""), "http://localhost:9000/bucket/key");
        assert_eq!(client.build_url("bucket", "", ""), "http://localhost:9000/bucket");
        assert_eq!(client.build_url("bucket", "key", "prefix=logs"), "http://localhost:9000/bucket/key?prefix=logs");
    }

    #[test]
    fn test_s3_client_build_url_virtual_hosted() {
        let client = S3Client {
            endpoint: "https://s3.us-east-1.amazonaws.com".to_string(),
            region: "us-east-1".to_string(),
            ak: "".to_string(),
            sk: "".to_string(),
            path_style: false,
            use_https: true,
            host: "s3.us-east-1.amazonaws.com".to_string(),
            port: 443,
            timeout_secs: 30,
        };
        assert_eq!(client.build_url("bucket", "key", ""), "https://bucket.s3.us-east-1.amazonaws.com/key");
        assert_eq!(client.build_url("", "", ""), "https://s3.us-east-1.amazonaws.com");
    }

    #[test]
    fn test_s3_client_sigv4_sign_empty_body() {
        let mut headers = vec![
            ("host".to_string(), "example.com".to_string()),
        ];
        let client = S3Client {
            endpoint: "http://localhost:9000".to_string(),
            region: "us-east-1".to_string(),
            ak: "AKIAIOSFODNN7EXAMPLE".to_string(),
            sk: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string(),
            path_style: true,
            use_https: false,
            host: "localhost".to_string(),
            port: 9000,
            timeout_secs: 30,
        };
        let auth = client.sigv4_sign("GET", "/bucket", "", &mut headers, &[], "20240101T000000Z");
        assert!(auth.starts_with("AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20240101/us-east-1/s3/aws4_request"));
        assert!(auth.contains("SignedHeaders=host;x-amz-content-sha256;x-amz-date"));
        // 签名应是 64 位十六进制
        let sig = auth.rsplit("Signature=").next().unwrap();
        assert_eq!(sig.len(), 64);
        assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_build_complete_xml() {
        // 空列表
        let xml = build_complete_xml(&[]);
        assert_eq!(xml, "<CompleteMultipartUpload></CompleteMultipartUpload>");

        // 单个分片
        let mk_part = |no: i64, etag: &str| -> Value {
            let m = crate::object_map::new_map();
            {
                let mut g = m.lock().unwrap();
                g.data.insert("partNumber".to_string(), Value::Int(no));
                g.data.insert("etag".to_string(), Value::Str(Arc::from(etag)));
            }
            Value::Object(m)
        };

        let parts = vec![mk_part(1, "\"abc123\"")];
        let xml = build_complete_xml(&parts);
        assert_eq!(xml, "<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>\"abc123\"</ETag></Part></CompleteMultipartUpload>");

        // 多个分片
        let parts = vec![mk_part(1, "\"abc123\""), mk_part(2, "\"def456\"")];
        let xml = build_complete_xml(&parts);
        assert!(xml.contains("<PartNumber>1</PartNumber>"));
        assert!(xml.contains("<PartNumber>2</PartNumber>"));
        assert!(xml.contains("\"abc123\""));
        assert!(xml.contains("\"def456\""));
    }

    #[test]
    fn test_build_complete_xml_skips_invalid() {
        // 非 Object 元素应被跳过（不崩溃）
        let parts = vec![Value::Int(42), Value::Undefined];
        let xml = build_complete_xml(&parts);
        assert_eq!(xml, "<CompleteMultipartUpload></CompleteMultipartUpload>");
    }

    #[test]
    fn test_extract_xml_upload_id() {
        // 模拟 S3 CreateMultipartUpload 响应
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<InitiateMultipartUploadResult>
    <Bucket>example-bucket</Bucket>
    <Key>example-object</Key>
    <UploadId>VXBsb2FkIElEIGZvciA2aWWpbmcncyBtebVzaWM</UploadId>
</InitiateMultipartUploadResult>"#;
        let ids = extract_xml_values(xml, "UploadId");
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], "VXBsb2FkIElEIGZvciA2aWWpbmcncyBtebVzaWM");
    }
}
