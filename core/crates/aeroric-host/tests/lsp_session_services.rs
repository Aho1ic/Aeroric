//! 切片一百一十:宿主 LSP **会话面**的端到端判据。
//!
//! 本机没有 `typescript-language-server`(窄表只认 ts/js),所以这里用一个
//! 讲 Content-Length 帧的 fake server(见 `tests/fixtures/fake-lsp.js`)经
//! `HostWiring::lsp_server` 注入,在 scratch 工程上驱动 hover/definition/
//! completion/signature/文档符号/工作区符号/代码动作/重命名/执行命令全链,
//! 断言的是宿主解析并投影出来的 DTO 形状(React 侧消费的就是这些字段);
//! 同时钉 diagnostics 通知经 sink 落 EventLog、远端 target 逐字拒绝、畸形
//! 请求在起进程之前就 InvalidArgument。

use aeroric_core::ide::lsp_session::engine;
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

fn success(reply: Frame) -> Value {
    match reply {
        Frame::Success(reply) => reply.result,
        other => panic!("expected success, got {other:?}"),
    }
}

fn failure(reply: Frame, code: ErrorCode) -> String {
    let Frame::Failure(reply) = reply else {
        panic!("expected failure, got {reply:?}");
    };
    assert_eq!(reply.error.code, code, "{reply:?}");
    reply.error.message
}

async fn authenticated(host: &Harness) -> (BufReader<OwnedReadHalf>, OwnedWriteHalf) {
    let (mut reader, mut writer) = host.connect().await;
    success(Harness::handshake(&mut reader, &mut writer, &host.token, "lsptest").await);
    (reader, writer)
}

async fn call(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    method: &str,
    params: Value,
) -> Frame {
    Harness::call(reader, writer, &Harness::request("lsptest", method, params)).await
}

static SEQ: AtomicU64 = AtomicU64::new(0);

/// scratch ts 工程 + 注入 fake server 的宿主。
async fn lsp_host(tag: &str) -> (Harness, PathBuf, PathBuf) {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let project = PathBuf::from("/tmp").join(format!("ah-lsp-{tag}-{}-{n}", std::process::id()));
    std::fs::create_dir_all(project.join("src")).expect("project dir");
    let app_file = project.join("src").join("App.ts");
    std::fs::write(&app_file, "const value: number = 1;\n").expect("app file");
    let canonical_app = app_file.canonicalize().expect("canonical app");
    let uri = format!("file://{}", canonical_app.display());

    // 引擎按 program+args **直接 exec**(不做 shell 分词),所以注入形状是
    // program = sh、args = ["-c", "exec node <script> --stdio <uri>"]。
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("fake-lsp.js");
    let args = vec![
        "-c".to_string(),
        format!("exec node {} --stdio {}", script.display(), uri),
    ];
    let host =
        Harness::start_with_wiring(tag, move |_data_dir| aeroric_host::dispatcher::HostWiring {
            lsp_server: Some(aeroric_host::lsp_services::LspServerInjection {
                program: Some("sh".to_string()),
                args: args.clone(),
                envs: vec![],
                diagnostics_event: Some("lsp://diagnostics".to_string()),
            }),
            ..Default::default()
        })
        .await;
    (host, project, canonical_app)
}

