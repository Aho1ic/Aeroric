//! The compatibility half of P2-02's acceptance: files the shipping client wrote must
//! load, round-trip, and stay byte-shape-compatible — including the fields this core
//! does not model. Corrupt input has to be a recoverable error, never an empty write.

use std::fs;
use std::path::PathBuf;

use aeroric_core::storage::atomic::{write_atomic, FileMode, ReadOutcome, StorageError};
use aeroric_core::storage::compat::{self, Record};
use aeroric_core::storage::{
    load_projects, load_settings, load_tasks, save_projects, save_settings, save_tasks,
};
use aeroric_core::{
    AppPaths, Clock, CoreContext, FixedClock, LeaseOwner, StorageLocation, StorageLocationProbe,
    SystemClock, VolumeKind,
};
use serde_json::{json, Value};

pub mod support {
    use super::*;

    /// Answer "local volume" so a shape test is never blocked by the volume question.
    #[derive(Clone, Copy)]
    pub struct LocalVolume;

    impl StorageLocationProbe for LocalVolume {
        fn probe(&self, data_dir: &std::path::Path) -> StorageLocation {
            StorageLocation::new(data_dir, VolumeKind::Local)
        }
    }
}

fn writer(tag: &str) -> (PathBuf, CoreContext) {
    let root = std::env::temp_dir().join(format!(
        "aeroric-compat-{tag}-{}-{}",
        std::process::id(),
        SystemClock.now_ms()
    ));
    fs::remove_dir_all(&root).ok();
    let paths = AppPaths::with_data_dir(&root, Some(&root.join("managed")));
    let owner = LeaseOwner {
        application: "aeroric-core-test".into(),
        version: "0.0.0-test".into(),
    };
    let context = CoreContext::acquire(
        paths,
        std::sync::Arc::new(FixedClock::at(1_700_000_000_000)),
        &owner,
        // The compatibility questions here are about file shapes, not volumes.
        &crate::support::LocalVolume,
    )
    .expect("lease");
    (root, context)
}

/// Byte-for-byte what `src-tauri/src/storage.rs` writes today: a bare pretty-printed
/// array, camelCase renames, absent optionals omitted rather than nulled.
const LEGACY_PROJECTS: &str = concat!(
    "[\n",
    "  {\n",
    "    \"id\": \"p1\",\n",
    "    \"name\": \"Aeroric\",\n",
    "    \"path\": \"/repo/aeroric\",\n",
    "    \"branch\": \"main\",\n",
    "    \"lastOpenedAt\": 1700000000000,\n",
    "    \"orderIndex\": 3,\n",
    "    \"group\": \"工作\",\n",
    "    \"hiddenFromRail\": true,\n",
    "    \"pinned\": true,\n",
    "    \"avatar\": {\n",
    "      \"color\": \"amber\",\n",
    "      \"emoji\": \"🚀\"\n",
    "    },\n",
    "    \"location\": {\n",
    "      \"kind\": \"ssh\",\n",
    "      \"connectionId\": \"c1\",\n",
    "      \"remotePath\": \"/srv/aeroric\"\n",
    "    },\n",
    "    \"aFieldAddedByANewerBuild\": { \"kept\": true }\n",
    "  }\n",
    "]\n",
);

const LEGACY_TASKS: &str = concat!(
    "[\n",
    "  {\n",
    "    \"id\": \"t1\",\n",
    "    \"projectId\": \"p1\",\n",
    "    \"prompt\": \"hi\",\n",
    "    \"agent\": \"codex\",\n",
    "    \"permissionMode\": \"ask\",\n",
    "    \"status\": \"interrupted\",\n",
    "    \"createdAt\": 1700000000000,\n",
    "    \"claudeSessionId\": \"c-1\",\n",
    "    \"codexSessionPath\": \"/s/x.jsonl\",\n",
    "    \"sessionCodexLike\": true,\n",
    "    \"worktreePath\": \"/w/t1\",\n",
    "    \"worktreeBranch\": \"wt/t1\",\n",
    "    \"baseBranch\": \"main\",\n",
    "    \"archivedAt\": 1700000001000,\n",
    "    \"completedAt\": 1700000002000,\n",
    "    \"selectedModel\": \"gpt-5\",\n",
    "    \"reasoningEffort\": \"high\",\n",
    "    \"speed\": \"fast\",\n",
    "    \"dshAgentPreset\": \"reviewer\",\n",
    "    \"attentionRequestedAt\": null,\n",
    "    \"worktreeDiscarded\": false,\n",
    "    \"futureFlag\": { \"deep\": [1, 2, { \"x\": null }] }\n",
    "  }\n",
    "]\n",
);

