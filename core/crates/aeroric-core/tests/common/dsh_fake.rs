//! 本地 fake DSH server ＋ `DshHostPorts` 的测试实现（P3-05 Action 5：
//! 「提供本地 fake DSH server，不调用真实付费模型」，补记二百六十九）。
//!
//! 形状：
//! - [`FakeDshServer`]：`127.0.0.1:0` 上的最小 HTTP/1.1。每个 `POST /api/<method>`
//!   按 `responses` 表返回完整信封（`{result:{ok,value}}`），未配置的返回
//!   `{result:{ok:true,value:{}}}`；`connection: close` 逐请求断连，reqwest 会
//!   重连，测试因此可以按序断言 `requests`。
//! - [`MockPorts`]：16 端口的记录型实现——`log` 按序记事件，`statuses` 收
//!   `report_status`，终端 echo 收进 `echoed`；`ensure_webui` 返回 fake server
//!   URL，`start_stream` 只记不真开流（stream 消费环在宿主，core 测试面不碰 WS）。

use std::collections::HashMap;
use std::sync::Arc;

use aeroric_core::agents::dsh_host::ports::{
    BoxFuture, DshHostPorts, DshSessionRegistration, DshStreamRequest, OutputSink,
};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

pub struct FakeDshServer {
    pub url: String,
    /// 按到达序记录的 `(method, 完整请求体)`。
    pub requests: Arc<Mutex<Vec<(String, Value)>>>,
    /// method -> 完整响应信封 JSON；缺省 `{result:{ok:true,value:{}}}`。
    pub responses: Arc<Mutex<HashMap<String, Value>>>,
    shutdown: Option<oneshot::Sender<()>>,
    _worker: tokio::task::JoinHandle<()>,
}

impl FakeDshServer {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("local addr").port();
        let requests: Arc<Mutex<Vec<(String, Value)>>> = Arc::new(Mutex::new(Vec::new()));
        let responses: Arc<Mutex<HashMap<String, Value>>> = Arc::new(Mutex::new(HashMap::new()));
        let (tx, mut rx) = oneshot::channel::<()>();

        let loop_requests = Arc::clone(&requests);
        let loop_responses = Arc::clone(&responses);
        let worker = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut rx => break,
                    accepted = listener.accept() => {
                        let Ok((mut socket, _)) = accepted else { break };
                        let requests = Arc::clone(&loop_requests);
                        let responses = Arc::clone(&loop_responses);
                        tokio::spawn(async move {
                            use tokio::io::{AsyncReadExt, AsyncWriteExt};
                            let mut buf: Vec<u8> = Vec::new();
                            let mut byte = [0u8; 1];
                            // 读到头（\r\n\r\n），再按 content-length 读 body。
                            loop {
                                match socket.read(&mut byte).await {
                                    Ok(0) | Err(_) => break,
                                    Ok(_) => {
                                        buf.push(byte[0]);
                                        if buf.ends_with(b"\r\n\r\n") {
                                            break;
                                        }
                                    }
                                }
                            }
                            let Ok(head) = std::str::from_utf8(&buf) else { return };
                            let Some(path) = head.lines().next().and_then(|l| l.split(' ').nth(1))
                            else {
                                return;
                            };
                            let Some(method) = path.strip_prefix("/api/") else { return };
                            let method = method.to_string();
                            let length = head
                                .lines()
                                .find_map(|l| {
                                    let l = l.to_ascii_lowercase();
                                    l.strip_prefix("content-length:")
                                        .and_then(|v| v.trim().parse::<usize>().ok())
                                })
                                .unwrap_or(0);
                            let mut body = vec![0u8; length];
                            if length > 0 && socket.read_exact(&mut body).await.is_err() {
                                return;
                            }
                            let payload: Value =
                                serde_json::from_slice(&body).unwrap_or(Value::Null);
                            requests.lock().push((method.clone(), payload));
                            let envelope = responses
                                .lock()
                                .get(&method)
                                .cloned()
                                .unwrap_or_else(|| json!({ "result": { "ok": true, "value": {} } }));
                            let text = envelope.to_string();
                            let response = format!(
                                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                                text.len(),
                                text
                            );
                            let _ = socket.write_all(response.as_bytes()).await;
                        });
                    }
                }
            }
        });

        Self {
            url: format!("http://127.0.0.1:{port}"),
            requests,
            responses,
            shutdown: Some(tx),
            _worker: worker,
        }
    }

    pub fn respond(&self, method: &str, envelope: Value) {
        self.responses.lock().insert(method.to_string(), envelope);
    }

    pub fn ok(value: Value) -> Value {
        json!({ "result": { "ok": true, "value": value } })
    }

    /// 取第 n 次（按到达序）某 method 的请求 payload（`{type,rpcId,method,payload}` 信封里的 payload）。
    pub fn payload_of(&self, method: &str, nth: usize) -> Value {
        self.requests
            .lock()
            .iter()
            .filter(|(m, _)| m == method)
            .nth(nth)
            .unwrap_or_else(|| {
                panic!(
                    "no {method} request #{nth}; seen: {:?}",
                    self.requests
                        .lock()
                        .iter()
                        .map(|(m, _)| m.clone())
                        .collect::<Vec<_>>()
                )
            })
            .1
            .get("payload")
            .cloned()
            .unwrap_or(Value::Null)
    }

    pub fn count(&self, method: &str) -> usize {
        self.requests
            .lock()
            .iter()
            .filter(|(m, _)| m == method)
            .count()
    }
}

