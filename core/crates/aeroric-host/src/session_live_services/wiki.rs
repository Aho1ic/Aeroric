//! B5.5-1 project Wiki (2026-10-31), the host's side: the `wiki.*` methods
//! the app calls, the `context.wiki*` methods behind `read_wiki`,
//! `search_wiki` and `flag_wiki_issue`, and the pass that recompiles a
//! project's Wiki when its notes, experiences or skills changed (Q-W6). The
//! Wikis are core's (`aeroric_core::knowledge`).

use std::path::PathBuf;
use std::sync::Arc;

use aeroric_core::knowledge::{compile::Sources, notes, WikiError};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use aeroric_protocol::memory::{MemoryKind, MemoryState, SkillProposalState};
use aeroric_protocol::wiki::{self as wire, CompileReport, WikiSourceGaps};
use serde_json::{json, Value};

use super::{blocking, failure, ok, params, Live};
use crate::dispatcher::ServiceSet;

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    live.executor
        .wikis()
        .set_dir(live.context.paths.data_dir().join("knowledge"));
    set.register(wire::PAGES_METHOD, blocking(live, wiki_pages));
    set.register(wire::PAGE_METHOD, blocking(live, wiki_page));
    set.register(wire::COMPILE_METHOD, blocking(live, wiki_compile));
    set.register(wire::EDIT_METHOD, blocking(live, wiki_edit));
    set.register(wire::REVISIONS_METHOD, blocking(live, wiki_revisions));
    set.register(wire::REVERT_METHOD, blocking(live, wiki_revert));
    set.register(wire::ISSUES_METHOD, blocking(live, wiki_issues));
    set.register(wire::ISSUE_ACTION_METHOD, blocking(live, wiki_issue_action));
    set.register(wire::EXPORT_PLAN_METHOD, blocking(live, wiki_export_plan));
    set.register(wire::EXPORT_METHOD, blocking(live, wiki_export));
    set.register(wire::SEARCH_METHOD, blocking(live, wiki_search));
    set.register("wiki.sweep", blocking(live, wiki_sweep));
    set.register("context.wikiAvailable", blocking(live, context_available));
    set.register("context.wikiRead", blocking(live, context_read));
    set.register("context.wikiSearch", blocking(live, context_search));
    set.register("context.wikiFlag", blocking(live, context_flag));
    start_sweeper(live);
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn wiki_failure(request: &Request, error: WikiError) -> Frame {
    let code = match error {
        WikiError::UnknownPage | WikiError::UnknownIssue => ErrorCode::NotFound,
        WikiError::BadExportDir => ErrorCode::InvalidArgument,
        _ => ErrorCode::IoError,
    };
    failure(request.id.clone(), code, &error.to_string())
}

fn answer<T: serde::Serialize>(
    request: &Request,
    result: Result<T, WikiError>,
    key: &str,
) -> Frame {
    match result {
        Ok(value) => ok(request, json!({ key: value })),
        Err(error) => wiki_failure(request, error),
    }
}

/// A local project's folder.
fn project_dir(live: &Live, project_id: &str) -> Option<PathBuf> {
    let projects = aeroric_core::projects::ProjectService::new(Arc::clone(&live.context))
        .list()
        .ok()?;
    let project = projects
        .iter()
        .find(|p| p.get("id").and_then(Value::as_str) == Some(project_id))?;
    let path = PathBuf::from(project.get("path").and_then(Value::as_str)?);
    path.is_dir().then_some(path)
}

/// Bumped when compiling itself changes, so every Wiki is compiled again.
const COMPILER_VERSION: u32 = 2;

