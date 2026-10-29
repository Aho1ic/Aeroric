use super::*;
use std::time::{Duration, SystemTime};

/// A throwaway home + data dir. Never the real home.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "aeroric-import-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }
    fn home(&self) -> PathBuf {
        self.root.join("home")
    }
    fn store(&self) -> ImportStore {
        ImportStore::new(&self.root.join("data"))
    }
    fn write(&self, relative: &str, text: &str) -> PathBuf {
        let path = self.home().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

const CLAUDE: &str = concat!(
    r#"{"type":"user","cwd":"/work/app","timestamp":"2026-10-01T10:00:00Z","message":{"role":"user","content":"<command-name>init</command-name>"}}"#,
    "\n",
    r#"{"type":"user","cwd":"/work/app","message":{"role":"user","content":[{"type":"text","text":"修复登录页\n细节略"}]}}"#,
    "\n",
    r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"好的"}]}}"#,
    "\n"
);

const CODEX: &str = concat!(
    r#"{"timestamp":"2026-10-02T09:00:00Z","type":"session_meta","payload":{"cwd":"/work/other/sub"}}"#,
    "\n",
    r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>x</environment_context>"}]}}"#,
    "\n",
    r#"{"type":"event_msg","payload":{"type":"user_message","message":"加一个测试"}}"#,
    "\n"
);

fn projects() -> Vec<(String, String)> {
    vec![
        ("p-app".to_string(), "/work/app".to_string()),
        ("p-other".to_string(), "/work/other".to_string()),
    ]
}

#[test]
fn nothing_is_read_without_a_grant() {
    let scratch = Scratch::new("ungranted");
    scratch.write(".claude/projects/-work-app/s1.jsonl", CLAUDE);
    let store = scratch.store();
    let report = store.scan(&projects()).unwrap();
    assert_eq!(report, ScanReport::default());
    assert!(store.index().is_empty());
}

#[test]
fn granted_sessions_are_indexed_with_cwd_title_and_project() {
    let scratch = Scratch::new("grant");
    let claude = scratch.write(".claude/projects/-work-app/s1.jsonl", CLAUDE);
    scratch.write(
        ".claude/projects/-work-app/s1/subagents/worker-1.jsonl",
        CLAUDE,
    );
    scratch.write(".codex/sessions/2026/10/02/rollout-a.jsonl", CODEX);
    scratch.write(".codex/sessions/2026/10/02/notes.jsonl", CODEX);
    let store = scratch.store();
    store
        .grant(
            ImportAgent::Claude,
            ImportAgent::Claude.default_root(&scratch.home()),
            1,
        )
        .unwrap();
    store
        .grant(
            ImportAgent::Codex,
            ImportAgent::Codex.default_root(&scratch.home()),
            1,
        )
        .unwrap();
    let report = store.scan(&projects()).unwrap();
    assert_eq!(
        report.added, 2,
        "subagent transcripts and non-rollout files are not sessions"
    );

    let index = store.index();
    let claude_entry = index
        .iter()
        .find(|s| s.agent == ImportAgent::Claude)
        .unwrap();
    assert_eq!(claude_entry.id, session_id(ImportAgent::Claude, &claude));
    assert_eq!(claude_entry.cwd.as_deref(), Some("/work/app"));
    assert_eq!(
        claude_entry.title, "修复登录页",
        "injected <…> blocks are skipped, first line only"
    );
    assert_eq!(
        claude_entry.first_at.as_deref(),
        Some("2026-10-01T10:00:00Z")
    );
    assert_eq!(claude_entry.project_id.as_deref(), Some("p-app"));

    let codex_entry = index
        .iter()
        .find(|s| s.agent == ImportAgent::Codex)
        .unwrap();
    assert_eq!(codex_entry.title, "加一个测试");
    assert_eq!(
        codex_entry.project_id.as_deref(),
        Some("p-other"),
        "a sub-directory belongs to its project"
    );
}

