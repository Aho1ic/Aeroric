//! P3-05 第一片金样：host 下行帧 → 事件名的 10 臂映射（Action 1 验收名单
//! `tests/{dsh_host,dsh_approvals,dsh_reconnect}.rs` 的 `dsh_host` 自本片起用）。
//!
//! 判据两条来源：① 字符串值是**在网契约**——前端 `src/tauriEvents.ts` 的
//! `DSH_HOST_*_EVENT` 逐字对账（`event_names.rs` 头注释原文「改名时同步改两侧」；
//! 台账的 ts_consts/rust_consts 双列由 `inventory --check` 每天核），所以这里
//! 逐条钉**字面量**而不是钉常量名互等；② 「未知帧静默丢弃」是补记一百二十
//! 定性过的既有前向兼容行为，返回 `None` 即其纯形。

use aeroric_core::agents::dsh_host::dispatcher::host_frame_event_name;
use aeroric_core::agents::dsh_host::dispatcher::{
    DSH_HOST_AGENT_ERROR, DSH_HOST_ARCHIVED_SESSIONS_CHANGED, DSH_HOST_REMOTE_EVENT,
    DSH_HOST_SESSION_ADDED, DSH_HOST_SESSION_REMOVED, DSH_HOST_SESSION_STATUS,
    DSH_HOST_STREAM_ERROR, DSH_HOST_WORKSPACE_CHANGED, DSH_HOST_WORKSPACE_ORDER_CHANGED,
    DSH_HOST_WORKSPACE_REMOVED,
};
use serde_json::json;

#[test]
fn ten_arms_map_to_their_pinned_wire_strings() {
    let cases = [
        (
            "host/session-added",
            DSH_HOST_SESSION_ADDED,
            "dsh-host-session-added",
        ),
        (
            "host/session-removed",
            DSH_HOST_SESSION_REMOVED,
            "dsh-host-session-removed",
        ),
        (
            "host/session-status",
            DSH_HOST_SESSION_STATUS,
            "dsh-host-session-status",
        ),
        (
            "host/agent-error",
            DSH_HOST_AGENT_ERROR,
            "dsh-host-agent-error",
        ),
        (
            "host/workspace-changed",
            DSH_HOST_WORKSPACE_CHANGED,
            "dsh-host-workspace-changed",
        ),
        (
            "host/workspace-removed",
            DSH_HOST_WORKSPACE_REMOVED,
            "dsh-host-workspace-removed",
        ),
        (
            "host/workspace-order-changed",
            DSH_HOST_WORKSPACE_ORDER_CHANGED,
            "dsh-host-workspace-order-changed",
        ),
        (
            "host/archived-sessions-changed",
            DSH_HOST_ARCHIVED_SESSIONS_CHANGED,
            "dsh-host-archived-sessions-changed",
        ),
        (
            "host/remote-event",
            DSH_HOST_REMOTE_EVENT,
            "dsh-host-remote-event",
        ),
        (
            "stream/error",
            DSH_HOST_STREAM_ERROR,
            "dsh-host-stream-error",
        ),
    ];
    for (frame, constant, literal) in cases {
        assert_eq!(
            host_frame_event_name(&json!({"type": frame})),
            Some(constant)
        );
        assert_eq!(constant, literal, "在网字符串逐字钉死（前端对账基准）");
    }
}

#[test]
fn unknown_frames_drop_silently_including_near_misses() {
    // 前向兼容：未知帧型返回 None（宿主 wrapper 的早退），不报错不计数。
    assert_eq!(
        host_frame_event_name(&json!({"type": "host/session-added-v2"})),
        None
    );
    assert_eq!(
        host_frame_event_name(&json!({"type": "dsh/host/session-added"})),
        None
    );
    // 事件名当帧型喂回来不成立——映射方向是 帧型→事件名，不是恒等。
    assert_eq!(
        host_frame_event_name(&json!({"type": "dsh-host-session-added"})),
        None
    );
    // 缺 type／非字符串 type／null payload：全部 None，不许 panic。
    assert_eq!(host_frame_event_name(&json!({})), None);
    assert_eq!(host_frame_event_name(&json!({"type": 7})), None);
    assert_eq!(host_frame_event_name(&json!(null)), None);
}

