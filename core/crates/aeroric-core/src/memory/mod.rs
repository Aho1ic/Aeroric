//! B5-1 long-term memory (2026-10-31): the store behind the "记忆" page,
//! `/记住`, the profile a starting session is handed and the context
//! server's `memory_recall` / `memory_propose` (frontier-roadmap §6;
//! Q-M1–Q-M8). Aeroric is the only writer; an item is never edited in place;
//! forgetting leaves a signature without the text (`barrier`).
//!
//! Kept in `<data>/memory/memory.db` (SQLite with an FTS5 index over the
//! CJK-segmented text, `notebook::rag::cjk`).

mod barrier;
pub mod chain;
pub mod contradict;
pub mod dream;
pub mod extract;
pub mod locate;
pub mod native;
pub mod placement;
pub(crate) mod profile;
pub mod run;
pub mod runs;
mod secrets;
pub mod semantic;
pub mod skills;
pub mod stale;
mod store;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod withdraw_tests;

use std::path::PathBuf;

use aeroric_protocol::memory::{
    Assertion, Evidence, MemoryDraft, MemoryItem, MemoryState, Withdrawal, MAX_EXCERPT_CHARS,
    MAX_TEXT_CHARS, RECALL_LIMIT,
};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};

use store::Store;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryError {
    /// The store has not been given its folder (or could not open it).
    Unavailable(String),
    Empty,
    TooLong,
    /// Looks like a key or token (what it looked like).
    Secret(&'static str),
    /// The user forgot this; an agent may not bring it back.
    Forgotten,
    UnknownItem,
    /// B5-3b: a skill name or file problem.
    Skill(String),
    /// The operation does not apply to an item in this state.
    WrongState,
    Storage(String),
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(why) => write!(f, "记忆暂不可用：{why}"),
            Self::Empty => write!(f, "记忆内容是空的"),
            Self::TooLong => write!(f, "一条记忆最多 {MAX_TEXT_CHARS} 个字"),
            Self::Secret(what) => write!(f, "看起来像密钥（{what}），不能写进记忆"),
            Self::Forgotten => write!(f, "用户已经让 Aeroric 永久忘掉这条内容，不能再提议"),
            Self::UnknownItem => write!(f, "没有这条记忆"),
            Self::Skill(why) => write!(f, "技能：{why}"),
            Self::WrongState => write!(f, "这条记忆现在的状态不能这样操作"),
            Self::Storage(why) => write!(f, "记忆存储出错：{why}"),
        }
    }
}

impl std::error::Error for MemoryError {}

#[cfg(test)]
impl Memory {
    /// Fail writes the way a damaged store would.
    pub(crate) fn execute_for_test(&self, sql: &str) {
        self.with(|store| store.execute_for_test(sql)).unwrap();
    }
}

impl From<rusqlite::Error> for MemoryError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

/// Evidence for an item, from the words it came from.
pub fn evidence(
    task_id: Option<&str>,
    harness: Option<&str>,
    model: Option<&str>,
    source: &str,
    at_ms: u64,
) -> Evidence {
    let digest = Sha256::digest(source.as_bytes());
    let hash: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    Evidence {
        task_id: task_id.map(str::to_string),
        harness: harness.map(str::to_string),
        model: model.map(str::to_string),
        excerpt: source.chars().take(MAX_EXCERPT_CHARS).collect(),
        hash,
        at_ms,
        location: None,
    }
}

/// One char in its comparable form: none for whitespace, half-width, lower
/// case (C1: shared with finding the words in a message).
pub(crate) fn fold(c: char) -> impl Iterator<Item = char> {
    let c = match c as u32 {
        _ if c.is_whitespace() => None,
        0xFF01..=0xFF5E => Some(char::from_u32(c as u32 - 0xFEE0).unwrap_or(c)),
        _ => Some(c),
    };
    c.into_iter().flat_map(char::to_lowercase)
}

