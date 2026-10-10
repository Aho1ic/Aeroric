//! Search has its own apply registrations; a known root and APPLY_LOCK do not
//! authorize a host that has no current data-directory lease.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use aeroric_host::dispatcher::HostWiring;
use aeroric_host::search_services::SearchPrograms;
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

const LEGACY: &str = "files.apply_text_replacements";
const NATIVE: &str = "files.apply_replacements";

struct Rig {
    host: Harness,
    project: PathBuf,
}

impl Rig {
    async fn start(tag: &str, read_only: bool) -> Self {
        let wiring = |data: &Path| {
            let project = data.join("search project");
            fs::create_dir(&project).unwrap();
            for name in ["legacy.txt", "native.txt"] {
                let file = project.join(name);
                fs::write(&file, "İ title\n").unwrap();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(file, fs::Permissions::from_mode(0o640)).unwrap();
                }
            }
            fs::write(
                data.join("projects.json"),
                serde_json::to_vec(&json!([
                    {"id": "p", "name": "search", "path": project}
                ]))
                .unwrap(),
            )
            .unwrap();
            // The two filename queries use distinct Git output delimiters.
            // This fake never reads a Git index/config or starts another tool.
            let git = data.join("fake-git");
            fs::write(&git, concat!(
                "#!/bin/sh\n",
                "for arg in \"$@\"; do\n",
                "  if [ \"$arg\" = '-z' ]; then printf 'legacy.txt\\000native.txt\\000'; exit 0; fi\n",
                "done\n",
                "printf 'legacy.txt\\nnative.txt\\n'\n",
            )).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&git, fs::Permissions::from_mode(0o700)).unwrap();
            }
            HostWiring {
                search: SearchPrograms {
                    rg: data.join("missing-rg").to_string_lossy().into_owned(),
                    git: Some(git.to_string_lossy().into_owned()),
                    env: vec![
                        ("HOME".into(), data.to_string_lossy().into_owned()),
                        (
                            "XDG_CONFIG_HOME".into(),
                            data.to_string_lossy().into_owned(),
                        ),
                        ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
                        (
                            "GIT_CONFIG_GLOBAL".into(),
                            data.join("empty-gitconfig").to_string_lossy().into_owned(),
                        ),
                    ],
                },
                ..HostWiring::default()
            }
        };
        let host = if read_only {
            Harness::start_read_only_with_wiring(tag, wiring).await
        } else {
            Harness::start_with_wiring(tag, wiring).await
        };
        let project = host.data_dir.join("search project").canonicalize().unwrap();
        Self { host, project }
    }

    async fn connect(&self) -> (BufReader<OwnedReadHalf>, OwnedWriteHalf) {
        let (mut reader, mut writer) = self.host.connect().await;
        result(
            Harness::handshake(
                &mut reader,
                &mut writer,
                &self.host.token,
                "search-authority",
            )
            .await,
        );
        (reader, writer)
    }

    fn params(&self, method: &str, expected: &str, replacement: &str) -> Value {
        let end = 3 + expected.len();
        if method == LEGACY {
            json!({"projectPath": self.project, "replacements": [{
                "path": self.project.join("legacy.txt"), "start": 3, "end": end,
                "matchText": expected, "replacementText": replacement
            }]})
        } else {
            json!({"projectId": "p", "path": "native.txt", "replacement": replacement,
                "matches": [{"start": 3, "end": end, "matchText": expected}]
            })
        }
    }

    fn snapshot(&self) -> Snapshot {
        let files = fs::read_dir(&self.project)
            .unwrap()
            .map(|item| {
                let item = item.unwrap();
                assert!(
                    item.file_type().unwrap().is_file(),
                    "no new directories/links"
                );
                (
                    item.file_name().to_string_lossy().into_owned(),
                    (fs::read(item.path()).unwrap(), mode(&item.path())),
                )
            })
            .collect();
        let registry = self.host.data_dir.join("projects.json");
        Snapshot {
            files,
            project_mode: mode(&self.project),
            registry: fs::read(&registry).unwrap(),
            registry_mode: mode(&registry),
        }
    }

    fn replace_lock(&self) {
        let lock = self.host.data_dir.join(".write.lock");
        assert!(lock.is_file());
        fs::rename(&lock, lock.with_extension("retired")).unwrap();
        fs::write(&lock, b"new inode at the same path\n").unwrap();
    }
}

