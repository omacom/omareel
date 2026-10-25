//! The two-regex TOML reading Theme does (`readToml` in Theme.cpp), by hand: keys become
//! `section.key` (`.key` at the top level), values are quoted strings or the text before a
//! comment. Anything else on a line is ignored, as the regexes ignore it.

use crate::compat::qfile;
use std::collections::HashMap;

/// PCRE's `\s` without Unicode properties: ASCII whitespace, vertical tab included.
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r')
}

fn skip_space(s: &str) -> &str {
    s.trim_start_matches(is_space)
}

/// `\s*(?:#.*)?$`: trailing space, an optional comment that stays on the line, then the end
/// or a final newline.
fn is_tail(t: &str) -> bool {
    let rest = skip_space(t);
    match rest.strip_prefix('#') {
        None => rest.is_empty(),
        Some(comment) => !comment.strip_suffix('\n').unwrap_or(comment).contains('\n'),
    }
}

/// `^\s*\[([^]]+)\]`
pub(crate) fn section(line: &str) -> Option<&str> {
    let rest = skip_space(line).strip_prefix('[')?;
    let end = rest.find(']')?;
    (end > 0).then(|| &rest[..end])
}

/// `^\s*([A-Za-z0-9_-]+)\s*=\s*(?:["']([^"']*)["']|([^#\s][^#]*?))\s*(?:#.*)?$`
pub(crate) fn value(line: &str) -> Option<(&str, &str)> {
    let rest = skip_space(line);
    let key_end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-')).unwrap_or(rest.len());
    if key_end == 0 {
        return None;
    }
    let key = &rest[..key_end];
    let rest = skip_space(&rest[key_end..]);
    let rest = skip_space(rest.strip_prefix('=')?);
    // A quoted value: everything up to the next quote of either kind.
    if let Some(inner) = rest.strip_prefix(['"', '\'']) {
        if let Some(close) = inner.find(['"', '\'']) {
            if is_tail(&inner[close + 1..]) {
                return Some((key, &inner[..close]));
            }
        }
    }
    // Otherwise the shortest run that starts with something other than '#' or space, holds
    // no '#', and leaves only a tail.
    let first = rest.chars().next()?;
    if first == '#' || is_space(first) {
        return None;
    }
    let limit = rest.find('#').unwrap_or(rest.len());
    let mut end = first.len_utf8();
    loop {
        if is_tail(&rest[end..]) {
            return Some((key, &rest[..end]));
        }
        if end >= limit {
            return None;
        }
        end += rest[end..].chars().next().map_or(1, char::len_utf8);
    }
}

/// The values of a TOML file Theme reads; empty when it can't be opened.
pub fn read(path: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    let Ok(bytes) = qfile::read_all(path) else { return values };
    let mut current = String::new();
    for line in qfile::lines(&bytes) {
        let line = String::from_utf8_lossy(line);
        if let Some(name) = section(&line) {
            current = name.to_owned();
            continue;
        }
        if let Some((key, raw)) = value(&line) {
            values.insert(format!("{current}.{key}"), raw.trim().to_owned());
        }
    }
    values
}

#[cfg(test)]
mod tests {
    /// Compares with Qt's regexes over a corpus recorded by a scratch program, when given.
    #[test]
    fn matches_recorded_qt_results() {
        let (Ok(lines), Ok(qt)) = (std::env::var("OMAREEL_TOML_LINES"), std::env::var("OMAREEL_TOML_QT")) else { return };
        let lines = std::fs::read(lines).unwrap();
        let qt = std::fs::read(qt).unwrap();
        let mut bad = 0;
        for (line, expected) in lines.split(|&b| b == 1).zip(qt.split(|&b| b == 1)) {
            if line.is_empty() {
                continue;
            }
            let line = String::from_utf8_lossy(line);
            let got = if let Some(name) = section(&line) {
                format!("S {name}")
            } else if let Some((k, v)) = value(&line) {
                format!("V {k}={}", v.trim())
            } else {
                "-".to_owned()
            };
            if got.as_bytes() != expected {
                bad += 1;
                if bad < 10 { eprintln!("{line:?}: qt {:?} rust {got:?}", String::from_utf8_lossy(expected)); }
            }
        }
        assert_eq!(bad, 0);
    }

    use super::*;

    #[test]
    fn reads_values_the_way_the_regexes_do() {
        assert_eq!(value("accent = \"#89b4fa\"\n"), Some(("accent", "#89b4fa")));
        assert_eq!(value("  mode='dark' # light or dark\n"), Some(("mode", "dark")));
        assert_eq!(value("size = 12   # px\n"), Some(("size", "12")));
        assert_eq!(value("a-b_c=1.5"), Some(("a-b_c", "1.5")));
        assert_eq!(value("x = \"a#b\"\n"), Some(("x", "a#b")));
        // A quoted value followed by more text is read whole, quotes and all.
        assert_eq!(value("x = \"a\" b\n"), Some(("x", "\"a\" b")));
        assert_eq!(value("x = # nothing\n"), None);
        assert_eq!(value("x =\n"), None);
        assert_eq!(value("= 1\n"), None);
        assert_eq!(value("x.y = 1\n"), None);
        assert_eq!(section("[font] # sizes\n"), Some("font"));
        assert_eq!(section("[]\n"), None);
    }
}
