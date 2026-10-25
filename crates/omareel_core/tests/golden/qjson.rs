//! qjson: `compat::qjson` against `QJsonDocument` and `QJsonValue` (README "qjson").

use crate::support::*;
use omareel_core::compat::qjson::{self, Format, Object, ParseError, ParseErrorKind, Value};

/// `QJsonParseError::ParseError` as the integer Qt prints.
fn error_code(kind: ParseErrorKind) -> i32 {
    match kind {
        ParseErrorKind::UnterminatedObject => 1,
        ParseErrorKind::MissingNameSeparator => 2,
        ParseErrorKind::UnterminatedArray => 3,
        ParseErrorKind::MissingValueSeparator => 4,
        ParseErrorKind::IllegalValue => 5,
        ParseErrorKind::TerminationByNumber => 6,
        ParseErrorKind::IllegalNumber => 7,
        ParseErrorKind::IllegalEscapeSequence => 8,
        ParseErrorKind::IllegalUtf8String => 9,
        ParseErrorKind::UnterminatedString => 10,
        ParseErrorKind::MissingObject => 11,
        ParseErrorKind::DeepNesting => 12,
        ParseErrorKind::DocumentTooLarge => 13,
        ParseErrorKind::GarbageAtEnd => 14,
    }
}

fn error_cells(error: &ParseError) -> [String; 4] {
    ["error".into(), error_code(error.kind).to_string(), error.offset.to_string(), quote(error.error_string())]
}

/// What a successful parse leaves in `QJsonParseError`.
const NO_ERROR: &str = "no error occurred";

/// Qt keeps an unpaired surrogate that a `\ud800`-style escape produced; a Rust string
/// cannot hold one, so the port's parser makes it U+FFFD (documented in `compat::qjson`). The
/// goldens show such a surrogate as `\u{d800}` in **str** cells and as `\ud800` in Qt's JSON,
/// and this maps both to U+FFFD so everything around them is still compared byte for byte.
/// Both formats double every literal backslash, so an escape is only one when an even number
/// of backslashes precedes it; **bytes** cells never contain one.
fn lone_surrogates_as_replacement(text: &[u8]) -> Vec<u8> {
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

#[track_caller]
fn assert_golden_qjson(relative: &str, actual: &str) {
    let expected = lone_surrogates_as_replacement(&read_expected(relative));
    assert_same(relative, &expected, actual.as_bytes());
}

fn qjson_difference(relative: &str, actual: &str) -> Option<String> {
    let expected = lone_surrogates_as_replacement(&read_expected(relative));
    difference(relative, &expected, actual.as_bytes())
}

/// A scalar has depth 0, a container 1 + its deepest child.
fn depth(value: &Value) -> usize {
    match value {
        Value::Object(object) => 1 + object.iter().map(|(_, v)| depth(v)).max().unwrap_or(0),
        Value::Array(items) => 1 + items.iter().map(depth).max().unwrap_or(0),
        _ => 0,
    }
}

const MAX_DUMP_DEPTH: usize = 64;

/// The oracle's `dumpTyped`: every value in pre-order with its JSON pointer.
fn dump_typed(out: &mut Text, pointer: &str, value: &Value) {
    out.row([quote(pointer), typed(value)]);
    match value {
        Value::Object(object) => {
            for (key, child) in object.iter() {
                let key = key.replace('~', "~0").replace('/', "~1");
                dump_typed(out, &format!("{pointer}/{key}"), child);
            }
        }
        Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                dump_typed(out, &format!("{pointer}/{i}"), child);
            }
        }
        _ => {}
    }
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::Object(_) => "object",
        Value::Array(_) => "array",
        _ => "other",
    }
}

struct Documents {
    parse: Text,
    typed: Text,
    /// `(file name under docs/, bytes)`.
    files: Vec<(String, String)>,
}

