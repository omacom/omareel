//! easing: `easing::ease_in_out_cubic` against `Omareel::Easing::easeInOutCubic` (README
//! "easing"). x86_64 only: the results are glibc `pow` values, bit for bit.

#[cfg(target_arch = "x86_64")]
#[test]
fn ease_in_out_cubic_txt() {
    use crate::support::*;
    use omareel_core::easing::ease_in_out_cubic;

    let mut inputs: Vec<f64> = (-1024..=5120).map(|i| f64::from(i) / 4096.0).collect();
    inputs.extend((0..=1000).map(|i| f64::from(i) / 1000.0));
    for value in [0.5, 0.0, 1.0] {
        inputs.push(next_after(value, -1.0));
        inputs.push(next_after(value, 2.0));
    }
    inputs.extend([
        -0.0,
        1e-300,
        5e-324,
        0.49999999999999994,
        1e300,
        -1e300,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
    ]);
    inputs.extend(read_doubles("numbers/doubles-format.txt").into_iter().filter(|v| (-0.5..=1.5).contains(v)));

    let mut out = Text::new();
    for t in inputs {
        let eased = ease_in_out_cubic(t);
        out.row([hex(t), num(t), num(eased), hex(eased)]);
    }
    assert_golden("easing/ease-in-out-cubic.txt", &out.into_string());
}
