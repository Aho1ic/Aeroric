//! AP7 — 适配器契约与四组高级能力的宿主级验收。
//!
//! 断言的是**线上字节**:真起宿主(临时数据根 + UDS),真握手,真调
//! `session.control` / `adapters.capabilities`,而适配器探测结果来自
//! `aeroric_core::agents::dsh_host::client` 里那份**钉在源码 commit 上**的方法表。
//!
//! 覆盖 PRD 的 AC-AP7-1(如实广告)、AC-AP7-2(会话控制闭环 + 反例)、
//! AC-AP7-7(入口存在且未广告项被服务端拒绝)。

mod host_harness;

use host_harness::Harness;
use serde_json::{json, Value};

use aeroric_core::adapters::CapabilityProbe;
use aeroric_host::adapter_services::AdapterInjection;
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;

/// 探测:方法与版本都取自 core 内钉住的 DSH 清单,而不是测试里手写的字符串。
fn dsh_probe() -> CapabilityProbe {
    use aeroric_core::agents::dsh_host::client;
    let mut probe = CapabilityProbe::new("dsh")
        .version(client::PACKAGE_VERSION)
        .native_prompt(true)
        .attachments(true);
    // 上游**确实**有的:方法表里存在就加。
    let methods = [
        "session.fork",
        "session.plan",
        "subagents.list",
        "goals.get",
        "session.share",
    ];
    for method in methods {
        if client::REMOTE_METHODS.contains(&method) {
            probe = probe.with(method);
        }
    }
    probe
}

fn injection() -> AdapterInjection {
    AdapterInjection {
        probe: dsh_probe(),
        queued_permission_mode: aeroric_protocol::task::PermissionMode::Ask,
        reachable_adapters: 1,
    }
}

async fn connect() -> (
    Harness,
    tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>,
    tokio::net::unix::OwnedWriteHalf,
) {
    let harness =
        Harness::start_with_wiring("ap7-adapters", |_| aeroric_host::dispatcher::HostWiring {
            adapters: Some(std::sync::Arc::new(injection())),
            ..Default::default()
        })
        .await;
    let (mut reader, mut writer) = harness.connect().await;
    Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    (harness, reader, writer)
}

fn success_result(frame: &Frame) -> &Value {
    match frame {
        Frame::Success(success) => &success.result,
        other => panic!("expected Success, got {other:?}"),
    }
}

fn error_code(frame: &Frame) -> Option<ErrorCode> {
    match frame {
        Frame::Failure(failure) => Some(failure.error.code.clone()),
        _ => None,
    }
}

#[tokio::test]
async fn handshake_advertises_only_probed_capabilities_with_evidence() {
    let (_harness, mut reader, mut writer) = connect().await;

    // 再握手一次拿清单(第一次在 connect 里已经用掉)。
    let (mut reader2, mut writer2) = _harness.connect().await;
    let frame = Harness::handshake(&mut reader2, &mut writer2, &_harness.token, "probe").await;
    let capabilities = success_result(&frame)
        .get("capabilities")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let by_name: std::collections::HashMap<&str, &Value> = capabilities
        .iter()
        .filter_map(|capability| {
            capability
                .get("name")
                .and_then(Value::as_str)
                .map(|name| (name, capability))
        })
        .collect();

    let controls = by_name["session-controls"];
    assert_eq!(controls["enabled"], json!(true));
    let note = controls["note"].as_str().unwrap();
    // 证据里必须有适配器名、版本,以及**具体**被广告的方法名。
    assert!(note.contains("dsh"), "{note}");
    assert!(
        note.contains(aeroric_core::agents::dsh_host::client::PACKAGE_VERSION),
        "{note}"
    );
    assert!(note.contains("session.fork"), "{note}");
    // plan/share 在真实方法表里不存在,因此不得出现在证据里。
    assert!(!note.contains("session.plan"), "{note}");
    assert!(!note.contains("session.share"), "{note}");

    assert_eq!(by_name["agent-work-graph"]["enabled"], json!(true));
    assert_eq!(by_name["workflows"]["enabled"], json!(true));
    assert_eq!(by_name["external-protocols"]["enabled"], json!(true));
    // 新增清单不得挤掉或重复已有条目。(`terminal.replay` 只在宿主真的装配了
    // 终端/注册表时才广告,这个 harness 没有,所以这里断言的是**不重复**。)
    assert_eq!(
        capabilities.len(),
        by_name.len(),
        "capability names must be unique: {capabilities:?}"
    );

    let _ = (&mut reader, &mut writer);
}

