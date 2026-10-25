//! `qRound`, `qRound64`, `qBound`, `qFuzzyCompare`, `qFuzzyIsNull`.
//!
//! These follow Qt 6.11's `qnumeric.h` and `qminmax.h` to the bit, including the parts that
//! depend on the CPU: Qt picks its rounding instruction per architecture, and converting an
//! out-of-range value to an integer is undefined behaviour in C++, so what the compiled C++
//! does is what the hardware conversion does. Both are reproduced for the two architectures
//! omareel ships on (x86-64 and aarch64).

/// `qMin(a, b)`: `(a < b) ? a : b`, so it returns `b` when the two compare equal or either is
/// NaN.
pub fn q_min<T: PartialOrd>(a: T, b: T) -> T {
    if a < b { a } else { b }
}

/// `qMax(a, b)`: `(a < b) ? b : a`, so it returns `a` when the two compare equal or either is
/// NaN.
pub fn q_max<T: PartialOrd>(a: T, b: T) -> T {
    if a < b { b } else { a }
}

/// `qBound(min, val, max)`: `qMax(min, qMin(max, val))`, evaluated in that order. Qt asserts
/// `!(max < min)` in debug builds only; when `min > max` the result is `min`. A NaN `val`
/// gives `min` (every comparison with NaN is false), and `qBound(0.0, -0.0, 1.0)` is `+0.0`
/// (`qMax` keeps its first argument on a tie).
pub fn q_bound<T: PartialOrd>(min: T, val: T, max: T) -> T {
    q_max(min, q_min(max, val))
}

/// `qRound(double)`: rounds half away from zero.
///
/// On x86-64 Qt computes `d + copysign(0.5, d)` and truncates, so the largest double below
/// 0.5 rounds to 1 (the addition rounds up to 1.0). Built by GCC for aarch64 (as omareel and
/// Arch Linux ARM's Qt are) it uses `round()`, which gives 0 there. Out of range (and NaN) is undefined in C++; x86-64's conversion returns
/// `i32::MIN`, aarch64's saturates and turns NaN into 0, and both are reproduced.
pub fn q_round(d: f64) -> i32 {
    let rounded = round_half_away_f64(d);
    if cfg!(any(target_arch = "x86_64", target_arch = "x86")) {
        if rounded > -2_147_483_649.0 && rounded < 2_147_483_648.0 {
            rounded as i32
        } else {
            i32::MIN
        }
    } else {
        rounded as i32
    }
}

/// `qRound64(double)`: [`q_round`] into 64 bits. On x86-64 the half-adding rounding also
/// pushes odd integers between 2^52 and 2^53 up by one (`d + 0.5` ties to even), and out of
/// range gives `i64::MIN`; aarch64 rounds exactly and saturates.
pub fn q_round64(d: f64) -> i64 {
    let rounded = round_half_away_f64(d);
    if cfg!(any(target_arch = "x86_64", target_arch = "x86")) {
        if (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&rounded) {
            rounded as i64
        } else {
            i64::MIN
        }
    } else {
        rounded as i64
    }
}

/// `qRound(float)`: the float overload, which Qt uses for colour channels. It rounds in
/// single precision (`f + copysignf(0.5f, f)` on x86-64, `roundf` on aarch64), so it is not
/// the same as rounding the value widened to double: on x86-64 `0.49999997f` rounds to 1 and
/// odd integers between 2^23 and 2^24 round up by one.
pub fn q_round_f32(f: f32) -> i32 {
    let rounded = if cfg!(target_arch = "aarch64") { f.round() } else { f + 0.5f32.copysign(f) };
    if cfg!(any(target_arch = "x86_64", target_arch = "x86")) {
        if (-2_147_483_648.0..2_147_483_648.0).contains(&rounded) {
            rounded as i32
        } else {
            i32::MIN
        }
    } else {
        rounded as i32
    }
}

/// The value `qRound` truncates: Qt's per-architecture `QRoundImpl::qRound`.
fn round_half_away_f64(d: f64) -> f64 {
    if cfg!(target_arch = "aarch64") { d.round() } else { d + 0.5f64.copysign(d) }
}

