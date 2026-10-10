//! P6-01：文件服务模块（core 独占；宿主 command 只做 adapter）。
//!
//! - [`path_policy`]：server 端路径校验（不依赖客户端过滤 `..`；TOCTOU 界面）。
//! - [`read_write`]：read/list/create/rename/copy/trash/delete 业务主体。
//! - [`search`]：rg JSON 解析＋替换 preview/apply（stale match 拒绝不覆盖）。
//! - [`watch`]：watcher coalesce/overflow → rescan 纯决策＋取消 token。
pub mod format;
pub mod legacy_copy;
pub mod legacy_directory;
pub mod legacy_entry;
pub mod legacy_history;
pub mod legacy_modify;
pub mod legacy_path;
pub mod legacy_read;
pub mod legacy_write;
pub mod local_history;
pub mod path_policy;
pub mod preview;
pub mod project_files;
pub mod read_write;
pub mod search;
pub mod watch;
pub mod watcher;
