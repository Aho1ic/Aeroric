//! C3 (2026-11-20): a 整理's life in 整理记录 — queued by the automatic
//! pass, running while the model is asked and its reply written, canceled
//! by the user, or found unfinished when Aeroric starts again. A row is in
//! the store for as long as the run is real, so the log always matches
//! what is going on.

use aeroric_protocol::memory::{
    Confidence, Evidence, ExperienceDetail, MemoryDraft, MemoryItem, MemoryRun, MemoryRunKind,
    MemoryRunState, RunOutcomeSubject,
};
use serde::{Deserialize, Serialize};

use super::store::Store;
use super::{Memory, MemoryError};

/// Said of a run that was going when Aeroric stopped.
pub const INTERRUPTED: &str = "Aeroric 退出时这次整理还没做完，没写进去的可以重试";
/// Said of a queued run the user canceled.
pub const CANCELED_QUEUED: &str =
    "排队时取消，没有调用模型；这个会话不再自动整理，需要时在会话菜单里手动整理";

/// At open: a run left `Running` failed (it can be retried, and an
/// extraction's session stays unmarked so the automatic pass takes it up
/// again); a `Queued` row is dropped — it never started, and the next pass
/// queues it again.
pub(super) fn recover(store: &Store) -> Result<(), MemoryError> {
    for mut run in store.open_runs()? {
        match run.state {
            MemoryRunState::Queued => store.delete_run(&run.id)?,
            MemoryRunState::Running => {
                run.state = MemoryRunState::Failed;
                run.reason = Some(match run.reason.take() {
                    Some(before) => format!("{INTERRUPTED}；{before}"),
                    None => INTERRUPTED.into(),
                });
                store.add_run(&run)?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// A new row for one session's extraction or one scope's dream.
pub(super) fn new_run(
    kind: MemoryRunKind,
    task_id: &str,
    project_id: &str,
    title: &str,
    state: MemoryRunState,
    now_ms: u64,
) -> MemoryRun {
    MemoryRun {
        id: format!("run-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]),
        task_id: task_id.to_string(),
        project_id: project_id.to_string(),
        title: title.to_string(),
        at_ms: now_ms,
        state,
        reason: None,
        model: None,
        input_tokens: 0,
        output_tokens: 0,
        cost_usd: None,
        facts: 0,
        candidates: 0,
        experiences: 0,
        failed_writes: 0,
        kind,
        retired: Vec::new(),
        proposed: 0,
        undone_ms: None,
        outcomes: Vec::new(),
    }
}

impl Memory {
    /// The automatic pass found a session to extract: a `Queued` row,
    /// shown at once; [`run::Session::run_id`](super::run::Session) carries
    /// its id into the extraction.
    pub fn queue_run(
        &self,
        task_id: &str,
        project_id: &str,
        title: &str,
        now_ms: u64,
    ) -> Result<String, MemoryError> {
        let run = new_run(
            MemoryRunKind::Extract,
            task_id,
            project_id,
            title,
            MemoryRunState::Queued,
            now_ms,
        );
        self.with(|store| store.add_run(&run))?;
        Ok(run.id)
    }

    /// The row, if the run is still waiting to start.
    pub fn queued(&self, run_id: &str) -> bool {
        self.run(run_id)
            .is_some_and(|run| run.state == MemoryRunState::Queued)
    }

    /// A queued row the pass did not get to (the day's limit, already
    /// done): it leaves the log.
    pub fn unqueue(&self, run_id: &str) {
        if self.queued(run_id) {
            let _ = self.with(|store| store.delete_run(run_id));
        }
    }

    pub fn run(&self, run_id: &str) -> Option<MemoryRun> {
        self.with(|store| store.run(run_id)).ok().flatten()
    }

    /// The run starts: its row says `Running` until it ends.
    pub(super) fn begin_run(&self, run: &mut MemoryRun) -> Result<(), MemoryError> {
        run.state = MemoryRunState::Running;
        self.with(|store| store.add_run(run))
    }

    /// `memory.runCancel`: a queued run is canceled at once (an
    /// extraction's session is marked so the automatic pass leaves it); a
    /// running one stops before its next write — what it already wrote
    /// stays. The row as it is now.
    pub fn cancel_run(&self, run_id: &str, now_ms: u64) -> Result<MemoryRun, MemoryError> {
        let mut run = self.run(run_id).ok_or(MemoryError::UnknownItem)?;
        match run.state {
            MemoryRunState::Queued => {
                run.state = MemoryRunState::Canceled;
                run.reason = Some(CANCELED_QUEUED.into());
                self.with(|store| {
                    store.add_run(&run)?;
                    store.mark_extracted(&run.task_id, "", now_ms)
                })?;
                Ok(run)
            }
            MemoryRunState::Running => {
                self.cancels.lock().insert(run.id.clone());
                Ok(run)
            }
            _ => Err(MemoryError::WrongState),
        }
    }

    /// Whether the user asked to cancel this run.
    pub(super) fn cancel_requested(&self, run_id: &str) -> bool {
        self.cancels.lock().contains(run_id)
    }

    /// The run ended (however): its cancel request is spent.
    pub(super) fn end_run(&self, run_id: &str) {
        self.cancels.lock().remove(run_id);
    }
}

/// C3 second step: one write an extraction wants to make, whole — so a
/// write the store failed can be made again without asking the model.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "subject", rename_all = "camelCase")]
pub(super) enum PendingWrite {
    #[serde(rename_all = "camelCase")]
    Fact {
        project_id: String,
        text: String,
        evidence: Evidence,
    },
    #[serde(rename_all = "camelCase")]
    Experience {
        project_id: String,
        title: String,
        detail: ExperienceDetail,
        confidence: Confidence,
        evidence: Evidence,
    },
    Candidate {
        draft: MemoryDraft,
        evidence: Evidence,
    },
}

impl PendingWrite {
    pub(super) fn subject(&self) -> RunOutcomeSubject {
        match self {
            Self::Fact { .. } => RunOutcomeSubject::Fact,
            Self::Experience { .. } => RunOutcomeSubject::Experience,
            Self::Candidate { .. } => RunOutcomeSubject::Candidate,
        }
    }

    /// The words it would keep (what forgetting and withdrawing compare).
    pub(super) fn text(&self) -> &str {
        match self {
            Self::Fact { text, .. } => text,
            Self::Experience { title, .. } => title,
            Self::Candidate { draft, .. } => &draft.text,
        }
    }

    pub(super) fn apply(&self, memory: &Memory, now_ms: u64) -> Result<MemoryItem, MemoryError> {
        match self {
            Self::Fact {
                project_id,
                text,
                evidence,
            } => memory.add_session_fact(project_id, text, evidence.clone(), now_ms),
            Self::Experience {
                project_id,
                title,
                detail,
                confidence,
                evidence,
            } => memory.add_experience(
                project_id,
                title,
                detail,
                *confidence,
                evidence.clone(),
                now_ms,
            ),
            Self::Candidate { draft, evidence } => {
                memory.propose(draft.clone(), evidence.clone(), now_ms)
            }
        }
    }
}

/// C3 second step: what one run still has to write. `index` is the write's
/// place in the run's `outcomes`; `epoch` the withdrawal number its model
/// call started at (C2: what was withdrawn since is not written).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Retry {
    pub epoch: u64,
    pub writes: Vec<(usize, PendingWrite)>,
}

impl Memory {
    /// Runs with writes waiting to be tried again, oldest first.
    pub fn retryable(&self, now_ms: u64) -> Vec<String> {
        self.with(|store| store.retry_ids(now_ms))
            .unwrap_or_default()
    }

    pub(super) fn save_retry(
        &self,
        run_id: &str,
        retry: &Retry,
        now_ms: u64,
    ) -> Result<(), MemoryError> {
        let body = serde_json::to_string(retry).map_err(|e| MemoryError::Storage(e.to_string()))?;
        self.with(|store| store.put_retry(run_id, now_ms, &body))
    }

    pub(super) fn load_retry(&self, run_id: &str) -> Result<Option<Retry>, MemoryError> {
        let body = self.with(|store| store.retry(run_id))?;
        Ok(body.and_then(|body| serde_json::from_str(&body).ok()))
    }

    pub(super) fn drop_retry(&self, run_id: &str) -> Result<(), MemoryError> {
        self.with(|store| store.delete_retry(run_id))
    }

    /// Whether the full-text index is whole (`memory.runs` says so, and
    /// the App offers 重建索引 when it is not).
    pub fn index_healthy(&self) -> bool {
        self.with(|store| Ok(store.index_healthy()))
            .unwrap_or(false)
    }

    /// 重建索引: the full-text index written again from the items, vectors
    /// of items gone dropped — a row of its own in 整理记录. No model is
    /// asked; the background embeds what lacks a vector.
    pub fn rebuild_index(&self, now_ms: u64) -> Result<MemoryRun, MemoryError> {
        let mut run = new_run(
            MemoryRunKind::Reindex,
            "",
            "",
            "重建索引",
            MemoryRunState::Running,
            now_ms,
        );
        self.begin_run(&mut run)?;
        let result = self.with(|store| store.rebuild_index());
        self.end_run(&run.id);
        match result {
            Ok((indexed, dropped, missing)) => {
                run.state = MemoryRunState::Done;
                let mut notes = vec![format!("全文索引重写了 {indexed} 条")];
                if dropped > 0 {
                    notes.push(format!("清掉 {dropped} 个已不存在的记忆的向量"));
                }
                if missing > 0 {
                    notes.push(format!("{missing} 条的语义向量等后台补算"));
                }
                run.reason = Some(notes.join("；"));
            }
            Err(error) => {
                run.state = MemoryRunState::Failed;
                run.reason = Some(format!("重建索引失败：{error}"));
            }
        }
        self.add_run(&run)?;
        Ok(run)
    }
}
