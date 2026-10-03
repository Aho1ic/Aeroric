//! P7-03 Action 1：Docker（core 独占；container/image 形状与 CLI 输出解析
//! ——与 `src-tauri/src/docker.rs` 同源语义 verbatim 上收；无 Docker daemon
//! 时显示不可用，不弹 sudo）。
use serde::{Deserialize, Serialize};

/// 长任务（Action 1：长任务返回 operationId，支持取消/状态查询）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LongOperation {
    pub operation_id: String,
    pub status: OperationStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
    Failed,
}

/// 长任务注册表（operationId → 状态；取消/状态查询）。
#[derive(Debug, Default)]
pub struct OperationRegistry {
    operations: std::collections::HashMap<String, OperationStatus>,
}

impl OperationRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一个长任务（operationId 由调用方铸造）。
    pub fn register(&mut self, operation_id: &str) {
        self.operations
            .insert(operation_id.to_string(), OperationStatus::Pending);
    }

    pub fn status(&self, operation_id: &str) -> Option<OperationStatus> {
        self.operations.get(operation_id).copied()
    }

    pub fn update(&mut self, operation_id: &str, status: OperationStatus) {
        self.operations.insert(operation_id.to_string(), status);
    }

    /// 取消（状态查询支持；Cancelled 后不可改）。
    pub fn cancel(&mut self, operation_id: &str) -> bool {
        if let Some(status) = self.operations.get_mut(operation_id) {
            if *status == OperationStatus::Pending || *status == OperationStatus::InProgress {
                *status = OperationStatus::Cancelled;
                return true;
            }
        }
        false
    }
}

/// Docker 镜像摘要。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DockerImageSummary {
    pub id: String,
    pub repository: String,
    pub tag: String,
    pub digest: String,
    pub created_since: String,
    pub size: String,
}

/// Docker 容器摘要。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DockerContainerSummary {
    pub id: String,
    pub image: String,
    pub names: String,
    pub state: String,
    pub status: String,
    pub ports: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawDockerImage {
    #[serde(rename = "ID")]
    id: Option<String>,
    repository: Option<String>,
    tag: Option<String>,
    digest: Option<String>,
    created_since: Option<String>,
    size: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawDockerContainer {
    #[serde(rename = "ID")]
    id: Option<String>,
    image: Option<String>,
    names: Option<String>,
    state: Option<String>,
    status: Option<String>,
    ports: Option<String>,
    #[serde(rename = "CreatedAt")]
    created_at: Option<String>,
}

/// Docker CLI JSON 输出解析（verbatim 上收；PascalCase 字段）。
pub fn parse_docker_images_json(stdout: &str) -> Vec<DockerImageSummary> {
    serde_json::from_str::<Vec<RawDockerImage>>(stdout)
        .map(|items| {
            items
                .into_iter()
                .map(|item| DockerImageSummary {
                    id: item.id.unwrap_or_default(),
                    repository: item.repository.unwrap_or_default(),
                    tag: item.tag.unwrap_or_default(),
                    digest: item.digest.unwrap_or_default(),
                    created_since: item.created_since.unwrap_or_default(),
                    size: item.size.unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn parse_docker_containers_json(stdout: &str) -> Vec<DockerContainerSummary> {
    serde_json::from_str::<Vec<RawDockerContainer>>(stdout)
        .map(|items| {
            items
                .into_iter()
                .map(|item| DockerContainerSummary {
                    id: item.id.unwrap_or_default(),
                    image: item.image.unwrap_or_default(),
                    names: item.names.unwrap_or_default(),
                    state: item.state.unwrap_or_default(),
                    status: item.status.unwrap_or_default(),
                    ports: item.ports.unwrap_or_default(),
                    created_at: item.created_at.unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// container 动作（Action 3：显式区分只读/破坏操作——破坏操作由调用方 UI
/// 确认；无 Docker daemon 时显示不可用，不弹 sudo）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DockerAction {
    Start,
    Stop,
    Restart,
    Remove,
}

impl DockerAction {
    /// 只读/破坏分类（Action 3：显式区分）。
    pub fn is_destructive(&self) -> bool {
        matches!(self, DockerAction::Remove)
    }
}

/// Docker 可用性判定（无 daemon 时显示不可用，不弹 sudo）。
pub fn daemon_unavailable_reason(daemon_output: &str) -> Option<String> {
    let lower = daemon_output.to_lowercase();
    if lower.contains("cannot connect") || lower.contains("is the docker daemon running") {
        Some("Docker daemon 不可用".to_string())
    } else if lower.contains("permission denied") {
        Some("Docker 权限不足（不弹 sudo；请检查 docker 组成员身份）".to_string())
    } else {
        None
    }
}
