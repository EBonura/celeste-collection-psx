//! PICO-8 16.16 fixed-point, ported from ccleste's `_fix32`
//! (celeste.cpp:32-119). Bit-exact arithmetic is required or the
//! physics diverge from the original.

use crate::sin_table::SIN_TBL;
use core::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};
use psx_math::int32::{isqrt_u32, mul_div_i32, mul_shr_i32, mul_shr_trunc_i32};

pub const FACTOR: i32 = 1 << 16;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fix32(pub i32);

/// Fix32 from a (compile-time) number literal — matches C's
/// `_fix32(double)` which truncates `x * 65536` toward zero.
// psx-numeric-allow-next-line: evaluated by the compiler for literals; no float reaches the guest
pub const fn fx(x: f64) -> Fix32 {
    Fix32((x * 65536.0) as i32)
}

impl Fix32 {
    pub const ZERO: Fix32 = Fix32(0);

    #[inline]
    pub const fn from_bits(n: i32) -> Self {
        Fix32(n)
    }
    #[inline]
    pub const fn from_int(i: i32) -> Self {
        Fix32(i.wrapping_mul(FACTOR))
    }
    /// C `(int)` cast: truncate toward zero.
    #[inline]
    pub fn to_int(self) -> i32 {
        self.0 / FACTOR
    }
    /// Low 16 bits (fractional part), as C `frac()`.
    #[inline]
    pub fn frac(self) -> u16 {
        self.0 as u16
    }
    /// `_fix32_floor`: arithmetic floor, as Fix32.
    #[inline]
    pub fn floor(self) -> Fix32 {
        Fix32(self.0 & !0xFFFF)
    }
    /// floor as an integer.
    #[inline]
    pub fn floor_int(self) -> i32 {
        self.0 >> 16
    }
    #[inline]
    pub fn abs(self) -> Fix32 {
        if self.0 >= 0 {
            self
        } else {
            Fix32(self.0.wrapping_neg())
        }
    }
    #[inline]
    pub fn min(self, b: Fix32) -> Fix32 {
        if self.0 < b.0 {
            self
        } else {
            b
        }
    }
    #[inline]
    pub fn max(self, b: Fix32) -> Fix32 {
        if self.0 > b.0 {
            self
        } else {
            b
        }
    }
    /// `_fix32_mod`: `((a % b) + b) % b`.
    #[inline]
    pub fn rem_floor(self, b: Fix32) -> Fix32 {
        Fix32(((self.0 % b.0) + b.0) % b.0)
    }
    /// PICO-8 `sin` (note: turns, and negated — matches `_fix32_sin`).
    pub fn sin(self) -> Fix32 {
        let mut index = ((self.0.wrapping_add(0x4002) >> 2) & 0x3FFF) as i32;
        if index > 0x1FFF {
            index = 0x4000 - index;
        }
        if index < 0x1000 {
            Fix32::from_bits(SIN_TBL[index as usize])
        } else {
            Fix32::from_bits(-SIN_TBL[(0x2000 - index) as usize])
        }
    }
    /// PICO-8 `cos(x) = -sin(x + 0.25)`.
    #[inline]
    pub fn cos(self) -> Fix32 {
        -((self + fx(0.25)).sin())
    }
    /// PICO-8 `sqrt`. Negative input returns 0 (PICO-8 yields 0 for x < 0).
    /// For a 16.16 value v = raw/2^16, sqrt(v).raw == isqrt(raw << 16), which
    /// is up to 47 bits; here it is the largest `y` with `y * y <= raw << 16`,
    /// found by binary search above the integer root: `256 * isqrt(raw)` is a
    /// lower bound and `256 * (isqrt(raw) + 1)` an exclusive upper one, so
    /// eight halving steps settle it. `y * y` is compared as its high 32 bits
    /// (`>> 16`) and low 16 bits, so nothing wider than 32 bits is formed.
    pub fn sqrt(self) -> Fix32 {
        if self.0 <= 0 {
            return Fix32::ZERO;
        }
        let raw = self.0 as u32;
        let mut y = isqrt_u32(raw) << 8;
        let mut step = 128u32;
        while step > 0 {
            let c = y + step;
            let high = mul_shr_i32(c as i32, c as i32, 16) as u32;
            let low = c.wrapping_mul(c) & 0xFFFF;
            if high < raw || (high == raw && low == 0) {
                y = c;
            }
            step >>= 1;
        }
        Fix32(y as i32)
    }
}

