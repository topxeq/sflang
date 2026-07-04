//! builtins_math.rs — 数学内置函数
//!
//! 设计要点（来自 AGENTS.md）：
//!   - 仅依赖 Rust 标准库（不引入第三方数学库）
//!   - 数值类型自动处理 Int/Float 提升
//!   - 随机数用进程内 LCG，种子取自启动时刻纳秒（可移植、无外部依赖）
//!
//! 函数列表：
//!   abs floor ceil round sqrt pow sign
//!   min max（可变参 + 单个数组两种形式）
//!   sin cos tan atan atan2 log log2 log10 exp
//!   pi e（常量，也作为全局 piG / eG 暴露）
//!   random（[0,1) 浮点）randInt(lo, hi)（含端点整数）

use std::sync::{Mutex, OnceLock};

use crate::builtins_helpers as bh;
use crate::value::Value;
use crate::vm::VM;

/// register 注册所有数学内置函数到 VM。
pub fn register(vm: &mut VM) {
    vm.register_builtin("abs", bi_abs);
    vm.register_builtin("floor", bi_floor);
    vm.register_builtin("ceil", bi_ceil);
    vm.register_builtin("round", bi_round);
    vm.register_builtin("sqrt", bi_sqrt);
    vm.register_builtin("pow", bi_pow);
    vm.register_builtin("sign", bi_sign);
    vm.register_builtin("min", bi_min);
    vm.register_builtin("max", bi_max);
    vm.register_builtin("sin", bi_sin);
    vm.register_builtin("cos", bi_cos);
    vm.register_builtin("tan", bi_tan);
    vm.register_builtin("atan", bi_atan);
    vm.register_builtin("atan2", bi_atan2);
    vm.register_builtin("log", bi_log);
    vm.register_builtin("log2", bi_log2);
    vm.register_builtin("log10", bi_log10);
    vm.register_builtin("exp", bi_exp);
    vm.register_builtin("pi", bi_pi);
    vm.register_builtin("e", bi_e);
    vm.register_builtin("random", bi_random);
    vm.register_builtin("randInt", bi_rand_int);
}

/// 对浮点结果按需装回 Int（若为整数值），否则保持 Float。
///
/// 范围检查用 i64::MAX 的精确浮点表示，避免 9.2e18 这类近似截断导致
/// [9.2e18, 9.2234e18) 区间的整数值被错误返回为 Float。
fn num(f: f64) -> Value {
    if f.is_finite() && f.fract() == 0.0 && f.abs() <= (i64::MAX as f64) {
        Value::Int(f as i64)
    } else {
        Value::Float(f)
    }
}

/// to_i64_checked 将 f64 安全转为 i64，越界返回 None。
///
/// 用于 floor/ceil/round 等"结果必为整数"的函数：输入超出 i64 范围时报错，
/// 而非静默饱和到 i64::MAX/MIN（避免返回错误结果）。
fn to_i64_checked(f: f64) -> Option<i64> {
    if f.is_finite() && f >= (i64::MIN as f64) && f <= (i64::MAX as f64) {
        Some(f as i64)
    } else {
        None
    }
}

/// bi_abs 绝对值。
fn bi_abs(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    match &args[0] {
        Value::Int(i) => Ok(Value::Int(i.wrapping_abs())),
        _ => Ok(num(bh::as_float(args, 0, "abs")?.abs())),
    }
}

/// bi_floor 向下取整（结果为 Int）。
///
/// 输入超出 i64 范围时报错（而非静默饱和）。
fn bi_floor(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let f = bh::as_float(args, 0, "floor")?.floor();
    match to_i64_checked(f) {
        Some(i) => Ok(Value::Int(i)),
        None => Err(crate::value::error_value(format!(
            "floor() 结果 {} 超出整数范围 (可能原因：输入值过大)", f,
        ))),
    }
}

