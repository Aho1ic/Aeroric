//! P7-06 验收：notebook 同步引擎契约（`tests/notebook_sync.rs`）。
//!
//! fake storage 验证：上传/下载往返、双端改动 → Pending（等用户）、解决后
//! 下一轮执行、断线 → Failed 进退避、**重复 index 不重复计费**（seq 只在
//! 全部落定时推进）。既有单测（engine/diff/store/scan/daemon 搬移自宿主）
//! 在 `--lib` 里跑；本文件是跨模块契约层。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use aeroric_core::notebook::sync::diff::ConflictStrategy;
use aeroric_core::notebook::sync::diff::RemoteEntry;
use aeroric_core::notebook::sync::engine::{self, OutcomeStatus, RemoteFs};
use aeroric_core::notebook::sync::store;

fn temp_vault(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "aeroric-sync-contract-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create vault");
    std::fs::canonicalize(&dir).expect("canonicalize")
}

fn bind_remote(vault: &Path, id: &str) {
    let conn = store::open(vault).expect("open sync.db");
    store::upsert_remote(&conn, id, "cloud", "/notes", "conn-1").expect("bind");
}

/// 内存假远端：内容 + 注入失败模式（断线/配额）。
struct FakeRemote {
    files: BTreeMap<String, Vec<u8>>,
    seq: i64,
    device: String,
    /// 非 None 时所有操作都失败（模拟断线 / quota）。
    fail_with: Option<String>,
}

impl FakeRemote {
    fn new() -> Self {
        Self {
            files: BTreeMap::new(),
            seq: 0,
            device: "device-b".to_string(),
            fail_with: None,
        }
    }

    fn put_file(&mut self, path: &str, bytes: &[u8]) {
        self.files.insert(path.to_string(), bytes.to_vec());
    }

    fn hash(bytes: &[u8]) -> String {
        aeroric_core::notebook::state::hash64(bytes).to_string()
    }
}

impl RemoteFs for FakeRemote {
    fn list(&self) -> Result<Vec<RemoteEntry>, String> {
        if let Some(err) = &self.fail_with {
            return Err(err.clone());
        }
        Ok(self
            .files
            .iter()
            .map(|(path, bytes)| RemoteEntry {
                path: path.clone(),
                hash: Self::hash(bytes),
                device: self.device.clone(),
                seq: self.seq,
            })
            .collect())
    }

    fn head(&self, path: &str) -> Result<Option<String>, String> {
        if let Some(err) = &self.fail_with {
            return Err(err.clone());
        }
        Ok(self.files.get(path).map(|b| Self::hash(b)))
    }

    fn get(&self, path: &str) -> Result<Vec<u8>, String> {
        if let Some(err) = &self.fail_with {
            return Err(err.clone());
        }
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| format!("not found: {path}"))
    }

    fn put(&mut self, path: &str, bytes: &[u8], _hash: &str) -> Result<(), String> {
        if let Some(err) = &self.fail_with {
            return Err(err.clone());
        }
        self.files.insert(path.to_string(), bytes.to_vec());
        self.seq += 1;
        Ok(())
    }

    fn delete(&mut self, path: &str) -> Result<(), String> {
        if let Some(err) = &self.fail_with {
            return Err(err.clone());
        }
        self.files.remove(path);
        Ok(())
    }
}

fn run_round(
    vault: &Path,
    remote: &mut FakeRemote,
    strategy: ConflictStrategy,
) -> engine::SyncReport {
    let mut local = aeroric_core::notebook::sync::local::VaultLocalFs;
    engine::run(vault, "r1", strategy, 1_756_700_000_000, remote, &mut local).expect("engine run")
}

fn status_of<'a>(report: &'a engine::SyncReport, path: &str) -> &'a OutcomeStatus {
    &report
        .outcomes
        .iter()
        .find(|o| o.path == path)
        .unwrap_or_else(|| panic!("no outcome for {path}"))
        .status
}

// ── 上传 / 下载往返 ──────────────────────────────────────────────────────────

#[test]
fn a_local_note_uploads_and_settles_the_seq() {
    let vault = temp_vault("upload");
    bind_remote(&vault, "r1");
    std::fs::write(vault.join("a.md"), "# local\n").expect("write note");

    let mut remote = FakeRemote::new();
    let report = run_round(&vault, &mut remote, ConflictStrategy::Local);
    assert!(matches!(status_of(&report, "a.md"), OutcomeStatus::Done));
    assert_eq!(
        remote.files.get("a.md").map(Vec::as_slice),
        Some(b"# local\n".as_slice())
    );
    assert_eq!(report.seq, Some(1), "全部落定才推进 seq");
    let _ = std::fs::remove_dir_all(vault);
}

#[test]
fn a_remote_note_downloads_through_the_real_local_fs() {
    let vault = temp_vault("download");
    bind_remote(&vault, "r1");
    let mut remote = FakeRemote::new();
    remote.put_file("b.md", b"# from remote\n");

    let report = run_round(&vault, &mut remote, ConflictStrategy::Local);
    assert!(matches!(status_of(&report, "b.md"), OutcomeStatus::Done));
    assert_eq!(
        std::fs::read_to_string(vault.join("b.md")).expect("downloaded"),
        "# from remote\n"
    );
    let _ = std::fs::remove_dir_all(vault);
}

// ── 冲突：双端改动挂起等用户，解决后下一轮执行 ──────────────────────────────

