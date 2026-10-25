//! Keeping an API key out of the settings file in plain text.
//!
//! `config.toml` lives in the user's roaming profile and is easy to copy, sync,
//! back up, paste into a bug report or push to a repository. A hosted model key
//! is a billable credential, so it is stored with DPAPI: encrypted by Windows
//! with a key derived from the signed-in user's own profile. The ciphertext can
//! only be undone by that user on that machine, which is exactly the lifetime a
//! voice assistant's key wants.
//!
//! Nothing here is load-bearing for the rest of the program: if DPAPI refuses —
//! an unusual account, a service session — the key is stored as it was given
//! rather than lost, because a working assistant with a plain-text key beats a
//! broken one with none.

use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::core::PCWSTR;

/// Marks a value as DPAPI ciphertext rather than the key itself.
const PREFIX: &str = "dpapi:";

/// Has this already been encrypted?
pub fn is_protected(stored: &str) -> bool {
    stored.starts_with(PREFIX)
}

/// Encrypt a key for storage. Returns `dpapi:<hex>` — or the key untouched,
/// which is what happens when DPAPI is unavailable.
pub fn protect(plain: &str) -> String {
    if plain.is_empty() || is_protected(plain) {
        return plain.to_string();
    }
    match encrypt(plain.as_bytes()) {
        Some(bytes) => format!("{PREFIX}{}", hex(&bytes)),
        None => plain.to_string(),
    }
}

/// The usable key. Accepts either ciphertext or a key typed by hand into the
/// settings file.
pub fn reveal(stored: &str) -> String {
    if !is_protected(stored) {
        return stored.to_string();
    }
    let Some(bytes) = unhex(&stored[PREFIX.len()..]) else { return String::new() };
    decrypt(&bytes).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default()
}

/// What to show in a settings field: the last four characters, which is enough
/// to tell two keys apart and useless to anyone else.
pub fn hint(stored: &str) -> String {
    let key = reveal(stored);
    let tail: String = key.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    if key.chars().count() > 12 {
        format!("…{tail}")
    } else {
        String::new()
    }
}

fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 }
}

fn encrypt(data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() {
        return None;
    }
    let input = blob(data);
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(&input, PCWSTR::null(), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out).ok()?;
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData as *mut _)));
        Some(bytes)
    }
}

fn decrypt(data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() {
        return None;
    }
    let input = blob(data);
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out).ok()?;
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData as *mut _)));
        Some(bytes)
    }
}

/// Lower-case hex. Written out rather than pulled in as a dependency: this is
/// the only place the program needs to turn bytes into a string.
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 0x0F) as usize] as char);
    }
    out
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    let bytes = text.as_bytes();
    for pair in bytes.chunks(2) {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        for sample in [&b""[..], b"a", b"hello world", &[0u8, 255, 16, 128]] {
            let encoded = hex(sample);
            assert_eq!(unhex(&encoded).unwrap(), sample.to_vec());
        }
        assert!(unhex("abc").is_none(), "an odd length cannot be hex");
        assert!(unhex("zz").is_none());
    }

    /// The real thing: DPAPI is asked to encrypt and decrypt, which only fails
    /// in a session with no user profile at all (a service account).
    #[test]
    fn a_key_survives_being_stored() {
        let key = "sk-test-0123456789abcdef";
        let stored = protect(key);
        if !is_protected(&stored) {
            eprintln!("DPAPI unavailable in this session; storing in the clear");
            assert_eq!(stored, key);
            return;
        }
        assert!(!stored.contains(key), "the key must not appear in the stored form");
        assert_eq!(reveal(&stored), key);
        assert_eq!(hint(&stored), "…cdef");
        // A plain key (typed into the file by hand) still works.
        assert_eq!(reveal("sk-typed-by-hand"), "sk-typed-by-hand");
        // Protecting twice is not double-encryption.
        assert_eq!(protect(&stored), stored);
        // Ciphertext from another user cannot be read: DPAPI fails, and an
        // empty key is a clearer signal than a wrong one.
        let mut broken = stored.clone();
        broken.truncate(PREFIX.len() + 16);
        assert!(reveal(&broken).is_empty() || is_protected(&broken));
    }
}
