//! B5.5-1 project Wiki (2026-10-31): per project, pages compiled from the
//! project's `.agents/notes`, its experiences and accepted skills — no model
//! (frontier-roadmap §4; Q-W1–Q-W6). The database is the authority; each
//! page is also written as `pages/<slug>.md` for people to read. A section
//! the user changed is theirs: compiles keep it and open an issue instead.
//!
//! `<data>/knowledge/<project>/wiki.db`, SQLite with an FTS5 index over the
//! CJK-segmented text.

pub mod code_index;
pub mod compile;
pub mod explain;
pub mod notes;
mod store;

#[cfg(test)]
mod code_index_tests;
#[cfg(test)]
mod explain_tests;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aeroric_protocol::wiki::{
    CompileReport, RevisionReason, WikiBlock, WikiExportPlan, WikiHit, WikiIssue, WikiIssueKind,
    WikiPage, WikiPageKind, WikiPageSummary, WikiRevision, WikiSourceGaps,
};
use parking_lot::Mutex;

use compile::{hash, Sources};
use store::Store;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WikiError {
    Unavailable(String),
    UnknownPage,
    UnknownIssue,
    BadExportDir,
    Storage(String),
}

impl std::fmt::Display for WikiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(why) => write!(f, "Wiki 暂不可用：{why}"),
            Self::UnknownPage => write!(f, "没有这一页"),
            Self::UnknownIssue => write!(f, "没有这个 issue"),
            Self::BadExportDir => write!(
                f,
                "导出目录必须是项目里的相对路径，不能在 .git 里，也不能经过符号链接"
            ),
            Self::Storage(why) => write!(f, "Wiki 存储出错：{why}"),
        }
    }
}

impl std::error::Error for WikiError {}

impl From<rusqlite::Error> for WikiError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

/// A page as Markdown: `# title`, then each block (`## heading` + text).
/// Blocks the user emptied are left out.
pub fn render(page: &WikiPage) -> String {
    let mut out = format!("# {}\n", page.title);
    for block in &page.blocks {
        if block.text.is_empty() && block.user_edited {
            continue;
        }
        out.push('\n');
        if let Some(heading) = &block.heading {
            out.push_str(&format!("## {heading}\n\n"));
        }
        out.push_str(block.text.trim_end());
        out.push('\n');
    }
    out
}

/// `(heading, text)` sections of edited Markdown; the part before the first
/// `##` (after `# title`) has no heading.
pub fn sections(markdown: &str) -> Vec<(Option<String>, String)> {
    let mut out: Vec<(Option<String>, Vec<&str>)> = vec![(None, Vec::new())];
    for line in markdown.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            out.push((Some(heading.trim().to_string()), Vec::new()));
        } else if line.starts_with("# ") && out.len() == 1 && out[0].1.is_empty() {
            continue;
        } else {
            out.last_mut().unwrap().1.push(line);
        }
    }
    out.into_iter()
        .map(|(h, lines)| (h, lines.join("\n").trim().to_string()))
        .filter(|(h, text)| h.is_some() || !text.is_empty())
        .collect()
}

