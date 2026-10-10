//! Persistence that either fully replaces a file or leaves the old one alone.
//!
//! The layout, the naming and the permission rules are inherited from
//! `src-tauri/src/storage.rs` on purpose: until Task P2-05 hands the writer role over,
//! the shipping client and the core are reading the same bytes, and a "cleaner" file
//! format on one side is a parse failure on the other.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// How private a replaced file must be. Everything under the data directory that can
/// hold a token, a session transcript or a hook fragment is [`FileMode::Private`];
/// `Inherited` exists only for files the shipping client already creates world-readable
/// and where changing the mode would be a behaviour change outside this task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileMode {
    Private,
    Inherited,
}

/// Distinguishing "no file" from "file we cannot understand" is the whole point: the
/// shipping client's settings path treats both as "start from defaults" and then writes
/// defaults over the user's file (`app_settings/load_save.rs:442-458`), which is data
/// loss with a plausible error message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadOutcome<T> {
    Found(T),
    Absent,
}

impl<T> ReadOutcome<T> {
    pub fn found(self) -> Option<T> {
        match self {
            ReadOutcome::Found(value) => Some(value),
            ReadOutcome::Absent => None,
        }
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> ReadOutcome<U> {
        match self {
            ReadOutcome::Found(value) => ReadOutcome::Found(f(value)),
            ReadOutcome::Absent => ReadOutcome::Absent,
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum StorageError {
    #[error("{path:?} holds unreadable data ({reason}); refusing to overwrite it — recover it by hand or pass an explicit force")]
    Corrupt { path: PathBuf, reason: String },
    #[error("cannot read {path:?}: {reason}")]
    Unreadable { path: PathBuf, reason: String },
    #[error("cannot write {path:?}: {reason}")]
    WriteFailed { path: PathBuf, reason: String },
    #[error("could not put the new file in place at {path:?}: {reason}")]
    ReplaceFailed { path: PathBuf, reason: String },
    #[error("{path:?} is not writable by this process; it is left untouched")]
    PermissionDenied { path: PathBuf },
    #[error("{path:?} was written by a newer core (schema {found}, this build understands {supported}); refusing to downgrade it")]
    UnsupportedSchema {
        path: PathBuf,
        found: u32,
        supported: u32,
    },
    #[error("refusing to overwrite {path:?}: it holds {found}, this writer was entitled to cover {expected} — reload it instead of retrying")]
    ContentDiverged {
        path: PathBuf,
        expected: String,
        found: String,
    },
    #[error("invalid identifier: {0}")]
    InvalidId(#[from] crate::paths::PathError),
    #[error(transparent)]
    Lease(#[from] crate::storage_lock::LeaseError),
    #[error("no write permission: {0}")]
    WritePermission(#[from] crate::context::StorageWritePermission),
}

/// The shipping client's non-private atomic mechanism, kept byte/error/mode
/// compatible. Unlike `write_atomic`, this does not create parents, enforce a
/// private mode, add a backup, or translate the OS error. Callers own authority.
pub fn write_legacy_atomic(path: &Path, content: &str) -> Result<(), String> {
    let uid = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = path.with_file_name(format!(".{file_name}.{uid}.tmp"));
    {
        let mut file = fs::File::create(&tmp).map_err(|e| e.to_string())?;
        if let Err(error) = file.write_all(content.as_bytes()) {
            drop(file);
            let _ = fs::remove_file(&tmp);
            return Err(error.to_string());
        }
        if let Err(error) = file.sync_all() {
            drop(file);
            let _ = fs::remove_file(&tmp);
            return Err(error.to_string());
        }
    }
    finish_legacy_atomic_rename(&tmp, path)
}

/// Shared with the legacy private writer; a failed rename must not leak a copy.
pub fn finish_legacy_atomic_rename(tmp: &Path, path: &Path) -> Result<(), String> {
    match fs::rename(tmp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(tmp);
            Err(error.to_string())
        }
    }
}

/// Write `bytes` so a reader never sees a half file: temp in the same directory, flush,
/// durability, then rename. On unix the new file is created 0600 for
/// [`FileMode::Private`] rather than tightened after the fact, because the window
/// between create and chmod is exactly where a readable token gets read.
pub fn write_atomic(path: &Path, bytes: &[u8], mode: FileMode) -> Result<(), StorageError> {
    let directory = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(<PathBuf as From<_>>::from)
        .unwrap_or_else(|| PathBuf::from("."));
    fs::create_dir_all(&directory).map_err(|e| StorageError::WriteFailed {
        path: path.to_path_buf(),
        reason: format!("cannot create {}: {e}", directory.display()),
    })?;
    let temp = temporary_name(&directory, path);
    let created = match mode {
        FileMode::Private => create_private(&temp),
        FileMode::Inherited => OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(|e| io_error(path, e, "create")),
    };
    let mut file = created?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.flush()) {
        discard(&temp);
        return Err(io_error(path, error, "write"));
    }
    if let Err(error) = file.sync_all() {
        discard(&temp);
        return Err(io_error(path, error, "sync"));
    }
    drop(file);
    if let Err(error) = fs::rename(&temp, path) {
        discard(&temp);
        return Err(io_error(path, error, "rename"));
    }
    // A rename survives a crash only if the directory entry is durable too. Not every
    // platform lets us fsync a directory, and failing to do it is not a failed write:
    // the previous file is still intact, which is the property callers rely on.
    let _ = sync_dir(&directory);
    Ok(())
}

#[cfg(unix)]
fn sync_dir(directory: &Path) -> std::io::Result<()> {
    File::open(directory)?.sync_all()
}

#[cfg(not(unix))]
fn sync_dir(_directory: &Path) -> std::io::Result<()> {
    // Windows has no equivalent of flushing a directory entry; the rename itself is
    // durable enough for our purpose, and the platform adapter owns anything stronger.
    Ok(())
}

/// Keep one generation of the file being replaced, then replace it.
///
/// This is the difference between "the writer crashed" and "the writer wrote the wrong
/// thing and now the old data is gone": with a backup a wrong snapshot is recoverable,
/// and the acceptance for P2-02 asks for exactly that. A first-ever write keeps nothing.
pub fn replace_with_backup(
    path: &Path,
    bytes: &[u8],
    mode: FileMode,
) -> Result<bool, StorageError> {
    let existed = path.exists();
    if existed {
        let backup = backup_path(path);
        fs::copy(path, &backup).map_err(|e| StorageError::WriteFailed {
            path: path.to_path_buf(),
            reason: format!("could not keep a copy of the previous file: {e}"),
        })?;
        #[cfg(unix)]
        if mode == FileMode::Private {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&backup, fs::Permissions::from_mode(0o600));
        }
    }
    write_atomic(path, bytes, mode)?;
    Ok(existed)
}

/// Read a whole file, reporting absence separately from failure.
pub fn read_bytes(path: &Path) -> Result<ReadOutcome<Vec<u8>>, StorageError> {
    match fs::read(path) {
        Ok(bytes) => Ok(ReadOutcome::Found(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ReadOutcome::Absent),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            Err(StorageError::PermissionDenied {
                path: path.to_path_buf(),
            })
        }
        Err(error) => Err(StorageError::Unreadable {
            path: path.to_path_buf(),
            reason: error.to_string(),
        }),
    }
}

/// Refuse to clobber what cannot be understood.
///
/// Absent is fine, and so is anything that still parses: the point is that an
/// unreadable file must never be replaced by a plausible-looking write. The shipping
/// client does exactly that to `settings.json` today (`load_save.rs:442-458` reads a
/// corrupt file as defaults, then any later save persists those defaults), and the
/// user's only copy of their configuration is gone with no trace.
pub fn ensure_replaceable(path: &Path) -> Result<(), StorageError> {
    match read_json(path)? {
        ReadOutcome::Absent => Ok(()),
        ReadOutcome::Found(_) => Ok(()),
    }
}

/// Parse JSON without letting a corrupt file look like an empty one.
pub fn read_json(path: &Path) -> Result<ReadOutcome<serde_json::Value>, StorageError> {
    read_bytes(path)
        .map(|outcome| outcome.map(|bytes| parse_json(path, &bytes)))
        .and_then(|outcome| match outcome {
            ReadOutcome::Found(value) => value.map(ReadOutcome::Found),
            ReadOutcome::Absent => Ok(ReadOutcome::Absent),
        })
}

fn parse_json(path: &Path, bytes: &[u8]) -> Result<serde_json::Value, StorageError> {
    serde_json::from_slice(bytes).map_err(|error| StorageError::Corrupt {
        path: path.to_path_buf(),
        reason: error.to_string(),
    })
}

fn create_private(temp: &Path) -> Result<File, StorageError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(temp)
            .map_err(|e| io_error(temp, e, "create"))
    }
    #[cfg(not(unix))]
    {
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(temp)
            .map_err(|e| io_error(temp, e, "create"))?;
        Ok(file)
    }
}

/// Same shape as the shipping client's temp names (`.name.<pid>-<nanos>.tmp`) so a
/// directory listing looks the same from either side, and so [`remove_stale_temps`]
/// recognises what a crashed writer left behind.
fn temporary_name(directory: &Path, target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "aeroric.tmp".to_string());
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    directory.join(format!(".{name}.{}-{nanos}.tmp", std::process::id()))
}

/// Remove temp files a crashed writer left in `directory`.
///
/// Only files older than `max_age_ms` are touched: the lease means there is one writer
/// at a time, but a slow rename on a spinning disk must not be deleted out from under a
/// live process that reacquired the lease after a restart.
pub fn remove_stale_temps(directory: &Path, now_ms: u64, max_age_ms: u64) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut removed = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !(name.starts_with('.') && name.ends_with(".tmp")) {
            continue;
        }
        let stamp = match name
            .rsplit_once('-')
            .and_then(|(_, rest)| rest.strip_suffix(".tmp"))
        {
            Some(digits) => digits.parse::<u128>().ok(),
            None => None,
        };
        let age = stamp.and_then(|ns| {
            let now_ns = u128::from(now_ms) * 1_000;
            now_ns.checked_sub(ns).map(|delta| (delta / 1_000) as u64)
        });
        let stale = age.is_none_or(|age| age > max_age_ms);
        if stale {
            let path = entry.path();
            if fs::remove_file(&path).is_ok() {
                removed.push(path);
            }
        }
    }
    removed
}

