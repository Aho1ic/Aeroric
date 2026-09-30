//! 事件序号分配/发布的单一原子边界（P3-06 Action 2）。
//!
//! 行为参考是宿主 `remote/event_log.rs`（重连 watermark 补发的数据源），
//! 但它**正是 Action 2 点名的反例形状**：`latest.fetch_add` 在锁外先定 seq，
//! 再另取 entries 锁 push——两个线程并发 append 时，拿到 seq=1 的线程可能
//! 比拿到 seq=2 的线程晚进锁，环形缓冲里就会出现 `[2, 1]`；`since()` 的
//! 补发序、`oldest` 判据、`filter(seq>after)` 的结果全部依赖「push 序＝seq 序」，
//! 这个前提在原实现里没有任何机制保证（Relaxed 只是不保证可见性顺序，真正的
//! 洞是**分配与发布跨了两个锁域**）。
//!
//! 这里重立为：**seq 分配、入环、`latest` 更新在同一个临界区内完成**。
//! `latest` 仍留一个原子镜像供无锁读，但它在锁内 store、锁释放前完成——
//! 读者看到 `latest == N` 时，N 必然已在环里；反向的瞬时滞后（环先进、镜像
//! 后见）只会造成「多补一条」，永远不造成「漏补」。
//!
//! 白名单（只记低频、错过会错乱的事件）是**调用方**的职责，与参考实现同形；
//! 本模块不筛。

pub mod log;
pub mod snapshot;

pub use log::{EventLog, LoggedEvent};
pub use snapshot::{since_response, SnapshotDecision};
