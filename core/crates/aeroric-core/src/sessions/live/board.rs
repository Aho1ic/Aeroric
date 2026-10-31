//! B4-3 (2026-10-31): the shared boards — one per manager session, shared
//! with its children (frontier-roadmap §5.4). Writes are per entry and every
//! one is kept in the board's change list with who made it; an agent's
//! decision is only ever proposed, and only the user or the board's owner
//! accepts it. Each reader gets only what changed since it last read.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use parking_lot::Mutex;

use aeroric_protocol::board::{
    Board, BoardAttention, BoardChange, BoardEntry, BoardKind, BoardState, BoardWrite, MAX_CHANGES,
    MAX_TEXT_CHARS, STALE_CLAIM_MS,
};

use super::now_ms;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BoardError {
    #[error("黑板上没有这一条")]
    UnknownEntry,
    #[error("新条目要写明类型")]
    NoKind,
    #[error("内容不能为空")]
    Empty,
    #[error("{kind:?} 类条目不能是 {state:?} 状态")]
    BadState { kind: BoardKind, state: BoardState },
    #[error("只有用户或主会话能采纳、否决决定")]
    NotAllowed,
    #[error("只有决定可以采纳或否决")]
    NotADecision,
}

#[derive(Default)]
struct State {
    loaded: HashSet<String>,
    boards: HashMap<String, Board>,
    /// (owner, reader) → the board revision the reader has seen.
    cursors: HashMap<(String, String), u64>,
}

#[derive(Default)]
pub struct Boards {
    dir: Mutex<Option<PathBuf>>,
    state: Mutex<State>,
}

fn cut(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= MAX_TEXT_CHARS {
        return text.to_string();
    }
    let mut out: String = text.chars().take(MAX_TEXT_CHARS).collect();
    out.push('…');
    out
}

impl Boards {
    /// `<data>/boards`; unset keeps boards in memory.
    pub fn set_dir(&self, dir: PathBuf) {
        *self.dir.lock() = Some(dir);
    }

    fn ensure_loaded(&self, state: &mut State, owner: &str) {
        if !state.loaded.insert(owner.to_string()) {
            return;
        }
        let Some(dir) = self.dir.lock().clone() else {
            return;
        };
        if let Ok(Some(bytes)) = super::checkpoint::store::read_task_file(&dir, owner) {
            if let Ok(board) = serde_json::from_slice::<Board>(&bytes) {
                state.boards.insert(owner.to_string(), board);
            }
        }
    }

    fn save(&self, board: &Board) {
        let Some(dir) = self.dir.lock().clone() else {
            return;
        };
        match serde_json::to_vec_pretty(board) {
            Ok(bytes) => {
                if let Err(error) =
                    super::checkpoint::store::write_task_file(&dir, &board.owner_task_id, &bytes)
                {
                    eprintln!("board: saving {} failed: {error}", board.owner_task_id);
                }
            }
            Err(error) => eprintln!("board: {error}"),
        }
    }

    /// The owner's board (empty when it has none yet).
    pub fn get(&self, owner: &str) -> Board {
        let mut state = self.state.lock();
        self.ensure_loaded(&mut state, owner);
        state.boards.get(owner).cloned().unwrap_or_else(|| Board {
            owner_task_id: owner.to_string(),
            ..Board::default()
        })
    }

