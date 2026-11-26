//! B4-1 review loop (2026-10-30), the host's side: the `review.*` methods,
//! a task's settings (its record's `reviewLoop`), picking the reviewer, and
//! starting it as a read-only task marked `reviewOf` that runs in the same
//! working directory. The loop itself is core's
//! (`aeroric_core::sessions::live::review`).

use std::sync::{Arc, Weak};

use aeroric_core::agent_connections::launch::{catalog_id, config_id_from_catalog};
use aeroric_core::sessions::live::review::{
    fact_check_eval, FactCheckAnswer, ReviewError, ReviewHost, Reviewer,
};
use aeroric_core::storage::Record;
use aeroric_core::tasks::launch_plan::working_directory;
use aeroric_core::tasks::session_owner::launch_family_of;
use aeroric_core::tasks::WorkingDirectory;
use aeroric_core::tasks::{NewTask, ProtocolFamily};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use aeroric_protocol::review::{self as wire, ReviewSettings, ReviewerChoice};
use aeroric_protocol::session_live::TaskParams;
use aeroric_protocol::submission::AgentConfiguration;
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
    /// E2: the runtime the analysis model's call runs on; the loop calls
    /// from a plain thread.
    runtime: Option<tokio::runtime::Handle>,
}

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>, probe_installed: bool) {
    let reviews = live.executor.reviews();
    reviews.set_dir(live.context.paths.data_dir().join("reviews"));
    reviews.set_host(Arc::new(HostReviews {
        live: Arc::downgrade(live),
        probe_installed,
        runtime: tokio::runtime::Handle::try_current().ok(),
    }));
    set.register(wire::GET_METHOD, blocking(live, review_get));
    set.register(wire::SET_METHOD, blocking(live, review_set));
    set.register(wire::REQUEST_METHOD, blocking(live, review_request));
    set.register(wire::SEND_METHOD, blocking(live, review_send));
    set.register(wire::RETRY_METHOD, blocking(live, review_retry));
    set.register(wire::REVIEWERS_METHOD, blocking(live, review_reviewers));
    set.register(wire::ATTENTION_METHOD, blocking(live, review_attention));
    set.register(wire::RULES_METHOD, blocking(live, review_rules));
    set.register(
        wire::FACT_CHECK_EVAL_METHOD,
        blocking(live, review_fact_check_eval),
    );
}

/// One harness that could review a task.
#[derive(Clone, Debug)]
struct Candidate {
    agent: &'static str,
    family: ProtocolFamily,
    label: &'static str,
}

/// The reviewer [`HostReviews::pick`] settled on.
struct Pick {
    candidate: Candidate,
    /// The Agent configuration's id (no catalog prefix).
    config: Option<String>,
    model: Option<String>,
    same_family: bool,
    /// "Codex", "Claude Code · sonnet", or "配置 名字 · 模型".
    label: String,
}

/// The project's session catalog (its Agent configurations among them).
fn project_catalog(live: &Live, project: &Record) -> Result<Vec<AgentConfiguration>, String> {
    let id = project
        .get("id")
        .and_then(Value::as_str)
        .ok_or("会话的项目没有 id")?;
    aeroric_core::settings::session_catalog::for_project(&live.context, id)
        .map(|(_, catalog)| catalog.configurations)
}

/// 2026-11-22: the project's Agent configurations that can review — they
/// run a reviewing harness available here and nothing blocks them. Built-in
/// entries are the candidates themselves and are not repeated.
fn config_reviewers(
    catalog: &[AgentConfiguration],
    candidates: &[Candidate],
    writer: ProtocolFamily,
) -> Vec<Value> {
    catalog
        .iter()
        .filter(|c| c.blocked_reasons.is_empty())
        .filter_map(|c| {
            let id = config_id_from_catalog(&c.id)?;
            let candidate = candidates.iter().find(|k| k.agent == c.agent)?;
            Some(json!({
                "agentConfigId": id,
                "title": c.display_name,
                "agent": c.agent,
                "models": c.models,
                "sameFamily": candidate.family == writer,
            }))
        })
        .collect()
}

