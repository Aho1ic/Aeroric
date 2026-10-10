//! Exact legacy entry-name/new-path policy and exclusive-create IO.
//! Callers retain their worker/guard/join order; validated-path IO helpers are
//! not project authority or a descriptor-relative race guarantee.

use std::path::Path;

/// Names whose stem (the substring before the first `.`) are reserved on Windows. Only consulted
/// when compiling for Windows; on Unix these are perfectly valid filenames (matching VS Code's
/// behavior of validating against the running OS, not the lowest common denominator).
#[cfg(target_os = "windows")]
const WINDOWS_RESERVED_STEMS: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM0", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
    "COM8", "COM9", "LPT0", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Validate a single path component that the user wants to create.
///
/// Cross-platform rejects (always):
/// - empty / `.` / `..`
/// - longer than 255 UTF-8 bytes
/// - contains `/`, `\\`, or NUL
///
/// Windows-only rejects (mirroring `CreateFileW` rules):
/// - extra forbidden characters (`< > : " | ? *`)
/// - ASCII control characters (< 0x20)
/// - trailing space or dot (Win32 would silently strip them)
/// - reserved DOS device names (CON/PRN/AUX/NUL/COM[0-9]/LPT[0-9]), case-insensitive
pub fn validate_entry_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("File name cannot be empty".to_string());
    }
    if name.len() > 255 {
        return Err("File name is too long (max 255 bytes)".to_string());
    }
    if name == "." || name == ".." {
        return Err("Invalid file name".to_string());
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err("File name contains forbidden characters".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        for ch in name.chars() {
            if matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                return Err("File name contains forbidden characters".to_string());
            }
            if (ch as u32) < 0x20 {
                return Err("File name contains control characters".to_string());
            }
        }
        if name.ends_with(' ') || name.ends_with('.') {
            return Err("File name cannot end with a space or a dot".to_string());
        }
        let stem = name.split_once('.').map(|(s, _)| s).unwrap_or(name);
        if !stem.is_empty() {
            let stem_upper = stem.to_ascii_uppercase();
            if WINDOWS_RESERVED_STEMS.iter().any(|r| *r == stem_upper) {
                return Err(format!("File name '{}' is reserved on Windows", stem));
            }
        }
    }

    Ok(())
}

/// Validate a not-yet-existing `target` path. Returns the canonicalized parent directory and the
/// raw basename. Existence is *not* checked here — the caller must use atomic create operations
/// (`OpenOptions::create_new` / `create_dir`) to avoid TOCTOU between an existence check and the
/// actual create.
pub fn validate_new_path_within(
    target: &str,
    allowed_root: &str,
) -> Result<(std::path::PathBuf, String), String> {
    let target_path = Path::new(target);

    if !target_path.is_absolute() {
        return Err("Path must be absolute".to_string());
    }

    let file_name = target_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "Invalid file name".to_string())?
        .to_string();

    validate_entry_name(&file_name)?;

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

    Ok((canonical_parent, file_name))
}

/// Exclusive creation at the caller-validated target; no parent repair or chmod.
pub fn create_file(target: &Path) -> Result<(), String> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map(|_| ())
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => {
                "A file or folder with that name already exists".to_string()
            }
            _ => e.to_string(),
        })
}

/// Exclusive creation at the caller-validated target; no parent repair or chmod.
pub fn create_directory(target: &Path) -> Result<(), String> {
    std::fs::create_dir(target).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => {
            "A file or folder with that name already exists".to_string()
        }
        _ => e.to_string(),
    })
}