#[test]
fn payload_beyond_type_does_not_change_the_mapping() {
    let bare = json!({"type": "host/workspace-changed"});
    let loaded = json!({"type": "host/workspace-changed", "id": "ws-1", "nested": {"a": [1, 2]}});
    assert_eq!(host_frame_event_name(&bare), host_frame_event_name(&loaded));
    assert_eq!(
        host_frame_event_name(&loaded),
        Some(DSH_HOST_WORKSPACE_CHANGED),
        "整帧原样转发是宿主 emit 的语义；映射只看 type"
    );
}

#[test]
fn rpc_envelope_decodes_value_and_keeps_both_error_wordings_verbatim() {
    // 与宿主 re-export 上的两条既有测试同一判据——core API 直接钉，防止
    // 未来某次「只改宿主」的漂移绕过。
    use aeroric_core::agents::dsh_host::client::decode_result_value;
    assert_eq!(
        decode_result_value(
            json!({"result":{"ok":true,"value":{"id":7}}}),
            "session.list"
        )
        .unwrap(),
        json!({"id":7})
    );
    assert_eq!(
        decode_result_value(json!({"result":{"ok":true}}), "x").unwrap(),
        json!(null),
        "缺 value 默认 Null 是既有语义"
    );
    assert_eq!(
        decode_result_value(json!({}), "session.list").unwrap_err(),
        "DSH API session.list response has no result"
    );
    assert_eq!(
        decode_result_value(
            json!({"result":{"ok":false,"error":{"message":"denied"}}}),
            "session.list"
        )
        .unwrap_err(),
        "DSH API session.list rejected the request: denied"
    );
    // error 缺 message ⇒ "unknown DSH API error" 兜底。
    assert_eq!(
        decode_result_value(json!({"result":{"ok":false,"error":{}}}), "m").unwrap_err(),
        "DSH API m rejected the request: unknown DSH API error"
    );
}

#[test]
fn pinned_inventory_keeps_generation_anchors_and_excludes_retired_names() {
    // 第三片钉面：三 pin 值＝快照对账基准（G17）；现用帧表不得重新收进 DSH-14 退役名；
    // `REMOTE_EVENTS.zip(REMOTE_EVENT_MODES)` 是宿主 `snapshot()` 的硬前提——长度必须成对。
    use aeroric_core::agents::dsh_host::client::{
        PACKAGE_VERSION, PROTOCOL_VERSION, REMOTE_EVENTS, REMOTE_EVENT_MODES, RPC_METHODS,
        SOURCE_COMMIT, STREAM_CONTROL_FRAMES, STREAM_DOWNLINK_FRAMES, STREAM_FOLLOW_FRAMES,
    };
    assert_eq!(SOURCE_COMMIT, "c291e7961a515f6d7af9304e7fd1d257929aef26");
    assert_eq!(PACKAGE_VERSION, "0.1.5-rc.2");
    assert_eq!(PROTOCOL_VERSION, 2);
    assert!(RPC_METHODS.contains(&"session.list") && RPC_METHODS.len() >= 60);
    assert_eq!(
        REMOTE_EVENTS.len(),
        REMOTE_EVENT_MODES.len(),
        "snapshot() 按 zip 配对，长度错位就静默吞尾"
    );
    for retired in [
        "session/event",
        "session/subscribed",
        "approval/requested",
        "stream/error",
    ] {
        assert!(
            !STREAM_FOLLOW_FRAMES.contains(&retired)
                && !STREAM_CONTROL_FRAMES.contains(&retired)
                && !STREAM_DOWNLINK_FRAMES.contains(&retired),
            "退役 firehose 名 {retired} 不得回流现用帧表"
        );
    }
}

#[test]
fn waterfall_verdicts_map_arms_and_pin_event_strings() {
    use aeroric_core::agents::dsh_host::approvals::{
        waterfall_request_kind, waterfall_resolved_event, WaterfallKind,
    };
    let (kind, name) = waterfall_request_kind("permission/request").unwrap();
    assert!(matches!(kind, WaterfallKind::Approval));
    assert_eq!(name, "dsh-approval-requested");
    let (kind, name) = waterfall_request_kind("user-questions/request").unwrap();
    assert!(matches!(kind, WaterfallKind::Question));
    assert_eq!(name, "dsh-question-requested");
    // 未知臂＝None，由宿主发 stream/error（不得静默吞的形状在宿主侧，判定面在此）。
    assert!(waterfall_request_kind("permission/answered").is_none());
    assert_eq!(
        waterfall_resolved_event(WaterfallKind::Approval),
        "dsh-approval-resolved"
    );
    assert_eq!(
        waterfall_resolved_event(WaterfallKind::Question),
        "dsh-question-resolved"
    );
}

