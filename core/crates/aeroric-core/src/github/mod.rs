//! D1 (2026-11-21): GitHub, read only — a pull request pinned as a snapshot
//! (title, branches, head and base commits, changed files, checks, comments)
//! to review. Nothing is written to GitHub: the client only sends GET.
//!
//! - [`target`] reads a PR link (`https://github.com/o/n/pull/12`,
//!   `o/n#12`, a GitHub Enterprise link);
//! - [`client::Client`] fetches it, page by page, up to caps that set
//!   `truncated`; a refusal says why (no token, no access, rate limit), never
//!   an empty list;
//! - [`store`] keeps snapshots per project, one per head commit, so fetching
//!   the same head again (a retry) adds nothing, and an older head is marked
//!   superseded (PR 已更新).
//!
//! Second step: [`review`] fetches the head into a ref of Aeroric's own
//! (`refs/aeroric/pr/<n>/<sha>`), checks it out in a detached worktree
//! beside the repository's others, hands a read-only reviewer the snapshot,
//! and reads its findings back, each bound to a file and line of that head.

pub mod client;
pub mod review;
pub mod store;

#[cfg(test)]
mod tests;

/// Changed files kept per snapshot.
pub const MAX_FILES: usize = 200;
/// Check runs kept.
pub const MAX_CHECKS: usize = 100;
/// Comments kept (the whole PR's and review comments together).
pub const MAX_COMMENTS: usize = 200;
/// Items per page asked of GitHub.
pub const PER_PAGE: usize = 100;

/// A pull request named by a link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullTarget {
    pub host: String,
    pub owner: String,
    pub name: String,
    pub number: u64,
}

impl PullTarget {
    /// The REST API's root: `https://api.github.com` for github.com, else
    /// GitHub Enterprise's `https://<host>/api/v3`.
    pub fn api_base(&self) -> String {
        if self.host == "github.com" {
            "https://api.github.com".into()
        } else {
            format!("https://{}/api/v3", self.host)
        }
    }
}

fn valid_part(part: &str) -> bool {
    !part.is_empty()
        && part != "."
        && part != ".."
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// The PR a link names: `https://<host>/<owner>/<name>/pull/<n>` (with or
/// without the scheme, anything after the number ignored) or `<owner>/<name>#<n>`.
pub fn target(link: &str) -> Result<PullTarget, String> {
    let link = link.trim();
    let bad =
        || format!("看不懂这个 PR 链接：{link}（例如 https://github.com/owner/repo/pull/12）");
    if let Some((repo, number)) = link.split_once('#') {
        let (owner, name) = repo.split_once('/').ok_or_else(bad)?;
        let number = number.parse::<u64>().map_err(|_| bad())?;
        if !valid_part(owner) || !valid_part(name) || number == 0 {
            return Err(bad());
        }
        return Ok(PullTarget {
            host: "github.com".into(),
            owner: owner.into(),
            name: name.into(),
            number,
        });
    }
    let rest = link
        .strip_prefix("https://")
        .or_else(|| link.strip_prefix("http://"))
        .unwrap_or(link);
    let parts: Vec<&str> = rest.split('/').collect();
    if parts.len() < 5 || parts[3] != "pull" {
        return Err(bad());
    }
    let host = parts[0].to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    let digits: String = parts[4].chars().take_while(char::is_ascii_digit).collect();
    let number = digits.parse::<u64>().map_err(|_| bad())?;
    let host_ok = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | ':'));
    if !host_ok || !valid_part(parts[1]) || !valid_part(parts[2]) || number == 0 {
        return Err(bad());
    }
    Ok(PullTarget {
        host,
        owner: parts[1].into(),
        name: parts[2].trim_end_matches(".git").into(),
        number,
    })
}

/// A snapshot's id: the repository's number, the PR's, and the head commit.
pub fn snapshot_id(repo_id: u64, number: u64, head_sha: &str) -> String {
    let short: String = head_sha.chars().take(12).collect();
    format!("{repo_id}-{number}-{short}")
}
