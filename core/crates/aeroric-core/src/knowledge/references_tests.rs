use super::*;

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aeroric-ref-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A small folder outside git: Python with a class, notes, and things to skip.
fn sample() -> PathBuf {
    let dir = temp("src");
    std::fs::create_dir_all(dir.join("models")).unwrap();
    std::fs::create_dir_all(dir.join("node_modules/x")).unwrap();
    std::fs::write(
        dir.join("models/head.py"),
        "import torch\n\nclass DetectHead:\n    \"\"\"Decoupled detection head.\"\"\"\n\n    def forward(self, x):\n        return self.cls(x), self.reg(x)\n",
    )
    .unwrap();
    std::fs::write(dir.join("README.md"), "# 示例\n\n这里讲解标签分配策略。\n").unwrap();
    std::fs::write(
        dir.join("node_modules/x/index.js"),
        "export const DetectHead = 1;\n",
    )
    .unwrap();
    std::fs::write(dir.join("weights.bin"), [0u8, 1, 2, 3]).unwrap();
    dir
}

fn library() -> References {
    let library = References::default();
    library.set_dir(temp("lib"));
    library
}

#[test]
fn a_folder_is_registered_once_and_indexed() {
    let library = library();
    let src = sample();
    let reference = library
        .add(src.to_str().unwrap(), None, Some("检测头的写法"))
        .unwrap();
    assert!(
        reference.id.starts_with("aeroric-ref-src-"),
        "{}",
        reference.id
    );
    assert_eq!(reference.purpose.as_deref(), Some("检测头的写法"));
    assert!(matches!(
        library.add(src.to_str().unwrap(), Some("again"), None),
        Err(ReferenceError::Duplicate(_))
    ));
    assert!(matches!(
        library.add("/no/such/folder", None, None),
        Err(ReferenceError::NotAFolder(_))
    ));

    assert!(library.begin_index(&reference.id));
    assert!(
        !library.begin_index(&reference.id),
        "one indexer per folder"
    );
    let report = library.index(&reference.id).unwrap();
    // head.py and README.md; node_modules and the binary are skipped.
    assert_eq!(report.files, 2);
    assert_eq!(report.read, 2);
    let status = library.status(reference.clone());
    assert_eq!(status.files, 2);
    assert!(status.symbols >= 1);
    assert!(!status.indexing && !status.stale);
    assert!(status.indexed_ms.is_some());

    // Nothing changed: nothing is read again.
    library.begin_index(&reference.id);
    assert_eq!(library.index(&reference.id).unwrap().read, 0);
}

#[test]
fn search_finds_definitions_first_then_text_in_any_language() {
    let library = library();
    let src = sample();
    let reference = library
        .add(src.to_str().unwrap(), Some("demo"), None)
        .unwrap();
    library.begin_index(&reference.id);
    library.index(&reference.id).unwrap();

    let hits = library.search("DetectHead", None).unwrap();
    assert_eq!(hits[0].kind, "symbol:class");
    assert_eq!(hits[0].path, "models/head.py");
    assert_eq!(hits[0].line, 3);
    assert_eq!(hits[0].repo, "demo");
    assert!(hits[0].excerpt.contains("class DetectHead"));

    let chinese = library.search("标签分配", None).unwrap();
    assert_eq!(chinese[0].path, "README.md");
    assert_eq!(chinese[0].line, 3);

    assert_eq!(
        library.search("forward", Some("demo")).unwrap()[0].path,
        "models/head.py"
    );
    assert!(matches!(
        library.search("x", Some("nope")),
        Err(ReferenceError::Unknown(_))
    ));
}

#[test]
fn changes_are_seen_and_only_they_are_read() {
    let library = library();
    let src = sample();
    let reference = library.add(src.to_str().unwrap(), None, None).unwrap();
    library.begin_index(&reference.id);
    library.index(&reference.id).unwrap();

    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(
        src.join("models/anchor.py"),
        "def assign_anchors(boxes):\n    pass\n",
    )
    .unwrap();
    std::fs::remove_file(src.join("README.md")).unwrap();
    assert!(library.status(reference.clone()).stale);

    library.begin_index(&reference.id);
    let report = library.index(&reference.id).unwrap();
    assert_eq!((report.read, report.removed, report.files), (1, 1, 2));
    assert!(library.search("标签分配", None).unwrap().is_empty());
    assert_eq!(
        library.search("assign_anchors", None).unwrap()[0].kind,
        "symbol:def"
    );
}

#[test]
fn reads_stay_inside_the_folder_and_are_numbered() {
    let library = library();
    let src = sample();
    let reference = library
        .add(src.to_str().unwrap(), Some("demo"), None)
        .unwrap();

    let text = library
        .read("demo", "models/head.py", Some(3), Some(4))
        .unwrap();
    assert!(
        text.starts_with("demo · models/head.py 第 3–4 行"),
        "{text}"
    );
    assert!(text.contains("3: class DetectHead:"));
    assert!(text.contains("还有 3 行"));

    for bad in ["../outside.txt", "/etc/hosts", "models/../../x"] {
        assert_eq!(
            library.read(&reference.id, bad, None, None),
            Err(ReferenceError::OutsideFolder),
            "{bad}"
        );
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/etc", src.join("escape")).unwrap();
        assert_eq!(
            library.read("demo", "escape/hosts", None, None),
            Err(ReferenceError::OutsideFolder)
        );
    }
    assert!(matches!(
        library.read("demo", "weights.bin", None, None),
        Err(ReferenceError::NotAFile(_))
    ));
}

#[test]
fn removing_forgets_the_index_but_not_the_folder() {
    let library = library();
    let src = sample();
    let reference = library.add(src.to_str().unwrap(), None, None).unwrap();
    library.begin_index(&reference.id);
    library.index(&reference.id).unwrap();
    library.remove(&reference.id).unwrap();
    assert!(library.list().is_empty());
    assert!(src.join("models/head.py").exists());
    assert!(!library.root().unwrap().join(&reference.id).exists());
    assert!(matches!(
        library.remove(&reference.id),
        Err(ReferenceError::Unknown(_))
    ));
}
