//! `QUrl::fromLocalFile(path).toString()`, the file URLs omareel hands QML for images and
//! thumbnails (Qt 6.11).
//!
//! What Qt does, reproduced:
//! - An empty path gives an empty URL, `""`.
//! - A path whose second UTF-16 unit is `:` (and whose first is not `/`) gets a `/` in front,
//!   Qt's Windows-drive rule, which it applies on every platform: `c:x` is `file:///c:x`. A
//!   character outside the BMP is two units, so `😀:x` stays relative.
//! - A path starting with `//` is a UNC path: the text up to the next `/` is the host (less a
//!   trailing `@SSL` in any case, which makes the scheme `webdavs`). A host Qt accepts becomes the
//!   URL's host and the rest is the path; when no `/` follows the host, or the host is empty and
//!   a `/` follows at once (`///x`), the path is dropped, as Qt drops it. Qt accepts: an IPv4
//!   address as its `parseIp4` reads it (one to four parts, each decimal, `0x` hex, `0b` binary or
//!   `0` octal after optional ASCII white space and `+`, the last filling the remaining bytes,
//!   cut at a NUL), written as a dotted quad; an IPv6 address in brackets as its `parseIp6` reads
//!   it (with a `%25` zone id), written in Qt's compressed form (`::ffff:1.2.3.4` and
//!   `::1.2.3.4` keep their IPv4 tail); an `[vH.…]` IPvFuture; and a registered name whose dot
//!   separated labels are 1 to 63 of `[A-Za-z0-9_-]`, not starting or ending with `-`, lower-cased
//!   (a trailing dot is kept). A bad bracketed host makes Qt's URL invalid, which prints as `""`;
//!   any other host Qt rejects keeps the whole path, so `//a b/c` is `file:////a b/c`.
//! - Other absolute paths get an empty host (`file:///...`); relative ones none (`file:a/b`).
//! - In the path, `%` and the characters a URL path cannot hold are percent-encoded with
//!   upper-case hex: the C0 controls, DEL, space excepted `"`, `#`, `<`, `>`, `?`, `[`, `\`, `]`,
//!   `^`, `` ` ``, `{`, `|`, `}`. Everything else, every non-ASCII character included, stays as it
//!   is (`toString()` is `PrettyDecoded`).
//!
//! Checked against Qt 6.11.2 (a scratch program printing `QUrl::fromLocalFile(p).toString()`)
//! over every BMP code point and a sample of the others in seven positions, 60,000 random paths
//! built from the characters that matter (drive letters, `//` hosts, `@SSL`, brackets, `%`,
//! controls, non-ASCII), and some 700 crafted hosts (IPv4 forms, IPv6 with and without IPv4 tails
//! and zone ids, IPvFuture, label rules), with no differences outside what follows. Not
//! reproduced, both only reachable through a `//host` path, which omareel never builds (its paths
//! are absolute, and start with `//` only when `$HOME` or an XDG variable is set that way): a host
//! with non-ASCII characters, which Qt maps and converts to Punycode (here it is rejected, so the
//! path is kept whole), and a host with an `xn--` label under a top-level domain on Qt's IDN
//! whitelist, which Qt shows decoded (here it stays in ACE form); and an IPv6 zone id holding a
//! `%`, which Qt re-encodes when it prints the host (here it is printed as given).

