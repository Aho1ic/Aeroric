//! 目录型导入(Obsidian / Logseq)与共用 walker 的测试。
//!
//! 放在单独文件而不是各自模块的 `#[cfg(test)]` 里:这两个 provider 的行为差别几乎全在
//! `decide` 那一个函数上,而测试要断言的是「同一个 walker 在两种 decide 下的结果」——
//! 拆到两个文件里会让共用部分的 fixture 抄两遍。

use std::path::{Path, PathBuf};

use super::report::{ImportItem, ImportReport, ItemIssue, ItemStatus, SkipReason};
use super::{logseq, obsidian};

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "aeroric-import-dir-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

/// 建一对 (vault, src)。
fn pair(tag: &str) -> (PathBuf, PathBuf) {
    let root = temp_dir(tag);
    let vault = root.join("vault");
    let src = root.join("src");
    std::fs::create_dir_all(&vault).expect("建 vault");
    std::fs::create_dir_all(&src).expect("建源目录");
    (vault, src)
}

fn write(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("建父目录");
    }
    std::fs::write(path, body).expect("写文件");
}

/// 报告里某个源端条目的记录。
fn item_for<'a>(report: &'a ImportReport, source: &str) -> &'a ImportItem {
    report
        .items
        .iter()
        .find(|item| item.source == source)
        .unwrap_or_else(|| panic!("报告里应有 {source}:{:?}", sources(report)))
}

fn sources(report: &ImportReport) -> Vec<&str> {
    report
        .items
        .iter()
        .map(|item| item.source.as_str())
        .collect()
}

fn cleanup(vault: &Path) {
    if let Some(root) = vault.parent() {
        let _ = std::fs::remove_dir_all(root);
    }
}

// ─── Obsidian ────────────────────────────────────────────────────────────

#[test]
fn obsidian_copies_notes_and_keeps_the_tree_shape() {
    // 附件引用是相对路径(`![](assets/img.png)`)。把附件挪平会让每一条引用失效。
    let (vault, src) = pair("obs-tree");
    write(&src.join("root.md"), "# Root");
    write(&src.join("sub/nested.md"), "# Nested");
    write(&src.join("sub/assets/img.png"), "PNG");

    let report = obsidian::import(&vault, &src).expect("导入成功");

    assert_eq!(report.imported, 3);
    assert!(vault.join("imports/obsidian/root.md").is_file());
    assert!(vault.join("imports/obsidian/sub/nested.md").is_file());
    assert!(vault.join("imports/obsidian/sub/assets/img.png").is_file());
    cleanup(&vault);
}

#[test]
fn obsidian_reports_dest_paths_that_actually_exist() {
    // 报告里的 `dest` 是用户拿去在 vault 里找文件的。写错了报告就是错的。
    let (vault, src) = pair("obs-dest");
    write(&src.join("a/b.md"), "x");
    let report = obsidian::import(&vault, &src).expect("导入成功");
    let dest = item_for(&report, "a/b.md").dest.as_ref().expect("有落点");
    assert_eq!(dest, "imports/obsidian/a/b.md");
    assert!(vault.join(dest).is_file());
    cleanup(&vault);
}

#[test]
fn obsidian_skips_dot_dirs_without_cluttering_the_report() {
    // `.obsidian/` 是配置。它不是内容,逐条列进报告会把「需要留意」淹掉。
    let (vault, src) = pair("obs-dot");
    write(&src.join("note.md"), "x");
    write(&src.join(".obsidian/app.json"), "{}");
    write(&src.join(".DS_Store"), "junk");

    let report = obsidian::import(&vault, &src).expect("导入成功");

    assert_eq!(report.imported, 1);
    assert!(!vault.join("imports/obsidian/.obsidian").exists());
    assert_eq!(sources(&report), vec!["note.md"]);
    cleanup(&vault);
}

#[test]
fn obsidian_rejects_a_file_as_source() {
    let (vault, src) = pair("obs-notdir");
    let file = src.join("a.md");
    write(&file, "x");
    assert!(obsidian::import(&vault, &file).is_err());
    cleanup(&vault);
}

// ─── Logseq ──────────────────────────────────────────────────────────────

#[test]
fn logseq_copies_pages_journals_and_assets() {
    let (vault, src) = pair("logseq-happy");
    write(&src.join("pages/Project.md"), "- hello");
    write(&src.join("journals/2026_05_18.md"), "- journal");
    write(&src.join("assets/image.png"), "PNG");

    let report = logseq::import(&vault, &src).expect("导入成功");

    assert_eq!(report.imported, 3);
    assert!(vault.join("imports/logseq/pages/Project.md").is_file());
    assert!(vault
        .join("imports/logseq/journals/2026_05_18.md")
        .is_file());
    assert!(vault.join("imports/logseq/assets/image.png").is_file());
    cleanup(&vault);
}