#[test]
fn session_id_prefers_session_key_then_agent_fallback() {
    use aeroric_core::agents::dsh_host::approvals::waterfall_session_id;
    assert_eq!(
        waterfall_session_id(&json!({"sessionId":"s1","agentId":"a1"})).as_deref(),
        Some("s1")
    );
    assert_eq!(
        waterfall_session_id(&json!({"agentId":"a1"})).as_deref(),
        Some("a1")
    );
    assert_eq!(waterfall_session_id(&json!({"sessionId":7})), None);
    assert_eq!(waterfall_session_id(&json!({})), None);
}

#[test]
fn request_frame_stamps_ids_without_clobbering_session_payload() {
    use aeroric_core::agents::dsh_host::approvals::stamp_waterfall_frame;
    let frame = stamp_waterfall_frame(
        &json!({"type":"permission/request","sessionId":"mine"}),
        "ev-1",
        Some("gen-9".to_string()),
        Some("other".to_string()),
    );
    assert_eq!(frame["eventId"], json!("ev-1"));
    assert_eq!(frame["clientId"], json!("gen-9"));
    assert_eq!(
        frame["sessionId"],
        json!("mine"),
        "or_insert 语义：payload 自带的 sessionId 不被补位值覆盖"
    );
    // client_id 缺位不注入；sessionId 缺位则补。
    let frame = stamp_waterfall_frame(&json!({"a":1}), "ev-2", None, Some("s2".to_string()));
    assert!(frame.get("clientId").is_none());
    assert_eq!(frame["sessionId"], json!("s2"));
    // 非 object payload 原样返回（宿主原 `as_object_mut` 分支）。
    assert_eq!(
        stamp_waterfall_frame(&json!(null), "ev-3", None, None),
        json!(null)
    );
}

#[test]
fn registry_consumes_once_and_clear_resets_generation_scoped_state() {
    use aeroric_core::agents::dsh_host::approvals::{
        PendingWaterfall, WaterfallKind, WaterfallRegistry,
    };
    let mut reg = WaterfallRegistry::default();
    assert_eq!(reg.client_id(), None);
    reg.set_client_id("gen-1".to_string());
    reg.insert(
        "ev-1".to_string(),
        PendingWaterfall {
            kind: WaterfallKind::Approval,
            session_id: Some("s1".to_string()),
        },
    );
    // take-and-consume：第一次 remove 拿到，第二次 None——网络重放不得二次批准。
    let first = reg.remove("ev-1").expect("first consume");
    assert!(matches!(first.kind, WaterfallKind::Approval));
    assert_eq!(first.session_id.as_deref(), Some("s1"));
    assert!(reg.remove("ev-1").is_none(), "重放必须拿不到");
    // 幂等兜底路由：unknown eventId 时 reply 路径读 all_pending().next()。
    reg.insert(
        "ev-2".to_string(),
        PendingWaterfall {
            kind: WaterfallKind::Question,
            session_id: None,
        },
    );
    let pending = reg.all_pending();
    assert_eq!(pending.len(), 1);
    assert!(pending[0].session_id.is_none());
    // 载体重连：client_id 与 pending 同代清空（默认值的两半一起归零）。
    reg.clear();
    assert_eq!(reg.client_id(), None);
    assert!(reg.all_pending().is_empty());
}

