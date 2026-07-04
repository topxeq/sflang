//! datetime.rs — 日期时间类型（纯标准库实现，无第三方依赖）
//!
//! 设计要点：
//!   - 内部存 Unix 毫秒（i64，UTC）+ 时区偏移（i32 分钟）
//!   - 历法换算用 Howard Hinnant 的 O(1) 整数算法（days_from_civil/civil_from_days），
//!     纯整数运算，正确处理公历闰年，无第三方依赖
//!   - 格式化/解析用 Go 风格参考时间 "2006-01-02 15:04:05.999 -07:00"
//!   - 不可变：加减运算返回新 DateTime
//!
//! 算法来源：Howard Hinnant "date" 算法（public domain），经广泛验证。

/// DateTime 日期时间值。
///
/// 内部表示：
///   - millis: Unix 毫秒（UTC，1970-01-01 00:00:00 UTC 起的毫秒数）
///   - tz_offset: 时区偏移（分钟，相对 UTC）。如北京 +480（东八区），UTC 为 0。
///
/// 字段访问（year/month/day/hour/minute/second/millis/weekday）按 tz_offset 换算后给出。
#[derive(Debug, Clone)]
pub struct DateTime {
    /// millis Unix 毫秒（UTC）。
    pub millis: i64,
    /// tz_offset 时区偏移（分钟，相对 UTC）。
    pub tz_offset: i32,
}

/// MILLIS_PER_DAY 每天的毫秒数。
const MILLIS_PER_DAY: i64 = 86_400_000;
/// MILLIS_PER_HOUR 每小时毫秒数。
const MILLIS_PER_HOUR: i64 = 3_600_000;
/// MILLIS_PER_MINUTE 每分钟毫秒数。
const MILLIS_PER_MINUTE: i64 = 60_000;

impl DateTime {
    /// from_millis_utc 从 Unix 毫秒构造（UTC，tz=0）。
    pub fn from_millis_utc(millis: i64) -> Self {
        DateTime { millis, tz_offset: 0 }
    }

    /// from_millis_with_tz 从 Unix 毫秒 + 时区偏移构造。
    pub fn from_millis_with_tz(millis: i64, tz_offset: i32) -> Self {
        DateTime { millis, tz_offset }
    }

    /// from_components 从年月日时分秒构造（公历）。
    ///
    /// tz_offset 为时区偏移（分钟）。秒可为小数（含毫秒），但此处取整毫秒。
    pub fn from_components(year: i32, month: i32, day: i32, hour: i32, min: i32, sec: i32, millis: i32, tz_offset: i32) -> Option<Self> {
        // 校验范围
        if !(1..=12).contains(&month) { return None; }
        if !(1..=31).contains(&day) { return None; }
        if !(0..=23).contains(&hour) { return None; }
        if !(0..=59).contains(&min) { return None; }
        if !(0..=59).contains(&sec) { return None; }
        if !(0..=999).contains(&millis) { return None; }
        // 校验 day 对 month/year 合法性（含闰年）
        let days_in_month = days_in_month(year, month);
        if day > days_in_month { return None; }
        // 算 UTC 毫秒：先算本地天数，转 UTC 天数 + 时间毫秒，再减时区偏移
        let days = days_from_civil(year, month, day);
        let local_millis = days * MILLIS_PER_DAY
            + hour as i64 * MILLIS_PER_HOUR
            + min as i64 * MILLIS_PER_MINUTE
            + sec as i64 * 1000
            + millis as i64;
        // 本地毫秒 - 时区偏移 = UTC 毫秒
        let utc_millis = local_millis - (tz_offset as i64) * MILLIS_PER_MINUTE;
        Some(DateTime { millis: utc_millis, tz_offset })
    }

    /// now 当前时间（本地时区）。
    pub fn now() -> Self {
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        // 本地时区偏移：用当前本地时间与 UTC 的差估算
        let tz_offset = local_tz_offset_minutes();
        DateTime { millis, tz_offset }
    }

