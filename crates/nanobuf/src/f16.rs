//! IEEE 754 binary16 wire scalar.
//!
//! Rust has no stable `f16` primitive, so the runtime carries the raw bits
//! and converts through `f32`. Storing bits keeps round trips exact; the
//! narrowing conversion rounds to nearest, ties to even, exactly like the
//! other language runtimes.

use crate::scalar::Scalar;

/// An IEEE 754 binary16 value, stored as its raw bit pattern.
///
/// Equality compares bits: `-0.0 != 0.0` and `NAN == NAN` when the
/// payloads match. Compare via [`F16::to_f32`] for numeric semantics.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct F16(pub u16);

impl F16 {
    pub const ZERO: F16 = F16(0);
    pub const INFINITY: F16 = F16(0x7c00);
    pub const NEG_INFINITY: F16 = F16(0xfc00);
    pub const NAN: F16 = F16(0x7e00);

    /// Rounds to the nearest representable value, ties to even. Values
    /// beyond the binary16 range become infinity.
    pub fn from_f32(value: f32) -> F16 {
        let bits = value.to_bits();
        let sign = ((bits >> 16) & 0x8000) as u16;
        let exp = ((bits >> 23) & 0xff) as i32;
        let frac = bits & 0x007f_ffff;

        if exp == 255 {
            if frac == 0 {
                return F16(sign | 0x7c00);
            }
            // NaN: keep the top payload bits, force the quiet bit so the
            // payload can never read back as infinity.
            return F16(sign | 0x7c00 | 0x0200 | (frac >> 13) as u16);
        }
        let unbiased = exp - 127;
        if unbiased >= 16 {
            return F16(sign | 0x7c00);
        }
        if unbiased >= -14 {
            // Normal result: drop 13 fraction bits with round-to-nearest-
            // even. A carry out of the fraction bumps the exponent, which
            // also turns 65520.0 and up into infinity.
            let mut half = (((unbiased + 15) as u32) << 10) | (frac >> 13);
            let dropped = frac & 0x1fff;
            if dropped > 0x1000 || (dropped == 0x1000 && half & 1 != 0) {
                half += 1;
            }
            return F16(sign | half as u16);
        }
        if unbiased >= -25 && exp != 0 {
            // Subnormal result; a round-up carry into bit 10 lands on the
            // smallest normal value, which is exactly right.
            let mantissa = frac | 0x0080_0000;
            let shift = (-1 - unbiased) as u32;
            let dropped = mantissa & ((1 << shift) - 1);
            let halfway = 1u32 << (shift - 1);
            let mut half = mantissa >> shift;
            if dropped > halfway || (dropped == halfway && half & 1 != 0) {
                half += 1;
            }
            return F16(sign | half as u16);
        }
        // Too small for binary16 (including all f32 subnormals).
        F16(sign)
    }

    /// Exact widening conversion.
    pub fn to_f32(self) -> f32 {
        let sign = u32::from(self.0 & 0x8000) << 16;
        let exp = u32::from(self.0 >> 10) & 0x1f;
        let frac = u32::from(self.0 & 0x03ff);
        let bits = match exp {
            0 if frac == 0 => sign,
            0 => {
                // Subnormal: renormalize under the wider exponent. The
                // fraction's top bit at position `p` becomes implicit.
                let p = 31 - frac.leading_zeros();
                let exp32 = 103 + p;
                let frac32 = (frac << (23 - p)) & 0x007f_ffff;
                sign | (exp32 << 23) | frac32
            }
            31 => sign | 0x7f80_0000 | (frac << 13),
            _ => sign | ((exp + 112) << 23) | (frac << 13),
        };
        f32::from_bits(bits)
    }
}

impl From<f32> for F16 {
    fn from(value: f32) -> F16 {
        F16::from_f32(value)
    }
}

impl From<F16> for f32 {
    fn from(value: F16) -> f32 {
        value.to_f32()
    }
}

impl std::fmt::Debug for F16 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.to_f32(), f)
    }
}

impl Scalar for F16 {
    const SIZE: usize = 2;

    #[inline]
    fn read(bytes: &[u8]) -> Self {
        F16(u16::read(bytes))
    }

    #[inline]
    fn store(self, bytes: &mut [u8]) {
        self.0.store(bytes);
    }

    #[inline]
    fn write(self, out: &mut Vec<u8>) {
        self.0.write(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widening_is_exact_for_every_pattern() {
        for bits in 0..=u16::MAX {
            let wide = F16(bits).to_f32();
            let back = F16::from_f32(wide);
            if wide.is_nan() {
                // The quiet bit is forced during narrowing; everything
                // else must survive.
                assert_eq!(back.0, bits | 0x0200, "bits {bits:#06x}");
            } else {
                assert_eq!(back.0, bits, "bits {bits:#06x}");
            }
        }
    }

    #[test]
    fn narrowing_rounds_to_nearest_even() {
        // (input, expected bits)
        let cases = [
            (0.0, 0x0000),
            (-0.0, 0x8000),
            (1.0, 0x3c00),
            (1.25, 0x3d00),
            (65504.0, 0x7bff), // largest normal
            (65519.9, 0x7bff), // rounds down to it
            (65520.0, 0x7c00), // halfway, ties away to inf
            (1e30, 0x7c00),    // overflow
            (-1e30, 0xfc00),
            (f32::INFINITY, 0x7c00),
            (f32::NEG_INFINITY, 0xfc00),
            (6.103_515_6e-5, 0x0400), // smallest normal, 2^-14
            (5.960_464_5e-8, 0x0001), // smallest subnormal, 2^-24
            (2.980_232_2e-8, 0x0000), // 2^-25: halfway, ties to even
            (2.980_233e-8, 0x0001),   // just above halfway
            (1e-10, 0x0000),          // underflow
            (-1e-10, 0x8000),
            (0.1, 0x2e66),                // inexact, rounds to nearest
            (1.0 + 1.0 / 1024.0, 0x3c01), // 1 + 2^-10, exact
            (1.0 + 1.0 / 2048.0, 0x3c00), // 1 + 2^-11: halfway, ties to even
            (1.0 + 3.0 / 2048.0, 0x3c02), // 1 + 3*2^-11 rounds up to even
        ];
        for (input, expected) in cases {
            assert_eq!(F16::from_f32(input).0, expected, "input {input}");
        }
        assert_eq!(F16::from_f32(f32::NAN).0 & 0x7c00, 0x7c00);
        assert_ne!(F16::from_f32(f32::NAN).0 & 0x03ff, 0);
    }

    #[test]
    fn wire_round_trip_is_little_endian() {
        let value = F16::from_f32(-1.5);
        assert_eq!(value.0, 0xbe00);
        let mut bytes = [0u8; 2];
        value.store(&mut bytes);
        assert_eq!(bytes, [0x00, 0xbe]);
        assert_eq!(F16::read(&bytes), value);
    }
}
