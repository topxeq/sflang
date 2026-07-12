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
//!   扩展函数：
//!     getNowTimeStamp()    — 当前 Unix 时间戳（秒）
//!     timeAddDate(dt, y, m, d) — datetime 加年月日（日历运算）
//!     runTicker(fn, ms)    — 周期执行函数，返回 ticker 句柄
//!     formatTime(dt, fmt)  — 格式化 datetime（dtFormat 的别名）

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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
    // 便捷函数
    vm.register_builtin("getNowStr", bi_get_now_str);
    // 扩展函数
    vm.register_builtin("getNowTimeStamp", bi_get_now_timestamp);
    vm.register_builtin("timeAddDate", bi_time_add_date);
    vm.register_builtin("runTicker", bi_run_ticker);
    vm.register_builtin("stopTicker", bi_stop_ticker);
    vm.register_builtin("formatTime", bi_format_time);
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

/// bi_get_now_str 返回当前时间的格式化字符串。
///
/// 用法：getNowStr() → "2026-07-11 14:30:25"（默认格式）
///       getNowStr("2006-01-02") → "2026-07-11"（自定义格式）
fn bi_get_now_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let fmt = if args.is_empty() {
        "2006-01-02 15:04:05"
    } else {
        bh::as_str(args, 0, "getNowStr")?
    };
    let dt = DateTime::now();
    Ok(Value::str_from(dt.format(fmt)))
}

// ---- 扩展内置函数 ----

/// bi_get_now_timestamp 返回当前 Unix 时间戳（秒，int）。
///
/// 用法：getNowTimeStamp() → 1720000000
fn bi_get_now_timestamp(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    let s = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .map_err(|e| crate::value::error_value(format!(
            "getNowTimeStamp() 系统时间异常: {}", e,
        )))?;
    Ok(Value::Int(s))
}

/// bi_time_add_date datetime 加年月日（日历运算）。
///
/// 用法：timeAddDate(dt, years, months, days) → datetime
///
/// 与 dtAddDays 不同，本函数按公历规则处理月份进位：
///   - years/months 直接相加并规范到 1-12 区间
///   - days 用毫秒运算叠加（处理跨月）
///   - day 截断到目标月份最大天数（如 1月31日 +1月 → 2月28/29日）
fn bi_time_add_date(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let dt = as_dt(args, 0, "timeAddDate")?;
    let years = bh::as_int(args, 1, "timeAddDate")? as i32;
    let months = bh::as_int(args, 2, "timeAddDate")? as i32;
    let days = bh::as_int(args, 3, "timeAddDate")?;
    let new_dt = dt.add_date(years, months, days);
    Ok(Value::DateTime(Arc::new(new_dt)))
}

/// TickerHandle runTicker 返回的句柄，用于停止周期任务。
///
/// 内部用 Arc<AtomicBool> 作为停止标志，新线程检查此标志决定是否继续执行。
pub struct TickerHandle {
    /// stop 停止标志，true 时新线程退出循环。
    pub stop: Arc<AtomicBool>,
}