#[test]
fn startup_url_gate_admits_only_plain_loopback_origins() {
    use aeroric_core::agents::dsh_host::startup::{
        dsh_web_command_args, parse_dsh_web_startup_url,
    };
    // 前缀不匹配的行：Ok(None)，抽干继续。
    assert_eq!(parse_dsh_web_startup_url("serving on :8080").unwrap(), None);
    assert_eq!(
        parse_dsh_web_startup_url("  dsh web: http://localhost:41234/")
            .unwrap()
            .unwrap()
            .0,
        "http://localhost:41234"
    );
    let (base, port) = parse_dsh_web_startup_url("dsh web: http://127.0.0.1:55/")
        .unwrap()
        .unwrap();
    assert_eq!((base.as_str(), port), ("http://127.0.0.1:55", 55));
    // IPv6 loopback 的 base 必须带方括号（否则 URL 重新解析就错位）。
    assert_eq!(
        parse_dsh_web_startup_url("dsh web: http://[::1]:9/")
            .unwrap()
            .unwrap()
            .0,
        "http://[::1]:9"
    );
    // 六条拒绝臂逐条点名（这是全部 RPC/WS 的 base，放窄不放宽）。
    let err = |line: &str| parse_dsh_web_startup_url(line).unwrap_err();
    assert!(err("dsh web: https://localhost:1/").contains("non-HTTP"));
    assert!(err("dsh web: http://example.com:80/").contains("non-loopback"));
    assert!(err("dsh web: http://user@localhost:80/").contains("unsupported startup URL shape"));
    assert!(err("dsh web: http://localhost:80/a").contains("unsupported startup URL shape"));
    assert!(err("dsh web: http://localhost:80/?x=1").contains("unsupported startup URL shape"));
    assert!(err("dsh web: http://localhost:80/#f").contains("unsupported startup URL shape"));
    assert!(err("dsh web: http://localhost/").contains("has no port"));
    assert!(err("dsh web: http://localhost:0/").contains("port 0"));
    assert!(err("dsh web: not a url").contains("invalid startup URL"));
    // 参数臂：headless 四件套，--no-open 不许丢（丢了每次拉起舞会弹系统浏览器）。
    assert_eq!(dsh_web_command_args(), ["web", "--port", "0", "--no-open"]);
}

#[test]
fn output_ring_truncates_on_char_boundary_and_details_concatenate_once() {
    use aeroric_core::agents::dsh_host::startup::{
        append_bounded_output, attach_startup_output, exited_dsh_web_error, startup_output_detail,
        DshWebStartupOutput, DSH_WEB_OUTPUT_LIMIT,
    };
    use parking_lot::Mutex;
    use std::sync::Arc;
    // 多字节行 ×N：环必须钳在上限且不切半字符（drain 到 char 边界）。
    let mut buf = String::new();
    for i in 0..2000 {
        append_bounded_output(&mut buf, &format!("行-{i}-é中文"));
        assert!(buf.len() <= DSH_WEB_OUTPUT_LIMIT + 64, "峰值 {}", buf.len());
        assert!(buf.is_char_boundary(buf.len()));
    }
    assert!(buf.len() <= DSH_WEB_OUTPUT_LIMIT);
    let out = Arc::new(Mutex::new(DshWebStartupOutput::default()));
    assert_eq!(startup_output_detail(&out), "", "空缓冲不产噪声");
    {
        let mut g = out.lock();
        g.stderr.push_str("boom\n");
        g.stdout.push_str("noise\n");
    }
    let detail = startup_output_detail(&out);
    assert_eq!(
        detail, "stderr:\nboom\nstdout:\nnoise",
        "stderr 先行是原文顺序"
    );
    // attach：detail 已在 error 里就不重复贴（同一真相只出现一次）。
    let joined = attach_startup_output("x".to_string(), &out);
    assert_eq!(joined, format!("x\n{detail}"));
    let once = attach_startup_output(joined.clone(), &out);
    assert_eq!(once, joined, "二次 attach 必须幂等");
    // exited 文案：status 的 Display 形插进括号（ExitStatus 没有公开构造器，用
    // 「必然失败的进程」取——mac/linux 走 false，Windows 走 cmd /C exit 1；
    // 用例数两侧一致，不踩「exact-count 地板 × cfg(test)」的已知坑）。
    #[cfg(unix)]
    let status = std::process::Command::new("false").status().unwrap();
    #[cfg(not(unix))]
    let status = std::process::Command::new("cmd")
        .args(["/C", "exit 1"])
        .status()
        .unwrap();
    let msg = exited_dsh_web_error(status, &out);
    assert!(
        msg.starts_with("DSH Web exited before becoming ready ("),
        "{msg}"
    );
    assert!(msg.ends_with(&detail), "detail 必须拼上: {msg}");
}

