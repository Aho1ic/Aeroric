//! 三方 diff:本地扫描 × 远端清单 × 基线 → 动作清单。
//!
//! 不读盘、不发网络请求,纯算法。上层负责把三路输入喂进来,以及执行算出来的动作。
//! 每条动作带 `reason`,既给单测断言用,也给 UI 解释「为什么要删这个」。
//!
//! ## 与 Markio 的两处实质分歧
//!
//! **一、远端身份不是 etag,是当前内容 hash。** Markio 判「远端变了没」靠
//! `remote.hash != base.remoteEtag`,而那个 `hash` 是各家云盘的 etag。Aeroric 的
//! `StorageEntry` 没有 etag(只有 size 与 mtime),拿 mtime 顶替既会漏检(改了但
//! mtime 没动)又会误报(touch 一下没改内容)。所以这一层每轮读取可 hash 的远端内容
//! 并计算 hash；sidecar manifest 只在 hash 验证一致时提供 device/seq 逻辑戳。
//!
//! **二、没有 `newest` 策略。**
//!
//! 这是本模块最重要的一条,写在这里免得后来有人「补上」它。Markio 的 `newest` 比的是
//! `local.mtime >= remote.mtime` —— 两台机器的两个时钟。差几秒就静默挑错边,而用户
//! 看到的只是「我明明后改的,怎么被覆盖了」。
//!
//! 换成逻辑戳也救不了它。给每个远端版本记 `(device, seq)` 之后,四种情形是:
//!
//! ```text
//! 远端变了、本地没变   → 下载。与顺序无关。
//! 本地变了、远端没变   → 上传。与顺序无关。
//! 都没变              → 无动作。
//! 都变了              → 两边都从共同祖先各自走了一步,谁也没见过谁。
//! ```
//!
//! 第四种就是**并发**的定义。没有向量钟或真实因果历史的话,它不可排序 —— 逻辑戳能
//! 告出「不可排序」,但告不出「谁更新」。所以诚实的做法是把它当一档独立结果交给用户
//! ([`Action::Conflict`]),而不是退化成「随便挑一个较新的」。策略只留
//! `ask` / `local` / `remote`。
//!
//! 这与 P8 的准入条件是一回事:提供一个只在时钟同步时才正确的 `newest`,就是把
//! 「跨机器时钟下的 newest 判定」这个已知缺口原样搬进来。

use std::collections::BTreeMap;

use super::scan::FileSig;
use super::store::{Baseline, StoredResolution, Tombstone};

/// 远端清单里的一条。内容 hash 来自本轮远端读取；device/seq 可来自已验证的 sidecar
/// manifest，而不是 provider 的 list 元数据。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteEntry {
    pub path: String,
    /// 内容 hash,口径与 [`FileSig::hash`] 一致。
    pub hash: String,
    /// 写下这个版本的设备,以及它当时的逻辑序号。用来判「不可排序」。
    pub device: String,
    pub seq: i64,
}

/// 冲突策略。**刻意没有 `newest`** —— 理由见模块文档。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictStrategy {
    /// 交给用户。默认。
    Ask,
    /// 一律保留本地。
    Local,
    /// 一律采用远端。
    Remote,
}

// 这里曾经有个 `Order { LocalLater, RemoteLater, Concurrent }`,是 `newest` 设计的残骸。
// `newest` 去掉之后它只剩一个可达取值:能构造出冲突的那唯一一种情形(两边相对基线都变了)
// 按定义就是并发。一个只能取一个值的字段不是信息,而且它摆在结构体里会让前端以为「有时候
// 能知道谁更新」—— 那正是我们刚论证过做不到的事。冲突就是冲突,不带顺序。

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Resolution {
    KeepLocal,
    KeepRemote,
    /// 远端版本另存一份,两边都留。
    Fork {
        fork_path: String,
    },
}

impl Resolution {
    /// 拆成两个可以直接进库的列。
    ///
    /// kind 用和 serde tag 一样的 camelCase 串:库里那一列和前端线上那个字段从此是同一
    /// 套取值,排查时不用在两种拼法之间换算。
    pub fn parts(&self) -> (&'static str, &str) {
        match self {
            Resolution::KeepLocal => ("keepLocal", ""),
            Resolution::KeepRemote => ("keepRemote", ""),
            Resolution::Fork { fork_path } => ("fork", fork_path.as_str()),
        }
    }

    /// 从库里那两列拼回来。
    ///
    /// 认不出的 kind 落回 `KeepLocal` —— 那是唯一不丢内容的一侧。库里出现意料外的值只
    /// 可能来自降级运行或手改,那时候「什么都不删」比「猜一个」安全。`fork` 但 `fork_path`
    /// 是空的同样落回去:拿空路径去写文件会写到 vault 根上。
    pub fn from_parts(kind: &str, fork_path: String) -> Self {
        match kind {
            "keepRemote" => Resolution::KeepRemote,
            "fork" if !fork_path.is_empty() => Resolution::Fork { fork_path },
            _ => Resolution::KeepLocal,
        }
    }
}