fn mode(path: &Path) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }
    #[cfg(not(unix))]
    u32::from(fs::metadata(path).unwrap().permissions().readonly())
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    files: BTreeMap<String, (Vec<u8>, u32)>,
    project_mode: u32,
    registry: Vec<u8>,
    registry_mode: u32,
}

async fn call(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    method: &str,
    params: Value,
) -> Frame {
    let reply = Harness::call(reader, writer, &Harness::request(method, method, params)).await;
    match &reply {
        Frame::Success(response) => assert_eq!(response.id, method),
        Frame::Failure(response) => assert_eq!(response.id, method),
        other => panic!("unexpected response: {other:?}"),
    }
    reply
}

fn result(reply: Frame) -> Value {
    match reply {
        Frame::Success(response) => response.result,
        other => panic!("expected success: {other:?}"),
    }
}

fn error(reply: Frame, code: ErrorCode) -> String {
    let Frame::Failure(response) = reply else {
        panic!("expected refusal: {reply:?}")
    };
    assert_eq!(response.error.code, code, "{response:?}");
    assert!(!response.error.retryable);
    response.error.message
}

fn no_writer(reply: Frame) {
    let message = error(reply, ErrorCode::IoError);
    assert!(message.contains("no write permission"), "{message}");
}

async fn deny(method: &str) {
    let rig = Rig::start(
        if method == LEGACY {
            "sw-legacy"
        } else {
            "sw-native"
        },
        true,
    )
    .await;
    let (mut reader, mut writer) = rig.connect().await;
    let before = rig.snapshot();
    no_writer(
        call(
            &mut reader,
            &mut writer,
            method,
            rig.params(method, "title", "wrong"),
        )
        .await,
    );
    assert_eq!(rig.snapshot(), before);
}

#[tokio::test]
async fn legacy_apply_refuses_a_read_only_context() {
    deny(LEGACY).await;
}

#[tokio::test]
async fn native_apply_refuses_a_read_only_context() {
    deny(NATIVE).await;
}

#[tokio::test]
async fn both_apply_methods_recheck_the_current_lease_on_one_connection() {
    let rig = Rig::start("sw-lost", false).await;
    let (mut reader, mut writer) = rig.connect().await;
    let applied = result(
        call(
            &mut reader,
            &mut writer,
            LEGACY,
            rig.params(LEGACY, "title", "first"),
        )
        .await,
    );
    assert_eq!(applied["replacementsApplied"], 1);
    rig.replace_lock();
    let before = rig.snapshot();
    let mut replies = Vec::new();
    for (method, expected) in [(LEGACY, "first"), (NATIVE, "title")] {
        replies.push(
            call(
                &mut reader,
                &mut writer,
                method,
                rig.params(method, expected, "wrong"),
            )
            .await,
        );
    }
    for reply in replies {
        let message = error(reply, ErrorCode::IoError);
        assert!(
            message.contains("no write permission") && message.contains("lease was lost"),
            "{message}"
        );
    }
    assert_eq!(rig.snapshot(), before);
    assert_eq!(result(call(&mut reader, &mut writer, "files.replace_preview",
        json!({"projectId": "p", "path": "native.txt", "query": "title", "replacement": "name"})).await)["totalMatches"], 1);
}

#[tokio::test]
async fn writer_refusal_precedes_registry_and_root_io() {
    let rig = Rig::start("sw-preio", true).await;
    let (mut reader, mut writer) = rig.connect().await;
    for corrupt in [false, true] {
        if corrupt {
            fs::write(
                rig.host.data_dir.join("projects.json"),
                b"{invalid registry",
            )
            .unwrap();
        }
        let before = rig.snapshot();
        for method in [LEGACY, NATIVE] {
            let mut params = rig.params(method, "title", "wrong");
            params["projectId"] = json!("not-registered");
            params["projectPath"] = json!(rig.project.join("missing"));
            no_writer(call(&mut reader, &mut writer, method, params).await);
        }
        assert_eq!(rig.snapshot(), before);
    }
}