#[test]
fn webui_status_and_recovery_serde_shapes_are_the_wire_contract() {
    use aeroric_core::agents::dsh_host::state::{DshRuntimeRecovery, WebUiStatus};
    assert_eq!(
        serde_json::to_string(&WebUiStatus::Starting).unwrap(),
        "\"starting\""
    );
    assert_eq!(
        serde_json::to_string(&WebUiStatus::Running).unwrap(),
        "\"running\""
    );
    for v in ["stopped", "error"] {
        assert_eq!(
            serde_json::from_str::<WebUiStatus>(&format!("\"{v}\"")).unwrap(),
            match v {
                "stopped" => WebUiStatus::Stopped,
                _ => WebUiStatus::Error,
            }
        );
    }
    // DshRuntimeRecovery 字段名无 rename——前端读的就是这四个 snake 键。
    let rec: DshRuntimeRecovery = serde_json::from_str(
        r#"{"restarted":true,"reconnected_sessions":2,"cancelled_turns":1,"errors":["e"]}"#,
    )
    .unwrap();
    assert!(rec.restarted && rec.reconnected_sessions == 2 && rec.errors.len() == 1);
    assert_eq!(
        serde_json::to_string(&DshRuntimeRecovery::default()).unwrap(),
        r#"{"restarted":false,"reconnected_sessions":0,"cancelled_turns":0,"errors":[]}"#
    );
}

#[test]
fn internal_command_echo_machine_keeps_its_two_asymmetric_rules() {
    use aeroric_core::agents::dsh_host::state::{
        is_command_echo_event, take_internal_command_echo, InternalCommandEcho,
    };
    use std::collections::HashMap;
    let run = |name: &str| json!({"event":{"type":"command/run","data":{"name":name}}});
    let done = json!({"event":{"type":"command/done"}});
    assert!(!is_command_echo_event(
        &json!({"event":{"type":"assistant/delta"}})
    ));
    assert!(is_command_echo_event(&run("permission")));

    let mut pending: HashMap<String, InternalCommandEcho> = HashMap::from([(
        "t1".to_string(),
        InternalCommandEcho {
            name: "permission".to_string(),
            saw_run: false,
        },
    )]);
    // 名字不配 ⇒ 不吞、登记原样留着等真 run。
    assert!(!take_internal_command_echo(
        &mut pending,
        "t1",
        &run("other")
    ));
    assert_eq!(pending["t1"].name, "permission");
    // 配对的 run ⇒ 吞并置位。
    assert!(take_internal_command_echo(
        &mut pending,
        "t1",
        &run("permission")
    ));
    // 规则①：第二次同名 run 不吞（saw_run 已置）。
    assert!(!take_internal_command_echo(
        &mut pending,
        "t1",
        &run("permission")
    ));
    // 未登记任务的一切 command 帧 ⇒ false。
    assert!(!take_internal_command_echo(
        &mut pending,
        "t9",
        &run("permission")
    ));
    // 规则②的镜像在下一段验证：先 done 不吞不清。这里先正常收尾 done ⇒ 吞且删登记。
    assert!(take_internal_command_echo(&mut pending, "t1", &done));
    assert!(!pending.contains_key("t1"), "done 必须终结一次性登记");

    // 规则②：done 早于 run 到达 ⇒ 不吞，且**登记不被清除**（真 run 还能来）。
    let mut pending: HashMap<String, InternalCommandEcho> = HashMap::from([(
        "t2".to_string(),
        InternalCommandEcho {
            name: "goal".to_string(),
            saw_run: false,
        },
    )]);
    assert!(!take_internal_command_echo(&mut pending, "t2", &done));
    assert!(pending.contains_key("t2"), "提前的 done 不得连登记一起吞掉");
    assert!(take_internal_command_echo(&mut pending, "t2", &run("goal")));
    assert!(take_internal_command_echo(&mut pending, "t2", &done));
}

