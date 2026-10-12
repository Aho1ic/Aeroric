//! Slice 106: the tests command family over live UDS. A registered method is
//! not proof of working discovery or a real test run: this drives both
//! commands against a scratch cargo project and asserts the wire shapes the
//! React side already consumes.

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

async fn authenticated(host: &Harness) -> (BufReader<OwnedReadHalf>, OwnedWriteHalf) {
    let (mut reader, mut writer) = host.connect().await;
    success(Harness::handshake(&mut reader, &mut writer, &host.token, "idetest").await);
    (reader, writer)
}

async fn call(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    method: &str,
    params: Value,
) -> Frame {
    Harness::call(reader, writer, &Harness::request("idetest", method, params)).await
}

static SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch_cargo_project() -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let root = PathBuf::from("/tmp").join(format!("ah-idetest-{}-{n}", std::process::id()));
    std::fs::create_dir_all(root.join("src")).expect("project dir");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"scratch-tests\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("manifest");
    std::fs::write(
        root.join("src/lib.rs"),
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn adds() {\n        assert_eq!(1 + 1, 2);\n    }\n}\n",
    )
    .expect("lib");
    root
}

#[tokio::test(flavor = "multi_thread")]
async fn discover_and_run_tests_work_over_live_uds() {
    let host = Harness::start("idetest").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let project = scratch_cargo_project();

    let discovery = success(
        call(
            &mut reader,
            &mut writer,
            "ide.discover_tests",
            json!({"projectPath": project}),
        )
        .await,
    );
    let profiles = discovery["profiles"].as_array().expect("profiles array");
    assert!(
        profiles
            .iter()
            .any(|profile| profile["id"] == "cargo" && profile["label"] == "Cargo"),
        "cargo profile must be discovered: {profiles:?}"
    );

    // A real `cargo test` run against the scratch project: the wire carries
    // the summary the React side renders.
    let run = success(
        call(
            &mut reader,
            &mut writer,
            "ide.run_tests",
            json!({"projectPath": project, "profile": "cargo"}),
        )
        .await,
    );
    assert_eq!(run["status"], "passed", "{run}");
    assert!(!run["tests"].as_array().expect("cases").is_empty());
    assert!(
        run["tests"]
            .as_array()
            .unwrap()
            .iter()
            .any(|case| case["name"].as_str().unwrap_or("").contains("adds")),
        "the scratch test must be among the cases: {run}"
    );
    assert_eq!(run["profile"], "cargo");
    assert!(run["rawOutput"].as_str().is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn malformed_requests_refuse_without_project_io() {
    let host = Harness::start("idebad").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    failure(
        call(&mut reader, &mut writer, "ide.discover_tests", json!({})).await,
        ErrorCode::InvalidArgument,
    );
    failure(
        call(&mut reader, &mut writer, "ide.run_tests", json!({})).await,
        ErrorCode::InvalidArgument,
    );
    // A root with no detected runner keeps its legacy refusal text (IoError).
    let missing = host.home.join("no-runners");
    std::fs::create_dir_all(&missing).expect("empty root");
    let message = failure(
        call(
            &mut reader,
            &mut writer,
            "ide.run_tests",
            json!({"projectPath": missing, "profile": "cargo"}),
        )
        .await,
        ErrorCode::IoError,
    );
    assert_eq!(message, "Cargo tests were not detected", "{message}");
}

fn failure(reply: Frame, code: ErrorCode) -> String {
    let Frame::Failure(reply) = reply else {
        panic!("expected failure, got {reply:?}");
    };
    assert_eq!(reply.error.code, code, "{reply:?}");
    reply.error.message
}

#[tokio::test(flavor = "multi_thread")]
async fn run_diagnostics_parses_real_cargo_check_output() {
    let host = Harness::start("idediag").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let project = PathBuf::from("/tmp").join(format!("ah-idediag-{}-{n}", std::process::id()));
    std::fs::create_dir_all(project.join("src")).expect("project dir");
    std::fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"scratch-diag\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("manifest");
    std::fs::write(
        project.join("src/main.rs"),
        "fn main() { let _x: i32 = \"not a number\"; }\n",
    )
    .expect("main");
    let result = success(
        call(
            &mut reader,
            &mut writer,
            "ide.run_diagnostics",
            json!({"projectPath": project, "profile": "cargo"}),
        )
        .await,
    );
    assert_eq!(result["profile"], "cargo");
    let diagnostics = result["diagnostics"].as_array().expect("diagnostics");
    assert!(
        diagnostics
            .iter()
            .any(|item| item["source"] == "cargo"
                && !item["message"].as_str().unwrap_or("").is_empty()),
        "a real cargo check diagnostic must be parsed: {result:?}"
    );
    failure(
        call(&mut reader, &mut writer, "ide.run_diagnostics", json!({})).await,
        ErrorCode::InvalidArgument,
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn run_config_processes_start_read_and_stop_over_live_uds() {
    let host = Harness::start("iderun").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let project = host.home.join("run-project");
    std::fs::create_dir_all(&project).expect("project dir");
    std::fs::write(
        project.join("run-configs.json"),
        r#"{"version":1,"configs":[{"type":"shell","id":"shell-1","name":"Echo","command":"echo run-face-output","cwd":".","env":{}}]}"#,
    )
    .expect("run configs");
    let config = json!({
        "type": "shell", "id": "shell-1", "name": "Echo",
        "command": "echo run-face-output", "cwd": ".", "env": {}
    });

    let started = success(
        call(
            &mut reader,
            &mut writer,
            "ide.start_run_config",
            json!({"projectPath": project, "config": config}),
        )
        .await,
    );
    assert_eq!(started["status"], "running", "{started}");
    assert_eq!(started["name"], "Echo");
    let run_id = started["runId"].as_str().expect("runId").to_string();

    // Output capture: the echo lands in the snapshot output ring.
    let mut captured = String::new();
    for _ in 0..50 {
        let snapshot = success(
            call(
                &mut reader,
                &mut writer,
                "ide.read_run_process",
                json!({"runId": run_id}),
            )
            .await,
        );
        captured = snapshot["output"].as_str().unwrap_or("").to_string();
        if captured.contains("run-face-output") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        captured.contains("run-face-output"),
        "output must be captured: {captured:?}"
    );

    let stopped = success(
        call(
            &mut reader,
            &mut writer,
            "ide.stop_run_config",
            json!({"runId": run_id}),
        )
        .await,
    );
    assert_eq!(stopped["status"], "stopped", "{stopped}");
    failure(
        call(
            &mut reader,
            &mut writer,
            "ide.read_run_process",
            json!({"runId": "run-missing"}),
        )
        .await,
        ErrorCode::IoError,
    );
}

/// Slice 109: the LSP lifecycle face over live UDS. `lsp_server_status` is a
/// pure probe (language id + default command + PATH availability) and
/// open/change/close/shutdown are pure registry bookkeeping, so a scratch
/// project with one `.ts` file exercises the whole face without a live
/// language server.
#[tokio::test(flavor = "multi_thread")]
async fn lsp_lifecycle_face_works_over_live_uds() {
    let host = Harness::start("idelsp").await;
    let (mut reader, mut writer) = authenticated(&host).await;
    let project = host.home.join("lsp-project");
    std::fs::create_dir_all(project.join("src")).expect("project dir");
    let ts_file = project.join("src").join("App.ts");
    std::fs::write(&ts_file, "const value: number = 1;\n").expect("ts file");

    // (1) status: supported language id + the default server command, and the
    // install hint reflects the real PATH probe outcome.
    let status = success(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_server_status",
            json!({"projectPath": project, "filePath": ts_file}),
        )
        .await,
    );
    assert_eq!(status["supported"], true, "{status}");
    assert_eq!(status["languageId"], "typescript", "{status}");
    assert_eq!(
        status["command"]["program"], "typescript-language-server",
        "{status}"
    );
    assert_eq!(status["command"]["args"][0], "--stdio", "{status}");
    // The availability probe reads the caller's PATH, so this pins the
    // contract (hint present exactly when unavailable) rather than assuming
    // the developer's machine lacks typescript-language-server.
    if status["available"] == true {
        assert!(status["installHint"].is_null(), "{status}");
    } else {
        assert_eq!(
            status["installHint"], "pnpm add -D typescript-language-server typescript",
            "{status}"
        );
    }

    // (2) open → change → close: the summary carries the registry's open count,
    // so a leak would show up here rather than only in the final shutdown.
    let opened = success(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_open_document",
            json!({
                "projectPath": project,
                "filePath": ts_file,
                "content": "const value: number = 1;\n",
                "version": 1
            }),
        )
        .await,
    );
    assert_eq!(opened["languageId"], "typescript", "{opened}");
    assert_eq!(opened["version"], 1, "{opened}");
    assert_eq!(opened["openDocuments"], 1, "{opened}");

    let changed = success(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_change_document",
            json!({
                "projectPath": project,
                "filePath": ts_file,
                "content": "const value: number = 2;\n",
                "version": 2
            }),
        )
        .await,
    );
    assert_eq!(changed["version"], 2, "{changed}");
    assert_eq!(changed["openDocuments"], 1, "{changed}");

    let closed = success(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_close_document",
            json!({"projectPath": project, "filePath": ts_file}),
        )
        .await,
    );
    assert_eq!(closed["version"], 2, "the last version is echoed: {closed}");
    assert_eq!(closed["openDocuments"], 0, "session drained: {closed}");

    // (3) shutdown closes whatever is still open for that project root.
    success(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_open_document",
            json!({
                "projectPath": project,
                "filePath": ts_file,
                "content": "const value: number = 3;\n",
                "version": 3
            }),
        )
        .await,
    );
    let shutdown = success(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_shutdown_project",
            json!({"projectPath": project}),
        )
        .await,
    );
    // 裸标量:legacy 的 `lsp_shutdown_project` 返回 `usize`,Tauri 壳按 usize
    // 解;这里曾经钉成 `{"closedDocuments":N}`——那是宿主自己发明的形状,两条
    // 边各测各的所以都没红,External 模式下真实调用会解析失败(切片110复盘)。
    assert_eq!(shutdown, json!(1), "{shutdown}");
    let reopened = success(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_shutdown_project",
            json!({"projectPath": project}),
        )
        .await,
    );
    assert_eq!(reopened, json!(0), "idempotent: {reopened}");

    // (4) unsupported extension and path escape keep their legacy wording.
    let unsupported = project.join("src").join("main.rs");
    std::fs::write(&unsupported, "fn main() {}\n").expect("rs file");
    let message = failure(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_server_status",
            json!({"projectPath": project, "filePath": unsupported}),
        )
        .await,
        ErrorCode::IoError,
    );
    assert_eq!(
        message, "language server is not supported for this file type",
        "{message}"
    );

    let outside = host.home.join("outside.ts");
    std::fs::write(&outside, "const outside = 1;\n").expect("outside file");
    let message = failure(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_open_document",
            json!({
                "projectPath": project,
                "filePath": outside,
                "content": "const outside = 1;\n",
                "version": 1
            }),
        )
        .await,
        ErrorCode::IoError,
    );
    assert_eq!(
        message, "document path is outside project root",
        "{message}"
    );

    // (5) malformed requests refuse before any project IO.
    failure(
        call(&mut reader, &mut writer, "ide.lsp_open_document", json!({})).await,
        ErrorCode::InvalidArgument,
    );
    failure(
        call(
            &mut reader,
            &mut writer,
            "ide.lsp_shutdown_project",
            json!({}),
        )
        .await,
        ErrorCode::InvalidArgument,
    );
}
