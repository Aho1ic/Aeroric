//! D1: the read-only client against a fake GitHub on 127.0.0.1, which
//! records every request so the tests can say only GET was sent.

use std::sync::{Arc, Mutex};

use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::Json;
use serde_json::{json, Value};

use super::client::Client;
use super::{store, target, PullTarget, MAX_FILES};

const OLD: &str = "1111111111111111111111111111111111111111";

#[derive(Default)]
struct Fake {
    head: String,
    files: usize,
    /// Requests seen: (method, path with query, authorization).
    seen: Vec<(String, String, String)>,
    /// The files list fails this many more times (a dropped connection).
    fail_files: usize,
    /// Every request is answered with this.
    refuse: Option<(u16, &'static str)>,
}

type Shared = Arc<Mutex<Fake>>;

fn page(uri: &Uri) -> (usize, usize) {
    let mut per = 30;
    let mut page = 1;
    for pair in uri.query().unwrap_or_default().split('&') {
        match pair.split_once('=') {
            Some(("per_page", v)) => per = v.parse().unwrap_or(30),
            Some(("page", v)) => page = v.parse().unwrap_or(1),
            _ => {}
        }
    }
    (per, page)
}

fn slice(items: Vec<Value>, uri: &Uri) -> Value {
    let (per, page) = page(uri);
    Value::Array(items.into_iter().skip((page - 1) * per).take(per).collect())
}

async fn answer(
    axum::extract::State(fake): axum::extract::State<Shared>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    let mut f = fake.lock().unwrap();
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    f.seen.push((
        method.to_string(),
        uri.path_and_query()
            .map(|p| p.to_string())
            .unwrap_or_default(),
        auth,
    ));
    if let Some((status, message)) = f.refuse {
        return (
            StatusCode::from_u16(status).unwrap(),
            Json(json!({"message": message})),
        );
    }
    let head = f.head.clone();
    let path = uri.path().to_string();
    let ok = |v: Value| (StatusCode::OK, Json(v));
    // The repository was renamed from `old`: both names answer as `widget`.
    if path == "/repos/acme/old" || path == "/repos/acme/widget" {
        return ok(json!({"id": 42, "name": "widget", "owner": {"login": "acme"}}));
    }
    let Some(rest) = path.strip_prefix("/repos/acme/widget") else {
        return (StatusCode::NOT_FOUND, Json(json!({"message": "Not Found"})));
    };
    match rest {
        "/pulls/7" => ok(json!({
            "title": "解析器支持注释", "user": {"login": "dev"}, "state": "open", "merged": false,
            "html_url": "https://github.com/acme/widget/pull/7",
            "head": {"ref": "comments", "sha": head}, "base": {"ref": "main", "sha": "b".repeat(40)},
        })),
        "/pulls/7/files" => {
            if f.fail_files > 0 {
                f.fail_files -= 1;
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({"message": "upstream"})),
                );
            }
            let files = (0..f.files)
                .map(|i| json!({"filename": format!("src/f{i}.rs"), "status": "modified", "additions": 2, "deletions": 1}))
                .collect();
            ok(slice(files, &uri))
        }
        "/issues/7/comments" => ok(slice(
            vec![
                json!({"user": {"login": "lead"}, "body": "整体可以", "created_at": "2026-11-20T10:00:00Z"}),
            ],
            &uri,
        )),
        "/pulls/7/comments" => ok(slice(
            vec![
                json!({"user": {"login": "lead"}, "body": "这里漏了空行", "path": "src/f0.rs", "side": "RIGHT", "line": 3, "commit_id": head}),
                json!({"user": {"login": "lead"}, "body": "旧提交上的意见", "path": "src/f1.rs", "side": "RIGHT", "line": 9, "commit_id": OLD}),
                json!({"user": {"login": "lead"}, "body": "那一行已经没了", "path": "src/f2.rs", "side": "RIGHT", "line": null, "commit_id": head}),
            ],
            &uri,
        )),
        _ if rest.starts_with("/compare/") => {
            ok(json!({"merge_base_commit": {"sha": "c".repeat(40)}}))
        }
        _ if rest.starts_with(&format!("/commits/{head}/check-runs")) => {
            ok(json!({"check_runs": slice(vec![
            json!({"name": "ci", "status": "completed", "conclusion": "failure", "html_url": "https://ci/1"}),
            json!({"name": "lint", "status": "in_progress", "conclusion": null, "html_url": ""}),
        ], &uri)}))
        }
        _ => (StatusCode::NOT_FOUND, Json(json!({"message": "Not Found"}))),
    }
}

async fn serve(fake: Shared) -> String {
    let app = axum::Router::new().fallback(answer).with_state(fake);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{at}")
}

fn fake(head: &str, files: usize) -> Shared {
    Arc::new(Mutex::new(Fake {
        head: head.into(),
        files,
        ..Fake::default()
    }))
}

fn pr(name: &str) -> PullTarget {
    PullTarget {
        host: "github.com".into(),
        owner: "acme".into(),
        name: name.into(),
        number: 7,
    }
}

