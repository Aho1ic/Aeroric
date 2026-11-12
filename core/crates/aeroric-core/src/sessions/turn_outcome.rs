//! R5-1 (2026-11-11): how a turn ended, told apart so a comparison of runs
//! never counts "the API refused" as "the model got it wrong".
//!
//! - **Not run**: the turn ended on the agent's API error (quota, login,
//!   a model the route does not serve, the service down, the network, a
//!   rejected request). Read from the transcript: Codex's `task_complete`
//!   with `error`, Claude's turn whose last assistant line is an API error.
//!   An error the agent retried past is only counted (`apiErrors`).
//! - **Product failure**: the turn ran, but the user undid it or the
//!   project's checks failed on it. Read from the turn ledger.
//! - Otherwise it completed, or the user stopped it (`interrupted`).

use aeroric_protocol::turn_ledger::{TurnRecord, TurnState};
use serde::Serialize;
use serde_json::Value;

use super::transcript_stats::TurnStat;

/// Kept of an error's text; enough to say what failed.
const MESSAGE_CHARS: usize = 300;

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NotRunReason {
    /// Out of quota, or rate limited.
    Quota,
    /// Not logged in, or the login or key is no longer valid.
    Login,
    /// The route does not serve the chosen model.
    Model,
    /// The service failed or is overloaded.
    Service,
    /// The agent could not reach the service.
    Network,
    /// The service rejected the request itself.
    Rejected,
    Other,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NotRun {
    pub reason: NotRunReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    pub message: String,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProductFailure {
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub undone: bool,
    /// The failed check's summary, e.g. `测试 3 项失败`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check: Option<String>,
}

/// Codex's `task_complete.error`: `{message, codex_error_info}`, the info a
/// word (`usage_limit_exceeded`) or `{http_connection_failed: {http_status_code}}`.
pub fn codex_not_run(error: &Value) -> NotRun {
    let info = &error["codex_error_info"];
    let status = info["http_connection_failed"]["http_status_code"]
        .as_u64()
        .and_then(|code| u16::try_from(code).ok());
    let hint = info.as_str().unwrap_or_default();
    let message = error["message"].as_str().unwrap_or_default();
    not_run(status, hint, message)
}

/// Claude's API error line: `error` names the kind, `apiErrorStatus` the
/// status, and the message's text says the rest.
pub fn claude_not_run(line: &Value) -> NotRun {
    let status = line["apiErrorStatus"]
        .as_u64()
        .and_then(|code| u16::try_from(code).ok());
    let hint = line["error"].as_str().unwrap_or_default();
    let text: String = line["message"]["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|block| block["text"].as_str())
        .collect();
    not_run(status, hint, &text)
}

fn not_run(status: Option<u16>, hint: &str, message: &str) -> NotRun {
    NotRun {
        reason: classify(status, hint, message),
        status,
        message: message.chars().take(MESSAGE_CHARS).collect(),
    }
}

/// The login words come first: a gateway reports an expired login upstream as
/// its own 503 (`auth_unavailable … Please log in again`). Quota words come
/// before the 401/403 status: a gateway out of credit answers 403
/// (`Insufficient account balance`).
fn classify(status: Option<u16>, hint: &str, message: &str) -> NotRunReason {
    let text = format!("{hint} {message}").to_lowercase();
    let says = |words: &[&str]| words.iter().any(|word| text.contains(word));
    let quota_words = [
        "usage_limit",
        "rate_limit",
        "quota",
        "usage limit",
        "insufficient account balance",
        "insufficient balance",
        "余额不足",
    ];
    if says(&[
        "authentication",
        "unauthorized",
        "auth_unavailable",
        "log in again",
        "/login",
        "没有可用的登录",
        "无效的令牌",
        "分组已停用",
    ]) {
        NotRunReason::Login
    } else if status == Some(429) || says(&quota_words) {
        NotRunReason::Quota
    } else if matches!(status, Some(401 | 403)) {
        NotRunReason::Login
    } else if status == Some(404) || says(&["model_not_found", "不支持所选模型"]) {
        NotRunReason::Model
    } else if says(&[
        "econnrefused",
        "econnreset",
        "eproto",
        "unable to connect",
        "connection refused",
        "connection dropped",
        "timed out",
    ]) {
        // Before the status: Claude files these under `server_error`.
        NotRunReason::Network
    } else if status.is_some_and(|code| code >= 500)
        || says(&["overloaded", "server_error", "at capacity", "high demand"])
    {
        NotRunReason::Service
    } else if status == Some(400) || says(&["invalid_request", "unsupported_value"]) {
        NotRunReason::Rejected
    } else {
        NotRunReason::Other
    }
}

/// Mark the turns the user undid or whose checks failed, matched to the
/// ledger by start time as the change cost is.
pub fn grade(turns: &mut [TurnStat], records: &[TurnRecord]) {
    for turn in turns {
        if turn.not_run.is_some() || turn.interrupted {
            continue;
        }
        let Some(record) = super::change_cost::nearest(turn.started_at, records) else {
            continue;
        };
        let undone = record.state == TurnState::Undone;
        let check = record
            .check
            .as_ref()
            .filter(|check| !check.passed)
            .map(|check| check.summary.clone());
        if undone || check.is_some() {
            turn.product_failure = Some(ProductFailure { undone, check });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::transcript_stats::{claude, codex};
    use aeroric_protocol::turn_ledger::TurnCheck;
    use serde_json::json;

    fn lines(raw: &[Value]) -> Vec<String> {
        raw.iter().map(Value::to_string).collect()
    }

    fn t(s: &str) -> String {
        format!("2026-11-11T01:00:{s}Z")
    }

    /// The failures seen in real rollouts and transcripts (2026-11-10/11).
    #[test]
    fn real_errors_are_told_apart() {
        let codex_error = |info: Value, message: &str| {
            codex_not_run(&json!({"message": message, "codex_error_info": info})).reason
        };
        let http = |code: u64| json!({"http_connection_failed": {"http_status_code": code}});
        assert_eq!(
            codex_error(
                json!("usage_limit_exceeded"),
                "You've hit your usage limit."
            ),
            NotRunReason::Quota
        );
        assert_eq!(
            codex_error(
                http(404),
                "unexpected status 404 Not Found: ANY: 当前 API 不支持所选模型 gpt-6.1-sol"
            ),
            NotRunReason::Model
        );
        assert_eq!(
            codex_error(
                http(401),
                "unexpected status 401 Unauthorized: 本机没有可用的登录"
            ),
            NotRunReason::Login
        );
        assert_eq!(
            codex_error(
                http(503),
                "unexpected status 503 Service Unavailable: auth_unavailable: no auth available \
                 (… \"Your session has ended. Please log in again.\")"
            ),
            NotRunReason::Login,
            "a gateway's 503 for an expired login upstream"
        );
        assert_eq!(
            codex_error(json!("server_overloaded"), "Selected model is at capacity."),
            NotRunReason::Service
        );
        // R5 目检 (2026-11-12): a gateway out of credit answers 403.
        assert_eq!(
            codex_error(
                http(403),
                "unexpected status 403 Forbidden: Chatflow: Insufficient account balance"
            ),
            NotRunReason::Quota
        );
        assert_eq!(
            codex_error(
                json!("other"),
                r#"{"error":{"message":"Codex: X-OpenAI-Internal-Codex-Responses-Lite requires `reasoning.context` to be `all_turns`.","type":"invalid_request_error"}}"#
            ),
            NotRunReason::Rejected
        );
        let claude_error = |kind: &str, status: Option<u64>, text: &str| {
            claude_not_run(
                &json!({"isApiErrorMessage": true, "error": kind, "apiErrorStatus": status,
                "message": {"content": [{"type": "text", "text": text}]}}),
            )
        };
        let rate = claude_error(
            "rate_limit",
            Some(429),
            "API Error: Request rejected (429) · Service Unavailable",
        );
        assert_eq!((rate.reason, rate.status), (NotRunReason::Quota, Some(429)));
        assert_eq!(
            claude_error(
                "server_error",
                None,
                "API Error: Unable to connect to API (EPROTO)"
            )
            .reason,
            NotRunReason::Network
        );
        assert_eq!(
            claude_error(
                "authentication_failed",
                Some(401),
                "Not logged in · Please run /login"
            )
            .reason,
            NotRunReason::Login
        );
        assert_eq!(
            claude_error(
                "server_error",
                Some(502),
                "API Error: 502 Chat Completions upstream"
            )
            .reason,
            NotRunReason::Service
        );
        let long = claude_error("unknown", Some(400), &"x".repeat(1000));
        assert_eq!(long.message.chars().count(), MESSAGE_CHARS);
    }

    #[test]
    fn a_codex_turn_that_ended_on_an_error_did_not_run() {
        let rollout = lines(&[
            json!({"timestamp": t("00.000"), "type": "event_msg", "payload": {"type": "task_started"}}),
            json!({"timestamp": t("02.000"), "type": "event_msg", "payload": {"type": "task_complete",
                "error": {"message": "unexpected status 404 Not Found: ANY: 当前 API 不支持所选模型 gpt-6.1-sol",
                          "codex_error_info": {"http_connection_failed": {"http_status_code": 404}}}}}),
            json!({"timestamp": t("10.000"), "type": "event_msg", "payload": {"type": "task_started"}}),
            json!({"timestamp": t("12.000"), "type": "event_msg", "payload": {"type": "task_complete"}}),
        ]);
        let stats = codex(&rollout).unwrap();
        let not_run = stats.turns[0]
            .not_run
            .as_ref()
            .expect("the first turn did not run");
        assert_eq!(
            (not_run.reason, not_run.status),
            (NotRunReason::Model, Some(404))
        );
        assert!(stats.turns[0].failed);
        assert!(stats.turns[1].not_run.is_none() && !stats.turns[1].failed);
    }

    #[test]
    fn a_claude_error_retried_past_is_only_counted() {
        let error = |at: &str| {
            json!({"timestamp": t(at), "type": "assistant", "isApiErrorMessage": true, "error": "rate_limit",
                "apiErrorStatus": 429, "message": {"role": "assistant", "content": [{"type": "text", "text": "API Error: 429"}]}})
        };
        let user = |at: &str| json!({"timestamp": t(at), "type": "user", "message": {"role": "user", "content": "hi"}});
        let raw = [
            user("00.000"),
            error("01.000"),
            error("02.000"),
            json!({"timestamp": t("05.000"), "type": "assistant", "message": {"id": "m1", "role": "assistant",
                "content": [{"type": "text", "text": "done"}]}}),
            user("10.000"),
            error("11.000"),
        ];
        let stats = claude(&lines(&raw)).unwrap();
        let (first, second) = (&stats.turns[0], &stats.turns[1]);
        assert!(!first.failed && first.not_run.is_none());
        assert_eq!(first.api_errors, 2);
        assert!(second.failed);
        assert_eq!(second.not_run.as_ref().unwrap().reason, NotRunReason::Quota);
    }

    #[test]
    fn undone_turns_and_failed_checks_are_product_failures() {
        let record =
            |turn: &str, started_ms: u64, state: TurnState, check: Option<TurnCheck>| TurnRecord {
                turn_id: turn.into(),
                started_ms,
                ended_ms: started_ms + 1,
                top: String::new(),
                before: String::new(),
                after: String::new(),
                head_before: None,
                head_after: None,
                files: vec![],
                additions: 0,
                deletions: 0,
                state,
                undone_ms: None,
                undo_group: None,
                state_before_undo: None,
                check,
            };
        let failed = TurnCheck {
            passed: false,
            summary: "测试 3 项失败".into(),
        };
        let records = [
            record("a", 1_000, TurnState::Undone, None),
            record("b", 100_000, TurnState::Pending, Some(failed)),
            record(
                "c",
                200_000,
                TurnState::Accepted,
                Some(TurnCheck {
                    passed: true,
                    summary: String::new(),
                }),
            ),
        ];
        let turn = |started_at: u64| TurnStat {
            started_at,
            ..TurnStat::default()
        };
        let mut turns = vec![turn(1_500), turn(100_500), turn(200_500), turn(400_000)];
        turns.push(TurnStat {
            interrupted: true,
            ..turn(1_200)
        });
        grade(&mut turns, &records);
        assert_eq!(
            turns[0].product_failure,
            Some(ProductFailure {
                undone: true,
                check: None
            })
        );
        assert_eq!(
            turns[1].product_failure,
            Some(ProductFailure {
                undone: false,
                check: Some("测试 3 项失败".into())
            })
        );
        assert_eq!(turns[2].product_failure, None, "accepted and passed");
        assert_eq!(
            turns[3].product_failure, None,
            "no record: changed no files"
        );
        assert_eq!(turns[4].product_failure, None, "the user stopped it");
    }
}
