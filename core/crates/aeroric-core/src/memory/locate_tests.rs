use super::*;

fn talk() -> Vec<(String, String)> {
    vec![
        ("user".into(), "帮我修 SSH 超时".into()),
        ("assistant".into(), "好的。以后回复都用简体中文？".into()),
        (
            "user".into(),
            "对，以后回复  都用 简体中文。另外 Release 前先跑 CHECK".into(),
        ),
    ]
}

fn proof(words: &str, messages: &[(String, String)]) -> Evidence {
    let mut evidence = super::super::evidence(Some("t1"), None, None, words, 1);
    evidence.location = locate(messages, words);
    evidence
}

/// The words are found the way extraction checks a quote — spaces, width
/// and case aside — in the user's message before the agent's, and the span
/// covers the message's own chars.
#[test]
fn the_words_are_found_in_the_users_message_with_their_span() {
    let talk = talk();
    let place = locate(&talk, "以后回复都用简体中文。").unwrap();
    assert_eq!((place.message_index, place.role.as_str()), (2, "user"));
    let text: Vec<char> = talk[2].1.chars().collect();
    let said: String = text[place.start as usize..place.end as usize]
        .iter()
        .collect();
    assert_eq!(said, "以后回复  都用 简体中文");
    let place = locate(&talk, "release 前先跑 check").unwrap();
    let said: String = text[place.start as usize..place.end as usize]
        .iter()
        .collect();
    assert_eq!(said, "Release 前先跑 CHECK");
    let full_width = locate(&talk, "ｒｅｌｅａｓｅ前").unwrap();
    assert_eq!(full_width.message_index, 2);
    assert_eq!(locate(&talk, "好的").unwrap().role, "assistant");
    assert_eq!(locate(&talk, "没说过的话"), None);
    assert_eq!(
        locate(&talk, "超时。帮我"),
        None,
        "across messages: no place"
    );
    assert_eq!(locate(&talk, "x"), None, "too short to place");
}

/// A lookup tells the five outcomes apart.
#[test]
fn a_lookup_says_what_became_of_the_source() {
    let talk = talk();
    let evidence = proof("Release 前先跑 CHECK", &talk);

    let found = lookup(&evidence, Some(&talk));
    assert_eq!(found.status, EvidenceStatus::Located);
    let message = found.message.unwrap();
    let said: String = message
        .text
        .chars()
        .skip(message.start as usize)
        .take((message.end - message.start) as usize)
        .collect();
    assert_eq!(said, "Release 前先跑 CHECK");
    assert!(!message.cut_before && !message.cut_after);

    // The agent compacted the history: the message moved, the words kept.
    let compacted = vec![
        ("user".into(), "（之前的对话已压缩）".into()),
        ("user".into(), "Release 前先跑 CHECK，别忘了".into()),
    ];
    let moved = lookup(&evidence, Some(&compacted));
    assert_eq!(moved.status, EvidenceStatus::Moved);
    assert_eq!(moved.message.unwrap().message_index, 1);

    // Rewritten without the words.
    let rewritten = vec![("user".into(), "（之前的对话已压缩）".into())];
    let changed = lookup(&evidence, Some(&rewritten));
    assert_eq!(changed.status, EvidenceStatus::Changed);
    assert_eq!(changed.message, None);
    assert_eq!(
        changed.excerpt, "Release 前先跑 CHECK",
        "the excerpt still shows"
    );

    assert_eq!(lookup(&evidence, None).status, EvidenceStatus::Gone);
    assert_eq!(lookup(&evidence, Some(&[])).status, EvidenceStatus::Gone);

    let mut old = evidence.clone();
    old.location = None;
    assert_eq!(lookup(&old, Some(&talk)).status, EvidenceStatus::Unrecorded);
}

/// A long message comes back as a window around the words, offsets moved
/// with it.
#[test]
fn a_long_message_is_a_window_around_the_words() {
    let long = format!("{}关键的一句话{}", "前".repeat(9_000), "后".repeat(9_000));
    let talk = vec![("user".to_string(), long)];
    let evidence = proof("关键的一句话", &talk);
    let message = lookup(&evidence, Some(&talk)).message.unwrap();
    assert_eq!(message.text.chars().count(), WINDOW_CHARS);
    assert!(message.cut_before && message.cut_after);
    let said: String = message
        .text
        .chars()
        .skip(message.start as usize)
        .take((message.end - message.start) as usize)
        .collect();
    assert_eq!(said, "关键的一句话");
    assert_eq!(message.start, 1_500);
}
