//! P8-05 / G43: real authenticated UDS requests, synthetic files only.
mod host_harness;

use aeroric_protocol::envelope::Frame;
use host_harness::Harness;
use serde_json::{json, Value};

async fn fixture(tag: &str, text: &str) -> Harness {
    Harness::start_with_wiring(tag, |data_dir| {
        let project = data_dir.join("fixture-project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("code.txt"), text).unwrap();
        std::fs::write(
            data_dir.join("projects.json"),
            serde_json::to_vec(&json!([{
                "id": "search-project", "name": "fixture", "path": project,
            }]))
            .unwrap(),
        )
        .unwrap();
        aeroric_host::dispatcher::HostWiring {
            search: aeroric_host::search_services::SearchPrograms {
                rg: data_dir.join("missing-rg").to_string_lossy().into_owned(),
                git: Some("git".into()),
                env: vec![
                    ("HOME".into(), data_dir.to_string_lossy().into_owned()),
                    (
                        "XDG_CONFIG_HOME".into(),
                        data_dir.to_string_lossy().into_owned(),
                    ),
                    ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
                    ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
                ],
            },
            ..Default::default()
        }
    })
    .await
}

async fn call(h: &Harness, method: &str, params: Value) -> Frame {
    let (mut reader, mut writer) = h.connect().await;
    assert!(matches!(
        Harness::handshake(&mut reader, &mut writer, &h.token, "search-test").await,
        Frame::Success(_)
    ));
    Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request("search", method, params),
    )
    .await
}

fn result(frame: Frame) -> Value {
    match frame {
        Frame::Success(success) => success.result,
        other => panic!("expected success: {other:?}"),
    }
}

#[tokio::test]
async fn native_preview_carries_text_and_original_byte_offsets() {
    let h = fixture("sr-wire", "İ title 你好\n").await;
    let preview = result(
        call(
            &h,
            "files.replace_preview",
            json!({
                "projectId": "search-project", "path": "code.txt",
                "query": "title", "replacement": "name",
            }),
        )
        .await,
    );
    let matched = &preview["files"][0]["matches"][0];
    assert!(matched.is_object(), "typed matches, not tuples: {preview}");
    assert_eq!(matched["start"], 3, "offset in original UTF-8 bytes");
    assert_eq!(matched["end"], 8);
    assert_eq!(matched["matchText"], "title", "stale-write precondition");
    assert_eq!(preview["files"][0]["path"], "code.txt");
}

#[tokio::test]
async fn native_apply_skips_equal_length_changed_match() {
    let h = fixture("sr-stale", "bravo\n").await;
    let summary = result(
        call(
            &h,
            "files.apply_replacements",
            json!({
                "projectId": "search-project", "path": "code.txt",
                "replacement": "wrong",
                "matches": [{"start": 0, "end": 5, "matchText": "alpha"}],
            }),
        )
        .await,
    );
    assert_eq!(summary["replacementsApplied"], 0);
    assert_eq!(summary["replacementsSkipped"], 1);
    assert_eq!(
        std::fs::read_to_string(h.data_dir.join("fixture-project/code.txt")).unwrap(),
        "bravo\n"
    );
}

#[tokio::test]
async fn native_apply_rejects_offsets_without_expected_text() {
    let h = fixture("sr-unchecked", "alpha\n").await;
    let reply = call(
        &h,
        "files.apply_replacements",
        json!({
            "projectId": "search-project", "path": "code.txt",
            "replacement": "wrong", "matches": [{"start": 0, "end": 5}],
        }),
    )
    .await;
    assert!(matches!(reply, Frame::Failure(_)), "{reply:?}");
    assert_eq!(
        std::fs::read_to_string(h.data_dir.join("fixture-project/code.txt")).unwrap(),
        "alpha\n"
    );
}

#[tokio::test]
async fn legacy_search_structured_and_rich_replacement_round_trip() {
    let h = fixture("sr-legacy", "function helper(a) { return title; }\n").await;
    let root = h.data_dir.join("fixture-project");
    let found = result(
        call(
            &h,
            "files.search_text",
            json!({
                "projectPath": root, "query": "title", "options": {},
            }),
        )
        .await,
    );
    assert_eq!(found[0]["matchText"], "title");
    assert!(found[0]["path"].as_str().unwrap().ends_with("/code.txt"));
    let structured = result(call(&h, "files.search_structured", json!({
        "target": {"kind":"local", "path":root}, "pattern":"function $Name$($Args$)", "options":{},
    })).await);
    assert_eq!(structured[0]["matchText"], "function helper(a)");
    let preview = result(
        call(
            &h,
            "files.replace_text_preview",
            json!({
                "projectPath":root, "query":"title", "replacement":"name", "options":{},
            }),
        )
        .await,
    );
    assert_eq!(preview["totalMatches"], 1);
    let matched = preview["files"][0]["matches"][0].clone();
    let applied = result(
        call(
            &h,
            "files.apply_text_replacements",
            json!({
                "projectPath":root, "replacements":[matched],
            }),
        )
        .await,
    );
    assert_eq!(applied["replacementsApplied"], 1);
    assert_eq!(
        std::fs::read_to_string(root.join("code.txt")).unwrap(),
        "function helper(a) { return name; }\n"
    );
}

