//! 方法注册表与认证前 gate（P3-06 Action 3）。
//!
//! 「只注册已完成服务」的机器形状：**没有默认分支**。方法要么在注册表里
//! （handler 已实现），要么得到 `UnsupportedCapability`——「先接线再说」
//! 在这里骗不过类型：没注册的方法不存在，注册了就必须带 handler。
//!
//! gate 是 `dispatch` 的第一行：连接未认证时，除 `handshake` 外的一切
//! 请求（**mutation 尤其**）直接 `Unauthenticated`。这不是防骚扰文案，
//! 是「任何 unauthenticated mutation 阻塞整个后续阶段」这条红线的执行点。

use std::collections::HashMap;
use std::sync::Arc;

use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::{ErrorCode, RpcError};
use tokio::io::BufReader;
use tokio::net::unix::OwnedReadHalf;

use crate::codec;
use crate::subscriptions::Connection;

/// handler 的输入：请求帧 + 这条连接的共享上下文。
pub struct CallContext {
    pub services: Arc<ServiceSet>,
    pub connection: Arc<tokio::sync::Mutex<Connection>>,
}

/// dispatcher 对 handler 的类型别名（services.rs 的注册点用它标注闭包）。
pub type Handler = Arc<
    dyn Fn(Request, Arc<CallContext>) -> futures_util::future::BoxFuture<'static, Frame>
        + Send
        + Sync,
>;

/// 已完成服务的集合。启动序列只把**实现完整**的方法放进来。
#[derive(Default)]
pub struct ServiceSet {
    methods: HashMap<String, Handler>,
}

impl ServiceSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, method: &str, handler: Handler) {
        self.methods.insert(method.to_string(), handler);
    }

    pub fn is_registered(&self, method: &str) -> bool {
        self.methods.contains_key(method)
    }
}

/// 单帧分发：gate → 查表 → handler。返回的帧由连接循环写回；
/// `None` = 连接该关（codec 错误等不可恢复形状由连接循环自己产出，见 [`serve_connection`]）。
pub async fn dispatch(request: Request, context: Arc<CallContext>) -> Frame {
    let id = request.id.clone();
    let connection = context.connection.lock().await;
    if !connection.is_authenticated() && request.method != Connection::HANDSHAKE_METHOD {
        return failure(
            id,
            ErrorCode::Unauthenticated,
            "the connection must handshake before any other method",
        );
    }
    drop(connection);
    // clone 出 Arc 再调用：`get` 的借用与把 `context` move 进 handler 冲突。
    let Some(handler) = context.services.methods.get(&request.method).cloned() else {
        return failure(
            id,
            ErrorCode::UnsupportedCapability,
            "this host does not offer the requested method",
        );
    };
    handler(request, context).await
}

/// 连接循环：读帧→分发→写帧，直到对端关闭或 codec 拒绝。
/// codec 的三种拒绝（Partial/Overlong/Frame）都是**断连不降级**——
/// 半帧与超长帧永远不该被猜测处理。
pub async fn serve_connection(
    reader: BufReader<OwnedReadHalf>,
    mut writer: tokio::net::unix::OwnedWriteHalf,
    context: Arc<CallContext>,
) {
    let mut reader = reader;
    loop {
        let frame = match codec::read_frame(&mut reader).await {
            Ok(Some(frame)) => frame,
            Ok(None) => return,
            Err(_) => return,
        };
        let response = match frame {
            Frame::Request(request) => dispatch(request, Arc::clone(&context)).await,
            other => failure(
                String::new(),
                ErrorCode::InvalidArgument,
                &format!("the host expects requests, got {}", frame_kind(&other)),
            ),
        };
        if codec::write_frame(&mut writer, &response).await.is_err() {
            return;
        }
    }
}

fn frame_kind(frame: &Frame) -> &'static str {
    match frame {
        Frame::Event(_) => "an event",
        Frame::Failure(_) => "a failure",
        Frame::Success(_) => "a success",
        Frame::Request(_) => "a request",
    }
}

pub fn failure(id: String, code: ErrorCode, message: &str) -> Frame {
    Frame::Failure(aeroric_protocol::envelope::Failure {
        v: 1,
        id,
        ok: false,
        error: RpcError {
            code,
            message: message.to_string(),
            retryable: false,
            detail: None,
        },
        extra: Default::default(),
    })
}

