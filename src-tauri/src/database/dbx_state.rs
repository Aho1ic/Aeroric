pub(crate) struct DbxState(pub(crate) aeroric_core::database::dbx_state::DbxState);

impl std::ops::Deref for DbxState {
    type Target = aeroric_core::database::dbx_state::DbxState;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
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
        let (state, degraded_reason) =
            tauri::async_runtime::block_on(aeroric_core::database::dbx_state::DbxState::new(
                &data_dir.path,
                env!("CARGO_PKG_VERSION"),
            ));
        if let Some(reason) = degraded_reason {
            crate::startup_diagnostics::record("dbx-state", reason, ":memory:");
        }
        Self(state)
    }
}
