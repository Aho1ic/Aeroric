//! The forget barrier (§6.6): a keyed signature of forgotten words, without
//! the words. The key is 32 random bytes in `<dir>/barrier.key` (0600),
//! made on first use; it never leaves the machine and is not in the
//! Keychain (Q-M6).

use std::path::Path;

use hmac::{Hmac, Mac};
use sha2::Sha256;

use super::MemoryError;

pub(super) struct Barrier {
    key: Vec<u8>,
}

impl Barrier {
    pub(super) fn open(dir: &Path) -> Result<Self, MemoryError> {
        let path = dir.join("barrier.key");
        let key = match std::fs::read(&path) {
            Ok(key) if key.len() == 32 => key,
            _ => {
                let mut key = Vec::with_capacity(32);
                key.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
                key.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
                std::fs::write(&path, &key).map_err(|e| MemoryError::Unavailable(e.to_string()))?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
                }
                key
            }
        };
        Ok(Self { key })
    }

    /// Hex HMAC-SHA256 of the normalized words.
    pub(super) fn sign(&self, norm: &str) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.key).expect("HMAC takes any key length");
        mac.update(norm.as_bytes());
        mac.finalize()
            .into_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}
