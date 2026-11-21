use std::cell::Cell;
use std::collections::HashSet;

use aeroric_protocol::memory::{MemoryDraft, MemoryKind, MemorySettings, MemoryState};

use super::{key, pairs, parse, Source, SYSTEM};
use crate::memory::dream::DreamScope;
use crate::memory::locate::message_hash;
use crate::memory::{evidence, Memory};
use crate::settings::model_detect::ChatOnce;

const TABS: &str = "代码缩进用 tab";
const SAID: &str = "以后代码缩进都改用空格，别再用 tab 了";

fn rig() -> Memory {
    let dir = std::env::temp_dir().join(format!("aeroric-contradict-{}", uuid::Uuid::new_v4()));
    let memory = Memory::default();
    memory.set_dir(dir).unwrap();
    memory.set_project_enabled("p1", true).unwrap();
    memory
        .set_settings(&MemorySettings {
            auto: true,
            connection_id: Some("c1".into()),
            model: Some("deepseek-v4.1-flash".into()),
            ..MemorySettings::default()
        })
        .unwrap();
    memory
}

fn fact(memory: &Memory, text: &str, at: u64) -> String {
    memory
        .remember(
            MemoryDraft {
                project_id: Some("p1".into()),
                kind: MemoryKind::Fact,
                category: None,
                text: text.into(),
                supersedes: None,
            },
            evidence(None, None, None, text, at),
            at,
        )
        .unwrap()
        .id
}

fn session(task: &str, ended: u64, messages: &[(&str, &str)]) -> Source {
    Source {
        task_id: task.into(),
        ended_ms: ended,
        messages: messages
            .iter()
            .map(|(r, t)| (r.to_string(), t.to_string()))
            .collect(),
    }
}

fn scope(sessions: &[Source]) -> DreamScope<'_> {
    DreamScope {
        project_id: Some("p1"),
        title: "trainer",
        forced: true,
        root: None,
        free_only: false,
        sessions,
    }
}

#[test]
fn an_item_is_set_beside_the_users_later_words_sharing_the_most() {
    let memory = rig();
    let id = fact(&memory, TABS, 5);
    fact(&memory, "数据集放在 /data/coco", 5);
    let items = memory.list(Some("p1"), None).unwrap();
    let sources = vec![
        // Ended before the item was written: not looked at.
        session("t-old", 3, &[("user", "代码缩进用 tab 吧")]),
        session(
            "t-new",
            10,
            &[
                ("assistant", "代码缩进用 tab 还是空格？"),
                ("user", "随便看看"),
                ("user", SAID),
            ],
        ),
    ];
    let none = HashSet::new();
    let found = pairs(&items, &sources, &none, &none);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].item.id, id);
    assert_eq!((found[0].source, found[0].message), (1, 2));

    // Asked about before, or a replacement already waiting: left out.
    let checked = HashSet::from([key(&id, SAID)]);
    assert!(pairs(&items, &sources, &checked, &none).is_empty());
    let waiting = HashSet::from([id.clone()]);
    assert!(pairs(&items, &sources, &none, &waiting).is_empty());
}

#[test]
fn the_message_an_item_came_from_is_not_held_against_it() {
    let memory = rig();
    let mut proof = evidence(Some("t-new"), Some("analysis"), None, TABS, 5);
    proof.location = crate::memory::locate::locate(&[("user".into(), SAID.into())], "改用空格");
    memory
        .remember(
            MemoryDraft {
                project_id: Some("p1".into()),
                kind: MemoryKind::Fact,
                category: None,
                text: TABS.into(),
                supersedes: None,
            },
            proof,
            5,
        )
        .unwrap();
    let items = memory.list(Some("p1"), None).unwrap();
    assert_eq!(
        items[0].evidence[0].location.as_ref().unwrap().message_hash,
        message_hash(SAID)
    );
    let sources = vec![session("t-new", 10, &[("user", SAID)])];
    let none = HashSet::new();
    assert!(pairs(&items, &sources, &none, &none).is_empty());
}

