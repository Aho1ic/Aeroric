//! E2 (2026-11-26): a fixed set of findings against small diffs, each with
//! the right outcome, to measure the fact check before it is ever on by
//! default. `review.factCheckEval` runs it with the analysis model; what
//! matters most is `wrongly_removed` — a right finding taken out.

use serde::Serialize;

use aeroric_protocol::review::{FilterGround, Finding, Severity};

use super::fact_check;

pub struct Case {
    pub name: &'static str,
    /// What the case tests, for the report.
    pub about: &'static str,
    pub diff: &'static str,
    pub finding: Finding,
    /// The right outcome: taken out (`true`) or kept.
    pub remove: bool,
}

fn finding(text: &str, file: &str, snippet: Option<&str>, unlocated: bool) -> Finding {
    Finding {
        id: "r1".into(),
        severity: Severity::Blocker,
        text: text.into(),
        file: Some(file.into()),
        snippet: snippet.map(str::to_string),
        category: Some("bug".into()),
        unlocated,
        ..Finding::default()
    }
}

const RETRY_DIFF: &str = "diff --git a/src/client.rs b/src/client.rs
--- a/src/client.rs
+++ b/src/client.rs
@@ -10,6 +10,10 @@ impl Client {
+const MAX_RETRIES: u32 = 3;
+
     pub fn send(&self, body: &str) -> Result<(), Error> {
         let mut attempt = 0;
         loop {
+            if attempt >= MAX_RETRIES {
+                return Err(Error::GaveUp);
+            }
             match self.post(body) {
                 Ok(_) => return Ok(()),
                 Err(_) => attempt += 1,";

const UNBOUNDED_DIFF: &str = "diff --git a/src/client.rs b/src/client.rs
--- a/src/client.rs
+++ b/src/client.rs
@@ -10,6 +10,9 @@ impl Client {
     pub fn send(&self, body: &str) -> Result<(), Error> {
+        loop {
+            if self.post(body).is_ok() {
+                return Ok(());
+            }
+        }
     }";

const EMPTY_CHECK_DIFF: &str = "diff --git a/src/user.rs b/src/user.rs
--- a/src/user.rs
+++ b/src/user.rs
@@ -3,5 +3,8 @@
 pub fn display_name(user: Option<&User>) -> String {
+    let Some(user) = user else {
+        return String::from(\"guest\");
+    };
-    user.unwrap().name.clone()
+    user.name.clone()
 }";

const COUNTER_DIFF: &str = "diff --git a/src/stats.rs b/src/stats.rs
--- a/src/stats.rs
+++ b/src/stats.rs
@@ -8,6 +8,8 @@ impl Stats {
     pub fn record(&self) {
+        let _guard = self.lock.lock();
+        self.total.set(self.total.get() + 1);
     }
+    pub fn reset(&self) {
+        self.total.set(0);
+    }";

const TIMEOUT_DIFF: &str = "diff --git a/src/net.rs b/src/net.rs
--- a/src/net.rs
+++ b/src/net.rs
@@ -1,4 +1,4 @@
-pub fn connect(addr: &str, timeout_secs: u64) -> Conn {
-    Conn::open(addr, Duration::from_secs(timeout_secs))
+pub fn connect(addr: &str, timeout_ms: u64) -> Conn {
+    Conn::open(addr, Duration::from_millis(timeout_ms))
 }";

const PARSE_DIFF: &str = "diff --git a/src/parse.rs b/src/parse.rs
--- a/src/parse.rs
+++ b/src/parse.rs
@@ -1,4 +1,7 @@
 pub fn parse(input: &str) -> Option<Item> {
+    let input = input.trim();
+    let (key, value) = input.split_once('=')?;
+    Some(Item::new(key, value))
 }";

const SORT_DIFF: &str = "diff --git a/src/report.rs b/src/report.rs
--- a/src/report.rs
+++ b/src/report.rs
@@ -4,6 +4,8 @@
 pub fn summary(mut rows: Vec<Row>) -> String {
+    rows.sort_by_key(|r| r.score);
     let mut out = String::new();
     for row in rows.iter() {
         out.push_str(&row.name);
     }";

const RETRIES_PARAM_DIFF: &str = "diff --git a/src/fetch.rs b/src/fetch.rs
--- a/src/fetch.rs
+++ b/src/fetch.rs
@@ -1,3 +1,6 @@
-pub fn fetch(url: &str) -> Body {
+pub fn fetch(url: &str, retries: u32) -> Body {
+    for _ in 0..retries {
+        if let Ok(body) = get(url) { return body; }
+    }
     get(url).unwrap_or_default()
 }";

const RENAME_DIFF: &str = "diff --git a/src/config.rs b/src/config.rs
--- a/src/config.rs
+++ b/src/config.rs
@@ -1,3 +1,3 @@
-pub fn load(path: &Path) -> Config {
+pub fn load(path: &Path) -> Result<Config, Error> {
     let text = std::fs::read_to_string(path)?;";

pub fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "refuted-retry-cap",
            about: "说没有重试上限，diff 里就有 MAX_RETRIES 和判断：应删",
            diff: RETRY_DIFF,
            finding: finding(
                "重试没有上限，post 一直失败时会死循环",
                "src/client.rs",
                Some("Err(_) => attempt += 1,"),
                false,
            ),
            remove: true,
        },
        Case {
            name: "refuted-unwrap",
            about: "说还在 unwrap 可能崩溃，diff 已改成 let-else：应删",
            diff: EMPTY_CHECK_DIFF,
            finding: finding(
                "display_name 里 user.unwrap() 在 user 为 None 时会崩溃",
                "src/user.rs",
                Some("user.unwrap().name.clone()"),
                true,
            ),
            remove: true,
        },
        Case {
            name: "not-in-diff",
            about: "引用的函数这次根本没改、也不在文件里：应删",
            diff: PARSE_DIFF,
            finding: finding(
                "legacy_parse 吞掉了错误，调用方拿不到失败原因",
                "src/parse.rs",
                Some("fn legacy_parse(input: &str) -> Item {"),
                true,
            ),
            remove: true,
        },
        Case {
            name: "real-unbounded-loop",
            about: "真问题：循环确实没有上限：应留",
            diff: UNBOUNDED_DIFF,
            finding: finding(
                "send 在 post 一直失败时会无限重试，没有次数上限",
                "src/client.rs",
                Some("loop {"),
                false,
            ),
            remove: false,
        },
        Case {
            name: "protected-concurrency",
            about: "并发主题：reset 没有拿锁，即使 record 拿了锁也不能删：应留",
            diff: COUNTER_DIFF,
            finding: finding(
                "reset 改 total 时没有加锁，和 record 并发时会有竞态",
                "src/stats.rs",
                Some("self.total.set(0);"),
                false,
            ),
            remove: false,
        },
        Case {
            name: "protected-compatibility",
            about: "兼容性主题：参数单位从秒改成毫秒，调用方的行为变了：应留",
            diff: TIMEOUT_DIFF,
            finding: finding(
                "connect 的超时参数从秒改成了毫秒，现有调用方传的 30 会变成 30 毫秒，行为变了",
                "src/net.rs",
                Some("pub fn connect(addr: &str, timeout_ms: u64) -> Conn {"),
                false,
            ),
            remove: false,
        },
        Case {
            name: "invented-evidence",
            about: "diff 里没有能驳倒它的行（没有判空字符串），模型如果编一行也不能删：应留",
            diff: PARSE_DIFF,
            finding: finding(
                "parse 对空字符串返回 None 前没有给出原因，调用方分不清是空还是格式错",
                "src/parse.rs",
                Some("let (key, value) = input.split_once('=')?;"),
                false,
            ),
            remove: false,
        },
        Case {
            name: "needs-reasoning",
            about: "要推理才能判断的意见（排序后按引用遍历，排序是有用的）：拿不准，应留",
            diff: SORT_DIFF,
            finding: finding(
                "rows 排序之后的顺序不影响输出，sort_by_key 是多余的",
                "src/report.rs",
                Some("rows.sort_by_key(|r| r.score);"),
                false,
            ),
            remove: false,
        },
        Case {
            name: "protected-unused-parameter",
            about: "未使用的参数主题：即使 diff 里用到了 retries 也不删：应留",
            diff: RETRIES_PARAM_DIFF,
            finding: finding(
                "新加的参数 retries 没有用到",
                "src/fetch.rs",
                Some("pub fn fetch(url: &str, retries: u32) -> Body {"),
                false,
            ),
            remove: false,
        },
        Case {
            name: "protected-signature",
            about: "声明一致性主题：返回类型改成 Result，调用方要跟着改：应留",
            diff: RENAME_DIFF,
            finding: finding(
                "load 的签名改成返回 Result，其他调用 load 的地方没有一起改",
                "src/config.rs",
                Some("pub fn load(path: &Path) -> Result<Config, Error> {"),
                false,
            ),
            remove: false,
        },
    ]
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaseResult {
    pub name: String,
    pub about: String,
    pub expected_removed: bool,
    pub removed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ground: Option<FilterGround>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    /// The model wanted it out; a guard kept it.
    pub guarded: bool,
    /// The call failed or its answer could not be read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub cases: Vec<CaseResult>,
    /// Right findings taken out: the number that must stay 0.
    pub wrongly_removed: u32,
    /// Wrong findings left in.
    pub missed: u32,
    pub guarded: u32,
    pub failed: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// One model answer: the text and its usage.
pub struct Answer {
    pub text: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Run every case through `call(system, user)`.
pub fn run(call: impl Fn(&str, &str) -> Result<Answer, String>) -> Report {
    let mut report = Report::default();
    for case in cases() {
        let mut result = CaseResult {
            name: case.name.into(),
            about: case.about.into(),
            expected_removed: case.remove,
            removed: false,
            ground: None,
            evidence: None,
            guarded: false,
            error: None,
        };
        let findings = vec![case.finding.clone()];
        match call(
            fact_check::SYSTEM,
            &fact_check::prompt(&findings, case.diff),
        ) {
            Err(error) => result.error = Some(error),
            Ok(answer) => {
                report.input_tokens += answer.input_tokens;
                report.output_tokens += answer.output_tokens;
                match fact_check::parse(&answer.text) {
                    None => result.error = Some("回答里没有能读的 JSON".into()),
                    Some(judgements) => {
                        let outcome = fact_check::apply(findings, &judgements, case.diff);
                        result.guarded = outcome.guarded > 0;
                        if let Some(filtered) = outcome.filtered.into_iter().next() {
                            result.removed = true;
                            result.ground = Some(filtered.ground);
                            result.evidence = Some(filtered.evidence);
                        }
                    }
                }
            }
        }
        if result.error.is_some() {
            report.failed += 1;
        } else if result.removed && !result.expected_removed {
            report.wrongly_removed += 1;
        } else if !result.removed && result.expected_removed {
            report.missed += 1;
        }
        if result.guarded {
            report.guarded += 1;
        }
        report.cases.push(result);
    }
    report
}