/// What a project's Wiki compiles from: its notes, its and global
/// experiences, its and global accepted skills; what could not be read; and
/// the fingerprint. A store error is an error, never an empty source.
fn sources(live: &Live, project_id: &str) -> Result<(Sources, WikiSourceGaps, String), WikiError> {
    let memory = live.executor.memory();
    let unavailable = |e: aeroric_core::memory::MemoryError| WikiError::Unavailable(e.to_string());
    let (scan, notes_print) = match project_dir(live, project_id) {
        Some(dir) => (notes::scan_checked(&dir), notes::fingerprint(&dir)),
        None => (notes::Scan::default(), "unavailable".to_string()),
    };
    let gaps = WikiSourceGaps {
        all: !scan.available,
        slugs: scan
            .unreadable
            .iter()
            .map(|path| aeroric_core::knowledge::compile::decision_slug(path))
            .collect(),
    };
    let mut experiences: Vec<_> = memory
        .list(Some(project_id), None)
        .map_err(unavailable)?
        .into_iter()
        .chain(memory.list(None, None).map_err(unavailable)?)
        .filter(|i| i.kind == MemoryKind::Experience && i.state == MemoryState::Active)
        .collect();
    experiences.sort_by(|a, b| a.id.cmp(&b.id));
    let skills: Vec<_> = memory
        .proposals()
        .map_err(unavailable)?
        .into_iter()
        .filter(|p| p.state == SkillProposalState::Accepted)
        .filter(|p| p.global || p.project_id.as_deref() == Some(project_id))
        .collect();
    let latest = experiences
        .iter()
        .map(|e| e.updated_ms)
        .chain(skills.iter().map(|s| s.updated_ms))
        .max()
        .unwrap_or(0);
    let fingerprint = format!(
        "{COMPILER_VERSION}|{notes_print}|{}|{}|{latest}",
        experiences.len(),
        skills.len()
    );
    Ok((
        Sources {
            notes: scan.notes,
            experiences,
            skills,
        },
        gaps,
        fingerprint,
    ))
}

fn compile(live: &Live, project_id: &str) -> Result<CompileReport, WikiError> {
    let (sources, gaps, fingerprint) = sources(live, project_id)?;
    let wikis = live.executor.wikis();
    let report = wikis.compile_checked(project_id, &sources, &gaps, now_ms())?;
    wikis.set_fingerprint(project_id, &fingerprint)?;
    Ok(report)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectParams {
    project_id: String,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    markdown: Option<String>,
    #[serde(default)]
    n: Option<u32>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    accept: Option<bool>,
    #[serde(default)]
    dir: Option<String>,
    #[serde(default)]
    query: Option<String>,
}

fn project_params(request: &Request) -> Result<ProjectParams, Box<Frame>> {
    params(request)
}

fn missing(request: &Request, what: &str) -> Frame {
    failure(
        request.id.clone(),
        ErrorCode::InvalidArgument,
        &format!("缺少 {what}"),
    )
}

/// `wiki.pages {projectId}`: the page tree.
fn wiki_pages(live: &Live, request: &Request) -> Frame {
    let p = match project_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let wikis = live.executor.wikis();
    match (wikis.pages(&p.project_id), wikis.source_gaps(&p.project_id)) {
        (Ok(pages), Ok(gaps)) => ok(
            request,
            json!({ "pages": pages, "sourcesUnavailable": gaps }),
        ),
        (Err(error), _) | (_, Err(error)) => wiki_failure(request, error),
    }
}

/// `wiki.page {projectId, slug}`.
fn wiki_page(live: &Live, request: &Request) -> Frame {
    let p = match project_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some(slug) = p.slug else {
        return missing(request, "slug");
    };
    answer(
        request,
        live.executor.wikis().page(&p.project_id, &slug),
        "page",
    )
}

/// `wiki.compile {projectId}`: 重新编译.
fn wiki_compile(live: &Live, request: &Request) -> Frame {
    let p = match project_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    answer(request, compile(live, &p.project_id), "report")
}

/// `wiki.edit {projectId, slug, markdown}`.
fn wiki_edit(live: &Live, request: &Request) -> Frame {
    let p = match project_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let (Some(slug), Some(markdown)) = (p.slug, p.markdown) else {
        return missing(request, "slug、markdown");
    };
    answer(
        request,
        live.executor
            .wikis()
            .edit(&p.project_id, &slug, &markdown, now_ms()),
        "page",
    )
}

/// `wiki.revisions {projectId, slug}`.
fn wiki_revisions(live: &Live, request: &Request) -> Frame {
    let p = match project_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some(slug) = p.slug else {
        return missing(request, "slug");
    };
    answer(
        request,
        live.executor.wikis().revisions(&p.project_id, &slug),
        "revisions",
    )
}

/// `wiki.revert {projectId, slug, n}`.
fn wiki_revert(live: &Live, request: &Request) -> Frame {
    let p = match project_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let (Some(slug), Some(n)) = (p.slug, p.n) else {
        return missing(request, "slug、n");
    };
    answer(
        request,
        live.executor
            .wikis()
            .revert(&p.project_id, &slug, n, now_ms()),
        "page",
    )
}

/// `wiki.issues {projectId}`.
fn wiki_issues(live: &Live, request: &Request) -> Frame {
    let p = match project_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    answer(
        request,
        live.executor.wikis().issues(&p.project_id),
        "issues",
    )
}

/// `wiki.issueAction {projectId, id, accept}`: 采纳建议 or 关闭.
fn wiki_issue_action(live: &Live, request: &Request) -> Frame {
    let p = match project_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some(id) = p.id else {
        return missing(request, "id");
    };
    answer(
        request,
        live.executor
            .wikis()
            .issue_action(&p.project_id, &id, p.accept.unwrap_or(false), now_ms()),
        "issue",
    )
}

/// `wiki.exportPlan {projectId, dir?}`: what 导出 would write.
fn wiki_export_plan(live: &Live, request: &Request) -> Frame {
    let p = match project_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some(root) = project_dir(live, &p.project_id) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "找不到这个项目的本地目录",
        );
    };
    let dir = p.dir.unwrap_or_else(|| wire::DEFAULT_EXPORT_DIR.into());
    match live
        .executor
        .wikis()
        .export_plan(&p.project_id, &root, &dir)
    {
        Ok(plan) => ok(
            request,
            json!({ "files": plan.files, "overwrites": plan.overwrites }),
        ),
        Err(error) => wiki_failure(request, error),
    }
}