/// `QUrl::fromLocalFile(path).toString()`.
pub fn from_local_file(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let mut path = path.to_owned();
    let mut scheme = "file";
    let mut units = path.encode_utf16();
    let first = units.next();
    let second = units.next();
    let slash = Some(u16::from(b'/'));
    let mut host: Option<String> = None;
    if first != slash && second == Some(u16::from(b':')) {
        path.insert(0, '/');
    } else if first == slash && second == slash {
        let index_of_path = path[2..].find('/').map(|i| i + 2);
        let mut host_spec = match index_of_path {
            Some(i) => &path[2..i],
            None => &path[2..],
        };
        let ssl_at = host_spec.len().wrapping_sub(4);
        if host_spec.len() >= 4 && host_spec.is_char_boundary(ssl_at) && host_spec[ssl_at..].eq_ignore_ascii_case("@SSL") {
            host_spec = &host_spec[..ssl_at];
            scheme = "webdavs";
        }
        match parse_host(host_spec) {
            Host::Valid(name) => {
                host = Some(name);
                path = match index_of_path {
                    Some(i) if i > 2 => path[i..].to_owned(),
                    _ => String::new(),
                };
            }
            // Qt returns the URL in its error state, which prints as nothing.
            Host::Invalid => return String::new(),
            // Not a valid host name: the whole path stays, behind an empty host.
            Host::InvalidRegName => {}
        }
    }
    let absolute = path.starts_with('/') || host.is_some();
    let mut out = String::with_capacity(path.len() + 8);
    out.push_str(scheme);
    out.push(':');
    if absolute {
        out.push_str("//");
        out.push_str(host.as_deref().unwrap_or(""));
    }
    for c in path.chars() {
        let encode = matches!(
            c,
            '\0'..='\u{1f}' | '\u{7f}' | '%' | '"' | '#' | '<' | '>' | '?' | '[' | '\\' | ']' | '^' | '`' | '{' | '|' | '}'
        );
        if encode {
            out.push_str(&format!("%{:02X}", c as u32));
        } else {
            out.push(c);
        }
    }
    out
}

enum Host {
    Valid(String),
    /// A registered name Qt does not accept: `fromLocalFile` keeps the path whole.
    InvalidRegName,
    /// Any other host error (a bad bracketed address): `fromLocalFile` gives up.
    Invalid,
}

/// `QUrlPrivate::setHost(host, 0, size, StrictMode)` for the hosts `fromLocalFile` can meet.
fn parse_host(host: &str) -> Host {
    if host.is_empty() {
        return Host::Valid(String::new());
    }
    let units: Vec<u16> = host.encode_utf16().collect();
    if units[0] == u16::from(b'[') {
        if units[units.len() - 1] != u16::from(b']') {
            return Host::Invalid;
        }
        let inner = &units[1..units.len() - 1];
        if units.len() > 5 && units[1] == u16::from(b'v') {
            return parse_ip_future(&units).map_or(Host::Invalid, Host::Valid);
        }
        return parse_bracketed_ip6(inner).map_or(Host::Invalid, Host::Valid);
    }
    if let Some(address) = ascii_buffer(&units).and_then(|buffer| parse_ip4(&buffer, true)) {
        return Host::Valid(ip4_to_string(address));
    }
    // qt_ACE_do on an ASCII name: lower-cased, every label checked. Non-ASCII names are not
    // reproduced (see the module documentation).
    if !host.is_ascii() {
        return Host::InvalidRegName;
    }
    let lower = host.to_ascii_lowercase();
    let labels: Vec<&str> = lower.strip_suffix('.').unwrap_or(&lower).split('.').collect();
    let valid_label = |label: &&str| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    };
    if labels.iter().all(valid_label) {
        Host::Valid(lower)
    } else {
        Host::InvalidRegName
    }
}

/// `checkedToAscii`: the units as bytes up to the first NUL (Qt copies them into a C string),
/// or `None` when any unit is DEL or above.
fn ascii_buffer(units: &[u16]) -> Option<Vec<u8>> {
    if units.iter().any(|&u| u >= 0x7f) {
        return None;
    }
    Some(units.iter().map(|&u| u as u8).take_while(|&b| b != 0).collect())
}