#[test]
fn both_sides_changed_pends_then_resolves_and_applies_next_round() {
    let vault = temp_vault("conflict");
    bind_remote(&vault, "r1");
    let mut remote = FakeRemote::new();
    remote.put_file("c.md", b"remote version\n");

    // 第一轮把远端内容拉下来、写进基线。
    let _ = run_round(&vault, &mut remote, ConflictStrategy::Local);
    // 双端各改各的。
    std::fs::write(vault.join("c.md"), "local version\n").expect("local edit");
    remote.put_file("c.md", b"remote edit\n");

    let report = run_round(&vault, &mut remote, ConflictStrategy::Ask);
    assert!(
        matches!(status_of(&report, "c.md"), OutcomeStatus::Pending { .. }),
        "ask 策略下双端改动挂起等用户: {:?}",
        report.outcomes
    );
    assert_eq!(report.seq, None, "有挂起不推进 seq");

    // 用户裁决 take local；下一轮执行决定。
    let conn = store::open(&vault).expect("open");
    let resolution = aeroric_core::notebook::sync::diff::Resolution::KeepLocal;
    store::set_resolution(
        &conn,
        "r1",
        &aeroric_core::notebook::sync::store::StoredResolution {
            path: "c.md".to_string(),
            resolution,
            local_hash: aeroric_core::notebook::state::hash64(b"local version\n").to_string(),
            remote_hash: aeroric_core::notebook::state::hash64(b"remote edit\n").to_string(),
            decided_at: 1_756_700_000_100,
        },
    )
    .expect("decide");

    let mut local = aeroric_core::notebook::sync::local::VaultLocalFs;
    let applied = engine::run(
        &vault,
        "r1",
        ConflictStrategy::Ask,
        1_756_700_000_200,
        &mut remote,
        &mut local,
    )
    .expect("run after decide");
    assert!(
        matches!(status_of(&applied, "c.md"), OutcomeStatus::Done),
        "决定后执行: {:?}",
        applied.outcomes
    );
    assert_eq!(
        remote.files.get("c.md").map(Vec::as_slice),
        Some(b"local version\n".as_slice()),
        "take local ＝ 把本地内容推上远端"
    );
    let _ = std::fs::remove_dir_all(vault);
}

// ── 断线：Failed 进退避，不推进 seq ─────────────────────────────────────────

#[test]
fn a_disconnected_remote_fails_without_advancing_the_seq() {
    let vault = temp_vault("disconnect");
    bind_remote(&vault, "r1");
    std::fs::write(vault.join("a.md"), "# local\n").expect("write");

    let mut remote = FakeRemote::new();
    remote.fail_with = Some("network unreachable".to_string());
    // 整轮失败＝Err（清单读不到，快照不完整——不是「那些文件都被删了」）。
    // daemon 的 run_once 把 Err 归为 RunResult::Failed 进退避。
    let error = {
        let mut local = aeroric_core::notebook::sync::local::VaultLocalFs;
        engine::run(
            &vault,
            "r1",
            ConflictStrategy::Local,
            1_756_700_000_000,
            &mut remote,
            &mut local,
        )
        .expect_err("断线整轮失败")
    };
    assert!(
        error.contains("network unreachable"),
        "断线错误原样上抛: {error}"
    );
    let _ = std::fs::remove_dir_all(vault);
}

// ── 重复 index 不重复计费：空库差 → 空计划，不写不推进 ─────────────────────

#[test]
fn an_unchanged_vault_produces_an_empty_plan_and_no_double_charge() {
    let vault = temp_vault("idempotent");
    bind_remote(&vault, "r1");
    std::fs::write(vault.join("same.md"), "stable\n").expect("write");

    let mut remote = FakeRemote::new();
    let first = run_round(&vault, &mut remote, ConflictStrategy::Local);
    assert_eq!(first.seq, Some(1));
    let seq_after_first = remote.seq;

    let second = run_round(&vault, &mut remote, ConflictStrategy::Local);
    assert!(
        second.plan.actions.is_empty(),
        "基线一致 → 空计划: {:?}",
        second.plan.actions
    );
    assert_eq!(
        remote.seq, seq_after_first,
        "空跑不产生任何远端写入（不重复计费）"
    );
    assert_eq!(second.seq, Some(2), "落定轮推进逻辑钟——但零写入、零上传");
    let _ = std::fs::remove_dir_all(vault);
}

// ── G03：daemon 可关停监督 ──────────────────────────────────────────────────

#[test]
fn the_sync_daemon_stops_and_joins_on_drop() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    let vault = temp_vault("daemon-stop");
    let state = Arc::new(aeroric_core::notebook::state::NotebookState::default());
    state.register_vault(&vault).expect("register");
    let fired = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&fired);
    let sink: aeroric_core::notebook::sync::daemon::SyncEventSink = Arc::new(move || {
        counter.fetch_add(1, Ordering::Relaxed);
    });

    let mut handle =
        aeroric_core::notebook::sync::daemon::start(Arc::clone(&state), vault.clone(), sink);
    assert!(!handle.is_stopped());
    // 不给 vault 绑远端 → 循环空转；stop 必须在有限时间内 join（≤MAX_TICK）。
    handle.stop();
    assert!(handle.is_stopped());
    std::thread::sleep(Duration::from_millis(10));
    let _ = std::fs::remove_dir_all(vault);
}
