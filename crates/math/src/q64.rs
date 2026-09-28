//! Q64.64 signed fixed point (`i128`, 64 fractional bits).

use core::fmt;

const FRAC_BITS: u32 = 64;
const ONE: i128 = 1 << FRAC_BITS;
const FRAC_MASK: i128 = ONE - 1;

/// Signed Q64.64.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Q64(pub i128);

impl Q64 {
    pub const ZERO: Self = Self(0);
    pub const ONE: Self = Self(ONE);
    pub const FRAC_BITS: u32 = FRAC_BITS;

    pub const fn from_raw(raw: i128) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> i128 {
        self.0
    }

    pub const fn from_int(x: i64) -> Self {
        Self((x as i128) << FRAC_BITS)
    }

    /// `num / den` with `den != 0`.
    pub fn from_ratio(num: i64, den: i64) -> Self {
        assert!(den != 0, "division by zero");
        let n = (num as i128) << FRAC_BITS;
        Self(n / den as i128)
    }

    pub fn saturating_add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }

    pub fn saturating_sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }

    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        self.0.checked_add(rhs.0).map(Self)
    }

    /// `(a * b) >> 64` using 64-bit limbs so the intermediate fits in i128 pieces.
    pub fn checked_mul(self, rhs: Self) -> Option<Self> {
        mul_q64(self.0, rhs.0).map(Self)
    }

    pub fn saturating_mul(self, rhs: Self) -> Self {
        self.checked_mul(rhs).unwrap_or(if (self.0 ^ rhs.0) < 0 {
            Self(i128::MIN)
        } else {
            Self(i128::MAX)
        })
    }

    pub fn checked_div(self, rhs: Self) -> Option<Self> {
        if rhs.0 == 0 {
            return None;
        }
        div_q64(self.0, rhs.0).map(Self)
    }

    /// Natural exp on `|x| <= 16`. Range-reduce \(x=k\ln 2+r\) so Taylor runs on `|r|<1`, not `|x|`.
    pub fn exp(self) -> Self {
        let x = self.clamp(Self::from_int(-16), Self::from_int(16));
        if x.raw() == 0 {
            return Self::ONE;
        }
        let k = x.saturating_mul(INV_LN2).round_i32();
        let r = x.saturating_sub(LN2.saturating_mul(Self::from_int(k as i64)));
        let mut term = Self::ONE;
        let mut sum = Self::ONE;
        for i in 1..=12 {
            term = term
                .saturating_mul(r)
                .checked_div(Self::from_int(i))
                .unwrap_or(Self::ZERO);
            sum = sum.saturating_add(term);
            if term.0.abs() < 8 {
                break;
            }
        }
        sum.mul_pow2(k)
    }

    fn round_i32(self) -> i32 {
        let half = 1i128 << (FRAC_BITS - 1);
        let adj = if self.0 >= 0 { self.0 + half } else { self.0 - half };
        (adj >> FRAC_BITS).clamp(i32::MIN as i128, i32::MAX as i128) as i32
    }

    fn mul_pow2(self, k: i32) -> Self {
        if k == 0 {
            return self;
        }
        if k > 0 {
            match self.0.checked_shl(k as u32) {
                Some(v) => Self(v),
                None => {
                    if self.0 >= 0 {
                        Self(i128::MAX)
                    } else {
                        Self(i128::MIN)
                    }
                }
            }
        } else {
            let sh = (-k) as u32;
            if sh >= 127 {
                Self::ZERO
            } else {
                Self(self.0 >> sh)
            }
        }
    }

    /// Natural log for `x > 0`.
    pub fn ln(self) -> Self {
        assert!(self.0 > 0, "ln of non-positive");
        let mut raw = self.0;
        let mut shift: i64 = 0;
        while raw >= (2 << FRAC_BITS) {
            raw >>= 1;
            shift += 1;
        }
        while raw < ONE {
            raw <<= 1;
            shift -= 1;
        }
        let y = Self(raw);
        let z = y
            .saturating_sub(Self::ONE)
            .checked_div(y.saturating_add(Self::ONE))
            .unwrap_or(Self::ZERO);
        let z2 = z.saturating_mul(z);
        let mut acc = z;
        let mut p = z;
        for k in (3..=21).step_by(2) {
            p = p.saturating_mul(z2);
            let term = p.checked_div(Self::from_int(k)).unwrap_or(Self::ZERO);
            acc = acc.saturating_add(term);
        }
        let ln_y = acc.saturating_add(acc);
        ln_y.saturating_add(Self::from_int(shift).saturating_mul(LN2))
    }

    pub fn clamp(self, lo: Self, hi: Self) -> Self {
        if self.0 < lo.0 {
            lo
        } else if self.0 > hi.0 {
            hi
        } else {
            self
        }
    }

    pub fn approx_eq(self, other: Self, abs_tol: i128) -> bool {
        (self.0 - other.0).abs() <= abs_tol
    }
}

const LN2: Q64 = Q64(0xB17217F7D1CF79AB);
/// \(1/\ln 2\) so `exp` range-reduce is a multiply, not a 64-step Q64 divide.
const INV_LN2: Q64 = Q64(0x171547652B82FE179);