#[tokio::test]
async fn unsupported_and_malformed_targets_are_rejected_before_writer_admission() {
    let rig = Rig::start("sw-target", true).await;
    let (mut reader, mut writer) = rig.connect().await;
    fs::write(
        rig.host.data_dir.join("projects.json"),
        b"{invalid registry",
    )
    .unwrap();
    let before = rig.snapshot();
    for method in [LEGACY, NATIVE] {
        for target in [
            json!({"kind": "ssh", "connectionId": "synthetic", "remotePath": "/srv/fake"}),
            json!({"kind": "wsl", "distribution": "synthetic", "linuxPath": "/srv/fake"}),
        ] {
            let mut params = rig.params(method, "title", "wrong");
            params["target"] = target;
            error(
                call(&mut reader, &mut writer, method, params).await,
                ErrorCode::UnsupportedCapability,
            );
        }
        for target in [Value::Null, json!({"kind": "local"}), json!(false)] {
            let mut params = rig.params(method, "title", "wrong");
            params["target"] = target;
            let message = error(
                call(&mut reader, &mut writer, method, params).await,
                ErrorCode::InvalidArgument,
            );
            assert!(message.contains("invalid target"), "{message}");
        }
    }
    assert_eq!(rig.snapshot(), before);
}

#[tokio::test]
async fn all_six_search_and_preview_queries_work_without_writer_authority() {
    let rig = Rig::start("sw-queries", true).await;
    let (mut reader, mut writer) = rig.connect().await;
    let before = rig.snapshot();
    for (method, params, count) in [
        (
            "files.search_text",
            json!({"projectId": "p", "query": "title"}),
            2,
        ),
        (
            "files.search_structured",
            json!({"projectId": "p", "pattern": "title"}),
            2,
        ),
        ("files.list_project_files", json!({"projectId": "p"}), 2),
        (
            "files.search_project_files",
            json!({"projectId": "p", "query": "native", "extensions": ["txt"]}),
            1,
        ),
    ] {
        assert_eq!(
            result(call(&mut reader, &mut writer, method, params).await)
                .as_array()
                .unwrap()
                .len(),
            count,
            "{method}"
        );
    }
    for (method, params, count) in [
        (
            "files.replace_text_preview",
            json!({"projectPath": rig.project, "query": "title", "replacement": "name"}),
            2,
        ),
        (
            "files.replace_preview",
            json!({"projectId": "p", "path": "native.txt", "query": "title", "replacement": "name"}),
            1,
        ),
    ] {
        assert_eq!(
            result(call(&mut reader, &mut writer, method, params).await)["totalMatches"],
            count,
            "{method}"
        );
    }
    assert_eq!(rig.snapshot(), before);
}

#[tokio::test]
async fn authorized_legacy_and_native_apply_keep_wire_and_stale_match_behavior() {
    let rig = Rig::start("sw-writer", false).await;
    let (mut reader, mut writer) = rig.connect().await;
    for method in [LEGACY, NATIVE] {
        let mut params = rig.params(method, "title", "name");
        params["target"] = json!({"kind": "local", "path": rig.project});
        assert_eq!(
            result(call(&mut reader, &mut writer, method, params.clone()).await),
            json!({"filesChanged": 1, "replacementsApplied": 1, "replacementsSkipped": 0})
        );
        assert_eq!(
            result(call(&mut reader, &mut writer, method, params).await),
            json!({"filesChanged": 0, "replacementsApplied": 0, "replacementsSkipped": 1})
        );
    }
    for file in ["legacy.txt", "native.txt"] {
        assert_eq!(
            fs::read_to_string(rig.project.join(file)).unwrap(),
            "İ name\n"
        );
        #[cfg(unix)]
        assert_eq!(mode(&rig.project.join(file)), 0o640);
    }
    let before = rig.snapshot();
    for method in [LEGACY, NATIVE] {
        let mut params = rig.params(method, "name", "wrong");
        params["projectPath"] = json!(rig.project.join("missing"));
        params["target"] = json!({"kind": "local", "path": rig.project});
        error(
            call(&mut reader, &mut writer, method, params).await,
            ErrorCode::InvalidArgument,
        );
    }
    assert_eq!(rig.snapshot(), before);
}

