//! What the pages should say (deterministic, no model) and how that merges
//! into what they say now without overwriting the user.

use std::collections::{BTreeMap, HashMap};

use aeroric_protocol::memory::{Confidence, MemoryItem, SkillProposal};
use aeroric_protocol::wiki::{
    SourceKind, SourceRef, WikiBlock, WikiIssue, WikiIssueKind, WikiPage, WikiPageKind,
};
use sha2::{Digest, Sha256};

use super::notes::Note;

pub fn hash(text: &str) -> String {
    Sha256::digest(text.as_bytes())[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// What a compile reads.
#[derive(Clone, Debug, Default)]
pub struct Sources {
    pub notes: Vec<Note>,
    /// Active experiences of the project and global ones.
    pub experiences: Vec<MemoryItem>,
    /// Accepted learned skills of the project and global ones.
    pub skills: Vec<SkillProposal>,
}

fn block(
    key: String,
    heading: Option<String>,
    text: String,
    source: Option<SourceRef>,
) -> WikiBlock {
    WikiBlock {
        compiled_hash: hash(&text),
        key,
        heading,
        text,
        source,
        user_edited: false,
    }
}

fn page(slug: String, kind: WikiPageKind, title: String, blocks: Vec<WikiBlock>) -> WikiPage {
    WikiPage {
        slug,
        kind,
        title,
        status: None,
        category: None,
        blocks,
        revision: 0,
        updated_ms: 0,
        title_edited: false,
        compiled_title: None,
    }
}

/// `2026-10-31-delegation.md` → `decision-delegation`.
pub fn decision_slug(path: &str) -> String {
    let stem = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".md");
    let bytes = stem.as_bytes();
    let dated = bytes.len() > 11
        && bytes[..10].iter().enumerate().all(|(i, b)| {
            if i == 4 || i == 7 {
                *b == b'-'
            } else {
                b.is_ascii_digit()
            }
        })
        && bytes[10] == b'-';
    let core = if dated { &stem[11..] } else { stem };
    let clean: String = core
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    format!("decision-{}", clean.trim_matches('-'))
}

fn level(c: Option<Confidence>) -> &'static str {
    match c {
        Some(Confidence::High) => "高",
        Some(Confidence::Medium) => "中",
        _ => "低",
    }
}

/// The pages the sources call for, index last.
pub fn target(sources: &Sources) -> Vec<WikiPage> {
    target_keeping(sources, &[])
}

/// `target`, with `kept` pages (their sources could not be read) still listed
/// in the index.
pub fn target_keeping(sources: &Sources, kept: &[WikiPage]) -> Vec<WikiPage> {
    let mut pages = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for note in &sources.notes {
        let mut slug = decision_slug(&note.path);
        let n = seen.entry(slug.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            slug = format!("{slug}-{n}");
        }
        let file = note
            .path
            .rsplit('/')
            .next()
            .unwrap_or(&note.path)
            .to_string();
        let blocks = note
            .sections
            .iter()
            .map(|(heading, text)| {
                block(
                    format!("note:{}#{heading}", note.path),
                    Some(heading.clone()),
                    text.clone(),
                    Some(SourceRef {
                        kind: SourceKind::Note,
                        target: format!("{}#{heading}", note.path),
                        label: format!("{file}#{heading}"),
                    }),
                )
            })
            .collect();
        let mut p = page(slug, WikiPageKind::Decision, note.title.clone(), blocks);
        p.status = note.status.clone();
        p.category = note.category.clone();
        pages.push(p);
    }
    if !sources.experiences.is_empty() || !sources.skills.is_empty() {
        let mut blocks = vec![block(
            "runbook:intro".into(),
            None,
            "本页由 Aeroric 根据学会的经验和已采纳的技能自动编译，按置信度排列。".into(),
            None,
        )];
        let mut experiences = sources.experiences.clone();
        experiences.sort_by_key(|e| {
            (
                std::cmp::Reverse(e.confidence),
                std::cmp::Reverse(e.stats.helpful),
            )
        });
        for e in &experiences {
            let d = e.experience.clone().unwrap_or_default();
            let mut text = String::new();
            if !d.problem.is_empty() {
                text.push_str(&format!("**问题**：{}\n\n", d.problem));
            }
            text.push_str(&format!("**做法**：{}", d.solution));
            let mut meta = vec![
                format!("置信度 {}", level(e.confidence)),
                format!("有用 {} 次", e.stats.helpful),
            ];
            if !d.envs.is_empty() {
                meta.insert(0, format!("适用：{}", d.envs.join("、")));
            }
            text.push_str(&format!("\n\n{}", meta.join(" · ")));
            blocks.push(block(
                format!("memory:{}", e.id),
                Some(e.text.clone()),
                text,
                Some(SourceRef {
                    kind: SourceKind::Memory,
                    target: e.id.clone(),
                    label: "经验".into(),
                }),
            ));
        }
        for s in &sources.skills {
            blocks.push(block(
                format!("skill:{}", s.name),
                Some(format!("技能 {}", s.name)),
                format!(
                    "{}\n\n调用：Claude Code 输入 `/aeroric:{name}`，Codex 输入 `${name}`。",
                    s.description,
                    name = s.name
                ),
                Some(SourceRef {
                    kind: SourceKind::Skill,
                    target: s.name.clone(),
                    label: "技能".into(),
                }),
            ));
        }
        pages.push(page(
            "runbook".into(),
            WikiPageKind::Runbook,
            "操作手册".into(),
            blocks,
        ));
    }
    let listed: Vec<WikiPage> = pages.iter().chain(kept).cloned().collect();
    pages.push(index(&listed));
    pages
}