fn mul_q64(a: i128, b: i128) -> Option<i128> {
    let sign = if (a ^ b) < 0 { -1i128 } else { 1 };
    let a = a.unsigned_abs();
    let b = b.unsigned_abs();
    let a_hi = a >> 64;
    let a_lo = a & 0xffff_ffff_ffff_ffff;
    let b_hi = b >> 64;
    let b_lo = b & 0xffff_ffff_ffff_ffff;
    let lo_lo = a_lo.checked_mul(b_lo)?;
    let cross = a_lo
        .checked_mul(b_hi)?
        .checked_add(a_hi.checked_mul(b_lo)?)?;
    let hi_hi = a_hi.checked_mul(b_hi)?;
    // (hi_hi << 128 + cross << 64 + lo_lo) >> 64 = hi_hi << 64 + cross + lo_lo>>64
    let tmp = (lo_lo >> 64).checked_add(cross)?;
    let (tmp, carry) = tmp.overflowing_add(hi_hi.checked_shl(64)?);
    if carry {
        return None;
    }
    if tmp > i128::MAX as u128 {
        return None;
    }
    (tmp as i128).checked_mul(sign)
}

/// `(rem * 2^64) / den` with `rem < den`.
fn shl64_div(rem: u128, den: u128) -> u128 {
    debug_assert!(den > rem);
    let mut r = rem;
    let mut q = 0u128;
    for _ in 0..64 {
        q <<= 1;
        if r >= 1u128 << 127 {
            r = r.wrapping_shl(1).wrapping_sub(den);
            q |= 1;
        } else {
            r <<= 1;
            if r >= den {
                r -= den;
                q |= 1;
            }
        }
    }
    q
}

fn div_q64(a: i128, b: i128) -> Option<i128> {
    let sign = if (a ^ b) < 0 { -1i128 } else { 1 };
    let au = a.unsigned_abs();
    let bu = b.unsigned_abs();
    if bu == 0 {
        return None;
    }
    // Integer divisor: `(a / n)` in Q64 is `a.raw / n`. Avoids the 64-step frac loop.
    if bu & (((1u128) << 64) - 1) == 0 {
        let n = bu >> 64;
        if n == 0 {
            return None;
        }
        let mag = au / n;
        if mag > i128::MAX as u128 {
            return None;
        }
        return (mag as i128).checked_mul(sign);
    }
    let int_q = au / bu;
    let rem = au % bu;
    let frac = shl64_div(rem, bu);
    if int_q > i64::MAX as u128 {
        return None;
    }
    let mag = int_q
        .checked_shl(64)?
        .checked_add(frac)?;
    if mag > i128::MAX as u128 {
        return None;
    }
    (mag as i128).checked_mul(sign)
}

impl fmt::Display for Q64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let abs = self.0.unsigned_abs();
        let int = abs >> FRAC_BITS;
        let frac = abs & (FRAC_MASK as u128);
        write!(f, "{sign}{int}.{:016x}", frac)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_plus_one() {
        let two = Q64::ONE.saturating_add(Q64::ONE);
        assert_eq!(two, Q64::from_int(2));
    }

    #[test]
    fn ratio_half() {
        let h = Q64::from_ratio(1, 2);
        assert_eq!(h.saturating_add(h), Q64::ONE);
    }

    #[test]
    fn mul_div_roundtrip() {
        let a = Q64::from_ratio(3, 2);
        let b = Q64::from_int(4);
        let p = a.checked_mul(b).unwrap();
        assert_eq!(p, Q64::from_int(6));
        let q = p.checked_div(b).unwrap();
        assert!(q.approx_eq(a, 2));
    }

    #[test]
    fn exp_zero_is_one() {
        assert!(Q64::ZERO.exp().approx_eq(Q64::ONE, 1 << 40));
    }

    #[test]
    fn exp_ln_one() {
        let e = Q64::ONE.exp();
        let back = e.ln();
        assert!(back.approx_eq(Q64::ONE, 1 << 50), "ln(e) got {}", back.raw());
    }

    #[test]
    fn exp_range_reduce_two() {
        let two = LN2.exp();
        assert!(two.approx_eq(Q64::from_int(2), 1 << 40), "exp(ln2)={}", two.raw());
        let half = Q64::from_raw(-LN2.raw()).exp();
        assert!(half.approx_eq(Q64::from_ratio(1, 2), 1 << 40), "exp(-ln2)={}", half.raw());
        let prod = Q64::ONE.exp().saturating_mul(Q64::from_raw(-Q64::ONE.raw()).exp());
        assert!(prod.approx_eq(Q64::ONE, 1 << 40), "e*e^-1={}", prod.raw());
    }

    #[test]
    fn inv_ln2_times_ln2_is_one() {
        let p = INV_LN2.saturating_mul(LN2);
        assert!(p.approx_eq(Q64::ONE, 1 << 32), "got {}", p.raw());
    }

    #[test]
    fn integer_div_is_q64_div() {
        let a = Q64::from_ratio(5, 1);
        let q = a.checked_div(Q64::from_int(3)).unwrap();
        assert!(q.approx_eq(Q64::from_ratio(5, 3), 2));
        let neg = Q64::from_int(-5).checked_div(Q64::from_int(2)).unwrap();
        assert!(neg.approx_eq(Q64::from_ratio(-5, 2), 2));
    }
}
