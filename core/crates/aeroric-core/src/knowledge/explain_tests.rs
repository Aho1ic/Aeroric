use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::code_index::{scan, CodeIndex};
use super::compile::Sources;
use super::explain::{build, cite_ok, prompt, render, verify, Claim, Explanation};
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
    let current: BTreeMap<String, String> =
        [("c/src/store/mod.rs".to_string(), "new".to_string())].into();
    assert_eq!(explanation.changed(&current), ["c/src/store/mod.rs"]);
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
