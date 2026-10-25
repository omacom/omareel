//! Easing curves (`Easing.h`).

use crate::compat::cpp;

/// `easeInOutCubic`: accelerate through the first half, decelerate through the second.
pub fn ease_in_out_cubic(t: f64) -> f64 {
    let t = cpp::clamp(t, 0.0, 1.0);
    if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powf(3.0) / 2.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eases_from_zero_to_one() {
        assert_eq!(ease_in_out_cubic(-1.0), 0.0);
        assert_eq!(ease_in_out_cubic(0.5), 0.5);
        assert_eq!(ease_in_out_cubic(2.0), 1.0);
        assert_eq!(ease_in_out_cubic(0.25), 0.0625);
    }
}
