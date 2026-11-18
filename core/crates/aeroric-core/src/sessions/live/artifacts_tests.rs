use std::process::Command;

use aeroric_protocol::turn_ledger::{TurnAction, TurnCheck, TurnState};

use super::*;
use crate::sessions::live::turn_changes::{parse_numstat_z, snapshot_tree};

fn run(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn repo() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aeroric-artifacts-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    run(&dir, &["init", "-q"]);
    std::fs::write(dir.join("keep.txt"), "keep\n").unwrap();
    run(&dir, &["add", "-A"]);
    run(&dir, &["commit", "-q", "-m", "base"]);
    dir.canonicalize().unwrap()
}

struct Fixture {
    dir: PathBuf,
    ledger: Arc<TurnLedger>,
    registry: ArtifactRegistry,
    clock: u64,
}

impl Fixture {
    fn new() -> Self {
        let ledger: Arc<TurnLedger> = Arc::default();
        let registry = ArtifactRegistry::new(Arc::clone(&ledger));
        registry.set_dir(data_dir());
        Self {
            dir: repo(),
            ledger,
            registry,
            clock: 1_000,
        }
    }

    /// One turn: `change` edits the work tree; the turn is recorded and
    /// registered as the tracker does it.
    fn turn(&mut self, turn_id: &str, change: impl FnOnce(&Path)) -> TurnRecord {
        let (top, before) = snapshot_tree(&self.dir).unwrap();
        change(&self.dir);
        let (_, after) = snapshot_tree(&self.dir).unwrap();
        let raw = run(
            &self.dir,
            &["diff", "--numstat", "-z", "--no-renames", &before, &after],
        );
        self.clock += 100;
        let record = TurnRecord {
            turn_id: turn_id.into(),
            started_ms: self.clock,
            ended_ms: self.clock + 50,
            top: top.to_string_lossy().into(),
            before,
            after,
            head_before: None,
            head_after: None,
            files: parse_numstat_z(&raw),
            additions: 0,
            deletions: 0,
            state: TurnState::Pending,
            undone_ms: None,
            undo_group: None,
            state_before_undo: None,
            check: None,
        };
        self.ledger.record("task-1", record.clone());
        self.registry.register("task-1", &record);
        record
    }

    fn only(&self) -> Artifact {
        let page = self.registry.list("task-1", None);
        assert_eq!(page.artifacts.len(), 1, "{page:?}");
        page.artifacts[0].clone()
    }

    fn text(&self, id: &str, version: Option<u32>) -> String {
        let read = self.registry.read("task-1", id, version, 0, None).unwrap();
        assert_eq!(read.encoding, "utf8");
        read.content
    }
}

fn data_dir() -> PathBuf {
    std::env::temp_dir().join(format!("aeroric-artifacts-data-{}", uuid::Uuid::new_v4()))
}

fn write<'a>(path: &'a str, text: &'a str) -> impl FnOnce(&Path) + 'a {
    move |dir: &Path| {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
}

#[test]
fn a_file_changed_in_three_turns_has_three_versions_each_readable() {
    let mut f = Fixture::new();
    f.turn("t1", write("src/report.md", "one\n"));
    f.turn("t2", write("src/report.md", "two\n"));
    f.turn("t3", write("src/report.md", "three\n"));

    let artifact = f.only();
    assert_eq!(artifact.path, "src/report.md");
    assert_eq!(artifact.kind, ArtifactKind::Document);
    assert_eq!(artifact.latest_version, 3);
    assert_eq!(artifact.current.as_ref().map(|v| v.version), Some(3));

    let versions = f.registry.versions("task-1", &artifact.id).unwrap();
    let shape: Vec<_> = versions
        .iter()
        .map(|v| (v.version, v.turn_id.as_str(), v.superseded, v.withdrawn))
        .collect();
    assert_eq!(
        shape,
        [
            (3, "t3", false, false),
            (2, "t2", true, false),
            (1, "t1", true, false)
        ]
    );
    assert_eq!(f.text(&artifact.id, Some(1)), "one\n");
    assert_eq!(f.text(&artifact.id, Some(2)), "two\n");
    assert_eq!(f.text(&artifact.id, None), "three\n");

    // Each version is pinned, so the object store keeps it.
    let pins = run(
        &f.dir,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/aeroric/artifacts",
        ],
    );
    assert_eq!(pins.lines().count(), 3, "{pins}");
}

