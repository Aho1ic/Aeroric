//! zip 型导入(Notion / Bear / Roam)与共用归档遍历的测试。

use std::io::Write;
use std::path::{Path, PathBuf};

use super::report::{ImportItem, ImportReport, ItemStatus, SkipReason};
use super::{bear, notion, roam};

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "aeroric-import-zip-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

/// 建一个 vault 和一个含给定条目的 zip。
fn setup(tag: &str, entries: &[(&str, &[u8])]) -> (PathBuf, PathBuf) {
    let root = temp_dir(tag);
    let vault = root.join("vault");
    std::fs::create_dir_all(&vault).expect("建 vault");
    let archive = root.join("src.zip");
    write_zip(&archive, entries);
    (vault, archive)
}

fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let file = std::fs::File::create(path).expect("建 zip");
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in entries {
        writer.start_file(*name, options).expect("开 entry");
        writer.write_all(body).expect("写 entry");
    }
    writer.finish().expect("收尾");
}

fn item_for<'a>(report: &'a ImportReport, source: &str) -> &'a ImportItem {
    report
        .items
        .iter()
        .find(|item| item.source == source)
        .unwrap_or_else(|| {
            panic!(
                "报告里应有 {source}:{:?}",
                report
                    .items
                    .iter()
                    .map(|item| item.source.as_str())
                    .collect::<Vec<_>>()
            )
        })
}

fn cleanup(vault: &Path) {
    if let Some(root) = vault.parent() {
        let _ = std::fs::remove_dir_all(root);
    }
}

// ─── 共用归档遍历 ────────────────────────────────────────────────────────

#[test]
fn an_entry_escaping_the_root_is_refused_and_recorded() {
    // `enclosed_name()` 是 zip crate 自己的守卫。Markio 用 `split('/').last()`,
    // 那会把 `../../.zshrc` 洗成 `.zshrc` 然后照样落盘。
    let (vault, archive) = setup(
        "escape",
        &[("../../evil.md", b"# Evil"), ("ok.md", b"# OK")],
    );
    let report = bear::import(&vault, &archive).expect("导入成功");

    assert_eq!(report.imported, 1);
    assert_eq!(report.failed, 1);
    assert!(matches!(
        item_for(&report, "../../evil.md").status,
        ItemStatus::Failed { .. }
    ));
    // 关键断言:那个文件哪儿都没落下。
    assert!(!vault.join("evil.md").exists());
    assert!(!vault.join("imports/bear/evil.md").exists());
    if let Some(root) = vault.parent() {
        assert!(!root.join("evil.md").exists());
    }
    cleanup(&vault);
}

