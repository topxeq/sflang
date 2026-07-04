//! builtins_time.rs — 时间相关内置函数
//!
//! 设计要点：
//!   - now/nowSec/clock 仅依赖 Rust 标准库（SystemTime / Instant）
//!   - datetime 系列基于自实现的 DateTime 类型（datetime.rs，纯标准库历法）
//!
//! 函数列表：
//!   时间戳：
//!     now()    — 自 epoch 的毫秒数（Int）
//!     nowSec() — 自 epoch 的秒数（Int）
//!     clock()  — 自解释器启动以来的微秒数（Int，单调）
//!   datetime 类型：
//!     nowDT()              — 当前时间（datetime）
//!     datetime(...)        — 构造（毫秒 或 年月日时分秒）
//!     datetimeFromMillis(n)— 毫秒 → datetime（UTC）
//!     datetimeParse(s,fmt) — 解析字符串
//!     isDatetime(x)        — 类型判断

use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::datetime::DateTime;
use crate::value::Value;
use crate::vm::VM;

/// register 注册所有时间内置函数到 VM。
pub fn register(vm: &mut VM) {
    vm.register_builtin("now", bi_now);
    vm.register_builtin("nowSec", bi_now_sec);
    vm.register_builtin("clock", bi_clock);
    // datetime 类型函数
    vm.register_builtin("nowDT", bi_now_dt);
    vm.register_builtin("datetime", bi_datetime);
    vm.register_builtin("datetimeFromMillis", bi_datetime_from_millis);
    vm.register_builtin("datetimeParse", bi_datetime_parse);
    vm.register_builtin("isDatetime", bi_is_datetime);
    // datetime 运算函数（datetime 不可变，运算返回新值）
    vm.register_builtin("dtFormat", bi_dt_format);
    vm.register_builtin("dtAddDays", bi_dt_add_days);
    vm.register_builtin("dtAddSeconds", bi_dt_add_seconds);
    vm.register_builtin("dtAddMillis", bi_dt_add_millis);
    vm.register_builtin("dtToMillis", bi_dt_to_millis);
}

/// MONOTONIC_BASE 进程级单调时钟基准（懒初始化）。
static MONOTONIC_BASE: OnceLock<Instant> = OnceLock::new();

/// base 返回单调时钟基准（首次调用时初始化）。
fn base() -> Instant {
    *MONOTONIC_BASE.get_or_init(Instant::now)
}

/// bi_now 返回自 epoch 以来的毫秒数（Int）。
fn bi_now(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .map_err(|e| crate::value::error_value(format!("now() 系统时间异常: {}", e)))?;
    Ok(Value::Int(ms))
}

/// bi_now_sec 返回自 epoch 以来的秒数。
fn bi_now_sec(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    let s = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .map_err(|e| crate::value::error_value(format!("nowSec() 系统时间异常: {}", e)))?;
    Ok(Value::Int(s))
}

/// bi_clock 返回自进程启动以来的微秒数（单调时钟）。
fn bi_clock(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    let us = base().elapsed().as_micros() as i64;
    Ok(Value::Int(us))
}

// ---- datetime 类型函数 ----

/// bi_now_dt 返回当前时间（datetime，本地时区）。
fn bi_now_dt(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    Ok(Value::DateTime(std::sync::Arc::new(DateTime::now())))
}

