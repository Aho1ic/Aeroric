//! dsh 续跑/恢复与模型目录判定（P3-05 Action 5 第二文件；补记二百六十九）。
//! 断言面＝带 `session_id` 的 launch（续跑＝带记录会话 id 的新 start，ADR-029
//! 的 handoff 形状）、provider 档案的 reasoning-effort 丢弃、目录 provider
//! 解析与 effort 白名单门。全部经 fake server，零真实模型调用。

mod common;

use common::dsh_fake::{session_created, FakeDshServer, MockPorts};

use aeroric_core::agents::dsh_host::executor::{run_dsh_task_core, DshLaunch};
use serde_json::json;

fn mk_launch(prompt: &str) -> DshLaunch {
    DshLaunch {
        generation: 3,
        task_id: "t1".to_string(),
        agent: "dsh".to_string(),
        project_path: "/proj".to_string(),
        prompt: prompt.to_string(),
        session_id: None,
        workspace_id: None,
        agent_preset: None,
        prompt_mode: None,
        selected_model: None,
        reasoning_effort: None,
        permission_mode: None,
        images: None,
        client_time_zone: None,
    }
}

fn catalog_with_reasoning(efforts: &[&str]) -> serde_json::Value {
    let efforts: Vec<_> = efforts
        .iter()
        .map(|id| json!({ "id": id, "name": id.to_string() }))
        .collect();
    json!({
        "default": { "provider": "official", "model": "m-basic" },
        "routableProviders": ["official"],
        "groups": [{
            "id": "official",
            "name": "Official",
            "models": [{
                "id": "m-basic",
                "name": "Basic",
                "reasoning": { "efforts": efforts }
            }]
        }]
    })
}

#[tokio::test]
async fn resume_launch_reuses_session_and_keeps_history() {
    let server = FakeDshServer::start().await;
    server.respond("session/create", session_created("s9"));
    server.respond(
        "session/modelCatalog",
        FakeDshServer::ok(common::dsh_fake::catalog()),
    );
    server.respond(
        "session/prompt",
        FakeDshServer::ok(json!({ "accepted": true })),
    );
    let ports = MockPorts::new(&server.url.clone());
    let mut launch = mk_launch("continue please");
    launch.session_id = Some("s9".to_string());

    run_dsh_task_core(&ports, launch, ports.sink())
        .await
        .expect("resumed launch settles");

    // wire 带原 sessionId（续跑不是新会话）。
    let created = server.payload_of("session/create", 0);
    assert_eq!(created["args"]["request"]["sessionId"], "s9");
    // 恢复路径绝不截断终端历史（兜底终端要留前一段）。
    assert!(
        !ports.events().iter().any(|e| e.starts_with("truncate:")),
        "带 session_id 的恢复不得清空历史：{:?}",
        ports.events()
    );
    // 活跃表/归属/登记都落在被恢复的会话 id 上。
    assert!(ports
        .events()
        .iter()
        .any(|e| e.starts_with("active:t1:s9:")));
    assert!(ports.events().iter().any(|e| e.starts_with("remember:s9:")));
}

#[tokio::test]
async fn provider_profile_drops_reasoning_effort_entirely() {
    let server = FakeDshServer::start().await;
    server.respond("session/create", session_created("s1"));
    server.respond(
        "session/modelCatalog",
        FakeDshServer::ok(catalog_with_reasoning(&["low", "high"])),
    );
    server.respond(
        "session/prompt",
        FakeDshServer::ok(json!({ "accepted": true })),
    );
    let ports = MockPorts::new(&server.url.clone());
    let mut launch = mk_launch("hi");
    launch.agent = "custom-provider".to_string();
    launch.reasoning_effort = Some("high".to_string());

    run_dsh_task_core(&ports, launch, ports.sink())
        .await
        .expect("provider profile launch settles");

    // agent!="dsh"：强度在门口就被丢弃，selected_model 也没给——
    // selectModel 根本不该发生。
    assert_eq!(server.count("session/selectModel"), 0);
}

#[tokio::test]
async fn model_selection_resolves_provider_from_catalog() {
    let server = FakeDshServer::start().await;
    server.respond("session/create", session_created("s1"));
    server.respond(
        "session/modelCatalog",
        FakeDshServer::ok(common::dsh_fake::catalog()),
    );
    server.respond(
        "session/prompt",
        FakeDshServer::ok(json!({ "accepted": true })),
    );
    let ports = MockPorts::new(&server.url.clone());
    let mut launch = mk_launch("hi");
    launch.selected_model = Some("m-basic".to_string());

    run_dsh_task_core(&ports, launch, ports.sink())
        .await
        .expect("model selection settles");

    let request = server.payload_of("session/selectModel", 0);
    let selection = &request["args"]["request"];
    assert_eq!(selection["sessionId"], "s1");
    assert_eq!(selection["model"], "m-basic");
    assert_eq!(selection["provider"], "official", "provider 从目录组解析");
    assert!(selection.get("reasoningEffort").is_none());
}

#[tokio::test]
async fn unknown_model_falls_back_to_current_provider() {
    let server = FakeDshServer::start().await;
    server.respond("session/create", session_created("s1"));
    server.respond(
        "session/modelCatalog",
        FakeDshServer::ok(common::dsh_fake::catalog()),
    );
    server.respond(
        "session/prompt",
        FakeDshServer::ok(json!({ "accepted": true })),
    );
    let ports = MockPorts::new(&server.url.clone());
    let mut launch = mk_launch("hi");
    launch.selected_model = Some("ghost-model".to_string());

    run_dsh_task_core(&ports, launch, ports.sink())
        .await
        .expect("unknown model still selects via current provider");

    let request = server.payload_of("session/selectModel", 0);
    let selection = &request["args"]["request"];
    assert_eq!(selection["model"], "ghost-model");
    assert_eq!(
        selection["provider"], "official",
        "目录查不到时回退 current.provider"
    );
}

#[tokio::test]
async fn effort_survives_only_when_catalog_lists_it() {
    // 目录列了 low：透传。
    let server = FakeDshServer::start().await;
    server.respond("session/create", session_created("s1"));
    server.respond(
        "session/modelCatalog",
        FakeDshServer::ok(catalog_with_reasoning(&["low", "medium"])),
    );
    server.respond(
        "session/prompt",
        FakeDshServer::ok(json!({ "accepted": true })),
    );
    let ports = MockPorts::new(&server.url.clone());
    let mut launch = mk_launch("hi");
    launch.reasoning_effort = Some("low".to_string());
    run_dsh_task_core(&ports, launch, ports.sink())
        .await
        .expect("supported effort launch settles");
    let request = server.payload_of("session/selectModel", 0);
    assert_eq!(request["args"]["request"]["reasoningEffort"], "low");

    // 目录没列 xhigh：即使前端回传也置空（selectModel 不带该字段）。
    let server = FakeDshServer::start().await;
    server.respond("session/create", session_created("s1"));
    server.respond(
        "session/modelCatalog",
        FakeDshServer::ok(catalog_with_reasoning(&["low", "medium"])),
    );
    server.respond(
        "session/prompt",
        FakeDshServer::ok(json!({ "accepted": true })),
    );
    let ports = MockPorts::new(&server.url.clone());
    let mut launch = mk_launch("hi");
    launch.reasoning_effort = Some("xhigh".to_string());
    run_dsh_task_core(&ports, launch, ports.sink())
        .await
        .expect("unsupported effort still settles");
    let request = server.payload_of("session/selectModel", 0);
    assert!(
        request["args"]["request"].get("reasoningEffort").is_none(),
        "白名单外的强度必须被目录判定抹掉：{request}"
    );
}
