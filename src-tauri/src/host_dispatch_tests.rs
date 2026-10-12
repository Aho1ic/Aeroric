//! P8-05 切片六验收：domain 命令转发面的判决与"绝不回退"。
//!
//! 三条判据，每条都是"写错了不会崩、只会静默跑两个写者"的那一类：
//!
//! 1. **Embedded 走本地**，且本地闭包**恰好被求值一次**（不是"两条都跑一遍"）。
//! 2. **External 走宿主**，且**本地闭包一次都不被求值**——这是"不回退"的机器形状：
//!    不是靠"记得别调用 local"，而是让 local 根本没有机会跑。
//! 3. **宿主说没有这个方法时是 `NotServed`（要协调升级）**，而不是 Transport（要等）；
//!    参数写错（InvalidArgument）**不算**"没有这个方法"，否则一条笔误会提示用户去升级宿主。
//!
//! 加上一条量表判据：`host_method_for` 的拼写必须与宿主真实注册的名字一致——用共享的
//! 脚本化宿主决定"真实"是什么，而不是再抄一份名单。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use aeroric_protocol::error::ErrorCode;
use serde_json::json;

use crate::core_adapter::{
    decide_placement, CoreGateway, CorePlacement, HostAvailability, PlacementProbe,
};
use crate::host_client::scripted::{failure, success, Script, ScriptedHost};
use crate::host_client::HostClient;
use crate::host_dispatch::route_for;

/// 用脚本化宿主造一个真的 `External` 网关（连接是真的，判决来自 `decide_placement`）。
struct ExternalRig {
    host: ScriptedHost,
    gateway: CoreGateway,
}

impl ExternalRig {
    async fn new(tag: &str, script: Script, token: &'static str) -> Self {
        let host = ScriptedHost::start(tag, script, Some(token)).await;
        let client = crate::host_client::HostClient::connect(&host.data_dir, "tauri")
            .await
            .expect("handshake");
        let gateway = CoreGateway::with_host_session_for_test(
            aeroric_core::paths::AppPaths::with_data_dir(&host.data_dir, Some(&host.data_dir)),
            client,
        );
        Self { host, gateway }
    }
}

/// 固定判决的探针：Embedded 用例用它（没有活宿主 ⇒ 内嵌）。
struct FixedPlacementProbe {
    availability: HostAvailability,
}

impl PlacementProbe for FixedPlacementProbe {
    fn availability(&self, _data_dir: &std::path::Path) -> HostAvailability {
        self.availability
    }

    fn attach(&self, _data_dir: &std::path::Path) -> Result<(HostClient, bool), String> {
        Err("unused".to_string())
    }
}

fn script_for_commands() -> Script {
    Arc::new(|method, _params| match method {
        "handshake" => Some(success(json!({ "clientId": "tauri", "capabilities": [] }))),
        "task.run" => Some(success(json!({ "ran": true }))),
        // 宿主没实现这个方法（老宿主）。
        "files.read_dir_entries" => Some(failure(
            ErrorCode::UnsupportedCapability,
            "unknown method: files.read_dir_entries",
        )),
        // 参数写错：**不是**"没有这个方法"。
        "task.cancel" => Some(failure(ErrorCode::InvalidArgument, "Missing param: taskId")),
        _ => None,
    })
}

