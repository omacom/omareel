//! `QString::number`, `QByteArray::number`, `QString::toDouble`, `QString::toInt` and
//! `QString::toLongLong`, in the C locale, as Qt 6.11 does them.
//!
//! Qt formats doubles with Google's double-conversion and lays the digits out itself
//! (`qt_doubleToAscii` and `dtoString` in `qlocale_tools.cpp`). Digits come from the exact
//! binary value: fixed and precision modes round exact halves up (away from zero), and
//! shortest mode picks the shortest digits that read back to the same double, the even one
//! when the value lies exactly halfway between two candidates.

/// `QLocale::FloatingPointShortest`: as a precision, asks for the fewest digits that read
/// back to the same double.
pub const FLOATING_POINT_SHORTEST: i32 = -128;

/// `QString::number(double n, char format, int precision)`.
///
/// `format` is `'f'` (fixed), `'e'` (exponent) or `'g'` (whichever is shorter, trailing
/// zeros dropped); upper case `'E'`/`'G'` write `E` and `INF`/`NAN`. Any other letter means
/// fixed, as in Qt. `precision` counts digits after the point for `'f'` and `'e'` and
/// significant digits for `'g'`; [`FLOATING_POINT_SHORTEST`] gives the shortest round-trip
/// digits, and any other negative precision means 6. Qt never writes `-0`: negative zero is
/// `"0"`, though a negative value that rounds to zero keeps its sign (`-0.0001` with `'f', 2`
/// is `"-0.00"`). NaN is `"nan"` (never signed), infinities `"inf"`/`"-inf"`. The default
/// `QString::number(d)` is `number_f64(d, 'g', 6)`.
pub fn number_f64(value: f64, format: char, precision: i32) -> String {
    let form = match format.to_ascii_lowercase() {
        'e' => Form::Exponent,
        'g' => Form::SignificantDigits,
        _ => Form::Decimal,
    };
    dto_string(value, form, precision, format.is_ascii_uppercase())
}

/// `QByteArray::number(double n, char format, int precision)`.
///
/// Qt 6.11 implements it with the same template as `QString::number`, so the text is
/// identical. `QJsonDocument` writes every finite double with
/// `byte_array_number(d, 'g', FLOATING_POINT_SHORTEST)` (and NaN or infinity as `null`, which
/// this function never sees there); called directly, NaN gives `"nan"` and infinities
/// `"inf"`/`"-inf"`.
pub fn byte_array_number(value: f64, format: char, precision: i32) -> String {
    number_f64(value, format, precision)
}

/// `QString::number(qlonglong)` (and `QByteArray::number(qlonglong)`): plain base-10 digits
/// with a leading `-` for negatives.
pub fn number_i64(value: i64) -> String {
    value.to_string()
}

/// `QString::toDouble(&ok)`: `Some(value)` when `ok` would be true.
///
/// Qt trims Unicode whitespace (`QChar::isSpace`) from both ends, then accepts
/// `[+-]digits[.digits][(e|E)[+-]digits]` (digits may be missing on one side of the point,
/// not both), `inf`/`+inf`/`-inf` and unsigned `nan` in any case. It rejects hex, `infinity`,
/// signed NaN, spaces after the sign, overflow to infinity, and underflow to zero from a
/// non-zero number.
pub fn to_double(text: &str) -> Option<f64> {
    match to_double_with_ok(text) {
        (value, true) => Some(value),
        (_, false) => None,
    }
}

/// `QString::toDouble(&ok)` as Qt returns it: the value and `ok`. When `ok` is false the
/// value is still what callers that ignore `ok` get: `±inf` on overflow, `±0.0` on underflow,
/// and `0.0` for anything that is not a number.
pub fn to_double_with_ok(text: &str) -> (f64, bool) {
    let trimmed = text.trim_matches(is_qchar_space);
    // Qt narrows to Latin-1 with '?' for anything wider; nothing outside ASCII can be part of
    // a number, so any such character fails the parse either way.
    if !trimmed.is_ascii() {
        return (0.0, false);
    }
    ascii_to_double(trimmed.as_bytes())
}