#[tokio::test]
async fn empty_apply_is_still_a_mutation_not_a_way_around_authority() {
    let rig = Rig::start("sw-empty", true).await;
    let (mut reader, mut writer) = rig.connect().await;
    let before = rig.snapshot();
    for method in [LEGACY, NATIVE] {
        let mut params = rig.params(method, "title", "wrong");
        params[if method == LEGACY {
            "replacements"
        } else {
            "matches"
        }] = json!([]);
        no_writer(call(&mut reader, &mut writer, method, params).await);
    }
    assert_eq!(rig.snapshot(), before);
}

#[tokio::test]
async fn both_apply_methods_require_authentication_before_any_business_logic() {
    let rig = Rig::start("sw-noauth", false).await;
    let (mut reader, mut writer) = rig.host.connect().await;
    let before = rig.snapshot();
    for method in [LEGACY, NATIVE] {
        error(
            call(
                &mut reader,
                &mut writer,
                method,
                rig.params(method, "title", "wrong"),
            )
            .await,
            ErrorCode::Unauthenticated,
        );
    }
    assert_eq!(rig.snapshot(), before);
}

// Slice38: stored record eligibility is distinct from explicit-target decoding.
const SEARCH_METHODS: [&str; 8] = [
    "files.search_text",
    "files.search_structured",
    "files.replace_text_preview",
    LEGACY,
    "files.replace_preview",
    NATIVE,
    "files.list_project_files",
    "files.search_project_files",
];

fn search_params(method: &str, root: &Path, route: &str) -> Value {
    let mut params = match method {
        "files.search_text" => json!({"query": "title"}),
        "files.search_structured" => json!({"pattern": "title"}),
        "files.replace_text_preview" => json!({"query": "title", "replacement": "name"}),
        LEGACY => json!({"replacements": [{
            "path": root.join("legacy.txt"), "start": 3, "end": 8,
            "matchText": "title", "replacementText": "name",
        }]}),
        "files.replace_preview" => {
            json!({"path": "native.txt", "query": "title", "replacement": "name"})
        }
        NATIVE => json!({"path": "native.txt", "replacement": "name",
            "matches": [{"start": 3, "end": 8, "matchText": "title"}]
        }),
        "files.list_project_files" => json!({}),
        "files.search_project_files" => json!({"query": "native", "extensions": ["txt"]}),
        _ => panic!("unknown search fixture method"),
    };
    match route {
        "id" => params["projectId"] = json!("p"),
        "id-local" => {
            params["projectId"] = json!("p");
            params["target"] = json!({"kind": "local", "path": root});
        }
        "path" | "worktree" => params["projectPath"] = json!(root),
        "target-only" => params["target"] = json!({"kind": "local", "path": root}),
        _ => panic!("unknown search fixture route"),
    }
    params
}

fn saved_location(case: &str, root: &Path) -> (Value, ErrorCode) {
    match case {
        "ssh" => (
            json!({"kind": "ssh", "connectionId": "fixture-only", "remotePath": root}),
            ErrorCode::UnsupportedCapability,
        ),
        "wsl" => (
            json!({"kind": "wsl", "distribution": "fixture-only", "linuxPath": root}),
            ErrorCode::UnsupportedCapability,
        ),
        "null" => (Value::Null, ErrorCode::InvalidArgument),
        "unknown" => (
            json!({"kind": "future-transport", "path": root}),
            ErrorCode::InvalidArgument,
        ),
        _ => panic!("unknown saved search location"),
    }
}

fn write_search_record(rig: &Rig, location: Value) {
    fs::write(
        rig.host.data_dir.join("projects.json"),
        serde_json::to_vec(&json!([{"id": "p", "path": rig.project, "location": location}]))
            .unwrap(),
    )
    .unwrap();
}