/// `wiki.export {projectId, dir?}`: write the pages into the project.
fn wiki_export(live: &Live, request: &Request) -> Frame {
    let p = match project_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some(root) = project_dir(live, &p.project_id) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "找不到这个项目的本地目录",
        );
    };
    let dir = p.dir.unwrap_or_else(|| wire::DEFAULT_EXPORT_DIR.into());
    answer(
        request,
        live.executor.wikis().export(&p.project_id, &root, &dir),
        "files",
    )
}

/// `wiki.search {projectId, query}`.
fn wiki_search(live: &Live, request: &Request) -> Frame {
    let p = match project_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let query = p.query.unwrap_or_default();
    answer(
        request,
        live.executor.wikis().search(&p.project_id, &query),
        "hits",
    )
}

/// The automatic pass: projects that use memory or already have a Wiki,
/// recompiled when their sources' fingerprint moved.
fn sweep(live: &Live) -> usize {
    let Ok(projects) =
        aeroric_core::projects::ProjectService::new(Arc::clone(&live.context)).list()
    else {
        return 0;
    };
    let wikis = live.executor.wikis();
    let memory = live.executor.memory();
    let mut done = 0;
    for project in projects {
        let Some(id) = project.get("id").and_then(Value::as_str) else {
            continue;
        };
        if !memory.project_enabled(id) && !wikis.has_pages(id) {
            continue;
        }
        let Ok((_, _, fingerprint)) = sources(live, id) else {
            continue;
        };
        if wikis.fingerprint(id).as_deref() == Some(fingerprint.as_str()) {
            continue;
        }
        if compile(live, id).is_ok() {
            done += 1;
        }
    }
    done
}

/// `wiki.sweep {}`: the automatic pass now.
fn wiki_sweep(live: &Live, request: &Request) -> Frame {
    ok(request, json!({ "compiled": sweep(live) }))
}

