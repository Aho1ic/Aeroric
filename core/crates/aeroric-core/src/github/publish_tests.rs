//! D2: publishing against a fake GitHub on 127.0.0.1 that keeps the
//! comments it is sent, so the tests can count them.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use aeroric_protocol::github::{
    PullFinding, PullReview, PullReviewState, PullSnapshot, RepoRef, WriteIntent, WriteIntentState,
    WriteItemState,
};
use axum::http::{Method, StatusCode, Uri};
use axum::Json;
use serde_json::{json, Value};

use super::{draft, edit, list, load, marker, publish, recover, save, Writer};

#[derive(Default)]
struct Fake {
    head: String,
    /// (list, body, id): `review` or `issue`.
    comments: Vec<(String, String, u64)>,
    posts: usize,
    /// The nth POST (1-based) is refused with 422.
    refuse_post: Option<usize>,
    /// The nth POST is kept but answered too late.
    late_post: Option<usize>,
    /// Bodies of the POSTs, for what was sent.
    sent: Vec<Value>,
}

type Shared = Arc<Mutex<Fake>>;

async fn answer(
    axum::extract::State(fake): axum::extract::State<Shared>,
    method: Method,
    uri: Uri,
    body: Option<Json<Value>>,
) -> (StatusCode, Json<Value>) {
    let late = {
        let mut f = fake.lock().unwrap();
        let path = uri.path().to_string();
        let page = uri
            .query()
            .and_then(|q| q.split('&').find_map(|p| p.strip_prefix("page=")))
            .and_then(|p| p.parse::<usize>().ok())
            .unwrap_or(1);
        match (method.as_str(), path.as_str()) {
            ("GET", "/repos/acme/widget/pulls/7") => {
                return (StatusCode::OK, Json(json!({"head": {"sha": f.head}})))
            }
            (
                "GET",
                "/repos/acme/widget/pulls/7/comments" | "/repos/acme/widget/issues/7/comments",
            ) => {
                let kind = if path.contains("/pulls/") {
                    "review"
                } else {
                    "issue"
                };
                let listed: Vec<Value> = if page == 1 {
                    f.comments
                        .iter()
                        .filter(|(k, _, _)| k == kind)
                        .map(|(_, b, id)| json!({"id": id, "body": b, "html_url": format!("https://gh/c/{id}")}))
                        .collect()
                } else {
                    Vec::new()
                };
                return (StatusCode::OK, Json(Value::Array(listed)));
            }
            (
                "POST",
                "/repos/acme/widget/pulls/7/comments" | "/repos/acme/widget/issues/7/comments",
            ) => {
                f.posts += 1;
                let n = f.posts;
                let body = body.map(|Json(v)| v).unwrap_or(Value::Null);
                f.sent.push(body.clone());
                if f.refuse_post == Some(n) {
                    return (
                        StatusCode::UNPROCESSABLE_ENTITY,
                        Json(
                            json!({"message": "pull_request_review_thread.line must be part of the diff"}),
                        ),
                    );
                }
                let kind = if path.contains("/pulls/") {
                    "review"
                } else {
                    "issue"
                };
                let id = 1000 + n as u64;
                f.comments.push((
                    kind.into(),
                    body["body"].as_str().unwrap_or_default().into(),
                    id,
                ));
                f.late_post == Some(n)
            }
            _ => return (StatusCode::NOT_FOUND, Json(json!({"message": "Not Found"}))),
        }
    };
    if late {
        tokio::time::sleep(Duration::from_millis(1500)).await;
    }
    let n = fake.lock().unwrap().posts as u64;
    (
        StatusCode::CREATED,
        Json(json!({"id": 1000 + n, "html_url": format!("https://gh/c/{}", 1000 + n)})),
    )
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

const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn rig() -> (Shared, Arc<Mutex<Vec<WriteIntent>>>) {
    (
        Arc::new(Mutex::new(Fake {
            head: HEAD.into(),
            ..Fake::default()
        })),
        Arc::new(Mutex::new(Vec::new())),
    )
}

/// A review with `n` findings on lines and one without.
fn intent(n: usize) -> WriteIntent {
    let mut findings: Vec<PullFinding> = (1..=n)
        .map(|i| PullFinding {
            path: format!("src/f{i}.rs"),
            line: Some(i as u64),
            severity: if i == 1 { "high".into() } else { "low".into() },
            body: format!("第 {i} 条"),
            basis: if i == 1 {
                "看到了".into()
            } else {
                String::new()
            },
        })
        .collect();
    findings.push(PullFinding {
        path: "README.md".into(),
        severity: "medium".into(),
        body: "说明没更新".into(),
        ..PullFinding::default()
    });
    let review = PullReview {
        id: "prrev-1".into(),
        project_id: "p1".into(),
        snapshot_id: "s".into(),
        head_sha: HEAD.into(),
        state: PullReviewState::Done,
        findings,
        ..PullReview::default()
    };
    let snapshot = PullSnapshot {
        repo: RepoRef {
            host: "github.com".into(),
            owner: "acme".into(),
            name: "widget".into(),
            repo_id: 42,
        },
        number: 7,
        ..PullSnapshot::default()
    };
    draft(&review, &snapshot, 1)
}

fn writer(api: &str) -> Writer {
    Writer::new(api, Some("tok".into()), Duration::from_millis(500)).unwrap()
}

fn keep(saved: &Arc<Mutex<Vec<WriteIntent>>>) -> impl Fn(&WriteIntent) -> Result<(), String> + '_ {
    move |i| {
        saved.lock().unwrap().push(i.clone());
        Ok(())
    }
}

