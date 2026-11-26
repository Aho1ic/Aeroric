use super::*;
use crate::sessions::live::review::fact_check_eval;
use aeroric_protocol::review::Severity;

const DIFF: &str = "diff --git a/src/client.rs b/src/client.rs
index 1..2 100644
--- a/src/client.rs
+++ b/src/client.rs
@@ -1,5 +1,8 @@
 fn send() {
-    loop { post(); }
+    if attempt >= MAX_RETRIES {
+        return;
+    }
+    post();
 }
diff --git a/src/other.rs b/src/other.rs
--- a/src/other.rs
+++ b/src/other.rs
@@ -1 +1 @@
+let limit = clamp(value, 0, 10);";

fn finding(id: &str, text: &str, snippet: Option<&str>, unlocated: bool) -> Finding {
    Finding {
        id: id.into(),
        severity: Severity::Blocker,
        text: text.into(),
        file: Some("src/client.rs".into()),
        snippet: snippet.map(str::to_string),
        category: Some("bug".into()),
        unlocated,
        ..Finding::default()
    }
}

fn judge(id: &str, verdict: Verdict, line: Option<&str>) -> Judgement {
    Judgement {
        id: id.into(),
        verdict,
        topic: "other".into(),
        line: line.map(str::to_string),
    }
}

#[test]
fn the_answer_is_read_from_fenced_or_bare_json_and_unknown_verdicts_keep() {
    let reply = "好的：\n```json\n{\"results\":[{\"id\":\"r1\",\"verdict\":\"refuted\",\"topic\":\"Other\",\"line\":\"x\"},{\"id\":2,\"verdict\":\"drop\"},{\"verdict\":\"refuted\"}]}\n```";
    let judgements = parse(reply).unwrap();
    assert_eq!(
        judgements,
        [
            Judgement {
                id: "r1".into(),
                verdict: Verdict::Refuted,
                topic: "other".into(),
                line: Some("x".into())
            },
            Judgement {
                id: "r2".into(),
                verdict: Verdict::Keep,
                topic: String::new(),
                line: None
            },
        ]
    );
    assert!(parse("没有 JSON").is_none());
}

#[test]
fn a_refutation_must_quote_a_line_the_diff_has_on_its_after_side() {
    let cases = [
        // Quoted word for word: out, with the line as evidence.
        (Some("if attempt >= MAX_RETRIES {"), true),
        // Re-spaced and with its `+`: still the same line.
        (Some("+ if   attempt >= MAX_RETRIES {"), true),
        // Part of a line, long enough.
        (Some("attempt >= MAX_RETRIES"), true),
        // Invented.
        (Some("if retries > 3 { break; }"), false),
        // Only on the removed side.
        (Some("loop { post(); }"), false),
        // Too short to mean anything.
        (Some("}"), false),
        (Some("post"), false),
        // In another file of the diff.
        (Some("let limit = clamp(value, 0, 10);"), false),
        (None, false),
    ];
    for (line, out) in cases {
        let outcome = apply(
            vec![finding("r1", "重试没有上限", Some("post();"), false)],
            &[judge("r1", Verdict::Refuted, line)],
            DIFF,
        );
        assert_eq!(outcome.filtered.len() == 1, out, "{line:?}");
        assert_eq!(outcome.guarded, u32::from(!out), "{line:?}");
        if out {
            assert_eq!(outcome.filtered[0].ground, FilterGround::Refuted);
            assert_eq!(outcome.filtered[0].evidence, "if attempt >= MAX_RETRIES {");
        }
    }
}

#[test]
fn not_in_diff_needs_a_quote_the_file_and_the_diff_both_lack() {
    let outcome = |snippet: Option<&str>, unlocated: bool| {
        apply(
            vec![finding("r1", "错误被吞掉了", snippet, unlocated)],
            &[judge("r1", Verdict::NotInDiff, None)],
            DIFF,
        )
    };
    let gone = outcome(Some("fn legacy_parse(input: &str) -> Item {"), true);
    assert_eq!(gone.filtered.len(), 1);
    assert_eq!(gone.filtered[0].ground, FilterGround::NotInDiff);
    // Found in the file after the turn: E2 placed it.
    assert_eq!(
        outcome(Some("fn legacy_parse(input: &str) -> Item {"), false)
            .filtered
            .len(),
        0
    );
    // On a removed line of the diff.
    assert_eq!(outcome(Some("loop { post(); }"), true).filtered.len(), 0);
    // No quote: nothing to check.
    assert_eq!(outcome(None, true).filtered.len(), 0);
}

