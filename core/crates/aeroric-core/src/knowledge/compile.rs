//! What the pages should say (deterministic, no model) and how that merges
//! into what they say now without overwriting the user.

use std::collections::{BTreeMap, HashMap};

use aeroric_protocol::memory::{Confidence, MemoryItem, SkillProposal};
use aeroric_protocol::wiki::{
    SourceKind, SourceRef, WikiBlock, WikiIssue, WikiIssueKind, WikiPage, WikiPageKind,
};
use sha2::{Digest, Sha256};

use super::code_index::{CodeIndex, Module};
use super::explain::{self, Explanation};
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
    /// R4-1: the project's code, read structurally; `None` when it was not
    /// read (a remote project, or no folder).
    pub code: Option<CodeIndex>,
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
        sources: Vec::new(),
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
        source_commit: None,
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
    target_with(sources, kept, &HashMap::new())
}

/// `target_keeping`, with the modules' generated explanations (R4-2).
pub fn target_with(
    sources: &Sources,
    kept: &[WikiPage],
    explanations: &HashMap<String, Explanation>,
) -> Vec<WikiPage> {
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
                        version: None,
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
                    version: None,
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
                    version: None,
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
    if let Some(code) = sources.code.as_ref().filter(|c| c.available) {
        pages.extend(module_pages(code, explanations));
    }
    let listed: Vec<WikiPage> = pages.iter().chain(kept).cloned().collect();
    pages.push(index(&listed));
    pages
}

/// `core/crates/x::sessions` → `module-core-crates-x-sessions`.
pub fn module_slug(id: &str) -> String {
    let mut out = String::from("module-");
    let mut dash = true;
    for c in id.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out == "module" {
        "module-root".into()
    } else {
        out
    }
}

fn code_ref(path: &str, blob: &str) -> SourceRef {
    SourceRef {
        kind: SourceKind::Code,
        target: path.to_string(),
        label: path.rsplit('/').next().unwrap_or(path).to_string(),
        version: Some(blob.to_string()),
    }
}

/// `3.4K`, `820`.
fn about(lines: u32) -> String {
    if lines >= 10_000 {
        format!("{}K", lines / 1000)
    } else if lines >= 1000 {
        format!("{:.1}K", lines as f64 / 1000.0)
    } else {
        format!("{}", lines / 10 * 10)
    }
}

