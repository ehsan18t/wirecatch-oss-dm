//! Checked numeric conversions: between whole numbers and floating point, and
//! between whole-number types of different ranges.
//!
//! Byte counts and rates are whole numbers, but speeds, ratios and display
//! need floating point, and numbers read from other programs (JSON, manifests)
//! often arrive as floating point. Rust's `as` converts both ways without a
//! word: it rounds large integers, and it turns NaN into 0 and negative or huge
//! floats into the nearest bound. Rust has no `TryFrom<f64>` for integers
//! either. Between whole-number types, `as` wraps or cuts silently, and
//! `try_from` leaves every caller to choose a fallback. These functions are the
//! one place those conversions happen, so each rule is written, checked and
//! tested once.
//!
//! ```
//! use wirecatch_engine::numeric::{approx_f64, f64_to_u64};
//!
//! let seconds = 2.5;
//! let bytes_per_second = approx_f64(10 * 1024 * 1024) / seconds;
//! assert_eq!(f64_to_u64(bytes_per_second), Some(4 * 1024 * 1024));
//!
//! // A duration from a JSON document: anything that is not a usable count is refused.
//! assert_eq!(f64_to_u64(f64::NAN), None);
//! assert_eq!(f64_to_u64(-1.0), None);
//! assert_eq!(f64_to_u64(1e30), None);
//! ```

/// `2^64`, the first value above every `u64`. A literal, since computing it
/// with `as` is the conversion this module exists to avoid.
const TWO_POW_64: f64 = 18_446_744_073_709_551_616.0;

/// `value` as the nearest `f64`.
///
/// Exact up to `2^53` (8 PiB), far beyond any byte count, size or rate a
/// download reaches; larger values are rounded to the nearest representable
/// `f64`. Use it for speeds, ratios and display, never to carry a count that
/// is converted back.
#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "exact below 2^53, which byte counts and rates stay far below; above it the documented rounding is the intended result"
)]
pub fn approx_f64(value: u64) -> f64 {
    value as f64
}

/// `value` as a `u64`, rounded toward zero; `None` when it is NaN, infinite,
/// negative or `2^64` and above.
///
/// Round first (`value.round()`) when the nearest whole number is wanted.
/// Every negative value is refused, even one above `-1` that would truncate to
/// zero; `-0.0` is zero.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is checked to be finite, not negative and below 2^64 on the line before, so `as` only drops the fraction, which is the documented rounding"
)]
pub fn f64_to_u64(value: f64) -> Option<u64> {
    // Also false for NaN, which compares false with everything.
    let in_range = (0.0..TWO_POW_64).contains(&value);
    in_range.then_some(value as u64)
}

/// Keeps [`saturating_cast`] to the whole-number types: the trait cannot be
/// named outside this crate, so no other type can implement it.
mod sealed {
    /// A whole-number type, with its smallest value, largest value and zero.
    pub trait Whole: Copy + PartialOrd {
        /// The smallest value.
        const MIN: Self;
        /// The largest value.
        const MAX: Self;
        /// Zero.
        const ZERO: Self;
    }

    macro_rules! whole {
        ($($t:ty),*) => {
            $(impl Whole for $t {
                const MIN: Self = <$t>::MIN;
                const MAX: Self = <$t>::MAX;
                const ZERO: Self = 0;
            })*
        };
    }

    whole!(
        u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize
    );
}

/// `value` as a `U`, or the nearest of `U`'s bounds when it does not fit: a
/// value too large becomes `U::MAX`, a negative one too small becomes `U::MIN`.
/// Both types are whole-number types (`u8` to `u128`, `i8` to `i128`, `usize`,
/// `isize`).
///
/// For counts, versions and limits, where "more than can be said" is the
/// honest answer: a list's length in a protocol's `u32`, a byte count in
/// SQLite's signed integers. Never for a buffer size handed to the operating
/// system, where a size larger than the buffer would be a lie; derive the
/// buffer from the size instead. Nor where a value out of range means damage
/// that should be reported rather than clamped.
///
/// ```
/// use wirecatch_engine::numeric::saturating_cast;
///
/// assert_eq!(saturating_cast::<u32, _>(7_usize), 7);
/// assert_eq!(saturating_cast::<u32, _>(u64::MAX), u32::MAX);
/// assert_eq!(saturating_cast::<i64, _>(u64::MAX), i64::MAX);
/// assert_eq!(saturating_cast::<u64, _>(-5_i64), 0);
/// ```
///
/// A floating-point value is refused at compile time; [`f64_to_u64`] converts
/// those.
///
/// ```compile_fail
/// use wirecatch_engine::numeric::saturating_cast;
///
/// let _: u32 = saturating_cast(1.5_f64);
/// ```
#[must_use]
pub fn saturating_cast<U, T>(value: T) -> U
where
    T: sealed::Whole,
    U: sealed::Whole + TryFrom<T>,
{
    let bound = if value < T::ZERO { U::MIN } else { U::MAX };
    U::try_from(value).unwrap_or(bound)
}

/// `n` as an index into a slice of length `len`: `n` itself when it fits,
/// otherwise `len`.
pub(crate) fn clamp_to_len(n: u64, len: usize) -> usize {
    usize::try_from(n).map_or(len, |n| n.min(len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saturating_cast_keeps_what_fits_and_clamps_the_rest_to_the_nearest_bound() {
        assert_eq!(saturating_cast::<u32, _>(u64::from(u32::MAX)), u32::MAX);
        assert_eq!(saturating_cast::<u32, _>(u64::from(u32::MAX) + 1), u32::MAX);
        assert_eq!(saturating_cast::<u32, _>(0_usize), 0);
        assert_eq!(saturating_cast::<i64, _>(u128::MAX), i64::MAX);
        assert_eq!(saturating_cast::<u64, _>(i64::MIN), 0);
        assert_eq!(saturating_cast::<i8, _>(-1000_i32), i8::MIN);
        assert_eq!(saturating_cast::<i8, _>(-5_i32), -5);
    }

    #[test]
    fn approx_f64_is_exact_up_to_two_pow_53() {
        let exact = 1u64 << 53;
        assert_eq!(approx_f64(exact), 9_007_199_254_740_992.0);
        assert_eq!(approx_f64(exact - 1), 9_007_199_254_740_991.0);
        assert_eq!(approx_f64(0), 0.0);
    }

    #[test]
    fn f64_to_u64_truncates_values_in_range() {
        assert_eq!(f64_to_u64(0.0), Some(0));
        assert_eq!(f64_to_u64(-0.0), Some(0));
        assert_eq!(f64_to_u64(1.9), Some(1));
        assert_eq!(f64_to_u64(1.5_f64.round()), Some(2));
        // The largest f64 below 2^64.
        let largest = 18_446_744_073_709_549_568.0;
        assert_eq!(f64_to_u64(largest), Some(18_446_744_073_709_549_568));
    }

    #[test]
    fn f64_to_u64_refuses_what_is_not_a_count() {
        for value in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            -0.5,
            -1.0,
            TWO_POW_64,
            1e30,
        ] {
            assert_eq!(f64_to_u64(value), None, "{value}");
        }
    }

    #[test]
    fn clamp_to_len_never_exceeds_the_length() {
        assert_eq!(clamp_to_len(3, 10), 3);
        assert_eq!(clamp_to_len(10, 10), 10);
        assert_eq!(clamp_to_len(11, 10), 10);
        assert_eq!(clamp_to_len(u64::MAX, 10), 10);
    }
}
