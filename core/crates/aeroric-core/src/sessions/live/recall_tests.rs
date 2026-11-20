use super::*;
use crate::memory::Memory;
use aeroric_protocol::memory::{MemoryDraft, MemoryKind};
use aeroric_protocol::wiki::WikiPageKind;

fn page(slug: &str, title: &str) -> WikiPageSummary {
    WikiPageSummary {
        slug: slug.into(),
        kind: WikiPageKind::Module,
        title: title.into(),
        status: None,
        category: None,
        revision: 1,
        edited: 0,
    }
}

#[test]
fn keywords_are_identifiers_and_cjk_pairs_without_common_words() {
    assert_eq!(
        keywords("帮我看看 dbx-ref.txt 里的版本"),
        ["我看", "dbx-ref.txt", "里的", "的版", "版本"]
    );
    assert_eq!(keywords("ok 好"), Vec::<String>::new());
    assert_eq!(query("Cargo test 跑不过"), "cargo test 跑不 不过");
}

/// A memory needs two shared keywords (a long identifier counts as two);
/// most shared first, at most three; a page is named by its title.
#[test]
fn hits_need_enough_shared_keywords() {
    let memory = Memory::default();
    let dir = std::env::temp_dir().join(format!("aeroric-recall-{}", uuid::Uuid::new_v4()));
    memory.set_dir(dir.clone()).unwrap();
    let said = crate::memory::evidence(Some("t"), Some("claude"), None, "x", 1);
    let add = |text: &str| {
        memory
            .remember(
                MemoryDraft {
                    project_id: Some("p".into()),
                    kind: MemoryKind::Fact,
                    category: None,
                    text: text.into(),
                    supersedes: None,
                },
                said.clone(),
                1,
            )
            .unwrap()
    };
    let pinned = add("core 依赖仓库外的 dbx，版本钉在 dbx-ref.txt");
    let weak = add("发布前要跑一遍版本检查");
    let message = "dbx 的版本钉在哪里？";
    let found: Vec<_> = memory
        .matches("p", &query(message), 30)
        .unwrap()
        .into_iter()
        .map(|(_, i)| i)
        .collect();
    let got = hits(message, &found, &[]);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].id, pinned.id);
    assert!(
        !got.iter().any(|h| h.id == weak.id),
        "one shared word is not enough"
    );
    // A long identifier alone is enough.
    let alone = hits("dbx-ref.txt 在哪", &found, &[]);
    assert_eq!(
        alone.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(),
        [pinned.id.as_str()]
    );

    let pages = [
        page("index", "首页"),
        page("module-core-b", "b-cli"),
        page("module-x", "x"),
    ];
    let named = hits("b-cli 的入口在哪", &[], &pages);
    assert_eq!(named.len(), 1);
    assert_eq!(
        (named[0].kind.as_str(), named[0].id.as_str()),
        ("wiki", "module-core-b")
    );
    assert!(block(&named).contains("Wiki 页「b-cli」（module-core-b）：要细节用 read_wiki 读全文"));
    assert!(block(&got).contains(&format!("- 记忆 {}：core 依赖仓库外的 dbx", pinned.id)));
    assert!(hits("没有关系的话题，完全不相干", &found, &pages).is_empty());
    assert_eq!(skip("继续"), Some("消息太短"));
    assert_eq!(skip("dbx 的版本钉在哪里？"), None);
    std::fs::remove_dir_all(&dir).ok();
}

/// The mode defaults to candidate and is kept; decisions are appended.
#[test]
fn the_mode_and_decisions_are_kept() {
    let log = RecallLog::default();
    let data = std::env::temp_dir().join(format!("aeroric-recall-log-{}", uuid::Uuid::new_v4()));
    log.set_dir(data.clone());
    assert_eq!(log.mode(), RecallMode::Candidate);
    log.set_mode(RecallMode::On).unwrap();
    assert_eq!(log.mode(), RecallMode::On);
    let decision = RecallDecision {
        at_ms: 1,
        mode: RecallMode::On,
        hits: vec![],
        attached: false,
        skipped: Some("消息太短".into()),
    };
    log.note("task/1", &decision);
    log.note("task/1", &decision);
    assert_eq!(log.decisions("task/1"), [decision.clone(), decision]);
    assert!(log.decisions("other").is_empty());
    std::fs::remove_dir_all(&data).ok();
}
