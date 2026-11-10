use aeroric_protocol::memory::{
    Assertion, Confidence, ExperienceDetail, MemoryItem, MemoryKind, MemoryState, MemoryStats,
};
use aeroric_protocol::wiki::{WikiIssueKind, WikiPageKind};

use super::compile::{decision_slug, Sources};
use super::{notes, render, sections, WikiError, Wikis};

const NOTE: &str = "# Agent Note: 委派与契约（B4-2）\n\nStatus: implemented\n\n## Problem\n\n主会话只能自己干活。\n\n## Decision\n\n子会话只拿契约。\n\n## Alternatives considered\n\n做成 sub-agent。\n\n## Consequences\n\n没有 worktree。\n";

fn project() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aeroric-wiki-{}", uuid::Uuid::new_v4()));
    let notes = dir.join(".agents/notes/implemented/feature");
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::write(notes.join("2026-10-31-delegation.md"), NOTE).unwrap();
    dir
}

fn fresh() -> Wikis {
    let wikis = Wikis::default();
    wikis.set_dir(std::env::temp_dir().join(format!("aeroric-knowledge-{}", uuid::Uuid::new_v4())));
    wikis
}

fn experience() -> MemoryItem {
    MemoryItem {
        id: "mem-e1".into(),
        project_id: Some("p".into()),
        kind: MemoryKind::Experience,
        category: None,
        text: "CSV 带 BOM：读取改用 utf-8-sig".into(),
        assertion: Assertion::ModelInferred,
        state: MemoryState::Active,
        pinned: false,
        evidence: Vec::new(),
        supersedes: None,
        superseded_by: None,
        created_ms: 1,
        updated_ms: 1,
        experience: Some(ExperienceDetail {
            problem: "首列名多出 BOM".into(),
            solution: "encoding=\"utf-8-sig\"".into(),
            search_keys: vec![],
            envs: vec!["Python 3".into()],
        }),
        confidence: Some(Confidence::High),
        stats: MemoryStats {
            helpful: 3,
            ..MemoryStats::default()
        },
    }
}

#[test]
fn notes_parse_into_title_status_category_and_sections() {
    let note = notes::parse(
        ".agents/notes/implemented/feature/2026-10-31-delegation.md",
        NOTE,
    );
    assert_eq!(note.title, "委派与契约（B4-2）");
    assert_eq!(note.status.as_deref(), Some("implemented"));
    assert_eq!(note.category.as_deref(), Some("feature"));
    assert_eq!(note.sections.len(), 4);
    assert_eq!(
        note.sections[1],
        ("Decision".into(), "子会话只拿契约。".into())
    );
    assert_eq!(decision_slug(&note.path), "decision-delegation");
    assert_eq!(decision_slug("x/undated-note.md"), "decision-undated-note");
}

#[test]
fn compiling_builds_decisions_a_runbook_and_an_index_and_is_searchable() {
    let dir = project();
    let wikis = fresh();
    let sources = Sources {
        notes: notes::scan(&dir),
        experiences: vec![experience()],
        skills: vec![],
        code: None,
    };
    let report = wikis.compile("p", &sources, 10).unwrap();
    assert_eq!((report.added, report.pages), (3, 3));
    let pages = wikis.pages("p").unwrap();
    assert_eq!(
        pages.iter().map(|p| p.kind).collect::<Vec<_>>(),
        [
            WikiPageKind::Index,
            WikiPageKind::Runbook,
            WikiPageKind::Decision
        ]
    );
    let index = render(&wikis.page("p", "index").unwrap());
    assert!(
        index.contains("- [委派与契约（B4-2）](decision-delegation.md) · implemented"),
        "{index}"
    );
    assert!(index.contains("[操作手册](runbook.md)"));
    let runbook = render(&wikis.page("p", "runbook").unwrap());
    assert!(runbook.contains("## CSV 带 BOM：读取改用 utf-8-sig"));
    assert!(runbook.contains("适用：Python 3 · 置信度 高 · 有用 3 次"));
    let decision = wikis.page("p", "decision-delegation").unwrap();
    assert_eq!(
        decision.blocks[0].source.as_ref().unwrap().label,
        "2026-10-31-delegation.md#Problem"
    );
    // Compiling again changes nothing.
    let again = wikis.compile("p", &sources, 11).unwrap();
    assert_eq!((again.added, again.updated, again.issues), (0, 0, 0));
    assert_eq!(
        wikis.search("p", "契约").unwrap()[0].slug,
        "decision-delegation"
    );
    assert!(wikis.search("p", "火星").unwrap().is_empty());
}

