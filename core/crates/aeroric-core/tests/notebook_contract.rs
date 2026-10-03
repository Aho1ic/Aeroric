//! P7-05 验收要求的 notebook 契约测试（`tests/notebook_contract.rs`）。
//!
//! 覆盖验收原文点名的样本：Markdown/富文本往返、图片附件、重名标签、反向链接、
//! 任务复选框、trash restore、导入失败部分报告；ZIP `../` 与符号链接不写出
//! 测试 vault；旧笔记与模板内容 hash 未被无关格式化改变。
//!
//! 既有单测（搬移自 `src-tauri/src/notebook/tests.rs` 与 `import/*_tests.rs`）
//! 在 `--lib` 里跑；本文件是跨模块的契约层。

use std::path::{Path, PathBuf};

use aeroric_core::notebook::attachments;
use aeroric_core::notebook::fs_ops::{self, SaveOutcome};
use aeroric_core::notebook::html2md::html_to_markdown;
use aeroric_core::notebook::import;
use aeroric_core::notebook::links::scan_vault_links;
use aeroric_core::notebook::migrate;
use aeroric_core::notebook::state::{resolve_within, NotebookState};
use aeroric_core::notebook::tag_rename::rename_vault_tag;
use aeroric_core::notebook::tags::scan_vault_tags;
use aeroric_core::notebook::tasks::scan_vault_tasks;
use aeroric_core::notebook::trash;
use aeroric_core::notebook::vault_index::scan_vault_titles;

