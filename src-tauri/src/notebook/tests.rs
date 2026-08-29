//! 随手记后端的单元测试。重点覆盖两块最容易出事的地方:
//! - **迁移**:幂等、回滚、非法文件名、有损转换的兜底
//! - **保存**:冲突检测的每个分支(这是唯一会静默丢数据的路径)

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::attachments;
use super::fs_ops::{self, SaveOutcome};
use super::migrate::{self, slugify};
use super::snapshots;
use super::state::{resolve_within, FileSig, NotebookState};
use super::trash;

/// 每个测试一个独立临时目录。用 pid + 纳秒 + 计数器命名,并行跑也不会撞。
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
    let dir = std::env::temp_dir().join(format!("aeroric-notebook-{label}-{unique}"));
    std::fs::create_dir_all(&dir).expect("create temp vault");
    dir
}

fn registered_state(vault: &Path) -> NotebookState {
    let state = NotebookState::default();
    state.register_vault(vault).expect("register vault");
    state
}

fn note_path(vault: &Path, name: &str) -> String {
    vault.join(name).to_string_lossy().to_string()
}

// ── slugify ────────────────────────────────────────────────────────────────

#[test]
fn slugify_replaces_path_separators_and_reserved_characters() {
    // 这些字符要么是路径分隔符,要么在 Windows 上非法。留一个都会让保存失败。
    assert_eq!(slugify("a/b"), "a-b");
    assert_eq!(slugify("a\\b"), "a-b");
    assert_eq!(slugify("Q1: plan"), "Q1-plan");
    assert_eq!(slugify("what?*\"<>|"), "what");
}

#[test]
fn slugify_collapses_runs_and_trims_edges() {
    assert_eq!(slugify("a???b"), "a-b");
    assert_eq!(slugify("  spaced  out  "), "spaced-out");
    // Windows 会静默吃掉结尾的点和空格,磁盘上的名字就和记录的不一致了。
    assert_eq!(slugify("trailing..."), "trailing");
    assert_eq!(slugify("trailing   "), "trailing");
}

#[test]
fn slugify_keeps_unicode_and_falls_back_when_empty() {
    // 中文标题是主要使用场景,不能被 ASCII 化。
    assert_eq!(slugify("发布计划"), "发布计划");
    assert_eq!(slugify("发布 计划"), "发布-计划");
    assert_eq!(slugify(""), "untitled");
    assert_eq!(slugify("///"), "untitled");
    assert_eq!(slugify("   "), "untitled");
}

#[test]
fn slugify_truncates_on_char_boundary() {
    // 200 个中文字 = 600 字节,必须按字符边界截断,否则得到非法 UTF-8。
    let long = "字".repeat(200);
    let slug = slugify(&long);
    assert!(slug.len() <= 200, "slug byte length {}", slug.len());
    // 能重新解析成合法字符串就说明没切裂。
    assert!(slug.chars().all(|c| c == '字'));
}

#[test]
fn slugify_escapes_windows_device_names() {
    // `CON.md` 在 Windows 上打不开,即使带了扩展名。
    assert_eq!(slugify("CON"), "note-CON");
    assert_eq!(slugify("nul"), "note-nul");
    // 只有完整匹配才算保留名。
    assert_eq!(slugify("CONSOLE"), "CONSOLE");
}

// ── allowlist ──────────────────────────────────────────────────────────────