impl TickerHandle {
    /// release 停止 ticker（置 stop 标志为 true，子线程下一轮检查后退出）。
    /// 幂等：多次调用无副作用。
    pub fn release(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// bi_run_ticker 周期执行函数。
///
/// 用法：runTicker(fn, intervalMs) → ticker 句柄
///
/// 启动一个独立线程，循环调用 fn()，每次间隔 intervalMs 毫秒。
/// 返回 TickerHandle，可通过 stopTicker(handle) 停止（或直接调用 handle.stop()）。
///
/// 实现说明（参考 vm.rs spawn_thread）：
///   - 新线程构造独立 VM，共享主线程的 globals 与 output 句柄
///   - 子线程内异常静默打印，不影响主线程
///   - 函数闭包与参数所有权转移到子线程
fn bi_run_ticker(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "runTicker")?;
    bh::require_arg(args, 1, "runTicker")?;

    // 校验第 1 个参数为函数值（Func 或 Builtin）
    let fn_val = args[0].clone();
    match &fn_val {
        Value::Func(_) | Value::Builtin(_) => {}
        v => return Err(crate::value::error_value(format!(
            "runTicker() 第 1 个参数应为 function，得到 {} (可能原因：参数顺序错误或未用 func 定义)",
            v.type_name(),
        ))),
    }

    let interval_ms = bh::as_int(args, 1, "runTicker")?;
    if interval_ms <= 0 {
        return Err(crate::value::error_value(format!(
            "runTicker() intervalMs 必须为正整数 (得到 {}) (可能原因：参数顺序错误或单位错误)",
            interval_ms,
        )));
    }

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();

    // 共享主线程的 globals 与 output 句柄，使子线程能访问全局变量与输出
    let globals = vm.globals_handle();
    let out = vm.output_handle();

    std::thread::spawn(move || {
        // 子线程构造独立 VM（独立栈/帧/调用深度），不与主线程共享栈
        let mut vm_child = VM::new();
        vm_child.set_globals_handle(globals);
        vm_child.set_output_handle(out);

        while !stop_clone.load(Ordering::Relaxed) {
            // 调用用户函数；异常静默打印，不影响后续循环
            match vm_child.call_function_value(fn_val.clone(), Vec::new()) {
                Ok(_) => {}
                Err(e) => {
                    // 异常输出到共享输出，提示但不中断 ticker
                    let msg = match &e {
                        Value::Error(er) => er.message.clone(),
                        other => other.to_str(),
                    };
                    let _ = writeln!(
                        vm_child.output_handle().lock().unwrap(),
                        "[runTicker 线程异常] {}",
                        msg,
                    );
                }
            }
            // 间隔休眠（每次循环检查 stop 标志，避免长时间阻塞）
            // 将总间隔拆分为不超过 100ms 的片段，以便及时响应 stop 信号
            let mut remaining = interval_ms as u64;
            while remaining > 0 && !stop_clone.load(Ordering::Relaxed) {
                let chunk = remaining.min(100);
                std::thread::sleep(std::time::Duration::from_millis(chunk));
                remaining -= chunk;
            }
        }
    });

    // 返回 TickerHandle 作为 Native 值，供 stopTicker/close 使用
    Ok(Value::Native(Arc::new(Arc::new(TickerHandle { stop: stop_flag }))))
}

/// bi_stop_ticker 停止 runTicker 启动的周期任务。
///
/// 用法：stopTicker(handle) → undefined
///
/// handle 为 runTicker 返回的 TickerHandle。
/// 调用后子线程会在当前周期结束后（最长 intervalMs）退出。
/// 幂等：对已停止的 ticker 调用无副作用。
fn bi_stop_ticker(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "stopTicker")?;
    match &args[0] {
        Value::Native(n) => {
            if let Some(h) = n.downcast_ref::<Arc<TickerHandle>>() {
                h.release();
                Ok(Value::Undefined)
            } else {
                Err(crate::value::error_value(format!(
                    "stopTicker() 参数不是 ticker 句柄 (可能原因：传入了其他 native 类型，应使用 runTicker 的返回值)",
                )))
            }
        }
        Value::Undefined => Err(crate::value::error_value(
            "stopTicker() 参数为 undefined (可能原因：runTicker 未被调用或返回值未保存)",
        )),
        other => Err(crate::value::error_value(format!(
            "stopTicker() 参数应为 ticker 句柄，得到 {} (可能原因：参数类型不匹配)",
            other.type_name(),
        ))),
    }
}

/// bi_format_time 格式化 datetime 为字符串（dtFormat 的别名）。
///
/// 用法：formatTime(dt, fmt) → string
///
/// fmt 为 Go 风格参考时间，如 "2006-01-02 15:04:05"。
/// 与 dtFormat 功能完全相同，提供更直观的命名。
fn bi_format_time(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let dt = as_dt(args, 0, "formatTime")?;
    let fmt = bh::as_str(args, 1, "formatTime")?;
    Ok(Value::str_from(dt.format(fmt)))
}
