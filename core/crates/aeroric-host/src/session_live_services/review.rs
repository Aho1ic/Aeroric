//! B4-1 review loop (2026-10-30), the host's side: the `review.*` methods,
//! a task's settings (its record's `reviewLoop`), picking the reviewer, and
//! starting it as a read-only task marked `reviewOf` that runs in the same
//! working directory. The loop itself is core's
//! (`aeroric_core::sessions::live::review`).

use std::sync::{Arc, Weak};

use aeroric_core::sessions::live::review::{ReviewError, ReviewHost, Reviewer};
use aeroric_core::sessions::live::SessionDriver;
use aeroric_core::storage::Record;
use aeroric_core::tasks::launch_plan::working_directory;
use aeroric_core::tasks::session_owner::launch_family_of;
use aeroric_core::tasks::{NewTask, ProtocolFamily};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use aeroric_protocol::review::{self as wire, ReviewSettings, ReviewerChoice};
use aeroric_protocol::session_live::TaskParams;
use serde_json::{json, Value};

use super::{blocking, failure, now_ms, ok, params, Live, BRANCH_OF_FIELD};
use crate::dispatcher::ServiceSet;

/// The record field naming the task a reviewer reviews.
pub(super) const REVIEW_OF_FIELD: &str = "reviewOf";
/// The record field holding a task's [`ReviewSettings`].
const SETTINGS_FIELD: &str = "reviewLoop";
/// Harnesses that can review: their read-only mode is enforced by the agent.
pub(super) const REVIEWERS: [(&str, ProtocolFamily, &str); 2] = [
    ("claude", ProtocolFamily::Claude, "Claude Code"),
    ("codex", ProtocolFamily::Codex, "Codex"),
];

pub(super) struct HostReviews {
    live: Weak<Live>,
    /// Ask whether the harness is installed (the product binary; a test host
    /// routes to fakes and installs nothing).
    probe_installed: bool,
}

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>, probe_installed: bool) {
    let reviews = live.executor.reviews();
    reviews.set_dir(live.context.paths.data_dir().join("reviews"));
    reviews.set_host(Arc::new(HostReviews {
        live: Arc::downgrade(live),
        probe_installed,
    }));
    set.register(wire::GET_METHOD, blocking(live, review_get));
    set.register(wire::SET_METHOD, blocking(live, review_set));
    set.register(wire::REQUEST_METHOD, blocking(live, review_request));
    set.register(wire::SEND_METHOD, blocking(live, review_send));
    set.register(wire::RETRY_METHOD, blocking(live, review_retry));
    set.register(wire::REVIEWERS_METHOD, blocking(live, review_reviewers));
    set.register(wire::ATTENTION_METHOD, blocking(live, review_attention));
}

/// One harness that could review a task.
#[derive(Clone, Debug)]
struct Candidate {
    agent: &'static str,
    family: ProtocolFamily,
    label: &'static str,
}

fn settings_of(record: &Record) -> Option<ReviewSettings> {
    if record.get(REVIEW_OF_FIELD).is_some() || record.get(BRANCH_OF_FIELD).is_some() {
        return None;
    }
    Some(
        record
            .get(SETTINGS_FIELD)
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default(),
    )
}

impl HostReviews {
    fn candidates(&self, live: &Live, project: &Record, record: &Record) -> Vec<Candidate> {
        let Some(cwd) = working_directory(record, project) else {
            return Vec::new();
        };
        REVIEWERS
            .iter()
            .filter(|(agent, family, _)| live.executor.takes_task(*family, agent, None, &cwd))
            .filter(|(agent, _, _)| {
                !self.probe_installed
                    || aeroric_core::settings::installed_agent_version(agent).is_some()
            })
            .map(|(agent, family, label)| Candidate {
                agent,
                family: *family,
                label,
            })
            .collect()
    }

    /// The reviewer for `choice`, and whether it shares the writer's family.
    fn pick(
        &self,
        live: &Live,
        project: &Record,
        record: &Record,
        choice: &ReviewerChoice,
    ) -> Result<(Candidate, Option<String>, Option<String>, bool), String> {
        let writer = launch_family_of(record, live.families.as_ref());
        let candidates = self.candidates(live, project, record);
        match choice {
            ReviewerChoice::Auto => {
                let other = candidates.iter().find(|c| c.family != writer);
                let pick = other
                    .or_else(|| candidates.first())
                    .cloned()
                    .ok_or("本机没有可以当审查者的 Claude Code 或 Codex")?;
                let same = pick.family == writer;
                Ok((pick, None, None, same))
            }
            ReviewerChoice::Named {
                agent,
                agent_config_id,
                model,
            } => {
                let pick = candidates
                    .iter()
                    .find(|c| c.agent == agent)
                    .cloned()
                    .ok_or_else(|| format!("审查者 {agent} 在本机不可用"))?;
                let same = pick.family == writer;
                Ok((pick, agent_config_id.clone(), model.clone(), same))
            }
        }
    }
}

