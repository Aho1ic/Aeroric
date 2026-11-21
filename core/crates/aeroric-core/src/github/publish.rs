//! D2 first step (2026-11-21): publishing a review to GitHub — the only
//! writes Aeroric makes there, and only when the user says so (no agent
//! tool reaches this).
//!
//! A review becomes a [`WriteIntent`]: line comments for findings with a
//! line, and one comment on the whole PR (总评) for the rest. Each item's
//! body ends with an invisible marker `<!-- aeroric:<intent>:<item> -->`.
//! Publishing:
//!
//! 1. reads the PR's head again — moved since the review, nothing is sent
//!    (目标已更新);
//! 2. looks on GitHub for the markers — an item already there is recorded as
//!    sent, whatever this side remembered (a lost answer, a restart);
//! 3. sends each item still to send, one by one, saving its state before
//!    and after: a request with no answer is *unknown* and is not sent
//!    again unless the user says so after the markers are looked for again.
//!
//! So the same item never becomes two comments on GitHub.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use aeroric_protocol::github::{
    PullReview, PullSnapshot, WriteIntent, WriteIntentState, WriteItem, WriteItemState,
};
use serde_json::{json, Value};

/// Comment pages read when looking for markers (100 each).
const MARKER_PAGES: usize = 20;

pub fn marker(intent_id: &str, item_id: &str) -> String {
    format!("<!-- aeroric:{intent_id}:{item_id} -->")
}

/// What is sent: the body, then the marker.
pub fn body_with_marker(intent: &WriteIntent, item: &WriteItem) -> String {
    format!(
        "{}\n\n{}",
        item.body.trim_end(),
        marker(&intent.id, &item.id)
    )
}

fn severity(s: &str) -> &'static str {
    match s {
        "high" => "严重",
        "low" => "建议",
        _ => "问题",
    }
}

