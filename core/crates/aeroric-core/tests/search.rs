//! G43 regressions. No ambient home, SSH, GUI or network access.
use aeroric_core::files::search::*;
use std::path::Path;
#[path = "common/scratch.rs"]
mod scratch;

fn options() -> TextSearchOptions {
    TextSearchOptions::default()
}

fn replacements(preview: ReplacePreview) -> Vec<TextReplacement> {
    preview
        .files
        .into_iter()
        .flat_map(|file| file.matches.into_iter().map(TextReplacement::from))
        .collect()
}

#[test]
fn rg_records_are_self_contained_root_scoped_and_bounded() {
    let record = |path: &str, start: usize| {
        serde_json::json!({
            "type":"match", "data": {
                "path":{"text":path}, "lines":{"text":"你好 title\r\n"},
                "line_number":3, "absolute_offset":20,
                "submatches":[{"match":{"text":"title"}, "start":start, "end":12}]
            }
        })
        .to_string()
    };
    let corpus = [
        "not-json".to_string(),
        record("../secret", 7),
        record("/secret", 7),
        record("bad.txt", usize::MAX),
        record("bad-utf8.txt", 1),
        record("src/文件 名.txt", 7),
        record("./second.txt", 7),
    ]
    .join("\n");
    let found = parse_rg_json_lines(Path::new("/repo"), &corpus, 1);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path, "/repo/src/文件 名.txt");
    assert_eq!(found[0].column, 8);
    assert_eq!(found[0].line_text, "你好 title");
    assert!(parse_rg_json_lines(Path::new("/repo"), &corpus, 0).is_empty());
}

#[test]
fn missing_rg_falls_back_with_globs_word_boundary_and_limit() {
    let dir = scratch::Scratch::new("g43-search");
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::create_dir_all(dir.path().join("target")).unwrap();
    std::fs::write(dir.path().join("src/a.txt"), "subtitle TITLE\ntitle\n").unwrap();
    std::fs::write(dir.path().join("src/no.md"), "title\n").unwrap();
    std::fs::write(dir.path().join("target/no.txt"), "title\n").unwrap();
    let opts = TextSearchOptions {
        whole_word: Some(true),
        include_glob: Some("*.txt".into()),
        limit: Some(1),
        ..options()
    };
    let found =
        search_text_for_root(dir.path(), "title", &opts, "/nonexistent-g43-rg", &[]).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].match_text, "TITLE");
    assert_eq!(found[0].column, 10);
}

#[test]
fn single_file_preview_has_original_utf8_offsets_and_expanded_captures() {
    let dir = scratch::Scratch::new("g43-search");
    std::fs::write(dir.path().join("code.txt"), "İ title\r\n你好 title2\n").unwrap();
    let opts = TextSearchOptions {
        regex: Some(true),
        ..options()
    };
    let preview = replace_text_preview_for_file(
        dir.path(),
        "code.txt",
        "(title)([0-9]?)",
        "${1}_${2}",
        &opts,
    )
    .unwrap();
    assert_eq!(preview.total_matches, 2);
    assert_eq!(preview.files[0].matches[0].start, 3);
    assert_eq!(preview.files[0].matches[1].start, 17);
    assert_eq!(preview.files[0].matches[1].replacement_text, "title_2");
    let summary = apply_text_replacements_for_root(dir.path(), &replacements(preview)).unwrap();
    assert_eq!(summary.replacements_applied, 2);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("code.txt")).unwrap(),
        "İ title_\r\n你好 title_2\n"
    );
}

#[test]
fn single_file_preview_empty_and_exact_limit_do_not_claim_truncation() {
    let dir = scratch::Scratch::new("g43-search");
    std::fs::write(dir.path().join("code.txt"), "title title\n").unwrap();
    let empty = replace_text_preview_for_file(dir.path(), "code.txt", "", "x", &options()).unwrap();
    assert_eq!(empty.total_matches, 0);
    assert!(empty.files.is_empty());
    let opts = TextSearchOptions {
        limit: Some(2),
        ..options()
    };
    let exact = replace_text_preview_for_file(dir.path(), "code.txt", "title", "x", &opts).unwrap();
    assert!(!exact.truncated);
    let opts = TextSearchOptions {
        limit: Some(1),
        ..options()
    };
    let truncated =
        replace_text_preview_for_file(dir.path(), "code.txt", "title", "x", &opts).unwrap();
    assert!(truncated.truncated);
    assert_eq!(truncated.total_matches, 1);
}

