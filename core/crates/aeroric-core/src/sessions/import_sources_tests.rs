use super::*;
use crate::sessions::import::ImportStore;
use std::time::{SystemTime, UNIX_EPOCH};

/// A throwaway home + data dir. Never the real home.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "aeroric-import-src-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("home")).unwrap();
        Self { root }
    }
    fn home(&self) -> PathBuf {
        self.root.join("home")
    }
    fn env(&self) -> DiscoveryEnv {
        DiscoveryEnv::new(&self.home())
    }
    fn write(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.home().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
    fn import(&self, agent: ImportAgent, projects: &[(String, String)]) -> ImportStore {
        let store = ImportStore::new(&self.root.join("data"));
        store
            .grant_roots(agent, agent.roots(&self.env()), 1)
            .unwrap();
        let report = store.scan(projects).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        store
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn zstd_lines(lines: &[String]) -> Vec<u8> {
    let mut text = lines.join("\n");
    text.push('\n');
    zstd::encode_all(text.as_bytes(), 0).unwrap()
}

#[test]
fn roots_follow_each_agents_own_variables_and_variants() {
    let scratch = Scratch::new("roots");
    let home = scratch.home();
    let mut env = scratch.env();
    assert_eq!(resolve_roots(ImportAgent::Dsh, &env)[0], home.join(".dsh"));
    assert_eq!(
        resolve_roots(ImportAgent::OpenCode, &env)[0],
        home.join(".local/share/opencode")
    );
    assert_eq!(
        resolve_roots(ImportAgent::Cursor, &env)[0],
        home.join(".cursor")
    );

    env.vars.insert("DSH_HOME".into(), "~/custom-dsh".into());
    env.vars.insert("XDG_DATA_HOME".into(), "/data".into());
    env.vars.insert("XDG_CONFIG_HOME".into(), "/config".into());
    assert_eq!(
        resolve_roots(ImportAgent::Dsh, &env)[0],
        home.join("custom-dsh")
    );
    assert_eq!(
        resolve_roots(ImportAgent::OpenCode, &env)[0],
        PathBuf::from("/data/opencode")
    );
    assert_eq!(
        resolve_roots(ImportAgent::Cursor, &env)[0],
        PathBuf::from("/config/cursor")
    );
    env.vars.insert("CURSOR_CONFIG_DIR".into(), "/cc".into());
    assert_eq!(
        resolve_roots(ImportAgent::Cursor, &env)[0],
        PathBuf::from("/cc")
    );

    // Only the China edition has sessions: it becomes the primary root.
    scratch.write(".qoder-cn/projects/-w/s.jsonl", b"{}\n");
    let env = scratch.env();
    assert_eq!(
        resolve_roots(ImportAgent::Qoder, &env),
        vec![home.join(".qoder-cn")]
    );
    scratch.write(".qoder/projects/-w/t.jsonl", b"{}\n");
    assert_eq!(
        resolve_roots(ImportAgent::Qoder, &env),
        vec![home.join(".qoder"), home.join(".qoder-cn")],
        "both editions are scanned"
    );
}

#[test]
fn dsh_v4_sessions_are_imported_and_sub_agents_skipped() {
    let scratch = Scratch::new("dsh");
    let main = [
        r#"{"type":"session","version":4,"id":"s1","createdAt":1761600000000,"cwd":"/work/app","delegationDepth":0}"#,
        r#"{"type":"user/message","seq":1,"data":{"content":[{"type":"text","text":"<runtime>"}],"source":{"kind":"runtime-context"},"role":"user"}}"#,
        r#"{"type":"user/message","seq":2,"data":{"content":[{"type":"text","text":"加一个导出按钮\n细节"}],"source":{"kind":"user"},"role":"user"}}"#,
        r#"{"type":"assistant/message","seq":3,"data":{"message":{"role":"assistant","content":[{"type":"text","text":"好"},{"type":"tool-call","id":"c1","name":"read","arguments":"{}"}],"id":"m1"}}}"#,
        r#"{"type":"tool/result","seq":4,"data":{"message":{"role":"tool","toolCallId":"c1","content":[{"type":"text","text":"file body"}],"isError":false,"id":"r1"}}}"#,
        r#"{"type":"session/title","seq":5,"data":{"title":"导出按钮"}}"#,
    ]
    .map(str::to_string);
    scratch.write(
        ".dsh/sessions/--work-app--/session-1/session.v4.jsonl.zstd",
        &zstd_lines(&main),
    );
    // The predecessor generation beside it is not a second session.
    scratch.write(
        ".dsh/sessions/--work-app--/session-1/session.jsonl.zstd",
        &zstd_lines(&main[..1]),
    );
    let child = [r#"{"type":"session","version":4,"id":"s2","createdAt":1,"cwd":"/work/app","delegationDepth":1}"#.to_string()];
    scratch.write(
        ".dsh/sessions/--work-app--/child/session.v4.jsonl.zstd",
        &zstd_lines(&child),
    );

    let projects = vec![("p1".to_string(), "/work/app".to_string())];
    let store = scratch.import(ImportAgent::Dsh, &projects);
    let index = store.index();
    assert_eq!(index.len(), 1, "the sub-agent session is not listed");
    let session = &index[0];
    assert_eq!(session.title, "导出按钮");
    assert_eq!(session.project_id.as_deref(), Some("p1"));
    assert!(session.source_path.ends_with("session.v4.jsonl.zstd"));

    let (_, messages) = store.messages(&session.id).unwrap();
    let roles: Vec<&str> = messages.iter().map(|m| m.role.as_str()).collect();
    assert_eq!(
        roles,
        ["user", "assistant", "user"],
        "runtime context is not shown"
    );
    assert_eq!(
        messages[2].content,
        vec![SessionContent::ToolResult {
            id: "c1".into(),
            output: "file body".into()
        }]
    );
}

#[test]
fn omp_profiles_are_all_scanned_and_labelled() {
    let scratch = Scratch::new("omp");
    let lines = |title: &str| {
        format!(
            "{{\"type\":\"title\",\"v\":1,\"title\":\"{title}\",\"pad\":\"   \"}}\n{}\n{}\n",
            r#"{"type":"session","version":3,"id":"o1","timestamp":"2026-10-01T10:00:00Z","cwd":"/work/app","title":""}"#,
            r#"{"type":"message","id":"e1","message":{"role":"user","content":"hi"}}"#
        )
    };
    scratch.write(
        ".omp/agent/sessions/-work-app/a.jsonl",
        lines("默认档").as_bytes(),
    );
    scratch.write(
        ".omp/profiles/sota/agent/sessions/-work-app/b.jsonl",
        lines("sota 档").as_bytes(),
    );
    scratch.write(
        ".omp/profiles/anything/agent/sessions/-work-app/c.jsonl",
        lines("任意名").as_bytes(),
    );
    let store = scratch.import(ImportAgent::Omp, &[]);
    let mut found: Vec<(String, Option<String>)> = store
        .index()
        .into_iter()
        .map(|s| (s.title, s.profile))
        .collect();
    found.sort();
    assert_eq!(
        found,
        vec![
            ("sota 档".to_string(), Some("sota".to_string())),
            ("任意名".to_string(), Some("anything".to_string())),
            ("默认档".to_string(), None),
        ]
    );
}

#[test]
fn qoder_reads_like_claude_and_skips_subagents() {
    let scratch = Scratch::new("qoder");
    let line = r#"{"type":"user","cwd":"/work/app","timestamp":"2026-10-01T10:00:00Z","message":{"role":"user","content":"写测试"}}"#;
    scratch.write(
        ".qoder-cn/projects/-work-app/s1.jsonl",
        format!("{line}\n").as_bytes(),
    );
    scratch.write(
        ".qoder-cn/projects/-work-app/s1/subagents/x.jsonl",
        format!("{line}\n").as_bytes(),
    );
    let store = scratch.import(ImportAgent::Qoder, &[]);
    let index = store.index();
    assert_eq!(index.len(), 1);
    assert_eq!(index[0].title, "写测试");
    assert_eq!(index[0].cwd.as_deref(), Some("/work/app"));
    assert_eq!(store.messages(&index[0].id).unwrap().1.len(), 1);
}

fn sqlite(path: &Path, sql: &str) -> Connection {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(sql).unwrap();
    conn
}

#[test]
fn cursor_cli_chats_follow_the_root_blob_order() {
    let scratch = Scratch::new("cursor-cli");
    let group = md5_hex("/work/app");
    let db = scratch
        .home()
        .join(format!(".cursor/chats/{group}/chat-1/store.db"));
    let conn = sqlite(&db, "CREATE TABLE blobs (id TEXT PRIMARY KEY, data BLOB); CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT);");
    let user = r#"{"role":"user","content":[{"type":"text","text":"<user_query>\n修复崩溃\n</user_query>"}]}"#.as_bytes().to_vec();
    let reply = r#"{"id":"1","role":"assistant","content":[{"type":"text","text":"已修复"}]}"#
        .as_bytes()
        .to_vec();
    let user_id = [1u8; 32];
    let reply_id = [2u8; 32];
    // Root: field 1 (tag 0x0a), length 32, per child — reply listed second.
    let mut root = Vec::new();
    for id in [&user_id, &reply_id] {
        root.extend([0x0a, 32]);
        root.extend(id.iter());
    }
    root.extend([0x10, 0x05]); // an unrelated varint field
    for (id, data) in [
        (hex::encode(reply_id), reply),
        (hex::encode(user_id), user),
        ("root".to_string(), root),
    ] {
        conn.execute(
            "INSERT INTO blobs VALUES (?1, ?2)",
            rusqlite::params![id, data],
        )
        .unwrap();
    }
    let meta =
        hex::encode(r#"{"name":"崩溃修复","createdAt":1761600000000,"latestRootBlobId":"root"}"#);
    conn.execute("INSERT INTO meta VALUES ('0', ?1)", [meta])
        .unwrap();
    drop(conn);

    let projects = vec![("p1".to_string(), "/work/app".to_string())];
    let store = scratch.import(ImportAgent::Cursor, &projects);
    let index = store.index();
    assert_eq!(index.len(), 1);
    assert_eq!(index[0].title, "崩溃修复");
    assert_eq!(
        index[0].project_id.as_deref(),
        Some("p1"),
        "cwd recovered from md5(cwd)"
    );
    let (_, messages) = store.messages(&index[0].id).unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(
        messages[0].content,
        vec![SessionContent::Text {
            text: "修复崩溃".into()
        }]
    );
    assert_eq!(messages[1].role, "assistant");
}

#[test]
fn cursor_editor_composers_get_their_workspace_folder() {
    let scratch = Scratch::new("cursor-ide");
    let user_dir = "Library/Application Support/Cursor/User";
    let global = scratch
        .home()
        .join(format!("{user_dir}/globalStorage/state.vscdb"));
    let conn = sqlite(
        &global,
        "CREATE TABLE cursorDiskKV (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);",
    );
    let composer = r#"{"composerId":"c1","name":"重构导航","createdAt":1761600000000,"lastUpdatedAt":1761600500000,"fullConversationHeadersOnly":[{"bubbleId":"b1","type":1},{"bubbleId":"b2","type":2}]}"#;
    let empty = r#"{"composerId":"c2","fullConversationHeadersOnly":[]}"#;
    for (key, value) in [
        ("composerData:c1", composer),
        ("composerData:c2", empty),
        (
            "bubbleId:c1:b1",
            r#"{"bubbleId":"b1","type":1,"text":"把导航拆成组件"}"#,
        ),
        (
            "bubbleId:c1:b2",
            r#"{"bubbleId":"b2","type":2,"text":"好的，已拆分"}"#,
        ),
    ] {
        conn.execute("INSERT INTO cursorDiskKV VALUES (?1, ?2)", [key, value])
            .unwrap();
    }
    drop(conn);
    scratch.write(
        &format!("{user_dir}/workspaceStorage/abc/workspace.json"),
        br#"{"folder":"file:///work/my%20app"}"#,
    );
    let ws = scratch
        .home()
        .join(format!("{user_dir}/workspaceStorage/abc/state.vscdb"));
    let conn = sqlite(
        &ws,
        "CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);",
    );
    conn.execute(
        "INSERT INTO ItemTable VALUES ('composer.composerData', ?1)",
        [r#"{"allComposers":[{"composerId":"c1"}]}"#],
    )
    .unwrap();
    drop(conn);

    let projects = vec![("p1".to_string(), "/work/my app".to_string())];
    let store = scratch.import(ImportAgent::Cursor, &projects);
    let index = store.index();
    assert_eq!(index.len(), 1, "an empty composer is not a conversation");
    assert_eq!(index[0].title, "重构导航");
    assert_eq!(index[0].source_key.as_deref(), Some("c1"));
    assert_eq!(index[0].project_id.as_deref(), Some("p1"));
    let (_, messages) = store.messages(&index[0].id).unwrap();
    let roles: Vec<&str> = messages.iter().map(|m| m.role.as_str()).collect();
    assert_eq!(roles, ["user", "assistant"]);
}

#[test]
fn opencode_v2_sessions_and_messages() {
    let scratch = Scratch::new("opencode");
    let db = scratch.home().join(".local/share/opencode/opencode.db");
    let conn = sqlite(
        &db,
        "CREATE TABLE session_v2 (id TEXT PRIMARY KEY, parent_id TEXT, directory TEXT NOT NULL, title TEXT,
            time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, time_archived INTEGER);
         CREATE TABLE session_message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, type TEXT NOT NULL,
            seq INTEGER NOT NULL, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL);",
    );
    conn.execute_batch(
        r#"INSERT INTO session_v2 VALUES ('s1', NULL, '/work/app', '调试构建', 1761600000000, 1761600900000, NULL);
           INSERT INTO session_v2 VALUES ('s2', 's1', '/work/app', 'child', 1, 2, NULL);
           INSERT INTO session_v2 VALUES ('s3', NULL, '/work/app', '旧会话', 1, 2, 5);
           INSERT INTO session_message VALUES ('m1','s1','user',1,1,1,'{"text":"构建失败了"}');
           INSERT INTO session_message VALUES ('m2','s1','idle',2,1,1,'{}');
           INSERT INTO session_message VALUES ('m3','s1','assistant',3,1,1,'{"content":[{"type":"reasoning","text":"看日志"},{"type":"tool","id":"t1","name":"bash","state":{"input":{"command":"make"},"output":"ok"}},{"type":"text","text":"修好了"}]}');"#,
    )
    .unwrap();
    drop(conn);

    let store = scratch.import(ImportAgent::OpenCode, &[]);
    let mut index = store.index();
    index.sort_by(|a, b| a.title.cmp(&b.title));
    assert_eq!(index.len(), 2, "the child session is not listed");
    let visible = store.visible();
    let old = visible.iter().find(|s| s.title == "旧会话").unwrap();
    assert!(old.archived, "archived in OpenCode, archived here");
    let build = index.iter().find(|s| s.title == "调试构建").unwrap();
    assert_eq!(build.cwd.as_deref(), Some("/work/app"));
    let (_, messages) = store.messages(&build.id).unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(
        messages[1].content.len(),
        4,
        "thinking, tool use, tool result, text"
    );
}

#[test]
fn sources_are_read_only() {
    let scratch = Scratch::new("readonly");
    let db = scratch.home().join(".local/share/opencode/opencode.db");
    sqlite(
        &db,
        "CREATE TABLE session_v2 (id TEXT, parent_id TEXT, directory TEXT, title TEXT, time_created INTEGER, time_updated INTEGER);",
    );
    let before = fs::read(&db).unwrap();
    let modified = fs::metadata(&db).unwrap().modified().unwrap();
    scratch.import(ImportAgent::OpenCode, &[]);
    assert_eq!(fs::read(&db).unwrap(), before);
    assert_eq!(fs::metadata(&db).unwrap().modified().unwrap(), modified);
}
