//! C4 (2026-11-19): `gateway.requests {limit?}` — the gateway's totals and
//! its latest requests, each with every try: where it went, its status, its
//! usage and whether that usage was reported at all.

use std::sync::Arc;

use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use serde_json::json;

use super::{blocking, failure, ok, params, Live};
use crate::dispatcher::ServiceSet;

pub const REQUESTS_METHOD: &str = "gateway.requests";
const DEFAULT_LIMIT: usize = 50;

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    set.register(REQUESTS_METHOD, blocking(live, requests));
    install_purposes(live);
}

/// E7 (2026-11-22): the gateway asks what a task's request is for; the
/// answer comes from the task record — a reviewer, a delegate, a discussion
/// member, a rerun branch, a session a scheduled rule opened, or else the
/// answer itself. A kind is kept once known (a task does not change kind);
/// "the answer" only for a minute, because a scheduled rule saves its run
/// just after the session it opened may have sent its first request.
fn install_purposes(live: &Arc<Live>) {
    use aeroric_core::local_router::{purpose, set_purpose_resolver};
    let weak = Arc::downgrade(live);
    type Known = std::collections::HashMap<String, (String, std::time::Instant)>;
    let known: Arc<parking_lot::Mutex<Known>> = Arc::default();
    set_purpose_resolver(Arc::new(move |task_id: &str| {
        if let Some((found, at)) = known.lock().get(task_id) {
            if found != purpose::ANSWER || at.elapsed() < std::time::Duration::from_secs(60) {
                return found.clone();
            }
        }
        let Some(live) = weak.upgrade() else {
            return purpose::ANSWER.to_string();
        };
        let Some((_, record)) = live.record(task_id) else {
            return purpose::ANSWER.to_string();
        };
        let found = kind_of(&record.0, || {
            aeroric_core::schedule::Schedules::open(live.context.paths.data_dir())
                .knows_task(task_id)
        })
        .to_string();
        known.lock().insert(
            task_id.to_string(),
            (found.clone(), std::time::Instant::now()),
        );
        found
    }));
}

#[derive(serde::Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RequestsParams {
    #[serde(default)]
    limit: Option<usize>,
}

fn requests(live: &Live, request: &Request) -> Frame {
    let p: RequestsParams = match params(request) {
        Ok(p) => p,
        Err(frame) => return *frame,
    };
    let limit = p.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, 500);
    match crate::gateway::requests(live.context.paths.data_dir(), limit) {
        Ok((summary, requests, mut purposes)) => {
            // E7 step 2: Aeroric's own direct calls beside the gateway's.
            purposes.extend(direct_purposes(live, crate::gateway::purpose_since_ms()));
            purposes.sort_by(|a, b| b.requests.cmp(&a.requests).then(a.purpose.cmp(&b.purpose)));
            ok(
                request,
                json!({"summary": summary, "requests": requests, "purposes": purposes}),
            )
        }
        Err(reason) => failure(request.id.clone(), ErrorCode::IoError, &reason),
    }
}

/// E7 step 2 (2026-11-22): Aeroric's own calls to the analysis model since
/// `since_ms`, read from its logs — memory consolidation from 整理记录, the
/// Wiki's module notes from their generation log. A run that failed after
/// spending tokens is a failed try, with those tokens.
fn direct_purposes(live: &Live, since_ms: u64) -> Vec<aeroric_core::local_router::PurposeUsage> {
    use aeroric_core::local_router::purpose;
    use aeroric_protocol::memory::MemoryRunState;
    let mut out = Vec::new();
    let runs = live.executor.memory().runs(2000).unwrap_or_default();
    let memory = tally(
        purpose::MEMORY,
        runs.iter()
            .filter(|r| r.at_ms >= since_ms)
            .filter(|r| r.input_tokens + r.output_tokens > 0 || r.state == MemoryRunState::Failed)
            .map(|r| {
                (
                    r.state == MemoryRunState::Failed,
                    r.input_tokens,
                    r.output_tokens,
                )
            }),
    );
    out.extend(memory);
    let wikis = live.executor.wikis();
    let projects =
        aeroric_core::storage::projects::load_projects(&live.context).unwrap_or_default();
    let explained: Vec<(bool, u64, u64)> = projects
        .iter()
        .filter_map(|p| p.get("id").and_then(serde_json::Value::as_str))
        .filter(|id| wikis.has_pages(id))
        .filter_map(|id| wikis.explain_runs(id, since_ms).ok())
        .flatten()
        .map(|r| (r.error.is_some(), r.input_tokens, r.output_tokens))
        .collect();
    out.extend(tally(purpose::WIKI, explained.into_iter()));
    out
}