/// bi_datetime 构造 datetime。
///
/// 用法：
///   datetime(millis)                      — 从 Unix 毫秒（UTC）
///   datetime(year, month, day)            — 日期（时分秒为 0）
///   datetime(year, month, day, hour, min, sec) — 完整日期时间
///   最后一个参数可为 tz_offset（分钟，整数）
fn bi_datetime(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let dt = match args.len() {
        1 => {
            // datetime(millis)
            let ms = bh::as_int(args, 0, "datetime")?;
            DateTime::from_millis_utc(ms)
        }
        3 | 4 => {
            // datetime(year, month, day[, tz])
            let y = bh::as_int(args, 0, "datetime")? as i32;
            let mo = bh::as_int(args, 1, "datetime")? as i32;
            let d = bh::as_int(args, 2, "datetime")? as i32;
            let tz = if args.len() == 4 { bh::as_int(args, 3, "datetime")? as i32 } else { 0 };
            DateTime::from_components(y, mo, d, 0, 0, 0, 0, tz)
                .ok_or_else(|| crate::value::error_value(format!(
                    "datetime() 日期非法: {}-{}-{} (可能原因：闰年/月份天数错误)", y, mo, d,
                )))?
        }
        6 | 7 => {
            // datetime(year, month, day, hour, min, sec[, tz])
            let y = bh::as_int(args, 0, "datetime")? as i32;
            let mo = bh::as_int(args, 1, "datetime")? as i32;
            let d = bh::as_int(args, 2, "datetime")? as i32;
            let h = bh::as_int(args, 3, "datetime")? as i32;
            let mi = bh::as_int(args, 4, "datetime")? as i32;
            let s = bh::as_int(args, 5, "datetime")? as i32;
            let tz = if args.len() == 7 { bh::as_int(args, 6, "datetime")? as i32 } else { 0 };
            DateTime::from_components(y, mo, d, h, mi, s, 0, tz)
                .ok_or_else(|| crate::value::error_value(format!(
                    "datetime() 日期时间非法: {}-{}-{} {}:{}:{} (可能原因：范围越界)", y, mo, d, h, mi, s,
                )))?
        }
        _ => return Err(crate::value::error_value(
            "datetime() 参数数应为 1(millis)/3(ymd)/4(ymd,tz)/6(ymdhms)/7(ymdhms,tz)",
        )),
    };
    Ok(Value::DateTime(std::sync::Arc::new(dt)))
}

/// bi_datetime_from_millis 毫秒 → datetime（UTC）。
fn bi_datetime_from_millis(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let ms = bh::as_int(args, 0, "datetimeFromMillis")?;
    Ok(Value::DateTime(std::sync::Arc::new(DateTime::from_millis_utc(ms))))
}

/// bi_datetime_parse 解析字符串为 datetime。
///
/// 用法：datetimeParse(s, fmt)，fmt 为 Go 风格参考时间（如 "2006-01-02 15:04:05"）。
fn bi_datetime_parse(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let s = bh::as_str(args, 0, "datetimeParse")?;
    let fmt = bh::as_str(args, 1, "datetimeParse")?;
    let dt = crate::datetime::parse(s, fmt).map_err(crate::value::error_value)?;
    Ok(Value::DateTime(std::sync::Arc::new(dt)))
}

/// bi_is_datetime 判断是否为 datetime 类型。
fn bi_is_datetime(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(args.get(0), Some(Value::DateTime(_)))))
}

/// as_dt 取第 idx 个参数为 DateTime 的 Arc 引用。
fn as_dt<'a>(args: &'a [Value], idx: usize, fn_name: &str) -> Result<&'a std::sync::Arc<DateTime>, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, idx, fn_name)?;
    match &args[idx] {
        Value::DateTime(dt) => Ok(dt),
        v => Err(crate::value::error_value(format!(
            "{}() 第 {} 个参数应为 datetime，得到 {} (可能原因：类型不匹配)",
            fn_name, idx + 1, v.type_name(),
        ))),
    }
}

/// bi_dt_format 格式化 datetime 为字符串。
///
/// 用法：dtFormat(dt, fmt)，fmt 为 Go 风格参考时间。
fn bi_dt_format(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let dt = as_dt(args, 0, "dtFormat")?;
    let fmt = bh::as_str(args, 1, "dtFormat")?;
    Ok(Value::str_from(dt.format(fmt)))
}

/// bi_dt_add_days datetime 加天数，返回新 datetime。
fn bi_dt_add_days(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let dt = as_dt(args, 0, "dtAddDays")?;
    let n = bh::as_int(args, 1, "dtAddDays")?;
    Ok(Value::DateTime(std::sync::Arc::new(dt.add_days(n))))
}

/// bi_dt_add_seconds datetime 加秒数，返回新 datetime。
fn bi_dt_add_seconds(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let dt = as_dt(args, 0, "dtAddSeconds")?;
    let n = bh::as_int(args, 1, "dtAddSeconds")?;
    Ok(Value::DateTime(std::sync::Arc::new(dt.add_seconds(n))))
}

/// bi_dt_add_millis datetime 加毫秒数，返回新 datetime。
fn bi_dt_add_millis(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let dt = as_dt(args, 0, "dtAddMillis")?;
    let n = bh::as_int(args, 1, "dtAddMillis")?;
    Ok(Value::DateTime(std::sync::Arc::new(dt.add_millis(n))))
}

/// bi_dt_to_millis datetime 转 Unix 毫秒（int）。
fn bi_dt_to_millis(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let dt = as_dt(args, 0, "dtToMillis")?;
    Ok(Value::Int(dt.to_millis()))
}