#[tokio::test]
async fn native_empty_query_returns_and_preview_then_apply_round_trips() {
    let h = fixture("sr-native", "你好 title\n").await;
    let empty = result(
        call(
            &h,
            "files.replace_preview",
            json!({
                "projectId":"search-project", "path":"code.txt", "query":"", "replacement":"x",
            }),
        )
        .await,
    );
    assert_eq!(empty["totalMatches"], 0);
    let preview = result(call(&h, "files.replace_preview", json!({
        "projectId":"search-project", "path":"code.txt", "query":"title", "replacement":"name",
    })).await);
    let matched = &preview["files"][0]["matches"][0];
    assert_eq!(matched["start"], 7);
    let summary = result(
        call(
            &h,
            "files.apply_replacements",
            json!({
                "projectId":"search-project", "path":"code.txt", "replacement":"name",
                "matches":[matched],
            }),
        )
        .await,
    );
    assert_eq!(summary["replacementsApplied"], 1);
    assert_eq!(
        std::fs::read_to_string(h.data_dir.join("fixture-project/code.txt")).unwrap(),
        "你好 name\n"
    );
}

#[tokio::test]
async fn unknown_roots_and_native_absolute_or_traversing_paths_are_rejected() {
    let h = fixture("sr-scope", "title\n").await;
    let outside = h.data_dir.join("unregistered");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), "title\n").unwrap();
    for params in [
        json!({"projectPath":outside, "query":"title", "options":{}}),
        json!({"projectId":"unknown", "query":"title", "options":{}}),
        json!({"projectId":"search-project", "projectPath":outside, "query":"title"}),
    ] {
        assert!(matches!(
            call(&h, "files.search_text", params).await,
            Frame::Failure(_)
        ));
    }
    for path in [
        "../unregistered/secret.txt".to_string(),
        outside.join("secret.txt").to_string_lossy().into_owned(),
    ] {
        assert!(matches!(call(&h, "files.replace_preview", json!({
            "projectId":"search-project", "path":path, "query":"title", "replacement":"oops",
        })).await, Frame::Failure(_)));
    }
    assert_eq!(
        std::fs::read_to_string(outside.join("secret.txt")).unwrap(),
        "title\n"
    );
}

#[tokio::test]
async fn ssh_and_wsl_targets_never_fall_through_to_local_search_or_apply() {
    let h = fixture("sr-target", "title\n").await;
    for target in [
        json!({"kind":"ssh","connectionId":"c1","remotePath":"/srv/app"}),
        json!({"kind":"wsl","distribution":"Ubuntu","linuxPath":"/home/app"}),
    ] {
        for method in [
            "files.search_text",
            "files.search_structured",
            "files.replace_text_preview",
            "files.apply_text_replacements",
        ] {
            let reply = call(
                &h,
                method,
                json!({
                    "target":target, "projectId":"search-project",
                    "query":"title", "pattern":"title", "replacement":"oops", "replacements":[],
                }),
            )
            .await;
            match reply {
                Frame::Failure(error) => assert_eq!(
                    error.error.code,
                    aeroric_protocol::error::ErrorCode::UnsupportedCapability
                ),
                other => panic!("remote target must be rejected: {other:?}"),
            }
        }
    }
    assert_eq!(
        std::fs::read_to_string(h.data_dir.join("fixture-project/code.txt")).unwrap(),
        "title\n"
    );
}

#[tokio::test]
async fn registered_notebook_vault_and_recorded_task_worktree_are_supported() {
    let h = fixture("sr-roots", "title\n").await;
    let vault = h.data_dir.join("vault");
    std::fs::create_dir_all(&vault).unwrap();
    std::fs::write(vault.join("note.md"), "title\n").unwrap();
    assert!(matches!(
        call(
            &h,
            "files.search_text",
            json!({"projectPath":vault,"query":"title"})
        )
        .await,
        Frame::Failure(_)
    ));
    result(call(&h, "notebook.register_vault", json!({"path":vault})).await);
    let found = result(
        call(
            &h,
            "files.search_text",
            json!({"projectPath":vault,"query":"title"}),
        )
        .await,
    );
    assert_eq!(found[0]["name"], "note.md");
    result(call(&h, "notebook.unregister_vault", json!({"path":vault})).await);
    assert!(matches!(
        call(
            &h,
            "files.search_text",
            json!({"projectPath":vault,"query":"title"})
        )
        .await,
        Frame::Failure(_)
    ));

    let worktree = h.data_dir.join("worktree");
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(worktree.join("task.txt"), "title\n").unwrap();
    let records = h.data_dir.join("projects/search-project");
    std::fs::create_dir_all(&records).unwrap();
    std::fs::write(
        records.join("tasks.json"),
        serde_json::to_vec(&json!([{
            "id":"t1", "worktreePath":worktree,
        }]))
        .unwrap(),
    )
    .unwrap();
    let found = result(
        call(
            &h,
            "files.search_text",
            json!({"projectPath":worktree,"query":"title"}),
        )
        .await,
    );
    assert_eq!(found[0]["name"], "task.txt");
}

