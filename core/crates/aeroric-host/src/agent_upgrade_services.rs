//! Agent upgrades and new-version reminders (2026-10-27): `agents.versions`,
//! `agents.checkUpdates`, `agents.upgrade`, `agents.setReminder`.
//!
//! Host-local UDS only, never the remote rpc. Upgrades run only when the user
//! asks (`agents.upgrade`); reading versions only asks the public npm registry.

use std::sync::Arc;

use aeroric_core::agents::upgrade::{Harness, ReminderMode, UpgradeCenter, UpgradeDeps};
use aeroric_core::context::CoreContext;
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use serde_json::Value;

use crate::dispatcher::{failure, Handler, ServiceSet};
use crate::services::ok;

pub const CAPABILITY: &str = "agents.upgrade";

pub fn register_agent_upgrade_services(set: &mut ServiceSet, context: Arc<CoreContext>) {
    register_with(
        set,
        UpgradeCenter::new(context.paths.data_dir(), UpgradeDeps::system()),
    );
}

/// The same four methods over a given center (tests pass fakes).
pub fn register_with(set: &mut ServiceSet, center: Arc<UpgradeCenter>) {
    set.register("agents.versions", handler(&center, Action::Versions));
    set.register("agents.checkUpdates", handler(&center, Action::Check));
    set.register("agents.upgrade", handler(&center, Action::Upgrade));
    set.register("agents.setReminder", handler(&center, Action::Remind));
}

#[derive(Clone, Copy)]
enum Action {
    Versions,
    Check,
    Upgrade,
    Remind,
}

fn harness_param(request: &Request) -> Result<Harness, Box<Frame>> {
    request
        .params
        .get("agent")
        .and_then(Value::as_str)
        .and_then(Harness::from_id)
        .ok_or_else(|| {
            Box::new(failure(
                request.id.clone(),
                ErrorCode::InvalidArgument,
                "agent must be one of claude, codex, dsh, omp",
            ))
        })
}

fn mode_param(request: &Request) -> Result<ReminderMode, Box<Frame>> {
    match request.params.get("mode").and_then(Value::as_str) {
        Some("on") => Ok(ReminderMode::On),
        Some("skipVersion") => Ok(ReminderMode::SkipVersion),
        Some("off") => Ok(ReminderMode::Off),
        _ => Err(Box::new(failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "mode must be on, skipVersion or off",
        ))),
    }
}

fn handler(center: &Arc<UpgradeCenter>, action: Action) -> Handler {
    let center = Arc::clone(center);
    Arc::new(move |request: Request, _call| {
        let center = Arc::clone(&center);
        Box::pin(async move {
            let id = request.id.clone();
            match action {
                Action::Versions => center.check(false).await,
                Action::Check => center.check(true).await,
                Action::Upgrade | Action::Remind => {}
            }
            let task = tokio::task::spawn_blocking(move || {
                match action {
                    Action::Upgrade => {
                        let harness = match harness_param(&request) {
                            Ok(harness) => harness,
                            Err(frame) => return *frame,
                        };
                        if let Err(message) = center.start(harness) {
                            return failure(request.id.clone(), ErrorCode::IoError, &message);
                        }
                    }
                    Action::Remind => {
                        let (harness, mode) = match (harness_param(&request), mode_param(&request))
                        {
                            (Ok(harness), Ok(mode)) => (harness, mode),
                            (Err(frame), _) | (_, Err(frame)) => return *frame,
                        };
                        if let Err(message) = center.set_reminder(harness, mode) {
                            return failure(
                                request.id.clone(),
                                ErrorCode::InvalidArgument,
                                &message,
                            );
                        }
                    }
                    Action::Versions | Action::Check => {}
                }
                match serde_json::to_value(center.snapshot()) {
                    Ok(value) => ok(&request, value),
                    Err(error) => {
                        failure(request.id.clone(), ErrorCode::IoError, &error.to_string())
                    }
                }
            });
            match task.await {
                Ok(frame) => frame,
                Err(error) => failure(id, ErrorCode::IoError, &error.to_string()),
            }
        })
    })
}
