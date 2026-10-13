//! 推送派发器:把宿主 `EventLog` 的任务事件翻译成「要不要推、推给谁」,
//! 并在需要时把请求交给宿主注入的 [`PushTransport`] 端口。
//!
//! 为什么是观察者而不是轮询:宿主的事件出口是 `EventLog`(单一出口、seq 与环
//! 同临界区),所以推送与「手机重连补发」看到的是**同一批事件、同一个 seq**。
//! 另开一条轮询/订阅会拿到不同的顺序,出现「通知说完成、快照还没完成」这种
//! 自相矛盾的状态。
//!
//! **为什么走端口而不是直接发 HTTP**:`EventObserver::observed` 在 append 的
//! 调用者线程上被调用(Runtime 的结算任务),在那里做网络等待会把任务结算拖住。
//! 所以 core 只做纯决策(不阻塞、可单测),真正的 IO 由宿主端口在自己的执行器上
//! 完成。core 里**没有** reqwest/hyper 依赖,这条边界由源码扫描钉住。
//!
//! 边界(§D AP6 + Q7/Q10):
//! * 派发器**不**决定重试。发送端回执里的 `retryAfterMs` 被记录并用于抑制接下来
//!   的若干次(见 [`Suppressor`]),不做后台重试队列 —— 通知不是可靠消息,
//!   假装它是会让行为不可预测。
//! * 载荷由 [`decide`](crate::remote::push::decide) 生成,本模块**没有**写正文的地方。
//! * 没有登记设备、未启用推送、发送端失败 → 静默跳过(**不**回落到别的通道),
//!   但每次跳过都有计数,便于诊断回答「为什么没收通知」。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::{json, Value};

use crate::events::{EventObserver, LoggedEvent};
use crate::remote::push::{
    clamp_retry_after, decide, envelope_for, interpret_response, PushBindings, PushDecision,
    PushDiagnostic, PushOutcome, PushSenderClient,
};

/// 把一次已决策的推送交给网络。宿主实现它(真实 HTTP);测试实现它(收集器)。
///
/// 只收「已经决定好要推的东西」(`PushDecision` + 目标设备 id)—— 端口实现没有
/// 机会改动载荷内容,载荷最小化的边界在 [`decide`]。
pub trait PushTransport: Send + Sync {
    fn send(
        &self,
        client: &PushSenderClient,
        device_ids: &[String],
        decision: &PushDecision,
    ) -> Result<PushOutcome, PushDiagnostic>;

    /// 注册/注销一台设备的 APNs token(`push.register` 的远端半边)。
    ///
    /// 与 [`PushTransport::send`] 同一理由做成同步:它在 `push_register` 的
    /// async 上下文里被调用,但真正的等待由实现用 `block_in_place` 移出。
    fn bind(
        &self,
        client: &PushSenderClient,
        params: &Value,
        now_ms: i64,
        nonce: &str,
    ) -> Result<Value, String>;
}

/// 未装配网络时的端口:任何请求都报「宿主没有推送出口」。
///
/// 这是**显式拒绝**而不是空成功:一个静默返回 `accepted: 0` 的实现会让
/// 「推送为什么没用」无从查起。
#[derive(Default)]
pub struct NoTransport;

impl PushTransport for NoTransport {
    fn send(
        &self,
        _client: &PushSenderClient,
        _device_ids: &[String],
        _decision: &PushDecision,
    ) -> Result<PushOutcome, PushDiagnostic> {
        Err(PushDiagnostic {
            code: remote_protocol::PushErrorCode::Unavailable,
            message: "this host has no push transport wired".to_string(),
            retryable: false,
        })
    }

    fn bind(
        &self,
        _client: &PushSenderClient,
        _params: &Value,
        _now_ms: i64,
        _nonce: &str,
    ) -> Result<Value, String> {
        Err("this host has no push transport wired".to_string())
    }
}

/// 退避抑制器:发送端报了 `retryAfterMs` 之后,在这段时间内不再推。
///
/// APNs 用 429/503 表达的正是「你先别发了」;宿主照做而不是自己排重试,是
/// 「通知不承诺必达」在代码里的落地方式。
#[derive(Default)]
pub struct Suppressor {
    until: Mutex<Option<Instant>>,
    suppressed: AtomicU64,
}

impl Suppressor {
    pub fn is_suppressed(&self) -> bool {
        self.until
            .lock()
            .is_some_and(|until| Instant::now() < until)
    }

