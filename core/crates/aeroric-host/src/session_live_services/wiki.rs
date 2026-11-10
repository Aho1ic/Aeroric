//! B5.5-1 project Wiki (2026-10-31), the host's side: the `wiki.*` methods
//! the app calls, the `context.wiki*` methods behind `read_wiki`,
//! `search_wiki` and `flag_wiki_issue`, and the pass that recompiles a
//! project's Wiki when its notes, experiences or skills changed (Q-W6). The
//! Wikis are core's (`aeroric_core::knowledge`).

use std::path::PathBuf;
use std::sync::Arc;

use aeroric_core::knowledge::{code_index, compile::Sources, notes, WikiError};
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
    set.register("wiki.explainState", blocking(live, wiki_explain_state));
    set.register("wiki.explain", in_background(live, wiki_explain));
    set.register("wiki.explainAll", in_background(live, wiki_explain_all));
    set.register("context.wikiAvailable", blocking(live, context_available));
    set.register("context.wikiRead", blocking(live, context_read));
    set.register("context.wikiSearch", blocking(live, context_search));
    set.register("context.wikiFlag", blocking(live, context_flag));
    start_sweeper(live);
}

/// A handler that needs the `Arc` (it starts work that outlives the call).
fn in_background(
    live: &Arc<Live>,
    run: fn(Arc<Live>, &Request) -> Frame,
) -> crate::dispatcher::Handler {
    let live = Arc::clone(live);
    Arc::new(move |request: Request, _call| {
        let live = Arc::clone(&live);
        Box::pin(async move {
            let id = request.id.clone();
            match tokio::task::spawn_blocking(move || run(live, &request)).await {
                Ok(frame) => frame,
                Err(e) => failure(id, ErrorCode::IoError, &e.to_string()),
            }
        })
    })
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
const COMPILER_VERSION: u32 = 4;

/// What a project's Wiki compiles from: its notes, its and global
/// experiences, its and global accepted skills, its code (R4-1; read only
/// when `read_code`, the fingerprint needs just its file list); what could
/// not be read; and the fingerprint. A store error is an error, never an
/// empty source.
fn sources(
    live: &Live,
    project_id: &str,
    read_code: bool,
) -> Result<(Sources, WikiSourceGaps, String), WikiError> {
    let memory = live.executor.memory();
    let unavailable = |e: aeroric_core::memory::MemoryError| WikiError::Unavailable(e.to_string());
    let dir = project_dir(live, project_id);
    let (scan, notes_print) = match &dir {
        Some(dir) => (notes::scan_checked(dir), notes::fingerprint(dir)),
        None => (notes::Scan::default(), "unavailable".to_string()),
    };
    let code_print = dir
        .as_deref()
        .map(code_index::fingerprint)
        .unwrap_or_default();
    let code = match (&dir, read_code) {
        (Some(dir), true) => Some(code_index::scan(dir)),
        _ => None,
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
        "{COMPILER_VERSION}|{notes_print}|{code_print}|{}|{}|{latest}",
        experiences.len(),
        skills.len()
    );
    Ok((
        Sources {
            notes: scan.notes,
            experiences,
            skills,
            code,
        },
        gaps,
        fingerprint,
    ))
}

fn compile(live: &Live, project_id: &str) -> Result<CompileReport, WikiError> {
    let (sources, gaps, fingerprint) = sources(live, project_id, true)?;
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
    let mut active = Vec::new();
    for project in projects {
        let Some(id) = project.get("id").and_then(Value::as_str) else {
            continue;
        };
        if !memory.project_enabled(id) && !wikis.has_pages(id) {
            continue;
        }
        active.push(id.to_string());
        let Ok((_, _, fingerprint)) = sources(live, id, false) else {
            continue;
        };
        if wikis.fingerprint(id).as_deref() == Some(fingerprint.as_str()) {
            continue;
        }
        if compile(live, id).is_ok() {
            done += 1;
        }
    }
    explain_idle(live, &active);
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

// MARK: module explanations (R4-2)

/// A project's "全部生成" in progress.
#[derive(Clone, Default)]
struct ExplainJob {
    running: bool,
    done: usize,
    total: usize,
    error: Option<String>,
}

static EXPLAIN_JOBS: std::sync::LazyLock<
    parking_lot::Mutex<std::collections::HashMap<String, ExplainJob>>,
> = std::sync::LazyLock::new(Default::default);

/// `module:<id>#overview` → `<id>`.
fn module_of(page: &aeroric_protocol::wiki::WikiPage) -> Option<String> {
    page.blocks.iter().find_map(|b| {
        b.key
            .strip_prefix("module:")?
            .strip_suffix("#overview")
            .map(str::to_string)
    })
}

/// The idle pass regenerates a stale module at most this often (目检
/// 2026-11-10: a catch-all module whose files kept changing was generated
/// four times in one afternoon, using up the day's limit). The page still
/// says it may be stale; the buttons are not held back.
const AUTO_REGENERATE_AFTER_MS: u64 = 24 * 3600 * 1000;

/// Module ids whose explanation is missing or read a file that changed
/// since (the page's overview sources carry the current versions), stale
/// ones first; a stale one generated less than `cooldown_ms` ago is left
/// out.
fn wanted(live: &Live, project_id: &str, cooldown_ms: u64) -> Vec<String> {
    let wikis = live.executor.wikis();
    let Ok(explanations) = wikis.explanations(project_id) else {
        return Vec::new();
    };
    let Ok(pages) = wikis.pages(project_id) else {
        return Vec::new();
    };
    let mut stale = Vec::new();
    let mut missing = Vec::new();
    for summary in pages
        .iter()
        .filter(|p| p.kind == wire::WikiPageKind::Module)
    {
        let Ok(page) = wikis.page(project_id, &summary.slug) else {
            continue;
        };
        let Some(id) = module_of(&page) else {
            continue;
        };
        match explanations.get(&id) {
            None => missing.push(id),
            Some(e) => {
                let current: std::collections::BTreeMap<String, String> = page
                    .blocks
                    .iter()
                    .find(|b| b.key.ends_with("#overview"))
                    .map(|b| {
                        b.sources
                            .iter()
                            .filter_map(|s| Some((s.target.clone(), s.version.clone()?)))
                            .collect()
                    })
                    .unwrap_or_default();
                if !e.changed(&current).is_empty()
                    && now_ms().saturating_sub(e.at_ms) >= cooldown_ms
                {
                    stale.push(id);
                }
            }
        }
    }
    stale.extend(missing);
    stale
}

/// Generates the modules `ids` one by one, then compiles so the pages show
/// them. Stops at the first failed call (no model, no key, unreachable),
/// which is logged; an answer that is not the JSON asked for is logged and
/// skipped.
fn generate(
    live: &Live,
    project_id: &str,
    ids: &[String],
    auto: bool,
    mut progress: impl FnMut(usize),
) -> Result<usize, String> {
    use aeroric_core::knowledge::explain::{self, ExplainRun, Explanation};
    let model = live
        .executor
        .memory()
        .settings()
        .model
        .clone()
        .ok_or("还没有选分析模型（记忆页里设置）")?;
    let dir = project_dir(live, project_id).ok_or("读不到项目目录")?;
    let index = code_index::scan(&dir);
    let notes = notes::scan_checked(&dir).notes;
    let wikis = live.executor.wikis();
    let mut written = 0;
    for (n, id) in ids.iter().enumerate() {
        let Some(module) = index.modules.iter().find(|m| &m.id == id) else {
            progress(n + 1);
            continue;
        };
        let input = explain::build(module, &index, &dir, &notes);
        let mut run = ExplainRun {
            module_id: id.clone(),
            model: model.clone(),
            input_tokens: 0,
            output_tokens: 0,
            at_ms: now_ms(),
            auto,
            error: None,
        };
        let call = super::memory::analysis_call(live, explain::MAX_OUTPUT_TOKENS);
        match call(explain::SYSTEM, &explain::prompt(&input)) {
            Ok(chat) => {
                run.input_tokens = chat.input_tokens;
                run.output_tokens = chat.output_tokens;
                match explain::verify(&chat.text, &input) {
                    Ok((duties, pitfalls, dropped)) => {
                        wikis
                            .save_explanation(
                                project_id,
                                &Explanation {
                                    module_id: id.clone(),
                                    duties,
                                    pitfalls,
                                    versions: input.versions.clone(),
                                    model: model.clone(),
                                    input_tokens: chat.input_tokens,
                                    output_tokens: chat.output_tokens,
                                    at_ms: run.at_ms,
                                    dropped,
                                },
                            )
                            .map_err(|e| e.to_string())?;
                        written += 1;
                    }
                    Err(error) => run.error = Some(error),
                }
                let _ = wikis.log_explain(project_id, &run);
            }
            Err(error) => {
                run.error = Some(error.clone());
                let _ = wikis.log_explain(project_id, &run);
                return Err(error);
            }
        }
        progress(n + 1);
    }
    if written > 0 {
        compile(live, project_id).map_err(|e| e.to_string())?;
    }
    Ok(written)
}

/// Starts generating `ids` in the background, unless the project already
/// has a generation going (a button or the idle pass); `(queued, running)`.
/// Generating one module can take longer than the app waits for an answer
/// (目检 2026-11-10: "宿主没有及时回应", then the page had it), so even one
/// page is a job the app follows through `wiki.explainState`.
fn start_job(live: Arc<Live>, project: String, ids: Vec<String>) -> (usize, bool) {
    {
        let mut jobs = EXPLAIN_JOBS.lock();
        if jobs.get(&project).is_some_and(|j| j.running) {
            return (0, true);
        }
        jobs.insert(
            project.clone(),
            ExplainJob {
                running: !ids.is_empty(),
                done: 0,
                total: ids.len(),
                error: None,
            },
        );
    }
    let total = ids.len();
    if total > 0 {
        tokio::runtime::Handle::current().spawn_blocking(move || {
            let result = generate(&live, &project, &ids, false, |done| {
                if let Some(job) = EXPLAIN_JOBS.lock().get_mut(&project) {
                    job.done = done;
                }
            });
            if let Some(job) = EXPLAIN_JOBS.lock().get_mut(&project) {
                job.running = false;
                job.error = result.err();
            }
        });
    }
    (total, total > 0)
}

/// `wiki.explain {projectId, slug}`: one module page, in the background.
fn wiki_explain(live: Arc<Live>, request: &Request) -> Frame {
    let p = match project_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some(slug) = p.slug else {
        return missing(request, "slug");
    };
    let page = match live.executor.wikis().page(&p.project_id, &slug) {
        Ok(page) => page,
        Err(error) => return wiki_failure(request, error),
    };
    let Some(id) = module_of(&page) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "这一页不是模块页",
        );
    };
    let (queued, running) = start_job(live, p.project_id, vec![id]);
    ok(request, json!({ "queued": queued, "running": running }))
}