/// `QByteArray::toDouble(&ok)`: `Some(value)` when `ok` would be true. ASCII whitespace may
/// surround the number; otherwise the rules are `QString::toDouble`'s (the two differ only in
/// what they return when `ok` is false).
pub fn byte_array_to_double(bytes: &[u8]) -> Option<f64> {
    let space = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r');
    let start = bytes.iter().position(|b| !space(b)).unwrap_or(bytes.len());
    let end = bytes.iter().rposition(|b| !space(b)).map_or(start, |i| i + 1);
    match ascii_to_double(&bytes[start..end]) {
        (value, true) => Some(value),
        (_, false) => None,
    }
}

/// `QString::toInt(&ok)` with base 10: [`to_long_long`], then `None` unless the value fits in
/// an `i32`. Qt returns 0 on failure.
pub fn to_int(text: &str) -> Option<i32> {
    to_long_long(text).and_then(|value| i32::try_from(value).ok())
}

/// `QString::toLongLong(&ok)` with base 10.
///
/// Unlike `toDouble`, only ASCII whitespace (`" \t\n\v\f\r"`) is skipped, before the sign and
/// after the digits. One `+` or `-`, then decimal digits; no `0x` prefix in base 10, and
/// overflow fails (but `-9223372036854775808` is accepted). Qt stops at a NUL character, so
/// `"12\0junk"` is 12.
pub fn to_long_long(text: &str) -> Option<i64> {
    // Qt narrows to Latin-1 first; '?' stands in for wider characters, which are never valid.
    let bytes: Vec<u8> = text.chars().map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?')).collect();
    let mut pos = skip_ascii_space(&bytes, 0);
    let negative = bytes.get(pos) == Some(&b'-');
    if negative || bytes.get(pos) == Some(&b'+') {
        pos += 1;
    }
    let digits_end = pos + bytes[pos..].iter().take_while(|b| b.is_ascii_digit()).count();
    if digits_end == pos {
        return None;
    }
    // Whatever follows the digits must be whitespace up to the end or a NUL.
    let rest = skip_ascii_space(&bytes, digits_end);
    if rest < bytes.len() && bytes[rest] != 0 {
        return None;
    }
    let mut magnitude: u64 = 0;
    for &digit in &bytes[pos..digits_end] {
        magnitude = magnitude.checked_mul(10)?.checked_add(u64::from(digit - b'0'))?;
    }
    if negative { 0i64.checked_sub_unsigned(magnitude) } else { i64::try_from(magnitude).ok() }
}

/// Qt's `ascii_isspace`: the six C-locale whitespace characters.
fn is_ascii_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

fn skip_ascii_space(bytes: &[u8], from: usize) -> usize {
    from + bytes[from..].iter().take_while(|&&b| is_ascii_space(b)).count()
}

/// `QChar::isSpace`: Unicode categories Zs, Zl and Zp plus U+0009..U+000D and U+0085.
pub(crate) fn is_qchar_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}'..='\u{d}'
            | ' '
            | '\u{85}'
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
    )
}

/// `qt_asciiToDouble` with trailing junk prohibited: Qt's own `inf`/`nan` check, then
/// double-conversion's `StringToDouble` with no flags, then Qt's overflow and underflow
/// checks.
fn ascii_to_double(num: &[u8]) -> (f64, bool) {
    if num.is_empty() {
        return (0.0, false);
    }
    if let Some(special) = parse_inf_nan(num) {
        return special;
    }
    if !is_plain_decimal(num) {
        return (0.0, false);
    }
    // The grammar is checked, so this is ASCII that Rust's correctly rounded parser accepts.
    let text = std::str::from_utf8(num).unwrap_or_default();
    let Ok(value) = text.parse::<f64>() else {
        return (0.0, false);
    };
    if value.is_infinite() {
        return (value, false);
    }
    if value == 0.0 {
        // Underflow: a non-zero digit before any exponent means a non-zero number was meant.
        let mantissa = num.iter().take_while(|&&b| b != b'e' && b != b'E');
        if mantissa.clone().any(|&b| (b'1'..=b'9').contains(&b)) {
            return (value, false);
        }
    }
    (value, true)
}

