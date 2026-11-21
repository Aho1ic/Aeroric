use aeroric_protocol::memory::{
    Assertion, MemoryCategory, MemoryDraft, MemoryKind, MemoryState, PROFILE_BUDGET_CHARS,
};

use super::{evidence, normalize, Memory, MemoryError};

fn rig() -> (Memory, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("aeroric-memory-{}", uuid::Uuid::new_v4()));
    let memory = Memory::default();
    memory.set_dir(dir.clone()).unwrap();
    (memory, dir)
}

fn draft(project: Option<&str>, kind: MemoryKind, text: &str) -> MemoryDraft {
    MemoryDraft {
        project_id: project.map(str::to_string),
        kind,
        category: None,
        text: text.into(),
        supersedes: None,
    }
}

fn said(text: &str) -> aeroric_protocol::memory::Evidence {
    evidence(Some("task-1"), Some("claude"), None, text, 1)
}

#[test]
fn the_user_remembering_takes_effect_and_turns_the_project_on() {
    let (memory, _dir) = rig();
    assert!(!memory.project_enabled("p1"));
    let item = memory
        .remember(
            draft(Some("p1"), MemoryKind::Fact, "只在 main 分支工作"),
            said("/记住 只在 main 分支工作"),
            10,
        )
        .unwrap();
    assert_eq!(item.state, MemoryState::Active);
    assert_eq!(item.assertion, Assertion::UserExplicit);
    assert_eq!(item.evidence.len(), 1);
    assert_eq!(item.evidence[0].hash.len(), 16);
    assert!(memory.project_enabled("p1"));
    // The same words again only add evidence.
    let again = memory
        .remember(
            draft(Some("p1"), MemoryKind::Fact, "只在 main 分支工作。"),
            said("again"),
            11,
        )
        .unwrap();
    assert_eq!(again.id, item.id);
    assert_eq!(again.evidence.len(), 2);
    assert_eq!(memory.list(Some("p1"), None).unwrap().len(), 1);
    assert!(memory.list(None, None).unwrap().is_empty());
}

#[test]
fn a_proposal_waits_and_a_rejection_bars_it() {
    let (memory, _dir) = rig();
    let item = memory
        .propose(
            draft(Some("p1"), MemoryKind::Fact, "发布前先跑 native:check"),
            said("x"),
            10,
        )
        .unwrap();
    assert_eq!(item.state, MemoryState::Pending);
    assert_eq!(memory.attention().unwrap().len(), 1);
    assert!(
        memory.profile(Some("p1")).is_none(),
        "pending is not handed out"
    );
    let kept = memory.confirm(&item.id, true, 11).unwrap().unwrap();
    assert_eq!(kept.assertion, Assertion::AgentConfirmed);
    assert!(memory.attention().unwrap().is_empty());
    assert_eq!(
        memory.confirm(&item.id, true, 12),
        Err(MemoryError::WrongState)
    );

    let other = memory
        .propose(
            draft(Some("p1"), MemoryKind::Fact, "用 rebase 合并"),
            said("y"),
            13,
        )
        .unwrap();
    assert_eq!(memory.confirm(&other.id, false, 14).unwrap(), None);
    assert_eq!(
        memory.propose(
            draft(Some("p1"), MemoryKind::Fact, "用 rebase 合并"),
            said("y"),
            15
        ),
        Err(MemoryError::Forgotten)
    );
}