/// The comparable form of a text: no whitespace, half-width, lower case,
/// without closing punctuation.
pub(crate) fn normalize(text: &str) -> String {
    let mut out: String = text.chars().flat_map(fold).collect();
    while out.ends_with(['.', '。', '!', '！', ';', '；', ',', '，']) {
        out.pop();
    }
    out
}

#[derive(Default)]
pub struct Memory {
    store: Mutex<Option<Store>>,
    /// B5-3b: the data folder (learned skills live beside the built-in ones).
    data_dir: Mutex<Option<PathBuf>>,
    /// H7: the store's own folder (which native files were read, by content).
    dir: Mutex<Option<PathBuf>>,
    /// B6: scopes being dreamt now (one dream per scope at a time).
    dreaming: Mutex<std::collections::HashSet<String>>,
    /// C3: runs the user asked to cancel while they ran.
    cancels: Mutex<std::collections::HashSet<String>>,
}

/// E8: [`Memory::profile_load`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProfileLoad {
    pub text: Option<String>,
    pub used_chars: usize,
    /// How many items it carries.
    pub carried: usize,
    /// Left out by the budget (`PROFILE_BUDGET_CHARS`).
    pub left_out: Vec<MemoryItem>,
    /// Facts H7 moved out of the opening.
    pub demoted: Vec<MemoryItem>,
}

impl Memory {
    /// Open (or create) the store under `dir`.
    pub fn set_dir(&self, dir: PathBuf) -> Result<(), MemoryError> {
        let store = Store::open(&dir)?;
        // C3: what was queued or running when Aeroric last stopped.
        runs::recover(&store)?;
        *self.store.lock() = Some(store);
        *self.data_dir.lock() = dir.parent().map(PathBuf::from);
        *self.dir.lock() = Some(dir);
        Ok(())
    }

    fn with<T>(
        &self,
        f: impl FnOnce(&mut Store) -> Result<T, MemoryError>,
    ) -> Result<T, MemoryError> {
        let mut guard = self.store.lock();
        let store = guard
            .as_mut()
            .ok_or_else(|| MemoryError::Unavailable("存储未打开".into()))?;
        f(store)
    }

    fn check(draft: &MemoryDraft) -> Result<String, MemoryError> {
        let text = draft.text.trim().to_string();
        if text.is_empty() {
            return Err(MemoryError::Empty);
        }
        if text.chars().count() > MAX_TEXT_CHARS {
            return Err(MemoryError::TooLong);
        }
        if let Some(what) = secrets::looks_secret(&text) {
            return Err(MemoryError::Secret(what));
        }
        Ok(text)
    }

    /// The user said to keep this: active at once, and an earlier "forget"
    /// of the same words no longer applies (a new, explicit wish). Turns the
    /// project's memory on.
    pub fn remember(
        &self,
        draft: MemoryDraft,
        evidence: Evidence,
        now_ms: u64,
    ) -> Result<MemoryItem, MemoryError> {
        let text = Self::check(&draft)?;
        self.with(|store| {
            store.lift_barrier(&normalize(&text))?;
            if let Some(project) = &draft.project_id {
                store.set_enabled(project, true)?;
            }
            store.insert(
                &draft,
                &text,
                Assertion::UserExplicit,
                MemoryState::Active,
                evidence,
                now_ms,
            )
        })
    }

    /// An agent proposes: pending until the user confirms. The same words
    /// already kept only gain the evidence; forgotten words are refused.
    pub fn propose(
        &self,
        draft: MemoryDraft,
        evidence: Evidence,
        now_ms: u64,
    ) -> Result<MemoryItem, MemoryError> {
        let text = Self::check(&draft)?;
        self.with(|store| {
            if store.barred(&normalize(&text))? {
                return Err(MemoryError::Forgotten);
            }
            store.insert(
                &draft,
                &text,
                Assertion::ModelInferred,
                MemoryState::Pending,
                evidence,
                now_ms,
            )
        })
    }

