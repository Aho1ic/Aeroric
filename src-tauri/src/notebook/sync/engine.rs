//! 执行一轮同步:扫描 → 写 tombstone → diff → 逐条执行 → 收尾。
//!
//! 远端那一侧抽成 [`RemoteFs`] trait,这一层不知道云盘协议。测试用内存假实现,P8c 用
//! `StorageBackend` 实现它。
//!
//! ## 破坏性动作前就地复验(缺口二的修法)
//!
//! 扫描和执行之间有一段时间 —— 整库同步可能跑几分钟,而用户正在里面写东西。Markio
//! 那边扫完就拿着那份快照一路执行到底,中间不复验,于是三处会吃掉这期间的编辑:
//!
//! ```text
//! delete_local   按过期的「本地没变」判断软删,吃掉刚写的内容
//! download       按过期判断覆盖写,同上
//! upload         读的是新内容,却把快照里的旧 hash 写进基线 —— 基线从此说谎,
//!                下一轮还会再传一次
//! delete_remote  按过期的「本地已删」判断删远端,而用户可能刚把它建回来
//! ```
//!
//! 这里每个动作执行前重新算一次那**一个**文件的签名,和快照对不上就把这条转成冲突,
//! 不往下走。代价是一次单文件 hash。
//!
//! ## tombstone 先落盘,再执行(缺口三的修法)
//!
//! `plan` 之前就把本轮推断出的本地删除写进库(见 [`super::diff::pending_tombstones`])。
//! 等远端删除成功之后再写的话,崩在中间就什么都没留下,下一轮把远端还在的那份当新文件
//! 拉回来 —— 删除复活。顺序是刻意的:宁可留下一条「删除意图」也不能留下空白。

use std::path::Path;

use super::diff::{self, Action, ConflictStrategy, DiffOpts, PlannedAction, RemoteEntry, SyncPlan};
use super::scan::{self, FileSig};
use super::store::{self, Baseline, Tombstone};

