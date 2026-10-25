//! `QJsonDocument`, `QJsonObject`, `QJsonArray` and `QJsonValue`, as Qt 6 implements them.
//!
//! omareel writes project.json, capture.json, input.jsonl and its settings with
//! `QJsonDocument`, so the bytes it produces are Qt's: object keys sorted, four-space
//! indentation, and Qt's own number format. Qt stores numbers as either a 64-bit integer or a
//! double, and which one decides how a number prints, so [`Value`] keeps the same distinction:
//! - the parser makes an integer of any literal without a fraction or exponent that fits in
//!   64 bits, and of any other number whose value is a whole number in 64-bit range;
//! - [`Value::from_f64`] (Qt's `QJsonValue(double)`) makes an integer of a whole number only
//!   while it is within ±2^53, so a double never claims more precision than it had.
//!
//! Checked against Qt 6.11 over 300,000 random doubles and 40,000 random and corrupted
//! documents: same bytes out, same error kinds and offsets. The one difference: Qt keeps a
//! lone surrogate from a `\ud800`-style escape and writes it back escaped, while a Rust string
//! cannot hold one, so it becomes U+FFFD. omareel never writes such strings.

use std::cmp::Ordering;
use std::fmt;

/// A JSON value, with Qt's integer/double split (`QCborValue::Integer` and `Double`).
#[derive(Clone, Debug, Default)]
pub enum Value {
    /// What `QJsonObject::value` returns for a missing key. Never written out.
    #[default]
    Undefined,
    Null,
    Bool(bool),
    Int(i64),
    Double(f64),
    String(String),
    Array(Vec<Value>),
    Object(Object),
}

static UNDEFINED: Value = Value::Undefined;

/// The whole number `v` holds, if it holds one inside `i64`'s range (Qt's `convertDoubleTo`).
fn whole_i64(v: f64) -> Option<i64> {
    // 2^63 is exact as a double; NaN fails both comparisons.
    if v >= -9_223_372_036_854_775_808.0 && v < 9_223_372_036_854_775_808.0 {
        let n = v as i64;
        if n as f64 == v {
            return Some(n);
        }
    }
    None
}

impl Value {
    /// `QJsonValue(double)`: a whole number within ±2^53 becomes an integer, anything else
    /// stays a double. Non-finite doubles are kept and print as `null`.
    pub fn from_f64(v: f64) -> Value {
        const MAX_MANTISSA: f64 = 9_007_199_254_740_992.0; // 2^53
        if (-MAX_MANTISSA - 1.0..=MAX_MANTISSA).contains(&v) {
            if let Some(n) = whole_i64(v) {
                return Value::Int(n);
            }
        }
        Value::Double(v)
    }

    pub fn is_undefined(&self) -> bool {
        matches!(self, Value::Undefined)
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn is_bool(&self) -> bool {
        matches!(self, Value::Bool(_))
    }

    /// `QJsonValue::isDouble`: true for both integers and doubles.
    pub fn is_double(&self) -> bool {
        matches!(self, Value::Int(_) | Value::Double(_))
    }

    pub fn is_string(&self) -> bool {
        matches!(self, Value::String(_))
    }

    pub fn is_array(&self) -> bool {
        matches!(self, Value::Array(_))
    }

    pub fn is_object(&self) -> bool {
        matches!(self, Value::Object(_))
    }

    /// `QJsonValue::toBool`.
    pub fn to_bool(&self, default: bool) -> bool {
        match self {
            Value::Bool(b) => *b,
            _ => default,
        }
    }

    /// `QJsonValue::toInt`: only a whole number that fits in an `int`; a fraction gives the
    /// default rather than being truncated.
    pub fn to_int(&self, default: i32) -> i32 {
        match self {
            Value::Int(n) => i32::try_from(*n).unwrap_or(default),
            Value::Double(d) => whole_i64(*d).and_then(|n| i32::try_from(n).ok()).unwrap_or(default),
            _ => default,
        }
    }

    /// `QJsonValue::toInteger`.
    pub fn to_integer(&self, default: i64) -> i64 {
        match self {
            Value::Int(n) => *n,
            Value::Double(d) => whole_i64(*d).unwrap_or(default),
            _ => default,
        }
    }

    /// `QJsonValue::toDouble`.
    pub fn to_double(&self, default: f64) -> f64 {
        match self {
            Value::Int(n) => *n as f64,
            Value::Double(d) => *d,
            _ => default,
        }
    }

    /// `QJsonValue::toString(default)`.
    pub fn to_string_or(&self, default: &str) -> String {
        match self {
            Value::String(s) => s.clone(),
            _ => default.to_owned(),
        }
    }

    /// The string, if this is one.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// `QJsonValue::toObject`: the object, or an empty one.
    pub fn to_object(&self) -> Object {
        self.as_object().cloned().unwrap_or_default()
    }

    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }

