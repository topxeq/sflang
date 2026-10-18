//! Cookie 会话管理（HTTP Session）。
//!
//! 提供跨请求的 Cookie Jar：`httpSession()` 创建会话对象，getWeb/postWeb/postForm
//! 通过 `--session=` 传入（位置参数形态），请求前按域名/路径自动附带 Cookie，
//! 响应后把 Set-Cookie 存入 Jar，实现登录态保持等场景。
//!
//! 实现为 RFC6265 精简版：
//! - 域匹配：host-only（Set-Cookie 未带 Domain，精确匹配主机）或后缀匹配（带 Domain）
//! - 路径匹配：按 RFC6265 §5.1.4 的路径前缀规则
//! - 有效期：支持 Max-Age（整数秒）；Expires 支持 RFC1123 GMT 格式（best-effort）
//! - Secure 标记：仅 HTTPS 请求发送
//! - 过期 Cookie 在每次访问 Jar 时清除

use std::sync::Mutex;

/// CookieAction 单条 Set-Cookie 的解析动作结果。
enum CookieAction {
    /// Store 正常入库。
    Store(CookieEntry),
    /// Delete 删除既有 Cookie（Max-Age<=0 或已过期的 Expires）。
    Delete { domain: String, path: String, name: String },
    /// Ignore 无效或被拒绝（域校验失败等）。
    Ignore,
}

/// CookieEntry 单条 Cookie。
#[derive(Clone, Debug)]
pub struct CookieEntry {
    /// domain 存储域（小写，无前导点）。
    pub domain: String,
    /// host_only 是否仅精确匹配该主机（Set-Cookie 未带 Domain 属性时为 true）。
    pub host_only: bool,
    /// path 路径前缀。
    pub path: String,
    /// name Cookie 名。
    pub name: String,
    /// value Cookie 值。
    pub value: String,
    /// expires_epoch 过期 Unix 秒；None = 会话 Cookie（随 Jar 生命周期）。
    pub expires_epoch: Option<i64>,
    /// secure_only 仅 HTTPS 请求发送。
    pub secure_only: bool,
}

/// HttpSession Cookie 会话（跨请求共享的 Cookie Jar）。
pub struct HttpSession {
    /// cookies 当前存储的 Cookie 列表。
    pub cookies: Mutex<Vec<CookieEntry>>,
}

impl HttpSession {
    /// new 创建空会话。
    pub fn new() -> Self {
        HttpSession { cookies: Mutex::new(Vec::new()) }
    }

    /// now_epoch 当前 Unix 秒。
    fn now_epoch() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    /// purge_expired 清除已过期的 Cookie。
    fn purge_expired(&self) {
        let now = Self::now_epoch();
        self.cookies.lock().unwrap().retain(|c| c.expires_epoch.map(|e| e > now).unwrap_or(true));
    }

    /// domain_matches 域匹配（RFC6265 §5.1.3 精简版）。
    fn domain_matches(host: &str, cookie: &CookieEntry) -> bool {
        let host = host.to_ascii_lowercase();
        if cookie.host_only {
            return host == cookie.domain;
        }
        host == cookie.domain || host.ends_with(&format!(".{}", cookie.domain))
    }

    /// path_matches 路径匹配（RFC6265 §5.1.4）。
    fn path_matches(req_path: &str, cookie_path: &str) -> bool {
        if req_path == cookie_path {
            return true;
        }
        if req_path.starts_with(cookie_path) {
            // cookie_path 以 / 结尾，或其后第一个字符是 /，则算前缀命中
            if cookie_path.ends_with('/') {
                return true;
            }
            return req_path.as_bytes().get(cookie_path.len()) == Some(&b'/');
        }
        false
    }

    /// cookie_header_for 生成请求应携带的 Cookie 头值；无匹配 Cookie 时返回 None。
    pub fn cookie_header_for(&self, scheme: &str, host: &str, path: &str) -> Option<String> {
        self.purge_expired();
        let jar = self.cookies.lock().unwrap();
        let pairs: Vec<String> = jar
            .iter()
            .filter(|c| {
                if c.secure_only && !scheme.eq_ignore_ascii_case("https") {
                    return false;
                }
                Self::domain_matches(host, c) && Self::path_matches(path, &c.path)
            })
            .map(|c| format!("{}={}", c.name, c.value))
            .collect();
        if pairs.is_empty() {
            None
        } else {
            Some(pairs.join("; "))
        }
    }