/// R4-1: one page per module, everything on it read from the code — files,
/// entry points, public symbols, and the dependencies its imports state.
/// The text leaves out what changes with every edit (line counts beyond a
/// rough size, file versions), so editing code does not make revisions;
/// the versions are on the blocks' sources.
fn module_pages(code: &CodeIndex, explanations: &HashMap<String, Explanation>) -> Vec<WikiPage> {
    let mut slugs: HashMap<&str, String> = HashMap::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for m in &code.modules {
        let mut slug = module_slug(&m.id);
        let n = seen.entry(slug.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            slug = format!("{slug}-{n}");
        }
        slugs.insert(m.id.as_str(), slug);
    }
    let titles: HashMap<&str, String> = code
        .modules
        .iter()
        .map(|m| (m.id.as_str(), m.title()))
        .collect();
    let mut dependents: HashMap<&str, Vec<(&Module, &super::code_index::Edge)>> = HashMap::new();
    for m in &code.modules {
        for e in &m.deps {
            dependents.entry(e.to.as_str()).or_default().push((m, e));
        }
    }
    let link = |id: &str| match (titles.get(id), slugs.get(id)) {
        (Some(title), Some(slug)) => format!("[{title}]({slug}.md)"),
        _ => format!("`{id}`"),
    };
    let mut pages = Vec::new();
    for m in &code.modules {
        let key = |section: &str| format!("module:{}#{section}", m.id);
        let all: Vec<SourceRef> = m.files.iter().map(|f| code_ref(&f.path, &f.blob)).collect();
        let first = m
            .entries
            .first()
            .and_then(|e| all.iter().find(|s| &s.target == e))
            .or(all.first())
            .cloned();
        let blob = |path: &str| {
            m.files
                .iter()
                .find(|f| f.path == path)
                .map(|f| f.blob.as_str())
                .unwrap_or("")
        };
        let langs: Vec<&str> = m.langs().iter().map(|l| l.label()).collect();
        let mut overview = format!(
            "`{}` · {} · {} 个文件 · 约 {} 行\n\n所属包：{}（`{}`）",
            if m.dir.is_empty() { "." } else { &m.dir },
            langs.join("、"),
            m.files.len(),
            about(m.lines()),
            m.package_name,
            if m.package.is_empty() {
                "."
            } else {
                &m.package
            },
        );
        // The commit is on the page (`source_commit`), not in the text: a
        // new commit would otherwise rewrite every module page.
        overview.push_str("\n\n本页从代码直接读出；依赖只列 import 里直接读到的。");
        let mut b = block(
            key("overview"),
            Some("概览".into()),
            overview,
            first.clone(),
        );
        b.sources = all.clone();
        let mut blocks = vec![b];
        if let Some(e) = explanations.get(&m.id) {
            blocks.extend(explained(e, m, &key));
        }
        if !m.entries.is_empty() {
            let mut b = block(
                key("entries"),
                Some("入口".into()),
                m.entries
                    .iter()
                    .map(|e| format!("- `{e}`"))
                    .collect::<Vec<_>>()
                    .join("\n"),
                first.clone(),
            );
            b.sources = m.entries.iter().map(|e| code_ref(e, blob(e))).collect();
            blocks.push(b);
        }
        let symbols_text = if m.symbols.is_empty() {
            if m.langs()
                .iter()
                .all(|l| *l == super::code_index::Lang::Other)
            {
                "这类文件不解析符号。".to_string()
            } else {
                "没有读到公开的类型或函数。".to_string()
            }
        } else {
            let mut lines: Vec<String> = m
                .symbols
                .iter()
                .take(super::code_index::MAX_SYMBOLS)
                .map(|s| format!("- `{}`（{}）— `{}:{}`", s.name, s.kind, s.path, s.line))
                .collect();
            if m.symbols.len() > super::code_index::MAX_SYMBOLS {
                lines.push(format!(
                    "- 另有 {} 个公开符号",
                    m.symbols.len() - super::code_index::MAX_SYMBOLS
                ));
            }
            lines.join("\n")
        };
        let mut b = block(
            key("symbols"),
            Some("关键类型与函数".into()),
            symbols_text,
            first.clone(),
        );
        let mut symbol_files: Vec<&str> = m
            .symbols
            .iter()
            .take(super::code_index::MAX_SYMBOLS)
            .map(|s| s.path.as_str())
            .collect();
        symbol_files.dedup();
        b.sources = symbol_files.iter().map(|p| code_ref(p, blob(p))).collect();
        blocks.push(b);
        let deps_text = if m.deps.is_empty() {
            "import 里没有读到本项目其他模块。".to_string()
        } else {
            m.deps
                .iter()
                .map(|e| {
                    format!(
                        "- {} — `{}:{}` 引入 `{}`",
                        link(&e.to),
                        e.path,
                        e.line,
                        e.text
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let mut b = block(
            key("deps"),
            Some("依赖（直接读到）".into()),
            deps_text,
            first.clone(),
        );
        b.sources = m
            .deps
            .iter()
            .map(|e| code_ref(&e.path, blob(&e.path)))
            .collect();
        blocks.push(b);
        let users = dependents.get(m.id.as_str()).cloned().unwrap_or_default();
        let mut b = block(
            key("dependents"),
            Some("被谁依赖（直接读到）".into()),
            if users.is_empty() {
                "没有读到本项目其他模块 import 它。".to_string()
            } else {
                users
                    .iter()
                    .map(|(from, e)| format!("- {} — `{}:{}`", link(&from.id), e.path, e.line))
                    .collect::<Vec<_>>()
                    .join("\n")
            },
            None,
        );
        b.sources = users
            .iter()
            .filter_map(|(from, e)| {
                from.files
                    .iter()
                    .find(|f| f.path == e.path)
                    .map(|f| code_ref(&f.path, &f.blob))
            })
            .collect();
        blocks.push(b);
        const LISTED_FILES: usize = 40;
        let mut files: Vec<String> = m
            .files
            .iter()
            .take(LISTED_FILES)
            .map(|f| format!("- `{}`", f.path))
            .collect();
        if m.files.len() > LISTED_FILES {
            files.push(format!("- 另有 {} 个文件", m.files.len() - LISTED_FILES));
        }
        let mut b = block(key("files"), Some("文件".into()), files.join("\n"), first);
        b.sources = all;
        blocks.push(b);
        let mut p = page(
            slugs[m.id.as_str()].clone(),
            WikiPageKind::Module,
            m.title(),
            blocks,
        );
        p.category = Some(m.package_name.clone());
        p.source_commit = code.commit.clone();
        pages.push(p);
    }
    pages
}

/// R4-2: the "职责（推断）" and "常见坑（推断）" blocks: what the model
/// wrote, each claim with its checked cites; marked possibly stale when a
/// file it read has changed since. The sources are the files it read, at
/// the versions it read.
fn explained(e: &Explanation, m: &Module, key: &dyn Fn(&str) -> String) -> Vec<WikiBlock> {
    let current: std::collections::BTreeMap<String, String> = m
        .files
        .iter()
        .map(|f| (f.path.clone(), f.blob.clone()))
        .collect();
    let changed = e.changed(&current);
    let stale = match changed.as_slice() {
        [] => String::new(),
        [one] => format!("> 可能过时：生成后 `{one}` 改过。\n\n"),
        [one, ..] => format!(
            "> 可能过时：生成后 `{one}` 等 {} 个文件改过。\n\n",
            changed.len()
        ),
    };
    let read: Vec<SourceRef> = e
        .versions
        .iter()
        .map(|(path, blob)| code_ref(path, blob))
        .collect();
    let first = read.first().cloned();
    let intro = format!("以下由 {} 根据代码推断，每条都核对过引用。\n\n", e.model);
    let mut duties = block(
        key("duties"),
        Some("职责（推断）".into()),
        format!("{stale}{intro}{}", explain::render(&e.duties)),
        first.clone(),
    );
    duties.sources = read.clone();
    let mut pitfalls = block(
        key("pitfalls"),
        Some("常见坑（推断）".into()),
        if e.pitfalls.is_empty() {
            format!("{stale}没有找到有出处的坑。")
        } else {
            format!("{stale}{}", explain::render(&e.pitfalls))
        },
        first,
    );
    pitfalls.sources = read;
    vec![duties, pitfalls]
}

fn index(pages: &[WikiPage]) -> WikiPage {
    let mut blocks = vec![block(
        "index:intro".into(),
        None,
        format!(
            "本项目 Wiki 共 {} 页，由 Aeroric 从项目代码、笔记、经验与技能编译。",
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
    let mut modules: BTreeMap<String, Vec<&WikiPage>> = BTreeMap::new();
    for p in pages.iter().filter(|p| p.kind == WikiPageKind::Module) {
        modules
            .entry(p.category.clone().unwrap_or_else(|| "其他".into()))
            .or_default()
            .push(p);
    }
    for (package, list) in modules {
        let lines: Vec<String> = list
            .iter()
            .map(|p| format!("- [{}]({}.md)", p.title, p.slug))
            .collect();
        blocks.push(block(
            format!("index:module:{package}"),
            Some(format!("模块 · {package}")),
            lines.join("\n"),
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
                kept.sources = t.sources.clone();
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
