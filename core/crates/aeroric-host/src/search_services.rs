//! G43: one search engine, two compatible adapters (legacy project-wide and
//! native single-file). A supplied path selects a known root, never grants one.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::files::{path_policy, search};
use aeroric_core::storage::{load_projects, tasks::load_stamped};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use aeroric_protocol::project::ProjectTarget;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::dispatcher::{failure, Handler, ServiceSet};
use crate::file_access::{require_local_record, write_refusal};
use crate::notebook_services::NotebookHandle;
use crate::services::ok;

/// Process choices belong to the host. Tests inject missing/fake executables and
/// scratch HOME/config paths, never changing process-global environment.
#[derive(Clone, Debug)]
pub struct SearchPrograms {
    pub rg: String,
    pub git: Option<String>,
    pub env: Vec<(String, String)>,
}

impl Default for SearchPrograms {
    fn default() -> Self {
        Self {
            rg: "rg".into(),
            git: Some("git".into()),
            env: Vec::new(),
        }
    }
}

#[derive(Clone, Copy)]
enum Operation {
    Text,
    Structured,
    Preview,
    Apply,
    NativePreview,
    NativeApply,
    ListFiles,
    FindFiles,
}

pub fn register_search_services(
    set: &mut ServiceSet,
    context: Arc<CoreContext>,
    notebook: NotebookHandle,
    programs: SearchPrograms,
) {
    let handler = |operation| -> Handler {
        let context = Arc::clone(&context);
        let notebook = notebook.clone();
        let programs = programs.clone();
        Arc::new(move |request: Request, _call| {
            let context = Arc::clone(&context);
            let notebook = notebook.clone();
            let programs = programs.clone();
            Box::pin(async move {
                let id = request.id.clone();
                match tokio::task::spawn_blocking(move || {
                    handle(request, &context, &notebook, &programs, operation)
                })
                .await
                {
                    Ok(frame) => frame,
                    Err(error) => failure(id, ErrorCode::IoError, &error.to_string()),
                }
            })
        })
    };
    set.register("files.search_text", handler(Operation::Text));
    set.register("files.search_structured", handler(Operation::Structured));
    set.register("files.replace_text_preview", handler(Operation::Preview));
    set.register("files.apply_text_replacements", handler(Operation::Apply));
    set.register("files.replace_preview", handler(Operation::NativePreview));
    set.register("files.apply_replacements", handler(Operation::NativeApply));
    set.register("files.list_project_files", handler(Operation::ListFiles));
    set.register("files.search_project_files", handler(Operation::FindFiles));
}

fn rejected(request: &Request, code: ErrorCode, detail: &str) -> Box<Frame> {
    Box::new(failure(request.id.clone(), code, detail))
}

fn string<'a>(request: &'a Request, field: &str) -> Result<&'a str, String> {
    request
        .params
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("requires params.{field}"))
}