#[test]
fn rescans_are_incremental_and_ids_are_stable() {
    let scratch = Scratch::new("incremental");
    let first = scratch.write(".claude/projects/-work-app/s1.jsonl", CLAUDE);
    scratch.write(".claude/projects/-work-app/s2.jsonl", CLAUDE);
    let store = scratch.store();
    store
        .grant(
            ImportAgent::Claude,
            ImportAgent::Claude.default_root(&scratch.home()),
            1,
        )
        .unwrap();
    store.scan(&projects()).unwrap();
    let ids: Vec<String> = store.index().iter().map(|s| s.id.clone()).collect();

    let again = store.scan(&projects()).unwrap();
    assert_eq!((again.added, again.updated, again.unchanged), (0, 0, 2));

    fs::write(&first, format!("{CLAUDE}{CLAUDE}")).unwrap();
    let changed = store.scan(&projects()).unwrap();
    assert_eq!((changed.updated, changed.unchanged), (1, 1));
    let mut after: Vec<String> = store.index().iter().map(|s| s.id.clone()).collect();
    let mut before = ids;
    after.sort();
    before.sort();
    assert_eq!(after, before, "ids are deterministic");

    fs::remove_file(&first).unwrap();
    let removed = store.scan(&projects()).unwrap();
    assert_eq!(removed.removed, 1);
    assert_eq!(store.index().len(), 1);
}

#[test]
fn source_files_are_never_modified() {
    let scratch = Scratch::new("readonly");
    let path = scratch.write(".claude/projects/-work-app/s1.jsonl", CLAUDE);
    let old = SystemTime::now() - Duration::from_secs(3600);
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(old)
        .unwrap();
    let before = fs::metadata(&path).unwrap();
    let store = scratch.store();
    store
        .grant(
            ImportAgent::Claude,
            ImportAgent::Claude.default_root(&scratch.home()),
            1,
        )
        .unwrap();
    store.scan(&projects()).unwrap();
    let id = store.index()[0].id.clone();
    store.messages(&id).unwrap();
    let after = fs::metadata(&path).unwrap();
    assert_eq!(before.modified().unwrap(), after.modified().unwrap());
    assert_eq!(before.len(), after.len());
    assert_eq!(fs::read_to_string(&path).unwrap(), CLAUDE);
}

#[test]
fn messages_parse_and_unmatched_cwd_stays_unassigned() {
    let scratch = Scratch::new("messages");
    scratch.write(
        ".claude/projects/-elsewhere/s1.jsonl",
        &CLAUDE.replace("/work/app", "/elsewhere"),
    );
    let store = scratch.store();
    store
        .grant(
            ImportAgent::Claude,
            ImportAgent::Claude.default_root(&scratch.home()),
            1,
        )
        .unwrap();
    store.scan(&projects()).unwrap();
    let entry = store.index()[0].clone();
    assert_eq!(entry.project_id, None);
    let (_, messages) = store.messages(&entry.id).unwrap();
    assert!(messages.iter().any(|m| m.role == "assistant"));
}

#[test]
fn oversized_sessions_are_listed_and_read_from_the_tail() {
    let scratch = Scratch::new("large");
    let path = scratch.write(".claude/projects/-work-app/big.jsonl", CLAUDE);
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(MAX_SESSION_BYTES + 1)
        .unwrap();
    let store = scratch.store();
    store
        .grant(
            ImportAgent::Claude,
            ImportAgent::Claude.default_root(&scratch.home()),
            1,
        )
        .unwrap();
    store.scan(&projects()).unwrap();
    let entry = store.index()[0].clone();
    assert!(
        !entry.too_large,
        "2026-10-28: nothing is refused for its size"
    );
    assert_eq!(entry.title, "修复登录页");
    let (_, messages) = store.messages(&entry.id).unwrap();
    assert!(!messages.is_empty(), "the newest lines are read");
}

#[test]
fn revoke_clears_the_index_and_blocks_reads() {
    let scratch = Scratch::new("revoke");
    let path = scratch.write(".claude/projects/-work-app/s1.jsonl", CLAUDE);
    scratch.write(".codex/sessions/rollout-a.jsonl", CODEX);
    let store = scratch.store();
    store
        .grant(
            ImportAgent::Claude,
            ImportAgent::Claude.default_root(&scratch.home()),
            1,
        )
        .unwrap();
    store
        .grant(
            ImportAgent::Codex,
            ImportAgent::Codex.default_root(&scratch.home()),
            1,
        )
        .unwrap();
    store.scan(&projects()).unwrap();
    let claude_id = session_id(ImportAgent::Claude, &path);
    store.revoke(ImportAgent::Claude).unwrap();
    assert!(store.index().iter().all(|s| s.agent == ImportAgent::Codex));
    assert_eq!(
        store.messages(&claude_id).unwrap_err(),
        ImportError::UnknownSession(claude_id)
    );
    assert!(path.exists(), "revoking never touches source files");
    assert_eq!(
        store.scan(&projects()).unwrap().added,
        0,
        "a revoked agent is not scanned again"
    );
}