/// 远端文件系统。P8c 用 `StorageBackend` 实现,测试用内存假实现。
pub trait RemoteFs {
    /// 远端清单。带内容 hash 与 `(device, seq)` 逻辑戳 —— 来自远端 sidecar manifest,
    /// 不是 provider 的 list 元数据(那里没有 etag,见 `diff` 的模块文档)。
    fn list(&self) -> Result<Vec<RemoteEntry>, String>;
    fn get(&self, path: &str) -> Result<Vec<u8>, String>;
    /// 写入并返回落盘后的内容 hash。
    fn put(&mut self, path: &str, bytes: &[u8], hash: &str) -> Result<(), String>;
    fn delete(&mut self, path: &str) -> Result<(), String>;
    /// 一轮结束时把远端侧的记账落盘(sidecar manifest)。
    ///
    /// 分成独立一步而不是每次 `put` 都写:整份清单每轮写一次就够,逐文件写的话两万个
    /// 文件要把清单来回写两万遍。这样做安全的前提是 **manifest 不充当存在性依据**
    /// —— 见 `manifest` 的模块文档。丢一次 commit 的代价只是下一轮多算几个 hash。
    ///
    /// 默认空实现:内存假实现和不需要记账的传输不必关心。
    fn commit(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// 本地文件系统的破坏性操作。抽出来是为了让「删除走回收站」这一点可测。
pub trait LocalFs {
    /// 软删:进回收站,不是 unlink。远端的一次误删不该让本地内容无法找回。
    fn soft_delete(&mut self, vault: &Path, rel_path: &str) -> Result<(), String>;
    fn write(&mut self, vault: &Path, rel_path: &str, bytes: &[u8]) -> Result<(), String>;
    fn read(&self, vault: &Path, rel_path: &str) -> Result<Vec<u8>, String>;
}

/// 一条动作的执行结果。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionOutcome {
    pub path: String,
    pub reason: &'static str,
    pub status: OutcomeStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum OutcomeStatus {
    Done,
    /// 挂起等用户处理(`ask` 策略下的冲突,或复验发现变了)。
    Pending {
        detail: &'static str,
    },
    Failed {
        error: String,
    },
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub plan: SyncPlan,
    pub outcomes: Vec<ActionOutcome>,
    /// 本轮新写的 tombstone 数。
    pub tombstones_written: usize,
    /// 成功推进到的逻辑序号。有任何挂起或失败时**不推进**,这里是 `None`。
    pub seq: Option<i64>,
}

/// 跑一轮。
///
/// `now_ms` 由调用方给,测试里才能稳定。注意它**只用于 tombstone 的时间戳和显示**,
/// 不参与任何顺序判定 —— 那是「没有 newest」那条设计的一部分。
pub fn run(
    vault: &Path,
    remote_id: &str,
    strategy: ConflictStrategy,
    now_ms: i64,
    remote: &mut dyn RemoteFs,
    local: &mut dyn LocalFs,
) -> Result<SyncReport, String> {
    let conn = store::open(vault)?;
    if store::get_remote(&conn, remote_id)?.is_none() {
        return Err(format!("Unknown notebook sync remote: {remote_id}"));
    }

    // 过期的 tombstone 在这里真删掉,不是只在读时过滤。
    //
    // 放在一轮的最前面有两个理由:这里本来就要按 TTL 判一次「哪些还算数」,先删后读是同一
    // 件事的两半;而且此时还没动任何文件,DELETE 失败就直接返回,代价只是这一轮没开始。
    // 放到收尾去做的话,一次失败会把整份 SyncReport 连带丢掉 —— 那时候文件都已经搬完了,
    // 用户却看不到发生了什么。
    //
    // 这一句是**全库**的(DELETE 不带 remote_id):过期判定只看时间,和挂了几个远端无关,
    // 而挂多个远端时每轮都只清自己那份会让别的远端的旧记录一直留着。
    store::prune_tombstones(&conn, now_ms)?;

    let scanned = scan::scan_vault(vault)?;
    let baselines = store::baselines(&conn, remote_id)?;

    // 先落 tombstone,再动任何东西。顺序见模块文档。
    let live = store::live_tombstones(&conn, remote_id, now_ms)?;
    let pending = diff::pending_tombstones(&scanned, &baselines, &live, now_ms);
    for tomb in &pending {
        store::add_tombstone(&conn, remote_id, tomb)?;
    }
    let tombstones_written = pending.len();

    // tombstone 写完基线也跟着变了(add_tombstone 会清掉同路径的基线),所以重读。
    let baselines = store::baselines(&conn, remote_id)?;
    let live = store::live_tombstones(&conn, remote_id, now_ms)?;

    let remote_entries = remote.list()?;
    let plan = diff::plan(
        &scanned,
        &remote_entries,
        &baselines,
        &live,
        DiffOpts { strategy },
    );

    let snapshot = scan::by_path(scanned);
    let mut outcomes = Vec::new();
    let mut all_settled = true;
    for planned in &plan.actions {
        let outcome = execute(
            vault,
            remote_id,
            planned,
            &snapshot,
            &baselines,
            &remote_entries,
            now_ms,
            &conn,
            remote,
            local,
        );
        if !matches!(outcome.status, OutcomeStatus::Done) {
            all_settled = false;
        }
        outcomes.push(outcome);
    }

    // 这里曾经有一段「两边都没了就清基线」的收尾。它是死的:本地文件消失必然先被
    // `pending_tombstones` 推断出来,而 `add_tombstone` 在同一个事务里就把同路径的基线
    // 删了(基线和 tombstone 互斥,见 store 的两个写入口)。变异测试证实了这一点 ——
    // 去掉那段之后 `a_double_deletion_clears_the_baseline` 依然过。留着只会让人以为
    // 防复活这件事有两道闸门,真出问题时两边都不敢改。

    // 远端记账落盘。放在推进 seq **之前**:commit 失败说明远端那份清单没写上,这一轮
    // 就不算完整。失败不算数据问题(清单可重建),但不能让 seq 说这一轮圆满了。
    if let Err(error) = remote.commit() {
        outcomes.push(ActionOutcome {
            path: String::new(),
            reason: "manifest_commit",
            status: OutcomeStatus::Failed { error },
        });
        all_settled = false;
    }

    // seq 只在全部落定时推进:有挂起或失败还推进的话,对端会以为我们这边已经是完整的
    // 一轮,而实际上有文件没同步上。
    let seq = if all_settled {
        Some(store::bump_seq(&conn, remote_id, now_ms)?)
    } else {
        None
    };

    Ok(SyncReport {
        plan,
        outcomes,
        tombstones_written,
        seq,
    })
}

#[allow(clippy::too_many_arguments)]
fn execute(
    vault: &Path,
    remote_id: &str,
    planned: &PlannedAction,
    snapshot: &std::collections::BTreeMap<String, FileSig>,
    baselines: &[Baseline],
    remote_entries: &[RemoteEntry],
    now_ms: i64,
    conn: &rusqlite::Connection,
    remote: &mut dyn RemoteFs,
    local: &mut dyn LocalFs,
) -> ActionOutcome {
    let path = planned.path.as_str();
    let settle = |status: OutcomeStatus| ActionOutcome {
        path: path.to_string(),
        reason: planned.reason,
        status,
    };
    let failed = |error: String| settle(OutcomeStatus::Failed { error });

    // 复验:这个文件从扫描到现在变了没。
    match verify(vault, path, snapshot) {
        Ok(Drift::Unchanged) => {}
        Ok(Drift::Changed) => {
            // 用户在同步期间改了它。别的判断全是基于旧内容做的,不能往下走。
            return settle(OutcomeStatus::Pending {
                detail: "local_changed_during_sync",
            });
        }
        Ok(Drift::Gone) => {
            // 同步期间被删了。删除会在下一轮被正常推断成 tombstone。
            if !matches!(planned.action, Action::DeleteRemote | Action::DeleteLocal) {
                return settle(OutcomeStatus::Pending {
                    detail: "local_gone_during_sync",
                });
            }
        }
        Ok(Drift::Appeared) => {
            // 计划里认为本地没有它,现在有了。删远端就是删掉用户刚建的东西的另一半。
            return settle(OutcomeStatus::Pending {
                detail: "local_appeared_during_sync",
            });
        }
        Err(error) => return failed(error),
    }

    match &planned.action {
        Action::Upload => {
            // 超大文件没有内容 hash,只有 `oversize:<size>` 这个标记。传上去的话远端
            // manifest 里就是个假 hash,之后每台设备都会看到「对不上」并永远重传。
            // `diff` 已经不为它生成动作(见 diff.rs 里 `is_oversize` 那一段),这里再挡
            // 一道:那是跨模块的约定,而这一层不该依赖别人不出错。
            if snapshot.get(path).is_some_and(|sig| sig.is_oversize()) {
                return settle(OutcomeStatus::Pending {
                    detail: "oversize_not_hashable",
                });
            }
            // 读的是**当前**内容,记的也是当前内容的 hash。复验已经保证它和快照一致,
            // 所以这两者必然对得上 —— 而 Markio 那边记的是快照 hash,基线会说谎。
            let bytes = match local.read(vault, path) {
                Ok(bytes) => bytes,
                Err(error) => return failed(error),
            };
            let hash = crate::notebook::state::hash64(&bytes).to_string();
            if let Err(error) = remote.put(path, &bytes, &hash) {
                return failed(error);
            }
            let sig = snapshot.get(path);
            let base = Baseline {
                path: path.to_string(),
                local_hash: hash.clone(),
                local_mtime_ms: sig.map(|s| s.mtime_ms).unwrap_or(now_ms),
                remote_hash: hash,
                remote_device: match store::device_id(conn) {
                    Ok(id) => id,
                    Err(error) => return failed(error),
                },
                remote_seq: 0,
                synced_at: now_ms,
            };
            match store::set_baseline(conn, remote_id, &base) {
                Ok(()) => settle(OutcomeStatus::Done),
                Err(error) => failed(error),
            }
        }
        Action::Download => {
            let bytes = match remote.get(path) {
                Ok(bytes) => bytes,
                Err(error) => return failed(error),
            };
            if let Err(error) = local.write(vault, path, &bytes) {
                return failed(error);
            }
            let entry = remote_entries.iter().find(|e| e.path == path);
            let hash = crate::notebook::state::hash64(&bytes).to_string();
            let base = Baseline {
                path: path.to_string(),
                local_hash: hash,
                local_mtime_ms: now_ms,
                remote_hash: entry.map(|e| e.hash.clone()).unwrap_or_default(),
                remote_device: entry.map(|e| e.device.clone()).unwrap_or_default(),
                remote_seq: entry.map(|e| e.seq).unwrap_or(0),
                synced_at: now_ms,
            };
            match store::set_baseline(conn, remote_id, &base) {
                Ok(()) => settle(OutcomeStatus::Done),
                Err(error) => failed(error),
            }
        }
        Action::DeleteRemote => {
            if let Err(error) = remote.delete(path) {
                return failed(error);
            }
            // 基线清掉,**但 tombstone 留着**。留着才是防复活的那一半:第三台设备可能
            // 还没看到这次删除,过几天把同一个文件传回来 —— 有 tombstone 才会把删除继续
            // 传播给它,没有的话它会被当成一个全新文件下载回来。TTL 到了自然过期。
            match store::clear_baseline(conn, remote_id, path) {
                Ok(()) => settle(OutcomeStatus::Done),
                Err(error) => failed(error),
            }
        }
        Action::DeleteLocal => {
            if let Err(error) = local.soft_delete(vault, path) {
                return failed(error);
            }
            let base = baselines.iter().find(|b| b.path == path);
            let tomb = Tombstone {
                path: path.to_string(),
                deleted_at: now_ms,
                remote_hash: base.map(|b| b.remote_hash.clone()).unwrap_or_default(),
            };
            // 记 tombstone 而不是只清基线:远端已经没有它了,但我们要记住「这个路径被
            // 删过」,否则第三台设备把它传回来时又会被当成新文件。
            match store::add_tombstone(conn, remote_id, &tomb) {
                Ok(()) => settle(OutcomeStatus::Done),
                Err(error) => failed(error),
            }
        }
        Action::Conflict { resolution } => match resolution {
            None => settle(OutcomeStatus::Pending {
                detail: "awaiting_user",
            }),
            Some(diff::Resolution::KeepLocal) => {
                let upload = PlannedAction {
                    path: planned.path.clone(),
                    action: Action::Upload,
                    reason: planned.reason,
                };
                execute(
                    vault,
                    remote_id,
                    &upload,
                    snapshot,
                    baselines,
                    remote_entries,
                    now_ms,
                    conn,
                    remote,
                    local,
                )
            }
            Some(diff::Resolution::KeepRemote) => {
                // 「远端删了 / 本地改了」这类冲突里远端其实不存在。采用远端 = 接受删除,
                // 不能去下载一个不存在的文件 —— 那会 404 每轮重试,永不收敛。
                let action = if remote_entries.iter().any(|e| e.path == planned.path) {
                    Action::Download
                } else {
                    Action::DeleteLocal
                };
                let next = PlannedAction {
                    path: planned.path.clone(),
                    action,
                    reason: planned.reason,
                };
                execute(
                    vault,
                    remote_id,
                    &next,
                    snapshot,
                    baselines,
                    remote_entries,
                    now_ms,
                    conn,
                    remote,
                    local,
                )
            }
            Some(diff::Resolution::Fork { fork_path }) => {
                // 远端那份另存一份,本地这份照常上传。两边都留,谁都不丢。
                let bytes = match remote.get(path) {
                    Ok(bytes) => bytes,
                    Err(error) => return failed(error),
                };
                if let Err(error) = local.write(vault, fork_path, &bytes) {
                    return failed(error);
                }
                let hash = crate::notebook::state::hash64(&bytes).to_string();
                if let Err(error) = remote.put(fork_path, &bytes, &hash) {
                    return failed(error);
                }
                let fork_base = Baseline {
                    path: fork_path.clone(),
                    local_hash: hash.clone(),
                    local_mtime_ms: now_ms,
                    remote_hash: hash,
                    remote_device: store::device_id(conn).unwrap_or_default(),
                    remote_seq: 0,
                    synced_at: now_ms,
                };
                // fork 出来的那份立刻记基线,否则下一轮把它当全新本地文件再传一次。
                if let Err(error) = store::set_baseline(conn, remote_id, &fork_base) {
                    return failed(error);
                }
                let upload = PlannedAction {
                    path: planned.path.clone(),
                    action: Action::Upload,
                    reason: planned.reason,
                };
                execute(
                    vault,
                    remote_id,
                    &upload,
                    snapshot,
                    baselines,
                    remote_entries,
                    now_ms,
                    conn,
                    remote,
                    local,
                )
            }
        },
    }
}

/// 复验的四种结果。
///
/// 「没了」和「出现了」要分开:前者对删除类动作是无害的(结果一致),对其余动作是危险的;
/// 后者只对删远端危险。混成一个「变了」会让本来能正常完成的删除白白挂起。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drift {
    Unchanged,
    Changed,
    Gone,
    Appeared,
}

fn verify(
    vault: &Path,
    path: &str,
    snapshot: &std::collections::BTreeMap<String, FileSig>,
) -> Result<Drift, String> {
    let current = scan::signature_at(vault, path)?;
    let before = snapshot.get(path);
    Ok(match (before, current) {
        (None, None) => Drift::Unchanged,
        (None, Some(_)) => Drift::Appeared,
        (Some(_), None) => Drift::Gone,
        (Some(before), Some(current)) => {
            if before.hash == current.hash {
                Drift::Unchanged
            } else {
                Drift::Changed
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    const NOW: i64 = 1_760_000_000_000;

    /// 内存假远端。记下每次调用,好断言「有没有真去删」。
    #[derive(Default)]
    struct FakeRemote {
        files: BTreeMap<String, (Vec<u8>, String, String, i64)>,
        deleted: Vec<String>,
        put: Vec<String>,
        fail_put: bool,
        fail_delete: bool,
    }

    impl FakeRemote {
        fn with(entries: &[(&str, &[u8])]) -> Self {
            let mut me = Self::default();
            for (path, bytes) in entries {
                let hash = crate::notebook::state::hash64(bytes).to_string();
                me.files.insert(
                    path.to_string(),
                    (bytes.to_vec(), hash, "dev-b".to_string(), 5),
                );
            }
            me
        }
    }

    impl RemoteFs for FakeRemote {
        fn list(&self) -> Result<Vec<RemoteEntry>, String> {
            Ok(self
                .files
                .iter()
                .map(|(path, (_, hash, device, seq))| RemoteEntry {
                    path: path.clone(),
                    hash: hash.clone(),
                    device: device.clone(),
                    seq: *seq,
                })
                .collect())
        }
        fn get(&self, path: &str) -> Result<Vec<u8>, String> {
            self.files
                .get(path)
                .map(|(bytes, _, _, _)| bytes.clone())
                .ok_or_else(|| format!("remote 404: {path}"))
        }
        fn put(&mut self, path: &str, bytes: &[u8], hash: &str) -> Result<(), String> {
            if self.fail_put {
                return Err("put boom".to_string());
            }
            self.put.push(path.to_string());
            self.files.insert(
                path.to_string(),
                (bytes.to_vec(), hash.to_string(), "dev-a".to_string(), 1),
            );
            Ok(())
        }
        fn delete(&mut self, path: &str) -> Result<(), String> {
            if self.fail_delete {
                return Err("delete boom".to_string());
            }
            self.deleted.push(path.to_string());
            self.files.remove(path);
            Ok(())
        }
    }

    /// 真实文件系统的本地侧,但「软删」记到回收站目录 —— 断言「没有真的 unlink」。
    #[derive(Default)]
    struct FakeLocal {
        soft_deleted: Vec<String>,
    }

    impl LocalFs for FakeLocal {
        fn soft_delete(&mut self, vault: &Path, rel_path: &str) -> Result<(), String> {
            self.soft_deleted.push(rel_path.to_string());
            let path = scan::resolve_rel(vault, rel_path)?;
            let trash = vault.join(".notebook").join("trash");
            std::fs::create_dir_all(&trash).map_err(|e| e.to_string())?;
            std::fs::rename(&path, trash.join(rel_path.replace('/', "_")))
                .map_err(|e| e.to_string())?;
            Ok(())
        }
        fn write(&mut self, vault: &Path, rel_path: &str, bytes: &[u8]) -> Result<(), String> {
            let path = scan::resolve_rel(vault, rel_path)?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(&path, bytes).map_err(|e| e.to_string())
        }
        fn read(&self, vault: &Path, rel_path: &str) -> Result<Vec<u8>, String> {
            let path = scan::resolve_rel(vault, rel_path)?;
            std::fs::read(&path).map_err(|e| e.to_string())
        }
    }

    fn temp_vault(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aeroric-sync-engine-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp vault");
        dir
    }

    fn write_note(vault: &Path, rel: &str, body: &[u8]) {
        let path = vault.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, body).expect("write");
    }

    fn bound(tag: &str) -> PathBuf {
        let vault = temp_vault(tag);
        let conn = store::open(&vault).expect("open");
        store::upsert_remote(&conn, "r1", "cloud", "notes/", "c1").expect("bind");
        vault
    }

    fn outcome<'a>(report: &'a SyncReport, path: &str) -> &'a ActionOutcome {
        report
            .outcomes
            .iter()
            .find(|o| o.path == path)
            .unwrap_or_else(|| panic!("没有 {path} 的结果: {:?}", report.outcomes))
    }

    #[test]
    fn a_new_local_note_is_uploaded_and_baselined() {
        let vault = bound("upload");
        write_note(&vault, "a.md", b"hello");
        let mut remote = FakeRemote::default();
        let mut local = FakeLocal::default();
        let report = run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW,
            &mut remote,
            &mut local,
        )
        .expect("run");
        assert_eq!(outcome(&report, "a.md").status, OutcomeStatus::Done);
        assert_eq!(remote.put, vec!["a.md"]);
        assert_eq!(report.seq, Some(1));

        let conn = store::open(&vault).expect("reopen");
        let bases = store::baselines(&conn, "r1").expect("bases");
        assert_eq!(bases.len(), 1);
        // 基线记的 hash 必须是真正上传的那份内容的 hash。
        assert_eq!(
            bases[0].local_hash,
            crate::notebook::state::hash64(b"hello").to_string()
        );
        assert_eq!(bases[0].remote_hash, bases[0].local_hash);
    }

    #[test]
    fn a_new_remote_note_is_downloaded() {
        let vault = bound("download");
        let mut remote = FakeRemote::with(&[("b.md", b"remote body")]);
        let mut local = FakeLocal::default();
        let report = run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW,
            &mut remote,
            &mut local,
        )
        .expect("run");
        assert_eq!(outcome(&report, "b.md").status, OutcomeStatus::Done);
        assert_eq!(
            std::fs::read(vault.join("b.md")).expect("read"),
            b"remote body"
        );
    }

