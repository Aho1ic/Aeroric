//! P6-03 验收：DAP 协议（fake LSP/DAP 的协议形状覆盖；server crash/restart
//! 生命周期判定）。
use aeroric_core::ide::dap;

#[test]
fn debug_definition_wire_matches_legacy_lowercase_and_launch_attach_defaults() {
    use serde_json::json;
    // Direct external expectations, not a self-roundtrip that hides a wrong enum.
    for (kind, value) in [
        (dap::DebugConfigType::Node, "node"),
        (dap::DebugConfigType::Python, "python"),
    ] {
        assert_eq!(serde_json::to_value(kind).unwrap(), json!(value));
        let config: dap::DebugConfig = serde_json::from_value(json!({
            "id":"synthetic", "name":" 中文 ", "type":value,
            "program":"missing.py", "cwd":"."
        }))
        .unwrap();
        assert_eq!(config.config_type, kind);
        assert_eq!(config.request, dap::DebugRequestType::Launch);
        assert_eq!(config.attach_host, "127.0.0.1");
        assert_eq!(config.attach_port, None);
        assert!(config.args.is_empty() && config.env.is_empty() && config.breakpoints.is_empty());
        assert_eq!(
            serde_json::to_value(config).unwrap(),
            json!({
                "id":"synthetic", "name":" 中文 ", "type":value, "request":"launch",
                "program":"missing.py", "cwd":".", "attachHost":"127.0.0.1",
                "args":[], "env":{}, "breakpoints":[]
            })
        );
    }
    for wrong in ["Node", "Python", "shell", "debug"] {
        assert!(serde_json::from_value::<dap::DebugConfigType>(json!(wrong)).is_err());
    }
}

#[test]
fn debug_session_snapshot_roundtrip() {
    let snapshot = dap::DebugSessionSnapshot {
        debug_id: "d1".to_string(),
        config_id: "c1".to_string(),
        name: "test".to_string(),
        program: "/repo/main".to_string(),
        cwd: "/repo".to_string(),
        status: dap::DebugSessionStatus::Paused,
        output: "hit breakpoint".to_string(),
        paused_reason: Some("breakpoint".to_string()),
        call_stack: vec![dap::DebugCallFrame {
            function_name: "main".to_string(),
            file: "src/main.rs".to_string(),
            line: 42,
            column: 5,
            frame_id: Some("1".to_string()),
        }],
        scopes: vec![dap::DebugVariableScope {
            name: "Local".to_string(),
            variables: vec![dap::DebugVariable {
                name: "x".to_string(),
                value: "1".to_string(),
                type_name: Some("i32".to_string()),
                object_id: None,
                has_children: false,
            }],
        }],
        exit_code: None,
        started_at: 1000,
        finished_at: None,
    };
    // 序列化 roundtrip（wire 形状 camelCase）。
    let json = serde_json::to_string(&snapshot).unwrap();
    assert!(json.contains("\"debugId\""));
    assert!(json.contains("\"callStack\""));
    assert!(json.contains("\"pausedReason\""));
    let parsed: dap::DebugSessionSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, snapshot);
    // server crash/restart 生命周期：Paused → Stop 合法；Exited 后无会话。
    assert!(dap::is_action_allowed(dap::DebugAction::Stop, true, true));
    assert!(
        dap::is_action_allowed(dap::DebugAction::Start, false, false),
        "crash 后可 restart"
    );
}

#[test]
fn debug_config_document_roundtrip_and_validation() {
    use std::collections::BTreeMap;
    let doc = dap::DebugConfigDocument {
        version: dap::DEBUG_CONFIG_VERSION,
        configs: vec![dap::DebugConfig {
            id: "d1".to_string(),
            name: "debug main".to_string(),
            config_type: dap::DebugConfigType::Python,
            request: dap::DebugRequestType::Launch,
            program: "/repo/main.py".to_string(),
            cwd: "/repo".to_string(),
            attach_host: dap::default_attach_host(),
            attach_port: Some(5678),
            args: vec![],
            env: BTreeMap::new(),
            breakpoints: vec![dap::DebugBreakpoint {
                file: "main.py".to_string(),
                line: 10,
                column: 1,
                condition: None,
                log_message: None,
            }],
        }],
    };
    assert!(dap::validate_document(&doc).is_ok());
    // wire 形状（camelCase；debugType/tag 保留）。
    let json = serde_json::to_string(&doc).unwrap();
    assert!(
        json.contains("\"type\""),
        "type wire 名保留（rename=type）；debugType 是 run_config 的字段"
    );
    assert!(json.contains("\"attachHost\""), "attachHost camelCase");
    // 重复 id 拒绝。
    let dup = dap::DebugConfigDocument {
        version: dap::DEBUG_CONFIG_VERSION,
        configs: vec![
            doc.configs[0].clone(),
            dap::DebugConfig {
                id: "d1".to_string(),
                name: "dup".to_string(),
                config_type: dap::DebugConfigType::Node,
                request: dap::DebugRequestType::Attach,
                program: "x".to_string(),
                cwd: "/".to_string(),
                attach_host: dap::default_attach_host(),
                attach_port: None,
                args: vec![],
                env: BTreeMap::new(),
                breakpoints: vec![],
            },
        ],
    };
    assert!(dap::validate_document(&dup).is_err());
}
