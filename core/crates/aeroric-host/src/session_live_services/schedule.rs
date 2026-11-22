//! A12 第一批 (2026-11-14): 定时任务. The host fires the rules every
//! `TICK_MS`, and opens their sessions the way the 新会话 button does
//! (`task.run`), with the Agent's default permission mode.
//!
//! 配额感知调度 (2026-11-14): a rule for the built-in Codex looks at the
//! subscription first — the sampler's readings when one is under
//! `QUOTA_FRESH_MS` old, else a read now; no reading runs as usual.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aeroric_core::metrics::quota_pace::{self, QuotaState};
use aeroric_core::schedule::{ScheduleEnv, Schedules};
use aeroric_core::tasks::ProtocolFamily;
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use aeroric_protocol::schedule::{self as wire, ScheduleRule};
use serde_json::{json, Value};

use super::{blocking, failure, ok, params, Live};
use crate::dispatcher::ServiceSet;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn schedules(live: &Live) -> Schedules {
    Schedules::open(live.context.paths.data_dir())
}

/// A project as the projects list has it.
pub(super) fn project(live: &Live, project_id: &str) -> Option<aeroric_core::storage::Record> {
    aeroric_core::projects::ProjectService::new(Arc::clone(&live.context))
        .list()
        .ok()?
        .into_iter()
        .find(|p| p.get("id").and_then(Value::as_str) == Some(project_id))
}

struct Env<'a> {
    live: &'a Live,
}

impl ScheduleEnv for Env<'_> {
    fn project_dir(&self, project_id: &str) -> Option<PathBuf> {
        project_dir(self.live, project_id)
    }

    fn busy(&self, task_id: &str) -> bool {
        if self
            .live
            .driver(task_id)
            .is_some_and(|d| d.snapshot().turn_active)
        {
            return true;
        }
        self.live.record(task_id).is_some_and(|(_, record)| {
            matches!(
                record.get("status").and_then(Value::as_str),
                Some("pending" | "running" | "input_required")
            )
        })
    }

    fn start(&self, rule: &ScheduleRule, prompt: &str) -> Result<String, String> {
        // E6: a reached cap or 暂停所有 Agent: this run does not start (one line).
        if let Some(why) = super::budget::hold(self.live, None) {
            return Err(format!("没有开始：{why}"));
        }
        // 2026-11-22: the configuration is checked again at each run — it
        // may have gone or been blocked since the rule was saved.
        let picked = match rule.configuration_id.as_deref() {
            Some(id) => Some(picked_configuration(self.live, rule, id)?),
            None => None,
        };
        open_session(
            self.live,
            &rule.project_id,
            &Runner {
                agent: picked.as_ref().map_or(&rule.agent, |p| &p.agent),
                model: rule.model.as_deref(),
                agent_config_id: picked.as_ref().and_then(|p| p.agent_config_id.as_deref()),
            },
            prompt,
            &format!("{}（定时）", rule.name),
            &format!("schedule-{}", rule.id),
        )
    }

    fn quota(&self, rule: &ScheduleRule, now_ms: u64) -> QuotaState {
        codex_quota(self.live, &rule.agent, now_ms)
    }

    fn latest_reply(&self, task_id: &str) -> Option<String> {
        super::latest_reply(self.live, task_id)
    }
}

/// The subscription `agent` would spend. Only the built-in Codex spends this
/// Mac's subscription; a custom profile has its own home or gateway, and the
/// rest are always fine. Also read by 无人值守自动重试.
pub(super) fn codex_quota(live: &Live, agent: &str, now_ms: u64) -> QuotaState {
    let families = live.families.as_ref();
    if !(families.is_builtin(agent) && families.session_family(agent) == ProtocolFamily::Codex) {
        return QuotaState::Fine;
    }
    let path = live.context.paths.data_dir().join(quota_pace::FILE);
    let now = now_ms as i64;
    let fresh = wire::QUOTA_FRESH_MS as i64;
    let mut readings = quota_pace::load(&path);
    // A test host has only the file: it never starts `codex`.
    if live.product && !readings.iter().any(|r| now - r.at_ms <= fresh) {
        quota_pace::sample(Arc::clone(&live.codex_rpc), &path, now);
        readings = quota_pace::load(&path);
    }
    quota_pace::state(&readings, now, fresh)
}

/// A local project's folder (`None`: gone, or remote).
pub(super) fn project_dir(live: &Live, project_id: &str) -> Option<PathBuf> {
    let path = PathBuf::from(project(live, project_id)?.get("path")?.as_str()?);
    path.is_dir().then_some(path)
}

/// Who runs a session [`open_session`] opens.
pub(super) struct Runner<'a> {
    pub agent: &'a str,
    pub model: Option<&'a str>,
    /// The task's `agentConfigId` (a catalog id without its prefix); `None`
    /// leaves the Agent's default source (2026-11-17, 交办选配置).
    pub agent_config_id: Option<&'a str>,
}

/// `task.run`'s params for [`open_session`].
pub(super) fn run_params(dir: &Path, task_id: &str, runner: &Runner, prompt: &str) -> Value {
    let mut params = json!({
        "projectPath": dir.to_string_lossy(),
        "agent": runner.agent,
        "prompt": prompt,
        "taskId": task_id,
    });
    if let Some(model) = runner.model {
        params["selectedModel"] = json!(model);
    }
    if let Some(config) = runner.agent_config_id {
        params["agentConfigId"] = json!(config);
    }
    params
}

