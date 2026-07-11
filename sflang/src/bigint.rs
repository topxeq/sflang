//! bigint.rs — 任意精度有符号整数（自实现，不依赖第三方库）
//!
//! 设计要点（AGENTS.md：尽量只用标准库）：
//!   - 内部用 Vec<u32> limbs 存储，base = 2^32，小端序（limbs[0] 是最低位）
//!   - 独立的 negative 符号位；零恒为 negative=false 且 limbs 为空（归一化后）
//!   - 支持加减乘除（教科书算法）、比较、与 i64 互转、十进制字符串解析
//!   - 不实现快速乘法（Karatsuba/FFT）与位运算——脚本层大数场景用不到
//!   - Send + Sync（Vec<u32> 自动满足），可作为 Value 变体跨 run 线程共享
//!
//! 算法复杂度：
//!   - 加减：O(n)
//!   - 乘：O(n*m) 教科书（n/m 为两操作数 limb 数）
//!   - 除：O(n*m) 长除法
//!   对于几百位数字足够快（微秒级）。数千位以上加密场景建议用 Native 扩展。

use std::cmp::Ordering;

/// BASE 每个 limb 表示的基数（2^32）。运算时中间结果用 u64 容纳进位。
const BASE: u64 = 1u64 << 32;

/// BigInt 任意精度有符号整数。
///
/// 内部表示：值 = (negative ? -1 : 1) * Σ limbs[i] * BASE^i
/// 归一化不变式：无前导零 limb（末尾非零，除非是零值）；零的 negative=false 且 limbs 空。
#[derive(Debug, Clone)]
pub struct BigInt {
    /// limbs 小端序数字数组（limbs[0] 最低位）。归一化后无前导零。
    limbs: Vec<u32>,
    /// negative 是否为负数。零值固定为 false。
    negative: bool,
}

impl BigInt {
    /// zero 零值。
    pub fn zero() -> Self {
        BigInt { limbs: Vec::new(), negative: false }
    }

    /// one 正一。
    pub fn one() -> Self {
        BigInt { limbs: vec![1], negative: false }
    }

    /// is_zero 是否为零。
    pub fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    /// from_i64 从 i64 构造。
    pub fn from_i64(x: i64) -> Self {
        if x == 0 {
            return Self::zero();
        }
        let negative = x < 0;
        // 取绝对值（i64::MIN 的绝对值溢出，用 wrapping_neg 转 u64 处理）
        let mag = (x as i128).unsigned_abs() as u128;
        let mut limbs = Vec::new();
        let mut m = mag;
        while m > 0 {
            limbs.push((m & 0xFFFF_FFFF) as u32);
            m >>= 32;
        }
        BigInt { limbs, negative }
    }

    /// normalize 去除前导零 limb，并修正零的符号。
    fn normalize(mut self) -> Self {
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
        if self.limbs.is_empty() {
            self.negative = false; // 零不为负
        }
        self
    }

    /// to_i64 若值在 i64 范围内则返回，否则 None。
    pub fn to_i64(&self) -> Option<i64> {
        if self.limbs.len() > 2 {
            return None;
        }
        let mut mag: u64 = 0;
        if let Some(&lo) = self.limbs.get(0) {
            mag = lo as u64;
        }
        if let Some(&hi) = self.limbs.get(1) {
            mag |= (hi as u64) << 32;
        }
        if self.negative {
            // 负数：mag 不能超过 2^63（i64::MIN 的绝对值）
            if mag > (1u64 << 63) {
                return None;
            }
            // i64::MIN 的 mag 恰为 2^63，需特殊处理
            if mag == (1u64 << 63) {
                return Some(i64::MIN);
            }
            Some(-(mag as i64))
        } else {
            if mag > i64::MAX as u64 {
                return None;
            }
            Some(mag as i64)
        }
    }

    /// negate 取负。
    pub fn negate(&self) -> Self {
        if self.is_zero() {
            Self::zero()
        } else {
            BigInt { limbs: self.limbs.clone(), negative: !self.negative }
        }
    }