/// 每个测试一个独立临时目录（与搬移来的单测同一命名规则，并行不撞）。
fn temp_vault(label: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let unique = format!(
        "{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let dir = std::env::temp_dir().join(format!("aeroric-notebook-contract-{label}-{unique}"));
    std::fs::create_dir_all(&dir).expect("create temp vault");
    // temp_dir 在 macOS 上带 symlink（/var → /private/var）；与
    // `resolve_in_vaults` 的 canonicalize 结果保持同一坐标系。
    std::fs::canonicalize(&dir).expect("canonicalize temp vault")
}

fn registered(vault: &Path) -> NotebookState {
    let state = NotebookState::default();
    state.register_vault(vault).expect("register vault");
    state
}

// ── Markdown / 富文本往返 ────────────────────────────────────────────────────

#[test]
fn a_markdown_note_survives_save_read_and_richtext_conversion() {
    let vault = temp_vault("roundtrip");
    let state = registered(&vault);
    let path = vault.join("Plan.md").to_string_lossy().to_string();

    let content = "# Plan\n\n**Ship** the thing.\n";
    let sig = fs_ops::create_note(&state, &path, content).expect("create");
    let opened = fs_ops::read_note(&state, &path).expect("read");
    assert_eq!(opened.content, content, "Markdown 原文逐字节往返");
    assert_eq!(opened.sig, sig);

    // 富文本往返：HTML → Markdown（迁移与粘贴共用这一份转换器）。
    let html = "<h1>Plan</h1><p><strong>Ship</strong> the thing.</p>";
    let markdown = html_to_markdown(html, false);
    assert!(markdown.contains("# Plan"), "got: {markdown}");
    assert!(markdown.contains("**Ship**"), "got: {markdown}");
    let _ = std::fs::remove_dir_all(vault);
}

// ── 图片附件 ─────────────────────────────────────────────────────────────────

#[test]
fn an_image_attachment_lands_lists_and_reads_back() {
    let vault = temp_vault("attachment");
    let state = registered(&vault);
    let note = vault.join("Shot.md");
    std::fs::write(&note, "# Shot\n\n![[shot.png]]\n").expect("write note");

    let mut png = vec![0x89_u8, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    png.extend_from_slice(&[0u8; 64]);
    // 镜像宿主 `notebook_attachment_save` 的编排：resolve → owning_vault → save_bytes。
    let resolved_note = state
        .resolve_in_vaults(note.to_str().unwrap(), false)
        .expect("resolve note");
    let owning = state.owning_vault(&resolved_note).expect("owning vault");
    let saved =
        attachments::save_bytes(&owning, &resolved_note, Some("shot.png"), "image/png", &png)
            .expect("save");
    assert!(
        saved.path.contains("attachments"),
        "落点在附件目录: {}",
        saved.path
    );

    let listed = attachments::list(&vault, 10).expect("list");
    assert_eq!(listed.len(), 1);
    let read = attachments::read(Path::new(&saved.path)).expect("read");
    assert_eq!(read, png, "附件字节逐字节往返");
    let _ = std::fs::remove_dir_all(vault);
}

// ── 重名标签 + 跨文件改写 ─────────────────────────────────────────────────────

#[test]
fn duplicate_tags_rename_reports_across_notes() {
    let vault = temp_vault("tags");
    let state = registered(&vault);
    for (name, body) in [
        ("A.md", "# A\n\n- [ ] ship #alpha today\n"),
        ("B.md", "# B\n\nsee #alpha and #alpha twice\n"),
    ] {
        fs_ops::create_note(&state, vault.join(name).to_string_lossy().as_ref(), body)
            .expect("create");
    }

    let scanned = scan_vault_tags(&vault).expect("scan");
    assert_eq!(scanned.len(), 2, "两篇笔记都含 #alpha");
    let total: usize = scanned.iter().map(|s| s.tags.len()).sum();
    assert_eq!(total, 3, "重名标签的每一处出现都单独可数");

    let report = rename_vault_tag(&state, &vault, "alpha", "beta").expect("rename");
    assert_eq!(report.changed.len(), 2, "两篇都被改写");

    let rescanned = scan_vault_tags(&vault).expect("rescan");
    let total_after: usize = rescanned.iter().map(|s| s.tags.len()).sum();
    assert_eq!(total_after, 3);
    assert!(rescanned
        .iter()
        .all(|s| s.tags.iter().all(|t| t.raw == "beta")));
    let _ = std::fs::remove_dir_all(vault);
}

// ── 反向链接 + 标题索引 ──────────────────────────────────────────────────────

#[test]
fn backlinks_and_titles_resolve_from_the_vault_scan() {
    let vault = temp_vault("backlinks");
    let state = registered(&vault);
    fs_ops::create_note(
        &state,
        vault.join("B.md").to_str().unwrap(),
        "---\ntitle: Deep Note\n---\n\nbody\n",
    )
    .expect("create B");
    fs_ops::create_note(
        &state,
        vault.join("A.md").to_str().unwrap(),
        "# A\n\nlinks to [[Deep Note]] here\n",
    )
    .expect("create A");

    let titles = scan_vault_titles(&vault).expect("titles");
    let b = titles
        .iter()
        .find(|e| e.path.ends_with("B.md"))
        .expect("B indexed");
    assert_eq!(b.title, "Deep Note", "frontmatter 标题优先于文件名");

    let links = scan_vault_links(&vault).expect("links");
    let a = links
        .iter()
        .find(|s| s.path.ends_with("A.md"))
        .expect("A links");
    assert_eq!(a.links.len(), 1);
    assert_eq!(a.links[0].raw, "Deep Note");
    assert_eq!(a.links[0].line, 3);
    assert!(!a.links[0].embed);
    let _ = std::fs::remove_dir_all(vault);
}

// ── 任务复选框 ────────────────────────────────────────────────────────────────

#[test]
fn task_checkboxes_flip_through_save_and_rescan() {
    let vault = temp_vault("tasks");
    let state = registered(&vault);
    let path = vault.join("Todo.md").to_string_lossy().to_string();
    let before = "# Todo\n\n- [ ] buy milk\n";
    let sig = fs_ops::create_note(&state, &path, before).expect("create");

    let scanned = scan_vault_tasks(&vault).expect("scan");
    assert_eq!(scanned[0].tasks.len(), 1);
    assert!(!scanned[0].tasks[0].checked);
    assert_eq!(scanned[0].tasks[0].text, "buy milk");

    let flipped = before.replace("- [ ] buy milk", "- [x] buy milk");
    let outcome = fs_ops::save_note(&state, &path, &flipped, Some(sig), false).expect("save");
    assert!(matches!(outcome, SaveOutcome::Saved { .. }));

    let rescanned = scan_vault_tasks(&vault).expect("rescan");
    assert!(rescanned[0].tasks[0].checked, "勾选写回后复扫为已完成");
    let _ = std::fs::remove_dir_all(vault);
}

// ── trash restore / purge ────────────────────────────────────────────────────

#[test]
fn delete_restore_purge_cycle_keeps_content_intact() {
    let vault = temp_vault("trash");
    let state = registered(&vault);
    let path = vault.join("Ephemeral.md");
    std::fs::write(&path, "# Ephemeral\n\nprecious body\n").expect("write");

    // 走宿主命令的同一套编排：resolve → owning_vault → trash → 清指纹。
    let resolved = state
        .resolve_in_vaults(path.to_str().unwrap(), false)
        .expect("resolve");
    let item = trash::trash(&vault, &resolved).expect("trash");
    assert_eq!(item.relative_path, "Ephemeral.md");
    assert!(!path.exists(), "软删后原路径不再有文件");
    state.record_close(&resolved).expect("close");

    let listed = trash::list(&vault).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, item.id);

    let restored = trash::restore(&vault, &item.id).expect("restore");
    assert!(restored.path.ends_with("Ephemeral.md"));
    assert_eq!(
        std::fs::read_to_string(&path).expect("read back"),
        "# Ephemeral\n\nprecious body\n",
        "恢复后内容逐字节一致"
    );

    let item2 = trash::trash(&vault, &resolved).expect("trash again");
    trash::purge(&vault, &item2.id).expect("purge");
    assert!(
        trash::list(&vault).expect("list").is_empty(),
        "彻底删除后清单为空"
    );
    let _ = std::fs::remove_dir_all(vault);
}

// ── 导入失败部分报告 ─────────────────────────────────────────────────────────

#[test]
fn an_import_failure_still_reports_the_part_that_landed() {
    let vault = temp_vault("import-partial");
    let error = import::run::run(&vault, "notion", &mut |session, dir, report| {
        let path = import::landing::unique_path(dir, "a.md");
        std::fs::write(&path, "# A").expect("write landed note");
        session.record("k1".to_string());
        report.push(import::report::ImportItem::imported(
            "a.md",
            "imports/notion/a.md",
        ));
        Err("归档中途损坏".to_string())
    })
    .expect_err("整轮失败");

    assert_eq!(error, "归档中途损坏");
    // 已经落地的部分在 vault 里，报告与清单也都写成功 —— 失败报告有样本。
    assert!(vault.join("imports/notion/a.md").is_file());
    assert!(import::manifest::Session::open(&vault, "notion").is_known("k1"));
    let report_notes: Vec<_> = std::fs::read_dir(vault.join("imports/notion"))
        .expect("read dir")
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("导入报告-"))
        .collect();
    assert_eq!(report_notes.len(), 1, "失败的一轮也应留下报告笔记");
    let _ = std::fs::remove_dir_all(vault);
}