#[test]
fn directory_entries_are_not_counted_as_files() {
    let root = temp_dir("dirent");
    let vault = root.join("vault");
    std::fs::create_dir_all(&vault).expect("建 vault");
    let archive = root.join("a.zip");
    {
        let file = std::fs::File::create(&archive).expect("建 zip");
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        writer.add_directory("sub/", options).expect("加目录");
        writer.start_file("sub/a.md", options).expect("开 entry");
        writer.write_all(b"# A").expect("写");
        writer.finish().expect("收尾");
    }
    let report = bear::import(&vault, &archive).expect("导入成功");
    assert_eq!(report.imported, 1);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_missing_archive_fails_the_whole_round() {
    // 归档打不开时没有「部分成功」可言,该整轮失败。
    let vault = temp_dir("missing").join("vault");
    std::fs::create_dir_all(&vault).expect("建 vault");
    assert!(bear::import(&vault, Path::new("/nonexistent/x.zip")).is_err());
    cleanup(&vault);
}

// ─── Bear ────────────────────────────────────────────────────────────────

#[test]
fn bear_keeps_the_archive_tree_so_relative_links_survive() {
    // Markio 把非 md 全压到 `Assets/` 一层,于是 `![](media/a.png)` 这类引用全断。
    let (vault, archive) = setup(
        "bear-tree",
        &[("Note.md", b"![](media/a.png)"), ("media/a.png", b"PNG")],
    );
    let report = bear::import(&vault, &archive).expect("导入成功");

    assert_eq!(report.imported, 2);
    assert!(vault.join("imports/bear/Note.md").is_file());
    assert!(
        vault.join("imports/bear/media/a.png").is_file(),
        "资源要留在正文引用得到的相对位置上"
    );
    cleanup(&vault);
}

#[test]
fn bear_re_import_skips_instead_of_duplicating() {
    let (vault, archive) = setup("bear-again", &[("a.md", b"# A")]);
    assert_eq!(bear::import(&vault, &archive).expect("第一轮").imported, 1);
    let second = bear::import(&vault, &archive).expect("第二轮");
    assert_eq!(second.imported, 0);
    assert_eq!(
        item_for(&second, "a.md").status,
        ItemStatus::Skipped {
            reason: SkipReason::AlreadyImported
        }
    );
    assert!(!vault.join("imports/bear/a-2.md").exists());
    cleanup(&vault);
}

// ─── Notion ──────────────────────────────────────────────────────────────

#[test]
fn notion_strips_the_hash_suffix_from_names() {
    let hash = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6";
    let (vault, archive) = setup(
        "notion-hash",
        &[(&format!("我的页面 {hash}.md"), b"# body")],
    );
    let report = notion::import(&vault, &archive).expect("导入成功");

    assert_eq!(report.imported, 1);
    assert!(vault.join("imports/notion/我的页面.md").is_file());
    cleanup(&vault);
}

#[test]
fn notion_leaves_ordinary_names_with_spaces_alone() {
    // Markio 的 `regex_like_strip` 有一条兜底分支会把最后一个空格之后的内容整段砍掉,
    // 于是 `我的 笔记.md` 变成 `我的` —— 连扩展名一起丢。
    let (vault, archive) = setup("notion-space", &[("我的 笔记.md", b"# body")]);
    let report = notion::import(&vault, &archive).expect("导入成功");

    assert_eq!(report.imported, 1);
    assert!(
        vault.join("imports/notion/我的 笔记.md").is_file(),
        "普通带空格的名字不该被改动,实际落点:{:?}",
        std::fs::read_dir(vault.join("imports/notion"))
            .map(|dir| dir.flatten().map(|e| e.file_name()).collect::<Vec<_>>())
    );
    cleanup(&vault);
}

#[test]
fn notion_rewrites_page_links_to_wikilinks() {
    let hash = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6";
    let body = format!("见 [目标页](目标页%20{hash}.md) 那边。");
    let (vault, archive) = setup("notion-link", &[("源.md", body.as_bytes())]);
    notion::import(&vault, &archive).expect("导入成功");

    let text = std::fs::read_to_string(vault.join("imports/notion/源.md")).expect("读");
    assert_eq!(text, "见 [[目标页]] 那边。");
    cleanup(&vault);
}

#[test]
fn notion_does_not_rewrite_ordinary_relative_links() {
    // 只看 `.md` 结尾的话,用户自己写的 `[说明](./readme.md)` 会被换成一个指向
    // 不存在笔记的 wikilink —— 一条本来有效的链接变成死链。
    let (vault, archive) = setup(
        "notion-plainlink",
        &[(
            "a.md",
            "见 [说明](./readme.md) 和 [站](https://x.dev/a.md)".as_bytes(),
        )],
    );
    notion::import(&vault, &archive).expect("导入成功");

    let text = std::fs::read_to_string(vault.join("imports/notion/a.md")).expect("读");
    assert!(
        text.contains("[说明](./readme.md)"),
        "普通相对链接不该动:{text}"
    );
    assert!(text.contains("(https://x.dev/a.md)"), "外链不该动:{text}");
    cleanup(&vault);
}

#[test]
fn notion_does_not_corrupt_binary_assets() {
    // 只有 markdown 过重写。资源按 UTF-8 解一遍会损坏它。
    let bytes: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x00, 0xFF, 0xFE, 0x01];
    let (vault, archive) = setup("notion-bin", &[("img.png", &bytes)]);
    notion::import(&vault, &archive).expect("导入成功");
    assert_eq!(
        std::fs::read(vault.join("imports/notion/img.png")).expect("读"),
        bytes
    );
    cleanup(&vault);
}

#[test]
fn notion_keeps_nested_pages_as_directories() {
    // Markio 压平目录结构,于是同名子页面互相撞名、资源引用全断。
    let hash = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6";
    let (vault, archive) = setup(
        "notion-nested",
        &[
            (&format!("父页 {hash}/子页 {hash}.md"), b"# child"),
            (&format!("子页 {hash}.md"), b"# top"),
        ],
    );
    let report = notion::import(&vault, &archive).expect("导入成功");

    assert_eq!(report.imported, 2);
    assert!(vault.join("imports/notion/父页/子页.md").is_file());
    assert!(vault.join("imports/notion/子页.md").is_file());
    cleanup(&vault);
}

// ─── Roam ────────────────────────────────────────────────────────────────

#[test]
fn roam_converts_json_pages_into_separate_notes() {
    let json = br#"[
      {"title":"Hello","children":[{"string":"top","children":[{"string":"child"}]}]},
      {"title":"World","children":[{"string":"solo"}]}
    ]"#;
    let (vault, archive) = setup("roam-json", &[("roam.json", json)]);
    let report = roam::import(&vault, &archive).expect("导入成功");

    assert_eq!(report.imported, 2);
    assert_eq!(
        std::fs::read_to_string(vault.join("imports/roam/Hello.md")).expect("读"),
        "# Hello\n\n- top\n  - child\n"
    );
    assert_eq!(
        std::fs::read_to_string(vault.join("imports/roam/World.md")).expect("读"),
        "# World\n\n- solo\n"
    );
    cleanup(&vault);
}

