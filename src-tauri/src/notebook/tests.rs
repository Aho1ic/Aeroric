//! 随手记后端的单元测试。重点覆盖两块最容易出事的地方:
//! - **迁移**:幂等、回滚、非法文件名、有损转换的兜底
//! - **保存**:冲突检测的每个分支(这是唯一会静默丢数据的路径)

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::fs_ops::{self, SaveOutcome};
use super::migrate::{self, slugify};
use super::snapshots;
use super::state::{resolve_within, FileSig, NotebookState};

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
    let mut save = |content: &str, sig: FileSig| match fs_ops::save_note(
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
