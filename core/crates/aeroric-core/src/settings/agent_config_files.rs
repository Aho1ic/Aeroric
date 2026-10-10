//! Explicit-path agent-config IO. Path selection and settings loading belong
//! elsewhere; the checked host and explicit legacy authority seams share IO.

use std::fs;
use std::path::Path;

use crate::context::CoreContext;
use crate::storage::atomic::write_legacy_atomic;

/// Preserve the shipping exists/None rule, including dangling symlinks.
pub fn read(path: &Path) -> Result<Option<String>, String> {
    if !path.exists() {
        return Ok(None);
    }
    fs::read_to_string(path)
        .map(Some)
        .map_err(|error| error.to_string())
}

/// Recheck authority at the IO boundary; host adapters also admit writes before
/// loading settings. This is not a transactional mid-write revocation guarantee.
pub fn write(context: &CoreContext, path: &Path, content: &str) -> Result<(), String> {
    context.require_write().map_err(|error| error.to_string())?;
    write_legacy(path, content)
}

/// The existing Tauri writer's semantics: create parents, temp/write/sync/rename,
/// inherited new-file permissions and cleanup on failure. No read-repair/backup.
pub fn write_legacy(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    write_legacy_atomic(path, content)
}