#[test]
fn links_name_a_pull_request() {
    let t = target("https://github.com/acme/widget/pull/7/files?w=1").unwrap();
    assert_eq!(
        (t.host.as_str(), t.owner.as_str(), t.name.as_str(), t.number),
        ("github.com", "acme", "widget", 7)
    );
    assert_eq!(t.api_base(), "https://api.github.com");
    assert_eq!(
        target("www.github.com/acme/widget/pull/12").unwrap().number,
        12
    );
    assert_eq!(target("acme/widget#3").unwrap().name, "widget");
    let ghe = target("https://git.corp.example/team/app/pull/5").unwrap();
    assert_eq!(ghe.api_base(), "https://git.corp.example/api/v3");
    for bad in [
        "https://github.com/acme/widget/issues/7",
        "https://github.com/acme/widget",
        "acme/widget#x",
        "https://github.com/../widget/pull/1",
        "https://github.com/acme/widget/pull/0",
    ] {
        assert!(target(bad).unwrap_err().contains("看不懂"), "{bad}");
    }
}

/// The whole PR: a renamed repository is followed and kept by its number;
/// 300 files stop at the cap with `truncated`; checks and comments come
/// along, a comment on an older commit or no longer on a line is outdated;
/// only GET is sent, each with the token.
#[tokio::test]
async fn a_pull_request_is_read_whole_with_get_only() {
    let head = "a".repeat(40);
    let f = fake(&head, 300);
    let api = serve(Arc::clone(&f)).await;
    let client = Client::new(&api, Some("tok".into())).unwrap();
    let snap = client.pull(&pr("old"), "p1", 5).await.unwrap();

    assert_eq!((snap.repo.repo_id, snap.repo.name.as_str()), (42, "widget"));
    assert_eq!(snap.id, format!("42-7-{}", &head[..12]));
    assert_eq!(snap.title, "解析器支持注释");
    assert_eq!(
        (snap.head_ref.as_str(), snap.base_ref.as_str()),
        ("comments", "main")
    );
    assert_eq!(snap.merge_base, "c".repeat(40));
    assert_eq!(snap.files.len(), MAX_FILES);
    assert!(snap.truncated, "300 files is past the cap");
    assert_eq!(snap.checks.len(), 2);
    assert_eq!(snap.checks[0].conclusion.as_deref(), Some("failure"));
    assert_eq!(snap.checks[1].conclusion, None);
    assert_eq!(snap.comments.len(), 4);
    assert!(snap.comments[0].anchor.is_none());
    let outdated: Vec<bool> = snap.comments[1..].iter().map(|c| c.outdated).collect();
    assert_eq!(outdated, [false, true, true]);
    assert_eq!(snap.comments[1].anchor.as_ref().unwrap().line, Some(3));

    let seen = f.lock().unwrap().seen.clone();
    assert!(seen.iter().all(|(m, _, _)| m == "GET"), "{seen:?}");
    assert!(seen.iter().all(|(_, _, auth)| auth == "Bearer tok"));
    assert!(seen
        .iter()
        .any(|(_, p, _)| p.starts_with("/repos/acme/widget/pulls/7/files")));
}

/// A refusal says why — never an empty PR.
#[tokio::test]
async fn refusals_say_why() {
    for (status, message, said) in [
        (403, "Resource not accessible by integration", "没有权限"),
        (403, "API rate limit exceeded for 1.2.3.4", "限流"),
        (401, "Bad credentials", "401"),
        (404, "Not Found", "私有仓库"),
    ] {
        let f = fake("a", 1);
        f.lock().unwrap().refuse = Some((status, message));
        let api = serve(f).await;
        let error = Client::new(&api, None)
            .unwrap()
            .pull(&pr("widget"), "p1", 1)
            .await
            .unwrap_err();
        assert!(error.contains(said), "{status} {message}: {error}");
    }
}

/// A dropped connection keeps nothing; the retry keeps one snapshot, and
/// fetching the same head again adds none (its pinned time stays). After a
/// force-push the new head is a new snapshot and the old one is superseded.
#[tokio::test]
async fn snapshots_are_one_per_head_and_the_older_is_superseded() {
    let data = std::env::temp_dir().join(format!("aeroric-github-{}", uuid::Uuid::new_v4()));
    let head = "a".repeat(40);
    let f = fake(&head, 3);
    f.lock().unwrap().fail_files = 1;
    let api = serve(Arc::clone(&f)).await;
    let client = Client::new(&api, None).unwrap();

    assert!(client
        .pull(&pr("widget"), "p1", 1)
        .await
        .unwrap_err()
        .contains("502"));
    assert!(store::list(&data, "p1").is_empty());
    let first = store::save(&data, &client.pull(&pr("widget"), "p1", 2).await.unwrap()).unwrap();
    let again = store::save(&data, &client.pull(&pr("widget"), "p1", 3).await.unwrap()).unwrap();
    assert_eq!(again, first, "the same head is the same snapshot");
    assert_eq!(store::list(&data, "p1").len(), 1);
    assert_eq!(store::list(&data, "p1")[0].fetched_ms, 2);

    f.lock().unwrap().head = "d".repeat(40);
    let pushed = store::save(&data, &client.pull(&pr("widget"), "p1", 4).await.unwrap()).unwrap();
    let listed = store::list(&data, "p1");
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].id, pushed.id);
    assert!(!listed[0].superseded);
    assert!(listed[1].superseded, "PR 已更新");
    assert!(store::get(&data, "p1", &first.id).unwrap().superseded);
    assert!(store::list(&data, "other").is_empty());
    std::fs::remove_dir_all(&data).ok();
}
