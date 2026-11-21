//! D5 first step (2026-11-21): a work plan's rules, decided without starting
//! anything — what a plan may hold, what each unit's state is now and why,
//! and which units to start next. Each unit is an ordinary delegation; the
//! host (next step) starts the ones [`schedule`] names and feeds back what
//! their delegation cards say.
//!
//! - at most `MAX_UNITS` units, `MAX_RUNNING` running at once (constants);
//! - a plan with a cycle is refused, naming the units on it;
//! - a prerequisite is met when its delegation reported and every acceptance
//!   criterion passed with none stale (B3); one that failed keeps its
//!   dependents waiting, saying so — nothing is skipped;
//! - one unit writing in the parent's directory at a time (Q-B10): read-only
//!   units and those in an isolated copy may run beside it;
//! - a unit gets its prerequisites' outputs (B6 refs) in its brief, not their
//!   conversations; when those outputs change after it started, its input is
//!   out of date and the user decides whether to redo it.

use std::collections::{HashMap, HashSet};

use aeroric_protocol::artifacts::ArtifactRef;
use aeroric_protocol::checkpoint::ObligationState;
use aeroric_protocol::delegation::{AcceptanceResult, DelegationState, Workspace};
use aeroric_protocol::work_plan::{
    UnitState, UnitStatus, WorkPlan, WorkUnit, MAX_RUNNING, MAX_UNITS,
};

/// What a started unit's delegation card says now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Observed {
    pub state: DelegationState,
    pub acceptance: Vec<AcceptanceResult>,
    pub outputs: Vec<ArtifactRef>,
}

/// A plan the rules allow: 1 to `MAX_UNITS` units, unique ids and titles
/// said, dependencies on units of the plan, no cycle.
pub fn validate(plan: &WorkPlan) -> Result<(), String> {
    if plan.units.is_empty() {
        return Err("计划里没有工作单元".into());
    }
    if plan.units.len() > MAX_UNITS {
        return Err(format!(
            "一个计划最多 {MAX_UNITS} 个工作单元，这个有 {} 个",
            plan.units.len()
        ));
    }
    let mut ids = HashSet::new();
    for unit in &plan.units {
        if unit.id.trim().is_empty() || unit.title.trim().is_empty() {
            return Err("每个工作单元都要有编号和标题".into());
        }
        if !ids.insert(unit.id.as_str()) {
            return Err(format!("工作单元编号重复：{}", unit.id));
        }
    }
    for unit in &plan.units {
        for dep in &unit.depends_on {
            if dep == &unit.id {
                return Err(format!("「{}」不能依赖它自己", unit.title));
            }
            if !ids.contains(dep.as_str()) {
                return Err(format!("「{}」依赖的 {dep} 不在这个计划里", unit.title));
            }
        }
    }
    if let Some(cycle) = find_cycle(plan) {
        let names: Vec<String> = cycle
            .iter()
            .map(|id| format!("「{}」", title(plan, id)))
            .collect();
        return Err(format!("计划里有环：{}", names.join(" → ")));
    }
    Ok(())
}

fn title<'a>(plan: &'a WorkPlan, id: &'a str) -> &'a str {
    plan.units
        .iter()
        .find(|u| u.id == id)
        .map_or(id, |u| u.title.as_str())
}

