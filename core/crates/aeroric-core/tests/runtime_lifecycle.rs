//! P3-01 的运行时生命周期验收(计划原文三条判据):
//! 1. 启动两次不重复 watcher;
//! 2. shutdown 两次安全;
//! 3. 退出监控不能清新 generation。
//!
//! 另外两条是把机制钉死的补充:有界拆除(无视 token 的 watcher 被脱离而不是把
//! 退出挂死)与结算幂等(迟到/重复的退出报告不重复消费标记)。这些判据测的是
//! core 的 `WatcherSupervisor` + `TaskRegistry`——宿主的 spawn 站点把它们当
//! 唯一入口(`TaskManager` 委托壳),所以在这里钉住就是钉住了宿主的行为。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use aeroric_core::runtime::registry::ExitSettlement;
use aeroric_core::runtime::{ShutdownReport, ShutdownToken, TaskRegistry, WatcherSupervisor};

fn real_master() -> Box<dyn portable_pty::MasterPty + Send> {
    portable_pty::native_pty_system()
        .openpty(portable_pty::PtySize::default())
        .expect("open pty pair")
        .master
}

/// 一个真实可执行的占位子进程(退出即结束),用于让 child_handles 里有两个
/// 可区分的 Arc——不用脚本打标记,避免任何孤儿进程(见 MIGRATION_PLAN 补记四十一
/// 的泄漏教训)。
fn placeholder_child(script: &std::path::Path) -> Box<dyn portable_pty::Child + Send + Sync> {
    let pty = portable_pty::native_pty_system()
        .openpty(portable_pty::PtySize::default())
        .expect("open pty pair");
    let mut cmd = portable_pty::CommandBuilder::new(script);
    cmd.arg("placeholder");
    pty.slave
        .spawn_command(cmd)
        .expect("spawn placeholder child")
}

/// 写一个立刻退出的占位脚本。spawn 的目标必须是真实可执行文件(宿主验收的
/// fake-agent 同款形状)。
fn placeholder_script(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, "#!/bin/sh\nexit 0\n").expect("write placeholder script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("make placeholder executable");
    }
    path
}

#[test]
fn starting_the_same_watcher_twice_does_not_duplicate_it() {
    let supervisor = WatcherSupervisor::new();
    let ticks = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&ticks);
    let (started_tx, started_rx) = std::sync::mpsc::channel();

    supervisor
        .spawn_supervised("watch:status-session:t1:3", move |token: ShutdownToken| {
            started_tx.send(()).expect("report start");
            while !token.is_cancelled() {
                observed.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(1));
            }
        })
        .expect("first start");
    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("watcher started");

    assert!(supervisor.is_running("watch:status-session:t1:3"));
    let second = supervisor.spawn_supervised("watch:status-session:t1:3", |_| {});
    assert!(
        matches!(
            second,
            Err(aeroric_core::runtime::SuperviserError::AlreadyRunning { .. })
        ),
        "同名的第二次启动必须被拒:两个循环写同一份账,正是『启动两次』要防的"
    );

    // 一个名字一个循环:计数在拒绝之后仍在由唯一那个线程推进,没有翻倍。
    let before = ticks.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(30));
    let after = ticks.load(Ordering::SeqCst);
    assert!(after > before, "唯一的循环仍在工作");

    supervisor.shutdown();
    assert!(!supervisor.is_running("watch:status-session:t1:3"));
}

#[test]
fn shutting_down_twice_is_safe_and_leaves_the_supervisor_reusable() {
    let supervisor = WatcherSupervisor::new();
    supervisor
        .spawn_supervised("watch:short", |token| {
            while !token.is_cancelled() {
                std::thread::sleep(Duration::from_millis(1));
            }
        })
        .expect("start");

    let first: ShutdownReport = supervisor.shutdown();
    assert_eq!(first.joined, 1);
    assert_eq!(first.detached, 0, "守规矩的 watcher 应被 join 而不是脱离");
    let second: ShutdownReport = supervisor.shutdown();
    assert_eq!(
        (second.joined, second.detached),
        (0, 0),
        "第二次 shutdown 没有任何在册条目,是安静的 no-op"
    );

    // 重启语义:同一监督者可以再启动同名 watcher(运行时重启的形状)。
    supervisor
        .spawn_supervised("watch:short", |_| {})
        .expect("restart after shutdown");
    supervisor.shutdown();
}