// ── ZIP 路径穿越 / 符号链接不写出 vault ─────────────────────────────────────

fn write_zip(entries: &[(&str, &[u8])], to: &Path) {
    use std::io::Write;
    let file = std::fs::File::create(to).expect("create zip");
    let mut writer = zip::ZipWriter::new(file);
    for (name, body) in entries {
        writer
            .start_file(
                *name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .expect("start entry");
        writer.write_all(body).expect("write entry");
    }
    writer.finish().expect("finish zip");
}

#[test]
fn a_zip_entry_trying_to_escape_is_rejected_without_touching_the_host_tree() {
    let parent = std::env::temp_dir().join(format!(
        "aeroric-notebook-contract-zip-host-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&parent);
    let vault = parent.join("vault");
    std::fs::create_dir_all(&vault).expect("create vault");

    let zip_path = parent.join("export.zip");
    write_zip(
        &[
            ("../evil.md", b"# escaped?\n" as &[u8]),
            ("My Page.md", b"# ok note\n"),
        ],
        &zip_path,
    );

    let report = import::notion::import(&vault, &zip_path).expect("import runs");
    assert_eq!(report.imported, 1, "正常条目照常落地");
    assert!(report.failed >= 1, "穿越条目在报告里记为 failed，不静默丢");
    assert!(
        report.items.iter().any(|i| i.source.contains("../evil.md")
            && matches!(i.status, import::report::ItemStatus::Failed { .. })),
        "报告引用归档里的原始名字: {:?}",
        report.items
    );
    assert!(
        !parent.join("evil.md").exists(),
        "`../evil.md` 绝不能落在 vault 外"
    );
    assert!(vault.join("imports/notion/My Page.md").is_file());

    let _ = std::fs::remove_dir_all(&parent);
}

#[test]
fn a_symlink_in_the_source_tree_is_skipped_not_followed() {
    let vault = temp_vault("symlink-src");
    let source = vault.join("source");
    let dest = vault.join("dest");
    std::fs::create_dir_all(&source).expect("source");
    std::fs::create_dir_all(&dest).expect("dest");
    std::fs::write(source.join("real.md"), "# real\n").expect("real");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&vault, source.join("loop")).expect("symlink");

    let mut report = import::report::ImportReport::new("obsidian", "imports/obsidian");
    let mut session = import::manifest::Session::open(&dest, "obsidian");
    let mut budget = import::guards::Budget::new();
    let mut ctx = import::walk::WalkCtx {
        provider: "obsidian",
        session: &mut session,
        report: &mut report,
        budget: &mut budget,
        root: &source,
        dest_prefix: "imports/obsidian",
    };
    import::walk::copy_dir(
        &mut ctx,
        &source,
        &dest.join("imports/obsidian"),
        0,
        &|_p| import::walk::Take::Copy,
    )
    .expect("walk completes");

    assert_eq!(report.imported, 1, "真笔记照常落地");
    #[cfg(unix)]
    assert!(
        report.items.iter().any(|i| i.source == "loop"
            && matches!(
                i.status,
                import::report::ItemStatus::Skipped {
                    reason: import::report::SkipReason::Symlink
                }
            )),
        "软链记为 skipped，绝不递归进去"
    );
    let _ = std::fs::remove_dir_all(vault);
}

// ── 旧笔记与模板内容 hash 未被无关格式化改变 ────────────────────────────────

const LEGACY_NOTES: &str = r##"[
  {"id":"n1","title":"Plain","body":"# Plain\n\nuntouched body\n","format":"markdown","updatedAt":1750000000000},
  {"id":"n2","title":"Rich","body":"<p>convert <b>me</b></p>","format":"richtext","updatedAt":1750000001000}
]"##;

#[test]
fn migration_and_richtext_conversion_leave_unrelated_notes_and_templates_byte_identical() {
    let vault = temp_vault("hash-stable");

    let report = migrate::migrate_legacy_notes(&vault, LEGACY_NOTES).expect("migrate");
    assert_eq!(report.migrated.len(), 2);

    // 模板先落一份 —— 转换/重跑迁移都不该碰它。
    let templates = vault.join(".notebook/templates");
    std::fs::create_dir_all(&templates).expect("templates dir");
    let template_bytes = "# Daily\n\n- [ ] {{date}}\n";
    std::fs::write(templates.join("daily.md"), template_bytes).expect("template");

    let plain = vault.join("Plain.md");
    let plain_before = std::fs::read(&plain).expect("plain bytes");

    let conversion = migrate::convert_richtext_notes(&vault).expect("convert");
    assert_eq!(conversion.converted.len(), 1, "只有 richtext 那篇被转换");
    assert_eq!(conversion.skipped, 1, "markdown 那篇被跳过");

    let plain_after = std::fs::read(&plain).expect("plain bytes after");
    assert_eq!(plain_before, plain_after, "无关笔记内容 hash 不被改变");
    assert_eq!(
        std::fs::read(templates.join("daily.md")).expect("template after"),
        template_bytes.as_bytes(),
        "模板内容不被无关格式化改变"
    );

    // 幂等：重跑迁移全走 skipped，不再写新文件。
    let again = migrate::migrate_legacy_notes(&vault, LEGACY_NOTES).expect("migrate again");
    assert_eq!(again.migrated.len(), 0);
    assert_eq!(again.skipped.len(), 2, "重复条目按 legacyId 跳过");
    let _ = std::fs::remove_dir_all(vault);
}

// ── 多窗口 expectedRevision 冲突 ─────────────────────────────────────────────

#[test]
fn a_stale_baseline_conflicts_instead_of_overwriting() {
    let vault = temp_vault("conflict");
    let state = registered(&vault);
    let path = vault.join("Shared.md").to_string_lossy().to_string();
    let original = "# Shared\n\nwindow A\n";
    let sig_a = fs_ops::create_note(&state, &path, original).expect("create");

    // 窗口 B 在 A 不知情的情况下改了盘上的内容。
    let window_b_write = "# Shared\n\nwindow B got here first\n";
    let sig_b = match fs_ops::save_note(&state, &path, window_b_write, Some(sig_a.clone()), false)
        .expect("B saves")
    {
        SaveOutcome::Saved { sig } => sig,
        other => panic!("window B save should succeed, got {other:?}"),
    };

    // 窗口 A 拿旧基线保存 → 冲突而不是静默覆盖。
    let stale = fs_ops::save_note(
        &state,
        &path,
        "# Shared\n\nwindow A overwrites\n",
        Some(sig_a),
        false,
    )
    .expect("save returns a normal branch");
    match stale {
        SaveOutcome::Conflict { disk } => {
            assert_eq!(disk, sig_b, "冲突里带回的是磁盘现状");
        }
        other => panic!("expected conflict, got {other:?}"),
    }
    assert_eq!(
        std::fs::read_to_string(&path).expect("disk"),
        window_b_write,
        "盘上内容没有被旧窗口覆盖"
    );

    // force 才允许覆盖 —— 由用户在 UI 上显式选择。
    let forced = fs_ops::save_note(&state, &path, "# Shared\n\nwindow A wins\n", None, true)
        .expect("force save");
    assert!(matches!(forced, SaveOutcome::Saved { .. }));
    let _ = std::fs::remove_dir_all(vault);
}

// ── allowlist 闸门 ───────────────────────────────────────────────────────────

#[test]
fn the_vault_allowlist_still_gates_every_path() {
    let vault = temp_vault("allowlist");
    let state = registered(&vault);
    let outside = std::env::temp_dir().join("aeroric-notebook-contract-outside.md");
    // 未注册路径拒绝（不存在也拒绝 —— allow_missing=false）。
    assert!(resolve_within(
        &state
            .registered_vaults()
            .map(|v| v.into_iter().collect::<std::collections::HashSet<_>>())
            .unwrap_or_default(),
        &outside.to_string_lossy(),
        false,
    )
    .is_err());
    // `..` 穿越拒绝。
    let inside = format!("{}/../etc", vault.to_string_lossy());
    assert!(state.resolve_in_vaults(&inside, false).is_err());
    let _ = std::fs::remove_dir_all(vault);
    let _ = std::fs::remove_file(outside);
}
