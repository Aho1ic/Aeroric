//! D1 (2026-11-21): GitHub pull requests pinned as snapshots — read only.
//! The token is the local `gh` login's (`gh auth token`), asked for each
//! fetch and never kept; without one, public repositories still read.

use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::github::{self, client::Client, store};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};

use crate::dispatcher::{failure, ServiceSet};
use crate::services::ok;

/// `github.pullSnapshot {projectId, url}`: fetch the PR now and keep it
/// (the same head again returns the snapshot kept).
pub const PULL_SNAPSHOT_METHOD: &str = "github.pullSnapshot";
/// `github.snapshots {projectId}`: the project's snapshots, newest first.
pub const SNAPSHOTS_METHOD: &str = "github.snapshots";
/// `github.snapshot {projectId, id}`: one snapshot.
pub const SNAPSHOT_METHOD: &str = "github.snapshot";

const GH_TIMEOUT_SECS: u64 = 5;

pub fn register_github_services(set: &mut ServiceSet, context: Arc<CoreContext>) {
    let c = Arc::clone(&context);
    set.register(
        PULL_SNAPSHOT_METHOD,
        Arc::new(move |request: Request, _call| {
            let context = Arc::clone(&c);
            Box::pin(async move { pull_snapshot(request, context).await })
        }),
    );
    let c = Arc::clone(&context);
    set.register(
        SNAPSHOTS_METHOD,
        Arc::new(move |request: Request, _call| {
            let context = Arc::clone(&c);
            Box::pin(async move { snapshots(request, context) })
        }),
    );
    set.register(
        SNAPSHOT_METHOD,
        Arc::new(move |request: Request, _call| {
            let context = Arc::clone(&context);
            Box::pin(async move { snapshot(request, context) })
        }),
    );
}

fn param<'a>(request: &'a Request, key: &str) -> Result<&'a str, Box<Frame>> {
    request
        .params
        .get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| {
            Box::new(failure(
                request.id.clone(),
                ErrorCode::InvalidArgument,
                &format!("需要 {key}"),
            ))
        })
}

/// The local `gh` login's token for `host`, if there is one.
async fn gh_token(host: &str) -> Option<String> {
    let run = tokio::process::Command::new("gh")
        .args(["auth", "token", "--hostname", host])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    let out = tokio::time::timeout(std::time::Duration::from_secs(GH_TIMEOUT_SECS), run)
        .await
        .ok()?
        .ok()?;
    let token = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !token.is_empty()).then_some(token)
}

async fn pull_snapshot(request: Request, context: Arc<CoreContext>) -> Frame {
    let (project, link) = match (param(&request, "projectId"), param(&request, "url")) {
        (Ok(p), Ok(u)) => (p.to_string(), u.to_string()),
        (Err(frame), _) | (_, Err(frame)) => return *frame,
    };
    let target = match github::target(&link) {
        Ok(t) => t,
        Err(why) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &why),
    };
    let token = gh_token(&target.host).await;
    let fetched = match Client::new(&target.api_base(), token) {
        Ok(client) => client.pull(&target, &project, now_ms()).await,
        Err(why) => Err(why),
    };
    let snapshot = match fetched {
        Ok(s) => s,
        Err(why) => return failure(request.id.clone(), ErrorCode::IoError, &why),
    };
    let data_dir = context.paths.data_dir().to_path_buf();
    match tokio::task::spawn_blocking(move || store::save(&data_dir, &snapshot)).await {
        Ok(Ok(kept)) => ok(&request, json!({ "snapshot": kept })),
        Ok(Err(why)) => failure(request.id.clone(), ErrorCode::IoError, &why),
        Err(e) => failure(request.id.clone(), ErrorCode::IoError, &e.to_string()),
    }
}

fn snapshots(request: Request, context: Arc<CoreContext>) -> Frame {
    let project = match param(&request, "projectId") {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let listed = store::list(context.paths.data_dir(), project);
    ok(&request, json!({ "snapshots": listed }))
}

fn snapshot(request: Request, context: Arc<CoreContext>) -> Frame {
    let (project, id) = match (param(&request, "projectId"), param(&request, "id")) {
        (Ok(p), Ok(i)) => (p, i),
        (Err(frame), _) | (_, Err(frame)) => return *frame,
    };
    match store::get(context.paths.data_dir(), project, id) {
        Some(s) => ok(&request, json!({ "snapshot": s })),
        None => failure(request.id.clone(), ErrorCode::NotFound, "没有这份快照"),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
