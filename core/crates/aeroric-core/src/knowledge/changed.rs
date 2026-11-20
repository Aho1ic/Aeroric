//! H4 (2026-11-21): a file a session changed marks the pages that cite it
//! as possibly out of date at once, not at the next compile. The marks are
//! the same judgement the compile makes (a code source no longer as read),
//! made early from the turn's changed files; the next compile, which sees
//! every file as it is, clears them.

use std::collections::BTreeMap;
use std::path::Path;

use aeroric_protocol::wiki::{SourceKind, WikiChangedFile, WikiPage};

/// Kept beside the project's wiki store.
pub const FILE: &str = "changed-since-compile.json";

/// Page slug → its cited files changed since the compile.
pub type Marks = BTreeMap<String, Vec<WikiChangedFile>>;

/// Whether a changed path (relative to the repository's top) is the cited
/// one (relative to the project folder, which may be below the top).
fn same_file(changed: &str, cited: &str) -> bool {
    changed == cited || changed.ends_with(&format!("/{cited}"))
}

/// The code files `page` cites.
fn cited(page: &WikiPage) -> Vec<&str> {
    let mut out: Vec<&str> = page
        .blocks
        .iter()
        .flat_map(|b| b.source.iter().chain(b.sources.iter()))
        .filter(|s| s.kind == SourceKind::Code)
        .map(|s| s.target.as_str())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Add to `marks` every page citing one of `changed`; returns the slugs
/// newly marked or marked again.
pub fn mark(
    marks: &mut Marks,
    pages: &[WikiPage],
    changed: &[String],
    task_id: &str,
    at_ms: u64,
) -> Vec<String> {
    let mut touched = Vec::new();
    for page in pages {
        let hits: Vec<&str> = cited(page)
            .into_iter()
            .filter(|cited| changed.iter().any(|c| same_file(c, cited)))
            .collect();
        if hits.is_empty() {
            continue;
        }
        let files = marks.entry(page.slug.clone()).or_default();
        for path in hits {
            files.retain(|f| f.path != path);
            files.push(WikiChangedFile {
                path: path.to_string(),
                task_id: task_id.to_string(),
                at_ms,
            });
        }
        touched.push(page.slug.clone());
    }
    touched
}

/// The line a page is read with while it is marked; empty when it is not.
pub fn note(files: &[WikiChangedFile]) -> String {
    let named = match files {
        [] => return String::new(),
        [one] => format!("`{}` ", one.path),
        [one, ..] => format!("`{}` 等 {} 个文件", one.path, files.len()),
    };
    format!("> 可能过时：这页生成后，{named}在会话里改过，还没重新生成。\n\n")
}

pub fn read(dir: &Path) -> Marks {
    std::fs::read_to_string(dir.join(FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn write(dir: &Path, marks: &Marks) -> Result<(), String> {
    if marks.is_empty() {
        return match std::fs::remove_file(dir.join(FILE)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    let text = serde_json::to_string(marks).map_err(|e| e.to_string())?;
    crate::storage::atomic::write_atomic(
        &dir.join(FILE),
        text.as_bytes(),
        crate::storage::atomic::FileMode::Private,
    )
    .map_err(|e| e.to_string())
}
