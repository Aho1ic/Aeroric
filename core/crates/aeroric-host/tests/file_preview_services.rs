//! P8-05 slice 92: `files.read_image_preview` + `files.read_clipboard_file_paths`.
//! Real UDS, scratch project roots. The clipboard case runs on the default
//! (NoPlatform) context and asserts the neutral empty list — it must never
//! touch the real system clipboard.

use std::fs;
use std::path::{Path, PathBuf};

use aeroric_host::dispatcher::HostWiring;
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

struct Rig {
    host: Harness,
    project: PathBuf,
}

impl Rig {
    async fn start(tag: &str) -> Self {
        let wiring = |data: &Path| {
            let project = data.join("preview project");
            fs::create_dir_all(&project).unwrap();
            fs::write(
                data.join("projects.json"),
                serde_json::to_vec(&json!([{"id": "p", "path": project, "name": "previews"}]))
                    .unwrap(),
            )
            .unwrap();
            HostWiring::default()
        };
        let host = Harness::start_read_only_with_wiring(tag, wiring).await;
        Self {
            project: host
                .data_dir
                .join("preview project")
                .canonicalize()
                .unwrap(),
            host,
        }
    }

    async fn connect(&self) -> (BufReader<OwnedReadHalf>, OwnedWriteHalf) {
        let (mut reader, mut writer) = self.host.connect().await;
        let reply =
            Harness::handshake(&mut reader, &mut writer, &self.host.token, "file-previews").await;
        assert!(matches!(reply, Frame::Success(_)), "{reply:?}");
        (reader, writer)
    }
}

async fn query(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    id: &str,
    method: &str,
    params: Value,
) -> Frame {
    let line = serde_json::json!({
        "v": 1,
        "id": id,
        "method": method,
        "params": params,
    });
    use tokio::io::AsyncWriteExt;
    writer
        .write_all(format!("{line}\n").as_bytes())
        .await
        .expect("write request");
    let mut line = String::new();
    use tokio::io::AsyncBufReadExt;
    reader.read_line(&mut line).await.expect("read reply");
    serde_json::from_str::<Frame>(line.trim()).expect("decode reply")
}

fn success_result(reply: Frame) -> Value {
    match reply {
        Frame::Success(success) => success.result,
        Frame::Failure(failure) => panic!("expected success, got failure: {failure:?}"),
        other => panic!("expected success frame, got {other:?}"),
    }
}

const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x62, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

#[tokio::test]
async fn files_read_image_preview_returns_data_url_and_survives_oversized() {
    let rig = Rig::start("file-image-preview").await;
    let (mut reader, mut writer) = rig.connect().await;

    fs::write(rig.project.join("tiny.png"), TINY_PNG).expect("write tiny png");

    let reply = query(
        &mut reader,
        &mut writer,
        "img-1",
        "files.read_image_preview",
        json!({ "projectId": "p", "path": "tiny.png" }),
    )
    .await;
    let result = success_result(reply);
    let data_url = result["dataUrl"].as_str().expect("dataUrl");
    assert!(
        data_url.starts_with("data:image/png;base64,"),
        "{data_url:?}"
    );
    assert_eq!(result["mimeType"], "image/png", "{result}");
    assert_eq!(result["byteLength"], TINY_PNG.len() as u64, "{result}");

    // An image whose base64 reply would exceed the 1 MiB UDS frame limit gets a
    // CLEAN error before any oversized frame is written — the connection must
    // survive it (the next query on the same connection still works).
    fs::write(rig.project.join("big.png"), vec![0u8; 1_500_000]).expect("write big png");
    let reply_big = query(
        &mut reader,
        &mut writer,
        "img-big",
        "files.read_image_preview",
        json!({ "projectId": "p", "path": "big.png" }),
    )
    .await;
    let Frame::Failure(failure_big) = reply_big else {
        panic!("oversized image must fail cleanly");
    };
    assert_eq!(failure_big.error.code, ErrorCode::InvalidArgument);
    assert!(
        failure_big.error.message.contains("1 MiB"),
        "error must name the frame limit: {failure_big:?}"
    );

    // The same connection still works after the oversized refusal.
    let reply_after = query(
        &mut reader,
        &mut writer,
        "img-after",
        "files.read_image_preview",
        json!({ "projectId": "p", "path": "tiny.png" }),
    )
    .await;
    let _ = success_result(reply_after);

    // Escape path is refused before any read.
    let reply_escape = query(
        &mut reader,
        &mut writer,
        "img-escape",
        "files.read_image_preview",
        json!({ "projectId": "p", "path": "../outside.png" }),
    )
    .await;
    let Frame::Failure(failure_escape) = reply_escape else {
        panic!("escape path must fail");
    };
    assert_eq!(failure_escape.error.code, ErrorCode::InvalidArgument);

    // Missing path -> InvalidArgument
    let reply_missing = query(
        &mut reader,
        &mut writer,
        "img-missing",
        "files.read_image_preview",
        json!({ "projectId": "p" }),
    )
    .await;
    let Frame::Failure(failure_missing) = reply_missing else {
        panic!("missing path must fail");
    };
    assert_eq!(failure_missing.error.code, ErrorCode::InvalidArgument);
}

#[tokio::test]
async fn files_read_clipboard_file_paths_returns_empty_list_by_default() {
    let rig = Rig::start("file-clipboard").await;
    let (mut reader, mut writer) = rig.connect().await;

    // The read-only harness context carries the NoPlatform default: the neutral
    // answer is an empty list (the port contract for "no capability"), never an
    // error and never the real system clipboard.
    let reply = query(
        &mut reader,
        &mut writer,
        "clip-1",
        "files.read_clipboard_file_paths",
        json!({}),
    )
    .await;
    assert_eq!(success_result(reply), json!([]));
}
