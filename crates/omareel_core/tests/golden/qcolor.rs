//! qcolor: `compat::qcolor::QColor` against `QColor` (README "qcolor").
//!
//! Parsing runs everywhere. Everything that rounds channels through `qRound(float)` (lighter,
//! darker, `setAlphaF`, `fromRgbF`) is x86_64 only; see `main.rs`.

use crate::support::*;
use omareel_core::compat::qcolor::QColor;

#[test]
fn parse_txt() {
    let mut out = Text::new();
    for input in read_json("qcolor/parse.json").to_array() {
        let text = input.to_string_or("");
        let parsed = QColor::parse(&text);
        out.row([
            quote(&text),
            boolean(parsed.is_valid()).into(),
            color(&parsed),
            parsed.name(),
            color_floats(&parsed),
            // `QColor::isValidColorName(name)` checks exactly what `QColor(name)` accepts; the
            // port has no separate function for it.
            boolean(QColor::parse(&text).is_valid()).into(),
        ]);
    }
    assert_golden("qcolor/parse.txt", &out.into_string());
}

/// `QColor::colorNames()` has no Rust counterpart, so the names come from the golden's first
/// column; what is checked is the colour each of them parses to.
#[test]
fn color_names_txt() {
    let expected = read_expected("qcolor/color-names.txt");
    let mut out = Text::new();
    for row in rows(&expected) {
        let name = row[0].strip_prefix('"').and_then(|n| n.strip_suffix('"')).expect("a quoted name");
        out.row([quote(name), color(&QColor::parse(name))]);
    }
    assert_same("qcolor/color-names.txt", &expected, out.into_string().as_bytes());
}

#[cfg(target_arch = "x86_64")]
const FACTORS: [i32; 13] = [50, 100, 101, 106, 108, 110, 112, 115, 118, 120, 150, 200, 300];

#[cfg(target_arch = "x86_64")]
#[test]
fn lighter_darker_grid_txt() {
    let mut out = Text::new();
    for r in (0..=255).step_by(17) {
        for g in (0..=255).step_by(17) {
            for b in (0..=255).step_by(17) {
                let base = QColor::from_rgb(r, g, b);
                for factor in FACTORS {
                    let (lighter, darker) = (base.lighter(factor), base.darker(factor));
                    let mut cells = vec![
                        base.name(),
                        factor.to_string(),
                        rgba64_hex(&lighter),
                        lighter.name(),
                        rgba64_hex(&darker),
                        darker.name(),
                    ];
                    if lighter.spec() != omareel_core::compat::qcolor::Spec::Rgb
                        || darker.spec() != omareel_core::compat::qcolor::Spec::Rgb
                    {
                        cells.push(spec_name(lighter.spec()).into());
                        cells.push(spec_name(darker.spec()).into());
                    }
                    out.row(cells);
                }
            }
        }
    }
    assert_golden("qcolor/lighter-darker-grid.txt", &out.into_string());
}

/// The input colours of `lighter-darker-special.txt`. `QColor::fromHsv`, `fromHsl` and
/// `fromCmyk` (and the specs they make) have no Rust counterpart, so their rows are left out;
/// the 64 `QRandomGenerator` colours are read back from the golden's input cells.
#[cfg(target_arch = "x86_64")]
#[test]
fn lighter_darker_special_txt() {
    const ROWS_PER_COLOR: usize = 17;
    const UNPORTED_SPECS: [&str; 3] = ["Hsv ", "Hsl ", "Cmyk "];
    let expected = read_expected("qcolor/lighter-darker-special.txt");
    let golden_rows = rows(&expected);

    let mut colors: Vec<QColor> = [
        "#7aa2f7",
        "#1a1b26",
        "#a9b1d6",
        "#faf4ed",
        "#80ff0000",
        "#00000000",
        "#123456789abc",
        "#fff000fff",
        "#ffffff",
        "#000000",
        "#808080",
        "#ff0000",
        "#00ff00",
        "#0000ff",
        "#010101",
        "#fefefe",
    ]
    .map(QColor::parse)
    .to_vec();
    colors.push(QColor::from_rgb_f(0.1, 0.2, 0.3, 0.4));
    colors.push(QColor::from_rgb_f(1.2, -0.1, 0.5, 1.0));
    // fromHsv(200, 128, 250), fromHsl(10, 200, 100) and fromCmyk(10, 20, 30, 40) come here.
    colors.push(QColor::invalid());
    const FIRST_RANDOM: usize = 16 + 2 + 3 + 1;
    for block in golden_rows.chunks(ROWS_PER_COLOR).skip(FIRST_RANDOM) {
        let input = &block[0][0];
        let fields: Vec<&str> = input.split(' ').collect();
        assert_eq!(fields[0], "Rgb", "random colours are Rgb: {input}");
        let channel = |i: usize| u16::from_str_radix(&fields[1][4 * i..4 * i + 4], 16).expect("rgba64 hex");
        colors.push(QColor::from_rgba64(channel(0), channel(1), channel(2), channel(3)));
    }
    assert_eq!(colors.len(), FIRST_RANDOM - 3 + 64);

    let mut out = Text::new();
    for base in &colors {
        for factor in [-10, 0, 1, 99].into_iter().chain(FACTORS) {
            out.row([color(base), factor.to_string(), color(&base.lighter(factor)), color(&base.darker(factor))]);
        }
    }
    let expected = keep_lines(&expected, |line| !UNPORTED_SPECS.iter().any(|spec| line.starts_with(spec.as_bytes())));
    assert_same(
        "qcolor/lighter-darker-special.txt (without the Hsv, Hsl and Cmyk inputs)",
        &expected,
        out.into_string().as_bytes(),
    );
}

