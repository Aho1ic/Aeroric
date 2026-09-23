//! Task P2-04 acceptance, part 3: the parity replay.
//!
//! `contracts/native/v1/fixtures/task-transitions.json` holds 283 cases whose `expected`
//! answers were produced by the shipping client's own TypeScript
//! (`src/test/task-core-parity.test.ts`). This file runs every one of them through the Rust
//! port and requires the same answer. A case may opt out only by carrying `coreExpected`
//! plus a `divergenceReason`, which the client-side suite proves really diverges; anything
//! else that disagrees is a bug in one of the two.
//!
//! This is the half that makes "we ported the rules" checkable. The service tests in
//! `task_lifecycle.rs` prove the *ordering* guarantees; nothing there would notice the
//! status machine quietly answering a different transition table.

use std::path::{Path, PathBuf};

use aeroric_core::tasks::cleanup::{self, CleanupSchedule, LocalCalendar};
use aeroric_core::tasks::handoff;
use aeroric_core::tasks::reducer;
use aeroric_core::tasks::session_owner::{self, AgentFamilies};
use aeroric_protocol::task::{ProtocolFamily, TaskStatus};
use serde_json::{json, Value};

const DAY_MS: i64 = 86_400_000;

fn fixture() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../contracts/native/v1/fixtures/task-transitions.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} must exist: {e}", path.display()));
    serde_json::from_str(&text).expect("the fixture is valid JSON")
}

fn record(value: &Value) -> aeroric_core::storage::compat::Record {
    aeroric_core::storage::compat::Record(
        value
            .as_object()
            .unwrap_or_else(|| panic!("expected one task record, got {value}"))
            .clone(),
    )
}

fn records(value: &Value) -> Vec<aeroric_core::storage::compat::Record> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("expected an array of task records, got {value}"))
        .iter()
        .map(|item| {
            aeroric_core::storage::compat::Record(
                item.as_object()
                    .unwrap_or_else(|| panic!("every task must be an object: {item}"))
                    .clone(),
            )
        })
        .collect()
}

fn records_json(rows: &[aeroric_core::storage::compat::Record]) -> Value {
    Value::Array(
        rows.iter()
            .map(|row| Value::Object(row.0.clone()))
            .collect(),
    )
}

fn status(value: &str) -> TaskStatus {
    serde_json::from_value(Value::String(value.to_string()))
        .unwrap_or_else(|_| panic!("{value} is not a status the contract knows"))
}

/// The table a case carries. `agentOption`'s fallback for a name neither side knows is
/// restated here because the client's own resolution depends on it; Task P3-02 owns the
/// real table, and this is the only place the task module can see one.
struct Table {
    entries: Vec<(String, ProtocolFamily, bool)>,
}

impl AgentFamilies for Table {
    /// Both answers are the *same* function here on purpose: the 283 replayed cases were
    /// produced by the client's own family resolution, so the parity harness must not
    /// introduce a launch/session split of its own. The divergence is pinned in
    /// `session_owner`/`launch_plan` unit tests, where it can be asserted per field.
    fn launch_family(&self, agent: &str) -> ProtocolFamily {
        self.resolve(agent)
    }
    fn session_family(&self, agent: &str) -> ProtocolFamily {
        self.resolve(agent)
    }
    fn native_resume_family(&self, agent: &str) -> ProtocolFamily {
        // The client answers all three questions from one field, so replaying its verdicts
        // must too. ADR-029's split lives in the host table, asserted in `session_owner`.
        self.resolve(agent)
    }
    fn is_builtin(&self, agent: &str) -> bool {
        self.entries
            .iter()
            .find(|(value, _, _)| value == agent)
            .map(|(_, _, builtin)| *builtin)
            .unwrap_or(false)
    }
}

impl Table {
    fn resolve(&self, agent: &str) -> ProtocolFamily {
        if let Some((_, family, _)) = self.entries.iter().find(|(value, _, _)| value == agent) {
            return *family;
        }
        // `src/agents.ts:170-178`: an unknown id is codex unless it is literally "claude".
        if agent == "claude" {
            ProtocolFamily::Claude
        } else {
            ProtocolFamily::Codex
        }
    }
}