/// The draft: a summary first (holding the finding without a line), then a
/// line comment per finding; items can be edited and left out, not once sent.
#[test]
fn a_review_becomes_a_draft_the_user_edits() {
    let mut i = intent(2);
    assert_eq!(i.items.len(), 3);
    assert_eq!(i.items[0].id, "summary");
    assert!(i.items[0].path.is_none());
    assert!(i.items[0]
        .body
        .starts_with("审查了提交 aaaaaaa：3 条意见。"));
    assert!(i.items[0].body.contains("`README.md`"));
    assert_eq!(i.items[1].path.as_deref(), Some("src/f1.rs"));
    assert_eq!(i.items[1].body, "**严重**：第 1 条\n\n依据：看到了");
    assert_eq!(i.items[2].body, "**建议**：第 2 条");
    edit(&mut i, "c2", Some("改过的".into()), Some(false)).unwrap();
    assert_eq!(
        (i.items[2].body.as_str(), i.items[2].included),
        ("改过的", false)
    );
    assert!(edit(&mut i, "c2", Some(" ".into()), None).is_err());
    i.items[1].state = WriteItemState::Sent;
    assert!(edit(&mut i, "c1", Some("x".into()), None)
        .unwrap_err()
        .contains("已经发出"));
}

/// Everything ticked goes out once, line comments on the reviewed commit,
/// each with its marker; publishing again sends nothing more.
#[tokio::test]
async fn a_draft_is_published_once() {
    let (fake, saved) = rig();
    let api = serve(Arc::clone(&fake)).await;
    let mut i = intent(2);
    edit(&mut i, "c2", None, Some(false)).unwrap();
    publish(&writer(&api), &mut i, false, 9, keep(&saved))
        .await
        .unwrap();
    assert_eq!(i.state, WriteIntentState::Done);
    assert_eq!(i.sent_ms, Some(9));
    assert_eq!(i.items[2].state, WriteItemState::Draft, "left out");
    assert!(i.items[1]
        .url
        .as_deref()
        .unwrap()
        .starts_with("https://gh/c/"));
    {
        let f = fake.lock().unwrap();
        assert_eq!(f.posts, 2);
        assert_eq!(f.sent[1]["commit_id"], HEAD);
        assert_eq!(f.sent[1]["path"], "src/f1.rs");
        assert!(f.sent[1]["body"]
            .as_str()
            .unwrap()
            .ends_with(&marker(&i.id, "c1")));
    }
    // Each request was saved as going out before it was sent.
    assert!(saved
        .lock()
        .unwrap()
        .iter()
        .any(|s| s.items[1].state == WriteItemState::Sending));
    publish(&writer(&api), &mut i, false, 10, keep(&saved))
        .await
        .unwrap();
    assert_eq!(fake.lock().unwrap().posts, 2, "nothing twice");
}