fn documents() -> Documents {
    let directory = corpus_path("qjson/docs");
    let mut documents = Documents { parse: Text::new(), typed: Text::new(), files: Vec::new() };
    for file in sorted_entries(&directory, false) {
        let name = file.strip_suffix(".json").expect("a .json file").to_owned();
        let bytes = std::fs::read(directory.join(&file)).expect("corpus document");
        let root = match qjson::from_json(&bytes) {
            Err(error) => {
                let mut cells = vec![name.clone()];
                cells.extend(error_cells(&error));
                documents.parse.row(cells);
                continue;
            }
            Ok(root) => root,
        };
        documents.parse.row([name.clone(), "ok".into(), kind(&root).into(), "0".into(), quote(NO_ERROR)]);
        documents.files.push((format!("{name}.compact.json"), root.to_json(Format::Compact)));
        if depth(&root) > MAX_DUMP_DEPTH {
            continue;
        }
        documents.files.push((format!("{name}.indented.json"), root.to_json(Format::Indented)));
        documents.typed.section(&name);
        dump_typed(&mut documents.typed, "", &root);
    }
    documents
}

/// Compares every regenerated file of a directory and checks that no golden file is missing
/// from the regenerated set (`skipped` names the ones deliberately not regenerated).
#[track_caller]
fn assert_directory(directory: &str, files: &[(String, String)], skipped: &[&str]) {
    let mut reports = Vec::new();
    for (file, bytes) in files {
        if let Some(report) = qjson_difference(&format!("{directory}/{file}"), bytes) {
            reports.push(report);
        }
    }
    for golden in sorted_entries(&expected_path(directory), false) {
        if !files.iter().any(|(file, _)| *file == golden) && !skipped.contains(&golden.as_str()) {
            reports.push(format!("{directory}/{golden}: not regenerated"));
        }
    }
    assert_no_differences(reports);
}

#[test]
fn parse_txt() {
    assert_golden("qjson/parse.txt", &documents().parse.into_string());
}

#[test]
fn docs() {
    assert_directory("qjson/docs", &documents().files, &[]);
}

#[test]
fn typed_txt() {
    assert_golden_qjson("qjson/typed.txt", &documents().typed.into_string());
}

/// The oracle's `runLiterals`: each line of the corpus file parsed as `[<line>]`.
fn literals(corpus: &str) -> String {
    let data = read_corpus(corpus);
    let mut lines: Vec<&[u8]> = data.split(|&b| b == b'\n').collect();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    let mut out = Text::new();
    for (index, literal) in lines.into_iter().enumerate() {
        let mut cells = vec![(index + 1).to_string(), quote_bytes(literal)];
        let document = [b"[".as_slice(), literal, b"]"].concat();
        match qjson::from_json(&document) {
            Err(error) => cells.extend(error_cells(&error)),
            Ok(root) => {
                let items = root.to_array();
                cells.extend(["ok".into(), items.len().to_string(), json(&root)]);
                if let Some(first) = items.first() {
                    cells.push(typed(first));
                }
            }
        }
        out.row(cells);
    }
    out.into_string()
}

#[test]
fn number_literals_txt() {
    assert_golden("qjson/number-literals.txt", &literals("qjson/number-literals.txt"));
}

#[test]
fn string_literals_txt() {
    assert_golden_qjson("qjson/string-literals.txt", &literals("qjson/string-literals.txt"));
}

/// The first element of a one-element array document.
fn parse_back(json: &str) -> Value {
    qjson::from_json(json.as_bytes()).expect("Qt's own output parses").to_array().remove(0)
}

#[test]
fn doubles_txt() {
    let mut out = Text::new();
    for value in read_doubles("numbers/doubles.txt") {
        let json = wrapped(&Value::from_f64(value));
        let back = parse_back(&json);
        out.row([
            hex(value),
            json,
            json_type_name(&back).into(),
            variant_type_name(&back).into(),
            hex(back.to_double(-7.0)),
        ]);
    }
    assert_golden("qjson/doubles.txt", &out.into_string());
}

#[test]
fn int64_txt() {
    let mut out = Text::new();
    for line in corpus_lines("numbers/int64.txt") {
        let text = std::str::from_utf8(&line).expect("ASCII");
        let value: i64 = text.trim().parse().unwrap_or_else(|_| panic!("bad int64 corpus line {text:?}"));
        let json = wrapped(&Value::from(value));
        let back = parse_back(&json);
        out.row([
            value.to_string(),
            json,
            variant_type_name(&back).into(),
            back.to_variant_long_long().to_string(),
            hex(back.to_double(-7.0)),
        ]);
    }
    assert_golden("qjson/int64.txt", &out.into_string());
}