#[test]
fn plain_single_line_peels_csi_osc_tabs_and_control_bytes() {
    use aeroric_core::agents::dsh_host::projection::{plain_single_line, view_str};
    assert_eq!(plain_single_line("\x1b[31mred\x1b[0m"), "red");
    // OSC 以 BEL 收尾。
    assert_eq!(plain_single_line("\x1b]0;title\x07after"), "after");
    // OSC 以 ESC \ 收尾。
    assert_eq!(plain_single_line("\x1b]8;;url\x1b\\tail"), "tail");
    // 孤立的 ESC+单字符也吃掉；tab 变空格；控制字符**直接丢弃**（BEL 不留空格——
    // 首版预期我写成 "a b c"，实测实现是 "a bc"，243/244 同型：错的是预期不是实现）。
    assert_eq!(plain_single_line("\x1bM a\tb\x07c"), "a bc");
    assert_eq!(plain_single_line("  padded  "), "padded");
    assert_eq!(plain_single_line("中文\x1b[1m粗\x1b[0m"), "中文粗");
    // view_str：空串当缺位（harness 对空白标题的处理）。
    let v = json!({"a":"x","b":"","c":7});
    assert_eq!(view_str(&v, "a"), Some("x"));
    assert_eq!(view_str(&v, "b"), None);
    assert_eq!(view_str(&v, "c"), None);
    assert_eq!(view_str(&v, "d"), None);
}

#[test]
fn echo_and_fold_rows_keep_the_terminal_shapes() {
    use aeroric_core::agents::dsh_host::projection::{
        clip_fragment, count_content_lines, fold_summary, plural, push_line, user_prompt_echo,
        ANSI_RESET, FOLD_SUMMARY_CHARS,
    };
    // 斜杠命令交给 command/run 回显，这里必须 None（双回显就是 bug）。
    assert!(user_prompt_echo("/permission full_access", 0).is_none());
    assert!(user_prompt_echo("   ", 0).is_none());
    let one = user_prompt_echo("看这个", 1).unwrap();
    assert!(
        one.starts_with("\r\n"),
        "xterm convertEol:false，行首显式 CRLF"
    );
    assert!(one.contains("❯"), "{one}");
    assert!(one.contains("· 1 image"), "{one}");
    let many = user_prompt_echo("两行\n续行", 12).unwrap();
    assert!(
        many.contains("两行\r\n  续行"),
        "续行两格缩进对齐 ❯: {many}"
    );
    assert!(many.contains("· 12 images"), "{many}");
    // clip：第 96 个字符保留、97 起换 …（按字符不按字节，中文安全）。
    let s96: String = "字".repeat(FOLD_SUMMARY_CHARS);
    assert_eq!(clip_fragment(&s96), s96);
    let clipped = clip_fragment(&format!("{s96}额外"));
    assert!(clipped.ends_with('…') && clipped.chars().count() == FOLD_SUMMARY_CHARS + 1);
    // fold：取第一条**有内容**的行且剥色。
    assert_eq!(
        fold_summary("\n  \n\x1b[32m内容\x1b[0m\n更多"),
        Some("内容".to_string())
    );
    assert_eq!(fold_summary("\x1b[0m\n \n"), None);
    assert_eq!(count_content_lines("a\n\n \nb\n"), 2);
    assert_eq!(plural(1, "line", "lines"), "line");
    assert_eq!(plural(2, "line", "lines"), "lines");
    // push_line：空 style 不加 RESET；CRLF 收尾。
    let mut out = String::new();
    push_line(&mut out, "", "plain");
    assert_eq!(out, "plain\r\n");
    let mut out2 = String::new();
    push_line(&mut out2, "\x1b[2m", "dim");
    assert_eq!(out2, format!("\x1b[2mdim{ANSI_RESET}\r\n"));
}