fn discard(temp: &Path) {
    let _ = fs::remove_file(temp);
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension(format!(
        "{}.bak",
        path.extension().and_then(|e| e.to_str()).unwrap_or("dat")
    ))
}

fn io_error(path: &Path, error: std::io::Error, stage: &str) -> StorageError {
    let reason = format!("{stage}: {error}");
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        return StorageError::PermissionDenied {
            path: path.to_path_buf(),
        };
    }
    StorageError::WriteFailed {
        path: path.to_path_buf(),
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_dir(tag: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("aeroric-atomic-{tag}-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn a_private_file_is_created_private() {
        let dir = temp_dir("mode");
        let file = dir.join("settings.json");
        write_atomic(&file, b"{}", FileMode::Private).expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "unexpected mode {mode:#o}");
        }
        assert_eq!(fs::read(&file).unwrap(), b"{}");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn an_existing_file_is_replaced_not_truncated() {
        let dir = temp_dir("replace");
        let file = dir.join("projects.json");
        write_atomic(&file, b"[{\"id\":\"a\"}]", FileMode::Private).unwrap();
        let kept = replace_with_backup(&file, b"[]", FileMode::Private).unwrap();
        assert!(kept, "the previous file existed, so a backup was kept");
        assert_eq!(fs::read(&file).unwrap(), b"[]");
        assert!(backup_path(&file).exists(), "the old bytes must survive");
        let first = replace_with_backup(&dir.join("new.json"), b"x", FileMode::Private).unwrap();
        assert!(!first, "a first write keeps no backup");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn corrupt_json_is_a_named_error_and_absence_is_not() {
        let dir = temp_dir("corrupt");
        let broken = dir.join("tasks.json");
        fs::write(&broken, b"{ not json").unwrap();
        let error = read_json(&broken).expect_err("corrupt must fail");
        assert!(matches!(error, StorageError::Corrupt { .. }), "{error:?}");
        assert!(
            error.to_string().contains("refusing to overwrite"),
            "the message must tell the operator what not to do: {error}"
        );
        assert!(
            fs::read(&broken).is_ok(),
            "reading must not damage the file"
        );
        assert_eq!(
            read_json(&dir.join("missing.json")).unwrap(),
            ReadOutcome::Absent
        );
        fs::write(dir.join("empty.json"), b"").unwrap();
        assert!(
            matches!(
                read_json(&dir.join("empty.json")).err(),
                Some(StorageError::Corrupt { .. })
            ),
            "an empty file is not an empty object"
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_failed_write_leaves_no_temp_file_behind() {
        let dir = temp_dir("cleanup");
        let target = dir.join("ok.json");
        // A directory as the rename target makes the final step fail on every unix.
        let blocked = dir.join("blocked.json");
        fs::create_dir_all(&blocked).unwrap();
        let error = write_atomic(&blocked, b"{}", FileMode::Private)
            .expect_err("renaming over a directory must fail");
        assert!(
            matches!(
                error,
                StorageError::WriteFailed { .. } | StorageError::ReplaceFailed { .. }
            ),
            "{error:?}"
        );
        write_atomic(&target, b"{}", FileMode::Private).unwrap();
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp files leaked: {leftovers:?}");
        fs::remove_dir_all(dir).ok();
    }

    /// A directory we may not write into must be reported, never quietly "succeeded".
    /// Assumes an unprivileged process: as root the mode bits would not bind and this
    /// test would fail loudly rather than pass for the wrong reason.
    #[test]
    #[cfg(unix)]
    fn an_unusable_directory_is_a_permission_failure_both_ways() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("noaccess");
        let sealed = dir.join("sealed");
        fs::create_dir_all(&sealed).unwrap();
        fs::set_permissions(&sealed, fs::Permissions::from_mode(0o000)).unwrap();
        let target = sealed.join("settings.json");
        let error = write_atomic(&target, b"{}", FileMode::Private)
            .expect_err("a 0000 directory refuses the temp file");
        assert!(
            matches!(error, StorageError::PermissionDenied { .. }),
            "{error:?}"
        );
        let read = read_bytes(&target).expect_err("and it cannot be read either");
        assert!(
            matches!(read, StorageError::PermissionDenied { .. }),
            "{read:?}"
        );
        assert!(
            !sealed.exists() || fs::read_dir(&sealed).is_err(),
            "a refused write must not leave a readable file behind"
        );
        fs::set_permissions(&sealed, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(
            fs::read_dir(&sealed).unwrap().next().is_none(),
            "nothing was created inside"
        );
        fs::remove_dir_all(dir).ok();
    }

    /// The io-failure branch of the same path: a parent that cannot exist at all, so
    /// `create_dir_all` fails the way a full or read-only volume would.
    #[test]
    fn a_write_into_a_non_directory_parent_fails_with_the_path_named() {
        let dir = temp_dir("nopath");
        let blocker = dir.join("blocker");
        fs::write(&blocker, b"not a directory").unwrap();
        let target = blocker.join("projects.json");
        let error = write_atomic(&target, b"[]", FileMode::Private)
            .expect_err("a file cannot hold a child");
        match error {
            StorageError::WriteFailed { path, reason } => {
                assert_eq!(path, target);
                assert!(reason.contains("cannot create"), "{reason}");
            }
            other => panic!("expected a write failure, got {other:?}"),
        }
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "no temp file may survive: {leftovers:?}"
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn stale_temps_are_swept_and_fresh_ones_are_left_alone() {
        let dir = temp_dir("sweep");
        let now_ms = 2_000_000_000_000u64;
        let ten_days_ms = 10 * 86_400_000;
        // Stamps are nanoseconds since the epoch, so these two differ by ten days.
        let old = dir.join(format!(
            ".settings.json.111-{}.tmp",
            (now_ms - ten_days_ms) * 1_000
        ));
        let fresh = dir.join(format!(
            ".settings.json.222-{}.tmp",
            (now_ms - 1_000) * 1_000
        ));
        fs::write(&old, b"").unwrap();
        fs::write(&fresh, b"keep").unwrap();
        let removed = remove_stale_temps(&dir, now_ms, 86_400_000);
        assert_eq!(
            removed,
            vec![old.clone()],
            "only the stale temp goes: {removed:?}"
        );
        assert!(!old.exists());
        assert_eq!(fs::read(&fresh).unwrap(), b"keep");
        fs::remove_dir_all(dir).ok();
    }
}