/// A review as a draft: a line comment per finding with a line, and a
/// comment on the whole PR naming the commit and holding the rest.
pub fn draft(review: &PullReview, snapshot: &PullSnapshot, now_ms: u64) -> WriteIntent {
    let id = format!("ghw-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
    let mut items = Vec::new();
    let mut rest = Vec::new();
    for (n, f) in review.findings.iter().enumerate() {
        let text = if f.basis.is_empty() {
            format!("**{}**：{}", severity(&f.severity), f.body)
        } else {
            format!(
                "**{}**：{}\n\n依据：{}",
                severity(&f.severity),
                f.body,
                f.basis
            )
        };
        match f.line {
            Some(line) => items.push(WriteItem {
                id: format!("c{}", n + 1),
                path: Some(f.path.clone()),
                line: Some(line),
                body: text,
                included: true,
                ..WriteItem::default()
            }),
            None => rest.push(format!("- `{}` {}", f.path, text.replace("\n\n", " "))),
        }
    }
    let short: String = review.head_sha.chars().take(7).collect();
    let mut summary = format!("审查了提交 {short}：{} 条意见。", review.findings.len());
    if !rest.is_empty() {
        summary.push_str("\n\n");
        summary.push_str(&rest.join("\n"));
    }
    items.insert(
        0,
        WriteItem {
            id: "summary".into(),
            body: summary,
            included: true,
            ..WriteItem::default()
        },
    );
    WriteIntent {
        id,
        project_id: review.project_id.clone(),
        review_id: review.id.clone(),
        repo: snapshot.repo.clone(),
        number: snapshot.number,
        head_sha: review.head_sha.clone(),
        items,
        state: WriteIntentState::Draft,
        reason: None,
        created_ms: now_ms,
        sent_ms: None,
    }
}

/// Change an item's text or whether it is published; only one not sent yet
/// and not out now.
pub fn edit(
    intent: &mut WriteIntent,
    item_id: &str,
    body: Option<String>,
    included: Option<bool>,
) -> Result<(), String> {
    if intent.state == WriteIntentState::Sending {
        return Err("正在发布，等它结束".into());
    }
    let item = intent
        .items
        .iter_mut()
        .find(|i| i.id == item_id)
        .ok_or("没有这一条")?;
    if !matches!(item.state, WriteItemState::Draft | WriteItemState::Failed) {
        return Err("这一条已经发出或结果不明，不能再改".into());
    }
    if let Some(body) = body {
        if body.trim().is_empty() {
            return Err("正文不能为空".into());
        }
        item.body = body;
    }
    if let Some(included) = included {
        item.included = included;
    }
    Ok(())
}

/// How one request ended.
#[derive(Debug, PartialEq)]
pub enum Sent {
    Ok {
        id: u64,
        url: String,
    },
    /// It did not happen: refused, or never left.
    Refused(String),
    /// Sent, no answer: it may have happened.
    Unknown(String),
}

/// The client that writes. Separate from the read client, which only GETs.
pub struct Writer {
    http: reqwest::Client,
    api: String,
    token: Option<String>,
}

impl Writer {
    pub fn new(api: &str, token: Option<String>, timeout: Duration) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .user_agent("Aeroric")
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Writer {
            http,
            api: api.trim_end_matches('/').to_string(),
            token: token.filter(|t| !t.trim().is_empty()),
        })
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let mut r = self
            .http
            .request(method, format!("{}{path}", self.api))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(token) = &self.token {
            r = r.bearer_auth(token);
        }
        r
    }

    async fn get(&self, path: &str) -> Result<Value, String> {
        let response = self
            .request(reqwest::Method::GET, path)
            .send()
            .await
            .map_err(|e| format!("连不上 GitHub：{e}"))?;
        let status = response.status().as_u16();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        if (200..300).contains(&status) {
            Ok(body)
        } else {
            Err(format!(
                "GitHub 返回 {status}：{}",
                body["message"].as_str().unwrap_or_default()
            ))
        }
    }

    /// The PR's head now.
    pub async fn head(&self, intent: &WriteIntent) -> Result<String, String> {
        let pull = self
            .get(&format!(
                "/repos/{}/{}/pulls/{}",
                intent.repo.owner, intent.repo.name, intent.number
            ))
            .await?;
        pull["head"]["sha"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| "GitHub 没有给出 PR 的提交".into())
    }

    /// The PR's comments that carry one of this intent's markers: marker →
    /// (id, link).
    pub async fn found(
        &self,
        intent: &WriteIntent,
    ) -> Result<HashMap<String, (u64, String)>, String> {
        let prefix = format!("<!-- aeroric:{}:", intent.id);
        let mut out = HashMap::new();
        let base = format!("/repos/{}/{}", intent.repo.owner, intent.repo.name);
        for list in [
            format!("{base}/pulls/{}/comments", intent.number),
            format!("{base}/issues/{}/comments", intent.number),
        ] {
            for page in 1..=MARKER_PAGES {
                let body = self
                    .get(&format!("{list}?per_page=100&page={page}"))
                    .await?;
                let items = body.as_array().cloned().unwrap_or_default();
                for c in &items {
                    let text = c["body"].as_str().unwrap_or_default();
                    if let Some(at) = text.find(&prefix) {
                        let end = text[at..].find("-->").map_or(text.len(), |e| at + e + 3);
                        out.insert(
                            text[at..end].to_string(),
                            (
                                c["id"].as_u64().unwrap_or(0),
                                c["html_url"].as_str().unwrap_or_default().to_string(),
                            ),
                        );
                    }
                }
                if items.len() < 100 {
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Send one item.
    pub async fn send(&self, intent: &WriteIntent, item: &WriteItem) -> Sent {
        let base = format!("/repos/{}/{}", intent.repo.owner, intent.repo.name);
        let text = body_with_marker(intent, item);
        let (path, payload) = match (&item.path, item.line) {
            (Some(path), Some(line)) => (
                format!("{base}/pulls/{}/comments", intent.number),
                json!({"body": text, "commit_id": intent.head_sha, "path": path, "line": line, "side": "RIGHT"}),
            ),
            _ => (
                format!("{base}/issues/{}/comments", intent.number),
                json!({"body": text}),
            ),
        };
        let response = match self
            .request(reqwest::Method::POST, &path)
            .json(&payload)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) if e.is_connect() || e.is_builder() => {
                return Sent::Refused(format!("没有发出去：连不上 GitHub（{e}）"))
            }
            Err(e) => return Sent::Unknown(format!("发出后没有收到回答：{e}")),
        };
        let status = response.status().as_u16();
        let body: Value = match response.json().await {
            Ok(v) => v,
            Err(e) if (200..300).contains(&status) => {
                return Sent::Unknown(format!("GitHub 收下了（{status}），但回答读不出来：{e}"))
            }
            Err(_) => Value::Null,
        };
        let said = body["message"].as_str().unwrap_or_default();
        match status {
            200..=299 => match body["id"].as_u64() {
                Some(id) => Sent::Ok {
                    id,
                    url: body["html_url"].as_str().unwrap_or_default().to_string(),
                },
                None => Sent::Unknown("GitHub 的回答里没有评论编号".into()),
            },
            500..=599 => {
                Sent::Unknown(format!("GitHub 出错（{status}），不知道发出去没有：{said}"))
            }
            401 => Sent::Refused("GitHub 不认这个令牌（401）".into()),
            403 => Sent::Refused(format!("没有权限评论这个 PR（403）：{said}")),
            _ => Sent::Refused(format!("GitHub 拒绝了（{status}）：{said}")),
        }
    }
}

fn settle(intent: &mut WriteIntent) {
    let included: Vec<&WriteItem> = intent.items.iter().filter(|i| i.included).collect();
    let any = |s: WriteItemState| included.iter().any(|i| i.state == s);
    intent.state = if any(WriteItemState::Unknown) || any(WriteItemState::Sending) {
        WriteIntentState::Unknown
    } else if included.iter().all(|i| i.state == WriteItemState::Sent) {
        WriteIntentState::Done
    } else if any(WriteItemState::Sent) {
        WriteIntentState::Partial
    } else {
        WriteIntentState::Failed
    };
}

/// Publish what is ticked and not sent yet (see the module). `resend_unknown`:
/// the user, told some went out with no answer and are not found on GitHub,
/// asked to send them again. `save` is called before and after each request.
pub async fn publish(
    writer: &Writer,
    intent: &mut WriteIntent,
    resend_unknown: bool,
    now_ms: u64,
    save: impl Fn(&WriteIntent) -> Result<(), String>,
) -> Result<(), String> {
    if intent.state == WriteIntentState::Sending {
        return Err("正在发布，等它结束".into());
    }
    if !intent.items.iter().any(|i| i.included) {
        return Err("没有勾选要发布的条目".into());
    }
    let head = writer.head(intent).await?;
    if head != intent.head_sha {
        intent.state = WriteIntentState::TargetMoved;
        intent.reason = Some(format!(
            "PR 在审查之后有了新提交（现在是 {}），没有发布；重新审查后再发",
            head.chars().take(7).collect::<String>()
        ));
        return save(intent);
    }
    let found = writer.found(intent).await?;
    for item in intent.items.iter_mut() {
        if item.state == WriteItemState::Sending {
            item.state = WriteItemState::Unknown;
        }
        if item.state != WriteItemState::Sent {
            if let Some((id, url)) = found.get(&marker(&intent.id, &item.id)) {
                item.state = WriteItemState::Sent;
                item.remote_id = Some(*id);
                item.url = Some(url.clone());
                item.reason = None;
            }
        }
    }
    intent.state = WriteIntentState::Sending;
    intent.reason = None;
    save(intent)?;
    for at in 0..intent.items.len() {
        let item = &intent.items[at];
        let due = item.included
            && match item.state {
                WriteItemState::Draft | WriteItemState::Failed => true,
                WriteItemState::Unknown => resend_unknown,
                _ => false,
            };
        if !due {
            continue;
        }
        intent.items[at].state = WriteItemState::Sending;
        save(intent)?;
        let result = writer.send(intent, &intent.items[at]).await;
        let item = &mut intent.items[at];
        match result {
            Sent::Ok { id, url } => {
                item.state = WriteItemState::Sent;
                item.remote_id = Some(id);
                item.url = Some(url);
                item.reason = None;
            }
            Sent::Refused(why) => {
                item.state = WriteItemState::Failed;
                item.reason = Some(why);
            }
            Sent::Unknown(why) => {
                item.state = WriteItemState::Unknown;
                item.reason = Some(why);
            }
        }
        save(intent)?;
    }
    settle(intent);
    intent.sent_ms = Some(now_ms);
    save(intent)
}

fn dir(data_dir: &Path, project_id: &str) -> PathBuf {
    let safe: String = project_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    data_dir.join("github").join(safe).join("writes")
}

pub fn save(data_dir: &Path, intent: &WriteIntent) -> Result<(), String> {
    let dir = dir(data_dir, &intent.project_id);
    crate::settings::ensure_private_dir(&dir)?;
    let text = serde_json::to_string_pretty(intent).map_err(|e| e.to_string())?;
    crate::settings::atomic_write_private(&dir.join(format!("{}.json", intent.id)), &text)
}

/// One kept intent. Read while nothing is sending (a host that stopped
/// mid-send), an item left `Sending` reads as unknown.
pub fn load(data_dir: &Path, project_id: &str, id: &str) -> Option<WriteIntent> {
    let path = dir(data_dir, project_id).join(format!("{id}.json"));
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// After a restart: whatever was sending is unknown.
pub fn recover(intent: &mut WriteIntent) {
    if intent.state != WriteIntentState::Sending {
        return;
    }
    for item in intent.items.iter_mut() {
        if item.state == WriteItemState::Sending {
            item.state = WriteItemState::Unknown;
            item.reason = Some("发送时 Aeroric 停了，不知道发出去没有".into());
        }
    }
    settle(intent);
}

/// A project's intents, newest first.
pub fn list(data_dir: &Path, project_id: &str) -> Vec<WriteIntent> {
    let Ok(entries) = std::fs::read_dir(dir(data_dir, project_id)) else {
        return Vec::new();
    };
    let mut out: Vec<WriteIntent> = entries
        .flatten()
        .filter_map(|e| serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok())
        .collect();
    out.sort_by(|a, b| b.created_ms.cmp(&a.created_ms).then(b.id.cmp(&a.id)));
    out
}

#[cfg(test)]
#[path = "publish_tests.rs"]
mod tests;