impl Add for Fix32 {
    type Output = Fix32;
    #[inline]
    fn add(self, b: Fix32) -> Fix32 {
        Fix32(self.0.wrapping_add(b.0))
    }
}
impl Sub for Fix32 {
    type Output = Fix32;
    #[inline]
    fn sub(self, b: Fix32) -> Fix32 {
        Fix32(self.0.wrapping_sub(b.0))
    }
}
impl Neg for Fix32 {
    type Output = Fix32;
    #[inline]
    fn neg(self) -> Fix32 {
        Fix32(self.0.wrapping_neg())
    }
}
impl Mul for Fix32 {
    type Output = Fix32;
    #[inline]
    fn mul(self, b: Fix32) -> Fix32 {
        Fix32(mul_shr_trunc_i32(self.0, b.0, 16))
    }
}
impl Div for Fix32 {
    type Output = Fix32;
    #[inline]
    fn div(self, b: Fix32) -> Fix32 {
        // PICO-8 decomp'd division (celeste.cpp:70-76).
        if b.0 == 0 {
            return Fix32::from_int(if self.0 > 0 { FACTOR } else { -FACTOR });
        }
        if b.frac() == 0 && b.to_int() > 0 {
            return Fix32(self.0 / (b.0 / FACTOR));
        }
        Fix32(mul_div_i32(self.0, FACTOR, b.0))
    }
}
impl AddAssign for Fix32 {
    #[inline]
    fn add_assign(&mut self, b: Fix32) {
        *self = *self + b;
    }
}
impl SubAssign for Fix32 {
    #[inline]
    fn sub_assign(&mut self, b: Fix32) {
        *self = *self - b;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rng(seed: u32) -> impl FnMut() -> u32 {
        let mut state = seed;
        move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        }
    }

    fn edges() -> Vec<i32> {
        let mut v = vec![i32::MIN, i32::MIN + 1, -65_537, -65_536, -65_535, -1, 0, 1, 2, 3];
        v.extend([65_535, 65_536, 65_537, 0x7FFF_0000, i32::MAX - 1, i32::MAX]);
        v.extend((-40..40).map(|n| n * 3277));
        v
    }

    #[test]
    fn mul_matches_the_wide_form() {
        let wide = |a: i32, b: i32| ((a as i64 * b as i64) / FACTOR as i64) as i32;
        for &a in &edges() {
            for &b in &edges() {
                assert_eq!((Fix32(a) * Fix32(b)).0, wide(a, b), "{a} {b}");
            }
        }
        let mut next = rng(0x1357_9bdf);
        for _ in 0..3_000_000 {
            let (a, b) = (next() as i32 >> (next() % 32), next() as i32 >> (next() % 32));
            assert_eq!((Fix32(a) * Fix32(b)).0, wide(a, b), "{a} {b}");
        }
    }

    #[test]
    fn div_matches_the_wide_form() {
        let wide = |a: i32, b: i32| -> i32 {
            if b == 0 {
                return Fix32::from_int(if a > 0 { FACTOR } else { -FACTOR }).0;
            }
            if (b as u16) == 0 && b / FACTOR > 0 {
                let bi = b as i64 / FACTOR as i64;
                return (a as i64 / bi) as i32;
            }
            ((a as i64 * FACTOR as i64) / b as i64) as i32
        };
        for &a in &edges() {
            for &b in &edges() {
                assert_eq!((Fix32(a) / Fix32(b)).0, wide(a, b), "{a} {b}");
            }
        }
        let mut next = rng(0x2468_ace1);
        for _ in 0..3_000_000 {
            let (a, mut b) = (next() as i32 >> (next() % 32), next() as i32 >> (next() % 32));
            if next() % 4 == 0 {
                b &= !0xFFFF; // whole-number divisors take the other path
            }
            assert_eq!((Fix32(a) / Fix32(b)).0, wide(a, b), "{a} {b}");
        }
    }

    #[test]
    fn sqrt_matches_the_wide_form() {
        let wide = |raw: i32| -> i32 {
            if raw <= 0 {
                return 0;
            }
            let mut x = (raw as u64) << 16;
            let mut res: u64 = 0;
            let mut bit: u64 = 1 << 46;
            while bit > x {
                bit >>= 2;
            }
            while bit != 0 {
                if x >= res + bit {
                    x -= res + bit;
                    res = (res >> 1) + bit;
                } else {
                    res >>= 1;
                }
                bit >>= 2;
            }
            res as i32
        };
        for raw in -5..(1 << 22) {
            assert_eq!(Fix32(raw).sqrt().0, wide(raw), "{raw}");
        }
        for k in 0..31 {
            for d in -3..=3i64 {
                let raw = ((1i64 << k) + d).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                assert_eq!(Fix32(raw).sqrt().0, wide(raw), "{raw}");
            }
        }
        // Around every perfect square of 16.16 root, where the comparison is exact.
        for r in (1u64..=11_863).step_by(1) {
            let sq = ((r * 256) * (r * 256)) >> 16;
            for d in -2i64..=2 {
                let raw = (sq as i64 + d).clamp(0, i32::MAX as i64) as i32;
                assert_eq!(Fix32(raw).sqrt().0, wide(raw), "{raw}");
            }
        }
        let mut next = rng(0x0badf00d);
        for _ in 0..5_000_000 {
            let raw = (next() >> 1) as i32 >> (next() % 31);
            assert_eq!(Fix32(raw).sqrt().0, wide(raw), "{raw}");
        }
    }
}
