//! D1 (2026-11-21): GitHub pull requests pinned as snapshots — read only.
//! The token is the local `gh` login's (`gh auth token`), asked for each
//! fetch and never kept; without one, public repositories still read.
//!
//! D2 (2026-11-21): a review's publishing draft and publishing it. These
//! are the App's methods only: the Context tools agents get have none of
//! them, so publishing is always the user's click. One publish per draft at
//! a time; a draft read while none is running and left sending reads as
//! unknown (the host stopped mid-send).

use std::collections::HashSet;
use std::sync::{Arc, LazyLock, Mutex};

use aeroric_core::context::CoreContext;
use aeroric_core::github::{self, client::Client, publish, review, store};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use aeroric_protocol::github::{PullReviewState, WriteIntent};
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

/// D2: `github.writeDraft {projectId, reviewId}` — the review's draft (one
/// per review: asked again, the same one).
pub const WRITE_DRAFT_METHOD: &str = "github.writeDraft";
/// D2: `github.writes {projectId}`.
pub const WRITES_METHOD: &str = "github.writes";
/// D2: `github.writeEdit {projectId, id, itemId, body?, included?}`.
pub const WRITE_EDIT_METHOD: &str = "github.writeEdit";
/// D2: `github.publish {projectId, id, resendUnknown?}` — the only write to
/// GitHub.
pub const PUBLISH_METHOD: &str = "github.publish";

const GH_TIMEOUT_SECS: u64 = 5;
const PUBLISH_TIMEOUT_SECS: u64 = 30;

/// Drafts being published now.
static PUBLISHING: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);

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
    let c = Arc::clone(&context);
    set.register(
        SNAPSHOT_METHOD,
        Arc::new(move |request: Request, _call| {
            let context = Arc::clone(&c);
            Box::pin(async move { snapshot(request, context) })
        }),
    );
    let c = Arc::clone(&context);
    set.register(
        WRITE_DRAFT_METHOD,
        Arc::new(move |request: Request, _call| {
            let context = Arc::clone(&c);
            Box::pin(async move { write_draft(request, context) })
        }),
    );
    let c = Arc::clone(&context);
    set.register(
        WRITES_METHOD,
        Arc::new(move |request: Request, _call| {
            let context = Arc::clone(&c);
            Box::pin(async move { writes(request, context) })
        }),
    );
    let c = Arc::clone(&context);
    set.register(
        WRITE_EDIT_METHOD,
        Arc::new(move |request: Request, _call| {
            let context = Arc::clone(&c);
            Box::pin(async move { write_edit(request, context) })
        }),
    );
    set.register(
        PUBLISH_METHOD,
        Arc::new(move |request: Request, _call| {
            let context = Arc::clone(&context);
            Box::pin(async move { publish_draft(request, context).await })
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

/// A draft as kept; one left sending while nothing publishes it reads as
/// unknown.
fn read_write(data_dir: &std::path::Path, project: &str, id: &str) -> Option<WriteIntent> {
    let mut intent = publish::load(data_dir, project, id)?;
    if !PUBLISHING.lock().unwrap().contains(id) {
        publish::recover(&mut intent);
    }
    Some(intent)
}

fn write_draft(request: Request, context: Arc<CoreContext>) -> Frame {
    let (project, review_id) = match (param(&request, "projectId"), param(&request, "reviewId")) {
        (Ok(p), Ok(r)) => (p, r),
        (Err(frame), _) | (_, Err(frame)) => return *frame,
    };
    let data_dir = context.paths.data_dir();
    if let Some(kept) = publish::list(data_dir, project)
        .into_iter()
        .find(|w| w.review_id == review_id)
    {
        let kept = read_write(data_dir, project, &kept.id).unwrap_or(kept);
        return ok(&request, json!({ "write": kept }));
    }
    let Some(pull_review) = review::get(data_dir, project, review_id) else {
        return failure(request.id.clone(), ErrorCode::NotFound, "没有这份审查");
    };
    if pull_review.state != PullReviewState::Done {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "这份审查还没有结论",
        );
    }
    let Some(snapshot) = store::get(data_dir, project, &pull_review.snapshot_id) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "这份审查的快照已经不在了",
        );
    };
    let draft = publish::draft(&pull_review, &snapshot, now_ms());
    match publish::save(data_dir, &draft) {
        Ok(()) => ok(&request, json!({ "write": draft })),
        Err(why) => failure(request.id.clone(), ErrorCode::IoError, &why),
    }
}