#[test]
fn the_users_paragraph_survives_a_compile_and_the_change_becomes_an_issue() {
    let dir = project();
    let wikis = fresh();
    let compile = |at| {
        let sources = Sources {
            notes: notes::scan(&dir),
            ..Sources::default()
        };
        wikis.compile("p", &sources, at).unwrap()
    };
    compile(1);
    let page = wikis.page("p", "decision-delegation").unwrap();
    let markdown =
        render(&page).replace("子会话只拿契约。", "子会话只拿契约（我补充：omp 不支持）。");
    let edited = wikis
        .edit("p", "decision-delegation", &markdown, 2)
        .unwrap();
    let decision = edited
        .blocks
        .iter()
        .find(|b| b.heading.as_deref() == Some("Decision"))
        .unwrap();
    assert!(decision.user_edited);
    assert!(
        !edited.blocks[0].user_edited,
        "the untouched section stays the compiler's"
    );
    // The note moves on: the untouched section follows, the user's stays and an issue opens.
    let file = dir.join(".agents/notes/implemented/feature/2026-10-31-delegation.md");
    std::fs::write(
        &file,
        NOTE.replace("主会话只能自己干活。", "主会话只能单干。")
            .replace("子会话只拿契约。", "子会话拿契约与预算。"),
    )
    .unwrap();
    let report = compile(3);
    assert_eq!(report.issues, 1);
    let page = wikis.page("p", "decision-delegation").unwrap();
    assert_eq!(page.blocks[0].text, "主会话只能单干。");
    let kept = page
        .blocks
        .iter()
        .find(|b| b.heading.as_deref() == Some("Decision"))
        .unwrap();
    assert_eq!(kept.text, "子会话只拿契约（我补充：omp 不支持）。");
    let issue = wikis.issues("p").unwrap().remove(0);
    assert_eq!(issue.kind, WikiIssueKind::SourceChanged);
    assert_eq!(issue.suggestion.as_deref(), Some("子会话拿契约与预算。"));
    assert_eq!(compile(4).issues, 0, "the same change opens one issue only");
    // Accepting the suggestion hands the section back to the compiler.
    wikis.issue_action("p", &issue.id, true, 5).unwrap();
    let page = wikis.page("p", "decision-delegation").unwrap();
    let back = page
        .blocks
        .iter()
        .find(|b| b.heading.as_deref() == Some("Decision"))
        .unwrap();
    assert_eq!(
        (back.text.as_str(), back.user_edited),
        ("子会话拿契约与预算。", false)
    );
    assert!(!wikis.issues("p").unwrap()[0].open);
    // Revisions: compile, edit, compile, issue; reverting to the first adds one.
    let revisions = wikis.revisions("p", "decision-delegation").unwrap();
    assert_eq!(revisions.len(), 4);
    let reverted = wikis.revert("p", "decision-delegation", 1, 6).unwrap();
    assert_eq!(reverted.revision, 5);
    assert_eq!(reverted.blocks[0].text, "主会话只能自己干活。");
    assert!(reverted.blocks[0].user_edited);
}