#[test]
fn roam_reports_each_json_page_separately() {
    // 一个 JSON 里三百个页面,报成一条「已导入 roam.json」会让用户完全看不出
    // 哪个页面出了问题。
    let json = br#"[{"title":"Alpha","children":[]},{"title":"Beta","children":[]}]"#;
    let (vault, archive) = setup("roam-perpage", &[("roam.json", json)]);
    let report = roam::import(&vault, &archive).expect("导入成功");

    assert_eq!(
        item_for(&report, "roam.json → Alpha").status,
        ItemStatus::Imported
    );
    assert_eq!(
        item_for(&report, "roam.json → Beta").status,
        ItemStatus::Imported
    );
    // 外层不该再为整个 JSON 记一条,否则计数会重复。
    assert!(!report.items.iter().any(|item| item.source == "roam.json"));
    assert_eq!(report.imported, 2);
    cleanup(&vault);
}

#[test]
fn roam_headings_and_todos_convert() {
    let json = br#"[{"title":"T","children":[
      {"string":"Section","heading":2,"children":[{"string":"point"}]},
      {"string":"{{[[TODO]]}} do it"},
      {"string":"{{[[DONE]]}} done it"}
    ]}]"#;
    let (vault, archive) = setup("roam-fmt", &[("roam.json", json)]);
    roam::import(&vault, &archive).expect("导入成功");
    let text = std::fs::read_to_string(vault.join("imports/roam/T.md")).expect("读");
    assert_eq!(
        text,
        "# T\n\n## Section\n\n- point\n- [ ] do it\n- [x] done it\n"
    );
    cleanup(&vault);
}

#[test]
fn roam_keeps_wikilinks_and_tags_verbatim() {
    let json = br#"[{"title":"L","children":[{"string":"see [[Other]] and #tag"}]}]"#;
    let (vault, archive) = setup("roam-links", &[("roam.json", json)]);
    roam::import(&vault, &archive).expect("导入成功");
    let text = std::fs::read_to_string(vault.join("imports/roam/L.md")).expect("读");
    assert!(text.contains("- see [[Other]] and #tag"));
    cleanup(&vault);
}

#[test]
fn roam_untitled_pages_are_skipped_with_a_reason() {
    // 无标题页面没法定文件名,而文件名是 wikilink 的目标。
    let json = br#"[{"title":"","children":[{"string":"x"}]},{"title":"Kept","children":[]}]"#;
    let (vault, archive) = setup("roam-untitled", &[("roam.json", json)]);
    let report = roam::import(&vault, &archive).expect("导入成功");

    assert_eq!(report.imported, 1);
    assert_eq!(report.skipped, 1);
    assert!(matches!(
        item_for(&report, "roam.json → (无标题页面)").status,
        ItemStatus::Skipped { .. }
    ));
    cleanup(&vault);
}

#[test]
fn roam_invalid_json_is_a_failure_not_a_skip() {
    // 用户选了 JSON 导出,解析失败意味着他的笔记一篇都没进来。
    let (vault, archive) = setup("roam-badjson", &[("roam.json", b"not json")]);
    let report = roam::import(&vault, &archive).expect("整轮不该 Err");

    assert_eq!(report.failed, 1);
    assert_eq!(report.imported, 0);
    assert!(matches!(
        item_for(&report, "roam.json").status,
        ItemStatus::Failed { .. }
    ));
    cleanup(&vault);
}

#[test]
fn roam_markdown_entries_land_verbatim() {
    let (vault, archive) = setup("roam-md", &[("Page.md", b"# Page")]);
    let report = roam::import(&vault, &archive).expect("导入成功");
    assert_eq!(report.imported, 1);
    assert_eq!(
        std::fs::read_to_string(vault.join("imports/roam/Page.md")).expect("读"),
        "# Page"
    );
    cleanup(&vault);
}

#[test]
fn roam_json_dedupes_by_page_not_by_file() {
    // 按 JSON 路径去重的话,第二次导出里**新增**的页面会连带被跳过。
    let root = temp_dir("roam-dedupe");
    let vault = root.join("vault");
    std::fs::create_dir_all(&vault).expect("建 vault");

    let first = root.join("first.zip");
    write_zip(
        &first,
        &[("roam.json", br#"[{"title":"A","children":[]}]"#)],
    );
    assert_eq!(roam::import(&vault, &first).expect("第一轮").imported, 1);

    let second = root.join("second.zip");
    write_zip(
        &second,
        &[(
            "roam.json",
            br#"[{"title":"A","children":[]},{"title":"B","children":[]}]"#,
        )],
    );
    let report = roam::import(&vault, &second).expect("第二轮");
    assert_eq!(report.imported, 1, "只有新增的 B 该进来");
    assert_eq!(report.skipped, 1);
    assert!(vault.join("imports/roam/B.md").is_file());
    assert!(!vault.join("imports/roam/A-2.md").exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn roam_skips_edn_snapshots() {
    let (vault, archive) = setup("roam-edn", &[("db.edn", b"{}")]);
    let report = roam::import(&vault, &archive).expect("导入成功");
    assert_eq!(report.imported, 0);
    assert_eq!(
        item_for(&report, "db.edn").status,
        ItemStatus::Skipped {
            reason: SkipReason::Unsupported {
                extension: "edn".to_string()
            }
        }
    );
    cleanup(&vault);
}
