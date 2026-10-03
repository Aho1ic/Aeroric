//! 随手记同步（P7-06 自 `src-tauri/src/notebook/sync/` 上收；scan/store/diff/
//! engine/local/git/schedule/remote/manifest 原样移动，daemon 重写为可关停
//! 监督——见 `daemon` 模块文档）。
//!
//! 分层与 P8 的子阶段一致:
//!
//! ```text
//! scan   本地一侧的输入:走一遍 vault 算全库签名(瞬态,每轮重算)
//! store  持久化的「上次同步成功时的样子」+ tombstone(每个远端目标一份)
//! diff   三方决策表:本地 × 远端 × 基线 → 动作清单(纯算法,不碰 IO)
//! engine 执行一轮:扫描 → 落 tombstone → diff → 逐条执行(带就地复验)→ 收尾
//! local  引擎的本地一侧出口:软删走回收站、写入过范围与 symlink 两道守卫
//! ```
//!
//! ## 引擎在 Rust 而不在前端(与 Markio 的架构分歧)
//!
//! Markio 把同步引擎放前端(`src/lib/sync/`,2973 行)。这边放 Rust,三条理由:传输层
//! `StorageBackend` 本身是 Rust 内部 trait,放前端等于先为它造一层逐文件 IPC;每个文件
//! 的字节都要过一次 IPC(Markio 的 `LocalFs.read` 统一走 base64),整库同步时这层开销
//! 是白付的;hash 必须和 `state::hash64` 一致,而那是 Rust。
//!
//! 代价是 Markio 的 `diff.ts` 与它的单测不能照抄,要重写 —— 决策表本身约 200 行纯逻辑。
//!
//! ## 为什么基线不并进 `index.db`
//!
//! RAG 索引库里的 `docs` 表已经有 `path` 和维护中的 `hash`,看着正好能用。但
//! `notebook_rag_clear` 会把那个库整个清掉,而基线一空,下一轮 diff 就把远端还在的
//! 文件全判成新文件拉回来 —— **已删除的笔记被复活**。让「清空 AI 索引」这个纯本地
//! 操作能导致数据复活是不能接受的。RAG 索引本身也是可选的(用户可能永远不建),而
//! 同步必须能独立工作。
//!
//! 两个库分开,但 hash 复用 `state::hash64`:`rag/index.rs` 已经在用它,同步再起
//! 第三套只会让三处基线互相对不上。
//!
//! ## daemon 是这条规则里唯一的重写（G03）
//!
//! 宿主版 daemon 以裸 `thread::spawn` 起后台循环（`G03`：app-lifetime 线程无关停
//! 路径）。core 版 [`daemon::SyncDaemonHandle`] 持 `JoinHandle`＋`AtomicBool`
//! 停止位，`stop()` 置位并 join——关窗/切模式不再留下不可关停的孤儿线程；
//! 宿主把 guard 存进 Tauri state，退出时 drop 即停。

pub mod daemon;
pub mod diff;
pub mod engine;
pub mod git;
pub mod local;
pub mod manifest;
pub mod remote;
pub mod scan;
pub mod schedule;
pub mod store;