    /// Add or change an entry. `agent`: an agent wrote it, so a decision
    /// stays proposed.
    pub fn write(
        &self,
        owner: &str,
        actor: &str,
        agent: bool,
        write: BoardWrite,
    ) -> Result<BoardEntry, BoardError> {
        let now = now_ms();
        let mut state = self.state.lock();
        self.ensure_loaded(&mut state, owner);
        let board = state
            .boards
            .entry(owner.to_string())
            .or_insert_with(|| Board {
                owner_task_id: owner.to_string(),
                ..Board::default()
            });
        let text = write.text.as_deref().map(cut);
        let (entry, action) = match &write.id {
            Some(id) => {
                let entry = board
                    .entries
                    .iter()
                    .find(|e| &e.id == id)
                    .cloned()
                    .ok_or(BoardError::UnknownEntry)?;
                let mut next = entry.clone();
                if let Some(text) = &text {
                    if text.is_empty() {
                        return Err(BoardError::Empty);
                    }
                    next.text = text.clone();
                }
                if let Some(state) = write.state {
                    if !next.kind.states().contains(&state) {
                        return Err(BoardError::BadState {
                            kind: next.kind,
                            state,
                        });
                    }
                    if next.kind == BoardKind::Decision && state != entry.state {
                        // Accepting or rejecting goes through `decide`.
                        return Err(BoardError::NotAllowed);
                    }
                    next.state = state;
                }
                if let Some(member) = &write.claimed_by {
                    next.claimed_by = Some(member.clone()).filter(|m| !m.trim().is_empty());
                }
                (next, "edit")
            }
            None => {
                let kind = write.kind.ok_or(BoardError::NoKind)?;
                let text = text.filter(|t| !t.is_empty()).ok_or(BoardError::Empty)?;
                let mut state = write.state.unwrap_or(kind.states()[0]);
                if !kind.states().contains(&state) {
                    return Err(BoardError::BadState { kind, state });
                }
                if kind == BoardKind::Decision && agent {
                    // A member's suggestion is not a team decision (§5.4).
                    state = BoardState::Proposed;
                }
                (
                    BoardEntry {
                        id: format!("b{}", board.entries.len() + 1),
                        kind,
                        text,
                        state,
                        author: actor.to_string(),
                        claimed_by: write.claimed_by.clone().filter(|m| !m.trim().is_empty()),
                        revision: 0,
                        created_ms: now,
                        updated_ms: now,
                    },
                    "add",
                )
            }
        };
        board.revision += 1;
        let mut entry = entry;
        entry.revision = board.revision;
        entry.updated_ms = now;
        match board.entries.iter_mut().find(|e| e.id == entry.id) {
            Some(slot) => *slot = entry.clone(),
            None => board.entries.push(entry.clone()),
        }
        board.changes.push(BoardChange {
            at_ms: now,
            actor: actor.to_string(),
            entry_id: entry.id.clone(),
            action: action.into(),
            text: entry.text.chars().take(120).collect(),
        });
        if board.changes.len() > MAX_CHANGES {
            let extra = board.changes.len() - MAX_CHANGES;
            board.changes.drain(..extra);
        }
        let saved = board.clone();
        drop(state);
        self.save(&saved);
        Ok(entry)
    }

    /// Accept or reject a proposed decision. The caller has checked that
    /// `actor` is the user or the board's owner.
    pub fn decide(
        &self,
        owner: &str,
        actor: &str,
        entry_id: &str,
        accept: bool,
    ) -> Result<BoardEntry, BoardError> {
        let now = now_ms();
        let mut state = self.state.lock();
        self.ensure_loaded(&mut state, owner);
        let board = state
            .boards
            .get_mut(owner)
            .ok_or(BoardError::UnknownEntry)?;
        board.revision += 1;
        let revision = board.revision;
        let entry = board
            .entries
            .iter_mut()
            .find(|e| e.id == entry_id)
            .ok_or(BoardError::UnknownEntry)?;
        if entry.kind != BoardKind::Decision {
            board.revision -= 1;
            return Err(BoardError::NotADecision);
        }
        entry.state = if accept {
            BoardState::Accepted
        } else {
            BoardState::Rejected
        };
        entry.revision = revision;
        entry.updated_ms = now;
        let entry = entry.clone();
        board.changes.push(BoardChange {
            at_ms: now,
            actor: actor.to_string(),
            entry_id: entry.id.clone(),
            action: if accept { "accept" } else { "reject" }.into(),
            text: String::new(),
        });
        let saved = board.clone();
        drop(state);
        self.save(&saved);
        Ok(entry)
    }

