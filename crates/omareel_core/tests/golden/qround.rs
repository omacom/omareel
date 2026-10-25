//! qround: `compat::qround` and `compat::cpp` against `qRound`, `qRound64`, `qFuzzyCompare`,
//! `qFuzzyIsNull`, `qBound`, `qMin`/`qMax` and `std::clamp`/`min`/`max` (README "qround").

use crate::support::*;
use omareel_core::compat::cpp;
use omareel_core::compat::qround::{q_bound, q_fuzzy_compare, q_fuzzy_is_null, q_max, q_min};

/// The oracle's `roundInputs`.
#[cfg(target_arch = "x86_64")]
fn round_inputs() -> Vec<f64> {
    let mut values = read_doubles("numbers/doubles.txt");
    for i in -400..=400 {
        let quarter = f64::from(i) / 4.0;
        values.push(quarter);
        if i % 2 != 0 {
            values.push(next_after(quarter, f64::NEG_INFINITY));
            values.push(next_after(quarter, f64::INFINITY));
        }
    }
    for big in [
        2147483646.5,
        2147483646.4999998,
        -2147483647.5,
        4503599627370495.5,
        4503599627370496.5,
        9007199254740991.0,
        4611686018427387904.5,
    ] {
        values.push(big);
        values.push(-big);
    }
    values
}

#[cfg(target_arch = "x86_64")]
#[test]
fn round_txt() {
    use omareel_core::compat::qround::{q_round, q_round64};
    let mut out = Text::new();
    for value in round_inputs() {
        let int = if value.is_finite() && value.abs() < 2147483647.0 { q_round(value).to_string() } else { "-".into() };
        let int64 = if value.is_finite() && value.abs() < 9.2e18 { q_round64(value).to_string() } else { "-".into() };
        out.row([hex(value), num(value), int, int64]);
    }
    assert_golden("qround/round.txt", &out.into_string());
}

fn fuzzy_values() -> Vec<f64> {
    vec![
        0.0,
        -0.0,
        1e-12,
        next_after(1e-12, 0.0),
        next_after(1e-12, 1.0),
        1e-13,
        1e-300,
        5e-324,
        0.5,
        0.55,
        0.55 + 1e-13,
        0.55 + 1e-12,
        0.75,
        0.75 + 1e-13,
        0.75 + 1e-12,
        0.7500000000001,
        0.02,
        0.020000000000001,
        1.0,
        next_after(1.0, 2.0),
        -1.0,
        2.0,
        2.000000000002,
        18.0,
        18.00000000001,
        20.0,
        20.00000000002,
        20.0000000001,
        25.0,
        25.000000000025,
        90.0,
        90.0000000001,
        1e300,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
    ]
}

#[test]
fn fuzzy_compare_txt() {
    let values = fuzzy_values();
    let mut out = Text::new();
    for &a in &values {
        for &b in &values {
            out.row([hex(a), hex(b), boolean(q_fuzzy_compare(a, b)).into()]);
        }
    }
    assert_golden("qround/fuzzy-compare.txt", &out.into_string());
}

#[test]
fn fuzzy_is_null_txt() {
    let mut out = Text::new();
    for a in fuzzy_values() {
        out.row([hex(a), boolean(q_fuzzy_is_null(a)).into()]);
    }
    assert_golden("qround/fuzzy-is-null.txt", &out.into_string());
}

#[test]
fn bound_txt() {
    let values = [
        f64::NAN,
        -f64::NAN,
        f64::NEG_INFINITY,
        -1.0,
        -0.5,
        -0.0,
        0.0,
        1e-300,
        0.25,
        0.5,
        0.75,
        1.0,
        next_after(1.0, 2.0),
        1.5,
        f64::INFINITY,
    ];
    let mut out = Text::new();
    for x in values {
        out.row(
            [
                x,
                q_bound(0.0, x, 1.0),
                cpp::clamp(x, 0.0, 1.0),
                q_max(0.0, x),
                q_max(x, 0.0),
                q_min(x, 1.0),
                q_min(1.0, x),
                cpp::max(0.0, x),
                cpp::max(x, 0.0),
                cpp::min(x, 1.0),
                cpp::min(1.0, x),
                cpp::clamp(x, -0.0, 0.0),
                q_bound(-0.0, x, 0.0),
            ]
            .map(hex),
        );
    }
    assert_golden("qround/bound.txt", &out.into_string());
}