#[test]
fn an_undone_turn_withdraws_its_version_and_the_one_before_stands() {
    let mut f = Fixture::new();
    f.turn("t1", write("src/a.rs", "fn a() {}\n"));
    f.turn("t2", write("src/a.rs", "fn a() { 2 }\n"));
    f.ledger
        .apply("task-1", "t2", TurnAction::Undo, false)
        .unwrap();

    let artifact = f.only();
    assert_eq!(artifact.latest_version, 2);
    assert_eq!(artifact.current.as_ref().map(|v| v.version), Some(1));
    let versions = f.registry.versions("task-1", &artifact.id).unwrap();
    assert!(versions[0].withdrawn, "v2 is withdrawn, not deleted");
    assert!(!versions[1].superseded, "v1 stands again");
    assert_eq!(f.text(&artifact.id, None), "fn a() {}\n");
    assert_eq!(f.text(&artifact.id, Some(2)), "fn a() { 2 }\n");

    // Taking the undo back makes v2 stand again.
    f.ledger
        .apply("task-1", "t2", TurnAction::Restore, false)
        .unwrap();
    assert_eq!(f.only().current.map(|v| v.version), Some(2));
}

#[test]
fn a_file_deleted_and_made_again_is_a_new_version_and_keeps_the_old() {
    let mut f = Fixture::new();
    f.turn("t1", write("out/data.csv", "a,b\n"));
    f.turn("t2", |dir: &Path| {
        std::fs::remove_file(dir.join("out/data.csv")).unwrap()
    });
    let removed = f.only();
    assert!(removed.removed);
    assert_eq!(removed.latest_version, 1);

    f.turn("t3", write("out/data.csv", "a,b\n1,2\n"));
    let back = f.only();
    assert!(!back.removed);
    assert_eq!(back.id, removed.id, "the same path is the same artifact");
    assert_eq!(back.latest_version, 2);
    assert_eq!(f.text(&back.id, Some(1)), "a,b\n");
    assert_eq!(f.text(&back.id, Some(2)), "a,b\n1,2\n");
}

#[test]
fn a_version_lists_the_checks_that_ran_while_it_stood() {
    let mut f = Fixture::new();
    f.turn("t1", write("src/lib.rs", "v1\n"));
    f.ledger.set_check(
        "task-1",
        "t1",
        TurnCheck {
            passed: false,
            summary: "测试 1 项失败".into(),
            items: vec![],
            incomplete: false,
        },
    );
    // Another file changes; lib.rs v1 is still what the check saw.
    f.turn("t2", write("src/other.rs", "x\n"));
    f.ledger.set_check(
        "task-1",
        "t2",
        TurnCheck {
            passed: true,
            summary: String::new(),
            items: vec![],
            incomplete: true,
        },
    );
    f.turn("t3", write("src/lib.rs", "v2\n"));
    f.ledger.set_check(
        "task-1",
        "t3",
        TurnCheck {
            passed: true,
            summary: String::new(),
            items: vec![],
            incomplete: false,
        },
    );

    let page = f.registry.list("task-1", None);
    let lib = page
        .artifacts
        .iter()
        .find(|a| a.path == "src/lib.rs")
        .unwrap();
    let versions = f.registry.versions("task-1", &lib.id).unwrap();
    let seen = |v: &ArtifactVersion| -> Vec<(String, String, bool)> {
        v.checks
            .iter()
            .map(|c| (c.evidence.detail.clone(), c.verdict.clone(), c.incomplete))
            .collect()
    };
    assert_eq!(seen(&versions[0]), [("t3".into(), "passed".into(), false)]);
    assert_eq!(
        seen(&versions[1]),
        [
            ("t1".into(), "failed".into(), false),
            ("t2".into(), "passed".into(), true)
        ]
    );
    assert_eq!(versions[1].checks[0].evidence.kind, EvidenceKind::AutoCheck);
}

#[test]
fn scratch_files_and_symlinks_are_not_deliverables() {
    let mut f = Fixture::new();
    f.turn("t1", |dir: &Path| {
        write("tmp/x.txt", "x")(dir);
        write("node_modules/p/i.js", "x")(dir);
        write("notes.md.swp", "x")(dir);
        write("docs/guide.md", "# guide\n")(dir);
        #[cfg(unix)]
        std::os::unix::fs::symlink("docs/guide.md", dir.join("link.md")).unwrap();
    });
    assert_eq!(f.only().path, "docs/guide.md");
    // A name that looks like a pattern is that name.
    f.turn("t2", write("docs/[draft]*.md", "draft\n"));
    let page = f.registry.list("task-1", None);
    let draft = page
        .artifacts
        .iter()
        .find(|a| a.path == "docs/[draft]*.md")
        .expect("registered by its literal name");
    assert_eq!(f.text(&draft.id, None), "draft\n");
    assert!(!counts(".cache/a"));
    assert!(counts("src/temporal.rs"));
    assert_eq!(kind_of("a/b/Icon.PNG"), ArtifactKind::Image);
    assert_eq!(kind_of("Makefile"), ArtifactKind::Other);
}