fn root_for(
    request: &Request,
    context: &Arc<CoreContext>,
    notebook: &NotebookHandle,
    operation: Operation,
) -> Result<PathBuf, Box<Frame>> {
    let target: Option<ProjectTarget> = request
        .params
        .get("target")
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()
        .map_err(|error| {
            rejected(
                request,
                ErrorCode::InvalidArgument,
                &format!("invalid target: {error}"),
            )
        })?;
    // ADR-006: remote twins address the parent method + target. Until a real
    // headless SSH owner exists, neither SSH nor WSL can fall through to local IO.
    if target.as_ref().is_some_and(|target| !target.is_local()) {
        return Err(rejected(
            request,
            ErrorCode::UnsupportedCapability,
            "Search target is not hosted: headless SSH/WSL search transport is unavailable",
        ));
    }
    if matches!(operation, Operation::Apply | Operation::NativeApply) {
        if let Some(frame) = write_refusal(request, context) {
            return Err(Box::new(frame));
        }
    }
    let projects = load_projects(context)
        .map_err(|error| rejected(request, ErrorCode::IoError, &error.to_string()))?;
    let validate = |path: &str| {
        path_policy::validate_project_root(path)
            .map_err(|error| rejected(request, ErrorCode::InvalidArgument, &error))
    };
    let requested = request
        .params
        .get("projectPath")
        .and_then(Value::as_str)
        .or_else(|| target.as_ref().and_then(ProjectTarget::local_path));
    if let (Some(explicit), Some(local)) = (
        request.params.get("projectPath").and_then(Value::as_str),
        target.as_ref().and_then(ProjectTarget::local_path),
    ) {
        if validate(explicit)? != validate(local)? {
            return Err(rejected(
                request,
                ErrorCode::InvalidArgument,
                "conflicting local search roots",
            ));
        }
    }
    if let Some(id) = request.params.get("projectId") {
        let id = id
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| rejected(request, ErrorCode::InvalidArgument, "invalid projectId"))?;
        let record = projects
            .iter()
            .find(|record| record.get("id").and_then(Value::as_str) == Some(id))
            .ok_or_else(|| rejected(request, ErrorCode::NotFound, "unknown projectId"))?;
        require_local_record(record).map_err(|error| Box::new(error.into_frame(request)))?;
        let root = validate(record.get("path").and_then(Value::as_str).ok_or_else(|| {
            rejected(
                request,
                ErrorCode::InvalidArgument,
                "project has no local path",
            )
        })?)?;
        if let Some(selected) = requested {
            if validate(selected)? != root {
                return Err(rejected(
                    request,
                    ErrorCode::InvalidArgument,
                    "projectId and path disagree",
                ));
            }
        }
        return Ok(root);
    }
    let selected = validate(requested.ok_or_else(|| {
        rejected(
            request,
            ErrorCode::InvalidArgument,
            "requires projectId or a registered projectPath",
        )
    })?)?;
    // Legacy SearchPanel also selects task worktrees and notebook vaults.
    // Only roots already owned by the host are candidates; never register here.
    for project in projects {
        // An ineligible stored project cannot authorize its raw path or tasks.
        // Other local projects and independently registered vaults remain usable.
        if require_local_record(&project).is_err() {
            continue;
        }
        if within_record_root(&selected, project.get("path").and_then(Value::as_str)) {
            return Ok(selected);
        }
        if let Some(id) = project.get("id").and_then(Value::as_str) {
            let tasks = load_stamped(context, id)
                .map_err(|error| rejected(request, ErrorCode::IoError, &error.to_string()))?;
            if tasks.records.iter().any(|task| {
                within_record_root(&selected, task.get("worktreePath").and_then(Value::as_str))
            }) {
                return Ok(selected);
            }
        }
    }
    let vaults = notebook
        .registered_vaults()
        .map_err(|error| rejected(request, ErrorCode::IoError, &error))?;
    if vaults.iter().any(|vault| selected.starts_with(vault)) {
        return Ok(selected);
    }
    Err(rejected(
        request,
        ErrorCode::NotFound,
        "search root is not a registered project, worktree or vault",
    ))
}

fn within_record_root(selected: &Path, raw: Option<&str>) -> bool {
    raw.and_then(|path| path_policy::validate_project_root(path).ok())
        .is_some_and(|root| selected.starts_with(root))
}