/// What a Rust string holds where `loneSurrogates()` in `qjson.cpp` builds
/// `"a" U+D800 "b" U+DC00 U+DBFF`: the U+FFFD the port's parser substitutes for each.
const LONE_SURROGATES: &str = "a\u{fffd}b\u{fffd}\u{fffd}";

/// `builtDocuments()` of `tools/oracle/cases/qjson.cpp`, as far as the Rust API has the same
/// operations: `from-variant-map` (`QJsonValue::fromVariant`) is left out, and `null-document`
/// is only its root, `Undefined` (see [`NO_FILES`]). `QJsonArray` is a plain `Vec<Value>`, and
/// `QLatin1String` keys are ordinary `&str`s.
fn built_documents() -> Vec<(&'static str, Value)> {
    let mut documents = Vec::new();

    let mut scalars = Object::new();
    scalars.insert("two", 2.0);
    scalars.insert("negativeZero", -0.0);
    scalars.insert("int", 7);
    scalars.insert("int64Max", i64::MAX);
    scalars.insert("int64Min", i64::MIN);
    scalars.insert("int64Big", 1i64 << 60);
    scalars.insert("int64TwoPow53Plus1", (1i64 << 53) + 1);
    scalars.insert("doubleTwoPow53", 9007199254740992.0);
    scalars.insert("doubleTwoPow63", 9223372036854775808.0);
    scalars.insert("double1e20", 1e20);
    scalars.insert("double1e300", 1e300);
    scalars.insert("doubleHalf", 0.5);
    scalars.insert("doubleThird", 1.0 / 3.0);
    scalars.insert("doubleTiny", 5e-324);
    scalars.insert("float", f64::from(0.1f32));
    scalars.insert("nan", f64::NAN);
    scalars.insert("inf", f64::INFINITY);
    scalars.insert("negativeInf", f64::NEG_INFINITY);
    scalars.insert("true", true);
    scalars.insert("false", false);
    scalars.insert("null", Value::Null);
    scalars.insert("string", "x");
    scalars.insert("emptyString", "");
    documents.push(("scalars", Value::Object(scalars)));

    let mut undefined_object = Object::from_iter([("kept", 1)]);
    undefined_object.insert("undefined", Value::Undefined);
    undefined_object.insert("kept", Value::Undefined);
    undefined_object.insert("after", 2);
    documents.push(("undefined-in-object", Value::Object(undefined_object)));
    let undefined_array = vec![Value::Undefined, Value::from(1), Value::Undefined];
    documents.push(("undefined-in-array", Value::Array(undefined_array)));
    let undefined_nested = Object::from_iter([
        ("o", Value::Object(Object::from_iter([("u", Value::Undefined)]))),
        ("a", Value::Array(vec![Value::Undefined])),
    ]);
    documents.push(("undefined-initializer-list", Value::Object(undefined_nested)));

    let mut nested = Object::new();
    nested.insert("z", Object::from_iter([("b", Value::from(1)), ("a", Value::Array(vec![]))]));
    nested.insert(
        "y",
        vec![Value::Object(Object::new()), Value::Array(vec![]), Value::Array(vec![Value::Array(vec![])])],
    );
    let deepest = vec![Value::from(1), Value::from(2.5), Value::from("s")];
    let deeper = Object::from_iter([("deepest", deepest)]);
    let deep = Object::from_iter([("deeper", deeper)]);
    nested.insert("x", Object::from_iter([("deep", deep)]));
    nested.insert("a", 1);
    nested.insert("a", 2); // replaces
    nested.insert("removed", 3);
    nested.remove("removed");
    let mut inserted = vec![Value::from(1), Value::from(2), Value::from(3)];
    inserted.insert(1, Value::from("one-and-a-half"));
    inserted[0] = Value::Null;
    inserted.insert(0, Value::Object(Object::from_iter([("first", true)])));
    inserted.remove(3);
    nested.insert("array", inserted);
    documents.push(("nested", Value::Object(nested)));

    let mut strings = Object::new();
    strings.insert("control", "\x01\x02\t\n\r\x08\x0c\x1f\x7f");
    strings.insert("nul", "\0x");
    strings.insert("quotes", "\"'\\/");
    strings.insert("unicode", "é€\u{1F600}\u{2028}\u{2029}\u{feff}\u{ffff}");
    strings.insert("c1", "\u{80}\u{9f}\u{a0}");
    strings.insert("lone", LONE_SURROGATES);
    strings.insert(LONE_SURROGATES, "key with lone surrogates");
    strings.insert("é", 1);
    strings.insert("e", 2);
    strings.insert("\u{1F600}", 3);
    strings.insert("！", 4);
    strings.insert("\0", 5);
    documents.push(("strings", Value::Object(strings)));

    let mut latin1 = Object::new();
    latin1.insert("b", 1);
    latin1.insert("a", 2);
    latin1.insert("\u{e9}", 3);
    latin1.insert("è", 4);
    latin1.insert("\u{ff}", 5);
    latin1.insert("Ā", 6);
    latin1.insert("A", 7);
    documents.push(("latin1-keys", Value::Object(latin1)));

    let mut numbers: Vec<Value> = [
        0.0,
        -0.0,
        1.0,
        1.5,
        0.1,
        1e15,
        1e16,
        1e17,
        1e21,
        1e22,
        1e-7,
        123456789012345678.0,
        9007199254740993.0,
        -1e300,
        2.5e-324,
    ]
    .into_iter()
    .map(Value::from_f64)
    .collect();
    numbers.extend([0i64, -1, 1 << 53, (1 << 53) + 1, i64::MAX].map(Value::from));
    numbers.push(Value::from(i32::MIN));
    numbers.push(Value::from_f64(Value::from(42i64).to_double(0.0)));
    documents.push(("numbers", Value::Array(numbers)));

    documents.push((NULL_DOCUMENT, Value::Undefined));
    documents.push(("empty-object", Value::Object(Object::new())));
    documents.push(("empty-array", Value::Array(vec![])));
    let empty_nested =
        vec![Value::Array(vec![]), Value::Object(Object::new()), Value::Array(vec![Value::Object(Object::new())])];
    documents.push(("empty-nested", Value::Array(empty_nested)));
    documents
}