#[test]
fn a_version_whose_object_is_gone_says_so_instead_of_reading_empty() {
    let mut f = Fixture::new();
    f.turn("t1", write("a.txt", "a\n"));
    let id = f.only().id;
    f.registry.update("task-1", |book| {
        book.artifacts[0].versions[0].blob = "0".repeat(40);
    });
    assert_eq!(
        f.registry.read("task-1", &id, Some(1), 0, None),
        Err(GONE.to_string())
    );
    assert_eq!(
        f.registry.read("task-1", &id, Some(9), 0, None),
        Err(NO_VERSION.to_string())
    );
    assert_eq!(
        f.registry.read("task-1", "art-nope", None, 0, None),
        Err(NO_ARTIFACT.to_string())
    );
}

#[test]
fn reading_in_slices_cuts_text_on_characters_and_says_where_to_go_on() {
    let mut f = Fixture::new();
    let text = "交付物".repeat(10); // 90 bytes, 3 per character
    f.turn("t1", write("a.md", &text));
    let id = f.only().id;
    let first = f.registry.read("task-1", &id, None, 0, Some(10)).unwrap();
    assert_eq!(first.encoding, "utf8");
    assert_eq!(first.content, "交付物");
    assert_eq!(first.next_offset, Some(9));
    let mut all = first.content.clone();
    let mut next = first.next_offset;
    while let Some(offset) = next {
        let part = f
            .registry
            .read("task-1", &id, None, offset, Some(10))
            .unwrap();
        all.push_str(&part.content);
        next = part.next_offset;
    }
    assert_eq!(all, text, "the slices put together are the file");

    f.turn("t2", write("b.bin", "\u{0}\u{1}"));
    let bin = f
        .registry
        .list("task-1", None)
        .artifacts
        .into_iter()
        .find(|a| a.path == "b.bin")
        .unwrap();
    let mut bytes = std::fs::read(f.dir.join("b.bin")).unwrap();
    bytes.push(0xff);
    let written = bytes.clone();
    f.turn("t3", move |dir: &Path| {
        std::fs::write(dir.join("b.bin"), written).unwrap()
    });
    let read = f.registry.read("task-1", &bin.id, None, 0, None).unwrap();
    assert_eq!(read.encoding, "base64");
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(read.content)
            .unwrap(),
        bytes
    );
}

#[test]
fn the_list_pages_with_a_cursor_and_survives_a_restart() {
    let mut f = Fixture::new();
    f.turn("t1", |dir: &Path| {
        for i in 0..(PAGE + 5) {
            write(&format!("out/f{i:03}.txt"), "x")(dir);
        }
    });
    let first = f.registry.list("task-1", None);
    assert_eq!(first.artifacts.len(), PAGE);
    let cursor = first.next_cursor.clone().expect("a cursor");
    let rest = f.registry.list("task-1", Some(&cursor));
    assert_eq!(rest.artifacts.len(), 5);
    assert_eq!(rest.next_cursor, None);

    let id = first.artifacts[0].id.clone();
    f.registry.exclude("task-1", &id, true).unwrap();

    // A new registry over the same folder reads it all back.
    let dir = f.registry.dir.lock().clone().unwrap();
    let again = ArtifactRegistry::new(Arc::clone(&f.ledger));
    again.set_dir(dir);
    let page = again.list("task-1", None);
    assert_eq!(page.artifacts.len(), PAGE);
    assert!(page.artifacts.iter().any(|a| a.id == id && a.excluded));
}

#[test]
fn a_task_keeps_its_latest_artifacts_and_says_how_many_went() {
    let mut f = Fixture::new();
    f.turn("t1", |dir: &Path| {
        for i in 0..MAX_ARTIFACTS {
            write(&format!("old/f{i:03}.txt"), "x")(dir);
        }
    });
    f.turn("t2", write("new.txt", "new"));
    let mut all = Vec::new();
    let mut cursor = None;
    loop {
        let page = f.registry.list("task-1", cursor.as_deref());
        assert_eq!(page.dropped, 1);
        all.extend(page.artifacts);
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(all.len(), MAX_ARTIFACTS);
    assert_eq!(all[0].path, "new.txt", "the latest changed comes first");
}

#[test]
fn a_folder_outside_git_says_versions_cannot_be_kept() {
    let dir = std::env::temp_dir().join(format!("aeroric-no-git-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    assert_eq!(unavailable(&dir), Some(NOT_GIT.to_string()));
    assert_eq!(unavailable(&repo()), None);
}
