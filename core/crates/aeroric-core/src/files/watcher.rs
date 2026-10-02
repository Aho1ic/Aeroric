//! P6-01 Action 5：文件 watcher 服务（真实 notify watcher——watcher 纯决策
//! 在 `watch.rs`，服务进程在这里；服务持有 [`watch::CancelToken`]，事件驱动
//! ＋1s 兜底轮询，不用 Swift Timer 高频轮询）。
use super::watch;

/// watcher 事件（coalesce 后的批次）。
pub enum WatchOutcome {
    /// 同批去重后的路径（按需刷新这些目录，不每个 event 重读整仓）。
    Batch(Vec<String>),
    /// 事件量溢出（notify 环形缓冲被冲掉）→ 整仓 rescan 一次。
    Rescan,
}

/// 服务句柄：watcher 线程持有 CancelToken；drop 即取消（通道断开，线程退出）。
pub struct FileWatcher {
    token: std::sync::Arc<watch::CancelToken>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl FileWatcher {
    /// spawn 真实 watcher（notify RecommendedWatcher, NonRecursive——按目录
    /// watch/unwatch 与 notebook_sync_daemon 同先例）；`on_batch` 在 coalesce
    /// 后的批次上回调（调用方决定刷新什么）。
    pub fn spawn(
        roots: Vec<std::path::PathBuf>,
        mut on_batch: impl FnMut(WatchOutcome) + Send + 'static,
    ) -> Result<Self, String> {
        use notify::Watcher;
        let token = std::sync::Arc::new(watch::CancelToken::new());
        let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
        let mut watcher = notify::RecommendedWatcher::new(tx, notify::Config::default())
            .map_err(|error| format!("cannot create watcher: {error}"))?;
        for root in &roots {
            watcher
                .watch(root, notify::RecursiveMode::NonRecursive)
                .map_err(|error| format!("cannot watch {}: {error}", root.display()))?;
        }
        let thread_token = std::sync::Arc::clone(&token);
        let handle = std::thread::spawn(move || {
            // 持有 watcher 直到线程退出（drop 即取消监听）。
            let _watcher = watcher;
            let mut pending: Vec<String> = Vec::new();
            loop {
                if thread_token.is_cancelled() {
                    return;
                }
                // recv_timeout 事件驱动；1s 兜底轮询（与 event_watcher 同先例）。
                match rx.recv_timeout(std::time::Duration::from_secs(1)) {
                    Ok(Ok(event)) => {
                        for path in event.paths {
                            if let Some(text) = path.to_str() {
                                pending.push(text.to_string());
                            }
                        }
                        // coalesce：recv_timeout 后排空同批事件（不每个 event
                        // 重读）。
                        while let Ok(more) = rx.try_recv() {
                            if let Ok(event) = more {
                                for path in event.paths {
                                    if let Some(text) = path.to_str() {
                                        pending.push(text.to_string());
                                    }
                                }
                            }
                        }
                        let batch = watch::coalesce(&pending);
                        if watch::should_rescan(batch.paths.len()) {
                            pending.clear();
                            on_batch(WatchOutcome::Rescan);
                        } else {
                            pending.clear();
                            on_batch(WatchOutcome::Batch(batch.paths));
                        }
                    }
                    Ok(Err(_)) => {
                        // notify 错误（环形缓冲溢出的信号）→ rescan。
                        on_batch(WatchOutcome::Rescan);
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        // 兜底：无事件，继续等（取消 token 检查在上面）。
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        // 通道断开（watcher drop）→ 退出。
                        return;
                    }
                }
            }
        });
        Ok(Self {
            token,
            handle: Some(handle),
        })
    }

    /// 取消（服务持有；drop 即取消——不用 Timer 轮询的显式化）。
    pub fn cancel(&self) {
        self.token.cancel();
    }
}

impl Drop for FileWatcher {
    fn drop(&mut self) {
        self.token.cancel();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
