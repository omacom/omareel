//! `QColor`: parsing, names, HSV lighter/darker, float conversions.
//!
//! This is Qt 6.11's `QColor` (`qcolor.cpp`) for the colour specs omareel can produce. Qt
//! keeps every RGB colour as four 16-bit channels and does its HSV round trip in `float` with
//! `qRound`, so `lighter(100)` can move a channel; all of that is reproduced. Out-of-range
//! `fromRgbF` components give an `ExtendedRgb` colour, stored as half floats like Qt does.

use super::qround::{q_bound, q_fuzzy_compare_f32, q_round_f32};

/// `QColor::Spec`, for the specs a colour built through this API can have. (`Hsv` only
/// exists inside [`QColor::lighter`] and [`QColor::darker`].)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spec {
    /// `QColor::Invalid`: what a failed parse or an out-of-range component gives.
    Invalid,
    /// `QColor::Rgb`: 16 bits per channel.
    Rgb,
    /// `QColor::ExtendedRgb`: half-float channels; red, green and blue may lie outside 0..1.
    ExtendedRgb,
}

/// `QColor`, stored as Qt stores it: the spec and `ct.argb`'s four 16-bit channels.
///
/// For `Rgb` (and `Invalid`) the channels are 0..=65535; 8-bit values are `x * 0x101`. For
/// `ExtendedRgb` they hold IEEE half-precision bit patterns. An invalid colour still has
/// channels, and they are observable: `QColor()` has alpha 65535, `QColor(256, 0, 0)` has
/// alpha 0, so the two are not equal and name themselves `#ff000000` and `#00000000`.
#[derive(Clone, Copy, Debug)]
pub struct QColor {
    spec: Spec,
    alpha: u16,
    red: u16,
    green: u16,
    blue: u16,
}

/// The HSV form `lighter` and `darker` work in (`ct.ahsv`, 16-bit; hue in hundredths of a
/// degree, 65535 for achromatic colours).
#[derive(Clone, Copy)]
struct Hsv {
    alpha: u16,
    hue: u16,
    saturation: u16,
    value: u16,
}

const MAX: f32 = 65535.0;

impl Default for QColor {
    /// `QColor()`: invalid, alpha 65535.
    fn default() -> Self {
        QColor::invalid()
    }
}

impl QColor {
    /// `QColor()`: an invalid colour (alpha 65535, other channels 0).
    pub const fn invalid() -> QColor {
        QColor { spec: Spec::Invalid, alpha: 0xffff, red: 0, green: 0, blue: 0 }
    }

    /// `QColor(int r, int g, int b)`: opaque; see [`QColor::from_rgba`].
    pub const fn from_rgb(r: i32, g: i32, b: i32) -> QColor {
        QColor::from_rgba(r, g, b, 255)
    }

    /// `QColor(int r, int g, int b, int a)`: 8-bit components. If any is outside 0..=255 the
    /// colour is invalid with all channels 0, alpha included (unlike [`QColor::invalid`]).
    pub const fn from_rgba(r: i32, g: i32, b: i32, a: i32) -> QColor {
        const fn wide(c: i32) -> u16 {
            (c * 0x101) as u16
        }
        let in_range =
            (r as u32) <= 255 && (g as u32) <= 255 && (b as u32) <= 255 && (a as u32) <= 255;
        if !in_range {
            return QColor { spec: Spec::Invalid, alpha: 0, red: 0, green: 0, blue: 0 };
        }
        QColor { spec: Spec::Rgb, alpha: wide(a), red: wide(r), green: wide(g), blue: wide(b) }
    }

    /// `QColor::fromRgba64(r, g, b, a)`: 16-bit components, always valid.
    pub const fn from_rgba64(r: u16, g: u16, b: u16, a: u16) -> QColor {
        QColor { spec: Spec::Rgb, alpha: a, red: r, green: g, blue: b }
    }

    /// `QColor::fromRgbF(float r, float g, float b, float a)`.
    ///
    /// Alpha outside 0..1 (or NaN) gives [`QColor::invalid`]. Components within 0..1 are
    /// stored as `qRound(c * 65535.f)`. Otherwise red, green or blue outside 0..1 makes an
    /// `ExtendedRgb` colour holding all four as half floats (NaN among them: invalid).
    ///
    /// Qt 6 takes `float`: a C++ caller passing doubles rounds them to `f32` first, and
    /// arithmetic on the float getters stays in `f32` until it meets a double. Theme's
    /// `from.redF() + (to.redF() - from.redF()) * t` with a double `t` is
    /// `(f64::from(from.red_f()) + f64::from(to.red_f() - from.red_f()) * t) as f32`.
    pub fn from_rgb_f(r: f32, g: f32, b: f32, a: f32) -> QColor {
        let in_range = |c: f32| (0.0..=1.0).contains(&c);
        if !in_range(a) {
            return QColor::invalid();
        }
        if in_range(r) && in_range(g) && in_range(b) {
            let channel = |c: f32| q_round_f32(c * MAX) as u16;
            return QColor::from_rgba64(channel(r), channel(g), channel(b), channel(a));
        }
        if r.is_nan() || g.is_nan() || b.is_nan() {
            return QColor::invalid();
        }
        QColor {
            spec: Spec::ExtendedRgb,
            alpha: f16_from_f32(a),
            red: f16_from_f32(r),
            green: f16_from_f32(g),
            blue: f16_from_f32(b),
        }
    }