fn table(input: &Value) -> Table {
    let entries = input
        .get("options")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Table {
        entries: entries
            .iter()
            .map(|item| {
                (
                    item["value"].as_str().expect("an option value").to_string(),
                    serde_json::from_value::<ProtocolFamily>(item["family"].clone())
                        .expect("a known family"),
                    // The fixture carries `builtIn`, read from the client's own
                    // `isBuiltInAgent`, so neither side consults a private list.
                    item["builtIn"].as_bool() == Some(true),
                )
            })
            .collect(),
    }
}

/// A calendar at the offset the client reported for the same instant: the rule under test is
/// the weekly slot, not which side of the planet the clock sits on. A case whose window
/// crossed a DST change would disagree here, which is why the fixture's dates stay inside one
/// offset (2026-09-06 … 2026-09-14, no populated zone transitions there).
struct Offset {
    minutes: i64,
}

impl LocalCalendar for Offset {
    fn parts(&self, now_ms: i64) -> (u32, i64, i64, i64, i64) {
        let local = now_ms + self.minutes * 60_000;
        let day = local.div_euclid(DAY_MS);
        let millis = local.rem_euclid(DAY_MS);
        let (year, month, date) = civil_from_days(day);
        (
            ((day + 4).rem_euclid(7)) as u32,
            year,
            month,
            date,
            millis / 3_600_000,
        )
    }

    fn slot_ms(&self, year: i64, month: i64, day: i64, hour: i64) -> i64 {
        days_from_civil(year, month, day) * DAY_MS + hour * 3_600_000 - self.minutes * 60_000
    }
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn field<'a>(input: &'a Value, key: &str) -> &'a Value {
    input
        .get(key)
        .unwrap_or_else(|| panic!("the case is missing {key}: {input}"))
}

fn opt_string(value: &Value) -> Option<String> {
    value.as_str().map(|text| text.to_string())
}