#[test]
fn apply_rejects_same_length_stale_invalid_utf8_reversed_and_overlapping_ranges() {
    let dir = scratch::Scratch::new("g43-search");
    let file = dir.path().join("code.txt");
    std::fs::write(&file, "你好 title\n").unwrap();
    let item = |start, end, matched: &str| TextReplacement {
        path: file.to_string_lossy().into_owned(),
        start,
        end,
        match_text: matched.into(),
        replacement_text: "name".into(),
    };
    let summary = apply_text_replacements_for_root(
        dir.path(),
        &[
            item(7, 12, "other"),
            item(1, 3, ""),
            item(6, 5, ""),
            item(7, 12, "title"),
            item(7, 12, "title"),
        ],
    )
    .unwrap();
    assert_eq!(summary.replacements_applied, 1);
    assert_eq!(summary.replacements_skipped, 4);
    assert_eq!(std::fs::read_to_string(file).unwrap(), "你好 name\n");
}

#[cfg(unix)]
#[test]
fn all_local_walks_skip_symlink_escape_and_directory_cycles() {
    let dir = scratch::Scratch::new("g43-search");
    let root = dir.path().join("project");
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), "function title(a)\n").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();
    std::os::unix::fs::symlink(&root, root.join("cycle")).unwrap();
    std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("secret.txt")).unwrap();
    assert!(
        search_text_for_root(&root, "title", &options(), "/nonexistent-g43-rg", &[])
            .unwrap()
            .is_empty()
    );
    assert!(
        structured_search_for_root(&root, "function $Name$($Args$)", &options())
            .unwrap()
            .is_empty()
    );
    assert!(
        replace_text_preview_for_root(&root, "title", "oops", &options())
            .unwrap()
            .files
            .is_empty()
    );
    assert!(
        replace_text_preview_for_file(&root, "secret.txt", "title", "oops", &options()).is_err()
    );
}

#[test]
fn structured_search_preserves_multiline_templates_and_limits() {
    let dir = scratch::Scratch::new("g43-search");
    std::fs::write(
        dir.path().join("code.ts"),
        "function helper(\n a: string\n) {}\nfunction next() {}",
    )
    .unwrap();
    let opts = TextSearchOptions {
        limit: Some(1),
        ..options()
    };
    let found = structured_search_for_root(dir.path(), "function $Name$($Args$)", &opts).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].match_text, "function helper(\n a: string\n)");
}

#[cfg(unix)]
#[test]
fn remote_algorithms_use_injected_transport_without_ssh_or_credentials() {
    struct Fake {
        calls: std::sync::Mutex<Vec<String>>,
    }
    impl RemoteSearchTransport for Fake {
        fn command(&self, remote_command: String) -> std::process::Command {
            self.calls.lock().unwrap().push(remote_command);
            let mut cmd = std::process::Command::new("/bin/sh");
            // rg exit 1 = no further matches, not a transport failure.
            cmd.args(["-c", "printf '%s' \"$1\"; exit 1", "fake-rg"])
                .arg(r#"{"type":"match","data":{"path":{"text":"code.txt"},"lines":{"text":"title\n"},"line_number":1,"absolute_offset":0,"submatches":[{"match":{"text":"title"},"start":0,"end":5}]}}"#);
            cmd
        }
        fn annotate_error(&self, detail: &str) -> String {
            format!("fake: {detail}")
        }
    }
    let fake = Fake {
        calls: Default::default(),
    };
    let root = "/srv/repo with 'quote";
    let found = remote_search_text_for_root(&fake, root, "title", &options()).unwrap();
    assert_eq!(found[0].path, format!("{root}/code.txt"));
    let preview =
        remote_replace_text_preview_for_root(&fake, root, "title", "name", &options()).unwrap();
    assert_eq!(preview.total_matches, 1);
    assert_eq!(preview.files[0].matches[0].match_text, "title");
    let calls = fake.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert!(calls
        .iter()
        .all(|c| c.starts_with("cd -- '/srv/repo with '\\''quote' && rg ")));
    assert!(calls.iter().all(|c| c.ends_with("'--' 'title'")));
}