    /// `QColor(const QString &name)`, i.e. `QColor::fromString(name)`.
    ///
    /// A leading `#` takes hex: `#rgb`, `#rrggbb`, `#aarrggbb`, `#rrrgggbbb` or
    /// `#rrrrggggbbbb`, digits in either case and nothing else (no spaces). Anything else is
    /// looked up among the SVG colour names (plus `transparent`) after dropping every space
    /// and tab and lower-casing ASCII letters. Qt narrows to Latin-1 first, which turns a
    /// character above U+00FF into a NUL, and the lookup stops at a NUL, so `"red\u{100}junk"`
    /// and `"red\0junk"` are red. More than 255 UTF-16 units is never a name. Anything else
    /// is [`QColor::invalid`].
    pub fn parse(name: &str) -> QColor {
        match name.as_bytes().first() {
            None => QColor::invalid(),
            Some(b'#') => parse_hex(&name[1..]).unwrap_or_default(),
            Some(_) => named_rgba(name).map_or_else(QColor::invalid, QColor::from_argb32),
        }
    }

    /// `QColor::isValid()`.
    pub fn is_valid(&self) -> bool {
        self.spec != Spec::Invalid
    }

    /// `QColor::spec()`.
    pub fn spec(&self) -> Spec {
        self.spec
    }

    /// `QColor::name()` (`HexRgb`): `#rrggbb`, lower case, from the 8-bit channels. Invalid
    /// colours name their stored channels (`#000000` for [`QColor::invalid`]).
    pub fn name(&self) -> String {
        let rgb = self.to_rgb();
        format!("#{:02x}{:02x}{:02x}", div_257(rgb.red), div_257(rgb.green), div_257(rgb.blue))
    }

    /// `QColor::name(QColor::HexArgb)`: `#aarrggbb`, lower case.
    pub fn name_argb(&self) -> String {
        let rgb = self.to_rgb();
        format!("#{:02x}{}", div_257(rgb.alpha), &rgb.name()[1..])
    }

    /// `QColor::red()`: 0..=255, the 16-bit channel rounded (`qt_div_257`).
    pub fn red(&self) -> i32 {
        i32::from(div_257(self.to_rgb().red))
    }

    /// `QColor::green()`.
    pub fn green(&self) -> i32 {
        i32::from(div_257(self.to_rgb().green))
    }

    /// `QColor::blue()`.
    pub fn blue(&self) -> i32 {
        i32::from(div_257(self.to_rgb().blue))
    }

    /// `QColor::alpha()`. For `ExtendedRgb` it is `qRound(alphaF() * 255)`, which can differ
    /// from going through 16 bits.
    pub fn alpha(&self) -> i32 {
        match self.spec {
            Spec::ExtendedRgb => q_round_f32(f32_from_f16(self.alpha) * 255.0),
            _ => i32::from(div_257(self.alpha)),
        }
    }

    /// `QColor::redF()`: `float` in Qt 6, the 16-bit channel over `65535.f` (the half float
    /// itself for `ExtendedRgb`).
    pub fn red_f(&self) -> f32 {
        self.channel_f(self.red)
    }

    /// `QColor::greenF()`.
    pub fn green_f(&self) -> f32 {
        self.channel_f(self.green)
    }

    /// `QColor::blueF()`.
    pub fn blue_f(&self) -> f32 {
        self.channel_f(self.blue)
    }

    /// `QColor::alphaF()`.
    pub fn alpha_f(&self) -> f32 {
        self.channel_f(self.alpha)
    }

    /// `QColor::rgba64()` as `(red, green, blue, alpha)` 16-bit channels. `ExtendedRgb`
    /// colours are clamped to 0..1 first.
    pub fn rgba64(&self) -> (u16, u16, u16, u16) {
        let rgb = self.to_rgb();
        (rgb.red, rgb.green, rgb.blue, rgb.alpha)
    }