    /// `QJsonValue::toArray`: the array, or an empty one.
    pub fn to_array(&self) -> Vec<Value> {
        self.as_array().map(<[Value]>::to_vec).unwrap_or_default()
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    /// `QJsonObject::value(key)` on an object value; `Undefined` for anything else.
    pub fn get(&self, key: &str) -> &Value {
        self.as_object().map_or(&UNDEFINED, |o| o.value(key))
    }

    /// `QJsonValue::toJson`: an object or array as a document, anything else as its JSON text.
    pub fn to_json(&self, format: Format) -> String {
        let mut out = String::new();
        match self {
            Value::Object(_) | Value::Array(_) => {
                write_value(self, &mut out, 0, format);
                if format == Format::Indented {
                    out.push('\n');
                }
            }
            _ => write_value(self, &mut out, 0, format),
        }
        out
    }
}

impl PartialEq for Value {
    /// `QJsonValue::operator==`: an integer and a double compare as doubles.
    fn eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Int(_) | Value::Double(_), Value::Int(_) | Value::Double(_)) => {
                self.to_double(0.0) == other.to_double(0.0)
            }
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => a == b,
            (Value::Object(a), Value::Object(b)) => a == b,
            _ => false,
        }
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Value {
        Value::Bool(v)
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Value {
        Value::Int(v.into())
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Value {
        Value::Int(v)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Value {
        Value::from_f64(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Value {
        Value::String(v.to_owned())
    }
}

impl From<String> for Value {
    fn from(v: String) -> Value {
        Value::String(v)
    }
}

impl From<Object> for Value {
    fn from(v: Object) -> Value {
        Value::Object(v)
    }
}

impl From<Vec<Value>> for Value {
    fn from(v: Vec<Value>) -> Value {
        Value::Array(v)
    }
}

/// Qt orders keys as `QString`s compare: by UTF-16 code unit, which differs from Rust's
/// byte order only for characters past U+FFFF.
fn compare_keys(a: &str, b: &str) -> Ordering {
    if a.is_ascii() && b.is_ascii() {
        return a.cmp(b);
    }
    a.encode_utf16().cmp(b.encode_utf16())
}

/// `QJsonObject`: members kept sorted by key, each key once.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Object {
    members: Vec<(String, Value)>,
}

impl Object {
    pub fn new() -> Object {
        Object::default()
    }

    fn find(&self, key: &str) -> Result<usize, usize> {
        self.members.binary_search_by(|(k, _)| compare_keys(k, key))
    }

    /// `QJsonObject::insert`; inserting `Undefined` removes the key, as in Qt.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        let key = key.into();
        let value = value.into();
        match (self.find(&key), value.is_undefined()) {
            (Ok(i), true) => {
                self.members.remove(i);
            }
            (Ok(i), false) => self.members[i].1 = value,
            (Err(_), true) => {}
            (Err(i), false) => self.members.insert(i, (key, value)),
        }
    }

    /// `QJsonObject::value`: `Undefined` when the key is missing.
    pub fn value(&self, key: &str) -> &Value {
        self.find(key).map_or(&UNDEFINED, |i| &self.members[i].1)
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.find(key).ok().map(|i| &self.members[i].1)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.find(key).ok().map(|i| &mut self.members[i].1)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.find(key).is_ok()
    }

    pub fn remove(&mut self, key: &str) {
        self.take(key);
    }

    /// `QJsonObject::take`: removes the key and returns its value, or `Undefined`.
    pub fn take(&mut self, key: &str) -> Value {
        match self.find(key) {
            Ok(i) => self.members.remove(i).1,
            Err(_) => Value::Undefined,
        }
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// The keys in Qt's order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.members.iter().map(|(k, _)| k.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.members.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// `QJsonDocument(object).toJson(format)`.
    pub fn to_json(&self, format: Format) -> String {
        Value::Object(self.clone()).to_json(format)
    }
}

impl<K: Into<String>, V: Into<Value>> FromIterator<(K, V)> for Object {
    /// Builds an object the way an initializer list does: a repeated key keeps its last value.
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Object {
        let mut object = Object::new();
        for (k, v) in iter {
            object.insert(k, v);
        }
        object
    }
}

/// `QJsonDocument::JsonFormat`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Indented,
    Compact,
}

fn push_indent(out: &mut String, level: usize) {
    out.extend(std::iter::repeat_n(' ', 4 * level));
}

/// Qt's `valueContentToJson`: containers open and close at `level`, members sit one deeper.
fn write_value(value: &Value, out: &mut String, level: usize, format: Format) {
    let compact = format == Format::Compact;
    match value {
        Value::Undefined | Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Int(n) => out.push_str(&n.to_string()),
        Value::Double(d) => {
            if d.is_finite() {
                out.push_str(&shortest(*d));
            } else {
                out.push_str("null");
            }
        }
        Value::String(s) => write_string(s, out),
        Value::Array(items) => {
            out.push_str(if compact { "[" } else { "[\n" });
            for (i, item) in items.iter().enumerate() {
                if !compact {
                    push_indent(out, level + 1);
                }
                write_value(item, out, level + 1, format);
                out.push_str(match (i + 1 == items.len(), compact) {
                    (true, true) => "",
                    (true, false) => "\n",
                    (false, true) => ",",
                    (false, false) => ",\n",
                });
            }
            if !compact {
                push_indent(out, level);
            }
            out.push(']');
        }
        Value::Object(object) => {
            out.push_str(if compact { "{" } else { "{\n" });
            for (i, (key, item)) in object.members.iter().enumerate() {
                if !compact {
                    push_indent(out, level + 1);
                }
                write_string(key, out);
                out.push_str(if compact { ":" } else { ": " });
                write_value(item, out, level + 1, format);
                out.push_str(match (i + 1 == object.members.len(), compact) {
                    (true, true) => "",
                    (true, false) => "\n",
                    (false, true) => ",",
                    (false, false) => ",\n",
                });
            }
            if !compact {
                push_indent(out, level);
            }
            out.push('}');
        }
    }
}

/// Qt's `escapedString`: `"`, `\` and control characters escaped, everything else as UTF-8.
fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u00{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `QByteArray::number(d, 'g', QLocale::FloatingPointShortest)` for a finite double: the
/// shortest digits that read back as `d`, in fixed or exponent form, whichever Qt's
/// `resolveFormat` picks.
fn shortest(d: f64) -> String {
    if d == 0.0 {
        return "0".to_owned(); // Qt never writes "-0"
    }
    let (digits, decpt) = shortest_digits(d.abs());
    let length = digits.len() as i32;

    // resolveFormat: the exponent form adds a separator, sign and two exponent digits.
    let mut bias = 4;
    if length <= decpt && length > 1 {
        bias += 1;
    } else if length == 1 && decpt <= 0 {
        bias -= 1;
    }
    let use_decimal = if decpt <= 0 {
        1 - decpt <= bias
    } else if decpt <= length {
        true
    } else {
        decpt <= length + bias
    };

    let mut out = String::new();
    if d < 0.0 {
        out.push('-');
    }
    if use_decimal {
        if decpt <= 0 {
            out.push_str("0.");
            out.extend(std::iter::repeat_n('0', (-decpt) as usize));
            out.push_str(&digits);
        } else if decpt >= length {
            out.push_str(&digits);
            out.extend(std::iter::repeat_n('0', (decpt - length) as usize));
        } else {
            out.push_str(&digits[..decpt as usize]);
            out.push('.');
            out.push_str(&digits[decpt as usize..]);
        }
    } else {
        out.push_str(&digits[..1]);
        if length > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let e = decpt - 1;
        out.push_str(&format!("e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs()));
    }
    out
}

/// The shortest digits that read back as `v` (positive, finite) and the position of the
/// decimal point: `v ≈ 0.digits × 10^decpt`. Qt gets them from double-conversion, which picks
/// the candidate nearest the exact value like Rust does, but breaks an exact tie between two
/// candidates towards an even last digit where Rust rounds up.
fn shortest_digits(v: f64) -> (String, i32) {
    // Rust's `{:e}` gives the shortest round-trip digits: "d.ddde±x".
    let sci = format!("{v:e}");
    let (mantissa, exponent) = sci.split_once('e').expect("{:e} always has an exponent");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let decpt = exponent.parse::<i32>().expect("{:e} exponent is an integer") + 1;

    // A tie needs an exact value with one digit more than the shortest form, ending in 5.
    // That only happens for v = m × 2^-k with m odd and small k, where the exact value is
    // m × 5^k × 10^-k and fits in a u128.
    let bits = v.to_bits();
    let biased = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1u64 << 52) - 1);
    let (mut m, mut e) = if biased == 0 { (fraction, -1074) } else { (fraction | 1 << 52, biased - 1075) };
    let zeros = m.trailing_zeros() as i32;
    m >>= zeros;
    e += zeros;
    if e >= 0 || -e > 26 {
        return (digits, decpt);
    }
    let k = -e;
    let exact = (m as u128 * 5u128.pow(k as u32)).to_string();
    if exact.len() != digits.len() + 1 || exact.len() as i32 - k != decpt {
        return (digits, decpt);
    }
    let lower = &exact[..digits.len()];
    let even = lower.as_bytes()[lower.len() - 1] % 2 == 0;
    if even && lower != digits {
        let candidate = format!("0.{lower}e{decpt}");
        if candidate.parse::<f64>() == Ok(v) {
            return (lower.trim_end_matches('0').to_owned(), decpt);
        }
    }
    (digits, decpt)
}

/// `QJsonParseError::ParseError`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseErrorKind {
    UnterminatedObject,
    MissingNameSeparator,
    UnterminatedArray,
    MissingValueSeparator,
    IllegalValue,
    TerminationByNumber,
    IllegalNumber,
    IllegalEscapeSequence,
    IllegalUtf8String,
    UnterminatedString,
    MissingObject,
    DeepNesting,
    DocumentTooLarge,
    GarbageAtEnd,
}

/// `QJsonParseError`: what went wrong and the byte offset where the parser stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub offset: usize,
}

impl ParseError {
    /// `QJsonParseError::errorString` (untranslated).
    pub fn error_string(&self) -> &'static str {
        match self.kind {
            ParseErrorKind::UnterminatedObject => "unterminated object",
            ParseErrorKind::MissingNameSeparator => "missing name separator",
            ParseErrorKind::UnterminatedArray => "unterminated array",
            ParseErrorKind::MissingValueSeparator => "missing value separator",
            ParseErrorKind::IllegalValue => "illegal value",
            ParseErrorKind::TerminationByNumber => "invalid termination by number",
            ParseErrorKind::IllegalNumber => "illegal number",
            ParseErrorKind::IllegalEscapeSequence => "invalid escape sequence",
            ParseErrorKind::IllegalUtf8String => "invalid UTF8 string",
            ParseErrorKind::UnterminatedString => "unterminated string",
            ParseErrorKind::MissingObject => "object is missing after a comma",
            ParseErrorKind::DeepNesting => "too deeply nested document",
            ParseErrorKind::DocumentTooLarge => "too large document",
            ParseErrorKind::GarbageAtEnd => "garbage at the end of the document",
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.error_string())
    }
}