impl Drop for FakeDshServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

/// `DshHostPorts` 的记录型实现。
pub struct MockPorts {
    pub url: String,
    pub log: Mutex<Vec<String>>,
    pub statuses: Mutex<Vec<(String, String, Option<String>)>>,
    pub echoed: Arc<Mutex<Vec<String>>>,
    locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl MockPorts {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            log: Mutex::new(Vec::new()),
            statuses: Mutex::new(Vec::new()),
            echoed: Arc::new(Mutex::new(Vec::new())),
            locks: Mutex::new(HashMap::new()),
        }
    }

    pub fn note(&self, event: &str) {
        self.log.lock().push(event.to_string());
    }

    pub fn events(&self) -> Vec<String> {
        self.log.lock().clone()
    }

    pub fn sink(&self) -> OutputSink {
        let echoed = Arc::clone(&self.echoed);
        Arc::new(move |text: String| echoed.lock().push(text))
    }
}

impl DshHostPorts for MockPorts {
    fn lifecycle_lock(&self, task_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.locks
            .lock()
            .entry(task_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    fn clear_prerun_state(&self, task_id: &str) {
        self.note(&format!("clear:{task_id}"));
    }

    fn truncate_history_for_new_session(&self, task_id: &str) {
        self.note(&format!("truncate:{task_id}"));
    }

    fn ensure_webui(&self, agent: &str) -> BoxFuture<'_, Result<String, String>> {
        let url = self.url.clone();
        let agent = agent.to_string();
        Box::pin(async move {
            // 与宿主端口的失败语义对齐：agent=="broken" 模拟启动失败。
            if agent == "broken" {
                return Err("DSH Web URL is unavailable".to_string());
            }
            Ok(url)
        })
    }

    fn register_run(&self, task_id: &str, generation: u64) {
        self.note(&format!("run:{task_id}:{generation}"));
    }

    fn register_active_session(
        &self,
        task_id: &str,
        session_id: &str,
        base_url: &str,
        _sink: OutputSink,
    ) {
        self.note(&format!("active:{task_id}:{session_id}:{base_url}"));
    }

    fn remember_session_host(&self, session_id: &str, base_url: &str) {
        self.note(&format!("remember:{session_id}:{base_url}"));
    }

    fn session_path(&self, _agent: &str, _project_path: &str, session_id: &str) -> Option<String> {
        Some(format!("/fake/sessions/{session_id}.jsonl"))
    }

    fn register_session(&self, facts: &DshSessionRegistration) {
        self.note(&format!(
            "session:{}:{}:{}:{}",
            facts.task_id,
            facts.generation,
            facts.session_path,
            facts.resolved_preset.as_deref().unwrap_or("-")
        ));
    }

    fn sync_terminal_cols(&self, task_id: &str) {
        self.note(&format!("cols:{task_id}"));
    }

    fn start_stream(&self, request: DshStreamRequest) -> BoxFuture<'_, Result<(), String>> {
        self.note(&format!("stream:{}", request.task_id));
        Box::pin(async { Ok(()) })
    }

    fn expect_internal_command(&self, task_id: &str, kind: &str) {
        self.note(&format!("expect:{task_id}:{kind}"));
    }

    fn clear_internal_command(&self, task_id: &str) {
        self.note(&format!("clear-ic:{task_id}"));
    }

    fn send_terminal(&self, _task_id: &str, _sink: &OutputSink, text: &str) {
        self.echoed.lock().push(text.to_string());
    }

    fn report_status(&self, task_id: &str, status: &str, reason: Option<&str>) {
        self.statuses.lock().push((
            task_id.to_string(),
            status.to_string(),
            reason.map(str::to_string),
        ));
    }

    fn retire_runs(&self, task_id: &str) {
        self.note(&format!("retire:{task_id}"));
    }
}

/// 一个够用的模型目录（`current` 是部署默认，groups 含 `m-basic`）。
pub fn catalog() -> Value {
    json!({
        "default": { "provider": "official", "model": "m-basic" },
        "routableProviders": ["official"],
        "groups": [{
            "id": "official",
            "name": "Official",
            "models": [{ "id": "m-basic", "name": "Basic" }]
        }]
    })
}

pub fn session_created(id: &str) -> Value {
    FakeDshServer::ok(json!({ "sessionId": id }))
}