    /// `QColor::setAlphaF(float)`: NaN becomes 0 and the value is clamped to 0..1, then
    /// stored as `qRound(alpha * 65535.f)` (a half float for `ExtendedRgb`). This applies to
    /// invalid colours too, which stay invalid.
    pub fn set_alpha_f(&mut self, alpha: f32) {
        let alpha = if alpha.is_nan() { 0.0 } else { q_bound(0.0, alpha, 1.0) };
        self.alpha = match self.spec {
            Spec::ExtendedRgb => f16_from_f32(alpha),
            _ => q_round_f32(alpha * MAX) as u16,
        };
    }

    /// `QColor::lighter(int factor)`: the HSV value times `factor / 100`; once the value
    /// tops out, saturation drops by the excess instead. A factor below 100 is
    /// `darker(10000 / factor)`, and a factor of 0 or less returns the colour unchanged. The
    /// result goes back through Qt's `float` HSV-to-RGB conversion, so even `lighter(100)`
    /// can change a channel.
    pub fn lighter(&self, factor: i32) -> QColor {
        if factor <= 0 {
            return *self;
        }
        if factor < 100 {
            return self.darker(10000 / factor);
        }
        let mut hsv = self.to_hsv();
        let mut saturation = i64::from(hsv.saturation);
        // C++ multiplies as `unsigned int`, wrapping on overflow.
        let mut value = u32::try_from(factor).unwrap_or(0).wrapping_mul(u32::from(hsv.value)) / 100;
        if value > u32::from(u16::MAX) {
            saturation = (saturation - (i64::from(value) - 65535)).max(0);
            value = u32::from(u16::MAX);
        }
        hsv.saturation = saturation as u16;
        hsv.value = value as u16;
        self.convert_back(hsv)
    }

    /// `QColor::darker(int factor)`: the HSV value times `100 / factor`, in integers. A
    /// factor below 100 is `lighter(10000 / factor)`; 0 or less returns the colour
    /// unchanged.
    pub fn darker(&self, factor: i32) -> QColor {
        if factor <= 0 {
            return *self;
        }
        if factor < 100 {
            return self.lighter(10000 / factor);
        }
        let mut hsv = self.to_hsv();
        hsv.value = (i32::from(hsv.value) * 100 / factor) as u16;
        self.convert_back(hsv)
    }

    /// `QColor(QRgb)` through `fromRgba`: an `0xAARRGGBB` value at 8 bits per channel.
    fn from_argb32(argb: u32) -> QColor {
        let byte = |shift: u32| ((argb >> shift) & 0xff) as i32;
        QColor::from_rgba(byte(16), byte(8), byte(0), byte(24))
    }

    /// The `redF()`-style getter for one stored channel.
    fn channel_f(&self, channel: u16) -> f32 {
        match self.spec {
            Spec::ExtendedRgb => f32_from_f16(channel),
            _ => f32::from(channel) / MAX,
        }
    }

    /// `QColor::toRgb()`: `ExtendedRgb` clamped to 0..1 and rounded to 16 bits; `Rgb` and
    /// `Invalid` unchanged.
    fn to_rgb(self) -> QColor {
        if self.spec != Spec::ExtendedRgb {
            return self;
        }
        let clamped =
            |channel: u16| q_round_f32(MAX * q_bound(0.0, f32_from_f16(channel), 1.0)) as u16;
        QColor {
            spec: Spec::Rgb,
            alpha: q_round_f32(MAX * f32_from_f16(self.alpha)) as u16,
            red: clamped(self.red),
            green: clamped(self.green),
            blue: clamped(self.blue),
        }
    }

    /// `QColor::toHsv()`, in `float` like Qt. An invalid colour is returned as is, so its
    /// channels are read as hue, saturation and value.
    fn to_hsv(self) -> Hsv {
        let rgb = self.to_rgb();
        let (r, g, b) = (rgb.red, rgb.green, rgb.blue);
        if rgb.spec == Spec::Invalid {
            return Hsv { alpha: rgb.alpha, hue: r, saturation: g, value: b };
        }
        let (min, max) = (r.min(g).min(b), r.max(g).max(b));
        if min == max {
            // Achromatic: hue is undefined.
            return Hsv { alpha: rgb.alpha, hue: u16::MAX, saturation: 0, value: max };
        }
        let chroma = f32::from(max - min);
        let saturation = q_round_f32((chroma / f32::from(max)) * MAX) as u16;
        let delta = |a: u16, b: u16| (i32::from(a) - i32::from(b)) as f32;
        let hue = if max == r {
            let hue = 0.0 + delta(g, b) / chroma;
            if hue < 0.0 { hue + 6.0 } else { hue }
        } else if max == g {
            2.0 + delta(b, r) / chroma
        } else {
            4.0 + delta(r, g) / chroma
        };
        Hsv { alpha: rgb.alpha, hue: q_round_f32(hue * 6000.0) as u16, saturation, value: max }
    }

