//! R4-2 (2026-11-10): a module's duties and pitfalls, written by the
//! analysis model from the module's own code and the notes that mention it,
//! and kept only where they cite what they were given.
//!
//! The model sees excerpts with their line numbers; every claim must cite a
//! `path:line` inside an excerpt or a `note:<path>` it was shown. A claim
//! with no such cite is dropped, never shown. What it wrote is marked as
//! inferred on the page, and goes stale when a file it read changes.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::code_index::{CodeIndex, Module};
use super::notes::Note;

/// What one module's prompt may carry, excerpts and notes together.
pub const MAX_INPUT_CHARS: usize = 24_000;
/// Automatic generations a day, across projects; the buttons are not capped.
pub const DAILY_LIMIT: u32 = 20;
pub const MAX_OUTPUT_TOKENS: u32 = 1500;
pub const MAX_DUTIES: usize = 4;
pub const MAX_PITFALLS: usize = 5;
const ENTRY_LINES: u32 = 80;
const SYMBOL_BEFORE: u32 = 4;
const SYMBOL_AFTER: u32 = 16;
const MAX_NOTES: usize = 3;
const NOTE_CHARS: usize = 1500;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Claim {
    pub text: String,
    /// `path:line` or `note:<path>`, each checked against the input.
    pub cites: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Explanation {
    pub module_id: String,
    pub duties: Vec<Claim>,
    pub pitfalls: Vec<Claim>,
    /// Every file shown to the model and its blob then.
    pub versions: BTreeMap<String, String>,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub at_ms: u64,
    /// Claims the model wrote that cited nothing it was given.
    #[serde(default)]
    pub dropped: u32,
}

impl Explanation {
    /// Files read for it that have changed since, given the current blobs.
    pub fn changed(&self, current: &BTreeMap<String, String>) -> Vec<String> {
        self.versions
            .iter()
            .filter(|(path, blob)| current.get(*path).is_some_and(|now| now != *blob))
            .map(|(path, _)| path.clone())
            .collect()
    }
}

/// One generation, for the cost line and the daily limit.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExplainRun {
    pub module_id: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub at_ms: u64,
    /// Started by the idle pass rather than a button.
    pub auto: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Excerpt {
    pub path: String,
    pub start: u32,
    pub lines: Vec<String>,
}

impl Excerpt {
    fn end(&self) -> u32 {
        self.start + self.lines.len() as u32 - 1
    }

    fn chars(&self) -> usize {
        self.lines.iter().map(|l| l.len() + 8).sum::<usize>() + self.path.len() + 24
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    pub module_id: String,
    pub title: String,
    pub facts: String,
    pub excerpts: Vec<Excerpt>,
    /// `(path, text)` of the notes shown.
    pub notes: Vec<(String, String)>,
    pub versions: BTreeMap<String, String>,
}

/// The windows of a file worth showing, as `(first, last)` line numbers,
/// merged where they overlap.
fn windows(module: &Module, path: &str) -> Vec<(u32, u32)> {
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    if module.entries.iter().any(|e| e == path) {
        ranges.push((1, ENTRY_LINES));
    }
    for s in module.symbols.iter().filter(|s| s.path == path) {
        ranges.push((
            s.line.saturating_sub(SYMBOL_BEFORE).max(1),
            s.line + SYMBOL_AFTER,
        ));
    }
    ranges.sort();
    let mut merged: Vec<(u32, u32)> = Vec::new();
    for (a, b) in ranges {
        match merged.last_mut() {
            Some(last) if a <= last.1 + 1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    merged
}

/// Notes that name one of the module's files (by full path or by its path
/// below `src/`).
fn notes_about<'a>(module: &Module, notes: &'a [Note]) -> Vec<&'a Note> {
    let needles: Vec<String> = module
        .files
        .iter()
        .take(60)
        .flat_map(|f| {
            let mut out = vec![f.path.clone()];
            if let Some((_, below)) = f.path.split_once("/src/") {
                if below.contains('/') {
                    out.push(below.to_string());
                }
            }
            out
        })
        .collect();
    notes
        .iter()
        .filter(|n| {
            n.sections
                .iter()
                .any(|(_, text)| needles.iter().any(|needle| text.contains(needle.as_str())))
        })
        .take(MAX_NOTES)
        .collect()
}

/// What the model is shown about `module`: the facts already on its page,
/// then excerpts (entry points first, then around each public symbol) and
/// the notes about it, within [`MAX_INPUT_CHARS`].
pub fn build(module: &Module, index: &CodeIndex, project_dir: &Path, notes: &[Note]) -> Input {
    let titles: BTreeMap<&str, String> = index
        .modules
        .iter()
        .map(|m| (m.id.as_str(), m.title()))
        .collect();
    let deps: Vec<String> = module
        .deps
        .iter()
        .map(|e| {
            titles
                .get(e.to.as_str())
                .cloned()
                .unwrap_or_else(|| e.to.clone())
        })
        .collect();
    let facts = format!(
        "模块：{}\n路径：{}\n文件数：{}\n入口：{}\n依赖：{}",
        module.title(),
        if module.dir.is_empty() {
            "."
        } else {
            &module.dir
        },
        module.files.len(),
        if module.entries.is_empty() {
            "无".to_string()
        } else {
            module.entries.join("、")
        },
        if deps.is_empty() {
            "无".to_string()
        } else {
            deps.join("、")
        },
    );
    let mut budget = MAX_INPUT_CHARS.saturating_sub(facts.len());
    let mut versions = BTreeMap::new();
    let mut excerpts = Vec::new();
    // Entry points first, then files in symbol order.
    let mut order: Vec<&str> = module.entries.iter().map(String::as_str).collect();
    for s in &module.symbols {
        if !order.contains(&s.path.as_str()) {
            order.push(&s.path);
        }
    }
    'files: for path in order {
        let Ok(text) = std::fs::read_to_string(project_dir.join(path)) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for (a, b) in windows(module, path) {
            let b = b.min(lines.len() as u32);
            if a > b {
                continue;
            }
            let excerpt = Excerpt {
                path: path.to_string(),
                start: a,
                lines: lines[(a - 1) as usize..b as usize]
                    .iter()
                    .map(|l| l.to_string())
                    .collect(),
            };
            if excerpt.chars() > budget {
                break 'files;
            }
            budget -= excerpt.chars();
            if let Some(f) = module.files.iter().find(|f| f.path == path) {
                versions.insert(path.to_string(), f.blob.clone());
            }
            excerpts.push(excerpt);
        }
    }
    let mut shown = Vec::new();
    for note in notes_about(module, notes) {
        let text: String = note
            .sections
            .iter()
            .map(|(h, t)| format!("## {h}\n{t}"))
            .collect::<Vec<_>>()
            .join("\n\n")
            .chars()
            .take(NOTE_CHARS)
            .collect();
        if text.len() > budget {
            break;
        }
        budget -= text.len();
        shown.push((note.path.clone(), format!("# {}\n{text}", note.title)));
    }
    Input {
        module_id: module.id.clone(),
        title: module.title(),
        facts,
        excerpts,
        notes: shown,
        versions,
    }
}

pub const SYSTEM: &str = "你在为一个代码项目的 Wiki 写某个模块的说明。只根据给出的代码片段和笔记写，不要猜测没给出的内容。\n\
输出一个 JSON 对象，不要输出别的文字：\n\
{\"duties\": [{\"text\": \"…\", \"cites\": [\"路径:行号\"]}], \"pitfalls\": [{\"text\": \"…\", \"cites\": [\"路径:行号\" 或 \"note:笔记路径\"]}]}\n\
duties 是这个模块负责什么，2 到 4 条；pitfalls 是改这个模块时容易踩的坑，最多 5 条，没有就给空数组。\n\
每条用简体中文写一句话，必须在 cites 里引用至少一处：代码写成片段标题里的路径加冒号加行号（行号在片段每行开头），笔记写成 note: 加笔记路径。\n\
引用不到给出内容的话不要写。";

/// The user message: facts, numbered excerpts, notes.
pub fn prompt(input: &Input) -> String {
    let mut out = format!("{}\n", input.facts);
    for e in &input.excerpts {
        out.push_str(&format!(
            "\n=== {} 第 {}–{} 行 ===\n",
            e.path,
            e.start,
            e.end()
        ));
        for (i, line) in e.lines.iter().enumerate() {
            out.push_str(&format!("{:>5}| {line}\n", e.start + i as u32));
        }
    }
    for (path, text) in &input.notes {
        out.push_str(&format!("\n=== 笔记 note:{path} ===\n{text}\n"));
    }
    out
}

#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    duties: Vec<RawClaim>,
    #[serde(default)]
    pitfalls: Vec<RawClaim>,
}