// ── 内建服务（handshake + events.since）──────────────────────────────────

use aeroric_core::events::{since_response, EventLog};
use aeroric_protocol::envelope::Success;

use crate::auth::{parse_handshake, HostToken};

/// token 与事件日志的共享装配根。`accept` 侧（第四片）持它构造 CallContext。
/// 装配时可选的注入（Task P8-04 刀三）。
///
/// 这些字段分开的原因是它们的可用性互不相关：无头 CLI 有 `codex_rpc` 槽与降级账，
/// 但没有 `maintenance`（网络抓取与"替换正在运行的 app"是桌面宿主的事）；测试可以
/// 只给其中一个。`Default` 是"都没给"，不是"给了一个空壳"。
#[derive(Default)]
pub struct HostWiring {
    /// 维护面端口。给了才注册 `maintenance.*`（没给＝能力不存在，客户端拿到
    /// method-not-found，而不是一个会假装工作的空壳）。
    pub maintenance: Option<Arc<dyn aeroric_core::maintenance::MaintenanceHost>>,
    /// `codex app-server` 的进程句柄槽（`usage.read_usage_snapshot` 用它）。
    pub codex_rpc: Arc<parking_lot::Mutex<Option<aeroric_core::metrics::usage::CodexRpcClient>>>,
    /// 启动降级账（`diagnostics.list_startup_degradations` 读它）。
    pub degradations: Arc<aeroric_core::maintenance::StartupDegradationLog>,
    /// G43: executable/environment injection for search and filename enumeration.
    pub search: crate::search_services::SearchPrograms,
    /// File side-effect ports; default retains core's OS-trash policy.
    pub files: crate::file_access::FilePorts,
    /// Trusted composition only: external home for agent-config defaults.
    /// None uses the context home. Managed roots always remain context-owned;
    /// this must not rebind unrelated metrics/session discovery roots.
    pub agent_config_external_home: Option<std::path::PathBuf>,
}

pub struct HostServices {
    pub token: Arc<HostToken>,
    pub log: Arc<EventLog>,
    pub services: Arc<ServiceSet>,
    pub approvals: Arc<std::sync::Mutex<crate::services::HostApprovals>>,
    /// 宿主在 handshake 应答里声明的能力清单（ADR-038：`terminal.replay` 只在
    /// 装配了终端重放面的宿主上为真；`v == 1` 从不蕴含能力）。
    pub capabilities: Vec<aeroric_protocol::capability::Capability>,
}

impl HostServices {
    /// 只装配**实现完整**的方法：handshake、events.since、③a 的只读/幂等登记
    /// （project/task），以及 b1.2 传入 `service` 时的执行三方法（run/cancel/
    /// complete）。`context` 是启动序列的产物，所有 handler 共享同一份写租约。
    pub fn new(
        token: Arc<HostToken>,
        log: Arc<EventLog>,
        context: Arc<aeroric_core::context::CoreContext>,
    ) -> Arc<Self> {
        Self::assemble(
            token,
            log,
            context,
            None,
            None,
            None,
            None,
            None,
            crate::dispatcher::HostWiring::default(),
        )
    }

    /// b1.2/b1.3：执行服务全装配（serve 在 `TaskService::new` + `attach` 之后调用）。
    pub fn with_tasks(
        token: Arc<HostToken>,
        log: Arc<EventLog>,
        context: Arc<aeroric_core::context::CoreContext>,
        service: Arc<aeroric_core::tasks::TaskService>,
        families: Arc<dyn aeroric_core::tasks::AgentFamilies>,
        executor: Arc<crate::executor::CliPtyExecutor>,
        terminal: Arc<aeroric_core::terminal::pty::TerminalService>,
        registry: Arc<aeroric_core::runtime::registry::TaskRegistry>,
    ) -> Arc<Self> {
        Self::assemble(
            token,
            log,
            context,
            Some(service),
            Some(families),
            Some(executor),
            Some(terminal),
            Some(registry),
            crate::dispatcher::HostWiring::default(),
        )
    }