    /// `hsv.convertTo(cspec)` for this colour's spec: back to `Rgb`, on to `ExtendedRgb`
    /// through `Rgb`, or (invalid) the HSV channels kept as they are.
    fn convert_back(self, hsv: Hsv) -> QColor {
        match self.spec {
            Spec::Invalid => QColor {
                spec: Spec::Invalid,
                alpha: hsv.alpha,
                red: hsv.hue,
                green: hsv.saturation,
                blue: hsv.value,
            },
            Spec::Rgb => hsv_to_rgb(hsv),
            Spec::ExtendedRgb => hsv_to_rgb(hsv).to_extended_rgb(),
        }
    }

    /// `QColor::toExtendedRgb()` from `Rgb`: each channel times `1.0f / 65535` as a half
    /// float.
    fn to_extended_rgb(self) -> QColor {
        const SCALE: f32 = 1.0 / 65535.0;
        let half = |channel: u16| f16_from_f32(f32::from(channel) * SCALE);
        QColor {
            spec: Spec::ExtendedRgb,
            alpha: half(self.alpha),
            red: half(self.red),
            green: half(self.green),
            blue: half(self.blue),
        }
    }
}

impl PartialEq for QColor {
    /// `QColor::operator==`. When either side is `ExtendedRgb` and the other is too, or is
    /// `Rgb`, the float channels are compared with `qFuzzyCompare(float, float)` (so a channel
    /// of exactly 0 only equals 0, and infinite channels never compare equal). Otherwise the
    /// spec and all stored channels must match, invalid colours included.
    fn eq(&self, other: &QColor) -> bool {
        let extended = self.spec == Spec::ExtendedRgb || other.spec == Spec::ExtendedRgb;
        let comparable =
            self.spec == other.spec || self.spec == Spec::Rgb || other.spec == Spec::Rgb;
        if extended && comparable {
            return q_fuzzy_compare_f32(self.alpha_f(), other.alpha_f())
                && q_fuzzy_compare_f32(self.red_f(), other.red_f())
                && q_fuzzy_compare_f32(self.green_f(), other.green_f())
                && q_fuzzy_compare_f32(self.blue_f(), other.blue_f());
        }
        self.spec == other.spec
            && self.alpha == other.alpha
            && self.red == other.red
            && self.green == other.green
            && self.blue == other.blue
    }
}

/// `QColor::toRgb()` from HSV, in `float` with `qRound(float)` like Qt.
fn hsv_to_rgb(hsv: Hsv) -> QColor {
    let rgb = |red: u16, green: u16, blue: u16| QColor::from_rgba64(red, green, blue, hsv.alpha);
    if hsv.saturation == 0 || hsv.hue == u16::MAX {
        // Achromatic.
        return rgb(hsv.value, hsv.value, hsv.value);
    }
    let h = if hsv.hue == 36000 { 0.0 } else { f32::from(hsv.hue) / 6000.0 };
    let s = f32::from(hsv.saturation) / MAX;
    let v = f32::from(hsv.value) / MAX;
    let sector = h as i32;
    let f = h - sector as f32;
    let p = v * (1.0 - s);
    let channel = |x: f32| q_round_f32(x * MAX) as u16;
    let (red, green, blue) = if sector & 1 == 1 {
        let q = v * one_minus_product(s, f);
        match sector {
            1 => (q, v, p),
            3 => (p, q, v),
            _ => (v, p, q),
        }
    } else {
        let t = v * one_minus_product(s, 1.0 - f);
        match sector {
            0 => (v, t, p),
            2 => (p, v, t),
            _ => (t, p, v),
        }
    };
    rgb(channel(red), channel(green), channel(blue))
}

/// `1.0f - (a * b)` as Qt's binaries compute it in `toRgb`. GCC contracts it into one fused
/// multiply-subtract on aarch64 (Arch Linux ARM's libQt6Gui uses `fmsub` there), which skips
/// rounding the product; x86-64 builds have no FMA and round twice. The two disagree on about
/// one `lighter`/`darker` result in 2500.
fn one_minus_product(a: f32, b: f32) -> f32 {
    if cfg!(target_arch = "aarch64") { (-a).mul_add(b, 1.0) } else { 1.0 - a * b }
}

/// `qt_div_257`: a 16-bit channel to 8 bits, rounding to nearest.
fn div_257(x: u16) -> u8 {
    let x = u32::from(x) + 128;
    ((x - (x >> 8)) >> 8) as u8
}

