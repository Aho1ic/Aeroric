use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::code_index::{scan, CodeIndex};
use super::compile::Sources;
use super::explain::{
    build, cite_ok, needs_rewrite, prompt, render, stale_lines, verify, Claim, Explanation,
    SourceState,
};
use super::notes::Note;
use super::Wikis;

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn fixture() -> PathBuf {
    let root = std::env::temp_dir().join(format!("aeroric-explain-{}", uuid::Uuid::new_v4()));
    write(&root, "c/Cargo.toml", "[package]\nname = \"c\"\n");
    write(&root, "c/src/lib.rs", "pub mod store;\n");
    let mut body = String::from("//! Keeps the pages.\nuse std::fs;\n");
    for i in 0..40 {
        body.push_str(&format!("// filler {i}\n"));
    }
    body.push_str("/// The store.\npub struct Store;\n");
    write(&root, "c/src/store/mod.rs", &body);
    root
}

fn note(path: &str, text: &str) -> Note {
    Note {
        path: path.into(),
        title: "笔记".into(),
        status: None,
        category: None,
        sections: vec![("Decision".into(), text.into())],
    }
}

fn input_of(root: &Path) -> (CodeIndex, super::explain::Input) {
    let index = scan(root);
    let module = index.modules.iter().find(|m| m.id == "c::store").unwrap();
    let notes = [
        note(".agents/notes/a.md", "改 `store/mod.rs` 时先关写入。"),
        note(".agents/notes/b.md", "和这个模块无关。"),
    ];
    let input = build(module, &index, root, &notes);
    (index, input)
}

#[test]
fn the_input_is_the_entry_and_the_symbols_with_line_numbers_and_the_notes_about_it() {
    let root = fixture();
    let (_, input) = input_of(&root);
    let _ = std::fs::remove_dir_all(&root);
    // The entry file's first 80 lines cover the symbol window too: merged.
    assert_eq!(input.excerpts.len(), 1);
    let excerpt = &input.excerpts[0];
    assert_eq!(
        (excerpt.path.as_str(), excerpt.start),
        ("c/src/store/mod.rs", 1)
    );
    assert_eq!(excerpt.lines.len(), 44);
    assert_eq!(input.notes.len(), 1, "only the note naming its file");
    assert_eq!(input.notes[0].0, ".agents/notes/a.md");
    assert_eq!(input.versions.len(), 1);
    let text = prompt(&input);
    assert!(text.contains("=== c/src/store/mod.rs 第 1–44 行 ==="));
    assert!(text.contains("   44| pub struct Store;"));
    assert!(text.contains("=== 笔记 note:.agents/notes/a.md ==="));
    assert!(text.starts_with("模块：c · store\n路径：c/src/store"));
}

#[test]
fn only_claims_citing_what_was_shown_are_kept() {
    let root = fixture();
    let (_, input) = input_of(&root);
    let _ = std::fs::remove_dir_all(&root);
    assert!(cite_ok("c/src/store/mod.rs:44", &input));
    assert!(cite_ok("`c/src/store/mod.rs:3-5`", &input));
    assert!(cite_ok("note:.agents/notes/a.md", &input));
    assert!(
        !cite_ok("c/src/store/mod.rs:45", &input),
        "past the excerpt"
    );
    assert!(!cite_ok("c/src/other.rs:1", &input));
    assert!(!cite_ok("note:.agents/notes/b.md", &input), "not shown");
    let answer = r#"好的：
```json
{"duties": [
  {"text": "保存页面。", "cites": ["c/src/store/mod.rs:44", "c/src/nope.rs:1"]},
  {"text": "瞎编的。", "cites": ["c/src/nope.rs:3"]},
  {"text": "没有引用。", "cites": []}
], "pitfalls": [{"text": "先关写入。", "cites": ["note:.agents/notes/a.md"]}]}
```"#;
    let (duties, pitfalls, dropped) = verify(answer, &input).unwrap();
    assert_eq!(
        duties,
        [Claim {
            text: "保存页面。".into(),
            cites: vec!["c/src/store/mod.rs:44".into()],
        }]
    );
    assert_eq!(pitfalls.len(), 1);
    assert_eq!(dropped, 2);
    assert!(verify("不知道", &input).is_err());
    assert_eq!(render(&duties), "- 保存页面。（`c/src/store/mod.rs:44`）");
}