fn writes(request: Request, context: Arc<CoreContext>) -> Frame {
    let project = match param(&request, "projectId") {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let data_dir = context.paths.data_dir();
    let listed: Vec<WriteIntent> = publish::list(data_dir, project)
        .into_iter()
        .map(|w| read_write(data_dir, project, &w.id).unwrap_or(w))
        .collect();
    ok(&request, json!({ "writes": listed }))
}

fn write_edit(request: Request, context: Arc<CoreContext>) -> Frame {
    let (project, id, item) = match (
        param(&request, "projectId"),
        param(&request, "id"),
        param(&request, "itemId"),
    ) {
        (Ok(p), Ok(i), Ok(t)) => (p, i, t),
        (Err(frame), _, _) | (_, Err(frame), _) | (_, _, Err(frame)) => return *frame,
    };
    if PUBLISHING.lock().unwrap().contains(id) {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "正在发布，等它结束",
        );
    }
    let data_dir = context.paths.data_dir();
    let Some(mut intent) = read_write(data_dir, project, id) else {
        return failure(request.id.clone(), ErrorCode::NotFound, "没有这份发布草稿");
    };
    let body = request
        .params
        .get("body")
        .and_then(Value::as_str)
        .map(str::to_string);
    let included = request.params.get("included").and_then(Value::as_bool);
    if let Err(why) = publish::edit(&mut intent, item, body, included) {
        return failure(request.id.clone(), ErrorCode::InvalidArgument, &why);
    }
    match publish::save(data_dir, &intent) {
        Ok(()) => ok(&request, json!({ "write": intent })),
        Err(why) => failure(request.id.clone(), ErrorCode::IoError, &why),
    }
}

/// Lets the draft go when the publish ends, however it ends.
struct Publishing(String);

impl Drop for Publishing {
    fn drop(&mut self) {
        PUBLISHING.lock().unwrap().remove(&self.0);
    }
}

async fn publish_draft(request: Request, context: Arc<CoreContext>) -> Frame {
    let (project, id) = match (param(&request, "projectId"), param(&request, "id")) {
        (Ok(p), Ok(i)) => (p.to_string(), i.to_string()),
        (Err(frame), _) | (_, Err(frame)) => return *frame,
    };
    let resend = request
        .params
        .get("resendUnknown")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !PUBLISHING.lock().unwrap().insert(id.clone()) {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "正在发布，等它结束",
        );
    }
    let _held = Publishing(id.clone());
    let data_dir = context.paths.data_dir().to_path_buf();
    let Some(mut intent) = publish::load(&data_dir, &project, &id) else {
        return failure(request.id.clone(), ErrorCode::NotFound, "没有这份发布草稿");
    };
    // Held now: whatever says sending on disk was cut short.
    publish::recover(&mut intent);
    if !intent.items.iter().any(|i| i.included) {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "没有勾选要发布的条目",
        );
    }
    let target = github::PullTarget {
        host: intent.repo.host.clone(),
        owner: intent.repo.owner.clone(),
        name: intent.repo.name.clone(),
        number: intent.number,
    };
    let token = gh_token(&target.host).await;
    let writer = match publish::Writer::new(
        &target.api_base(),
        token,
        std::time::Duration::from_secs(PUBLISH_TIMEOUT_SECS),
    ) {
        Ok(w) => w,
        Err(why) => return failure(request.id.clone(), ErrorCode::IoError, &why),
    };
    let save = |w: &WriteIntent| publish::save(&data_dir, w);
    match publish::publish(&writer, &mut intent, resend, now_ms(), save).await {
        Ok(()) => ok(&request, json!({ "write": intent })),
        Err(why) => failure(request.id.clone(), ErrorCode::InvalidArgument, &why),
    }
}
