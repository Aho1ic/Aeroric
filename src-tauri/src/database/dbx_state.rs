use std::collections::HashMap;
use std::sync::Arc;

use dbx_core::connection::AppState;
use dbx_core::storage::Storage;
use tokio::sync::RwLock;

use super::types::AeroricDbConnectionConfig;

pub(crate) struct DbxState {
    pub app_state: Arc<AppState>,
    pub connections: RwLock<HashMap<String, AeroricDbConnectionConfig>>,
    pub loaded_connections: RwLock<bool>,
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
        })
    }
}
