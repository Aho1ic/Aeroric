//! P6-03 验收：LSP 协议（中文/emoji 位置、offset encoding 转换）、run_config
//! 校验、DAP 协议与断点、test failure 定位、preview URL 判定。
use aeroric_core::ide::dap;
use aeroric_core::ide::diagnostics;
use aeroric_core::ide::lsp;
use aeroric_core::ide::run_config;
use aeroric_core::ide::ProjectTarget;

#[test]
fn utf16_offset_handles_cjk_and_emoji() {
    // 中文：每个字符 1 个 UTF-16 code unit（字节索引是 UTF-8 字节）。
    let line = "你好世界";
    assert_eq!(
        lsp::utf16_character_to_string_index(line, 2),
        6,
        "2 个 CJK 字符＝6 字节（UTF-8）"
    );
    // emoji：每个字符 2 个 UTF-16 code unit（surrogate pair）。
    let emoji = "😀x";
    // character=2 → 越过 emoji（2 units）→ 字节索引 4（x 的起点）。
    assert_eq!(lsp::utf16_character_to_string_index(emoji, 2), 4);
    // character=3 ＝ x 的 UTF-16 span 末端（x 占 units 2..3）→ 字节 5。
    assert_eq!(lsp::utf16_character_to_string_index(emoji, 3), 5);
    // 回转。
    assert_eq!(lsp::string_index_to_utf16_character(emoji, 2), 2);
    assert_eq!(lsp::string_index_to_utf16_character("你好", 1), 1);
}

#[test]
fn stale_document_version_is_dropped() {
    assert!(lsp::is_stale_document_version(5, 4), "过时版本丢弃");
    assert!(!lsp::is_stale_document_version(5, 5));
    assert!(!lsp::is_stale_document_version(5, 6));
}

#[test]
fn language_id_and_default_server_command() {
    assert_eq!(lsp::language_id_for_path("src/main.rs"), Some("rust"));
    assert_eq!(lsp::language_id_for_path("a.tsx"), Some("typescriptreact"));
    assert_eq!(lsp::language_id_for_path("b.unknown"), None);
    let cmd = lsp::default_server_command("rust").unwrap();
    assert_eq!(cmd.program, "rust-analyzer");
    assert!(lsp::default_server_command("unknown").is_none());
}

#[test]
fn tsc_parser_handles_error_warning_and_position() {
    let output = "src/main.ts(42,7): error TS2345: Argument of type 'x' is not assignable.\n\
                  src/lib.ts(7,1): warning TS6133: unused variable.\n";
    let root = std::path::Path::new("/repo");
    let items = diagnostics::parse_tsc_output(root, output);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].severity, diagnostics::DiagnosticSeverity::Error);
    assert_eq!(items[0].line, 42);
    assert_eq!(items[0].column, 7);
    assert_eq!(items[0].code.as_deref(), Some("TS2345"));
    assert_eq!(items[1].severity, diagnostics::DiagnosticSeverity::Warning);
}

#[test]
fn eslint_parser_and_drop_stale() {
    let output = r#"[{"filePath":"/repo/src/a.ts","messages":[{"line":3,"column":5,"message":"unexpected","ruleId":"no-undef","severity":2}]}]"#;
    let root = std::path::Path::new("/repo");
    let items = diagnostics::parse_eslint_json(root, output);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].severity, diagnostics::DiagnosticSeverity::Error);
    // 过时 diagnostics 丢弃。
    assert!(diagnostics::drop_stale(5, 4, items.clone()).is_empty());
    assert_eq!(diagnostics::drop_stale(5, 5, items).len(), 1);
}