fn run_case(op: &str, input: &Value) -> Value {
    match op {
        "applyTaskStatusTransition" => {
            let rows = records(field(input, "tasks"));
            let extra = input
                .get("extra")
                .and_then(|v| v.get("attentionRequestedAt"));
            let outcome = reducer::apply_status_transition(
                &rows,
                field(input, "taskId").as_str().expect("a task id"),
                status(field(input, "status").as_str().expect("a status")),
                extra.and_then(Value::as_u64),
                input.get("failureReason").and_then(Value::as_str),
                field(input, "now").as_u64().expect("now"),
            );
            json!({
                "tasks": records_json(&outcome.records),
                "changed": outcome.changed,
                "task": outcome.task.map(|row| Value::Object(row.0)),
            })
        }
        "shouldIgnoreTaskStatusTransition" => {
            json!(reducer::should_ignore_transition(
                Some(status(field(input, "current").as_str().expect("current"))),
                status(field(input, "next").as_str().expect("next")),
            ))
        }
        "archiveTasksInList" => {
            let ids: Vec<String> = field(input, "taskIds")
                .as_array()
                .expect("taskIds")
                .iter()
                .map(|item| item.as_str().expect("an id").to_string())
                .collect();
            let now = input.get("now").and_then(Value::as_u64).unwrap_or(0);
            let (rows, _) = reducer::archive(&records(field(input, "tasks")), &ids, now);
            records_json(&rows)
        }
        "unarchiveTasksInList" => {
            let ids: Vec<String> = field(input, "taskIds")
                .as_array()
                .expect("taskIds")
                .iter()
                .map(|item| item.as_str().expect("an id").to_string())
                .collect();
            let (rows, _) = reducer::unarchive(&records(field(input, "tasks")), &ids);
            records_json(&rows)
        }
        "toggleTaskStarInList" => records_json(&reducer::toggle_star(
            &records(field(input, "tasks")),
            field(input, "taskId").as_str().expect("a task id"),
        )),
        "renameTaskInList" => records_json(&reducer::rename(
            &records(field(input, "tasks")),
            field(input, "taskId").as_str().expect("a task id"),
            field(input, "name").as_str().expect("a name"),
        )),
        "rollbackTaskMutation" => Value::Object(
            reducer::rollback_mutation(
                &record(field(input, "current")),
                &record(field(input, "previous")),
                &record(field(input, "applied")),
            )
            .0,
        ),
        "applyResolvedTaskSession" => {
            let owner = field(input, "owner");
            let declared = owner.get("family").and_then(opt_string);
            let family = match declared.as_deref() {
                Some(text) => {
                    serde_json::from_value::<ProtocolFamily>(Value::String(text.to_string()))
                        .expect("a declared family")
                }
                None => {
                    // `familyFromCodexLike` is the pre-family spelling of the same choice.
                    if owner["codexLike"].as_bool() == Some(true) {
                        ProtocolFamily::Codex
                    } else {
                        ProtocolFamily::Claude
                    }
                }
            };
            let session = field(input, "session");
            let result = session_owner::apply_resolved_session(
                &record(field(input, "task")),
                &session_owner::SessionOwner {
                    agent: owner["agent"].as_str().expect("an agent").to_string(),
                    family,
                    codex_like: family == ProtocolFamily::Codex,
                },
                session.get("sessionId").and_then(Value::as_str),
                session.get("sessionPath").and_then(Value::as_str),
            );
            Value::Object(result.0)
        }
        "resolveTaskSessionOwner" => {
            let owner =
                session_owner::resolve_session_owner(&record(field(input, "task")), &table(input));
            json!({
                "agent": owner.agent,
                "family": owner.family,
                "codexLike": owner.codex_like,
            })
        }
        "hasTaskSession" => {
            json!(session_owner::has_task_session(&record(field(
                input, "task"
            ))))
        }
        "hasTaskSessionPath" => json!(session_owner::has_task_session_path(&record(field(
            input, "task"
        )))),
        "getTaskSessionFieldsByFamily" => {
            let family = serde_json::from_value::<ProtocolFamily>(field(input, "family").clone())
                .expect("a family");
            let fields = session_owner::session_fields(&record(field(input, "task")), family);
            let mut object = serde_json::Map::new();
            for (key, value) in [
                ("sessionId", fields.session_id),
                ("sessionPath", fields.session_path),
                ("legacySessionId", fields.legacy_session_id),
                ("legacySessionPath", fields.legacy_session_path),
            ] {
                if let Some(value) = value {
                    object.insert(key.to_string(), Value::String(value));
                }
            }
            Value::Object(object)
        }
        "canNativeResumeWithAgent" => json!(session_owner::can_native_resume(
            &record(field(input, "task")),
            field(input, "target").as_str().expect("a target"),
            &table(input),
        )),
        "canAdoptSessionForAgent" => json!(session_owner::can_adopt(
            &record(field(input, "task")),
            field(input, "target").as_str().expect("a target"),
            &table(input),
        )),
        "resolveConfigSwitchSessionStrategy" => {
            let strategy = session_owner::resolve_config_switch_strategy(
                &record(field(input, "task")),
                field(input, "target").as_str().expect("a target"),
                field(input, "supported").as_bool().expect("supported"),
                &table(input),
            );
            json!(match strategy {
                session_owner::ConfigSwitchStrategy::Resume => "resume",
                session_owner::ConfigSwitchStrategy::Adopt => "adopt",
                session_owner::ConfigSwitchStrategy::Handoff => "handoff",
            })
        }
        "hasTaskContinuationContext" => json!(session_owner::has_continuation_context(&record(
            field(input, "task")
        ))),
        "taskCompletionCommand" => {
            let row = record(field(input, "task")).clone();
            let owner = session_owner::resolve_session_owner(&row, &table(input));
            let command = if owner.family == ProtocolFamily::Dsh {
                Some("complete_dsh_task")
            } else if matches!(
                reducer::status_of(&row),
                Some(TaskStatus::Pending | TaskStatus::Running | TaskStatus::InputRequired)
            ) {
                Some("complete_task")
            } else {
                None
            };
            command.map(Value::from).unwrap_or(Value::Null)
        }
        "expiredTaskIds" => {
            let settings =
                CleanupSchedule::from_record(&records(&json!([field(input, "settings")]))[0]);
            Value::Array(
                cleanup::expired_task_ids(
                    &records(field(input, "tasks")),
                    &settings,
                    field(input, "nowMs").as_i64().expect("nowMs"),
                )
                .into_iter()
                .map(Value::from)
                .collect(),
            )
        }
        "shouldRunCleanup" => {
            let settings =
                CleanupSchedule::from_record(&records(&json!([field(input, "settings")]))[0]);
            let calendar = Offset {
                minutes: field(input, "offsetMinutes")
                    .as_i64()
                    .expect("offsetMinutes"),
            };
            json!(cleanup::should_run_cleanup(
                &calendar,
                &settings,
                field(input, "nowMs").as_i64().expect("nowMs"),
            )
            .is_due())
        }
        "stripTerminalControlSequences" => Value::from(handoff::strip_terminal_control_sequences(
            field(input, "value").as_str().expect("a string"),
        )),
        "sanitizeTerminalHistoryForHandoff" => {
            Value::from(handoff::sanitize_terminal_history_for_handoff(
                field(input, "value").as_str().expect("a string"),
            ))
        }
        other => panic!("the Rust replay knows nothing about {other}"),
    }
}