/// Qt catches infinity and NaN before double-conversion sees the text: when the first
/// character after an optional sign is past `'9'` (a letter), the whole text must be `inf` or
/// (unsigned) `nan`, case-insensitively, or the parse fails.
fn parse_inf_nan(num: &[u8]) -> Option<(f64, bool)> {
    if num.len() < 3 || !b"+-IiNn".contains(&num[0]) {
        return None;
    }
    let negative = num[0] == b'-';
    let offset = usize::from(num[0] == b'-' || num[0] == b'+');
    if num[offset] <= b'9' {
        return None;
    }
    if num.len() != offset + 3 {
        return Some((0.0, false));
    }
    let word = [num[offset] | 0x20, num[offset + 1] | 0x20, num[offset + 2] | 0x20];
    Some(match &word {
        b"inf" if negative => (f64::NEG_INFINITY, true),
        b"inf" => (f64::INFINITY, true),
        b"nan" if offset == 0 => (f64::NAN, true),
        _ => (0.0, false),
    })
}

/// double-conversion's grammar with no flags: `[+-]`, then digits with an optional point
/// (at least one digit on either side), then an optional exponent with at least one digit,
/// and nothing else.
fn is_plain_decimal(num: &[u8]) -> bool {
    let mut pos = usize::from(matches!(num.first(), Some(b'+' | b'-')));
    let count_digits = |from: usize| num[from..].iter().take_while(|b| b.is_ascii_digit()).count();
    let integer_digits = count_digits(pos);
    pos += integer_digits;
    let mut fraction_digits = 0;
    if num.get(pos) == Some(&b'.') {
        fraction_digits = count_digits(pos + 1);
        pos += 1 + fraction_digits;
    }
    if integer_digits + fraction_digits == 0 {
        return false;
    }
    if matches!(num.get(pos), Some(b'e' | b'E')) {
        pos += 1;
        if matches!(num.get(pos), Some(b'+' | b'-')) {
            pos += 1;
        }
        let exponent_digits = count_digits(pos);
        if exponent_digits == 0 {
            return false;
        }
        pos += exponent_digits;
    }
    pos == num.len()
}

/// `QLocaleData::DoubleForm`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Form {
    Decimal,
    Exponent,
    SignificantDigits,
}

/// Decimal digits of a non-negative value: `0.DIGITS × 10^decpt`, with no leading or
/// trailing zeros. Empty digits mean the value rounded to zero in fixed mode.
struct Digits {
    digits: Vec<u8>,
    decpt: i32,
}

/// `dtoString`: digits from [`double_to_ascii`], laid out as Qt does.
fn dto_string(d: f64, mut form: Form, precision: i32, uppercase: bool) -> String {
    // Undocumented: aside from FloatingPointShortest, a negative precision means 6.
    let precision =
        if precision != FLOATING_POINT_SHORTEST && precision < 0 { 6 } else { precision };
    if !d.is_finite() {
        let text = match (d.is_nan(), d < 0.0) {
            (true, _) => "nan",
            (false, true) => "-inf",
            (false, false) => "inf",
        };
        return if uppercase { text.to_ascii_uppercase() } else { text.to_owned() };
    }
    let Digits { digits, decpt } = double_to_ascii(d.abs(), form, precision);
    let succinct = form == Form::SignificantDigits;
    if succinct {
        form = resolve_format(precision, decpt, digits.len());
    }
    let mut out = String::new();
    if d < 0.0 {
        // Qt never writes "-0", but keeps the sign of a negative value rounded to zero.
        out.push('-');
    }
    let digits = std::str::from_utf8(&digits).unwrap_or_default();
    match form {
        Form::Exponent => push_exponent(&mut out, digits, decpt, precision, succinct, uppercase),
        _ => push_decimal(&mut out, digits, decpt, precision, succinct),
    }
    out
}

/// `resolveFormat`: which layout `'g'` uses.
fn resolve_format(precision: i32, decpt: i32, length: usize) -> Form {
    let length = i32::try_from(length).unwrap_or(i32::MAX);
    let use_decimal = if precision == FLOATING_POINT_SHORTEST {
        // Exponent form adds a separator, a sign and (at least) two exponent digits.
        let mut bias = 2 + 2;
        if length <= decpt && length > 1 {
            bias += 1;
        } else if length == 1 && decpt <= 0 {
            bias -= 1;
        }
        if decpt <= 0 {
            1 - decpt <= bias
        } else if decpt <= length {
            true
        } else {
            decpt <= length + bias
        }
    } else {
        // printf's %g rule: decimal when -4 <= exponent < precision.
        decpt > -4 && decpt <= if precision != 0 { precision } else { 1 }
    };
    if use_decimal { Form::Decimal } else { Form::Exponent }
}