impl ReviewHost for HostReviews {
    fn settings(&self, task_id: &str) -> Option<ReviewSettings> {
        let live = self.live.upgrade()?;
        let (_, record) = live.record(task_id)?;
        settings_of(&record)
    }

    fn start_reviewer(&self, parent: &str, prompt: String) -> Result<Reviewer, String> {
        let live = self.live.upgrade().ok_or("宿主已关闭")?;
        let (project, record) = live.record(parent).ok_or("会话不存在")?;
        let settings = settings_of(&record).ok_or("这个会话不能被审查")?;
        let (pick, config, model, same_family) =
            self.pick(&live, &project, &record, &settings.reviewer)?;
        let project_id = project
            .get("id")
            .and_then(Value::as_str)
            .ok_or("会话的项目没有 id")?
            .to_string();
        let id = format!("task-{}", uuid::Uuid::new_v4().simple());
        live.service
            .create(
                &project,
                NewTask {
                    id: id.clone(),
                    project_id: project_id.clone(),
                    prompt,
                    agent: pick.agent.to_string(),
                    permission_mode: "plan".into(),
                    immediate: false,
                    selected_model: model.clone(),
                    reasoning_effort: None,
                    speed: None,
                    dsh_agent_preset: None,
                    agent_config_id: config,
                    images: Vec::new(),
                    texts: Vec::new(),
                },
            )
            .map_err(|e| e.to_string())?;
        // Same working directory as the writer (its worktree included).
        let worktree: Vec<(String, Value)> = record
            .0
            .iter()
            .filter(|(key, _)| key.starts_with("worktree"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let (reviewer, parent_id) = (id.clone(), parent.to_string());
        live.service
            .mutate(&project_id, |records, _now| {
                let next = records
                    .iter()
                    .map(|r| {
                        let mut r = r.clone();
                        if r.get("id").and_then(Value::as_str) == Some(reviewer.as_str()) {
                            r.0.insert(REVIEW_OF_FIELD.into(), Value::String(parent_id.clone()));
                            for (key, value) in &worktree {
                                r.0.insert(key.clone(), value.clone());
                            }
                        }
                        r
                    })
                    .collect();
                (next, true)
            })
            .map_err(|e| e.to_string())?;
        let title = ["name", "title"]
            .iter()
            .find_map(|key| {
                record
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|n| !n.is_empty() && *n != pick.agent)
                    .map(str::to_string)
            })
            .or_else(|| {
                record
                    .get("prompt")
                    .and_then(Value::as_str)
                    .and_then(|p| p.lines().next())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "会话".into());
        let title: String = title.chars().take(60).collect();
        live.service
            .rename(&project_id, &id, &format!("{title}（审查）"))
            .map_err(|e| e.to_string())?;
        let (project, _) = live.record(&id).ok_or("审查会话没有建成")?;
        live.service
            .start(
                &project,
                &id,
                &format!("review-start-{id}-{}", now_ms()),
                true,
                None,
                false,
                Default::default(),
                Default::default(),
            )
            .map_err(|e| e.to_string())?;
        let label = match &model {
            Some(model) => format!("{} · {model}", pick.label),
            None => pick.label.to_string(),
        };
        Ok(Reviewer {
            task_id: id,
            label,
            same_family,
        })
    }

    fn prompt(&self, task_id: &str, text: String) -> Result<(), String> {
        let live = self.live.upgrade().ok_or("宿主已关闭")?;
        let driver: Arc<dyn SessionDriver> = match live.driver(task_id) {
            Some(driver) => driver,
            None => live.reopen_quiet(task_id)?,
        };
        driver.prompt(text).map_err(|e| e.to_string())
    }
}

fn review_failure(request: &Request, error: ReviewError) -> Frame {
    let code = match error {
        ReviewError::NotReviewable => ErrorCode::NotFound,
        _ => ErrorCode::InvalidArgument,
    };
    failure(request.id.clone(), code, &error.to_string())
}

fn task_params(request: &Request) -> Result<TaskParams, Box<Frame>> {
    params(request)
}

/// `review.get {taskId}`: the settings, the latest card, and the task this
/// one reviews (when it is a reviewer).
fn review_get(live: &Live, request: &Request) -> Frame {
    let p = match task_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some((_, record)) = live.record(&p.task_id) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "no task with that id in any known project",
        );
    };
    ok(
        request,
        json!({
            "settings": settings_of(&record),
            "card": live.executor.reviews().card(&p.task_id),
            "reviewOf": record.get(REVIEW_OF_FIELD),
        }),
    )
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SetParams {
    task_id: String,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    reviewer: Option<ReviewerChoice>,
    #[serde(default)]
    paused: Option<bool>,
}

/// `review.set {taskId, enabled?, reviewer?, paused?}`: applies from the
/// task's next turn on; answers the settings now in force.
fn review_set(live: &Live, request: &Request) -> Frame {
    let p: SetParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some((project, record)) = live.record(&p.task_id) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "no task with that id in any known project",
        );
    };
    let Some(mut settings) = settings_of(&record) else {
        return review_failure(request, ReviewError::NotReviewable);
    };
    if let Some(enabled) = p.enabled {
        settings.enabled = enabled;
    }
    if let Some(reviewer) = p.reviewer {
        settings.reviewer = reviewer;
    }
    if let Some(paused) = p.paused {
        settings.paused = paused;
    }
    let project_id = project
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let value = serde_json::to_value(&settings).expect("settings serialize");
    let task = p.task_id.clone();
    let written = live.service.mutate(&project_id, |records, _now| {
        let next = records
            .iter()
            .map(|r| {
                let mut r = r.clone();
                if r.get("id").and_then(Value::as_str) == Some(task.as_str()) {
                    r.0.insert(SETTINGS_FIELD.into(), value.clone());
                }
                r
            })
            .collect();
        (next, true)
    });
    if let Err(error) = written {
        return failure(request.id.clone(), ErrorCode::IoError, &error.to_string());
    }
    ok(request, json!({ "settings": settings }))
}

