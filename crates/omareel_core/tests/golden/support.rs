//! Shared pieces of the golden tests: where the fixtures live, how to read the corpus, the cell
//! formats of `tests/golden/README.md` ("Formatting rules"), and a comparator that reports
//! where a regenerated file departs from its golden.

use omareel_core::compat::qcolor::{QColor, Spec};
use omareel_core::compat::qjson::{self, Format, Value};
use omareel_core::compat::qnumber;
use omareel_core::project::Clip;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/golden");

/// `tests/golden` without the `..` components, so paths built from it (and messages that
/// contain them) start with [`corpus_root`].
fn golden_root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| std::fs::canonicalize(GOLDEN).expect("tests/golden"))
}

/// The corpus directory, the oracle's `$CORPUS`.
pub fn corpus_root() -> PathBuf {
    golden_root().join("corpus")
}

pub fn corpus_path(relative: &str) -> PathBuf {
    corpus_root().join(relative)
}

pub fn expected_path(relative: &str) -> PathBuf {
    golden_root().join("expected").join(relative)
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

pub fn read_corpus(relative: &str) -> Vec<u8> {
    read(&corpus_path(relative))
}

pub fn read_expected(relative: &str) -> Vec<u8> {
    read(&expected_path(relative))
}

/// The oracle's `corpusLines`: the file split on LF, empty lines and `#` comments dropped.
pub fn corpus_lines(relative: &str) -> Vec<Vec<u8>> {
    read_corpus(relative)
        .split(|&b| b == b'\n')
        .filter(|line| !line.is_empty() && !line.starts_with(b"#"))
        .map(<[u8]>::to_vec)
        .collect()
}

/// The oracle's `readDoubles`: one IEEE-754 bit pattern per line as 16 hex digits.
pub fn read_doubles(relative: &str) -> Vec<f64> {
    corpus_lines(relative)
        .iter()
        .map(|line| {
            let text = std::str::from_utf8(line).expect("hex double").trim();
            assert_eq!(text.len(), 16, "bad hex double {text:?} in {relative}");
            f64::from_bits(u64::from_str_radix(text, 16).expect("hex double"))
        })
        .collect()
}

/// A corpus JSON document, read with the port's own parser (the oracle used Qt's).
pub fn read_json(relative: &str) -> Value {
    qjson::from_json(&read_corpus(relative)).unwrap_or_else(|e| panic!("corpus file {relative} is not JSON: {e}"))
}

/// The oracle's `sortedEntries`: the files (or directories) of a directory, hidden ones
/// included, in byte-wise order of their UTF-8 names.
pub fn sorted_entries(directory: &Path, directories: bool) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(directory)
        .unwrap_or_else(|e| panic!("cannot list {}: {e}", directory.display()))
        .map(|entry| entry.expect("directory entry"))
        .filter(|entry| entry.path().is_dir() == directories)
        .map(|entry| entry.file_name().into_string().expect("UTF-8 file name"))
        .collect();
    names.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    names
}

/// `nextafter(x, ±inf)` (`f64::next_up`/`next_down` need Rust 1.86; the workspace allows 1.85).
pub fn next_after(x: f64, toward: f64) -> f64 {
    if x.is_nan() || toward.is_nan() {
        return f64::NAN;
    }
    if x == toward {
        return toward;
    }
    if x == 0.0 {
        let tiny = f64::from_bits(1);
        return if toward > 0.0 { tiny } else { -tiny };
    }
    let away_from_zero = (toward > x) == (x > 0.0);
    let bits = x.to_bits();
    f64::from_bits(if away_from_zero { bits + 1 } else { bits - 1 })
}

/// Parses a **num** cell back into the double it printed (the format round-trips).
#[cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]
pub fn parse_num(cell: &str) -> f64 {
    cell.parse().unwrap_or_else(|_| panic!("bad num cell {cell:?}"))
}

// ---------------------------------------------------------------------------------------------
// Grids (README "Grids")

/// prev(x): the double just below `x`.
pub fn before(x: f64) -> f64 {
    next_after(x, f64::NEG_INFINITY)
}

/// next(x): the double just above `x`.
pub fn after(x: f64) -> f64 {
    next_after(x, f64::INFINITY)
}

