use std::collections::HashMap;
use std::sync::Arc;

use dbx_core::connection::AppState;
use dbx_core::storage::Storage;
use tokio::sync::{oneshot, Mutex, RwLock};

use super::types::AeroricDbConnectionConfig;

pub(crate) struct DbxState {
    pub app_state: Arc<AppState>,
    pub connections: RwLock<HashMap<String, AeroricDbConnectionConfig>>,
    pub loaded_connections: RwLock<bool>,
    /// 等待前端答复的生产库确认。key 是 request id,前端带着它回调
    /// `respond_dbx_production_confirmation`。
    ///
    /// 挂在这里而不是 `AppState`:后者来自 dbx_core(path 依赖的外部 crate),
    /// 不该为 Aeroric 的 UI 流程改它的形状。
    pub pending_production_confirmations: Mutex<HashMap<String, oneshot::Sender<bool>>>,
}

impl DbxState {
    /// 启动期构造,永不失败。
    ///
    /// 原先返回 `Result` 并在 `run()` 里 `.expect()`:磁盘满、home 只读、
    /// `dbx-core.db` 被别的进程锁住或损坏,都会在窗口出现之前 panic,用户只看到
    /// 图标闪一下。数据库连不上不该让整个应用打不开——其余功能(任务、终端、git)
    /// 与它无关。所以逐级降级:正常目录 → 临时目录 → 内存库,并把原因记进
    /// 启动诊断,由前端弹横幅告知。
    pub(crate) fn new_blocking() -> Self {
        let data_dir = crate::storage::resolve_data_dir();
        if let Some(reason) = &data_dir.degraded_reason {
            crate::startup_diagnostics::record(
                "dbx-state",
                reason.clone(),
                data_dir.path.display().to_string(),
            );
        }
        let plugin_dir = data_dir.path.join("dbx-plugins");
        let agent_dir = data_dir.path.join("dbx-agents");
        // 插件/agent 目录建不出来只影响这两项功能,不影响连库,失败不升级。
        let _ = std::fs::create_dir_all(&plugin_dir);
        let _ = std::fs::create_dir_all(&agent_dir);

        let db_path = data_dir.path.join("dbx-core.db");
        let storage = match tauri::async_runtime::block_on(Storage::open(&db_path)) {
            Ok(storage) => storage,
            Err(disk_error) => {
                // 最后一层退路:内存库。连接配置不跨重启保留,但应用能开、能用。
                crate::startup_diagnostics::record("dbx-state", disk_error, ":memory:");
                tauri::async_runtime::block_on(Storage::open(std::path::Path::new(":memory:")))
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
            env!("CARGO_PKG_VERSION"),
        ));
        Self {
            app_state,
            connections: RwLock::new(HashMap::new()),
            loaded_connections: RwLock::new(false),
            pending_production_confirmations: Mutex::new(HashMap::new()),
        }
    }
}
