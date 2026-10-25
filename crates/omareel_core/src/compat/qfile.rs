//! `QFile` reading: Qt's error strings and `readLine` semantics.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// `QFile::errorString()` after a failed system call: the C library's `strerror` text, which
/// Rust's error display also starts with.
pub fn error_string(error: &io::Error) -> String {
    let text = error.to_string();
    match error.raw_os_error() {
        Some(code) => text.strip_suffix(&format!(" (os error {code})")).unwrap_or(&text).to_owned(),
        None => text,
    }
}

/// `QFile::open(QIODevice::ReadOnly)`, with QFile's own messages for an empty name and a
/// directory.
pub fn open_read(path: impl AsRef<Path>) -> Result<File, String> {
    let path = path.as_ref();
    if path.as_os_str().is_empty() {
        return Err("No file name specified".into());
    }
    let file = File::open(path).map_err(|e| error_string(&e))?;
    match file.metadata() {
        Ok(meta) if meta.is_dir() => Err("file to open is a directory".into()),
        _ => Ok(file),
    }
}

/// `QFile::readAll` after a successful open.
pub fn read_all(path: impl AsRef<Path>) -> Result<Vec<u8>, String> {
    let mut file = open_read(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|e| error_string(&e))?;
    Ok(bytes)
}

/// The lines `QIODevice::readLine` returns until `atEnd()`: each ends at and includes a
/// `\n`, and the last may lack one.
pub fn lines(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    bytes.split_inclusive(|&b| b == b'\n')
}

/// `QByteArray::trimmed`: strips ASCII whitespace (space, `\t`, `\n`, `\v`, `\f`, `\r`).
pub fn trimmed(bytes: &[u8]) -> &[u8] {
    let space = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r');
    let start = bytes.iter().position(|b| !space(b)).unwrap_or(bytes.len());
    let end = bytes.iter().rposition(|b| !space(b)).map_or(start, |i| i + 1);
    &bytes[start..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_read_like_qt() {
        assert_eq!(open_read("/nonexistent/omareel").unwrap_err(), "No such file or directory");
        assert_eq!(open_read("/").unwrap_err(), "file to open is a directory");
        assert_eq!(open_read("").unwrap_err(), "No file name specified");
    }

    #[test]
    fn lines_keep_their_newlines_and_trim() {
        let all: Vec<_> = lines(b"a\n\n b \r\nlast").collect();
        assert_eq!(all, [&b"a\n"[..], b"\n", b" b \r\n", b"last"]);
        assert_eq!(trimmed(b" \t b \r\n"), b"b");
        assert_eq!(trimmed(b" \n"), b"");
    }
}