/// The `DFExponent` branch of `dtoString`: `d.ddde+XX`, at least two exponent digits.
fn push_exponent(
    out: &mut String,
    digits: &str,
    decpt: i32,
    precision: i32,
    succinct: bool,
    uppercase: bool,
) {
    let (first, rest) = digits.split_at(digits.len().min(1));
    out.push_str(first);
    if !rest.is_empty() || (!succinct && precision > 0) {
        out.push('.');
        out.push_str(rest);
        if !succinct {
            push_zeros(out, precision - len_i32(rest));
        }
    }
    let exponent = decpt - 1;
    out.push(if uppercase { 'E' } else { 'e' });
    out.push(if exponent < 0 { '-' } else { '+' });
    out.push_str(&format!("{:02}", exponent.unsigned_abs()));
}

/// The `DFDecimal` branch of `dtoString`.
fn push_decimal(out: &mut String, digits: &str, decpt: i32, precision: i32, succinct: bool) {
    if decpt < 0 {
        out.push_str("0.");
        push_zeros(out, -decpt);
        out.push_str(digits);
        if !succinct {
            push_zeros(out, precision + decpt - len_i32(digits));
        }
        return;
    }
    let fraction = if decpt > len_i32(digits) {
        out.push_str(digits);
        push_zeros(out, decpt - len_i32(digits));
        ""
    } else if decpt > 0 {
        let (integer, fraction) = digits.split_at(usize::try_from(decpt).unwrap_or(0));
        out.push_str(integer);
        fraction
    } else {
        out.push('0');
        digits
    };
    if !fraction.is_empty() || (!succinct && len_i32(fraction) < precision) {
        out.push('.');
        out.push_str(fraction);
        if !succinct {
            push_zeros(out, precision - len_i32(fraction));
        }
    }
}

fn push_zeros(out: &mut String, count: i32) {
    for _ in 0..count.max(0) {
        out.push('0');
    }
}

fn len_i32(text: &str) -> i32 {
    i32::try_from(text.len()).unwrap_or(i32::MAX)
}

/// `qt_doubleToAscii` for a finite, non-negative value: the digits double-conversion's
/// `DoubleToAscii` produces, with trailing zeros dropped.
fn double_to_ascii(v: f64, form: Form, precision: i32) -> Digits {
    if v == 0.0 {
        return Digits { digits: vec![b'0'], decpt: 1 };
    }
    let mut result = if precision == FLOATING_POINT_SHORTEST {
        shortest_digits(v)
    } else if form == Form::Decimal {
        fixed_digits(v, precision)
    } else {
        // 0 significant digits silently means 1; 'e' wants one more digit before the point.
        let count = match form {
            Form::Exponent => precision + 1,
            _ => precision.max(1),
        };
        precision_digits(v, count)
    };
    while result.digits.len() > 1 && result.digits.last() == Some(&b'0') {
        result.digits.pop();
    }
    result
}

/// SHORTEST mode: the fewest digits that read back as `v`, closest to `v`. Rust's shortest
/// formatting agrees except when `v` is exactly halfway between two such candidates, where
/// it rounds up and double-conversion (`GenerateShortestDigits`) picks the even digit.
fn shortest_digits(v: f64) -> Digits {
    let text = format!("{v:e}");
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    let shortest: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).collect();
    let decpt = exponent.parse::<i32>().unwrap_or(0) + 1;
    match halfway_alternative(v, &shortest, decpt) {
        Some(even) => Digits { digits: even, decpt },
        None => Digits { digits: shortest, decpt },
    }
}

