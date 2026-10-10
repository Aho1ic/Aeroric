//! Legacy absolute-target policy, distinct from native relative/root guards.
//! No settings, project registry, writer admission or directory initialization.

use std::path::Path;

/// Keep the original root spelling: it may be relative or even a file.
/// Fully resolved targets inside that root succeed in both modes, including
/// normalized dot segments and outside aliases that resolve back inside.
/// Otherwise strict mode refuses; legacy read mode checks only the canonical
/// parent and returns its unresolved leaf. This is point-in-time validation,
/// not a symlink-race guarantee or an RPC-controlled permission grant.
pub fn validate_path_within(
    target: &str,
    allowed_root: &str,
    allow_symlink_escape: bool,
) -> Result<std::path::PathBuf, String> {
    let target_path = Path::new(target);

    if !target_path.is_absolute() {
        return Err("Path must be absolute".to_string());
    }

    let canonical_root = Path::new(allowed_root)
        .canonicalize()
        .map_err(|e| format!("Cannot resolve root directory: {}", e))?;

    // Fast path: fully resolve the target (following symlinks). If it still lands
    // inside the project root, accept it as-is.
    if let Ok(canonical_target) = target_path.canonicalize() {
        if canonical_target.starts_with(&canonical_root) {
            return Ok(canonical_target);
        }
    }

    if !allow_symlink_escape {
        return Err("Path is outside the allowed directory".to_string());
    }

    // Fall back to the canonical parent and an unresolved final component.
    // Parent containment is still required, but the leaf need not exist or be
    // a symlink. This preserves legacy reads of external instruction-file links.
    let file_name = target_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "Invalid file name".to_string())?;
    let parent = target_path
        .parent()
        .ok_or_else(|| "Cannot resolve parent directory".to_string())?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|e| format!("Cannot resolve path: {}", e))?;

    if !canonical_parent.starts_with(&canonical_root) {
        return Err("Path is outside the allowed directory".to_string());
    }

    Ok(canonical_parent.join(file_name))
}