/// One direct purpose's row from `(failed, input, output)` per call; none
/// when there were no calls.
fn tally(
    name: &str,
    calls: impl Iterator<Item = (bool, u64, u64)>,
) -> Option<aeroric_core::local_router::PurposeUsage> {
    let mut row = aeroric_core::local_router::PurposeUsage {
        purpose: name.to_string(),
        direct: true,
        ..Default::default()
    };
    for (failed, input, output) in calls {
        row.requests += 1;
        if failed {
            row.failed_requests += 1;
            if input + output > 0 {
                row.failed_attempts += 1;
                row.failed_attempt_input_tokens += input;
                row.failed_attempt_output_tokens += output;
            }
        } else {
            row.input_tokens += input;
            row.output_tokens += output;
        }
    }
    (row.requests > 0).then_some(row)
}

/// E7: a task's purpose from its record; `scheduled` is asked last.
fn kind_of(
    record: &serde_json::Map<String, serde_json::Value>,
    scheduled: impl FnOnce() -> bool,
) -> &'static str {
    use aeroric_core::local_router::purpose;
    let has = |key: &str| record.get(key).is_some_and(|v| !v.is_null());
    if has("reviewOf") {
        purpose::REVIEW
    } else if has("delegateOf") {
        purpose::DELEGATION
    } else if has("discussionOf")
        || record
            .get("discussion")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
    {
        purpose::DISCUSSION
    } else if has("branchOf") {
        purpose::RERUN
    } else if scheduled() {
        purpose::SCHEDULE
    } else {
        purpose::ANSWER
    }
}

#[cfg(test)]
mod tests {
    use super::kind_of;
    use serde_json::json;

    /// E7: what a task's requests were for, from the fields that make it a
    /// reviewer, delegate, discussion member or rerun; a scheduled rule's
    /// session is asked about only when nothing else says.
    #[test]
    fn a_task_record_says_what_its_requests_were_for() {
        let of =
            |v: serde_json::Value, scheduled: bool| kind_of(v.as_object().unwrap(), || scheduled);
        assert_eq!(of(json!({"reviewOf": "t1"}), true), "review");
        assert_eq!(of(json!({"delegateOf": "t1"}), false), "delegation");
        assert_eq!(of(json!({"discussion": true}), false), "discussion");
        assert_eq!(of(json!({"discussionOf": "d"}), false), "discussion");
        assert_eq!(of(json!({"branchOf": "t1"}), false), "rerun");
        assert_eq!(of(json!({"reviewOf": null}), true), "schedule");
        assert_eq!(of(json!({}), false), "answer");
    }

    /// E7 step 2: a direct purpose counts its calls; a failed one that
    /// spent tokens is a failed try with them, not in the answered sums.
    #[test]
    fn direct_calls_are_tallied_with_failed_ones_apart() {
        let row = super::tally(
            "memory",
            [(false, 900, 80), (true, 300, 0), (true, 0, 0)].into_iter(),
        )
        .unwrap();
        assert!(row.direct);
        assert_eq!((row.requests, row.failed_requests), (3, 2));
        assert_eq!((row.input_tokens, row.output_tokens), (900, 80));
        assert_eq!(
            (row.failed_attempts, row.failed_attempt_input_tokens),
            (1, 300)
        );
        assert_eq!(super::tally("wiki", std::iter::empty()), None);
    }
}