/// When `v` lies exactly halfway between two shortest candidates (its exact expansion has
/// one digit more than `shortest`, ending in 5) and both read back as `v`, the candidate
/// with the even last digit; `None` when Rust's choice stands.
fn halfway_alternative(v: f64, shortest: &[u8], decpt: i32) -> Option<Vec<u8>> {
    // A tie needs an exact expansion of at most 18 digits, which rules out anything with
    // more than 25 binary fraction digits or above 10^40.
    let (_, exponent) = odd_significand(v);
    if exponent < -25 || v >= 1e40 {
        return None;
    }
    let exact = exact_digits(v);
    if exact.decpt != decpt
        || exact.digits.len() != shortest.len() + 1
        || exact.digits.last() != Some(&b'5')
    {
        return None;
    }
    let down = Digits { digits: exact.digits[..shortest.len()].to_vec(), decpt };
    let up = increment(&down);
    if digits_value(&down) != v || digits_value(&up) != v {
        // Only one candidate reads back; it is the one Rust chose.
        return None;
    }
    let even_down = down.digits.last().is_some_and(|digit| digit % 2 == 0);
    let mut chosen = if even_down { down } else { up };
    while chosen.digits.len() > 1 && chosen.digits.last() == Some(&b'0') {
        chosen.digits.pop();
    }
    (chosen.decpt == decpt && chosen.digits != shortest).then_some(chosen.digits)
}

/// FIXED mode: `v` rounded half up at `precision` digits after the point.
fn fixed_digits(v: f64, precision: i32) -> Digits {
    let exact = exact_digits(v);
    let keep = i64::from(exact.decpt) + i64::from(precision);
    let rounded = round_half_up(exact, keep);
    if rounded.digits.is_empty() {
        // Nothing left: double-conversion reports the point at -precision.
        return Digits { digits: Vec::new(), decpt: -precision };
    }
    rounded
}

/// PRECISION mode: `v` rounded half up to `count` significant digits.
fn precision_digits(v: f64, count: i32) -> Digits {
    round_half_up(exact_digits(v), i64::from(count))
}

/// Keeps the first `keep` digits, rounding up when the next digit is 5 or more (the digits
/// are exact, so that is round half up). A carry out of the first digit moves the point.
fn round_half_up(exact: Digits, keep: i64) -> Digits {
    let Digits { digits, decpt } = exact;
    if keep < 0 {
        return Digits { digits: Vec::new(), decpt };
    }
    let keep = usize::try_from(keep).unwrap_or(usize::MAX);
    if keep >= digits.len() {
        return Digits { digits, decpt };
    }
    let kept = Digits { digits: digits[..keep].to_vec(), decpt };
    if digits[keep] >= b'5' { increment(&kept) } else { kept }
}

/// Adds one in the last place. When the carry runs off the front ("99" or nothing at all),
/// the result is "1" followed by zeros, one place higher.
fn increment(value: &Digits) -> Digits {
    let mut digits = value.digits.clone();
    for digit in digits.iter_mut().rev() {
        if *digit == b'9' {
            *digit = b'0';
        } else {
            *digit += 1;
            return Digits { digits, decpt: value.decpt };
        }
    }
    digits.insert(0, b'1');
    Digits { digits, decpt: value.decpt + 1 }
}

/// The double nearest to `0.DIGITS × 10^decpt`.
fn digits_value(value: &Digits) -> f64 {
    let digits = std::str::from_utf8(&value.digits).unwrap_or("0");
    format!("0.{digits}e{}", value.decpt).parse().unwrap_or(f64::NAN)
}

/// `v = significand × 2^exponent` with an odd significand (for finite, non-zero `v`).
fn odd_significand(v: f64) -> (u64, i32) {
    let bits = v.to_bits();
    let biased = i32::try_from((bits >> 52) & 0x7ff).unwrap_or(0);
    let fraction = bits & ((1 << 52) - 1);
    let (significand, exponent) =
        if biased == 0 { (fraction, -1074) } else { (fraction | 1 << 52, biased - 1075) };
    let zeros = significand.trailing_zeros();
    (significand >> zeros, exponent + i32::try_from(zeros).unwrap_or(0))
}

