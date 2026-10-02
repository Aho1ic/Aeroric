//! P7-01 Action 1：SFTP（core 独占；entry 形状、路径校验、传输队列纯逻辑
//! ——与 `src-tauri/src/sftp.rs` 同源语义 verbatim 上收；SFTP 进程由宿主管理）。
use serde::Serialize;

/// SFTP 条目（与 Tauri `SftpEntry` 字段对齐）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SftpEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub extension: Option<String>,
    pub size: Option<u64>,
    pub modified_at_ms: Option<u64>,
}

/// 目录摘要。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SftpDirectorySummary {
    pub file_count: u64,
    pub directory_count: u64,
    pub total_size: u64,
    pub modified_at_ms: Option<u64>,
}

/// entry 名校验（verbatim）。
pub fn validate_entry_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name == "." || name == ".." {
        return Err("Invalid file name".to_string());
    }
    if name.len() > 255 {
        return Err("File name is too long".to_string());
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err("File name contains forbidden characters".to_string());
    }
    Ok(())
}

/// 远程路径 NUL 守卫（verbatim）。
pub fn ensure_no_nul(path: &str) -> Result<String, String> {
    if path.contains('\0') {
        return Err("Remote path contains forbidden characters".to_string());
    }
    Ok(path.to_string())
}

/// basename（verbatim）。
pub fn basename(path: &str) -> Result<&str, String> {
    let trimmed = path.trim_end_matches('/');
    trimmed
        .rsplit_once('/')
        .map(|(_, name)| name)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "Invalid file name".to_string())
}

/// remote parent（verbatim）。
pub fn remote_parent(path: &str) -> Result<&str, String> {
    let trimmed = path.trim_end_matches('/');
    let Some((parent, _)) = trimmed.rsplit_once('/') else {
        return Err("Cannot resolve parent directory".to_string());
    };
    Ok(if parent.is_empty() { "/" } else { parent })
}

pub fn join_remote_path(parent: &str, name: &str) -> String {
    if parent == "/" {
        format!("/{name}")
    } else {
        format!("{}/{}", parent.trim_end_matches('/'), name)
    }
}

/// 传输队列（Action 5：上传/下载进度、取消、恢复、覆盖确认与权限错误逐项
/// 实现；cancel transfer 清临时文件而非删除原目标）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransferStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferItem {
    pub id: String,
    pub direction: TransferDirection,
    pub local_path: String,
    pub remote_path: String,
    pub status: TransferStatus,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransferDirection {
    Upload,
    Download,
}

/// 传输队列（进度/取消/恢复的纯簿记；覆盖确认由调用方 UI 承载）。
#[derive(Debug, Default)]
pub struct TransferQueue {
    items: Vec<TransferItem>,
}

impl TransferQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn items(&self) -> &[TransferItem] {
        &self.items
    }

    pub fn enqueue(&mut self, item: TransferItem) {
        self.items.push(item);
    }

    /// 进度推进。
    pub fn update_progress(&mut self, id: &str, bytes: u64) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.bytes_transferred = bytes;
            if item.status == TransferStatus::Pending {
                item.status = TransferStatus::InProgress;
            }
        }
    }

    pub fn complete(&mut self, id: &str) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.status = TransferStatus::Completed;
        }
    }

    pub fn fail(&mut self, id: &str, error: String) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.status = TransferStatus::Failed;
            item.error = Some(error);
        }
    }

    /// 取消（cancel transfer 清临时文件而非删除原目标——调用方清理临时文件，
    /// 队列只标状态）。
    pub fn cancel(&mut self, id: &str) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.status = TransferStatus::Cancelled;
        }
    }

    /// 恢复（断线后恢复：从头或断点续传由调用方承载，队列标回 Pending）。
    pub fn resume(&mut self, id: &str) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.status = TransferStatus::Pending;
            item.error = None;
        }
    }
}