/// `wiki.explainAll {projectId}`: every module page missing an explanation
/// or with a stale one, in the background.
fn wiki_explain_all(live: Arc<Live>, request: &Request) -> Frame {
    let p = match project_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let ids = wanted(&live, &p.project_id, 0);
    let (queued, running) = start_job(live, p.project_id, ids);
    ok(request, json!({ "queued": queued, "running": running }))
}

/// Local midnight, in ms.
fn today_start_ms() -> u64 {
    use chrono::{Local, TimeZone};
    let now = Local::now();
    Local
        .from_local_datetime(&now.date_naive().and_hms_opt(0, 0, 0).unwrap_or_default())
        .single()
        .map(|t| t.timestamp_millis().max(0) as u64)
        .unwrap_or(0)
}

fn month_start_ms() -> u64 {
    use chrono::{Datelike, Local, TimeZone};
    let now = Local::now();
    now.date_naive()
        .with_day(1)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .and_then(|t| Local.from_local_datetime(&t).single())
        .map(|t| t.timestamp_millis().max(0) as u64)
        .unwrap_or(0)
}

/// `wiki.explainState {projectId}`: the background job, what this month's
/// generations cost, how many modules still want one, and the model.
fn wiki_explain_state(live: &Live, request: &Request) -> Frame {
    let p = match project_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let job = EXPLAIN_JOBS
        .lock()
        .get(&p.project_id)
        .cloned()
        .unwrap_or_default();
    let runs = live
        .executor
        .wikis()
        .explain_runs(&p.project_id, month_start_ms())
        .unwrap_or_default();
    ok(
        request,
        json!({
            "running": job.running,
            "done": job.done,
            "total": job.total,
            "error": job.error,
            "pending": wanted(live, &p.project_id, 0).len(),
            "monthRuns": runs.len(),
            "monthInputTokens": runs.iter().map(|r| r.input_tokens).sum::<u64>(),
            "monthOutputTokens": runs.iter().map(|r| r.output_tokens).sum::<u64>(),
            "model": live.executor.memory().settings().model,
        }),
    )
}

