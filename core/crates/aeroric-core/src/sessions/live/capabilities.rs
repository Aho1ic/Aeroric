//! R1-3a (2026-11-03): the capability table — every family, place and
//! feature has an answer, so a new family will not compile until it is
//! filled in (Q-S11). Facts come from the drivers: Codex runs in an OS
//! sandbox, Claude and omp keep their own approvals; omp fixes its
//! permission at start; Claude takes a steer as its next turn.

use aeroric_protocol::live_capability::{CapabilityEntry, Feature, Place, SupportState};

use crate::tasks::ProtocolFamily;

fn yes(note: Option<&str>) -> (SupportState, Option<String>) {
    (SupportState::Yes, note.map(str::to_string))
}

fn no(why: &str) -> (SupportState, Option<String>) {
    (SupportState::No, Some(why.to_string()))
}

fn degraded(how: &str) -> (SupportState, Option<String>) {
    (SupportState::Degraded, Some(how.to_string()))
}

fn unknown(what: &str) -> (SupportState, Option<String>) {
    (SupportState::Unknown, Some(what.to_string()))
}

/// Who holds a permission tier to its word.
fn enforced_by(family: ProtocolFamily) -> &'static str {
    match family {
        ProtocolFamily::Codex => "沙盒强制",
        ProtocolFamily::Claude => "由 Claude 把关 · 无沙盒",
        ProtocolFamily::Omp => "由 omp 把关 · 无沙盒",
        ProtocolFamily::Dsh => "只靠提示，不强制",
    }
}

pub fn support(family: ProtocolFamily, place: Place, feature: Feature) -> CapabilityEntry {
    use Feature as F;
    use ProtocolFamily as P;
    let enforced = enforced_by(family);
    let (state, note) = match (family, feature) {
        (P::Dsh, _) => no("dsh 不走原生会话"),
        (_, F::PermAsk | F::PermAutoEdit | F::PermFullAccess) => yes(Some(enforced)),
        (P::Omp, F::PermChange) => degraded("omp 启动时固定权限：下次启动生效"),
        (P::Codex | P::Claude, F::PermChange) => yes(None),
        (P::Codex | P::Claude, F::PlanMode) => yes(None),
        (P::Omp, F::PlanMode) => unknown("omp 的只读规划还没核对"),
        (P::Codex | P::Claude | P::Omp, F::Effort) => yes(None),
        (P::Codex | P::Claude | P::Omp, F::Compact) => yes(None),
        (P::Codex, F::Review) => yes(None),
        (P::Claude, F::Review) => no("Claude 没有内置审查，可用“审查”让另一族来看"),
        (P::Omp, F::Review) => no("omp 没有内置审查，可用“审查”让另一族来看"),
        (_, F::Goal) => no("目标模式还没接通"),
        (P::Codex | P::Omp, F::Steer) => yes(None),
        (P::Claude, F::Steer) => degraded("Claude 会作为下一轮发出"),
        (P::Codex | P::Claude | P::Omp, F::DelegateChild | F::Reviewer) => match place {
            Place::Local => yes(None),
            Place::Remote => unknown("远程主机上还没核对"),
        },
        (P::Codex | P::Claude | P::Omp, F::Receipts) => yes(None),
    };
    CapabilityEntry {
        feature,
        state,
        note,
    }
}

/// One family's whole table at `place`.
pub fn table(family: ProtocolFamily, place: Place) -> Vec<CapabilityEntry> {
    Feature::ALL
        .iter()
        .map(|f| support(family, place, *f))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_family_and_place_answers_every_feature() {
        for family in ProtocolFamily::ALL {
            for place in [Place::Local, Place::Remote] {
                let table = table(*family, place);
                assert_eq!(table.len(), Feature::ALL.len());
                for entry in &table {
                    if entry.state != SupportState::Yes {
                        assert!(
                            entry.note.is_some(),
                            "{family:?} {place:?} {entry:?} says why"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_drivers_facts_hold() {
        let at = |f, feature| support(f, Place::Local, feature);
        assert_eq!(
            at(ProtocolFamily::Codex, Feature::Review).state,
            SupportState::Yes
        );
        assert_eq!(
            at(ProtocolFamily::Claude, Feature::Review).state,
            SupportState::No
        );
        assert_eq!(
            at(ProtocolFamily::Claude, Feature::Steer).state,
            SupportState::Degraded
        );
        assert_eq!(
            at(ProtocolFamily::Omp, Feature::PermChange).state,
            SupportState::Degraded
        );
        assert_eq!(
            at(ProtocolFamily::Codex, Feature::PermAsk).note.as_deref(),
            Some("沙盒强制")
        );
        assert_eq!(
            support(ProtocolFamily::Codex, Place::Remote, Feature::DelegateChild).state,
            SupportState::Unknown
        );
    }
}
