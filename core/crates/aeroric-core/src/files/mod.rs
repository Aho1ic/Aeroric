//! P6-01：文件服务模块（core 独占；宿主 command 只做 adapter）。
//!
//! - [`path_policy`]：server 端路径校验（不依赖客户端过滤 `..`；TOCTOU 界面）。
//! - [`read_write`]：read/list/create/rename/copy/trash/delete 业务主体。
//! - [`search`]：rg JSON 解析＋替换 preview/apply（stale match 拒绝不覆盖）。
//! - [`watch`]：watcher coalesce/overflow → rescan 纯决策＋取消 token。
pub mod path_policy;
pub mod read_write;
pub mod search;
pub mod watch;
pub mod watcher;
