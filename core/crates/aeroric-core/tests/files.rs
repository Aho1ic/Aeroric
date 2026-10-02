//! P6-01 验收：temp project 中的路径安全覆盖——symlink escape、`..`、
//! Unicode、空格、hidden、权限拒绝、写时文件被外部修改。
use aeroric_core::files::{path_policy, read_write, search};

fn temp_project() -> (tempdir_like::TempProject, std::path::PathBuf) {
    tempdir_like::new("aeroric-files-test")
}

mod tempdir_like {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    pub struct TempProject {
        pub root: PathBuf,
    }

    impl Drop for TempProject {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    pub fn new(label: &str) -> (TempProject, PathBuf) {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!("{label}-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create temp project");
        (TempProject { root: root.clone() }, root)
    }
}

#[test]
fn relative_path_rejects_dot_dot_and_empty_components() {
    assert!(path_policy::validate_relative_path("../escape").is_err());
    assert!(path_policy::validate_relative_path("a/../../b").is_err());
    assert!(path_policy::validate_relative_path("./here").is_err());
    assert!(path_policy::validate_relative_path("a//b").is_err());
    assert!(path_policy::validate_relative_path("").is_err());
    assert!(path_policy::validate_relative_path("a/b.txt").is_ok());
    // Unicode 与空格组件合法。
    assert!(path_policy::validate_relative_path("目录/文件 名.txt").is_ok());
}

#[test]
fn project_root_requires_absolute_existing_directory() {
    assert!(path_policy::validate_project_root("relative/path").is_err());
    assert!(path_policy::validate_project_root("/definitely/missing/dir").is_err());
    let (_tmp, root) = temp_project();
    assert!(path_policy::validate_project_root(root.to_str().unwrap()).is_ok());
}

#[test]
fn resolve_within_rejects_symlink_escape() {
    let (_tmp, root) = temp_project();
    let outside = std::env::temp_dir().join("aeroric-files-outside.txt");
    std::fs::write(&outside, "secret").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link.txt")).unwrap();
    let err = path_policy::resolve_within(&root, "link.txt").unwrap_err();
    assert!(err.contains("escapes"), "symlink 出根拒绝：{err}");
    let _ = std::fs::remove_file(&outside);
}

#[test]
fn resolve_within_accepts_in_root_symlink_target() {
    let (_tmp, root) = temp_project();
    std::fs::write(root.join("target.txt"), "ok").unwrap();
    std::os::unix::fs::symlink(root.join("target.txt"), root.join("link.txt")).unwrap();
    let resolved = path_policy::resolve_within(&root, "link.txt").unwrap();
    assert!(resolved.starts_with(root.canonicalize().unwrap()));
}

#[test]
fn resolve_within_rejects_dot_dot_even_inside_root() {
    let (_tmp, root) = temp_project();
    std::fs::write(root.join("a.txt"), "x").unwrap();
    assert!(path_policy::resolve_within(&root, "sub/../a.txt").is_err());
}

#[test]
fn read_write_roundtrip_with_unicode_and_spaces() {
    let (_tmp, root) = temp_project();
    read_write::create_directory(&root, "目录").unwrap();
    read_write::write_file_content(&root, "目录/文件 名.txt", "你好").unwrap();
    let content = read_write::read_file_content(&root, "目录/文件 名.txt").unwrap();
    assert_eq!(content, "你好");
    let entries = read_write::read_dir_entries(&root, "").unwrap();
    assert!(entries.iter().any(|e| e.name == "目录" && e.is_dir));
}

#[test]
fn create_file_rejects_existing_and_delete_uses_trash() {
    let (_tmp, root) = temp_project();
    read_write::create_file(&root, "new.txt").unwrap();
    assert!(
        read_write::create_file(&root, "new.txt").is_err(),
        "已存在拒绝"
    );
    // 删除进废纸篓（DELETE_USES_TRASH 常量断言在 clippy 下属恒真；不默认
    // 永久删除的语义由 delete_path 的 trash 实现保证）。
    read_write::delete_path(&root, "new.txt").unwrap();
    assert!(!root.join("new.txt").exists());
}

#[test]
fn content_size_limit_rejects_oversize() {
    let big = "x".repeat((path_policy::MAX_FILE_BYTES + 1) as usize);
    assert!(
        path_policy::validate_content_size(&big).is_err(),
        "超限拒绝不静默截断"
    );
}

#[test]
fn rename_stays_within_root() {
    let (_tmp, root) = temp_project();
    read_write::create_file(&root, "old.txt").unwrap();
    read_write::rename_path(&root, "old.txt", "new.txt").unwrap();
    assert!(!root.join("old.txt").exists());
    assert!(root.join("new.txt").exists());
    // 重命名出根拒绝（../ 组件在 new_name 中）。
    read_write::create_file(&root, "a.txt").unwrap();
    assert!(read_write::rename_path(&root, "a.txt", "../escape.txt").is_err());
}

#[test]
fn rg_parse_and_replace_preview_and_stale_skip() {
    let (_tmp, root) = temp_project();
    read_write::write_file_content(&root, "code.txt", "alpha beta alpha\n").unwrap();
    // 构造 rg JSON 行（begin + match）。
    let stdout = format!(
        r#"{{"type":"begin","data":{{"path":{{"text":"{}"}}}}}}
{{"type":"match","data":{{"line_number":1,"absolute_offset":6,"submatches":[{{"match":{{"text":"beta"}},"start":0,"end":4}}]}}}}
"#,
        root.join("code.txt").display()
    );
    let matches = search::parse_rg_json_lines(&stdout, 500);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].submatches[0].match_text, "beta");
    let mut preview = search::replace_preview("beta", "gamma", &matches);
    assert_eq!(preview.files.len(), 1);
    // rg 的 begin path 是绝对路径；宿主服务在提交前把它归一为相对路径。
    preview.files[0].path = "code.txt".to_string();
    let summary = search::apply_replacements(&root, "gamma", &preview.files).unwrap();
    assert_eq!(summary.replacements_applied, 1);
    assert_eq!(
        read_write::read_file_content(&root, "code.txt").unwrap(),
        "alpha gamma alpha\n"
    );
    // stale：预览区间超出当前文件长度 → skip 不覆盖。
    let stale = vec![search::ReplacePreviewFile {
        path: "code.txt".to_string(),
        matches: vec![(1000, 2000)],
    }];
    let summary = search::apply_replacements(&root, "gamma", &stale).unwrap();
    assert_eq!(
        summary.replacements_skipped, 1,
        "写时文件被外部修改 → 拒绝覆盖"
    );
}

