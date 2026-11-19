//! C2 (2026-11-19): where an item's words live beyond its row, and that
//! forgetting or archiving reaches each of them. The list (with the places
//! outside this store) is in the C2 note.

use aeroric_protocol::memory::{
    Confidence, DreamProposal, DreamProposalState, DreamRelation, ExperienceDetail, MemoryDraft,
    MemoryKind, MemoryState, SkillProposalState,
};

use super::{evidence, Memory};

fn rig() -> Memory {
    let dir = std::env::temp_dir().join(format!("aeroric-withdraw-{}", uuid::Uuid::new_v4()));
    let memory = Memory::default();
    memory.set_dir(dir.join("memory")).unwrap();
    memory
}

fn remember(memory: &Memory, text: &str, at: u64) -> String {
    memory
        .remember(
            MemoryDraft {
                project_id: Some("p1".into()),
                kind: MemoryKind::Fact,
                category: None,
                text: text.into(),
                supersedes: None,
            },
            evidence(Some("t1"), None, None, text, at),
            at,
        )
        .unwrap()
        .id
}

fn withdrawn(memory: &Memory, epoch: u64, text: &str) -> bool {
    memory.withdrawn_since(epoch, &[text]).unwrap()[0]
}

/// Every way words leave the kept set is numbered: forgetting, archiving,
/// replacing by an edit, accepting a proposal that replaces, a dream's
/// merge. Words compare as `normalize` does.
#[test]
fn every_way_out_is_numbered() {
    let memory = rig();
    let start = memory.epoch();
    let a = remember(&memory, "发布前先跑 check", 1);
    assert!(!withdrawn(&memory, start, "发布前先跑 check"));

    memory.archive(&a, true, 2).unwrap();
    let archived = memory.epoch();
    assert!(archived > start);
    assert!(
        withdrawn(&memory, start, "发布前先跑  CHECK。"),
        "compared normalized"
    );
    assert!(
        !withdrawn(&memory, archived, "发布前先跑 check"),
        "not after"
    );
    memory.archive(&a, false, 3).unwrap();

    // An edit replaces it.
    let edited = memory
        .remember(
            MemoryDraft {
                project_id: Some("p1".into()),
                kind: MemoryKind::Fact,
                category: None,
                text: "发布前先跑 native:check".into(),
                supersedes: Some(a.clone()),
            },
            evidence(None, None, None, "x", 4),
            4,
        )
        .unwrap();
    assert!(withdrawn(&memory, archived, "发布前先跑 check"));

    // A proposal that replaces, once accepted.
    let before = memory.epoch();
    let proposal = memory
        .propose(
            MemoryDraft {
                project_id: Some("p1".into()),
                kind: MemoryKind::Fact,
                category: None,
                text: "发布前跑 pnpm verify".into(),
                supersedes: Some(edited.id.clone()),
            },
            evidence(Some("t2"), None, None, "x", 5),
            5,
        )
        .unwrap();
    assert!(
        !withdrawn(&memory, before, "发布前先跑 native:check"),
        "pending: not yet"
    );
    memory.confirm(&proposal.id, true, 6).unwrap();
    assert!(withdrawn(&memory, before, "发布前先跑 native:check"));

    // A dream's merge.
    let b = remember(&memory, "只在 main 分支工作", 7);
    let c = remember(&memory, "只在 main 上干活", 8);
    let before = memory.epoch();
    assert!(memory.with(|store| store.retire(&c, &b, 9)).unwrap());
    assert!(withdrawn(&memory, before, "只在 main 上干活"));

    // Forgetting.
    let before = memory.epoch();
    memory.forget(&b, 10).unwrap();
    assert!(withdrawn(&memory, before, "只在 main 分支工作"));
}

/// Forgetting the item a dream kept takes what it merged too: undoing that
/// dream cannot bring the forgotten words back.
#[test]
fn forgetting_takes_what_a_dream_merged_into_it() {
    let memory = rig();
    let keep = remember(&memory, "只在 main 分支工作", 1);
    let merged = remember(&memory, "只在 main 上干活", 2);
    assert!(memory
        .with(|store| store.retire(&merged, &keep, 3))
        .unwrap());
    let withdrawal = memory.forget(&keep, 4).unwrap();
    assert_eq!(withdrawal.items, 2);
    assert!(memory.item(&merged).unwrap().is_none());
    assert!(
        !memory
            .with(|store| store.unretire(&merged, &keep, 5))
            .unwrap(),
        "nothing to undo into"
    );
    assert!(memory.list(Some("p1"), None).unwrap().is_empty());
}

