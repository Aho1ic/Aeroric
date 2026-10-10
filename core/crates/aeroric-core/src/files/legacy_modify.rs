//! Exact legacy existing-entry policy and rename, not native read/write rules.
//! The actual OS Trash boundary remains with the caller.

use super::legacy_entry::validate_entry_name;
use std::path::{Path, PathBuf};

fn ensure_not_protected_path(path: &Path, root: &Path, action: &str) -> Result<(), String> {
    if path == root {
        return Err(format!("Cannot {} the project root", action));
    }
    if let Ok(rel) = path.strip_prefix(root) {
        if let Some(first) = rel.components().next() {
            if let Some(name) = first.as_os_str().to_str() {
                if PROTECTED_FIRST_SEGMENTS
                    .iter()
                    .any(|protected| protected.eq_ignore_ascii_case(name))
                {
                    return Err(format!("Cannot {} protected directory: {}", action, name));
                }
            }
        }
    }
    Ok(())
}

/// First-segment names under the project root that are never deletable through this command.
const PROTECTED_FIRST_SEGMENTS: &[&str] = &[".git", ".aeroric"];

/// Validate a deletion target. Unlike `validate_path_within`, the target itself is NOT
/// canonicalized — only its parent — so symlinks are moved to trash as themselves rather than
/// following through to the link target. Also enforces a denylist on the first segment under
/// the project root.
pub fn validate_existing_path_for_delete(
    target: &str,
    allowed_root: &str,
) -> Result<std::path::PathBuf, String> {
    let target_path = Path::new(target);

    if !target_path.is_absolute() {
        return Err("Path must be absolute".to_string());
    }

    let file_name = target_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "Invalid file name".to_string())?;

    let parent = target_path
        .parent()
        .ok_or_else(|| "Cannot resolve parent directory".to_string())?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|e| format!("Cannot resolve parent directory: {}", e))?;
    let canonical_root = Path::new(allowed_root)
        .canonicalize()
        .map_err(|e| format!("Cannot resolve root directory: {}", e))?;

    if !canonical_parent.starts_with(&canonical_root) {
        return Err("Path is outside the allowed directory".to_string());
    }

    let resolved = canonical_parent.join(file_name);

    if resolved.symlink_metadata().is_err() {
        return Err("Path does not exist".to_string());
    }

    ensure_not_protected_path(&resolved, &canonical_root, "delete")?;
    Ok(resolved)
}

pub fn validate_existing_path_for_modify(
    target: &str,
    allowed_root: &str,
    action: &str,
) -> Result<(PathBuf, PathBuf), String> {
    let target_path = Path::new(target);

    if !target_path.is_absolute() {
        return Err("Path must be absolute".to_string());
    }

    let file_name = target_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "Invalid file name".to_string())?;

    let parent = target_path
        .parent()
        .ok_or_else(|| "Cannot resolve parent directory".to_string())?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|e| format!("Cannot resolve parent directory: {}", e))?;
    let canonical_root = Path::new(allowed_root)
        .canonicalize()
        .map_err(|e| format!("Cannot resolve root directory: {}", e))?;

    if !canonical_parent.starts_with(&canonical_root) {
        return Err("Path is outside the allowed directory".to_string());
    }

    let resolved = canonical_parent.join(file_name);
    if resolved.symlink_metadata().is_err() {
        return Err("Path does not exist".to_string());
    }
    ensure_not_protected_path(&resolved, &canonical_root, action)?;
    Ok((resolved, canonical_root))
}

/// Preserve the original legacy name/guard/destination policy and rename IO.
pub fn rename_path(path: String, new_name: String, project_path: String) -> Result<(), String> {
    let name = new_name.trim();
    validate_entry_name(name)?;
    let (source, _) = validate_existing_path_for_modify(&path, &project_path, "rename")?;
    let parent = source
        .parent()
        .ok_or_else(|| "Cannot resolve parent directory".to_string())?;
    let destination = parent.join(name);
    if destination.exists() {
        return Err("A file or folder with that name already exists".to_string());
    }
    std::fs::rename(&source, &destination).map_err(|e| e.to_string())
}