/// `get_hex_rgb` for the text after `#`: 3, 6, 8 (`aarrggbb`), 9 or 12 hex digits.
fn parse_hex(digits: &str) -> Option<QColor> {
    // Anything outside ASCII is never a hex digit (Qt sees '\0' or a Latin-1 letter).
    let digits = digits.as_bytes();
    if !digits.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    // The `index`th group of `width` digits.
    let group = |index: usize, width: usize| {
        digits[index * width..(index + 1) * width].iter().fold(0u16, |value, &digit| {
            value << 4 | (char::from(digit).to_digit(16).unwrap_or(0) as u16)
        })
    };
    let (red, green, blue, alpha) = match digits.len() {
        3 => (group(0, 1) * 0x1111, group(1, 1) * 0x1111, group(2, 1) * 0x1111, u16::MAX),
        6 => (group(0, 2) * 0x101, group(1, 2) * 0x101, group(2, 2) * 0x101, u16::MAX),
        8 => (group(1, 2) * 0x101, group(2, 2) * 0x101, group(3, 2) * 0x101, group(0, 2) * 0x101),
        9 => {
            // 12 bits per channel, widened by repeating the top nibble.
            let widen = |x: u16| x << 4 | x >> 8;
            (widen(group(0, 3)), widen(group(1, 3)), widen(group(2, 3)), u16::MAX)
        }
        12 => (group(0, 4), group(1, 4), group(2, 4), u16::MAX),
        _ => return None,
    };
    Some(QColor::from_rgba64(red, green, blue, alpha))
}

/// `get_named_rgb`: the `0xAARRGGBB` value of an SVG colour name, ignoring spaces, tabs and
/// ASCII case, read up to the first NUL (or character above U+00FF).
fn named_rgba(name: &str) -> Option<u32> {
    if name.encode_utf16().count() > 255 {
        return None;
    }
    let mut key = String::new();
    for c in name.chars() {
        match c {
            ' ' | '\t' => {}
            '\0' => break,
            c if u32::from(c) > 0xff => break,
            c => key.push(c.to_ascii_lowercase()),
        }
    }
    let index = NAMED_COLORS.binary_search_by(|(entry, _)| entry.cmp(&key.as_str())).ok()?;
    Some(NAMED_COLORS[index].1)
}

/// An IEEE half float's bit pattern from `f32`, rounding to nearest even (`qfloat16(float)`).
fn f16_from_f32(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32;
    let mantissa = bits & 0x7f_ffff;
    if exponent == 0xff {
        // Infinity stays infinity; NaN stays a (quiet) NaN.
        let nan = if mantissa != 0 { 0x200 | (mantissa >> 13) as u16 } else { 0 };
        return sign | 0x7c00 | nan;
    }
    let unbiased = exponent - 127;
    if unbiased > 15 {
        return sign | 0x7c00;
    }
    // Normal halves keep the top 10 of the 23 mantissa bits; subnormal ones count units of
    // 2^-24, taken from the significand with its implicit bit.
    let (significand, dropped_bits, exponent_field) = if unbiased >= -14 {
        (mantissa, 13, ((unbiased + 15) as u32) << 10)
    } else if unbiased >= -25 {
        (mantissa | 0x80_0000, (-unbiased - 1) as u32, 0)
    } else {
        return sign;
    };
    let kept = exponent_field | (significand >> dropped_bits);
    let dropped = significand & ((1 << dropped_bits) - 1);
    let half = 1 << (dropped_bits - 1);
    let round_up = dropped > half || (dropped == half && kept & 1 == 1);
    // A carry out of the mantissa bumps the exponent, up to infinity: that is correct.
    sign | (kept + u32::from(round_up)) as u16
}

/// An `f32` from an IEEE half float's bit pattern (exact).
fn f32_from_f16(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = u32::from((bits >> 10) & 0x1f);
    let mantissa = u32::from(bits & 0x3ff);
    let magnitude = match exponent {
        0 => mantissa as f32 * f32::from_bits(0x3380_0000), // 2^-24
        0x1f => f32::from_bits(0x7f80_0000 | (mantissa << 13)),
        _ => f32::from_bits(((exponent + 112) << 23) | (mantissa << 13)),
    };
    sign * magnitude
}