#[test]
fn prompt_image_accepts_both_shapes_with_a_four_type_whitelist() {
    use aeroric_core::agents::dsh_host::attachments::{normalize_prompt_mode, parse_prompt_image};
    assert_eq!(
        parse_prompt_image("data:image/png;base64,QUJD").unwrap(),
        ("image/png", "QUJD")
    );
    assert_eq!(
        parse_prompt_image("image/jpeg:raw-bytes").unwrap(),
        ("image/jpeg", "raw-bytes")
    );
    let err = |v: &str| parse_prompt_image(v).unwrap_err();
    assert_eq!(
        err("data:image/png,non-b64"),
        "DSH image data URL must use base64 encoding"
    );
    assert_eq!(err("data:image/png;base64"), "Invalid DSH image data URL");
    assert_eq!(err("no-colon-here"), "Invalid DSH image payload");
    assert_eq!(
        err("application/pdf:x"),
        "Unsupported DSH image media type \"application/pdf\""
    );
    assert_eq!(err("data:image/gif;base64,"), "DSH image payload is empty");
    // mode：两臂过、其余带期望值拒绝（文案是 toast 面）。
    assert_eq!(normalize_prompt_mode("queue").unwrap(), "queue");
    assert_eq!(normalize_prompt_mode("steer").unwrap(), "steer");
    assert_eq!(
        normalize_prompt_mode("yolo").unwrap_err(),
        "Unsupported DSH prompt mode \"yolo\"; expected queue or steer"
    );
}

#[test]
fn permission_preset_maps_three_modes_and_refuses_the_rest() {
    use aeroric_core::agents::dsh_host::approvals::dsh_permission_preset;
    assert_eq!(dsh_permission_preset("ask").unwrap(), "read-only");
    assert_eq!(
        dsh_permission_preset("auto_edit").unwrap(),
        "workspace-write"
    );
    assert_eq!(
        dsh_permission_preset("full_access").unwrap(),
        "danger-full-access"
    );
    assert_eq!(
        dsh_permission_preset("root").unwrap_err(),
        "Unsupported DSH permission mode \"root\""
    );
}

#[test]
fn terminal_wrap_moves_whole_words_and_collapses_blank_runs() {
    use aeroric_core::agents::dsh_host::projection::TerminalWrap;
    let mut w = TerminalWrap::default();
    // 低于 MIN_WRAP_COLS 的宽度不软换：原样透传（交给 xterm 硬折）。
    w.set_cols(10);
    assert_eq!(w.push("alpha beta gamma"), "alpha beta gamma");
    // 打开换行：短词整词搬运、超长词在行末硬折，字符一个不丢。
    w.set_cols(20);
    let out = w.push("short averylongwordthatwillnotfit");
    let flushed = w.flush();
    let full = out.clone() + &flushed;
    assert!(out.starts_with("short "), "空格先落地: {out:?}");
    assert!(full.contains("\r\n"), "超长词必须产生折行: {full:?}");
    assert_eq!(
        full.replace("\r\n", ""),
        "short averylongwordthatwillnotfit",
        "软换行只插 CRLF，不得增减内容字符"
    );
    assert!(!out.contains("\n\n"), "不产生空行: {out:?}");
    // 连续空行最多留一个、首屏空行丢弃：喂三段空行为独立机器验证。
    let mut fresh = TerminalWrap::default();
    fresh.set_cols(40);
    assert_eq!(fresh.push("\r\n\r\n\r\n"), "", "首屏之前的空行全部丢掉");
    let seen = fresh.push("content\r\n\r\n\r\nnext");
    // 尾词 "next" 按设计滞留待续，flush 后才落地。
    let seen_full = seen + &fresh.flush();
    assert!(
        seen_full.contains("content\r\n\r\nnext"),
        "三换行压成一空行: {seen_full:?}"
    );
}

#[test]
fn reasoning_fold_closes_summary_and_counts_lines() {
    use aeroric_core::agents::dsh_host::projection::ReasoningFold;
    let mut fold = ReasoningFold::default();
    assert_eq!(fold.take_row(), None, "没推理就不打行");
    fold.push("想清楚了");
    fold.push("更多字\r\n第二行");
    let row = fold.take_row().expect("有内容必须出行");
    assert!(row.starts_with("\r\n"), "{row:?}");
    assert!(row.contains("✻ Thinking · 想清楚了更多字"), "{row:?}");
    assert!(row.contains("2 lines"), "跨行计数: {row:?}");
    // take_row 即 reset：同一实例再收一段是全新的折叠。
    fold.push("第二段");
    let row2 = fold.take_row().unwrap();
    assert!(
        row2.contains("第二段") && !row2.contains("lines"),
        "第二条不带旧行号: {row2:?}"
    );
    // 96 字符后加省略号且不再增长（剥 ANSI 后核长=前缀+97）。
    use aeroric_core::agents::dsh_host::projection::plain_single_line;
    let mut long = ReasoningFold::default();
    long.push(&"字".repeat(200));
    let row3 = long.take_row().unwrap();
    let visible = plain_single_line(row3.replace("\r\n", " ").as_str());
    assert!(visible.contains('…'), "超 96 必带省略号: {visible}");
    assert_eq!(
        visible.chars().count(),
        "✻ Thinking · ".chars().count() + 97,
        "96+省略号封顶"
    );
}

