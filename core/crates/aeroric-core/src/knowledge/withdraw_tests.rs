//! C2 second step (2026-11-20): a withdrawn memory's paragraphs leave the
//! Wiki at once; a forgotten one's words leave its history too.

use aeroric_protocol::memory::{
    Assertion, Confidence, ExperienceDetail, MemoryItem, MemoryKind, MemoryState, MemoryStats,
};
use aeroric_protocol::wiki::{RevisionReason, WikiIssueKind};

use super::compile::Sources;
use super::{render, Wikis, FORGOTTEN_HEADING, FORGOTTEN_ISSUE};

const GONE: &str = "部署前清空 zebra-cache 目录";
const KEPT: &str = "CSV 带 BOM：读取改用 utf-8-sig";

fn experience(id: &str, title: &str, solution: &str) -> MemoryItem {
    MemoryItem {
        id: id.into(),
        project_id: Some("p".into()),
        kind: MemoryKind::Experience,
        category: None,
        text: title.into(),
        assertion: Assertion::ModelInferred,
        state: MemoryState::Active,
        pinned: false,
        evidence: Vec::new(),
        supersedes: None,
        superseded_by: None,
        created_ms: 1,
        updated_ms: 1,
        experience: Some(ExperienceDetail {
            problem: String::new(),
            solution: solution.into(),
            search_keys: vec![],
            envs: vec![],
        }),
        confidence: Some(Confidence::High),
        stats: MemoryStats::default(),
    }
}

/// A Wiki compiled from two experiences; its folder.
fn wiki() -> (Wikis, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("aeroric-wiki-withdraw-{}", uuid::Uuid::new_v4()));
    let wikis = Wikis::default();
    wikis.set_dir(dir.clone());
    let sources = Sources {
        experiences: vec![
            experience("mem-gone", GONE, "rm -rf zebra-cache"),
            experience("mem-kept", KEPT, "encoding=\"utf-8-sig\""),
        ],
        ..Sources::default()
    };
    wikis.compile("p", &sources, 1).unwrap();
    assert!(render(&wikis.page("p", "runbook").unwrap()).contains(GONE));
    (wikis, dir)
}

fn everywhere(wikis: &Wikis, dir: &std::path::Path, words: &str) -> Vec<&'static str> {
    let mut found = Vec::new();
    let page = wikis.page("p", "runbook").unwrap();
    if render(&page).contains(words) {
        found.push("page");
    }
    if !wikis.search("p", "zebra").unwrap().is_empty() {
        found.push("search");
    }
    let file = std::fs::read_to_string(dir.join("p/pages/runbook.md")).unwrap();
    if file.contains(words) {
        found.push("markdown file");
    }
    if wikis
        .revisions("p", "runbook")
        .unwrap()
        .iter()
        .any(|r| r.markdown.contains(words))
    {
        found.push("history");
    }
    if wikis.issues("p").unwrap().iter().any(|i| {
        i.text.contains(words) || i.suggestion.as_deref().is_some_and(|s| s.contains(words))
    }) {
        found.push("issues");
    }
    found
}

/// Archived: the paragraph leaves the page, the search, the Markdown file
/// now — not at the next compile — with a revision saying why; the history
/// keeps it (archived is not forgotten). The other paragraph stays.
#[test]
fn an_archived_memorys_paragraph_leaves_the_page_now() {
    let (wikis, dir) = wiki();
    let report = wikis
        .withdraw_memory("p", &["mem-gone".to_string()], None, 2)
        .unwrap();
    assert_eq!(
        (
            report.pages,
            report.removed,
            report.flagged,
            report.scrubbed
        ),
        (1, 1, 0, 0)
    );
    assert_eq!(everywhere(&wikis, &dir, GONE), ["history"]);
    assert!(render(&wikis.page("p", "runbook").unwrap()).contains(KEPT));
    let revisions = wikis.revisions("p", "runbook").unwrap();
    assert_eq!(revisions[0].reason, RevisionReason::Withdraw);
    // Nothing left to withdraw: no new revision.
    let again = wikis
        .withdraw_memory("p", &["mem-gone".to_string()], None, 3)
        .unwrap();
    assert_eq!(again.pages, 0);
    assert_eq!(
        wikis.revisions("p", "runbook").unwrap().len(),
        revisions.len()
    );
}

/// Forgotten: the words leave the history and the issues too — nowhere in
/// the Wiki reads them back.
#[test]
fn a_forgotten_memorys_words_leave_the_history_too() {
    let (wikis, dir) = wiki();
    let forgotten = |heading: &str| heading == GONE;
    let report = wikis
        .withdraw_memory("p", &["mem-gone".to_string()], Some(&forgotten), 2)
        .unwrap();
    assert_eq!(report.removed, 1);
    assert!(report.scrubbed >= 1, "{report:?}");
    assert!(
        everywhere(&wikis, &dir, GONE).is_empty(),
        "{:?}",
        everywhere(&wikis, &dir, GONE)
    );
    assert!(
        wikis
            .revisions("p", "runbook")
            .unwrap()
            .iter()
            .all(|r| r.markdown.contains(KEPT)),
        "the rest of each revision stays"
    );
}

/// A paragraph the user changed stays theirs, with an issue to check; when
/// the memory is forgotten, its heading (the memory's words) goes and the
/// issue does not repeat them.
#[test]
fn the_users_paragraph_stays_with_an_issue() {
    let (wikis, dir) = wiki();
    let page = wikis.page("p", "runbook").unwrap();
    let markdown = render(&page).replace("rm -rf zebra-cache", "我改过：先备份再清");
    wikis.edit("p", "runbook", &markdown, 2).unwrap();

    let report = wikis
        .withdraw_memory("p", &["mem-gone".to_string()], None, 3)
        .unwrap();
    assert_eq!((report.removed, report.flagged), (0, 1));
    let page = wikis.page("p", "runbook").unwrap();
    assert!(render(&page).contains("我改过：先备份再清"));
    let issue = wikis.issues("p").unwrap().remove(0);
    assert_eq!(issue.kind, WikiIssueKind::SourceRemoved);
    assert_eq!(issue.block_key.as_deref(), Some("memory:mem-gone"));
    assert!(issue.text.contains("已归档"), "{}", issue.text);

    let forgotten = |heading: &str| heading == GONE;
    wikis
        .withdraw_memory("p", &["mem-gone".to_string()], Some(&forgotten), 4)
        .unwrap();
    let page = wikis.page("p", "runbook").unwrap();
    let text = render(&page);
    assert!(text.contains("我改过：先备份再清"), "the user's words stay");
    assert!(text.contains(FORGOTTEN_HEADING));
    assert!(wikis
        .issues("p")
        .unwrap()
        .iter()
        .filter(|i| i.block_key.as_deref() == Some("memory:mem-gone"))
        .all(|i| i.text == FORGOTTEN_ISSUE && i.suggestion.is_none()));
    assert!(
        everywhere(&wikis, &dir, GONE).is_empty(),
        "{:?}",
        everywhere(&wikis, &dir, GONE)
    );
}
