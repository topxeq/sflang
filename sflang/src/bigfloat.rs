//! bigfloat.rs — 任意精度十进制浮点（自实现，复用 bigint）
//!
//! 设计要点：
//!   - 定点表示：值 = mantissa / 10^scale（mantissa 是 BigInt，scale 是小数位数）
//!   - 十进制精确：bigFloat("0.1") + bigFloat("0.2") = bigFloat("0.3")（避免 float 的精度问题）
//!   - 加减：对齐 scale 后运算；乘：scale 相加；除：指定结果小数位数（默认 20）
//!   - 复用 BigInt 的全部算术，只是多记一个 scale
//!
//! 适用场景：精确财务计算（货币）、需要避免二进制浮点误差的科学计算。
//! 不适用：极高精度（数千位）需求——那是 Native 扩展的领域。

use std::cmp::Ordering;

use crate::bigint::BigInt;

/// BigFloat 定点十进制浮点：值 = mantissa / 10^scale。
///
/// scale ≥ 0；mantissa 可为负。例如 bigFloat("1.23") = mantissa=123, scale=2。
#[derive(Debug, Clone)]
pub struct BigFloat {
    pub mantissa: BigInt,
    pub scale: u32,
}

/// DEFAULT_DIV_PRECISION 除法默认结果小数位数。
const DEFAULT_DIV_PRECISION: u32 = 20;

impl BigFloat {
    /// zero 零值。
    pub fn zero() -> Self {
        BigFloat { mantissa: BigInt::zero(), scale: 0 }
    }

    /// from_i64 从整数构造（scale=0）。
    pub fn from_i64(x: i64) -> Self {
        BigFloat { mantissa: BigInt::from_i64(x), scale: 0 }
    }

    /// from_bigint 从 BigInt 构造（scale=0）。
    pub fn from_bigint(b: BigInt) -> Self {
        BigFloat { mantissa: b, scale: 0 }
    }

    /// is_zero 是否为零。
    pub fn is_zero(&self) -> bool {
        self.mantissa.is_zero()
    }

    /// from_str_decimal 从十进制字符串解析（如 "3.14"、"-0.001"、"100"）。
    pub fn from_str_decimal(s: &str) -> Result<Self, String> {
        let s = s.trim();
        if s.is_empty() {
            return Err("bigFloat 解析：空字符串".into());
        }
        // 分离符号
        let (neg, body) = if let Some(r) = s.strip_prefix('-') {
            (true, r)
        } else if let Some(r) = s.strip_prefix('+') {
            (false, r)
        } else {
            (false, s)
        };
        // 分离小数点
        let (int_part, frac_part) = match body.find('.') {
            Some(pos) => (&body[..pos], &body[pos + 1..]),
            None => (body, ""),
        };
        // 校验：两部分都应为纯数字（int_part 可空如 ".5"，frac_part 可空如 "5."）
        if !int_part.chars().all(|c| c.is_ascii_digit()) || !frac_part.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!("bigFloat 解析：无效十进制 '{}'", s));
        }
        if int_part.is_empty() && frac_part.is_empty() {
            return Err(format!("bigFloat 解析：无效十进制 '{}'", s));
        }
        // 合并为纯整数字符串（去掉小数点），scale = 小数位数
        let combined = format!("{}{}", int_part, frac_part);
        let scale = frac_part.len() as u32;
        let signed = if neg { format!("-{}", combined) } else { combined };
        let mantissa = BigInt::from_str_decimal(&signed)?;
        Ok(BigFloat { mantissa, scale }.normalized())
    }

    /// normalized 归一化：去除尾数末尾的多余零（降低 scale）。
    fn normalized(mut self) -> Self {
        if self.mantissa.is_zero() {
            self.scale = 0;
            return self;
        }
        let ten = BigInt::from_i64(10);
        while self.scale > 0 {
            let (q, r) = self.mantissa.divmod(&ten).unwrap();
            if r.is_zero() {
                self.mantissa = q;
                self.scale -= 1;
            } else {
                break;
            }
        }
        self
    }

    /// to_string 转十进制字符串（去尾零，带负号）。
    pub fn to_string(&self) -> String {
        if self.mantissa.is_zero() {
            return "0".to_string();
        }
        let mag = self.mantissa.to_string_decimal();
        let neg = mag.starts_with('-');
        let digits = if neg { &mag[1..] } else { &mag[..] };
        let scale = self.scale as usize;
        let result = if scale == 0 {
            mag.clone()
        } else if digits.len() <= scale {
            // 补前导零：0.xxx
            format!("{}0.{}{}", if neg { "-" } else { "" }, "0".repeat(scale - digits.len()), digits)
        } else {
            // 插入小数点
            let pos = digits.len() - scale;
            format!("{}{}.{}", if neg { "-" } else { "" }, &digits[..pos], &digits[pos..])
        };
        // 去掉可能的 "-0.0..."（零值已早返回，此处理论上不会出现）
        result
    }

    // ---- 对齐 scale ----

    /// align_scale 返回一个等值但 scale == target 的 BigFloat（向左补零增加 scale）。
    fn align_scale(&self, target: u32) -> Self {
        if target <= self.scale {
            return self.clone();
        }
        let diff = target - self.scale;
        // mantissa * 10^diff
        let mut m = self.mantissa.clone();
        let ten_pow = pow_ten(diff);
        m = m.mul(&ten_pow);
        BigFloat { mantissa: m, scale: target }
    }

    // ---- 加减 ----

    /// add 加法：对齐到较大 scale 后尾数相加。
    pub fn add(&self, other: &Self) -> Self {
        let scale = self.scale.max(other.scale);
        let a = self.align_scale(scale);
        let b = other.align_scale(scale);
        BigFloat {
            mantissa: a.mantissa.add(&b.mantissa),
            scale,
        }.normalized()
    }

    /// sub 减法。
    pub fn sub(&self, other: &Self) -> Self {
        self.add(&other.negate())
    }

    /// negate 取负。
    pub fn negate(&self) -> Self {
        BigFloat { mantissa: self.mantissa.negate(), scale: self.scale }
    }

    // ---- 乘法 ----

    /// mul 乘法：尾数相乘，scale 相加。
    pub fn mul(&self, other: &Self) -> Self {
        BigFloat {
            mantissa: self.mantissa.mul(&other.mantissa),
            scale: self.scale + other.scale,
        }.normalized()
    }

    // ---- 除法 ----

    /// div 除法：self / other，结果保留 prec 位小数。
    ///
    /// 算法：将被除数放大 10^prec，使除法结果直接含目标精度。
    pub fn div(&self, other: &Self, prec: u32) -> Result<Self, String> {
        if other.is_zero() {
            return Err("bigFloat division by zero (除零错误)".into());
        }
        // 调整 scale：self.scale - other.scale + prec
        // 被除数 mantissa 放大 10^(prec + other.scale - self.scale)（若为正）
        let shift = prec as i64 + other.scale as i64 - self.scale as i64;
        let mut a = self.mantissa.clone();
        if shift > 0 {
            a = a.mul(&pow_ten(shift as u32));
        } else if shift < 0 {
            // 被除数反而要缩小（少见的 scale 差），用除法
            let (q, _r) = a.divmod(&pow_ten((-shift) as u32))?;
            a = q;
        }
        let b = other.mantissa.clone();
        let (q, _r) = a.divmod(&b)?;
        Ok(BigFloat { mantissa: q, scale: prec }.normalized())
    }

    /// div_default 除法（默认精度 20 位小数）。
    pub fn div_default(&self, other: &Self) -> Result<Self, String> {
        self.div(other, DEFAULT_DIV_PRECISION)
    }

    // ---- 比较 ----

    /// cmp 比较（先对齐 scale 再比尾数）。
    pub fn cmp(&self, other: &Self) -> Ordering {
        let scale = self.scale.max(other.scale);
        let a = self.align_scale(scale);
        let b = other.align_scale(scale);
        a.mantissa.cmp(&b.mantissa)
    }
}