#[derive(Deserialize)]
struct RawClaim {
    #[serde(default)]
    text: String,
    #[serde(default)]
    cites: Vec<String>,
}

/// Whether `cite` points inside what `input` showed.
pub fn cite_ok(cite: &str, input: &Input) -> bool {
    let cite = cite.trim().trim_matches('`');
    if let Some(path) = cite.strip_prefix("note:") {
        return input.notes.iter().any(|(p, _)| p == path.trim());
    }
    let Some((path, line)) = cite.rsplit_once(':') else {
        return false;
    };
    let line = line
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .and_then(|n| n.parse::<u32>().ok());
    let Some(line) = line else {
        return false;
    };
    input
        .excerpts
        .iter()
        .any(|e| e.path == path.trim() && (e.start..=e.end()).contains(&line))
}

/// The model's answer, keeping each claim's checked cites and dropping a
/// claim left with none. `Err` when the answer is not the JSON asked for.
pub fn verify(answer: &str, input: &Input) -> Result<(Vec<Claim>, Vec<Claim>, u32), String> {
    let start = answer.find('{').ok_or("回答里没有 JSON")?;
    let end = answer.rfind('}').ok_or("回答里没有 JSON")?;
    let parsed: Answer = serde_json::from_str(&answer[start..=end])
        .map_err(|e| format!("回答不是要求的 JSON：{e}"))?;
    let mut dropped = 0;
    let mut keep = |raw: Vec<RawClaim>, max: usize| -> Vec<Claim> {
        let mut out = Vec::new();
        for c in raw {
            let text = c.text.trim().to_string();
            let cites: Vec<String> = c
                .cites
                .iter()
                .map(|s| s.trim().trim_matches('`').to_string())
                .filter(|s| cite_ok(s, input))
                .collect();
            if text.is_empty() || cites.is_empty() || out.len() >= max {
                dropped += 1;
                continue;
            }
            out.push(Claim { text, cites });
        }
        out
    };
    let duties = keep(parsed.duties, MAX_DUTIES);
    let pitfalls = keep(parsed.pitfalls, MAX_PITFALLS);
    Ok((duties, pitfalls, dropped))
}

/// `- text（`a.rs:12`、`note:x.md`）`.
pub fn render(claims: &[Claim]) -> String {
    claims
        .iter()
        .map(|c| {
            let cites: Vec<String> = c.cites.iter().map(|s| format!("`{s}`")).collect();
            format!("- {}（{}）", c.text, cites.join("、"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}
