//! watcher 的监督者:每个后台循环领一枚 [`ShutdownToken`],退出时统一取消并 join。
//!
//! 计划验收(`tests/runtime_lifecycle.rs`)的三条判据对应这里的三个行为:
//! 启动两次不重复 watcher(按名字去重注册)、shutdown 两次安全(join 幂等)、
//! 迟到的退出监控不能清新 generation(那是 [`crate::tasks::executor::LiveRuns`] 与
//! `RunKey` 寻址的职责,监督者只管"循环停没停",不碰任何按代键的表)。

use std::collections::HashMap;
use std::sync::Mutex;

use super::shutdown::ShutdownToken;

struct Supervised {
    token: ShutdownToken,
    handle: Option<std::thread::JoinHandle<()>>,
}

/// 按名字登记的后台循环监督者。名字是"启动两次不重复"的键:同名的第二次启动
/// 要么复用要么被拒,由调用方根据 [`WatcherSupervisor::is_running`] 决定。
#[derive(Default)]
pub struct WatcherSupervisor {
    watchers: Mutex<HashMap<String, Supervised>>,
}

impl WatcherSupervisor {
    pub fn new() -> Self {
        Self::default()
    }

    /// 在一个独立线程里跑 `body`,并把令牌与句柄登记在案。`body` 必须周期性检查
    /// `token.is_cancelled()` 并自行返回——监督者的取消只翻转标志,不杀线程。
    pub fn spawn_supervised<F>(&self, name: &str, body: F) -> Result<(), SuperviserError>
    where
        F: FnOnce(ShutdownToken) + Send + 'static,
    {
        let mut watchers = self.watchers.lock().expect("supervisor lock poisoned");
        if watchers.contains_key(name) {
            return Err(SuperviserError::AlreadyRunning {
                name: name.to_string(),
            });
        }
        let token = ShutdownToken::new();
        let worker_token = token.clone();
        let handle = std::thread::Builder::new()
            .name(format!("aeroric-watcher:{name}"))
            .spawn(move || body(worker_token))
            .map_err(|error| SuperviserError::SpawnFailed {
                name: name.to_string(),
                error,
            })?;
        watchers.insert(
            name.to_string(),
            Supervised {
                token,
                handle: Some(handle),
            },
        );
        Ok(())
    }

    pub fn is_running(&self, name: &str) -> bool {
        self.watchers
            .lock()
            .expect("supervisor lock poisoned")
            .contains_key(name)
    }

    /// 取消全部 watcher 并等待它们的线程退出。幂等:第二次调用没有任何在册条目,
    /// 直接返回;join 之后句柄被清掉,登记表归空,可以安全重用同一监督者。
    pub fn shutdown(&self) {
        let mut watchers = self.watchers.lock().expect("supervisor lock poisoned");
        for supervised in watchers.values() {
            supervised.token.cancel();
        }
        for (_, mut supervised) in watchers.drain() {
            if let Some(handle) = supervised.handle.take() {
                let _ = handle.join();
            }
        }
    }
}

/// 监督者的两种启动失败。同名重复是"启动两次"那条验收要接住的形状,
/// spawn 系统错误原样携带,不吞。
#[derive(Debug)]
pub enum SuperviserError {
    AlreadyRunning { name: String },
    SpawnFailed { name: String, error: std::io::Error },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn a_second_start_of_the_same_watcher_is_rejected_while_it_runs() {
        let supervisor = WatcherSupervisor::new();
        let (tx, rx) = std::sync::mpsc::channel();
        supervisor
            .spawn_supervised("loop", move |token| {
                tx.send(()).expect("report start");
                while !token.is_cancelled() {
                    std::thread::sleep(Duration::from_millis(1));
                }
            })
            .expect("first start");
        rx.recv_timeout(Duration::from_secs(5)).expect("started");

        assert!(supervisor.is_running("loop"));
        let second = supervisor.spawn_supervised("loop", |_| {});
        assert!(
            matches!(second, Err(SuperviserError::AlreadyRunning { .. })),
            "同名的第二次启动必须被拒,而不是悄悄再开一个循环"
        );
        supervisor.shutdown();
    }

    #[test]
    fn shutdown_cancels_the_token_and_joins_the_thread_before_returning() {
        let supervisor = WatcherSupervisor::new();
        let observed = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&observed);
        supervisor
            .spawn_supervised("counting", move |token| {
                while !token.is_cancelled() {
                    seen.fetch_add(1, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(1));
                }
            })
            .expect("start");

        supervisor.shutdown();
        assert!(!supervisor.is_running("counting"));
        let after_shutdown = observed.load(Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(
            observed.load(Ordering::SeqCst),
            after_shutdown,
            "join 返回后线程必须已经退出,计数不得再涨"
        );
    }

    #[test]
    fn a_second_shutdown_is_a_no_op_and_the_supervisor_is_reusable() {
        let supervisor = WatcherSupervisor::new();
        supervisor.spawn_supervised("brief", |_| {}).expect("start");
        supervisor.shutdown();
        supervisor.shutdown(); // 必须安静地什么也不做

        // 登记表已空,同一个监督者可以再次启动同名 watcher(重启语义)。
        supervisor
            .spawn_supervised("brief", |_| {})
            .expect("restart after shutdown");
        supervisor.shutdown();
    }

    #[test]
    fn shutdown_reaches_every_registered_watcher_not_just_the_first() {
        let supervisor = WatcherSupervisor::new();
        let counter = Arc::new(AtomicUsize::new(0));
        for name in ["a", "b", "c"] {
            let seen = Arc::clone(&counter);
            supervisor
                .spawn_supervised(name, move |token| {
                    while !token.is_cancelled() {
                        seen.fetch_add(1, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(1));
                    }
                })
                .expect("start");
        }
        supervisor.shutdown();
        assert!(!supervisor.is_running("a") && !supervisor.is_running("c"));
    }
}
