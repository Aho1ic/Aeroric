//! Q-M8: the harnesses' own memory files, shown read-only on the memory
//! page with where they came from. Never indexed, never injected, never
//! written (§6.4: no two authorities).

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
