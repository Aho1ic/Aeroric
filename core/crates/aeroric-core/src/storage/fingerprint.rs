//! "Are the bytes I am about to cover still the bytes I saw?" — ADR-012 decision 2.
//!
//! `runtime.json` holds a monotonic `revision`, but a revision only answers that
//! question for writers that *read* it. The shipping client has no revision anywhere in
//! `projects/*/tasks.json`, so during the migration there is a window where a legacy
//! save and a core save both consider themselves current. The fingerprint closes it:
//! every write carries the `(sha256, size)` of the bytes it replaced, and the next
//! write may only go through while those bytes are still on disk.
//!
//! Two baselines, one rule:
//!
//! - [`Cover::Observed`] — the caller read the file through this service and holds the
//!   fingerprint it saw. This is the ordinary path, and it is what makes a
//!   read-modify-write safe against a concurrent legacy save.
//! - [`Cover::LastCoreWrite`] — the caller read nothing and asks to overwrite anyway.
//!   The only thing it may overwrite is exactly what this core last wrote, as recorded
//!   in `runtime.json`. A writer that has never seen the file cannot claim a fresh
//!   baseline, so a legacy edit that landed after our last write is still caught.
//!
//! The order is data-then-table, deliberately. If the process dies between them, the
//! table is one generation behind the disk and the *next* write is refused — a noisy
//! failure a caller clears by reloading. The reverse order would let the table promise
//! bytes that never reached the disk, which is the failure that loses data.

use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::context::CoreContext;
use crate::storage::atomic::{self, StorageError};
use crate::storage::runtime;

/// What a caller is asking to be allowed to cover.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cover {
    /// The bytes this caller read (`None` when the file was not there yet).
    Observed(Option<Fingerprint>),
    /// Whatever `runtime.json` says this core last produced.
    LastCoreWrite,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Fingerprint {
    /// Lowercase hex, 64 chars.
    pub sha256: String,
    /// Byte count, kept alongside the digest so a "same hash, different file" bug in
    /// either writer is visible in the refusal message rather than silently accepted.
    pub size: u64,
}

impl Fingerprint {
    pub fn of(bytes: &[u8]) -> Self {
        let digest = Sha256::digest(bytes);
        Fingerprint {
            sha256: digest.iter().map(|b| format!("{b:02x}")).collect(),
            size: bytes.len() as u64,
        }
    }

    pub fn matches(&self, bytes: &[u8]) -> bool {
        self == &Fingerprint::of(bytes)
    }
}

impl std::fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}…/{}B", &self.sha256[..12], self.size)
    }
}

/// The digest of what is on disk right now. A missing file has no fingerprint rather
/// than the digest of an empty file: "absent" and "empty" are different states here, and
/// blurring them would let a deletion pass as an unchanged document.
pub fn of_file(path: &Path) -> Result<Option<Fingerprint>, StorageError> {
    match atomic::read_bytes(path)? {
        atomic::ReadOutcome::Absent => Ok(None),
        atomic::ReadOutcome::Found(bytes) => Ok(Some(Fingerprint::of(&bytes))),
    }
}

