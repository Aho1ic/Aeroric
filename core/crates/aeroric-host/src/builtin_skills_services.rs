//! Built-in skills on the host (B2, 2026-10-30): offering them to each
//! session at launch, and the 技能 page's methods.
//!
//! Mounting never blocks a session: a failure is logged and the session
//! starts without the built-in skills.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::extensions::builtin_skills as skills;
use aeroric_core::projects::mutations;
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};

use crate::dispatcher::{failure, Handler, ServiceSet};
use crate::services::ok;

pub use skills::Mount;

/// Prepares the skills for one launch; returns the plugin folder for
/// [`Mount::ClaudePlugin`]. A failure is logged and the session starts
/// without them.
pub fn prepare(
    context: &CoreContext,
    harness: &str,
    cwd: Option<&Path>,
    mount: Mount,
) -> Option<PathBuf> {
    match skills::mount(context.paths.data_dir(), harness, cwd, mount) {
        Ok(plugin) => plugin,
        Err(reason) => {
            eprintln!("[aeroric] built-in skills not offered to {harness}: {reason}");
            None
        }
    }
}

pub fn register_builtin_skill_services(set: &mut ServiceSet, context: Arc<CoreContext>) {
    set.register("builtinSkills.list", blocking(&context, list));
    set.register("builtinSkills.setEnabled", blocking(&context, set_enabled));
    set.register(
        "builtinSkills.removeFromProject",
        blocking(&context, remove_from_project),
    );
    set.register(
        "builtinSkills.projectsWithCopies",
        blocking(&context, projects_with_copies),
    );
}

fn blocking(
    context: &Arc<CoreContext>,
    run: impl Fn(Request, Arc<CoreContext>) -> Frame + Send + Sync + 'static,
) -> Handler {
    let context = Arc::clone(context);
    let run = Arc::new(run);
    Arc::new(move |request: Request, _call| {
        let context = Arc::clone(&context);
        let run = Arc::clone(&run);
        Box::pin(async move {
            let id = request.id.clone();
            match tokio::task::spawn_blocking(move || run(request, context)).await {
                Ok(frame) => frame,
                Err(e) => failure(id, ErrorCode::IoError, &e.to_string()),
            }
        })
    })
}

/// A registered local project's folder; never an arbitrary path.
fn project_path(context: &CoreContext, request: &Request) -> Option<PathBuf> {
    let wanted = request.params.get("projectPath").and_then(Value::as_str)?;
    let projects = aeroric_core::storage::load_projects(context).ok()?;
    projects
        .iter()
        .filter_map(|record| mutations::text(record, "path"))
        .find(|path| *path == wanted)
        .map(PathBuf::from)
}

fn on_path(program: &str) -> bool {
    std::process::Command::new("/usr/bin/env")
        .args(["which", program])
        .envs(
            aeroric_core::settings::get_login_shell_env()
                .iter()
                .map(|(k, v)| (k, v)),
        )
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Every built-in skill with its switch, what shadows it per harness, what it
/// is missing, and whether the given project holds Aeroric's copies.
fn list(request: Request, context: Arc<CoreContext>) -> Frame {
    let data_dir = context.paths.data_dir();
    let settings = skills::load_settings(data_dir);
    let project = project_path(&context, &request);
    let home = aeroric_core::settings::home_dir();
    let shadows: Vec<(&str, BTreeSet<String>)> = ["claude", "codex", "omp", "dsh"]
        .into_iter()
        .map(|h| {
            (
                h,
                skills::user_skill_names(&skills::user_skill_dirs(
                    h,
                    project.as_deref(),
                    home.as_deref(),
                )),
            )
        })
        .collect();
    let has_python = on_path("python3");
    let has_uv = on_path("uv");
    let copies = project
        .as_deref()
        .map(skills::project_copies)
        .unwrap_or_default();
    let items: Vec<Value> = skills::catalog()
        .into_iter()
        .map(|skill| {
            let shadowed: Vec<&str> = shadows
                .iter()
                .filter(|(_, names)| names.contains(&skill.id))
                .map(|(h, _)| *h)
                .collect();
            let missing: Vec<&str> = skill
                .needs
                .iter()
                .filter(|need| need.as_str() == "python" && !has_python)
                .map(String::as_str)
                .collect();
            json!({
                "id": skill.id,
                "group": skill.group,
                "title": skill.title,
                "detail": skill.detail,
                "symbol": skill.symbol,
                "needs": skill.needs,
                "enabled": !settings.disabled.contains(&skill.id),
                "shadowedBy": shadowed,
                "missing": missing,
                "inProject": copies.contains(&skill.id),
            })
        })
        .collect();
    ok(
        &request,
        json!({ "skills": items, "hasPython": has_python, "hasUv": has_uv, "projectCopies": copies.len() }),
    )
}

fn set_enabled(request: Request, context: Arc<CoreContext>) -> Frame {
    let id = request.params.get("id").and_then(Value::as_str);
    let enabled = request.params.get("enabled").and_then(Value::as_bool);
    let (Some(id), Some(enabled)) = (id, enabled) else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "builtinSkills.setEnabled requires id and enabled",
        );
    };
    if !skills::catalog().iter().any(|s| s.id == id) {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "no built-in skill with that id",
        );
    }
    let data_dir = context.paths.data_dir();
    let mut settings = skills::load_settings(data_dir);
    if enabled {
        settings.disabled.remove(id);
    } else {
        settings.disabled.insert(id.to_string());
    }
    match skills::save_settings(data_dir, &settings) {
        Ok(()) => ok(&request, json!({ "enabled": enabled })),
        Err(e) => failure(request.id.clone(), ErrorCode::IoError, &e),
    }
}

fn remove_from_project(request: Request, context: Arc<CoreContext>) -> Frame {
    let Some(project) = project_path(&context, &request) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "builtinSkills.removeFromProject requires a registered projectPath",
        );
    };
    match skills::remove_from_project(&project, &skills::move_to_trash) {
        Ok(removed) => ok(&request, json!({ "removed": removed })),
        Err(e) => failure(request.id.clone(), ErrorCode::IoError, &e),
    }
}

/// Registered local projects that hold Aeroric's copies (for 技能 → 项目里的副本).
fn projects_with_copies(request: Request, context: Arc<CoreContext>) -> Frame {
    let projects = aeroric_core::storage::load_projects(&context).unwrap_or_default();
    let items: Vec<Value> = projects
        .iter()
        .filter_map(|record| {
            let path = mutations::text(record, "path")?;
            let count = skills::project_copies(Path::new(path)).len();
            (count > 0).then(|| {
                json!({
                    "path": path,
                    "name": mutations::text(record, "name").unwrap_or(path),
                    "count": count,
                })
            })
        })
        .collect();
    ok(&request, json!({ "projects": items }))
}