/// `i / denominator` for `i = from..=to`, as the oracle's `uniform` divides (`i / double(d)`).
pub fn uniform(denominator: i32, from: i32, to: i32) -> Vec<f64> {
    (from..=to).map(|i| f64::from(i) / f64::from(denominator)).collect()
}

/// The oracle's `sortedUnique`: ascending, exact duplicates removed (`std::sort` then
/// `std::unique`). Grids hold no NaN at this point; the extra values are appended after.
pub fn sorted_unique(mut values: Vec<f64>) -> Vec<f64> {
    values.sort_by(|a, b| a.partial_cmp(b).expect("no NaN in a grid"));
    values.dedup();
    values
}

// ---------------------------------------------------------------------------------------------
// Cell formats

/// **num**: glibc's `printf("%.17g")`, with every NaN `nan` and the infinities `inf`/`-inf`.
///
/// Rust's `{:.16e}` gives the same 17 correctly rounded significant digits (exact halves go to
/// the even digit, as glibc's do in the default rounding mode); `%g` then picks the layout from
/// that rounded exponent and drops trailing zeros.
pub fn num(value: f64) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf" } else { "inf" }.into();
    }
    let scientific = format!("{:.16e}", value.abs());
    let (mantissa, exponent) = scientific.split_once('e').expect("exponent");
    let exponent: i32 = exponent.parse().expect("exponent digits");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    debug_assert_eq!(digits.len(), 17);
    let mut out = String::new();
    if value.is_sign_negative() {
        out.push('-');
    }
    let strip = |text: String| -> String {
        if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_owned() } else { text }
    };
    if !(-4..17).contains(&exponent) {
        let mantissa = strip(format!("{}.{}", &digits[..1], &digits[1..]));
        let sign = if exponent < 0 { '-' } else { '+' };
        let _ = write!(out, "{mantissa}e{sign}{:02}", exponent.abs());
    } else if exponent >= 0 {
        let point = exponent as usize + 1;
        out += &strip(format!("{}.{}", &digits[..point], &digits[point..]));
    } else {
        let zeros = "0".repeat((-exponent - 1) as usize);
        out += &strip(format!("0.{zeros}{digits}"));
    }
    out
}

