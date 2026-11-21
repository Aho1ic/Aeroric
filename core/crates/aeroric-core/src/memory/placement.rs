//! H7 (2026-11-21): moving facts between the opening (handed to every new
//! session) and recall only, by how they are used — not on a fixed
//! schedule. A fact the opening carried many times that nobody found
//! useful or looked up leaves it; one that, out of it, is looked up again
//! or found useful comes back. The user's profile and pinned items stay.
//! Every move is recorded with its reason.

use aeroric_protocol::memory::{MemoryItem, MemoryKind, MemoryPlacement};

/// Carried this many times with no use at all: out.
pub const DEMOTE_AFTER_INJECTED: u32 = 8;
/// Found wrong more than useful, once carried twice: out.
const DEMOTE_WRONG_AFTER: u32 = 2;
/// Out, then looked up this many more times: back.
pub const PROMOTE_AFTER_RETRIEVED: u32 = 2;

/// Whether the opening may let `item` go: a fact the user did not pin.
fn movable(item: &MemoryItem) -> bool {
    item.kind == MemoryKind::Fact && !item.pinned
}

/// Why `item` should leave the opening, if it should.
pub fn demote_reason(item: &MemoryItem) -> Option<String> {
    let s = item.stats;
    if !movable(item) {
        return None;
    }
    if s.injected >= DEMOTE_WRONG_AFTER && s.wrong > s.helpful {
        return Some(format!(
            "被标“不对”{} 次，多于“有用”{} 次",
            s.wrong, s.helpful
        ));
    }
    if s.injected >= DEMOTE_AFTER_INJECTED && s.helpful == 0 && s.retrieved == 0 {
        return Some(format!(
            "开场白里带了 {} 次，没被标过有用，也没被检索过",
            s.injected
        ));
    }
    None
}

/// Why a demoted `item` should come back, given its counts when it left.
pub fn promote_reason(item: &MemoryItem, retrieved_then: u32, helpful_then: u32) -> Option<String> {
    let s = item.stats;
    if s.helpful > helpful_then {
        return Some("移出开场白后被标有用".into());
    }
    let again = s.retrieved.saturating_sub(retrieved_then);
    (again >= PROMOTE_AFTER_RETRIEVED).then(|| format!("移出开场白后又被检索 {again} 次"))
}

impl super::Memory {
    /// H7: move the scope's facts in or out of the opening by their use;
    /// returns the moves made.
    pub fn rebalance(
        &self,
        project_id: Option<&str>,
        now_ms: u64,
    ) -> Result<Vec<MemoryPlacement>, super::MemoryError> {
        self.with(|store| {
            let demoted = store.demoted()?;
            let mut moves = Vec::new();
            for item in store.active(project_id)? {
                let change = match demoted.get(&item.id) {
                    Some((retrieved, helpful)) => {
                        promote_reason(&item, *retrieved, *helpful).map(|why| (false, why))
                    }
                    None => demote_reason(&item).map(|why| (true, why)),
                };
                if let Some((demote, reason)) = change {
                    store.place(&item, demote, &reason, now_ms)?;
                    moves.push(MemoryPlacement {
                        item_id: item.id.clone(),
                        at_ms: now_ms,
                        demoted: demote,
                        reason,
                    });
                }
            }
            Ok(moves)
        })
    }

    /// H7: the latest moves, newest first.
    pub fn placements(&self, limit: usize) -> Result<Vec<MemoryPlacement>, super::MemoryError> {
        self.with(|store| store.placements(limit))
    }
}
