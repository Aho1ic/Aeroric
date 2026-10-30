//! Durable submission receipts, owned by the existing data-root writer.
//! A crash after `pending` never authorizes a replay.
use std::collections::BTreeMap;
use std::sync::Mutex;

use aeroric_protocol::submission::{
    OptionConsumption, SubmissionLookup, SubmissionReceipt, SubmissionRequest, SubmissionState,
    SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::context::CoreContext;
use crate::storage::atomic::{self, FileMode, ReadOutcome, StorageError};
use crate::storage::compat::require_writer;

#[derive(Debug, thiserror::Error)]
pub enum SubmissionError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("invalid submission identity or fingerprint")]
    Invalid,
    #[error("this submission identity already belongs to a different fingerprint")]
    Conflict,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipts {
    schema_version: u32,
    receipts: BTreeMap<String, SubmissionReceipt>,
}

pub fn fingerprint(request: &SubmissionRequest) -> String {
    format!(
        "sha256:{:x}",
        Sha256::digest(request.fingerprint_material())
    )
}

fn valid_identity(id: &str, fingerprint: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        && fingerprint.strip_prefix("sha256:").is_some_and(|hash| {
            hash.len() == 64
                && hash
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

fn load(context: &CoreContext) -> Result<Receipts, StorageError> {
    let path = context.paths.data_dir().join("task-submissions.json");
    let receipts: Receipts = match atomic::read_json(&path)? {
        ReadOutcome::Absent => Receipts {
            schema_version: SCHEMA_VERSION,
            ..Receipts::default()
        },
        ReadOutcome::Found(value) => {
            serde_json::from_value(value).map_err(|error| StorageError::Corrupt {
                path: path.clone(),
                reason: error.to_string(),
            })?
        }
    };
    if receipts.schema_version != SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchema {
            path,
            found: receipts.schema_version,
            supported: SCHEMA_VERSION,
        });
    }
    if receipts.receipts.iter().any(|(id, receipt)| {
        id != &receipt.submission_id
            || receipt.schema_version != SCHEMA_VERSION
            || !valid_identity(id, &receipt.fingerprint)
            || (receipt.state == SubmissionState::Accepted
                && receipt.task_id.as_deref() != Some(id))
    }) {
        return Err(StorageError::Corrupt {
            path,
            reason: "invalid submission receipt identity".into(),
        });
    }
    Ok(receipts)
}

fn persist(context: &CoreContext, receipts: &Receipts) -> Result<(), StorageError> {
    require_writer(context)?;
    let path = context.paths.data_dir().join("task-submissions.json");
    let bytes = serde_json::to_vec_pretty(receipts).map_err(|error| StorageError::WriteFailed {
        path: path.clone(),
        reason: error.to_string(),
    })?;
    atomic::write_atomic(&path, &bytes, FileMode::Private)
}

fn receipt(
    request: &SubmissionRequest,
    state: SubmissionState,
    reasons: Vec<String>,
) -> SubmissionReceipt {
    SubmissionReceipt {
        schema_version: SCHEMA_VERSION,
        submission_id: request.submission_id.clone(),
        project_id: request.project_id.clone(),
        configuration_id: request.configuration_id.clone(),
        configuration_version: request.configuration_version.clone(),
        fingerprint: request.fingerprint.clone(),
        state,
        task_id: None,
        reasons,
        agent: request.agent.clone(),
        selected_model: request.selected_model.clone(),
        reasoning_effort: request.reasoning_effort.clone(),
        permission_mode: request.permission_mode.clone(),
        permission_agent: request.permission_agent.clone(),
        options: vec![],
    }
}

pub(crate) fn submit(
    context: &CoreContext,
    gate: &Mutex<()>,
    request: &SubmissionRequest,
    mut reasons: Vec<String>,
    execute: impl FnOnce() -> Result<(), super::TaskError>,
) -> Result<SubmissionReceipt, SubmissionError> {
    if !valid_identity(&request.submission_id, &request.fingerprint)
        || request.fingerprint != fingerprint(request)
    {
        return Err(SubmissionError::Invalid);
    }
    if request.schema_version != SCHEMA_VERSION {
        reasons.push("不支持此提交契约版本。".into());
    }
    if request.prompt.trim().is_empty() {
        reasons.push("提示词不能为空。".into());
    }
    if request.permission_agent != request.agent {
        reasons.push("请为当前 Agent 明确重新选择权限；不会继承其他 Agent 的授权。".into());
    }
    if request.team {
        reasons.push("团队协作尚未接通真实交办能力。".into());
    }
    if request.review {
        reasons.push("独立审查尚未接通真实交办能力。".into());
    }
    if request.evolution {
        reasons.push("任务工具进化尚未接通真实交办能力。".into());
    }
    if request.recipe_id.is_some() {
        reasons.push("工作配方尚未接通真实交办能力。".into());
    }
    if !request.attachments.is_empty() {
        reasons.push("附件尚无宿主消费支持；没有读取或上传。".into());
    }
    if request.worktree.is_some() {
        reasons.push("工作目录覆盖尚无宿主消费支持；没有创建 worktree。".into());
    }
    let _guard = gate.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    require_writer(context)?;
    let mut receipts = load(context)?;
    if let Some(previous) = receipts.receipts.get(&request.submission_id) {
        if previous.fingerprint != request.fingerprint {
            return Err(SubmissionError::Conflict);
        }
        // This also applies after completion, deletion, configuration changes and
        // host restarts. Never recreate the task from an old submission.
        return Ok(previous.clone());
    }
    let mut result = receipt(
        request,
        if reasons.is_empty() {
            SubmissionState::Pending
        } else {
            SubmissionState::RejectedBeforeSideEffect
        },
        reasons,
    );
    receipts
        .receipts
        .insert(request.submission_id.clone(), result.clone());
    persist(context, &receipts)?;
    if result.state == SubmissionState::RejectedBeforeSideEffect {
        return Ok(result);
    }

    // This pending record is durable before create/start can reach an executor.
    match execute() {
        Ok(()) => {
            result.state = SubmissionState::Accepted;
            result.task_id = Some(request.submission_id.clone());
            result.options = [
                ("configuration", true),
                ("model", request.selected_model.is_some()),
                ("effort", request.reasoning_effort.is_some()),
                ("permission", true),
                ("attachments", false),
                ("worktree", false),
                ("recipe", false),
                ("team", false),
                ("review", false),
                ("evolution", false),
                ("clarify", request.clarify),
            ]
            .into_iter()
            .map(|(option, applied)| OptionConsumption {
                option: option.into(),
                applied,
            })
            .collect();
        }
        Err(_) => {
            // The execution port may have made a partial external side effect.
            // Even an RPC error is not evidence that sending is safe again.
            result.state = SubmissionState::Unknown;
            result
                .reasons
                .push("任务创建或启动未取得完整回执；保留原身份核对，不自动重发。".into());
        }
    }
    receipts
        .receipts
        .insert(request.submission_id.clone(), result.clone());
    // Failure here leaves the durable pending record. A later lookup remains
    // inconclusive; it must never rerun `execute`.
    persist(context, &receipts)?;
    Ok(result)
}

pub(crate) fn lookup(
    context: &CoreContext,
    gate: &Mutex<()>,
    submission_id: &str,
    fingerprint: &str,
) -> Result<SubmissionLookup, SubmissionError> {
    if !valid_identity(submission_id, fingerprint) {
        return Err(SubmissionError::Invalid);
    }
    let _guard = gate.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let receipts = load(context)?;
    let receipt = receipts.receipts.get(submission_id).cloned();
    if receipt
        .as_ref()
        .is_some_and(|receipt| receipt.fingerprint != fingerprint)
    {
        return Err(SubmissionError::Conflict);
    }
    Ok(SubmissionLookup {
        schema_version: SCHEMA_VERSION,
        submission_id: submission_id.into(),
        fingerprint: fingerprint.into(),
        receipt,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_support::writer;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    #[test]
    fn fixture_pins_utf8_nil_and_fingerprint_without_json_assumptions() {
        let request = aeroric_protocol::submission::fixture_request();
        assert_eq!(fingerprint(&request), request.fingerprint);
        let mut changed = request.clone();
        changed.reasoning_effort = Some(String::new());
        assert_ne!(
            fingerprint(&changed),
            request.fingerprint,
            "null differs from empty"
        );
        changed = request.clone();
        changed.submission_id = "another-attempt".into();
        assert_eq!(
            fingerprint(&changed),
            request.fingerprint,
            "attempt identity is separate"
        );
    }

    #[test]
    fn concurrent_retries_and_reopen_return_one_immutable_receipt() {
        let (root, context) = writer("native-receipts-concurrent");
        let context = Arc::new(context);
        let gate = Arc::new(Mutex::new(()));
        let count = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(8));
        let request = aeroric_protocol::submission::fixture_request();
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let (context, gate, count, barrier, request) = (
                    context.clone(),
                    gate.clone(),
                    count.clone(),
                    barrier.clone(),
                    request.clone(),
                );
                std::thread::spawn(move || {
                    barrier.wait();
                    submit(&context, &gate, &request, vec![], || {
                        let pending = load(&context).unwrap();
                        assert_eq!(
                            pending.receipts[&request.submission_id].state,
                            SubmissionState::Pending
                        );
                        count.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    })
                    .unwrap()
                })
            })
            .collect();
        let receipts: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert!(receipts.iter().all(|receipt| receipt == &receipts[0]));
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(receipts[0].state, SubmissionState::Accepted);
        let reopened = lookup(
            &context,
            &Mutex::new(()),
            &request.submission_id,
            &request.fingerprint,
        )
        .unwrap();
        assert_eq!(reopened.receipt, Some(receipts[0].clone()));
        let mut changed = request;
        changed.prompt.push_str("changed");
        changed.fingerprint = fingerprint(&changed);
        assert!(matches!(
            submit(&context, &gate, &changed, vec![], || panic!("no replay")),
            Err(SubmissionError::Conflict)
        ));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(context.paths.data_dir().join("task-submissions.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        drop(context);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unknown_pending_missing_corrupt_and_read_only_never_authorize_execution() {
        let (root, context) = writer("native-receipts-unknown");
        let gate = Mutex::new(());
        let request = aeroric_protocol::submission::fixture_request();
        assert!(lookup(
            &context,
            &gate,
            &request.submission_id,
            &request.fingerprint
        )
        .unwrap()
        .receipt
        .is_none());
        let result = submit(&context, &gate, &request, vec![], || {
            Err(super::super::TaskError::Launch(
                "fake partial execution".into(),
            ))
        })
        .unwrap();
        assert_eq!(result.state, SubmissionState::Unknown);
        assert_eq!(
            submit(&context, &gate, &request, vec![], || panic!("no replay")).unwrap(),
            result
        );
        let path = context.paths.data_dir().join("task-submissions.json");
        let mut state = load(&context).unwrap();
        state
            .receipts
            .get_mut(&request.submission_id)
            .unwrap()
            .state = SubmissionState::Pending;
        persist(&context, &state).unwrap();
        assert_eq!(
            submit(&context, &gate, &request, vec![], || panic!(
                "no replay after crash"
            ))
            .unwrap()
            .state,
            SubmissionState::Pending
        );
        let before = std::fs::read(&path).unwrap();
        let reader = CoreContext::read_only(context.paths.clone());
        assert!(submit(&reader, &gate, &request, vec![], || panic!("no writer")).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::write(&path, b"{malformed").unwrap();
        assert!(submit(&context, &gate, &request, vec![], || panic!(
            "no corrupt fallback"
        ))
        .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"{malformed");
        drop(context);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn every_unsupported_option_gets_a_pre_execution_refusal() {
        let (root, context) = writer("native-receipts-options");
        let mut request = aeroric_protocol::submission::fixture_request();
        request.attachments.push("/synthetic/never-read".into());
        request.worktree = Some("/synthetic/never-created".into());
        request.recipe_id = Some("fixture".into());
        request.team = true;
        request.review = true;
        request.evolution = true;
        request.fingerprint = fingerprint(&request);
        let receipt = submit(&context, &Mutex::new(()), &request, vec![], || {
            panic!("no unsupported execution")
        })
        .unwrap();
        assert_eq!(receipt.state, SubmissionState::RejectedBeforeSideEffect);
        for name in [
            "附件",
            "工作目录",
            "工作配方",
            "团队协作",
            "独立审查",
            "任务工具进化",
        ] {
            assert!(
                receipt.reasons.iter().any(|reason| reason.contains(name)),
                "{name}"
            );
        }
        assert!(receipt.task_id.is_none());
        assert!(!root.join("synthetic").exists());
        drop(context);
        std::fs::remove_dir_all(root).unwrap();
    }
}
