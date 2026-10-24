//! 无限画布的宿主面（UI 改版 M5）：`canvas.boards.*` 与 `canvas.nodes.*`。只在本机 UDS 注册，
//! 不进 remote rpc。所有写入经 core `CanvasStore`（原子写、全局 revision）；SwiftUI 不直写文件。

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use aeroric_core::canvas::{CanvasError, CanvasStore, Node, NodeMove};
use aeroric_core::context::CoreContext;
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::dispatcher::{failure, Handler, ServiceSet};
use crate::services::ok;

pub const CAPABILITY: &str = "canvas";

pub fn register_canvas_services(set: &mut ServiceSet, context: Arc<CoreContext>) {
    set.register("canvas.boards.list", blocking(&context, boards_list));
    set.register("canvas.boards.create", blocking(&context, boards_create));
    set.register("canvas.boards.update", blocking(&context, boards_update));
    set.register("canvas.boards.delete", blocking(&context, boards_delete));
    set.register("canvas.nodes.list", blocking(&context, nodes_list));
    set.register("canvas.nodes.upsert", blocking(&context, nodes_upsert));
    set.register("canvas.nodes.move", blocking(&context, nodes_move));
    set.register("canvas.nodes.delete", blocking(&context, nodes_delete));
}

fn blocking(
    context: &Arc<CoreContext>,
    run: impl Fn(Request, Arc<CoreContext>) -> Frame + Send + Sync + 'static,
) -> Handler {
    let context = Arc::clone(context);
    let run = Arc::new(run);
    Arc::new(move |request: Request, _call| {
        let context = Arc::clone(&context);
        let run = Arc::clone(&run);
        Box::pin(async move {
            let id = request.id.clone();
            match tokio::task::spawn_blocking(move || run(request, context)).await {
                Ok(frame) => frame,
                Err(e) => failure(id, ErrorCode::IoError, &e.to_string()),
            }
        })
    })
}

fn store(context: &CoreContext) -> CanvasStore {
    CanvasStore::new(context.paths.data_dir())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn canvas_failure(request: &Request, error: CanvasError) -> Frame {
    let code = match error {
        CanvasError::NotFound(_) => ErrorCode::NotFound,
        CanvasError::Invalid(_) | CanvasError::Limit(_) => ErrorCode::InvalidArgument,
        CanvasError::Io(_) => ErrorCode::IoError,
    };
    failure(request.id.clone(), code, &error.to_string())
}

fn bad_params(request: &Request, what: &str) -> Frame {
    failure(request.id.clone(), ErrorCode::InvalidArgument, what)
}

fn string_param<'a>(request: &'a Request, key: &str) -> Option<&'a str> {
    request.params.get(key).and_then(Value::as_str)
}

fn typed_param<T: DeserializeOwned>(request: &Request, key: &str) -> Option<T> {
    serde_json::from_value(request.params.get(key)?.clone()).ok()
}

/// Absent → keep (`None`); JSON null → clear (`Some(None)`); string → set.
fn patch_param(request: &Request, key: &str) -> Option<Option<String>> {
    match request.params.get(key)? {
        Value::Null => Some(None),
        Value::String(text) => Some(Some(text.clone())),
        _ => None,
    }
}

fn boards_list(request: Request, context: Arc<CoreContext>) -> Frame {
    match store(&context).list_boards() {
        Ok((boards, revision)) => ok(&request, json!({ "boards": boards, "revision": revision })),
        Err(error) => canvas_failure(&request, error),
    }
}

fn boards_create(request: Request, context: Arc<CoreContext>) -> Frame {
    let name = string_param(&request, "name").map(str::to_string);
    let color = string_param(&request, "color").map(str::to_string);
    match store(&context).create_board(name, color, now_ms()) {
        Ok((board, revision)) => ok(&request, json!({ "board": board, "revision": revision })),
        Err(error) => canvas_failure(&request, error),
    }
}

fn boards_update(request: Request, context: Arc<CoreContext>) -> Frame {
    let Some(id) = string_param(&request, "id") else {
        return bad_params(&request, "canvas.boards.update requires params.id");
    };
    let result = store(&context).update_board(
        id,
        patch_param(&request, "name"),
        patch_param(&request, "color"),
        now_ms(),
    );
    match result {
        Ok((board, revision)) => ok(&request, json!({ "board": board, "revision": revision })),
        Err(error) => canvas_failure(&request, error),
    }
}

fn boards_delete(request: Request, context: Arc<CoreContext>) -> Frame {
    let Some(id) = string_param(&request, "id") else {
        return bad_params(&request, "canvas.boards.delete requires params.id");
    };
    match store(&context).delete_board(id) {
        Ok(revision) => ok(&request, json!({ "revision": revision })),
        Err(error) => canvas_failure(&request, error),
    }
}

fn nodes_list(request: Request, context: Arc<CoreContext>) -> Frame {
    let Some(board_id) = string_param(&request, "boardId") else {
        return bad_params(&request, "canvas.nodes.list requires params.boardId");
    };
    match store(&context).nodes(board_id) {
        Ok((nodes, revision)) => ok(
            &request,
            json!({ "boardId": board_id, "nodes": nodes, "revision": revision }),
        ),
        Err(error) => canvas_failure(&request, error),
    }
}