/// Qt's `rgbTbl`: the SVG colour names plus `transparent`, sorted, as `0xAARRGGBB`.
const NAMED_COLORS: [(&str, u32); 148] = [
    ("aliceblue", 0xfff0f8ff),
    ("antiquewhite", 0xfffaebd7),
    ("aqua", 0xff00ffff),
    ("aquamarine", 0xff7fffd4),
    ("azure", 0xfff0ffff),
    ("beige", 0xfff5f5dc),
    ("bisque", 0xffffe4c4),
    ("black", 0xff000000),
    ("blanchedalmond", 0xffffebcd),
    ("blue", 0xff0000ff),
    ("blueviolet", 0xff8a2be2),
    ("brown", 0xffa52a2a),
    ("burlywood", 0xffdeb887),
    ("cadetblue", 0xff5f9ea0),
    ("chartreuse", 0xff7fff00),
    ("chocolate", 0xffd2691e),
    ("coral", 0xffff7f50),
    ("cornflowerblue", 0xff6495ed),
    ("cornsilk", 0xfffff8dc),
    ("crimson", 0xffdc143c),
    ("cyan", 0xff00ffff),
    ("darkblue", 0xff00008b),
    ("darkcyan", 0xff008b8b),
    ("darkgoldenrod", 0xffb8860b),
    ("darkgray", 0xffa9a9a9),
    ("darkgreen", 0xff006400),
    ("darkgrey", 0xffa9a9a9),
    ("darkkhaki", 0xffbdb76b),
    ("darkmagenta", 0xff8b008b),
    ("darkolivegreen", 0xff556b2f),
    ("darkorange", 0xffff8c00),
    ("darkorchid", 0xff9932cc),
    ("darkred", 0xff8b0000),
    ("darksalmon", 0xffe9967a),
    ("darkseagreen", 0xff8fbc8f),
    ("darkslateblue", 0xff483d8b),
    ("darkslategray", 0xff2f4f4f),
    ("darkslategrey", 0xff2f4f4f),
    ("darkturquoise", 0xff00ced1),
    ("darkviolet", 0xff9400d3),
    ("deeppink", 0xffff1493),
    ("deepskyblue", 0xff00bfff),
    ("dimgray", 0xff696969),
    ("dimgrey", 0xff696969),
    ("dodgerblue", 0xff1e90ff),
    ("firebrick", 0xffb22222),
    ("floralwhite", 0xfffffaf0),
    ("forestgreen", 0xff228b22),
    ("fuchsia", 0xffff00ff),
    ("gainsboro", 0xffdcdcdc),
    ("ghostwhite", 0xfff8f8ff),
    ("gold", 0xffffd700),
    ("goldenrod", 0xffdaa520),
    ("gray", 0xff808080),
    ("green", 0xff008000),
    ("greenyellow", 0xffadff2f),
    ("grey", 0xff808080),
    ("honeydew", 0xfff0fff0),
    ("hotpink", 0xffff69b4),
    ("indianred", 0xffcd5c5c),
    ("indigo", 0xff4b0082),
    ("ivory", 0xfffffff0),
    ("khaki", 0xfff0e68c),
    ("lavender", 0xffe6e6fa),
    ("lavenderblush", 0xfffff0f5),
    ("lawngreen", 0xff7cfc00),
    ("lemonchiffon", 0xfffffacd),
    ("lightblue", 0xffadd8e6),
    ("lightcoral", 0xfff08080),
    ("lightcyan", 0xffe0ffff),
    ("lightgoldenrodyellow", 0xfffafad2),
    ("lightgray", 0xffd3d3d3),
    ("lightgreen", 0xff90ee90),
    ("lightgrey", 0xffd3d3d3),
    ("lightpink", 0xffffb6c1),
    ("lightsalmon", 0xffffa07a),
    ("lightseagreen", 0xff20b2aa),
    ("lightskyblue", 0xff87cefa),
    ("lightslategray", 0xff778899),
    ("lightslategrey", 0xff778899),
    ("lightsteelblue", 0xffb0c4de),
    ("lightyellow", 0xffffffe0),
    ("lime", 0xff00ff00),
    ("limegreen", 0xff32cd32),
    ("linen", 0xfffaf0e6),
    ("magenta", 0xffff00ff),
    ("maroon", 0xff800000),
    ("mediumaquamarine", 0xff66cdaa),
    ("mediumblue", 0xff0000cd),
    ("mediumorchid", 0xffba55d3),
    ("mediumpurple", 0xff9370db),
    ("mediumseagreen", 0xff3cb371),
    ("mediumslateblue", 0xff7b68ee),
    ("mediumspringgreen", 0xff00fa9a),
    ("mediumturquoise", 0xff48d1cc),
    ("mediumvioletred", 0xffc71585),
    ("midnightblue", 0xff191970),
    ("mintcream", 0xfff5fffa),
    ("mistyrose", 0xffffe4e1),
    ("moccasin", 0xffffe4b5),
    ("navajowhite", 0xffffdead),
    ("navy", 0xff000080),
    ("oldlace", 0xfffdf5e6),
    ("olive", 0xff808000),
    ("olivedrab", 0xff6b8e23),
    ("orange", 0xffffa500),
    ("orangered", 0xffff4500),
    ("orchid", 0xffda70d6),
    ("palegoldenrod", 0xffeee8aa),
    ("palegreen", 0xff98fb98),
    ("paleturquoise", 0xffafeeee),
    ("palevioletred", 0xffdb7093),
    ("papayawhip", 0xffffefd5),
    ("peachpuff", 0xffffdab9),
    ("peru", 0xffcd853f),
    ("pink", 0xffffc0cb),
    ("plum", 0xffdda0dd),
    ("powderblue", 0xffb0e0e6),
    ("purple", 0xff800080),
    ("red", 0xffff0000),
    ("rosybrown", 0xffbc8f8f),
    ("royalblue", 0xff4169e1),
    ("saddlebrown", 0xff8b4513),
    ("salmon", 0xfffa8072),
    ("sandybrown", 0xfff4a460),
    ("seagreen", 0xff2e8b57),
    ("seashell", 0xfffff5ee),
    ("sienna", 0xffa0522d),
    ("silver", 0xffc0c0c0),
    ("skyblue", 0xff87ceeb),
    ("slateblue", 0xff6a5acd),
    ("slategray", 0xff708090),
    ("slategrey", 0xff708090),
    ("snow", 0xfffffafa),
    ("springgreen", 0xff00ff7f),
    ("steelblue", 0xff4682b4),
    ("tan", 0xffd2b48c),
    ("teal", 0xff008080),
    ("thistle", 0xffd8bfd8),
    ("tomato", 0xffff6347),
    ("transparent", 0x00000000),
    ("turquoise", 0xff40e0d0),
    ("violet", 0xffee82ee),
    ("wheat", 0xfff5deb3),
    ("white", 0xffffffff),
    ("whitesmoke", 0xfff5f5f5),
    ("yellow", 0xffffff00),
    ("yellowgreen", 0xff9acd32),
];