/// The exact decimal expansion of a finite, positive `v`. A double with `k` binary fraction
/// digits has exactly `k` decimal fraction digits, and Rust prints that many exactly.
fn exact_digits(v: f64) -> Digits {
    let (_, exponent) = odd_significand(v);
    let fraction_digits = usize::try_from(-exponent).unwrap_or(0);
    let text = format!("{v:.fraction_digits$}");
    let (integer, fraction) = text.split_once('.').unwrap_or((&text, ""));
    let mut decpt = len_i32(integer);
    let mut digits: Vec<u8> = integer.bytes().chain(fraction.bytes()).collect();
    let leading = digits.iter().take_while(|&&digit| digit == b'0').count();
    digits.drain(..leading);
    decpt -= i32::try_from(leading).unwrap_or(0);
    while digits.last() == Some(&b'0') {
        digits.pop();
    }
    Digits { digits, decpt }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHORTEST: i32 = FLOATING_POINT_SHORTEST;

    #[test]
    fn default_and_fixed_formats() {
        // Expected strings are Qt 6.11's own output.
        assert_eq!(number_f64(123456789.0, 'g', 6), "1.23457e+08");
        assert_eq!(number_f64(0.1 + 0.2, 'g', 6), "0.3");
        assert_eq!(number_f64(1e-5, 'g', 6), "1e-05");
        assert_eq!(number_f64(1e21, 'g', 6), "1e+21");
        assert_eq!(number_f64(1.5, 'G', 3), "1.5");
        assert_eq!(number_f64(1e-10, 'G', 3), "1E-10");
        assert_eq!(number_f64(1.5, 'x', 3), "1.500");
        assert_eq!(number_f64(1.0, 'f', -5), "1.000000");
        assert_eq!(number_f64(12.3456789, 'f', 6), "12.345679");
        assert_eq!(number_f64(0.0, 'e', 3), "0.000e+00");
        assert_eq!(number_f64(0.0, 'g', 0), "0");
        assert_eq!(number_f64(1.0, 'e', 0), "1e+00");
    }

    #[test]
    fn exact_halves_round_up() {
        assert_eq!(number_f64(0.5, 'f', 0), "1");
        assert_eq!(number_f64(2.5, 'f', 0), "3");
        assert_eq!(number_f64(0.125, 'f', 2), "0.13");
        assert_eq!(number_f64(0.125, 'e', 1), "1.3e-01");
        assert_eq!(number_f64(25.0, 'g', 1), "3e+01");
        assert_eq!(number_f64(9.5, 'e', 0), "1e+01");
        // 0.95 is really 0.94999999999999996.
        assert_eq!(number_f64(0.95, 'f', 1), "0.9");
    }

    #[test]
    fn signs_and_specials() {
        assert_eq!(number_f64(-0.0, 'g', 6), "0");
        assert_eq!(number_f64(-0.0, 'f', 2), "0.00");
        assert_eq!(number_f64(-0.0001, 'f', 2), "-0.00");
        assert_eq!(number_f64(-0.4, 'f', 0), "-0");
        assert_eq!(number_f64(f64::NAN, 'g', 6), "nan");
        assert_eq!(number_f64(-f64::NAN, 'f', 2), "nan");
        assert_eq!(number_f64(f64::NEG_INFINITY, 'E', 3), "-INF");
        assert_eq!(number_f64(f64::INFINITY, 'g', SHORTEST), "inf");
    }

    #[test]
    fn shortest() {
        assert_eq!(number_f64(0.1, 'g', SHORTEST), "0.1");
        assert_eq!(number_f64(0.1 + 0.2, 'g', SHORTEST), "0.30000000000000004");
        assert_eq!(number_f64(0.0001, 'g', SHORTEST), "1e-04");
        assert_eq!(number_f64(0.001, 'g', SHORTEST), "0.001");
        assert_eq!(number_f64(0.00001, 'g', SHORTEST), "1e-05");
        assert_eq!(number_f64(1e15, 'g', SHORTEST), "1e+15");
        assert_eq!(number_f64(123456e10, 'g', SHORTEST), "1.23456e+15");
        assert_eq!(number_f64(9007199254740993.0, 'g', SHORTEST), "9007199254740992");
        assert_eq!(number_f64(5e-324, 'g', SHORTEST), "5e-324");
        assert_eq!(number_f64(1.7976931348623157e308, 'g', SHORTEST), "1.7976931348623157e+308");
        assert_eq!(number_f64(123.456, 'e', SHORTEST), "1.23456e+02");
        assert_eq!(number_f64(100.0, 'e', SHORTEST), "1e+02");
        assert_eq!(number_f64(1.0, 'f', SHORTEST), "1");
        assert_eq!(number_f64(1e22, 'f', SHORTEST), "10000000000000000000000");
        assert_eq!(byte_array_number(65535.0, 'g', SHORTEST), "65535");
        assert_eq!(byte_array_number(-2.5, 'g', SHORTEST), "-2.5");
    }

    #[test]
    #[allow(clippy::excessive_precision)] // the literals are the exact binary values
    fn shortest_halfway_cases_pick_the_even_digit() {
        // Exactly halfway between two 17-digit candidates; Rust's own formatting rounds up.
        assert_eq!(number_f64(261537232372670.625, 'g', SHORTEST), "261537232372670.62");
        assert_eq!(number_f64(1684788023902572.25, 'g', SHORTEST), "1684788023902572.2");
        assert_eq!(number_f64(1125899906842624.75, 'g', SHORTEST), "1125899906842624.8");
    }

    #[test]
    fn long_exact_expansions() {
        assert_eq!(number_f64(0.1, 'e', 30), "1.000000000000000055511151231258e-01");
        assert_eq!(
            number_f64(1e300, 'f', 2),
            concat!(
                "1000000000000000052504760255204420248704468581108159154915854115511802457988908",
                "1957863713750804478640437044438328838781769425232353604305756447921847867069828",
                "4838720092657580373783023379478809005936895323497079994508111903896764088007465",
                "2742780142494579258788820056842838115669472196386865459400540160.00"
            )
        );
    }

    #[test]
    fn integers() {
        assert_eq!(number_i64(0), "0");
        assert_eq!(number_i64(-42), "-42");
        assert_eq!(number_i64(i64::MIN), "-9223372036854775808");
    }

    #[test]
    fn parses_doubles_like_qt() {
        assert_eq!(to_double(" 1.5 "), Some(1.5));
        assert_eq!(to_double("\u{a0}\u{3000}7\u{2029}"), Some(7.0));
        assert_eq!(to_double(".5"), Some(0.5));
        assert_eq!(to_double("5."), Some(5.0));
        assert_eq!(to_double("0.e5"), Some(0.0));
        assert_eq!(to_double("1E5"), Some(100000.0));
        assert_eq!(to_double("-INF"), Some(f64::NEG_INFINITY));
        assert!(to_double("NaN").is_some_and(f64::is_nan));
        assert!(to_double("-0").is_some_and(|d| d == 0.0 && d.is_sign_negative()));
        assert_eq!(to_double("3e-324"), Some(5e-324));
        for bad in [
            "",
            " ",
            ".",
            "-",
            "1e",
            "1e+",
            ".e5",
            "e5",
            "+nan",
            "-nan",
            "infinity",
            "0x10",
            "1,5",
            "1 5",
            "- 5",
            "+-5",
            "1.5\0",
            "\u{180e}1",
            "\u{200b}1",
        ] {
            assert_eq!(to_double(bad), None, "{bad:?}");
        }
        assert_eq!(to_double_with_ok("1e400"), (f64::INFINITY, false));
        assert_eq!(to_double_with_ok("-1e400"), (f64::NEG_INFINITY, false));
        let (underflow, ok) = to_double_with_ok("-1e-400");
        assert!(!ok && underflow == 0.0 && underflow.is_sign_negative());
        assert_eq!(to_double("0e-400"), Some(0.0));
        assert_eq!(to_double_with_ok("abc"), (0.0, false));
    }

    #[test]
    fn parses_integers_like_qt() {
        assert_eq!(to_int(" +7 "), Some(7));
        assert_eq!(to_int("\t-12\n"), Some(-12));
        assert_eq!(to_int("007"), Some(7));
        assert_eq!(to_int("-2147483648"), Some(i32::MIN));
        assert_eq!(to_int("2147483648"), None);
        assert_eq!(to_int("12\0abc"), Some(12));
        assert_eq!(to_int(" 12 \0 x"), Some(12));
        for bad in ["", " ", "+", "-", "+-5", "- 5", "0x10", "1.0", "1e3", "12abc", "\u{a0}5", "\0"]
        {
            assert_eq!(to_int(bad), None, "{bad:?}");
        }
        assert_eq!(to_long_long("-9223372036854775808"), Some(i64::MIN));
        assert_eq!(to_long_long("9223372036854775807"), Some(i64::MAX));
        assert_eq!(to_long_long("9223372036854775808"), None);
        assert_eq!(to_long_long("-9223372036854775809"), None);
    }
}
