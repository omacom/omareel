//! The ids split and inserted clips and new zooms get: `"c-"`, `"m-"` or `"z-"` and
//! `QUuid::createUuid().toString(QUuid::WithoutBraces)`.

use std::sync::atomic::{AtomicU64, Ordering};

/// A random (version 4) UUID as `QUuid::createUuid().toString(QUuid::WithoutBraces)` writes it:
/// lower-case hex in 8-4-4-4-12 groups, from the system's random source like Qt's
/// `QRandomGenerator::system()`.
pub fn random_uuid() -> String {
    let mut bytes = [0u8; 16];
    if !fill_random(&mut bytes) {
        // No random source at all: still distinct within the process.
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        bytes[..8].copy_from_slice(&nanos.to_le_bytes());
        bytes[8..].copy_from_slice(&(COUNTER.fetch_add(1, Ordering::Relaxed) ^ u64::from(std::process::id())).to_le_bytes());
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}

fn fill_random(bytes: &mut [u8]) -> bool {
    let mut filled = 0;
    while filled < bytes.len() {
        // SAFETY: getrandom writes at most the given length into the buffer.
        let n = unsafe { libc::getrandom(bytes[filled..].as_mut_ptr().cast(), bytes.len() - filled, 0) };
        if n <= 0 {
            break;
        }
        filled += n as usize;
    }
    if filled == bytes.len() {
        return true;
    }
    use std::io::Read;
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(bytes)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuids_look_like_quuid() {
        let id = random_uuid();
        assert_eq!(id.len(), 36);
        let groups: Vec<&str> = id.split('-').collect();
        assert_eq!(groups.iter().map(|g| g.len()).collect::<Vec<_>>(), [8, 4, 4, 4, 12]);
        assert!(id.chars().all(|c| c == '-' || c.is_ascii_digit() || ('a'..='f').contains(&c)));
        assert!(groups[2].starts_with('4'));
        assert!("89ab".contains(&groups[3][..1]));
        assert_ne!(random_uuid(), id);
    }
}