/// **hex**: the bit pattern of a double as 16 lowercase hex digits.
pub fn hex(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

/// **bool**.
pub fn boolean(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

/// **str**: a string in quotes with `"` and `\` escaped and the invisible or ambiguous code
/// points as `\u{X}`. (A Rust string cannot hold the unpaired surrogates the format also
/// escapes.)
pub fn quote(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        let code = u32::from(c);
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ if code < 0x20
                || code == 0x7f
                || (0x80..=0x9f).contains(&code)
                || matches!(code, 0x2028 | 0x2029 | 0xfeff | 0xfffe | 0xffff) =>
            {
                let _ = write!(out, "\\u{{{code:x}}}");
            }
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// **bytes**: printable ASCII stays (with `"` and `\` escaped), every other byte is `\xNN`.
pub fn quote_bytes(value: &[u8]) -> String {
    let mut out = String::from("\"");
    for &byte in value {
        match byte {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            0x20..=0x7e => out.push(char::from(byte)),
            _ => {
                let _ = write!(out, "\\x{byte:02x}");
            }
        }
    }
    out.push('"');
    out
}

/// **strlist**.
pub fn quote_list<S: AsRef<str>>(values: &[S]) -> String {
    let items: Vec<String> = values.iter().map(|v| quote(v.as_ref())).collect();
    format!("[{}]", items.join(","))
}

/// **clips**: `[<id> <in> <out> <speed> <layout> <media> <fit>;...]`.
pub fn clips_text(clips: &[Clip]) -> String {
    let items: Vec<String> = clips
        .iter()
        .map(|c| {
            [quote(&c.id), num(c.in_), num(c.out), num(c.speed), quote(&c.layout), quote(&c.media), quote(&c.fit)]
                .join(" ")
        })
        .collect();
    format!("[{}]", items.join(";"))
}

/// The `QVariant` types the composition family prints (README "composition").
pub enum Variant {
    Double(f64),
    Int(i32),
    Bool(bool),
    String(String),
    StringList(Vec<String>),
    /// A `QVariantMap`; [`variant`] puts the keys in its order.
    Map(Vec<(String, Variant)>),
    List(Vec<Variant>),
}

/// **variant**: a double as num, an int as `int(<n>)`, a map as `{key=value,...}` with the keys
/// in `QVariantMap` order (ascending UTF-16 code units, raw UTF-8), a list as `[...]`.
pub fn variant(value: &Variant) -> String {
    match value {
        Variant::Double(d) => num(*d),
        Variant::Int(n) => format!("int({n})"),
        Variant::Bool(b) => boolean(*b).into(),
        Variant::String(s) => quote(s),
        Variant::StringList(list) => quote_list(list),
        Variant::Map(entries) => {
            let mut entries: Vec<&(String, Variant)> = entries.iter().collect();
            entries.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
            let items: Vec<String> = entries.iter().map(|(key, value)| format!("{key}={}", variant(value))).collect();
            format!("{{{}}}", items.join(","))
        }
        Variant::List(items) => format!("[{}]", items.iter().map(variant).collect::<Vec<_>>().join(",")),
    }
}

pub fn spec_name(spec: Spec) -> &'static str {
    match spec {
        Spec::Invalid => "Invalid",
        Spec::Rgb => "Rgb",
        Spec::ExtendedRgb => "ExtendedRgb",
    }
}

/// `rgba64()` as four 4-digit hex numbers, red green blue alpha.
pub fn rgba64_hex(color: &QColor) -> String {
    let (r, g, b, a) = color.rgba64();
    format!("{r:04x}{g:04x}{b:04x}{a:04x}")
}

/// `redF() greenF() blueF() alphaF()` as four **num** joined by spaces.
pub fn color_floats(color: &QColor) -> String {
    [color.red_f(), color.green_f(), color.blue_f(), color.alpha_f()].map(|f| num(f64::from(f))).join(" ")
}

/// **color**: `<spec> <rgba64> <#aarrggbb>`, plus the four float channels for `ExtendedRgb`.
pub fn color(color: &QColor) -> String {
    let mut out = format!("{} {} {}", spec_name(color.spec()), rgba64_hex(color), color.name_argb());
    if color.spec() == Spec::ExtendedRgb {
        out.push(' ');
        out += &color_floats(color);
    }
    out
}

/// **json**: `toJson(Compact)` of a document.
pub fn json(value: &Value) -> String {
    value.to_json(Format::Compact)
}

/// **wrapped**: a single value as the compact JSON of a one-element array.
pub fn wrapped(value: &Value) -> String {
    json(&Value::Array(vec![value.clone()]))
}

/// The `QJsonValue::Type` name; Qt has one `Double` type for integers and doubles.
pub fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Undefined => "Undefined",
        Value::Null => "Null",
        Value::Bool(_) => "Bool",
        Value::Int(_) | Value::Double(_) => "Double",
        Value::String(_) => "String",
        Value::Array(_) => "Array",
        Value::Object(_) => "Object",
    }
}

/// `toVariant().typeName()`, or `<invalid>` for the variant of `Undefined`.
pub fn variant_type_name(value: &Value) -> &'static str {
    match value {
        Value::Undefined => "<invalid>",
        Value::Null => "std::nullptr_t",
        Value::Bool(_) => "bool",
        Value::Int(_) => "qlonglong",
        Value::Double(_) => "double",
        Value::String(_) => "QString",
        Value::Array(_) => "QVariantList",
        Value::Object(_) => "QVariantMap",
    }
}

/// `toVariant().toString()`: a double prints as `QString::number(d, 'g',
/// QLocale::FloatingPointShortest)`; null, containers and the invalid variant are empty.
pub fn variant_to_string(value: &Value) -> String {
    match value {
        Value::Bool(b) => boolean(*b).into(),
        Value::Int(n) => qnumber::number_i64(*n),
        Value::Double(d) => qnumber::number_f64(*d, 'g', qnumber::FLOATING_POINT_SHORTEST),
        Value::String(s) => s.clone(),
        Value::Undefined | Value::Null | Value::Array(_) | Value::Object(_) => String::new(),
    }
}