#[test]
fn the_page_shows_the_explanation_and_says_when_a_file_read_changed() {
    let root = fixture();
    let (mut index, input) = input_of(&root);
    let _ = std::fs::remove_dir_all(&root);
    let wikis = Wikis::default();
    wikis.set_dir(std::env::temp_dir().join(format!("aeroric-knowledge-{}", uuid::Uuid::new_v4())));
    let explanation = Explanation {
        module_id: "c::store".into(),
        duties: vec![Claim {
            text: "保存页面。".into(),
            cites: vec!["c/src/store/mod.rs:44".into()],
        }],
        pitfalls: Vec::new(),
        versions: input.versions.clone(),
        model: "m-1".into(),
        input_tokens: 900,
        output_tokens: 80,
        at_ms: 1,
        dropped: 0,
    };
    wikis.save_explanation("p", &explanation).unwrap();
    let sources = |code: CodeIndex| Sources {
        code: Some(code),
        ..Sources::default()
    };
    wikis.compile("p", &sources(index.clone()), 1).unwrap();
    let page = wikis.page("p", "module-c-store").unwrap();
    let keys: Vec<&str> = page.blocks.iter().map(|b| b.key.as_str()).collect();
    assert_eq!(
        &keys[..3],
        [
            "module:c::store#overview",
            "module:c::store#duties",
            "module:c::store#pitfalls"
        ]
    );
    let duties = &page.blocks[1];
    assert_eq!(duties.heading.as_deref(), Some("职责（推断）"));
    assert_eq!(
        duties.text,
        "以下由 m-1 根据代码推断，每条都核对过引用。\n\n- 保存页面。（`c/src/store/mod.rs:44`）"
    );
    assert_eq!(
        duties.sources[0].version.as_deref(),
        input.versions.values().next().map(String::as_str)
    );
    assert_eq!(page.blocks[2].text, "没有找到有出处的坑。");

    // The file it read changed.
    let current: BTreeMap<String, Option<String>> =
        [("c/src/store/mod.rs".to_string(), Some("new".to_string()))].into();
    assert_eq!(
        explanation.sources_now(&current),
        [("c/src/store/mod.rs".to_string(), SourceState::Changed)]
    );
    for m in &mut index.modules {
        for f in &mut m.files {
            f.blob = "new".into();
        }
    }
    wikis.compile("p", &sources(index), 2).unwrap();
    let page = wikis.page("p", "module-c-store").unwrap();
    assert!(page.blocks[1]
        .text
        .starts_with("> 可能过时：生成后 `c/src/store/mod.rs` 改过。\n\n以下由 m-1"));
}

/// R4-3 evaluation (2026-11-11): with the Wiki offered, agents used it in 2 of
/// 8 questions, answered no better and spent more input tokens, so it is off
/// until the user turns it on for a project.
#[test]
fn agents_read_the_wiki_only_once_the_user_turns_it_on() {
    let wikis = Wikis::default();
    wikis.set_dir(std::env::temp_dir().join(format!("aeroric-knowledge-{}", uuid::Uuid::new_v4())));
    assert!(!wikis.agent_access("p"));
    wikis.set_agent_access("p", true).unwrap();
    assert!(wikis.agent_access("p"));
    assert!(!wikis.agent_access("q"), "per project");
    wikis.set_agent_access("p", false).unwrap();
    assert!(!wikis.agent_access("p"));
}