fn ascii_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `qstrntoull(text, size, base)`: ASCII white space, an optional `+` (a `-` fails), a `0x` or
/// `0b` prefix (or a leading `0` meaning octal) when `base` is 0, then digits as
/// `std::from_chars` reads them. The value and how many bytes from the start it used; `None`
/// when there are no digits or the value overflows 64 bits.
fn strntoull(text: &[u8], base: u32) -> Option<(u64, usize)> {
    let mut p = 0;
    while p < text.len() && ascii_space(text[p]) {
        p += 1;
    }
    if p >= text.len() || text[p] == b'-' {
        return None;
    }
    if text[p] == b'+' {
        p += 1;
    }
    let mut base = base;
    if p < text.len() && text[p].is_ascii_digit() {
        if text[p] == b'0' {
            match text.get(p + 1) {
                Some(b'b' | b'B') => {
                    if base == 0 {
                        base = 2;
                    }
                    if base == 2 {
                        p += 2;
                    }
                }
                Some(b'x' | b'X') => {
                    if base == 0 {
                        base = 16;
                    }
                    if base == 16 {
                        p += 2;
                    }
                }
                _ => {
                    if base == 0 {
                        base = 8;
                    }
                }
            }
        } else if base == 0 {
            base = 10;
        }
    }
    if base == 0 || p >= text.len() {
        return None;
    }
    let digits = text[p..].iter().take_while(|&&b| char::from(b).is_digit(base)).count();
    if digits == 0 {
        return None;
    }
    let mut value: u64 = 0;
    for &b in &text[p..p + digits] {
        let digit = u64::from(char::from(b).to_digit(base)?);
        value = value.checked_mul(u64::from(base))?.checked_add(digit)?;
    }
    Some((value, p + digits))
}

/// `parseIp4Internal`: up to four parts; a part followed by a dot is one byte, the last fills
/// what is left. `text` is the NUL-free C string Qt parses.
fn parse_ip4(text: &[u8], accept_leading_zero: bool) -> Option<u32> {
    let mut address: u32 = 0;
    let mut dot_count = 0;
    let mut pos = 0;
    let at = |i: usize| text.get(i).copied().unwrap_or(0);
    while dot_count < 4 {
        if !accept_leading_zero && at(pos) == b'0' && at(pos + 1) != b'.' && at(pos + 1) != 0 {
            return None;
        }
        let (value, used) = strntoull(&text[pos..], 0)?;
        let x = u32::try_from(value).ok()?;
        let next = at(pos + used);
        if next == b'.' || dot_count == 3 {
            if x & !0xff != 0 {
                return None;
            }
            address <<= 8;
        } else if dot_count == 2 {
            if x & !0xffff != 0 {
                return None;
            }
            address <<= 16;
        } else if dot_count == 1 {
            if x & !0xff_ffff != 0 {
                return None;
            }
            address <<= 24;
        }
        address |= x;
        if dot_count == 3 || next == 0 {
            return (next == 0).then_some(address);
        }
        if next != b'.' {
            return None;
        }
        dot_count += 1;
        pos += used + 1;
    }
    None
}

fn ip4_to_string(address: u32) -> String {
    let b = address.to_be_bytes();
    format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3])
}

/// The `parseIp6` in qurl.cpp: an optional `%25` zone id (once, and not empty) split off, the
/// address read by `QIPAddressUtils::parseIp6`, and the host written back in brackets.
fn parse_bracketed_ip6(inner: &[u16]) -> Option<String> {
    let zone_marker: Vec<u16> = "%25".encode_utf16().collect();
    let mut positions = (0..inner.len().saturating_sub(2)).filter(|&i| inner[i..i + 3] == zone_marker[..]);
    let mut address_units = inner;
    let mut zone: &[u16] = &[];
    if let Some(first) = positions.next() {
        if positions.next_back().is_none() {
            zone = &inner[first + 3..];
            address_units = &inner[..first];
            if zone.is_empty() {
                return None;
            }
        }
    }
    if address_units.is_empty() {
        return None;
    }
    let address = parse_ip6(address_units)?;
    let mut host = String::from("[");
    host.push_str(&ip6_to_string(&address));
    if !zone.is_empty() {
        host.push_str("%25");
        host.push_str(&String::from_utf16_lossy(zone));
    }
    host.push(']');
    Some(host)
}