/// **typed**: the nine cells describing a `QJsonValue`.
pub fn typed(value: &Value) -> String {
    [
        json_type_name(value).to_owned(),
        value.to_int(-7).to_string(),
        value.to_integer(-7).to_string(),
        num(value.to_double(-7.0)),
        boolean(value.to_bool(false)).to_owned(),
        quote(&value.to_string_or("<def>")),
        variant_type_name(value).to_owned(),
        quote(&variant_to_string(value)),
        value.to_variant_long_long().to_string(),
    ]
    .join("\t")
}

/// **path** for the families that only see the corpus directory: it is written `$CORPUS`.
/// (The hermetic directories are put back by `Hermetic::sanitize`.)
pub fn sanitize(text: &str) -> String {
    let corpus = corpus_root();
    let root = corpus.to_str().expect("UTF-8 corpus path");
    sanitize_with(text, &[("$CORPUS".to_owned(), root.to_owned())])
}

/// **path** with the given `(name, path)` roots, tried in order (longest first, as the oracle
/// sorts them): an occurrence of a path becomes its name unless the character after it is a
/// letter, digit, `.`, `_` or `-`.
pub fn sanitize_with(text: &str, roots: &[(String, String)]) -> String {
    let path_character = |c: char| c.is_alphanumeric() || matches!(c, '.' | '_' | '-');
    let mut out = String::new();
    let mut i = 0;
    'scan: while let Some(c) = text[i..].chars().next() {
        let rest = &text[i..];
        for (name, path) in roots {
            if !path.is_empty()
                && rest.starts_with(path.as_str())
                && !rest[path.len()..].chars().next().is_some_and(path_character)
            {
                out += name;
                i += path.len();
                continue 'scan;
            }
        }
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// Qt keeps an unpaired surrogate that a `\ud800`-style escape produced; a Rust string
/// cannot hold one, so the port's parser makes it U+FFFD (documented in `compat::qjson`). The
/// goldens show such a surrogate as `\u{d800}` in **str** cells and as `\ud800` in Qt's JSON,
/// and this maps both to U+FFFD so everything around them is still compared byte for byte.
/// Both formats double every literal backslash, so an escape is only one when an even number
/// of backslashes precedes it; **bytes** cells never contain one.
pub fn lone_surrogates_as_replacement(text: &[u8]) -> Vec<u8> {
    let surrogate = |hex: &[u8]| {
        !hex.is_empty()
            && hex.iter().all(u8::is_ascii_hexdigit)
            && std::str::from_utf8(hex)
                .ok()
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .is_some_and(|code| (0xd800..=0xdfff).contains(&code))
    };
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if text[i] != b'\\' {
            out.push(text[i]);
            i += 1;
            continue;
        }
        let rest = &text[i + 1..];
        if rest.first() == Some(&b'\\') {
            out.extend_from_slice(b"\\\\");
            i += 2;
        } else if let Some(braced) = rest.strip_prefix(b"u{") {
            match braced.iter().position(|&b| b == b'}') {
                Some(end) if surrogate(&braced[..end]) => {
                    out.extend_from_slice("\u{fffd}".as_bytes());
                    i += 1 + 2 + end + 1;
                }
                _ => {
                    out.push(b'\\');
                    i += 1;
                }
            }
        } else if rest.len() >= 5 && rest[0] == b'u' && surrogate(&rest[1..5]) {
            out.extend_from_slice("\u{fffd}".as_bytes());
            i += 6;
        } else {
            out.push(b'\\');
            i += 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Output and comparison

/// An output file assembled in memory, like the oracle's `Text`.
#[derive(Default)]
pub struct Text(String);

impl Text {
    pub fn new() -> Text {
        Text::default()
    }

    /// Cells joined by tabs, then a newline.
    pub fn row<S: AsRef<str>>(&mut self, cells: impl IntoIterator<Item = S>) {
        let mut first = true;
        for cell in cells {
            if !first {
                self.0.push('\t');
            }
            first = false;
            self.0.push_str(cell.as_ref());
        }
        self.0.push('\n');
    }

    /// A `== <label>` line.
    pub fn section(&mut self, label: &str) {
        self.0.push_str("== ");
        self.0.push_str(label);
        self.0.push('\n');
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

/// The lines of `text`, each without its `\n`; a final newline does not start an empty line.
fn lines(text: &[u8]) -> Vec<&[u8]> {
    let mut lines: Vec<&[u8]> = text.split(|&b| b == b'\n').collect();
    if text.ends_with(b"\n") || text.is_empty() {
        lines.pop();
    }
    lines
}

fn shown(line: Option<&[u8]>) -> String {
    match line {
        None => "<no line>".into(),
        Some(line) => {
            let text = String::from_utf8_lossy(line);
            let mut shown = format!("{text:?}");
            if shown.len() > 400 {
                let mut end = 400;
                while !shown.is_char_boundary(end) {
                    end -= 1;
                }
                shown.truncate(end);
                shown.push_str("...");
            }
            shown
        }
    }
}

/// Compares regenerated bytes with a golden: `None` when identical, otherwise a report of the
/// first differing lines (1-based line numbers) and how many differ in total.
pub fn difference(name: &str, expected: &[u8], actual: &[u8]) -> Option<String> {
    if expected == actual {
        return None;
    }
    let (want, got) = (lines(expected), lines(actual));
    let total = want.len().max(got.len());
    let differing: Vec<usize> = (0..total).filter(|&i| want.get(i) != got.get(i)).collect();
    let mut report = format!(
        "{name}: {} of {total} lines differ (golden has {} lines, Rust wrote {})",
        differing.len(),
        want.len(),
        got.len()
    );
    if differing.is_empty() {
        // Same lines, so only the final newline differs.
        report += &format!(
            "; golden ends with a newline: {}, Rust output does: {}",
            expected.ends_with(b"\n"),
            actual.ends_with(b"\n")
        );
    }
    for &i in differing.iter().take(10) {
        let _ = write!(
            report,
            "\n  line {}:\n    golden: {}\n    rust:   {}",
            i + 1,
            shown(want.get(i).copied()),
            shown(got.get(i).copied())
        );
    }
    if differing.len() > 10 {
        let _ = write!(report, "\n  ... and {} more", differing.len() - 10);
    }
    Some(report)
}

#[track_caller]
pub fn assert_same(name: &str, expected: &[u8], actual: &[u8]) {
    if let Some(report) = difference(name, expected, actual) {
        panic!("{report}");
    }
}

/// Whether the tests were started with `OMAREEL_GOLDEN_BLESS=1`. Read from the environment the
/// process started with: the hermetic tests replace the live one.
fn blessing() -> bool {
    std::fs::read("/proc/self/environ")
        .is_ok_and(|environ| environ.split(|&b| b == 0).any(|entry| entry == b"OMAREEL_GOLDEN_BLESS=1"))
}

/// Compares a whole regenerated file with `expected/<relative>`. With `OMAREEL_GOLDEN_BLESS=1`
/// the file is rewritten instead, for a change that means to alter it (review the diff).
#[track_caller]
pub fn assert_golden(relative: &str, actual: &str) {
    if blessing() {
        std::fs::write(expected_path(relative), actual).expect("the golden file is writable");
        return;
    }
    assert_same(relative, &read_expected(relative), actual.as_bytes());
}

/// Fails with every report collected by a test that compares several files.
#[track_caller]
pub fn assert_no_differences(reports: Vec<String>) {
    if !reports.is_empty() {
        panic!("{} file(s) differ:\n{}", reports.len(), reports.join("\n\n"));
    }
}

/// A file cut into its `== ` sections: the label (`None` for lines before the first section)
/// and the section's bytes, header line included.
pub fn sections(text: &[u8]) -> Vec<(Option<String>, Vec<u8>)> {
    let mut result: Vec<(Option<String>, Vec<u8>)> = Vec::new();
    for line in text.split_inclusive(|&b| b == b'\n') {
        if let Some(label) = line.strip_prefix(b"== ") {
            let label = String::from_utf8_lossy(label.strip_suffix(b"\n").unwrap_or(label));
            result.push((Some(label.into_owned()), Vec::new()));
        } else if result.is_empty() {
            result.push((None, Vec::new()));
        }
        result.last_mut().expect("a section").1.extend_from_slice(line);
    }
    result
}

/// The sections of `text` whose label passes `keep` (lines before the first section always
/// stay), joined back together.
pub fn keep_sections(text: &[u8], keep: impl Fn(&str) -> bool) -> Vec<u8> {
    sections(text)
        .into_iter()
        .filter(|(label, _)| label.as_deref().is_none_or(&keep))
        .flat_map(|(_, bytes)| bytes)
        .collect()
}

/// The lines of `text` that pass `keep`, each with its newline.
pub fn keep_lines(text: &[u8], keep: impl Fn(&[u8]) -> bool) -> Vec<u8> {
    text.split_inclusive(|&b| b == b'\n')
        .filter(|line| keep(line.strip_suffix(b"\n").unwrap_or(line)))
        .flatten()
        .copied()
        .collect()
}

/// The cells of a golden row.
pub fn cells(line: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(line).split('\t').map(str::to_owned).collect()
}

/// The rows of a golden file as cells.
pub fn rows(text: &[u8]) -> Vec<Vec<String>> {
    lines(text).into_iter().map(cells).collect()
}

#[test]
#[allow(clippy::excessive_precision)]
fn num_prints_like_glibc() {
    let cases: &[(f64, &str)] = &[
        (0.1, "0.10000000000000001"),
        (1.0, "1"),
        (1e16, "10000000000000000"),
        (1e17, "1e+17"),
        (1e-5, "1.0000000000000001e-05"),
        (0.0001, "0.0001"),
        (5e-324, "4.9406564584124654e-324"),
        (-0.0, "-0"),
        (-2.5, "-2.5"),
        (1e300, "1.0000000000000001e+300"),
        // Exactly halfway at the 17th digit: glibc keeps the even digit. The literals are
        // exact doubles, written out in full on purpose.
        (1000000000000000.25, "1000000000000000.2"),
        (1000000000000000.75, "1000000000000000.8"),
        (f64::NAN, "nan"),
        (-f64::NAN, "nan"),
        (f64::INFINITY, "inf"),
        (f64::NEG_INFINITY, "-inf"),
    ];
    for &(value, text) in cases {
        assert_eq!(num(value), text, "{:016x}", value.to_bits());
    }
}

/// The variant cells of `composition/editor.txt` (which `composition_editor.rs` regenerates
/// whole on x86_64), rebuilt by hand with map keys out of order on every architecture.
#[test]
fn variant_prints_like_the_oracle() {
    use Variant::*;
    let map = |entries: Vec<(&str, Variant)>| Map(entries.into_iter().map(|(k, v)| (k.to_owned(), v)).collect());
    let golden = rows(&read_expected("composition/editor.txt"));
    let first = golden.iter().find(|row| row.len() > 1).expect("a row");
    let cells = [
        map(vec![("velocity", Double(0.0)), ("scale", Double(1.0)), ("cy", Double(0.5)), ("cx", Double(0.5))]),
        map(vec![("cameraFull", Double(1.0)), ("bubble", Double(0.0))]),
        map(vec![
            ("width", Int(16)),
            ("ready", Bool(true)),
            ("missing", Bool(false)),
            ("height", Int(9)),
            ("fit", String("fit".into())),
            ("active", Bool(false)),
        ]),
    ];
    for (i, cell) in cells.iter().enumerate() {
        assert_eq!(variant(cell), first[2 + i]);
    }
    assert_eq!(variant(&List(Vec::new())), first[6]);
    let keys = ["Ctrl", "S"];
    let pills = List(vec![map(vec![
        ("text", String(keys.join(" + "))),
        ("opacity", Double(1.0)),
        ("keys", StringList(keys.iter().map(|k| (*k).to_owned()).collect())),
    ])]);
    assert!(golden.iter().any(|row| row.get(7) == Some(&variant(&pills))));
    // Keys sort by UTF-16 code units: U+1F600 (0xD83D 0xDE00) before U+FF5E, unlike their UTF-8.
    assert_eq!(variant(&map(vec![("\u{ff5e}", Int(2)), ("\u{1f600}", Int(1))])), "{\u{1f600}=int(1),\u{ff5e}=int(2)}");
}
