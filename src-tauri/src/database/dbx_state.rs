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
    pub(crate) fn new_blocking() -> Result<Self, String> {
        crate::storage::ensure_aeroric_dirs()?;
        let data_dir = crate::storage::aeroric_dir()?;
        let db_path = data_dir.join("dbx-core.db");
        let plugin_dir = data_dir.join("dbx-plugins");
        let agent_dir = data_dir.join("dbx-agents");
        std::fs::create_dir_all(&plugin_dir).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&agent_dir).map_err(|e| e.to_string())?;
        let storage = tauri::async_runtime::block_on(Storage::open(&db_path))?;
        let app_state = Arc::new(AppState::new_with_plugin_and_agent_dir_and_app_version(
            storage,
            plugin_dir,
            agent_dir,
            env!("CARGO_PKG_VERSION"),
        ));
        Ok(Self {
            app_state,
            connections: RwLock::new(HashMap::new()),
            loaded_connections: RwLock::new(false),
            pending_production_confirmations: Mutex::new(HashMap::new()),
        })
    }
}