fn request_body(app: &std::path::Path, project: &std::path::Path) -> Value {
    json!({
        "projectPath": project,
        "filePath": app,
        "content": "const value: number = 1;\n",
        "line": 0,
        "character": 6
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn session_face_projects_typed_results_over_live_uds() {
    let (host, project, app) = lsp_host("session").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let body = request_body(&app, &project);

    // hover:contents 从 markdown 对象里取出来,range 保留。
    let hover = success(call(&mut reader, &mut writer, "ide.lsp_hover", body.clone()).await);
    assert_eq!(hover["contents"], "**fake hover**", "{hover}");
    assert_eq!(hover["range"]["start"]["character"], 0, "{hover}");
    assert_eq!(hover["range"]["end"]["character"], 3, "{hover}");

    // definition:location 的 uri 与（file uri 解出来的）path 都在。
    let definitions =
        success(call(&mut reader, &mut writer, "ide.lsp_definition", body.clone()).await);
    let definitions = definitions.as_array().expect("array");
    assert_eq!(definitions.len(), 1, "{definitions:?}");
    assert_eq!(definitions[0]["path"], "/scratch/src/helper.ts");
    assert_eq!(definitions[0]["range"]["start"]["line"], 9);

    // completion:强类型 item 逐字段。
    let completions =
        success(call(&mut reader, &mut writer, "ide.lsp_completion", body.clone()).await);
    assert_eq!(completions[0]["label"], "fakeCompletion", "{completions}");
    assert_eq!(completions[0]["detail"], "fake detail", "{completions}");
    assert_eq!(
        completions[0]["documentation"], "fake docs",
        "{completions}"
    );

    // signature help:嵌套的 parameters 也要出来。
    let signature = success(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_signature_help",
            body.clone(),
        )
        .await,
    );
    assert_eq!(signature["signatures"][0]["label"], "fake(label: string)");
    assert_eq!(
        signature["signatures"][0]["parameters"][0]["label"],
        "label"
    );
    assert_eq!(signature["activeSignature"], 0, "{signature}");

    // 文档符号:uri/path 由宿主按请求文档补齐,端状态字段齐全。
    let symbols = success(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_document_symbols",
            body.clone(),
        )
        .await,
    );
    assert_eq!(symbols[0]["name"], "fakeSymbol", "{symbols}");
    assert_eq!(symbols[0]["kind"], 12);
    assert!(symbols[0]["uri"]
        .as_str()
        .unwrap_or("")
        .starts_with("file://"));

    // 代码动作:edit.changes 被折成 files[] 强类型。
    let actions = success(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_code_actions",
            json!({
                "projectPath": project,
                "filePath": app,
                "content": "const value: number = 1;\n",
                "line": 0,
                "character": 6,
                "diagnostics": []
            }),
        )
        .await,
    );
    assert_eq!(actions[0]["title"], "fake quickfix", "{actions}");
    assert_eq!(actions[0]["kind"], "quickfix");
    assert_eq!(
        actions[0]["edit"]["files"][0]["path"],
        "/scratch/src/fake.ts"
    );

    // 重命名:WorkspaceEdit 的形状与代码动作一致。
    let rename = success(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_rename",
            json!({
                "projectPath": project,
                "filePath": app,
                "content": "const value: number = 1;\n",
                "line": 0,
                "character": 6,
                "newName": "renamedByFakeServer"
            }),
        )
        .await,
    );
    assert_eq!(
        rename["files"][0]["edits"][0]["newText"], "renamedByFakeServer",
        "{rename}"
    );

    // 执行命令:旧 wire 是 null(不是空对象)。
    let executed = success(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_execute_command",
            json!({
                "projectPath": project,
                "filePath": app,
                "content": "const value: number = 1;\n",
                "line": 0,
                "character": 6,
                "command": { "title": null, "command": "fake.command", "arguments": [1] }
            }),
        )
        .await,
    );
    assert_eq!(executed, Value::Null, "{executed}");

    // 工作区符号:会话键是 `local:<root>:typescript`,宿主先读该会话已开文档。
    success(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_open_document",
            json!({
                "projectPath": project,
                "filePath": app,
                "content": "const value: number = 1;\n",
                "version": 1
            }),
        )
        .await,
    );
    let workspace_symbols = success(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_workspace_symbols",
            json!({ "projectPath": project, "query": "fake" }),
        )
        .await,
    );
    assert_eq!(
        workspace_symbols[0]["name"], "fakeWorkspaceSymbol",
        "{workspace_symbols}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn diagnostics_notifications_land_in_the_host_event_log() {
    let (host, project, app) = lsp_host("diagnostics").await;
    let (mut reader, mut writer) = authenticated(&host).await;

    // 任意一次会话请求都会让引擎拉起 server 并完成握手;握手后的
    // publishDiagnostics 通知必须经 sink 落 EventLog。
    success(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_hover",
            request_body(&app, &project),
        )
        .await,
    );

    let mut found = None;
    for _ in 0..40 {
        let events = success(
            call(
                &mut reader,
                &mut writer,
                "events.since",
                json!({ "after": "0" }),
            )
            .await,
        );
        if let Some(event) = events["events"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|event| event["event"] == "lsp://diagnostics")
        {
            found = Some(event["data"].clone());
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let payload =
        found.expect("the fake server's diagnostics notification must reach the EventLog");
    let canonical_app = app.canonicalize().expect("canonical app");
    assert_eq!(
        payload["filePath"],
        canonical_app.to_string_lossy().to_string(),
        "{payload}"
    );
    assert_eq!(payload["diagnostics"][0]["source"], "lsp:fake", "{payload}");
    assert_eq!(payload["diagnostics"][0]["line"], 3, "{payload}");
    assert_eq!(payload["diagnostics"][0]["column"], 5, "{payload}");
    assert_eq!(payload["diagnostics"][0]["code"], "4242", "{payload}");
}

#[tokio::test(flavor = "multi_thread")]
async fn malformed_and_remote_targets_refuse_before_starting_a_server() {
    let (host, project, app) = lsp_host("refuse").await;
    let (mut reader, mut writer) = authenticated(&host).await;

    failure(
        call(&mut reader, &mut writer, "ide.lsp_hover", json!({})).await,
        ErrorCode::InvalidArgument,
    );
    // 远端 target:逐字拒绝,绝不静默回落本地(也不起任何进程)。
    let message = failure(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_hover",
            json!({
                "projectPath": project,
                "filePath": app,
                "content": "const value = 1;\n",
                "line": 0,
                "character": 0,
                "target": { "kind": "ssh", "connectionId": "ssh-1", "remotePath": "/srv/app" }
            }),
        )
        .await,
        ErrorCode::InvalidArgument,
    );
    assert!(message.contains("not hosted"), "{message}");
    // 路径逃逸在引擎之前就被拒(不起进程、不发帧)。
    let outside = PathBuf::from("/tmp").join(format!("ah-lsp-outside-{}.ts", std::process::id()));
    std::fs::write(&outside, "const outside = 1;\n").expect("outside file");
    let message = failure(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_hover",
            json!({
                "projectPath": project,
                "filePath": outside,
                "content": "const outside = 1;\n",
                "line": 0,
                "character": 0
            }),
        )
        .await,
        ErrorCode::IoError,
    );
    assert_eq!(
        message, "document path is outside project root",
        "{message}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn apply_workspace_edit_writes_and_reports_counts() {
    let (host, project, app) = lsp_host("apply").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let canonical_app = app.canonicalize().expect("canonical app");

    let summary = success(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_apply_workspace_edit",
            json!({
                "projectPath": project,
                "edit": {
                    "files": [{
                        "uri": format!("file://{}", canonical_app.display()),
                        "path": canonical_app,
                        "edits": [{
                            "range": {
                                "start": { "line": 0, "character": 6 },
                                "end": { "line": 0, "character": 11 }
                            },
                            "newText": "total"
                        }]
                    }]
                }
            }),
        )
        .await,
    );
    assert_eq!(summary["filesChanged"], 1, "{summary}");
    assert_eq!(summary["editsApplied"], 1, "{summary}");
    assert_eq!(summary["editsSkipped"], 0, "{summary}");
    assert_eq!(
        std::fs::read_to_string(&canonical_app).expect("read back"),
        "const total: number = 1;\n"
    );
}

