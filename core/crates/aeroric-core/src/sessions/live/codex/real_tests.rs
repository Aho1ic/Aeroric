//! Opt-in (2026-11-08): Aeroric's Codex server and driver against a real
//! `codex app-server` whose model is a scripted fake. The unit tests replay
//! the wire as we believe Codex sends it; this checks that it does. Needs a
//! native Codex binary and runs only when asked:
//!
//! ```text
//! AERORIC_REAL_CODEX=<codex binary> cargo test -p aeroric-core --lib real_codex -- --ignored
//! ```
//!
//! The fake model plays both agents: the parent spawns one subagent and waits
//! for it; the subagent runs `touch subagent-probe.txt` asking for
//! escalation. The test declines with a reason and follows it to whoever can
//! hear it, then reads the parent's rollout the way the history page does.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::driver::{CodexSession, CodexSessionSpec};
use super::fake::Sink;
use super::server::{process_launcher, CodexServer, ProcessSpec};
use crate::sessions::codex::parse_codex_session;
use crate::sessions::live::update::{SessionUpdateKind, ToolStatus};
use crate::sessions::live::{Decision, LiveStatusSink, SessionDriver, SharedLog};
use crate::sessions::SessionContent;
use crate::tasks::TaskStatus;

const CHILD: &str = "CHILD-MARKER-7f3a";
const REASON: &str = "Do not create files.";

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs AERORIC_REAL_CODEX=<codex binary>"]
async fn real_codex_v1_subagent_is_asked_here_and_hears_the_reason() {
    subagent_round(false).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs AERORIC_REAL_CODEX=<codex binary>"]
async fn real_codex_v2_subagent_is_asked_here_and_its_reason_reaches_the_session() {
    subagent_round(true).await;
}

async fn subagent_round(v2: bool) {
    let Some(codex) = std::env::var_os("AERORIC_REAL_CODEX") else {
        eprintln!("skipped: AERORIC_REAL_CODEX is not set");
        return;
    };
    let root = std::env::temp_dir().join(format!(
        "aeroric-real-codex-{}-{}",
        if v2 { "v2" } else { "v1" },
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let (home, project) = (root.join("home"), root.join("project"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&project).unwrap();

    let model = Arc::new(Model::default());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(serve(listener, Arc::clone(&model)));
    std::fs::write(
        home.join("config.toml"),
        format!(
            r#"model = "gpt-5.1-codex"
model_provider = "fake"

[model_providers.fake]
name = "fake"
base_url = "http://127.0.0.1:{port}/v1"
wire_api = "responses"
env_key = "AERORIC_FAKE_MODEL_KEY"
request_max_retries = 0
stream_max_retries = 0
supports_websockets = false

[features]
multi_agent_v2 = {v2}
"#
        ),
    )
    .unwrap();

    let launch = ProcessSpec {
        program: codex.to_string_lossy().into_owned(),
        args: Vec::new(),
        env: vec![
            ("CODEX_HOME".into(), home.display().to_string()),
            ("AERORIC_FAKE_MODEL_KEY".into(), "fake".into()),
        ],
        unset_env: Vec::new(),
        cwd: Some(project.display().to_string()),
    };
    let sink = Arc::new(Sink::default());
    let session = CodexSession::start(
        CodexSessionSpec {
            task_id: "real".into(),
            project_id: "p".into(),
            generation: 1,
            agent: "codex".into(),
            cwd: project.display().to_string(),
            extra_dirs: Vec::new(),
            // 「每次询问」: untrusted, workspace-write sandbox (C 批).
            permission: "ask".into(),
            model: None,
            effort: None,
            thread_id: None,
        },
        CodexServer::new(process_launcher(launch)),
        Arc::clone(&sink) as Arc<dyn LiveStatusSink>,
        SharedLog::new("real"),
        &tokio::runtime::Handle::current(),
    );
    session.prompt("go".into()).unwrap();

    let asked = Arc::clone(&session);
    wait("the subagent's approval", move || {
        !asked.snapshot().pending.is_empty()
    })
    .await;
    let pending = session.snapshot().pending[0].clone();
    // v2 names the child by its path; v1 sends no name at all.
    let label = if v2 { "probe" } else { "子代理" };
    assert_eq!(pending.subagent.as_deref(), Some(label));
    assert!(pending.summary.contains("touch subagent-probe.txt"));
    session
        .respond(
            &pending.id,
            Decision::Deny {
                reason: Some(REASON.into()),
            },
        )
        .unwrap();
    let done = Arc::clone(&sink);
    wait("the session's turn to end", move || {
        done.last_status() == Some(TaskStatus::Done)
    })
    .await;

    // Codex refuses input to a v2 child, so its reason goes to the session.
    // A v1 child hears it itself, unless its turn already ended (the fake
    // model answers at once), and then the session hears it.
    let heard =
        |hearer: &str, text: &str| {
            model.requests.lock().iter().any(|(role, request)| {
                role == hearer && request["input"].to_string().contains(text)
            })
        };
    if v2 {
        assert!(
            heard(
                "parent",
                &format!("用户拒绝了子代理 probe 的这一步：{REASON}")
            ),
            "the session heard the reason"
        );
    } else {
        assert!(
            heard("child", &format!("用户拒绝了这一步：{REASON}"))
                || heard("parent", &format!("用户拒绝了子代理的这一步：{REASON}")),
            "the subagent or the session heard the reason"
        );
    }
    // v1's child may end just after the session's turn does.
    let title = if v2 { "子代理 probe" } else { "子代理" };
    let finished = (ToolStatus::Completed, format!("{title} 完成\n"));
    for _ in 0..250 {
        if spawn_lines(&sink).last() == Some(&finished) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    assert_eq!(
        spawn_lines(&sink).last(),
        Some(&finished),
        "card lines {:?}; updates {:?}",
        spawn_lines(&sink),
        sink.update_types()
    );
    assert_eq!(
        spawn_lines(&sink).first().map(|l| l.1.clone()),
        Some(format!("{title} 开始\n"))
    );
    assert!(!project.join("subagent-probe.txt").exists());

    // After the turn the page shows the rollout instead of the live rows.
    let rollout = sink
        .sessions
        .lock()
        .first()
        .and_then(|(_, path)| path.clone())
        .expect("the thread's rollout path");
    let wanted = if v2 {
        "子代理 probe 开始\n子代理 probe 完成\n回报：child: done"
    } else {
        "开始"
    };
    let mut card = String::new();
    for _ in 0..250 {
        card = spawn_card(Path::new(&rollout));
        if card.contains(wanted) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    assert!(
        card.starts_with("子代理") && card.contains(wanted),
        "spawn card in {rollout}: {card:?}"
    );

    session.close().unwrap();
    let _ = std::fs::remove_dir_all(&root);
}

fn spawn_lines(sink: &Sink) -> Vec<(ToolStatus, String)> {
    sink.updates
        .lock()
        .iter()
        .filter_map(|u| match &u.kind {
            SessionUpdateKind::ToolCallUpdate {
                id,
                status,
                output_delta: Some(line),
                ..
            } if id == "parent-spawn" => Some((*status, line.clone())),
            _ => None,
        })
        .collect()
}

/// The spawn card's result as the history page builds it.
fn spawn_card(rollout: &Path) -> String {
    let text = std::fs::read_to_string(rollout).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    parse_codex_session(&lines)
        .iter()
        .flat_map(|m| m.content.iter())
        .find_map(|c| match c {
            SessionContent::ToolResult { id, output } if id == "parent-spawn" => {
                Some(output.clone())
            }
            _ => None,
        })
        .unwrap_or_default()
}

async fn wait(what: &str, condition: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while !condition() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The scripted model behind the Responses API. A request is the child's
/// when the marker is its task, not just inside the parent's spawn call.
#[derive(Default)]
struct Model {
    requests: Mutex<Vec<(String, Value)>>,
}

impl Model {
    fn answer(&self, request: &Value) -> Vec<Value> {
        let input = request["input"].as_array().cloned().unwrap_or_default();
        let child = input.iter().any(|item| {
            !matches!(
                item["type"].as_str(),
                Some("function_call" | "custom_tool_call")
            ) && item.to_string().contains(CHILD)
        });
        let answered = |call: &str| {
            input.iter().any(|item| {
                item["call_id"] == call
                    && matches!(
                        item["type"].as_str(),
                        Some("function_call_output" | "custom_tool_call_output")
                    )
            })
        };
        let tools = tool_index(&request["tools"]);
        let id = {
            let mut requests = self.requests.lock();
            requests.push((
                if child { "child" } else { "parent" }.into(),
                request.clone(),
            ));
            format!("resp-{}", requests.len())
        };
        if child {
            if answered("child-exec") {
                return message(&id, "child: done");
            }
            let (namespace, _) = tools.get("exec_command").cloned().unwrap_or_default();
            let args = json!({"cmd": "touch subagent-probe.txt",
                "sandbox_permissions": "require_escalated", "justification": "probe: create the file"});
            return call(&id, "exec_command", args, "child-exec", namespace);
        }
        if !answered("parent-spawn") {
            let (namespace, schema) = tools.get("spawn_agent").cloned().unwrap_or_default();
            let mut args = json!({"message": format!("{CHILD} run touch")});
            if schema.pointer("/parameters/properties/task_name").is_some() {
                args["task_name"] = json!("probe");
            }
            return call(&id, "spawn_agent", args, "parent-spawn", namespace);
        }
        // The user's reason, steered in, cuts the wait short: wait again.
        let heard = input
            .iter()
            .any(|item| item["role"] == "user" && item.to_string().contains("用户拒绝了"));
        let wait = if !answered("parent-wait") {
            Some("parent-wait")
        } else if heard && !answered("parent-wait-2") {
            Some("parent-wait-2")
        } else {
            None
        };
        if let Some(wait) = wait {
            let (namespace, schema) = tools.get("wait_agent").cloned().unwrap_or_default();
            let mut args = json!({"timeout_ms": 30000});
            // v1 waits on agent ids, which the spawn's output names.
            if let Some(agent) = spawned_agent(&input) {
                if schema.pointer("/parameters/properties/targets").is_some() {
                    args["targets"] = json!([agent]);
                }
            }
            return call(&id, "wait_agent", args, wait, namespace);
        }
        message(&id, "parent: done")
    }
}

fn spawned_agent(input: &[Value]) -> Option<String> {
    let output = input
        .iter()
        .find(|item| item["call_id"] == "parent-spawn" && item["type"] == "function_call_output")?;
    let output: Value = serde_json::from_str(output["output"].as_str()?).ok()?;
    output["agent_id"].as_str().map(str::to_string)
}

/// Tool name → (its namespace, its schema), across plain and namespace tools.
fn tool_index(tools: &Value) -> HashMap<String, (Option<String>, Value)> {
    let mut index = HashMap::new();
    for tool in tools.as_array().into_iter().flatten() {
        if tool["type"] == "namespace" {
            let namespace = tool["name"].as_str().map(str::to_string);
            for inner in tool["tools"].as_array().into_iter().flatten() {
                if let Some(name) = inner["name"].as_str() {
                    index.insert(name.to_string(), (namespace.clone(), inner.clone()));
                }
            }
        } else if let Some(name) = tool["name"].as_str() {
            index.insert(name.to_string(), (None, tool.clone()));
        }
    }
    index
}

fn completed(id: &str) -> Value {
    json!({"type": "response.completed", "response": {"id": id, "usage": {
        "input_tokens": 1, "input_tokens_details": null, "output_tokens": 1,
        "output_tokens_details": null, "total_tokens": 2}}})
}

fn call(id: &str, name: &str, args: Value, call_id: &str, namespace: Option<String>) -> Vec<Value> {
    let mut item = json!({"type": "function_call", "name": name,
        "arguments": args.to_string(), "call_id": call_id});
    if let Some(namespace) = namespace {
        item["namespace"] = json!(namespace);
    }
    vec![
        json!({"type": "response.created", "response": {"id": id}}),
        json!({"type": "response.output_item.done", "item": item}),
        completed(id),
    ]
}

fn message(id: &str, text: &str) -> Vec<Value> {
    vec![
        json!({"type": "response.created", "response": {"id": id}}),
        json!({"type": "response.output_item.done", "item": {"type": "message",
            "role": "assistant", "id": format!("msg-{id}"),
            "content": [{"type": "output_text", "text": text}]}}),
        completed(id),
    ]
}

async fn serve(listener: TcpListener, model: Arc<Model>) {
    while let Ok((mut socket, _)) = listener.accept().await {
        let model = Arc::clone(&model);
        tokio::spawn(async move {
            let Some((head, body)) = read_request(&mut socket).await else {
                return;
            };
            let reply = if head.starts_with("POST") {
                let request: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                let stream: String = model
                    .answer(&request)
                    .iter()
                    .map(|event| {
                        format!(
                            "event: {}\ndata: {event}\n\n",
                            event["type"].as_str().unwrap_or("")
                        )
                    })
                    .collect();
                http("text/event-stream", &stream)
            } else {
                http("application/json", r#"{"object":"list","data":[]}"#)
            };
            let _ = socket.write_all(reply.as_bytes()).await;
            let _ = socket.shutdown().await;
        });
    }
}

fn http(kind: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

async fn read_request(socket: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    let split = loop {
        let read = socket.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(at) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..split]).into_owned();
    let length = head
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    let mut body = buffer[split..].to_vec();
    while body.len() < length {
        let read = socket.read(&mut chunk).await.ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    Some((head, body))
}