/// `QIPAddressUtils::parseIp6`.
fn parse_ip6(units: &[u16]) -> Option<[u8; 16]> {
    let buffer = ascii_buffer(units)?;
    let len = units.len();
    let at = |i: usize| buffer.get(i).copied().unwrap_or(0);
    let colon_count = buffer.iter().filter(|&&b| b == b':').count();
    let dot_count = buffer.iter().filter(|&&b| b == b'.').count();
    if dot_count != 0 && dot_count != 3 {
        return None;
    }
    let mut address = [0u8; 16];
    if colon_count == 2 && len == 2 {
        // Qt checks the length, not the text: any two units with two colons are "::".
        return Some(address);
    }
    let ends_in_double_colon = len >= 2 && at(len - 2) == b':' && at(len - 1) == b':';
    let mut zero_words_to_fill: i64 = if (at(0) == b':' && at(1) == b':') || ends_in_double_colon {
        9 - colon_count as i64
    } else if !(2..=7).contains(&colon_count) {
        return None;
    } else {
        8 - colon_count as i64
    };
    if dot_count != 0 {
        zero_words_to_fill -= 1;
    }
    let mut pos: usize = 0;
    let mut ptr: usize = 0;
    while pos < 15 {
        if at(ptr) == b':' {
            // An empty field, which had better be "::".
            if zero_words_to_fill < 1 {
                return None;
            }
            if pos == 0 || pos == colon_count * 2 {
                if at(ptr) == 0 || at(ptr + 1) != b':' {
                    return None;
                }
                ptr += 1;
            }
            pos += zero_words_to_fill as usize * 2;
            zero_words_to_fill = 0;
            ptr += 1;
            continue;
        }
        let (value, used) = strntoull(&buffer[ptr.min(buffer.len())..], 16).unwrap_or((0, 0));
        if used == 0 || used > 4 {
            return None;
        }
        let next = at(ptr + used);
        if next == b'.' {
            // An IPv4 tail, only as the last two words.
            if pos != 12 {
                return None;
            }
            let ip4 = parse_ip4(&buffer[ptr..], false)?;
            address[12..].copy_from_slice(&ip4.to_be_bytes());
            return Some(address);
        }
        if pos + 1 >= 16 {
            return None;
        }
        address[pos] = (value >> 8) as u8;
        address[pos + 1] = value as u8;
        pos += 2;
        if next == 0 {
            break;
        }
        if next != b':' {
            return None;
        }
        ptr += used + 1;
    }
    (pos == 16).then_some(address)
}

/// `QIPAddressUtils::toString` for IPv6: the first longest run of two or more zero words is
/// `::`, and a mapped (`::ffff:`) or compatible (`::` then a non-zero word that is not just its
/// last byte) address ends in dotted IPv4.
fn ip6_to_string(address: &[u8; 16]) -> String {
    let mut out = String::new();
    let mut embedded_ip4 = false;
    if address[..10].iter().all(|&b| b == 0) {
        if address[10] == 0xff && address[11] == 0xff {
            embedded_ip4 = true;
        } else if address[10] == 0 && address[11] == 0 {
            if address[12] != 0 || address[13] != 0 || address[14] != 0 {
                embedded_ip4 = true;
            } else if address[15] == 0 {
                return "::".into();
            }
        }
    }
    let mut zero_run_length = 0;
    let mut zero_run_offset: i32 = 0;
    let mut i = 0;
    while i < 16 {
        if address[i] == 0 && address[i + 1] == 0 {
            let mut j = i;
            while j < 16 && address[j] == 0 && address[j + 1] == 0 {
                j += 2;
            }
            if j - i > zero_run_length {
                zero_run_length = j - i;
                zero_run_offset = i as i32;
                i = j;
                continue;
            }
        }
        i += 2;
    }
    if zero_run_length < 4 {
        zero_run_offset = -1;
    } else if zero_run_offset == 0 {
        out.push(':');
    }
    let mut i = 0;
    while i < 16 {
        if i as i32 == zero_run_offset {
            out.push(':');
            i += zero_run_length;
            continue;
        }
        if i == 12 && embedded_ip4 {
            out.push_str(&ip4_to_string(u32::from_be_bytes([address[12], address[13], address[14], address[15]])));
            return out;
        }
        out.push_str(&format!("{:x}", u16::from_be_bytes([address[i], address[i + 1]])));
        if i != 14 {
            out.push(':');
        }
        i += 2;
    }
    out
}