#[cfg(test)]
mod tests {
    use super::*;

    // Expected values are Qt 6.11's own output (x86-64; aarch64 Qt agrees unless noted).

    fn rgba64(name: &str) -> (u16, u16, u16, u16) {
        QColor::parse(name).rgba64()
    }

    #[test]
    fn parses_hex_forms() {
        let c = QColor::parse("#abc");
        assert_eq!((c.name(), c.name_argb()), ("#aabbcc".into(), "#ffaabbcc".into()));
        assert_eq!(QColor::parse("#80ff0000").alpha(), 128);
        assert_eq!(QColor::parse("#80ff0000").name_argb(), "#80ff0000");
        assert_eq!(rgba64("#123456789"), (4657, 17764, 30871, 65535));
        assert_eq!(rgba64("#0123456789ab"), (291, 17767, 35243, 65535));
        assert_eq!(QColor::parse("#7AA2F7"), QColor::from_rgb(0x7a, 0xa2, 0xf7));
        for bad in ["", "#", "#12", "#1234", "#1234567", "#12345678901", " #abc", "#abc ", "#ggg"] {
            assert!(!QColor::parse(bad).is_valid(), "{bad:?}");
        }
    }

    #[test]
    fn parses_names_like_qt() {
        assert_eq!(QColor::parse(" R e d\t").name(), "#ff0000");
        assert_eq!(QColor::parse("LightGoldenRodYellow").name(), "#fafad2");
        // Qt narrows to Latin-1 (U+0100 becomes NUL) and stops at the NUL.
        assert_eq!(QColor::parse("red\u{100}junk").name(), "#ff0000");
        assert_eq!(QColor::parse("red\0junk").name(), "#ff0000");
        assert_eq!(QColor::parse("transparent").name_argb(), "#00000000");
        // 255 UTF-16 units is the limit, spaces included.
        assert!(QColor::parse(&format!("{}red", " ".repeat(252))).is_valid());
        assert!(!QColor::parse(&format!("{}red", " ".repeat(253))).is_valid());
        assert!(!QColor::parse("redd").is_valid());
        assert!(!QColor::parse("red\n").is_valid());
    }

    #[test]
    fn invalid_colours_keep_their_channels() {
        let default = QColor::invalid();
        let out_of_range = QColor::from_rgb(256, 0, 0);
        assert!(!default.is_valid() && !out_of_range.is_valid());
        assert_eq!(default.name_argb(), "#ff000000");
        assert_eq!(out_of_range.name_argb(), "#00000000");
        assert_ne!(default, out_of_range);
        assert_eq!(QColor::parse("nope"), QColor::default());
        let mut faded = QColor::invalid();
        faded.set_alpha_f(0.5);
        assert!(!faded.is_valid());
        assert_eq!((faded.alpha(), faded.name_argb()), (128, "#80000000".into()));
        assert_eq!(faded.lighter(150), faded);
    }

    #[test]
    fn lighter_and_darker() {
        let accent = QColor::parse("#7aa2f7");
        assert_eq!(accent.lighter(112).name(), "#94b6ff");
        assert_eq!(accent.lighter(112).rgba64(), (37931, 46764, 65535, 65535));
        assert_eq!(accent.darker(108).name(), "#7196e5");
        assert_eq!(accent.darker(118).name(), "#6789d1");
        assert_eq!(QColor::parse("#1a1b26").lighter(106).rgba64(), (7082, 7355, 10351, 65535));
        let orange = QColor::parse("#ff8a00");
        assert_eq!(orange.lighter(300).name(), "#ffffff");
        assert_eq!(orange.lighter(50).rgba64(), (32767, 17732, 0, 65535));
        assert_eq!(orange.darker(0), orange);
        // The float HSV round trip moves 16-bit channels even at factor 100.
        assert_eq!(
            QColor::from_rgba64(0, 1009, 10130, 65535).lighter(100).rgba64(),
            (0, 1010, 10130, 65535)
        );
    }