/// A cycle's units in order, back to the first, if there is one.
fn find_cycle(plan: &WorkPlan) -> Option<Vec<String>> {
    let deps: HashMap<&str, Vec<&str>> = plan
        .units
        .iter()
        .map(|u| {
            (
                u.id.as_str(),
                u.depends_on.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    // 0 unseen, 1 on the path, 2 done.
    let mut mark: HashMap<&str, u8> = HashMap::new();
    fn visit<'a>(
        id: &'a str,
        deps: &HashMap<&'a str, Vec<&'a str>>,
        mark: &mut HashMap<&'a str, u8>,
        path: &mut Vec<&'a str>,
    ) -> Option<Vec<String>> {
        mark.insert(id, 1);
        path.push(id);
        for next in deps.get(id).into_iter().flatten() {
            match mark.get(next).copied().unwrap_or(0) {
                1 => {
                    let from = path.iter().position(|p| p == next).unwrap_or(0);
                    let mut cycle: Vec<String> =
                        path[from..].iter().map(|s| s.to_string()).collect();
                    cycle.push(next.to_string());
                    return Some(cycle);
                }
                0 => {
                    if let Some(found) = visit(next, deps, mark, path) {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        path.pop();
        mark.insert(id, 2);
        None
    }
    for unit in &plan.units {
        if mark.get(unit.id.as_str()).copied().unwrap_or(0) == 0 {
            if let Some(cycle) = visit(&unit.id, &deps, &mut mark, &mut Vec::new()) {
                return Some(cycle);
            }
        }
    }
    None
}

/// A started unit's state, from its delegation card.
pub fn unit_state(observed: &Observed) -> UnitState {
    match observed.state {
        DelegationState::Starting | DelegationState::Running | DelegationState::Checking => {
            UnitState::Running
        }
        DelegationState::Reported => {
            let failed = observed
                .acceptance
                .iter()
                .any(|a| a.state == ObligationState::Failed);
            let all_passed = observed
                .acceptance
                .iter()
                .all(|a| a.state == ObligationState::Passed && !a.stale);
            if failed {
                UnitState::Failed
            } else if all_passed {
                UnitState::Passed
            } else {
                UnitState::AwaitingAcceptance
            }
        }
        DelegationState::Failed | DelegationState::OverBudget | DelegationState::Interrupted => {
            UnitState::Failed
        }
        DelegationState::Stopped => UnitState::Cancelled,
    }
}

/// The plan now: each unit's state and reason, and which to start.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Schedule {
    pub statuses: Vec<UnitStatus>,
    pub start: Vec<String>,
}

impl Schedule {
    pub fn of(&self, id: &str) -> Option<&UnitStatus> {
        self.statuses.iter().find(|s| s.id == id)
    }
}

fn writes_in_place(unit: &WorkUnit) -> bool {
    unit.workspace == Workspace::Shared
}

/// The prerequisites' outputs as they stand now.
pub fn inputs(
    plan: &WorkPlan,
    observed: &HashMap<String, Observed>,
    unit: &WorkUnit,
) -> Vec<ArtifactRef> {
    let mut out = Vec::new();
    for dep in &unit.depends_on {
        if plan.units.iter().any(|u| &u.id == dep) {
            if let Some(o) = observed.get(dep) {
                out.extend(o.outputs.iter().cloned());
            }
        }
    }
    out
}

/// Every unit's state and why, and the units to start now (in plan order,
/// within the limits).
pub fn schedule(plan: &WorkPlan, observed: &HashMap<String, Observed>) -> Schedule {
    let mut statuses = Vec::new();
    let mut state_of: HashMap<&str, UnitState> = HashMap::new();
    // Started units first: their state is their delegation's. One that
    // could not be started counts as failed.
    for unit in &plan.units {
        if unit.delegation_id.is_some() {
            let state = observed
                .get(&unit.id)
                .map_or(UnitState::Running, unit_state);
            state_of.insert(unit.id.as_str(), state);
        } else if unit.start_error.is_some() {
            state_of.insert(unit.id.as_str(), UnitState::Failed);
        }
    }
    let mut running = state_of
        .values()
        .filter(|s| **s == UnitState::Running)
        .count();
    let mut writer: Option<&WorkUnit> = plan
        .units
        .iter()
        .find(|u| writes_in_place(u) && state_of.get(u.id.as_str()) == Some(&UnitState::Running));
    let mut start = Vec::new();
    for unit in &plan.units {
        if unit.delegation_id.is_some() {
            let state = state_of[unit.id.as_str()];
            let reason = match state {
                UnitState::Running => "在运行".to_string(),
                UnitState::AwaitingAcceptance => "已回报，验收还没全部通过".into(),
                UnitState::Passed => "验收全部通过".into(),
                UnitState::Failed => "失败了，等你处理".into(),
                UnitState::Cancelled => "已停止".into(),
                _ => String::new(),
            };
            let now = inputs(plan, observed, unit);
            let inputs_stale = !unit.started_with.is_empty()
                && unit.started_with.iter().any(|was| !now.contains(was));
            statuses.push(UnitStatus {
                id: unit.id.clone(),
                state,
                reason: if inputs_stale {
                    format!("{reason}；前置的产物在它开始后又变了，输入已过期")
                } else {
                    reason
                },
                inputs_stale,
            });
            continue;
        }
        if let Some(error) = &unit.start_error {
            statuses.push(UnitStatus {
                id: unit.id.clone(),
                state: UnitState::Failed,
                reason: format!("开不起来：{error}"),
                inputs_stale: false,
            });
            continue;
        }
        // Not started: are its prerequisites met?
        let mut blocking: Option<(String, bool)> = None;
        for dep in &unit.depends_on {
            let state = state_of
                .get(dep.as_str())
                .copied()
                .unwrap_or(UnitState::NotStarted);
            if state != UnitState::Passed {
                let failed = matches!(state, UnitState::Failed | UnitState::Cancelled);
                if blocking.as_ref().is_none_or(|(_, f)| !f && failed) {
                    blocking = Some((title(plan, dep).to_string(), failed));
                }
            }
        }
        let (state, reason) = match blocking {
            Some((dep, true)) => (
                UnitState::WaitingDeps,
                format!("前置「{dep}」失败或停止了，等你处理"),
            ),
            Some((dep, false)) => (UnitState::WaitingDeps, format!("等「{dep}」通过验收")),
            None if !plan.confirmed => (UnitState::NotStarted, "计划还没确认".into()),
            None if running >= MAX_RUNNING => (
                UnitState::NotStarted,
                format!("同时最多运行 {MAX_RUNNING} 个，等前面的跑完"),
            ),
            None if writes_in_place(unit) && writer.is_some() => (
                UnitState::NotStarted,
                format!(
                    "等「{}」写完：同一个目录同时只能有一个单元写文件",
                    writer.map_or("", |w| w.title.as_str())
                ),
            ),
            None => {
                start.push(unit.id.clone());
                running += 1;
                if writes_in_place(unit) {
                    writer = Some(unit);
                }
                (UnitState::NotStarted, "现在开始".into())
            }
        };
        statuses.push(UnitStatus {
            id: unit.id.clone(),
            state,
            reason,
            inputs_stale: false,
        });
    }
    Schedule { statuses, start }
}

/// A unit's instruction: its brief, then its prerequisites' outputs to read
/// with `artifact_read` (not their conversations).
pub fn brief(plan: &WorkPlan, unit: &WorkUnit, inputs: &[ArtifactRef]) -> String {
    let mut out = unit.brief.trim().to_string();
    if !inputs.is_empty() {
        out.push_str("\n\n前置单元的产物（用 artifact_read 读）：\n");
        for input in inputs {
            out.push_str(&format!("- {}：{}\n", input.path, input.label()));
        }
    }
    if !unit.depends_on.is_empty() && inputs.is_empty() {
        let names: Vec<String> = unit
            .depends_on
            .iter()
            .map(|d| format!("「{}」", title(plan, d)))
            .collect();
        out.push_str(&format!("\n\n前置单元 {} 没有登记产物。", names.join("、")));
    }
    out
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