/// `QJsonValue::fromVariant(QVariantMap)`, which has no Rust counterpart.
const FROM_VARIANT_MAP: &str = "from-variant-map";

/// `QJsonDocument()`: there is no Rust document type, so nothing writes its two empty files,
/// but the `Undefined` root it dumps in `built-typed.txt` is an ordinary value.
const NULL_DOCUMENT: &str = "null-document";

/// Built documents without regenerated `.json` files.
const NO_FILES: &[&str] = &[FROM_VARIANT_MAP, NULL_DOCUMENT];

/// Built documents whose typed dump differs: `QJsonArray::append` and `QJsonArray`'s
/// initializer list store `Undefined` as `Null`, but a `Vec<Value>` keeps it, so these
/// arrays' elements read back as `Undefined` (the JSON text is the same, `null`).
const UNDEFINED_ARRAY_ELEMENTS: &[&str] = &["undefined-in-array", "undefined-initializer-list"];

fn built_typed(keep: impl Fn(&str) -> bool) -> String {
    let mut out = Text::new();
    for (name, document) in built_documents() {
        if keep(name) {
            out.section(name);
            dump_typed(&mut out, "", &document);
        }
    }
    out.into_string()
}

#[test]
fn built() {
    let mut files = Vec::new();
    for (name, document) in built_documents().into_iter().filter(|(name, _)| !NO_FILES.contains(name)) {
        files.push((format!("{name}.compact.json"), document.to_json(Format::Compact)));
        files.push((format!("{name}.indented.json"), document.to_json(Format::Indented)));
    }
    let skipped: Vec<String> =
        NO_FILES.iter().flat_map(|name| [format!("{name}.compact.json"), format!("{name}.indented.json")]).collect();
    let skipped: Vec<&str> = skipped.iter().map(String::as_str).collect();
    assert_directory("qjson/built", &files, &skipped);
}

#[test]
fn built_typed_txt() {
    let keep = |name: &str| name != FROM_VARIANT_MAP && !UNDEFINED_ARRAY_ELEMENTS.contains(&name);
    let expected = keep_sections(&lone_surrogates_as_replacement(&read_expected("qjson/built-typed.txt")), keep);
    assert_same("qjson/built-typed.txt (Rust-buildable sections)", &expected, built_typed(keep).as_bytes());
}