/// A named reviewer's Agent configuration checked against the catalog;
/// its label ("配置名 · 模型"), or why it cannot review.
fn config_label(
    catalog: &[AgentConfiguration],
    agent: &str,
    config_id: &str,
    model: Option<&str>,
) -> Result<String, String> {
    let picked =
        super::todo::pick_configuration(catalog, agent, model, Some(&catalog_id(config_id)))?;
    Ok(picked.label.unwrap_or_else(|| config_id.to_string()))
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
    ) -> Result<Pick, String> {
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
                Ok(Pick {
                    label: pick.label.to_string(),
                    candidate: pick,
                    config: None,
                    model: None,
                    same_family: same,
                })
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
                let mut label = pick.label.to_string();
                // 2026-11-22: an Agent configuration is checked as 交办 checks
                // one — it exists, nothing blocks it, it is this Agent's and
                // offers the model — and names the reviewer.
                if let Some(id) = agent_config_id {
                    label = config_label(
                        &project_catalog(live, project)?,
                        agent,
                        id,
                        model.as_deref(),
                    )?;
                } else if let Some(model) = model {
                    label = format!("{label} · {model}");
                }
                Ok(Pick {
                    candidate: pick,
                    config: agent_config_id.clone(),
                    model: model.clone(),
                    same_family: same,
                    label,
                })
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
        // E6: a reviewer is unattended work.
        if let Some(why) = super::budget::hold(&live, Some(parent)) {
            return Err(format!("审查没有开始：{why}"));
        }
        let (project, record) = live.record(parent).ok_or("会话不存在")?;
        let settings = settings_of(&record).ok_or("这个会话不能被审查")?;
        let Pick {
            candidate: pick,
            config,
            model,
            same_family,
            label,
        } = self.pick(&live, &project, &record, &settings.reviewer)?;
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
        Ok(Reviewer {
            task_id: id,
            label,
            same_family,
        })
    }

    /// R1-2: comments and review requests go through the delivery queue.
    fn prompt(&self, task_id: &str, text: String) -> Result<(), String> {
        let live = self.live.upgrade().ok_or("宿主已关闭")?;
        super::delivery::enqueue_internal(
            &live,
            task_id,
            aeroric_protocol::delivery::DeliveryKind::Review,
            text,
            None,
        )
        .map(|_| ())
    }

    /// E2: the memory's analysis model (its 整理设置), as the memory and the
    /// Wiki call it.
    fn fact_check(&self, system: &str, user: &str) -> Result<FactCheckAnswer, String> {
        let live = self.live.upgrade().ok_or("宿主已关闭")?;
        let model = live.executor.memory().settings().model;
        let call = super::memory::analysis_call(
            &live,
            aeroric_core::sessions::live::review::fact_check::MAX_TOKENS,
        );
        let reply = match &self.runtime {
            Some(handle) => {
                let _entered = handle.enter();
                call(system, user)
            }
            None => {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(1)
                    .enable_all()
                    .build()
                    .map_err(|e| e.to_string())?;
                let _entered = runtime.enter();
                call(system, user)
            }
        }
        .map_err(|e| {
            if e.contains("没有选") {
                "还没有选分析模型（记忆页的“整理设置”）".to_string()
            } else {
                e
            }
        })?;
        Ok(FactCheckAnswer {
            text: reply.text,
            model,
        })
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
    #[serde(default)]
    fact_check: Option<bool>,
}

/// `review.set {taskId, enabled?, reviewer?, paused?, factCheck?}`: applies
/// from the task's next turn on; answers the settings now in force.
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
    if let Some(fact_check) = p.fact_check {
        settings.fact_check = fact_check;
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
        runtime: None,
    };
    let writer = launch_family_of(&record, live.families.as_ref());
    let found = host.candidates(live, &project, &record);
    let candidates: Vec<Value> = found
        .iter()
        .map(|c| json!({"agent": c.agent, "label": c.label, "sameFamily": c.family == writer}))
        .collect();
    // An unreadable catalog leaves the harnesses; it does not fail the list.
    let configurations = project_catalog(live, &project)
        .map(|catalog| config_reviewers(&catalog, &found, writer))
        .unwrap_or_default();
    let auto = host
        .pick(live, &project, &record, &ReviewerChoice::Auto)
        .ok()
        .map(
            |p| json!({"agent": p.candidate.agent, "label": p.label, "sameFamily": p.same_family}),
        );
    ok(
        request,
        json!({ "candidates": candidates, "configurations": configurations, "auto": auto }),
    )
}

/// `review.rules {taskId}` (E4): the review rules in force for the task's
/// repository as it is on disk now — built-ins, and the project file's.
fn review_rules(live: &Live, request: &Request) -> Frame {
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
    use aeroric_core::sessions::live::review::rules;
    let rules = match working_directory(&record, &project) {
        Some(WorkingDirectory::Local { path }) => rules::load(&repo_top(&path), ""),
        _ => wire::ReviewRules {
            file: rules::FILE.into(),
            state: wire::RulesFileState::Unavailable,
            error: None,
            rules: rules::builtin(),
        },
    };
    ok(request, json!({ "rules": rules }))
}