    pub fn suppress_for(&self, suggested_ms: Option<u64>) {
        if let Some(window) = clamp_retry_after(suggested_ms) {
            *self.until.lock() = Some(Instant::now() + window);
        }
    }

    fn note_suppressed(&self) {
        self.suppressed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn suppressed_count(&self) -> u64 {
        self.suppressed.load(Ordering::Relaxed)
    }

    pub fn clear(&self) {
        *self.until.lock() = None;
    }
}

/// 运行统计(诊断用;`push.status` 会回给设置面)。
#[derive(Default)]
pub struct PushStats {
    decisions: AtomicU64,
    attempts: AtomicU64,
    accepted: AtomicU64,
    invalidated: AtomicU64,
    failures: AtomicU64,
    skipped_no_devices: AtomicU64,
}

impl PushStats {
    pub fn snapshot(&self) -> Value {
        json!({
            "decisions": self.decisions.load(Ordering::Relaxed),
            "attempts": self.attempts.load(Ordering::Relaxed),
            "accepted": self.accepted.load(Ordering::Relaxed),
            "invalidatedDevices": self.invalidated.load(Ordering::Relaxed),
            "failures": self.failures.load(Ordering::Relaxed),
            "skippedNoDevices": self.skipped_no_devices.load(Ordering::Relaxed),
        })
    }
}

/// 观察者实现:接到 `EventLog` 的每一次 append。
pub struct PushDispatcher {
    /// `None` = 推送未启用(或未配置发送端);此时 `observed` 直接返回。
    client: Mutex<Option<PushSenderClient>>,
    bindings: Arc<PushBindings>,
    transport: Arc<dyn PushTransport>,
    suppressor: Suppressor,
    stats: PushStats,
    /// nonce 序号:同一进程内的派发不会复用 nonce(复用会被发送端当重放拒掉)。
    nonce_counter: AtomicU64,
    /// 时间源可注入(测试要精确控制签名时间戳)。
    clock_ms: Arc<dyn Fn() -> i64 + Send + Sync>,
}

impl PushDispatcher {
    pub fn new(
        bindings: Arc<PushBindings>,
        transport: Arc<dyn PushTransport>,
        clock_ms: Arc<dyn Fn() -> i64 + Send + Sync>,
    ) -> Self {
        Self {
            client: Mutex::new(None),
            bindings,
            transport,
            suppressor: Suppressor::default(),
            stats: PushStats::default(),
            nonce_counter: AtomicU64::new(0),
            clock_ms,
        }
    }