/// `review.request {taskId}`: review the task's last changed turn now.
fn review_request(live: &Live, request: &Request) -> Frame {
    let p = match task_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match live.executor.reviews().request(&p.task_id) {
        Ok(card) => ok(request, json!({ "card": card })),
        Err(error) => review_failure(request, error),
    }
}

/// `review.send {taskId}`: send findings a paused loop held back.
fn review_send(live: &Live, request: &Request) -> Frame {
    let p = match task_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match live.executor.reviews().send(&p.task_id) {
        Ok(()) => ok(request, json!({ "accepted": true })),
        Err(error) => review_failure(request, error),
    }
}

/// `review.retry {taskId}`: run a failed or interrupted round again.
fn review_retry(live: &Live, request: &Request) -> Frame {
    let p = match task_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match live.executor.reviews().retry(&p.task_id) {
        Ok(()) => ok(request, json!({ "accepted": true })),
        Err(error) => review_failure(request, error),
    }
}

/// `review.reviewers {taskId}`: who could review this task, and whom
/// `auto` would pick.
fn review_reviewers(live: &Live, request: &Request) -> Frame {
    let p = match task_params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some((project, record)) = live.record(&p.task_id) else {
        return failure(
            request.id.clone(),
            ErrorCode::NotFound,
            "no task with that id in any known project",
        );
    };
    let host = HostReviews {
        live: Weak::new(),
        probe_installed: live.probe_installed(),
    };
    let writer = launch_family_of(&record, live.families.as_ref());
    let candidates: Vec<Value> = host
        .candidates(live, &project, &record)
        .into_iter()
        .map(|c| json!({"agent": c.agent, "label": c.label, "sameFamily": c.family == writer}))
        .collect();
    let auto = host
        .pick(live, &project, &record, &ReviewerChoice::Auto)
        .ok()
        .map(|(c, _, _, same)| json!({"agent": c.agent, "label": c.label, "sameFamily": same}));
    ok(request, json!({ "candidates": candidates, "auto": auto }))
}

/// `review.attention {}`: loops that need the user ("需要我处理").
fn review_attention(live: &Live, request: &Request) -> Frame {
    let items: Vec<Value> = live
        .executor
        .reviews()
        .attention()
        .into_iter()
        .map(|(task, card)| json!({"taskId": task, "card": card}))
        .collect();
    ok(request, json!({ "items": items }))
}