    /// store_from_response 解析响应中的 Set-Cookie 头并存入 Jar。
    ///
    /// set_cookies 为 (名, 值) 形式的原始 Set-Cookie 头列表（同名头可重复出现）。
    pub fn store_from_response(&self, scheme: &str, host: &str, req_path: &str, set_cookies: &[String]) {
        if set_cookies.is_empty() {
            return;
        }
        self.purge_expired();
        let mut jar = self.cookies.lock().unwrap();
        for raw in set_cookies {
            match Self::parse_set_cookie(raw, host, req_path) {
                CookieAction::Store(entry) => {
                    // 同 (domain, path, name) 覆盖旧值
                    jar.retain(|c| !(c.domain == entry.domain && c.path == entry.path && c.name == entry.name));
                    jar.push(entry);
                }
                CookieAction::Delete { domain, path, name } => {
                    // Max-Age<=0 / 已过期的 Expires：按 RFC6265 语义删除既有 Cookie
                    jar.retain(|c| !(c.domain == domain && c.path == path && c.name == name));
                }
                CookieAction::Ignore => {}
            }
        }
        let _ = scheme; // scheme 仅用于 secure 标记匹配，不参与存储
    }

    /// parse_set_cookie 解析单条 Set-Cookie 值的动作结果。
    ///
    /// Store：正常入库；Delete：Max-Age<=0 或已过期的 Expires（RFC6265 删除语义）；
    /// Ignore：无效或域校验失败（防 Cookie 注入）。
    fn parse_set_cookie(raw: &str, req_host: &str, req_path: &str) -> CookieAction {
        let mut parts = raw.split(';');
        let nv = match parts.next() {
            Some(nv) => nv.trim(),
            None => return CookieAction::Ignore,
        };
        let (name, value) = match nv.split_once('=') {
            Some((n, v)) => (n.trim().to_string(), v.trim().to_string()),
            None => return CookieAction::Ignore, // 无 = 的 Set-Cookie 视为无效
        };
        if name.is_empty() {
            return CookieAction::Ignore;
        }

        let mut domain = req_host.to_ascii_lowercase();
        let mut host_only = true;
        let mut path = String::new();
        let mut expires_epoch: Option<i64> = None;
        let mut secure_only = false;
        let mut delete = false;

        for attr in parts {
            let attr = attr.trim();
            let (lower_val, attr_val) = match attr.split_once('=') {
                Some((k, v)) => (k.trim().to_ascii_lowercase(), Some(v.trim())),
                None => (attr.to_ascii_lowercase(), None),
            };
            match lower_val.as_str() {
                "domain" => {
                    if let Some(d) = attr_val {
                        let d = d.trim_start_matches('.').to_ascii_lowercase();
                        if !d.is_empty() {
                            // 请求主机必须是该域或其子域，否则拒绝（防 Cookie 注入）
                            let host_l = req_host.to_ascii_lowercase();
                            if host_l == d || host_l.ends_with(&format!(".{}", d)) {
                                domain = d;
                                host_only = false;
                            } else {
                                return CookieAction::Ignore;
                            }
                        }
                    }
                }
                "path" => {
                    if let Some(pp) = attr_val {
                        let p = pp.trim();
                        if p.starts_with('/') {
                            path = p.to_string();
                        }
                    }
                }
                "secure" => secure_only = true,
                "max-age" => {
                    if let Some(v) = attr_val {
                        if let Ok(secs) = v.parse::<i64>() {
                            if secs <= 0 {
                                delete = true; // 立即删除既有 Cookie
                            } else {
                                expires_epoch = Some(Self::now_epoch() + secs);
                            }
                        }
                    }
                }
                "expires" => {
                    if let Some(v) = attr_val {
                        if let Some(epoch) = parse_http_date_gmt(v) {
                            if epoch <= Self::now_epoch() {
                                delete = true; // 已过期 → 删除既有 Cookie
                            } else {
                                expires_epoch = Some(epoch);
                            }
                        }
                        // 无法识别的日期格式按会话 Cookie 处理（best-effort）
                    }
                }
                _ => {} // httponly/samesite/partitioned 等忽略
            }
        }

        // 默认路径：请求路径去掉最后一段（含末尾斜杠）；无法取目录时为 "/"
        if path.is_empty() {
            match req_path.rfind('/') {
                Some(0) => path = "/".to_string(),
                Some(idx) => path = req_path[..idx].to_string(),
                None => path = "/".to_string(),
            }
        }

        if delete {
            return CookieAction::Delete { domain, path, name };
        }
        CookieAction::Store(CookieEntry {
            domain,
            host_only,
            path,
            name,
            value,
            expires_epoch,
            secure_only,
        })
    }

    /// clear 清空 Jar。
    pub fn clear(&self) {
        self.cookies.lock().unwrap().clear();
    }

