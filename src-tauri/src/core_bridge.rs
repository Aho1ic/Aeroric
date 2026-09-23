//! Where the shipping client already reuses the native core (Task P2-02 Action 5).
//!
//! Deliberately tiny. Only functions that are pure and provably identical on both sides
//! are delegated, and the writer role does **not** move here: `storage.rs` still owns
//! every save, takes no lease, and P2-05 is the single atomic switch of the writer.
//! Running two writers against one data directory is what plan INV-03 forbids, so this
//! module must not look like a writer even partially.
//!
//! The tests below are the reason the delegation is safe: they pin the legacy behaviour
//! against the core implementation, so a future edit to either side goes red here rather
//! than turning into a directory both clients read differently.

use aeroric_core::AppPaths;

/// Which ids may become a path component. Delegates to the core so the two clients
/// cannot disagree about what is safe to join onto `~/.aeroric/projects/`.
///
/// The message stays the legacy one, byte for byte: the frontend matches on it.
pub fn validate_id(id: &str, label: &'static str) -> Result<(), String> {
    AppPaths::validate_id(id, label).map_err(|_| format!("Invalid {label} id"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Everything `validate_storage_id` has to say no to, plus the shapes that must
    /// survive: ids here come from the UI and from session files on disk.
    const REFUSED: &[&str] = &[
        "", "   ", "../evil", "a/b", "a\\b", "a;b", "$(id)", "a b", ".", "..", "a\u{8}b",
        "id\u{0}", "项目", "id.json",
    ];
    const ACCEPTED: &[&str] = &["1783647251756", "task_1-safe", "A-9_b", "0"];

    #[test]
    fn the_legacy_validator_and_the_core_agree_on_every_spelling() {
        for id in REFUSED {
            assert!(
                validate_id(id, "task").is_err(),
                "core accepted {id:?} that the shipping client refuses"
            );
            assert!(
                crate::storage::validate_storage_id(id, "task").is_err(),
                "legacy path must stay closed for {id:?}"
            );
        }
        for id in ACCEPTED {
            assert!(validate_id(id, "project").is_ok(), "{id:?} must pass");
            assert!(crate::storage::validate_storage_id(id, "project").is_ok());
        }
    }

    #[test]
    fn a_delegated_refusal_keeps_the_message_the_frontend_matches() {
        assert_eq!(
            validate_id("../evil", "project"),
            Err("Invalid project id".to_string())
        );
        assert_eq!(
            crate::storage::validate_storage_id("../evil", "task"),
            Err("Invalid task id".to_string())
        );
    }

    /// The byte-level reason P2-05 may hand the writer role over at all: given the same
    /// content, the two writers leave the same file behind.
    #[test]
    fn both_writers_produce_the_same_file_bytes_and_mode() {
        let root = std::env::temp_dir().join(format!(
            "aeroric-core-bridge-{}-{}",
            std::process::id(),
            std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let content = "[\n  { \"id\": \"p1\", \"name\": \"中文 路径\" }\n]\n";
        let legacy = root.join("legacy.json");
        let core = root.join("core.json");
        crate::storage::atomic_write_private_bytes(&legacy, content.as_bytes()).unwrap();
        aeroric_core::storage::atomic::write_atomic(
            &core,
            content.as_bytes(),
            aeroric_core::storage::FileMode::Private,
        )
        .unwrap();
        assert_eq!(
            std::fs::read(&legacy).unwrap(),
            std::fs::read(&core).unwrap(),
            "one directory, two readers: the bytes must be interchangeable"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode =
                |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(
                mode(&legacy),
                mode(&core),
                "and neither may leave the file group-readable"
            );
            assert_eq!(mode(&core), 0o600);
        }
        // Neither writer leaves a temp file behind on success.
        let leftovers: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|name| name.starts_with('.'))
            .collect();
        assert!(leftovers.is_empty(), "stray temp files: {leftovers:?}");
        std::fs::remove_dir_all(&root).ok();
    }

    /// Every field the shipping client's `Task` DTO has, populated. Constructed field by
    /// field on purpose: a new legacy field makes this fail to *compile* until the fixture
    /// carries it, which is the drift detector P2-05 will lean on.
    fn legacy_task_fixture() -> crate::storage::Task {
        crate::storage::Task {
            id: "t1".into(),
            project_id: "p1".into(),
            name: Some("中文 名字".into()),
            prompt: "keep every field".into(),
            agent: "omp".into(),
            permission_mode: "ask".into(),
            selected_model: Some("gpt-5".into()),
            dsh_agent_preset: Some("reviewer".into()),
            reasoning_effort: Some("ultra".into()),
            speed: Some("fast".into()),
            status: "interrupted".into(),
            created_at: 1_700_000_000_000,
            attention_requested_at: Some(1_700_000_000_500),
            completed_at: Some(1_700_000_001_000),
            archived_at: Some(1_700_000_002_000),
            claude_session_id: Some("c-1".into()),
            claude_session_path: Some("/s/c.jsonl".into()),
            codex_session_id: Some("x-1".into()),
            codex_session_path: Some("/s/x.jsonl".into()),
            dsh_session_id: Some("d-1".into()),
            dsh_session_path: Some("/s/d.jsonl".into()),
            omp_session_id: Some("o-1".into()),
            omp_session_path: Some("/s/o.jsonl".into()),
            session_agent: Some("omp".into()),
            session_codex_like: Some(true),
            session_family: Some("omp".into()),
            starred: Some(true),
            failure_reason: Some("signal 9".into()),
            worktree_path: Some("/w/t1".into()),
            worktree_branch: Some("aeroric/t1".into()),
            base_branch: Some("main".into()),
            worktree_discarded: Some(true),
            additions: Some(12),
            deletions: Some(3),
        }
    }

    /// P2-02 acceptance, the direction that actually loses data: a file written by the
    /// old DTO must read identically through the core, and what the core writes back must
    /// still be readable — byte for byte — by the old DTO. Runs entirely in a temp
    /// directory; the real `~/.aeroric` is never opened.
    #[test]
    fn the_legacy_dto_and_the_core_reader_round_trip_the_same_file() {
        use aeroric_core::{
            AppPaths, Clock, CoreContext, FixedClock, LeaseOwner, StorageLocation,
            StorageLocationProbe, VolumeKind,
        };
        use std::sync::Arc;

        /// The volume answer for this test's own temp directory. The shipping adapter does
        /// not probe yet — that is P2-05's blocking item (ADR-010).
        #[derive(Clone, Copy)]
        struct TempLocalVolume;

        impl StorageLocationProbe for TempLocalVolume {
            fn probe(&self, data_dir: &std::path::Path) -> StorageLocation {
                StorageLocation::new(data_dir, VolumeKind::Local)
            }
        }

        let task = legacy_task_fixture();
        let legacy_json = {
            let mut value = serde_json::to_value(&task).unwrap();
            // A key neither client knows about: the core must carry it through untouched.
            value.as_object_mut().unwrap().insert(
                "aFieldNeitherSideModels".into(),
                serde_json::json!([1, 2, 3]),
            );
            let text = serde_json::to_string_pretty(&[value]).unwrap();
            format!("{text}\n")
        };

        let root = std::env::temp_dir().join(format!(
            "aeroric-legacy-dto-{}-{}",
            std::process::id(),
            aeroric_core::SystemClock.now_ms()
        ));
        std::fs::remove_dir_all(&root).ok();
        let paths = AppPaths::with_data_dir(&root, Some(&root.join("managed")));
        let context = CoreContext::acquire(
            paths.clone(),
            Arc::new(FixedClock::at(1_700_000_000_000)),
            &LeaseOwner {
                application: "aeroric-test".into(),
                version: "0.0.0-test".into(),
            },
            // P2-05 is where the Tauri adapter grows a real `statvfs` probe; a test that
            // only asks "do the two readers agree" supplies the answer for its own temp
            // directory instead of pretending the product already probes.
            &TempLocalVolume,
        )
        .expect("lease on a temp directory");
        std::fs::create_dir_all(paths.project_dir("p1").unwrap()).unwrap();
        let file = paths.tasks_file("p1").unwrap();
        std::fs::write(&file, legacy_json.as_bytes()).unwrap();

        let loaded = aeroric_core::storage::load_tasks(&context, "p1").expect("core reads it");
        assert_eq!(loaded.len(), 1);
        let written: serde_json::Value = serde_json::from_str(&legacy_json).unwrap();
        let read_back = serde_json::to_value(&loaded[0]).unwrap();
        assert_eq!(
            read_back,
            written.as_array().expect("array fixture")[0],
            "the core reader may not drop or rewrite a single key"
        );
        for key in [
            "claudeSessionId",
            "codexSessionId",
            "dshSessionId",
            "ompSessionId",
            "sessionFamily",
            "worktreePath",
            "worktreeDiscarded",
            "archivedAt",
            "aFieldNeitherSideModels",
        ] {
            assert!(
                read_back.get(key).is_some(),
                "{key} did not survive the read"
            );
        }

        aeroric_core::storage::save_tasks(&context, "p1", &loaded).expect("core writes it");
        let on_disk = std::fs::read_to_string(&file).unwrap();
        assert_eq!(
            on_disk, legacy_json,
            "a core save must be invisible to the client that wrote the file"
        );
        let via_legacy: Vec<crate::storage::Task> =
            serde_json::from_str(&on_disk).expect("the legacy DTO parses core output");
        assert_eq!(
            serde_json::to_value(&via_legacy[0]).unwrap(),
            serde_json::to_value(&task).unwrap(),
            "every DTO field must come back with the same value"
        );
        std::fs::remove_dir_all(root).ok();
    }
}