/// The name a file is fingerprinted under: its path relative to the data directory, so
/// the table survives a moved root, stays short enough to read in `runtime.json`, and has
/// exactly one spelling across services.
pub fn table_key(context: &CoreContext, file: &Path) -> Result<String, StorageError> {
    let relative =
        file.strip_prefix(context.paths.data_dir())
            .map_err(|_| StorageError::Unreadable {
                path: file.to_path_buf(),
                reason: "is not inside the data directory, so it has no fingerprint key".into(),
            })?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

/// Refuse unless the current bytes match the baseline `cover` entitles the caller to
/// overwrite. Returns the fingerprint of the bytes found on disk.
pub fn check_cover(
    context: &CoreContext,
    key: &str,
    path: &Path,
    cover: &Cover,
) -> Result<Option<Fingerprint>, StorageError> {
    let on_disk = of_file(path)?;
    let baseline = match cover {
        Cover::Observed(baseline) => baseline.clone(),
        Cover::LastCoreWrite => match runtime::recorded_cover(context, key)? {
            // Some(Some(..)) = last write produced bytes; Some(None) = last write removed
            // the file. Neither may be silently replaced by a foreign edit.
            Some(recorded) => recorded,
            // This core has never published this file, so it has no cover to defend.
            None => on_disk.clone(),
        },
    };
    if on_disk != baseline {
        return Err(StorageError::ContentDiverged {
            path: path.to_path_buf(),
            expected: baseline
                .map(|f| f.to_string())
                .unwrap_or_else(|| "absent".into()),
            found: on_disk
                .map(|f| f.to_string())
                .unwrap_or_else(|| "absent".into()),
        });
    }
    Ok(on_disk)
}

impl StorageError {
    /// ADR-011: a diverged file is a `staleWrite`, which maps onto the existing
    /// `REVISION_CONFLICT` — there is no new protocol code, and a client already knows
    /// what to do with that one: reload instead of retry.
    pub fn is_stale_write(&self) -> bool {
        matches!(self, StorageError::ContentDiverged { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_support::writer;

    #[test]
    fn the_digest_is_sha256_at_every_block_boundary() {
        // These hex strings come from an independent implementation, not from this one.
        // `sha2` is not new to the product (src-tauri already ships 0.10.9), so what is
        // being pinned here is the wiring: hex casing, length, and that `size` is bytes
        // rather than characters.
        let vectors: &[(&str, &str)] = &[
            (
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                "",
            ),
            (
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
                "abc",
            ),
            (
                "21cbb9adbb630b2adb2413d6dc8f948e5426eac93df6fbf1942d527138ae6ac9",
                "\u{5de5}\u{4f5c}\u{2713}",
            ),
        ];
        for (expected, text) in vectors {
            let found = Fingerprint::of(text.as_bytes());
            assert_eq!(&found.sha256, expected, "{text:?}");
            assert_eq!(found.size, text.len() as u64, "{text:?}");
        }
        // 56/63/64/65 bytes walk through the padding and the second-block boundary.
        for length in [56usize, 63, 64, 65, 130] {
            let bytes = vec![b'a'; length];
            let found = Fingerprint::of(&bytes);
            assert_eq!(found.sha256.len(), 64);
            assert!(
                found
                    .sha256
                    .chars()
                    .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
                "{}",
                found.sha256
            );
            assert_eq!(found.size, length as u64);
            assert!(found.matches(&bytes));
            let mut edited = bytes.clone();
            edited[length - 1] = b'b';
            assert!(
                !found.matches(&edited),
                "a one-byte edit must move the digest"
            );
        }
    }

    #[test]
    fn an_absent_file_is_not_the_empty_file() {
        let (root, _) = writer("absent");
        let path = root.join("nothing.json");
        assert_eq!(of_file(&path).unwrap(), None);
        std::fs::write(&path, b"").unwrap();
        let empty = of_file(&path).unwrap().expect("an empty file is present");
        assert_eq!(empty.size, 0);
        assert_ne!(
            Cover::Observed(None),
            Cover::Observed(Some(empty)),
            "a delete-then-write must not read as an unchanged document"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_diverged_file_is_refused_and_names_both_fingerprints() {
        let (root, context) = writer("diverge");
        let path = root.join("tasks.json");
        std::fs::write(&path, b"[{\"id\":\"a\"}]").unwrap();
        let seen = of_file(&path).unwrap().expect("just wrote it");
        // Somebody else gets there first.
        std::fs::write(&path, b"[{\"id\":\"b\"}]").unwrap();
        let error = check_cover(
            &context,
            "tasks",
            &path,
            &Cover::Observed(Some(seen.clone())),
        )
        .expect_err("the cover is gone");
        assert!(error.is_stale_write(), "{error:?}");
        let text = error.to_string();
        assert!(text.contains(&seen.sha256[..12]), "{text}");
        assert!(text.contains("tasks.json"), "{text}");
        std::fs::remove_dir_all(root).ok();
    }
}
