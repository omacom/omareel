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

/// `std::clamp(v, lo, hi)`.
pub fn clamp<T: PartialOrd>(v: T, lo: T, hi: T) -> T {
    if v < lo {
        lo
    } else if hi < v {
        hi
    } else {
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nan_behaves_as_in_cpp() {
        assert!(max(f64::NAN, 1.0).is_nan());
        assert_eq!(max(1.0, f64::NAN), 1.0);
        assert!(clamp(f64::NAN, 0.0, 1.0).is_nan());
        assert_eq!(min(2, 3), 2);
    }
}