    /// dump 返回当前 Cookie 快照（供 httpSessionDump 内置函数输出）。
    pub fn dump(&self) -> Vec<CookieEntry> {
        self.purge_expired();
        self.cookies.lock().unwrap().clone()
    }
}

/// parse_http_date_gmt 解析 RFC1123 GMT 日期（如 "Wed, 21 Oct 2015 07:28:00 GMT"）为 Unix 秒。
///
/// 仅支持这一种最常见格式；解析失败返回 None（调用方按会话 Cookie 处理）。
fn parse_http_date_gmt(s: &str) -> Option<i64> {
    let s = s.trim();
    // 跳过 "Wed, " 星期前缀（存在与否都接受）
    let rest = match s.split_once(", ") {
        Some((_, r)) => r.trim(),
        None => s,
    };
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    // 期望：[dd] Mon yyyy HH:MM:SS GMT
    if tokens.len() < 4 {
        return None;
    }
    let day: i64 = tokens[0].parse().ok()?;
    let month = match tokens[1].to_ascii_lowercase().as_str() {
        "jan" => 1, "feb" => 2, "mar" => 3, "apr" => 4, "may" => 5, "jun" => 6,
        "jul" => 7, "aug" => 8, "sep" => 9, "oct" => 10, "nov" => 11, "dec" => 12,
        _ => return None,
    };
    let year: i64 = tokens[2].parse().ok()?;
    let hms: Vec<&str> = tokens[3].split(':').collect();
    if hms.len() != 3 {
        return None;
    }
    let hour: i64 = hms[0].parse().ok()?;
    let minute: i64 = hms[1].parse().ok()?;
    let second: i64 = hms[2].parse().ok()?;

    // civil days → Unix 秒（ Howard Hinnant 算法，纯整数实现，避免引入时间库）
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + hour * 3600 + minute * 60 + second)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_basic_flow() {
        let s = HttpSession::new();
        s.store_from_response(
            "https", "api.example.com", "/login",
            &["sid=abc123; Path=/; HttpOnly".to_string()],
        );
        let h = s.cookie_header_for("https", "api.example.com", "/me");
        assert_eq!(h, Some("sid=abc123".to_string()));

        // 子路径命中
        assert_eq!(s.cookie_header_for("https", "api.example.com", "/a/b/c"), Some("sid=abc123".to_string()));
        // 其他域不命中
        assert_eq!(s.cookie_header_for("https", "other.com", "/me"), None);
        // 纯 HTTP 不命中 Secure Cookie
        let s2 = HttpSession::new();
        s2.store_from_response("https", "a.com", "/", &["t=1; Secure".to_string()]);
        assert_eq!(s2.cookie_header_for("http", "a.com", "/x"), None);
        assert_eq!(s2.cookie_header_for("https", "a.com", "/x"), Some("t=1".to_string()));
    }

    #[test]
    fn test_session_max_age_and_overwrite() {
        let s = HttpSession::new();
        s.store_from_response("http", "h.com", "/a/b", &["k=v1; Path=/; Max-Age=100".to_string()]);
        s.store_from_response("http", "h.com", "/a/b", &["k=v2; Path=/; Max-Age=100".to_string()]);
        assert_eq!(s.cookie_header_for("http", "h.com", "/a/c"), Some("k=v2".to_string()));

        // Max-Age=0 立即删除（不入库）
        s.store_from_response("http", "h.com", "/a/b", &["k=v2; Path=/; Max-Age=0".to_string()]);
        assert_eq!(s.cookie_header_for("http", "h.com", "/a/c"), None);
    }

    #[test]
    fn test_session_domain_suffix() {
        let s = HttpSession::new();
        // 带 Domain → 后缀匹配（非 host-only）
        s.store_from_response("http", "sub.example.com", "/", &["d=1; Domain=example.com".to_string()]);
        assert_eq!(s.cookie_header_for("http", "sub.example.com", "/x"), Some("d=1".to_string()));
        assert_eq!(s.cookie_header_for("http", "example.com", "/x"), Some("d=1".to_string()));
        // 无关域拒绝
        s.store_from_response("http", "a.com", "/", &["bad=1; Domain=other.com".to_string()]);
        assert_eq!(s.cookie_header_for("http", "a.com", "/x"), None);
    }

    #[test]
    fn test_http_date_parse() {
        // 2015-10-21 07:28:00 GMT = 1445412480
        assert_eq!(parse_http_date_gmt("Wed, 21 Oct 2015 07:28:00 GMT"), Some(1445412480));
        assert_eq!(parse_http_date_gmt("not a date"), None);
    }
}