impl RemoteEntry {
    /// Whether the entry is present but deliberately not content-hashed.
    pub fn is_oversize(&self) -> bool {
        self.hash.starts_with(super::scan::OVERSIZE_PREFIX)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Action {
    Upload,
    Download,
    DeleteRemote,
    /// 本地删除走回收站,不是 unlink —— 远端的一次误删不该让本地内容无法找回。
    DeleteLocal,
    /// 两边相对基线都变了 —— 并发编辑,不可排序。哪一份「更新」在这里是问不出来的。
    Conflict {
        /// `None` 表示等用户选(`ask` 策略下)。
        resolution: Option<Resolution>,
        /// 本轮看到的本地 hash。空串 = 本地没有这个文件。
        ///
        /// 平铺在这里而不是让前端另外查一次:算 diff 的时候它本来就在手上,而前端提交
        /// 决定时必须带上**它看到的是哪两份** —— 否则决定和内容对不上号,那道防覆盖的
        /// 闸门就没有输入。
        local_hash: String,
        /// 本轮看到的远端 hash。空串 = 远端没有这个文件。
        remote_hash: String,
    },
}

/// 一条动作。
///
/// 只 `Serialize`:这是算出来给前端看的结果,不接受从前端反序列化回来。`reason` 是
/// `&'static str` 也就成立了 —— 反过来的话它得是 `String`,而那意味着每条动作多一次
/// 堆分配,还让「reason 只能是代码里写死的那几个」这个约束从类型上消失。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedAction {
    pub path: String,
    pub action: Action,
    /// 决策依据。单测断言的就是这个,UI 也拿它解释动作。
    pub reason: &'static str,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanSummary {
    pub upload: usize,
    pub download: usize,
    pub delete_remote: usize,
    pub delete_local: usize,
    pub conflict: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPlan {
    pub actions: Vec<PlannedAction>,
    pub summary: PlanSummary,
}

/// diff 的三路输入之外的参数。
#[derive(Debug, Clone, Copy)]
pub struct DiffOpts<'a> {
    pub strategy: ConflictStrategy,
    /// 用户对具体路径做过的决定。空切片 = 一条都没有(纯按策略走)。
    ///
    /// 放在 opts 里而不是给 `plan` 加第五个参数:它和 `strategy` 是同一件事的两档粒度,
    /// 摆在一起才看得出「逐路径的优先」这个关系。
    pub decided: &'a [StoredResolution],
}

/// 算出这一轮该做什么。
///
/// 三路输入:`local`(本轮扫描)、`remote`(本轮远端内容快照)、`baseline` + `tombstones`
/// (上次同步成功时的样子)。`tombstones` 只该传**还在窗口期内**的那些,过期判定在
/// [`super::store::live_tombstones`] 一处做。
pub fn plan(
    local: &[FileSig],
    remote: &[RemoteEntry],
    baseline: &[Baseline],
    tombstones: &[Tombstone],
    opts: DiffOpts<'_>,
) -> SyncPlan {
    let local_map: BTreeMap<&str, &FileSig> = local.iter().map(|f| (f.path.as_str(), f)).collect();
    let remote_map: BTreeMap<&str, &RemoteEntry> =
        remote.iter().map(|e| (e.path.as_str(), e)).collect();
    let base_map: BTreeMap<&str, &Baseline> =
        baseline.iter().map(|b| (b.path.as_str(), b)).collect();
    let tomb_map: BTreeMap<&str, &Tombstone> =
        tombstones.iter().map(|t| (t.path.as_str(), t)).collect();

    let mut actions: Vec<PlannedAction> = Vec::new();

    // 第一遍:本地有的。
    for (path, local_file) in &local_map {
        // 超大文件:算不出内容 hash,任何基于 hash 的判断都不成立。不产生动作 ——
        // 但它**出现在扫描结果里**这件事本身是有意义的,那让下面第二遍不会把它当成
        // 「本地已删」去删远端。
        if local_file.is_oversize() {
            continue;
        }
        let remote_entry = remote_map.get(path).copied();
        // Remote oversize entries are presence markers only. Their content is
        // intentionally unavailable for a safe hash comparison, so neither
        // side may be selected as the source of a destructive action.
        if remote_entry.is_some_and(|entry| entry.is_oversize()) {
            continue;
        }
        let base = base_map.get(path).copied();
        match (remote_entry, base) {
            (None, None) => actions.push(one(path, Action::Upload, "new_local")),
            (None, Some(base)) => {
                // 曾经同步过,现在远端没了 → 远端被删。
                if local_file.hash == base.local_hash {
                    // 本地没动 → 跟着删。
                    actions.push(one(
                        path,
                        Action::DeleteLocal,
                        "remote_deleted_local_unchanged",
                    ));
                } else {
                    // 本地改过 → 「删」和「改」撞上了,不可排序。
                    // 远端此刻不存在,所以远端那侧记空串。
                    actions.push(one(
                        path,
                        conflict(opts, path, &local_file.hash, ""),
                        "remote_deleted_local_modified",
                    ));
                }
            }
            (Some(remote_entry), None) => {
                // 双方都有但没基线:第一次同步碰上同名文件。
                if remote_entry.hash == local_file.hash {
                    // 内容相同(用户预先拷过去了)→ 收编,不必传。
                    actions.push(one(path, Action::Upload, "same_content_no_baseline"));
                } else {
                    actions.push(one(
                        path,
                        conflict(opts, path, &local_file.hash, &remote_entry.hash),
                        "both_present_no_baseline",
                    ));
                }
            }
            (Some(remote_entry), Some(base)) => {
                let local_changed = local_file.hash != base.local_hash;
                let remote_changed = remote_changed(remote_entry, base);
                match (local_changed, remote_changed) {
                    (false, false) => {}
                    (true, false) => actions.push(one(path, Action::Upload, "local_modified")),
                    (false, true) => actions.push(one(path, Action::Download, "remote_modified")),
                    (true, true) => actions.push(one(
                        path,
                        conflict(opts, path, &local_file.hash, &remote_entry.hash),
                        "both_modified",
                    )),
                }
            }
        }
    }

    // 第二遍:远端有、本地没有的。
    for (path, remote_entry) in &remote_map {
        if local_map.contains_key(path) {
            continue;
        }
        // Do not turn an unhashable remote presence marker into an unbounded
        // download (or a delete based on an incomplete comparison).
        if remote_entry.is_oversize() {
            continue;
        }
        let base = base_map.get(path).copied();
        let tomb = tomb_map.get(path).copied();
        match (base, tomb) {
            (_, Some(tomb)) => {
                // 本地删过。远端那份自删除之后有没有被改过?
                if remote_entry.hash == tomb.remote_hash {
                    actions.push(one(
                        path,
                        Action::DeleteRemote,
                        "local_tombstone_remote_unchanged",
                    ));
                } else {
                    // 别的设备在我们删掉之后又改了它 —— 那是一次「删 vs 改」的并发,
                    // 不能默默把对方的编辑删掉。
                    // 本地此刻不存在(被删了),所以本地那侧记空串。
                    actions.push(one(
                        path,
                        conflict(opts, path, "", &remote_entry.hash),
                        "local_tombstone_remote_modified",
                    ));
                }
            }
            (Some(base), None) => {
                // 有基线、没 tombstone、本地文件不在了。
                //
                // 这条路是**上一轮崩在中途**才会走到的:正常情况下 diff 一推断出本地
                // 删除就会写 tombstone(见 store 的 `add_tombstone`)。所以这里不能
                // 直接删远端 —— 我们不知道本地是被用户删了还是同步自己没写完。
                if remote_entry.hash == base.remote_hash {
                    actions.push(one(
                        path,
                        Action::DeleteRemote,
                        "local_missing_remote_unchanged",
                    ));
                } else {
                    actions.push(one(path, Action::Download, "local_missing_remote_modified"));
                }
            }
            (None, None) => actions.push(one(path, Action::Download, "new_remote")),
        }
    }

    actions.sort_by(|a, b| a.path.cmp(&b.path));
    let summary = summarize(&actions);
    SyncPlan { actions, summary }
}

/// 远端那份和我们上次见到的是不是同一个版本。
///
/// 比内容 hash,不比时间也不比 etag。`(device, seq)` 不参与这个判断 —— 同一份内容
/// 被另一台设备重传过(hash 相同、stamp 不同)不算变更,否则每台设备的每一轮都会把
/// 别人重传过的文件重新下载一遍。
fn remote_changed(remote: &RemoteEntry, base: &Baseline) -> bool {
    remote.hash != base.remote_hash
}

/// 把「并发」落成具体动作:先看用户有没有对这个路径做过决定,没有才按策略。
///
/// 逐路径的决定优先于 vault 级策略。反过来的话策略一旦不是 `Ask`,用户在面板上一条条
/// 做的选择就全被顶掉了。
///
/// `Ask` 且没有决定时留 `None`:执行层看到 `None` 就把这一条挂起交给用户,不动文件。
fn conflict(opts: DiffOpts<'_>, path: &str, local_hash: &str, remote_hash: &str) -> Action {
    let resolution =
        decided_for(opts.decided, path, local_hash, remote_hash).or(match opts.strategy {
            ConflictStrategy::Ask => None,
            ConflictStrategy::Local => Some(Resolution::KeepLocal),
            ConflictStrategy::Remote => Some(Resolution::KeepRemote),
        });
    Action::Conflict {
        resolution,
        local_hash: local_hash.to_string(),
        remote_hash: remote_hash.to_string(),
    }
}

/// 存着的决定还算不算数。
///
/// **两侧都得对得上。** 决定是针对用户当时看到的那两份内容做的;任何一侧在那之后又变了,
/// 就意味着他没见过现在这一份。此时执行原决定会静默覆盖掉他从没看到的内容 —— 这正是
/// 「交给用户」想避免的那件事。对不上就返回 `None`,让它退回策略(`Ask` 下就是再问一次)。
fn decided_for(
    decided: &[StoredResolution],
    path: &str,
    local_hash: &str,
    remote_hash: &str,
) -> Option<Resolution> {
    decided
        .iter()
        .find(|d| d.path == path)
        .filter(|d| d.local_hash == local_hash && d.remote_hash == remote_hash)
        .map(|d| d.resolution.clone())
}

fn one(path: &str, action: Action, reason: &'static str) -> PlannedAction {
    PlannedAction {
        path: path.to_string(),
        action,
        reason,
    }
}

fn summarize(actions: &[PlannedAction]) -> PlanSummary {
    let mut summary = PlanSummary::default();
    for planned in actions {
        match planned.action {
            Action::Upload => summary.upload += 1,
            Action::Download => summary.download += 1,
            Action::DeleteRemote => summary.delete_remote += 1,
            Action::DeleteLocal => summary.delete_local += 1,
            Action::Conflict { .. } => summary.conflict += 1,
        }
    }
    summary
}

/// 本轮该写哪些 tombstone。
///
/// **这是 Markio 那个缺口的正面修法。** 那边 `addTombstone` 除单测外没有调用点,
/// `tombstones` 恒为空,于是本地删除在下一轮被判成「远端新文件」拉回来 —— 删除复活。
///
/// 判据:基线里有、本地扫描里没有、且还没记过 tombstone。调用方必须在**执行任何动作
/// 之前**把这些写盘 —— 等远端删除成功之后再写的话,崩在中间就什么都没留下,而那正是
/// 复活发生的时机。
pub fn pending_tombstones(
    local: &[FileSig],
    baseline: &[Baseline],
    tombstones: &[Tombstone],
    now_ms: i64,
) -> Vec<Tombstone> {
    let local_paths: std::collections::BTreeSet<&str> =
        local.iter().map(|f| f.path.as_str()).collect();
    let known: std::collections::BTreeSet<&str> =
        tombstones.iter().map(|t| t.path.as_str()).collect();
    baseline
        .iter()
        .filter(|base| !local_paths.contains(base.path.as_str()))
        .filter(|base| !known.contains(base.path.as_str()))
        .map(|base| Tombstone {
            path: base.path.clone(),
            deleted_at: now_ms,
            remote_hash: base.remote_hash.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_760_000_000_000;

    fn local(path: &str, hash: &str) -> FileSig {
        FileSig {
            path: path.to_string(),
            mtime_ms: NOW,
            size: 10,
            hash: hash.to_string(),
        }
    }

    fn oversize(path: &str) -> FileSig {
        FileSig {
            path: path.to_string(),
            mtime_ms: NOW,
            size: 999,
            hash: format!("{}{}", super::super::scan::OVERSIZE_PREFIX, 999),
        }
    }

    fn remote(path: &str, hash: &str) -> RemoteEntry {
        RemoteEntry {
            path: path.to_string(),
            hash: hash.to_string(),
            device: "dev-b".to_string(),
            seq: 5,
        }
    }

    fn baseline(path: &str, local_hash: &str, remote_hash: &str) -> Baseline {
        Baseline {
            path: path.to_string(),
            local_hash: local_hash.to_string(),
            local_mtime_ms: NOW,
            remote_hash: remote_hash.to_string(),
            remote_device: "dev-b".to_string(),
            remote_seq: 5,
            synced_at: NOW,
        }
    }

    fn tomb(path: &str, remote_hash: &str) -> Tombstone {
        Tombstone {
            path: path.to_string(),
            deleted_at: NOW,
            remote_hash: remote_hash.to_string(),
        }
    }

    fn ask() -> DiffOpts<'static> {
        DiffOpts {
            strategy: ConflictStrategy::Ask,
            decided: &[],
        }
    }

    /// 带一批逐路径决定的 opts。策略仍是 `Ask` —— 这样通过的断言只能是决定起了作用。
    fn with_decided(decided: &[StoredResolution]) -> DiffOpts<'_> {
        DiffOpts {
            strategy: ConflictStrategy::Ask,
            decided,
        }
    }

    /// 一条决定。`local` / `remote` 是做决定时看到的两侧 hash。
    fn decision(path: &str, resolution: Resolution, local: &str, remote: &str) -> StoredResolution {
        StoredResolution {
            path: path.to_string(),
            resolution,
            local_hash: local.to_string(),
            remote_hash: remote.to_string(),
            decided_at: 1_760_000_000_000,
        }
    }

    /// 只取一条动作,顺手断言总数 —— 多出来的动作是最容易漏掉的一类 bug。
    fn only(plan: SyncPlan) -> PlannedAction {
        assert_eq!(
            plan.actions.len(),
            1,
            "期望恰好一条动作: {:?}",
            plan.actions
        );
        plan.actions.into_iter().next().expect("one")
    }

    #[test]
    fn a_brand_new_local_file_is_uploaded() {
        let got = only(plan(&[local("a.md", "1")], &[], &[], &[], ask()));
        assert_eq!(got.action, Action::Upload);
        assert_eq!(got.reason, "new_local");
    }

    #[test]
    fn a_brand_new_remote_file_is_downloaded() {
        let got = only(plan(&[], &[remote("a.md", "1")], &[], &[], ask()));
        assert_eq!(got.action, Action::Download);
        assert_eq!(got.reason, "new_remote");
    }

    #[test]
    fn nothing_to_do_when_neither_side_moved() {
        let got = plan(
            &[local("a.md", "1")],
            &[remote("a.md", "2")],
            &[baseline("a.md", "1", "2")],
            &[],
            ask(),
        );
        assert!(got.actions.is_empty(), "{:?}", got.actions);
        assert_eq!(got.summary, PlanSummary::default());
    }

    #[test]
    fn local_only_change_uploads_and_remote_only_change_downloads() {
        let up = only(plan(
            &[local("a.md", "1-new")],
            &[remote("a.md", "2")],
            &[baseline("a.md", "1", "2")],
            &[],
            ask(),
        ));
        assert_eq!(up.action, Action::Upload);
        assert_eq!(up.reason, "local_modified");

        let down = only(plan(
            &[local("a.md", "1")],
            &[remote("a.md", "2-new")],
            &[baseline("a.md", "1", "2")],
            &[],
            ask(),
        ));
        assert_eq!(down.action, Action::Download);
        assert_eq!(down.reason, "remote_modified");
    }

    #[test]
    fn both_sides_changed_is_concurrent_never_auto_ordered() {
        // 这是「没有 newest」那条设计的正面断言:两边都动过就是并发,`ask` 下不产生
        // 任何自动决定。有人「补上」newest 的话这条会红。
        let got = only(plan(
            &[local("a.md", "1-new")],
            &[remote("a.md", "2-new")],
            &[baseline("a.md", "1", "2")],
            &[],
            ask(),
        ));
        assert_eq!(
            got.action,
            Action::Conflict {
                resolution: None,
                local_hash: "1-new".to_string(),
                remote_hash: "2-new".to_string(),
            }
        );
        assert_eq!(got.reason, "both_modified");
    }

    #[test]
    fn a_remote_rewrite_with_identical_content_is_not_a_change() {
        // 另一台设备重传了同一份内容(hash 相同、device/seq 不同)。算变更的话每台
        // 设备每轮都会把别人重传过的文件重新下载一遍。
        let mut entry = remote("a.md", "2");
        entry.device = "dev-z".to_string();
        entry.seq = 99;
        let got = plan(
            &[local("a.md", "1")],
            &[entry],
            &[baseline("a.md", "1", "2")],
            &[],
            ask(),
        );
        assert!(got.actions.is_empty(), "{:?}", got.actions);
    }

    #[test]
    fn remote_deleted_and_local_untouched_deletes_local() {
        let got = only(plan(
            &[local("a.md", "1")],
            &[],
            &[baseline("a.md", "1", "2")],
            &[],
            ask(),
        ));
        assert_eq!(got.action, Action::DeleteLocal);
        assert_eq!(got.reason, "remote_deleted_local_unchanged");
    }

    #[test]
    fn remote_deleted_but_local_edited_is_a_conflict() {
        // 「删」撞上「改」。默默删掉本地就是丢用户刚写的东西。
        let got = only(plan(
            &[local("a.md", "1-new")],
            &[],
            &[baseline("a.md", "1", "2")],
            &[],
            ask(),
        ));
        assert!(matches!(got.action, Action::Conflict { .. }));
        assert_eq!(got.reason, "remote_deleted_local_modified");
    }

    #[test]
    fn same_content_on_both_sides_without_a_baseline_is_adopted_not_conflicted() {
        // 用户先手工拷了一份过去,然后才开同步。判成冲突会让第一次同步弹出一堆假冲突。
        let got = only(plan(
            &[local("a.md", "same")],
            &[remote("a.md", "same")],
            &[],
            &[],
            ask(),
        ));
        assert_eq!(got.action, Action::Upload);
        assert_eq!(got.reason, "same_content_no_baseline");
    }

    #[test]
    fn different_content_on_both_sides_without_a_baseline_is_a_conflict() {
        let got = only(plan(
            &[local("a.md", "mine")],
            &[remote("a.md", "theirs")],
            &[],
            &[],
            ask(),
        ));
        assert!(matches!(got.action, Action::Conflict { .. }));
        assert_eq!(got.reason, "both_present_no_baseline");
    }

    #[test]
    fn a_tombstone_propagates_the_deletion_instead_of_resurrecting_the_file() {
        // **缺口三的回归测试。** tombstone 不写(Markio 的实际状态)时,下面这一局的
        // 远端条目会命中 `new_remote` 被下载回来 —— 删除复活。有 tombstone 才会正确地
        // 把删除传播过去。
        let with_tomb = only(plan(
            &[],
            &[remote("gone.md", "2")],
            &[],
            &[tomb("gone.md", "2")],
            ask(),
        ));
        assert_eq!(with_tomb.action, Action::DeleteRemote);
        assert_eq!(with_tomb.reason, "local_tombstone_remote_unchanged");

        // 同一局,少了 tombstone:就是复活。这一条把「差别到底在哪」钉住。
        let without_tomb = only(plan(&[], &[remote("gone.md", "2")], &[], &[], ask()));
        assert_eq!(
            without_tomb.action,
            Action::Download,
            "没有 tombstone 时就是这样把删除复活的"
        );
    }

    #[test]
    fn a_tombstone_yields_to_a_remote_edit_made_after_the_deletion() {
        // 我们删了,但别的设备在那之后又改了它。默默删掉就是丢对方的编辑。
        let got = only(plan(
            &[],
            &[remote("gone.md", "2-edited")],
            &[],
            &[tomb("gone.md", "2")],
            ask(),
        ));
        assert!(matches!(got.action, Action::Conflict { .. }));
        assert_eq!(got.reason, "local_tombstone_remote_modified");
    }

    #[test]
    fn a_missing_local_file_without_a_tombstone_still_propagates_when_remote_matches() {
        // 上一轮崩在写 tombstone 之前才会走到这里。远端还是我们上次见到的那份 → 传播删除。
        let got = only(plan(
            &[],
            &[remote("a.md", "2")],
            &[baseline("a.md", "1", "2")],
            &[],
            ask(),
        ));
        assert_eq!(got.action, Action::DeleteRemote);
        assert_eq!(got.reason, "local_missing_remote_unchanged");
    }

    #[test]
    fn a_missing_local_file_defers_to_a_changed_remote() {
        // 远端在我们「本地不见了」之后被改过 —— 保守拉回来,不要抹掉别人的新编辑。
        let got = only(plan(
            &[],
            &[remote("a.md", "2-new")],
            &[baseline("a.md", "1", "2")],
            &[],
            ask(),
        ));
        assert_eq!(got.action, Action::Download);
        assert_eq!(got.reason, "local_missing_remote_modified");
    }

    #[test]
    fn an_oversize_local_file_produces_no_action_but_is_not_treated_as_deleted() {
        // 关键在后半句:它在场,所以第二遍不会把它当本地删除去删远端。
        let got = plan(
            &[oversize("big.bin")],
            &[remote("big.bin", "2")],
            &[baseline("big.bin", "1", "2")],
            &[],
            ask(),
        );
        assert!(got.actions.is_empty(), "{:?}", got.actions);
    }

    #[test]
    fn an_oversize_file_that_is_new_on_both_sides_is_still_left_alone() {
        let got = plan(&[oversize("big.bin")], &[], &[], &[], ask());
        assert!(got.actions.is_empty(), "{:?}", got.actions);
    }

    #[test]
    fn an_oversize_remote_file_missing_locally_is_not_downloaded() {
        let got = plan(
            &[],
            &[remote(
                "big.bin",
                &format!("{}{}", super::super::scan::OVERSIZE_PREFIX, 70 * 1024 * 1024),
            )],
            &[],
            &[],
            ask(),
        );
        assert!(
            got.actions.is_empty(),
            "an unhashable remote file must not become an unbounded download: {:?}",
            got.actions
        );
    }

    #[test]
    fn an_oversize_remote_file_does_not_override_a_hashable_local_file() {
        let got = plan(
            &[local("big.bin", "local-hash")],
            &[remote(
                "big.bin",
                &format!("{}{}", super::super::scan::OVERSIZE_PREFIX, 70 * 1024 * 1024),
            )],
            &[baseline("big.bin", "old-local", "old-remote")],
            &[],
            ask(),
        );
        assert!(
            got.actions.is_empty(),
            "an unknown remote hash must not trigger upload/download/conflict: {:?}",
            got.actions
        );
    }

    #[test]
    fn the_local_strategy_resolves_every_conflict_toward_local() {
        let opts = DiffOpts {
            strategy: ConflictStrategy::Local,
            decided: &[],
        };
        let got = only(plan(
            &[local("a.md", "1-new")],
            &[remote("a.md", "2-new")],
            &[baseline("a.md", "1", "2")],
            &[],
            opts,
        ));
        assert_eq!(
            got.action,
            Action::Conflict {
                resolution: Some(Resolution::KeepLocal),
                local_hash: "1-new".to_string(),
                remote_hash: "2-new".to_string(),
            }
        );
    }

    #[test]
    fn the_remote_strategy_resolves_every_conflict_toward_remote() {
        let opts = DiffOpts {
            strategy: ConflictStrategy::Remote,
            decided: &[],
        };
        let got = only(plan(
            &[local("a.md", "1-new")],
            &[remote("a.md", "2-new")],
            &[baseline("a.md", "1", "2")],
            &[],
            opts,
        ));
        assert_eq!(
            got.action,
            Action::Conflict {
                resolution: Some(Resolution::KeepRemote),
                local_hash: "1-new".to_string(),
                remote_hash: "2-new".to_string(),
            }
        );
    }

    #[test]
    fn every_conflict_path_honours_the_strategy() {
        // 四条冲突路径各写一遍策略映射的话,漏掉一处不会有任何症状 —— 那一处会在
        // local/remote 策略下依然挂起等用户。这里逐条过。
        let opts = DiffOpts {
            strategy: ConflictStrategy::Local,
            decided: &[],
        };
        let cases: Vec<(&str, SyncPlan)> = vec![
            (
                "both_modified",
                plan(
                    &[local("a.md", "1-new")],
                    &[remote("a.md", "2-new")],
                    &[baseline("a.md", "1", "2")],
                    &[],
                    opts,
                ),
            ),
            (
                "remote_deleted_local_modified",
                plan(
                    &[local("a.md", "1-new")],
                    &[],
                    &[baseline("a.md", "1", "2")],
                    &[],
                    opts,
                ),
            ),
            (
                "both_present_no_baseline",
                plan(
                    &[local("a.md", "mine")],
                    &[remote("a.md", "theirs")],
                    &[],
                    &[],
                    opts,
                ),
            ),
            (
                "local_tombstone_remote_modified",
                plan(
                    &[],
                    &[remote("a.md", "2-edited")],
                    &[],
                    &[tomb("a.md", "2")],
                    opts,
                ),
            ),
        ];
        for (reason, got) in cases {
            let action = only(got);
            assert_eq!(action.reason, reason);
            // 四条路径的两侧 hash 各不相同(有一条本地那侧还是空的),所以这里只断言
            // resolution。hash 的正确性由 `every_conflict_carries_the_two_hashes_it_saw` 管。
            assert!(
                matches!(
                    action.action,
                    Action::Conflict {
                        resolution: Some(Resolution::KeepLocal),
                        ..
                    }
                ),
                "{reason} 这条冲突没走策略映射:{:?}",
                action.action
            );
        }
    }

    // ---- 逐路径决定 ----

    #[test]
    fn every_conflict_carries_the_two_hashes_it_saw() {
        // 前端提交决定时要回传这两个值。任何一条冲突漏填(或填反),那条路径上的防覆盖
        // 闸门就没有输入 —— 症状是「这个文件的决定总是不生效」,而另外三条好使。
        let cases: [(&str, SyncPlan, &str, &str); 4] = [
            (
                "both_modified",
                plan(
                    &[local("a.md", "L")],
                    &[remote("a.md", "R")],
                    &[baseline("a.md", "1", "2")],
                    &[],
                    ask(),
                ),
                "L",
                "R",
            ),
            (
                // 远端没了 → 远端那侧是空串,不是某个占位 hash。
                "remote_deleted_local_modified",
                plan(
                    &[local("a.md", "L")],
                    &[],
                    &[baseline("a.md", "1", "2")],
                    &[],
                    ask(),
                ),
                "L",
                "",
            ),
            (
                "both_present_no_baseline",
                plan(
                    &[local("a.md", "L")],
                    &[remote("a.md", "R")],
                    &[],
                    &[],
                    ask(),
                ),
                "L",
                "R",
            ),
            (
                // 本地删了 → 本地那侧是空串。
                "local_tombstone_remote_modified",
                plan(
                    &[],
                    &[remote("a.md", "R")],
                    &[],
                    &[tomb("a.md", "2")],
                    ask(),
                ),
                "",
                "R",
            ),
        ];
        for (reason, got, want_local, want_remote) in cases {
            let action = only(got);
            assert_eq!(action.reason, reason);
            let Action::Conflict {
                local_hash,
                remote_hash,
                ..
            } = &action.action
            else {
                panic!("{reason} 不是冲突:{:?}", action.action);
            };
            assert_eq!(local_hash, want_local, "{reason} 的本地 hash");
            assert_eq!(remote_hash, want_remote, "{reason} 的远端 hash");
        }
    }

    #[test]
    fn a_stored_decision_resolves_the_conflict() {
        let decided = [decision("a.md", Resolution::KeepRemote, "L", "R")];
        let got = only(plan(
            &[local("a.md", "L")],
            &[remote("a.md", "R")],
            &[baseline("a.md", "1", "2")],
            &[],
            with_decided(&decided),
        ));
        assert!(
            matches!(
                got.action,
                Action::Conflict {
                    resolution: Some(Resolution::KeepRemote),
                    ..
                }
            ),
            "{:?}",
            got.action
        );
    }

    #[test]
    fn a_stored_decision_outranks_the_vault_strategy() {
        // 反过来的话,策略一旦不是 `Ask`,用户在面板上一条条做的选择就全被顶掉了 ——
        // 而他做那些选择的前提正是「这几个我要分别处理」。
        let decided = [decision("a.md", Resolution::KeepRemote, "L", "R")];
        let opts = DiffOpts {
            strategy: ConflictStrategy::Local,
            decided: &decided,
        };
        let got = only(plan(
            &[local("a.md", "L")],
            &[remote("a.md", "R")],
            &[baseline("a.md", "1", "2")],
            &[],
            opts,
        ));
        assert!(
            matches!(
                got.action,
                Action::Conflict {
                    resolution: Some(Resolution::KeepRemote),
                    ..
                }
            ),
            "{:?}",
            got.action
        );
    }

    #[test]
    fn a_decision_only_applies_to_its_own_path() {
        let decided = [decision("other.md", Resolution::KeepRemote, "L", "R")];
        let got = only(plan(
            &[local("a.md", "L")],
            &[remote("a.md", "R")],
            &[baseline("a.md", "1", "2")],
            &[],
            with_decided(&decided),
        ));
        assert!(
            matches!(
                got.action,
                Action::Conflict {
                    resolution: None,
                    ..
                }
            ),
            "{:?}",
            got.action
        );
    }

    #[test]
    fn a_decision_is_void_once_the_local_side_moves_on() {
        // 用户看着两份内容点了「采用远端」,然后又在本地写了几行。直接执行会把他刚写的
        // 那几行覆盖掉 —— 而他做决定时还不知道自己会写它们。
        let decided = [decision("a.md", Resolution::KeepRemote, "L", "R")];
        let got = only(plan(
            &[local("a.md", "L-again")],
            &[remote("a.md", "R")],
            &[baseline("a.md", "1", "2")],
            &[],
            with_decided(&decided),
        ));
        assert!(
            matches!(
                got.action,
                Action::Conflict {
                    resolution: None,
                    ..
                }
            ),
            "本地又变了,旧决定不该还算数:{:?}",
            got.action
        );
    }

    #[test]
    fn a_decision_is_void_once_the_remote_side_moves_on() {
        // 同一件事的另一半:他点了「保留本地」,期间另一台设备又改了远端。执行原决定会
        // 覆盖掉一份他从没见过的内容。
        let decided = [decision("a.md", Resolution::KeepLocal, "L", "R")];
        let got = only(plan(
            &[local("a.md", "L")],
            &[remote("a.md", "R-again")],
            &[baseline("a.md", "1", "2")],
            &[],
            with_decided(&decided),
        ));
        assert!(
            matches!(
                got.action,
                Action::Conflict {
                    resolution: None,
                    ..
                }
            ),
            "远端又变了,旧决定不该还算数:{:?}",
            got.action
        );
    }

    #[test]
    fn a_void_decision_falls_back_to_the_strategy_not_to_pending() {
        // 作废之后要退回策略,不是硬性挂起。策略是 `Remote` 的用户已经表达过「这个 vault
        // 一律采用远端」,一条过期的决定不该把他拽回逐条确认。
        let decided = [decision("a.md", Resolution::KeepLocal, "L", "R")];
        let opts = DiffOpts {
            strategy: ConflictStrategy::Remote,
            decided: &decided,
        };
        let got = only(plan(
            &[local("a.md", "L-again")],
            &[remote("a.md", "R")],
            &[baseline("a.md", "1", "2")],
            &[],
            opts,
        ));
        assert!(
            matches!(
                got.action,
                Action::Conflict {
                    resolution: Some(Resolution::KeepRemote),
                    ..
                }
            ),
            "{:?}",
            got.action
        );
    }

    #[test]
    fn a_decision_on_a_one_sided_conflict_matches_the_empty_hash() {
        // 「远端删了 / 本地改了」这类冲突里远端那侧是空串。决定也记空串,两边才对得上。
        // 拿某个占位值(比如 "0")去填的话,这类冲突的决定永远作废 —— 而它恰好是最需要
        // 用户拍板的一类。
        let decided = [decision("a.md", Resolution::KeepLocal, "L", "")];
        let got = only(plan(
            &[local("a.md", "L")],
            &[],
            &[baseline("a.md", "1", "2")],
            &[],
            with_decided(&decided),
        ));
        assert!(
            matches!(
                got.action,
                Action::Conflict {
                    resolution: Some(Resolution::KeepLocal),
                    ..
                }
            ),
            "{:?}",
            got.action
        );
    }

    #[test]
    fn a_fork_decision_survives_the_round_trip_into_the_plan() {
        let decided = [decision(
            "a.md",
            Resolution::Fork {
                fork_path: "a.conflict.md".to_string(),
            },
            "L",
            "R",
        )];
        let got = only(plan(
            &[local("a.md", "L")],
            &[remote("a.md", "R")],
            &[baseline("a.md", "1", "2")],
            &[],
            with_decided(&decided),
        ));
        let Action::Conflict {
            resolution: Some(Resolution::Fork { fork_path }),
            ..
        } = &got.action
        else {
            panic!("{:?}", got.action);
        };
        assert_eq!(fork_path, "a.conflict.md");
    }

    #[test]
    fn a_resolved_conflict_still_counts_as_a_conflict_in_the_summary() {
        // 汇总里的 `conflict` 数是「这一轮有几个并发」,不是「有几个还没定」。混淆的话
        // 状态栏在决定之后会显示 0 冲突,而文件还没搬 —— 用户以为已经完事了。
        let decided = [decision("a.md", Resolution::KeepLocal, "L", "R")];
        let got = plan(
            &[local("a.md", "L")],
            &[remote("a.md", "R")],
            &[baseline("a.md", "1", "2")],
            &[],
            with_decided(&decided),
        );
        assert_eq!(got.summary.conflict, 1);
        assert_eq!(got.summary.upload, 0, "决定不该在计划阶段就变成上传");
    }

    #[test]
    fn actions_come_out_sorted_and_counted() {
        let got = plan(
            &[local("b.md", "1"), local("a.md", "1")],
            &[remote("c.md", "9")],
            &[],
            &[],
            ask(),
        );
        let paths: Vec<&str> = got.actions.iter().map(|a| a.path.as_str()).collect();
        assert_eq!(paths, vec!["a.md", "b.md", "c.md"]);
        assert_eq!(got.summary.upload, 2);
        assert_eq!(got.summary.download, 1);
        assert_eq!(got.summary.conflict, 0);
    }

    #[test]
    fn a_file_both_sides_forgot_produces_nothing() {
        // 基线里有、两边都没有 = 双删。不产生动作,基线由 finalize 清掉。
        let got = plan(&[], &[], &[baseline("a.md", "1", "2")], &[], ask());
        assert!(got.actions.is_empty(), "{:?}", got.actions);
    }

    #[test]
    fn pending_tombstones_catch_exactly_the_locally_deleted_files() {
        let bases = vec![
            baseline("gone.md", "1", "2"),
            baseline("still.md", "3", "4"),
            baseline("already.md", "5", "6"),
        ];
        let locals = vec![local("still.md", "3")];
        let known = vec![tomb("already.md", "6")];
        let got = pending_tombstones(&locals, &bases, &known, NOW);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].path, "gone.md");
        assert_eq!(got[0].deleted_at, NOW);
        // remote_hash 要来自基线 —— 它就是「删除时远端是什么样」,后面判「远端有没有
        // 在删之后被改」全靠它。带错了会让 tombstone 永远命中 remote_modified 分支。
        assert_eq!(got[0].remote_hash, "2");
    }

    #[test]
    fn an_oversize_local_file_is_not_mistaken_for_a_deletion() {
        // 超大文件在扫描结果里(带 oversize hash)。pending_tombstones 只看路径在不在,
        // 所以它不该被当成已删。看 hash 的话会给它记一条 tombstone,下一轮就去删远端。
        let bases = vec![baseline("big.bin", "1", "2")];
        let locals = vec![oversize("big.bin")];
        assert!(pending_tombstones(&locals, &bases, &[], NOW).is_empty());
    }
}
