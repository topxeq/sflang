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
    vm.register_builtin("flexEval", bi_flex_eval);
    vm.register_builtin("calDistanceOfLatLon", bi_cal_distance_of_lat_lon);
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

// ---- 表达式求值（递归下降解析器）----
//
// 支持 + - * / % 和括号，整数和浮点数。
// 不支持变量、函数调用、幂运算等复杂特性（保持简单）。
// 返回 int 或 float：若表达式仅含整数且运算结果为整数则返回 int，
// 否则返回 float。

/// EvalParser 表达式求值解析器。
struct EvalParser<'a> {
    // 输入字节切片
    src: &'a [u8],
    // 当前位置
    pos: usize,
}

impl<'a> EvalParser<'a> {
    fn new(src: &'a str) -> Self {
        EvalParser { src: src.as_bytes(), pos: 0 }
    }

    /// skip_ws 跳过空白字符。
    fn skip_ws(&mut self) {
        while self.pos < self.src.len() && self.src[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    /// peek 当前字节（不消费）。
    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    /// parse_expr 解析表达式（顶层：加减）。
    fn parse_expr(&mut self) -> Result<f64, String> {
        let mut left = self.parse_term()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'+') => {
                    self.pos += 1;
                    let right = self.parse_term()?;
                    left += right;
                }
                Some(b'-') => {
                    self.pos += 1;
                    let right = self.parse_term()?;
                    left -= right;
                }
                _ => break,
            }
        }
        Ok(left)
    }

    /// parse_term 解析项（乘除模）。
    fn parse_term(&mut self) -> Result<f64, String> {
        let mut left = self.parse_factor()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'*') => {
                    self.pos += 1;
                    let right = self.parse_factor()?;
                    left *= right;
                }
                Some(b'/') => {
                    self.pos += 1;
                    let right = self.parse_factor()?;
                    if right == 0.0 {
                        return Err("除以零 (可能原因：表达式中分母为 0)".to_string());
                    }
                    left /= right;
                }
                Some(b'%') => {
                    self.pos += 1;
                    let right = self.parse_factor()?;
                    if right == 0.0 {
                        return Err("模零 (可能原因：表达式中取模的除数为 0)".to_string());
                    }
                    left = left % right;
                }
                _ => break,
            }
        }
        Ok(left)
    }

    /// parse_factor 解析因子（数字 / 括号 / 一元正负号）。
    fn parse_factor(&mut self) -> Result<f64, String> {
        self.skip_ws();
        match self.peek() {
            Some(b'(') => {
                self.pos += 1;
                let v = self.parse_expr()?;
                self.skip_ws();
                if self.peek() != Some(b')') {
                    return Err("缺少右括号 ')' (可能原因：括号未闭合)".to_string());
                }
                self.pos += 1;
                Ok(v)
            }
            Some(b'+') => {
                self.pos += 1;
                self.parse_factor()
            }
            Some(b'-') => {
                self.pos += 1;
                let v = self.parse_factor()?;
                Ok(-v)
            }
            Some(c) if c.is_ascii_digit() || c == b'.' => {
                self.parse_number()
            }
            other => Err(format!(
                "意外的字符 '{}' (可能原因：表达式语法错误)",
                other.map(|c| c as char).unwrap_or('?'),
            )),
        }
    }

    /// parse_number 解析数字（整数或浮点）。
    fn parse_number(&mut self) -> Result<f64, String> {
        let start = self.pos;
        // 整数部分
        while self.pos < self.src.len() && self.src[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        // 小数部分
        if self.pos < self.src.len() && self.src[self.pos] == b'.' {
            self.pos += 1;
            while self.pos < self.src.len() && self.src[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        // 科学计数法（e/E[+/-]digits）
        if self.pos < self.src.len() && (self.src[self.pos] == b'e' || self.src[self.pos] == b'E') {
            self.pos += 1;
            if self.pos < self.src.len() && (self.src[self.pos] == b'+' || self.src[self.pos] == b'-') {
                self.pos += 1;
            }
            while self.pos < self.src.len() && self.src[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        let s = std::str::from_utf8(&self.src[start..self.pos])
            .map_err(|_| "数字解析失败 (UTF-8 异常)".to_string())?;
        s.parse::<f64>().map_err(|_| format!("无效数字: '{}' (可能原因：数字格式错误)", s))
    }
}

/// bi_flex_eval 表达式求值。
///
/// 用法：flexEval(expr) → int 或 float
///
/// 支持四则运算（+ - * /）、取模（%）和括号。
/// 支持整数和浮点数（含科学计数法如 1e3）。
/// 一元正负号支持（如 -5、+3.14）。
///
/// 返回值：若表达式仅含整数且结果为整数则返回 int，否则返回 float。
/// 除零、模零、语法错误均返回错误对象。
fn bi_flex_eval(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let expr = bh::as_str(args, 0, "flexEval")?;
    if expr.trim().is_empty() {
        return Err(crate::value::error_value(
            "flexEval() 表达式为空 (可能原因：未传入表达式或仅含空白字符)",
        ));
    }
    let mut parser = EvalParser::new(expr);
    let result = parser.parse_expr().map_err(|e| crate::value::error_value(format!(
        "flexEval() 解析失败: {} (表达式: '{}')", e, expr,
    )))?;

    // 检查是否所有输入都已消费（避免 "1 + 2 xxx" 这类被静默接受）
    parser.skip_ws();
    if parser.pos < parser.src.len() {
        let rest = std::str::from_utf8(&parser.src[parser.pos..]).unwrap_or("");
        return Err(crate::value::error_value(format!(
            "flexEval() 表达式末尾有未识别的内容: '{}' (可能原因：多余的字符或语法错误)", rest,
        )));
    }

    // 结果若为整数值则返回 int，否则返回 float
    Ok(num(result))
}

/// bi_cal_distance_of_lat_lon 经纬度距离计算（Haversine 公式）。
///
/// 用法：calDistanceOfLatLon(lat1, lon1, lat2, lon2) → float (米)
///
/// lat/lon 为十进制度数（东经/北纬为正，西经/南纬为负）。
/// 返回两点之间的球面距离，单位为米。
/// 地球半径取 6371000 米（平均半径）。
fn bi_cal_distance_of_lat_lon(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let lat1 = bh::as_float(args, 0, "calDistanceOfLatLon")?;
    let lon1 = bh::as_float(args, 1, "calDistanceOfLatLon")?;
    let lat2 = bh::as_float(args, 2, "calDistanceOfLatLon")?;
    let lon2 = bh::as_float(args, 3, "calDistanceOfLatLon")?;

    // 地球半径（米）
    const R: f64 = 6_371_000.0;

    // 度 → 弧度
    let to_rad = |d: f64| d * std::f64::consts::PI / 180.0;

    let lat1_rad = to_rad(lat1);
    let lat2_rad = to_rad(lat2);
    let d_lat = to_rad(lat2 - lat1);
    let d_lon = to_rad(lon2 - lon1);

    // Haversine 公式
    let a = (d_lat / 2.0).sin().powi(2)
        + lat1_rad.cos() * lat2_rad.cos() * (d_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    let d = R * c;

    Ok(Value::Float(d))
}