/// `parseIpFuture`: `[v`, one hex digit (written upper case), `.`, then letters, digits and
/// `!$&'()*+,;=:-._~`. `units` includes the brackets.
fn parse_ip_future(units: &[u16]) -> Option<String> {
    if units[3] != u16::from(b'.') {
        return None;
    }
    let version = char::from_u32(u32::from(units[2])).filter(char::is_ascii_hexdigit)?;
    let mut host = format!("[v{}.", version.to_ascii_uppercase());
    for &u in &units[4..units.len() - 1] {
        let c = char::from_u32(u32::from(u)).filter(|c| c.is_ascii_alphanumeric() || "!$&'()*+,;=:-._~".contains(*c))?;
        host.push(c);
    }
    host.push(']');
    Some(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What Qt 6.11.2 printed for each path (a scratch program calling
    /// `QUrl::fromLocalFile(path).toString()`).
    #[test]
    fn matches_qt() {
        let cases: &[(&str, &str)] = &[
            ("/tmp/a b.png", "file:///tmp/a b.png"),
            ("/tmp/a%b.png", "file:///tmp/a%25b.png"),
            ("/tmp/a#b?c.png", "file:///tmp/a%23b%3Fc.png"),
            ("/tmp/ünï cödé/ß.png", "file:///tmp/ünï cödé/ß.png"),
            ("/tmp/[x]{y}|z^`\"<>\\.png", "file:///tmp/%5Bx%5D%7By%7D%7Cz%5E%60%22%3C%3E%5C.png"),
            ("/tmp/;=+&$!'()*,:@~.png", "file:///tmp/;=+&$!'()*,:@~.png"),
            ("/tmp/\u{1}\u{1f}\u{7f}.png", "file:///tmp/%01%1F%7F.png"),
            ("/tmp/tab\there", "file:///tmp/tab%09here"),
            ("/tmp/new\nline", "file:///tmp/new%0Aline"),
            ("/tmp/\u{80}\u{9f}", "file:///tmp/\u{80}\u{9f}"),
            ("/tmp/\u{a0}x\u{2028}y", "file:///tmp/\u{a0}x\u{2028}y"),
            ("/tmp/\u{feff}", "file:///tmp/\u{feff}"),
            ("/tmp/😀.png", "file:///tmp/😀.png"),
            ("/tmp/a%20b", "file:///tmp/a%2520b"),
            ("/tmp/%%41", "file:///tmp/%25%2541"),
            ("/tmp/a//b/./c/../d", "file:///tmp/a//b/./c/../d"),
            ("", ""),
            ("/", "file:///"),
            ("relative/file.png", "file:relative/file.png"),
            ("file.png", "file:file.png"),
            (".", "file:."),
            ("..", "file:.."),
            ("ab:c", "file:ab:c"),
            ("a/b:c", "file:a/b:c"),
            (":x", "file::x"),
            ("1:x", "file:///1:x"),
            ("c:x", "file:///c:x"),
            ("C:/x.png", "file:///C:/x.png"),
            ("Z:", "file:///Z:"),
            ("é:/x", "file:///é:/x"),
            ("-:x", "file:///-:x"),
            ("a:b:c", "file:///a:b:c"),
            ("//host/share/x.png", "file://host/share/x.png"),
            ("//host", "file://host"),
            ("//", "file://"),
            ("///x", "file://"),
            ("////x", "file://"),
            ("//a/", "file://a/"),
            ("//a//b", "file://a//b"),
            ("//Host/x", "file://host/x"),
            ("//A-B.C_D/x", "file://a-b.c_d/x"),
            ("//ho_st/x", "file://ho_st/x"),
            ("//_a/x", "file://_a/x"),
            ("//a.b./x", "file://a.b./x"),
            ("//1a/x", "file://1a/x"),
            ("//1.a/x", "file://1.a/x"),
            ("//.cache/x", "file:////.cache/x"),
            ("//.", "file:////."),
            ("//a..b/x", "file:////a..b/x"),
            ("//-a/x", "file:////-a/x"),
            ("//a-/x", "file:////a-/x"),
            ("//a.-b/x", "file:////a.-b/x"),
            ("//host:80/x", "file:////host:80/x"),
            ("//host%20/x", "file:////host%2520/x"),
            ("//a b/c", "file:////a b/c"),
            ("//a@b/c", "file:////a@b/c"),
            ("//a~b/c", "file:////a~b/c"),
            ("//a/b?c", "file://a/b%3Fc"),
            ("//1.2.3.4/x", "file://1.2.3.4/x"),
            ("//127.1/x", "file://127.0.0.1/x"),
            ("//0x7f.1/x", "file://127.0.0.1/x"),
            ("//123/x", "file://0.0.0.123/x"),
            ("//0/x", "file://0.0.0.0/x"),
            ("//1.2.3/x", "file://1.2.0.3/x"),
            ("//08/x", "file://08/x"),
            ("//0x1g/x", "file://0x1g/x"),
            ("//256.1.1.1/x", "file://256.1.1.1/x"),
            ("//1.2.3.4.5/x", "file://1.2.3.4.5/x"),
            ("//[::1]/x", "file://[::1]/x"),
            ("//[zz]/x", ""),
            ("//host@ssl/x", "webdavs://host/x"),
            // IPv4 as parseIp4 reads it: white space and "+" before a part, 0b and 0x, a NUL ends it.
            ("// 9", "file://0.0.0.9"),
            ("//\t0x10/x", "file://0.0.0.16/x"),
            ("//+1/x", "file://0.0.0.1/x"),
            ("//-1/x", "file:////-1/x"),
            ("//0b101/x", "file://0.0.0.5/x"),
            ("//1. 2/x", "file://1.0.0.2/x"),
            ("//90\0- {0*|90]", "file://0.0.0.90"),
            ("//\u{7f}1/x", "file:////%7F1/x"),
            // IPv6 as Qt writes it back.
            ("//[::1.2.3.4]/x", "file://[::1.2.3.4]/x"),
            ("//[::0.0.0.5]/x", "file://[::5]/x"),
            ("//[1:2:3:4:5:6:1.2.3.4]/x", "file://[1:2:3:4:5:6:102:304]/x"),
            ("//[1:2:3:4:5:6:7::]/x", "file://[1:2:3:4:5:6:7:0]/x"),
            ("//[1:0:0:2:0:0:3:4]/x", "file://[1::2:0:0:3:4]/x"),
            ("//[ 1::2]/x", "file://[1::2]/x"),
            ("//[1::2 ]/x", ""),
            ("//[1:2:3:4:5:6:7:8::]/x", "file://[1:2:3:4:5:6:7:8]/x"),
            ("//[fe80::1%25eth0]/x", "file://[fe80::1%25eth0]/x"),
            ("//[fe80::1%eth0]/x", ""),
            ("//[::ffff:01.2.3.4]/x", ""),
            // IPvFuture.
            ("//[v1.x]/x", "file://[v1.x]/x"),
            ("//[vf.!$&'()*+,;=:-._~]", "file://[vF.!$&'()*+,;=:-._~]"),
            ("//[V1.x]/x", ""),
            ("//[v12.x]/x", ""),
            // Labels.
            ("//xn--a/x", "file://xn--a/x"),
            ("//xn--/x", "file:////xn--/x"),
            ("//ab--cd/x", "file://ab--cd/x"),
            // The drive rule counts UTF-16 units.
            ("😀:x", "file:😀:x"),
            ("é:/x", "file:///é:/x"),
            ("//host@SSL", "webdavs://host"),
            ("//@SSL/x", "webdavs:///x"),
        ];
        for &(path, expected) in cases {
            assert_eq!(from_local_file(path), expected, "{path:?}");
        }
        let long = format!("//{}/x", "a".repeat(63));
        assert_eq!(from_local_file(&long), format!("file:{long}").replacen("file://", "file://", 1));
        let too_long = format!("//{}/x", "a".repeat(64));
        assert_eq!(from_local_file(&too_long), format!("file://{too_long}"));
    }

    #[test]
    fn every_ascii_character() {
        // Qt 6.11.2: these are encoded, everything else from U+0001 to U+007F is kept.
        let encoded = "\"#%<>?[\\]^`{|}\u{7f}";
        for code in 1u8..128 {
            let c = char::from(code);
            let expected = if c < ' ' || encoded.contains(c) { format!("%{code:02X}") } else { c.to_string() };
            assert_eq!(from_local_file(&format!("/t/{c}")), format!("file:///t/{expected}"));
        }
    }
}
