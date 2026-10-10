//! Legacy clipboard-copy loop and helpers, not project/root admission. The actual
//! command retains target admission; native project-copy is a different contract.

use super::legacy_entry::validate_entry_name;
use std::path::{Path, PathBuf};

pub fn copy_path_recursive(source: &Path, destination: &Path) -> Result<(), String> {
    let metadata = source.symlink_metadata().map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() {
        let target = std::fs::read_link(source).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, destination).map_err(|e| e.to_string())?;
        }
        #[cfg(windows)]
        {
            if source.is_dir() {
                std::os::windows::fs::symlink_dir(target, destination)
                    .map_err(|e| e.to_string())?;
            } else {
                std::os::windows::fs::symlink_file(target, destination)
                    .map_err(|e| e.to_string())?;
            }
        }
        return Ok(());
    }
    if metadata.is_dir() {
        std::fs::create_dir(destination).map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => {
                "A file or folder with that name already exists".to_string()
            }
            _ => e.to_string(),
        })?;
        for entry in std::fs::read_dir(source).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            copy_path_recursive(&entry.path(), &destination.join(entry.file_name()))?;
        }
        return Ok(());
    }
    std::fs::copy(source, destination)
        .map(|_| ())
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => {
                "A file or folder with that name already exists".to_string()
            }
            _ => e.to_string(),
        })
}

pub fn ensure_copy_target_not_inside_source(source: &Path, target: &Path) -> Result<(), String> {
    if !source.is_dir() {
        return Ok(());
    }
    let canonical_source = source
        .canonicalize()
        .map_err(|e| format!("Cannot resolve source directory: {}", e))?;
    let canonical_target = target
        .canonicalize()
        .map_err(|e| format!("Cannot resolve paste target: {}", e))?;
    if canonical_target.starts_with(&canonical_source) {
        return Err("Cannot copy a folder into itself".to_string());
    }
    Ok(())
}

/// Copy in source order to an already-admitted target. This is not root authority.
pub fn copy_paths_to_directory(source_paths: Vec<String>, target: PathBuf) -> Result<(), String> {
    if !target.is_dir() {
        return Err("Paste target must be a directory".to_string());
    }
    for source_path in source_paths {
        let source = PathBuf::from(&source_path);
        if !source.is_absolute() {
            return Err("Source path must be absolute".to_string());
        }
        if !source.exists() {
            return Err(format!("Source path does not exist: {}", source_path));
        }
        let name = source
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| "Invalid source file name".to_string())?;
        validate_entry_name(name)?;
        let destination = target.join(name);
        if destination.exists() {
            return Err("A file or folder with that name already exists".to_string());
        }
        ensure_copy_target_not_inside_source(&source, &target)?;
        copy_path_recursive(&source, &destination)?;
    }
    Ok(())
}