/// The idle pass's share: with no session running, one module a sweep, up
/// to the day's limit across projects.
fn explain_idle(live: &Live, projects: &[String]) {
    use aeroric_core::knowledge::explain::DAILY_LIMIT;
    if live.executor.memory().settings().model.is_none() {
        return;
    }
    let busy = live
        .executor
        .registry()
        .all()
        .iter()
        .any(|(_, driver)| driver.idle_since_ms().is_none());
    if busy || EXPLAIN_JOBS.lock().values().any(|j| j.running) {
        return;
    }
    let wikis = live.executor.wikis();
    let since = today_start_ms();
    let today: usize = projects
        .iter()
        .filter_map(|p| wikis.explain_runs(p, since).ok())
        .map(|runs| runs.iter().filter(|r| r.auto).count())
        .sum();
    if today >= DAILY_LIMIT as usize {
        return;
    }
    for project in projects {
        if let Some(id) = wanted(live, project, AUTO_REGENERATE_AFTER_MS)
            .into_iter()
            .next()
        {
            // Seen as running, so a button does not start the same module.
            EXPLAIN_JOBS.lock().insert(
                project.clone(),
                ExplainJob {
                    running: true,
                    done: 0,
                    total: 1,
                    error: None,
                },
            );
            let result = generate(live, project, &[id], true, |_| {});
            if let Some(job) = EXPLAIN_JOBS.lock().get_mut(project) {
                job.running = false;
                job.done = 1;
                job.error = result.err();
            }
            return;
        }
    }
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