#[test]
fn a_removed_section_stays_removed_and_a_vanished_note_takes_only_untouched_pages() {
    let dir = project();
    let wikis = fresh();
    let sources = || Sources {
        notes: notes::scan(&dir),
        ..Sources::default()
    };
    wikis.compile("p", &sources(), 1).unwrap();
    let page = wikis.page("p", "decision-delegation").unwrap();
    let without = render(&page).replace("## Consequences\n\n没有 worktree。\n", "");
    wikis.edit("p", "decision-delegation", &without, 2).unwrap();
    wikis.compile("p", &sources(), 3).unwrap();
    let page = wikis.page("p", "decision-delegation").unwrap();
    assert!(
        !render(&page).contains("Consequences"),
        "the user removed it"
    );
    std::fs::remove_file(dir.join(".agents/notes/implemented/feature/2026-10-31-delegation.md"))
        .unwrap();
    let report = wikis.compile("p", &sources(), 4).unwrap();
    assert_eq!(
        report.removed, 1,
        "only removed sections left: no words of the user's to keep"
    );
    // An untouched page goes with its note.
    let dir2 = project();
    let wikis2 = fresh();
    wikis2
        .compile(
            "q",
            &Sources {
                notes: notes::scan(&dir2),
                ..Sources::default()
            },
            1,
        )
        .unwrap();
    std::fs::remove_file(dir2.join(".agents/notes/implemented/feature/2026-10-31-delegation.md"))
        .unwrap();
    let gone = wikis2.compile("q", &Sources::default(), 2).unwrap();
    assert_eq!(gone.removed, 1);
    assert_eq!(
        wikis2.page("q", "decision-delegation"),
        Err(WikiError::UnknownPage)
    );
}

#[test]
fn export_writes_only_pages_inside_the_project_and_flags_are_kept() {
    let dir = project();
    let wikis = fresh();
    wikis
        .compile(
            "p",
            &Sources {
                notes: notes::scan(&dir),
                ..Sources::default()
            },
            1,
        )
        .unwrap();
    assert_eq!(
        wikis.export_plan("p", &dir, "docs/wiki").unwrap().files,
        ["docs/wiki/index.md", "docs/wiki/decision-delegation.md"]
    );
    for bad in ["../x", "/tmp/x", ".git/wiki", "", "a/../b"] {
        assert_eq!(
            wikis.export_plan("p", &dir, bad),
            Err(WikiError::BadExportDir),
            "{bad}"
        );
    }
    let written = wikis.export("p", &dir, "docs/wiki").unwrap();
    assert_eq!(written.len(), 2);
    assert!(std::fs::read_to_string(dir.join("docs/wiki/index.md"))
        .unwrap()
        .starts_with("# 索引"));
    let flagged = wikis
        .flag("p", "index", "索引漏了 runbook", "task-1", 2)
        .unwrap();
    assert_eq!(flagged.kind, WikiIssueKind::Agent);
    assert_eq!(
        wikis.flag("p", "nope", "x", "task-1", 2),
        Err(WikiError::UnknownPage)
    );
    assert!(wikis.has_pages("p"));
    assert!(!wikis.has_pages("empty"));
    assert_eq!(
        sections("# t\n\n前言\n\n## A\n\nx\n"),
        [(None, "前言".into()), (Some("A".into()), "x".into())]
    );
    let before = notes::fingerprint(&dir);
    std::fs::write(
        dir.join(".agents/notes/implemented/feature/2026-10-31-more.md"),
        "# m\n",
    )
    .unwrap();
    assert_ne!(notes::fingerprint(&dir), before);
}

fn note_file(dir: &std::path::Path) -> std::path::PathBuf {
    let mut files = Vec::new();
    let root = dir.join(".agents/notes");
    let mut stack = vec![root];
    while let Some(folder) = stack.pop() {
        for entry in std::fs::read_dir(folder).unwrap().flatten() {
            if entry.path().is_dir() {
                stack.push(entry.path());
            } else {
                files.push(entry.path());
            }
        }
    }
    files.into_iter().next().unwrap()
}

