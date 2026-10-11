//! Shared DBX state, initialized on the caller's live async runtime.
//! Paths and application version are injected; disk degradation is returned to
//! the adapter's diagnostics log. No environment lookup or temporary runtime.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use dbx_core::connection::AppState;
use dbx_core::storage::Storage;
use tokio::sync::{Mutex, RwLock};

use crate::database::dbx_types::AeroricDbConnectionConfig;

pub struct DbxState {
    pub app_state: Arc<AppState>,
    pub connections: RwLock<HashMap<String, AeroricDbConnectionConfig>>,
    pub loaded_connections: RwLock<bool>,
    /// Serializes connection loading and whole-file RMW inside one instance.
    /// The adapter must still hold the cross-process data-directory writer lease.
    pub(crate) connection_operations: Mutex<()>,
    /// 等待前端答复的生产库确认。key 是 request id,前端带着它回调
    /// `respond_dbx_production_confirmation`。
    ///
    /// 挂在这里而不是 `AppState`:后者来自 dbx_core(path 依赖的外部 crate),
    /// 不该为 Aeroric 的 UI 流程改它的形状。
    pub pending_production_confirmations:
        Mutex<HashMap<String, tokio::sync::oneshot::Sender<bool>>>,
}

impl DbxState {
    /// 启动期构造,永不失败。
    ///
    /// 原先返回 `Result` 并在 `run()` 里 `.expect()`:磁盘满、home 只读、
    /// `dbx-core.db` 被别的进程锁住或损坏,都会在窗口出现之前 panic,用户只看到
    /// 图标闪一下。数据库连不上不该让整个应用打不开——其余功能(任务、终端、git)
    /// 与它无关。目录选择/临时目录回退属于调用方；这里在所注入的目录失败时
    /// 退到内存库，并返回原因供调用方记录。连接 JSON 仍在原注入目录，
    /// 其持久化失败不因 SQLite 降级而被吞掉。
    pub async fn new(data_dir: &Path, app_version: &str) -> (Self, Option<String>) {
        let plugin_dir = data_dir.join("dbx-plugins");
        let agent_dir = data_dir.join("dbx-agents");
        // 插件/agent 目录建不出来只影响这两项功能,不影响连库,失败不升级。
        let _ = std::fs::create_dir_all(&plugin_dir);
        let _ = std::fs::create_dir_all(&agent_dir);

        let db_path = data_dir.join("dbx-core.db");
        let mut degradation: Option<String> = None;
        // 挂在整条 `let` 上：`panic!` 是下面 `Err` 分支里闭包的尾表达式，属性只能上提到这里。
        #[allow(clippy::panic, reason = "内存库都起不来，刻意 fail-fast")]
        let storage = match Storage::open(&db_path).await {
            Ok(storage) => storage,
            Err(disk_error) => {
                // 最后一层退路:仅 DBX 内部存储退到内存，不迁移连接 JSON 根。
                degradation = Some(disk_error.clone());
                Storage::open(std::path::Path::new(":memory:"))
                    .await
                    .unwrap_or_else(|memory_error| {
                        // 内存库都开不起来说明 SQLite 本身不可用,已无退路可选。
                        // 这里保留 panic 而不是静默:继续跑只会在每条命令上报错。
                        panic!("SQLite is unusable, even in-memory: {memory_error}")
                    })
            }
        };
        let app_state = Arc::new(AppState::new_with_plugin_and_agent_dir_and_app_version(
            storage,
            plugin_dir,
            agent_dir,
            app_version,
        ));
        (
            Self {
                app_state,
                connections: RwLock::new(HashMap::new()),
                loaded_connections: RwLock::new(false),
                connection_operations: Mutex::new(()),
                pending_production_confirmations: Mutex::new(HashMap::new()),
            },
            degradation,
        )
    }
}