#[tokio::test]
async fn embedded_runs_the_local_implementation_once() {
    let root =
        std::path::PathBuf::from("/tmp").join(format!("ahd-embedded-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let probe = FixedPlacementProbe {
        availability: HostAvailability::Absent,
    };
    let gateway = CoreGateway::startup_with_probe(
        aeroric_core::paths::AppPaths::with_data_dir(&root, Some(&root)),
        "test",
        &probe,
    );
    assert_eq!(gateway.placement(), CorePlacement::Embedded);

    let calls = AtomicUsize::new(0);
    let result = route_for(&gateway, "files.read_dir", json!({}), async {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(json!({ "local": true }))
    })
    .await
    .expect("local runs");
    assert_eq!(result["local"], json!(true));
    assert_eq!(calls.load(Ordering::SeqCst), 1, "本地实现恰好跑一次");
    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn external_forward_never_evaluates_the_local_implementation() {
    let rig = ExternalRig::new("fwd", script_for_commands(), "t").await;
    assert_eq!(rig.gateway.placement(), CorePlacement::External);

    let calls = AtomicUsize::new(0);
    let result = route_for(&rig.gateway, "task.run", json!({ "taskId": "t1" }), async {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(json!({ "local": true }))
    })
    .await
    .expect("host answers");
    assert_eq!(result["ran"], json!(true), "结果是宿主的");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "external 下本地实现一次都不许跑（这就是『不回退』的机器形状）"
    );
    assert!(rig.host.methods().contains(&"task.run".to_string()));
}

#[tokio::test]
async fn a_missing_host_method_is_not_served_and_says_so() {
    let rig = ExternalRig::new("missing", script_for_commands(), "t").await;
    let error = route_for(&rig.gateway, "files.read_dir_entries", json!({}), async {
        panic!("本地实现不许跑")
    })
    .await
    .expect_err("host does not serve it");
    assert!(
        error.contains("不提供") && error.contains("协调重启"),
        "要说清是能力缺失且需要协调升级：{error}"
    );
}

#[tokio::test]
async fn a_bad_argument_is_not_dressed_up_as_a_missing_method() {
    let rig = ExternalRig::new("badarg", script_for_commands(), "t").await;
    let error = route_for(&rig.gateway, "task.cancel", json!({}), async {
        panic!("本地实现不许跑")
    })
    .await
    .expect_err("invalid argument");
    assert!(!error.contains("协调重启"), "笔误不该提示升级宿主：{error}");
    assert!(error.contains("Missing param"), "原始原因要透出：{error}");
}

// 非 macOS 上 `writer_handover_allowed()` 为 false ⇒ `decide_placement` 短路成
// `Embedded`,External 档走不到(ADR-039 §4 S2)。
#[cfg(target_os = "macos")]
#[tokio::test]
async fn external_without_a_session_refuses_rather_than_running_locally() {
    let root =
        std::path::PathBuf::from("/tmp").join(format!("ahd-nosession-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let refusals = AtomicUsize::new(0);
    let probe = RefusingProbe;
    let gateway = CoreGateway::startup_with_probe(
        aeroric_core::paths::AppPaths::with_data_dir(&root, Some(&root)),
        "test",
        &probe,
    );
    assert_eq!(gateway.placement(), CorePlacement::External);
    let error = route_for(&gateway, "task.run", json!({}), async {
        refusals.fetch_add(1, Ordering::SeqCst);
        Ok(json!({ "local": true }))
    })
    .await
    .expect_err("no session");
    assert!(error.contains("没有可用的宿主连接"), "{error}");
    assert_eq!(refusals.load(Ordering::SeqCst), 0, "连不上也不许在本地跑");
    std::fs::remove_dir_all(&root).ok();
}

/// 说"有活宿主"但连不上的探针（`attach` 失败 ⇒ `External` + 无 session）。
// 只被下面那条 macOS-only 用例使用(ADR-039 §4 S2);不 gate 会在 non-macOS 上成
// `struct is never constructed`(clippy `-D warnings`)。
#[cfg(target_os = "macos")]
struct RefusingProbe;

#[cfg(target_os = "macos")]
impl PlacementProbe for RefusingProbe {
    fn availability(&self, _data_dir: &std::path::Path) -> HostAvailability {
        HostAvailability::Live
    }

    fn attach(&self, _data_dir: &std::path::Path) -> Result<(HostClient, bool), String> {
        Err("connection refused".to_string())
    }
}

/// `decide_placement` 与转发面共用同一个判决：外置的判据只有一条来源。
#[test]
fn the_dispatch_route_follows_the_same_placement_decision() {
    assert_eq!(
        decide_placement(true, HostAvailability::Live),
        CorePlacement::External
    );
    assert_eq!(
        decide_placement(true, HostAvailability::Absent),
        CorePlacement::Embedded
    );
}