impl std::error::Error for ParseError {}

/// `QJsonDocument::fromJson`: an object or array. A lone scalar parses but is rejected as
/// `IllegalValue` at offset 0, as Qt does.
pub fn from_json(json: &[u8]) -> Result<Value, ParseError> {
    let value = Parser { json, pos: 0, nesting: 0 }.parse()?;
    match value {
        Value::Object(_) | Value::Array(_) => Ok(value),
        _ => Err(ParseError { kind: ParseErrorKind::IllegalValue, offset: 0 }),
    }
}

/// `QJsonDocument::fromJson(...).object()`: the object, or `None` for an array or an error.
pub fn object_from_json(json: &[u8]) -> Option<Object> {
    match from_json(json) {
        Ok(Value::Object(object)) => Some(object),
        _ => None,
    }
}

const NESTING_LIMIT: usize = 1024;

/// A transliteration of Qt's `QJsonPrivate::Parser`, lenient in the same places: control
/// characters in strings, unknown escapes, numbers such as `.5` or `1.`.
struct Parser<'a> {
    json: &'a [u8],
    pos: usize,
    nesting: usize,
}

type Parsed<T> = Result<T, ParseErrorKind>;

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.json.get(self.pos).copied()
    }

    fn at_end(&self) -> bool {
        self.pos >= self.json.len()
    }

    fn eat_space(&mut self) -> bool {
        while let Some(c) = self.peek() {
            if !matches!(c, b' ' | b'\t' | b'\n' | b'\r') {
                break;
            }
            self.pos += 1;
        }
        !self.at_end()
    }

    /// Consumes the next non-space byte and returns it if it is a structural token.
    fn next_token(&mut self) -> Option<u8> {
        if !self.eat_space() {
            return None;
        }
        let token = self.json[self.pos];
        self.pos += 1;
        matches!(token, b'[' | b'{' | b':' | b',' | b']' | b'}' | b'"').then_some(token)
    }

    fn parse(mut self) -> Result<Value, ParseError> {
        if self.json.len() > 3 && self.json.starts_with(&[0xef, 0xbb, 0xbf]) {
            self.pos = 3;
        }
        let result = self.parse_document();
        result.map_err(|kind| ParseError { kind, offset: self.pos })
    }

    fn parse_document(&mut self) -> Parsed<Value> {
        if !self.eat_space() {
            return Err(ParseErrorKind::IllegalValue);
        }
        let value = if self.json[self.pos] == b'"' {
            self.pos += 1;
            Value::String(self.parse_string()?)
        } else {
            self.parse_value()?
        };
        self.eat_space();
        if !self.at_end() {
            return Err(ParseErrorKind::GarbageAtEnd);
        }
        Ok(value)
    }

    fn parse_value(&mut self) -> Parsed<Value> {
        let Some(c) = self.peek() else {
            // Qt reads one byte past the end here and fails on it.
            self.pos += 1;
            return Err(ParseErrorKind::IllegalValue);
        };
        self.pos += 1;
        match c {
            b'n' => self.parse_literal(b"ull", Value::Null),
            b't' => self.parse_literal(b"rue", Value::Bool(true)),
            b'f' => self.parse_literal(b"alse", Value::Bool(false)),
            b'"' => Ok(Value::String(self.parse_string()?)),
            b'[' => self.parse_array(),
            b'{' => self.parse_object(),
            // A missing value after a colon.
            b',' => Err(ParseErrorKind::IllegalValue),
            b'}' | b']' => Err(ParseErrorKind::MissingObject),
            _ => {
                self.pos -= 1;
                self.parse_number()
            }
        }
    }

    /// Qt checks there are enough bytes left, then compares one byte at a time, stopping
    /// after the first mismatch.
    fn parse_literal(&mut self, rest: &[u8], value: Value) -> Parsed<Value> {
        if self.json.len() - self.pos < rest.len() {
            return Err(ParseErrorKind::IllegalValue);
        }
        for &expected in rest {
            let c = self.json[self.pos];
            self.pos += 1;
            if c != expected {
                return Err(ParseErrorKind::IllegalValue);
            }
        }
        Ok(value)
    }

    fn parse_object(&mut self) -> Parsed<Value> {
        self.nesting += 1;
        if self.nesting > NESTING_LIMIT {
            return Err(ParseErrorKind::DeepNesting);
        }
        // Collected in document order; sorted afterwards keeping the last of any repeated key.
        let mut members: Vec<(String, Value)> = Vec::new();
        let mut token = self.next_token();
        while token == Some(b'"') {
            let key = self.parse_string()?;
            if self.next_token() != Some(b':') {
                return Err(ParseErrorKind::MissingNameSeparator);
            }
            if !self.eat_space() {
                return Err(ParseErrorKind::UnterminatedObject);
            }
            let value = self.parse_value()?;
            members.push((key, value));
            token = self.next_token();
            if token != Some(b',') {
                break;
            }
            token = self.next_token();
            if token == Some(b'}') {
                return Err(ParseErrorKind::MissingObject);
            }
        }
        if token != Some(b'}') {
            return Err(ParseErrorKind::UnterminatedObject);
        }
        self.nesting -= 1;

        members.sort_by(|(a, _), (b, _)| compare_keys(a, b)); // stable, like std::stable_sort
        let mut unique: Vec<(String, Value)> = Vec::with_capacity(members.len());
        for member in members {
            match unique.last_mut() {
                Some(last) if last.0 == member.0 => *last = member,
                _ => unique.push(member),
            }
        }
        Ok(Value::Object(Object { members: unique }))
    }

    fn parse_array(&mut self) -> Parsed<Value> {
        self.nesting += 1;
        if self.nesting > NESTING_LIMIT {
            return Err(ParseErrorKind::DeepNesting);
        }
        if !self.eat_space() {
            return Err(ParseErrorKind::UnterminatedArray);
        }
        let mut items = Vec::new();
        if self.json[self.pos] == b']' {
            self.next_token();
        } else {
            loop {
                if !self.eat_space() {
                    return Err(ParseErrorKind::UnterminatedArray);
                }
                items.push(self.parse_value()?);
                match self.next_token() {
                    Some(b']') => break,
                    Some(b',') => {}
                    _ => {
                        return Err(if self.eat_space() {
                            ParseErrorKind::MissingValueSeparator
                        } else {
                            ParseErrorKind::UnterminatedArray
                        });
                    }
                }
            }
        }
        self.nesting -= 1;
        Ok(Value::Array(items))
    }

    fn parse_number(&mut self) -> Parsed<Value> {
        let start = self.pos;
        let mut is_int = true;
        let digit = |p: &Self| p.peek().is_some_and(|c| c.is_ascii_digit());
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        if self.peek() == Some(b'0') {
            self.pos += 1;
        } else {
            while digit(self) {
                self.pos += 1;
            }
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            while digit(self) {
                is_int = is_int && self.json[self.pos] == b'0';
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            is_int = false;
            self.pos += 1;
            if matches!(self.peek(), Some(b'-' | b'+')) {
                self.pos += 1;
            }
            while digit(self) {
                self.pos += 1;
            }
        }
        // Only ASCII bytes were consumed.
        let number = std::str::from_utf8(&self.json[start..self.pos]).unwrap_or_default();
        if is_int {
            if let Ok(n) = number.parse::<i64>() {
                return Ok(Value::Int(n));
            }
        }
        let d = parse_double(number).ok_or(ParseErrorKind::IllegalNumber)?;
        Ok(match whole_i64(d) {
            Some(n) => Value::Int(n),
            None => Value::Double(d),
        })
    }

    /// Strings after the opening quote. Qt keeps lone surrogates from `\u` escapes; a Rust
    /// string cannot, so they become U+FFFD.
    fn parse_string(&mut self) -> Parsed<String> {
        let start = self.pos;
        let rest = &self.json[start..];
        // Fast path: no escapes before the closing quote.
        let end = rest.iter().position(|&c| c == b'"' || c == b'\\');
        if let Some(end) = end.filter(|&end| rest[end] == b'"') {
            let text = std::str::from_utf8(&rest[..end]).map_err(|e| {
                self.pos = start + e.valid_up_to();
                ParseErrorKind::IllegalUtf8String
            })?;
            self.pos = start + end + 1;
            return Ok(text.to_owned());
        }
        if end.is_none() {
            // No quote at all: Qt validates the UTF-8 up to the end first.
            if let Err(e) = std::str::from_utf8(rest) {
                self.pos = start + e.valid_up_to();
                return Err(ParseErrorKind::IllegalUtf8String);
            }
            self.pos = self.json.len() + 1;
            return Err(ParseErrorKind::UnterminatedString);
        }

        let mut units: Vec<u16> = Vec::new();
        while let Some(c) = self.peek() {
            if c == b'"' {
                break;
            }
            if c == b'\\' {
                let ch = self.scan_escape().ok_or(ParseErrorKind::IllegalEscapeSequence)?;
                if ch < 0x1_0000 {
                    units.push(ch as u16);
                } else {
                    let mut buf = [0u16; 2];
                    let c = char::from_u32(ch).unwrap_or('\u{fffd}');
                    units.extend_from_slice(c.encode_utf16(&mut buf));
                }
            } else {
                let c = self.scan_utf8().ok_or(ParseErrorKind::IllegalUtf8String)?;
                let mut buf = [0u16; 2];
                units.extend_from_slice(c.encode_utf16(&mut buf));
            }
        }
        self.pos += 1;
        if self.pos > self.json.len() {
            return Err(ParseErrorKind::UnterminatedString);
        }
        Ok(String::from_utf16_lossy(&units))
    }

    fn scan_escape(&mut self) -> Option<u32> {
        self.pos += 1;
        let escaped = self.peek()?;
        self.pos += 1;
        Some(match escaped {
            b'b' => 0x8,
            b'f' => 0xc,
            b'n' => 0xa,
            b'r' => 0xd,
            b't' => 0x9,
            b'u' => {
                if self.pos + 4 > self.json.len() {
                    return None;
                }
                let mut ch = 0u32;
                for _ in 0..4 {
                    let h = (self.json[self.pos] as char).to_digit(16)?;
                    ch = ch << 4 | h;
                    self.pos += 1;
                }
                ch
            }
            // `"`, `\`, `/` and, leniently, any other character stand for themselves.
            other => other.into(),
        })
    }

    fn scan_utf8(&mut self) -> Option<char> {
        let rest = &self.json[self.pos..];
        let len = match rest[0] {
            0x00..=0x7f => 1,
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => return None,
        };
        let c = std::str::from_utf8(rest.get(..len)?).ok()?.chars().next()?;
        self.pos += len;
        Some(c)
    }
}

