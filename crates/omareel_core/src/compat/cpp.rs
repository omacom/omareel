//! C++ standard library semantics that differ from Rust's around NaN.
//!
//! `std::max(a, b)` is `a < b ? b : a`, so `std::max(NaN, 1.0)` is NaN, while Rust's
//! `f64::max` returns the non-NaN operand. Ported code uses these to keep C++'s results.

/// `std::max(a, b)`.
pub fn max<T: PartialOrd>(a: T, b: T) -> T {
    if a < b { b } else { a }
}

/// `std::min(a, b)`.
pub fn min<T: PartialOrd>(a: T, b: T) -> T {
    if b < a { b } else { a }
}

/// `std::clamp(v, lo, hi)` as libstdc++ (GCC 16, which builds omareel) implements it:
/// `std::min(std::max(v, lo), hi)`. When `hi < lo`, which the standard leaves undefined (and
/// `_GLIBCXX_ASSERTIONS` builds abort on), that gives `hi`, where the textbook
/// `v < lo ? lo : hi < v ? hi : v` would give `lo` for a `v` below `lo`. A clip whose out is
/// before its in reaches this in `ClipTimeline`. Otherwise the two agree, NaN included.
pub fn clamp<T: PartialOrd>(v: T, lo: T, hi: T) -> T {
    min(max(v, lo), hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nan_behaves_as_in_cpp() {
        assert!(max(f64::NAN, 1.0).is_nan());
        assert_eq!(max(1.0, f64::NAN), 1.0);
        assert!(clamp(f64::NAN, 0.0, 1.0).is_nan());
        assert!(clamp(f64::NAN, 1.0, 0.0).is_nan());
        assert_eq!(clamp(0.5, f64::NAN, 1.0), 0.5);
        assert_eq!(clamp(2.0, 0.0, f64::NAN), 2.0);
        assert!(clamp(-0.0_f64, 0.0, 1.0).is_sign_negative());
        // hi < lo: libstdc++ gives hi whatever v is.
        assert_eq!(clamp(0.0, 3.0, 2.0), 2.0);
        assert_eq!(clamp(2.5, 3.0, 2.0), 2.0);
        assert_eq!(clamp(4.0, 3.0, 2.0), 2.0);
        assert_eq!(min(2, 3), 2);
    }
}