    /// 只读装配＋注入（P8-04 刀三：验收用；生产走 `with_tasks_and_wiring`）。
    pub fn with_wiring(
        token: Arc<HostToken>,
        log: Arc<EventLog>,
        context: Arc<aeroric_core::context::CoreContext>,
        wiring: HostWiring,
    ) -> Arc<Self> {
        Self::assemble(token, log, context, None, None, None, None, None, wiring)
    }

    /// 全装配＋可选的维护面注入（P8-04 刀三：cli serve 与桌面 embedded host 都走这里）。
    #[allow(clippy::too_many_arguments)]
    pub fn with_tasks_and_wiring(
        token: Arc<HostToken>,
        log: Arc<EventLog>,
        context: Arc<aeroric_core::context::CoreContext>,
        service: Arc<aeroric_core::tasks::TaskService>,
        families: Arc<dyn aeroric_core::tasks::AgentFamilies>,
        executor: Arc<crate::executor::CliPtyExecutor>,
        terminal: Arc<aeroric_core::terminal::pty::TerminalService>,
        registry: Arc<aeroric_core::runtime::registry::TaskRegistry>,
        wiring: HostWiring,
    ) -> Arc<Self> {
        Self::assemble(
            token,
            log,
            context,
            Some(service),
            Some(families),
            Some(executor),
            Some(terminal),
            Some(registry),
            wiring,
        )
    }

    fn assemble(
        token: Arc<HostToken>,
        log: Arc<EventLog>,
        context: Arc<aeroric_core::context::CoreContext>,
        service: Option<Arc<aeroric_core::tasks::TaskService>>,
        families: Option<Arc<dyn aeroric_core::tasks::AgentFamilies>>,
        executor: Option<Arc<crate::executor::CliPtyExecutor>>,
        terminal: Option<Arc<aeroric_core::terminal::pty::TerminalService>>,
        registry: Option<Arc<aeroric_core::runtime::registry::TaskRegistry>>,
        mut wiring: HostWiring,
    ) -> Arc<Self> {
        let mut set = ServiceSet::new();
        let mut capabilities: Vec<aeroric_protocol::capability::Capability> = Vec::new();
        crate::services::register_read_services(&mut set, Arc::clone(&context));
        let approvals = Arc::new(std::sync::Mutex::new(
            crate::services::HostApprovals::default(),
        ));
        crate::services::register_approval_services(&mut set, Arc::clone(&approvals));
        // 先克隆 context：下面的 if-let 分支里 register_session_services 会
        // 把它 move 走，而重放面还要一份。
        let replay_context = context.clone();
        crate::services::register_file_services(
            &mut set,
            Arc::clone(&context),
            wiring.files.clone(),
        );
        let notebook =
            crate::notebook_services::register_notebook_services(&mut set, Arc::clone(&context));
        crate::search_services::register_search_services(
            &mut set,
            Arc::clone(&context),
            notebook,
            wiring.search.clone(),
        );
        // P8-04 刀三：用量族与降级账总是注册（无头也有这两块能力）；维护面按注入
        // 注册——没有端口＝没有这个能力（见 `HostWiring::maintenance`）。
        // P8-05 切片九：设置面（读 / 整份写 / 单字段更新）。
        crate::settings_services::register_settings_services(
            &mut set,
            Arc::clone(&context),
            wiring.agent_config_external_home.take(),
        );
        crate::ide_services::register_ide_services(&mut set, Arc::clone(&context));
        // P8-05 切片七：文件服务第二批（格式化 / 本地历史 / 复制）。
        crate::file_services2::register_file_services_batch2(
            &mut set,
            Arc::clone(&context),
            Arc::new(crate::file_services2::RecordRootResolver),
        );
        crate::maintenance_services::register_usage_services(
            &mut set,
            Arc::clone(&context),
            Arc::clone(&wiring.codex_rpc),
        );
        crate::maintenance_services::register_diagnostics_services(
            &mut set,
            Arc::clone(&wiring.degradations),
        );
        crate::maintenance_services::register_if_hosted(
            &mut set,
            wiring.maintenance.take(),
            context.paths.data_dir(),
        );
        if let (Some(service), Some(families), Some(executor), Some(terminal), Some(registry)) =
            (service, families, executor, terminal, registry)
        {
            // 先克隆给重放面：register_task_services 会把 terminal/registry
            // move 走。
            let replay_terminal = Arc::clone(&terminal);
            let replay_registry = Arc::clone(&registry);
            crate::services::register_task_services(
                &mut set,
                Arc::clone(&service),
                Arc::clone(&context),
                executor,
                terminal,
                registry,
            );
            crate::services::register_session_services(&mut set, service, replay_context, families);
            crate::services::register_terminal_replay_services(
                &mut set,
                replay_terminal,
                replay_registry,
                context,
            );
            capabilities.push(aeroric_protocol::capability::Capability {
                name: "terminal.replay".to_string(),
                enabled: true,
                since: Some(1),
                note: None,
            });
        }
        {
            let token = Arc::clone(&token);
            let capabilities = capabilities.clone();
            set.register(
                crate::subscriptions::Connection::HANDSHAKE_METHOD,
                Arc::new(move |request, context| {
                    let token = Arc::clone(&token);
                    let capabilities = capabilities.clone();
                    Box::pin(async move { handshake(request, token, capabilities, context).await })
                }),
            );
        }
        {
            let log = Arc::clone(&log);
            set.register(
                "events.since",
                Arc::new(move |request, _context| {
                    let log = Arc::clone(&log);
                    Box::pin(async move { events_since(request, &log) })
                }),
            );
        }
        Arc::new(Self {
            token,
            log,
            services: Arc::new(set),
            approvals,
            capabilities,
        })
    }
}