fn prepare_search_records(rig: &Rig, location: Value) -> PathBuf {
    write_search_record(rig, location);
    let worktree = rig.host.data_dir.join("worktree");
    fs::create_dir(&worktree).unwrap();
    for name in ["legacy.txt", "native.txt"] {
        fs::copy(rig.project.join(name), worktree.join(name)).unwrap();
    }
    let tasks = rig.host.data_dir.join("projects/p");
    fs::create_dir_all(&tasks).unwrap();
    fs::write(
        tasks.join("tasks.json"),
        serde_json::to_vec(&json!([{"id": "t", "worktreePath": worktree}])).unwrap(),
    )
    .unwrap();
    // Only new tests instrument this existing fake; old fixture bytes stay intact.
    let git = rig.host.data_dir.join("fake-git");
    let original = fs::read_to_string(&git).unwrap();
    let marker = rig.host.data_dir.join("git-invoked");
    assert!(!marker.to_string_lossy().contains('\''));
    fs::write(
        git,
        format!(
            "#!/bin/sh\nprintf invoked > '{}'\n{}",
            marker.display(),
            original.strip_prefix("#!/bin/sh\n").unwrap()
        ),
    )
    .unwrap();
    worktree
}

#[derive(Debug, PartialEq, Eq)]
struct SearchRecordSnapshot {
    project: Snapshot,
    worktree: BTreeMap<String, (Vec<u8>, u32)>,
    worktree_mode: u32,
    tasks: (Vec<u8>, u32),
}

fn search_record_snapshot(rig: &Rig, worktree: &Path) -> SearchRecordSnapshot {
    let tasks = rig.host.data_dir.join("projects/p/tasks.json");
    SearchRecordSnapshot {
        project: rig.snapshot(),
        worktree: fs::read_dir(worktree)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                assert!(entry.file_type().unwrap().is_file());
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    (fs::read(entry.path()).unwrap(), mode(&entry.path())),
                )
            })
            .collect(),
        worktree_mode: mode(worktree),
        tasks: (fs::read(&tasks).unwrap(), mode(&tasks)),
    }
}

fn record_refusal(reply: &Frame, code: ErrorCode, location_detail: bool) -> bool {
    matches!(reply, Frame::Failure(response)
        if response.error.code == code && !response.error.retryable
            && !response.error.message.is_empty()
            && (!location_detail || response.error.detail.as_ref().and_then(|detail| detail.get("field"))
                == Some(&json!("location"))))
}