#[test]
fn estimate_counts_without_reading_or_indexing() {
    let scratch = Scratch::new("estimate");
    scratch.write(".codex/sessions/2026/rollout-a.jsonl", CODEX);
    scratch.write(".codex/sessions/2026/rollout-b.jsonl", CODEX);
    let estimate = estimate(
        ImportAgent::Codex,
        &ImportAgent::Codex.default_root(&scratch.home()),
    );
    assert!(estimate.root_exists);
    assert_eq!(estimate.sessions, 2);
    assert_eq!(estimate.bytes, 2 * CODEX.len() as u64);
    assert!(estimate.latest_ms.is_some());
    assert!(
        !scratch.root.join("data").exists(),
        "estimate writes nothing"
    );
    let missing = super::estimate(
        ImportAgent::Claude,
        &ImportAgent::Claude.default_root(&scratch.home()),
    );
    assert!(!missing.root_exists);
    assert_eq!(missing.sessions, 0);
}

#[test]
fn project_assignment_prefers_the_longest_match() {
    let projects = vec![
        ("outer".to_string(), "/work".to_string()),
        ("inner".to_string(), "/work/app".to_string()),
    ];
    assert_eq!(
        assign_project(Some("/work/app/src"), &projects).as_deref(),
        Some("inner")
    );
    assert_eq!(
        assign_project(Some("/work/apparel"), &projects).as_deref(),
        Some("outer"),
        "path-component match, not string prefix"
    );
    assert_eq!(assign_project(None, &projects), None);
}

/// 2026-10-27: the picker previews without saving; the choice decides what
/// Aeroric shows; archive and remove only change Aeroric's view.
#[test]
fn the_picker_previews_then_chooses_archives_and_removes() {
    let scratch = Scratch::new("picker");
    let app = scratch.write(".claude/projects/-work-app/s1.jsonl", CLAUDE);
    scratch.write(".codex/sessions/2026/10/02/rollout-a.jsonl", CODEX);
    let store = scratch.store();
    let roots = [
        (
            ImportAgent::Claude,
            ImportAgent::Claude.default_root(&scratch.home()),
        ),
        (
            ImportAgent::Codex,
            ImportAgent::Codex.default_root(&scratch.home()),
        ),
    ];
    let preview = store.preview(&roots, &projects());
    assert_eq!(preview.len(), 2);
    assert!(
        preview.iter().all(|p| p.selected),
        "everything is ticked at first"
    );
    assert!(store.index().is_empty(), "a preview saves nothing");

    let claude_id = session_id(ImportAgent::Claude, &app);
    let offered: Vec<String> = preview.iter().map(|p| p.session.id.clone()).collect();
    store
        .choose(&offered, std::slice::from_ref(&claude_id))
        .unwrap();
    for (agent, root) in &roots {
        store.grant(*agent, root.clone(), 1).unwrap();
    }
    store.scan(&projects()).unwrap();
    let visible: Vec<String> = store.visible().into_iter().map(|s| s.id).collect();
    assert_eq!(
        visible,
        std::slice::from_ref(&claude_id),
        "the unticked session is left out"
    );
    let again = store.preview(&roots, &projects());
    assert_eq!(
        again.iter().filter(|p| p.selected).count(),
        1,
        "the choice is remembered"
    );

    store.set_archived(&claude_id, true).unwrap();
    assert!(store.visible()[0].archived);
    store.set_archived(&claude_id, false).unwrap();
    assert!(!store.visible()[0].archived);
    store.remove(&claude_id).unwrap();
    assert!(store.visible().is_empty());
    assert!(app.exists(), "removing never touches the agent's own file");
    assert!(store.remove("imp_unknown").is_err());
}