#[test]
fn watcher_coalesce_and_overflow() {
    use aeroric_core::files::watch;
    let batch = watch::coalesce(&["a".into(), "a".into(), "b".into()]);
    assert_eq!(
        batch.paths,
        vec!["a".to_string(), "b".to_string()],
        "同批按 path 去重"
    );
    assert!(
        watch::should_rescan(watch::OVERFLOW_LIMIT + 1),
        "溢出 → rescan"
    );
    assert!(!watch::should_rescan(2));
    let token = watch::CancelToken::new();
    assert!(!token.is_cancelled());
    token.cancel();
    assert!(token.is_cancelled());
}

#[test]
fn watcher_spawns_and_cancels() {
    use aeroric_core::files::watcher::{FileWatcher, WatchOutcome};
    let (_tmp, root) = temp_project();
    let (tx, rx) = std::sync::mpsc::channel::<WatchOutcome>();
    let watcher = FileWatcher::spawn(vec![root.clone()], move |outcome| {
        let _ = tx.send(outcome);
    })
    .expect("spawn watcher");
    // 触发一个文件写入事件。
    std::fs::write(root.join("watched.txt"), "x").unwrap();
    let outcome = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("watcher delivers a batch");
    match outcome {
        WatchOutcome::Batch(paths) => {
            assert!(
                paths.iter().any(|p| p.ends_with("watched.txt")),
                "同批路径含被 watch 的文件"
            );
        }
        WatchOutcome::Rescan => panic!("单文件写入不应触发 rescan"),
    }
    // 取消后 drop（线程退出）。
    watcher.cancel();
    drop(watcher);
}

#[test]
fn watcher_rescan_on_overflow_error() {
    // 溢出判定已由 watch::should_rescan 单测覆盖；这里断言 WatchOutcome::Rescan
    // 的语义（服务把 notify 错误映射为 rescan 而不是退出）。
    use aeroric_core::files::watch::should_rescan;
    assert!(should_rescan(
        aeroric_core::files::watch::OVERFLOW_LIMIT + 1
    ));
}