fn start_sweeper(live: &Arc<Live>) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let weak = Arc::downgrade(live);
    handle.spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(120)).await;
            let Some(live) = weak.upgrade() else {
                break;
            };
            let _ = tokio::task::spawn_blocking(move || sweep(&live)).await;
        }
    });
}

// MARK: context server

fn token_scope(
    live: &Live,
    request: &Request,
    token: &str,
) -> Result<(String, String), Box<Frame>> {
    live.executor
        .context_tokens()
        .resolve(token)
        .ok_or_else(|| {
            Box::new(failure(
                request.id.clone(),
                ErrorCode::Unauthenticated,
                "上下文令牌无效或会话已结束",
            ))
        })
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContextParams {
    token: String,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

/// `context.wikiAvailable {token}`: whether to offer the Wiki tools.
fn context_available(live: &Live, request: &Request) -> Frame {
    let p: ContextParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let available = live
        .executor
        .context_tokens()
        .resolve(&p.token)
        .is_some_and(|(_, project)| live.executor.wikis().has_pages(&project));
    ok(request, json!({ "available": available }))
}

/// `context.wikiRead {token, slug?}`: a page as Markdown (the index when no
/// slug; the list of pages when the slug is unknown).
fn context_read(live: &Live, request: &Request) -> Frame {
    let p: ContextParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let (_, project) = match token_scope(live, request, &p.token) {
        Ok(scope) => scope,
        Err(frame) => return *frame,
    };
    let wikis = live.executor.wikis();
    let slug = p.slug.unwrap_or_else(|| "index".into());
    match wikis.page(&project, &slug) {
        Ok(page) => ok(
            request,
            json!({ "text": aeroric_core::knowledge::render(&page), "slug": page.slug }),
        ),
        Err(WikiError::UnknownPage) => {
            let list: Vec<String> = wikis
                .pages(&project)
                .unwrap_or_default()
                .into_iter()
                .map(|p| format!("- {}（{}）", p.slug, p.title))
                .collect();
            ok(
                request,
                json!({ "text": format!("没有「{slug}」这一页。现有页面：\n{}", list.join("\n")) }),
            )
        }
        Err(error) => wiki_failure(request, error),
    }
}

/// `context.wikiSearch {token, query}`.
fn context_search(live: &Live, request: &Request) -> Frame {
    let p: ContextParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let (_, project) = match token_scope(live, request, &p.token) {
        Ok(scope) => scope,
        Err(frame) => return *frame,
    };
    match live
        .executor
        .wikis()
        .search(&project, p.query.as_deref().unwrap_or_default())
    {
        Ok(hits) => {
            let text = if hits.is_empty() {
                "Wiki 里没有找到相关页面。".to_string()
            } else {
                let lines: Vec<String> = hits
                    .iter()
                    .map(|h| format!("- [{}] {}：{}", h.slug, h.title, h.excerpt))
                    .collect();
                format!(
                    "找到 {} 页（用 read_wiki 读全文）：\n{}",
                    hits.len(),
                    lines.join("\n")
                )
            };
            ok(request, json!({ "text": text, "hits": hits }))
        }
        Err(error) => wiki_failure(request, error),
    }
}

/// `context.wikiFlag {token, slug, text}`: an agent reports a problem.
fn context_flag(live: &Live, request: &Request) -> Frame {
    let p: ContextParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let (task, project) = match token_scope(live, request, &p.token) {
        Ok(scope) => scope,
        Err(frame) => return *frame,
    };
    let (Some(slug), Some(text)) = (p.slug, p.text) else {
        return missing(request, "slug、text");
    };
    match live
        .executor
        .wikis()
        .flag(&project, &slug, &text, &task, now_ms())
    {
        Ok(issue) => ok(
            request,
            json!({ "text": format!("已记下 Wiki issue：{}", issue.id), "issue": issue }),
        ),
        Err(error) => wiki_failure(request, error),
    }
}