#[test]
fn run_config_document_validation_and_diff() {
    use std::collections::BTreeMap;
    let doc = run_config::RunConfigDocument {
        version: run_config::RUN_CONFIG_VERSION,
        configs: vec![run_config::RunConfig::Shell {
            id: "r1".to_string(),
            name: "test".to_string(),
            command: "cargo test".to_string(),
            cwd: "/repo".to_string(),
            env: BTreeMap::new(),
        }],
    };
    assert!(run_config::validate_document(&doc).is_ok());
    // 重复 id 拒绝。
    let dup = run_config::RunConfigDocument {
        version: run_config::RUN_CONFIG_VERSION,
        configs: vec![
            run_config::RunConfig::Shell {
                id: "r1".to_string(),
                name: "a".to_string(),
                command: "x".to_string(),
                cwd: "/".to_string(),
                env: BTreeMap::new(),
            },
            run_config::RunConfig::Shell {
                id: "r1".to_string(),
                name: "b".to_string(),
                command: "y".to_string(),
                cwd: "/".to_string(),
                env: BTreeMap::new(),
            },
        ],
    };
    assert!(run_config::validate_document(&dup).is_err());
    // 无效版本拒绝（保存失败不污染旧配置）。
    let bad_version = run_config::RunConfigDocument {
        version: 99,
        configs: vec![],
    };
    assert!(run_config::validate_document(&bad_version).is_err());
    // 断点 diff（mutation 身份）。
    let bp = run_config::RunDebugBreakpoint {
        file: "a.rs".to_string(),
        line: 1,
        column: 1,
    };
    let incoming_bp = bp.clone();
    assert!(run_config::breakpoints_equal(&[bp], &[incoming_bp]));
    assert!(run_config::find_config(&doc, "r1").is_some());
}

#[test]
fn dap_action_lifecycle_and_breakpoints() {
    // Start/Attach 只对无会话。
    assert!(dap::is_action_allowed(
        dap::DebugAction::Start,
        false,
        false
    ));
    assert!(!dap::is_action_allowed(
        dap::DebugAction::Start,
        true,
        false
    ));
    // Pause/Continue/Step 只对有会话（paused 判定）。
    assert!(dap::is_action_allowed(dap::DebugAction::Pause, true, false));
    assert!(!dap::is_action_allowed(dap::DebugAction::Pause, true, true));
    assert!(dap::is_action_allowed(
        dap::DebugAction::Continue,
        true,
        true
    ));
    assert!(!dap::is_action_allowed(
        dap::DebugAction::Continue,
        true,
        false
    ));
    // Stop 只对有会话（断开 debug 不误 kill 其他任务：按 debug_id 定向）。
    assert!(dap::is_action_allowed(dap::DebugAction::Stop, true, false));
    assert!(!dap::is_action_allowed(
        dap::DebugAction::Stop,
        false,
        false
    ));
    // 断点 diff（含 condition/logMessage 字段）。
    let bp = dap::DebugBreakpoint {
        file: "a.rs".to_string(),
        line: 1,
        column: 1,
        condition: Some("x > 0".to_string()),
        log_message: None,
    };
    let incoming_bp = bp.clone();
    assert!(dap::breakpoints_equal(&[bp], &[incoming_bp]));
    // 配置文档校验。
    let doc = dap::DebugConfigDocument {
        version: dap::DEBUG_CONFIG_VERSION,
        configs: vec![],
    };
    assert!(dap::validate_document(&doc).is_ok());
}

#[test]
fn test_failure_location_and_preview_url() {
    // cargo test 失败定位。
    let loc = ProjectTarget::local("/repo");
    assert_eq!(loc.flavor, "local");
    let failure = aeroric_core::ide::parse_cargo_test_failure_line("  --> src/main.rs:42:5");
    assert_eq!(
        failure.as_ref().map(|f| f.file.as_str()),
        Some("src/main.rs")
    );
    assert_eq!(failure.map(|f| f.line), Some(42));
    // web preview 只允许显式批准的本地/项目 URL。
    assert!(aeroric_core::ide::is_approved_preview_url(
        "http://localhost:3000",
        &[]
    ));
    assert!(aeroric_core::ide::is_approved_preview_url(
        "http://127.0.0.1:5173",
        &[]
    ));
    assert!(aeroric_core::ide::is_approved_preview_url(
        "http://myhost.dev:8080",
        &["myhost.dev".to_string()]
    ));
    assert!(
        !aeroric_core::ide::is_approved_preview_url("http://evil.com", &[]),
        "外部 URL 拒绝"
    );
    assert!(!aeroric_core::ide::is_approved_preview_url(
        "javascript:alert(1)",
        &[]
    ));
    assert!(!aeroric_core::ide::is_approved_preview_url(
        "file:///etc/passwd",
        &[]
    ));
    assert!(!aeroric_core::ide::is_approved_preview_url(
        "data:text/html,<script>",
        &[]
    ));
}
