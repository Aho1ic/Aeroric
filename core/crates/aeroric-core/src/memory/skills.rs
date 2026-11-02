//! B5-3b (2026-10-31): a trusted experience becomes a skill proposal; the
//! user edits, accepts or rejects it; an accepted one is a versioned skill
//! every session of its project is offered (frontier-roadmap §6.5; A06;
//! Q-M22–Q-M26). No model writes it: the SKILL.md is a template filled from
//! the experience.
//!
//! Versions live in `<data>/builtin-skills/learned/<name>/v<n>/SKILL.md`,
//! written once each; `builtin_skills::mount` reads the index this module
//! rewrites after every change.

use std::path::{Path, PathBuf};

use aeroric_protocol::memory::{
    MemoryItem, SkillEdit, SkillProposal, SkillProposalState, SkillVersion,
};

use super::{Memory, MemoryError};
use crate::extensions::builtin_skills::{self, LearnedEntry, LearnedIndex};

/// The SKILL.md a proposal starts with.
pub fn template(name: &str, description: &str, item: &MemoryItem) -> String {
    let detail = item.experience.clone().unwrap_or_default();
    let mut out =
        format!(
        "---\nname: {name}\ndescription: {}\n---\n\n# {}\n\n## 何时使用\n\n{}\n\n## 做法\n\n{}\n",
        description.replace('\n', " "),
        item.text,
        if detail.problem.is_empty() { item.text.as_str() } else { detail.problem.as_str() },
        detail.solution
    );
    if !detail.envs.is_empty() {
        out.push_str("\n## 适用环境\n\n");
        for env in &detail.envs {
            out.push_str(&format!("- {env}\n"));
        }
    }
    out.push_str(&format!(
        "\n## 来源\n\nAeroric 从会话整理的经验 {}，被标“有用” {} 次。\n",
        item.id, item.stats.helpful
    ));
    out
}

fn description_of(item: &MemoryItem) -> String {
    let problem: String = item
        .experience
        .as_ref()
        .map(|d| d.problem.chars().take(60).collect())
        .unwrap_or_default();
    if problem.is_empty() {
        format!("{}。", item.text)
    } else {
        format!("{}。遇到「{problem}」时使用。", item.text)
    }
}

fn check_name(name: &str) -> Result<(), MemoryError> {
    crate::extensions::skills::validate_skill_name(name).map_err(MemoryError::Skill)?;
    if builtin_skills::catalog().iter().any(|s| s.id == name) {
        return Err(MemoryError::Skill(format!(
            "“{name}”是内置技能的名字，换一个"
        )));
    }
    Ok(())
}

fn version_dir(data_dir: &Path, name: &str, n: u32) -> PathBuf {
    builtin_skills::learned_dir(data_dir)
        .join(name)
        .join(format!("v{n}"))
}

fn write_version(data_dir: &Path, version: &SkillVersion) -> Result<(), MemoryError> {
    let dir = version_dir(data_dir, &version.name, version.n);
    std::fs::create_dir_all(&dir).map_err(|e| MemoryError::Storage(e.to_string()))?;
    std::fs::write(dir.join("SKILL.md"), &version.body)
        .map_err(|e| MemoryError::Storage(e.to_string()))
}

impl Memory {
    fn data_dir(&self) -> Result<PathBuf, MemoryError> {
        self.data_dir
            .lock()
            .clone()
            .ok_or_else(|| MemoryError::Unavailable("存储未打开".into()))
    }