    #[test]
    fn editing_during_a_sync_does_not_lose_the_edit() {
        // **缺口二的回归测试。** 计划是「下载覆盖本地」(远端变了、本地没变),但执行到它
        // 之前用户改了本地。不复验就会把刚敲进去的内容覆盖掉。
        //
        // 扫描和执行之间那段窗口在 `run` 内部无从插手 —— 它扫完就往下走。所以这里手工
        // 摆出那个中间状态:拿扫描时的快照,改盘上的文件,再调 `execute`。这正是
        // `run` 跑到长队列后半段时的真实处境。
        let vault = bound("toctou-download");
        write_note(&vault, "a.md", b"v1");
        let snapshot = scan::by_path(scan::scan_vault(&vault).expect("scan"));
        std::fs::write(vault.join("a.md"), b"user-typed-this").expect("meddle");

        let mut remote = FakeRemote::with(&[("a.md", b"v2-remote")]);
        let mut local = FakeLocal::default();
        let conn = store::open(&vault).expect("conn");
        let planned = PlannedAction {
            path: "a.md".to_string(),
            action: Action::Download,
            reason: "remote_modified",
        };
        let got = execute(
            &vault,
            "r1",
            &planned,
            &snapshot,
            &[],
            &remote.list().expect("list"),
            NOW,
            &conn,
            &mut remote,
            &mut local,
        );
        assert_eq!(
            got.status,
            OutcomeStatus::Pending {
                detail: "local_changed_during_sync"
            }
        );
        assert_eq!(
            std::fs::read(vault.join("a.md")).expect("read"),
            b"user-typed-this",
            "用户在同步期间写的内容必须还在"
        );
    }