/// The user's Markdown over `page`'s blocks: a section that matches a block
/// by heading (or, headingless, the headingless block) replaces its text —
/// marked as the user's when it differs; a block whose section is gone is
/// kept as removed (empty, the user's); new sections are the user's own.
pub fn apply_edit(page: &WikiPage, markdown: &str) -> WikiPage {
    let edited = sections(markdown);
    let mut used = vec![false; edited.len()];
    let mut blocks = Vec::new();
    for old in &page.blocks {
        let found = edited
            .iter()
            .enumerate()
            .find(|(i, (h, _))| !used[*i] && *h == old.heading);
        let mut next = old.clone();
        match found {
            Some((i, (_, text))) => {
                used[i] = true;
                if *text != old.text.trim() {
                    next.text = text.clone();
                    next.user_edited = true;
                }
            }
            None if old.text.is_empty() && old.user_edited => {}
            None => {
                next.text = String::new();
                next.user_edited = true;
            }
        }
        blocks.push(next);
    }
    let mut n = page
        .blocks
        .iter()
        .filter(|b| b.key.starts_with("user:"))
        .count();
    for (i, (heading, text)) in edited.into_iter().enumerate() {
        if used[i] {
            continue;
        }
        n += 1;
        blocks.push(WikiBlock {
            key: format!("user:{n}"),
            heading,
            compiled_hash: hash(&text),
            text,
            source: None,
            sources: Vec::new(),
            user_edited: true,
        });
    }
    let mut next = page.clone();
    next.blocks = blocks;
    // 2026-11-02: the `# ` line renames the page.
    if let Some(title) = markdown
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .and_then(|l| l.strip_prefix("# "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        if title != page.title {
            if !page.title_edited {
                next.compiled_title = Some(page.title.clone());
            }
            next.title = title.to_string();
            next.title_edited = true;
        }
    }
    next
}

#[derive(Default)]
pub struct Wikis {
    dir: Mutex<Option<PathBuf>>,
    open: Mutex<HashMap<String, Arc<Mutex<Store>>>>,
}

impl Wikis {
    /// `<data>/knowledge`.
    pub fn set_dir(&self, dir: PathBuf) {
        *self.dir.lock() = Some(dir);
    }

    fn store(&self, project: &str) -> Result<Arc<Mutex<Store>>, WikiError> {
        if let Some(store) = self.open.lock().get(project) {
            return Ok(Arc::clone(store));
        }
        let dir = self
            .dir
            .lock()
            .clone()
            .ok_or_else(|| WikiError::Unavailable("存储未打开".into()))?;
        let safe: String = project
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let store = Arc::new(Mutex::new(Store::open(&dir.join(safe))?));
        self.open
            .lock()
            .insert(project.to_string(), Arc::clone(&store));
        Ok(store)
    }

    pub fn has_pages(&self, project: &str) -> bool {
        self.store(project)
            .and_then(|s| s.lock().pages())
            .is_ok_and(|p| !p.is_empty())
    }

    pub fn pages(&self, project: &str) -> Result<Vec<WikiPageSummary>, WikiError> {
        let store = self.store(project)?;
        let pages = store.lock().pages()?;
        Ok(pages
            .into_iter()
            .map(|p| WikiPageSummary {
                edited: p.blocks.iter().filter(|b| b.user_edited).count() as u32,
                slug: p.slug,
                kind: p.kind,
                title: p.title,
                status: p.status,
                category: p.category,
                revision: p.revision,
            })
            .collect())
    }

    pub fn page(&self, project: &str, slug: &str) -> Result<WikiPage, WikiError> {
        self.store(project)?
            .lock()
            .page(slug)?
            .ok_or(WikiError::UnknownPage)
    }

    /// Bring every page in line with `sources`.
    pub fn compile(
        &self,
        project: &str,
        sources: &Sources,
        now: u64,
    ) -> Result<CompileReport, WikiError> {
        self.compile_checked(project, sources, &WikiSourceGaps::default(), now)
    }

    /// R1-1: `compile`, keeping the decision pages whose sources `gaps` says
    /// could not be read. Only a confirmed removal takes a page away.
    pub fn compile_checked(
        &self,
        project: &str,
        sources: &Sources,
        gaps: &WikiSourceGaps,
        now: u64,
    ) -> Result<CompileReport, WikiError> {
        let store = self.store(project)?;
        let mut store = store.lock();
        let mut report = CompileReport::default();
        let kept: Vec<WikiPage> = store
            .pages()?
            .into_iter()
            .filter(|p| {
                (p.kind == WikiPageKind::Decision
                    && (gaps.all || gaps.slugs.contains(&p.slug)))
                    // R4-1: the project folder could not be read; its module
                    // pages stay as they are.
                    || (p.kind == WikiPageKind::Module && gaps.all)
            })
            .collect();
        let explanations = store.explanations()?;
        let targets = compile::target_with(sources, &kept, &explanations);
        let wanted: std::collections::HashSet<String> = targets
            .iter()
            .chain(&kept)
            .map(|p| p.slug.clone())
            .collect();
        for target in targets {
            let current = store.page(&target.slug)?;
            let merged = compile::merge(current.as_ref(), target, now);
            for issue in &merged.issues {
                store.put_issue(issue)?;
                report.issues += 1;
            }
            let said = |p: &WikiPage| -> Vec<(String, String, bool, String)> {
                p.blocks
                    .iter()
                    .map(|b| {
                        (
                            b.key.clone(),
                            b.text.clone(),
                            b.user_edited,
                            b.compiled_hash.clone(),
                        )
                    })
                    .collect()
            };
            let changed = current.as_ref().is_none_or(|c| {
                render(c) != render(&merged.page)
                    || said(c) != said(&merged.page)
                    || c.title != merged.page.title
                    || c.status != merged.page.status
                    || c.category != merged.page.category
            });
            if !changed {
                if let Some(c) = &current {
                    if c.blocks != merged.page.blocks
                        || c.source_commit != merged.page.source_commit
                    {
                        store.refresh(&merged.page)?;
                    }
                }
            }
            if changed {
                if current.is_some() {
                    report.updated += 1;
                } else {
                    report.added += 1;
                }
                store.save(merged.page, RevisionReason::Compile, now)?;
            }
        }
        for page in store.pages()? {
            if wanted.contains(&page.slug) {
                continue;
            }
            if compile::has_user_words(&page) {
                if !store.open_issue_for(&page.slug, WikiIssueKind::SourceRemoved)? {
                    store.put_issue(&WikiIssue {
                        id: format!("wis-{}", &uuid::Uuid::new_v4().simple().to_string()[..10]),
                        slug: page.slug.clone(),
                        block_key: None,
                        kind: WikiIssueKind::SourceRemoved,
                        text: format!(
                            "「{}」的来源已删除；这页有你改过的段落，先保留。",
                            page.title
                        ),
                        suggestion: None,
                        open: true,
                        author: "compiler".into(),
                        at_ms: now,
                    })?;
                    report.issues += 1;
                }
            } else {
                store.remove(&page.slug)?;
                report.removed += 1;
            }
        }
        report.pages = store.pages()?.len() as u32;
        store.set_gaps(gaps)?;
        Ok(report)
    }

    /// R4-2: the generated duties and pitfalls, by module id.
    pub fn explanations(
        &self,
        project: &str,
    ) -> Result<std::collections::HashMap<String, explain::Explanation>, WikiError> {
        self.store(project)?.lock().explanations()
    }

    /// Keeps a generation; the next compile puts it on the module's page.
    pub fn save_explanation(
        &self,
        project: &str,
        explanation: &explain::Explanation,
    ) -> Result<(), WikiError> {
        self.store(project)?.lock().put_explanation(explanation)
    }

    pub fn log_explain(&self, project: &str, run: &explain::ExplainRun) -> Result<(), WikiError> {
        self.store(project)?.lock().log_explain(run)
    }

    pub fn explain_runs(
        &self,
        project: &str,
        since_ms: u64,
    ) -> Result<Vec<explain::ExplainRun>, WikiError> {
        self.store(project)?.lock().explain_runs(since_ms)
    }

    /// What the last compile could not read.
    pub fn source_gaps(&self, project: &str) -> Result<WikiSourceGaps, WikiError> {
        self.store(project)?.lock().gaps()
    }

    pub fn edit(
        &self,
        project: &str,
        slug: &str,
        markdown: &str,
        now: u64,
    ) -> Result<WikiPage, WikiError> {
        let store = self.store(project)?;
        let mut store = store.lock();
        let page = store.page(slug)?.ok_or(WikiError::UnknownPage)?;
        let next = apply_edit(&page, markdown);
        store.save(next, RevisionReason::Edit, now)
    }

    pub fn revisions(&self, project: &str, slug: &str) -> Result<Vec<WikiRevision>, WikiError> {
        self.store(project)?.lock().revisions(slug)
    }

    /// 回到第 n 版: its Markdown, applied like an edit, as a new revision.
    pub fn revert(
        &self,
        project: &str,
        slug: &str,
        n: u32,
        now: u64,
    ) -> Result<WikiPage, WikiError> {
        let store = self.store(project)?;
        let mut store = store.lock();
        let page = store.page(slug)?.ok_or(WikiError::UnknownPage)?;
        let old = store
            .revisions(slug)?
            .into_iter()
            .find(|r| r.n == n)
            .ok_or(WikiError::UnknownPage)?;
        let next = apply_edit(&page, &old.markdown);
        store.save(next, RevisionReason::Revert, now)
    }

    pub fn issues(&self, project: &str) -> Result<Vec<WikiIssue>, WikiError> {
        self.store(project)?.lock().issues()
    }

    /// 采纳建议 (the block takes the compiler's text and is no longer the
    /// user's) or 关闭.
    pub fn issue_action(
        &self,
        project: &str,
        id: &str,
        accept: bool,
        now: u64,
    ) -> Result<WikiIssue, WikiError> {
        let store = self.store(project)?;
        let mut store = store.lock();
        let mut issue = store
            .issues()?
            .into_iter()
            .find(|i| i.id == id)
            .ok_or(WikiError::UnknownIssue)?;
        if accept {
            if let (Some(key), Some(text)) = (&issue.block_key, &issue.suggestion) {
                if let Some(mut page) = store.page(&issue.slug)? {
                    if key == aeroric_protocol::wiki::TITLE_BLOCK_KEY {
                        page.title = text.clone();
                        page.title_edited = false;
                        page.compiled_title = Some(text.clone());
                        store.save(page, RevisionReason::Issue, now)?;
                    } else if let Some(block) = page.blocks.iter_mut().find(|b| &b.key == key) {
                        block.text = text.clone();
                        block.user_edited = false;
                        block.compiled_hash = hash(text);
                        store.save(page, RevisionReason::Issue, now)?;
                    }
                }
            }
        }
        issue.open = false;
        store.put_issue(&issue)?;
        Ok(issue)
    }

    /// `flag_wiki_issue`: an agent (or the user) reports a problem.
    pub fn flag(
        &self,
        project: &str,
        slug: &str,
        text: &str,
        author: &str,
        now: u64,
    ) -> Result<WikiIssue, WikiError> {
        let store = self.store(project)?;
        let store = store.lock();
        store.page(slug)?.ok_or(WikiError::UnknownPage)?;
        let issue = WikiIssue {
            id: format!("wis-{}", &uuid::Uuid::new_v4().simple().to_string()[..10]),
            slug: slug.to_string(),
            block_key: None,
            kind: WikiIssueKind::Agent,
            text: text.chars().take(1000).collect(),
            suggestion: None,
            open: true,
            author: author.to_string(),
            at_ms: now,
        };
        store.put_issue(&issue)?;
        Ok(issue)
    }

    pub fn search(&self, project: &str, query: &str) -> Result<Vec<WikiHit>, WikiError> {
        self.store(project)?.lock().search(query)
    }

    pub fn fingerprint(&self, project: &str) -> Option<String> {
        self.store(project)
            .ok()?
            .lock()
            .fingerprint()
            .ok()
            .flatten()
    }

    /// R4-3: whether agents on `project` get `read_wiki`, `search_wiki` and
    /// `flag_wiki_issue`; off until the user turns it on, since the R4-3
    /// evaluation found no net benefit. The pages stay for the user either way.
    pub fn agent_access(&self, project: &str) -> bool {
        self.store(project)
            .ok()
            .and_then(|s| s.lock().agent_access().ok())
            .unwrap_or(false)
    }

    pub fn set_agent_access(&self, project: &str, on: bool) -> Result<(), WikiError> {
        self.store(project)?.lock().set_agent_access(on)
    }

    pub fn set_fingerprint(&self, project: &str, value: &str) -> Result<(), WikiError> {
        self.store(project)?.lock().set_fingerprint(value)
    }

    /// The files `export` would write, and which of them already exist.
    pub fn export_plan(
        &self,
        project: &str,
        project_dir: &Path,
        dir: &str,
    ) -> Result<WikiExportPlan, WikiError> {
        let rel = export_dir(dir)?;
        let target = export_target(project_dir, &rel)?;
        let mut plan = WikiExportPlan::default();
        for page in self.store(project)?.lock().pages()? {
            let name = format!("{}.md", page.slug);
            let file = format!("{rel}/{name}");
            if std::fs::symlink_metadata(target.join(&name)).is_ok() {
                plan.overwrites.push(file.clone());
            }
            plan.files.push(file);
        }
        Ok(plan)
    }

    /// 导出到仓库: every page to `<project>/<dir>/<slug>.md`; nothing else.
    pub fn export(
        &self,
        project: &str,
        project_dir: &Path,
        dir: &str,
    ) -> Result<Vec<String>, WikiError> {
        let rel = export_dir(dir)?;
        let target = export_target(project_dir, &rel)?;
        std::fs::create_dir_all(&target).map_err(|e| WikiError::Storage(e.to_string()))?;
        let mut written = Vec::new();
        for page in self.store(project)?.lock().pages()? {
            let file = target.join(format!("{}.md", page.slug));
            if std::fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(WikiError::BadExportDir);
            }
            let temp = target.join(format!(".{}.{}.tmp", page.slug, uuid::Uuid::new_v4()));
            std::fs::write(&temp, render(&page))
                .and_then(|()| std::fs::rename(&temp, &file))
                .map_err(|e| {
                    let _ = std::fs::remove_file(&temp);
                    WikiError::Storage(e.to_string())
                })?;
            written.push(format!("{rel}/{}.md", page.slug));
        }
        Ok(written)
    }
}

/// R1-1: where `rel` lands, refusing any existing part of the way that is a
/// symbolic link or not a folder, so a checked path cannot lead out of the project.
fn export_target(project_dir: &Path, rel: &str) -> Result<PathBuf, WikiError> {
    let mut current = project_dir.to_path_buf();
    for part in rel.split('/') {
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(WikiError::BadExportDir),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(WikiError::Storage(error.to_string())),
        }
    }
    Ok(current)
}

/// A relative folder inside the project, not in `.git`, no `..`.
fn export_dir(dir: &str) -> Result<String, WikiError> {
    if dir.trim().starts_with('/') {
        return Err(WikiError::BadExportDir);
    }
    let dir = dir.trim().trim_matches('/');
    let parts: Vec<&str> = dir.split('/').collect();
    if dir.is_empty()
        || Path::new(dir).is_absolute()
        || parts
            .iter()
            .any(|p| p.is_empty() || *p == ".." || *p == ".")
        || parts.iter().any(|p| p.eq_ignore_ascii_case(".git"))
    {
        return Err(WikiError::BadExportDir);
    }
    Ok(dir.to_string())
}