/// R1-1: a source that cannot be read keeps its page; only a confirmed
/// removal takes it away.
#[test]
fn an_unreadable_source_keeps_its_page_and_a_confirmed_removal_does_not() {
    let dir = project();
    let wikis = fresh();
    let sources = |notes| Sources {
        notes,
        experiences: vec![],
        skills: vec![],
        code: None,
    };
    wikis.compile("p", &sources(notes::scan(&dir)), 1).unwrap();
    let slug = "decision-delegation";
    assert!(wikis.page("p", slug).is_ok());

    let gaps = aeroric_protocol::wiki::WikiSourceGaps {
        all: true,
        slugs: vec![],
    };
    let report = wikis
        .compile_checked("p", &sources(vec![]), &gaps, 2)
        .unwrap();
    assert_eq!(report.removed, 0);
    assert!(wikis.page("p", slug).is_ok(), "the page stays");
    assert_eq!(wikis.source_gaps("p").unwrap(), gaps);
    assert!(
        render(&wikis.page("p", "index").unwrap()).contains(slug),
        "the index still lists it"
    );

    let one = aeroric_protocol::wiki::WikiSourceGaps {
        all: false,
        slugs: vec![slug.into()],
    };
    wikis
        .compile_checked("p", &sources(vec![]), &one, 3)
        .unwrap();
    assert!(wikis.page("p", slug).is_ok());

    let report = wikis.compile("p", &sources(vec![]), 4).unwrap();
    assert_eq!(report.removed, 1, "readable and gone: removed");
    assert_eq!(wikis.source_gaps("p").unwrap(), Default::default());
}