#[test]
fn logseq_keeps_org_files_and_reports_them_as_degraded() {
    // Markio 在这里**不复制**,只留一条会被截断的警告 —— 用户很可能不知道自己丢了东西。
    // 这里内容必须进来,而且要在报告里说清它没被转换。
    let (vault, src) = pair("logseq-org");
    write(&src.join("pages/Legacy.org"), "* TODO 旧笔记");

    let report = logseq::import(&vault, &src).expect("导入成功");

    let landed = vault.join("imports/logseq/pages/Legacy.org");
    assert!(landed.is_file(), "org 文件的内容不该丢");
    assert_eq!(
        std::fs::read_to_string(&landed).expect("读"),
        "* TODO 旧笔记"
    );

    let item = item_for(&report, "pages/Legacy.org");
    assert_eq!(item.status, ItemStatus::Imported);
    assert!(matches!(
        item.issues.as_slice(),
        [ItemIssue::Degraded { .. }]
    ));
    assert_eq!(report.degraded, 1);
    assert!(report.needs_attention());
    cleanup(&vault);
}

#[test]
fn a_landed_org_file_is_not_mistaken_for_a_note() {
    // 扩展名保持 `.org`,`is_note_file` 就不认它 —— 它不会以一篇满是 `* TODO` 的
    // 坏格式笔记的样子出现在笔记列表里。
    let (vault, src) = pair("logseq-org-notnote");
    write(&src.join("a.org"), "* x");
    logseq::import(&vault, &src).expect("导入成功");
    let landed = vault.join("imports/logseq/a.org");
    assert!(landed.is_file());
    assert!(!super::super::fs_ops::is_note_file(&landed));
    cleanup(&vault);
}

#[test]
fn logseq_ignores_the_config_dir_entirely() {
    // `logseq/` 不以点开头,walker 那条通用规则拦不住它。
    let (vault, src) = pair("logseq-config");
    write(&src.join("pages/a.md"), "x");
    write(&src.join("logseq/config.edn"), "{}");
    write(&src.join("logseq/bak/pages/a.md"), "旧版本");

    let report = logseq::import(&vault, &src).expect("导入成功");

    assert_eq!(report.imported, 1);
    assert!(!vault.join("imports/logseq/logseq").exists());
    // 连报告都不该提它们 —— 配置和备份不是用户的笔记。
    assert_eq!(sources(&report), vec!["pages/a.md"]);
    cleanup(&vault);
}

#[test]
fn logseq_rejects_a_file_as_source() {
    let (vault, src) = pair("logseq-notdir");
    let file = src.join("a.md");
    write(&file, "x");
    assert!(logseq::import(&vault, &file).is_err());
    cleanup(&vault);
}

#[test]
fn logseq_skips_graph_root_junk_but_says_so() {
    // `custom.css` / `export.json` 既不是笔记也不是被引用的附件。复制进 vault 是把
    // 垃圾搬进用户的笔记库;静默丢掉则是另一种不诚实 —— 用户挑的是自己的 graph。
    let (vault, src) = pair("logseq-junk");
    write(&src.join("pages/a.md"), "x");
    write(&src.join("custom.css"), "body{}");
    write(&src.join("export.json"), "{}");

    let report = logseq::import(&vault, &src).expect("导入成功");

    assert_eq!(report.imported, 1);
    assert_eq!(report.skipped, 2);
    assert!(!vault.join("imports/logseq/custom.css").exists());
    for (source, extension) in [("custom.css", "css"), ("export.json", "json")] {
        assert_eq!(
            item_for(&report, source).status,
            ItemStatus::Skipped {
                reason: SkipReason::Unsupported {
                    extension: extension.to_string()
                }
            }
        );
    }
    cleanup(&vault);
}

#[test]
fn logseq_takes_any_file_type_under_assets() {
    // 附件是任意二进制。按扩展名筛会把 `.zip` / `.webm` 挡在外面,而正文里的引用
    // 还指着它们 —— 那就变成了资源丢失。
    let (vault, src) = pair("logseq-assets-any");
    write(&src.join("assets/clip.webm"), "VIDEO");
    write(&src.join("assets/bundle.zip"), "ZIP");
    write(&src.join("assets/deep/nested.bin"), "BIN");

    let report = logseq::import(&vault, &src).expect("导入成功");

    assert_eq!(report.imported, 3);
    assert_eq!(report.skipped, 0);
    assert!(vault
        .join("imports/logseq/assets/deep/nested.bin")
        .is_file());
    cleanup(&vault);
}