#[test]
fn resolve_rejects_paths_outside_every_vault() {
    let vault = temp_vault("allowlist");
    let mut vaults = HashSet::new();
    vaults.insert(vault.canonicalize().expect("canon"));

    let outside = std::env::temp_dir().join("aeroric-notebook-outsider.md");
    let error = resolve_within(&vaults, &outside.to_string_lossy(), true)
        .expect_err("must reject path outside vault");
    assert!(error.contains("outside"), "unexpected error: {error}");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn resolve_rejects_traversal_out_of_vault() {
    let vault = temp_vault("traversal");
    let mut vaults = HashSet::new();
    vaults.insert(vault.canonicalize().expect("canon"));

    // `..` 会在 canonicalize 父目录时被解析掉,于是落在 vault 外被拒。
    let escape = vault.join("..").join("escaped.md");
    assert!(resolve_within(&vaults, &escape.to_string_lossy(), true).is_err());

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn resolve_rejects_sibling_with_shared_prefix() {
    // `/tmp/x-evil` 不能被 `/tmp/x` 放行。starts_with 是按组件比对的,
    // 但这条不变式值得钉住 —— 换成字符串比较就会漏。
    let base = temp_vault("prefix");
    let sibling = PathBuf::from(format!("{}-evil", base.to_string_lossy()));
    std::fs::create_dir_all(&sibling).expect("create sibling");
    let target = sibling.join("note.md");
    std::fs::write(&target, "x").expect("write");

    let mut vaults = HashSet::new();
    vaults.insert(base.canonicalize().expect("canon"));
    assert!(resolve_within(&vaults, &target.to_string_lossy(), false).is_err());

    std::fs::remove_dir_all(&base).ok();
    std::fs::remove_dir_all(&sibling).ok();
}

#[test]
fn resolve_rejects_everything_when_no_vault_registered() {
    // allowlist 为空时必须全拒。空集合下 `any()` 返回 false,但要显式钉住:
    // 前端忘了调 ensure_default_vault 时不能退化成"什么都能写"。
    let vaults = HashSet::new();
    let error = resolve_within(&vaults, "/tmp/whatever.md", true).expect_err("must reject");
    assert!(error.contains("No notebook vault"), "unexpected: {error}");
}

#[test]
fn resolve_requires_absolute_path() {
    let vault = temp_vault("relative");
    let mut vaults = HashSet::new();
    vaults.insert(vault.canonicalize().expect("canon"));
    assert!(resolve_within(&vaults, "notes/relative.md", true).is_err());
    std::fs::remove_dir_all(&vault).ok();
}

// ── 保存与冲突检测 ──────────────────────────────────────────────────────────

#[test]
fn save_then_reopen_roundtrips_content() {
    let vault = temp_vault("roundtrip");
    let state = registered_state(&vault);
    let path = note_path(&vault, "note.md");

    fs_ops::create_note(&state, &path, "# Hello\n").expect("create");
    let opened = fs_ops::read_note(&state, &path).expect("open");
    assert_eq!(opened.content, "# Hello\n");

    let outcome = fs_ops::save_note(&state, &path, "# Hello\n\nmore\n", Some(opened.sig), false)
        .expect("save");
    assert!(matches!(outcome, SaveOutcome::Saved { .. }));
    assert_eq!(
        std::fs::read_to_string(vault.join("note.md")).expect("read"),
        "# Hello\n\nmore\n"
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn save_reports_conflict_when_disk_changed_underneath() {
    let vault = temp_vault("conflict");
    let state = registered_state(&vault);
    let path = note_path(&vault, "note.md");

    fs_ops::create_note(&state, &path, "original\n").expect("create");
    let opened = fs_ops::read_note(&state, &path).expect("open");

    // 模拟外部编辑器改了同一个文件。mtime 精度在某些文件系统上只到秒,
    // 所以内容也一起变 —— 这正是 hash 存在的理由。
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(vault.join("note.md"), "changed by someone else\n").expect("external write");

    let outcome = fs_ops::save_note(&state, &path, "my version\n", Some(opened.sig), false)
        .expect("save call itself must not fail");
    assert!(
        matches!(outcome, SaveOutcome::Conflict { .. }),
        "external change must surface as a conflict, not a silent overwrite"
    );
    // 关键:冲突时磁盘内容不能被动过。
    assert_eq!(
        std::fs::read_to_string(vault.join("note.md")).expect("read"),
        "changed by someone else\n"
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn save_with_force_overwrites_after_conflict() {
    let vault = temp_vault("force");
    let state = registered_state(&vault);
    let path = note_path(&vault, "note.md");

    fs_ops::create_note(&state, &path, "original\n").expect("create");
    let opened = fs_ops::read_note(&state, &path).expect("open");
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(vault.join("note.md"), "external\n").expect("external write");

    let outcome = fs_ops::save_note(&state, &path, "mine\n", Some(opened.sig), true).expect("save");
    assert!(matches!(outcome, SaveOutcome::Saved { .. }));
    assert_eq!(
        std::fs::read_to_string(vault.join("note.md")).expect("read"),
        "mine\n"
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn save_without_baseline_reports_conflict_for_existing_file() {
    let vault = temp_vault("nobaseline");
    let state = registered_state(&vault);
    let path = note_path(&vault, "existing.md");
    std::fs::write(vault.join("existing.md"), "on disk\n").expect("seed");

    // 没有基线 + 文件已存在 = 无法判断来历,必须报冲突而不是覆盖。
    let outcome = fs_ops::save_note(&state, &path, "blind write\n", None, false).expect("save");
    assert!(matches!(outcome, SaveOutcome::Conflict { .. }));
    assert_eq!(
        std::fs::read_to_string(vault.join("existing.md")).expect("read"),
        "on disk\n"
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn save_creates_new_file_without_baseline() {
    let vault = temp_vault("newfile");
    let state = registered_state(&vault);
    let path = note_path(&vault, "fresh.md");

    // 文件不存在时没有基线是正常的,不该报冲突。
    let outcome = fs_ops::save_note(&state, &path, "fresh\n", None, false).expect("save");
    assert!(matches!(outcome, SaveOutcome::Saved { .. }));

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn save_leaves_no_temporary_files_behind() {
    let vault = temp_vault("tmpclean");
    let state = registered_state(&vault);
    let path = note_path(&vault, "note.md");
    fs_ops::create_note(&state, &path, "a\n").expect("create");
    let opened = fs_ops::read_note(&state, &path).expect("open");
    fs_ops::save_note(&state, &path, "b\n", Some(opened.sig), false).expect("save");

    let leftovers: Vec<String> = std::fs::read_dir(&vault)
        .expect("read dir")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "leftover temp files: {leftovers:?}");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn create_note_refuses_to_overwrite() {
    let vault = temp_vault("nooverwrite");
    let state = registered_state(&vault);
    let path = note_path(&vault, "note.md");

    fs_ops::create_note(&state, &path, "first\n").expect("create");
    let error = fs_ops::create_note(&state, &path, "second\n").expect_err("must not overwrite");
    assert!(error.starts_with("ALREADY_EXISTS:"), "unexpected: {error}");
    assert_eq!(
        std::fs::read_to_string(vault.join("note.md")).expect("read"),
        "first\n"
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn rename_moves_the_saved_baseline_along() {
    let vault = temp_vault("rename");
    let state = registered_state(&vault);
    let from = note_path(&vault, "before.md");
    let to = note_path(&vault, "after.md");

    fs_ops::create_note(&state, &from, "body\n").expect("create");
    fs_ops::read_note(&state, &from).expect("open");
    fs_ops::rename_note(&state, &from, &to).expect("rename");

    // 改名后基线要跟着走,否则下一次保存拿不到基线就会被判成冲突。
    let outcome = fs_ops::save_note(&state, &to, "body2\n", None, false).expect("save");
    assert!(
        matches!(outcome, SaveOutcome::Saved { .. }),
        "baseline should have followed the rename"
    );

    std::fs::remove_dir_all(&vault).ok();
}

// ── 树扫描 ─────────────────────────────────────────────────────────────────

#[test]
fn tree_lists_notes_and_hides_private_and_skip_dirs() {
    let vault = temp_vault("tree");
    let state = registered_state(&vault);
    std::fs::write(vault.join("b.md"), "b").expect("write");
    std::fs::write(vault.join("a.md"), "a").expect("write");
    std::fs::write(vault.join("not-a-note.png"), "x").expect("write");
    std::fs::create_dir_all(vault.join("sub")).expect("mkdir");
    std::fs::write(vault.join("sub").join("c.md"), "c").expect("write");
    // 这两个都不该出现在树里。
    std::fs::create_dir_all(vault.join(".notebook")).expect("mkdir");
    std::fs::write(vault.join(".notebook").join("secret.md"), "s").expect("write");
    std::fs::create_dir_all(vault.join("node_modules")).expect("mkdir");
    std::fs::write(vault.join("node_modules").join("dep.md"), "d").expect("write");

    let tree = fs_ops::read_tree(&state, &vault.to_string_lossy()).expect("tree");
    let names: Vec<&str> = tree.iter().map(|entry| entry.name.as_str()).collect();
    // 目录在前、文件在后,各自按名字排序。
    assert_eq!(names, vec!["sub", "a.md", "b.md"]);

    let sub = tree.iter().find(|entry| entry.name == "sub").expect("sub");
    let children = sub.children.as_ref().expect("children");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].name, "c.md");

    std::fs::remove_dir_all(&vault).ok();
}

// ── 版本历史(快照)─────────────────────────────────────────────────────────

/// 无视最小间隔连造若干条快照。
///
/// 走 `NOTE_LAYOUT` 换掉间隔而不是自己拼一个 layout:保留上限和目录必须还是
/// 生产用的那两个值,否则这些测试验的是测试自己的常量。
fn force_snapshots(vault: &Path, file: &Path, count: usize) {
    let layout = crate::local_history::HistoryLayout {
        min_interval_ms: 0,
        ..snapshots::NOTE_LAYOUT
    };
    for index in 0..count {
        // 每次都改一下内容:`record_snapshot_in` 会跳过"下一版和当前一样"的写入。
        std::fs::write(file, format!("version {index}\n")).expect("seed content");
        crate::local_history::record_snapshot_in(layout, vault, file, None).expect("snapshot");
    }
}

fn history_dir(vault: &Path) -> PathBuf {
    vault.join(".notebook").join("history")
}

/// 把已有快照的 id 往前挪一小时,让最小间隔窗口过期。
///
/// 限流看的是**最新快照的 id**(时间戳),不是文件 mtime,所以改名就够了。
/// 存在的意义:没有它,任何"窗口外再保存一次"的行为都得等三分钟才测得到,
/// 于是限流会把别的守卫一起遮住 —— 那些守卫就永远处在测不到的状态。
fn age_snapshots(vault: &Path) {
    let root = history_dir(vault);
    for note_dir in std::fs::read_dir(&root).expect("read history root") {
        let note_dir = note_dir.expect("history entry").path();
        for snapshot in std::fs::read_dir(&note_dir).expect("read note history") {
            let snapshot = snapshot.expect("snapshot entry").path();
            let stem = snapshot
                .file_stem()
                .and_then(|stem| stem.to_str())
                .expect("snapshot stem");
            let (base, suffix) = match stem.split_once('-') {
                Some((base, suffix)) => (base, format!("-{suffix}")),
                None => (stem, String::new()),
            };
            let aged: u64 = base.parse::<u64>().expect("timestamp id") - 60 * 60 * 1000;
            std::fs::rename(&snapshot, note_dir.join(format!("{aged}{suffix}.txt")))
                .expect("age snapshot");
        }
    }
}

#[test]
fn save_snapshots_the_previous_content_not_the_new_one() {
    let vault = temp_vault("snap-before");
    let state = registered_state(&vault);
    let path = note_path(&vault, "note.md");

    fs_ops::create_note(&state, &path, "v1\n").expect("create");
    let opened = fs_ops::read_note(&state, &path).expect("open");
    fs_ops::save_note(&state, &path, "v2\n", Some(opened.sig), false).expect("save");

    let entries = snapshots::list(&state, &path).expect("list");
    assert_eq!(entries.len(), 1, "one save must leave exactly one snapshot");
    let snapshot = snapshots::read(&state, &path, &entries[0].id).expect("read");
    // 快照存的是被覆盖掉的那一版。存成新内容的话历史里根本没有可回滚的东西。
    assert_eq!(snapshot.content, "v1\n");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn rapid_autosaves_share_one_snapshot() {
    let vault = temp_vault("snap-throttle");
    let state = registered_state(&vault);
    let path = note_path(&vault, "note.md");

    fs_ops::create_note(&state, &path, "v0\n").expect("create");
    let mut sig = fs_ops::read_note(&state, &path).expect("open").sig;
    // 自动保存每 800ms 一次。连着来五次,只该留下第一条(其余落在间隔窗口内)。
    for index in 1..=5 {
        let outcome = fs_ops::save_note(
            &state,
            &path,
            &format!("v{index}\n"),
            Some(sig.clone()),
            false,
        )
        .expect("save");
        match outcome {
            SaveOutcome::Saved { sig: next } => sig = next,
            SaveOutcome::Conflict { .. } => panic!("own saves must not conflict"),
        }
    }

    let entries = snapshots::list(&state, &path).expect("list");
    assert_eq!(
        entries.len(),
        1,
        "autosave bursts must not burn the retention window"
    );
    let snapshot = snapshots::read(&state, &path, &entries[0].id).expect("read");
    // 留下来的必须是**最早**那一版 —— 那才是用户想回到的地方。
    assert_eq!(snapshot.content, "v0\n");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn saving_identical_content_adds_no_snapshot() {
    let vault = temp_vault("snap-dedup");
    let state = registered_state(&vault);
    let path = note_path(&vault, "note.md");

    fs_ops::create_note(&state, &path, "v1\n").expect("create");
    let opened = fs_ops::read_note(&state, &path).expect("open");
    let saved = fs_ops::save_note(&state, &path, "v2\n", Some(opened.sig), false).expect("save");
    let sig = match saved {
        SaveOutcome::Saved { sig } => sig,
        SaveOutcome::Conflict { .. } => panic!("own save must not conflict"),
    };
    assert_eq!(snapshots::list(&state, &path).expect("list").len(), 1);

    // 让限流窗口过期,否则下面那次保存被限流拦下,而不是被"内容没变"拦下 ——
    // 两道守卫叠在一起就分不清是哪一道在起作用。
    age_snapshots(&vault);
    fs_ops::save_note(&state, &path, "v2\n", Some(sig), false).expect("save again");

    // 内容一模一样的保存不该产生快照:回滚到一个和当前完全相同的版本毫无意义,
    // 而这种条目会把 30 条的窗口占掉。
    let entries = snapshots::list(&state, &path).expect("list again");
    assert_eq!(entries.len(), 1, "identical save must not add a snapshot");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn editing_again_after_the_window_adds_a_snapshot() {
    let vault = temp_vault("snap-window");
    let state = registered_state(&vault);
    let path = note_path(&vault, "note.md");

    fs_ops::create_note(&state, &path, "v1\n").expect("create");
    let opened = fs_ops::read_note(&state, &path).expect("open");
    let saved = fs_ops::save_note(&state, &path, "v2\n", Some(opened.sig), false).expect("save");
    let sig = match saved {
        SaveOutcome::Saved { sig } => sig,
        SaveOutcome::Conflict { .. } => panic!("own save must not conflict"),
    };

    age_snapshots(&vault);
    fs_ops::save_note(&state, &path, "v3\n", Some(sig), false).expect("save again");

    // 限流只压同一段编辑里的连续自动保存。窗口过去之后必须重新开始记 ——
    // 不然一条笔记的历史会永远停在第一次编辑那里。
    let entries = snapshots::list(&state, &path).expect("list");
    assert_eq!(entries.len(), 2, "throttle must expire, not stop history");
    let newest = snapshots::read(&state, &path, &entries[0].id).expect("read newest");
    assert_eq!(newest.content, "v2\n");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn throttle_measures_from_the_newest_snapshot() {
    let vault = temp_vault("snap-newest");
    let state = registered_state(&vault);
    let path = note_path(&vault, "note.md");

    fs_ops::create_note(&state, &path, "v1\n").expect("create");
    let mut sig = fs_ops::read_note(&state, &path).expect("open").sig;
    let save = |content: &str, sig: FileSig| match fs_ops::save_note(
        &state,
        &path,
        content,
        Some(sig),
        false,
    )
    .expect("save")
    {
        SaveOutcome::Saved { sig } => sig,
        SaveOutcome::Conflict { .. } => panic!("own save must not conflict"),
    };

    sig = save("v2\n", sig);
    // 第一条快照挪老,让它落在窗口外。
    age_snapshots(&vault);
    sig = save("v3\n", sig);
    // 现在历史里一条老、一条新。第三次保存必须被**新**的那条挡住。
    assert_eq!(snapshots::list(&state, &path).expect("list").len(), 2);

    save("v4\n", sig);

    // 拿最旧那条算间隔的话,窗口永远显示"早就过期了",限流从第二条快照起就
    // 彻底失效 —— 自动保存会重新开始每 800ms 写一条。
    let entries = snapshots::list(&state, &path).expect("list again");
    assert_eq!(
        entries.len(),
        2,
        "throttle must look at the newest snapshot, not the oldest"
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn conflicted_save_leaves_no_snapshot() {
    let vault = temp_vault("snap-conflict");
    let state = registered_state(&vault);
    let path = note_path(&vault, "note.md");

    fs_ops::create_note(&state, &path, "original\n").expect("create");
    let opened = fs_ops::read_note(&state, &path).expect("open");
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(vault.join("note.md"), "external\n").expect("external write");

    let outcome =
        fs_ops::save_note(&state, &path, "mine\n", Some(opened.sig), false).expect("save");
    assert!(matches!(outcome, SaveOutcome::Conflict { .. }));

    // 冲突的保存没有写盘,给它留快照等于用没发生过的改动挤掉真实历史。
    let entries = snapshots::list(&state, &path).expect("list");
    assert!(entries.is_empty(), "conflict must not create a snapshot");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn restore_rolls_back_and_keeps_the_overwritten_version() {
    let vault = temp_vault("snap-restore");
    let state = registered_state(&vault);
    let path = note_path(&vault, "note.md");

    fs_ops::create_note(&state, &path, "first\n").expect("create");
    let opened = fs_ops::read_note(&state, &path).expect("open");
    fs_ops::save_note(&state, &path, "second\n", Some(opened.sig), false).expect("save");
    let entries = snapshots::list(&state, &path).expect("list");
    let first_id = entries[0].id.clone();

    let restored = snapshots::restore(&state, &path, &first_id).expect("restore");

    assert_eq!(restored.content, "first\n");
    assert_eq!(
        std::fs::read_to_string(vault.join("note.md")).expect("read"),
        "first\n"
    );
    // 回滚本身必须可撤销:被它覆盖掉的 "second" 要在历史里躺着,而且不能因为
    // 落在最小间隔窗口内就被限流丢掉。
    let after = snapshots::list(&state, &path).expect("list again");
    assert_eq!(after.len(), 2, "rollback must snapshot what it overwrote");
    let newest = snapshots::read(&state, &path, &after[0].id).expect("read newest");
    assert_eq!(newest.content, "second\n");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn restore_returns_a_baseline_the_next_save_accepts() {
    let vault = temp_vault("snap-baseline");
    let state = registered_state(&vault);
    let path = note_path(&vault, "note.md");

    fs_ops::create_note(&state, &path, "first\n").expect("create");
    let opened = fs_ops::read_note(&state, &path).expect("open");
    fs_ops::save_note(&state, &path, "second\n", Some(opened.sig), false).expect("save");
    let entries = snapshots::list(&state, &path).expect("list");
    let restored = snapshots::restore(&state, &path, &entries[0].id).expect("restore");

    // 回滚换掉了磁盘内容,前端手里的基线跟着换成 restored.sig。不返回新指纹的话
    // 下一次保存会撞上一个我们自己造出来的"冲突"。
    let outcome = fs_ops::save_note(&state, &path, "third\n", Some(restored.sig), false)
        .expect("save after restore");
    assert!(
        matches!(outcome, SaveOutcome::Saved { .. }),
        "restore must hand back a usable baseline"
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn snapshots_stay_out_of_the_note_tree() {
    let vault = temp_vault("snap-hidden");
    let state = registered_state(&vault);
    let path = note_path(&vault, "note.md");

    fs_ops::create_note(&state, &path, "v1\n").expect("create");
    let opened = fs_ops::read_note(&state, &path).expect("open");
    fs_ops::save_note(&state, &path, "v2\n", Some(opened.sig), false).expect("save");

    // 落在 vault 私有目录里 —— 那是树扫描唯一排除掉的地方。
    assert!(
        history_dir(&vault).is_dir(),
        "snapshots must live under the private dir"
    );
    let tree = fs_ops::read_tree(&state, &vault.to_string_lossy()).expect("tree");
    let names: Vec<&str> = tree.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names, vec!["note.md"], "history leaked into the note tree");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn history_keeps_only_the_newest_thirty_snapshots() {
    let vault = temp_vault("snap-retention");
    let state = registered_state(&vault);
    let path = note_path(&vault, "note.md");
    fs_ops::create_note(&state, &path, "seed\n").expect("create");

    force_snapshots(&vault, Path::new(&path), 34);

    let entries = snapshots::list(&state, &path).expect("list");
    assert_eq!(entries.len(), 30, "retention cap must hold on disk");
    // 数一遍磁盘:`list` 自己也会截断,只看它的长度分不出"裁剪生效"和
    // "裁剪没生效但列表截断了"。
    let on_disk = std::fs::read_dir(history_dir(&vault))
        .expect("read history root")
        .filter_map(Result::ok)
        .flat_map(|dir| std::fs::read_dir(dir.path()).expect("read note history"))
        .filter_map(Result::ok)
        .count();
    assert_eq!(
        on_disk, 30,
        "old snapshot files must be deleted, not hidden"
    );
    // 留下来的是最新的那批。
    let newest = snapshots::read(&state, &path, &entries[0].id).expect("read newest");
    assert_eq!(newest.content, "version 33\n");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn same_millisecond_snapshots_do_not_overwrite_each_other() {
    let vault = temp_vault("snap-collision");
    let state = registered_state(&vault);
    let path = note_path(&vault, "note.md");
    fs_ops::create_note(&state, &path, "seed\n").expect("create");

    // 连着造 12 条不限流的快照。id 的基数是毫秒时间戳,这个循环必然有几条撞在
    // 同一毫秒 —— 撞了要靠 `-N` 后缀分开,而不是后来的覆盖先来的。
    force_snapshots(&vault, Path::new(&path), 12);

    let entries = snapshots::list(&state, &path).expect("list");
    assert_eq!(
        entries.len(),
        12,
        "same-millisecond snapshots got clobbered"
    );
    let unique: HashSet<&str> = entries.iter().map(|entry| entry.id.as_str()).collect();
    assert_eq!(unique.len(), 12, "snapshot ids must be unique");
    // 内容也要各自独立,不能只是文件名不同。
    let mut contents: Vec<String> = entries
        .iter()
        .map(|entry| {
            snapshots::read(&state, &path, &entry.id)
                .expect("read")
                .content
        })
        .collect();
    contents.sort();
    contents.dedup();
    assert_eq!(contents.len(), 12, "snapshot bodies must all survive");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn nested_vault_notes_snapshot_into_their_own_vault() {
    let outer = temp_vault("snap-outer");
    let inner = outer.join("project").join(".aeroric").join("notes");
    std::fs::create_dir_all(&inner).expect("mkdir inner");
    let state = registered_state(&outer);
    state.register_vault(&inner).expect("register inner");
    let path = note_path(&inner, "note.md");

    fs_ops::create_note(&state, &path, "v1\n").expect("create");
    let opened = fs_ops::read_note(&state, &path).expect("open");
    fs_ops::save_note(&state, &path, "v2\n", Some(opened.sig), false).expect("save");

    // 两个 vault 嵌套时快照必须落在**最内层**那个里。落到外层的话项目笔记的
    // 历史会写进用户 home,而删掉项目目录后历史还在那儿。
    assert!(
        history_dir(&inner).is_dir(),
        "snapshot must land in the innermost vault"
    );
    assert!(
        !history_dir(&outer).exists(),
        "snapshot leaked into the outer vault"
    );

    std::fs::remove_dir_all(&outer).ok();
}

// ── 回收站(软删)───────────────────────────────────────────────────────────

fn trash_dir(vault: &Path) -> PathBuf {
    vault.join(".notebook").join("trash")
}

/// 回收站里的载荷文件名(不含清单),排序后返回。
fn trash_payloads(vault: &Path) -> Vec<String> {
    let dir = trash_dir(vault);
    if !dir.exists() {
        return Vec::new();
    }
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("read trash")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.ends_with(".meta.json"))
        .collect();
    names.sort();
    names
}

#[test]
fn trashed_note_leaves_its_original_path_but_stays_recoverable() {
    let vault = temp_vault("trash-roundtrip");
    let state = registered_state(&vault);
    let path = note_path(&vault, "Doomed.md");
    fs_ops::create_note(&state, &path, "keep me\n").expect("create");

    let item = trash::trash(&vault, Path::new(&path)).expect("trash");

    assert!(!Path::new(&path).exists(), "note still at its old path");
    assert_eq!(item.name, "Doomed.md");
    assert_eq!(item.relative_path, "Doomed.md");
    assert!(!item.is_dir);

    let listed = trash::list(&vault).expect("list");
    assert_eq!(listed, vec![item.clone()]);

    let restored = trash::restore(&vault, &item.id).expect("restore");

    assert_eq!(restored.path, path);
    assert_eq!(
        std::fs::read_to_string(&path).expect("read restored"),
        "keep me\n"
    );
    // 恢复完清单和载荷都要走干净,否则回收站里会留一条永远恢复不了的幽灵。
    assert!(trash::list(&vault).expect("list after").is_empty());
    assert!(trash_payloads(&vault).is_empty());

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn trashed_folder_restores_with_everything_inside_it() {
    let vault = temp_vault("trash-folder");
    let state = registered_state(&vault);
    let folder = vault.join("Journal");
    std::fs::create_dir_all(folder.join("2026")).expect("mkdir");
    fs_ops::create_note(&state, &note_path(&folder, "index.md"), "top\n").expect("create top");
    fs_ops::create_note(
        &state,
        &note_path(&folder.join("2026"), "aug.md"),
        "nested\n",
    )
    .expect("create nested");

    let item = trash::trash(&vault, &folder).expect("trash folder");
    assert!(item.is_dir);
    assert!(!folder.exists(), "folder still in the vault");

    trash::restore(&vault, &item.id).expect("restore folder");

    // 验收项:目录软删可完整恢复。整棵搬回来 —— 包括嵌套那一层。
    assert_eq!(
        std::fs::read_to_string(folder.join("index.md")).expect("read top"),
        "top\n"
    );
    assert_eq!(
        std::fs::read_to_string(folder.join("2026").join("aug.md")).expect("read nested"),
        "nested\n"
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn same_millisecond_deletes_of_the_same_name_do_not_overwrite_each_other() {
    let vault = temp_vault("trash-samems");
    let state = registered_state(&vault);
    let left = vault.join("a");
    let right = vault.join("b");
    std::fs::create_dir_all(&left).expect("mkdir a");
    std::fs::create_dir_all(&right).expect("mkdir b");
    fs_ops::create_note(&state, &note_path(&left, "note.md"), "left\n").expect("create left");
    fs_ops::create_note(&state, &note_path(&right, "note.md"), "right\n").expect("create right");

    // 验收项:同毫秒同名删除不互相覆盖。时间戳写死,否则这条守卫只在机器够慢
    // 的时候才真的被验到。
    let first = trash::trash_at(&vault, &left.join("note.md"), 1_700_000_000_000).expect("trash a");
    let second =
        trash::trash_at(&vault, &right.join("note.md"), 1_700_000_000_000).expect("trash b");

    assert_ne!(first.id, second.id, "same millisecond reused one id");
    assert_eq!(first.deleted_at_ms, second.deleted_at_ms);
    assert_eq!(trash::list(&vault).expect("list").len(), 2);

    trash::restore(&vault, &first.id).expect("restore a");
    trash::restore(&vault, &second.id).expect("restore b");

    // 两条内容都还在 —— 覆盖发生的话这里会读到同一份。
    assert_eq!(
        std::fs::read_to_string(left.join("note.md")).expect("read a"),
        "left\n"
    );
    assert_eq!(
        std::fs::read_to_string(right.join("note.md")).expect("read b"),
        "right\n"
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn restore_refuses_when_the_original_path_is_taken_again() {
    let vault = temp_vault("trash-occupied");
    let state = registered_state(&vault);
    let path = note_path(&vault, "note.md");
    fs_ops::create_note(&state, &path, "old\n").expect("create");
    let item = trash::trash(&vault, Path::new(&path)).expect("trash");
    fs_ops::create_note(&state, &path, "new\n").expect("recreate");

    let error = trash::restore(&vault, &item.id).expect_err("must refuse");

    // 用和新建 / 改名一样的前缀,前端能复用同一套「换个名字」的处理。
    assert!(error.starts_with("ALREADY_EXISTS:"), "unexpected: {error}");
    // 拒绝之后那条还必须留在回收站里 —— 报个错就把它顺手清掉等于直接吃掉数据。
    assert_eq!(trash::list(&vault).expect("list").len(), 1);
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "new\n",
        "the occupying note was overwritten"
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn restore_recreates_a_parent_folder_that_was_deleted_too() {
    let vault = temp_vault("trash-parent");
    let state = registered_state(&vault);
    let folder = vault.join("Archive");
    std::fs::create_dir_all(&folder).expect("mkdir");
    let path = note_path(&folder, "note.md");
    fs_ops::create_note(&state, &path, "body\n").expect("create");

    let note_item = trash::trash(&vault, Path::new(&path)).expect("trash note");
    // 再把空了的父目录也删掉。恢复笔记时它已经不在了。
    trash::trash(&vault, &folder).expect("trash folder");

    trash::restore(&vault, &note_item.id).expect("restore note");

    // 父目录补出来而不是报错:否则用户必须先猜出"要手工建一个 Archive 文件夹"
    // 才能恢复自己的笔记。
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "body\n");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn purge_sends_the_payload_out_of_the_vault_and_drops_its_history() {
    let vault = temp_vault("trash-purge");
    let state = registered_state(&vault);
    let path = note_path(&vault, "note.md");
    fs_ops::create_note(&state, &path, "v1\n").expect("create");
    let opened = fs_ops::read_note(&state, &path).expect("open");
    fs_ops::save_note(&state, &path, "v2\n", Some(opened.sig), false).expect("save");
    assert_eq!(
        snapshots::list(&state, &path)
            .expect("list snapshots")
            .len(),
        1,
        "the fixture needs a snapshot to prove it gets dropped"
    );

    let item = trash::trash(&vault, Path::new(&path)).expect("trash");
    trash::purge(&vault, &item.id).expect("purge");

    assert!(trash::list(&vault).expect("list").is_empty());
    assert!(trash_payloads(&vault).is_empty(), "payload stayed in vault");

    // 历史按**相对路径**归档,不跟着文件走。不清的话同路径的新笔记一出生就继承
    // 上一条的历史 —— 用户会在一份空白笔记里看到别人的旧内容。
    fs_ops::create_note(&state, &path, "fresh\n").expect("recreate");
    assert!(
        snapshots::list(&state, &path)
            .expect("list after purge")
            .is_empty(),
        "the purged note's history was inherited by a new note at the same path"
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn purging_a_folder_drops_the_history_of_the_notes_inside_it() {
    let vault = temp_vault("trash-purge-folder");
    let state = registered_state(&vault);
    let folder = vault.join("Archive");
    std::fs::create_dir_all(folder.join("deep")).expect("mkdir");
    let shallow = note_path(&folder, "one.md");
    let deep = note_path(&folder.join("deep"), "two.md");
    for path in [&shallow, &deep] {
        fs_ops::create_note(&state, path, "v1\n").expect("create");
        let opened = fs_ops::read_note(&state, path).expect("open");
        fs_ops::save_note(&state, path, "v2\n", Some(opened.sig), false).expect("save");
    }

    let item = trash::trash(&vault, &folder).expect("trash folder");
    trash::purge(&vault, &item.id).expect("purge folder");

    // 目录自己没有历史,里面每条笔记才有。逐条清 —— 只清目录路径的话嵌套那层
    // 的历史会永远留在私有目录里。
    std::fs::create_dir_all(folder.join("deep")).expect("remkdir");
    for path in [&shallow, &deep] {
        fs_ops::create_note(&state, path, "fresh\n").expect("recreate");
        assert!(
            snapshots::list(&state, path).expect("list").is_empty(),
            "history survived the purge for {path}"
        );
    }

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn trashing_a_note_keeps_its_history_so_restore_brings_it_back() {
    let vault = temp_vault("trash-keeps-history");
    let state = registered_state(&vault);
    let path = note_path(&vault, "note.md");
    fs_ops::create_note(&state, &path, "v1\n").expect("create");
    let opened = fs_ops::read_note(&state, &path).expect("open");
    fs_ops::save_note(&state, &path, "v2\n", Some(opened.sig), false).expect("save");

    let item = trash::trash(&vault, Path::new(&path)).expect("trash");
    trash::restore(&vault, &item.id).expect("restore");

    // 软删不是彻底删除,历史必须原样在。恢复回来的笔记还能继续往回滚 —— 否则
    // 「误删再恢复」会静默吃掉这条笔记的全部版本。
    assert_eq!(
        snapshots::list(&state, &path).expect("list").len(),
        1,
        "soft delete dropped the note's history"
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn purge_all_empties_the_trash_including_payloads_without_a_manifest() {
    let vault = temp_vault("trash-purge-all");
    let state = registered_state(&vault);
    for name in ["a.md", "b.md"] {
        let path = note_path(&vault, name);
        fs_ops::create_note(&state, &path, "body\n").expect("create");
        trash::trash(&vault, Path::new(&path)).expect("trash");
    }
    // 崩在"清单写完、载荷还没搬"之后留下的孤儿载荷。清空必须连它一起收走,
    // 否则回收站显示为空而磁盘上还躺着文件。
    std::fs::write(trash_dir(&vault).join("999.bin"), "orphan\n").expect("seed orphan");

    let purged = trash::purge_all(&vault).expect("purge all");

    assert_eq!(purged, 2, "count must be the manifests actually purged");
    assert!(trash::list(&vault).expect("list").is_empty());
    assert!(
        trash_payloads(&vault).is_empty(),
        "leftovers: {:?}",
        trash_payloads(&vault)
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn list_hides_entries_whose_payload_is_gone_and_puts_the_newest_first() {
    let vault = temp_vault("trash-list");
    let state = registered_state(&vault);
    let mut ids = Vec::new();
    for (name, at) in [("old.md", 1_000), ("new.md", 2_000)] {
        let path = note_path(&vault, name);
        fs_ops::create_note(&state, &path, "body\n").expect("create");
        ids.push(
            trash::trash_at(&vault, Path::new(&path), at)
                .expect("trash")
                .id,
        );
    }

    let listed = trash::list(&vault).expect("list");
    assert_eq!(
        listed
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        vec!["new.md", "old.md"],
        "newest deletion must come first"
    );

    // 载荷被外部删掉(用户自己去 Finder 里清了)的条目不该列出来:点恢复什么都
    // 不会发生,而用户以为自己的笔记还在回收站里。
    std::fs::remove_file(trash_dir(&vault).join(format!("{}.bin", ids[1]))).expect("drop payload");

    let listed = trash::list(&vault).expect("list again");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "old.md");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn trash_refuses_the_vault_root_and_its_private_directory() {
    let vault = temp_vault("trash-guards");
    let private = vault.join(".notebook").join("history");
    std::fs::create_dir_all(&private).expect("mkdir private");

    // 根目录:搬走它等于把整个 vault 塞进它自己的子目录。
    let error = trash::trash(&vault, &vault).expect_err("root must be refused");
    assert!(error.contains("vault root"), "unexpected: {error}");

    // 私有目录:回收站和历史都住在这儿。允许软删就意味着"删除回收站"会把回收站
    // 搬进回收站,而恢复又落回私有目录 —— 一个没有出口的循环。
    let error = trash::trash(&vault, &private).expect_err("private dir must be refused");
    assert!(error.contains("private directory"), "unexpected: {error}");
    assert!(private.is_dir(), "private directory was moved anyway");

    // vault 外的路径:这个函数直接动用户的文件,不能假设调用方一定先过了
    // `resolve_in_vaults`。
    let outside = temp_vault("trash-outside");
    let victim = outside.join("victim.md");
    std::fs::write(&victim, "not yours\n").expect("seed");
    let error = trash::trash(&vault, &victim).expect_err("outside must be refused");
    assert!(error.contains("outside the vault"), "unexpected: {error}");
    assert!(victim.exists(), "a file outside the vault was moved");

    std::fs::remove_dir_all(&vault).ok();
    std::fs::remove_dir_all(&outside).ok();
}

#[test]
fn restore_refuses_a_manifest_that_points_out_of_the_vault() {
    // vault 故意多套一层:这条测试要断言"`..` 指向的地方没有被写出文件",而
    // `temp_vault` 的父目录是**全机器共享**的临时目录。直接断言在那上面的话,任何
    // 来源的同名残留(比如某次真的越界写成功了的运行)都会永久毒住这条测试,而且
    // 并行跑的别的测试清理临时目录时又可能顺手把它扫掉 —— 于是同一个断言时红时绿。
    let outside = temp_vault("trash-traversal");
    let vault = outside.join("vault");
    std::fs::create_dir_all(&vault).expect("create vault");
    let state = registered_state(&vault);
    let path = note_path(&vault, "note.md");
    fs_ops::create_note(&state, &path, "body\n").expect("create");
    let item = trash::trash(&vault, Path::new(&path)).expect("trash");

    // 清单是磁盘上的 JSON:用户手改过、别的工具写过都有可能。它决定写入位置,
    // 所以必须当成不可信输入 —— `vault.join("../…")` 会一路爬出 vault。
    let manifest = trash_dir(&vault).join(format!("{}.meta.json", item.id));
    let text = std::fs::read_to_string(&manifest).expect("read manifest");
    std::fs::write(&manifest, text.replace("\"note.md\"", "\"../escaped.md\"")).expect("tamper");

    let error = trash::restore(&vault, &item.id).expect_err("must refuse");

    assert!(
        error.contains("unsafe original path"),
        "unexpected: {error}"
    );
    assert!(
        !outside.join("escaped.md").exists(),
        "a file was written outside the vault"
    );

    std::fs::remove_dir_all(&outside).ok();
}

#[test]
fn restore_refuses_a_manifest_aimed_at_the_private_directory() {
    let vault = temp_vault("trash-private-target");
    let state = registered_state(&vault);
    let path = note_path(&vault, "note.md");
    fs_ops::create_note(&state, &path, "body\n").expect("create");
    let item = trash::trash(&vault, Path::new(&path)).expect("trash");

    let manifest = trash_dir(&vault).join(format!("{}.meta.json", item.id));
    let text = std::fs::read_to_string(&manifest).expect("read manifest");
    std::fs::write(
        &manifest,
        text.replace("\"note.md\"", "\".notebook/trash/hijack.md\""),
    )
    .expect("tamper");

    let error = trash::restore(&vault, &item.id).expect_err("must refuse");

    // 往私有目录里塞用户文件轻则被树扫描忽略(笔记再也看不见),重则覆盖掉回收站
    // 自己的清单。
    assert!(error.contains("private directory"), "unexpected: {error}");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn trash_ids_from_the_frontend_cannot_escape_the_trash_directory() {
    let vault = temp_vault("trash-id-guard");

    // ID 会原样回传。不校验的话它就是个任意路径读写入口。
    for id in ["../../etc/passwd", "..", "note.md", ""] {
        let error = trash::restore(&vault, id).expect_err("restore must refuse");
        assert!(error.contains("Invalid trash entry id"), "{id}: {error}");
        let error = trash::purge(&vault, id).expect_err("purge must refuse");
        assert!(error.contains("Invalid trash entry id"), "{id}: {error}");
    }

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn trashing_a_folder_forgets_the_baselines_of_the_notes_inside_it() {
    let vault = temp_vault("trash-baselines");
    let state = registered_state(&vault);
    let folder = vault.join("Archive");
    std::fs::create_dir_all(&folder).expect("mkdir");
    let path = note_path(&folder, "note.md");
    fs_ops::create_note(&state, &path, "body\n").expect("create");
    fs_ops::read_note(&state, &path).expect("open");
    assert!(
        state.last_sig(Path::new(&path)).is_some(),
        "fixture needs a baseline"
    );

    let resolved = state
        .resolve_in_vaults(&folder.to_string_lossy(), false)
        .expect("resolve");
    trash::trash(&vault, &resolved).expect("trash folder");
    state
        .record_close_subtree(&resolved)
        .expect("forget subtree");

    // 只清目录自己的指纹不够:里面的笔记可能正开着 tab。指纹留着的话这个路径
    // 将来被复用时会拿一份属于**已经不在这里的文件**的基线去比对,而那次比对
    // 会说"磁盘没变" —— 于是静默覆盖掉别人的内容。
    assert!(
        state.last_sig(Path::new(&path)).is_none(),
        "the baseline of a note inside the trashed folder survived"
    );

    std::fs::remove_dir_all(&vault).ok();
}

// ── 迁移 ───────────────────────────────────────────────────────────────────

// 用 `r##"…"##`:正文里的 `"# Release` 含 `"#`,会提前结束 `r#"…"#`。
const TWO_NOTES: &str = r##"[
  {"id":"n1","title":"Deploy notes","body":"# Release\n\n**Ship it**","format":"markdown","updatedAt":1750000000000},
  {"id":"n2","title":"Meeting","body":"<p>Hello <b>world</b></p>","format":"richtext","updatedAt":1750000001000}
]"##;

#[test]
fn migration_writes_files_and_backs_up_first() {
    let vault = temp_vault("migrate");
    let report = migrate::migrate_legacy_notes(&vault, TWO_NOTES).expect("migrate");

    assert_eq!(report.total_input, 2);
    assert_eq!(report.migrated.len(), 2);
    assert!(report.skipped.is_empty());

    // 备份必须存在,且内容是原始 JSON 的逐字副本。
    let backup = std::fs::read_to_string(&report.backup_path).expect("backup exists");
    assert_eq!(backup, TWO_NOTES);

    let markdown = std::fs::read_to_string(vault.join("Deploy-notes.md")).expect("md note");
    assert!(markdown.contains("title: \"Deploy notes\""));
    assert!(markdown.contains("legacyId: \"n1\""));
    assert!(markdown.contains("**Ship it**"));
    // Markdown 笔记不该被标上 editor —— 缺省即 Markdown。
    assert!(!markdown.contains("editor:"));

    // 富文本笔记的正文原样保留 HTML,只加 editor 标记。迁移是无损的:
    // HTML → Markdown 的有损转换推到 P1(WYSIWYG 到位之后)。
    let rich = std::fs::read_to_string(vault.join("Meeting.md")).expect("rich note");
    assert!(rich.contains("editor: richtext"));
    assert!(
        rich.contains("<p>Hello <b>world</b></p>"),
        "richtext body must survive byte-for-byte: {rich}"
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn migration_preserves_richtext_markup_exactly() {
    let vault = temp_vault("lossless");
    // 内联 style、颜色、嵌套表格是 HTML → Markdown 最容易丢的东西。
    // P0 不转换,所以它们必须一字不差地留下来。
    let body = "<p><span style=\"color:#2563eb\">tinted</span></p><table><tbody><tr><td><ul><li>nested</li></ul></td></tr></tbody></table>";
    let json = format!(
        r#"[{{"id":"rt","title":"Fancy","body":{},"format":"richtext","updatedAt":1}}]"#,
        serde_json::to_string(body).expect("encode")
    );
    let report = migrate::migrate_legacy_notes(&vault, &json).expect("migrate");

    let text = std::fs::read_to_string(&report.migrated[0].path).expect("read");
    assert!(text.contains(body), "markup was altered: {text}");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn migration_is_idempotent_across_repeated_runs() {
    let vault = temp_vault("idempotent");

    let first = migrate::migrate_legacy_notes(&vault, TWO_NOTES).expect("first");
    assert_eq!(first.migrated.len(), 2);

    // 重复运行必须靠 legacyId 认出来,不能产出 `Deploy-notes-2.md`。
    for _ in 0..2 {
        let again = migrate::migrate_legacy_notes(&vault, TWO_NOTES).expect("repeat");
        assert!(again.migrated.is_empty(), "must not re-migrate");
        assert_eq!(again.skipped.len(), 2);
    }

    let notes: Vec<String> = std::fs::read_dir(&vault)
        .expect("read dir")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.ends_with(".md"))
        .collect();
    assert_eq!(notes.len(), 2, "unexpected files: {notes:?}");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn migration_deduplicates_identical_titles() {
    let vault = temp_vault("dupes");
    let json = r#"[
      {"id":"a","title":"Same","body":"first","format":"markdown","updatedAt":1},
      {"id":"b","title":"Same","body":"second","format":"markdown","updatedAt":2},
      {"id":"c","title":"Same","body":"third","format":"markdown","updatedAt":3}
    ]"#;
    let report = migrate::migrate_legacy_notes(&vault, json).expect("migrate");
    assert_eq!(report.migrated.len(), 3);

    assert!(vault.join("Same.md").exists());
    assert!(vault.join("Same-2.md").exists());
    assert!(vault.join("Same-3.md").exists());
    // 三条正文都要在,不能互相覆盖。
    let bodies: HashSet<String> = ["Same.md", "Same-2.md", "Same-3.md"]
        .iter()
        .map(|name| {
            let text = std::fs::read_to_string(vault.join(name)).expect("read");
            text.lines().last().unwrap_or_default().to_string()
        })
        .collect();
    assert_eq!(bodies.len(), 3, "bodies collided: {bodies:?}");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn migration_does_not_clobber_hand_written_notes() {
    let vault = temp_vault("preexisting");
    // 用户已经手写了一个同名文件(没有 legacyId)。迁移不能覆盖它。
    std::fs::write(vault.join("Same.md"), "hand written\n").expect("seed");

    let json = r#"[{"id":"a","title":"Same","body":"migrated","format":"markdown","updatedAt":1}]"#;
    let report = migrate::migrate_legacy_notes(&vault, json).expect("migrate");
    assert_eq!(report.migrated.len(), 1);

    assert_eq!(
        std::fs::read_to_string(vault.join("Same.md")).expect("read"),
        "hand written\n"
    );
    assert!(vault.join("Same-2.md").exists());

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn migration_handles_illegal_titles_and_empty_fields() {
    let vault = temp_vault("edge");
    let json = r#"[
      {"id":"slash","title":"a/b:c","body":"x","format":"markdown","updatedAt":1},
      {"id":"empty-title","title":"","body":"y","format":"markdown","updatedAt":2},
      {"id":"no-body","title":"Bodyless","format":"markdown","updatedAt":3},
      {"id":"emoji","title":"🚀 发布","body":"z","format":"markdown","updatedAt":4}
    ]"#;
    let report = migrate::migrate_legacy_notes(&vault, json).expect("migrate");
    assert_eq!(report.migrated.len(), 4);

    assert!(vault.join("a-b-c.md").exists());
    assert!(vault.join("Untitled-quick-note.md").exists());
    assert!(vault.join("Bodyless.md").exists());
    assert!(vault.join("🚀-发布.md").exists());

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn migration_assigns_stable_ids_to_entries_without_one() {
    let vault = temp_vault("noid");
    // 缺 id 的条目按序号造稳定 id,于是重跑仍然幂等。
    let json = r#"[{"title":"First","body":"a","format":"markdown","updatedAt":1}]"#;
    let first = migrate::migrate_legacy_notes(&vault, json).expect("first");
    assert_eq!(first.migrated.len(), 1);

    let again = migrate::migrate_legacy_notes(&vault, json).expect("second");
    assert!(again.migrated.is_empty(), "should recognise the same entry");
    assert_eq!(again.skipped.len(), 1);

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn migration_escapes_yaml_hostile_titles() {
    let vault = temp_vault("yaml");
    // 引号和冒号直接拼进 frontmatter 会产出解析不了的 YAML。
    let json = r#"[{"id":"q","title":"He said \"hi\": really","body":"x","format":"markdown","updatedAt":1}]"#;
    let report = migrate::migrate_legacy_notes(&vault, json).expect("migrate");

    let text = std::fs::read_to_string(&report.migrated[0].path).expect("read");
    assert!(
        text.contains(r#"title: "He said \"hi\": really""#),
        "title not escaped: {text}"
    );
    // legacyId 也要能被读回来,否则幂等失效。
    let again = migrate::migrate_legacy_notes(&vault, json).expect("repeat");
    assert_eq!(again.skipped.len(), 1);

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn migration_keeps_html_comments_in_the_body_intact() {
    let vault = temp_vault("comment");
    // 正文里本来就可能有 HTML 注释。迁移不再往文件里加注释块,所以这些注释
    // 应该原样留在正文里 —— 不被转义,也不被截断。
    let json = r#"[{"id":"c","title":"Tricky","body":"<p>a</p><!-- keep --><p>b</p>","format":"richtext","updatedAt":1}]"#;
    let report = migrate::migrate_legacy_notes(&vault, json).expect("migrate");

    let text = std::fs::read_to_string(&report.migrated[0].path).expect("read");
    assert!(
        text.contains("<p>a</p><!-- keep --><p>b</p>"),
        "body comments must survive: {text}"
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn migration_rejects_invalid_json_without_touching_the_vault() {
    let vault = temp_vault("badjson");
    let error = migrate::migrate_legacy_notes(&vault, "{not json").expect_err("must fail");
    assert!(error.contains("not valid JSON"), "unexpected: {error}");

    // 失败时 vault 里不能留下任何东西 —— 连备份目录都不该建。
    let entries: Vec<String> = std::fs::read_dir(&vault)
        .expect("read dir")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();
    assert!(entries.is_empty(), "vault should be untouched: {entries:?}");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn migration_accepts_empty_list() {
    let vault = temp_vault("emptylist");
    let report = migrate::migrate_legacy_notes(&vault, "[]").expect("migrate");
    assert_eq!(report.total_input, 0);
    assert!(report.migrated.is_empty());
    // 备份仍然要写:知道"迁移时是空的"本身也是信息。
    assert!(Path::new(&report.backup_path).exists());
    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn migration_treats_legacy_txt_format_as_richtext() {
    let vault = temp_vault("txt");
    // `txt` 是 richtext 的前身(见前端 normalizeFormat)。
    let json =
        r#"[{"id":"t","title":"Old","body":"<p>bold <b>x</b></p>","format":"txt","updatedAt":1}]"#;
    let report = migrate::migrate_legacy_notes(&vault, json).expect("migrate");
    // `txt` 也要被认成富文本,否则面板会用 Markdown 编辑器打开一堆 HTML 标签。
    assert!(report.migrated[0].richtext);

    let text = std::fs::read_to_string(&report.migrated[0].path).expect("read");
    assert!(text.contains("editor: richtext"));
    assert!(
        text.contains("<p>bold <b>x</b></p>"),
        "body altered: {text}"
    );

    std::fs::remove_dir_all(&vault).ok();
}

// ── 文件名分配 ─────────────────────────────────────────────────────────────

#[test]
fn allocate_path_dedupes_against_existing_files() {
    let vault = temp_vault("allocate");
    std::fs::write(vault.join("Plan.md"), "x").expect("seed");

    let first = migrate::allocate_note_path(&vault, "Plan").expect("allocate");
    assert_eq!(first.file_name().unwrap(), "Plan-2.md");

    std::fs::write(&first, "y").expect("seed 2");
    let second = migrate::allocate_note_path(&vault, "Plan").expect("allocate");
    assert_eq!(second.file_name().unwrap(), "Plan-3.md");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn allocate_path_dedupes_case_insensitively() {
    let vault = temp_vault("allocate-case");
    // macOS APFS 和 Windows NTFS 默认不区分大小写,`plan.md` 和 `Plan.md`
    // 在那里是同一个文件。统一按小写去重,三个平台行为一致。
    std::fs::write(vault.join("plan.md"), "x").expect("seed");
    let next = migrate::allocate_note_path(&vault, "Plan").expect("allocate");
    assert_eq!(next.file_name().unwrap(), "Plan-2.md");
    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn allocate_path_sanitises_illegal_titles() {
    let vault = temp_vault("allocate-illegal");
    let path = migrate::allocate_note_path(&vault, "a/b:c").expect("allocate");
    assert_eq!(path.file_name().unwrap(), "a-b-c.md");
    // 空标题也要给出一个能落盘的名字。
    let fallback = migrate::allocate_note_path(&vault, "").expect("allocate");
    assert_eq!(fallback.file_name().unwrap(), "untitled.md");
    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn migration_refuses_when_vault_is_missing() {
    // 备份写不进去就不能开始迁移。vault 不存在是最直接的那种失败。
    let missing = std::env::temp_dir().join("aeroric-notebook-definitely-absent-dir");
    std::fs::remove_dir_all(&missing).ok();
    assert!(migrate::migrate_legacy_notes(&missing, TWO_NOTES).is_err());
}

// ── 富文本 → Markdown 转换 ─────────────────────────────────────────────────
//
// 这些测试守的是 P1 收尾迁移的正确性:随手记的富文本编辑器产出的 HTML 转成
// Markdown 时,哪些能无损、哪些必然有损。有损的部分要**明确**,不能是意外。

/// 富文本编辑器实际会产出的 HTML(见 NotebookPanel 的工具栏命令)。
fn convert(html: &str) -> String {
    super::html2md::html_to_markdown(html, false)
}

#[test]
fn richtext_converts_standard_inline_formatting() {
    assert_eq!(convert("<p>a <b>bold</b> b</p>"), "a **bold** b");
    assert_eq!(convert("<p>a <i>ital</i> b</p>"), "a *ital* b");
    assert_eq!(convert("<p>a <strike>gone</strike> b</p>"), "a ~~gone~~ b");
}

#[test]
fn richtext_preserves_formats_markdown_cannot_express() {
    // markdown 没有下划线 / 高亮 / 颜色的语法,但**允许内联 HTML**。原样保留,
    // 否则用户迁移后会发现标注全没了。渲染侧的 DOMPurify 已放行这些标签。
    assert_eq!(convert("<p>a <u>under</u> b</p>"), "a <u>under</u> b");
    assert_eq!(convert("<p>a <mark>hi</mark> b</p>"), "a <mark>hi</mark> b");
    assert_eq!(
        convert("<p><span style=\"color:#2563eb\">tinted</span></p>"),
        "<span style=\"color:#2563eb\">tinted</span>"
    );
    assert_eq!(
        convert("<p><span style=\"background-color:#fef08a\">bg</span></p>"),
        "<span style=\"background-color:#fef08a\">bg</span>"
    );
}

#[test]
fn richtext_drops_structural_spans_without_style() {
    // contentEditable 会塞一堆没有样式的 span。留着只会让正文全是空标签。
    assert_eq!(convert("<p><span>plain</span></p>"), "plain");
    // 有 style 但不含 color(比如只有 font-weight)也不留 —— 加粗有 markdown 语法。
    assert_eq!(
        convert("<p><span style=\"font-weight:700\">x</span></p>"),
        "x"
    );
}

#[test]
fn richtext_span_close_tags_stay_paired() {
    // 保留的 span 才补 `</span>`;不配对会产出孤立闭标签,渲染出来是乱的。
    let mixed = convert("<p><span>a</span><span style=\"color:red\">b</span><span>c</span></p>");
    assert_eq!(
        mixed.matches("<span").count(),
        mixed.matches("</span>").count()
    );
    assert_eq!(mixed, "a<span style=\"color:red\">b</span>c");
}

#[test]
fn richtext_escapes_quotes_in_style_attribute() {
    // 裸引号会把属性提前闭合,后面的内容就跑到标签外面去了。
    let out = convert("<p><span style='color:red;font-family:\"X\"'>q</span></p>");
    assert!(out.contains("&quot;"), "quotes not escaped: {out}");
}

#[test]
fn richtext_code_block_skips_the_language_dropdown() {
    // 富文本编辑器往每个代码块里塞了语言选择下拉。不整段丢掉的话,所有选项的
    // 文字(Text / SQL / Python…)会被当成代码内容拼进正文 —— 用户会看到自己的
    // 代码前面多出一串语言名。
    let html = "<pre data-notebook-code-block=\"true\">\
<select data-notebook-code-language=\"true\">\
<option value=\"text\">Text</option><option value=\"sql\">SQL</option>\
</select><code data-language=\"sql\">SELECT 1;</code></pre>";
    let out = convert(html);
    assert_eq!(out, "```\nSELECT 1;\n```");
    assert!(!out.contains("Text"), "dropdown leaked into code: {out}");
    assert!(!out.contains("SQL"), "dropdown leaked into code: {out}");
}

#[test]
fn richtext_converts_lists_headings_tables() {
    assert_eq!(
        convert("<ul><li>one</li><li>two</li></ul>"),
        "- one\n\n- two"
    );
    assert_eq!(
        convert("<ol><li>one</li><li>two</li></ol>"),
        "1. one\n\n2. two"
    );
    assert_eq!(convert("<h1>Head</h1><h2>Sub</h2>"), "# Head\n\n## Sub");
    assert_eq!(
        convert(
            "<table><thead><tr><th>A</th></tr></thead><tbody><tr><td>1</td></tr></tbody></table>"
        ),
        "| A |\n| --- |\n| 1 |"
    );
}

#[test]
fn richtext_keeps_text_when_structure_cannot_survive() {
    // markdown 的表格单元格里放不下列表。结构会变,但**文字不能丢** ——
    // 这是「有损」的下限:用户能看到内容,只是排版退化。
    let out = convert("<table><tbody><tr><td><ul><li>nested</li></ul></td></tr></tbody></table>");
    assert!(out.contains("nested"), "text was lost: {out}");
}

#[test]
fn richtext_line_breaks_become_hard_breaks() {
    // 两个尾随空格是 markdown 的硬换行。
    assert_eq!(convert("<p>a<br>b</p>"), "a  \nb");
}

#[test]
fn richtext_conversion_is_idempotent_for_preserved_html() {
    // 保留下来的内联 HTML 再转一次不该被二次包裹或吃掉 —— 收尾迁移可能被重跑。
    let once = convert("<p>a <u>u</u> <mark>m</mark></p>");
    let twice = convert(&format!("<p>{once}</p>"));
    assert_eq!(once, twice);
}

// ── P1 收尾迁移:富文本笔记 → Markdown ──────────────────────────────────────

#[test]
fn convert_richtext_rewrites_marked_notes_and_clears_the_flag() {
    let vault = temp_vault("convert");
    std::fs::write(
        vault.join("Rich.md"),
        "---\ntitle: \"Rich\"\neditor: richtext\ncustom: keep\n---\n\n<p>a <b>bold</b></p>\n",
    )
    .expect("seed");

    let report = migrate::convert_richtext_notes(&vault).expect("convert");
    assert_eq!(report.converted.len(), 1);
    assert_eq!(report.converted[0].title, "Rich");

    let text = std::fs::read_to_string(vault.join("Rich.md")).expect("read");
    assert!(text.contains("a **bold**"), "not converted: {text}");
    // editor 标记要清掉,否则重跑会再转一次(而 HTML 已经不在了)。
    assert!(!text.contains("editor:"), "flag not cleared: {text}");
    // 其它 frontmatter 字段原样保留 —— 可能是第三方工具写的。
    assert!(text.contains("title: \"Rich\""));
    assert!(text.contains("custom: keep"));

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn convert_richtext_backs_up_before_touching_anything() {
    let vault = temp_vault("convert-backup");
    let original = "---\ntitle: \"B\"\neditor: richtext\n---\n\n<p>original</p>\n";
    std::fs::write(vault.join("B.md"), original).expect("seed");

    let report = migrate::convert_richtext_notes(&vault).expect("convert");

    // 备份必须是转换前的逐字副本 —— 用户发现转换不满意时的唯一退路。
    let backup =
        std::fs::read_to_string(Path::new(&report.backup_dir).join("B.md")).expect("backup exists");
    assert_eq!(backup, original);

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn convert_richtext_leaves_markdown_notes_alone() {
    let vault = temp_vault("convert-skip");
    let plain = "---\ntitle: \"Plain\"\n---\n\n# Already markdown\n";
    std::fs::write(vault.join("Plain.md"), plain).expect("seed");

    let report = migrate::convert_richtext_notes(&vault).expect("convert");
    assert!(report.converted.is_empty());
    assert_eq!(report.skipped, 1);
    // 一个字节都不该动。
    assert_eq!(
        std::fs::read_to_string(vault.join("Plain.md")).expect("read"),
        plain
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn convert_richtext_is_idempotent() {
    let vault = temp_vault("convert-idem");
    std::fs::write(
        vault.join("R.md"),
        "---\ntitle: \"R\"\neditor: richtext\n---\n\n<p>x</p>\n",
    )
    .expect("seed");

    let first = migrate::convert_richtext_notes(&vault).expect("first");
    assert_eq!(first.converted.len(), 1);
    let after_first = std::fs::read_to_string(vault.join("R.md")).expect("read");

    // 重跑:标记已清,该被跳过,内容不变。
    let second = migrate::convert_richtext_notes(&vault).expect("second");
    assert!(second.converted.is_empty());
    assert_eq!(second.skipped, 1);
    assert_eq!(
        std::fs::read_to_string(vault.join("R.md")).expect("read"),
        after_first
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn convert_richtext_preserves_underline_and_colors() {
    let vault = temp_vault("convert-lossless");
    std::fs::write(
        vault.join("Fancy.md"),
        "---\ntitle: \"F\"\neditor: richtext\n---\n\n<p><u>u</u> <mark>m</mark> <span style=\"color:red\">c</span></p>\n",
    )
    .expect("seed");

    migrate::convert_richtext_notes(&vault).expect("convert");

    let text = std::fs::read_to_string(vault.join("Fancy.md")).expect("read");
    // 这三种格式 markdown 没有语法,靠保留内联 HTML 做到无损。
    assert!(text.contains("<u>u</u>"), "underline lost: {text}");
    assert!(text.contains("<mark>m</mark>"), "highlight lost: {text}");
    assert!(text.contains("color:red"), "color lost: {text}");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn convert_richtext_handles_a_note_without_frontmatter() {
    let vault = temp_vault("convert-nofront");
    // 没有 frontmatter 就不该被认成富文本 —— 没有 editor 标记。
    std::fs::write(vault.join("Bare.md"), "<p>bare html</p>\n").expect("seed");

    let report = migrate::convert_richtext_notes(&vault).expect("convert");
    assert!(report.converted.is_empty());
    assert_eq!(report.skipped, 1);

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn convert_richtext_reports_empty_vault_without_creating_backup() {
    let vault = temp_vault("convert-empty");
    let report = migrate::convert_richtext_notes(&vault).expect("convert");
    assert!(report.converted.is_empty());
    // 没有待转文件就不该建备份目录,免得 vault 里攒一堆空目录。
    assert!(!Path::new(&report.backup_dir).exists());
    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn convert_richtext_refuses_when_vault_is_missing() {
    let missing = std::env::temp_dir().join("aeroric-notebook-convert-absent");
    std::fs::remove_dir_all(&missing).ok();
    assert!(migrate::convert_richtext_notes(&missing).is_err());
}

#[test]
fn convert_richtext_falls_back_to_file_stem_for_title() {
    let vault = temp_vault("convert-title");
    // 没有 title 字段时报告里用文件名,不能是空字符串(UI 要显示它)。
    std::fs::write(
        vault.join("Untitled-One.md"),
        "---\neditor: richtext\n---\n\n<p>x</p>\n",
    )
    .expect("seed");

    let report = migrate::convert_richtext_notes(&vault).expect("convert");
    assert_eq!(report.converted[0].title, "Untitled-One");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn convert_richtext_drops_frontmatter_block_when_only_editor_remains() {
    let vault = temp_vault("convert-onlyeditor");
    std::fs::write(
        vault.join("Only.md"),
        "---\neditor: richtext\n---\n\n<p>body</p>\n",
    )
    .expect("seed");

    migrate::convert_richtext_notes(&vault).expect("convert");

    let text = std::fs::read_to_string(vault.join("Only.md")).expect("read");
    // 清掉 editor 后 frontmatter 空了,不该留一个空的 `---\n---`。
    assert!(!text.starts_with("---"), "empty frontmatter kept: {text}");
    assert!(text.contains("body"));

    std::fs::remove_dir_all(&vault).ok();
}

// ── 附件 ───────────────────────────────────────────────────────────────────

fn attachment_dir(vault: &Path) -> PathBuf {
    attachments::dir(vault)
}

fn attachment_names(vault: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(attachment_dir(vault))
        .map(|read| {
            read.flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn saving_an_attachment_lands_in_the_attachment_dir_and_returns_markdown() {
    let vault = temp_vault("attach-save");
    let note = vault.join("Note.md");
    std::fs::write(&note, "body\n").expect("seed");

    let saved =
        attachments::save_bytes(&vault, &note, Some("shot.png"), "image/png", b"\x89PNG\r\n")
            .expect("save");

    // 链接必须相对**笔记所在目录**,这条笔记在 vault 根下,所以没有 `../`。
    assert_eq!(saved.link, format!("attachments/{}", saved.name));
    assert!(
        saved.markdown.starts_with("!["),
        "not an image: {}",
        saved.markdown
    );
    assert!(saved.markdown.ends_with(&format!("({})", saved.link)));
    assert_eq!(attachment_names(&vault), vec![saved.name.clone()]);
    assert_eq!(
        std::fs::read(&saved.path).expect("read back"),
        b"\x89PNG\r\n"
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn attachment_links_climb_out_of_nested_note_directories() {
    let vault = temp_vault("attach-nested");
    let folder = vault.join("a").join("b");
    std::fs::create_dir_all(&folder).expect("mkdir");
    let note = folder.join("Deep.md");
    std::fs::write(&note, "body\n").expect("seed");

    let saved =
        attachments::save_bytes(&vault, &note, Some("x.png"), "image/png", b"png").expect("save");

    // 附件是平铺在 vault 根下的,子目录里的笔记必须爬回去。写成
    // `attachments/x.png` 的话在别的 markdown 工具里就是一条断链。
    assert_eq!(saved.link, format!("../../attachments/{}", saved.name));

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn same_millisecond_attachments_do_not_overwrite_each_other() {
    let vault = temp_vault("attach-race");
    let dir = attachment_dir(&vault);

    // 时间戳写死,不然两次保存之间隔着一次 `sync_all`,现实里几乎永远落在不同
    // 毫秒 —— 撞名这条路径就永远走不到,而它正是"先 exists 再写"会丢图的地方。
    let first = attachments::write_claimed(&dir, "note", "png", 1_700_000_000_000, b"first")
        .expect("first");
    let second = attachments::write_claimed(&dir, "note", "png", 1_700_000_000_000, b"second")
        .expect("second");

    assert_ne!(first, second, "the second write reused the first name");
    assert_eq!(std::fs::read(&first).expect("read a"), b"first");
    assert_eq!(std::fs::read(&second).expect("read b"), b"second");
    assert_eq!(attachment_names(&vault).len(), 2);

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn attachment_names_stay_under_the_filesystem_byte_limit() {
    let vault = temp_vault("attach-long");
    // 多数文件系统的单段名上限是 255 **字节**,一个 CJK 字符占 3 字节。不截断的
    // 话这条笔记的附件名就是 ENAMETOOLONG,保存直接失败。
    let long_stem = "安".repeat(200);
    let note = vault.join(format!("{long_stem}.md"));
    std::fs::write(&note, "body\n").expect("seed");

    let saved =
        attachments::save_bytes(&vault, &note, Some("x.png"), "image/png", b"png").expect("save");

    assert!(
        saved.name.len() < 255,
        "{} bytes: {}",
        saved.name.len(),
        saved.name
    );
    assert!(Path::new(&saved.path).exists());

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn svg_attachments_render_as_images() {
    let vault = temp_vault("attach-svg");
    let note = vault.join("Note.md");
    std::fs::write(&note, "body\n").expect("seed");

    let saved =
        attachments::save_bytes(&vault, &note, None, "image/svg+xml", b"<svg/>").expect("save");

    // SVG 是图片。走成普通链接的话拖一个图标进笔记,页面上只有一行蓝字。
    assert!(saved.name.ends_with(".svg"), "unexpected: {}", saved.name);
    assert!(
        saved.markdown.starts_with("!["),
        "svg not an image: {}",
        saved.markdown
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn listing_does_not_follow_symlinks_out_of_the_vault() {
    let vault = temp_vault("attach-list-symlink");
    let outside = temp_vault("attach-list-symlink-outside");
    std::fs::write(outside.join("secret.png"), b"x").expect("seed");
    std::fs::create_dir_all(attachment_dir(&vault)).expect("mkdir");
    std::fs::write(attachment_dir(&vault).join("real.png"), b"x").expect("seed");

    #[cfg(unix)]
    {
        // 跟随链接有两个后果:vault 外的文件被当成"vault 里的附件"列出来,以及
        // 一条指回父目录的链接把扫描拖进无限循环。
        std::os::unix::fs::symlink(&outside, vault.join("linked")).expect("symlink dir");
        std::os::unix::fs::symlink(outside.join("secret.png"), vault.join("linked.png"))
            .expect("symlink file");

        let names: Vec<String> = attachments::list(&vault, 50)
            .expect("list")
            .into_iter()
            .map(|item| item.name)
            .collect();
        assert_eq!(names, vec!["real.png"], "symlink was followed");
    }

    std::fs::remove_dir_all(&vault).ok();
    std::fs::remove_dir_all(&outside).ok();
}

#[test]
fn non_image_attachments_become_plain_links() {
    let vault = temp_vault("attach-pdf");
    let note = vault.join("Note.md");
    std::fs::write(&note, "body\n").expect("seed");

    let saved =
        attachments::save_bytes(&vault, &note, Some("paper.pdf"), "application/pdf", b"%PDF")
            .expect("save");

    // `![](x.pdf)` 只会渲染成一个坏掉的图片框,PDF 要走普通链接。
    assert!(
        !saved.markdown.starts_with("!"),
        "pdf as image: {}",
        saved.markdown
    );
    assert!(saved.markdown.starts_with(&format!("[{}]", saved.name)));

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn clipboard_attachments_take_their_extension_from_the_mime_type() {
    let vault = temp_vault("attach-mime");
    let note = vault.join("Note.md");
    std::fs::write(&note, "body\n").expect("seed");

    // 剪贴板里的图没有文件名,只有 mime。认不出扩展名的话文件会叫 `.bin`,
    // 于是附件面板不认它、系统也不知道拿什么打开。
    let saved = attachments::save_bytes(&vault, &note, None, "image/webp", b"RIFF").expect("save");

    assert!(
        saved.name.ends_with(".webp"),
        "unexpected name: {}",
        saved.name
    );
    assert_eq!(attachments::kind_of(&saved.name), Some("image"));

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn attachment_names_keep_cjk_and_never_carry_path_separators() {
    let vault = temp_vault("attach-name");
    // 笔记名里带斜杠、空格和标点 —— 附件名是从它派生的,洗不干净就等于让笔记名
    // 决定附件写到哪去。
    let note = vault.join("安全 报告 v2.md");
    std::fs::write(&note, "body\n").expect("seed");

    let saved = attachments::save_bytes(&vault, &note, Some("图 1.png"), "image/png", b"png")
        .expect("save");

    // CJK 留着:中文笔记名占多数,洗成 `----` 等于附件名全都认不出来。
    assert!(
        saved.name.starts_with("安全-报告-v2-"),
        "stem lost: {}",
        saved.name
    );
    assert!(!saved.name.contains('/') && !saved.name.contains('\\') && !saved.name.contains(' '));
    // 附件必须真的落在附件目录里,而不是被名字带到别处。
    assert_eq!(
        Path::new(&saved.path).parent(),
        Some(attachment_dir(&vault).as_path())
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn attachment_names_never_come_out_empty() {
    let vault = temp_vault("attach-name-empty");
    // 纯标点的笔记名洗完什么都不剩。返回空串的话文件会叫 `-1730000000000.png`,
    // 更糟的是 `.png` —— 一个隐藏文件。
    let note = vault.join("!!!.md");
    std::fs::write(&note, "body\n").expect("seed");

    let saved =
        attachments::save_bytes(&vault, &note, Some("x.png"), "image/png", b"png").expect("save");

    assert!(
        saved.name.starts_with("attachment-"),
        "unexpected: {}",
        saved.name
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn image_alt_text_stays_inside_the_markdown_link() {
    let vault = temp_vault("attach-alt");
    let note = vault.join("Note.md");
    std::fs::write(&note, "body\n").expect("seed");

    let saved = attachments::save_bytes(&vault, &note, Some("a]b[c.png"), "image/png", b"png")
        .expect("save");

    // `]` 会提前闭合 alt,于是 `![a]b[c](…)` 在页面上是一段字面文本加一条指向
    // 别处的链接。alt 是从**用户给的文件名**来的,所以这是外部输入。
    assert_eq!(saved.markdown, format!("![a-b-c]({})", saved.link));

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn image_alt_text_does_not_leak_the_timestamp() {
    let vault = temp_vault("attach-alt-stamp");
    let note = vault.join("Note.md");
    std::fs::write(&note, "body\n").expect("seed");

    let saved = attachments::save_bytes(&vault, &note, Some("logo.png"), "image/png", b"png")
        .expect("save");

    // 最终文件名带毫秒时间戳。拿它当 alt 的话,图渲染不出来时页面上显示的是
    // 一串数字,用户看不出那本来是什么。
    assert_eq!(saved.markdown, format!("![logo]({})", saved.link));
    assert!(
        saved.name.contains('-'),
        "stamp missing from name: {}",
        saved.name
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn saving_refuses_a_note_outside_the_vault() {
    let vault = temp_vault("attach-outside");
    let outside = temp_vault("attach-outside-other");
    let note = outside.join("Note.md");
    std::fs::write(&note, "body\n").expect("seed");

    let error = attachments::save_bytes(&vault, &note, Some("x.png"), "image/png", b"png")
        .expect_err("must refuse");
    assert!(error.contains("outside the vault"), "unexpected: {error}");
    // 附件目录都不该被建出来。
    assert!(!attachment_dir(&vault).exists());

    std::fs::remove_dir_all(&vault).ok();
    std::fs::remove_dir_all(&outside).ok();
}

#[test]
fn copying_an_attachment_from_disk_refuses_symlinks() {
    let vault = temp_vault("attach-symlink");
    let note = vault.join("Note.md");
    std::fs::write(&note, "body\n").expect("seed");
    let secret = temp_vault("attach-symlink-target").join("secret.png");
    std::fs::create_dir_all(secret.parent().expect("parent")).ok();
    std::fs::write(&secret, b"secret").expect("seed secret");
    let link = vault.parent().expect("parent").join("link-to-secret.png");
    std::fs::remove_file(&link).ok();

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&secret, &link).expect("symlink");
        // 拖进来的可能是一条 symlink。跟随它就等于允许把任意位置的文件复制进
        // vault,而用户看到的只是一个图片名。
        let error =
            attachments::save_from_path(&vault, &note, &link).expect_err("must refuse symlink");
        assert!(error.contains("symbolic link"), "unexpected: {error}");
        assert!(!attachment_dir(&vault).exists());
    }

    std::fs::remove_file(&link).ok();
    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn copying_an_attachment_from_disk_keeps_its_extension() {
    let vault = temp_vault("attach-from-disk");
    let note = vault.join("Note.md");
    std::fs::write(&note, "body\n").expect("seed");
    let source = temp_vault("attach-from-disk-src").join("original.jpg");
    std::fs::create_dir_all(source.parent().expect("parent")).ok();
    std::fs::write(&source, b"jpeg-bytes").expect("seed src");

    let saved = attachments::save_from_path(&vault, &note, &source).expect("save");

    assert!(saved.name.ends_with(".jpg"), "unexpected: {}", saved.name);
    assert_eq!(std::fs::read(&saved.path).expect("read"), b"jpeg-bytes");
    // 复制,不是移动:源文件是用户自己的,不能因为拖了一下就消失。
    assert!(source.exists(), "source file was moved away");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn listing_finds_attachments_next_to_notes_and_puts_the_newest_first() {
    let vault = temp_vault("attach-list");
    std::fs::create_dir_all(vault.join("assets")).expect("mkdir");
    // 用户从别处导入的笔记会把图片放在笔记旁边。只列 `attachments/` 里的那些
    // 等于对导入的内容视而不见。
    std::fs::write(vault.join("assets/old.png"), b"old").expect("seed old");
    std::fs::create_dir_all(attachment_dir(&vault)).expect("mkdir attach");
    std::fs::write(attachment_dir(&vault).join("new.png"), b"new").expect("seed new");
    // markdown 和认不出扩展名的文件都不是附件。
    std::fs::write(vault.join("Note.md"), "body\n").expect("seed note");
    std::fs::write(vault.join(".DS_Store"), b"junk").expect("seed junk");

    let listed = attachments::list(&vault, 50).expect("list");

    let names: Vec<&str> = listed.iter().map(|item| item.name.as_str()).collect();
    assert_eq!(names.len(), 2, "unexpected list: {names:?}");
    assert!(names.contains(&"old.png") && names.contains(&"new.png"));
    assert!(listed.iter().all(|item| item.kind == "image"));
    // 相对路径要带上子目录,UI 靠它告诉用户"这个附件在哪"。
    let old = listed
        .iter()
        .find(|item| item.name == "old.png")
        .expect("old");
    assert_eq!(old.relative_path, "assets/old.png");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn listing_skips_the_private_directory_and_build_output() {
    let vault = temp_vault("attach-skip");
    for dir in [".notebook/trash", "node_modules/pkg", "target/debug"] {
        std::fs::create_dir_all(vault.join(dir)).expect("mkdir");
        std::fs::write(vault.join(dir).join("hidden.png"), b"x").expect("seed");
    }
    std::fs::create_dir_all(attachment_dir(&vault)).expect("mkdir");
    std::fs::write(attachment_dir(&vault).join("real.png"), b"x").expect("seed");

    let listed = attachments::list(&vault, 50).expect("list");

    // 回收站里躺着的图不是"vault 里的附件" —— 列出来会让用户以为自己还引用着
    // 它。node_modules / target 里的更是扫都不该扫。
    let names: Vec<&str> = listed.iter().map(|item| item.name.as_str()).collect();
    assert_eq!(names, vec!["real.png"]);

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn the_attachment_dir_does_not_show_up_as_an_empty_folder_in_the_tree() {
    let vault = temp_vault("attach-tree");
    let state = registered_state(&vault);
    std::fs::create_dir_all(attachment_dir(&vault)).expect("mkdir");
    std::fs::write(attachment_dir(&vault).join("x.png"), b"png").expect("seed");
    std::fs::write(vault.join("Note.md"), "body\n").expect("seed note");

    let tree = fs_ops::read_tree(&state, &vault.to_string_lossy()).expect("tree");

    // 树只收目录和笔记文件,附件目录里全是图片 —— 留着它用户就看到一个永远
    // 展不开的空文件夹,而附件面板同时说里面有图。
    assert!(
        tree.iter()
            .all(|entry| entry.name != attachments::ATTACHMENT_DIR),
        "attachment dir leaked into the tree: {:?}",
        tree.iter().map(|entry| &entry.name).collect::<Vec<_>>()
    );
    assert!(tree.iter().any(|entry| entry.name == "Note.md"));

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn reading_an_attachment_refuses_a_directory() {
    let vault = temp_vault("attach-read-dir");
    std::fs::create_dir_all(attachment_dir(&vault)).expect("mkdir");

    let error = attachments::read(&attachment_dir(&vault)).expect_err("must refuse");
    assert!(error.contains("is a directory"), "unexpected: {error}");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn reading_an_attachment_returns_the_exact_bytes() {
    let vault = temp_vault("attach-read");
    let note = vault.join("Note.md");
    std::fs::write(&note, "body\n").expect("seed");
    let saved = attachments::save_bytes(&vault, &note, Some("x.png"), "image/png", b"\x00\x01\x02")
        .expect("save");

    // 前端要拿这些字节做 blob URL。少一个字节图就废了,所以是逐字节相等。
    assert_eq!(
        attachments::read(Path::new(&saved.path)).expect("read"),
        b"\x00\x01\x02"
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn attachments_reject_empty_and_oversized_payloads() {
    let vault = temp_vault("attach-limits");
    let note = vault.join("Note.md");
    std::fs::write(&note, "body\n").expect("seed");

    // 空字节:粘贴路径上拿到空剪贴板时会走到这里。放过去的话笔记里多一条指向
    // 0 字节文件的死链接,而用户以为图片存下来了。
    let empty = attachments::save_bytes(&vault, &note, Some("x.png"), "image/png", b"")
        .expect_err("must refuse empty");
    assert!(empty.contains("empty"), "unexpected: {empty}");

    // 超限:上限存在的意义是别让一个 vault 因为误拖了一个视频而变成几个 GB。
    let huge = vec![0u8; 25 * 1024 * 1024 + 1];
    let too_big = attachments::save_bytes(&vault, &note, Some("x.png"), "image/png", &huge)
        .expect_err("must refuse oversized");
    assert!(too_big.contains("too large"), "unexpected: {too_big}");

    // 两条都不该在附件目录里留下半个文件。
    assert!(
        attachment_names(&vault).is_empty(),
        "{:?}",
        attachment_names(&vault)
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn note_stat_reports_the_size_and_mtime_on_disk() {
    let vault = temp_vault("stat-basic");
    let note = vault.join("Note.md");
    std::fs::write(&note, "hello\n").expect("seed");

    let stat = super::stat_note(&note).expect("stat");

    assert_eq!(stat.size, 6);
    // 属性面板会把它格式化成日期。0 会显示成 1970,那比留空更容易被当成真的。
    assert!(stat.modified_ms > 1_600_000_000_000, "{}", stat.modified_ms);
    // 创建时间要么真的取不到(部分 Linux 文件系统不记),要么是个真实时间戳。
    // 取不到时报 `Some(0)` 会在面板上变成一行 1970-01-01,而面板正是靠 None
    // 来决定整行不显示的。
    match stat.created_ms {
        None => {}
        Some(created) => assert!(created > 1_600_000_000_000, "{created}"),
    }

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn note_stat_follows_the_file_not_the_link() {
    let vault = temp_vault("stat-symlink");
    let real = vault.join("Real.md");
    std::fs::write(&real, "0123456789").expect("seed");
    let link = vault.join("Link.md");

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        let stat = super::stat_note(&link).expect("stat");
        // 跟进软链的话这里会是 10 —— 面板报的就成了别处那个文件的大小。
        assert_ne!(stat.size, 10);
    }
    #[cfg(not(unix))]
    {
        let _ = link;
    }

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn note_stat_refuses_a_directory() {
    let vault = temp_vault("stat-dir");
    let dir = vault.join("folder");
    std::fs::create_dir_all(&dir).expect("seed");

    // 目录的 len() 是个和内容无关的数字(不同文件系统上从 0 到几 KB 不等),
    // 报出来只会让人以为笔记有那么大。
    let error = super::stat_note(&dir).expect_err("must refuse");
    assert!(error.contains("is a directory"), "unexpected: {error}");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn note_stat_reports_a_missing_file_instead_of_zeroes() {
    let vault = temp_vault("stat-missing");
    std::fs::create_dir_all(&vault).expect("seed");

    // 报 0 字节的话面板会显示成"一条空笔记",而真实情况是文件已经不在了。
    let error = super::stat_note(&vault.join("Gone.md")).expect_err("must fail");
    assert!(error.contains("Cannot read"), "unexpected: {error}");

    std::fs::remove_dir_all(&vault).ok();
}

// ── vault_index ────────────────────────────────────────────────────────────

/// 索引里某条路径对应的标题。
fn indexed_title(entries: &[super::vault_index::VaultIndexEntry], name: &str) -> Option<String> {
    entries
        .iter()
        .find(|entry| entry.path.ends_with(name))
        .map(|entry| entry.title.clone())
}

#[test]
fn vault_index_prefers_the_frontmatter_title_over_the_file_name() {
    let vault = temp_vault("index-front");
    // 这正是索引存在的理由:文件名是新建时的 slug,标题后来改过。少了这一档,
    // `[[周报]]` 在目标笔记被打开过之前解析不到。
    std::fs::write(
        vault.join("cao-gao.md"),
        "---\ntitle: 周报\ntags: []\n---\n\n# 别的标题\n",
    )
    .expect("seed");

    let entries = super::vault_index::scan_vault_titles(&vault).expect("scan");
    assert_eq!(
        indexed_title(&entries, "cao-gao.md").as_deref(),
        Some("周报")
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn vault_index_falls_back_to_the_first_heading_then_the_stem() {
    let vault = temp_vault("index-fallback");
    std::fs::write(vault.join("a.md"), "# 第一个标题\n\n正文\n").expect("seed");
    std::fs::write(vault.join("b.md"), "没有标题的正文\n").expect("seed");
    // `#hashtag` 不是标题(缺空格),`####### ` 也不是(七个 #)。两者都该回落到文件名。
    std::fs::write(vault.join("c.md"), "#hashtag\n####### 太多井号\n").expect("seed");
    // frontmatter 里空的 title 不算给了标题。
    std::fs::write(vault.join("d.md"), "---\ntitle:\n---\n\n# 正文标题\n").expect("seed");

    let entries = super::vault_index::scan_vault_titles(&vault).expect("scan");
    assert_eq!(
        indexed_title(&entries, "a.md").as_deref(),
        Some("第一个标题")
    );
    assert_eq!(indexed_title(&entries, "b.md").as_deref(), Some("b"));
    assert_eq!(indexed_title(&entries, "c.md").as_deref(), Some("c"));
    assert_eq!(indexed_title(&entries, "d.md").as_deref(), Some("正文标题"));

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn vault_index_unquotes_frontmatter_scalars() {
    let vault = temp_vault("index-quotes");
    // 带 `:` 的标题必须加引号才是合法 YAML,所以这条路径是常态而非边角。
    std::fs::write(
        vault.join("a.md"),
        "---\ntitle: \"Q1: 计划\"\n---\n\n正文\n",
    )
    .expect("seed");
    std::fs::write(
        vault.join("b.md"),
        "---\ntitle: 'it''s here'\n---\n\n正文\n",
    )
    .expect("seed");
    std::fs::write(
        vault.join("c.md"),
        "---\ntitle: \"a \\\"b\\\"\"\n---\n\n正文\n",
    )
    .expect("seed");

    let entries = super::vault_index::scan_vault_titles(&vault).expect("scan");
    assert_eq!(indexed_title(&entries, "a.md").as_deref(), Some("Q1: 计划"));
    assert_eq!(
        indexed_title(&entries, "b.md").as_deref(),
        Some("it's here")
    );
    assert_eq!(indexed_title(&entries, "c.md").as_deref(), Some("a \"b\""));

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn vault_index_ignores_an_unterminated_frontmatter_block() {
    let vault = temp_vault("index-unterminated");
    // 开了 `---` 却没闭合的是正文里的一条分隔线。把它当 frontmatter 会让
    // 「title:」这行字面量变成标题,而前端 `splitNote` 不这么认 —— 两边算出
    // 不同的标题就会出现「列表里叫 A、链接解析成 B」。
    std::fs::write(vault.join("a.md"), "---\ntitle: 假的\n\n# 真标题\n").expect("seed");

    let entries = super::vault_index::scan_vault_titles(&vault).expect("scan");
    assert_eq!(indexed_title(&entries, "a.md").as_deref(), Some("真标题"));

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn vault_index_reads_titles_from_nested_folders_and_skips_private_dirs() {
    let vault = temp_vault("index-nested");
    std::fs::create_dir_all(vault.join("sub/deeper")).expect("mkdir");
    std::fs::create_dir_all(vault.join(".notebook/history")).expect("mkdir");
    std::fs::create_dir_all(vault.join("node_modules")).expect("mkdir");
    std::fs::write(vault.join("sub/deeper/n.md"), "# 深处\n").expect("seed");
    std::fs::write(vault.join(".notebook/history/old.md"), "# 快照\n").expect("seed");
    std::fs::write(vault.join("node_modules/dep.md"), "# 依赖\n").expect("seed");
    std::fs::write(vault.join("plain.txt"), "# 不是笔记\n").expect("seed");

    let entries = super::vault_index::scan_vault_titles(&vault).expect("scan");
    let titles: Vec<&str> = entries.iter().map(|e| e.title.as_str()).collect();
    // 历史快照和依赖目录里的 .md 出现在索引里会让 `[[快照]]` 指到一个用户在树里
    // 根本看不到的文件。
    assert_eq!(titles, vec!["深处"], "unexpected index: {titles:?}");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn vault_index_sorts_by_path_so_ambiguous_links_are_stable() {
    let vault = temp_vault("index-order");
    std::fs::create_dir_all(vault.join("a")).expect("mkdir");
    std::fs::create_dir_all(vault.join("b")).expect("mkdir");
    // 两篇同名笔记。前端遇到歧义取第一篇 —— 如果索引顺序跟着文件系统的遍历顺序
    // 走,同一次点击今天进 a/、明天进 b/。
    std::fs::write(vault.join("b/dup.md"), "---\ntitle: 重名\n---\n").expect("seed");
    std::fs::write(vault.join("a/dup.md"), "---\ntitle: 重名\n---\n").expect("seed");
    std::fs::write(vault.join("m.md"), "# 中间\n").expect("seed");

    let paths: Vec<String> = super::vault_index::scan_vault_titles(&vault)
        .expect("scan")
        .into_iter()
        .map(|e| e.path)
        .collect();
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted, "index order is not stable");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn vault_index_does_not_follow_symlinks() {
    let vault = temp_vault("index-symlink");
    let outside = temp_vault("index-symlink-outside");
    std::fs::write(outside.join("secret.md"), "# 库外\n").expect("seed");
    std::fs::write(vault.join("real.md"), "# 库内\n").expect("seed");

    #[cfg(unix)]
    {
        // 跟随链接会把 vault 外的文件放进索引,于是 `[[库外]]` 能解析到一条
        // allowlist 拒绝打开的路径 —— 点开就是一个报错。
        std::os::unix::fs::symlink(&outside, vault.join("linked")).expect("symlink dir");
        std::os::unix::fs::symlink(outside.join("secret.md"), vault.join("linked.md"))
            .expect("symlink file");

        let titles: Vec<String> = super::vault_index::scan_vault_titles(&vault)
            .expect("scan")
            .into_iter()
            .map(|e| e.title)
            .collect();
        assert_eq!(titles, vec!["库内"], "symlink was followed");
    }

    std::fs::remove_dir_all(&vault).ok();
    std::fs::remove_dir_all(&outside).ok();
}

#[test]
fn vault_index_finds_a_title_field_that_is_not_on_the_first_line() {
    let vault = temp_vault("index-late-title");
    // 第三方工具(Obsidian 插件等)写的 frontmatter 里 title 可能排在很后面。
    // 只读第一行的实现会把这些笔记全判成"没有标题"。
    let mut front = String::from("---\n");
    for i in 0..40 {
        front.push_str(&format!("field{i}: value{i}\n"));
    }
    front.push_str("title: 靠后的标题\n---\n\n正文\n");
    std::fs::write(vault.join("a.md"), front).expect("seed");

    let entries = super::vault_index::scan_vault_titles(&vault).expect("scan");
    assert_eq!(
        indexed_title(&entries, "a.md").as_deref(),
        Some("靠后的标题")
    );

    std::fs::remove_dir_all(&vault).ok();
}

// ── 自定义图标 ─────────────────────────────────────────────────────────────

#[test]
fn icons_round_trip_through_the_private_dir() {
    let vault = temp_vault("icons-round-trip");
    let mut icons = std::collections::BTreeMap::new();
    icons.insert("Note.md".to_string(), "book".to_string());
    icons.insert("sub/Deep.md".to_string(), "target".to_string());

    fs_ops::write_icons(&vault, &icons).expect("write");
    assert_eq!(fs_ops::read_icons(&vault), icons);
    // 落在 vault 私有目录里 —— 图标要跟着笔记走,用户搬走整个 vault 时不该留在
    // 原来那台机器上。
    assert!(vault.join(".notebook/icons.json").is_file());

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn icons_read_returns_empty_when_missing_or_corrupt() {
    let vault = temp_vault("icons-corrupt");
    // 还没设过任何图标。
    assert!(fs_ops::read_icons(&vault).is_empty());

    std::fs::create_dir_all(fs_ops::private_dir(&vault)).expect("mkdir");
    std::fs::write(vault.join(".notebook/icons.json"), "{ not json").expect("seed");
    // 损坏的表回落到空,不该让面板打不开 —— 图标丢了只是回到默认图标。
    assert!(fs_ops::read_icons(&vault).is_empty());

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn icons_write_creates_the_private_dir_and_replaces_the_whole_table() {
    let vault = temp_vault("icons-replace");
    // 私有目录还不存在(全新 vault 里第一次设图标就是这个情形)。
    assert!(!fs_ops::private_dir(&vault).exists());

    let mut first = std::collections::BTreeMap::new();
    first.insert("A.md".to_string(), "book".to_string());
    first.insert("B.md".to_string(), "target".to_string());
    fs_ops::write_icons(&vault, &first).expect("write");
    // 目录是写的时候顺手建出来的,不需要调用方先准备好。
    assert!(fs_ops::private_dir(&vault).is_dir());

    // 整张表替换:第二次写只留 A,B 必须消失。合并语义会让"恢复默认图标"
    // 变成写不掉的操作。
    let mut second = std::collections::BTreeMap::new();
    second.insert("A.md".to_string(), "flame".to_string());
    fs_ops::write_icons(&vault, &second).expect("write");

    let read = fs_ops::read_icons(&vault);
    assert_eq!(read.get("A.md").map(String::as_str), Some("flame"));
    assert_eq!(read.get("B.md"), None, "B 没被清掉");

    std::fs::remove_dir_all(&vault).ok();
}

/// 扫描结果里某个文件的链接 raw 列表。找不到那个文件就是 None(与"扫到了但没链接"
/// 区分开 —— 后者根本不进结果)。
fn scanned_links(sources: &[super::links::NoteLinkSource], name: &str) -> Option<Vec<String>> {
    sources
        .iter()
        .find(|source| source.path.ends_with(name))
        .map(|source| source.links.iter().map(|link| link.raw.clone()).collect())
}

#[test]
fn vault_links_reports_raw_bodies_with_line_numbers() {
    let vault = temp_vault("links-basic");
    std::fs::write(
        vault.join("a.md"),
        "---\ntitle: A\n---\n\n第一段\n见 [[周报]] 与 ![[图]]\n尾巴\n",
    )
    .expect("seed");

    let sources = super::links::scan_vault_links(&vault).expect("scan");
    let source = sources
        .iter()
        .find(|s| s.path.ends_with("a.md"))
        .expect("a.md");
    assert_eq!(source.links.len(), 2);
    assert_eq!(source.links[0].raw, "周报");
    // frontmatter 也算进行号:前端跳转是按整篇源码的行数走的。
    assert_eq!(source.links[0].line, 6);
    assert!(!source.links[0].embed);
    assert!(source.links[1].embed, "`![[图]]` 是嵌入");
    // 同一行的两条共用那一行的预览。
    assert_eq!(source.links[1].preview, "见 [[周报]] 与 ![[图]]");

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn vault_links_omits_notes_without_links() {
    let vault = temp_vault("links-empty");
    std::fs::write(vault.join("plain.md"), "没有任何链接\n").expect("seed");
    std::fs::write(vault.join("linked.md"), "[[plain]]\n").expect("seed");

    let sources = super::links::scan_vault_links(&vault).expect("scan");
    // 没有链接的笔记不占位 —— 反链面板只关心"谁指向了谁",空条目只让 payload 变大。
    assert_eq!(scanned_links(&sources, "plain.md"), None);
    assert_eq!(
        scanned_links(&sources, "linked.md"),
        Some(vec!["plain".to_string()])
    );

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn vault_links_skips_private_dirs_and_non_notes() {
    let vault = temp_vault("links-skip");
    let private = vault.join(".notebook");
    std::fs::create_dir_all(private.join("trash")).expect("mkdir");
    // 回收站里的笔记不该有反链:它已经被删了,让它把别人指出来只会造出点不开的条目。
    std::fs::write(private.join("trash/gone.md"), "[[target]]\n").expect("seed");
    std::fs::write(vault.join("notes.txt"), "[[target]]\n").expect("seed");
    std::fs::write(vault.join("real.md"), "[[target]]\n").expect("seed");

    let sources = super::links::scan_vault_links(&vault).expect("scan");
    assert_eq!(scanned_links(&sources, "gone.md"), None);
    assert_eq!(scanned_links(&sources, "notes.txt"), None);
    assert!(scanned_links(&sources, "real.md").is_some());

    std::fs::remove_dir_all(&vault).ok();
}

#[test]
fn vault_links_walks_subdirectories_and_sorts_by_path() {
    let vault = temp_vault("links-sub");
    std::fs::create_dir_all(vault.join("sub")).expect("mkdir");
    std::fs::write(vault.join("sub/deep.md"), "[[target]]\n").expect("seed");
    std::fs::write(vault.join("aaa.md"), "[[target]]\n").expect("seed");

    let sources = super::links::scan_vault_links(&vault).expect("scan");
    let paths: Vec<&str> = sources.iter().map(|s| s.path.as_str()).collect();
    assert_eq!(paths.len(), 2);
    // 排序保证两次扫描顺序一致,否则反链列表的排列会随文件系统遍历顺序漂移。
    let mut sorted = paths.clone();
    sorted.sort_unstable();
    assert_eq!(paths, sorted);

    std::fs::remove_dir_all(&vault).ok();
}
