//! `QString` operations whose rules are not Rust's `str` rules, as Qt 6.11.2 has them.

use super::qchar::{map_case, CaseRun};

/// `QString::size()`: the length in UTF-16 code units, so a character outside the Basic
/// Multilingual Plane counts twice.
pub fn size(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `QString::toLower()`: each code point lowercased on its own, by its full mapping (U+0130 `İ`
/// becomes `i` and U+0307). There is no context: unlike `str::to_lowercase`, a final `Σ` becomes
/// `σ`, not `ς`. The table is Qt's (see `qchar`), so the result does not depend on the Unicode
/// version of the Rust toolchain.
pub fn to_lower(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        if c == '\u{130}' {
            result.push_str("i\u{307}");
            continue;
        }
        // The table maps scalar values to scalar values.
        result.push(char::from_u32(map_case(&LOWER, u32::from(c))).unwrap_or(c));
    }
    result
}

/// `QString::arg(const QString &a)`: replaces every occurrence of the lowest-numbered `%N`
/// placeholder with `a`. Placeholders are `%` and one or two ASCII digits (`%0` to `%99`, so
/// `%02` is 2), optionally with an `L` after the `%`, which only matters for numbers. The
/// replacement is not scanned again, but the result of one `arg` is scanned whole by the next,
/// so `"%1-s%2".arg("a%1").arg(5)` is `"a5-s%2"`. Without a placeholder Qt warns
/// ("QString::arg: Argument missing") and returns the string unchanged.
///
/// For a number, pass its decimal text: that is what Qt's integer overload inserts for `%N`. For
/// `%LN` Qt would format it in the default `QLocale` (digit grouping and, in a few locales, other
/// digits), which this does not reproduce; under the C locale the two are the same.
pub fn arg(s: &str, a: &str) -> String {
    let bytes = s.as_bytes();
    let digit = |i: usize| bytes.get(i).filter(|b| b.is_ascii_digit()).map(|b| u32::from(b - b'0'));
    // (start, end, number) of every placeholder, scanned as Qt's findArgEscapes does.
    let mut escapes = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        if bytes.get(i) == Some(&b'L') {
            i += 1;
        }
        let Some(first) = digit(i) else { continue };
        i += 1;
        let mut number = first;
        if let Some(second) = digit(i) {
            number = number * 10 + second;
            i += 1;
        }
        escapes.push((start, i, number));
    }
    let Some(lowest) = escapes.iter().map(|&(_, _, number)| number).min() else { return s.to_owned() };
    let mut result = String::with_capacity(s.len() + a.len());
    let mut copied = 0;
    for (start, end, number) in escapes {
        if number == lowest {
            result.push_str(&s[copied..start]);
            result.push_str(a);
            copied = end;
        }
    }
    result.push_str(&s[copied..]);
    result
}