/// A dream proposal waiting on an item that is forgotten or archived is
/// withdrawn: it cannot be decided any more.
#[test]
fn a_dream_proposal_about_a_withdrawn_item_is_withdrawn() {
    let memory = rig();
    let a = remember(&memory, "只在 main 分支工作", 1);
    let b = remember(&memory, "在 dev 分支上开发", 2);
    let c = remember(&memory, "提交前跑测试", 3);
    let proposal = |id: &str, ids: Vec<String>| DreamProposal {
        id: id.into(),
        run_id: "run-1".into(),
        project_id: Some("p1".into()),
        kind: MemoryKind::Fact,
        relation: DreamRelation::Conflict,
        keep: ids[0].clone(),
        ids,
        reason: "矛盾".into(),
        state: DreamProposalState::Pending,
        created_ms: 4,
        decided_ms: None,
        missing: Vec::new(),
        rewrite: Vec::new(),
        created: Vec::new(),
    };
    memory
        .with(|store| {
            store.put_dream_proposal(&proposal("d1", vec![a.clone(), b.clone()]))?;
            store.put_dream_proposal(&proposal("d2", vec![c.clone(), b.clone()]))
        })
        .unwrap();
    let (_, withdrawal) = memory.archive(&a, true, 5).unwrap();
    assert_eq!(withdrawal.dreams, 1);
    let state = |id: &str| {
        memory
            .dream_proposals(Some("p1"))
            .unwrap()
            .into_iter()
            .find(|p| p.id == id)
            .unwrap()
            .state
    };
    assert_eq!(state("d1"), DreamProposalState::Withdrawn);
    assert_eq!(state("d2"), DreamProposalState::Pending);
    assert_eq!(memory.forget(&c, 6).unwrap().dreams, 1);
    assert_eq!(state("d2"), DreamProposalState::Withdrawn);
}

/// A learned skill made from an experience is marked 依据已撤回 when the
/// experience is archived or forgotten — never removed: an accepted skill
/// stays accepted until the user decides. Restoring takes the mark back.
#[test]
fn a_skill_from_a_withdrawn_experience_is_marked_not_removed() {
    let memory = rig();
    let detail = ExperienceDetail {
        problem: "读 CSV 时首列名多出 BOM".into(),
        solution: "open(path, encoding=\"utf-8-sig\")".into(),
        search_keys: vec!["csv".into()],
        envs: vec![],
    };
    let id = memory
        .add_experience(
            "p1",
            "CSV 带 BOM：读取改用 utf-8-sig",
            &detail,
            Confidence::Low,
            evidence(Some("t"), None, None, "x", 1),
            1,
        )
        .unwrap()
        .id;
    for at in 2..5 {
        memory.feedback(&id, true, at).unwrap();
    }
    let skill = memory.proposals().unwrap().remove(0);
    memory.decide_proposal(&skill.id, true, None, 5).unwrap();

    let (item, withdrawal) = memory.archive(&id, true, 6).unwrap();
    assert_eq!(item.state, MemoryState::Archived);
    assert_eq!(withdrawal.skills, vec![skill.name.clone()]);
    let marked = memory.proposal(&skill.id).unwrap();
    assert_eq!(marked.source_withdrawn_ms, Some(6));
    assert_eq!(marked.state, SkillProposalState::Accepted);

    memory.archive(&id, false, 7).unwrap();
    assert_eq!(
        memory.proposal(&skill.id).unwrap().source_withdrawn_ms,
        None
    );

    let withdrawal = memory.forget(&id, 8).unwrap();
    assert_eq!(withdrawal.skills, vec![skill.name]);
    let kept = memory.proposal(&skill.id).unwrap();
    assert_eq!(kept.source_withdrawn_ms, Some(8));
    assert_eq!(kept.state, SkillProposalState::Accepted);
}

/// After forgetting, the words are not found by any read of this store:
/// the list, search (recall), the pending list, the profile.
#[test]
fn forgotten_words_are_not_read_back_from_the_store() {
    let memory = rig();
    memory.set_project_enabled("p1", true).unwrap();
    let id = remember(&memory, "部署口令放在 vault 的 zebra-42 路径", 1);
    assert_eq!(memory.recall("p1", "zebra").unwrap().len(), 1);
    memory.forget(&id, 2).unwrap();
    let all = memory.list(Some("p1"), None).unwrap();
    assert!(all.iter().all(|i| !i.text.contains("zebra")));
    assert!(memory.recall("p1", "zebra").unwrap().is_empty());
    assert!(memory.list(Some("p1"), Some("zebra")).unwrap().is_empty());
    assert!(memory.attention().unwrap().is_empty());
    assert!(memory
        .profile(Some("p1"))
        .is_none_or(|p| !p.contains("zebra")));
    assert!(memory.item(&id).unwrap().is_none());
}