#[test]
fn an_edit_supersedes_and_forgetting_takes_the_old_versions_too() {
    let (memory, dir) = rig();
    let old = memory
        .remember(
            draft(None, MemoryKind::Profile, "回复用英文"),
            said("a"),
            10,
        )
        .unwrap();
    let mut change = draft(None, MemoryKind::Profile, "回复一律用简体中文");
    change.supersedes = Some(old.id.clone());
    let new = memory.remember(change, said("b"), 11).unwrap();
    let all = memory.list(None, None).unwrap();
    let old_now = all.iter().find(|i| i.id == old.id).unwrap();
    assert_eq!(old_now.state, MemoryState::Superseded);
    assert_eq!(old_now.superseded_by.as_deref(), Some(new.id.as_str()));

    assert_eq!(memory.forget(&new.id, 12).unwrap().items, 2);
    assert!(memory.list(None, None).unwrap().is_empty());
    for text in ["回复用英文", "回复一律用简体中文"] {
        assert_eq!(
            memory.propose(draft(None, MemoryKind::Profile, text), said(text), 13),
            Err(MemoryError::Forgotten)
        );
    }
    // The words themselves are gone from the database file.
    drop(memory);
    let bytes = std::fs::read(dir.join("memory.db")).unwrap();
    let wal = std::fs::read(dir.join("memory.db-wal")).unwrap_or_default();
    let needle = "简体中文".as_bytes();
    for b in [&bytes, &wal] {
        assert!(!b.windows(needle.len()).any(|w| w == needle));
    }
    // The user saying it again is a new wish.
    let memory = Memory::default();
    memory.set_dir(dir).unwrap();
    assert!(memory
        .remember(
            draft(None, MemoryKind::Profile, "回复一律用简体中文"),
            said("c"),
            14
        )
        .is_ok());
}

#[test]
fn secrets_are_refused_and_undo_leaves_no_barrier() {
    let (memory, _dir) = rig();
    for text in [
        "key is sk-ant-abcdefgh12345",
        "token ghp_0123456789abcdef",
        "api_key=hunter22",
    ] {
        assert!(matches!(
            memory.remember(draft(None, MemoryKind::Fact, text), said(text), 1),
            Err(MemoryError::Secret(_))
        ));
    }
    assert_eq!(
        memory.remember(draft(None, MemoryKind::Fact, "  "), said(""), 1),
        Err(MemoryError::Empty)
    );
    let item = memory
        .remember(draft(None, MemoryKind::Fact, "用 pnpm"), said("x"), 2)
        .unwrap();
    memory.undo(&item.id).unwrap();
    assert!(memory
        .propose(draft(None, MemoryKind::Fact, "用 pnpm"), said("x"), 3)
        .is_ok());
}

#[test]
fn the_profile_puts_pins_first_and_keeps_to_its_budget() {
    let (memory, _dir) = rig();
    let mut lang = draft(None, MemoryKind::Profile, "回复一律用简体中文");
    lang.category = Some(MemoryCategory::Language);
    memory.remember(lang, said("a"), 1).unwrap();
    for i in 0..60 {
        memory
            .remember(
                draft(
                    Some("p1"),
                    MemoryKind::Fact,
                    &format!("第 {i} 条项目约定：一些足够长的说明文字"),
                ),
                said("b"),
                2 + i,
            )
            .unwrap();
    }
    let pinned = memory
        .remember(
            draft(Some("p1"), MemoryKind::Fact, "docs/backlog.md 绝不删除"),
            said("c"),
            1,
        )
        .unwrap();
    memory.pin(&pinned.id, true, 100).unwrap();
    let text = memory.profile(Some("p1")).unwrap();
    assert!(text.chars().count() <= PROFILE_BUDGET_CHARS + 40);
    assert!(text.contains("[语言] 回复一律用简体中文"));
    let facts = text.split("## 事实与约定").nth(1).unwrap();
    assert!(facts.trim_start().starts_with("- docs/backlog.md 绝不删除"));
    assert!(text.contains("条未列出"));
    // A project that does not use memory gets only the profile.
    let other = memory.profile(Some("p2")).unwrap();
    assert!(!other.contains("事实与约定"));
    memory.set_project_enabled("p1", false).unwrap();
    assert!(!memory.profile(Some("p1")).unwrap().contains("项目约定"));
}

#[test]
fn recall_finds_chinese_words_in_scope_and_says_nothing_when_nothing_matches() {
    let (memory, _dir) = rig();
    memory
        .remember(
            draft(
                Some("p1"),
                MemoryKind::Fact,
                "core 依赖仓库外的 dbx，版本钉在 dbx-ref.txt",
            ),
            said("a"),
            1,
        )
        .unwrap();
    memory
        .remember(
            draft(Some("p2"), MemoryKind::Fact, "dbx 在另一个项目里"),
            said("b"),
            2,
        )
        .unwrap();
    memory
        .remember(
            draft(None, MemoryKind::Profile, "回复一律用简体中文"),
            said("c"),
            3,
        )
        .unwrap();
    let hits = memory.recall("p1", "版本 依赖").unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].text.contains("dbx-ref"));
    let hits = memory.recall("p1", "dbx 中文").unwrap();
    assert_eq!(
        hits.len(),
        2,
        "the project's item and the global one, not p2's"
    );
    assert_eq!(hits[0].project_id.as_deref(), Some("p1"));
    assert!(memory.recall("p1", "火星").unwrap().is_empty());
    assert!(memory.recall("p1", "，。").unwrap().is_empty());
}