#[test]
fn only_corrections_quoting_that_message_are_kept() {
    let memory = rig();
    fact(&memory, TABS, 5);
    let items = memory.list(Some("p1"), None).unwrap();
    let sources = vec![session("t-new", 10, &[("user", SAID)])];
    let none = HashSet::new();
    let found = pairs(&items, &sources, &none, &none);
    let reply = r#"好的：{"corrections": [
        {"pair": 1, "quote": "代码缩进都改用空格", "text": "代码缩进用空格", "reason": "用户改了主意"},
        {"pair": 1, "quote": "代码缩进用空格", "text": "代码缩进用空格", "reason": "重复"},
        {"pair": 2, "quote": "改用空格", "text": "用空格", "reason": "没有这一对"},
        {"pair": 1, "quote": "用 Tab", "text": "代码缩进用 TAB", "reason": "说的还是原话"}
    ]}"#;
    let (kept, dropped) = parse(reply, &found, &sources).unwrap();
    assert_eq!(dropped, 3, "{kept:?}");
    assert_eq!(kept.len(), 1);
    let c = &kept[0];
    assert_eq!(c.text, "代码缩进用空格");
    assert_eq!(c.quote, "代码缩进都改用空格");
    assert_eq!(c.location.message_index, 0);
    let chars: Vec<char> = SAID.chars().collect();
    let span: String = chars[c.location.start as usize..c.location.end as usize]
        .iter()
        .collect();
    assert_eq!(span, c.quote);
    assert_eq!(c.location.message_hash, message_hash(SAID));
}

#[test]
fn a_dream_proposes_a_replacement_carrying_where_the_user_said_it() {
    let memory = rig();
    let old = fact(&memory, TABS, 5);
    let sources = vec![session(
        "t-new",
        10,
        &[
            ("user", "先看下格式"),
            ("user", SAID),
            ("assistant", "好的"),
        ],
    )];
    let asked = Cell::new(0);
    let call = |system: &str, prompt: &str| -> Result<ChatOnce, String> {
        assert_eq!(system, SYSTEM, "only the check needs the model here");
        assert!(prompt.contains(TABS) && prompt.contains(SAID), "{prompt}");
        asked.set(asked.get() + 1);
        Ok(ChatOnce {
            text: r#"{"corrections": [{"pair": 1, "quote": "代码缩进都改用空格", "text": "代码缩进用空格", "reason": "用户后来改用空格"}]}"#.into(),
            input_tokens: 300,
            output_tokens: 40,
        })
    };
    let run = memory.dream(scope(&sources), call, 20).unwrap().unwrap();
    assert_eq!(asked.get(), 1);
    assert_eq!(run.proposed, 1, "{run:?}");
    assert!(
        run.reason.as_deref().unwrap_or("").contains("对不上"),
        "{run:?}"
    );

    let pending = memory.attention().unwrap();
    assert_eq!(pending.len(), 1, "{pending:?}");
    let new = &pending[0];
    assert_eq!(new.state, MemoryState::Pending);
    assert_eq!(new.text, "代码缩进用空格");
    assert_eq!(new.supersedes.as_deref(), Some(old.as_str()));
    let proof = &new.evidence[0];
    assert_eq!(proof.task_id.as_deref(), Some("t-new"));
    assert_eq!(proof.excerpt, "代码缩进都改用空格");
    let at = proof.location.as_ref().unwrap();
    assert_eq!((at.message_index, at.role.as_str()), (1, "user"));
    // C1's lookup finds it where it was said.
    let found = crate::memory::locate::lookup(proof, Some(&sources[0].messages));
    assert_eq!(
        serde_json::to_value(&found).unwrap()["status"],
        serde_json::json!("located")
    );
    // Nothing changes until the user accepts it; then it replaces the old.
    assert_eq!(
        memory.item(&old).unwrap().unwrap().state,
        MemoryState::Active
    );
    memory.confirm(&new.id, true, 30).unwrap();
    assert_eq!(
        memory.item(&old).unwrap().unwrap().state,
        MemoryState::Superseded
    );
}

#[test]
fn a_pair_asked_about_once_is_not_asked_again() {
    let memory = rig();
    fact(&memory, TABS, 5);
    let sources = vec![session("t-new", 10, &[("user", SAID)])];
    let asked = Cell::new(0);
    let call = |_: &str, _: &str| -> Result<ChatOnce, String> {
        asked.set(asked.get() + 1);
        Ok(ChatOnce {
            text: r#"{"corrections": []}"#.into(),
            input_tokens: 10,
            output_tokens: 5,
        })
    };
    let first = memory.dream(scope(&sources), call, 20).unwrap().unwrap();
    assert_eq!(first.proposed, 0);
    let again = memory.dream(scope(&sources), call, 21).unwrap().unwrap();
    assert_eq!(asked.get(), 1, "{again:?}");
    assert!(memory.attention().unwrap().is_empty());
}