    /// year 年（按 tz_offset）。
    pub fn year(&self) -> i32 {
        let (y, _, _) = self.date_part();
        y
    }
    /// month 月（1-12）。
    pub fn month(&self) -> i32 {
        let (_, m, _) = self.date_part();
        m
    }
    /// day 日（1-31）。
    pub fn day(&self) -> i32 {
        let (_, _, d) = self.date_part();
        d
    }
    /// hour 时（0-23）。
    pub fn hour(&self) -> i32 {
        (self.local_millis() / MILLIS_PER_HOUR % 24) as i32
    }
    /// minute 分（0-59）。
    pub fn minute(&self) -> i32 {
        (self.local_millis() / MILLIS_PER_MINUTE % 60) as i32
    }
    /// second 秒（0-59）。
    pub fn second(&self) -> i32 {
        (self.local_millis() / 1000 % 60) as i32
    }
    /// millis 毫秒部分（0-999）。
    pub fn millis_part(&self) -> i32 {
        (self.local_millis() % 1000) as i32
    }
    /// weekday 星期几（0=周日，1=周一...6=周六；对齐 Go）。
    pub fn weekday(&self) -> i32 {
        // 1970-01-01 是周四（weekday=4）。days_from_civil(1970,1,1)=719468
        let days = self.local_millis().div_euclid(MILLIS_PER_DAY);
        // 4 + days mod 7，规范到 0..7
        let w = (4 + days.rem_euclid(7)) % 7;
        w as i32
    }

    /// local_millis 按时区偏移换算后的本地毫秒数。
    fn local_millis(&self) -> i64 {
        self.millis + (self.tz_offset as i64) * MILLIS_PER_MINUTE
    }

    /// date_part 算 (year, month, day)。用 civil_from_days。
    fn date_part(&self) -> (i32, i32, i32) {
        let days = self.local_millis().div_euclid(MILLIS_PER_DAY);
        civil_from_days(days)
    }

    /// add_millis 加毫秒，返回新 DateTime（时区不变）。
    pub fn add_millis(&self, n: i64) -> Self {
        DateTime { millis: self.millis + n, tz_offset: self.tz_offset }
    }
    /// add_seconds 加秒。
    pub fn add_seconds(&self, n: i64) -> Self {
        self.add_millis(n * 1000)
    }
    /// add_days 加天。
    pub fn add_days(&self, n: i64) -> Self {
        self.add_millis(n * MILLIS_PER_DAY)
    }

    /// to_millis 转 Unix 毫秒（UTC，int）。
    pub fn to_millis(&self) -> i64 {
        self.millis
    }

    /// format 按 Go 风格参考时间格式化。
    ///
    /// 支持的占位符（Go 参考时间 2006-01-02 15:04:05.999 -0700）：
    ///   2006→年(4位)  01→月  02→日  15→时(24h)  04→分  05→秒
    ///   .999→毫秒(去尾零)  .000→毫秒(3位)  -0700→时区偏移
    pub fn format(&self, fmt: &str) -> String {
        let y = self.year();
        let mo = self.month();
        let d = self.day();
        let h = self.hour();
        let mi = self.minute();
        let s = self.second();
        let ms = self.millis_part();
        let wd = self.weekday();
        let tz_min = self.tz_offset;
        let mut out = String::with_capacity(fmt.len() + 8);
        let bytes = fmt.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            // 尝试匹配各占位符（按长度降序匹配，避免前缀误中）
            let rest = &fmt[i..];
            let matched = if rest.starts_with("2006") {
                out.push_str(&format!("{:04}", y)); Some(4)
            } else if rest.starts_with("-0700") {
                let sign = if tz_min >= 0 { '+' } else { '-' };
                let abs = tz_min.unsigned_abs() as i32;
                out.push_str(&format!("{}{:02}{:02}", sign, abs / 60, abs % 60));
                Some(5)
            } else if rest.starts_with(".999") {
                if ms > 0 { out.push_str(&format!(".{:03}", ms).trim_end_matches('0')); } 
                Some(4)
            } else if rest.starts_with(".000") {
                out.push_str(&format!(".{:03}", ms)); Some(4)
            } else if rest.starts_with("01") {
                out.push_str(&format!("{:02}", mo)); Some(2)
            } else if rest.starts_with("02") {
                out.push_str(&format!("{:02}", d)); Some(2)
            } else if rest.starts_with("15") {
                out.push_str(&format!("{:02}", h)); Some(2)
            } else if rest.starts_with("04") {
                out.push_str(&format!("{:02}", mi)); Some(2)
            } else if rest.starts_with("05") {
                out.push_str(&format!("{:02}", s)); Some(2)
            } else if rest.starts_with("Monday") {
                out.push_str(weekday_name(wd, true)); Some(6)
            } else if rest.starts_with("Jan") {
                out.push_str(month_name(mo)); Some(3)
            } else {
                None
            };
            match matched {
                Some(n) => i += n,
                None => { out.push(bytes[i] as char); i += 1; }
            }
        }
        out
    }

    /// inspect 用于打印/调试的可读表示。
    pub fn inspect(&self) -> String {
        if self.tz_offset == 0 {
            self.format("2006-01-02 15:04:05.000 UTC")
        } else {
            self.format("2006-01-02 15:04:05.000 -0700")
        }
    }
}