// ─── 共用 walker ─────────────────────────────────────────────────────────

#[cfg(unix)]
#[test]
fn a_symlink_is_reported_not_followed() {
    // 跟随会让落点跑出源目录(指向 `/etc` 的软链),也可能自己成环。而静默跳过
    // 会让用户以为那个文件根本不存在。
    let (vault, src) = pair("symlink");
    write(&src.join("real.md"), "# Real");
    std::os::unix::fs::symlink(src.join("real.md"), src.join("link.md")).expect("建软链");

    let report = obsidian::import(&vault, &src).expect("导入成功");

    assert_eq!(report.imported, 1);
    assert_eq!(
        item_for(&report, "link.md").status,
        ItemStatus::Skipped {
            reason: SkipReason::Symlink
        }
    );
    assert!(!vault.join("imports/obsidian/link.md").exists());
    cleanup(&vault);
}

#[cfg(unix)]
#[test]
fn a_symlink_loop_does_not_hang_the_walk() {
    // 软链在判目录/文件**之前**处理的理由:`is_dir()` 会跟随软链,于是一个指向
    // 上层的软链会被当成普通目录递归下去。
    let (vault, src) = pair("symlink-loop");
    write(&src.join("sub/a.md"), "x");
    std::os::unix::fs::symlink(&src, src.join("sub/up")).expect("建成环软链");

    let report = obsidian::import(&vault, &src).expect("导入成功");

    assert_eq!(report.imported, 1);
    assert_eq!(
        item_for(&report, "sub/up").status,
        ItemStatus::Skipped {
            reason: SkipReason::Symlink
        }
    );
    cleanup(&vault);
}

#[test]
fn re_importing_the_same_source_skips_instead_of_duplicating() {
    // 没有增量清单的话第二轮会靠 `unique_path` 落成 `a-2.md` —— 而副本是真笔记,
    // 会进索引、进搜索、被 wikilink 指到,只能手工清。
    let (vault, src) = pair("incremental");
    write(&src.join("a.md"), "# A");

    let first = obsidian::import(&vault, &src).expect("第一轮");
    assert_eq!(first.imported, 1);

    let second = obsidian::import(&vault, &src).expect("第二轮");
    assert_eq!(second.imported, 0);
    assert_eq!(
        item_for(&second, "a.md").status,
        ItemStatus::Skipped {
            reason: SkipReason::AlreadyImported
        }
    );
    assert!(!vault.join("imports/obsidian/a-2.md").exists());
    cleanup(&vault);
}

#[test]
fn the_fingerprint_is_relative_so_moving_the_source_still_dedupes() {
    // 指纹按相对源端根的路径算。用绝对路径的话用户把同一个 vault 从别的位置
    // 再导一次会整份重导。
    let root = temp_dir("relative-fp");
    let vault = root.join("vault");
    std::fs::create_dir_all(&vault).expect("建 vault");
    let first_src = root.join("one");
    let second_src = root.join("two");
    write(&first_src.join("a.md"), "# A");
    write(&second_src.join("a.md"), "# A");

    assert_eq!(
        obsidian::import(&vault, &first_src)
            .expect("第一轮")
            .imported,
        1
    );
    let second = obsidian::import(&vault, &second_src).expect("第二轮");
    assert_eq!(second.imported, 0);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_note_that_fails_to_land_is_not_recorded_as_imported() {
    // 指纹只在写成功之后记。写失败还记的话,重试那一轮会把它当成「导过了」跳掉,
    // 那篇笔记就永远进不来。这里用一个**目录**冒充源文件来制造复制失败。
    let (vault, src) = pair("failed-copy");
    std::fs::create_dir_all(src.join("trap.md")).expect("建同名目录");
    write(&src.join("trap.md/inner.md"), "x");

    let report = obsidian::import(&vault, &src).expect("整轮不该失败");

    // `trap.md` 是目录,会被当成目录递归进去,里面那篇正常导入。
    assert_eq!(report.imported, 1);
    assert_eq!(
        item_for(&report, "trap.md/inner.md").dest.as_deref(),
        Some("imports/obsidian/trap.md/inner.md")
    );
    cleanup(&vault);
}