    /// abs 绝对值。
    pub fn abs(&self) -> Self {
        BigInt { limbs: self.limbs.clone(), negative: false }
    }

    // ---- 比较 ----

    /// cmp_unsigned 比较两数的绝对值大小（忽略符号）。
    fn cmp_unsigned(a: &[u32], b: &[u32]) -> Ordering {
        if a.len() != b.len() {
            return a.len().cmp(&b.len());
        }
        // 高位在数组末尾，从高位比
        for i in (0..a.len()).rev() {
            match a[i].cmp(&b[i]) {
                Ordering::Equal => continue,
                ord => return ord,
            }
        }
        Ordering::Equal
    }

    /// cmp 比较（考虑符号）。
    pub fn cmp(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (false, true) => Ordering::Greater, // 正 > 负
            (true, false) => Ordering::Less,    // 负 < 正
            (false, false) => Self::cmp_unsigned(&self.limbs, &other.limbs),
            (true, true) => Self::cmp_unsigned(&other.limbs, &self.limbs), // 都负：绝对值大的更小
        }
    }

    // ---- 加减法 ----

    /// add_unsigned 无符号加法 a + b（忽略符号）。
    fn add_unsigned(a: &[u32], b: &[u32]) -> Vec<u32> {
        let mut result = Vec::with_capacity(a.len().max(b.len()) + 1);
        let mut carry: u64 = 0;
        let n = a.len().max(b.len());
        for i in 0..n {
            let av = *a.get(i).unwrap_or(&0) as u64;
            let bv = *b.get(i).unwrap_or(&0) as u64;
            let sum = av + bv + carry;
            result.push((sum & 0xFFFF_FFFF) as u32);
            carry = sum >> 32;
        }
        if carry > 0 {
            result.push(carry as u32);
        }
        result
    }

    /// sub_unsigned 无符号减法 a - b（要求 |a| >= |b|）。
    fn sub_unsigned(a: &[u32], b: &[u32]) -> Vec<u32> {
        let mut result = Vec::with_capacity(a.len());
        let mut borrow: i64 = 0;
        for i in 0..a.len() {
            let av = a[i] as i64;
            let bv = *b.get(i).unwrap_or(&0) as i64;
            let mut diff = av - bv - borrow;
            if diff < 0 {
                diff += 1i64 << 32;
                borrow = 1;
            } else {
                borrow = 0;
            }
            result.push(diff as u32);
        }
        result
    }

    /// add 加法。
    pub fn add(&self, other: &Self) -> Self {
        if self.negative == other.negative {
            // 同号：绝对值相加，符号不变
            let limbs = Self::add_unsigned(&self.limbs, &other.limbs);
            BigInt { limbs, negative: self.negative }.normalize()
        } else {
            // 异号：绝对值相减
            match Self::cmp_unsigned(&self.limbs, &other.limbs) {
                Ordering::Equal => Self::zero(), // 相反数相加为零
                Ordering::Greater => {
                    // |self| > |other|，符号同 self
                    let limbs = Self::sub_unsigned(&self.limbs, &other.limbs);
                    BigInt { limbs, negative: self.negative }.normalize()
                }
                Ordering::Less => {
                    let limbs = Self::sub_unsigned(&other.limbs, &self.limbs);
                    BigInt { limbs, negative: other.negative }.normalize()
                }
            }
        }
    }

    /// sub 减法（self - other = self + (-other)）。
    pub fn sub(&self, other: &Self) -> Self {
        self.add(&other.negate())
    }

    // ---- 乘法 ----

    /// mul 乘法（教科书 O(n*m)）。
    pub fn mul(&self, other: &Self) -> Self {
        if self.is_zero() || other.is_zero() {
            return Self::zero();
        }
        let a = &self.limbs;
        let b = &other.limbs;
        let mut result = vec![0u32; a.len() + b.len()];
        for i in 0..a.len() {
            let mut carry: u64 = 0;
            let av = a[i] as u64;
            for j in 0..b.len() {
                let bv = b[j] as u64;
                let cur = result[i + j] as u64;
                let prod = av * bv + cur + carry;
                result[i + j] = (prod & 0xFFFF_FFFF) as u32;
                carry = prod >> 32;
            }
            // 处理剩余进位
            let mut k = i + b.len();
            while carry > 0 {
                let cur = result[k] as u64;
                let sum = cur + carry;
                result[k] = (sum & 0xFFFF_FFFF) as u32;
                carry = sum >> 32;
                k += 1;
            }
        }
        // 符号：异号为负
        let negative = self.negative != other.negative;
        BigInt { limbs: result, negative }.normalize()
    }

    // ---- 除法 ----

    /// divmod 除法与取模，返回 (商, 余数)。
    ///
    /// 语义对齐 Rust/Go 的带符号除法：商向零取整，余数与被除数同号。
    /// 除零返回 Err。
    pub fn divmod(&self, divisor: &Self) -> Result<(Self, Self), String> {
        if divisor.is_zero() {
            return Err("bigInt division by zero (除零错误；可能原因：除数为 0)".into());
        }
        // 按绝对值做无符号除法
        let (q_mag, r_mag) = Self::divmod_unsigned(&self.abs().limbs, &divisor.abs().limbs);
        // 商的符号：异号为负
        let q_neg = self.negative != divisor.negative && !q_mag.is_empty();
        let quotient = BigInt { limbs: q_mag, negative: q_neg }.normalize();
        // 余数的符号：与被除数同号（truncated division）
        let r_neg = self.negative && !r_mag.is_empty();
        let remainder = BigInt { limbs: r_mag, negative: r_neg }.normalize();
        Ok((quotient, remainder))
    }

    /// divmod_unsigned 无符号除法，返回 (商, 余数)。
    ///
    /// 算法：长除法 + 二分搜索商位。对每个商位用二分查找最大的 q 使 q*b ≤ 当前余数。
    /// 二分范围 [0, BASE)，最多 32 次比较，循环严格有界——彻底避免死循环。
    /// 复杂度 O(limbs² × 32)，对脚本场景（几百位）足够快。
    fn divmod_unsigned(a: &[u32], b: &[u32]) -> (Vec<u32>, Vec<u32>) {
        // 单 limb 除数：快速路径
        if b.len() == 1 {
            return divmod_single(a, b[0]);
        }
        if Self::cmp_unsigned(a, b) == Ordering::Less {
            return (Vec::new(), a.to_vec()); // 商 0，余 a
        }
        let mut quotient = vec![0u32; a.len()];
        let mut remainder: Vec<u32> = Vec::new();
        // 从高位到低位逐 limb 处理
        for i in (0..a.len()).rev() {
            // remainder = remainder * BASE + a[i]
            remainder.insert(0, a[i]);
            while remainder.last() == Some(&0) {
                remainder.pop();
            }
            if Self::cmp_unsigned(&remainder, b) == Ordering::Less {
                // 商位为 0
                quotient[i] = 0;
                continue;
            }
            // 二分搜索最大的 q ∈ [1, BASE) 使 q * b ≤ remainder
            let mut lo: u64 = 1;
            let mut hi: u64 = BASE - 1;
            while lo < hi {
                let mid = (lo + hi + 1) / 2; // 向上取整避免死循环
                let prod = mul_small(b, mid as u32);
                if Self::cmp_unsigned(&prod, &remainder) != Ordering::Greater {
                    lo = mid; // mid*b ≤ remainder，可取更大
                } else {
                    hi = mid - 1; // mid*b > remainder，缩小
                }
            }
            let q = lo as u32;
            // 执行减法：remainder -= q * b
            let prod = mul_small(b, q);
            remainder = Self::sub_unsigned(&remainder, &prod);
            while remainder.last() == Some(&0) {
                remainder.pop();
            }
            quotient[i] = q;
        }
        (quotient, remainder)
    }

    // ---- 字符串解析与输出 ----

    /// from_str_decimal 从十进制字符串解析（允许前导 +/-，忽略前后空白）。
    pub fn from_str_decimal(s: &str) -> Result<Self, String> {
        let s = s.trim();
        if s.is_empty() {
            return Err("bigInt 解析：空字符串".into());
        }
        let (negative, digits) = if let Some(rest) = s.strip_prefix('-') {
            (true, rest)
        } else if let Some(rest) = s.strip_prefix('+') {
            (false, rest)
        } else {
            (false, s)
        };
        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!("bigInt 解析：无效十进制 '{}'", s));
        }
        // 逐位累加：result = result * 10 + digit
        let mut result = Self::zero();
        let ten = Self::from_small(10);
        for c in digits.chars() {
            result = result.mul(&ten);
            result = result.add(&Self::from_small((c as u8 - b'0') as u32));
        }
        result.negative = negative && !result.is_zero();
        Ok(result.normalize())
    }

    /// from_small 从 u32 构造（非负）。
    fn from_small(x: u32) -> Self {
        if x == 0 {
            return Self::zero();
        }
        BigInt { limbs: vec![x], negative: false }
    }

    /// to_string_decimal 转十进制字符串（带负号）。
    pub fn to_string_decimal(&self) -> String {
        if self.is_zero() {
            return "0".to_string();
        }
        // 反复除以 10^9 取余数（每次处理 9 位，减少除法次数）
        let mut digits = String::new();
        let mut n = self.abs();
        let ten9 = Self::from_small(1_000_000_000);
        while !n.is_zero() {
            let (q, r) = n.divmod(&ten9).unwrap();
            let r_val = if r.is_zero() {
                0u32
            } else {
                r.limbs[0]
            };
            if q.is_zero() {
                // 最高段：不加前导零
                digits = format!("{}{}", r_val, digits);
            } else {
                // 中间段：补前导零到 9 位
                digits = format!("{:09}{}", r_val, digits);
            }
            n = q;
        }
        if self.negative {
            format!("-{}", digits)
        } else {
            digits
        }
    }
}