#[test]
fn archive_pin_and_move() {
    let (memory, _dir) = rig();
    let item = memory
        .remember(
            draft(Some("p1"), MemoryKind::Profile, "我喜欢短回答"),
            said("a"),
            1,
        )
        .unwrap();
    assert_eq!(
        memory.archive(&item.id, true, 2).unwrap().0.state,
        MemoryState::Archived
    );
    assert!(memory.profile(Some("p1")).is_none());
    assert_eq!(
        memory.archive(&item.id, true, 3).map(|(item, _)| item),
        Err(MemoryError::WrongState)
    );
    memory.archive(&item.id, false, 4).unwrap();
    let moved = memory.move_to(&item.id, None, 5).unwrap();
    assert_eq!(moved.project_id, None);
    assert!(memory
        .profile(Some("other"))
        .unwrap()
        .contains("我喜欢短回答"));
}

#[test]
fn small_helpers() {
    assert_eq!(normalize(" Hello， Ｗorld。"), "hello,world");
}

#[test]
fn native_files_are_found_read_only() {
    let root = std::env::temp_dir().join(format!("aeroric-native-{}", uuid::Uuid::new_v4()));
    let claude = root.join(".claude");
    let cwd = root.join("proj");
    let encoded = crate::sessions::discovery::encode_claude_project_path(&cwd.to_string_lossy());
    let mem = claude.join("projects").join(encoded).join("memory");
    std::fs::create_dir_all(&mem).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(mem.join("MEMORY.md"), "- 用户偏好中文").unwrap();
    std::fs::write(cwd.join("AGENTS.md"), "x".repeat(9000)).unwrap();
    let found = super::native::discover(&[("claude".into(), claude)], Some(&cwd));
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].title, "Claude 自动记忆 MEMORY.md");
    assert!(found[1].truncated);
}

#[test]
fn a_proposal_that_supersedes_waits_and_session_facts_stay_out_of_the_profile() {
    let (memory, _dir) = rig();
    let old = memory
        .remember(
            draft(Some("p1"), MemoryKind::Fact, "用 merge 合并"),
            said("a"),
            1,
        )
        .unwrap();
    let mut change = draft(Some("p1"), MemoryKind::Fact, "用 rebase 合并");
    change.supersedes = Some(old.id.clone());
    let proposal = memory.propose(change, said("b"), 2).unwrap();
    let list = memory.list(Some("p1"), None).unwrap();
    assert_eq!(
        list.iter().find(|i| i.id == old.id).unwrap().state,
        MemoryState::Active
    );
    memory.confirm(&proposal.id, true, 3).unwrap();
    let list = memory.list(Some("p1"), None).unwrap();
    let old_now = list.iter().find(|i| i.id == old.id).unwrap();
    assert_eq!(old_now.state, MemoryState::Superseded);
    assert_eq!(old_now.superseded_by.as_deref(), Some(proposal.id.as_str()));

    let clue = memory
        .add_session_fact("p1", "任务：修 SSH 超时｜结果：已修", said("c"), 4)
        .unwrap();
    assert_eq!(clue.kind, MemoryKind::SessionFact);
    assert_eq!(clue.state, MemoryState::Active);
    assert!(!memory.profile(Some("p1")).unwrap().contains("SSH"));
    assert_eq!(memory.recall("p1", "SSH").unwrap()[0].id, clue.id);
    assert!(memory
        .kept("p1")
        .iter()
        .all(|i| i.kind != MemoryKind::SessionFact));
}

