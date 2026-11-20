//! Q-M8: the harnesses' own memory files, shown read-only on the memory
//! page with where they came from. Never written (§6.4: no two
//! authorities), never injected as they are.
//!
//! H7 (2026-11-21): read as evidence. The automatic pass reads each file
//! once per content and turns its lines into pending proposals that name
//! the file; only what the user accepts becomes Aeroric's. The files stay
//! byte for byte as they were.

use std::path::{Path, PathBuf};

use aeroric_protocol::memory::NativeMemory;

/// Shown up to this many bytes.
const MAX_BYTES: usize = 8 * 1024;

fn read(harness: &str, path: PathBuf, title: &str, out: &mut Vec<NativeMemory>) {
    let Ok(bytes) = std::fs::read(&path) else {
        return;
    };
    if bytes.is_empty() {
        return;
    }
    let truncated = bytes.len() > MAX_BYTES;
    let mut end = bytes.len().min(MAX_BYTES);
    while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
        end -= 1;
    }
    out.push(NativeMemory {
        harness: harness.to_string(),
        path: path.to_string_lossy().into_owned(),
        title: title.to_string(),
        text: String::from_utf8_lossy(&bytes[..end]).into_owned(),
        truncated,
    });
}

/// `homes` are `(harness, config home)` pairs — `~/.claude`, `~/.codex`
/// and managed homes; `cwd` is the project's folder.
pub fn discover(homes: &[(String, PathBuf)], cwd: Option<&Path>) -> Vec<NativeMemory> {
    let mut out = Vec::new();
    for (harness, home) in homes {
        if harness.starts_with("claude") {
            read(
                harness,
                home.join("CLAUDE.md"),
                "用户级 CLAUDE.md",
                &mut out,
            );
            if let Some(cwd) = cwd {
                let encoded =
                    crate::sessions::discovery::encode_claude_project_path(&cwd.to_string_lossy());
                read(
                    harness,
                    home.join("projects")
                        .join(encoded)
                        .join("memory")
                        .join("MEMORY.md"),
                    "Claude 自动记忆 MEMORY.md",
                    &mut out,
                );
            }
        } else if harness.starts_with("codex") {
            read(
                harness,
                home.join("AGENTS.md"),
                "用户级 AGENTS.md",
                &mut out,
            );
            if let Ok(entries) = std::fs::read_dir(home.join("memories")) {
                let mut files: Vec<PathBuf> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x == "md"))
                    .collect();
                files.sort();
                for file in files {
                    let title = format!(
                        "Codex 记忆 {}",
                        file.file_name().unwrap_or_default().to_string_lossy()
                    );
                    read(harness, file, &title, &mut out);
                }
            }
        }
    }
    if let Some(cwd) = cwd {
        read("project", cwd.join("CLAUDE.md"), "项目 CLAUDE.md", &mut out);
        read("project", cwd.join("AGENTS.md"), "项目 AGENTS.md", &mut out);
    }
    out
}

/// B5-2 (Q-M14): a memory file's lines as candidate texts — headings, blank
/// lines and code blocks skipped, list markers dropped. No model involved.
pub fn import_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_code = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code || line.is_empty() || line.starts_with('#') || line.starts_with("---") {
            continue;
        }
        let mut body = line.trim_start_matches(['-', '*', '+']).trim_start();
        if let Some((number, rest)) = body.split_once(['.', '、', ')']) {
            if !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()) {
                body = rest.trim_start();
            }
        }
        let body: String = body
            .chars()
            .take(aeroric_protocol::memory::MAX_TEXT_CHARS)
            .collect();
        if body.chars().count() >= 2 {
            out.push(body);
        }
    }
    out
}

/// H7: at most this many new proposals from one file in one pass.
pub const MAX_PROPOSALS_PER_FILE: usize = 20;

/// H7: whether a file is about one project (its facts) rather than the
/// user (their profile): a project's own file, or Claude's memory for it.
pub fn is_project_file(file: &NativeMemory) -> bool {
    file.harness == "project"
        || (file.path.contains("/projects/") && file.path.ends_with("/memory/MEMORY.md"))
}

/// H7: a file's path as evidence names it, short enough to leave room for
/// the line: the home folder as `~`, Claude's encoded project folder (the
/// whole project path again) as `<本项目>`.
pub fn short_path(path: &str) -> String {
    let mut out = path.to_string();
    if let (Some(at), true) = (out.find("/projects/"), out.ends_with("/memory/MEMORY.md")) {
        out = format!("{}/projects/<本项目>/memory/MEMORY.md", &out[..at]);
    }
    if let Some(home) = crate::settings::home_dir() {
        let home = home.to_string_lossy().into_owned();
        if !home.is_empty() && out.starts_with(&format!("{home}/")) {
            out = format!("~{}", &out[home.len()..]);
        }
    }
    out
}

/// H7: what one pass over the files did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Harvest {
    /// Files read this time (new or changed since the last pass).
    pub read: usize,
    pub proposed: usize,
}

impl super::Memory {
    /// H7: turn the lines of each native file whose content changed since
    /// the last pass into pending proposals (a project's file into facts of
    /// `project_id`, the user's into their profile), each naming its file.
    /// A line already known, or one the user rejected, is not proposed
    /// again. Files are only read.
    pub fn harvest_native(
        &self,
        files: &[NativeMemory],
        project_id: Option<&str>,
        now_ms: u64,
    ) -> Harvest {
        use aeroric_protocol::memory::{MemoryDraft, MemoryKind, MemoryState};
        let Some(ledger) = self.dir.lock().clone().map(|d| d.join("native-read.json")) else {
            return Harvest::default();
        };
        let mut seen: std::collections::BTreeMap<String, String> = std::fs::read_to_string(&ledger)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        let mut out = Harvest::default();
        for file in files {
            let project = if is_project_file(file) {
                project_id
            } else {
                None
            };
            if is_project_file(file) && project.is_none() {
                continue;
            }
            let key = format!("{}|{}", project.unwrap_or(""), file.path);
            let hash = super::evidence(None, None, None, &file.text, now_ms).hash;
            if seen.get(&key) == Some(&hash) {
                continue;
            }
            out.read += 1;
            let mut proposed = 0;
            for line in import_lines(&file.text) {
                if proposed >= MAX_PROPOSALS_PER_FILE {
                    break;
                }
                let draft = MemoryDraft {
                    project_id: project.map(str::to_string),
                    kind: if project.is_some() {
                        MemoryKind::Fact
                    } else {
                        MemoryKind::Profile
                    },
                    category: None,
                    text: line.clone(),
                    supersedes: None,
                };
                let proof = super::evidence(
                    None,
                    Some(&file.harness),
                    None,
                    &format!("{} · {line}", short_path(&file.path)),
                    now_ms,
                );
                if let Ok(item) = self.propose(draft, proof, now_ms) {
                    if item.state == MemoryState::Pending && item.created_ms == now_ms {
                        proposed += 1;
                    }
                }
            }
            out.proposed += proposed;
            seen.insert(key, hash);
        }
        if out.read > 0 {
            if let Ok(text) = serde_json::to_string(&seen) {
                let _ = crate::settings::atomic_write_private(&ledger, &text);
            }
        }
        out
    }
}

#[cfg(test)]
mod import_tests {
    #[test]
    fn lines_become_candidates() {
        let text = "# 偏好\n\n- 回复用中文\n* 只在 main 工作\n1. 发布前跑检查\n```\ncode\n```\n普通一句话\n-\n";
        assert_eq!(
            super::import_lines(text),
            ["回复用中文", "只在 main 工作", "发布前跑检查", "普通一句话"]
        );
    }
}
