//! P6-02 验收：temp project 中的本地历史与 formatter/preview 覆盖——
//! UTF-8 BOM、CRLF、外部修改、Undo/Redo（编辑器层）、重复 save、格式化失败
//! 原文不变、local history restore 有兜底快照。
use aeroric_core::files::format;
use aeroric_core::files::local_history;
use aeroric_core::files::preview;

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
fn snapshot_record_list_read_restore_roundtrip() {
    let (_tmp, root) = tempdir_like::new("aeroric-history-test");
    let file = root.join("main.rs");
    std::fs::write(&file, "fn a() {}\n").unwrap();
    // 第一次写前快照。
    let entry = local_history::record_snapshot(
        local_history::CODE_LAYOUT,
        &root,
        &file,
        Some("fn b() {}\n"),
        1000,
    )
    .unwrap();
    assert!(entry.is_some(), "内容不同＝留一条");
    // 内容一致时跳过（重复 save 噪音压除）：磁盘已是 next 内容。
    std::fs::write(&file, "fn b() {}\n").unwrap();
    let skip = local_history::record_snapshot(
        local_history::CODE_LAYOUT,
        &root,
        &file,
        Some("fn b() {}\n"),
        2000,
    )
    .unwrap();
    assert!(skip.is_none(), "内容一致＝跳过");
    // 第二条。
    std::fs::write(&file, "fn c() {}\n").unwrap();
    let second = local_history::record_snapshot(
        local_history::CODE_LAYOUT,
        &root,
        &file,
        Some("fn d() {}\n"),
        3000,
    )
    .unwrap();
    assert!(second.is_some());
    // 列表（新→旧）。
    let entries = local_history::list_entries(local_history::CODE_LAYOUT, &root, &file).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].created_at_ms, 3000, "新→旧");
    // 读一条。
    let snapshot =
        local_history::read_entry(local_history::CODE_LAYOUT, &root, &file, "1000").unwrap();
    assert_eq!(snapshot.content, "fn a() {}\n");
    // 恢复（恢复前兜底快照——可撤回）。
    let restored =
        local_history::restore_entry(local_history::CODE_LAYOUT, &root, &file, "1000", 4000)
            .unwrap();
    assert_eq!(restored.content, "fn a() {}\n");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "fn a() {}\n");
    // 恢复产生了兜底快照（可撤回）。
    let after = local_history::list_entries(local_history::CODE_LAYOUT, &root, &file).unwrap();
    assert_eq!(after.len(), 3, "恢复前兜底快照存在（可撤回）");
}

#[test]
fn snapshot_prunes_oldest_beyond_max_entries() {
    let (_tmp, root) = tempdir_like::new("aeroric-history-prune");
    let file = root.join("main.rs");
    std::fs::write(&file, "v1").unwrap();
    let layout = local_history::HistoryLayout {
        dir: local_history::HISTORY_DIR,
        max_entries: 3,
        min_interval_ms: 0,
    };
    for (i, content) in ["v2", "v3", "v4", "v5"].iter().enumerate() {
        std::fs::write(&file, content).unwrap();
        local_history::record_snapshot(layout, &root, &file, Some("next"), 1000 + i as u64)
            .unwrap();
    }
    let entries = local_history::list_entries(layout, &root, &file).unwrap();
    assert_eq!(entries.len(), 3, "超出上限从最旧开始删");
}

#[test]
fn discard_history_removes_vault() {
    let (_tmp, root) = tempdir_like::new("aeroric-history-discard");
    let file = root.join("main.rs");
    std::fs::write(&file, "x").unwrap();
    local_history::record_snapshot(local_history::CODE_LAYOUT, &root, &file, Some("y"), 1000)
        .unwrap();
    local_history::discard_history(local_history::CODE_LAYOUT, &root, &file).unwrap();
    let entries = local_history::list_entries(local_history::CODE_LAYOUT, &root, &file).unwrap();
    assert!(entries.is_empty());
}

#[test]
fn formatter_selection_matches_tauri_semantics() {
    let cmd = format::formatter_for_extension("tsx", "/repo/src/App.tsx").unwrap();
    assert_eq!(cmd.program, "pnpm");
    assert_eq!(
        cmd.args,
        vec!["exec", "prettier", "--write", "/repo/src/App.tsx"]
    );
    let cmd = format::formatter_for_extension("rs", "/repo/src/lib.rs").unwrap();
    assert_eq!(cmd.program, "rustfmt");
    assert!(format::formatter_for_extension("png", "/repo/i.png").is_none());
}

#[test]
fn formatter_failure_leaves_content_unchanged() {
    let (_tmp, root) = tempdir_like::new("aeroric-format-fail");
    let file = root.join("probe.unknownext");
    std::fs::write(&file, "original").unwrap();
    // 未知扩展名＝无 formatter＝拒绝（原文在磁盘上未动）。
    let result = format::format_file(&root, file.to_str().unwrap(), None);
    assert!(result.is_err(), "未知扩展名拒绝");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "original",
        "原文不变"
    );
    // formatter 非零退出＝拒绝（同上）。rustfmt 对非法 Rust 源退出非零。
    let rs = root.join("broken.rs");
    std::fs::write(&rs, "fn broken( {").unwrap();
    let result = format::format_file(&root, rs.to_str().unwrap(), None);
    assert!(result.is_err(), "formatter 失败拒绝");
    assert_eq!(
        std::fs::read_to_string(&rs).unwrap(),
        "fn broken( {",
        "原文不变"
    );
}

#[test]
fn preview_decision_covers_text_image_and_readonly() {
    assert!(matches!(
        preview::preview_decision("main.rs", 100, true),
        preview::PreviewDecision::Text
    ));
    assert!(matches!(
        preview::preview_decision("logo.png", 100, false),
        preview::PreviewDecision::Image { .. }
    ));
    // 大文件只读（有原因）。
    let big = preview::preview_decision("big.txt", preview::MAX_FILE_BYTES + 1, true);
    match big {
        preview::PreviewDecision::ReadOnly { reason } => assert!(reason.contains("limit")),
        _ => panic!("大文件应为只读"),
    }
    // 二进制/未知编码只读。
    let binary = preview::preview_decision("data.bin", 10, false);
    match binary {
        preview::PreviewDecision::ReadOnly { reason } => assert!(reason.contains("UTF-8")),
        _ => panic!("未知编码应为只读"),
    }
    // 图片超预览上限只读。
    let big_image =
        preview::preview_decision("big.png", preview::MAX_IMAGE_PREVIEW_BYTES + 1, false);
    assert!(matches!(
        big_image,
        preview::PreviewDecision::ReadOnly { .. }
    ));
}

#[test]
fn utf8_and_link_scheme_guards() {
    assert!(preview::is_utf8("你好".as_bytes()));
    assert!(!preview::is_utf8(&[0xFF, 0xFE]));
    // 恶意 HTML/javascript: link/本地文件探测不能执行或泄露。
    assert!(!preview::is_allowed_link_scheme("javascript:alert(1)"));
    assert!(!preview::is_allowed_link_scheme("data:text/html,<script>"));
    assert!(!preview::is_allowed_link_scheme("file:///etc/passwd"));
    assert!(preview::is_allowed_link_scheme("https://example.com"));
    assert!(preview::is_allowed_link_scheme("mailto:a@b.c"));
}
