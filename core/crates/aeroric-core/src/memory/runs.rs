//! C3 (2026-11-20): a 整理's life in 整理记录 — queued by the automatic
//! pass, running while the model is asked and its reply written, canceled
//! by the user, or found unfinished when Aeroric starts again. A row is in
//! the store for as long as the run is real, so the log always matches
//! what is going on.

use aeroric_protocol::memory::{MemoryRun, MemoryRunKind, MemoryRunState};

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