/// Open a new session in a local project the way the 新会话 button does
/// (`task.run`, the Agent's default permission mode), named `name` in the
/// sidebar; its task id. Used by 定时任务 and 待办 (2026-11-15).
pub(super) fn open_session(
    live: &Live,
    project_id: &str,
    runner: &Runner,
    prompt: &str,
    name: &str,
    request_id: &str,
) -> Result<String, String> {
    let dir = project_dir(live, project_id).ok_or("项目不在本机或已移除")?;
    let task_id = format!("task-{}", uuid::Uuid::new_v4().simple());
    let params = run_params(&dir, &task_id, runner, prompt);
    let request: Request = serde_json::from_value(json!({
        "v": 1,
        "id": request_id,
        "method": "task.run",
        "params": params,
    }))
    .map_err(|e| e.to_string())?;
    let frame = crate::services::task_run(
        request,
        Arc::clone(&live.service),
        Arc::clone(&live.context),
    );
    let reply = serde_json::to_value(&frame).map_err(|e| e.to_string())?;
    if let Some(error) = reply.get("error") {
        return Err(error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("开会话失败")
            .to_string());
    }
    // The session says where it came from in the sidebar.
    let _ = live.service.rename(project_id, &task_id, name);
    Ok(task_id)
}

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    set.register(wire::LIST_METHOD, blocking(live, schedule_list));
    set.register(wire::SAVE_METHOD, blocking(live, schedule_save));
    set.register(wire::DELETE_METHOD, blocking(live, schedule_delete));
    set.register(wire::RUN_NOW_METHOD, blocking(live, schedule_run_now));
    set.register(wire::RUNS_METHOD, blocking(live, schedule_runs));
    start_ticker(live);
}

fn start_ticker(live: &Arc<Live>) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let weak = Arc::downgrade(live);
    handle.spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(wire::TICK_MS)).await;
            let Some(live) = weak.upgrade() else {
                break;
            };
            let _ = tokio::task::spawn_blocking(move || {
                schedules(&live).tick(&chrono::Local, &Env { live: &live }, now_ms());
            })
            .await;
        }
    });
}

/// `schedule.list {}` → `{rules}`, each with when it fires next.
fn schedule_list(live: &Live, request: &Request) -> Frame {
    ok(
        request,
        json!({ "rules": schedules(live).list(&chrono::Local, now_ms()) }),
    )
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SaveParams {
    rule: ScheduleRule,
}

/// 2026-11-22: `rule`'s configuration checked against its project's
/// catalog (it exists, nothing blocks it, it offers the rule's model).
fn picked_configuration(
    live: &Live,
    rule: &ScheduleRule,
    configuration: &str,
) -> Result<super::todo::Picked, String> {
    let (_, catalog) =
        aeroric_core::settings::session_catalog::for_project(&live.context, &rule.project_id)?;
    super::todo::pick_configuration(
        &catalog.configurations,
        "",
        rule.model.as_deref(),
        Some(configuration),
    )
}

/// `schedule.save {rule}` → `{rule}`: a new rule (empty id) or a change.
fn schedule_save(live: &Live, request: &Request) -> Frame {
    let mut p: SaveParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    if project(live, &p.rule.project_id).is_none() {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "没有这个项目",
        );
    }
    p.rule.configuration_id = p
        .rule
        .configuration_id
        .take()
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty());
    if let Some(id) = p.rule.configuration_id.clone() {
        match picked_configuration(live, &p.rule, &id) {
            Ok(picked) => p.rule.agent = picked.agent,
            Err(reason) => return failure(request.id.clone(), ErrorCode::InvalidArgument, &reason),
        }
    }
    match schedules(live).save_rule(p.rule, now_ms()) {
        Ok(rule) => ok(request, json!({ "rule": rule })),
        Err(error) => failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IdParams {
    id: String,
}

/// `schedule.delete {id}` → `{deleted}`; the rule's runs go with it.
fn schedule_delete(live: &Live, request: &Request) -> Frame {
    let p: IdParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match schedules(live).delete(&p.id) {
        Ok(true) => ok(request, json!({ "deleted": true })),
        Ok(false) => failure(request.id.clone(), ErrorCode::NotFound, "没有这个定时任务"),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

/// `schedule.runNow {id}` → `{run}`: 立即运行 (busy, the daily limit and the
/// pre-check still apply).
fn schedule_run_now(live: &Live, request: &Request) -> Frame {
    let p: IdParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    match schedules(live).run_now(&chrono::Local, &Env { live }, &p.id, now_ms()) {
        Ok(run) => ok(request, json!({ "run": run })),
        Err(error) => failure(request.id.clone(), ErrorCode::NotFound, &error),
    }
}

/// `schedule.runs {id}` → `{runs}`, newest first.
fn schedule_runs(live: &Live, request: &Request) -> Frame {
    let p: IdParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    ok(request, json!({ "runs": schedules(live).runs(&p.id) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_is_opened_with_its_model_and_configuration() {
        let dir = Path::new("/p");
        let plain = run_params(
            dir,
            "task-1",
            &Runner {
                agent: "codex",
                model: None,
                agent_config_id: None,
            },
            "做事",
        );
        assert_eq!(plain["agent"], "codex");
        assert!(plain.get("selectedModel").is_none() && plain.get("agentConfigId").is_none());
        let pinned = run_params(
            dir,
            "task-1",
            &Runner {
                agent: "codex",
                model: Some("deepseek-v4"),
                agent_config_id: Some("deepseek"),
            },
            "做事",
        );
        assert_eq!(pinned["selectedModel"], "deepseek-v4");
        assert_eq!(pinned["agentConfigId"], "deepseek");
        assert_eq!(pinned["taskId"], "task-1");
    }
}
