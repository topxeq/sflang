//! builtins_email.rs — 邮件发送内置函数（基于 lettre）
//!
//! 支持 SMTP 明文/TLS/STARTTLS，纯文本和 HTML 邮件。
//!
//! 函数：
//!   sendMail("--host=smtp.example.com", "--port=465",
//!            "--user=user@example.com", "--password=pass",
//!            "--from=user@example.com", "--to=dest@example.com",
//!            "--subject=标题", "--body=正文内容",
//!            "--html",                    // 可选：HTML 邮件
//!            "--ssl",                     // 可选：隐式 TLS (端口 465)
//!            "--starttls")                // 可选：STARTTLS (端口 587)

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

    if host.is_empty() || from.is_empty() || to.is_empty() {
        return Ok(crate::value::error_value(
            "sendMail() 至少需要 --host, --from, --to 参数",
        ));
    }

    // 构建邮件
    let email = Message::builder()
        .from(from.parse().unwrap_or_else(|_| "noreply@example.com".parse().unwrap()))
        .to(to.parse().unwrap_or_else(|_| "noreply@example.com".parse().unwrap()))
        .subject(subject)
        .header(lettre::message::header::ContentType::parse(
            if is_html { "text/html; charset=utf-8" } else { "text/plain; charset=utf-8" }
        ).unwrap())
        .body(body)
        .map_err(|e| crate::value::error_value(format!("sendMail() 构建邮件失败: {}", e)))?;

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
