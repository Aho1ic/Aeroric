//! The read-only REST client. Only GET is sent.

use aeroric_protocol::github::{
    CommentAnchor, PullCheck, PullComment, PullFile, PullSnapshot, RepoRef,
};
use serde_json::Value;

use super::{snapshot_id, PullTarget, MAX_CHECKS, MAX_COMMENTS, MAX_FILES, PER_PAGE};

const TIMEOUT_SECS: u64 = 20;

pub struct Client {
    http: reqwest::Client,
    api: String,
    token: Option<String>,
}

fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_string()
}

impl Client {
    /// `api`: the REST root ([`PullTarget::api_base`], or a test server).
    pub fn new(api: &str, token: Option<String>) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
            .user_agent("Aeroric")
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Client {
            http,
            api: api.trim_end_matches('/').to_string(),
            token: token.filter(|t| !t.trim().is_empty()),
        })
    }

    /// One GET; a refusal is said in words.
    async fn get(&self, path: &str) -> Result<Value, String> {
        let mut request = self
            .http
            .get(format!("{}{path}", self.api))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .await
            .map_err(|e| format!("连不上 GitHub：{e}"))?;
        let status = response.status().as_u16();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        let said = body["message"].as_str().unwrap_or_default();
        match status {
            200..=299 => Ok(body),
            401 => Err("GitHub 不认这个令牌（401）：重新登录 gh，或换一个令牌".into()),
            403 | 429 if said.to_ascii_lowercase().contains("rate limit") => Err(format!(
                "GitHub 限流了（{status}）：稍后再试，或用登录过的 gh"
            )),
            403 => Err(format!(
                "没有权限读这个仓库（403）：令牌要能读它的内容和 PR{}",
                if said.is_empty() {
                    String::new()
                } else {
                    format!("。GitHub 说：{said}")
                }
            )),
            404 => Err(if self.token.is_some() {
                "找不到这个仓库或 PR（404）：确认链接，或令牌能看到这个仓库".into()
            } else {
                "找不到这个仓库或 PR（404）：私有仓库要先登录 gh 或填令牌".into()
            }),
            _ => Err(format!("GitHub 返回 {status}：{said}")),
        }
    }

    /// Pages of a list until a short page or `cap` items; true when the cap
    /// left some out.
    async fn pages(
        &self,
        path: &str,
        field: Option<&str>,
        cap: usize,
    ) -> Result<(Vec<Value>, bool), String> {
        let mut out = Vec::new();
        let sep = if path.contains('?') { '&' } else { '?' };
        for page in 1.. {
            let body = self
                .get(&format!("{path}{sep}per_page={PER_PAGE}&page={page}"))
                .await?;
            let items = match field {
                Some(field) => body[field].as_array().cloned().unwrap_or_default(),
                None => body.as_array().cloned().unwrap_or_default(),
            };
            let short = items.len() < PER_PAGE;
            out.extend(items);
            if out.len() > cap {
                out.truncate(cap);
                return Ok((out, true));
            }
            if short {
                break;
            }
            if out.len() == cap {
                // Exactly at the cap: one more page says whether more exist.
                let more = self
                    .get(&format!("{path}{sep}per_page=1&page={}", cap + 1))
                    .await?;
                let left = match field {
                    Some(field) => more[field].as_array().is_some_and(|a| !a.is_empty()),
                    None => more.as_array().is_some_and(|a| !a.is_empty()),
                };
                return Ok((out, left));
            }
        }
        Ok((out, false))
    }

    /// The PR as it is now. Its repository is read first: a renamed one is
    /// followed (GitHub redirects) and recorded under its number.
    pub async fn pull(
        &self,
        target: &PullTarget,
        project_id: &str,
        now_ms: u64,
    ) -> Result<PullSnapshot, String> {
        let repo = self
            .get(&format!("/repos/{}/{}", target.owner, target.name))
            .await?;
        let repo = RepoRef {
            host: target.host.clone(),
            owner: repo["owner"]["login"]
                .as_str()
                .unwrap_or(&target.owner)
                .to_string(),
            name: repo["name"].as_str().unwrap_or(&target.name).to_string(),
            repo_id: repo["id"].as_u64().ok_or("GitHub 没有给出仓库编号")?,
        };
        let base = format!("/repos/{}/{}", repo.owner, repo.name);
        let number = target.number;
        let pull = self.get(&format!("{base}/pulls/{number}")).await?;
        let head_sha = text(&pull["head"], "sha");
        if head_sha.is_empty() {
            return Err("GitHub 没有给出 PR 的提交".into());
        }
        let base_sha = text(&pull["base"], "sha");
        let merge_base = self
            .get(&format!("{base}/compare/{base_sha}...{head_sha}"))
            .await
            .map(|c| text(&c["merge_base_commit"], "sha"))
            .unwrap_or_default();
        let (files, files_cut) = self
            .pages(&format!("{base}/pulls/{number}/files"), None, MAX_FILES)
            .await?;
        let (checks, checks_cut) = self
            .pages(
                &format!("{base}/commits/{head_sha}/check-runs"),
                Some("check_runs"),
                MAX_CHECKS,
            )
            .await?;
        let (talk, talk_cut) = self
            .pages(
                &format!("{base}/issues/{number}/comments"),
                None,
                MAX_COMMENTS,
            )
            .await?;
        let (review, review_cut) = self
            .pages(
                &format!("{base}/pulls/{number}/comments"),
                None,
                MAX_COMMENTS,
            )
            .await?;
        let mut comments: Vec<PullComment> = talk
            .iter()
            .map(|c| PullComment {
                author: text(&c["user"], "login"),
                body: text(c, "body"),
                anchor: None,
                outdated: false,
                at: text(c, "created_at"),
            })
            .collect();
        comments.extend(review.iter().map(|c| {
            let commit = text(c, "commit_id");
            PullComment {
                author: text(&c["user"], "login"),
                body: text(c, "body"),
                outdated: commit != head_sha || c["line"].is_null(),
                anchor: Some(CommentAnchor {
                    path: text(c, "path"),
                    side: text(c, "side"),
                    line: c["line"].as_u64(),
                    commit_sha: commit,
                }),
                at: text(c, "created_at"),
            }
        }));
        let comments_cut = talk_cut || review_cut || comments.len() > MAX_COMMENTS;
        comments.truncate(MAX_COMMENTS);
        Ok(PullSnapshot {
            id: snapshot_id(repo.repo_id, number, &head_sha),
            project_id: project_id.to_string(),
            number,
            title: text(&pull, "title"),
            author: text(&pull["user"], "login"),
            state: text(&pull, "state"),
            merged: pull["merged"].as_bool().unwrap_or(false),
            url: text(&pull, "html_url"),
            head_ref: text(&pull["head"], "ref"),
            base_ref: text(&pull["base"], "ref"),
            head_sha,
            base_sha,
            merge_base,
            fetched_ms: now_ms,
            files: files
                .iter()
                .map(|f| PullFile {
                    path: text(f, "filename"),
                    status: text(f, "status"),
                    additions: f["additions"].as_u64().unwrap_or(0),
                    deletions: f["deletions"].as_u64().unwrap_or(0),
                    previous_path: f["previous_filename"].as_str().map(str::to_string),
                })
                .collect(),
            checks: checks
                .iter()
                .map(|c| PullCheck {
                    name: text(c, "name"),
                    status: text(c, "status"),
                    conclusion: c["conclusion"].as_str().map(str::to_string),
                    url: text(c, "html_url"),
                })
                .collect(),
            comments,
            truncated: files_cut || checks_cut || comments_cut,
            superseded: false,
            repo,
        })
    }
}
