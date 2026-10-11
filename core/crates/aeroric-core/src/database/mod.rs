//! P7-04：database 模块（core 独占；production assessment/grid 分页/确认链
//! 纯函数；DBX 业务主体在 dbx-core，原 command 同名保留为 adapter）。
pub mod dbx_connections;
pub mod dbx_secrets;
pub mod dbx_state;
pub mod dbx_types;
pub mod grid;
pub mod legacy_sqlite;
pub mod query;
pub mod sql_builders;