#[test]
fn settings_runs_and_the_extracted_mark_persist() {
    use aeroric_protocol::memory::{MemoryRun, MemoryRunState, MemorySettings};
    let (memory, dir) = rig();
    assert_eq!(memory.settings(), MemorySettings::default());
    let settings = MemorySettings {
        auto: true,
        model: Some("flash".into()),
        ..MemorySettings::default()
    };
    memory.set_settings(&settings).unwrap();
    let run = |id: &str, at: u64, state: MemoryRunState| MemoryRun {
        id: id.into(),
        task_id: "t".into(),
        project_id: "p".into(),
        title: "x".into(),
        at_ms: at,
        state,
        reason: None,
        model: None,
        input_tokens: 0,
        output_tokens: 0,
        cost_usd: None,
        facts: 0,
        candidates: 0,
        experiences: 0,
        failed_writes: 0,
        kind: aeroric_protocol::memory::MemoryRunKind::Extract,
        retired: Vec::new(),
        proposed: 0,
        undone_ms: None,
        outcomes: Vec::new(),
    };
    memory
        .add_run(&run("r1", 10, MemoryRunState::Done))
        .unwrap();
    memory
        .add_run(&run("r2", 20, MemoryRunState::Skipped))
        .unwrap();
    memory
        .add_run(&run("r3", 30, MemoryRunState::Failed))
        .unwrap();
    assert_eq!(memory.counted_since(0), 2, "skipped runs are free");
    assert_eq!(memory.counted_since(15), 1);
    assert_eq!(memory.runs(10).unwrap()[0].id, "r3");
    memory.mark_extracted("t", "h1", 1).unwrap();
    drop(memory);
    let memory = Memory::default();
    memory.set_dir(dir).unwrap();
    assert_eq!(memory.settings(), settings);
    assert_eq!(memory.extracted("t").as_deref(), Some("h1"));
}

fn detail(keys: &[&str]) -> aeroric_protocol::memory::ExperienceDetail {
    aeroric_protocol::memory::ExperienceDetail {
        problem: "SSH 连接卡住不返回".into(),
        solution: "改用 connect_timeout".into(),
        search_keys: keys.iter().map(|k| k.to_string()).collect(),
        envs: vec!["macOS".into()],
    }
}

#[test]
fn experiences_are_recalled_counted_and_follow_their_votes() {
    use aeroric_protocol::memory::Confidence;
    let (memory, _dir) = rig();
    memory.set_project_enabled("p1", true).unwrap();
    let low = memory
        .add_experience(
            "p1",
            "SSH 超时的修法",
            &detail(&["ssh", "超时"]),
            Confidence::Low,
            said("a"),
            1,
        )
        .unwrap();
    assert_eq!(low.kind, MemoryKind::Experience);
    assert_eq!(low.confidence, Some(Confidence::Low));
    assert_eq!(low.experience.as_ref().unwrap().envs, ["macOS"]);
    let mid = memory
        .add_experience(
            "p1",
            "另一个超时办法",
            &detail(&["timeout"]),
            Confidence::Medium,
            said("b"),
            2,
        )
        .unwrap();
    // Found by its keys and fix, not only its title; injected never.
    let hits = memory.recall("p1", "connect_timeout").unwrap();
    assert_eq!(
        hits.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
        [mid.id.as_str(), low.id.as_str()]
    );
    assert!(memory.profile(Some("p1")).is_none());
    assert_eq!(memory.item(&low.id).unwrap().unwrap().stats.retrieved, 1);

    // 有用 raises, three times is high; 不对 lowers, and at 低 it is archived.
    let up = memory.feedback(&low.id, true, 3).unwrap();
    assert_eq!(
        (up.confidence, up.stats.helpful),
        (Some(Confidence::Medium), 1)
    );
    memory.feedback(&low.id, true, 4).unwrap();
    let high = memory.feedback(&low.id, true, 5).unwrap();
    assert_eq!(high.confidence, Some(Confidence::High));
    let down = memory.feedback(&mid.id, false, 6).unwrap();
    assert_eq!(down.confidence, Some(Confidence::Low));
    let gone = memory.feedback(&mid.id, false, 7).unwrap();
    assert_eq!(gone.state, MemoryState::Archived);
    assert_eq!(
        memory.feedback(&mid.id, true, 8),
        Err(MemoryError::WrongState)
    );
    assert_eq!(
        memory.feedback("mem-none", true, 8),
        Err(MemoryError::UnknownItem)
    );

    // Any item can be voted on; a profile item is counted when injected.
    let said_item = memory
        .remember(draft(None, MemoryKind::Profile, "回复用中文"), said("c"), 9)
        .unwrap();
    assert!(memory
        .profile_for_session(Some("p1"), None)
        .unwrap()
        .contains("回复用中文"));
    let counted = memory.feedback(&said_item.id, true, 10).unwrap();
    assert_eq!(
        (
            counted.stats.injected,
            counted.stats.helpful,
            counted.confidence
        ),
        (1, 1, None)
    );
    assert!(matches!(
        memory.add_experience(
            "p1",
            "坏的",
            &aeroric_protocol::memory::ExperienceDetail {
                solution: "key sk-ant-abcdefgh123456".into(),
                ..detail(&[])
            },
            Confidence::Low,
            said("d"),
            11
        ),
        Err(MemoryError::Secret(_))
    ));
}