/// 切片一百一十反思(独立审查 P1):关文档/关项目的**会话回收必须是宿主侧
/// 的承重实现**。此前它只写在 Tauri 的 Embedded 闭包里,而 External 判决下
/// 闭包根本不会被 poll —— 宿主进程里的 `LocalLspSession` 会留在静态表里、
/// 子进程与管道照旧活着(每个项目一份常驻语言服务器)。
///
/// 判据取**真进程的存活性**:fake server 收到 `shutdown` 会回一帧,随后收到
/// `exit` 时自我了断。关掉最后一个文档后,宿主端必须已经把会话取出并 shutdown,
/// 于是 `take_local_lsp_sessions_with_prefix("local:")` 为空、且那份录音的
/// stdin 已关(进程随后退出)。这里用「快照表里已经没有该会话」作为机器可读的
/// 主判据,「进程退出」作为辅证。
#[tokio::test(flavor = "multi_thread")]
async fn closing_the_last_document_tears_the_session_down_on_the_host() {
    let (host, project, app) = lsp_host("teardown").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let body = request_body(&app, &project);

    // 先真起一次会话(握手 + publishDiagnostics),证明 server 确实活着。
    success(call(&mut reader, &mut writer, "ide.lsp_hover", body.clone()).await);

    let opened = success(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_open_document",
            json!({
                "projectPath": project,
                "filePath": app,
                "content": "const value: number = 1;\n",
                "version": 1
            }),
        )
        .await,
    );
    assert_eq!(opened["openDocuments"], 1, "{opened}");

    let closed = success(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_close_document",
            json!({ "projectPath": project, "filePath": app }),
        )
        .await,
    );
    assert_eq!(closed["openDocuments"], 0, "{closed}");

    // 会话表必须在宿主进程里被清空(回收发生在宿主,不依赖任何 Tauri 闭包)。
    // shutdown 是异步收尾,给它一个有界窗口。
    let mut drained = false;
    for _ in 0..40 {
        if engine::take_local_lsp_sessions_with_prefix("local:").is_empty() {
            drained = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        drained,
        "关闭最后一个文档后,宿主必须已经回收该会话(shutdown 不能在 Tauri 闭包那半边)"
    );

    // 关项目同理:幂等到 0,且表里没有残留。
    let shutdown = success(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_shutdown_project",
            json!({ "projectPath": project }),
        )
        .await,
    );
    // 裸标量(不是 {"closedDocuments":N})——Tauri 壳按 usize 解,包成对象在
    // External 模式会解析失败。
    assert_eq!(shutdown, json!(0), "{shutdown}");
    assert!(engine::take_local_lsp_sessions_with_prefix("local:").is_empty());
}