    /// Called when an experience reaches 高: one proposal per experience,
    /// ever — a rejected one is not offered again.
    pub(super) fn propose_skill(
        &self,
        item: &MemoryItem,
        now_ms: u64,
    ) -> Result<Option<SkillProposal>, MemoryError> {
        self.with(|store| {
            if store.proposal_for(&item.id)?.is_some() {
                return Ok(None);
            }
            let hex: String = item
                .id
                .trim_start_matches("mem-")
                .chars()
                .filter(|c| c.is_ascii_hexdigit())
                .take(6)
                .collect();
            let name = format!("learned-{hex}");
            let description = description_of(item);
            let proposal = SkillProposal {
                id: format!("skp-{}", &uuid::Uuid::new_v4().simple().to_string()[..10]),
                experience_id: item.id.clone(),
                task_id: item.evidence.iter().find_map(|e| e.task_id.clone()),
                project_id: item.project_id.clone(),
                project_path: None,
                global: item.project_id.is_none(),
                body: template(&name, &description, item),
                name,
                description,
                state: SkillProposalState::Pending,
                versions: Vec::new(),
                active_version: None,
                created_ms: now_ms,
                updated_ms: now_ms,
            };
            store.put_proposal(&proposal)?;
            Ok(Some(proposal))
        })
    }

    pub fn proposals(&self) -> Result<Vec<SkillProposal>, MemoryError> {
        self.with(|store| store.proposals())
    }

    pub fn proposal(&self, id: &str) -> Result<SkillProposal, MemoryError> {
        self.with(|store| store.proposal(id))?
            .ok_or(MemoryError::UnknownItem)
    }

    fn save_proposal(&self, proposal: &SkillProposal) -> Result<(), MemoryError> {
        self.with(|store| store.put_proposal(proposal))?;
        self.write_index()
    }

    /// 编辑: a pending proposal changes in place; an accepted (or disabled)
    /// skill gets a new version, which becomes the active one.
    pub fn edit_proposal(
        &self,
        id: &str,
        edit: SkillEdit,
        now_ms: u64,
    ) -> Result<SkillProposal, MemoryError> {
        let mut p = self.proposal(id)?;
        if p.state == SkillProposalState::Rejected {
            return Err(MemoryError::WrongState);
        }
        if let Some(name) = edit.name.map(|n| n.trim().to_string()) {
            check_name(&name)?;
            p.name = name;
        }
        if let Some(description) = edit.description {
            p.description = description.trim().to_string();
        }
        if let Some(body) = edit.body {
            if let Some(what) = super::secrets::looks_secret(&body) {
                return Err(MemoryError::Secret(what));
            }
            p.body = body;
        }
        if let Some(global) = edit.global {
            p.global = global;
        }
        p.updated_ms = now_ms;
        if p.state != SkillProposalState::Pending {
            self.add_version(&mut p, now_ms)?;
        }
        self.save_proposal(&p)?;
        Ok(p)
    }

    fn add_version(&self, p: &mut SkillProposal, now_ms: u64) -> Result<(), MemoryError> {
        let version = SkillVersion {
            n: p.versions.last().map(|v| v.n + 1).unwrap_or(1),
            name: p.name.clone(),
            description: p.description.clone(),
            body: p.body.clone(),
            at_ms: now_ms,
        };
        write_version(&self.data_dir()?, &version)?;
        p.active_version = Some(version.n);
        p.versions.push(version);
        Ok(())
    }

    /// 采纳 (writes v1; `project_path` is where its project lives) or 拒绝.
    pub fn decide_proposal(
        &self,
        id: &str,
        accept: bool,
        project_path: Option<PathBuf>,
        now_ms: u64,
    ) -> Result<SkillProposal, MemoryError> {
        let mut p = self.proposal(id)?;
        if p.state != SkillProposalState::Pending {
            return Err(MemoryError::WrongState);
        }
        p.updated_ms = now_ms;
        if accept {
            check_name(&p.name)?;
            p.project_path = project_path.map(|path| path.to_string_lossy().into_owned());
            self.add_version(&mut p, now_ms)?;
            p.state = SkillProposalState::Accepted;
        } else {
            p.state = SkillProposalState::Rejected;
        }
        self.save_proposal(&p)?;
        Ok(p)
    }