fn nodes_upsert(request: Request, context: Arc<CoreContext>) -> Frame {
    let Some(node) = typed_param::<Node>(&request, "node") else {
        return bad_params(
            &request,
            "canvas.nodes.upsert requires a well-formed params.node",
        );
    };
    match store(&context).upsert_node(node, now_ms()) {
        Ok((node, revision)) => ok(&request, json!({ "node": node, "revision": revision })),
        Err(error) => canvas_failure(&request, error),
    }
}

fn nodes_move(request: Request, context: Arc<CoreContext>) -> Frame {
    let (Some(board_id), Some(moves)) = (
        string_param(&request, "boardId"),
        typed_param::<Vec<NodeMove>>(&request, "moves"),
    ) else {
        return bad_params(
            &request,
            "canvas.nodes.move requires params.boardId and params.moves",
        );
    };
    match store(&context).move_nodes(board_id, &moves, now_ms()) {
        Ok(revision) => ok(&request, json!({ "revision": revision })),
        Err(error) => canvas_failure(&request, error),
    }
}

fn nodes_delete(request: Request, context: Arc<CoreContext>) -> Frame {
    let (Some(board_id), Some(ids)) = (
        string_param(&request, "boardId"),
        typed_param::<Vec<String>>(&request, "ids"),
    ) else {
        return bad_params(
            &request,
            "canvas.nodes.delete requires params.boardId and params.ids",
        );
    };
    match store(&context).delete_nodes(board_id, &ids, now_ms()) {
        Ok(revision) => ok(&request, json!({ "revision": revision })),
        Err(error) => canvas_failure(&request, error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
    use aeroric_core::paths::AppPaths;
    use aeroric_core::storage_lock::LeaseOwner;
    use std::path::Path;

    struct LocalProbe;
    impl StorageLocationProbe for LocalProbe {
        fn probe(&self, data_dir: &Path) -> StorageLocation {
            StorageLocation {
                directory: data_dir.to_path_buf(),
                volume: VolumeKind::Local,
                private_permissions_enforced: true,
            }
        }
    }

    fn request(method: &str, params: Value) -> Request {
        serde_json::from_value(json!({ "v": 1, "id": "r1", "method": method, "params": params }))
            .unwrap()
    }

    fn result(frame: Frame) -> Value {
        match frame {
            Frame::Success(success) => success.result,
            other => panic!("expected success, got {other:?}"),
        }
    }

    #[test]
    fn boards_and_nodes_round_trip_through_the_host() {
        let root = std::env::temp_dir().join(format!(
            "aeroric-canvas-host-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let paths = AppPaths::from_user_home(root.join("home"));
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        let context = Arc::new(
            CoreContext::acquire(
                paths,
                Arc::new(aeroric_core::clock::SystemClock),
                &LeaseOwner {
                    application: "canvas-test".into(),
                    version: "0-test".into(),
                },
                &LocalProbe,
            )
            .unwrap(),
        );

        let created = result(boards_create(
            request("canvas.boards.create", json!({"name": "规划"})),
            Arc::clone(&context),
        ));
        let board_id = created["board"]["id"].as_str().unwrap().to_string();
        let renamed = result(boards_update(
            request(
                "canvas.boards.update",
                json!({"id": board_id, "color": "blue"}),
            ),
            Arc::clone(&context),
        ));
        assert_eq!(renamed["board"]["name"], "规划", "an absent name is kept");
        assert_eq!(renamed["board"]["color"], "blue");

        let node = json!({"id": "", "boardId": board_id, "kind": "note", "content": "想法",
                          "x": 0.0, "y": 0.0, "width": 200.0, "height": 120.0, "z": 0, "updatedAtMs": 0});
        let saved = result(nodes_upsert(
            request("canvas.nodes.upsert", json!({"node": node})),
            Arc::clone(&context),
        ));
        let node_id = saved["node"]["id"].as_str().unwrap().to_string();
        result(nodes_move(
            request(
                "canvas.nodes.move",
                json!({"boardId": board_id, "moves": [{"id": node_id, "x": 16.0, "y": 24.0}]}),
            ),
            Arc::clone(&context),
        ));
        let listed = result(nodes_list(
            request("canvas.nodes.list", json!({"boardId": board_id})),
            Arc::clone(&context),
        ));
        assert_eq!(listed["nodes"][0]["x"], 16.0);
        assert!(listed["revision"].as_u64().unwrap() >= 4);

        match nodes_upsert(
            request("canvas.nodes.upsert", json!({"node": {"kind": "bogus"}})),
            Arc::clone(&context),
        ) {
            Frame::Failure(_) => {}
            other => panic!("malformed nodes are rejected: {other:?}"),
        }
        result(nodes_delete(
            request(
                "canvas.nodes.delete",
                json!({"boardId": board_id, "ids": [node_id]}),
            ),
            Arc::clone(&context),
        ));
        let boards = result(boards_list(
            request("canvas.boards.list", json!({})),
            Arc::clone(&context),
        ));
        assert_eq!(boards["boards"][0]["nodeCount"], 0);
        result(boards_delete(
            request("canvas.boards.delete", json!({"id": board_id})),
            Arc::clone(&context),
        ));
        match nodes_list(
            request("canvas.nodes.list", json!({"boardId": board_id})),
            Arc::clone(&context),
        ) {
            Frame::Failure(_) => {}
            other => panic!("a deleted board is gone: {other:?}"),
        }
        std::fs::remove_dir_all(&root).ok();
    }
}