/// bi_ceil 向上取整（结果为 Int）。
fn bi_ceil(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let f = bh::as_float(args, 0, "ceil")?.ceil();
    match to_i64_checked(f) {
        Some(i) => Ok(Value::Int(i)),
        None => Err(crate::value::error_value(format!(
            "ceil() 结果 {} 超出整数范围 (可能原因：输入值过大)", f,
        ))),
    }
}

/// bi_round 四舍五入到最近整数（结果为 Int）。
fn bi_round(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let f = bh::as_float(args, 0, "round")?.round();
    match to_i64_checked(f) {
        Some(i) => Ok(Value::Int(i)),
        None => Err(crate::value::error_value(format!(
            "round() 结果 {} 超出整数范围 (可能原因：输入值过大)", f,
        ))),
    }
}

/// bi_sqrt 平方根（返回 Float）。
fn bi_sqrt(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let x = bh::as_float(args, 0, "sqrt")?;
    if x < 0.0 {
        return Err(crate::value::error_value(
            "sqrt() 参数不能为负数 (可能原因：传入了负值或表达式结果为负)",
        ));
    }
    Ok(Value::Float(x.sqrt()))
}

/// bi_pow 幂运算 base^exp。
///
/// 结果若为整数值则返回 Int，否则返回 Float。
fn bi_pow(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let base = bh::as_float(args, 0, "pow")?;
    let exp = bh::as_float(args, 1, "pow")?;
    Ok(num(base.powf(exp)))
}

/// bi_sign 符号函数：-1 / 0 / 1。
fn bi_sign(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let x = bh::as_float(args, 0, "sign")?;
    Ok(Value::Int(if x > 0.0 {
        1
    } else if x < 0.0 {
        -1
    } else {
        0
    }))
}

/// 归约辅助：对一组数值（可变参或单个数组）应用比较，返回极值 Value。
fn reduce_num<F: Fn(f64, f64) -> f64>(args: &[Value], fn_name: &str, cmp: F) -> Result<Value, Value> {
    // 单参数且为数组：按数组元素归约；否则按可变参处理。
    let values: Vec<f64> = if args.len() == 1 {
        if let Value::Array(a) = &args[0] {
            a.lock().unwrap().iter().map(|v| v.to_f64()).collect::<Option<_>>()
                .ok_or_else(|| crate::value::error_value(format!(
                    "{}() 数组元素需全部为数字 (可能原因：数组含非数字类型)", fn_name)))?
        } else {
            vec![bh::as_float(args, 0, fn_name)?]
        }
    } else {
        (0..args.len())
            .map(|i| bh::as_float(args, i, fn_name))
            .collect::<Result<Vec<_>, _>>()?
    };
    if values.is_empty() {
        return Err(crate::value::error_value(format!(
            "{}() 至少需要 1 个参数 (可能原因：传入空数组或无参数)", fn_name)));
    }
    let mut acc = values[0];
    for &x in &values[1..] {
        acc = cmp(acc, x);
    }
    Ok(num(acc))
}

/// bi_min 最小值（可变参或单个数组）。
fn bi_min(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    reduce_num(args, "min", |a, b| if a < b { a } else { b })
}

/// bi_max 最大值（可变参或单个数组）。
fn bi_max(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    reduce_num(args, "max", |a, b| if a > b { a } else { b })
}

/// bi_sin 正弦（弧度）。
fn bi_sin(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Float(bh::as_float(args, 0, "sin")?.sin()))
}

/// bi_cos 余弦（弧度）。
fn bi_cos(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Float(bh::as_float(args, 0, "cos")?.cos()))
}

/// bi_tan 正切（弧度）。
fn bi_tan(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Float(bh::as_float(args, 0, "tan")?.tan()))
}

/// bi_atan 反正切。
fn bi_atan(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Float(bh::as_float(args, 0, "atan")?.atan()))
}