/// days_from_civil 公历年月日 → Unix 天数（Howard Hinnant 算法，O(1)）。
///
/// 1970-01-01 对应 719468。正确处理闰年（公历规则：4年闰/100年不闰/400年闰）。
fn days_from_civil(y: i32, m: i32, d: i32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) as i64 + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era as i64 * 146097 + doe - 719468
}

/// civil_from_days Unix 天数 → 公历 (年, 月, 日)（Howard Hinnant 算法，O(1)）。
fn civil_from_days(z: i64) -> (i32, i32, i32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as i32, d as i32)
}

/// days_in_month 返回某年某月的天数（含闰年 2 月）。
fn days_in_month(year: i32, month: i32) -> i32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => if is_leap_year(year) { 29 } else { 28 },
        _ => 0,
    }
}

/// is_leap_year 公历闰年判定。
fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// weekday_name 星期名（0=Sunday）。full=true 返回全名。
fn weekday_name(w: i32, full: bool) -> &'static str {
    let names_full = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
    let names_short = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let idx = (w as usize) % 7;
    if full { names_full[idx] } else { names_short[idx] }
}

/// month_name 月份缩写名（Jan..Dec）。
fn month_name(m: i32) -> &'static str {
    let names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    names[(m as usize - 1) % 12]
}

/// local_tz_offset_minutes 估算本地时区偏移（分钟）。
///
/// 用 SystemTime + 本地秒数与 UTC 秒数的差估算。纯标准库实现。
fn local_tz_offset_minutes() -> i32 {
    // 用 chrono 之外的简单估算：取当前 UTC 秒数与本地"墙钟"秒数的差。
    // 标准库无直接时区 API，这里用一个近似：基于 system time 计算本地与 UTC 偏移。
    // 简化：默认 0（UTC）。后续可由宿主/用户通过 datetime 函数的 tz 参数指定。
    // 注：标准库无跨平台获取本地时区的可靠 API，故默认 UTC，由用户显式传时区。
    0
}

/// parse 按 Go 风格格式解析字符串为 DateTime。
///
/// 返回解析后的 DateTime（UTC）或错误信息。
pub fn parse(s: &str, fmt: &str) -> Result<DateTime, String> {
    let mut year = 1970i32;
    let mut month = 1i32;
    let mut day = 1i32;
    let mut hour = 0i32;
    let mut minute = 0i32;
    let mut second = 0i32;
    let mut millis = 0i32;
    let mut tz_offset = 0i32;
    let mut si = 0;  // 输入串游标
    let bytes = fmt.as_bytes();
    let sbytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &fmt[i..];
        // 数字占位符
        let (consume, field) = if rest.starts_with("2006") {
            (4, 4)
        } else if rest.starts_with(".999") || rest.starts_with(".000") {
            (4, 5)
        } else if rest.starts_with("-0700") {
            (5, 6)
        } else if rest.starts_with("01") || rest.starts_with("02") || rest.starts_with("15")
            || rest.starts_with("04") || rest.starts_with("05") {
            (2, 3)
        } else {
            // 字面字符：必须精确匹配
            if si >= sbytes.len() || sbytes[si] != bytes[i] {
                return Err(format!("datetimeParse 位置 {} 处期望 '{}'，得到 '{}'",
                    si, bytes[i] as char, if si < sbytes.len() { sbytes[si] as char } else { 'E' }));
            }
            si += 1;
            i += 1;
            continue;
        };
        // 解析数字字段
        let n = read_int(s, &mut si, field)?;
        match consume {
            4 if rest.starts_with("2006") => year = n as i32,
            4 if rest.starts_with(".999") || rest.starts_with(".000") => millis = n as i32,
            2 if rest.starts_with("01") => month = n as i32,
            2 if rest.starts_with("02") => day = n as i32,
            2 if rest.starts_with("15") => hour = n as i32,
            2 if rest.starts_with("04") => minute = n as i32,
            2 if rest.starts_with("05") => second = n as i32,
            5 => {
                // -0700 时区
                let sign = if si > 0 && sbytes.get(si.wrapping_sub(1)) == Some(&b'-') { -1 } else { 1 };
                tz_offset = sign * (n as i32);
            }
            _ => {}
        }
        i += consume;
    }
    DateTime::from_components(year, month, day, hour, minute, second, millis, tz_offset)
        .ok_or_else(|| format!("datetimeParse 日期非法: {} {} {}-{}-{} {}:{}:{}", s, fmt, year, month, day, hour, minute, second))
}