/// `qFuzzyCompare(double, double)`: `qAbs(p1 - p2) * 1e12 <= qMin(qAbs(p1), qAbs(p2))`.
/// Like Qt's, it is never true when exactly one side is 0, and never true for infinities or
/// NaN.
pub fn q_fuzzy_compare(p1: f64, p2: f64) -> bool {
    (p1 - p2).abs() * 1_000_000_000_000.0 <= q_min(p1.abs(), p2.abs())
}

/// `qFuzzyCompare(float, float)`: `qAbs(p1 - p2) * 100000.f <= qMin(qAbs(p1), qAbs(p2))`, in
/// single precision.
pub fn q_fuzzy_compare_f32(p1: f32, p2: f32) -> bool {
    (p1 - p2).abs() * 100_000.0 <= q_min(p1.abs(), p2.abs())
}

/// `qFuzzyIsNull(double)`: `qAbs(d) <= 1e-12`.
pub fn q_fuzzy_is_null(d: f64) -> bool {
    d.abs() <= 0.000_000_000_001
}

/// `qFuzzyIsNull(float)`: `qAbs(f) <= 0.00001f`.
pub fn q_fuzzy_is_null_f32(f: f32) -> bool {
    f.abs() <= 0.000_01
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_half_away_from_zero() {
        assert_eq!(q_round(0.5), 1);
        assert_eq!(q_round(1.5), 2);
        assert_eq!(q_round(2.5), 3);
        assert_eq!(q_round(-0.5), -1);
        assert_eq!(q_round(-2.5), -3);
        assert_eq!(q_round(-0.0), 0);
        assert_eq!(q_round(2_147_483_647.4), 2_147_483_647);
        assert_eq!(q_round64(-9_223_372_036_854_775_808.0), i64::MIN);
        assert_eq!(q_round_f32(127.5), 128);
        assert_eq!(q_round_f32(-127.5), -128);
    }

    #[test]
    #[cfg(target_arch = "x86_64")]
    fn x86_64_rounding_quirks() {
        // Qt's `d + 0.5` rounds these up where `round()` would not.
        assert_eq!(q_round(0.499_999_999_999_999_94), 1);
        assert_eq!(q_round(-0.499_999_999_999_999_94), -1);
        assert_eq!(q_round64(4_503_599_627_370_497.0), 4_503_599_627_370_498);
        assert_eq!(q_round_f32(0.499_999_97), 1);
        assert_eq!(q_round_f32(8_388_609.0), 8_388_610);
        // Out of range and NaN: the conversion's "integer indefinite" value.
        assert_eq!(q_round(2_147_483_647.5), i32::MIN);
        assert_eq!(q_round(f64::NAN), i32::MIN);
        assert_eq!(q_round64(f64::INFINITY), i64::MIN);
        assert_eq!(q_round_f32(3e9), i32::MIN);
    }

    #[test]
    fn bound_keeps_qt_order_of_evaluation() {
        assert_eq!(q_bound(0.0, 0.5, 1.0), 0.5);
        assert_eq!(q_bound(0.0, 2.0, 1.0), 1.0);
        assert_eq!(q_bound(0.0, -2.0, 1.0), 0.0);
        // min > max: qMin(max, val) first, then qMax with min wins.
        assert_eq!(q_bound(5, 3, 1), 5);
        assert_eq!(q_bound(0.0, f64::NAN, 1.0), 0.0);
        assert!(q_bound(0.0_f64, -0.0, 1.0).is_sign_positive());
        assert_eq!(q_min(1, 1), 1);
        assert!(q_max(0.0_f64, -0.0).is_sign_positive());
    }

    #[test]
    fn fuzzy_comparisons() {
        assert!(q_fuzzy_compare(0.75, 0.75));
        assert!(q_fuzzy_compare(1.0, 1.0 + 1e-13));
        assert!(!q_fuzzy_compare(1.0, 1.0 + 1e-11));
        assert!(!q_fuzzy_compare(0.0, 1e-300));
        assert!(q_fuzzy_compare(0.0, 0.0));
        assert!(!q_fuzzy_compare(f64::INFINITY, f64::INFINITY));
        assert!(q_fuzzy_compare_f32(1.0, 1.000_001));
        assert!(!q_fuzzy_compare_f32(1.0, 1.000_1));
        assert!(q_fuzzy_is_null(1e-12));
        assert!(!q_fuzzy_is_null(1.1e-12));
        assert!(q_fuzzy_is_null_f32(0.000_01));
        assert!(!q_fuzzy_is_null_f32(0.000_011));
    }
}
