//! builtins_email.rs — 邮件发送内置函数（基于 lettre）
//!
//! 支持 SMTP 明文/TLS/STARTTLS，纯文本和 HTML 邮件，附件。
//!
//! 函数：
//!   sendMail("--host=smtp.example.com", "--port=465",
//!            "--user=user@example.com", "--password=pass",
//!            "--from=user@example.com", "--to=dest@example.com",
//!            "--subject=标题", "--body=正文内容",
//!            "--html",                    // 可选：HTML 邮件
//!            "--ssl",                     // 可选：隐式 TLS (端口 465)
//!            "--starttls",                // 可选：STARTTLS (端口 587)
//!            "--attach=/path/to/file.pdf")  // 可选：附件（可重复多个）

use crate::value::Value;
use crate::vm::VM;

pub fn register(vm: &mut VM) {
    vm.register_builtin("sendMail", bi_send_mail);
}

fn get_switch(args: &[Value], key: &str, default: &str) -> String {
    let p1 = format!("--{}=", key);
    let p2 = format!("-{}=", key);
    for arg in args {
        if let Value::Str(s) = arg {
            if let Some(rest) = s.strip_prefix(&p1).or_else(|| s.strip_prefix(&p2)) {
                return rest.to_string();
            }
        }
    }
    default.to_string()
}

fn has_switch(args: &[Value], key: &str) -> bool {
    let p1 = format!("--{}", key);
    let p2 = format!("-{}", key);
    args.iter().any(|arg| {
        if let Value::Str(s) = arg { &**s == p1 || &**s == p2 }
        else { false }
    })
}

/// get_all_switches 获取所有 --key=value 的值（用于多个附件）。
fn get_all_switches(args: &[Value], key: &str) -> Vec<String> {
    let p1 = format!("--{}=", key);
    let p2 = format!("-{}=", key);
    let mut result = Vec::new();
    for arg in args {
        if let Value::Str(s) = arg {
            if let Some(rest) = s.strip_prefix(&p1).or_else(|| s.strip_prefix(&p2)) {
                result.push(rest.to_string());
            }
        }
    }
    result
}

fn bi_send_mail(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{Message, SmtpTransport, Transport};

    let host = get_switch(args, "host", "");
    let port: u16 = get_switch(args, "port", "587").parse().unwrap_or(587);
    let user = get_switch(args, "user", "");
    let password = get_switch(args, "password", "");
    let from = get_switch(args, "from", "");
    let to = get_switch(args, "to", "");
    let subject = get_switch(args, "subject", "(No Subject)");
    let body = get_switch(args, "body", "");
    let is_html = has_switch(args, "html");
    let use_ssl = has_switch(args, "ssl");
    let use_starttls = has_switch(args, "starttls");
    let attachments = get_all_switches(args, "attach");

    if host.is_empty() || from.is_empty() || to.is_empty() {
        return Ok(crate::value::error_value(
            "sendMail() 至少需要 --host, --from, --to 参数",
        ));
    }

    // 构建邮件
    let mut builder = Message::builder()
        .from(from.parse().unwrap_or_else(|_| "noreply@example.com".parse().unwrap()))
        .to(to.parse().unwrap_or_else(|_| "noreply@example.com".parse().unwrap()))
        .subject(subject);

    // 如果没有附件，用简单 body
    // 如果有附件，用 multipart
    let email = if attachments.is_empty() {
        // 纯文本或 HTML
        builder = builder.header(lettre::message::header::ContentType::parse(
            if is_html { "text/html; charset=utf-8" } else { "text/plain; charset=utf-8" }
        ).unwrap());
        builder.body(body)
    } else {
        // 有附件：构建 multipart
        let content_type = lettre::message::header::ContentType::parse(
            if is_html { "text/html; charset=utf-8" } else { "text/plain; charset=utf-8" }
        ).unwrap();

        // 构建 multipart body
        let mut multipart = lettre::message::MultiPart::mixed().build();

        // 正文部分
        let text_part = lettre::message::SinglePart::builder()
            .header(content_type)
            .body(body);
        multipart = multipart.singlepart(text_part);

        // 附件部分
        for file_path in &attachments {
            let data = match std::fs::read(file_path) {
                Ok(d) => d,
                Err(e) => return Ok(crate::value::error_value(format!(
                    "sendMail() 读取附件 '{}' 失败: {}", file_path, e,
                ))),
            };
            let file_name = std::path::Path::new(file_path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "attachment".to_string());

            let mime_str = guess_mime_type(&file_name);
            let ct = lettre::message::header::ContentType::parse(&mime_str)
                .unwrap_or_else(|_| lettre::message::header::ContentType::parse("application/octet-stream").unwrap());

            let attachment = lettre::message::Attachment::new(file_name).body(data, ct);
            multipart = multipart.singlepart(attachment);
        }

        builder.multipart(multipart)
    };

    let email = match email {
        Ok(e) => e,
        Err(e) => return Ok(crate::value::error_value(format!(
            "sendMail() 构建邮件失败: {}", e,
        ))),
    };

    // 构建 SMTP transport
    let transport = if use_ssl {
        let mut builder = SmtpTransport::relay(&host)
            .map_err(|e| crate::value::error_value(format!(
                "sendMail() 创建 TLS 连接失败: {} (可能原因：网络不通)", e,
            )))?;
        builder = builder.port(port);
        if !user.is_empty() {
            builder = builder.credentials(Credentials::new(user, password));
        }
        builder.build()
    } else if use_starttls || port == 587 {
        let mut builder = SmtpTransport::starttls_relay(&host)
            .map_err(|e| crate::value::error_value(format!(
                "sendMail() 创建 STARTTLS 连接失败: {}", e,
            )))?;
        builder = builder.port(port);
        if !user.is_empty() {
            builder = builder.credentials(Credentials::new(user, password));
        }
        builder.build()
    } else {
        let mut builder = SmtpTransport::builder_dangerous(&host);
        builder = builder.port(port);
        if !user.is_empty() {
            builder = builder.credentials(Credentials::new(user, password));
        }
        builder.build()
    };

    // 发送
    match transport.send(&email) {
        Ok(_) => Ok(Value::Undefined),
        Err(e) => Ok(crate::value::error_value(format!(
            "sendMail() 发送失败: {} (可能原因：认证失败、网络问题、收件人地址错误)", e,
        ))),
    }
}

/// guess_mime_type 根据文件扩展名猜测 MIME 类型。
fn guess_mime_type(file_name: &str) -> String {
    let ext = std::path::Path::new(file_name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" | "tgz" => "application/gzip",
        "tar" => "application/x-tar",
        "7z" => "application/x-7z-compressed",
        "rar" => "application/vnd.rar",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "txt" | "log" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "xml" => "text/xml",
        "json" => "application/json",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "avi" => "video/x-msvideo",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }.to_string()
}