/// pow_ten 计算 10^n（返回 BigInt）。
fn pow_ten(n: u32) -> BigInt {
    let mut result = BigInt::one();
    let ten = BigInt::from_i64(10);
    for _ in 0..n {
        result = result.mul(&ten);
    }
    result
}

impl std::fmt::Display for BigFloat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse() {
        assert_eq!(BigFloat::from_str_decimal("3.14").unwrap().to_string(), "3.14");
        assert_eq!(BigFloat::from_str_decimal("-0.001").unwrap().to_string(), "-0.001");
        assert_eq!(BigFloat::from_str_decimal("100").unwrap().to_string(), "100");
        assert_eq!(BigFloat::from_str_decimal(".5").unwrap().to_string(), "0.5");
        // 去尾零
        assert_eq!(BigFloat::from_str_decimal("1.2300").unwrap().to_string(), "1.23");
    }

    #[test]
    fn test_add_sub() {
        let a = BigFloat::from_str_decimal("0.1").unwrap();
        let b = BigFloat::from_str_decimal("0.2").unwrap();
        // 验证避免 float 精度问题：0.1 + 0.2 = 0.3（精确）
        assert_eq!(a.add(&b).to_string(), "0.3");
        assert_eq!(a.sub(&b).to_string(), "-0.1");
        // 不同 scale 对齐
        let c = BigFloat::from_str_decimal("1.5").unwrap();
        let d = BigFloat::from_str_decimal("2.25").unwrap();
        assert_eq!(c.add(&d).to_string(), "3.75");
    }

    #[test]
    fn test_mul() {
        let a = BigFloat::from_str_decimal("1.1").unwrap();
        assert_eq!(a.mul(&a).to_string(), "1.21");
        let b = BigFloat::from_str_decimal("0.5").unwrap();
        assert_eq!(BigFloat::from_str_decimal("10").unwrap().mul(&b).to_string(), "5");
    }

    #[test]
    fn test_div() {
        let a = BigFloat::from_str_decimal("1").unwrap();
        let b = BigFloat::from_str_decimal("3").unwrap();
        let r = a.div(&b, 5).unwrap();
        assert_eq!(r.to_string(), "0.33333");
        // 除零
        assert!(a.div(&BigFloat::zero(), 5).is_err());
    }

    #[test]
    fn test_cmp() {
        let a = BigFloat::from_str_decimal("0.1").unwrap();
        let b = BigFloat::from_str_decimal("0.2").unwrap();
        assert_eq!(a.cmp(&b), Ordering::Less);
        assert_eq!(b.cmp(&a), Ordering::Greater);
        // 不同 scale 等值
        let c = BigFloat::from_str_decimal("0.10").unwrap();
        assert_eq!(a.cmp(&c), Ordering::Equal);
    }

    #[test]
    fn test_large() {
        // 大数 + 小数
        let big = BigFloat::from_str_decimal("99999999999999999999.99").unwrap();
        let small = BigFloat::from_str_decimal("0.01").unwrap();
        assert_eq!(big.add(&small).to_string(), "100000000000000000000");
    }
}
