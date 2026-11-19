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
        Ok((summary, requests)) => ok(request, json!({"summary": summary, "requests": requests})),
        Err(reason) => failure(request.id.clone(), ErrorCode::IoError, &reason),
    }
}