    #[test]
    fn an_upload_records_the_hash_of_what_was_actually_sent() {
        // 复验放过「本地没变」之后,上传记的 hash 必须来自真正读到的字节。Markio 那边
        // 记的是快照里的旧 hash,基线从此说谎 —— 表现是同一个文件每轮都重传。
        let vault = bound("upload-hash");
        write_note(&vault, "a.md", b"the bytes");
        let mut remote = FakeRemote::default();
        let mut local = FakeLocal::default();
        run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW,
            &mut remote,
            &mut local,
        )
        .expect("run");

        let conn = store::open(&vault).expect("conn");
        let bases = store::baselines(&conn, "r1").expect("bases");
        let sent = &remote.files["a.md"];
        assert_eq!(
            bases[0].remote_hash, sent.1,
            "基线记的 hash 要等于远端那份的 hash"
        );
        assert_eq!(
            bases[0].local_hash,
            crate::notebook::state::hash64(&sent.0).to_string()
        );
    }

    #[test]
    fn a_local_delete_during_sync_does_not_get_overwritten_by_a_download() {
        let vault = bound("toctou-gone");
        write_note(&vault, "a.md", b"v1");
        let snapshot = scan::by_path(scan::scan_vault(&vault).expect("scan"));
        std::fs::remove_file(vault.join("a.md")).expect("rm");
        assert_eq!(
            verify(&vault, "a.md", &snapshot).expect("verify"),
            Drift::Gone
        );

        let mut remote = FakeRemote::with(&[("a.md", b"v2")]);
        let mut local = FakeLocal::default();
        let conn = store::open(&vault).expect("conn");
        let planned = PlannedAction {
            path: "a.md".to_string(),
            action: Action::Download,
            reason: "remote_modified",
        };
        let got = execute(
            &vault,
            "r1",
            &planned,
            &snapshot,
            &[],
            &remote.list().expect("list"),
            NOW,
            &conn,
            &mut remote,
            &mut local,
        );
        assert_eq!(
            got.status,
            OutcomeStatus::Pending {
                detail: "local_gone_during_sync"
            }
        );
        assert!(!vault.join("a.md").exists(), "不该把它悄悄写回来");
    }

    #[test]
    fn recreating_a_file_during_sync_stops_the_remote_delete() {
        // 计划是「本地已删 → 删远端」,但执行前用户又把它建回来了。删掉远端那份就是把
        // 用户刚恢复的东西的另一半抹掉。
        let vault = bound("toctou-appeared");
        let snapshot: BTreeMap<String, FileSig> = BTreeMap::new();
        write_note(&vault, "a.md", b"back again");
        assert_eq!(
            verify(&vault, "a.md", &snapshot).expect("verify"),
            Drift::Appeared
        );

        let mut remote = FakeRemote::with(&[("a.md", b"old")]);
        let mut local = FakeLocal::default();
        let conn = store::open(&vault).expect("conn");
        let planned = PlannedAction {
            path: "a.md".to_string(),
            action: Action::DeleteRemote,
            reason: "local_tombstone_remote_unchanged",
        };
        let got = execute(
            &vault,
            "r1",
            &planned,
            &snapshot,
            &[],
            &remote.list().expect("list"),
            NOW,
            &conn,
            &mut remote,
            &mut local,
        );
        assert_eq!(
            got.status,
            OutcomeStatus::Pending {
                detail: "local_appeared_during_sync"
            }
        );
        assert!(remote.deleted.is_empty(), "远端不该被删");
    }

    #[test]
    fn deleting_a_note_locally_writes_a_tombstone_before_touching_the_remote() {
        // **缺口三的回归测试(端到端)。** 第一轮建立基线,删掉本地文件,第二轮必须
        // 把删除传播到远端 —— 而不是把远端那份拉回来。
        let vault = bound("tombstone-e2e");
        write_note(&vault, "gone.md", b"body");
        let mut remote = FakeRemote::default();
        let mut local = FakeLocal::default();
        run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW,
            &mut remote,
            &mut local,
        )
        .expect("seed");
        assert!(remote.files.contains_key("gone.md"));

        std::fs::remove_file(vault.join("gone.md")).expect("rm");
        let report = run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW + 1000,
            &mut remote,
            &mut local,
        )
        .expect("second round");

        assert_eq!(
            report.tombstones_written, 1,
            "推断出本地删除时就要写 tombstone"
        );
        assert_eq!(
            outcome(&report, "gone.md").reason,
            "local_tombstone_remote_unchanged"
        );
        assert_eq!(remote.deleted, vec!["gone.md"]);
        assert!(!vault.join("gone.md").exists(), "删除不该被复活");
    }

    #[test]
    fn a_deletion_survives_a_failed_remote_delete_and_is_retried() {
        // **缺口三的核心。** tombstone 在 `plan` 之前落盘,所以远端删除失败(等价于崩在
        // 中间)之后,下一轮仍然知道「这个路径被删过」,继续去删远端 —— 而不是把远端还
        // 在的那份当成新文件拉回来。Markio 在删除成功之后才记,这一格是空白,于是复活。
        let vault = bound("tombstone-survives");
        write_note(&vault, "gone.md", b"body");
        let mut remote = FakeRemote::default();
        let mut local = FakeLocal::default();
        run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW,
            &mut remote,
            &mut local,
        )
        .expect("seed");
        std::fs::remove_file(vault.join("gone.md")).expect("rm");

        // 第二轮:tombstone 写进去了,但删远端炸了。
        remote.fail_delete = true;
        let failed_round = run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW + 1000,
            &mut remote,
            &mut local,
        )
        .expect("run");
        assert_eq!(failed_round.tombstones_written, 1);
        assert!(matches!(
            outcome(&failed_round, "gone.md").status,
            OutcomeStatus::Failed { .. }
        ));
        assert!(failed_round.seq.is_none());
        assert!(remote.files.contains_key("gone.md"), "远端那份还在");
        {
            let conn = store::open(&vault).expect("conn");
            let tombs = store::live_tombstones(&conn, "r1", NOW + 1000).expect("tombs");
            assert_eq!(tombs.len(), 1, "删除意图必须已经落盘");
        }

        // 第三轮:远端恢复正常。删除继续传播,不是被当成新文件下载回来。
        remote.fail_delete = false;
        let retry = run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW + 2000,
            &mut remote,
            &mut local,
        )
        .expect("retry");
        assert_eq!(
            outcome(&retry, "gone.md").reason,
            "local_tombstone_remote_unchanged"
        );
        assert_eq!(remote.deleted, vec!["gone.md"]);
        assert!(!vault.join("gone.md").exists(), "删除不该被复活成一次下载");
    }

    #[test]
    fn remote_deletion_of_an_untouched_note_soft_deletes_locally() {
        // 软删而不是 unlink:远端的一次误删不该让本地内容无法找回。
        let vault = bound("remote-del");
        write_note(&vault, "a.md", b"body");
        let mut remote = FakeRemote::default();
        let mut local = FakeLocal::default();
        run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW,
            &mut remote,
            &mut local,
        )
        .expect("seed");

        remote.files.clear();
        let report = run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW + 1000,
            &mut remote,
            &mut local,
        )
        .expect("run");
        assert_eq!(
            outcome(&report, "a.md").reason,
            "remote_deleted_local_unchanged"
        );
        assert_eq!(local.soft_deleted, vec!["a.md"]);
        assert!(!vault.join("a.md").exists());
        assert!(vault.join(".notebook/trash/a.md").exists(), "内容要能找回");
    }

    #[test]
    fn an_unresolved_conflict_stays_pending_and_touches_nothing() {
        let vault = bound("pending");
        write_note(&vault, "a.md", b"mine");
        let mut remote = FakeRemote::with(&[("a.md", b"theirs")]);
        let mut local = FakeLocal::default();
        let report = run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW,
            &mut remote,
            &mut local,
        )
        .expect("run");
        assert_eq!(
            outcome(&report, "a.md").status,
            OutcomeStatus::Pending {
                detail: "awaiting_user"
            }
        );
        assert!(remote.put.is_empty(), "挂起的冲突不该动远端");
        assert_eq!(std::fs::read(vault.join("a.md")).expect("read"), b"mine");
    }

    #[test]
    fn the_seq_only_advances_when_everything_settled() {
        // 有挂起还推进 seq 的话,对端会以为我们这边已经是完整的一轮。
        let vault = bound("seq");
        write_note(&vault, "a.md", b"mine");
        let mut remote = FakeRemote::with(&[("a.md", b"theirs")]);
        let mut local = FakeLocal::default();
        let report = run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW,
            &mut remote,
            &mut local,
        )
        .expect("run");
        assert!(report.seq.is_none(), "有挂起时不该推进 seq");
        let conn = store::open(&vault).expect("conn");
        assert_eq!(
            store::get_remote(&conn, "r1")
                .expect("get")
                .expect("some")
                .seq,
            0
        );
    }

    #[test]
    fn keep_remote_on_a_deleted_remote_deletes_locally_instead_of_404ing() {
        // 「远端删了 / 本地改了」+ 采用远端。去 download 一个不存在的文件会 404 每轮
        // 重试,永不收敛。
        let vault = bound("keep-remote-404");
        write_note(&vault, "a.md", b"v1");
        let mut remote = FakeRemote::default();
        let mut local = FakeLocal::default();
        run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW,
            &mut remote,
            &mut local,
        )
        .expect("seed");

        remote.files.clear();
        write_note(&vault, "a.md", b"v2-local-edit");
        let report = run(
            &vault,
            "r1",
            ConflictStrategy::Remote,
            NOW + 1000,
            &mut remote,
            &mut local,
        )
        .expect("run");
        let got = outcome(&report, "a.md");
        assert_eq!(got.reason, "remote_deleted_local_modified");
        assert_eq!(got.status, OutcomeStatus::Done, "不该失败在 404 上");
        assert_eq!(local.soft_deleted, vec!["a.md"]);
    }

    #[test]
    fn keep_local_on_a_conflict_uploads_the_current_bytes() {
        let vault = bound("keep-local");
        write_note(&vault, "a.md", b"mine");
        let mut remote = FakeRemote::with(&[("a.md", b"theirs")]);
        let mut local = FakeLocal::default();
        let report = run(
            &vault,
            "r1",
            ConflictStrategy::Local,
            NOW,
            &mut remote,
            &mut local,
        )
        .expect("run");
        assert_eq!(outcome(&report, "a.md").status, OutcomeStatus::Done);
        assert_eq!(remote.files["a.md"].0, b"mine".to_vec());
    }

    #[test]
    fn a_failing_remote_put_is_reported_not_swallowed() {
        let vault = bound("put-fail");
        write_note(&vault, "a.md", b"body");
        let mut remote = FakeRemote {
            fail_put: true,
            ..Default::default()
        };
        let mut local = FakeLocal::default();
        let report = run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW,
            &mut remote,
            &mut local,
        )
        .expect("run");
        assert!(matches!(
            outcome(&report, "a.md").status,
            OutcomeStatus::Failed { .. }
        ));
        assert!(report.seq.is_none());
        // 失败的那条不该留下基线 —— 留了的话下一轮以为已经传上去了。
        let conn = store::open(&vault).expect("conn");
        assert!(store::baselines(&conn, "r1").expect("bases").is_empty());
    }

    #[test]
    fn a_failed_manifest_commit_keeps_the_round_incomplete() {
        // 清单没写上不是数据问题(它可重建),但 seq 不能说这一轮圆满了 —— 对端会据此
        // 以为我们这边已经是完整的一份。
        struct NoCommit(FakeRemote);
        impl RemoteFs for NoCommit {
            fn list(&self) -> Result<Vec<RemoteEntry>, String> {
                self.0.list()
            }
            fn get(&self, path: &str) -> Result<Vec<u8>, String> {
                self.0.get(path)
            }
            fn put(&mut self, path: &str, bytes: &[u8], hash: &str) -> Result<(), String> {
                self.0.put(path, bytes, hash)
            }
            fn delete(&mut self, path: &str) -> Result<(), String> {
                self.0.delete(path)
            }
            fn commit(&mut self) -> Result<(), String> {
                Err("manifest write failed".to_string())
            }
        }

        let vault = bound("commit-fail");
        write_note(&vault, "a.md", b"body");
        let mut remote = NoCommit(FakeRemote::default());
        let mut local = FakeLocal::default();
        let report = run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW,
            &mut remote,
            &mut local,
        )
        .expect("run");
        // 文件本身传上去了,基线也记了 —— 那些都成功了。
        assert_eq!(outcome(&report, "a.md").status, OutcomeStatus::Done);
        assert!(remote.0.put.contains(&"a.md".to_string()));
        // 但这一轮不算完整。
        assert!(report.seq.is_none(), "commit 失败时 seq 不该推进");
        assert!(matches!(
            outcome(&report, "").status,
            OutcomeStatus::Failed { .. }
        ));
        assert_eq!(outcome(&report, "").reason, "manifest_commit");
    }

    #[test]
    fn an_unknown_remote_is_refused_before_anything_is_scanned() {
        let vault = temp_vault("unbound");
        let mut remote = FakeRemote::default();
        let mut local = FakeLocal::default();
        assert!(run(
            &vault,
            "nope",
            ConflictStrategy::Ask,
            NOW,
            &mut remote,
            &mut local
        )
        .is_err());
    }

    #[test]
    fn a_second_run_with_no_changes_does_nothing() {
        // 幂等。第二轮冒出动作说明基线没记对 —— 那是最容易出的一类 bug,表现是每轮
        // 都把整库重传一遍。
        let vault = bound("idempotent");
        write_note(&vault, "a.md", b"body");
        write_note(&vault, "attachments/i.png", &[1, 2, 3]);
        let mut remote = FakeRemote::default();
        let mut local = FakeLocal::default();
        let first = run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW,
            &mut remote,
            &mut local,
        )
        .expect("first");
        assert_eq!(first.plan.actions.len(), 2);
        assert_eq!(first.seq, Some(1));

        let second = run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW + 1000,
            &mut remote,
            &mut local,
        )
        .expect("second");
        assert!(
            second.plan.actions.is_empty(),
            "第二轮不该有动作: {:?}",
            second.plan.actions
        );
        assert_eq!(second.seq, Some(2));
    }

    #[test]
    fn a_double_deletion_clears_the_baseline() {
        // 两边都没了还留着基线的话,将来重建同名文件会被判成本地删除再删一次远端。
        let vault = bound("double-del");
        write_note(&vault, "a.md", b"body");
        let mut remote = FakeRemote::default();
        let mut local = FakeLocal::default();
        run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW,
            &mut remote,
            &mut local,
        )
        .expect("seed");

        std::fs::remove_file(vault.join("a.md")).expect("rm");
        remote.files.clear();
        run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW + 1000,
            &mut remote,
            &mut local,
        )
        .expect("run");

        let conn = store::open(&vault).expect("conn");
        assert!(
            store::baselines(&conn, "r1").expect("bases").is_empty(),
            "双删之后基线要清掉"
        );
    }

    #[test]
    fn a_round_prunes_expired_tombstones() {
        // 没有这一步的话 tombstone 表只增不减:`live_tombstones` 只在读时按 TTL 过滤,
        // 从不删行。一个用了几年的 vault 会攒下一张全是死记录的表。
        let vault = bound("prune");
        let conn = store::open(&vault).expect("conn");
        store::add_tombstone(
            &conn,
            "r1",
            &store::Tombstone {
                path: "old.md".to_string(),
                deleted_at: NOW - store::TOMBSTONE_TTL_MS - 1,
                remote_hash: "h".to_string(),
            },
        )
        .expect("old tombstone");
        store::add_tombstone(
            &conn,
            "r1",
            &store::Tombstone {
                path: "fresh.md".to_string(),
                deleted_at: NOW - 1000,
                remote_hash: "h".to_string(),
            },
        )
        .expect("fresh tombstone");
        drop(conn);

        let mut remote = FakeRemote::default();
        let mut local = FakeLocal::default();
        run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW,
            &mut remote,
            &mut local,
        )
        .expect("run");

        let conn = store::open(&vault).expect("conn");
        let rows: i64 = conn
            .query_row("SELECT count(*) FROM tombstone", [], |row| row.get(0))
            .expect("count");
        // 数真实行数,不是数 `live_tombstones` 的返回值 —— 后者对过期记录本来就返回空,
        // 拿它断言的话这个测试在「压根没删」时也会过。
        assert_eq!(rows, 1, "过期那条要真的从表里消失");
        let live = store::live_tombstones(&conn, "r1", NOW).expect("live");
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].path, "fresh.md", "没过期的不能被一起清掉");
    }

    #[test]
    fn an_oversize_file_is_neither_uploaded_nor_deleted() {
        // 超大文件算不出 hash。既不能传(manifest 里会是个假 hash,之后永远重传),也
        // 不能因为「基线里没有它」就当成本地删除。稀疏文件,不真占盘。
        let vault = bound("oversize");
        let path = vault.join("big.bin");
        let file = std::fs::File::create(&path).expect("create");
        file.set_len(scan::MAX_HASH_BYTES + 1).expect("set_len");
        drop(file);
        write_note(&vault, "a.md", b"normal");

        let mut remote = FakeRemote::default();
        let mut local = FakeLocal::default();
        let report = run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW,
            &mut remote,
            &mut local,
        )
        .expect("run");
        assert_eq!(remote.put, vec!["a.md"], "只有正常文件被传上去");
        assert!(
            !report.outcomes.iter().any(|o| o.path == "big.bin"),
            "超大文件不该产生任何动作: {:?}",
            report.outcomes
        );
        assert!(path.exists(), "它还在本地");
    }

    #[test]
    fn the_engine_refuses_to_upload_an_oversize_file_even_if_asked_directly() {
        // 上面那条测的是 `diff` 不生成动作。这条测的是 engine 自己也挡 —— 那两处是不同
        // 模块的约定,别让这一层的正确性依赖另一层不出错。
        let vault = bound("oversize-direct");
        let path = vault.join("big.bin");
        let file = std::fs::File::create(&path).expect("create");
        file.set_len(scan::MAX_HASH_BYTES + 1).expect("set_len");
        drop(file);
        let snapshot = scan::by_path(scan::scan_vault(&vault).expect("scan"));
        assert!(snapshot["big.bin"].is_oversize());

        let mut remote = FakeRemote::default();
        let mut local = FakeLocal::default();
        let conn = store::open(&vault).expect("conn");
        let planned = PlannedAction {
            path: "big.bin".to_string(),
            action: Action::Upload,
            reason: "new_local",
        };
        let got = execute(
            &vault,
            "r1",
            &planned,
            &snapshot,
            &[],
            &[],
            NOW,
            &conn,
            &mut remote,
            &mut local,
        );
        assert_eq!(
            got.status,
            OutcomeStatus::Pending {
                detail: "oversize_not_hashable"
            }
        );
        assert!(remote.put.is_empty());
    }

    #[test]
    fn a_propagated_deletion_keeps_its_tombstone() {
        // 删除传播成功之后 tombstone 要留着,直到 TTL 过期。清掉的话,还没看到这次删除
        // 的第三台设备把文件传回来时,这边会当成新文件下载 —— 复活。
        let vault = bound("tombstone-kept");
        write_note(&vault, "gone.md", b"body");
        let mut remote = FakeRemote::default();
        let mut local = FakeLocal::default();
        run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW,
            &mut remote,
            &mut local,
        )
        .expect("seed");
        std::fs::remove_file(vault.join("gone.md")).expect("rm");
        run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW + 1000,
            &mut remote,
            &mut local,
        )
        .expect("propagate");
        assert_eq!(remote.deleted, vec!["gone.md"]);

        let conn = store::open(&vault).expect("conn");
        assert_eq!(
            store::live_tombstones(&conn, "r1", NOW + 2000)
                .expect("tombs")
                .len(),
            1,
            "tombstone 要活到 TTL 过期"
        );

        // 第三台设备把它传回来 —— 应该再删一次,不是下载回来。
        remote.files.insert(
            "gone.md".to_string(),
            (
                b"body".to_vec(),
                crate::notebook::state::hash64(b"body").to_string(),
                "dev-c".to_string(),
                9,
            ),
        );
        let third = run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW + 3000,
            &mut remote,
            &mut local,
        )
        .expect("third");
        assert_eq!(
            outcome(&third, "gone.md").reason,
            "local_tombstone_remote_unchanged"
        );
        assert!(!vault.join("gone.md").exists(), "不该被复活");
    }

    #[test]
    fn attachments_ride_along_with_the_notes() {
        // 附件是同步的一等公民。只传 .md 的表现是「笔记过去了,图片全裂」。
        let vault = bound("attachments");
        write_note(&vault, "note.md", b"![](attachments/i.png)");
        write_note(&vault, "attachments/i.png", &[0x89, 0x50, 0x4e, 0x47]);
        let mut remote = FakeRemote::default();
        let mut local = FakeLocal::default();
        run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW,
            &mut remote,
            &mut local,
        )
        .expect("run");
        assert!(remote.files.contains_key("attachments/i.png"));
        assert_eq!(
            remote.files["attachments/i.png"].0,
            vec![0x89, 0x50, 0x4e, 0x47]
        );
    }

    #[test]
    fn the_private_dir_never_leaves_the_machine() {
        // `.notebook/` 里是索引库、历史快照、回收站。同步过去会让两台机器互相覆盖彼此
        // 的索引,而那个库还带着 -wal。
        let vault = bound("private");
        write_note(&vault, "a.md", b"body");
        write_note(&vault, ".notebook/history/a.md", b"old version");
        let mut remote = FakeRemote::default();
        let mut local = FakeLocal::default();
        run(
            &vault,
            "r1",
            ConflictStrategy::Ask,
            NOW,
            &mut remote,
            &mut local,
        )
        .expect("run");
        assert!(
            !remote.files.keys().any(|k| k.starts_with(".notebook")),
            "远端出现了私有目录的内容: {:?}",
            remote.files.keys().collect::<Vec<_>>()
        );
    }
}
