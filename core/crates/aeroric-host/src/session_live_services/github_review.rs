//! D1 second step (2026-11-21): reviewing a pinned PR. `github.reviewStart`
//! fetches the snapshot's head into Aeroric's own ref, checks it out
//! detached, and starts a read-only (`plan`) session there with the
//! snapshot; reading a review takes the findings from that session's last
//! answer once its turn is over. Nothing is sent to GitHub.

use std::path::Path;
use std::sync::Arc;

use aeroric_core::github::{review, store};
use aeroric_core::sessions::live::isolated_copy;
use aeroric_core::storage::Record;
use aeroric_core::tasks::NewTask;
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use aeroric_protocol::github::{PullReview, PullReviewState};
use serde_json::{json, Value};

use super::{blocking, failure, now_ms, ok, Live};
use crate::dispatcher::ServiceSet;

/// `github.reviewStart {projectId, snapshotId, agent?}` (agent: `claude`
/// by default, or `codex`).
pub const REVIEW_START_METHOD: &str = "github.reviewStart";
/// `github.reviews {projectId}`: the project's reviews, newest first.
pub const REVIEWS_METHOD: &str = "github.reviews";
/// `github.review {projectId, id}`: one review.
pub const REVIEW_METHOD: &str = "github.review";

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    set.register(REVIEW_START_METHOD, blocking(live, review_start));
    set.register(REVIEWS_METHOD, blocking(live, reviews));
    set.register(REVIEW_METHOD, blocking(live, review_one));
}

fn text<'a>(request: &'a Request, key: &str) -> Result<&'a str, Box<Frame>> {
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

fn project(live: &Live, project_id: &str) -> Option<Record> {
    aeroric_core::projects::ProjectService::new(Arc::clone(&live.context))
        .list()
        .ok()?
        .into_iter()
        .find(|p| p.get("id").and_then(Value::as_str) == Some(project_id))
}