#[test]
fn a_schema_one_database_gains_the_new_columns() {
    let dir = std::env::temp_dir().join(format!("aeroric-memory-v1-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    {
        let conn = rusqlite::Connection::open(dir.join("memory.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE items(id TEXT PRIMARY KEY, project TEXT NOT NULL DEFAULT '', kind TEXT NOT NULL,
                category TEXT, text TEXT NOT NULL, norm TEXT NOT NULL, assertion TEXT NOT NULL,
                state TEXT NOT NULL, pinned INTEGER NOT NULL DEFAULT 0, supersedes TEXT,
                superseded_by TEXT, created_ms INTEGER NOT NULL, updated_ms INTEGER NOT NULL);
             INSERT INTO items VALUES ('mem-old', '', 'profile', NULL, '旧的', '旧的', 'userExplicit',
                'active', 0, NULL, NULL, 1, 1);
             CREATE TABLE evidence(item_id TEXT NOT NULL, task_id TEXT, harness TEXT, model TEXT,
                excerpt TEXT NOT NULL, hash TEXT NOT NULL, at_ms INTEGER NOT NULL);
             INSERT INTO evidence VALUES ('mem-old', 't1', 'analysis', NULL, '旧的依据', 'h', 1);
             CREATE TABLE meta(key TEXT PRIMARY KEY, value INTEGER NOT NULL);
             INSERT INTO meta VALUES ('schema_version', 1);",
        )
        .unwrap();
    }
    let memory = Memory::default();
    memory.set_dir(dir.clone()).unwrap();
    let old = memory.item("mem-old").unwrap().unwrap();
    assert_eq!(old.stats, aeroric_protocol::memory::MemoryStats::default());
    assert_eq!(old.confidence, None);
    // C1: evidence from before keeps reading, with no place.
    assert_eq!(old.evidence.len(), 1);
    assert_eq!(old.evidence[0].excerpt, "旧的依据");
    assert_eq!(old.evidence[0].location, None);
    let conn = rusqlite::Connection::open(dir.join("memory.db")).unwrap();
    let version: i64 = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(version, 2);
}

/// R1-1: other projects' stronger hits must not crowd this project out.
#[test]
fn search_filters_by_project_before_ranking() {
    let (memory, _dir) = rig();
    for n in 0..200 {
        memory
            .remember(
                draft(
                    Some("other"),
                    MemoryKind::Fact,
                    &format!("超时 超时 超时 超时 第{n}条"),
                ),
                said("x"),
                1,
            )
            .unwrap();
    }
    memory
        .remember(
            draft(
                Some("mine"),
                MemoryKind::Fact,
                "这个项目的连接超时要用 connect_timeout，还有很长的说明文字",
            ),
            said("x"),
            1,
        )
        .unwrap();
    let hits = memory.recall("mine", "超时").unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].project_id.as_deref(), Some("mine"));
}