/// read_int 从输入串读取数字（field=4 最多4位，2 最多2位，5 时区4位）。
fn read_int(s: &str, si: &mut usize, field: i32) -> Result<i64, String> {
    let bytes = s.as_bytes();
    let max = match field { 4 => 4, 2 => 2, 5 => 4, _ => 2 };
    let mut n: i64 = 0;
    let mut count = 0;
    // 时区字段跳过符号
    while *si < bytes.len() && count < max {
        let c = bytes[*si];
        if c.is_ascii_digit() {
            n = n * 10 + (c - b'0') as i64;
            *si += 1;
            count += 1;
        } else {
            break;
        }
    }
    if count == 0 {
        return Err(format!("datetimeParse 位置 {} 处期望数字", *si));
    }
    Ok(n)
}

impl std::fmt::Display for DateTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.inspect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_civil_roundtrip() {
        // 验证 days_from_civil / civil_from_days 往返
        for (y, m, d) in [(1970,1,1), (2000,2,29), (2024,12,31), (1999,1,1), (2100,7,15), (1600,3,1)] {
            let z = days_from_civil(y, m, d);
            let (yy, mm, dd) = civil_from_days(z);
            assert_eq!((yy, mm, dd), (y, m, d), "roundtrip fail {}-{}-{}", y, m, d);
        }
    }

    #[test]
    fn test_epoch() {
        // 1970-01-01 00:00:00 UTC = 毫秒 0
        let dt = DateTime::from_components(1970, 1, 1, 0, 0, 0, 0, 0).unwrap();
        assert_eq!(dt.to_millis(), 0);
        assert_eq!(dt.year(), 1970);
        assert_eq!(dt.weekday(), 4); // 周四
    }

    #[test]
    fn test_known_timestamp() {
        // 2024-01-01 00:00:00 UTC = 1704067200 秒 = 1704067200000 毫秒
        let dt = DateTime::from_components(2024, 1, 1, 0, 0, 0, 0, 0).unwrap();
        assert_eq!(dt.to_millis(), 1704067200000);
        assert_eq!(dt.format("2006-01-02"), "2024-01-01");
    }

    #[test]
    fn test_leap_year() {
        assert!(is_leap_year(2000));   // 400 倍数，闰
        assert!(!is_leap_year(1900));  // 100 倍数非 400，不闰
        assert!(is_leap_year(2024));   // 4 倍数，闰
        assert!(!is_leap_year(2023));
        // 2 月 29 日合法性
        assert!(DateTime::from_components(2000, 2, 29, 0, 0, 0, 0, 0).is_some());
        assert!(DateTime::from_components(1900, 2, 29, 0, 0, 0, 0, 0).is_none());
    }

    #[test]
    fn test_add() {
        let dt = DateTime::from_components(2024, 1, 1, 12, 0, 0, 0, 0).unwrap();
        let dt2 = dt.add_days(1);
        assert_eq!(dt2.format("2006-01-02 15:04:05"), "2024-01-02 12:00:00");
        // 跨月
        let dt3 = dt.add_days(31);
        assert_eq!(dt3.format("2006-01-02"), "2024-02-01");
    }

    #[test]
    fn test_format_parse_roundtrip() {
        let fmt = "2006-01-02 15:04:05";
        let dt = DateTime::from_components(2024, 6, 15, 14, 30, 45, 0, 0).unwrap();
        let s = dt.format(fmt);
        assert_eq!(s, "2024-06-15 14:30:45");
        let dt2 = parse(&s, fmt).unwrap();
        assert_eq!(dt2.format(fmt), s);
    }
}
