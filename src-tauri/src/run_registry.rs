//! The `(task_id, generation)` namespace (ADR-020 + ADR-025).
//!
//! ADR-017 allows two runs of one task to be live at the same time, so every decision that
//! used to be answerable by a task id now needs a second word. This module is that word, and
//! only that word: it tracks which runs of a task exist, and answers the two questions ADR-025
//! splits apart —
//!
//! - a **resource** operation (write input, resize, attach a session) addresses the newest
//!   *registered* generation, because that is the one the user is looking at;
//! - an **intent** operation (cancel, manual completion) addresses *every* live generation,
//!   because "stop that task" is said about the task.
//!
//! Two rules it exists to enforce, both tested:
//!
//! 1. "newest" means **registered**, not "what the counter has reached". A task whose only run
//!    retired has no generation at all — pointing a resize at the next number would address a
//!    process that does not exist.
//! 2. It never allocates. Generations come from core's ledger and arrive through
//!    [`RunRegistry::register`]; a second allocator here would be the fourth run-number this
//!    repo has already produced once (ADR-020's background).
//!
//! Locking: a leaf. It takes no other lock while held, so it may be locked last from any of
//! the maps in `TaskManager` (see the order table at `lib.rs:105-110`).

use std::collections::{BTreeMap, BTreeSet};

use parking_lot::Mutex;

/// The key every `TaskManager` container is addressed by (ADR-020's decision 1).
///
/// `generation` is core's per-task number, arriving through `LaunchRequest`/`ResumeRequest`.
/// One value is reserved and it is not a number core minted:
/// [`RunKey::UNMINTED`] (`0`) means "this run was started by a launch path that has not gone
/// through core yet" — every shell, and every task run the old commands still start. It is a
/// constant, not an allocation, so this is not the fourth run counter ADR-020 exists to
/// prevent; and because core's numbers start above zero, an unminted run can never be
/// mistaken for the newest one once a real generation registers for the same task.
///
/// Two consequences worth stating plainly: two *unminted* runs of one task still share a slot
/// (that is today's overwrite behaviour, kept verbatim — it ends when a launch path starts
/// going through core, which is P3-01's job), and ADR-025's addressing rules are total over
/// the mixed set because `newest` reads registered generations, whatever their number.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct RunKey {
    id: String,
    generation: u64,
}

impl RunKey {
    /// The generation a run carries when no ledger minted it.
    pub(crate) const UNMINTED: u64 = 0;

    pub(crate) fn minted(task_id: &str, generation: u64) -> Self {
        debug_assert!(
            generation != Self::UNMINTED,
            "generation 0 belongs to unminted runs; core never mints it"
        );
        RunKey {
            id: task_id.to_string(),
            generation,
        }
    }

    pub(crate) fn unminted(id: &str) -> Self {
        RunKey {
            id: id.to_string(),
            generation: Self::UNMINTED,
        }
    }

    /// The task or shell this run belongs to — what the pre-ADR-020 lookups keyed on.
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// Whether core's ledger owns this run's identity.
    pub(crate) fn is_minted(&self) -> bool {
        self.generation != Self::UNMINTED
    }
}

#[derive(Default)]
pub(crate) struct RunRegistry {
    live: Mutex<BTreeMap<String, BTreeSet<u64>>>,
}