    /// 启用/更新发送端配置。`None` 关闭推送(已登记的绑定保留,便于重新启用)。
    pub fn set_client(&self, client: Option<PushSenderClient>) {
        let disabled = client.is_none();
        *self.client.lock() = client;
        if disabled {
            // 关掉时清抑制窗口:下次启用不该还被上一个窗口挡着。
            self.suppressor.clear();
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.client.lock().is_some()
    }

    pub fn stats(&self) -> &PushStats {
        &self.stats
    }

    pub fn suppressor(&self) -> &Suppressor {
        &self.suppressor
    }

    /// 手动推一条(设置面的「验证链路」)。走同一条最小化载荷,**不**因为是
    /// 测试就放宽内容边界。
    pub fn push_manual(
        &self,
        kind: remote_protocol::PushKind,
        task_hint: &str,
    ) -> Result<Value, String> {
        let client = self.client.lock().clone();
        let Some(client) = client else {
            return Err(
                "push is not configured; set a sender URL and credential first".to_string(),
            );
        };
        let devices = self.bindings.active_device_ids();
        if devices.is_empty() {
            return Err("no paired device has registered for notifications yet".to_string());
        }
        // 手动推送没有真实事件 seq:`eventSeq: 0` 是「无 watermark」哨兵,所以用 1,
        // 并靠 collapse key 区分。手机侧只把它当「有变化」的提示。
        let decision = PushDecision {
            kind,
            envelope: envelope_for(task_hint, kind, 1),
        };
        self.stats.decisions.fetch_add(1, Ordering::Relaxed);
        self.attempt(&client, &devices, &decision)
    }

    /// 一次派发的完整决策 + 执行(测试直接调用,不必真的 append 事件)。
    pub fn dispatch_status(&self, payload: &Value, seq: u64) {
        let Some(decision) = decide(payload, seq) else {
            return;
        };
        self.stats.decisions.fetch_add(1, Ordering::Relaxed);
        let client = self.client.lock().clone();
        let Some(client) = client else {
            return;
        };
        if self.suppressor.is_suppressed() {
            self.suppressor.note_suppressed();
            return;
        }
        let devices = self.bindings.active_device_ids();
        if devices.is_empty() {
            self.stats
                .skipped_no_devices
                .fetch_add(1, Ordering::Relaxed);
            return;
        }
        let _ = self.attempt(&client, &devices, &decision);
    }

    fn attempt(
        &self,
        client: &PushSenderClient,
        devices: &[String],
        decision: &PushDecision,
    ) -> Result<Value, String> {
        self.stats.attempts.fetch_add(1, Ordering::Relaxed);
        match self.transport.send(client, devices, decision) {
            Ok(outcome) => {
                self.absorb_outcome(&outcome);
                Ok(json!({
                    "accepted": outcome.accepted,
                    "invalidatedDeviceIds": outcome.invalidated_device_ids,
                    "retryAfterMs": outcome.retry_after_ms,
                }))
            }
            Err(diagnostic) => {
                self.stats.failures.fetch_add(1, Ordering::Relaxed);
                eprintln!(
                    "[push] delivery failed ({:?}, retryable={}): {}",
                    diagnostic.code, diagnostic.retryable, diagnostic.message
                );
                if diagnostic.retryable {
                    // 可重试失败但发送端没说等多久 → 用默认退避,否则紧接着的
                    // 任务事件会立刻再打一次 APNs。
                    self.suppressor
                        .suppress_for(Some(crate::remote::push::DEFAULT_BACKOFF_MS));
                }
                Err(diagnostic.message)
            }
        }
    }

    fn absorb_outcome(&self, outcome: &PushOutcome) {
        self.stats
            .accepted
            .fetch_add(outcome.accepted as u64, Ordering::Relaxed);
        for device_id in &outcome.invalidated_device_ids {
            // 发送端(最终是 APNs)说这个 token 没用了:本机绑定表同步失效,
            // 之后不再把它算作目标。
            if self.bindings.mark_invalidated(device_id) {
                self.stats.invalidated.fetch_add(1, Ordering::Relaxed);
                eprintln!("[push] device {device_id} push token invalidated by APNs");
            }
        }
        if outcome.retry_after_ms.is_some() {
            self.suppressor.suppress_for(outcome.retry_after_ms);
        }
    }

    /// 供宿主端口用的 nonce。
    pub fn next_nonce(&self) -> String {
        let counter = self.nonce_counter.fetch_add(1, Ordering::Relaxed);
        format!("h{counter:x}{:x}", (self.clock_ms)())
    }

    /// 当前推送配置(未启用时 `None`)。
    pub fn client(&self) -> Option<PushSenderClient> {
        self.client.lock().clone()
    }

    pub fn bindings(&self) -> &Arc<PushBindings> {
        &self.bindings
    }
}

impl EventObserver for PushDispatcher {
    fn observed(&self, event: &LoggedEvent) {
        // 只认任务状态族。其它事件(传输进度、rag 进度、审批确认)与通知无关。
        if event.event != "task-status" {
            return;
        }
        self.dispatch_status(&event.data, event.seq);
    }
}

/// 解析发送端回执的薄包装(宿主端口用;核心语义在 [`interpret_response`])。
pub fn parse_sender_response(status: u16, body: &str) -> Result<Value, String> {
    match interpret_response(status, body) {
        Ok(outcome) => Ok(json!({
            "accepted": outcome.accepted,
            "invalidatedDeviceIds": outcome.invalidated_device_ids,
            "retryAfterMs": outcome.retry_after_ms,
        })),
        Err(diagnostic) => Err(diagnostic.message),
    }
}

/// 当前毫秒时间戳(UTC epoch 毫秒)。与发送端 `auth::now_ms` 同口径:
/// 一侧按本地时钟签、另一侧按本地时钟验,两侧都只依赖系统时钟的绝对偏差,
/// 不依赖任何共享的启动时刻。
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

/// 诊断用的抑制窗口时长(`push.status` 回显;`None` = 未抑制)。
pub fn suppression_window(suppressor: &Suppressor) -> Option<Duration> {
    let until = *suppressor.until.lock();
    until.map(|until| until.saturating_duration_since(Instant::now()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::push::{PushSenderConfig, MAX_RETRY_AFTER_MS};
    use remote_protocol::{PushErrorCode, PushKind};
    use std::sync::Mutex as StdMutex;

    /// 记录每次发送的收集器(不联网)。
    #[derive(Default)]
    struct Recorder {
        sent: StdMutex<Vec<(Vec<String>, PushDecision)>>,
        binds: StdMutex<Vec<Value>>,
        outcome: StdMutex<Option<Result<PushOutcome, PushDiagnostic>>>,
    }

    impl PushTransport for Recorder {
        fn send(
            &self,
            _client: &PushSenderClient,
            device_ids: &[String],
            decision: &PushDecision,
        ) -> Result<PushOutcome, PushDiagnostic> {
            self.sent
                .lock()
                .expect("recorder")
                .push((device_ids.to_vec(), decision.clone()));
            self.outcome
                .lock()
                .expect("recorder")
                .clone()
                .unwrap_or(Ok(PushOutcome {
                    accepted: device_ids.len(),
                    invalidated_device_ids: Vec::new(),
                    retry_after_ms: None,
                }))
        }

        fn bind(
            &self,
            _client: &PushSenderClient,
            params: &Value,
            _now_ms: i64,
            _nonce: &str,
        ) -> Result<Value, String> {
            self.binds.lock().expect("recorder").push(params.clone());
            Ok(json!({ "bound": true }))
        }
    }

    fn client() -> PushSenderClient {
        PushSenderClient::new(PushSenderConfig {
            url: "https://push.example.com".to_string(),
            user: "u1".to_string(),
            secret: "s1".to_string(),
        })
        .expect("client")
    }

    fn dispatcher(recorder: Arc<Recorder>) -> PushDispatcher {
        PushDispatcher::new(
            Arc::new(PushBindings::new()),
            recorder,
            Arc::new(|| 1_700_000_000_000),
        )
    }

    fn sent(recorder: &Recorder) -> Vec<(Vec<String>, PushDecision)> {
        recorder.sent.lock().expect("recorder").clone()
    }

    #[test]
    fn disabled_dispatcher_counts_decisions_but_never_attempts() {
        let recorder = Arc::new(Recorder::default());
        let dispatcher = dispatcher(recorder.clone());
        assert!(!dispatcher.is_enabled());
        dispatcher.dispatch_status(&json!({"task_id": "t1", "status": "done"}), 1);
        // 「决策」计数与发送端无关:它回答的是「有多少事件本来该提醒用户」,
        // 这是诊断「我为什么没收到通知」时要看的第一位数。真正发出去的次数
        // 记在 `attempts`,这里必须是 0。
        assert_eq!(dispatcher.stats().snapshot()["decisions"], json!(1));
        assert_eq!(dispatcher.stats().snapshot()["attempts"], json!(0));
        assert!(sent(&recorder).is_empty());
    }

    #[test]
    fn only_done_failed_and_approval_states_are_dispatched() {
        let recorder = Arc::new(Recorder::default());
        let dispatcher = dispatcher(recorder.clone());
        dispatcher.set_client(Some(client()));
        dispatcher.bindings().register("d1", "ios", 0);

        dispatcher.dispatch_status(&json!({"task_id": "t1", "status": "running"}), 1);
        assert_eq!(dispatcher.stats().snapshot()["decisions"], json!(0));

        for (status, expected) in [
            ("done", PushKind::TaskDone),
            ("failed", PushKind::TaskFailed),
            ("input_required", PushKind::ApprovalPending),
        ] {
            dispatcher.dispatch_status(&json!({"task_id": "t1", "status": status}), 2);
            assert_eq!(
                sent(&recorder).last().expect("sent").1.kind,
                expected,
                "status {status}"
            );
        }
        assert_eq!(dispatcher.stats().snapshot()["decisions"], json!(3));
    }

    #[test]
    fn no_active_device_means_no_request_at_all() {
        let recorder = Arc::new(Recorder::default());
        let dispatcher = dispatcher(recorder.clone());
        dispatcher.set_client(Some(client()));
        dispatcher.bindings().register("d1", "ios", 0);
        dispatcher.bindings().mark_invalidated("d1");

        dispatcher.dispatch_status(&json!({"task_id": "t1", "status": "done"}), 1);
        let stats = dispatcher.stats().snapshot();
        assert_eq!(stats["decisions"], json!(1));
        assert_eq!(stats["skippedNoDevices"], json!(1));
        assert!(sent(&recorder).is_empty());
    }

    #[test]
    fn invalidation_reported_by_the_sender_updates_the_local_table_once() {
        let recorder = Arc::new(Recorder::default());
        *recorder.outcome.lock().expect("recorder") = Some(Ok(PushOutcome {
            accepted: 0,
            invalidated_device_ids: vec!["d1".to_string()],
            retry_after_ms: None,
        }));
        let dispatcher = dispatcher(recorder.clone());
        dispatcher.set_client(Some(client()));
        dispatcher.bindings().register("d1", "ios", 0);

        dispatcher.dispatch_status(&json!({"task_id": "t1", "status": "done"}), 1);
        assert!(dispatcher.bindings().active_device_ids().is_empty());
        assert_eq!(
            dispatcher.stats().snapshot()["invalidatedDevices"],
            json!(1)
        );

        // 幂等:第二次事件里 d1 已不是目标,不再计数。
        dispatcher.dispatch_status(&json!({"task_id": "t2", "status": "done"}), 2);
        assert_eq!(
            dispatcher.stats().snapshot()["invalidatedDevices"],
            json!(1)
        );
    }

    #[test]
    fn a_backoff_window_suppresses_further_pushes_without_queueing() {
        let recorder = Arc::new(Recorder::default());
        *recorder.outcome.lock().expect("recorder") = Some(Ok(PushOutcome {
            accepted: 0,
            invalidated_device_ids: Vec::new(),
            retry_after_ms: Some(60_000),
        }));
        let dispatcher = dispatcher(recorder.clone());
        dispatcher.set_client(Some(client()));
        dispatcher.bindings().register("d1", "ios", 0);

        dispatcher.dispatch_status(&json!({"task_id": "t1", "status": "done"}), 1);
        assert_eq!(sent(&recorder).len(), 1);
        assert!(dispatcher.suppressor().is_suppressed());

        // 抑制窗口内的后续事件:**不排队**,只计数。
        dispatcher.dispatch_status(&json!({"task_id": "t2", "status": "failed"}), 2);
        dispatcher.dispatch_status(&json!({"task_id": "t3", "status": "done"}), 3);
        assert_eq!(sent(&recorder).len(), 1, "no retry queue");
        assert_eq!(dispatcher.suppressor().suppressed_count(), 2);
    }

    #[test]
    fn retryable_failures_set_a_backoff_and_permanent_ones_do_not() {
        let recorder = Arc::new(Recorder::default());
        let dispatcher = dispatcher(recorder.clone());
        dispatcher.set_client(Some(client()));
        dispatcher.bindings().register("d1", "ios", 0);

        *recorder.outcome.lock().expect("recorder") = Some(Err(PushDiagnostic {
            code: PushErrorCode::Unauthorized,
            message: "unknown user or bad signature".to_string(),
            retryable: false,
        }));
        dispatcher.dispatch_status(&json!({"task_id": "t1", "status": "done"}), 1);
        assert_eq!(dispatcher.stats().snapshot()["failures"], json!(1));
        assert!(
            !dispatcher.suppressor().is_suppressed(),
            "a bad credential is not something waiting will fix"
        );

        *recorder.outcome.lock().expect("recorder") = Some(Err(PushDiagnostic {
            code: PushErrorCode::Upstream,
            message: "APNs 503".to_string(),
            retryable: true,
        }));
        dispatcher.dispatch_status(&json!({"task_id": "t2", "status": "done"}), 2);
        assert!(
            dispatcher.suppressor().is_suppressed(),
            "a retryable failure without a sender hint must still back off"
        );
        let window = suppression_window(dispatcher.suppressor()).expect("window");
        assert!(window <= Duration::from_millis(crate::remote::push::DEFAULT_BACKOFF_MS));
    }

    #[test]
    fn observer_only_reacts_to_the_task_status_topic() {
        let recorder = Arc::new(Recorder::default());
        let dispatcher = dispatcher(recorder.clone());
        dispatcher.set_client(Some(client()));
        dispatcher.bindings().register("d1", "ios", 0);

        for event in [
            "dbx.transfer_progress",
            "notebook.rag_index_progress",
            "task-session",
        ] {
            dispatcher.observed(&LoggedEvent {
                seq: 1,
                event: event.to_string(),
                data: json!({"task_id": "t1", "status": "done"}),
            });
        }
        assert!(sent(&recorder).is_empty());

        dispatcher.observed(&LoggedEvent {
            seq: 5,
            event: "task-status".to_string(),
            data: json!({"task_id": "t1", "status": "done"}),
        });
        let sent = sent(&recorder);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, vec!["d1".to_string()]);
        assert_eq!(sent[0].1.envelope.event_seq, 5);
    }

    #[test]
    fn manual_push_refuses_clearly_instead_of_silently_succeeding() {
        let recorder = Arc::new(Recorder::default());
        let dispatcher = dispatcher(recorder.clone());
        let error = dispatcher
            .push_manual(PushKind::TaskDone, "t1")
            .unwrap_err();
        assert!(error.contains("not configured"), "{error}");

        dispatcher.set_client(Some(client()));
        let error = dispatcher
            .push_manual(PushKind::TaskDone, "t1")
            .unwrap_err();
        assert!(error.contains("no paired device"), "{error}");

        dispatcher.bindings().register("d1", "ios", 0);
        let value = dispatcher
            .push_manual(PushKind::TaskDone, "t1")
            .expect("sent");
        assert_eq!(value["accepted"], json!(1));
        assert_eq!(sent(&recorder).len(), 1);
    }

    #[test]
    fn manual_push_surfaces_sender_errors_instead_of_reporting_success() {
        let recorder = Arc::new(Recorder::default());
        *recorder.outcome.lock().expect("recorder") = Some(Err(PushDiagnostic {
            code: PushErrorCode::Unauthorized,
            message: "unknown user or bad signature".to_string(),
            retryable: false,
        }));
        let dispatcher = dispatcher(recorder);
        dispatcher.set_client(Some(client()));
        dispatcher.bindings().register("d1", "ios", 0);
        let error = dispatcher
            .push_manual(PushKind::TaskDone, "t1")
            .unwrap_err();
        assert!(error.contains("bad signature"), "{error}");
    }

    #[test]
    fn nonces_are_unique_per_dispatch() {
        let dispatcher = dispatcher(Arc::new(Recorder::default()));
        let first = dispatcher.next_nonce();
        let second = dispatcher.next_nonce();
        assert_ne!(first, second);
        assert!(first.starts_with('h') && second.starts_with('h'));
    }

    #[test]
    fn no_transport_refuses_explicitly_rather_than_acking_zero() {
        let dispatcher = PushDispatcher::new(
            Arc::new(PushBindings::new()),
            Arc::new(NoTransport),
            Arc::new(|| 0),
        );
        dispatcher.set_client(Some(client()));
        dispatcher.bindings().register("d1", "ios", 0);
        let error = dispatcher
            .push_manual(PushKind::TaskDone, "t1")
            .unwrap_err();
        assert!(error.contains("no push transport"), "{error}");
    }

    #[test]
    fn sender_response_parsing_matches_the_wire_contract() {
        let ok = parse_sender_response(
            200,
            &json!({"v":1,"accepted":2,"invalidatedDeviceIds":["d9"]}).to_string(),
        )
        .expect("ok");
        assert_eq!(ok["accepted"], json!(2));
        assert_eq!(ok["invalidatedDeviceIds"], json!(["d9"]));

        let error = parse_sender_response(
            429,
            &json!({"code":"rate_limited","message":"slow down","retryable":true}).to_string(),
        )
        .unwrap_err();
        assert!(error.contains("slow down"), "{error}");
    }

    #[test]
    fn clearing_the_client_drops_a_stale_backoff_window() {
        let dispatcher = dispatcher(Arc::new(Recorder::default()));
        dispatcher.set_client(Some(client()));
        dispatcher.suppressor().suppress_for(Some(600_000));
        assert!(dispatcher.suppressor().is_suppressed());
        assert!(suppression_window(dispatcher.suppressor()).is_some());
        dispatcher.set_client(None);
        assert!(!dispatcher.suppressor().is_suppressed());
        assert!(suppression_window(dispatcher.suppressor()).is_none());
    }

    #[test]
    fn backoff_windows_are_clamped_from_the_sender_hint() {
        let suppressor = Suppressor::default();
        suppressor.suppress_for(Some(0));
        assert!(
            suppressor.is_suppressed(),
            "tiny values are raised to a floor"
        );
        suppressor.clear();
        suppressor.suppress_for(None);
        assert!(!suppressor.is_suppressed(), "None is not a backoff request");
        assert!(
            clamp_retry_after(Some(u64::MAX)).expect("clamped")
                <= Duration::from_millis(MAX_RETRY_AFTER_MS)
        );
    }
}
