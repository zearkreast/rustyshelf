//! API keys as the Electron app keeps them in the settings: encrypted for this Windows user, so a copied or shared
//! RustyShelf folder doesn't carry a usable key. The Electron app seals a key with Chromium's own scheme
//! (AES-256-GCM under a key that Windows' DPAPI protects, kept in its "Local State" file); this reads and writes
//! the same, so both apps open each other's keys. Where that key isn't to be had (the Electron app never ran
//! here, or not Windows) a key stays as it is, as the Electron app leaves it without a keyring.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;

use crate::emucfg::Var;

/// What Chromium writes before a sealed text, and the length of the nonce after it.
const VERSION: &[u8] = b"v10";
const NONCE: usize = 12;

#[cfg(windows)]
mod dpapi {
    use std::ffi::c_void;

    #[repr(C)]
    struct Blob {
        len: u32,
        data: *mut u8,
    }

    #[link(name = "crypt32")]
    unsafe extern "system" {
        fn CryptUnprotectData(data: *const Blob, description: *mut *mut u16, entropy: *const Blob, reserved: *mut c_void, prompt: *const c_void, flags: u32, out: *mut Blob) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }

    /// What DPAPI protected for this Windows user, opened.
    pub fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
        let input = Blob { len: data.len() as u32, data: data.as_ptr().cast_mut() };
        let mut out = Blob { len: 0, data: std::ptr::null_mut() };
        // SAFETY: `input` points at `data` for the call; `out` is filled by Windows with memory it allocated, copied
        // and freed here
        unsafe {
            if CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null(), 0, &mut out) == 0 || out.data.is_null() {
                return None;
            }
            let opened = std::slice::from_raw_parts(out.data, out.len as usize).to_vec();
            LocalFree(out.data.cast());
            Some(opened)
        }
    }
}

/// The key the Electron app seals with on this computer: from its "Local State", opened by Windows for this user.
/// None where there's none.
pub fn key(var: Var) -> Option<[u8; 32]> {
    let state = crate::js::join(&[&crate::emucfg::get(var, "APPDATA")?, "RustyShelf", "Local State"]);
    let state = crate::files::read_json(std::path::Path::new(&state))?;
    let wrapped = base64::engine::general_purpose::STANDARD.decode(state["os_crypt"]["encrypted_key"].as_str()?).ok()?;
    let wrapped = wrapped.strip_prefix(b"DPAPI")?;
    #[cfg(windows)]
    return dpapi::unprotect(wrapped)?.try_into().ok();
    #[cfg(not(windows))]
    {
        let _ = wrapped;
        None
    }
}

/// A text sealed under `key`, as the settings keep it (base64).
pub fn seal(text: &str, key: &[u8; 32], nonce: [u8; NONCE]) -> Option<String> {
    let sealed = Aes256Gcm::new(key.into()).encrypt(Nonce::from_slice(&nonce), text.as_bytes()).ok()?;
    Some(base64::engine::general_purpose::STANDARD.encode([VERSION, &nonce, &sealed].concat()))
}

/// A sealed text opened; None when it wasn't sealed under `key` (a folder moved to another computer).
pub fn unseal(sealed: &str, key: &[u8; 32]) -> Option<String> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(sealed).ok()?;
    let rest = bytes.strip_prefix(VERSION).filter(|r| r.len() > NONCE)?;
    let opened = Aes256Gcm::new(key.into()).decrypt(Nonce::from_slice(&rest[..NONCE]), &rest[NONCE..]).ok()?;
    String::from_utf8(opened).ok()
}

/// A nonce for sealing: never the same twice (the time to the nanosecond, this program, and a count).
pub fn nonce() -> [u8; NONCE] {
    use sha2::Digest;
    static COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let mut hash = sha2::Sha256::new();
    hash.update(now.to_le_bytes());
    hash.update(std::process::id().to_le_bytes());
    hash.update(COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed).to_le_bytes());
    hash.finalize()[..NONCE].try_into().unwrap_or([0; NONCE])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_what_node_sealed_and_its_own() {
        // sealed in Node as Chromium does: "v10", a 12-byte nonce, AES-256-GCM's ciphertext and tag
        let key: [u8; 32] = std::array::from_fn(|i| i as u8);
        assert_eq!(unseal(NODE_SEALED, &key).as_deref(), Some("made-up key 123"));
        let ours = seal("another key", &key, [7; NONCE]).unwrap();
        assert_eq!(unseal(&ours, &key).as_deref(), Some("another key"));
        assert_eq!(unseal(&ours, &[1; 32]), None);
        assert_eq!(unseal("not sealed", &key), None);
        assert_ne!(nonce(), [0; NONCE]);
    }

    const NODE_SEALED: &str = "djEwAQIDBAUGBwgJCgsMaIs+sMHhgKYnxxpnISHZwROhE1hDAh8A/yRDkYzbIw==";
}
