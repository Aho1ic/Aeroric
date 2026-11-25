//! 2026-11-24 设置 › 存储: `storage.usage` → what Aeroric's data takes on
//! disk, by kind (`aeroric_core::maintenance::storage_usage`). Read only.

use std::path::PathBuf;
use std::sync::Arc;

use aeroric_core::maintenance::storage_usage;
use aeroric_core::projects::mutations;
use aeroric_protocol::envelope::{Frame, Request};
use serde_json::json;

use super::{blocking, ok, Live};
use crate::dispatcher::ServiceSet;

pub const USAGE_METHOD: &str = "storage.usage";

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    set.register(USAGE_METHOD, blocking(live, usage));
}

/// `storage.usage {}`: the data folder by kind, with each registered
/// project's `.aeroric/worktrees` under 工作副本.
fn usage(live: &Live, request: &Request) -> Frame {
    let roots: Vec<PathBuf> = aeroric_core::storage::load_projects(&live.context)
        .unwrap_or_default()
        .iter()
        .filter_map(|record| mutations::text(record, "path"))
        .map(PathBuf::from)
        .collect();
    let measured = storage_usage::measure(live.context.paths.data_dir(), &roots);
    ok(request, json!(measured))
}
