//! Embeds the built-in skills (`core/skills/builtin`, B2 2026-10-30) into
//! the crate so the host, the headless CLI and tests share one copy.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.join("../../skills/builtin");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    collect(&root, &root, &mut files);
    files.sort();
    let mut out = String::from("pub static FILES: &[(&str, &[u8], bool)] = &[\n");
    for rel in &files {
        let full = root.join(rel);
        println!("cargo:rerun-if-changed={}", full.display());
        let executable = is_executable(&full);
        writeln!(
            out,
            "    ({:?}, include_bytes!({:?}), {executable}),",
            rel,
            full.canonicalize().unwrap().display().to_string()
        )
        .unwrap();
    }
    out.push_str("];\n");
    let dest = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("builtin_skills.rs");
    std::fs::write(dest, out).unwrap();
}

fn collect(root: &Path, dir: &Path, files: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".DS_Store" {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, files);
        } else if path.is_file() {
            files.push(
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(_: &Path) -> bool {
    false
}