/// divmod_single 单 limb 除数快速路径：a / d，返回 (商, 余数 limb)。
fn divmod_single(a: &[u32], d: u32) -> (Vec<u32>, Vec<u32>) {
    let d = d as u64;
    let mut quotient = vec![0u32; a.len()];
    let mut rem: u64 = 0;
    for i in (0..a.len()).rev() {
        let cur = (rem << 32) | (a[i] as u64);
        quotient[i] = (cur / d) as u32;
        rem = cur % d;
    }
    let remainder = if rem == 0 { Vec::new() } else { vec![rem as u32] };
    (quotient, remainder)
}

/// mul_small 大数乘以小 u32。（保留：to_string_decimal 等仍用）
fn mul_small(a: &[u32], k: u32) -> Vec<u32> {
    if k == 0 {
        return Vec::new();
    }
    let mut result = Vec::with_capacity(a.len() + 1);
    let mut carry: u64 = 0;
    let k = k as u64;
    for &limb in a {
        let prod = (limb as u64) * k + carry;
        result.push((prod & 0xFFFF_FFFF) as u32);
        carry = prod >> 32;
    }
    if carry > 0 {
        result.push(carry as u32);
    }
    result
}

impl std::fmt::Display for BigInt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string_decimal())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic() {
        assert_eq!(BigInt::zero().to_string_decimal(), "0");
        assert_eq!(BigInt::from_i64(42).to_string_decimal(), "42");
        assert_eq!(BigInt::from_i64(-42).to_string_decimal(), "-42");
        assert_eq!(BigInt::from_i64(0).to_string_decimal(), "0");
        assert_eq!(BigInt::from_i64(i64::MAX).to_string_decimal(), i64::MAX.to_string());
        assert_eq!(BigInt::from_i64(i64::MIN).to_string_decimal(), i64::MIN.to_string());
    }

    #[test]
    fn test_add_sub() {
        let a = BigInt::from_i64(123);
        let b = BigInt::from_i64(456);
        assert_eq!(a.add(&b).to_string_decimal(), "579");
        assert_eq!(a.sub(&b).to_string_decimal(), "-333");
        assert_eq!(b.sub(&a).to_string_decimal(), "333");
        // 负数
        let neg = BigInt::from_i64(-100);
        assert_eq!(neg.add(&BigInt::from_i64(50)).to_string_decimal(), "-50");
        assert_eq!(neg.add(&BigInt::from_i64(100)).to_string_decimal(), "0");
    }

    #[test]
    fn test_mul() {
        assert_eq!(BigInt::from_i64(12).mul(&BigInt::from_i64(13)).to_string_decimal(), "156");
        assert_eq!(BigInt::from_i64(-7).mul(&BigInt::from_i64(8)).to_string_decimal(), "-56");
        assert_eq!(BigInt::from_i64(-7).mul(&BigInt::from_i64(-8)).to_string_decimal(), "56");
        // 大数乘法（验证进位）
        let big = BigInt::from_str_decimal("999999999999").unwrap();
        assert_eq!(big.mul(&big).to_string_decimal(), "999999999998000000000001");
    }

    #[test]
    fn test_divmod() {
        let a = BigInt::from_i64(100);
        let b = BigInt::from_i64(7);
        let (q, r) = a.divmod(&b).unwrap();
        assert_eq!(q.to_string_decimal(), "14");
        assert_eq!(r.to_string_decimal(), "2");
        // 负数（truncated：商向零，余数同被除数符号）
        let neg = BigInt::from_i64(-100);
        let (q, r) = neg.divmod(&b).unwrap();
        assert_eq!(q.to_string_decimal(), "-14");
        assert_eq!(r.to_string_decimal(), "-2");
        // 除零
        assert!(a.divmod(&BigInt::zero()).is_err());
        // 大数除法
        let big = BigInt::from_str_decimal("100000000000000000000").unwrap();
        let (q, r) = big.divmod(&BigInt::from_i64(3)).unwrap();
        assert_eq!(q.to_string_decimal(), "33333333333333333333");
        assert_eq!(r.to_string_decimal(), "1");
    }

    #[test]
    fn test_cmp() {
        assert_eq!(BigInt::from_i64(5).cmp(&BigInt::from_i64(5)), Ordering::Equal);
        assert_eq!(BigInt::from_i64(5).cmp(&BigInt::from_i64(3)), Ordering::Greater);
        assert_eq!(BigInt::from_i64(-5).cmp(&BigInt::from_i64(3)), Ordering::Less);
        assert_eq!(BigInt::from_i64(-5).cmp(&BigInt::from_i64(-3)), Ordering::Less);
    }

    #[test]
    fn test_large_arithmetic() {
        // 阶乘 20! = 2432902008176640000（超过 i64? 不，刚好在 i64 内）
        // 阶乘 25! = 15511210043330985984000000（超过 i64）
        let mut fact = BigInt::one();
        for i in 1..=25 {
            fact = fact.mul(&BigInt::from_i64(i));
        }
        assert_eq!(fact.to_string_decimal(), "15511210043330985984000000");
    }

    #[test]
    fn test_string_roundtrip() {
        let cases = ["0", "1", "-1", "42", "-42", "999999999999999999999", "-999999999999999999999"];
        for c in cases {
            let bi = BigInt::from_str_decimal(c).unwrap();
            assert_eq!(bi.to_string_decimal(), c);
        }
    }

    #[test]
    fn test_to_i64() {
        assert_eq!(BigInt::from_i64(42).to_i64(), Some(42));
        assert_eq!(BigInt::from_i64(i64::MAX).to_i64(), Some(i64::MAX));
        assert_eq!(BigInt::from_i64(i64::MIN).to_i64(), Some(i64::MIN));
        // 超出范围
        let big = BigInt::from_str_decimal("99999999999999999999").unwrap();
        assert_eq!(big.to_i64(), None);
    }

    #[test]
    fn test_divmod_multilimb() {
        // 多 limb 除数（≥ 2^32）—— 之前的死循环 bug 场景
        // a == b：商 1 余 0
        let a = BigInt::from_str_decimal("100000000000000000000").unwrap();
        let (q, r) = a.divmod(&a).unwrap();
        assert_eq!(q.to_string_decimal(), "1");
        assert_eq!(r.to_string_decimal(), "0");
        // 2^32 / 2^32（恰好触发归一化）
        let p = BigInt::from_i64(1i64 << 32);
        let (q, r) = p.divmod(&p).unwrap();
        assert_eq!(q.to_string_decimal(), "1");
        assert_eq!(r.to_string_decimal(), "0");
        // 大数除以多 limb 除数
        let big = BigInt::from_str_decimal("99999999999999999999999999").unwrap();
        let d = BigInt::from_str_decimal("999999999999").unwrap(); // 多 limb
        let (q, r) = big.divmod(&d).unwrap();
        // 不变式：q*d + r == big（必须成立）
        let check = q.mul(&d).add(&r);
        assert_eq!(check.cmp(&big), Ordering::Equal,
            "不变式失败: {} * {} + {} = {} != {}",
            q.to_string_decimal(), d.to_string_decimal(), r.to_string_decimal(),
            check.to_string_decimal(), big.to_string_decimal());
        assert!(r.cmp(&d) == Ordering::Less, "余数应 < 除数");
        // (b*2^32+6) / (b*2^32+5) = 商 1 余 1
        let base = BigInt::from_i64(1i64 << 32);
        let m1 = base.mul(&BigInt::from_i64(1)).add(&BigInt::from_i64(6));
        let m2 = base.mul(&BigInt::from_i64(1)).add(&BigInt::from_i64(5));
        let (q, r) = m1.divmod(&m2).unwrap();
        assert_eq!(q.to_string_decimal(), "1");
        assert_eq!(r.to_string_decimal(), "1");
    }

    #[test]
    fn test_divmod_randomized() {
        // 随机化测试：验证 a == q*b + r 且 |r| < |b|
        let mut seed = 12345u64;
        let mut rng = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            seed
        };
        for _ in 0..2000 {
            // 生成 a（1-4 limb）、b（1-3 limb）
            let an = (rng() % 4 + 1) as usize;
            let bn = (rng() % 3 + 1) as usize;
            let mut a_limbs = Vec::new();
            for _ in 0..an {
                a_limbs.push((rng() & 0xFFFF_FFFF) as u32);
            }
            let mut b_limbs = Vec::new();
            for _ in 0..bn {
                b_limbs.push((rng() & 0xFFFF_FFFF) as u32);
            }
            // 确保 b 非零
            if b_limbs.iter().all(|&x| x == 0) {
                b_limbs[0] = 1;
            }
            let a = BigInt { limbs: a_limbs.clone(), negative: false }.normalize().abs();
            let b = BigInt { limbs: b_limbs.clone(), negative: false }.normalize().abs();
            if b.is_zero() {
                continue;
            }
            let (q, r) = a.divmod(&b).unwrap();
            // 不变式：q*b + r == a，且 0 <= r < b
            let reconstructed = q.mul(&b).add(&r);
            assert_eq!(reconstructed.cmp(&a), Ordering::Equal,
                "a={} b={} q={} r={} recon={}",
                a.to_string_decimal(), b.to_string_decimal(),
                q.to_string_decimal(), r.to_string_decimal(),
                reconstructed.to_string_decimal());
            assert!(r.cmp(&b) == Ordering::Less,
                "余数 >= 除数: a={} b={} r={}",
                a.to_string_decimal(), b.to_string_decimal(), r.to_string_decimal());
        }
    }
}
