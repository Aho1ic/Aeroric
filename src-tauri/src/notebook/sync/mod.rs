//! 随手记同步。
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

// 这一层曾经有六个 `#![allow(dead_code)]`(那时候 84 处「还没有非测试调用方」),现在**一条
// 都没有了** —— 云盘的命令层(`notebook_sync_bind` / `unbind` / `remotes` / `run`)接上之后
// 每个导出项都有了真实调用方。摘掉它们的过程还顺带查出一处真的没人调:`prune_tombstones`。
// 那不是「暂时没接」,是 tombstone 表会无界增长(`live_tombstones` 只在读时按 TTL 过滤,
// 从不删行),现在接到了 `engine::run` 的开头。
//
// 别再往这里加 mod 级 allow。要加就加在具体文件上并写清条件,否则它会吞掉将来真正的死代码。

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