/// bi_atan2 反正切（两参数 atan2(y, x)）。
fn bi_atan2(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let y = bh::as_float(args, 0, "atan2")?;
    let x = bh::as_float(args, 1, "atan2")?;
    Ok(Value::Float(y.atan2(x)))
}

/// bi_log 自然对数。
fn bi_log(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let x = bh::as_float(args, 0, "log")?;
    if x <= 0.0 {
        return Err(crate::value::error_value(
            "log() 参数需为正数 (可能原因：传入了 0 或负数)",
        ));
    }
    Ok(Value::Float(x.ln()))
}

/// bi_log2 以 2 为底对数。
fn bi_log2(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let x = bh::as_float(args, 0, "log2")?;
    if x <= 0.0 {
        return Err(crate::value::error_value(
            "log2() 参数需为正数 (可能原因：传入了 0 或负数)",
        ));
    }
    Ok(Value::Float(x.log2()))
}

/// bi_log10 以 10 为底对数。
fn bi_log10(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let x = bh::as_float(args, 0, "log10")?;
    if x <= 0.0 {
        return Err(crate::value::error_value(
            "log10() 参数需为正数 (可能原因：传入了 0 或负数)",
        ));
    }
    Ok(Value::Float(x.log10()))
}

/// bi_exp 自然指数 e^x。
fn bi_exp(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Float(bh::as_float(args, 0, "exp")?.exp()))
}

/// bi_pi 返回圆周率。
fn bi_pi(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Float(std::f64::consts::PI))
}

/// bi_e 返回自然常数。
fn bi_e(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Float(std::f64::consts::E))
}

// ---- 随机数实现 ----
//
// 使用 xorshift64，进程级共享单一状态（Mutex 保护，跨线程安全且流不重复）。
// 种子取自首次调用时 SystemTime 的纳秒，避免外部依赖。
// 阶段三：从 thread_local 改为进程级 Mutex，避免多线程流重复（修复 #7）。

/// rng_state 进程级共享 RNG 状态（懒初始化）。
fn rng_state() -> &'static Mutex<u64> {
    static STATE: OnceLock<Mutex<u64>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(seed_init()))
}

/// seed_init 取一个非零初始种子（基于启动时刻）。
fn seed_init() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E3779B97F4A7C15);
    // 保证非零
    if nanos == 0 {
        0x9E3779B97F4A7C15
    } else {
        nanos
    }
}

/// next_rand 推进一步 xorshift64，返回下一个 u64（线程安全，加锁）。
/// pub 供 randomStr 等其他模块复用。
pub fn next_rand() -> u64 {
    let mut guard = rng_state().lock().unwrap();
    let mut x = *guard;
    if x == 0 {
        x = 0x9E3779B97F4A7C15;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *guard = x;
    x
}

/// next_f64 取 [0, 1) 浮点随机数。
fn next_f64() -> f64 {
    // 取高 53 位作为尾数，构造 [0,1)。
    let bits = next_rand() >> 11;
    (bits as f64) / ((1u64 << 53) as f64)
}

/// bi_random 返回 [0, 1) 浮点随机数。
fn bi_random(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Float(next_f64()))
}

/// bi_randInt 返回 [lo, hi] 闭区间内的随机整数。
///
/// 注意两端都包含。若 lo > hi 则交换。
fn bi_rand_int(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let mut lo = bh::as_int(args, 0, "randInt")?;
    let mut hi = bh::as_int(args, 1, "randInt")?;
    if lo > hi {
        std::mem::swap(&mut lo, &mut hi);
    }
    // 用 u128 计算 span，避免 hi=i64::MAX 时 hi-lo+1 溢出 i64/u64。
    // 最大 span = i64::MAX - i64::MIN + 1 = 2^64，超出 u64 范围，故用 u128。
    let span = (hi as i128 - lo as i128 + 1) as u128;
    let r = ((next_rand() as u128) % span) as i64;
    Ok(Value::Int(lo + r))
}
