//! R5-2 (2026-11-11): `rerun.start` runs one turn again with up to three
//! other configurations, each a submission of its own in an isolated copy
//! made from the turn's `before` snapshot; `rerun.list` reads the original
//! turn and every run side by side. Judging is the user's: nothing is scored.

use std::sync::Arc;

use aeroric_core::sessions::rerun::{self, RerunGroup, RerunRun, MAX_RUNS};
use aeroric_core::sessions::transcript_stats::TurnStat;
use aeroric_core::sessions::{change_cost, turn_outcome};
use aeroric_core::storage::Record;
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use aeroric_protocol::submission::{SubmissionRequest, SubmissionState};
use aeroric_protocol::turn_ledger::TurnRecord;
use aeroric_protocol::ProtocolFamily;
use serde::Deserialize;
use serde_json::{json, Value};

use super::delegation::{field, title_of};
use super::{blocking, failure, ok, params, Live};
use crate::dispatcher::ServiceSet;

pub const START_METHOD: &str = "rerun.start";
pub const LIST_METHOD: &str = "rerun.list";

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    set.register(START_METHOD, blocking(live, start));
    set.register(LIST_METHOD, blocking(live, list));
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Target {
    configuration_id: String,
    model: Option<String>,
    reasoning_effort: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StartParams {
    task_id: String,
    turn_id: String,
    message: String,
    /// Send what the session had settled before the turn along (default).
    #[serde(default = "yes")]
    brief: bool,
    targets: Vec<Target>,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListParams {
    task_id: String,
}

fn reruns_dir(live: &Live) -> std::path::PathBuf {
    live.context.paths.data_dir().join("reruns")
}

fn ledger(live: &Live, task_id: &str) -> Vec<TurnRecord> {
    aeroric_core::sessions::live::turn_ledger::read_at(
        &live.context.paths.data_dir().join("turns"),
        task_id,
    )
}

/// The task's turns from its transcript, graded against its ledger.
fn turns(live: &Live, record: &Record) -> Vec<TurnStat> {
    use aeroric_core::sessions::transcript_stats;
    let (_, path) = aeroric_core::tasks::session_owner::current_session(record);
    let Some(raw) = path.and_then(|p| std::fs::read_to_string(p).ok()) else {
        return Vec::new();
    };
    let lines: Vec<String> = raw.lines().map(str::to_string).collect();
    let agent = field(record, "agent").unwrap_or_default();
    let stats = match live.families.session_family(&agent) {
        ProtocolFamily::Codex => transcript_stats::codex(&lines),
        ProtocolFamily::Claude => transcript_stats::claude(&lines),
        ProtocolFamily::Omp => transcript_stats::omp(&lines),
        ProtocolFamily::Dsh => transcript_stats::dsh(&lines),
    };
    let mut turns = stats.map(|s| s.turns).unwrap_or_default();
    let id = field(record, "id").unwrap_or_default();
    turn_outcome::grade(&mut turns, &ledger(live, &id));
    turns
}

fn start(live: &Live, request: &Request) -> Frame {
    let p: StartParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let fail = |code, text: &str| failure(request.id.clone(), code, text);
    if p.targets.is_empty() || p.targets.len() > MAX_RUNS {
        return fail(
            ErrorCode::InvalidArgument,
            &format!("一次选 1 到 {MAX_RUNS} 个配置"),
        );
    }
    if p.message.trim().is_empty() {
        return fail(ErrorCode::InvalidArgument, "消息不能为空");
    }
    let Some((project, record)) = live.record(&p.task_id) else {
        return fail(ErrorCode::NotFound, "原会话不存在");
    };
    let Some(project_id) = field(&project, "id") else {
        return fail(ErrorCode::NotFound, "会话的项目没有 id");
    };
    let Some(turn) = ledger(live, &p.task_id)
        .into_iter()
        .find(|r| r.turn_id == p.turn_id)
    else {
        return fail(
            ErrorCode::NotFound,
            "这一轮没有改动记录，不能从它开始前的代码重跑",
        );
    };
    let catalog =
        match aeroric_core::settings::session_catalog::for_project(&live.context, &project_id) {
            Ok((_, catalog)) => catalog,
            Err(reason) => return fail(ErrorCode::InvalidArgument, &reason),
        };
    let brief = if p.brief {
        // The turn's place among the session's turns, matched like the bill.
        let index = turns(live, &record).iter().position(|t| {
            change_cost::nearest(t.started_at, std::slice::from_ref(&turn)).is_some()
        });
        let checkpoint = std::fs::read(
            live.context
                .paths
                .data_dir()
                .join("checkpoints")
                .join(format!("{}.json", p.task_id)),
        )
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        index
            .zip(checkpoint)
            .and_then(|(index, checkpoint)| rerun::brief(&checkpoint, index as u64))
    } else {
        None
    };
    let prompt = rerun::prompt(&p.message, brief.as_deref());
    let original_permission = field(&record, "permissionMode");
    let title = title_of(&record);
    let mut runs = Vec::new();
    let mut refused = Vec::new();
    for target in &p.targets {
        let Some(configuration) = catalog
            .configurations
            .iter()
            .find(|c| c.id == target.configuration_id)
        else {
            refused.push(format!("{}：配置不存在", target.configuration_id));
            continue;
        };
        let permission = rerun::permission(
            &configuration.permission_modes,
            original_permission.as_deref(),
        );
        let label = match &target.model {
            Some(model) => format!("{} · {model}", configuration.display_name),
            None => configuration.display_name.clone(),
        };
        let mut submission = SubmissionRequest {
            schema_version: aeroric_protocol::submission::SCHEMA_VERSION,
            submission_id: format!("rerun-{}", uuid::Uuid::new_v4().simple()),
            project_id: project_id.clone(),
            configuration_id: configuration.id.clone(),
            configuration_version: catalog.revision.clone(),
            fingerprint: String::new(),
            agent: configuration.agent.clone(),
            prompt: prompt.clone(),
            permission_mode: permission.clone(),
            permission_agent: configuration.agent.clone(),
            selected_model: target.model.clone(),
            reasoning_effort: target.reasoning_effort.clone(),
            attachments: vec![],
            worktree: None,
            recipe_id: None,
            team: false,
            review: false,
            evolution: false,
            clarify: false,
            isolated: true,
            copy_from: Some(turn.before.clone()),
        };
        submission.fingerprint = aeroric_core::tasks::submissions::fingerprint(&submission);
        let receipt = match live.service.submit(&submission) {
            Ok(receipt) => receipt,
            Err(error) => {
                refused.push(format!("{label}：{error}"));
                continue;
            }
        };
        let Some(task_id) = receipt
            .task_id
            .filter(|_| receipt.state != SubmissionState::RejectedBeforeSideEffect)
        else {
            refused.push(format!("{label}：{}", receipt.reasons.join("；")));
            continue;
        };
        let _ = live
            .service
            .rename(&project_id, &task_id, &format!("{title}（重跑 · {label}）"));
        runs.push(RerunRun {
            task_id,
            configuration_id: configuration.id.clone(),
            agent: configuration.agent.clone(),
            model: target.model.clone(),
            label,
        });
    }
    if runs.is_empty() {
        return fail(ErrorCode::InvalidArgument, &refused.join("\n"));
    }
    let group = RerunGroup {
        id: format!("rerun-group-{}", uuid::Uuid::new_v4().simple()),
        origin_task_id: p.task_id.clone(),
        turn_id: p.turn_id.clone(),
        turn_started_ms: turn.started_ms,
        base: turn.before.clone(),
        message: p.message.clone(),
        with_brief: brief.is_some(),
        created_ms: super::now_ms(),
        runs,
    };
    if let Err(error) = rerun::append(&reruns_dir(live), group.clone()) {
        return fail(ErrorCode::IoError, &format!("记录重跑失败：{error}"));
    }
    ok(request, json!({ "group": group, "refused": refused }))
}

fn list(live: &Live, request: &Request) -> Frame {
    let p: ListParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let Some((_, record)) = live.record(&p.task_id) else {
        return failure(request.id.clone(), ErrorCode::NotFound, "原会话不存在");
    };
    let origin_turns = turns(live, &record);
    let origin_ledger = ledger(live, &p.task_id);
    let groups: Vec<Value> = rerun::load(&reruns_dir(live), &p.task_id)
        .into_iter()
        .map(|group| {
            let original_record: Vec<&TurnRecord> = origin_ledger
                .iter()
                .filter(|r| r.turn_id == group.turn_id)
                .collect();
            let original_stat = origin_turns.iter().find(|t| {
                change_cost::nearest(t.started_at, &origin_ledger)
                    .is_some_and(|r| r.turn_id == group.turn_id)
            });
            let original = rerun::column(
                original_stat,
                &original_record,
                false,
                Some(group.created_ms),
            );
            let copies = live.context.paths.data_dir().join("copies");
            let runs: Vec<Value> = group
                .runs
                .iter()
                .map(|run| {
                    let running = live
                        .driver(&run.task_id)
                        .is_some_and(|d| d.snapshot().turn_active);
                    let Some((_, run_record)) = live.record(&run.task_id) else {
                        return json!({"run": run, "gone": true});
                    };
                    let run_turns = turns(live, &run_record);
                    let run_ledger = ledger(live, &run.task_id);
                    // The run's first turn is the rerun; later ones are the
                    // user carrying on in that session.
                    let first = run_turns.first();
                    let changes: Vec<&TurnRecord> = run_ledger
                        .iter()
                        .filter(|r| {
                            first.is_some_and(|t| {
                                change_cost::nearest(t.started_at, std::slice::from_ref(r))
                                    .is_some()
                            })
                        })
                        .collect();
                    // R5 目检: after 全部丢弃 the buttons stayed live with no copy left.
                    let copy = aeroric_core::sessions::live::isolated_copy::load(&copies, &run.task_id)
                        .is_some();
                    json!({"run": run, "copy": copy, "result": rerun::column(first, &changes, running, None)})
                })
                .collect();
            json!({"group": group, "original": original, "runs": runs})
        })
        .collect();
    ok(request, json!({ "groups": groups }))
}