async fn stored_search_refusals(method: &str) {
    let mut failures = Vec::new();
    for case in ["ssh", "wsl", "null", "unknown"] {
        for route in ["id", "id-local", "path", "target-only", "worktree"] {
            let rig = Rig::start("ss-record", false).await;
            let (location, code) = saved_location(case, &rig.project);
            let worktree = prepare_search_records(&rig, location);
            let selected = if route == "worktree" {
                &worktree
            } else {
                &rig.project
            };
            let (mut reader, mut writer) = rig.connect().await;
            let before = search_record_snapshot(&rig, &worktree);
            let reply = call(
                &mut reader,
                &mut writer,
                method,
                search_params(method, selected, route),
            )
            .await;
            let identified = route.starts_with("id");
            let expected = if identified {
                code
            } else {
                ErrorCode::NotFound
            };
            let changed = search_record_snapshot(&rig, &worktree) != before;
            let ran_git = rig.host.data_dir.join("git-invoked").exists();
            if !record_refusal(&reply, expected, identified) || changed || ran_git {
                failures.push(format!(
                    "{case}/{route}: {reply:?}, changed={changed}, ran_git={ran_git}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{method}: {}", failures.join("; "));
}

macro_rules! stored_search_regression {
    ($name:ident, $method:expr) => {
        #[tokio::test]
        async fn $name() {
            stored_search_refusals($method).await;
        }
    };
}

stored_search_regression!(
    saved_record_text_search_refuses_local_shadow,
    "files.search_text"
);
stored_search_regression!(
    saved_record_structured_search_refuses_local_shadow,
    "files.search_structured"
);
stored_search_regression!(
    saved_record_legacy_preview_refuses_local_shadow,
    "files.replace_text_preview"
);
stored_search_regression!(saved_record_legacy_apply_refuses_local_shadow, LEGACY);
stored_search_regression!(
    saved_record_native_preview_refuses_local_shadow,
    "files.replace_preview"
);
stored_search_regression!(saved_record_native_apply_refuses_local_shadow, NATIVE);
stored_search_regression!(
    saved_record_file_list_refuses_local_shadow,
    "files.list_project_files"
);
stored_search_regression!(
    saved_record_filename_search_refuses_local_shadow,
    "files.search_project_files"
);

fn assert_search_control(method: &str, rig: &Rig, root: &Path, reply: Frame) {
    let value = result(reply);
    match method {
        LEGACY | NATIVE => {
            assert_eq!(
                value,
                json!({"filesChanged": 1, "replacementsApplied": 1, "replacementsSkipped": 0})
            );
            let file = root.join(if method == LEGACY {
                "legacy.txt"
            } else {
                "native.txt"
            });
            assert_eq!(fs::read_to_string(&file).unwrap(), "İ name\n");
            assert_eq!(mode(&file), 0o640);
        }
        "files.replace_preview" => assert_eq!(value["totalMatches"], 1),
        "files.replace_text_preview" => assert_eq!(value["totalMatches"], 2),
        "files.search_project_files" => assert_eq!(value.as_array().unwrap().len(), 1),
        _ => assert_eq!(value.as_array().unwrap().len(), 2),
    }
    assert_eq!(
        rig.host.data_dir.join("git-invoked").exists(),
        matches!(
            method,
            "files.list_project_files" | "files.search_project_files"
        ),
        "fake-Git marker must be live in allowed controls"
    );
}

#[tokio::test]
async fn saved_record_local_and_worktree_controls_keep_all_search_wires() {
    for method in SEARCH_METHODS {
        for route in ["id", "path", "worktree"] {
            let rig = Rig::start("ss-local", false).await;
            let worktree =
                prepare_search_records(&rig, json!({"kind": "local", "path": rig.project}));
            let selected = if route == "worktree" {
                &worktree
            } else {
                &rig.project
            };
            let before_registry = fs::read(rig.host.data_dir.join("projects.json")).unwrap();
            let (mut reader, mut writer) = rig.connect().await;
            assert_search_control(
                method,
                &rig,
                selected,
                call(
                    &mut reader,
                    &mut writer,
                    method,
                    search_params(method, selected, route),
                )
                .await,
            );
            assert_eq!(
                fs::read(rig.host.data_dir.join("projects.json")).unwrap(),
                before_registry
            );
        }
    }
}

#[tokio::test]
async fn saved_record_read_only_searches_refuse_but_applies_still_check_writer_first() {
    let rig = Rig::start("ss-readonly", true).await;
    let worktree = prepare_search_records(&rig, saved_location("ssh", &rig.project).0);
    let before = search_record_snapshot(&rig, &worktree);
    let (mut reader, mut writer) = rig.connect().await;
    let mut failures = Vec::new();
    for method in SEARCH_METHODS {
        let reply = call(
            &mut reader,
            &mut writer,
            method,
            search_params(method, &rig.project, "id"),
        )
        .await;
        let apply = matches!(method, LEGACY | NATIVE);
        if !record_refusal(
            &reply,
            if apply {
                ErrorCode::IoError
            } else {
                ErrorCode::UnsupportedCapability
            },
            !apply,
        ) {
            failures.push(format!("{method}: {reply:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
    assert_eq!(search_record_snapshot(&rig, &worktree), before);
    assert!(!rig.host.data_dir.join("git-invoked").exists());
}

#[tokio::test]
async fn saved_record_unrelated_bad_candidates_skip_poison_tasks_and_keep_local_roots() {
    for method in SEARCH_METHODS {
        let rig = Rig::start("ss-poison", false).await;
        prepare_search_records(&rig, json!({"kind": "local", "path": rig.project}));
        let bad_root = rig.host.data_dir.join("bad-root");
        fs::create_dir(&bad_root).unwrap();
        let bad_tasks = rig.host.data_dir.join("projects/bad");
        fs::create_dir_all(&bad_tasks).unwrap();
        fs::write(bad_tasks.join("tasks.json"), b"broken tasks").unwrap();
        fs::write(
            rig.host.data_dir.join("projects.json"),
            serde_json::to_vec(&json!([
                {"id": "bad", "path": bad_root, "location": {"kind": "future-transport"}},
                {"id": "p", "path": rig.project},
            ]))
            .unwrap(),
        )
        .unwrap();
        let (mut reader, mut writer) = rig.connect().await;
        assert_search_control(
            method,
            &rig,
            &rig.project,
            call(
                &mut reader,
                &mut writer,
                method,
                search_params(method, &rig.project, "path"),
            )
            .await,
        );
        assert_eq!(
            fs::read(bad_tasks.join("tasks.json")).unwrap(),
            b"broken tasks"
        );
        assert!(fs::read_dir(bad_root).unwrap().next().is_none());
    }
}

#[tokio::test]
async fn saved_record_remote_does_not_override_independent_vault_registration_or_revocation() {
    let mut failures = Vec::new();
    for method in SEARCH_METHODS {
        let rig = Rig::start("ss-vault", false).await;
        let worktree = prepare_search_records(&rig, saved_location("ssh", &rig.project).0);
        let (mut reader, mut writer) = rig.connect().await;
        let params = search_params(method, &rig.project, "path");
        let before = search_record_snapshot(&rig, &worktree);
        let reply = call(&mut reader, &mut writer, method, params.clone()).await;
        if !record_refusal(&reply, ErrorCode::NotFound, false)
            || search_record_snapshot(&rig, &worktree) != before
        {
            failures.push(format!("{method}: unregistered {reply:?}"));
        }
        // Reset only scratch fixtures after red, so the independent allowed stage runs.
        for name in ["legacy.txt", "native.txt"] {
            fs::write(rig.project.join(name), "İ title\n").unwrap();
        }
        fs::remove_file(rig.host.data_dir.join("git-invoked")).ok();
        result(
            call(
                &mut reader,
                &mut writer,
                "notebook.register_vault",
                json!({"path": rig.project}),
            )
            .await,
        );
        let before = search_record_snapshot(&rig, &worktree);
        let id_reply = call(
            &mut reader,
            &mut writer,
            method,
            search_params(method, &rig.project, "id"),
        )
        .await;
        if !record_refusal(&id_reply, ErrorCode::UnsupportedCapability, true)
            || search_record_snapshot(&rig, &worktree) != before
        {
            failures.push(format!("{method}: identified remote {id_reply:?}"));
        }
        for name in ["legacy.txt", "native.txt"] {
            fs::write(rig.project.join(name), "İ title\n").unwrap();
        }
        assert_search_control(
            method,
            &rig,
            &rig.project,
            call(&mut reader, &mut writer, method, params.clone()).await,
        );
        result(
            call(
                &mut reader,
                &mut writer,
                "notebook.unregister_vault",
                json!({"path": rig.project}),
            )
            .await,
        );
        fs::remove_file(rig.host.data_dir.join("git-invoked")).ok();
        let before = search_record_snapshot(&rig, &worktree);
        let reply = call(&mut reader, &mut writer, method, params).await;
        if !record_refusal(&reply, ErrorCode::NotFound, false)
            || search_record_snapshot(&rig, &worktree) != before
            || rig.host.data_dir.join("git-invoked").exists()
        {
            failures.push(format!("{method}: revoked {reply:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
}

#[tokio::test]
async fn saved_record_changes_are_observed_on_the_same_search_connection() {
    let rig = Rig::start("ss-refresh", false).await;
    let worktree = prepare_search_records(&rig, json!({"kind": "local", "path": rig.project}));
    let (mut reader, mut writer) = rig.connect().await;
    assert_eq!(
        result(
            call(
                &mut reader,
                &mut writer,
                "files.search_text",
                search_params("files.search_text", &rig.project, "id")
            )
            .await
        )
        .as_array()
        .unwrap()
        .len(),
        2
    );
    write_search_record(&rig, saved_location("ssh", &rig.project).0);
    let before = search_record_snapshot(&rig, &worktree);
    let reply = call(
        &mut reader,
        &mut writer,
        NATIVE,
        search_params(NATIVE, &rig.project, "id"),
    )
    .await;
    assert!(
        record_refusal(&reply, ErrorCode::UnsupportedCapability, true),
        "{reply:?}"
    );
    assert_eq!(search_record_snapshot(&rig, &worktree), before);
    write_search_record(&rig, json!({"kind": "local", "path": rig.project}));
    assert_search_control(
        NATIVE,
        &rig,
        &rig.project,
        call(
            &mut reader,
            &mut writer,
            NATIVE,
            search_params(NATIVE, &rig.project, "id"),
        )
        .await,
    );
}