fn review_start(live: &Live, request: &Request) -> Frame {
    let (project_id, snapshot_id) = match (text(request, "projectId"), text(request, "snapshotId"))
    {
        (Ok(p), Ok(s)) => (p.to_string(), s.to_string()),
        (Err(frame), _) | (_, Err(frame)) => return *frame,
    };
    let agent = request
        .params
        .get("agent")
        .and_then(Value::as_str)
        .unwrap_or("claude")
        .to_string();
    let fail = |code, why: &str| failure(request.id.clone(), code, why);
    let data_dir = live.context.paths.data_dir().to_path_buf();
    let Some(snapshot) = store::get(&data_dir, &project_id, &snapshot_id) else {
        return fail(ErrorCode::NotFound, "没有这份快照");
    };
    if snapshot.superseded {
        return fail(
            ErrorCode::InvalidArgument,
            "这个 PR 已经有更新的快照，审最新的那份",
        );
    }
    let Some(project) = project(live, &project_id) else {
        return fail(ErrorCode::NotFound, "没有这个项目");
    };
    let Some(path) = project.get("path").and_then(Value::as_str) else {
        return fail(
            ErrorCode::InvalidArgument,
            "这个项目不在本机，不能取 PR 的代码",
        );
    };
    let checkout = isolated_copy::check(Path::new(path)).and_then(|top| {
        let remote = review::remote_for(&top, &snapshot.repo);
        review::fetch(&top, &remote, &snapshot)?;
        review::checkout(&top, &snapshot)
    });
    let checkout = match checkout {
        Ok(at) => at,
        Err(why) => return fail(ErrorCode::IoError, &why),
    };
    let task_id = format!("task-{}", uuid::Uuid::new_v4().simple());
    let created = live.service.create(
        &project,
        NewTask {
            id: task_id.clone(),
            project_id: project_id.clone(),
            prompt: review::prompt(&snapshot),
            agent,
            // Read only, held by the harness (A4).
            permission_mode: "plan".into(),
            immediate: false,
            selected_model: None,
            reasoning_effort: None,
            speed: None,
            dsh_agent_preset: None,
            agent_config_id: None,
            images: Vec::new(),
            texts: Vec::new(),
        },
    );
    if let Err(e) = created {
        return fail(ErrorCode::InvalidArgument, &e.to_string());
    }
    let at = checkout.to_string_lossy().to_string();
    let placed = live.service.mutate(&project_id, |records, _now| {
        let next = records
            .iter()
            .map(|r| {
                let mut r = r.clone();
                if r.get("id").and_then(Value::as_str) == Some(task_id.as_str()) {
                    r.0.insert("worktreePath".into(), Value::String(at.clone()));
                }
                r
            })
            .collect();
        (next, true)
    });
    if let Err(e) = placed {
        return fail(ErrorCode::IoError, &e.to_string());
    }
    let title: String = format!("审查 PR #{}：{}", snapshot.number, snapshot.title)
        .chars()
        .take(80)
        .collect();
    let _ = live.service.rename(&project_id, &task_id, &title);
    let pull_review = PullReview {
        id: format!("prrev-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]),
        project_id: project_id.clone(),
        snapshot_id: snapshot.id.clone(),
        head_sha: snapshot.head_sha.clone(),
        task_id: task_id.clone(),
        state: PullReviewState::Running,
        created_ms: now_ms(),
        ..PullReview::default()
    };
    if let Err(why) = review::save(&data_dir, &pull_review) {
        return fail(ErrorCode::IoError, &why);
    }
    let Some((project, _)) = live.record(&task_id) else {
        return fail(ErrorCode::IoError, "审查会话没有建成");
    };
    if let Err(e) = live.service.start(
        &project,
        &task_id,
        &format!("pr-review-{task_id}-{}", now_ms()),
        true,
        None,
        false,
        Default::default(),
        Default::default(),
    ) {
        return fail(ErrorCode::IoError, &e.to_string());
    }
    ok(request, json!({ "review": pull_review }))
}

/// A running review, once its session's turn is over: the findings from
/// its last answer, or why there are none.
fn settle(live: &Live, data_dir: &Path, mut r: PullReview) -> PullReview {
    if r.state == PullReviewState::Done {
        return r;
    }
    let before = r.clone();
    match live.record(&r.task_id) {
        None => {
            r.state = PullReviewState::Failed;
            r.reason = Some("审查会话已经不在了".into());
        }
        Some((_, record)) => {
            let status = record.get("status").and_then(Value::as_str).unwrap_or("");
            if matches!(status, "running" | "pending") {
                return r;
            }
            let snapshot = store::get(data_dir, &r.project_id, &r.snapshot_id);
            let checkout = record
                .get("worktreePath")
                .and_then(Value::as_str)
                .map(std::path::PathBuf::from);
            let answer = super::context_messages(live, &record, false)
                .ok()
                .and_then(|m| m.into_iter().rev().find(|(role, _)| role != "user"))
                .map(|(_, text)| text);
            let found = match (&snapshot, &checkout, &answer) {
                (Some(s), Some(c), Some(a)) => review::findings(a, s, c),
                _ => None,
            };
            match found {
                Some((findings, dropped)) => {
                    r.state = PullReviewState::Done;
                    r.findings = findings;
                    r.dropped = dropped;
                    r.reason = None;
                }
                None => {
                    r.state = PullReviewState::Failed;
                    r.reason = Some(match (snapshot.is_none(), answer.is_none()) {
                        (true, _) => "这份快照已经不在了".into(),
                        (_, true) => "审查会话没有回答".into(),
                        _ => "审查者的回答里没有按格式给出结论；在会话里让它补上 json 块后再打开"
                            .into(),
                    });
                }
            }
        }
    }
    if r != before {
        let _ = review::save(data_dir, &r);
    }
    r
}

fn reviews(live: &Live, request: &Request) -> Frame {
    let project_id = match text(request, "projectId") {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let data_dir = live.context.paths.data_dir().to_path_buf();
    let listed: Vec<PullReview> = review::list(&data_dir, project_id)
        .into_iter()
        .map(|r| settle(live, &data_dir, r))
        .collect();
    ok(request, json!({ "reviews": listed }))
}

fn review_one(live: &Live, request: &Request) -> Frame {
    let (project_id, id) = match (text(request, "projectId"), text(request, "id")) {
        (Ok(p), Ok(i)) => (p, i),
        (Err(frame), _) | (_, Err(frame)) => return *frame,
    };
    let data_dir = live.context.paths.data_dir().to_path_buf();
    match review::get(&data_dir, project_id, id) {
        Some(r) => ok(request, json!({ "review": settle(live, &data_dir, r) })),
        None => failure(request.id.clone(), ErrorCode::NotFound, "没有这份审查"),
    }
}
