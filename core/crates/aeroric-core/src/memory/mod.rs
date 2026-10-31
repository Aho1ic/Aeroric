//! B5-1 long-term memory (2026-10-31): the store behind the "记忆" page,
//! `/记住`, the profile a starting session is handed and the context
//! server's `memory_recall` / `memory_propose` (frontier-roadmap §6;
//! Q-M1–Q-M8). Aeroric is the only writer; an item is never edited in place;
//! forgetting leaves a signature without the text (`barrier`).
//!
//! Kept in `<data>/memory/memory.db` (SQLite with an FTS5 index over the
//! CJK-segmented text, `notebook::rag::cjk`).

mod barrier;
pub mod extract;
pub mod native;
mod profile;
pub mod run;
mod secrets;
mod store;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use aeroric_protocol::memory::{
    Assertion, Evidence, MemoryDraft, MemoryItem, MemoryState, MAX_EXCERPT_CHARS, MAX_TEXT_CHARS,
    RECALL_LIMIT,
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
            Self::WrongState => write!(f, "这条记忆现在的状态不能这样操作"),
            Self::Storage(why) => write!(f, "记忆存储出错：{why}"),
        }
    }
}

impl std::error::Error for MemoryError {}

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
    }
}

/// The comparable form of a text: no whitespace, half-width, lower case,
/// without closing punctuation.
pub(crate) fn normalize(text: &str) -> String {
    let mut out: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| match c as u32 {
            0xFF01..=0xFF5E => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            _ => c,
        })
        .flat_map(char::to_lowercase)
        .collect();
    while out.ends_with(['.', '。', '!', '！', ';', '；', ',', '，']) {
        out.pop();
    }
    out
}

#[derive(Default)]
pub struct Memory {
    store: Mutex<Option<Store>>,
}

impl Memory {
    /// Open (or create) the store under `dir`.
    pub fn set_dir(&self, dir: PathBuf) -> Result<(), MemoryError> {
        let store = Store::open(&dir)?;
        *self.store.lock() = Some(store);
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

    pub fn archive(
        &self,
        id: &str,
        archived: bool,
        now_ms: u64,
    ) -> Result<MemoryItem, MemoryError> {
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
            store.set_state(id, to, now_ms)
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
    pub fn forget(&self, id: &str, now_ms: u64) -> Result<usize, MemoryError> {
        self.with(|store| {
            let item = store.get(id)?.ok_or(MemoryError::UnknownItem)?;
            store.forget(&item, now_ms)
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
    /// counts as injected.
    pub fn profile_for_session(&self, project_id: Option<&str>) -> Option<String> {
        let (text, ids) = self.render_profile(project_id)?;
        let _ = self.with(|store| store.bump(&ids, "injected"));
        Some(text)
    }

    fn render_profile(&self, project_id: Option<&str>) -> Option<(String, Vec<String>)> {
        self.with(|store| {
            let global = store.active(None)?;
            let project = match project_id {
                Some(id) if store.enabled(id)? => store.active(Some(id))?,
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

    /// B5-3a: 有用 / 不对 from the user or an agent (Q-M19, Q-M20).
    pub fn feedback(
        &self,
        id: &str,
        helpful: bool,
        now_ms: u64,
    ) -> Result<MemoryItem, MemoryError> {
        self.with(|store| {
            let item = store.get(id)?.ok_or(MemoryError::UnknownItem)?;
            if item.state != MemoryState::Active {
                return Err(MemoryError::WrongState);
            }
            store.feedback(&item, helpful, now_ms)
        })
    }

    /// `memory_recall`: active items of the project and global that match,
    /// the project's first, pinned and user-stated ones ahead.
    pub fn recall(&self, project_id: &str, query: &str) -> Result<Vec<MemoryItem>, MemoryError> {
        self.with(|store| {
            let mut found = store.search(project_id, query, 30)?;
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
            store.bump(&ids, "retrieved")?;
            Ok(items)
        })
    }
}