/// The repository's top for `cwd`, or `cwd` itself outside one.
fn repo_top(cwd: &str) -> std::path::PathBuf {
    std::process::Command::new("git")
        .args(["-C", cwd, "rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|top| !top.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| cwd.into())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn config(id: &str, agent: &str, models: &[&str], blocked: &[&str]) -> AgentConfiguration {
        AgentConfiguration {
            id: id.into(),
            version: "v1".into(),
            display_name: format!("配置 {id}"),
            agent: agent.into(),
            harness: agent.into(),
            models: models.iter().map(|m| m.to_string()).collect(),
            efforts: Vec::new(),
            permission_modes: Vec::new(),
            blocked_reasons: blocked.iter().map(|b| b.to_string()).collect(),
            model_details: Vec::new(),
        }
    }

    fn catalog() -> Vec<AgentConfiguration> {
        vec![
            config("codex", "codex", &["gpt-6.1-sol"], &[]),
            config(
                "agent-config:magpie",
                "claude",
                &["42/deepseek-flash", "42/deepseek-pro"],
                &[],
            ),
            config("agent-config:sol", "codex", &["gpt-6.2-sol"], &[]),
            config("agent-config:off", "claude", &[], &["连接没有可用的模型"]),
            config("agent-config:omp", "omp", &[], &[]),
        ]
    }

    fn candidates() -> Vec<Candidate> {
        REVIEWERS
            .iter()
            .map(|(agent, family, label)| Candidate {
                agent,
                family: *family,
                label,
            })
            .collect()
    }

    /// 2026-11-22 (目检 C1 没测成): the progress page can pick an Agent
    /// configuration as the reviewer — the project's own, of a reviewing
    /// harness, not blocked; built-ins are the harness rows already.
    #[test]
    fn agent_configurations_of_reviewing_harnesses_are_offered() {
        let offered = config_reviewers(&catalog(), &candidates(), ProtocolFamily::Codex);
        assert_eq!(offered.len(), 2, "{offered:?}");
        assert_eq!(offered[0]["agentConfigId"], "magpie");
        assert_eq!(offered[0]["title"], "配置 agent-config:magpie");
        assert_eq!(offered[0]["agent"], "claude");
        assert_eq!(offered[0]["models"][0], "42/deepseek-flash");
        assert_eq!(offered[0]["sameFamily"], false);
        assert_eq!(offered[1]["agentConfigId"], "sol");
        assert_eq!(offered[1]["sameFamily"], true);

        // A harness that is not available here offers none of its configurations.
        let codex_only: Vec<Candidate> = candidates()
            .into_iter()
            .filter(|c| c.agent == "codex")
            .collect();
        let offered = config_reviewers(&catalog(), &codex_only, ProtocolFamily::Claude);
        assert_eq!(offered.len(), 1);
        assert_eq!(offered[0]["agentConfigId"], "sol");
    }

    #[test]
    fn a_picked_configuration_is_checked_and_names_the_reviewer() {
        let label = config_label(&catalog(), "claude", "magpie", Some("42/deepseek-flash"));
        assert_eq!(
            label.unwrap(),
            "配置 agent-config:magpie · 42/deepseek-flash"
        );
        let gone = config_label(&catalog(), "claude", "gone", None).unwrap_err();
        assert!(gone.contains("配置不存在"), "{gone}");
        let blocked = config_label(&catalog(), "claude", "off", None).unwrap_err();
        assert!(blocked.contains("现在不能用"), "{blocked}");
        let model = config_label(&catalog(), "claude", "magpie", Some("gpt-4")).unwrap_err();
        assert!(model.contains("不支持模型 gpt-4"), "{model}");
        let other = config_label(&catalog(), "codex", "magpie", None).unwrap_err();
        assert!(other.contains("不是 codex"), "{other}");
    }
}

/// `review.factCheckEval {}` (E2, 2026-11-26): the fact check's fixed cases
/// through the analysis model; `wronglyRemoved` is how many right findings
/// it took out. Calls the model once per case.
fn review_fact_check_eval(live: &Live, request: &Request) -> Frame {
    let model = live.executor.memory().settings().model;
    if model.is_none() {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "还没有选分析模型：在记忆页的“整理设置”里选一个",
        );
    }
    let call = super::memory::analysis_call(
        live,
        aeroric_core::sessions::live::review::fact_check::MAX_TOKENS,
    );
    let report = fact_check_eval::run(|system, user| {
        call(system, user).map(|reply| fact_check_eval::Answer {
            text: reply.text,
            input_tokens: reply.input_tokens,
            output_tokens: reply.output_tokens,
        })
    });
    ok(request, json!({ "model": model, "report": report }))
}