/// 2026-10-27: new Codex rollouts put developer instructions, AGENTS.md and a
/// large world_state before the first real prompt (often past 64 KB), and the
/// AGENTS.md message is a "user" message. The title is the real prompt.
#[test]
fn codex_titles_skip_agents_md_and_reach_past_a_large_preamble() {
    let scratch = Scratch::new("codex-title");
    let filler = "x".repeat(120 * 1024);
    let text = [
        r#"{"timestamp":"2026-10-02T09:00:00Z","type":"session_meta","payload":{"cwd":"/work/other"}}"#.to_string(),
        r##"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"# AGENTS.md instructions for /work/other\n\n<INSTRUCTIONS>规则</INSTRUCTIONS>"},{"type":"input_text","text":"<environment_context>x</environment_context>"}]}}"##.to_string(),
        format!(r#"{{"type":"world_state","payload":{{"blob":"{filler}"}}}}"#),
        // The prompt line carries a pasted screenshot that runs past the head window.
        format!(
            r#"{{"type":"response_item","payload":{{"type":"message","role":"user","content":[{{"type":"input_text","text":"给项目加 OBB 支持"}},{{"type":"input_image","image_url":"data:image/png;base64,{}"}}]}}}}"#,
            "A".repeat(1536 * 1024)
        ),
        String::new(),
    ]
    .join("\n");
    scratch.write(".codex/sessions/2026/10/02/rollout-b.jsonl", &text);
    let store = scratch.store();
    store
        .grant(
            ImportAgent::Codex,
            ImportAgent::Codex.default_root(&scratch.home()),
            1,
        )
        .unwrap();
    store.scan(&projects()).unwrap();
    let entry = store.index().into_iter().next().unwrap();
    assert_eq!(entry.title, "给项目加 OBB 支持");
    let (_, messages) = store.messages(&entry.id).unwrap();
    let users: Vec<_> = messages.iter().filter(|m| m.role == "user").collect();
    assert_eq!(
        users.len(),
        1,
        "the AGENTS.md / environment block is not a user bubble"
    );
}

#[test]
fn a_session_without_a_prompt_is_named_by_agent_and_time() {
    let scratch = Scratch::new("codex-untitled");
    scratch.write(
        ".codex/sessions/2026/08/01/rollout-2026-08-01T21-20-22-0198.jsonl",
        "{\"type\":\"session_meta\",\"payload\":{\"cwd\":\"/work/app\"}}\n",
    );
    let store = scratch.store();
    store
        .grant(
            ImportAgent::Codex,
            ImportAgent::Codex.default_root(&scratch.home()),
            1,
        )
        .unwrap();
    store.scan(&projects()).unwrap();
    assert_eq!(store.index()[0].title, "Codex 会话 · 2026-08-01 21:20");
}

#[test]
fn codex_archived_sessions_are_found_and_shown_archived_until_brought_back() {
    let scratch = Scratch::new("codex-archived");
    scratch.write(".codex/sessions/2026/10/02/rollout-a.jsonl", CODEX);
    let archived = scratch.write(".codex/archived_sessions/rollout-b.jsonl", CODEX);
    let store = scratch.store();
    store
        .grant(
            ImportAgent::Codex,
            ImportAgent::Codex.default_root(&scratch.home()),
            1,
        )
        .unwrap();
    assert_eq!(store.scan(&projects()).unwrap().added, 2);
    let id = session_id(ImportAgent::Codex, &archived);
    let shown = |store: &ImportStore| store.visible().into_iter().find(|s| s.id == id).unwrap();
    assert!(shown(&store).archived, "Codex archived it");
    store.set_archived(&id, false).unwrap();
    assert!(!shown(&store).archived, "the user brought it back");
    store.set_archived(&id, true).unwrap();
    assert!(shown(&store).archived);
}

#[test]
fn a_continued_session_leaves_the_list_and_keeps_its_native_id() {
    let scratch = Scratch::new("continue");
    scratch.write(".claude/projects/-work-app/0b1c-uuid.jsonl", CLAUDE);
    scratch.write(
        ".codex/sessions/2026/10/01/rollout-2026-10-01T10-00-00-0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b.jsonl",
        CODEX,
    );
    let store = scratch.store();
    for agent in [ImportAgent::Claude, ImportAgent::Codex] {
        store
            .grant(agent, agent.default_root(&scratch.home()), 1)
            .unwrap();
    }
    store.scan(&projects()).unwrap();
    let index = store.index();
    let claude = index
        .iter()
        .find(|s| s.agent == ImportAgent::Claude)
        .unwrap();
    let codex = index
        .iter()
        .find(|s| s.agent == ImportAgent::Codex)
        .unwrap();
    assert_eq!(native_session_id(claude).as_deref(), Some("0b1c-uuid"));
    assert_eq!(
        native_session_id(codex).as_deref(),
        Some("0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b")
    );

    store.set_continued(&claude.id, "task-1").unwrap();
    assert_eq!(store.continued_task(&claude.id).as_deref(), Some("task-1"));
    assert!(store.visible().iter().all(|s| s.id != claude.id));
    assert_eq!(store.visible().len(), 1);
}