    #[test]
    #[cfg(target_arch = "x86_64")]
    fn darker_rounds_like_x86_64_qt() {
        assert_eq!(QColor::from_rgb(24, 175, 126).darker(112).name(), "#159c70");
    }

    #[test]
    #[cfg(target_arch = "aarch64")]
    fn darker_rounds_like_aarch64_qt() {
        // Arch Linux ARM's Qt fuses `1 - s * f` into one multiply-subtract.
        assert_eq!(QColor::from_rgb(24, 175, 126).darker(112).name(), "#159c71");
    }

    #[test]
    fn float_channels() {
        let c = QColor::from_rgb_f(0.1, 0.2, 0.3, 0.4);
        assert_eq!(c.rgba64(), (6554, 13107, 19661, 26214));
        assert_eq!(c.name_argb(), "#661a334d");
        assert_eq!(c.red_f(), 0.100_007_63);
        let mut c = QColor::from_rgb(169, 177, 214);
        c.set_alpha_f(0.07);
        assert_eq!((c.rgba64().3, c.alpha(), c.name_argb()), (4587, 18, "#12a9b1d6".into()));
        c.set_alpha_f(f32::NAN);
        assert_eq!(c.alpha(), 0);
        assert!(!QColor::from_rgb_f(0.5, 0.5, 0.5, 1.5).is_valid());
        assert!(!QColor::from_rgb_f(f32::NAN, 2.0, 0.0, 1.0).is_valid());
    }

    #[test]
    fn extended_rgb() {
        let c = QColor::from_rgb_f(1.5, -0.25, 0.5, 1.0);
        assert_eq!(c.spec(), Spec::ExtendedRgb);
        assert_eq!((c.red_f(), c.green_f(), c.blue_f()), (1.5, -0.25, 0.5));
        assert_eq!((c.name(), c.rgba64()), ("#ff0080".into(), (65535, 0, 32768, 65535)));
        let lighter = c.lighter(150);
        assert_eq!(lighter.spec(), Spec::ExtendedRgb);
        assert_eq!((lighter.red_f(), lighter.green_f(), lighter.blue_f()), (1.0, 0.5, 0.75));
        // Fuzzy comparison: Rgb's 16-bit 0.5 is not within float fuzz of the half float 0.5,
        // and infinite channels never compare equal.
        assert_ne!(
            QColor::from_rgb_f(1.000_000_1, 0.5, 0.25, 1.0),
            QColor::from_rgb_f(1.0, 0.5, 0.25, 1.0)
        );
        let infinite = QColor::from_rgb_f(f32::INFINITY, 0.5, 0.25, 1.0);
        assert_ne!(infinite, infinite);
    }

    #[test]
    fn theme_mix_color() {
        // Theme's mixColor(#a9b1d6, #1a1b26, .36), written as the C++ evaluates it.
        let (from, to, t) = (QColor::parse("#a9b1d6"), QColor::parse("#1a1b26"), 0.36);
        let mix = |a: f32, b: f32| (f64::from(a) + f64::from(b - a) * t) as f32;
        let mixed = QColor::from_rgb_f(
            mix(from.red_f(), to.red_f()),
            mix(from.green_f(), to.green_f()),
            mix(from.blue_f(), to.blue_f()),
            mix(from.alpha_f(), to.alpha_f()),
        );
        assert_eq!(
            (mixed.name(), mixed.rgba64()),
            ("#767b97".into(), (30203, 31611, 38714, 65535))
        );
    }

    #[test]
    fn half_float_conversion_rounds_to_nearest_even() {
        assert_eq!(f16_from_f32(1.0), 0x3c00);
        assert_eq!(f16_from_f32(65504.0), 0x7bff);
        assert_eq!(f16_from_f32(65520.0), 0x7c00);
        assert_eq!(f16_from_f32(f32::NEG_INFINITY), 0xfc00);
        assert_eq!(f16_from_f32(2.0f32.powi(-24)), 0x0001);
        assert_eq!(f16_from_f32(2.0f32.powi(-25)), 0x0000);
        assert_eq!(f16_from_f32(1.0 + 2.0f32.powi(-11)), 0x3c00);
        assert_eq!(f16_from_f32(1.0 + 3.0 * 2.0f32.powi(-11)), 0x3c02);
        assert_eq!(f32_from_f16(0x0001), 2.0f32.powi(-24));
        assert_eq!(f32_from_f16(0xc000), -2.0);
    }
}