/// E 批 (2026-11-08): `<private>` parts of the person's messages are not
/// remembered; a turn that was all private is left out with its reply.
#[test]
fn private_parts_never_reach_the_extraction() {
    let talk = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(r, t)| (r.to_string(), t.to_string()))
            .collect()
    };
    let messages = talk(&[
        ("user", "用 pnpm <private>我的工号是 9527</private>来装依赖"),
        ("assistant", "好的"),
        ("user", "<private>别记这一轮</private>"),
        ("assistant", "这一轮的回复"),
        ("user", "再跑一遍测试 <private>没关上的标签"),
    ]);
    let transcript = super::extract::transcript(&messages, None);
    assert!(!transcript.text.contains("9527"), "{}", transcript.text);
    assert!(
        transcript.text.contains("用 pnpm 来装依赖"),
        "{}",
        transcript.text
    );
    assert!(!transcript.text.contains("这一轮的回复"));
    assert!(!transcript.text.contains("没关上的标签"));
    assert!(transcript.text.contains("再跑一遍测试"));
    assert_eq!(transcript.user_turns, 2);
}

/// C3 (2026-11-20): an item recalled or injected again in the same task is
/// one use; another task is another. Forgetting drops its use rows.
#[test]
fn uses_are_counted_once_per_task() {
    let (memory, _) = rig();
    memory.set_project_enabled("p1", true).unwrap();
    let item = memory
        .remember(
            draft(Some("p1"), MemoryKind::Fact, "发布前先跑 lint"),
            said("a"),
            1,
        )
        .unwrap();
    for _ in 0..10 {
        let found = memory
            .recall_in_task("p1", "lint", None, Some("t1"))
            .unwrap();
        assert_eq!(found[0].id, item.id);
    }
    memory
        .recall_in_task("p1", "lint", None, Some("t2"))
        .unwrap();
    let profile = memory
        .remember(draft(None, MemoryKind::Profile, "回复用中文"), said("b"), 2)
        .unwrap();
    for _ in 0..3 {
        memory.profile_for_session(Some("p1"), Some("t1"));
    }
    memory.profile_for_session(Some("p1"), Some("t2"));
    let stats = |id: &str| memory.item(id).unwrap().unwrap().stats;
    assert_eq!(stats(&item.id).retrieved, 2);
    assert_eq!(stats(&profile.id).injected, 2);
    // Without a task (the App's search), each call counts, as before.
    memory.recall("p1", "lint").unwrap();
    assert_eq!(stats(&item.id).retrieved, 3);
    memory.forget(&item.id, 3).unwrap();
    let again = memory
        .remember(
            draft(Some("p1"), MemoryKind::Fact, "发布前先跑 clippy"),
            said("c"),
            4,
        )
        .unwrap();
    memory
        .recall_in_task("p1", "clippy", None, Some("t1"))
        .unwrap();
    assert_eq!(stats(&again.id).retrieved, 1);
}

/// C3: an agent's 有用 / 不对 is one vote per item per task — ten votes in
/// one session do not make an experience trusted.
#[test]
fn an_agent_votes_once_per_task() {
    let (memory, _) = rig();
    let id = memory
        .add_experience(
            "p1",
            "SSH 超时改用 ConnectTimeout",
            &aeroric_protocol::memory::ExperienceDetail::default(),
            aeroric_protocol::memory::Confidence::Low,
            said("x"),
            1,
        )
        .unwrap()
        .id;
    let first = memory
        .feedback_in_task(&id, true, "t1", 2)
        .unwrap()
        .unwrap();
    assert_eq!(first.stats.helpful, 1);
    for at in 3..12 {
        assert_eq!(memory.feedback_in_task(&id, true, "t1", at).unwrap(), None);
    }
    assert_eq!(
        memory.feedback_in_task(&id, false, "t1", 12).unwrap(),
        None,
        "one vote, either way"
    );
    let second = memory
        .feedback_in_task(&id, true, "t2", 13)
        .unwrap()
        .unwrap();
    assert_eq!(second.stats.helpful, 2);
    assert_ne!(
        second.confidence,
        Some(aeroric_protocol::memory::Confidence::High),
        "two sessions are not three"
    );
    assert_eq!(
        memory.feedback_in_task("mem-none", true, "t1", 14),
        Err(MemoryError::UnknownItem)
    );
}