/// A9 (2026-11-17): changed, deleted and unreadable are told apart; only
/// the first two are a reason to write it again.
#[test]
fn sources_are_changed_removed_or_unreadable_and_said_apart() {
    let explanation = Explanation {
        module_id: "m".into(),
        duties: Vec::new(),
        pitfalls: Vec::new(),
        versions: [
            ("a.rs".to_string(), "1".to_string()),
            ("b.rs".to_string(), "1".to_string()),
            ("c.rs".to_string(), "1".to_string()),
            ("d.rs".to_string(), "1".to_string()),
        ]
        .into(),
        model: "m".into(),
        input_tokens: 0,
        output_tokens: 0,
        at_ms: 0,
        dropped: 0,
    };
    let current: BTreeMap<String, Option<String>> = [
        ("a.rs".to_string(), Some("2".to_string())),
        ("c.rs".to_string(), None),
        ("d.rs".to_string(), Some("1".to_string())),
    ]
    .into();
    let states = explanation.sources_now(&current);
    assert_eq!(
        states,
        [
            ("a.rs".to_string(), SourceState::Changed),
            ("b.rs".to_string(), SourceState::Removed),
            ("c.rs".to_string(), SourceState::Unreadable),
        ]
    );
    assert!(needs_rewrite(&states));
    assert_eq!(
        stale_lines(&states),
        "> 可能过时：生成后 `a.rs` 改过；`b.rs` 被删了或移出了这个模块。\n\n\
         > 待核对：`c.rs` 这次读不到，没有据此改写。\n\n"
    );
    let unread_only = [("c.rs".to_string(), SourceState::Unreadable)];
    assert!(!needs_rewrite(&unread_only), "an unread file is no reason");
    let two_gone = [
        ("a.rs".to_string(), SourceState::Removed),
        ("b.rs".to_string(), SourceState::Removed),
    ];
    assert_eq!(
        stale_lines(&two_gone),
        "> 可能过时：生成后 `a.rs` 等 2 个文件被删了或移出了这个模块。\n\n"
    );
    assert_eq!(stale_lines(&[]), "");
}

fn explanation_of(input: &super::explain::Input) -> Explanation {
    Explanation {
        module_id: "c::store".into(),
        duties: vec![Claim {
            text: "保存页面。".into(),
            cites: vec!["c/src/store/mod.rs:44".into()],
        }],
        pitfalls: Vec::new(),
        versions: input.versions.clone(),
        model: "m-1".into(),
        input_tokens: 900,
        output_tokens: 80,
        at_ms: 1,
        dropped: 0,
    }
}

fn wikis() -> Wikis {
    let wikis = Wikis::default();
    wikis.set_dir(std::env::temp_dir().join(format!("aeroric-knowledge-{}", uuid::Uuid::new_v4())));
    wikis
}

fn code(index: CodeIndex) -> Sources {
    Sources {
        code: Some(index),
        ..Sources::default()
    }
}

/// The overview's sources as the idle pass reads them.
fn overview_now(page: &aeroric_protocol::wiki::WikiPage) -> BTreeMap<String, Option<String>> {
    page.blocks[0]
        .sources
        .iter()
        .map(|s| (s.target.clone(), s.version.clone()))
        .collect()
}

/// Makes `path` unreadable; false (and nothing changed) when this user can
/// read it anyway (root).
#[cfg(unix)]
fn lock(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(path).is_ok() {
        unlock(path);
        return false;
    }
    true
}

#[cfg(unix)]
fn unlock(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).unwrap();
}

/// A9: a file it read that cannot be read this time leaves the explanation
/// as it was, says "待核对", and is no reason to write it again; deleted,
/// it is "可能过时" and is.
#[cfg(unix)]
#[test]
fn an_unreadable_source_is_to_be_checked_and_a_deleted_one_is_stale() {
    let root = fixture();
    write(&root, "c/src/store/extra.rs", "pub fn extra() {}\n");
    let (_, input) = input_of(&root);
    let explanation = explanation_of(&input);
    let wikis = wikis();
    wikis.save_explanation("p", &explanation).unwrap();
    let file = root.join("c/src/store/mod.rs");
    if !lock(&file) {
        let _ = std::fs::remove_dir_all(&root);
        return;
    }
    let index = scan(&root);
    unlock(&file);
    assert!(index.unreadable.contains("c/src/store/mod.rs"));
    wikis.compile("p", &code(index), 1).unwrap();
    let page = wikis.page("p", "module-c-store").unwrap();
    assert!(
        page.blocks[1]
            .text
            .starts_with("> 待核对：`c/src/store/mod.rs` 这次读不到，没有据此改写。\n\n以下由 m-1"),
        "{}",
        page.blocks[1].text
    );
    let now = overview_now(&page);
    assert_eq!(now.get("c/src/store/mod.rs"), Some(&None));
    assert!(!needs_rewrite(&explanation.sources_now(&now)));

    std::fs::remove_file(&file).unwrap();
    let index = scan(&root);
    let _ = std::fs::remove_dir_all(&root);
    assert!(index.unreadable.is_empty(), "gone is not unreadable");
    wikis.compile("p", &code(index), 2).unwrap();
    let page = wikis.page("p", "module-c-store").unwrap();
    assert!(page.blocks[1]
        .text
        .starts_with("> 可能过时：生成后 `c/src/store/mod.rs` 被删了或移出了这个模块。\n\n"));
    assert!(needs_rewrite(
        &explanation.sources_now(&overview_now(&page))
    ));
}

