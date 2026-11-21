use std::path::{Path, PathBuf};

use super::{skill_id, tier_in, Rollout, Tier, RECALL};
use crate::extensions::builtin_skills::{
    learned_described, learned_offers, write_learned_index, LearnedEntry, LearnedIndex,
};

fn rig() -> (Rollout, PathBuf) {
    let data = std::env::temp_dir().join(format!("aeroric-rollout-{}", uuid::Uuid::new_v4()));
    let rollout = Rollout::default();
    rollout.set_dir(data.clone());
    (rollout, data)
}

/// Recall goes up one tier at a time and 退回 retraces the moves, then
/// steps down to off; below off there is nothing.
#[test]
fn recall_moves_through_the_tiers_and_back() {
    let (r, data) = rig();
    assert_eq!(r.tier(RECALL), Tier::Candidate, "where it starts");
    r.set(RECALL, Tier::Described, 10).unwrap();
    let up = r.set(RECALL, Tier::Default, 20).unwrap();
    assert_eq!(up.tier, Tier::Default);
    assert_eq!(
        up.history
            .iter()
            .map(|s| (s.tier, s.at_ms))
            .collect::<Vec<_>>(),
        [(Tier::Candidate, 10), (Tier::Described, 20)]
    );
    // The same tier again is not a move.
    assert_eq!(r.set(RECALL, Tier::Default, 30).unwrap().history.len(), 2);
    assert_eq!(tier_in(&data, RECALL), Tier::Default, "kept on disk");

    assert_eq!(r.revert(RECALL).unwrap().tier, Tier::Described);
    assert_eq!(r.revert(RECALL).unwrap().tier, Tier::Candidate);
    let off = r.revert(RECALL).unwrap();
    assert_eq!(
        (off.tier, off.history.len()),
        (Tier::Off, 0),
        "one tier down"
    );
    assert!(r.revert(RECALL).unwrap_err().contains("最低一档"));
    std::fs::remove_dir_all(&data).ok();
}

/// A jump back from 默认生效 to 候选 is one 退回 away from 默认生效.
#[test]
fn revert_undoes_a_jump() {
    let (r, data) = rig();
    r.set(RECALL, Tier::Default, 1).unwrap();
    r.set(RECALL, Tier::Off, 2).unwrap();
    assert_eq!(r.revert(RECALL).unwrap().tier, Tier::Default);
    std::fs::remove_dir_all(&data).ok();
}

/// A learned skill starts mounted (只挂描述), may go to 候选 or 默认生效,
/// never off here; 退回 stops at 候选.
#[test]
fn a_learned_skill_stays_between_candidate_and_default() {
    let (r, data) = rig();
    let id = skill_id("ssh-timeout");
    assert_eq!(r.tier(&id), Tier::Described);
    assert!(r.set(&id, Tier::Off, 1).unwrap_err().contains("停用"));
    assert_eq!(r.set(&id, Tier::Default, 2).unwrap().tier, Tier::Default);
    assert_eq!(r.revert(&id).unwrap().tier, Tier::Described);
    assert_eq!(r.revert(&id).unwrap().tier, Tier::Candidate);
    assert!(r.revert(&id).is_err());
    std::fs::remove_dir_all(&data).ok();
}

#[test]
fn decisions_are_counted() {
    let (r, data) = rig();
    r.note(RECALL, true);
    r.note(RECALL, false);
    r.note(RECALL, true);
    let f = r.feature(RECALL);
    assert_eq!((f.checked, f.would), (3, 2));
    assert_eq!(r.features().len(), 1);
    std::fs::remove_dir_all(&data).ok();
}

pub(crate) fn learned(data: &Path, name: &str, description: &str) {
    let dir = data.join("skills-src").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n"),
    )
    .unwrap();
    let mut index: LearnedIndex =
        std::fs::read(crate::extensions::builtin_skills::learned_dir(data).join("index.json"))
            .ok()
            .and_then(|raw| serde_json::from_slice(&raw).ok())
            .unwrap_or_default();
    index.skills.push(LearnedEntry {
        name: name.into(),
        dir,
        version: 1,
        global: true,
        project_path: None,
    });
    write_learned_index(data, &index).unwrap();
}

/// 候选 takes a learned skill out of what sessions are offered; moving it
/// back up offers it again.
#[test]
fn a_candidate_skill_is_not_offered() {
    let (r, data) = rig();
    learned(&data, "ssh-timeout", "SSH 连接超时时加 ConnectTimeout");
    assert_eq!(
        learned_described(&data),
        [(
            "ssh-timeout".to_string(),
            "SSH 连接超时时加 ConnectTimeout".to_string()
        )]
    );
    assert_eq!(learned_offers(&data, None).len(), 1);
    r.set(&skill_id("ssh-timeout"), Tier::Candidate, 1).unwrap();
    assert!(learned_offers(&data, None).is_empty());
    r.revert(&skill_id("ssh-timeout")).unwrap();
    assert_eq!(learned_offers(&data, None).len(), 1);
    std::fs::remove_dir_all(&data).ok();
}