fn index(pages: &[WikiPage]) -> WikiPage {
    let mut blocks = vec![block(
        "index:intro".into(),
        None,
        format!(
            "本项目 Wiki 共 {} 页，由 Aeroric 从项目笔记、经验与技能编译。",
            pages.len() + 1
        ),
        None,
    )];
    if pages.iter().any(|p| p.kind == WikiPageKind::Runbook) {
        blocks.push(block(
            "index:runbook".into(),
            Some("操作手册".into()),
            "- [操作手册](runbook.md)：构建、测试、排障时可复用的做法".into(),
            None,
        ));
    }
    let mut groups: BTreeMap<String, Vec<&WikiPage>> = BTreeMap::new();
    for p in pages.iter().filter(|p| p.kind == WikiPageKind::Decision) {
        groups
            .entry(p.category.clone().unwrap_or_else(|| "其他".into()))
            .or_default()
            .push(p);
    }
    for (category, list) in groups {
        let lines: Vec<String> = list
            .iter()
            .map(|p| {
                let status = p
                    .status
                    .as_deref()
                    .map(|s| format!(" · {s}"))
                    .unwrap_or_default();
                format!("- [{}]({}.md){status}", p.title, p.slug)
            })
            .collect();
        blocks.push(block(
            format!("index:{category}"),
            Some(format!("决策 · {category}")),
            lines.join("\n"),
            None,
        ));
    }
    page("index".into(), WikiPageKind::Index, "索引".into(), blocks)
}

/// What merging produced for one page.
pub struct Merged {
    pub page: WikiPage,
    pub issues: Vec<WikiIssue>,
}

fn issue(
    slug: &str,
    key: Option<&str>,
    kind: WikiIssueKind,
    text: String,
    suggestion: Option<String>,
    now: u64,
) -> WikiIssue {
    WikiIssue {
        id: format!("wis-{}", &uuid::Uuid::new_v4().simple().to_string()[..10]),
        slug: slug.to_string(),
        block_key: key.map(str::to_string),
        kind,
        text,
        suggestion,
        open: true,
        author: "compiler".into(),
        at_ms: now,
    }
}

/// `target` over `current`: untouched blocks follow the sources; a block
/// the user changed stays theirs and a changed or vanished source opens an
/// issue; blocks the user added stay at the end.
pub fn merge(current: Option<&WikiPage>, target: WikiPage, now: u64) -> Merged {
    let Some(current) = current else {
        return Merged {
            page: target,
            issues: Vec::new(),
        };
    };
    let mut issues = Vec::new();
    // A title the user changed stays theirs; a changed source title is an issue.
    let mut target = target;
    if current.title_edited {
        let compiled = current.compiled_title.as_deref().unwrap_or(&current.title);
        if compiled != target.title {
            issues.push(issue(
                &target.slug,
                Some(aeroric_protocol::wiki::TITLE_BLOCK_KEY),
                WikiIssueKind::SourceChanged,
                "这页标题的来源有更新，你改过的标题没有被覆盖。".into(),
                Some(target.title.clone()),
                now,
            ));
        }
        target.compiled_title = Some(target.title.clone());
        target.title = current.title.clone();
        target.title_edited = true;
    }
    let existing: HashMap<&str, &WikiBlock> =
        current.blocks.iter().map(|b| (b.key.as_str(), b)).collect();
    let mut blocks = Vec::new();
    for t in &target.blocks {
        match existing.get(t.key.as_str()) {
            Some(old) if old.user_edited => {
                let mut kept = (*old).clone();
                if old.compiled_hash != t.compiled_hash {
                    issues.push(issue(
                        &target.slug,
                        Some(&t.key),
                        WikiIssueKind::SourceChanged,
                        format!(
                            "「{}」的来源有更新，你改过的这段没有被覆盖。",
                            t.heading.as_deref().unwrap_or(&target.title)
                        ),
                        Some(t.text.clone()),
                        now,
                    ));
                    kept.compiled_hash = t.compiled_hash.clone();
                }
                kept.source = t.source.clone();
                blocks.push(kept);
            }
            _ => blocks.push(t.clone()),
        }
    }
    let target_keys: std::collections::HashSet<&str> =
        target.blocks.iter().map(|b| b.key.as_str()).collect();
    for old in &current.blocks {
        if target_keys.contains(old.key.as_str()) {
            continue;
        }
        if old.key.starts_with("user:") {
            blocks.push(old.clone());
        } else if old.user_edited && !old.text.is_empty() {
            issues.push(issue(
                &target.slug,
                Some(&old.key),
                WikiIssueKind::SourceRemoved,
                format!(
                    "「{}」的来源已删除，你改过的这段先保留。",
                    old.heading.as_deref().unwrap_or(&current.title)
                ),
                None,
                now,
            ));
            let mut kept = old.clone();
            kept.source = None;
            blocks.push(kept);
        }
    }
    let mut page = target;
    page.blocks = blocks;
    page.revision = current.revision;
    page.updated_ms = current.updated_ms;
    Merged { page, issues }
}

/// Whether a page whose sources vanished still holds the user's words.
pub fn has_user_words(page: &WikiPage) -> bool {
    page.blocks
        .iter()
        .any(|b| b.user_edited && !b.text.is_empty())
}