/// A9: a module whose only file cannot be read keeps its page as it was;
/// once the file is deleted the page goes.
#[cfg(unix)]
#[test]
fn a_module_whose_files_cannot_be_read_keeps_its_page() {
    let root = fixture();
    let (index, input) = input_of(&root);
    let wikis = wikis();
    wikis
        .save_explanation("p", &explanation_of(&input))
        .unwrap();
    wikis.compile("p", &code(index), 1).unwrap();
    let before = wikis.page("p", "module-c-store").unwrap();

    let file = root.join("c/src/store/mod.rs");
    if !lock(&file) {
        let _ = std::fs::remove_dir_all(&root);
        return;
    }
    let index = scan(&root);
    unlock(&file);
    assert!(index.modules.iter().all(|m| m.id != "c::store"));
    wikis.compile("p", &code(index), 2).unwrap();
    let kept = wikis.page("p", "module-c-store").unwrap();
    assert_eq!(kept.blocks, before.blocks, "kept as it was");

    std::fs::remove_file(&file).unwrap();
    let index = scan(&root);
    let _ = std::fs::remove_dir_all(&root);
    wikis.compile("p", &code(index), 3).unwrap();
    assert!(
        wikis.page("p", "module-c-store").is_err(),
        "gone once deleted"
    );
}

#[cfg(unix)]
fn lock_dir(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read_dir(path).is_ok() {
        unlock_dir(path);
        return false;
    }
    true
}

#[cfg(unix)]
fn unlock_dir(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// B1-9: a module page stays while its folder cannot be listed, its
/// package manifest cannot be read, or its only file is too large to read.
#[cfg(unix)]
#[test]
fn a_module_page_stays_while_its_folder_manifest_or_size_hides_it() {
    let root = fixture();
    let (index, input) = input_of(&root);
    let wikis = wikis();
    wikis
        .save_explanation("p", &explanation_of(&input))
        .unwrap();
    wikis.compile("p", &code(index), 1).unwrap();
    let before = wikis.page("p", "module-c-store").unwrap();

    // The folder cannot be listed (outside git).
    let folder = root.join("c/src/store");
    if !lock_dir(&folder) {
        let _ = std::fs::remove_dir_all(&root);
        return;
    }
    let index = scan(&root);
    unlock_dir(&folder);
    assert!(
        index.unreadable_dirs.contains("c/src/store/"),
        "{:?}",
        index.unreadable_dirs
    );
    wikis.compile("p", &code(index), 2).unwrap();
    assert_eq!(
        wikis.page("p", "module-c-store").unwrap().blocks,
        before.blocks
    );

    // The package manifest cannot be read.
    let manifest = root.join("c/Cargo.toml");
    assert!(lock(&manifest));
    let index = scan(&root);
    unlock(&manifest);
    assert!(index.unreadable.contains("c/Cargo.toml"));
    assert!(index.unreadable_dirs.contains("c/"));
    wikis.compile("p", &code(index), 3).unwrap();
    assert_eq!(
        wikis.page("p", "module-c-store").unwrap().blocks,
        before.blocks
    );

    // Its only file grew past what is read.
    let big = "// x\n".repeat(super::code_index::MAX_FILE_BYTES as usize / 5 + 1);
    write(&root, "c/src/store/mod.rs", &big);
    let index = scan(&root);
    assert!(index.too_large.contains("c/src/store/mod.rs"));
    wikis.compile("p", &code(index), 4).unwrap();
    assert_eq!(
        wikis.page("p", "module-c-store").unwrap().blocks,
        before.blocks
    );

    // Deleted, the page goes.
    std::fs::remove_file(root.join("c/src/store/mod.rs")).unwrap();
    let index = scan(&root);
    let _ = std::fs::remove_dir_all(&root);
    wikis.compile("p", &code(index), 5).unwrap();
    assert!(wikis.page("p", "module-c-store").is_err());
}