/// R1-1: what scanning can and cannot tell.
#[test]
fn scanning_separates_absent_from_unreadable_and_skips_links() {
    let dir = project();
    let scan = notes::scan_checked(&dir);
    assert!(scan.available);
    assert_eq!(scan.notes.len(), 1);

    let huge = dir.join(".agents/notes/huge.md");
    std::fs::write(&huge, vec![b'x'; notes::MAX_NOTE_BYTES as usize + 1]).unwrap();
    let scan = notes::scan_checked(&dir);
    assert_eq!(scan.unreadable, [".agents/notes/huge.md"]);
    std::fs::remove_file(&huge).unwrap();

    #[cfg(unix)]
    {
        let outside = project();
        std::os::unix::fs::symlink(outside.join(".agents/notes"), dir.join(".agents/notes/out"))
            .unwrap();
        std::os::unix::fs::symlink(dir.join(".agents/notes"), dir.join(".agents/notes/loop"))
            .unwrap();
        let scan = notes::scan_checked(&dir);
        assert_eq!(scan.notes.len(), 1, "links are not followed");
        assert!(scan.available);
    }

    let empty = std::env::temp_dir().join(format!("aeroric-no-notes-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&empty).unwrap();
    assert!(
        notes::scan_checked(&empty).available,
        "no notes folder is a fact"
    );
    std::fs::remove_dir_all(&empty).unwrap();
    assert!(
        !notes::scan_checked(&empty).available,
        "no project proves nothing"
    );
}

/// R1-1: same size, same time, different words: still a change.
#[test]
fn the_fingerprint_follows_contents() {
    let dir = project();
    let file = note_file(&dir);
    let before = notes::fingerprint(&dir);
    let meta = std::fs::metadata(&file).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    let swapped = text.replacen("委派", "派委", 1);
    assert_eq!(swapped.len(), text.len());
    std::fs::write(&file, swapped).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(meta.modified().unwrap())
        .unwrap();
    assert_ne!(notes::fingerprint(&dir), before);
}

/// R1-1: changing only a note's status updates its page.
#[test]
fn a_status_only_change_updates_the_page() {
    let dir = project();
    let wikis = fresh();
    let compile = |wikis: &Wikis, at| {
        wikis
            .compile(
                "p",
                &Sources {
                    notes: notes::scan(&dir),
                    experiences: vec![],
                    skills: vec![],
                    code: None,
                },
                at,
            )
            .unwrap()
    };
    compile(&wikis, 1);
    let file = note_file(&dir);
    let text = std::fs::read_to_string(&file).unwrap();
    std::fs::write(
        &file,
        text.replace("Status: implemented", "Status: rejected"),
    )
    .unwrap();
    let report = compile(&wikis, 2);
    assert!(report.updated >= 1);
    assert_eq!(
        wikis
            .page("p", "decision-delegation")
            .unwrap()
            .status
            .as_deref(),
        Some("rejected")
    );
}

/// R1-1: a link on the way cannot carry an export out of the project.
#[test]
fn exports_refuse_links_and_report_overwrites() {
    let dir = project();
    let wikis = fresh();
    wikis
        .compile(
            "p",
            &Sources {
                notes: notes::scan(&dir),
                experiences: vec![],
                skills: vec![],
                code: None,
            },
            1,
        )
        .unwrap();
    let plan = wikis.export_plan("p", &dir, "docs/wiki").unwrap();
    assert!(plan.overwrites.is_empty());
    wikis.export("p", &dir, "docs/wiki").unwrap();
    let plan = wikis.export_plan("p", &dir, "docs/wiki").unwrap();
    assert_eq!(plan.overwrites, plan.files);

    assert_eq!(
        wikis.export_plan("p", &dir, "docs/.GIT/x"),
        Err(WikiError::BadExportDir)
    );
    #[cfg(unix)]
    {
        let outside = project();
        std::os::unix::fs::symlink(&outside, dir.join("escape")).unwrap();
        assert_eq!(
            wikis.export("p", &dir, "escape/wiki"),
            Err(WikiError::BadExportDir)
        );
        assert!(!outside.join("wiki").exists());

        std::fs::create_dir_all(dir.join("pinned")).unwrap();
        let victim = outside.join("victim.md");
        std::fs::write(&victim, "mine").unwrap();
        std::os::unix::fs::symlink(&victim, dir.join("pinned/index.md")).unwrap();
        assert_eq!(
            wikis.export("p", &dir, "pinned"),
            Err(WikiError::BadExportDir)
        );
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "mine");
    }
}

/// 2026-11-02: the title can be renamed; compiling keeps it and a changed
/// source title opens an issue whose suggestion restores the source's.
#[test]
fn a_renamed_title_is_kept_and_a_source_rename_is_an_issue() {
    let dir = project();
    let wikis = fresh();
    let compile = |wikis: &Wikis, at| {
        wikis
            .compile(
                "p",
                &Sources {
                    notes: notes::scan(&dir),
                    experiences: vec![],
                    skills: vec![],
                    code: None,
                },
                at,
            )
            .unwrap()
    };
    compile(&wikis, 1);
    let slug = "decision-delegation";
    let page = wikis.page("p", slug).unwrap();
    let markdown = render(&page).replacen(&format!("# {}", page.title), "# 我的标题", 1);
    let edited = wikis.edit("p", slug, &markdown, 2).unwrap();
    assert_eq!(edited.title, "我的标题");
    assert!(edited.title_edited);

    compile(&wikis, 3);
    assert_eq!(wikis.page("p", slug).unwrap().title, "我的标题");
    assert!(
        wikis.issues("p").unwrap().is_empty(),
        "nothing changed upstream"
    );

    let file = note_file(&dir);
    let text = std::fs::read_to_string(&file).unwrap();
    std::fs::write(
        &file,
        text.replacen("# Agent Note: ", "# Agent Note: 新", 1),
    )
    .unwrap();
    compile(&wikis, 4);
    assert_eq!(wikis.page("p", slug).unwrap().title, "我的标题");
    let issue = wikis
        .issues("p")
        .unwrap()
        .into_iter()
        .find(|i| i.block_key.as_deref() == Some("title"))
        .expect("a title issue");
    wikis.issue_action("p", &issue.id, true, 5).unwrap();
    let page = wikis.page("p", slug).unwrap();
    assert!(page.title.starts_with('新'));
    assert!(!page.title_edited);
}