#[test]
fn every_client_answer_is_reproduced_by_the_rust_port() {
    let cases = fixture()["cases"].as_array().expect("cases").clone();
    assert!(cases.len() > 150, "the fixture has {} cases", cases.len());
    let mut mismatches = Vec::new();
    let mut checked = 0usize;
    for case in &cases {
        let id = case["id"].as_str().expect("an id").to_string();
        let op = case["op"].as_str().expect("an op").to_string();
        let input = &case["input"];
        let want = case
            .get("coreExpected")
            .cloned()
            .unwrap_or_else(|| case["expected"].clone());
        let found = run_case(&op, input);
        if found != want {
            mismatches.push(format!(
                "{id}\n    client/rust-want: {}\n    rust produced: {}",
                serde_json::to_string(&want).expect("serialisable"),
                serde_json::to_string(&found).expect("serialisable"),
            ));
        }
        checked += 1;
    }
    assert_eq!(checked, cases.len(), "every case must be replayed");
    assert!(
        mismatches.is_empty(),
        "{} of {checked} parity cases disagree:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

#[test]
fn the_fixture_is_the_table_the_client_published_not_a_local_copy() {
    // The committed contract and the generator's knowledge table must be one file. A drift
    // here means someone edited `contracts/` by hand, which is the one thing the whole chain
    // exists to prevent.
    let knowledge = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../aeroric-protocol/knowledge/task-parity-cases.json");
    let committed = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../contracts/native/v1/fixtures/task-transitions.json");
    // The contract writer emits its own canonical rendering, so the comparison is on
    // content: same cases, same inputs, same expectations, same order.
    let left: Value =
        serde_json::from_str(&std::fs::read_to_string(&knowledge).expect("the knowledge table"))
            .expect("valid JSON");
    let right: Value =
        serde_json::from_str(&std::fs::read_to_string(&committed).expect("the contract fixture"))
            .expect("valid JSON");
    assert_eq!(
        left["cases"], right["cases"],
        "the committed contract and the generator's table are different documents —          regenerate with `node scripts/native/contracts.mjs --write`"
    );
}

#[test]
fn a_declared_divergence_is_never_silently_equal() {
    let cases = fixture()["cases"].as_array().expect("cases").clone();
    for case in cases
        .iter()
        .filter(|item| item.get("coreExpected").is_some())
    {
        let client = &case["expected"];
        let core = &case["coreExpected"];
        assert_ne!(
            client, core,
            "{} declares a divergence the client-side suite can see through",
            case["id"]
        );
        assert!(
            case["divergenceReason"]
                .as_str()
                .map(|reason| reason.len() > 10)
                .unwrap_or(false),
            "{} needs a reason a reviewer can read",
            case["id"]
        );
    }
}

#[test]
fn the_table_covers_every_state_and_every_family() {
    // Coverage of the table itself, so a future edit cannot quietly narrow it to the cases
    // that already pass.
    let cases = fixture()["cases"].as_array().expect("cases").clone();
    let transitions = cases
        .iter()
        .filter(|case| case["op"] == "applyTaskStatusTransition")
        .count();
    assert!(
        transitions >= 81,
        "9 × 9 pairs are the whole machine; a missing pair is an untested transition          (found {transitions})"
    );
    for family in ["claude", "codex", "dsh", "omp"] {
        assert!(
            cases
                .iter()
                .any(|case| case["op"] == "getTaskSessionFieldsByFamily"
                    && case["input"]["family"] == family),
            "{family} is never exercised"
        );
    }
    assert!(
        cases.iter().any(|case| case["op"] == "shouldRunCleanup"),
        "the weekly scheduler is a data-loss path and must be in the table"
    );
    assert!(
        cases
            .iter()
            .any(|case| case["op"] == "sanitizeTerminalHistoryForHandoff"),
        "the escape-sequence stripper is a security boundary, not a nicety"
    );
}