#[tokio::test]
async fn search_requires_authentication_and_malformed_matches_fail_without_partial_writes() {
    let h = fixture("sr-auth", "title\n").await;
    let (mut reader, mut writer) = h.connect().await;
    let reply = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request(
            "no-auth",
            "files.search_text",
            json!({"projectId":"search-project","query":"title"}),
        ),
    )
    .await;
    match reply {
        Frame::Failure(error) => assert_eq!(
            error.error.code,
            aeroric_protocol::error::ErrorCode::Unauthenticated
        ),
        other => panic!("{other:?}"),
    }
    let reply = call(&h, "files.apply_replacements", json!({
        "projectId":"search-project", "path":"code.txt", "replacement":"wrong",
        "matches":[{"start":0,"end":5,"matchText":"title"}, {"start":-1,"end":2,"matchText":"bad"}],
    })).await;
    assert!(matches!(reply, Frame::Failure(_)));
    assert_eq!(
        std::fs::read_to_string(h.data_dir.join("fixture-project/code.txt")).unwrap(),
        "title\n"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_escape_and_cycle_are_not_searched_or_replaced() {
    let h = fixture("sr-link", "plain\n").await;
    let root = h.data_dir.join("fixture-project");
    let outside = h.data_dir.join("secret.txt");
    std::fs::write(&outside, "secret title\n").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("escape.txt")).unwrap();
    std::os::unix::fs::symlink(&root, root.join("cycle")).unwrap();
    let found = result(
        call(
            &h,
            "files.search_text",
            json!({"projectId":"search-project","query":"title"}),
        )
        .await,
    );
    assert_eq!(found, json!([]));
    let preview = result(
        call(
            &h,
            "files.replace_text_preview",
            json!({
                "projectId":"search-project","query":"title","replacement":"oops",
            }),
        )
        .await,
    );
    assert_eq!(preview["totalMatches"], 0);
    let summary = result(call(&h, "files.apply_text_replacements", json!({
        "projectId":"search-project","replacements":[{
            "path":root.join("escape.txt"),"start":7,"end":12,"matchText":"title","replacementText":"oops"
        }],
    })).await);
    assert_eq!(summary["replacementsSkipped"], 1);
    assert_eq!(std::fs::read_to_string(outside).unwrap(), "secret title\n");
}

#[tokio::test]
async fn filename_methods_use_the_selected_root_and_preserve_git_semantics() {
    let h = fixture("sr-names", "title\n").await;
    let root = h.data_dir.join("fixture-project");
    // Non-repository list uses the directory fallback.
    let listed = result(
        call(
            &h,
            "files.list_project_files",
            json!({"projectId":"search-project"}),
        )
        .await,
    );
    assert_eq!(listed, json!(["code.txt"]));
    std::fs::write(root.join("代码.ts"), "code\n").unwrap();
    for args in [&["init", "--quiet"][..], &["add", "--", "."][..]] {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .env("HOME", &h.home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    std::fs::write(root.join("untracked.ts"), "new\n").unwrap();
    let found = result(
        call(
            &h,
            "files.search_project_files",
            json!({
                "projectPath":root, "query":"代", "extensions":[".TS"], "limit":10,
            }),
        )
        .await,
    );
    assert_eq!(found.as_array().unwrap().len(), 1);
    assert_eq!(found[0]["name"], "代码.ts");
    assert_eq!(found[0]["extension"], "ts");
    let listed = result(
        call(
            &h,
            "files.list_project_files",
            json!({"projectId":"search-project"}),
        )
        .await,
    );
    assert!(listed.as_array().unwrap().contains(&json!("untracked.ts")));
}

#[tokio::test]
async fn native_literal_wire_rejects_regex_and_legacy_preserves_capture_replacements() {
    let h = fixture("sr-captures", "title2\n").await;
    let params = json!({
        "projectId":"search-project", "path":"code.txt",
        "query":"(title)([0-9])", "replacement":"${1}_${2}", "options":{"regex":true},
    });
    // Native's published apply shape has one uniform literal replacement. It
    // cannot promise a capture preview then write the unexpanded template.
    let native = call(&h, "files.replace_preview", params.clone()).await;
    assert!(matches!(native, Frame::Failure(_)), "{native:?}");
    let preview = result(call(&h, "files.replace_text_preview", params).await);
    assert_eq!(
        preview["files"][0]["matches"][0]["replacementText"],
        "title_2"
    );
    let applied = result(
        call(
            &h,
            "files.apply_text_replacements",
            json!({
                "projectId":"search-project", "replacements":preview["files"][0]["matches"],
            }),
        )
        .await,
    );
    assert_eq!(applied["replacementsApplied"], 1);
    assert_eq!(
        std::fs::read_to_string(h.data_dir.join("fixture-project/code.txt")).unwrap(),
        "title_2\n"
    );
}