#[test]
#[ignore = "a Vec<Value> keeps Value::Undefined where QJsonArray::append and QJsonArray{...} \
            store Null, so the elements of undefined-in-array and undefined-initializer-list's \
            \"a\" read back as Undefined (type, typeName <invalid>) instead of Null \
            (std::nullptr_t); their JSON text matches"]
fn built_typed_txt_undefined_array_elements() {
    let keep = |name: &str| UNDEFINED_ARRAY_ELEMENTS.contains(&name);
    let expected = keep_sections(&read_expected("qjson/built-typed.txt"), keep);
    assert_same("qjson/built-typed.txt (undefined array elements)", &expected, built_typed(keep).as_bytes());
}

/// `builtValues()` of `qjson.cpp` with a Rust counterpart: everything but the
/// `QJsonValue::fromVariant` rows. (`latin1 abc` is an ordinary string here.)
fn built_values() -> Vec<(&'static str, Value)> {
    vec![
        ("QJsonValue()", Value::Null),
        ("Undefined", Value::Undefined),
        ("true", Value::from(true)),
        ("false", Value::from(false)),
        ("int 0", Value::from(0)),
        ("int -1", Value::from(-1)),
        ("int max", Value::from(i32::MAX)),
        ("int min", Value::from(i32::MIN)),
        ("qint64 max", Value::from(i64::MAX)),
        ("qint64 min", Value::from(i64::MIN)),
        ("qint64 2^31", Value::from(1i64 << 31)),
        ("qint64 2^53+1", Value::from((1i64 << 53) + 1)),
        ("double 0", Value::from_f64(0.0)),
        ("double -0", Value::from_f64(-0.0)),
        ("double 1.5", Value::from_f64(1.5)),
        ("double -1.5", Value::from_f64(-1.5)),
        ("double 2.5", Value::from_f64(2.5)),
        ("double 2", Value::from_f64(2.0)),
        ("double 2^31", Value::from_f64(2147483648.0)),
        ("double -2^31-1", Value::from_f64(-2147483649.0)),
        ("double 2^53", Value::from_f64(9007199254740992.0)),
        ("double 2^63", Value::from_f64(9223372036854775808.0)),
        ("double -2^63", Value::from_f64(-9223372036854775808.0)),
        ("double 1e19", Value::from_f64(1e19)),
        ("double 1e300", Value::from_f64(1e300)),
        ("double 0.1", Value::from_f64(0.1)),
        ("double nan", Value::from_f64(f64::NAN)),
        ("double inf", Value::from_f64(f64::INFINITY)),
        ("double -inf", Value::from_f64(f64::NEG_INFINITY)),
        ("string empty", Value::from("")),
        ("string abc", Value::from("abc")),
        ("string 12", Value::from("12")),
        ("string 1e3", Value::from("1e3")),
        ("string -0x10", Value::from("-0x10")),
        ("string true", Value::from("true")),
        ("string 1.5", Value::from("1.5")),
        ("string  7 ", Value::from(" 7 ")),
        ("string 9223372036854775808", Value::from("9223372036854775808")),
        ("latin1 abc", Value::from("abc")),
        ("array empty", Value::Array(vec![])),
        ("array [1]", Value::Array(vec![Value::from(1)])),
        ("object empty", Value::Object(Object::new())),
        ("object {a:1}", Value::Object(Object::from_iter([("a", 1)]))),
    ]
}

#[test]
fn values_txt() {
    let mut out = Text::new();
    for (label, value) in built_values() {
        out.row([label.to_owned(), typed(&value), wrapped(&value)]);
    }
    let expected = keep_lines(&read_expected("qjson/values.txt"), |line| !line.starts_with(b"fromVariant("));
    assert_same("qjson/values.txt (without the fromVariant rows)", &expected, out.into_string().as_bytes());
}

#[test]
fn lone_surrogate_normalisation() {
    // A JSON escape, a str escape, and the same text behind an escaped backslash.
    let golden = br#"["\ud800a"]	"\u{dc00}\u{d800}"	"\\ud800"	"\\u{d800}"	"\u{d83d}x\u{41}""#;
    let expected = "[\"\u{fffd}a\"]\t\"\u{fffd}\u{fffd}\"\t\"\\\\ud800\"\t\"\\\\u{d800}\"\t\"\u{fffd}x\\u{41}\"";
    assert_eq!(String::from_utf8(lone_surrogates_as_replacement(golden)).unwrap(), expected);
}