/// Code points that `QString::toLower()` maps to one other code point. U+0130, the one with a
/// longer mapping, is handled in `to_lower`.
#[rustfmt::skip]
const LOWER: [CaseRun; 185] = [
    (0x0041, 0x005A, 32, 1), (0x00C0, 0x00D6, 32, 1), (0x00D8, 0x00DE, 32, 1), (0x0100, 0x012E, 1, 2),
    (0x0132, 0x0136, 1, 2), (0x0139, 0x0147, 1, 2), (0x014A, 0x0176, 1, 2), (0x0178, 0x0178, -121, 1),
    (0x0179, 0x017D, 1, 2), (0x0181, 0x0181, 210, 1), (0x0182, 0x0184, 1, 2), (0x0186, 0x0186, 206, 1),
    (0x0187, 0x0187, 1, 1), (0x0189, 0x018A, 205, 1), (0x018B, 0x018B, 1, 1), (0x018E, 0x018E, 79, 1),
    (0x018F, 0x018F, 202, 1), (0x0190, 0x0190, 203, 1), (0x0191, 0x0191, 1, 1), (0x0193, 0x0193, 205, 1),
    (0x0194, 0x0194, 207, 1), (0x0196, 0x0196, 211, 1), (0x0197, 0x0197, 209, 1), (0x0198, 0x0198, 1, 1),
    (0x019C, 0x019C, 211, 1), (0x019D, 0x019D, 213, 1), (0x019F, 0x019F, 214, 1), (0x01A0, 0x01A4, 1, 2),
    (0x01A6, 0x01A6, 218, 1), (0x01A7, 0x01A7, 1, 1), (0x01A9, 0x01A9, 218, 1), (0x01AC, 0x01AC, 1, 1),
    (0x01AE, 0x01AE, 218, 1), (0x01AF, 0x01AF, 1, 1), (0x01B1, 0x01B2, 217, 1), (0x01B3, 0x01B5, 1, 2),
    (0x01B7, 0x01B7, 219, 1), (0x01B8, 0x01B8, 1, 1), (0x01BC, 0x01BC, 1, 1), (0x01C4, 0x01C4, 2, 1),
    (0x01C5, 0x01C5, 1, 1), (0x01C7, 0x01C7, 2, 1), (0x01C8, 0x01C8, 1, 1), (0x01CA, 0x01CA, 2, 1),
    (0x01CB, 0x01DB, 1, 2), (0x01DE, 0x01EE, 1, 2), (0x01F1, 0x01F1, 2, 1), (0x01F2, 0x01F4, 1, 2),
    (0x01F6, 0x01F6, -97, 1), (0x01F7, 0x01F7, -56, 1), (0x01F8, 0x021E, 1, 2), (0x0220, 0x0220, -130, 1),
    (0x0222, 0x0232, 1, 2), (0x023A, 0x023A, 10795, 1), (0x023B, 0x023B, 1, 1), (0x023D, 0x023D, -163, 1),
    (0x023E, 0x023E, 10792, 1), (0x0241, 0x0241, 1, 1), (0x0243, 0x0243, -195, 1), (0x0244, 0x0244, 69, 1),
    (0x0245, 0x0245, 71, 1), (0x0246, 0x024E, 1, 2), (0x0370, 0x0372, 1, 2), (0x0376, 0x0376, 1, 1),
    (0x037F, 0x037F, 116, 1), (0x0386, 0x0386, 38, 1), (0x0388, 0x038A, 37, 1), (0x038C, 0x038C, 64, 1),
    (0x038E, 0x038F, 63, 1), (0x0391, 0x03A1, 32, 1), (0x03A3, 0x03AB, 32, 1), (0x03CF, 0x03CF, 8, 1),
    (0x03D8, 0x03EE, 1, 2), (0x03F4, 0x03F4, -60, 1), (0x03F7, 0x03F7, 1, 1), (0x03F9, 0x03F9, -7, 1),
    (0x03FA, 0x03FA, 1, 1), (0x03FD, 0x03FF, -130, 1), (0x0400, 0x040F, 80, 1), (0x0410, 0x042F, 32, 1),
    (0x0460, 0x0480, 1, 2), (0x048A, 0x04BE, 1, 2), (0x04C0, 0x04C0, 15, 1), (0x04C1, 0x04CD, 1, 2),
    (0x04D0, 0x052E, 1, 2), (0x0531, 0x0556, 48, 1), (0x10A0, 0x10C5, 7264, 1), (0x10C7, 0x10C7, 7264, 1),
    (0x10CD, 0x10CD, 7264, 1), (0x13A0, 0x13EF, 38864, 1), (0x13F0, 0x13F5, 8, 1), (0x1C89, 0x1C89, 1, 1),
    (0x1C90, 0x1CBA, -3008, 1), (0x1CBD, 0x1CBF, -3008, 1), (0x1E00, 0x1E94, 1, 2), (0x1E9E, 0x1E9E, -7615, 1),
    (0x1EA0, 0x1EFE, 1, 2), (0x1F08, 0x1F0F, -8, 1), (0x1F18, 0x1F1D, -8, 1), (0x1F28, 0x1F2F, -8, 1),
    (0x1F38, 0x1F3F, -8, 1), (0x1F48, 0x1F4D, -8, 1), (0x1F59, 0x1F5F, -8, 2), (0x1F68, 0x1F6F, -8, 1),
    (0x1F88, 0x1F8F, -8, 1), (0x1F98, 0x1F9F, -8, 1), (0x1FA8, 0x1FAF, -8, 1), (0x1FB8, 0x1FB9, -8, 1),
    (0x1FBA, 0x1FBB, -74, 1), (0x1FBC, 0x1FBC, -9, 1), (0x1FC8, 0x1FCB, -86, 1), (0x1FCC, 0x1FCC, -9, 1),
    (0x1FD8, 0x1FD9, -8, 1), (0x1FDA, 0x1FDB, -100, 1), (0x1FE8, 0x1FE9, -8, 1), (0x1FEA, 0x1FEB, -112, 1),
    (0x1FEC, 0x1FEC, -7, 1), (0x1FF8, 0x1FF9, -128, 1), (0x1FFA, 0x1FFB, -126, 1), (0x1FFC, 0x1FFC, -9, 1),
    (0x2126, 0x2126, -7517, 1), (0x212A, 0x212A, -8383, 1), (0x212B, 0x212B, -8262, 1), (0x2132, 0x2132, 28, 1),
    (0x2160, 0x216F, 16, 1), (0x2183, 0x2183, 1, 1), (0x24B6, 0x24CF, 26, 1), (0x2C00, 0x2C2F, 48, 1),
    (0x2C60, 0x2C60, 1, 1), (0x2C62, 0x2C62, -10743, 1), (0x2C63, 0x2C63, -3814, 1), (0x2C64, 0x2C64, -10727, 1),
    (0x2C67, 0x2C6B, 1, 2), (0x2C6D, 0x2C6D, -10780, 1), (0x2C6E, 0x2C6E, -10749, 1), (0x2C6F, 0x2C6F, -10783, 1),
    (0x2C70, 0x2C70, -10782, 1), (0x2C72, 0x2C72, 1, 1), (0x2C75, 0x2C75, 1, 1), (0x2C7E, 0x2C7F, -10815, 1),
    (0x2C80, 0x2CE2, 1, 2), (0x2CEB, 0x2CED, 1, 2), (0x2CF2, 0x2CF2, 1, 1), (0xA640, 0xA66C, 1, 2),
    (0xA680, 0xA69A, 1, 2), (0xA722, 0xA72E, 1, 2), (0xA732, 0xA76E, 1, 2), (0xA779, 0xA77B, 1, 2),
    (0xA77D, 0xA77D, -35332, 1), (0xA77E, 0xA786, 1, 2), (0xA78B, 0xA78B, 1, 1), (0xA78D, 0xA78D, -42280, 1),
    (0xA790, 0xA792, 1, 2), (0xA796, 0xA7A8, 1, 2), (0xA7AA, 0xA7AA, -42308, 1), (0xA7AB, 0xA7AB, -42319, 1),
    (0xA7AC, 0xA7AC, -42315, 1), (0xA7AD, 0xA7AD, -42305, 1), (0xA7AE, 0xA7AE, -42308, 1), (0xA7B0, 0xA7B0, -42258, 1),
    (0xA7B1, 0xA7B1, -42282, 1), (0xA7B2, 0xA7B2, -42261, 1), (0xA7B3, 0xA7B3, 928, 1), (0xA7B4, 0xA7C2, 1, 2),
    (0xA7C4, 0xA7C4, -48, 1), (0xA7C5, 0xA7C5, -42307, 1), (0xA7C6, 0xA7C6, -35384, 1), (0xA7C7, 0xA7C9, 1, 2),
    (0xA7CB, 0xA7CB, -42343, 1), (0xA7CC, 0xA7DA, 1, 2), (0xA7DC, 0xA7DC, -42561, 1), (0xA7F5, 0xA7F5, 1, 1),
    (0xFF21, 0xFF3A, 32, 1), (0x10400, 0x10427, 40, 1), (0x104B0, 0x104D3, 40, 1), (0x10570, 0x1057A, 39, 1),
    (0x1057C, 0x1058A, 39, 1), (0x1058C, 0x10592, 39, 1), (0x10594, 0x10595, 39, 1), (0x10C80, 0x10CB2, 64, 1),
    (0x10D50, 0x10D65, 32, 1), (0x118A0, 0x118BF, 32, 1), (0x16E40, 0x16E5F, 32, 1), (0x16EA0, 0x16EB8, 27, 1),
    (0x1E900, 0x1E921, 34, 1),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_counts_utf16_code_units() {
        assert_eq!(size(""), 0);
        assert_eq!(size("A"), 1);
        assert_eq!(size("é"), 1);
        assert_eq!(size("€"), 1);
        assert_eq!(size("😀"), 2);
    }

    #[test]
    fn lower_case_has_no_context() {
        assert_eq!(to_lower("PAGE_UP"), "page_up");
        assert_eq!(to_lower("ΑΣ ΣΑΣ"), "ασ σασ");
        assert_eq!(to_lower("İ"), "i\u{307}");
        assert_eq!(to_lower("ẞ"), "ß");
        assert_eq!(to_lower("Ǆ ǅ"), "ǆ ǆ");
        assert_eq!(to_lower("𐐀"), "𐐨"); // Deseret, outside the BMP
        assert_eq!(to_lower("ß\u{FEFF}1"), "ß\u{FEFF}1");
    }

    // Each checked against Qt 6.11.2 under LC_ALL=C.
    #[test]
    fn arg_replaces_the_lowest_placeholder() {
        let split = |id: &str, n: &str| arg(&arg("%1-s%2", id), n);
        assert_eq!(split("c1", "1"), "c1-s1");
        assert_eq!(split("a%1b", "5"), "a5b-s%2");
        assert_eq!(split("%2", "5"), "5-s5");
        assert_eq!(split("x%L0%%0y%10%3", "7"), "x7%7y%10%3-s%2");
        assert_eq!(split("p%L1q%1", "1234567"), "p1234567q1234567-s%2");
        assert_eq!(split("p%", "3"), "p%-s3");
        assert_eq!(split("p%L", "3"), "p%L-s3");
        assert_eq!(split("%02", "3"), "3-s3");
        assert_eq!(split("%\u{661}", "3"), "%\u{661}-s3");
        assert_eq!(split("%-1", "-4"), "%-1-s-4");
        assert_eq!(arg("Num %1", "%"), "Num %");
        assert_eq!(arg("no placeholder", "x"), "no placeholder");
        assert_eq!(arg("%%", "x"), "%%");
    }
}