impl RunRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Records that `generation` of `task_id` is now a run this process knows about.
    /// Called when core's claim turns into a spawn, never to invent a number.
    pub(crate) fn register(&self, task_id: &str, generation: u64) {
        self.live
            .lock()
            .entry(task_id.to_string())
            .or_default()
            .insert(generation);
    }

    /// Retires one run. Returns whether anything was actually retired, so a duplicate exit
    /// report can be told apart from the first — an exit monitor that fires twice must not
    /// look like it stopped a second process.
    pub(crate) fn retire(&self, task_id: &str, generation: u64) -> bool {
        let mut live = self.live.lock();
        let Some(generations) = live.get_mut(task_id) else {
            return false;
        };
        let retired = generations.remove(&generation);
        if generations.is_empty() {
            live.remove(task_id);
        }
        retired
    }

    /// Drops every run of a task, returning what was live. Used by task deletion.
    pub(crate) fn retire_task(&self, task_id: &str) -> Vec<u64> {
        self.live
            .lock()
            .remove(task_id)
            .map(|set| set.into_iter().collect())
            .unwrap_or_default()
    }

    /// What a resource operation addresses (ADR-025 rule 1).
    pub(crate) fn newest(&self, task_id: &str) -> Option<u64> {
        self.live
            .lock()
            .get(task_id)
            .and_then(|set| set.iter().next_back().copied())
    }

    /// What an intent operation must reach (ADR-025 rule 2).
    pub(crate) fn live_generations(&self, task_id: &str) -> Vec<u64> {
        self.live
            .lock()
            .get(task_id)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    pub(crate) fn is_live(&self, task_id: &str, generation: u64) -> bool {
        self.live
            .lock()
            .get(task_id)
            .is_some_and(|set| set.contains(&generation))
    }

    /// Whether another run is still in flight for this task — the check behind ADR-017's
    /// "no terminal settlement while another generation is live".
    pub(crate) fn has_other_live(&self, task_id: &str, generation: u64) -> bool {
        self.live_generations(task_id)
            .into_iter()
            .any(|other| other != generation)
    }

    pub(crate) fn live_task_count(&self) -> usize {
        self.live
            .lock()
            .values()
            .filter(|set| !set.is_empty())
            .count()
    }

    /// ADR-025 rule 1, as a key. `None` means there is nothing to address — the distinction
    /// [`RunKey::UNMINTED`] exists to keep is that a registered unminted run is a real run
    /// (generation 0 is stored like any other), while "the counter reached 5" is not.
    pub(crate) fn resource_key(&self, task_id: &str) -> Option<RunKey> {
        self.newest(task_id).map(|generation| {
            if generation == RunKey::UNMINTED {
                RunKey::unminted(task_id)
            } else {
                RunKey::minted(task_id, generation)
            }
        })
    }

    /// ADR-025 rule 2, as keys: every live run an intent must reach, oldest first.
    pub(crate) fn intent_keys(&self, task_id: &str) -> Vec<RunKey> {
        self.live_generations(task_id)
            .into_iter()
            .map(|generation| {
                if generation == RunKey::UNMINTED {
                    RunKey::unminted(task_id)
                } else {
                    RunKey::minted(task_id, generation)
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{RunKey, RunRegistry};

    #[test]
    fn newest_means_registered_not_counted() {
        // The distinction ADR-025 calls out: after the only run retires, a task-id-only
        // resource request must find *nothing*, not "the next number".
        let registry = RunRegistry::new();
        registry.register("t1", 1);
        assert_eq!(registry.newest("t1"), Some(1));
        registry.register("t1", 3);
        assert_eq!(
            registry.newest("t1"),
            Some(3),
            "a gap in the numbers is fine; the highest registered run wins"
        );
        assert!(registry.retire("t1", 3));
        assert_eq!(
            registry.newest("t1"),
            Some(1),
            "and addressing falls back to the older run"
        );
        assert!(registry.retire("t1", 1));
        assert_eq!(
            registry.newest("t1"),
            None,
            "with nothing registered there is no generation to address"
        );
    }

    #[test]
    fn an_intent_sees_every_live_generation_and_a_retire_is_idempotent() {
        let registry = RunRegistry::new();
        registry.register("t1", 1);
        registry.register("t1", 2);
        registry.register("t1", 2);
        assert_eq!(
            registry.live_generations("t1"),
            vec![1, 2],
            "a duplicate registration is the same run, not two"
        );
        assert!(registry.has_other_live("t1", 1));
        assert!(registry.retire("t1", 1));
        assert!(registry.retire("t1", 2));
        assert!(
            !registry.retire("t1", 2),
            "a second exit report for the same run retires nothing"
        );
        assert_eq!(registry.live_generations("t1"), Vec::<u64>::new());
        assert!(!registry.has_other_live("t1", 2));
    }

    #[test]
    fn tasks_do_not_share_namespace_and_deletion_clears_the_whole_task() {
        let registry = RunRegistry::new();
        registry.register("t1", 1);
        registry.register("t2", 1);
        assert_eq!(registry.live_task_count(), 2);
        assert_eq!(registry.retire_task("t1"), vec![1]);
        assert_eq!(registry.newest("t1"), None);
        assert_eq!(registry.newest("t2"), Some(1), "its sibling is untouched");
        assert_eq!(registry.retire_task("t1"), Vec::<u64>::new());
        assert_eq!(registry.live_task_count(), 1);
    }

    #[test]
    fn an_unregistered_generation_is_not_live() {
        // Guards against "is_live" degrading into "the task has any run", which would let a
        // late event from a retired generation look legitimate.
        let registry = RunRegistry::new();
        registry.register("t1", 4);
        assert!(registry.is_live("t1", 4));
        assert!(!registry.is_live("t1", 3));
        assert!(!registry.is_live("t9", 4));
    }

    #[test]
    fn an_unminted_run_and_a_minted_one_are_distinguishable_and_ordered() {
        // The migration case: a task started before the handover (unminted) and one started
        // through core's ledger (minted) coexist for the same id. A resource operation must
        // address the minted one, and an intent must reach both.
        let registry = RunRegistry::new();
        registry.register("t1", RunKey::UNMINTED);
        registry.register("t1", 4);
        assert_eq!(
            registry.resource_key("t1"),
            Some(RunKey::minted("t1", 4)),
            "generation 0 never wins an addressing contest"
        );
        assert_eq!(
            registry.intent_keys("t1"),
            vec![RunKey::unminted("t1"), RunKey::minted("t1", 4)],
            "both are live processes, so 'stop that task' reaches both"
        );
        assert!(registry.retire("t1", 4));
        assert_eq!(registry.resource_key("t1"), Some(RunKey::unminted("t1")));
        assert!(!registry.resource_key("t1").unwrap().is_minted());
    }

    #[test]
    fn a_run_that_was_never_registered_is_not_addressable() {
        let registry = RunRegistry::new();
        assert_eq!(registry.resource_key("t1"), None);
        assert_eq!(registry.intent_keys("t1"), Vec::<RunKey>::new());
        registry.register("t1", RunKey::UNMINTED);
        assert_eq!(registry.resource_key("t1"), Some(RunKey::unminted("t1")));
        assert!(registry.retire("t1", RunKey::UNMINTED));
        assert_eq!(registry.resource_key("t1"), None);
    }

    #[test]
    fn a_key_names_its_run_and_carries_the_id_lookups_used_before() {
        let minted = RunKey::minted("t7", 9);
        let unminted = RunKey::unminted("shell:project:1:1700000000000");
        assert_eq!(minted.id(), "t7");
        assert_eq!(minted.generation(), 9);
        assert!(minted.is_minted());
        assert_eq!(
            unminted.id(),
            "shell:project:1:1700000000000",
            "a shell's id is already its run identity"
        );
        assert!(!unminted.is_minted());
        assert_ne!(minted, RunKey::minted("t7", 10), "one task, two runs");
    }

    #[test]
    fn concurrent_registration_and_retirement_do_not_lose_a_run() {
        // Two windows starting two runs at once is the exact situation ADR-017 permits.
        let registry = std::sync::Arc::new(RunRegistry::new());
        let threads: Vec<_> = (0..16u64)
            .map(|index| {
                let registry = std::sync::Arc::clone(&registry);
                std::thread::spawn(move || {
                    registry.register("t1", index);
                    registry.register("t1", index);
                    if index % 2 == 0 {
                        registry.retire("t1", index);
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().expect("registry thread");
        }
        let live = registry.live_generations("t1");
        let expected: Vec<u64> = (0..16).filter(|index| index % 2 == 1).collect();
        assert_eq!(
            live, expected,
            "every concurrently registered run is accounted for"
        );
    }
}