    /// The user accepts a proposal (it becomes agent-confirmed) or rejects
    /// it (it is forgotten, so the agent cannot bring it back).
    pub fn confirm(
        &self,
        id: &str,
        accept: bool,
        now_ms: u64,
    ) -> Result<Option<MemoryItem>, MemoryError> {
        self.with(|store| {
            let item = store.get(id)?.ok_or(MemoryError::UnknownItem)?;
            if item.state != MemoryState::Pending {
                return Err(MemoryError::WrongState);
            }
            if accept {
                store.activate(&item, now_ms).map(Some)
            } else {
                store.forget(&item, now_ms)?;
                Ok(None)
            }
        })
    }

    /// 全部确认 / 全部拒绝: many proposals under one lock; returns how many
    /// were decided (an item no longer pending is skipped, not an error).
    pub fn confirm_many(
        &self,
        ids: &[String],
        accept: bool,
        now_ms: u64,
    ) -> Result<usize, MemoryError> {
        let mut done = 0;
        for id in ids {
            match self.confirm(id, accept, now_ms) {
                Ok(_) => done += 1,
                Err(MemoryError::WrongState | MemoryError::UnknownItem) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(done)
    }

    /// 归档 / 恢复. C2: archiving withdraws the words from runs still going,
    /// marks skills made from it 依据已撤回 and withdraws dream proposals
    /// about it; restoring takes the skills' mark back.
    pub fn archive(
        &self,
        id: &str,
        archived: bool,
        now_ms: u64,
    ) -> Result<(MemoryItem, Withdrawal), MemoryError> {
        self.with(|store| {
            let item = store.get(id)?.ok_or(MemoryError::UnknownItem)?;
            let (from, to) = if archived {
                (MemoryState::Active, MemoryState::Archived)
            } else {
                (MemoryState::Archived, MemoryState::Active)
            };
            if item.state != from {
                return Err(MemoryError::WrongState);
            }
            let ids = [id.to_string()];
            let mut withdrawal = Withdrawal::default();
            if archived {
                store.withdraw(&[&item.text], now_ms)?;
                withdrawal.items = 1;
                withdrawal.experiences = experiences(std::slice::from_ref(&item));
                withdrawal.skills = store.mark_skill_sources(&ids, Some(now_ms))?;
                withdrawal.dreams = store.withdraw_dreams(&ids)?;
            } else {
                store.mark_skill_sources(&ids, None)?;
            }
            Ok((store.set_state(id, to, now_ms)?, withdrawal))
        })
    }

    pub fn pin(&self, id: &str, pinned: bool, now_ms: u64) -> Result<MemoryItem, MemoryError> {
        self.with(|store| {
            store.get(id)?.ok_or(MemoryError::UnknownItem)?;
            store.set_pinned(id, pinned, now_ms)
        })
    }

    /// 改为全局 / back into a project.
    pub fn move_to(
        &self,
        id: &str,
        project_id: Option<&str>,
        now_ms: u64,
    ) -> Result<MemoryItem, MemoryError> {
        self.with(|store| {
            store.get(id)?.ok_or(MemoryError::UnknownItem)?;
            store.set_project(id, project_id, now_ms)
        })
    }

    /// 撤销 right after `/记住`: the item goes and leaves no signature.
    pub fn undo(&self, id: &str) -> Result<(), MemoryError> {
        self.with(|store| {
            let item = store.get(id)?.ok_or(MemoryError::UnknownItem)?;
            store.delete(&item)
        })
    }

    /// 永久遗忘: the item and the versions it replaced are deleted; their
    /// signatures stay so they cannot come back.
    /// C2: and what it touched beyond the item (see [`Withdrawal`]).
    pub fn forget(&self, id: &str, now_ms: u64) -> Result<Withdrawal, MemoryError> {
        self.with(|store| {
            let item = store.get(id)?.ok_or(MemoryError::UnknownItem)?;
            let gone = store.forget(&item, now_ms)?;
            let ids: Vec<String> = gone.iter().map(|i| i.id.clone()).collect();
            Ok(Withdrawal {
                items: ids.len() as u32,
                skills: store.mark_skill_sources(&ids, Some(now_ms))?,
                dreams: store.withdraw_dreams(&ids)?,
                experiences: experiences(&gone),
                ..Withdrawal::default()
            })
        })
    }

    /// C2: Wiki withdrawals that failed, to try again (ids, never words).
    pub fn wiki_backlog(&self) -> Vec<WikiBacklog> {
        self.with(|store| {
            Ok(store
                .kv(WIKI_BACKLOG)?
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default())
        })
        .unwrap_or_default()
    }

    pub fn set_wiki_backlog(&self, backlog: &[WikiBacklog]) -> Result<(), MemoryError> {
        let text = serde_json::to_string(backlog).unwrap_or_else(|_| "[]".into());
        self.with(|store| store.set_kv(WIKI_BACKLOG, &text))
    }

    /// C2: whether these words were forgotten (永久遗忘): the Wiki scrubs
    /// them from its history.
    pub fn is_forgotten(&self, text: &str) -> bool {
        self.with(|store| store.barred(&normalize(text)))
            .unwrap_or(false)
    }

    /// C2: the number of the last withdrawal; a run reads it before its
    /// model call and checks [`Self::withdrawn_since`] before writing.
    pub fn epoch(&self) -> u64 {
        self.with(|store| store.epoch()).unwrap_or(0)
    }

    /// C2: which of `texts` were deleted, archived or replaced after
    /// `epoch` (compared as [`normalize`] does).
    pub fn withdrawn_since(&self, epoch: u64, texts: &[&str]) -> Result<Vec<bool>, MemoryError> {
        self.with(|store| {
            let gone = store.withdrawn_since(epoch)?;
            Ok(texts
                .iter()
                .map(|text| gone.contains(&store.signature(text)))
                .collect())
        })
    }

    /// Every item in a scope (`None` is global), newest first; with a query,
    /// only those that match.
    pub fn list(
        &self,
        project_id: Option<&str>,
        query: Option<&str>,
    ) -> Result<Vec<MemoryItem>, MemoryError> {
        self.with(|store| store.list(project_id, query))
    }

    /// Proposals waiting for the user, in every scope.
    pub fn attention(&self) -> Result<Vec<MemoryItem>, MemoryError> {
        self.with(|store| store.pending())
    }

    pub fn project_enabled(&self, project_id: &str) -> bool {
        self.with(|store| store.enabled(project_id))
            .unwrap_or(false)
    }

    pub fn set_project_enabled(&self, project_id: &str, enabled: bool) -> Result<(), MemoryError> {
        self.with(|store| store.set_enabled(project_id, enabled))
    }

    /// B5-2: a finished session's clue — active at once, model-made, never
    /// injected. A forgotten clue does not come back.
    pub fn add_session_fact(
        &self,
        project_id: &str,
        text: &str,
        evidence: Evidence,
        now_ms: u64,
    ) -> Result<MemoryItem, MemoryError> {
        let mut draft = MemoryDraft {
            project_id: Some(project_id.to_string()),
            kind: aeroric_protocol::memory::MemoryKind::SessionFact,
            category: None,
            text: text.to_string(),
            supersedes: None,
        };
        let text = Self::check(&draft)?;
        self.with(|store| {
            if store.barred(&normalize(&text))? {
                return Err(MemoryError::Forgotten);
            }
            // CU F1 (2026-10-31): one clue per session — extracting it again
            // replaces the clue it had.
            if let Some(task_id) = &evidence.task_id {
                draft.supersedes = store.session_fact_of(task_id)?;
            }
            store.insert(
                &draft,
                &text,
                Assertion::ModelInferred,
                MemoryState::Active,
                evidence,
                now_ms,
            )
        })
    }

    pub fn settings(&self) -> aeroric_protocol::memory::MemorySettings {
        self.with(|store| {
            Ok(store
                .kv("settings")?
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default())
        })
        .unwrap_or_default()
    }

    pub fn set_settings(
        &self,
        settings: &aeroric_protocol::memory::MemorySettings,
    ) -> Result<(), MemoryError> {
        let text =
            serde_json::to_string(settings).map_err(|e| MemoryError::Storage(e.to_string()))?;
        self.with(|store| store.set_kv("settings", &text))
    }

    pub fn add_run(&self, run: &aeroric_protocol::memory::MemoryRun) -> Result<(), MemoryError> {
        self.with(|store| store.add_run(run))
    }

    /// R1-1: the run and its transcript's mark commit together.
    pub fn finish_run(
        &self,
        run: &aeroric_protocol::memory::MemoryRun,
        task_id: &str,
        hash: &str,
        now_ms: u64,
    ) -> Result<(), MemoryError> {
        self.with(|store| store.finish_run(run, task_id, hash, now_ms))
    }

    pub fn runs(
        &self,
        limit: usize,
    ) -> Result<Vec<aeroric_protocol::memory::MemoryRun>, MemoryError> {
        self.with(|store| store.runs(limit))
    }

    /// Extractions (done or failed) since `since_ms`, for the daily limit.
    pub fn counted_since(&self, since_ms: u64) -> u32 {
        self.with(|store| store.counted_since(since_ms))
            .unwrap_or(0)
    }

    pub fn extracted(&self, task_id: &str) -> Option<String> {
        self.with(|store| store.extracted(task_id)).ok().flatten()
    }

    pub fn mark_extracted(
        &self,
        task_id: &str,
        hash: &str,
        now_ms: u64,
    ) -> Result<(), MemoryError> {
        self.with(|store| store.mark_extracted(task_id, hash, now_ms))
    }

    /// Active profile and fact items a model may see: the project's and global.
    pub fn kept(&self, project_id: &str) -> Vec<MemoryItem> {
        self.with(|store| {
            let mut items = store.active(Some(project_id))?;
            items.extend(store.active(None)?);
            items.retain(|i| {
                !matches!(
                    i.kind,
                    aeroric_protocol::memory::MemoryKind::SessionFact
                        | aeroric_protocol::memory::MemoryKind::Experience
                )
            });
            Ok(items)
        })
        .unwrap_or_default()
    }

    /// What a starting session is handed: the user's profile, and the
    /// project's facts when the project uses memory. `None` when empty.
    pub fn profile(&self, project_id: Option<&str>) -> Option<String> {
        self.render_profile(project_id).map(|(text, _)| text)
    }

    /// B5-3a: [`Memory::profile`] for a starting session — what it carries
    /// counts as injected; C3: once per task (a resumed session is not a
    /// new use).
    pub fn profile_for_session(
        &self,
        project_id: Option<&str>,
        task_id: Option<&str>,
    ) -> Option<String> {
        let (text, ids) = self.render_profile(project_id)?;
        let _ = self.with(|store| store.bump(&ids, "injected", task_id));
        Some(text)
    }

    /// E8 (2026-11-22): what a session started now would be handed, and
    /// what it would not: the items the budget left out, and the facts H7
    /// moved out of the opening (found by recall only).
    pub fn profile_load(&self, project_id: Option<&str>) -> ProfileLoad {
        self.with(|store| {
            let demoted = store.demoted()?;
            let global = store.active(None)?;
            let project = match project_id {
                Some(id) if store.enabled(id)? => store.active(Some(id))?,
                _ => Vec::new(),
            };
            let (kept_global, moved_global): (Vec<_>, Vec<_>) = global
                .into_iter()
                .partition(|i| !demoted.contains_key(&i.id));
            let (kept_project, moved_project): (Vec<_>, Vec<_>) = project
                .into_iter()
                .partition(|i| !demoted.contains_key(&i.id));
            let load = profile::measure(&kept_global, &kept_project);
            let by_id = |id: &String| {
                kept_project
                    .iter()
                    .chain(&kept_global)
                    .find(|i| &i.id == id)
                    .cloned()
            };
            let left_out = load.left_out.iter().filter_map(by_id).collect();
            let demoted = moved_project
                .into_iter()
                .chain(moved_global)
                .filter(|i| {
                    matches!(
                        i.kind,
                        aeroric_protocol::memory::MemoryKind::Profile
                            | aeroric_protocol::memory::MemoryKind::Fact
                    )
                })
                .collect();
            Ok(ProfileLoad {
                text: load.text,
                used_chars: load.used_chars,
                carried: load.carried.len(),
                left_out,
                demoted,
            })
        })
        .unwrap_or_default()
    }

    fn render_profile(&self, project_id: Option<&str>) -> Option<(String, Vec<String>)> {
        self.with(|store| {
            // H7: a fact moved out of the opening is found by recall only.
            let demoted = store.demoted()?;
            let kept = |items: Vec<MemoryItem>| -> Vec<MemoryItem> {
                items
                    .into_iter()
                    .filter(|i| !demoted.contains_key(&i.id))
                    .collect()
            };
            let global = kept(store.active(None)?);
            let project = match project_id {
                Some(id) if store.enabled(id)? => kept(store.active(Some(id))?),
                _ => Vec::new(),
            };
            Ok(profile::render(&global, &project))
        })
        .ok()
        .flatten()
    }

    /// One item, with its evidence.
    pub fn item(&self, id: &str) -> Result<Option<MemoryItem>, MemoryError> {
        self.with(|store| store.get(id))
    }

    /// B5-3a: a reusable fix, active at once, recall only (Q-M17, Q-M18).
    pub fn add_experience(
        &self,
        project_id: &str,
        title: &str,
        detail: &aeroric_protocol::memory::ExperienceDetail,
        confidence: aeroric_protocol::memory::Confidence,
        evidence: Evidence,
        now_ms: u64,
    ) -> Result<MemoryItem, MemoryError> {
        let draft = MemoryDraft {
            project_id: Some(project_id.to_string()),
            kind: aeroric_protocol::memory::MemoryKind::Experience,
            category: None,
            text: title.to_string(),
            supersedes: None,
        };
        let text = Self::check(&draft)?;
        let body = format!("{} {}", detail.problem, detail.solution);
        if let Some(what) = secrets::looks_secret(&body) {
            return Err(MemoryError::Secret(what));
        }
        self.with(|store| {
            if store.barred(&normalize(&text))? {
                return Err(MemoryError::Forgotten);
            }
            let item = store.insert(
                &draft,
                &text,
                Assertion::ModelInferred,
                MemoryState::Active,
                evidence,
                now_ms,
            )?;
            if item.confidence.is_none() {
                store.set_experience(&item.id, &text, detail, confidence)?;
            }
            store.get(&item.id)?.ok_or(MemoryError::UnknownItem)
        })
    }

    /// C3: an agent's 有用 / 不对 — one vote per item per task, so one
    /// session cannot raise (or sink) an experience's confidence on its own.
    /// `None` when this task already voted on it.
    pub fn feedback_in_task(
        &self,
        id: &str,
        helpful: bool,
        task_id: &str,
        now_ms: u64,
    ) -> Result<Option<MemoryItem>, MemoryError> {
        let first = self.with(|store| {
            let item = store.get(id)?.ok_or(MemoryError::UnknownItem)?;
            if item.state != MemoryState::Active {
                return Err(MemoryError::WrongState);
            }
            store.first_use(id, task_id, "vote")
        })?;
        if !first {
            return Ok(None);
        }
        self.feedback(id, helpful, now_ms).map(Some)
    }

    /// B5-3a: 有用 / 不对 from the user or an agent (Q-M19, Q-M20).
    pub fn feedback(
        &self,
        id: &str,
        helpful: bool,
        now_ms: u64,
    ) -> Result<MemoryItem, MemoryError> {
        let (before, after) = self.with(|store| {
            let item = store.get(id)?.ok_or(MemoryError::UnknownItem)?;
            if item.state != MemoryState::Active {
                return Err(MemoryError::WrongState);
            }
            let after = store.feedback(&item, helpful, now_ms)?;
            Ok((item.confidence, after))
        })?;
        // B5-3b: reaching 高 proposes the experience as a skill (once).
        let high = Some(aeroric_protocol::memory::Confidence::High);
        if after.confidence == high && before != high {
            self.propose_skill(&after, now_ms)?;
        }
        Ok(after)
    }

    /// H4 (2026-11-21): active items of the project and global that match
    /// `query`, with their full-text rank (`bm25 × 1000`, lower is closer).
    /// Unlike [`Memory::recall`] it counts nothing: a look the agent did
    /// not ask for is not a retrieval.
    pub fn matches(
        &self,
        project_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(i64, MemoryItem)>, MemoryError> {
        self.with(|store| store.search(project_id, query, limit))
    }

    /// `memory_recall`: active items of the project and global that match,
    /// the project's first, pinned and user-stated ones ahead.
    pub fn recall(&self, project_id: &str, query: &str) -> Result<Vec<MemoryItem>, MemoryError> {
        self.recall_with(project_id, query, None)
    }

    /// B6 可选 embedding: with the query's unit vector, the nearest items
    /// join the FTS hits and the two lists are fused (RRF); the fused score
    /// takes the rank's place below.
    pub fn recall_with(
        &self,
        project_id: &str,
        query: &str,
        vector: Option<&[f32]>,
    ) -> Result<Vec<MemoryItem>, MemoryError> {
        self.recall_in_task(project_id, query, vector, None)
    }

    /// C3: [`Memory::recall_with`] for an agent's session — an item found
    /// again in the same task is not counted again.
    pub fn recall_in_task(
        &self,
        project_id: &str,
        query: &str,
        vector: Option<&[f32]>,
        task_id: Option<&str>,
    ) -> Result<Vec<MemoryItem>, MemoryError> {
        self.with(|store| {
            let mut found = store.search(project_id, query, 30)?;
            if let Some(vector) = vector {
                let near = semantic::nearest(store, project_id, vector)?;
                let words: Vec<String> = found.iter().map(|(_, i)| i.id.clone()).collect();
                let score = semantic::fuse(&[words, near.clone()]);
                for id in near {
                    if !found.iter().any(|(_, i)| i.id == id) {
                        if let Some(item) = store.get(&id)? {
                            found.push((0, item));
                        }
                    }
                }
                for (rank, item) in found.iter_mut() {
                    *rank = -((score.get(&item.id).copied().unwrap_or(0.0) * 1e6) as i64);
                }
            }
            // B5-3a: trusted and found useful first; more 不对 than 有用 last.
            found.sort_by_key(|(rank, item)| {
                (
                    item.project_id.is_none(),
                    !item.pinned,
                    item.assertion != Assertion::UserExplicit,
                    item.stats.wrong > item.stats.helpful,
                    std::cmp::Reverse(item.confidence),
                    std::cmp::Reverse(item.stats.helpful),
                    *rank,
                )
            });
            let items: Vec<MemoryItem> = found
                .into_iter()
                .map(|(_, item)| item)
                .take(RECALL_LIMIT)
                .collect();
            let ids: Vec<String> = items.iter().map(|i| i.id.clone()).collect();
            store.bump(&ids, "retrieved", task_id)?;
            Ok(items)
        })
    }
}

/// C2: the experiences among `items`, `(id, project)` — the Wiki compiles
/// paragraphs from them.
fn experiences(items: &[MemoryItem]) -> Vec<(String, Option<String>)> {
    items
        .iter()
        .filter(|i| i.kind == aeroric_protocol::memory::MemoryKind::Experience)
        .map(|i| (i.id.clone(), i.project_id.clone()))
        .collect()
}

const WIKI_BACKLOG: &str = "wiki_backlog";

/// C2: one project's Wiki still holding paragraphs of withdrawn memories.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiBacklog {
    pub project_id: String,
    pub ids: Vec<String>,
    /// Forgotten (scrub the history too), not just archived.
    pub forgotten: bool,
    /// Why the last try failed.
    pub reason: String,
    pub at_ms: u64,
}
