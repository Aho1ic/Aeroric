use super::*;

const GIVEN: u64 = 1_800_000_000_000;

fn stages(init: Option<u64>, done: Option<u64>, listed: Option<u64>) -> Stages {
    Stages {
        started_ms: GIVEN + 300,
        pid: 42,
        client: Some("codex-mcp-client".into()),
        initialize_ms: init.map(|d| GIVEN + d),
        initialized_ms: done.map(|d| GIVEN + d),
        tools_list_ms: listed.map(|d| GIVEN + d),
        ..Stages::default()
    }
}

/// The file is named from the token, never with it, and reads back whole.
#[test]
fn the_stage_file_is_named_from_the_token_and_reads_back() {
    let dir = std::env::temp_dir().join(format!("aeroric-stages-{}", uuid::Uuid::new_v4()));
    let at = path(&dir, "secret-token-value");
    assert_eq!(at, path(&dir, "secret-token-value"));
    assert_ne!(at, path(&dir, "another-token"));
    assert!(!at.to_string_lossy().contains("secret"));
    let written = stages(Some(500), Some(900), Some(1000));
    write(&at, &written);
    assert_eq!(read(&at), Some(written));
    assert_eq!(read(&dir.join("none.json")), None);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    prune(&dir, now);
    assert!(at.exists(), "a fresh file stays");
}

/// Each missing step is said with how long the earlier ones took; once
/// the tools were listed, nothing is said.
#[test]
fn the_verdict_names_the_step_that_did_not_come() {
    let now = GIVEN + 31_000;
    let none = verdict(None, GIVEN, now).unwrap();
    assert!(
        none.contains("没有启动") && none.contains("31 秒"),
        "{none}"
    );

    let no_init = verdict(Some(&stages(None, None, None)), GIVEN, now).unwrap();
    assert!(no_init.contains("没有收到 initialize"), "{no_init}");
    assert!(no_init.contains("0.3 秒后启动"), "{no_init}");

    let stuck = verdict(Some(&stages(Some(500), None, None)), GIVEN, now).unwrap();
    assert!(stuck.contains("卡在向 Aeroric 宿主查询"), "{stuck}");
    assert!(stuck.contains("启动后 0.2 秒收到 initialize"), "{stuck}");

    let unlisted = verdict(Some(&stages(Some(500), Some(2500), None)), GIVEN, now).unwrap();
    assert!(unlisted.contains("用 2 秒答完"), "{unlisted}");
    assert!(unlisted.contains("没有来要工具清单"), "{unlisted}");
    assert!(unlisted.contains("codex-mcp-client"), "{unlisted}");
    assert!(unlisted.contains("用不了记忆"), "{unlisted}");

    assert_eq!(
        verdict(Some(&stages(Some(500), Some(900), Some(1000))), GIVEN, now),
        None
    );
}

/// A file from an earlier start (before this token was given) does not
/// count as this start's.
#[test]
fn a_file_from_before_the_server_was_given_does_not_count() {
    let mut old = stages(Some(500), Some(900), Some(1000));
    old.started_ms = GIVEN - 60_000;
    assert!(verdict(Some(&old), GIVEN, GIVEN + 5000)
        .unwrap()
        .contains("没有启动"));
}