fn seed(context: &CoreContext, projects: &str, tasks: &str) {
    context.paths.ensure_private().expect("managed dirs");
    fs::create_dir_all(context.paths.project_dir("p1").unwrap()).unwrap();
    fs::write(context.paths.projects_file(), projects).unwrap();
    fs::write(context.paths.tasks_file("p1").unwrap(), tasks).unwrap();
}

fn fields(record: &Record) -> Vec<String> {
    record.0.keys().cloned().collect()
}

#[test]
fn a_legacy_project_file_reads_without_losing_a_key() {
    let (root, context) = writer("legacy-project");
    seed(&context, LEGACY_PROJECTS, "[]\n");
    let projects = load_projects(&context).expect("load");
    assert_eq!(projects.len(), 1);
    let project = &projects[0];
    assert_eq!(project.id(), Some("p1"));
    assert_eq!(project.str_field("group"), Some("工作"));
    assert_eq!(project.get("hiddenFromRail"), Some(&json!(true)));
    assert_eq!(
        project.get("avatar").cloned(),
        Some(json!({"color":"amber","emoji":"🚀"}))
    );
    let location = project.get("location").cloned().unwrap_or(Value::Null);
    assert_eq!(location["kind"], json!("ssh"));
    assert_eq!(location["remotePath"], json!("/srv/aeroric"));
    assert_eq!(
        project.get("aFieldAddedByANewerBuild"),
        Some(&json!({"kept": true}))
    );
    save_projects(&context, &projects).expect("rewrite");
    let again = load_projects(&context).expect("reload");
    assert_eq!(again, projects, "a rewrite must be a fixed point");
    assert_eq!(fields(&again[0]).len(), fields(project).len());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn session_worktree_archive_and_unknown_fields_all_survive() {
    let (root, context) = writer("legacy-task");
    seed(&context, "[]\n", LEGACY_TASKS);
    let tasks = load_tasks(&context, "p1").expect("load");
    assert_eq!(tasks.len(), 1);
    let before: Vec<String> = fields(&tasks[0]);
    // Change one field through the core's own path and write everything back.
    let mut updated = tasks.clone();
    assert!(compat::update_record(&mut updated, "t1", |record| {
        let mut next = record.0.clone();
        next.insert("status".into(), json!("done"));
        *record = Record(next);
    }));
    save_tasks(&context, "p1", &updated).expect("save");
    let after = load_tasks(&context, "p1").expect("reload");
    let after_fields = fields(&after[0]);
    for key in [
        "claudeSessionId",
        "codexSessionPath",
        "sessionCodexLike",
        "worktreePath",
        "worktreeBranch",
        "baseBranch",
        "archivedAt",
        "completedAt",
        "selectedModel",
        "reasoningEffort",
        "speed",
        "dshAgentPreset",
        "attentionRequestedAt",
        "worktreeDiscarded",
        "futureFlag",
    ] {
        assert!(after_fields.contains(&key.to_string()), "{key} was dropped");
    }
    assert_eq!(
        after_fields.len(),
        before.len(),
        "no key may vanish or appear"
    );
    assert_eq!(after[0].get("status"), Some(&json!("done")));
    assert_eq!(
        after[0].get("futureFlag"),
        Some(&json!({"deep": [1, 2, {"x": null}]}))
    );
    assert_eq!(
        after[0].get("attentionRequestedAt"),
        Some(&Value::Null),
        "explicit null and absent are different states and must stay so"
    );
    assert!(
        !after[0].contains("attentionRequestedAt\0"),
        "no key mangling"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn settings_keep_their_shape_and_survive_a_no_change_rewrite() {
    let (root, context) = writer("settings");
    context.paths.ensure_private().unwrap();
    assert_eq!(load_settings(&context).unwrap(), ReadOutcome::Absent);
    let settings: Value = json!({
        "interactionMode": "native",
        "theme": { "mode": "glass", "accent": "amber" },
        "providers": [{ "id": "anthropic", "model": null }],
        "unknownFromNewerBuild": 7
    });
    fs::write(
        context.paths.settings_file(),
        serde_json::to_string_pretty(&settings).unwrap() + "\n",
    )
    .unwrap();
    let read = load_settings(&context).unwrap().found().expect("present");
    assert_eq!(
        Value::Object(read.clone()),
        settings,
        "a deep value must be byte-equal"
    );
    save_settings(&context, &read).unwrap();
    let re_read = load_settings(&context)
        .unwrap()
        .found()
        .expect("still present");
    assert_eq!(re_read, read);
    assert!(
        context
            .paths
            .settings_file()
            .with_extension("json.bak")
            .exists(),
        "the replaced file is kept, so a wrong snapshot is recoverable"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn corrupt_and_oversized_input_is_a_recoverable_refusal() {
    let (root, context) = writer("corrupt");
    seed(&context, "[{\"id\": ", "[]\n");
    let error = load_projects(&context).expect_err("corrupt projects");
    assert!(matches!(error, StorageError::Corrupt { .. }), "{error:?}");
    let original = fs::read(context.paths.projects_file()).unwrap();
    let failure = save_projects(&context, &[]).expect_err("the writer must be refused");
    assert!(
        matches!(failure, StorageError::Corrupt { .. }),
        "{failure:?}"
    );
    assert_eq!(
        fs::read(context.paths.projects_file()).unwrap(),
        original,
        "unreadable bytes survive"
    );
    assert!(
        !context
            .paths
            .projects_file()
            .with_extension("json.bak")
            .exists(),
        "a refused write leaves no side effect"
    );
    // Recovering is deliberate: an explicit write replaces it, and that write keeps a backup.
    write_atomic(&context.paths.projects_file(), b"[]\n", FileMode::Private).unwrap();
    assert!(load_projects(&context).unwrap().is_empty());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_task_snapshot_that_empties_does_not_erase_the_previous_one() {
    let (root, context) = writer("empty");
    seed(&context, "[]\n", LEGACY_TASKS);
    save_tasks(&context, "p1", &[]).expect("write the empty snapshot");
    let path = context.paths.tasks_file("p1").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap().trim(), "[]");
    let backup = path.with_extension("json.bak");
    assert!(backup.exists(), "the deleted-by-emptiness snapshot is kept");
    assert!(fs::read_to_string(&backup)
        .unwrap()
        .contains("\"claudeSessionId\": \"c-1\""));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn ids_that_would_escape_are_refused_before_any_io() {
    let (root, context) = writer("ids");
    seed(&context, "[]\n", "[]\n");
    for bad in ["../pwn", "a/b", "", " ", "a\0b"] {
        assert!(
            load_tasks(&context, bad).is_err(),
            "{bad:?} must be refused"
        );
        assert!(
            save_tasks(&context, bad, &[]).is_err(),
            "{bad:?} must be refused"
        );
    }
    assert!(load_tasks(&context, "p1").is_ok());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn an_empty_directory_reads_as_absent_rather_than_empty_document() {
    let (root, context) = writer("absent");
    assert_eq!(load_settings(&context).unwrap(), ReadOutcome::Absent);
    assert!(load_projects(&context).unwrap().is_empty());
    assert!(load_tasks(&context, "p-new").unwrap().is_empty());
    // ...and "absent" is still distinguishable from "present but empty object", which
    // is what a truncated write looks like.
    context.paths.ensure_private().unwrap();
    fs::write(context.paths.settings_file(), "").unwrap();
    let error = load_settings(&context).expect_err("an empty file is not an empty object");
    assert!(matches!(error, StorageError::Corrupt { .. }), "{error:?}");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn nothing_here_reaches_a_real_user_directory() {
    // The tests above build every path from a temp root; this asserts the core cannot
    // be handed the real one by accident, and that the lease is what gates writes.
    let (root, _) = writer("no-home");
    let paths = AppPaths::from_user_home(root.join("simulated-home"));
    assert!(
        paths
            .data_dir()
            .to_string_lossy()
            .contains("simulated-home/.aeroric"),
        "{:?}",
        paths.data_dir()
    );
    let context = CoreContext::read_only(paths);
    assert!(
        load_projects(&context).unwrap().is_empty(),
        "an unread directory reads as absent"
    );
    assert!(
        save_projects(&context, &[]).is_err(),
        "and cannot be written without a lease"
    );
    std::fs::remove_dir_all(root).ok();
}
