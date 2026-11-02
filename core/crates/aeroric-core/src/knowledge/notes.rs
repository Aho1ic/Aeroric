//! `.agents/notes/**.md`, read as decisions (write-notes-like-deepseek's
//! four sections: Problem, Decision, Alternatives considered, Consequences).

use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// Relative to the project, `/`-separated.
    pub path: String,
    pub title: String,
    pub status: Option<String>,
    /// The folder below the status folder (`feature`, `architecture`, …).
    pub category: Option<String>,
    /// `(heading, text)` per `##` section, in order.
    pub sections: Vec<(String, String)>,
}

pub fn parse(path: &str, text: &str) -> Note {
    let mut title = None;
    let mut status = None;
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut current: Option<(String, Vec<&str>)> = None;
    for line in text.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            if let Some((h, body)) = current.take() {
                sections.push((h, body.join("\n").trim().to_string()));
            }
            current = Some((heading.trim().to_string(), Vec::new()));
        } else if let Some(body) = current.as_mut() {
            body.1.push(line);
        } else if let Some(t) = line.strip_prefix("# ") {
            title.get_or_insert_with(|| t.trim_start_matches("Agent Note:").trim().to_string());
        } else if let Some(s) = line.strip_prefix("Status:") {
            status.get_or_insert_with(|| s.trim().to_string());
        }
    }
    if let Some((h, body)) = current {
        sections.push((h, body.join("\n").trim().to_string()));
    }
    let parts: Vec<&str> = path.split('/').collect();
    let category = parts
        .iter()
        .position(|p| *p == "notes")
        .and_then(|i| (parts.len() > i + 3).then(|| parts[i + 2].to_string()));
    let stem = parts.last().unwrap_or(&"").trim_end_matches(".md");
    Note {
        path: path.to_string(),
        title: title.unwrap_or_else(|| stem.to_string()),
        status,
        category,
        sections,
    }
}

/// R1-1: a note larger than this is reported unreadable, not loaded.
pub const MAX_NOTE_BYTES: u64 = 1024 * 1024;

/// What reading the notes found, and what it could not read.
#[derive(Clone, Debug, Default)]
pub struct Scan {
    pub notes: Vec<Note>,
    /// False when a folder could not be listed: absent notes prove nothing.
    pub available: bool,
    /// Notes that exist but could not be read, relative to the project.
    pub unreadable: Vec<String>,
}

fn relative(project: &Path, file: &Path) -> Option<String> {
    Some(
        file.strip_prefix(project)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/"),
    )
}

/// Every note under `<project>/.agents/notes`, sorted by path. A project with
/// no notes folder is available and empty; a folder that cannot be read is not.
pub fn scan_checked(project: &Path) -> Scan {
    let root = project.join(".agents").join("notes");
    let mut files = Vec::new();
    let mut scan = Scan {
        available: walk(&root, &mut files) && std::fs::metadata(project).is_ok(),
        ..Scan::default()
    };
    files.sort();
    for file in files {
        let Some(rel) = relative(project, &file) else {
            continue;
        };
        let small = std::fs::metadata(&file).is_ok_and(|m| m.len() <= MAX_NOTE_BYTES);
        match std::fs::read_to_string(&file) {
            Ok(text) if small => scan.notes.push(parse(&rel, &text)),
            _ => scan.unreadable.push(rel),
        }
    }
    scan
}

pub fn scan(project: &Path) -> Vec<Note> {
    scan_checked(project).notes
}

/// False when a directory could not be listed. Symbolic links are not
/// followed, so a link cannot lead out of the project or into a cycle.
fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> bool {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => return error.kind() == std::io::ErrorKind::NotFound,
    };
    let mut complete = true;
    for entry in entries {
        let Ok(entry) = entry else {
            complete = false;
            continue;
        };
        let Ok(kind) = entry.file_type() else {
            complete = false;
            continue;
        };
        let path = entry.path();
        if kind.is_dir() {
            complete &= walk(&path, out);
        } else if kind.is_file()
            && path.extension().is_some_and(|e| e == "md")
            && !path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        {
            out.push(path);
        }
    }
    complete
}

/// The notes' fingerprint: paths and contents, and whether they could be read.
pub fn fingerprint(project: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut files = Vec::new();
    let complete = walk(&project.join(".agents").join("notes"), &mut files);
    files.sort();
    let mut hasher = Sha256::new();
    hasher.update([complete as u8]);
    for file in files {
        hasher.update(file.to_string_lossy().as_bytes());
        match std::fs::read(&file) {
            Ok(bytes) => hasher.update(&bytes),
            Err(_) => hasher.update(b"\0unreadable"),
        }
        hasher.update([0u8]);
    }
    hasher.finalize()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
