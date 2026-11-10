use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use aeroric_protocol::wiki::{SourceKind, WikiPageKind};

use super::code_index::{blob_hash, scan, CodeIndex, Lang, Module, MAX_MODULES};
use super::compile::Sources;
use super::Wikis;

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A project of four languages, outside git (so the walk lists it).
fn fixture() -> PathBuf {
    let root = std::env::temp_dir().join(format!("aeroric-code-{}", uuid::Uuid::new_v4()));
    write(&root, "core/a/Cargo.toml", "[package]\nname = \"a-core\"\n");
    write(
        &root,
        "core/a/src/lib.rs",
        "pub mod sessions;\nmod util;\nuse crate::sessions::Store;\n/// The engine.\npub struct Engine;\n",
    );
    write(
        &root,
        "core/a/src/sessions/mod.rs",
        "use crate::util::helper;\npub struct Store;\npub(crate) fn hidden() {}\nfn private() {}\n",
    );
    write(&root, "core/a/src/util.rs", "pub fn helper() {}\n");
    write(
        &root,
        "core/b/Cargo.toml",
        "[package]\nname = \"b-cli\"\n[dependencies]\nname = \"no\"\n",
    );
    write(
        &root,
        "core/b/src/main.rs",
        "use a_core::sessions::Store;\nfn main() {}\n",
    );
    write(&root, "core/target/debug/junk.rs", "pub struct Junk;\n");
    write(
        &root,
        "mac/Kit/Package.swift",
        "// swift-tools-version:5.9\n",
    );
    write(
        &root,
        "mac/Kit/Sources/KitUI/View.swift",
        "import SwiftUI\nimport KitCore\npublic struct Card {}\nprivate class Hidden {}\n",
    );
    write(
        &root,
        "mac/Kit/Sources/KitCore/Model.swift",
        "public protocol Thing {}\nextension Int {}\nenum Mode { case a }\n",
    );
    write(&root, "py/pkg/__init__.py", "");
    write(
        &root,
        "py/pkg/core.py",
        "class Engine:\n    pass\ndef _private():\n    pass\n",
    );
    write(&root, "py/pkg/sub/__init__.py", "");
    write(
        &root,
        "py/pkg/sub/x.py",
        "from pkg.core import Engine\nimport os\n",
    );
    write(&root, "web/package.json", "{\"name\": \"@app/web\"}");
    write(
        &root,
        "web/src/components/Button.tsx",
        "import { api } from \"../api/client\";\nimport React from \"react\";\nexport class Button {}\n",
    );
    write(
        &root,
        "web/src/api/client.ts",
        "export function api() {}\nconst hidden = 1;\n",
    );
    write(
        &root,
        "web/node_modules/x/index.js",
        "export function nope() {}\n",
    );
    write(&root, "tools/gen/main.go", "package main\n");
    write(
        &root,
        "core/vendor/dep/src/lib.rs",
        "pub struct Vendored;\n",
    );
    root
}

fn module<'a>(index: &'a CodeIndex, id: &str) -> &'a Module {
    index
        .modules
        .iter()
        .find(|m| m.id == id)
        .unwrap_or_else(|| panic!("no module {id}: {:?}", ids(index)))
}

fn ids(index: &CodeIndex) -> BTreeSet<String> {
    index.modules.iter().map(|m| m.id.clone()).collect()
}

fn names(m: &Module) -> Vec<&str> {
    m.symbols.iter().map(|s| s.name.as_str()).collect()
}

fn deps(m: &Module) -> Vec<&str> {
    m.deps.iter().map(|e| e.to.as_str()).collect()
}

