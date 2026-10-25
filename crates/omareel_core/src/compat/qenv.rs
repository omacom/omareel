//! `qEnvironmentVariableIntValue`.

/// `qEnvironmentVariableIntValue(name, &ok)`: the variable read as an `int`, base 10, or
/// hexadecimal after `0x`, or octal after a leading `0`, around ASCII white space; `None` when
/// it is unset or no such number.
pub fn int_value(name: &str) -> Option<i32> {
    parse_int_value(&std::env::var_os(name)?.to_string_lossy())
}

/// `qEnvironmentVariableIntValue(name)`: as [`int_value`], 0 when that is `None`.
pub fn int_value_or_zero(name: &str) -> i32 {
    int_value(name).unwrap_or(0)
}

/// The text of an environment variable read as [`int_value`] reads it.
pub fn parse_int_value(text: &str) -> Option<i32> {
    let text = text.trim_matches(|c: char| c.is_ascii_whitespace() || c == '\x0b');
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let (radix, digits) = if let Some(hex) = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")) {
        (16, hex)
    } else if digits.len() > 1 && digits.starts_with('0') {
        (8, &digits[1..])
    } else {
        (10, digits)
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    let magnitude = i64::from_str_radix(digits, radix).ok()?;
    i32::try_from(if negative { -magnitude } else { magnitude }).ok()
}