/// The third of five refused: the rest still go, the result is partial and
/// says why; publishing again sends only that one.
#[tokio::test]
async fn a_refused_item_is_partial_and_only_it_is_sent_again() {
    let (fake, saved) = rig();
    fake.lock().unwrap().refuse_post = Some(3);
    let api = serve(Arc::clone(&fake)).await;
    let mut i = intent(4);
    publish(&writer(&api), &mut i, false, 1, keep(&saved))
        .await
        .unwrap();
    assert_eq!(i.state, WriteIntentState::Partial);
    assert_eq!(i.items[2].state, WriteItemState::Failed);
    assert!(i.items[2].reason.as_deref().unwrap().contains("422"));
    assert_eq!(fake.lock().unwrap().comments.len(), 4);
    publish(&writer(&api), &mut i, false, 2, keep(&saved))
        .await
        .unwrap();
    assert_eq!(i.state, WriteIntentState::Done);
    let f = fake.lock().unwrap();
    assert_eq!((f.posts, f.comments.len()), (6, 5));
}

/// The PR moved on since the review: nothing is sent.
#[tokio::test]
async fn a_moved_pr_is_not_published() {
    let (fake, saved) = rig();
    fake.lock().unwrap().head = "b".repeat(40);
    let api = serve(Arc::clone(&fake)).await;
    let mut i = intent(1);
    publish(&writer(&api), &mut i, false, 1, keep(&saved))
        .await
        .unwrap();
    assert_eq!(i.state, WriteIntentState::TargetMoved);
    assert!(i.reason.as_deref().unwrap().contains("bbbbbbb"));
    assert_eq!(fake.lock().unwrap().posts, 0);
}

/// Sent with no answer (GitHub kept it): unknown, not sent again; the next
/// publish finds its marker and records it sent.
#[tokio::test]
async fn an_answer_lost_is_found_by_its_marker_not_sent_twice() {
    let (fake, saved) = rig();
    fake.lock().unwrap().late_post = Some(2);
    let api = serve(Arc::clone(&fake)).await;
    let mut i = intent(1);
    publish(&writer(&api), &mut i, false, 1, keep(&saved))
        .await
        .unwrap();
    assert_eq!(i.state, WriteIntentState::Unknown);
    assert_eq!(i.items[1].state, WriteItemState::Unknown);
    tokio::time::sleep(Duration::from_millis(1200)).await;
    publish(&writer(&api), &mut i, false, 2, keep(&saved))
        .await
        .unwrap();
    assert_eq!(i.state, WriteIntentState::Done);
    assert_eq!(i.items[1].remote_id, Some(1002));
    assert_eq!(
        fake.lock().unwrap().comments.len(),
        2,
        "one comment per item"
    );
}

/// Aeroric stopped mid-send: the item reads unknown. Not on GitHub, it is
/// only sent again when the user asks.
#[tokio::test]
async fn a_send_cut_by_a_restart_waits_for_the_user() {
    let data = std::env::temp_dir().join(format!("aeroric-ghw-{}", uuid::Uuid::new_v4()));
    let (fake, saved) = rig();
    let api = serve(Arc::clone(&fake)).await;
    let mut i = intent(1);
    i.state = WriteIntentState::Sending;
    i.items[1].state = WriteItemState::Sending;
    save(&data, &i).unwrap();
    assert!(
        publish(&writer(&api), &mut i, false, 1, keep(&saved))
            .await
            .unwrap_err()
            .contains("正在发布"),
        "a second click while sending"
    );

    let mut back = load(&data, "p1", &i.id).unwrap();
    recover(&mut back);
    assert_eq!(back.items[1].state, WriteItemState::Unknown);
    assert_eq!(back.state, WriteIntentState::Unknown);
    publish(&writer(&api), &mut back, false, 2, keep(&saved))
        .await
        .unwrap();
    assert_eq!(
        back.items[1].state,
        WriteItemState::Unknown,
        "not on GitHub, not resent"
    );
    assert_eq!(back.items[0].state, WriteItemState::Sent);
    assert_eq!(fake.lock().unwrap().posts, 1);
    publish(&writer(&api), &mut back, true, 3, keep(&saved))
        .await
        .unwrap();
    assert_eq!(back.state, WriteIntentState::Done);
    assert_eq!(fake.lock().unwrap().posts, 2);
    save(&data, &back).unwrap();
    assert_eq!(list(&data, "p1").len(), 1);
    std::fs::remove_dir_all(&data).ok();
}