fn handle(
    request: Request,
    context: &Arc<CoreContext>,
    notebook: &NotebookHandle,
    programs: &SearchPrograms,
    operation: Operation,
) -> Frame {
    let root = match root_for(&request, context, notebook, operation) {
        Ok(root) => root,
        Err(frame) => return *frame,
    };
    let execute = || -> Result<Value, String> {
        let options: search::TextSearchOptions = serde_json::from_value(
            request
                .params
                .get("options")
                .cloned()
                .unwrap_or_else(|| json!({})),
        )
        .map_err(|error| format!("invalid options: {error}"))?;
        match operation {
            Operation::ListFiles => {
                serde_json::to_value(aeroric_core::files::project_files::list_project_files(
                    &root,
                    programs.git.as_deref(),
                    &programs.env,
                )?)
            }
            Operation::FindFiles => {
                let extensions: Vec<String> = serde_json::from_value(
                    request
                        .params
                        .get("extensions")
                        .cloned()
                        .ok_or("requires params.extensions")?,
                )
                .map_err(|error| format!("invalid extensions: {error}"))?;
                let limit: Option<usize> = serde_json::from_value(
                    request.params.get("limit").cloned().unwrap_or(Value::Null),
                )
                .map_err(|error| format!("invalid limit: {error}"))?;
                serde_json::to_value(aeroric_core::files::project_files::search_project_files(
                    &root,
                    string(&request, "query")?,
                    &extensions,
                    limit,
                    programs.git.as_deref().ok_or("Git is unavailable")?,
                    &programs.env,
                )?)
            }
            Operation::Text => serde_json::to_value(search::search_text_for_root(
                &root,
                string(&request, "query")?,
                &options,
                &programs.rg,
                &programs.env,
            )?),
            Operation::Structured => serde_json::to_value(search::structured_search_for_root(
                &root,
                string(&request, "pattern")?,
                &options,
            )?),
            Operation::Preview => serde_json::to_value(search::replace_text_preview_for_root(
                &root,
                string(&request, "query")?,
                string(&request, "replacement")?,
                &options,
            )?),
            Operation::Apply => {
                let replacements: Vec<search::TextReplacement> = serde_json::from_value(
                    request
                        .params
                        .get("replacements")
                        .cloned()
                        .ok_or("requires params.replacements")?,
                )
                .map_err(|error| format!("invalid replacements: {error}"))?;
                serde_json::to_value(search::apply_text_replacements_for_root(
                    &root,
                    &replacements,
                )?)
            }
            Operation::NativePreview => {
                // Native apply accepts one literal replacement, not each rich
                // match's expanded replacementText. Do not preview captures
                // that this wire shape cannot apply faithfully.
                if options.regex.unwrap_or(false) {
                    return Err(
                        "Native replacement is literal-only; use files.replace_text_preview for regex replacements"
                            .to_string(),
                    );
                }
                let mut preview = search::replace_text_preview_for_file(
                    &root,
                    string(&request, "path")?,
                    string(&request, "query")?,
                    string(&request, "replacement")?,
                    &options,
                )?;
                for file in &mut preview.files {
                    // Keep the client-selected relative spelling, including an
                    // in-root file symlink, so apply selects the same preview.
                    file.path = string(&request, "path")?.to_string();
                    for matched in &mut file.matches {
                        matched.path.clone_from(&file.path);
                    }
                }
                serde_json::to_value(preview)
            }
            Operation::NativeApply => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Match {
                    start: usize,
                    end: usize,
                    match_text: String,
                }
                let file = path_policy::resolve_within(&root, string(&request, "path")?)?;
                let matches: Vec<Match> = serde_json::from_value(
                    request
                        .params
                        .get("matches")
                        .cloned()
                        .ok_or("requires params.matches")?,
                )
                .map_err(|error| format!("invalid matches (matchText is required): {error}"))?;
                let replacement = string(&request, "replacement")?;
                let replacements: Vec<_> = matches
                    .into_iter()
                    .map(|matched| search::TextReplacement {
                        path: file.to_string_lossy().into_owned(),
                        start: matched.start,
                        end: matched.end,
                        match_text: matched.match_text,
                        replacement_text: replacement.to_string(),
                    })
                    .collect();
                serde_json::to_value(search::apply_text_replacements_for_root(
                    &root,
                    &replacements,
                )?)
            }
        }
        .map_err(|error| error.to_string())
    };
    match execute() {
        Ok(value) => ok(&request, value),
        Err(error) => failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    }
}
