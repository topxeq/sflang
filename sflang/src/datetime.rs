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
        // 校验范围（年份限 1-9999：与 4 位年份布局一致，且保证内部毫秒运算不溢出）
        if !(1..=9999).contains(&year) { return None; }
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
    ///
    /// 1970 前的时间戳（local_millis 为负）必须用 div_euclid/rem_euclid：
    /// 截断除法 `/` 与 `%` 对负数向零取整，会得到负的时/分/秒（如 -1）。
    pub fn hour(&self) -> i32 {
        (self.local_millis().div_euclid(MILLIS_PER_HOUR).rem_euclid(24)) as i32
    }
    /// minute 分（0-59）。同 hour，用欧几里得除法保证负时间戳正确。
    pub fn minute(&self) -> i32 {
        (self.local_millis().div_euclid(MILLIS_PER_MINUTE).rem_euclid(60)) as i32
    }
    /// second 秒（0-59）。同 hour，用欧几里得除法保证负时间戳正确。
    pub fn second(&self) -> i32 {
        (self.local_millis().div_euclid(1000).rem_euclid(60)) as i32
    }
    /// millis 毫秒部分（0-999）。rem_euclid 保证负时间戳得到 0-999（如 -1ms → 999）。
    pub fn millis_part(&self) -> i32 {
        (self.local_millis().rem_euclid(1000)) as i32
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

    /// add_millis 加毫秒，返回新 DateTime（时区不变）。结果溢出 i64 时返回 Err。
    pub fn add_millis(&self, n: i64) -> Result<Self, String> {
        let millis = self.millis.checked_add(n).ok_or_else(|| format!(
            "datetime 加毫秒溢出: {} + {} 超出 int 范围 (可能原因：参数过大)", self.millis, n,
        ))?;
        Ok(DateTime { millis, tz_offset: self.tz_offset })
    }
    /// add_seconds 加秒。换算为毫秒时溢出或结果溢出均返回 Err。
    pub fn add_seconds(&self, n: i64) -> Result<Self, String> {
        let ms = n.checked_mul(1000).ok_or_else(|| format!(
            "datetime 加秒溢出: {} 秒无法换算为毫秒 (可能原因：参数过大)", n,
        ))?;
        self.add_millis(ms)
    }
    /// add_days 加天。换算为毫秒时溢出或结果溢出均返回 Err。
    pub fn add_days(&self, n: i64) -> Result<Self, String> {
        let ms = n.checked_mul(MILLIS_PER_DAY).ok_or_else(|| format!(
            "datetime 加天数溢出: {} 天无法换算为毫秒 (可能原因：参数过大)", n,
        ))?;
        self.add_millis(ms)
    }

    /// add_date 加减年月日（日历运算，处理月份进位与闰年）。
    ///
    /// 与 add_days（纯毫秒运算）不同，本方法按公历规则：
    ///   - 先将 year/month 相加并规范到合法区间（1-12）
    ///   - day 按目标月份的最大天数截断（如 1月31日 +1月 → 2月28/29日）
    ///   - 再用 add_days 叠加 days 参数（days 可为负）
    ///
    /// 返回新 DateTime（时区偏移不变）；days 溢出时返回 Err。
    pub fn add_date(&self, years: i32, months: i32, days: i64) -> Result<Self, String> {
        let (y0, m0, d0) = self.date_part();
        let h = self.hour();
        let mi = self.minute();
        let s = self.second();
        let ms = self.millis_part();
        let tz = self.tz_offset;

        // 年月相加并规范：把 month 转为 0-based 偏移便于整除
        let total_month = (y0 as i64) * 12 + (m0 as i64 - 1)
            + years as i64 * 12
            + months as i64;
        let new_year = (total_month.div_euclid(12)) as i32;
        let new_month = (total_month.rem_euclid(12)) as i32 + 1; // 1-based

        // day 截断到目标月份最大天数（处理 1月31日 +1月 → 2月28日 等）
        let max_day = days_in_month(new_year, new_month);
        let new_day = d0.min(max_day);

        // 用 from_components 构造，再叠加 days（days 部分用 add_days 完成跨月）
        let base = DateTime::from_components(new_year, new_month, new_day, h, mi, s, ms, tz)
            .unwrap_or_else(|| DateTime::from_millis_with_tz(self.millis, tz));
        base.add_days(days)
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
                None => {
                    // 未匹配占位符：按完整 UTF-8 字符前进（处理中文等多字节字符）
                    let ch_len = utf8_char_len(bytes[i]);
                    let end = (i + ch_len).min(bytes.len());
                    if let Ok(s) = std::str::from_utf8(&bytes[i..end]) {
                        out.push_str(s);
                    } else {
                        out.push(bytes[i] as char); // 回退
                    }
                    i = end;
                }
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

/// utf8_char_len 根据 UTF-8 首字节返回字符长度。
fn utf8_char_len(b: u8) -> usize {
    if b < 0x80 { 1 }
    else if b < 0xC0 { 1 }
    else if b < 0xE0 { 2 }
    else if b < 0xF0 { 3 }
    else { 4 }
}

/// days_from_civil 公历年月日 → Unix 天数（Howard Hinnant 算法，O(1)）。
///
/// 1970-01-01 对应 719468。正确处理闰年（公历规则：4年闰/100年不闰/400年闰）。
/// 内部全程用 i64 运算，避免极端年份（i32 边界附近）时 y-1、y-399 等中间量溢出。
fn days_from_civil(y: i32, m: i32, d: i32) -> i64 {
    let y = y as i64;
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // 纪元内年份偏移，恒在 [0, 399]
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) as i64 + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
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

/// local_tz_offset_minutes 获取本地时区偏移（分钟，相对 UTC，如北京 +480、UTC 为 0）。
///
/// 纯标准库实现：
///   - Windows：GetTimeZoneInformation API（含夏令时/标准时状态修正）
///   - Unix（Linux/macOS）：解析 /etc/localtime（TZif 格式，RFC 8536），
///     取当前时刻生效的偏移
/// 获取失败时回退 0（UTC），不产生错误（调用方无须处理失败分支）。
pub fn local_tz_offset_minutes() -> i32 {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    local_tz_offset_minutes_at(now_secs)
}

/// local_tz_offset_minutes_at 获取指定时刻的本地时区偏移（分钟）。
///
/// 拆出时刻参数便于按历史夏令时切换测试（TZif 含历史转换记录）。
fn local_tz_offset_minutes_at(now_secs: i64) -> i32 {
    #[cfg(windows)]
    {
        // Windows API 直接返回当前生效的时区状态，无须时刻参数
        let _ = now_secs;
        windows_tz_offset_minutes()
    }
    #[cfg(unix)]
    {
        std::fs::read("/etc/localtime")
            .ok()
            .and_then(|data| parse_tzif_offset(&data, now_secs))
            .unwrap_or(0)
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = now_secs;
        0
    }
}

/// windows_tz_offset_minutes 通过 Windows API 获取当前本地时区偏移（分钟）。
#[cfg(windows)]
fn windows_tz_offset_minutes() -> i32 {
    use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    // SAFETY：GetTimeZoneInformation 只写入调用方提供的缓冲区，无其他副作用
    unsafe {
        let mut tzi: TIME_ZONE_INFORMATION = std::mem::zeroed();
        let state = GetTimeZoneInformation(&mut tzi);
        // Bias 语义：UTC = 本地 + Bias（即"本地比 UTC 慢多少分钟"，西半球为正），
        // 故本地相对 UTC 的偏移 = -Bias；标准时/夏令时生效时再叠加对应 Bias。
        let mut bias = tzi.Bias;
        // 返回值：0=UNKNOWN 1=STANDARD 2=DAYLIGHT
        if state == 1 {
            bias += tzi.StandardBias;
        } else if state == 2 {
            bias += tzi.DaylightBias;
        }
        -bias
    }
}

/// parse_tzif_offset 解析 TZif 时区文件内容（RFC 8536），返回 now_secs 时刻生效的
/// UTC 偏移（分钟）。
///
/// 兼容要点：
///   - 文件可能含 v1（32 位转换时间）与 v2/v3（64 位）两个数据块，取最后一个块；
///     仅 v1 的旧文件则用第一块
///   - "slim" 格式的现代 tzdata 常无转换记录（timecnt=0），此时回退到
///     首个非夏令时类型（即标准时偏移）
///   - 偏移以秒存储，换算为分钟（整除，均为 60 的倍数）
// 非 Unix 平台该函数仅被单元测试使用（Unix 运行时路径在 cfg(unix) 分支内）
#[cfg_attr(not(unix), allow(dead_code))]
fn parse_tzif_offset(data: &[u8], now_secs: i64) -> Option<i32> {
    // 定位文件中最后一个 "TZif" 魔数（v2+ 双块布局的第二个块；v1 单块文件即第一个）
    let mut hdr: Option<usize> = None;
    let mut from = 0usize;
    while let Some(pos) = find_slice(data, from, b"TZif") {
        hdr = Some(pos);
        from = pos + 4;
    }
    let hdr = hdr?;
    if data.len() < hdr + 44 {
        return None;
    }
    // 版本字节决定转换时间宽度：'2'/'3' 为 64 位，其余（含 '\0' = v1）为 32 位
    let time_size = match data[hdr + 4] {
        b'2' | b'3' => 8usize,
        _ => 4usize,
    };
    // 头部 6 个大端 u32 计数（RFC 8536 §3.1）
    let be32 = |off: usize| -> Option<u32> {
        let b = data.get(off..off + 4)?;
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    let isutcnt = be32(hdr + 20)? as usize;
    let isstdcnt = be32(hdr + 24)? as usize;
    let leapcnt = be32(hdr + 28)? as usize;
    let timecnt = be32(hdr + 32)? as usize;
    let typecnt = be32(hdr + 36)? as usize;
    let charcnt = be32(hdr + 40)? as usize;
    if typecnt == 0 {
        return None;
    }
    // 各段起始位置（RFC 8536 §3.2）：转换时间表 → 类型索引表 → 类型信息表 → 其余
    let trans_start = hdr + 44;
    let idx_start = trans_start + timecnt * time_size;
    let types_start = idx_start + timecnt;
    let rest_start = types_start + typecnt * 6;
    let rest_size = charcnt
        + leapcnt * (time_size + 4)
        + isstdcnt
        + isutcnt;
    if data.len() < rest_start + rest_size {
        return None;
    }
    // 遍历转换时间表，找 now_secs 之前最近一次转换对应的类型索引
    let mut type_idx: Option<usize> = None;
    for i in 0..timecnt {
        let t = if time_size == 8 {
            let b = data.get(trans_start + i * 8..trans_start + i * 8 + 8)?;
            i64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
        } else {
            let b = data.get(trans_start + i * 4..trans_start + i * 4 + 4)?;
            i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as i64
        };
        if t <= now_secs {
            type_idx = Some(*data.get(idx_start + i)? as usize);
        } else {
            break;
        }
    }
    // 无适用转换（早于首个转换或无转换记录）：回退到首个非夏令时类型。
    // 类型项结构为 utoff(4 字节) + isdst(1) + desigidx(1)，isdst 位于项内偏移 4。
    let type_idx = type_idx.or_else(|| {
        (0..typecnt).find(|&i| data.get(types_start + i * 6 + 4).copied().unwrap_or(1) == 0)
    })?;
    if type_idx >= typecnt {
        return None;
    }
    // 类型信息结构：utoff(i32 大端) + isdst(u8) + desigidx(u8)，取 utoff 换算分钟
    let b = data.get(types_start + type_idx * 6..types_start + type_idx * 6 + 4)?;
    let utoff = i32::from_be_bytes([b[0], b[1], b[2], b[3]]);
    Some(utoff / 60)
}

/// find_slice 在 data[from..] 中查找子串，返回绝对位置。
fn find_slice(data: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if from > data.len() {
        return None;
    }
    data[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// parse 按 Go 风格格式解析字符串为 DateTime。
///
/// 支持的占位符与 format 一致：
///   2006→年(最多4位)  01→月  02→日  15→时(24h)  04→分  05→秒
///   .000→毫秒(3位)  .999→毫秒(可变长，可为 0 时整体省略)  -0700→时区(±HHMM)
///
/// 规则：
///   - 布局中的字面字符（含中文等多字节字符）须与输入逐字符精确匹配，
///     按 UTF-8 字符推进，绝不panic
///   - 时区占位符先消费可选的 '+'/'-' 符号，再读 4 位数字 HHMM，
///     按 ±(HH*60+MM) 换算为分钟偏移
///   - 小数秒占位符中的 '.' 属于布局本身，解析时输入须先有 '.'（.999 可省略）
///   - 解析结束后要求输入被完全消费，有多余字符返回错误（提示含剩余内容）
///
/// 返回解析后的 DateTime（保持解析出的时区偏移）或错误信息。
pub fn parse(s: &str, fmt: &str) -> Result<DateTime, String> {
    let mut year = 1970i32;
    let mut month = 1i32;
    let mut day = 1i32;
    let mut hour = 0i32;
    let mut minute = 0i32;
    let mut second = 0i32;
    let mut millis = 0i32;
    let mut tz_offset = 0i32;
    let sbytes = s.as_bytes();
    let fbytes = fmt.as_bytes();
    let mut si = 0usize; // 输入串游标（字节，始终位于字符边界）
    let mut i = 0usize;  // 布局游标（字节，始终位于字符边界）
    while i < fbytes.len() {
        let rest = &fmt[i..];
        if rest.starts_with("2006") {
            year = read_digits(s, &mut si, 1, 4)? as i32;
            i += 4;
        } else if rest.starts_with("-0700") {
            tz_offset = read_tz_offset(s, &mut si)?;
            i += 5;
        } else if rest.starts_with(".000") {
            // 固定 3 位小数秒：布局中的 '.' 须在输入中精确出现
            expect_char(s, &mut si, b'.')?;
            millis = read_fraction_millis(s, &mut si)?;
            i += 4;
        } else if rest.starts_with(".999") {
            // 去尾零小数秒：毫秒为 0 时 format 不输出 '.'，故 '.' 与数字整体可选
            if sbytes.get(si) == Some(&b'.') {
                si += 1;
                millis = read_fraction_millis(s, &mut si)?;
            }
            i += 4;
        } else if rest.starts_with("01") {
            month = read_digits(s, &mut si, 1, 2)? as i32;
            i += 2;
        } else if rest.starts_with("02") {
            day = read_digits(s, &mut si, 1, 2)? as i32;
            i += 2;
        } else if rest.starts_with("15") {
            hour = read_digits(s, &mut si, 1, 2)? as i32;
            i += 2;
        } else if rest.starts_with("04") {
            minute = read_digits(s, &mut si, 1, 2)? as i32;
            i += 2;
        } else if rest.starts_with("05") {
            second = read_digits(s, &mut si, 1, 2)? as i32;
            i += 2;
        } else {
            // 字面字符：按完整 UTF-8 字符比较并推进（正确处理"年"等多字节字符，
            // 按字节推进会在 &fmt[i..] 处触发 char boundary panic）
            let flen = utf8_char_len(fbytes[i]);
            let fend = (i + flen).min(fbytes.len());
            let fch = std::str::from_utf8(&fbytes[i..fend]).unwrap_or("\u{FFFD}");
            if si >= sbytes.len() {
                return Err(format!(
                    "datetimeParse 输入提前结束：位置 {} 处仍期望字面字符 \"{}\" (布局 \"{}\"，输入 \"{}\")",
                    si, fch, fmt, s,
                ));
            }
            let slen = utf8_char_len(sbytes[si]);
            let send = (si + slen).min(sbytes.len());
            let sch = std::str::from_utf8(&sbytes[si..send]).unwrap_or("\u{FFFD}");
            if fch != sch {
                return Err(format!(
                    "datetimeParse 位置 {} 处期望字面字符 \"{}\"，得到 \"{}\" (布局 \"{}\") (可能原因：布局与输入不匹配，或 s 与 fmt 参数顺序颠倒)",
                    si, fch, sch, fmt,
                ));
            }
            si = send;
            i = fend;
        }
    }
    // 校验输入被布局完全消费
    if si != s.len() {
        return Err(format!(
            "datetimeParse 输入在位置 {} 处有多余内容 \"{}\"，未被布局 \"{}\" 消费 (可能原因：布局缺少对应占位符)",
            si, &s[si..], fmt,
        ));
    }
    DateTime::from_components(year, month, day, hour, minute, second, millis, tz_offset)
        .ok_or_else(|| format!(
            "datetimeParse 日期非法: {}-{}-{} {}:{}:{}.{} (布局 \"{}\") (可能原因：字段越界，年份须在 1-9999)",
            year, month, day, hour, minute, second, millis, fmt,
        ))
}

/// read_digits 从输入读取 min..=max 位十进制数字，返回数值。
///
/// 实际位数不足 min 时返回错误（含位置信息）。游标仅越过 ASCII 数字，保持字符边界。
fn read_digits(s: &str, si: &mut usize, min: usize, max: usize) -> Result<i64, String> {
    let bytes = s.as_bytes();
    let mut n: i64 = 0;
    let mut count = 0usize;
    while *si < bytes.len() && count < max {
        let c = bytes[*si];
        if !c.is_ascii_digit() { break; }
        n = n * 10 + (c - b'0') as i64;
        *si += 1;
        count += 1;
    }
    if count < min {
        return Err(format!(
            "datetimeParse 位置 {} 处期望 {}-{} 位数字，实际只有 {} 位 (可能原因：布局与输入不匹配)",
            *si, min, max, count,
        ));
    }
    Ok(n)
}

/// expect_char 校验输入当前位置为指定 ASCII 字符并消费它。
fn expect_char(s: &str, si: &mut usize, c: u8) -> Result<(), String> {
    if s.as_bytes().get(*si) == Some(&c) {
        *si += 1;
        Ok(())
    } else {
        let got = char_at(s, *si).unwrap_or("输入已结束".to_string());
        Err(format!(
            "datetimeParse 位置 {} 处期望字符 '{}'，得到 \"{}\" (可能原因：布局与输入不匹配)",
            *si, c as char, got,
        ))
    }
}

/// read_fraction_millis 读取小数秒并换算为毫秒（0-999）。
///
/// 位数不足 3 位时右侧补零（如 ".5" = 500 毫秒、".05" = 50 毫秒）；
/// 超过 3 位时丢弃更低精度（微秒及以下），保证后续布局可继续对齐。
fn read_fraction_millis(s: &str, si: &mut usize) -> Result<i32, String> {
    let bytes = s.as_bytes();
    let mut n: i64 = 0;
    let mut count = 0usize;
    while *si < bytes.len() && count < 3 {
        let c = bytes[*si];
        if !c.is_ascii_digit() { break; }
        n = n * 10 + (c - b'0') as i64;
        *si += 1;
        count += 1;
    }
    if count == 0 {
        return Err(format!(
            "datetimeParse 位置 {} 处期望小数秒数字 (可能原因：布局与输入不匹配)",
            *si,
        ));
    }
    // 位数不足 3 位时补零
    for _ in count..3 { n *= 10; }
    // 丢弃微秒及以下的更低精度
    while *si < bytes.len() && bytes[*si].is_ascii_digit() { *si += 1; }
    Ok(n as i32)
}

/// read_tz_offset 读取时区偏移（如 "+0530"/"-0800"）并换算为分钟。
///
/// 先消费可选的 '+'/'-' 符号（dtFormat 输出恒带符号），再读 4 位数字 HHMM，
/// 返回 ±(HH*60+MM)。
fn read_tz_offset(s: &str, si: &mut usize) -> Result<i32, String> {
    let bytes = s.as_bytes();
    let mut sign = 1i64;
    match bytes.get(*si) {
        Some(&b'+') => { *si += 1; }
        Some(&b'-') => { sign = -1; *si += 1; }
        // 符号可选：无符号按正偏移处理
        _ => {}
    }
    let n = read_digits(s, si, 4, 4)?;
    let hh = n / 100;
    let mm = n % 100;
    if hh > 23 || mm > 59 {
        return Err(format!(
            "datetimeParse 时区偏移非法: HH={} MM={} (要求 HH 0-23、MM 0-59)",
            hh, mm,
        ));
    }
    Ok((sign * (hh * 60 + mm)) as i32)
}

/// char_at 取字符串中某字节位置起的第一个完整 UTF-8 字符（用于错误信息展示）。
///
/// 位置非法（越界或不在字符边界）时返回 None。
fn char_at(s: &str, pos: usize) -> Option<String> {
    s.get(pos..).and_then(|rest| rest.chars().next()).map(|ch| ch.to_string())
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
        let dt2 = dt.add_days(1).unwrap();
        assert_eq!(dt2.format("2006-01-02 15:04:05"), "2024-01-02 12:00:00");
        // 跨月
        let dt3 = dt.add_days(31).unwrap();
        assert_eq!(dt3.format("2006-01-02"), "2024-02-01");
        // 溢出返回错误，不 panic
        assert!(dt.add_days(i64::MAX).is_err());
        assert!(dt.add_seconds(i64::MAX).is_err());
        assert!(dt.add_millis(i64::MAX - dt.to_millis() + 1).is_err());
    }

    #[test]
    fn test_pre_epoch_components() {
        // -1 毫秒 = 1969-12-31 23:59:59.999 UTC（修复前各组件为负数）
        let dt = DateTime::from_millis_utc(-1);
        assert_eq!((dt.year(), dt.month(), dt.day()), (1969, 12, 31));
        assert_eq!((dt.hour(), dt.minute(), dt.second()), (23, 59, 59));
        assert_eq!(dt.millis_part(), 999);
        assert_eq!(dt.weekday(), 3); // 1969-12-31 是周三
        assert_eq!(dt.format("2006-01-02 15:04:05.000"), "1969-12-31 23:59:59.999");
        // 整天：-86400000 = 1969-12-31 00:00:00.000
        let d2 = DateTime::from_millis_utc(-86_400_000);
        assert_eq!(d2.format("2006-01-02 15:04:05.000"), "1969-12-31 00:00:00.000");
        // 与 +1ms 往返：-1ms + 1ms = epoch
        assert_eq!(dt.add_millis(1).unwrap().to_millis(), 0);
    }

    #[test]
    fn test_year_range() {
        // 年份限 1-9999
        assert!(DateTime::from_components(0, 1, 1, 0, 0, 0, 0, 0).is_none());
        assert!(DateTime::from_components(10000, 1, 1, 0, 0, 0, 0, 0).is_none());
        assert!(DateTime::from_components(1, 1, 1, 0, 0, 0, 0, 0).is_some());
        assert!(DateTime::from_components(9999, 12, 31, 23, 59, 59, 999, 0).is_some());
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

    #[test]
    fn test_parse_multibyte_literal() {
        // 中文布局：字面字符按完整 UTF-8 字符匹配（修复前按字节推进会 panic）
        let dt = parse("2024年06月15日", "2006年01月02日").unwrap();
        assert_eq!(dt.format("2006-01-02"), "2024-06-15");
        // 不匹配时返回错误（而非 panic）
        assert!(parse("2024X06月15日", "2006年01月02日").is_err());
    }

    #[test]
    fn test_parse_tz_roundtrip() {
        // 带符号时区往返：+0530 / -0500
        for (tz, tz_str) in [(330, "+0530"), (-300, "-0500")] {
            let fmt = "2006-01-02 15:04:05 -0700";
            let dt = DateTime::from_components(2024, 6, 15, 14, 30, 45, 0, tz).unwrap();
            let s = dt.format(fmt);
            assert_eq!(s, format!("2024-06-15 14:30:45 {}", tz_str));
            let dt2 = parse(&s, fmt).unwrap();
            assert_eq!(dt2.to_millis(), dt.to_millis(), "UTC 毫秒应一致");
            assert_eq!(dt2.tz_offset, tz, "时区偏移应往返一致");
            assert_eq!(dt2.format(fmt), s);
        }
        // UTC（+0000）往返
        let fmt0 = "2006-01-02 15:04:05 -0700";
        let dt0 = DateTime::from_components(2024, 6, 15, 14, 30, 45, 0, 0).unwrap();
        let s0 = dt0.format(fmt0);
        assert_eq!(s0, "2024-06-15 14:30:45 +0000");
        assert!(parse(&s0, fmt0).is_ok());
    }

    #[test]
    fn test_parse_fraction_roundtrip() {
        // .000 固定 3 位
        let fmt = "2006-01-02 15:04:05.000";
        let dt = DateTime::from_millis_utc(1_704_067_200_123);
        let s = dt.format(fmt);
        assert_eq!(s, "2024-01-01 00:00:00.123");
        let dt2 = parse(&s, fmt).unwrap();
        assert_eq!(dt2.to_millis(), dt.to_millis());
        // .999 去尾零：".12" 解析为 120 毫秒
        let fmt9 = "2006-01-02 15:04:05.999";
        let dt3 = DateTime::from_millis_utc(120);
        let s3 = dt3.format(fmt9);
        assert_eq!(s3, "1970-01-01 00:00:00.12");
        assert_eq!(parse(&s3, fmt9).unwrap().to_millis(), 120);
        // .999 毫秒为 0 时整体省略，解析须容忍无 '.' 输入
        let dt4 = DateTime::from_millis_utc(0);
        let s4 = dt4.format(fmt9);
        assert_eq!(s4, "1970-01-01 00:00:00");
        assert_eq!(parse(&s4, fmt9).unwrap().to_millis(), 0);
        // 单位补零：".5" = 500 毫秒
        assert_eq!(parse("1970-01-01 00:00:00.5", fmt9).unwrap().millis_part(), 500);
    }

    #[test]
    fn test_parse_rejects_extra_input() {
        // 多余字符报错，且错误信息包含剩余内容
        let err = parse("2024-06-15xyz", "2006-01-02").unwrap_err();
        assert!(err.contains("xyz"), "错误信息应包含剩余内容: {}", err);
        assert!(parse("2024-06-15 12:00", "2006-01-02").is_err());
        // 完全消费的输入正常
        assert!(parse("2024-06-15", "2006-01-02").is_ok());
    }

    /// build_tzif_v2 构造一个最小的 v2 TZif 缓冲区用于解析测试。
    ///
    /// 类型表固定两个：0 号 = 夏令时 UTC+0，1 号 = 标准时 UTC+8（28800 秒）。
    /// transitions: (转换时刻秒, 指向的类型索引)；typecnt 为类型表条数。
    fn build_tzif_v2(transitions: &[(i64, usize)], typecnt: usize) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"TZif");
        buf.push(b'2');
        buf.extend_from_slice(&[0u8; 15]); // 保留字段
        let timecnt = transitions.len() as u32;
        for n in [0u32, 0, 0, timecnt, typecnt as u32, 4] {
            buf.extend_from_slice(&n.to_be_bytes());
        }
        // 转换时间表（64 位）+ 类型索引表
        for (t, _) in transitions {
            buf.extend_from_slice(&t.to_be_bytes());
        }
        for (_, idx) in transitions {
            buf.push(*idx as u8);
        }
        // 类型信息表：0 号夏令时 UTC+0，1 号标准时 UTC+8（28800 秒）
        let types: Vec<(i32, u8)> = vec![(0, 1), (28800, 0)];
        for (utoff, isdst) in &types[..typecnt] {
            buf.extend_from_slice(&utoff.to_be_bytes());
            buf.push(*isdst);
            buf.push(0); // desigidx
        }
        buf.extend_from_slice(b"UTC\0"); // charcnt=4 的缩写串区
        buf
    }

    #[test]
    fn test_tzif_after_transition() {
        // 2024-01-01 起切到 UTC+8 标准时（1 号类型）：其后时刻应取 28800 秒 = +480 分钟
        let data = build_tzif_v2(&[(1_704_067_200, 1)], 2);
        assert_eq!(parse_tzif_offset(&data, 1_800_000_000), Some(480));
    }

    #[test]
    fn test_tzif_before_first_transition_falls_back() {
        // 早于首个转换：回退到首个非夏令时类型（1 号 +8h），而非夏令时的 0 号类型
        let data = build_tzif_v2(&[(1_704_067_200, 1)], 2);
        assert_eq!(parse_tzif_offset(&data, 1_000), Some(480));
    }

    #[test]
    fn test_tzif_slim_no_transitions() {
        // slim 格式（timecnt=0）：直接回退首个非夏令时类型
        let data = build_tzif_v2(&[], 2);
        assert_eq!(parse_tzif_offset(&data, 1_800_000_000), Some(480));
    }

    #[test]
    fn test_tzif_dst_type_not_preferred() {
        // 只有夏令时类型时的回退：没有非夏令时类型可选则返回 None
        let data = build_tzif_v2(&[], 1);
        assert_eq!(parse_tzif_offset(&data, 1_800_000_000), None);
    }

    #[test]
    fn test_tzif_truncated_rejected() {
        // 截断的数据返回 None（不 panic）
        let data = build_tzif_v2(&[(1_704_067_200, 1)], 2);
        for cut in [10usize, 44, 60, data.len() - 1] {
            assert_eq!(parse_tzif_offset(&data[..cut], 1_800_000_000), None);
        }
        assert_eq!(parse_tzif_offset(b"", 0), None);
        assert_eq!(parse_tzif_offset(b"TZif3", 0), None);
    }

    #[test]
    fn test_local_tz_offset_sane() {
        // 本地时区偏移应为合法范围（±24h 内），且当前实现不 panic
        let off = local_tz_offset_minutes();
        assert!((-24 * 60..=24 * 60).contains(&off), "非法偏移: {}", off);
    }
}