#[tokio::test]
async fn a_host_without_a_probed_adapter_registers_nothing_and_says_why() {
    // 没有注入:方法表里根本没有 adapters.capabilities / session.control。
    let harness = Harness::start("ap7-no-adapters").await;
    let (mut reader, mut writer) = harness.connect().await;
    let frame = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    let capabilities = success_result(&frame)["capabilities"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        !capabilities
            .iter()
            .any(|capability| capability["name"] == "session-controls"),
        "a host with no adapter injection must not advertise session controls"
    );

    let frame = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request(
            "c-1",
            "session.control",
            json!({"capability": "fork", "sessionId": "s"}),
        ),
    )
    .await;
    // 方法根本没注册 → 与「方法存在但未广告」同一个公开语义:`UnsupportedCapability`。
    // 客户端因此只有一种分支要处理,而这两种情况都不会假装成功。
    assert_eq!(
        error_code(&frame),
        Some(ErrorCode::UnsupportedCapability),
        "no adapter plane must not fake a success: {frame:?}"
    );
}

#[tokio::test]
async fn adapters_capabilities_reports_the_probe_verbatim() {
    let (_harness, mut reader, mut writer) = connect().await;
    let frame = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request("a-1", "adapters.capabilities", json!({})),
    )
    .await;
    let result = success_result(&frame);
    assert_eq!(result["adapter"], json!("dsh"));
    assert_eq!(
        result["version"],
        json!(aeroric_core::agents::dsh_host::client::PACKAGE_VERSION)
    );
    let agent = &result["agent"];
    assert_eq!(agent["nativePrompt"], json!(true));
    assert_eq!(
        agent["fork"],
        json!(true),
        "session.fork is in the pinned table"
    );
    assert_eq!(agent["planMode"], json!(false), "session.plan is not");
    assert_eq!(agent["share"], json!(false), "session.share is not");
    // 探不到的项要能解释自己。
    let unsupported = agent["extra"]["unsupported"].as_array().unwrap();
    assert!(unsupported.iter().any(|value| value == "session.plan"));
}

#[tokio::test]
async fn an_advertised_capability_completes_the_round_trip() {
    let (_harness, mut reader, mut writer) = connect().await;
    let frame = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request(
            "a-2",
            "session.control",
            json!({"capability": "fork", "sessionId": "s-1"}),
        ),
    )
    .await;
    let result = success_result(&frame);
    assert_eq!(result["capability"], json!("fork"));
    assert_eq!(result["method"], json!("session.fork"));
}

#[tokio::test]
async fn an_unadvertised_capability_is_refused_by_the_host_not_hidden_by_ui() {
    let (_harness, mut reader, mut writer) = connect().await;
    for capability in ["plan", "compact", "revert", "share"] {
        let frame = Harness::call(
            &mut reader,
            &mut writer,
            &Harness::request(
                "a-3",
                "session.control",
                json!({"capability": capability, "sessionId": "s-1"}),
            ),
        )
        .await;
        assert_eq!(
            error_code(&frame),
            Some(ErrorCode::UnsupportedCapability),
            "{capability} must be refused by the server: {frame:?}"
        );
    }
}

#[tokio::test]
async fn a_queue_permission_mismatch_is_refused_even_for_an_advertised_capability() {
    let (_harness, mut reader, mut writer) = connect().await;
    // 入队是 Ask(注入值),消费时 UI 已经切到 FullAccess。
    let frame = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request(
            "a-4",
            "session.control",
            json!({
                "capability": "fork",
                "sessionId": "s-1",
                "currentPermissionMode": "full_access",
            }),
        ),
    )
    .await;
    assert_eq!(error_code(&frame), Some(ErrorCode::InvalidArgument));

    // 提高权限的命令同样被拒。
    let frame = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request(
            "a-5",
            "session.control",
            json!({
                "capability": "fork",
                "sessionId": "s-1",
                "requestedPermissionMode": "full_access",
            }),
        ),
    )
    .await;
    assert_eq!(error_code(&frame), Some(ErrorCode::InvalidArgument));

    // 与入队值一致是允许的。
    let frame = Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request(
            "a-6",
            "session.control",
            json!({
                "capability": "fork",
                "sessionId": "s-1",
                "currentPermissionMode": "ask",
            }),
        ),
    )
    .await;
    assert!(matches!(frame, Frame::Success(_)));
}
