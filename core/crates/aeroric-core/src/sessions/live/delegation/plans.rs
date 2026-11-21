//! D5 second step (2026-11-21): work plans kept and advanced. A plan is
//! saved as a draft, confirmed by the user, then advanced whenever one of
//! its parent's delegation cards changes (and when it is read): the rules
//! (`plan::schedule`) say which units start, each becomes an ordinary
//! delegation, and its id and the inputs it got are written back. Nothing
//! else is kept: after a restart the plan and the cards give the same
//! answer again.
//!
//! Plans live at `<delegation dir>/plans/<parent>/<plan id>.json`.

use std::collections::HashMap;
use std::path::PathBuf;

use aeroric_protocol::checkpoint::Acceptance;
use aeroric_protocol::delegation::{Contract, Origin};
use aeroric_protocol::work_plan::{UnitStatus, WorkPlan};
use serde::{Deserialize, Serialize};

use super::plan::{self, Observed};
use super::Delegations;

/// A plan and where each unit stands.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    pub plan: WorkPlan,
    pub statuses: Vec<UnitStatus>,
}

fn safe(part: &str) -> String {
    part.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

impl Delegations {
    fn plans_dir(&self, parent: &str) -> Option<PathBuf> {
        Some(self.dir()?.join("plans").join(safe(parent)))
    }

    fn write_plan(&self, plan: &WorkPlan) -> Result<(), String> {
        let dir = self.plans_dir(&plan.parent_task_id).ok_or("没有数据目录")?;
        crate::settings::ensure_private_dir(&dir)?;
        let text = serde_json::to_string_pretty(plan).map_err(|e| e.to_string())?;
        crate::settings::atomic_write_private(&dir.join(format!("{}.json", safe(&plan.id))), &text)
    }

    fn read_plans(&self, parent: &str) -> Vec<WorkPlan> {
        let Some(dir) = self.plans_dir(parent) else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut out: Vec<WorkPlan> = entries
            .flatten()
            .filter_map(|e| serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok())
            .collect();
        out.sort_by(|a, b| a.created_ms.cmp(&b.created_ms).then(a.id.cmp(&b.id)));
        out
    }

    /// What each unit's delegation card says now, by unit id.
    fn observed(&self, plan: &WorkPlan) -> HashMap<String, Observed> {
        let cards = self.list(&plan.parent_task_id);
        plan.units
            .iter()
            .filter_map(|u| {
                let id = u.delegation_id.as_ref()?;
                let card = cards.iter().find(|c| &c.delegation_id == id)?;
                Some((
                    u.id.clone(),
                    Observed {
                        state: card.state,
                        acceptance: card.acceptance.clone(),
                        outputs: card.artifacts.clone(),
                    },
                ))
            })
            .collect()
    }

    /// Save a draft plan (the user's, or one an agent proposed). A confirmed
    /// plan is not changed this way.
    pub fn save_plan(&self, mut draft: WorkPlan, now_ms: u64) -> Result<PlanView, String> {
        if draft.id.trim().is_empty() {
            draft.id = format!("plan-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
            draft.created_ms = now_ms;
        }
        if self
            .read_plans(&draft.parent_task_id)
            .iter()
            .any(|p| p.id == draft.id && p.confirmed)
        {
            return Err("计划已经确认，不能再改".into());
        }
        draft.confirmed = false;
        for unit in draft.units.iter_mut() {
            unit.delegation_id = None;
            unit.started_with.clear();
            unit.start_error = None;
        }
        plan::validate(&draft)?;
        self.write_plan(&draft)?;
        let statuses = plan::schedule(&draft, &HashMap::new()).statuses;
        Ok(PlanView {
            plan: draft,
            statuses,
        })
    }

    /// The user confirms a plan: from now on its units start as the rules say.
    pub fn confirm_plan(&self, parent: &str, id: &str) -> Result<PlanView, String> {
        let mut found = self
            .read_plans(parent)
            .into_iter()
            .find(|p| p.id == id)
            .ok_or("没有这个计划")?;
        if !found.confirmed {
            found.confirmed = true;
            self.write_plan(&found)?;
        }
        self.advance_plans(parent);
        self.plans(parent)
            .into_iter()
            .find(|v| v.plan.id == id)
            .ok_or_else(|| "没有这个计划".into())
    }

    /// A parent's plans and where each unit stands (nothing is started here).
    pub fn plans(&self, parent: &str) -> Vec<PlanView> {
        self.read_plans(parent)
            .into_iter()
            .map(|p| {
                let statuses = plan::schedule(&p, &self.observed(&p)).statuses;
                PlanView { plan: p, statuses }
            })
            .collect()
    }

    /// Start what the rules say should start now, in every confirmed plan of
    /// `parent`. Starting a unit changes a card, which asks again: that ask
    /// is folded into one more round here instead of running inside.
    pub fn advance_plans(&self, parent: &str) {
        {
            let mut advancing = self.plan_rounds.lock();
            if let Some(again) = advancing.get_mut(parent) {
                *again = true;
                return;
            }
            advancing.insert(parent.to_string(), false);
        }
        loop {
            for mut p in self.read_plans(parent).into_iter().filter(|p| p.confirmed) {
                let observed = self.observed(&p);
                let next = plan::schedule(&p, &observed).start;
                for unit_id in next {
                    let Some(at) = p.units.iter().position(|u| u.id == unit_id) else {
                        continue;
                    };
                    let inputs = plan::inputs(&p, &observed, &p.units[at]);
                    let unit = &p.units[at];
                    let contract = Contract {
                        instruction: plan::brief(&p, unit, &inputs),
                        role: unit.role.clone(),
                        workspace: Some(unit.workspace),
                        acceptance: unit
                            .acceptance
                            .iter()
                            .map(|text| Acceptance {
                                text: text.clone(),
                                check: None,
                            })
                            .collect(),
                        ..Contract::default()
                    };
                    let actor = format!("plan:{}", p.id);
                    let started = self.start(parent, contract, Origin::User, &actor);
                    let unit = &mut p.units[at];
                    match started {
                        Ok(card) => {
                            unit.delegation_id = Some(card.delegation_id);
                            unit.started_with = inputs;
                        }
                        Err(error) => unit.start_error = Some(error.to_string()),
                    }
                    if let Err(error) = self.write_plan(&p) {
                        eprintln!("delegation: saving plan {} failed: {error}", p.id);
                    }
                }
            }
            let mut advancing = self.plan_rounds.lock();
            if advancing.get(parent).copied().unwrap_or(false) {
                advancing.insert(parent.to_string(), false);
                continue;
            }
            advancing.remove(parent);
            break;
        }
    }

    /// Whether `parent` has any plan (so a card change need not look).
    pub(super) fn has_plans(&self, parent: &str) -> bool {
        self.plans_dir(parent).is_some_and(|d| d.is_dir())
    }
}