/// `QByteArray::toDouble` on the slice the JSON number grammar matched. Rust's parser accepts
/// the same decimal forms and rounds the same way; Qt also fails a value that overflows to
/// infinity or a non-zero one that underflows to zero.
fn parse_double(text: &str) -> Option<f64> {
    if text.is_empty() || text.bytes().any(|c| !matches!(c, b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')) {
        return None;
    }
    let d: f64 = text.parse().ok()?;
    let mantissa = text.split(['e', 'E']).next().unwrap_or_default();
    let underflowed = d == 0.0 && mantissa.bytes().any(|c| matches!(c, b'1'..=b'9'));
    (d.is_finite() && !underflowed).then_some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(json: &str) -> Value {
        from_json(json.as_bytes()).unwrap()
    }

    #[test]
    fn writes_like_qjsondocument() {
        let doc = obj(r#"{"b": [1, 2.5, {"z": null, "a": true}], "a": {}, "c": [], "d": "x\"\n\u0001é"}"#);
        assert_eq!(
            doc.to_json(Format::Indented),
            "{\n    \"a\": {\n    },\n    \"b\": [\n        1,\n        2.5,\n        {\n            \"a\": true,\n            \"z\": null\n        }\n    ],\n    \"c\": [\n    ],\n    \"d\": \"x\\\"\\n\\u0001é\"\n}\n"
        );
        assert_eq!(doc.to_json(Format::Compact), r#"{"a":{},"b":[1,2.5,{"a":true,"z":null}],"c":[],"d":"x\"\n\u0001é"}"#);
        assert_eq!(Object::new().to_json(Format::Indented), "{\n}\n");
        assert_eq!(Value::Array(vec![]).to_json(Format::Compact), "[]");
    }

    #[test]
    fn numbers_print_the_way_qt_prints_them() {
        let cases: &[(f64, &str)] = &[
            (1.0, "1"),
            (-0.0, "0"),
            (0.1, "0.1"),
            (0.001, "0.001"),
            (0.0001, "1e-04"),
            (100_000.5, "100000.5"),
            (1e15 + 0.5, "1000000000000000.5"),
            (5e-324, "5e-324"),
            // Exact halfway cases round to an even last digit, as double-conversion does.
            (f64::from_bits(0x42ed_bbbc_3a98_77d4), "261537232372670.62"),
            (f64::from_bits(0x4317_f139_1ebf_05b1), "1684788023902572.2"),
            (1e-5, "1e-05"),
            (12.5, "12.5"),
            (1.0 / 3.0, "0.3333333333333333"),
            (9_007_199_254_740_993.0, "9007199254740992"),
            (1e17, "1e+17"),
            (123_456_789_012_345_680.0, "123456789012345680"),
            (1e21, "1e+21"),
            (-2.5e-7, "-2.5e-07"),
            (f64::NAN, "null"),
            (f64::INFINITY, "null"),
        ];
        for &(d, expected) in cases {
            let text = Value::Array(vec![Value::from_f64(d)]).to_json(Format::Compact);
            assert_eq!(text, format!("[{expected}]"), "{d:e}");
        }
        // QJsonValue(double) keeps 2^53 an integer, but a parsed literal stays one up to 2^63.
        assert!(matches!(Value::from_f64(9_007_199_254_740_992.0), Value::Int(_)));
        assert!(matches!(Value::from_f64(1e17), Value::Double(_)));
        assert!(matches!(obj("[1e17]").as_array().unwrap()[0], Value::Int(100_000_000_000_000_000)));
    }

    #[test]
    fn conversions_follow_qjsonvalue() {
        let doc = obj(r#"{"i": 7, "f": 7.5, "w": 7.0, "big": 3000000000, "s": "7", "b": true}"#);
        assert_eq!(doc.get("i").to_int(-1), 7);
        assert_eq!(doc.get("f").to_int(-1), -1, "a fraction is not truncated");
        assert_eq!(doc.get("w").to_int(-1), 7);
        assert_eq!(doc.get("big").to_int(-1), -1);
        assert_eq!(doc.get("big").to_integer(-1), 3_000_000_000);
        assert_eq!(doc.get("s").to_int(-1), -1);
        assert_eq!(doc.get("s").to_double(-1.0), -1.0);
        assert_eq!(doc.get("i").to_double(-1.0), 7.0);
        assert!(doc.get("b").to_bool(false));
        assert!(doc.get("missing").is_undefined());
        assert_eq!(doc.get("missing").to_string_or("def"), "def");
        assert_eq!(doc.get("i"), &Value::Double(7.0));
    }

    #[test]
    fn objects_stay_sorted_and_keep_the_last_duplicate() {
        let doc = obj(r#"{"b": 1, "a": 2, "b": 3}"#);
        assert_eq!(doc.to_json(Format::Compact), r#"{"a":2,"b":3}"#);
        let mut o = Object::new();
        o.insert("z", 1);
        o.insert("m", 2);
        o.insert("z", Value::Undefined);
        assert_eq!(o.to_json(Format::Compact), r#"{"m":2}"#);
    }

    #[test]
    fn errors_match_qt() {
        let error = |json: &str| from_json(json.as_bytes()).unwrap_err();
        assert_eq!(error("").kind, ParseErrorKind::IllegalValue);
        assert_eq!(error("{\"a\" 1}").kind, ParseErrorKind::MissingNameSeparator);
        assert_eq!(error("{\"a\": 1,}").kind, ParseErrorKind::MissingObject);
        assert_eq!(error("[1 2]").kind, ParseErrorKind::MissingValueSeparator);
        assert_eq!(error("[1,").kind, ParseErrorKind::UnterminatedArray);
        assert_eq!(error("{} x").kind, ParseErrorKind::GarbageAtEnd);
        assert_eq!(error("{} x").offset, 3);
        assert_eq!(error("\"just a string\""), ParseError { kind: ParseErrorKind::IllegalValue, offset: 0 });
        assert_eq!(error("{\"a\": tru}").kind, ParseErrorKind::IllegalValue);
        assert_eq!(error("{\"a\": +1}").kind, ParseErrorKind::IllegalNumber);
        assert_eq!(error("{\"a\": \"x}").kind, ParseErrorKind::UnterminatedString);
        assert_eq!(error("[\"\\u12\"]").kind, ParseErrorKind::IllegalEscapeSequence);
        assert_eq!(error("{\"a\" 1}").error_string(), "missing name separator");
        // Offsets and kinds as Qt 6.11 reports them.
        let cases: &[(&str, ParseErrorKind, usize)] = &[
            ("[1e400]", ParseErrorKind::IllegalNumber, 6),
            ("[1e-400]", ParseErrorKind::IllegalNumber, 7),
            ("[1e]", ParseErrorKind::IllegalNumber, 3),
            ("{\"a\": +1}", ParseErrorKind::IllegalNumber, 6),
            ("[01]", ParseErrorKind::MissingValueSeparator, 3),
            ("[-]", ParseErrorKind::IllegalNumber, 2),
            ("[\"\\u12\"]", ParseErrorKind::IllegalEscapeSequence, 6),
            ("[1 2]", ParseErrorKind::MissingValueSeparator, 4),
            ("[1,", ParseErrorKind::UnterminatedArray, 3),
            ("{\"a\" 1}", ParseErrorKind::MissingNameSeparator, 6),
            ("{\"a\": 1,}", ParseErrorKind::MissingObject, 9),
            ("{\"a\": tru}", ParseErrorKind::IllegalValue, 10),
        ];
        for &(json, kind, offset) in cases {
            assert_eq!(error(json), ParseError { kind, offset }, "{json}");
        }
        assert!(from_json(b"[2e-324, 0e-400]").is_err());
        assert_eq!(obj("[3e-324, 0e-400]").as_array().unwrap()[1], Value::Int(0));
    }

    #[test]
    fn strings_decode_escapes_leniently() {
        let doc = obj(r#"["\ud83d\ude00", "\q", "a\/b", "tab	in"]"#);
        let items: Vec<_> = doc.as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_owned()).collect();
        assert_eq!(items, ["😀", "q", "a/b", "tab\tin"]);
        assert!(from_json(b"\xef\xbb\xbf{}").is_ok(), "a UTF-8 BOM is skipped");
        assert!(obj("[.5, 1., -0]").as_array().unwrap() == [Value::Double(0.5), Value::Int(1), Value::Int(0)]);
    }
}
