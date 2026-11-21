//! H8 second step (2026-11-21): 分三档上线. An automatic behaviour is at one
//! tier — 候选 (decide and record whether it would have fired, change
//! nothing), 只挂描述 (the agent is told it is there, by name or a one-line
//! pointer, and takes it if it wants), 默认生效 (it acts) — or, for those
//! that can be, off. Every move is kept; 退回 goes back to the tier before
//! the last move, or one tier down when there is none.
//!
//! Two behaviours have tiers now: per-message recall (H4, `recall`) and
//! each learned skill (B5-3b, `skill:<name>`). The tiers live in
//! `<data>/rollout/state.json`; how often a behaviour would have fired is
//! counted beside them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// Per-message recall (H4).
pub const RECALL: &str = "recall";
/// Moves kept per behaviour.
const HISTORY_KEEP: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tier {
    Off,
    /// Decide and record whether it would fire; change nothing.
    Candidate,
    /// The agent is told it is there and takes it if it wants.
    Described,
    /// It acts.
    Default,
}

impl Tier {
    pub fn word(self) -> &'static str {
        match self {
            Tier::Off => "关",
            Tier::Candidate => "候选",
            Tier::Described => "只挂描述",
            Tier::Default => "默认生效",
        }
    }

    fn below(self) -> Option<Tier> {
        match self {
            Tier::Off => None,
            Tier::Candidate => Some(Tier::Off),
            Tier::Described => Some(Tier::Candidate),
            Tier::Default => Some(Tier::Described),
        }
    }
}

/// The id of a learned skill's behaviour.
pub fn skill_id(name: &str) -> String {
    format!("skill:{name}")
}

/// Where a behaviour starts: recall as a candidate (H4); a learned skill as
/// the user accepted it, mounted (B5-3b).
pub fn default_tier(id: &str) -> Tier {
    if id.starts_with("skill:") {
        Tier::Described
    } else {
        Tier::Candidate
    }
}

/// The lowest tier a behaviour may be at: a learned skill is turned off by
/// 停用 on the skill, not here.
fn floor(id: &str) -> Tier {
    if id.starts_with("skill:") {
        Tier::Candidate
    } else {
        Tier::Off
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    /// The tier it was at before the move.
    pub tier: Tier,
    pub at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feature {
    pub id: String,
    pub tier: Tier,
    /// Earlier tiers, oldest first; 退回 takes the last.
    #[serde(default)]
    pub history: Vec<Step>,
    /// Times it was decided (a message looked at).
    #[serde(default)]
    pub checked: u64,
    /// Of those, times it would have fired.
    #[serde(default)]
    pub would: u64,
}

impl Feature {
    fn new(id: &str) -> Self {
        Feature {
            id: id.to_string(),
            tier: default_tier(id),
            history: Vec::new(),
            checked: 0,
            would: 0,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    #[serde(default)]
    features: BTreeMap<String, Feature>,
}

fn path(data_dir: &Path) -> PathBuf {
    data_dir.join("rollout").join("state.json")
}

fn read(data_dir: &Path) -> State {
    std::fs::read_to_string(path(data_dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// A behaviour's tier, read from `data_dir` (for code without a [`Rollout`]).
pub fn tier_in(data_dir: &Path, id: &str) -> Tier {
    read(data_dir)
        .features
        .get(id)
        .map_or_else(|| default_tier(id), |f| f.tier)
}

#[derive(Default)]
pub struct Rollout {
    data_dir: Mutex<Option<PathBuf>>,
}

impl Rollout {
    pub fn set_dir(&self, data_dir: PathBuf) {
        *self.data_dir.lock() = Some(data_dir);
    }

    pub fn data_dir(&self) -> Option<PathBuf> {
        self.data_dir.lock().clone()
    }

    pub fn tier(&self, id: &str) -> Tier {
        self.data_dir()
            .map_or_else(|| default_tier(id), |dir| tier_in(&dir, id))
    }

    /// One behaviour as kept (its default when never moved).
    pub fn feature(&self, id: &str) -> Feature {
        self.data_dir()
            .and_then(|dir| read(&dir).features.remove(id))
            .unwrap_or_else(|| Feature::new(id))
    }

    /// Every behaviour moved or counted so far.
    pub fn features(&self) -> Vec<Feature> {
        self.data_dir()
            .map(|dir| read(&dir).features.into_values().collect())
            .unwrap_or_default()
    }

    fn change<T>(
        &self,
        id: &str,
        apply: impl FnOnce(&mut Feature) -> Result<T, String>,
    ) -> Result<T, String> {
        let dir = self.data_dir().ok_or("没有数据目录")?;
        // One writer at a time: the lock is held across read and write.
        let guard = self.data_dir.lock();
        let mut state = read(&dir);
        let feature = state
            .features
            .entry(id.to_string())
            .or_insert_with(|| Feature::new(id));
        let out = apply(feature)?;
        let target = path(&dir);
        if let Some(parent) = target.parent() {
            crate::settings::ensure_private_dir(parent)?;
        }
        let text = serde_json::to_string_pretty(&state).map_err(|e| e.to_string())?;
        crate::settings::atomic_write_private(&target, &text)?;
        drop(guard);
        Ok(out)
    }

    /// Move a behaviour to `tier`; the tier it leaves is kept for 退回.
    pub fn set(&self, id: &str, tier: Tier, now_ms: u64) -> Result<Feature, String> {
        if tier < floor(id) {
            return Err(format!(
                "{id} 最低是“{}”；学到的技能要关掉，在技能上停用",
                floor(id).word()
            ));
        }
        self.change(id, |f| {
            if f.tier != tier {
                f.history.push(Step {
                    tier: f.tier,
                    at_ms: now_ms,
                });
                let skip = f.history.len().saturating_sub(HISTORY_KEEP);
                f.history.drain(..skip);
                f.tier = tier;
            }
            Ok(f.clone())
        })
    }

    /// 退回: the tier before the last move, or one tier down.
    pub fn revert(&self, id: &str) -> Result<Feature, String> {
        let lowest = floor(id);
        self.change(id, |f| {
            let back = match f.history.pop() {
                Some(step) => step.tier,
                None => f
                    .tier
                    .below()
                    .filter(|t| *t >= lowest)
                    .ok_or_else(|| format!("已经是最低一档（{}）", f.tier.word()))?,
            };
            f.tier = back.max(lowest);
            Ok(f.clone())
        })
    }

    /// One decision of a behaviour: whether it would have fired.
    pub fn note(&self, id: &str, would: bool) {
        let _ = self.change(id, |f| {
            f.checked += 1;
            f.would += u64::from(would);
            Ok(())
        });
    }
}

#[cfg(test)]
#[path = "rollout_tests.rs"]
pub(crate) mod tests;