    /// What changed on the owner's board since `reader` last read it, and
    /// the revision it has now seen (Q-B20: only the diff is injected).
    pub fn read_since(&self, owner: &str, reader: &str) -> (Vec<BoardEntry>, u64) {
        let mut state = self.state.lock();
        self.ensure_loaded(&mut state, owner);
        let key = (owner.to_string(), reader.to_string());
        let seen = state.cursors.get(&key).copied().unwrap_or(0);
        let (entries, revision) = match state.boards.get(owner) {
            Some(board) => (
                board
                    .entries
                    .iter()
                    .filter(|e| e.revision > seen)
                    .cloned()
                    .collect(),
                board.revision,
            ),
            None => (Vec::new(), 0),
        };
        state.cursors.insert(key, revision);
        (entries, revision)
    }

    /// Proposed decisions and stale claims on every board (Q-B22).
    pub fn attention(&self) -> Vec<BoardAttention> {
        let mut state = self.state.lock();
        // CU 2026-10-31: take the folder first. In `if let Some(dir) =
        // self.dir.lock().clone()` the guard lives through the body, and
        // `ensure_loaded` locks `dir` again — the thread deadlocked itself,
        // then every attention poll queued behind it until the connection's
        // 64 request slots were gone.
        let dir = self.dir.lock().clone();
        if let Some(dir) = dir {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if let Some(owner) = name.strip_suffix(".json") {
                        if !owner.starts_with('.') {
                            self.ensure_loaded(&mut state, owner);
                        }
                    }
                }
            }
        }
        let now = now_ms();
        let mut out = Vec::new();
        for board in state.boards.values() {
            for entry in &board.entries {
                let reason = match (entry.kind, entry.state) {
                    (BoardKind::Decision, BoardState::Proposed) => "decision",
                    (BoardKind::Claim, BoardState::Open)
                        if now.saturating_sub(entry.updated_ms) > STALE_CLAIM_MS =>
                    {
                        "staleClaim"
                    }
                    _ => continue,
                };
                out.push(BoardAttention {
                    owner_task_id: board.owner_task_id.clone(),
                    entry: entry.clone(),
                    reason: reason.into(),
                });
            }
        }
        out.sort_by_key(|a| a.entry.updated_ms);
        out
    }

    #[cfg(test)]
    fn age(&self, owner: &str, entry_id: &str, by_ms: u64) {
        let mut state = self.state.lock();
        if let Some(e) = state
            .boards
            .get_mut(owner)
            .and_then(|b| b.entries.iter_mut().find(|e| e.id == entry_id))
        {
            e.updated_ms = e.updated_ms.saturating_sub(by_ms);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(kind: BoardKind, text: &str) -> BoardWrite {
        BoardWrite {
            kind: Some(kind),
            text: Some(text.into()),
            ..BoardWrite::default()
        }
    }

    #[test]
    fn an_agents_decision_is_only_ever_proposed() {
        let boards = Boards::default();
        let proposed = boards
            .write(
                "p",
                "child",
                true,
                BoardWrite {
                    state: Some(BoardState::Accepted),
                    ..add(BoardKind::Decision, "用 connect_timeout")
                },
            )
            .unwrap();
        assert_eq!(proposed.state, BoardState::Proposed);
        assert_eq!(
            boards
                .write(
                    "p",
                    "child",
                    true,
                    BoardWrite {
                        id: Some(proposed.id.clone()),
                        state: Some(BoardState::Accepted),
                        ..BoardWrite::default()
                    }
                )
                .unwrap_err(),
            BoardError::NotAllowed
        );
        assert_eq!(boards.attention()[0].reason, "decision");
        let accepted = boards.decide("p", "user", &proposed.id, true).unwrap();
        assert_eq!(accepted.state, BoardState::Accepted);
        assert!(boards.attention().is_empty());
        let changes: Vec<String> = boards
            .get("p")
            .changes
            .iter()
            .map(|c| c.action.clone())
            .collect();
        assert_eq!(changes, ["add", "accept"]);
    }

    #[test]
    fn states_must_fit_the_kind_and_text_is_required() {
        let boards = Boards::default();
        assert!(matches!(
            boards
                .write(
                    "p",
                    "user",
                    false,
                    BoardWrite {
                        state: Some(BoardState::Accepted),
                        ..add(BoardKind::Hypothesis, "x")
                    }
                )
                .unwrap_err(),
            BoardError::BadState { .. }
        ));
        assert_eq!(
            boards
                .write("p", "user", false, add(BoardKind::Goal, "  "))
                .unwrap_err(),
            BoardError::Empty
        );
        assert_eq!(
            boards
                .write("p", "user", false, BoardWrite::default())
                .unwrap_err(),
            BoardError::NoKind
        );
        let goal = boards
            .write("p", "user", false, add(BoardKind::Goal, "修超时"))
            .unwrap();
        assert_eq!(
            boards.decide("p", "user", &goal.id, true).unwrap_err(),
            BoardError::NotADecision
        );
        let hypothesis = boards
            .write("p", "user", false, add(BoardKind::Hypothesis, "是 DNS 慢"))
            .unwrap();
        let refuted = boards
            .write(
                "p",
                "child",
                true,
                BoardWrite {
                    id: Some(hypothesis.id),
                    state: Some(BoardState::Refuted),
                    ..BoardWrite::default()
                },
            )
            .unwrap();
        assert_eq!(refuted.state, BoardState::Refuted);
    }

    #[test]
    fn each_reader_gets_only_what_changed_since_its_last_read() {
        let boards = Boards::default();
        boards
            .write("p", "user", false, add(BoardKind::Goal, "修超时"))
            .unwrap();
        let (first, _) = boards.read_since("p", "child-1");
        assert_eq!(first.len(), 1);
        assert!(boards.read_since("p", "child-1").0.is_empty());
        boards
            .write(
                "p",
                "child-2",
                true,
                add(BoardKind::Question, "要兼容 HTTP/1 吗"),
            )
            .unwrap();
        let (next, revision) = boards.read_since("p", "child-1");
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].kind, BoardKind::Question);
        assert_eq!(revision, 2);
        // Another reader starts from the beginning.
        assert_eq!(boards.read_since("p", "child-2").0.len(), 2);
    }

    #[test]
    fn a_claim_untouched_for_half_an_hour_needs_the_user_and_boards_persist() {
        let dir =
            std::env::temp_dir().join(format!("aeroric-boards-{}", uuid::Uuid::new_v4().simple()));
        let boards = Boards::default();
        boards.set_dir(dir.clone());
        let claim = boards
            .write(
                "p",
                "user",
                false,
                BoardWrite {
                    claimed_by: Some("task-4".into()),
                    ..add(BoardKind::Claim, "查 hyper")
                },
            )
            .unwrap();
        assert!(boards.attention().is_empty());
        boards.age("p", &claim.id, STALE_CLAIM_MS + 1);
        let stale = boards.attention();
        assert_eq!(stale[0].reason, "staleClaim");
        assert_eq!(stale[0].entry.claimed_by.as_deref(), Some("task-4"));
        let again = Boards::default();
        again.set_dir(dir.clone());
        assert_eq!(again.get("p").entries[0].text, "查 hyper");

        // A board on disk that is not loaded yet: attention loads it and
        // returns (it used to deadlock on the folder lock).
        let cold = std::sync::Arc::new(Boards::default());
        cold.set_dir(dir);
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::sync::Arc::clone(&cold);
        std::thread::spawn(move || {
            let _ = tx.send(worker.attention().len());
        });
        // The aged claim was only aged in memory; on disk it is fresh.
        assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(5)), Ok(0));
        assert_eq!(cold.get("p").entries.len(), 1, "attention loaded it");
    }
}