    /// 回到第 N 版.
    pub fn use_version(&self, id: &str, n: u32, now_ms: u64) -> Result<SkillProposal, MemoryError> {
        let mut p = self.proposal(id)?;
        let Some(v) = p.versions.iter().find(|v| v.n == n).cloned() else {
            return Err(MemoryError::UnknownItem);
        };
        if !matches!(
            p.state,
            SkillProposalState::Accepted | SkillProposalState::Disabled
        ) {
            return Err(MemoryError::WrongState);
        }
        p.active_version = Some(n);
        p.name = v.name;
        p.description = v.description;
        p.body = v.body;
        p.updated_ms = now_ms;
        self.save_proposal(&p)?;
        Ok(p)
    }

    /// 停用 / 启用.
    pub fn disable_skill(
        &self,
        id: &str,
        disabled: bool,
        now_ms: u64,
    ) -> Result<SkillProposal, MemoryError> {
        let mut p = self.proposal(id)?;
        p.state = match (p.state, disabled) {
            (SkillProposalState::Accepted, true) => SkillProposalState::Disabled,
            (SkillProposalState::Disabled, false) => SkillProposalState::Accepted,
            _ => return Err(MemoryError::WrongState),
        };
        p.updated_ms = now_ms;
        self.save_proposal(&p)?;
        Ok(p)
    }

    /// The index `builtin_skills::mount` reads: accepted skills, active versions.
    fn write_index(&self) -> Result<(), MemoryError> {
        let data_dir = self.data_dir()?;
        let skills = self
            .proposals()?
            .into_iter()
            .filter(|p| p.state == SkillProposalState::Accepted)
            .filter_map(|p| {
                let v = p.versions.iter().find(|v| Some(v.n) == p.active_version)?;
                Some(LearnedEntry {
                    name: v.name.clone(),
                    dir: version_dir(&data_dir, &v.name, v.n),
                    version: v.n,
                    global: p.global,
                    project_path: p.project_path.as_ref().map(PathBuf::from),
                })
            })
            .collect();
        builtin_skills::write_learned_index(&data_dir, &LearnedIndex { skills })
            .map_err(MemoryError::Storage)
    }
}

#[cfg(test)]
mod tests {
    use aeroric_protocol::memory::{Confidence, ExperienceDetail, SkillEdit, SkillProposalState};

    use super::super::{evidence, Memory, MemoryError};
    use crate::extensions::builtin_skills::{learned_offers, Mount};

    fn rig() -> (Memory, std::path::PathBuf) {
        let data = std::env::temp_dir().join(format!("aeroric-skills-{}", uuid::Uuid::new_v4()));
        let memory = Memory::default();
        memory.set_dir(data.join("memory")).unwrap();
        (memory, data)
    }

    fn experience(memory: &Memory) -> String {
        let detail = ExperienceDetail {
            problem: "读 CSV 时首列名多出 \\ufeff".into(),
            solution: "open(path, encoding=\"utf-8-sig\")".into(),
            search_keys: vec!["csv".into()],
            envs: vec!["Python 3".into()],
        };
        memory
            .add_experience(
                "p1",
                "CSV 带 BOM：读取改用 utf-8-sig",
                &detail,
                Confidence::Low,
                evidence(Some("t"), None, None, "x", 1),
                1,
            )
            .unwrap()
            .id
    }

    #[test]
    fn three_helpful_votes_propose_a_skill_once() {
        let (memory, _data) = rig();
        let id = experience(&memory);
        memory.feedback(&id, true, 2).unwrap();
        memory.feedback(&id, true, 3).unwrap();
        assert!(
            memory.proposals().unwrap().is_empty(),
            "medium is not enough"
        );
        memory.feedback(&id, true, 4).unwrap();
        let proposals = memory.proposals().unwrap();
        assert_eq!(proposals.len(), 1);
        let p = &proposals[0];
        assert_eq!(p.state, SkillProposalState::Pending);
        assert!(
            p.name.starts_with("learned-") && p.name.len() == "learned-".len() + 6,
            "{}",
            p.name
        );
        assert!(p.description.contains("遇到「读 CSV"));
        assert!(p
            .body
            .contains("## 做法\n\nopen(path, encoding=\"utf-8-sig\")"));
        assert!(p.body.contains("- Python 3"));
        assert!(!p.global);
        // Down to medium and back to high: still the one proposal.
        memory.feedback(&id, false, 5).unwrap();
        memory.feedback(&id, true, 6).unwrap();
        assert_eq!(memory.proposals().unwrap().len(), 1);
    }