#[cfg(target_arch = "x86_64")]
#[test]
fn set_alpha_f_txt() {
    let mut alphas: Vec<f64> = (0..=1000).map(|i| f64::from(i) / 1000.0).collect();
    alphas.extend((0..=255).map(|i| f64::from(i) / 255.0));
    alphas.extend([
        0.04,
        0.06,
        0.07,
        0.08,
        0.10,
        0.11,
        0.12,
        0.14,
        0.15,
        0.16,
        0.18,
        0.2,
        0.22,
        0.25,
        0.28,
        0.35,
        0.36,
        0.40,
        0.5,
        0.56,
        0.7,
        0.95,
        0.97,
        1.0 / 3.0,
        0.5 / 65535.0,
        1.5 / 65535.0,
        1e-9,
        0.99999999,
        -0.0,
    ]);
    alphas.extend([-0.5, 1.5, -1e-9, 1.0000001, f64::NAN]);

    let mut out = Text::new();
    for value in alphas {
        let mut c = QColor::parse("#7aa2f7");
        c.set_alpha_f(value as f32); // a float parameter: the double converts first
        out.row([
            hex(value),
            num(value),
            c.alpha().to_string(),
            c.rgba64().3.to_string(),
            num(f64::from(c.alpha_f())),
            color(&c),
        ]);
    }
    assert_golden("qcolor/set-alpha-f.txt", &out.into_string());
}

/// The arguments are floats: each printed double is converted first. The 1600 rows of
/// `QRandomGenerator` arguments at the end are read back from the golden's input cells.
#[cfg(target_arch = "x86_64")]
#[test]
fn from_rgb_f_txt() {
    let expected = read_expected("qcolor/from-rgb-f.txt");
    let mut channel: Vec<f64> = (0..=512).map(|i| f64::from(i) / 512.0).collect();
    for i in 0..=255 {
        channel.push(f64::from(i) / 255.0);
        channel.push((f64::from(i) + 0.5) / 255.0);
    }
    channel.extend([-0.5, -1e-9, -0.0, 1e-9, 1.0000001, 1.5, 2.0, 0.123456789, 0.999999, 1.0 / 3.0, 2.0 / 3.0]);

    let mut arguments: Vec<[f64; 4]> = channel.iter().map(|&v| [v, 0.25, 0.5, 1.0]).collect();
    arguments.extend(channel.iter().map(|&v| [0.0, 0.0, 0.0, v]));
    let deterministic = arguments.len();
    for row in rows(&expected).into_iter().skip(deterministic) {
        arguments.push([0, 1, 2, 3].map(|i| parse_num(&row[i])));
    }
    assert_eq!(arguments.len(), deterministic + 1600);

    let mut out = Text::new();
    for [r, g, b, a] in arguments {
        let c = QColor::from_rgb_f(r as f32, g as f32, b as f32, a as f32);
        out.row([num(r), num(g), num(b), num(a), color(&c), color_floats(&c)]);
    }
    assert_same("qcolor/from-rgb-f.txt", &expected, out.into_string().as_bytes());
}
