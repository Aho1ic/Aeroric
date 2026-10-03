//! 存储后端（宿主 shim）。P7-06：业务本体上收 `aeroric_core::storage_backends`
//! （trait/能力表/18 协议元信息/5 个后端/路径守卫 原样移动；`build_backend`
//! 以 data_dir 参数化）。这里保留宿主旧的无 data_dir 签名——数据根取宿主
//! `aeroric_dir()`——调用点（sftp/ssh/remote_fs）零改动。

pub use aeroric_core::storage_backends::*;

/// 按连接构造后端实例（数据根 = 宿主 `aeroric_dir()`；mount 后端的挂载点在
/// `<aeroric-dir>/mounts/<id>`，与迁移前一致）。
pub fn build_backend(connection: &StorageConnection) -> Result<Box<dyn StorageBackend>, String> {
    aeroric_core::storage_backends::build_backend(&crate::storage::aeroric_dir()?, connection)
}

/// 卸载某个系统挂载连接（storage_conn 命令层用）。
pub fn unmount(data_dir: &std::path::Path, connection_id: &str) -> Result<(), String> {
    mount::unmount(data_dir, connection_id)
}