/// C3: a damaged full-text index is seen, and 重建索引 — a row of its own
/// in 整理记录, using up nothing — makes search find everything again.
#[test]
fn a_broken_index_is_seen_and_rebuilt_as_a_run() {
    use aeroric_protocol::memory::{MemoryRunKind, MemoryRunState};
    let (memory, _) = rig();
    memory.set_project_enabled("p1", true).unwrap();
    let item = memory
        .remember(
            draft(Some("p1"), MemoryKind::Fact, "部署前清空缓存目录"),
            said("a"),
            1,
        )
        .unwrap();
    memory
        .add_experience(
            "p1",
            "SSH 超时改用 ConnectTimeout",
            &aeroric_protocol::memory::ExperienceDetail {
                problem: "连接卡住".into(),
                solution: "加 ConnectTimeout".into(),
                ..Default::default()
            },
            aeroric_protocol::memory::Confidence::Low,
            said("x"),
            1,
        )
        .unwrap();
    assert!(memory.index_healthy());
    memory.execute_for_test("DELETE FROM items_fts;");
    assert!(!memory.index_healthy());
    assert!(memory.recall("p1", "缓存").unwrap().is_empty());

    let run = memory.rebuild_index(2).unwrap();
    assert_eq!(
        (run.kind, run.state),
        (MemoryRunKind::Reindex, MemoryRunState::Done)
    );
    assert_eq!(run.title, "重建索引");
    assert!(run
        .reason
        .as_deref()
        .unwrap()
        .starts_with("全文索引重写了 2 条"));
    assert!(memory.index_healthy());
    assert_eq!(memory.recall("p1", "缓存").unwrap()[0].id, item.id);
    assert_eq!(
        memory.recall("p1", "卡住").unwrap().len(),
        1,
        "an experience's detail too"
    );
    assert_eq!(memory.runs(10).unwrap(), vec![run]);
    assert_eq!(memory.counted_since(0), 0, "no model, no allowance");
}

/// H7 (2026-11-21): the harnesses' own memory files read as evidence — a
/// project's file into the project's pending facts, the user's into their
/// pending profile, each naming its file; the files stay byte for byte;
/// an unchanged file is not read again; a rejected line does not return.
#[test]
fn native_files_become_proposals_and_stay_untouched() {
    use aeroric_protocol::memory::{MemoryKind, MemoryState};
    let (memory, _dir) = rig();
    let root = std::env::temp_dir().join(format!("aeroric-harvest-{}", uuid::Uuid::new_v4()));
    let claude = root.join(".claude");
    let cwd = root.join("proj");
    let encoded = crate::sessions::discovery::encode_claude_project_path(&cwd.to_string_lossy());
    let mem = claude.join("projects").join(encoded).join("memory");
    std::fs::create_dir_all(&mem).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    let project_file = mem.join("MEMORY.md");
    std::fs::write(
        &project_file,
        "# 记忆\n- 测试用 cargo nextest\n- 发布前跑 verify\n",
    )
    .unwrap();
    std::fs::write(claude.join("CLAUDE.md"), "- 回复一律用简体中文\n").unwrap();
    let before = std::fs::read(&project_file).unwrap();
    let homes = [("claude".to_string(), claude.clone())];

    let files = super::native::discover(&homes, Some(&cwd));
    let first = memory.harvest_native(&files, Some("p1"), 10);
    assert_eq!((first.read, first.proposed), (2, 3));
    let facts = memory.list(Some("p1"), None).unwrap();
    assert_eq!(facts.len(), 2);
    assert!(facts
        .iter()
        .all(|i| i.state == MemoryState::Pending && i.kind == MemoryKind::Fact));
    assert!(
        facts[0].evidence[0]
            .excerpt
            .starts_with(&super::native::short_path(&project_file.to_string_lossy())),
        "{:?}",
        facts[0].evidence
    );
    let profile = memory.list(None, None).unwrap();
    assert_eq!(profile.len(), 1);
    assert_eq!(profile[0].kind, MemoryKind::Profile);
    assert_eq!(
        std::fs::read(&project_file).unwrap(),
        before,
        "the file is only read"
    );

    let again = memory.harvest_native(&files, Some("p1"), 11);
    assert_eq!(
        (again.read, again.proposed),
        (0, 0),
        "unchanged: not read again"
    );

    // The user rejects one line; the file changes; only the new line comes.
    let nextest = facts.iter().find(|i| i.text.contains("nextest")).unwrap();
    memory.confirm(&nextest.id, false, 12).unwrap();
    std::fs::write(
        &project_file,
        "- 测试用 cargo nextest\n- 发布前跑 verify\n- 文档只写现状\n",
    )
    .unwrap();
    let files = super::native::discover(&homes, Some(&cwd));
    let changed = memory.harvest_native(&files, Some("p1"), 13);
    assert_eq!((changed.read, changed.proposed), (1, 1));
    let texts: Vec<String> = memory
        .list(Some("p1"), None)
        .unwrap()
        .into_iter()
        .filter(|i| i.state == MemoryState::Pending)
        .map(|i| i.text)
        .collect();
    assert!(texts.contains(&"文档只写现状".to_string()));
    assert!(
        !texts.iter().any(|t| t.contains("nextest")),
        "rejected stays out"
    );
    // Without a project, a project's file is left for its project.
    let none = memory.harvest_native(&files, None, 14);
    assert_eq!(none.proposed, 0);
    assert_eq!(
        super::native::short_path("/x/.claude/projects/-x-proj/memory/MEMORY.md"),
        "/x/.claude/projects/<本项目>/memory/MEMORY.md"
    );
    std::fs::remove_dir_all(&root).ok();
}

