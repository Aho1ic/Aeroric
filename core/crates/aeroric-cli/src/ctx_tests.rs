use super::*;

/// The help is the tool table: every tool Aeroric can offer is listed,
/// with its required arguments marked and the key on the keyed ones.
#[test]
fn the_help_is_written_from_the_tool_table() {
    let text = help();
    for (name, _, _) in aeroric_protocol::tools::AERORIC_TOOLS {
        assert!(text.contains(&format!("  {name}（")), "{name} missing");
    }
    assert!(
        text.contains("search_sessions（参数：query（必填）"),
        "{text}"
    );
    let delegate = text
        .lines()
        .find(|l| l.starts_with("  delegate（"))
        .unwrap();
    assert!(delegate.contains("idempotency_key"), "{delegate}");
    let read = text
        .lines()
        .find(|l| l.starts_with("  board_read（"))
        .unwrap();
    assert!(!read.contains("idempotency_key"), "{read}");
    assert!(text.contains("AERORIC_CONTEXT_TOKEN"));
}

#[test]
fn arguments_must_be_a_json_object() {
    assert_eq!(arguments(None).unwrap(), json!({}));
    assert_eq!(
        arguments(Some(r#"{"query": "登录"}"#)).unwrap(),
        json!({"query": "登录"})
    );
    assert!(arguments(Some("[1]")).unwrap_err().contains("JSON 对象"));
    assert!(arguments(Some("{x"))
        .unwrap_err()
        .contains("不是有效的 JSON"));
}

#[test]
fn the_exit_code_follows_the_status() {
    let with = |status: &str| json!({"structuredContent": {"status": status}});
    assert_eq!(code_of(&with("ok")), exit::OK);
    assert_eq!(code_of(&with("partial")), exit::OK);
    assert_eq!(code_of(&with("retry")), exit::CONNECT);
    assert_eq!(code_of(&with("rejected")), exit::TASK_FAILED);
    assert_eq!(code_of(&with("unclear")), exit::TASK_FAILED);
    assert_eq!(code_of(&json!({})), exit::TASK_FAILED);
}

/// The same input through `ctx` and through the MCP server's `tools/call`
/// reaches the host as the same method with the same parameters and
/// comes back as the same result — with a host, without one, and for a
/// tool that does not exist.
#[tokio::test]
async fn ctx_and_mcp_answer_the_same_input_alike() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let dir = std::env::temp_dir().join(format!(
        "aeroric-ctx-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("host.token"), "tok").unwrap();
    let socket = aeroric_host::uds::default_socket_path(&dir);
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    // Two connections (MCP's, then ctx's), each: hello, then one call.
    let host = tokio::spawn(async move {
        let mut seen = Vec::new();
        for _ in 0..2 {
            let (stream, _) = listener.accept().await.unwrap();
            let (read, mut write) = stream.into_split();
            let mut lines = tokio::io::BufReader::new(read).lines();
            let hello: Value =
                serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            let reply = json!({"v": 1, "id": hello["id"], "ok": true, "result": {}});
            write
                .write_all(format!("{reply}\n").as_bytes())
                .await
                .unwrap();
            let call: Value =
                serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            let reply = json!({"v": 1, "id": call["id"], "ok": true,
                               "result": {"text": "记得：登录用 OAuth", "nextOffset": 3}});
            write
                .write_all(format!("{reply}\n").as_bytes())
                .await
                .unwrap();
            seen.push((call["method"].clone(), call["params"].clone()));
        }
        seen
    });

    let arguments = json!({"query": "登录", "limit": 3});
    let through_mcp = crate::mcp_context::call_tool(
        &mut None,
        &dir,
        "task-token",
        &json!({"name": "memory_recall", "arguments": arguments}),
    )
    .await;
    let through_ctx = call(&dir, "task-token", "memory_recall", arguments).await;
    assert_eq!(through_ctx, through_mcp);
    assert_eq!(through_ctx["structuredContent"]["status"], "ok");
    assert_eq!(through_ctx["structuredContent"]["cursor"], 3);
    assert_eq!(code_of(&through_ctx), exit::OK);
    let seen = host.await.unwrap();
    assert_eq!(seen[0], seen[1], "the host sees the same call");
    assert_eq!(seen[0].0, "context.memoryRecall");
    assert_eq!(seen[0].1["token"], "task-token");

    // No host: both say to try again.
    let gone = std::env::temp_dir().join("aeroric-ctx-no-host");
    let mcp = crate::mcp_context::call_tool(
        &mut None,
        &gone,
        "t",
        &json!({"name": "board_read", "arguments": {}}),
    )
    .await;
    let ctx = call(&gone, "t", "board_read", json!({})).await;
    assert_eq!(ctx, mcp);
    assert_eq!(code_of(&ctx), exit::CONNECT);

    // No such tool: both refuse it.
    let ctx = call(&gone, "t", "rm_rf", json!({})).await;
    assert_eq!(ctx["structuredContent"]["status"], "rejected");
    assert_eq!(code_of(&ctx), exit::TASK_FAILED);
    std::fs::remove_dir_all(&dir).ok();
}