/// 切片一百一十反思(独立审查 P2):`ide.lsp_inlay_hints` 的旧命令走的是
/// **整文档 range**(`RequestParams::DocumentRange`),不是位置型。宿主面若
/// 用 Position,同一前端调用在 Embedded/External 两种模式下会真的发出两种
/// params(Embedded 走 range),对 `textDocument/inlayHint` 这类 range 语义的
/// 请求就是行为分叉。判据:让 fake server 把收到的 params 原样回显,断言
/// 宿主发出去的是 `range` 而非 `position`。
#[tokio::test(flavor = "multi_thread")]
async fn inlay_hints_sends_a_document_range_request() {
    let (host, project, app) = lsp_host("range").await;
    let (mut reader, mut writer) = authenticated(&host).await;

    success(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_inlay_hints",
            request_body(&app, &project),
        )
        .await,
    );

    // fake server 把每次请求的 path+params 落盘(见 fixture):`params` 经宿主
    // 解析成强类型 DTO 后未知字段会被丢掉,所以只能从录音里读真实形状。
    let canonical_app = app.canonicalize().expect("canonical app");
    let recorded: Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{}.params.json", canonical_app.display()))
            .expect("the fake server must have recorded the request"),
    )
    .expect("recorded params are JSON");
    assert_eq!(recorded["method"], "textDocument/inlayHint", "{recorded}");
    let params = &recorded["params"];
    assert!(
        params.get("range").is_some(),
        "inlay hints 必须发整文档 range(legacy 形态),收到的是: {params}"
    );
    assert!(
        params.get("position").is_none(),
        "inlay hints 不得发 position(那是 legacy 没有的形态): {params}"
    );
    // "const value: number = 1;\n" 拆行后是两行,末行(空行)UTF-16 长度 0。
    assert_eq!(params["range"]["start"]["line"], 0, "{params}");
    assert_eq!(params["range"]["start"]["character"], 0, "{params}");
    assert_eq!(params["range"]["end"]["line"], 1, "{params}");
    assert_eq!(params["range"]["end"]["character"], 0, "{params}");
}