#[test]
fn render_envelope_routes_by_for_and_refuses_unknown_shapes() {
    use aeroric_core::agents::dsh_host::projection::{plain_single_line, render_tool_event_view};
    // view 缺失 / for 不匹配 / 未知 card：全部 None＝回退 raw event（dsh 对无该
    // 能力 UI 的规定行为），不得造行。
    assert!(render_tool_event_view(
        &json!({"view":{"for":"call","view":{"card":"terminal"}}}),
        "result"
    )
    .is_none());
    // for 不匹配 ⇒ None（call 投递不带 result envelope）。
    assert!(render_tool_event_view(
        &json!({"view":{"for":"result","view":{"card":"terminal","title":"t"}}}),
        "call"
    )
    .is_none());
    // 未知 card ⇒ None＝回退 raw event。
    assert!(render_tool_event_view(
        &json!({"view":{"for":"call","view":{"card":"hologram","title":"t"}}}),
        "call"
    )
    .is_none());
    // terminal call 行：标题加粗臂＋description 折叠（剥 ANSI 后核可见文本）。
    let row = render_tool_event_view(
        &json!({"view":{"for":"call","view":{"card":"terminal","title":"Run tests","description":"第一行说明\n第二行不要"}}}),
        "call",
    )
    .unwrap();
    assert!(row.starts_with("\r\n"), "call 行前间隔 CRLF: {row:?}");
    let visible = plain_single_line(&row);
    assert_eq!(visible, "▸ Run tests · 第一行说明", "{visible:?}");
}

#[test]
fn result_rows_summarize_diff_search_and_exit_states() {
    use aeroric_core::agents::dsh_host::projection::{plain_single_line, render_tool_event_view};
    use serde_json::Value;
    let result = |v: Value| {
        render_tool_event_view(&json!({"view": {"for": "result", "view": v}}), "result").unwrap()
    };
    // diff：head/tail 修剪后的 hunk 计数 + 新文件标注。
    let diff = result(json!({
        "card":"diff",
        "diffs":[{"path":"a.rs","oldText":null,"newText":"x\ny\n"}],
    }));
    assert_eq!(
        plain_single_line(&diff),
        "✔ · a.rs (new file) +3 -0",
        "尾部 \\n 也是一行空文本"
    );
    // search/matches：hits 复数与 scope 单文件路径。
    let search = result(json!({
        "card":"search","shape":"matches",
        "files":[{"path":"src/x.rs","matches":[{},{},{}]}],
    }));
    assert_eq!(plain_single_line(&search), "✔ · 3 matches in src/x.rs");
    // terminal 失败：signal 压过 exitCode、无输出臂、truncated 注记。
    let fail = result(json!({
        "card":"terminal","signal":"SIGKILL","truncated":true,
    }));
    assert_eq!(
        plain_single_line(&fail),
        "✖ · SIGKILL · no output · truncated by the harness"
    );
    // exitCode 非 0 也是失败；output 折叠＋行数尾巴。
    let code = result(json!({
        "card":"terminal","exitCode":2,"output":"boom\nline2\n",
    }));
    assert_eq!(plain_single_line(&code), "✖ · exit 2 · boom · 2 lines");
}

#[test]
fn diff_rows_counts_hunks_without_lcs_and_flags_creates() {
    use aeroric_core::agents::dsh_host::projection::diff_rows;
    // old=None：全行加法（create/overwrite 无 before-image）。
    assert_eq!(
        diff_rows(None, "a\nb"),
        vec![('+', "a".to_string()), ('+', "b".to_string())]
    );
    // 公共头尾修剪后只剩中间 hunk；\r 被剥。
    let rows = diff_rows(Some("keep\nold\nkeep2\r"), "keep\nnew\nkeep2\r");
    assert_eq!(
        rows,
        vec![('-', "old".to_string()), ('+', "new".to_string())]
    );
}