#[test]
fn modules_are_packages_and_their_first_level() {
    let root = fixture();
    let index = scan(&root);
    let _ = std::fs::remove_dir_all(&root);
    assert!(index.available);
    assert_eq!(index.commit, None);
    let expected: BTreeSet<String> = [
        "core/a",
        "core/a::sessions",
        "core/a::util",
        "core/b",
        "mac/Kit::KitCore",
        "mac/Kit::KitUI",
        "py/pkg",
        "py/pkg::sub",
        "web::api",
        "web::components",
        "tools::gen",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(
        ids(&index),
        expected,
        "build and dependency folders are skipped"
    );
    let a = module(&index, "core/a");
    assert_eq!(
        (a.package_name.as_str(), a.title().as_str()),
        ("a-core", "a-core")
    );
    assert_eq!(
        module(&index, "core/a::sessions").title(),
        "a-core · sessions"
    );
    assert_eq!(a.entries, ["core/a/src/lib.rs"]);
    assert_eq!(
        module(&index, "core/a::util").entries,
        ["core/a/src/util.rs"]
    );
    assert_eq!(module(&index, "web::components").package_name, "@app/web");
    assert_eq!(module(&index, "tools::gen").langs(), [Lang::Other].into());
}

#[test]
fn only_public_symbols_are_read() {
    let root = fixture();
    let index = scan(&root);
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(names(module(&index, "core/a")), ["Engine"]);
    assert_eq!(names(module(&index, "core/a::sessions")), ["Store"]);
    assert_eq!(names(module(&index, "mac/Kit::KitUI")), ["Card"]);
    // Swift's default is internal: kept; an extension is not a type.
    assert_eq!(names(module(&index, "mac/Kit::KitCore")), ["Thing", "Mode"]);
    assert_eq!(names(module(&index, "py/pkg")), ["Engine"]);
    assert_eq!(names(module(&index, "web::api")), ["api"]);
    let store = &module(&index, "core/a::sessions").symbols[0];
    assert_eq!((store.kind.as_str(), store.line), ("struct", 2));
    assert!(module(&index, "tools::gen").symbols.is_empty());
}

#[test]
fn edges_are_the_imports_that_name_a_module_of_the_project() {
    let root = fixture();
    let index = scan(&root);
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(deps(module(&index, "core/a")), ["core/a::sessions"]);
    assert_eq!(deps(module(&index, "core/a::sessions")), ["core/a::util"]);
    assert_eq!(deps(module(&index, "core/b")), ["core/a::sessions"]);
    // `import SwiftUI` and `import React` are not the project's.
    assert_eq!(deps(module(&index, "mac/Kit::KitUI")), ["mac/Kit::KitCore"]);
    assert_eq!(deps(module(&index, "web::components")), ["web::api"]);
    assert_eq!(deps(module(&index, "py/pkg::sub")), ["py/pkg"]);
    let edge = &module(&index, "core/b").deps[0];
    assert_eq!(
        (edge.path.as_str(), edge.line, edge.text.as_str()),
        ("core/b/src/main.rs", 1, "a_core::sessions::Store")
    );
}

#[test]
fn a_file_version_is_its_git_blob_hash() {
    // `printf 'hello\n' | git hash-object --stdin`
    assert_eq!(
        blob_hash(b"hello\n"),
        "ce013625030ba8dba906f756967f9e9ca394464a"
    );
}

#[test]
fn too_many_modules_fold_the_biggest_package_first() {
    let make = |package: &str, name: Option<String>, lines: u32| Module {
        id: match &name {
            Some(n) => format!("{package}::{n}"),
            None => package.to_string(),
        },
        package: package.into(),
        package_name: package.into(),
        name,
        dir: package.into(),
        files: vec![super::code_index::CodeFile {
            path: format!("{package}/{lines}"),
            blob: String::new(),
            lines,
            lang: Lang::Rust,
        }],
        entries: Vec::new(),
        symbols: Vec::new(),
        deps: Vec::new(),
    };
    let mut modules: Vec<Module> = (0..MAX_MODULES + 5)
        .map(|i| make("big", Some(format!("m{i}")), 2000))
        .chain((0..8).map(|i| make("small", Some(format!("s{i}")), 10)))
        .collect();
    let mut omitted = 0;
    super::code_index::fold_for_test(&mut modules, &mut omitted);
    // The small package folds into one page; the big one keeps its pages
    // but its seven smallest become one "其他" page.
    assert_eq!(modules.len(), MAX_MODULES);
    assert!(modules
        .iter()
        .any(|m| m.id == "small" && m.files.len() == 8));
    let other = modules.iter().find(|m| m.id == "big::其他").unwrap();
    assert_eq!(other.files.len(), 7);
    assert_eq!(omitted, 0);
}

#[test]
fn module_pages_link_their_dependencies_and_keep_file_versions_out_of_the_history() {
    let root = fixture();
    let mut index = scan(&root);
    let _ = std::fs::remove_dir_all(&root);
    index.commit = Some("0123456789abcdef".into());
    let wikis = Wikis::default();
    wikis.set_dir(std::env::temp_dir().join(format!("aeroric-knowledge-{}", uuid::Uuid::new_v4())));
    let sources = |code: CodeIndex| Sources {
        code: Some(code),
        ..Sources::default()
    };
    wikis.compile("p", &sources(index.clone()), 1).unwrap();
    let page = wikis.page("p", "module-core-b").unwrap();
    assert_eq!(page.kind, WikiPageKind::Module);
    assert_eq!(page.category.as_deref(), Some("b-cli"));
    assert_eq!(page.source_commit.as_deref(), Some("0123456789abcdef"));
    let deps = page
        .blocks
        .iter()
        .find(|b| b.key == "module:core/b#deps")
        .unwrap();
    assert!(
        deps.text
            .contains("[a-core · sessions](module-core-a-sessions.md) — `core/b/src/main.rs:1`"),
        "{}",
        deps.text
    );
    let used = wikis.page("p", "module-core-a-sessions").unwrap();
    let dependents = used
        .blocks
        .iter()
        .find(|b| b.key.ends_with("#dependents"))
        .unwrap();
    assert!(dependents.text.contains("[b-cli](module-core-b.md)"));
    let overview = &page.blocks[0];
    let file = &overview.sources[0];
    assert_eq!(file.kind, SourceKind::Code);
    assert_eq!(file.target, "core/b/src/main.rs");
    assert_eq!(
        file.version.as_deref(),
        Some(index.modules[3].files[0].blob.as_str())
    );
    let index_page = wikis.page("p", "index").unwrap();
    assert!(index_page
        .blocks
        .iter()
        .any(|b| b.heading.as_deref() == Some("模块 · a-core")));

    // A file edited without changing what the page says: its version moves,
    // the page gets no new revision.
    let mut edited = index.clone();
    for m in &mut edited.modules {
        for f in &mut m.files {
            f.blob = "f".repeat(40);
        }
    }
    edited.commit = Some("fedcba".into());
    wikis.compile("p", &sources(edited), 2).unwrap();
    let after = wikis.page("p", "module-core-b").unwrap();
    assert_eq!(after.revision, page.revision);
    assert_eq!(
        after.blocks[0].sources[0].version.as_deref(),
        Some("f".repeat(40).as_str())
    );
    assert_eq!(after.source_commit.as_deref(), Some("fedcba"));
    assert_eq!(wikis.revisions("p", "module-core-b").unwrap().len(), 1);

    // The folder could not be read: the module pages stay.
    let gone = CodeIndex::default();
    wikis
        .compile_checked(
            "p",
            &sources(gone),
            &aeroric_protocol::wiki::WikiSourceGaps {
                all: true,
                slugs: Vec::new(),
            },
            3,
        )
        .unwrap();
    assert!(wikis.page("p", "module-core-b").is_ok());
}

#[test]
fn an_import_of_a_folded_module_points_at_the_page_it_went_into() {
    let root = std::env::temp_dir().join(format!("aeroric-code-{}", uuid::Uuid::new_v4()));
    write(&root, "c/Cargo.toml", "[package]\nname = \"c\"\n");
    write(
        &root,
        "c/src/lib.rs",
        "use crate::m0::X;\npub struct Root;\n/// big\n",
    );
    for i in 0..MAX_MODULES + 2 {
        write(&root, &format!("c/src/m{i}.rs"), "pub struct X;\n");
    }
    let index = scan(&root);
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(index.modules.len(), MAX_MODULES);
    assert_eq!(index.omitted, 0);
    let other = module(&index, "c::其他");
    assert_eq!(other.files.len(), 4, "the smallest, one merge at a time");
    assert_eq!(deps(module(&index, "c")), ["c::其他"]);
}