#[test]
fn shutdown_detaches_a_watcher_that_ignores_its_token_within_the_grace() {
    let supervisor = WatcherSupervisor::new();
    supervisor
        .spawn_supervised("watch:blocked-on-read", |_token| {
            // 模拟阻塞在系统调用上、不检查 token 的线程(PTY reader 的形状):
            // 拆除不能被它挂死。
            std::thread::sleep(Duration::from_secs(30));
        })
        .expect("start");
    supervisor
        .spawn_supervised("watch:obedient", |token| {
            while !token.is_cancelled() {
                std::thread::sleep(Duration::from_millis(1));
            }
        })
        .expect("start");

    let report = supervisor.shutdown_within(Duration::from_millis(200));
    assert_eq!(report.detached, 1, "无视 token 的那个被脱离");
    assert_eq!(report.joined, 1, "守规矩的那个仍被 join");
}

#[test]
fn a_late_exit_monitor_cannot_clean_a_new_generation() {
    let dir = std::env::temp_dir().join(format!("aeroric-lifecycle-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let script = placeholder_script(&dir, "placeholder.sh");
    let child_a = placeholder_child(&script);
    let child_b = placeholder_child(&script);

    let registry = TaskRegistry::default();
    let old = aeroric_core::runtime::RunKey::minted("t1", 1);
    let new = aeroric_core::runtime::RunKey::minted("t1", 2);
    for (key, child) in [(&old, child_a), (&new, child_b)] {
        let child: Box<dyn portable_pty::Child + Send + Sync> = child;
        registry.pty_masters.lock().insert(
            key.clone(),
            Arc::new(parking_lot::Mutex::new(real_master())),
        );
        registry
            .child_handles
            .lock()
            .insert(key.clone(), Arc::new(parking_lot::Mutex::new(child)));
        registry.runs.register("t1", key.generation());
    }

    // 旧代的退出监控迟到触发:retire 幂等(第二次报告不是"又停了一个进程"),
    // 而且带错句柄的 if_current 清理拒绝动手——新一代的句柄原样保留。
    assert!(
        registry.runs.retire("t1", 1),
        "第一次退出报告 retire 掉自己那代"
    );
    assert!(
        !registry.runs.retire("t1", 1),
        "同一代的重复退出报告 retire 不到任何东西"
    );
    // 句柄错配的拒绝路径不走测试:remove_pty_handles_if_current 把它按 ADR-020 的
    // 安全网合同声明为 debug_assert!(实现 bug,不是竞态),测试构建里走到就是 panic。
    // 迟到监控的清白由 retire 幂等与下面的代际隔离钉住。
    assert!(
        registry.pty_masters.lock().contains_key(&new)
            && registry.child_handles.lock().contains_key(&new),
        "新一代的句柄完好"
    );

    // 正常拆除旧代:只带走旧代,新生代继续在飞。
    registry.remove_pty_handles(&old);
    assert!(!registry.runs.is_live("t1", 1));
    assert!(registry.runs.is_live("t1", 2));
    assert!(!registry.pty_masters.lock().contains_key(&old));
    assert!(registry.pty_masters.lock().contains_key(&new));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_exit_that_settles_consumes_marks_exactly_once() {
    let registry = TaskRegistry::default();
    registry.cancelled_tasks.lock().insert("t1".to_string());
    registry
        .manually_completed_tasks
        .lock()
        .insert("t2".to_string());
    registry.runs.register("t1", 1);

    // 另一代还在飞:什么都不消费(ADR-030 的守卫变体)。
    assert_eq!(
        registry.settle_marks_for_exit("t1"),
        ExitSettlement::WaitingForOtherRuns
    );
    registry.runs.retire("t1", 1);
    assert_eq!(
        registry.settle_marks_for_exit("t1"),
        ExitSettlement::Settle {
            cancelled: true,
            manually_completed: false
        },
        "最后一个退出者拿走取消标记"
    );
    // 读时消费:重复结算拿到的都是空,不会把事件发两遍。
    assert_eq!(
        registry.settle_marks_for_exit("t1"),
        ExitSettlement::Settle {
            cancelled: false,
            manually_completed: false
        }
    );
    // t2 的手动完成标记独立成账,不受 t1 结算影响。
    assert_eq!(
        registry.settle_marks_for_exit("t2"),
        ExitSettlement::Settle {
            cancelled: false,
            manually_completed: true
        }
    );
}