    #[test]
    fn accepting_versions_reverting_and_disabling_change_what_sessions_get() {
        let (memory, data) = rig();
        let id = experience(&memory);
        for at in 2..5 {
            memory.feedback(&id, true, at).unwrap();
        }
        let p = memory.proposals().unwrap().remove(0);
        assert!(
            matches!(
                memory.edit_proposal(
                    &p.id,
                    SkillEdit {
                        name: Some("brainstorming".into()),
                        ..SkillEdit::default()
                    },
                    5
                ),
                Err(MemoryError::Skill(_))
            ),
            "a built-in name is refused"
        );
        let edited = memory
            .edit_proposal(
                &p.id,
                SkillEdit {
                    description: Some("读 BOM CSV".into()),
                    ..SkillEdit::default()
                },
                6,
            )
            .unwrap();
        assert!(edited.versions.is_empty(), "pending edits make no version");
        let project = data.join("proj");
        std::fs::create_dir_all(&project).unwrap();
        let accepted = memory
            .decide_proposal(&p.id, true, Some(project.clone()), 7)
            .unwrap();
        assert_eq!(
            (accepted.state, accepted.active_version),
            (SkillProposalState::Accepted, Some(1))
        );
        let offers = learned_offers(&data, Some(&project));
        assert_eq!(offers.len(), 1);
        assert_eq!(offers[0].version, "learned-v1");
        assert!(learned_offers(&data, Some(&data.join("other"))).is_empty());

        let v2 = memory
            .edit_proposal(
                &p.id,
                SkillEdit {
                    body: Some("---\nname: x\n---\n# v2\n".into()),
                    global: Some(true),
                    ..SkillEdit::default()
                },
                8,
            )
            .unwrap();
        assert_eq!(v2.active_version, Some(2));
        let offers = learned_offers(&data, Some(&data.join("other")));
        assert_eq!(offers[0].version, "learned-v2", "global now");
        assert!(std::fs::read_to_string(offers[0].dir.join("SKILL.md"))
            .unwrap()
            .contains("# v2"));
        let back = memory.use_version(&p.id, 1, 9).unwrap();
        assert_eq!(back.active_version, Some(1));
        assert_eq!(
            learned_offers(&data, Some(&project))[0].version,
            "learned-v1"
        );
        memory.disable_skill(&p.id, true, 10).unwrap();
        assert!(learned_offers(&data, Some(&project)).is_empty());
        let home = data.join("home");
        crate::extensions::builtin_skills::mount(&data, "omp", Some(&project), Mount::Home(&home))
            .unwrap();
        assert!(!home.join("skills").join(&p.name).exists());
        memory.disable_skill(&p.id, false, 11).unwrap();
        assert_eq!(learned_offers(&data, Some(&project)).len(), 1);
        assert_eq!(
            memory.decide_proposal(&p.id, false, None, 12),
            Err(MemoryError::WrongState)
        );
    }

    #[test]
    fn a_rejected_proposal_is_not_offered_again() {
        let (memory, _data) = rig();
        let id = experience(&memory);
        for at in 2..5 {
            memory.feedback(&id, true, at).unwrap();
        }
        let p = memory.proposals().unwrap().remove(0);
        memory.decide_proposal(&p.id, false, None, 5).unwrap();
        memory.feedback(&id, false, 6).unwrap();
        memory.feedback(&id, true, 7).unwrap();
        let all = memory.proposals().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].state, SkillProposalState::Rejected);
    }
}