/// H7 (2026-11-21): a fact the opening carried many times with no use
/// leaves it (recall still finds it) and the move is recorded; looked up
/// again twice, it comes back. Pinned facts and the profile stay; a fact
/// found wrong more than useful leaves early.
#[test]
fn facts_move_out_of_the_opening_by_use_and_back() {
    let (memory, _dir) = rig();
    let add = |kind: MemoryKind, text: &str| {
        memory
            .remember(draft(Some("p1"), kind, text), said(text), 1)
            .unwrap()
    };
    let idle = add(MemoryKind::Fact, "旧的打包脚本在 scripts/old 下");
    let used = add(MemoryKind::Fact, "测试用 cargo nextest 跑");
    let pinned = add(MemoryKind::Fact, "只在 main 分支工作");
    memory.pin(&pinned.id, true, 1).unwrap();
    let about = add(MemoryKind::Profile, "回复一律用简体中文");
    let wrong = add(MemoryKind::Fact, "数据库是 Postgres");

    // Carried by eight sessions; one fact is looked up, one is found wrong.
    for n in 0..8 {
        let task = format!("t{n}");
        assert!(memory
            .profile_for_session(Some("p1"), Some(&task))
            .is_some());
    }
    memory.recall("p1", "nextest").unwrap();
    memory.feedback(&wrong.id, false, 2).unwrap();
    let moves = memory.rebalance(Some("p1"), 10).unwrap();
    let out: Vec<&str> = moves
        .iter()
        .filter(|m| m.demoted)
        .map(|m| m.item_id.as_str())
        .collect();
    assert_eq!(moves.len(), 2, "{moves:?}");
    assert!(out.contains(&idle.id.as_str()) && out.contains(&wrong.id.as_str()));
    assert!(moves.iter().any(|m| m.reason.contains("带了 8 次")));
    assert!(moves.iter().any(|m| m.reason.contains("不对")));

    let opening = memory.profile(Some("p1")).unwrap();
    assert!(!opening.contains("scripts/old") && !opening.contains("Postgres"));
    assert!(
        opening.contains("nextest")
            && opening.contains("main 分支")
            && opening.contains("简体中文")
    );
    assert_eq!(
        memory.recall("p1", "打包脚本").unwrap()[0].id,
        idle.id,
        "recall still finds it"
    );
    assert!(
        memory.rebalance(Some("p1"), 11).unwrap().is_empty(),
        "one look is not enough"
    );
    memory.recall("p1", "旧的 打包").unwrap();
    let back = memory.rebalance(Some("p1"), 12).unwrap();
    assert_eq!(back.len(), 1);
    assert!(!back[0].demoted && back[0].item_id == idle.id);
    assert!(back[0].reason.contains("又被检索 2 次"));
    assert!(memory.profile(Some("p1")).unwrap().contains("scripts/old"));
    let log = memory.placements(10).unwrap();
    assert_eq!(log.len(), 3);
    assert_eq!(log[0].item_id, idle.id, "newest first");
    let _ = (used, about);
}