async fn handshake(
    request: Request,
    token: Arc<HostToken>,
    capabilities: Vec<aeroric_protocol::capability::Capability>,
    context: Arc<CallContext>,
) -> Frame {
    let id = request.id.clone();
    let (presented, client_id) = match parse_handshake(&request.params) {
        Ok(parsed) => parsed,
        Err(error) => {
            return failure(id, ErrorCode::InvalidArgument, &error.to_string());
        }
    };
    if !token.verify(&presented) {
        return failure(
            id,
            ErrorCode::Unauthenticated,
            "presented token does not match this host's data directory",
        );
    }
    let mut connection = context.connection.lock().await;
    if !connection.mark_authenticated(client_id.clone()) {
        return failure(
            id,
            ErrorCode::Unauthenticated,
            "this connection is already authenticated; reconnect instead of re-handshaking",
        );
    }
    drop(connection);
    Frame::Success(Success {
        v: 1,
        id,
        ok: true,
        result: serde_json::json!({
            "clientId": client_id,
            // 能力清单随应答走（ADR-038）：v==1 从不蕴含能力，客户端按这份
            // 清单决定渲染重放面还是降级提示。
            "capabilities": capabilities,
        }),
        extra: Default::default(),
    })
}

fn events_since(request: Request, log: &EventLog) -> Frame {
    let id = request.id.clone();
    let after = request.params.get("after").and_then(|value| {
        value
            .as_str()
            .and_then(|text| text.parse::<u64>().ok())
            .or_else(|| value.as_u64())
    });
    let Some(after) = after else {
        return failure(
            id,
            ErrorCode::InvalidArgument,
            "events.since requires params.after (a decimal string or number)",
        );
    };
    Frame::Success(Success {
        v: 1,
        id,
        ok: true,
        result: since_response(log, after),
        extra: Default::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subscriptions::Connection;
    use aeroric_protocol::envelope::Request;
    use serde_json::json;

    fn request(id: &str, method: &str, params: &serde_json::Value) -> Request {
        Request {
            v: 1,
            id: id.to_string(),
            method: method.to_string(),
            params: params.clone(),
            idempotency_key: None,
            extra: Default::default(),
        }
    }

    /// 每测试一个唯一目录（pid 不够——cargo test 并行同进程共享 pid）；
    /// 目录留原地不清理：token 0600、内容随机，无敏感残留。
    fn harness() -> (Arc<HostServices>, Arc<CallContext>, std::path::PathBuf) {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir =
            std::path::PathBuf::from("/tmp").join(format!("ah-disp-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let token = Arc::new(HostToken::load_or_create(&dir.join("host.token")).expect("token"));
        let services = HostServices::new(
            token,
            Arc::new(EventLog::default()),
            Arc::new(aeroric_core::context::CoreContext::read_only(
                aeroric_core::paths::AppPaths::from_user_home("/nonexistent-test-home"),
            )),
        );
        let context = Arc::new(CallContext {
            services: Arc::clone(&services.services),
            connection: Arc::new(tokio::sync::Mutex::new(Connection::new())),
        });
        (services, context, dir)
    }

    fn real_token(dir: &std::path::Path) -> String {
        // 从文件读回真 token（HostToken 不暴露内容——掩码是设计）。
        std::fs::read_to_string(dir.join("host.token"))
            .expect("read")
            .trim()
            .to_string()
    }

    fn error_code(frame: &Frame) -> ErrorCode {
        match frame {
            Frame::Failure(f) => f.error.code.clone(),
            other => panic!("expected failure, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unauthenticated_non_handshake_is_refused_before_dispatch() {
        let (_services, context, _dir) = harness();
        // 红线：mutation 类方法在未认证时**不存在**——gate 先于查表。
        let frame = dispatch(
            request("1", "tasks.start", &json!({"task": "x"})),
            Arc::clone(&context),
        )
        .await;
        assert_eq!(error_code(&frame), ErrorCode::Unauthenticated);
        // 未注册的未知方法在未认证时同样是 Unauthenticated：gate 不泄露
        // 「这个方法存不存在」的信息。
        let frame = dispatch(request("2", "nope", &json!({})), context).await;
        assert_eq!(error_code(&frame), ErrorCode::Unauthenticated);
    }

    #[tokio::test]
    async fn handshake_rejects_wrong_token_and_accepts_the_real_one() {
        let (_services, context, dir) = harness();
        let frame = dispatch(
            request(
                "1",
                "handshake",
                &json!({"token": "guess", "clientId": "c"}),
            ),
            Arc::clone(&context),
        )
        .await;
        assert_eq!(error_code(&frame), ErrorCode::Unauthenticated);
        assert!(!context.connection.lock().await.is_authenticated());

        let frame = dispatch(
            request(
                "2",
                "handshake",
                &json!({"token": real_token(&dir), "clientId": "c1"}),
            ),
            Arc::clone(&context),
        )
        .await;
        assert!(matches!(frame, Frame::Success(s) if s.result["clientId"] == "c1"));
        assert!(context.connection.lock().await.is_authenticated());

        // 再握手＝拒绝（重连走新连接，不在旧连接上换身份）。
        let frame = dispatch(
            request(
                "3",
                "handshake",
                &json!({"token": real_token(&dir), "clientId": "c2"}),
            ),
            context,
        )
        .await;
        assert_eq!(error_code(&frame), ErrorCode::Unauthenticated);
    }

    #[tokio::test]
    async fn authenticated_unknown_method_gets_unsupported_capability() {
        let (_services, context, dir) = harness();
        dispatch(
            request(
                "1",
                "handshake",
                &json!({"token": real_token(&dir), "clientId": "c"}),
            ),
            Arc::clone(&context),
        )
        .await;
        let frame = dispatch(request("2", "tasks.start", &json!({})), context).await;
        assert_eq!(error_code(&frame), ErrorCode::UnsupportedCapability);
    }

    #[tokio::test]
    async fn events_since_backfills_after_handshake() {
        let (services, context, dir) = harness();
        services.log.append("task-status", json!({"a": 1}));
        services.log.append("task-status", json!({"a": 2}));
        dispatch(
            request(
                "1",
                "handshake",
                &json!({"token": real_token(&dir), "clientId": "c"}),
            ),
            Arc::clone(&context),
        )
        .await;
        let frame = dispatch(
            request("2", "events.since", &json!({"after": "1"})),
            Arc::clone(&context),
        )
        .await;
        match frame {
            Frame::Success(success) => {
                assert_eq!(success.result["latestSeq"], 2);
                assert_eq!(success.result["events"].as_array().expect("arr").len(), 1);
                assert_eq!(success.result["events"][0]["seq"], 2);
            }
            other => panic!("expected success, got {other:?}"),
        }
        // after 缺失/形状错 = InvalidArgument，不是崩。
        let frame = dispatch(request("3", "events.since", &json!({})), context).await;
        assert_eq!(error_code(&frame), ErrorCode::InvalidArgument);
    }
}