#[test]
fn protected_findings_stay_whatever_the_model_says() {
    let refute = |f: Finding, topic: &str| {
        let mut j = judge("r1", Verdict::Refuted, Some("if attempt >= MAX_RETRIES {"));
        j.topic = topic.into();
        apply(vec![f], &[j], DIFF)
    };
    let plain = || finding("r1", "重试没有上限", Some("post();"), false);
    assert_eq!(refute(plain(), "other").filtered.len(), 1);
    // The model's own topic.
    for topic in [
        "concurrency",
        "memory_safety",
        "declaration",
        "compatibility",
        "unused_parameter",
        "",
    ] {
        let outcome = refute(plain(), topic);
        assert!(outcome.filtered.is_empty(), "{topic}");
        assert_eq!(outcome.guarded, 1);
    }
    // The finding's words.
    for text in [
        "这里有竞态",
        "counter 没有加锁",
        "unlocked access to the map",
        "a lock is held across await",
        "数组越界",
        "签名改了，调用方没跟",
        "breaking change for callers",
        "新加的参数 retries 没有用到",
    ] {
        let mut f = plain();
        f.text = text.into();
        assert!(refute(f, "other").filtered.is_empty(), "{text}");
    }
    // `block` and `clock` are not `lock`.
    let mut f = plain();
    f.text = "this blocks the clock tick".into();
    assert_eq!(refute(f, "other").filtered.len(), 1);
    // Security, and a finding the last round raised too.
    let mut f = plain();
    f.category = Some("security".into());
    assert!(refute(f, "other").filtered.is_empty());
    let mut f = plain();
    f.repeats = Some("r1".into());
    assert!(refute(f, "other").filtered.is_empty());
}

#[test]
fn findings_the_model_did_not_mention_or_kept_stay() {
    let outcome = apply(
        vec![
            finding("r1", "重试没有上限", Some("post();"), false),
            finding("r2", "命名", None, false),
        ],
        &[judge(
            "r1",
            Verdict::Keep,
            Some("if attempt >= MAX_RETRIES {"),
        )],
        DIFF,
    );
    assert_eq!(outcome.kept.len(), 2);
    assert_eq!(outcome.guarded, 0);
}

#[test]
fn the_prompt_carries_each_finding_and_the_diff() {
    let text = prompt(
        &[finding("r1", "重试没有上限", Some("post();"), false)],
        DIFF,
    );
    assert!(text.contains("- id: r1"));
    assert!(text.contains("位置: src/client.rs"));
    assert!(text.contains("    post();"));
    assert!(text.contains("+    if attempt >= MAX_RETRIES {"));
}

/// The cases' own answers pass the guards: what should go can go, what
/// should stay stays. A model that wants everything out with invented
/// lines takes nothing out; one that quotes a real line for everything can
/// still take out only the unprotected ones.
#[test]
fn the_evaluation_cases_and_the_guards_agree() {
    let right_line = |name: &str| match name {
        "refuted-retry-cap" => Some("if attempt >= MAX_RETRIES {"),
        "refuted-unwrap" => Some("let Some(user) = user else {"),
        _ => None,
    };
    let oracle = fact_check_eval::run(|_, user| {
        let case = fact_check_eval::cases()
            .into_iter()
            .find(|c| user.contains(&c.finding.text))
            .unwrap();
        let (verdict, line) = match (case.remove, right_line(case.name)) {
            (true, Some(line)) => ("refuted", line),
            (true, None) => ("not_in_diff", ""),
            (false, _) => ("keep", ""),
        };
        Ok(fact_check_eval::Answer {
            text: format!("{{\"results\":[{{\"id\":\"r1\",\"verdict\":\"{verdict}\",\"topic\":\"other\",\"line\":\"{line}\"}}]}}"),
            input_tokens: 1,
            output_tokens: 1,
        })
    });
    assert_eq!(
        (oracle.wrongly_removed, oracle.missed, oracle.failed),
        (0, 0, 0),
        "{oracle:#?}"
    );

    let inventor = fact_check_eval::run(|_, _| {
        Ok(fact_check_eval::Answer {
            text: "{\"results\":[{\"id\":\"r1\",\"verdict\":\"refuted\",\"topic\":\"other\",\"line\":\"this line is nowhere\"}]}".into(),
            input_tokens: 0,
            output_tokens: 0,
        })
    });
    assert!(inventor.cases.iter().all(|c| !c.removed), "{inventor:#?}");

    let greedy = fact_check_eval::run(|_, user| {
        // The first added line of the diff, quoted for every finding.
        let line = user
            .lines()
            .find_map(|l| l.strip_prefix('+').filter(|l| !l.starts_with("++")))
            .unwrap_or_default()
            .trim()
            .replace('"', "\\\"");
        Ok(fact_check_eval::Answer {
            text: format!("{{\"results\":[{{\"id\":\"r1\",\"verdict\":\"refuted\",\"topic\":\"other\",\"line\":\"{line}\"}}]}}"),
            input_tokens: 0,
            output_tokens: 0,
        })
    });
    for case in &greedy.cases {
        if case.name.starts_with("protected-") {
            assert!(!case.removed, "{}", case.name);
        }
    }
}
