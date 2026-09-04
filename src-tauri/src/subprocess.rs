#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(windows)]
pub(crate) fn configure_background_command(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;

    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub(crate) fn configure_background_command(_cmd: &mut std::process::Command) {}

#[cfg(windows)]
pub(crate) fn configure_background_tokio_command(cmd: &mut tokio::process::Command) {
    cmd.creation_flags(CREATE_NO_WINDOW);
}

/// 让子进程成为新进程组的组长,这样 `terminate_process_tree` 可以用 `kill(-pgid)`
/// 一次带走它 fork 出来的全部后代(shell -c 里的后台进程、npx 拉起的 node 等)。
#[cfg(unix)]
pub(crate) fn configure_terminable_process_tree(cmd: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;

    // SAFETY: pre_exec 的闭包运行在 fork 之后、exec 之前的子进程里。此时进程只有
    // 一个线程,但父进程持有的锁可能停在任意状态,所以闭包只允许调用
    // async-signal-safe 的函数、不得分配内存、不得取锁。
    // 这里只有 libc::setpgid 与 Error::last_os_error()(读 errno),两者都满足;
    // 返回的 Err 由标准库在 exec 前经 pipe 回传给父进程,不在闭包内做格式化。
    unsafe {
        cmd.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
}

/// `configure_terminable_process_tree` 的 tokio 版本。
///
/// tokio 的 `Command` 与 `std` 的不是同一个类型,之前只有 std 一侧有进程组设置,
/// 于是所有用 tokio 启动的常驻子进程(dsh web 等)都留在应用自己的进程组里,
/// 停止时只能杀到直接子进程,它的后代会变成孤儿继续跑。
///
/// Windows 上顺带保留 `CREATE_NO_WINDOW`:这些都是后台 sidecar,少了这个标志
/// 会弹出真实的控制台窗口。
#[cfg(unix)]
pub(crate) fn configure_terminable_tokio_process_tree(cmd: &mut tokio::process::Command) {
    // SAFETY: 与上面 std 版本同一份约束与同一份实现,只有 setpgid + 读 errno。
    // (`pre_exec` 由 tokio::process::Command 自己提供,不需要 std 的 CommandExt。)
    unsafe {
        cmd.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
}

#[cfg(windows)]
pub(crate) fn configure_terminable_tokio_process_tree(cmd: &mut tokio::process::Command) {
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn configure_terminable_tokio_process_tree(_cmd: &mut tokio::process::Command) {}

/// 向 `configure_terminable_tokio_process_tree` 建好的进程组发信号。
///
/// `pid` 是子进程 pid,`setpgid(0, 0)` 之后它同时就是 pgid,所以 `-pid` 指向整组。
/// 组不存在(setpgid 失败,或进程已退出)时退回只发给 `pid` 本身,保证至少不比
/// 改动前弱。返回是否有信号送达。
#[cfg(unix)]
pub(crate) fn signal_process_group(pid: u32, signal: i32) -> bool {
    // SAFETY: kill 只读取内核进程表,不碰本进程内存。目标是子进程自己的进程组
    // (它是组长),不会包含当前进程,所以不存在自杀风险。
    let group_result = unsafe { libc::kill(-(pid as i32), signal) };
    if group_result == 0 {
        return true;
    }
    unsafe { libc::kill(pid as i32, signal) == 0 }
}

#[cfg(windows)]
pub(crate) fn configure_terminable_process_tree(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn configure_terminable_process_tree(_cmd: &mut std::process::Command) {}

#[cfg(unix)]
pub(crate) fn terminate_process_tree(child: &mut std::process::Child) -> std::io::Result<()> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }
    let process_group = -(child.id() as i32);
    // SAFETY: 同 signal_process_group——kill 不触碰本进程内存,目标进程组由
    // configure_terminable_process_tree 建立,组长是子进程而非当前进程。
    let result = unsafe { libc::kill(process_group, libc::SIGKILL) };
    if result == 0 {
        Ok(())
    } else {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            child.kill()
        } else {
            Err(error)
        }
    }
}

#[cfg(windows)]
pub(crate) fn terminate_process_tree(child: &mut std::process::Child) -> std::io::Result<()> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }
    let mut taskkill = std::process::Command::new("taskkill.exe");
    configure_background_command(&mut taskkill);
    let status = taskkill
        .args(taskkill_process_tree_args(child.id()))
        .status()?;
    if status.success() {
        Ok(())
    } else {
        child.kill()
    }
}

#[cfg(any(windows, test))]
fn taskkill_process_tree_args(pid: u32) -> [String; 4] {
    [
        "/PID".to_string(),
        pid.to_string(),
        "/T".to_string(),
        "/F".to_string(),
    ]
}

/// Terminate a Tokio child and all of its descendants.
///
/// The child must have been started with
/// [`configure_terminable_tokio_process_tree`]. On Unix that puts the child
/// in its own process group so one `SIGKILL` reaches descendants; on Windows
/// `taskkill /T` provides the equivalent tree operation. The final `wait`
/// always reaps the direct child instead of leaving a zombie behind.
#[cfg(unix)]
pub(crate) async fn terminate_tokio_process_tree(
    child: &mut tokio::process::Child,
) -> std::io::Result<()> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }

    if let Some(pid) = child.id() {
        // The process was configured with setpgid(0, 0), so its pid is also
        // the process-group id. `signal_process_group` falls back to the
        // direct pid when the group has already disappeared.
        let _ = signal_process_group(pid, libc::SIGKILL);
    } else {
        // A child without a pid is unusual (normally it means it already
        // exited), but preserve the old best-effort behaviour.
        let _ = child.kill().await;
    }

    child.wait().await.map(|_| ())
}

/// Windows uses `taskkill /T` because `Child::kill` only targets the direct
/// process. This is the equivalent of the Unix process-group path above.
#[cfg(windows)]
pub(crate) async fn terminate_tokio_process_tree(
    child: &mut tokio::process::Child,
) -> std::io::Result<()> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }
    let pid = child.id().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "child has no process id")
    })?;
    let mut taskkill = tokio::process::Command::new("taskkill.exe");
    configure_background_tokio_command(&mut taskkill);
    let taskkill_succeeded = match taskkill
        .args(taskkill_process_tree_args(pid))
        .status()
        .await
    {
        Ok(status) => status.success(),
        Err(_) => false,
    };
    if !taskkill_succeeded {
        child.kill().await?;
    }
    child.wait().await.map(|_| ())
}

#[cfg(not(any(unix, windows)))]
pub(crate) async fn terminate_tokio_process_tree(
    child: &mut tokio::process::Child,
) -> std::io::Result<()> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }
    child.kill().await?;
    child.wait().await.map(|_| ())
}

#[cfg(test)]
mod argument_tests {
    use super::taskkill_process_tree_args;

    #[test]
    fn taskkill_targets_the_complete_process_tree() {
        assert_eq!(
            taskkill_process_tree_args(42),
            ["/PID", "42", "/T", "/F"].map(str::to_string)
        );
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    fn process_exists(pid: i32) -> bool {
        let result = unsafe { libc::kill(pid, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }

    #[test]
    fn terminates_the_entire_spawned_process_group() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let pid_path = std::env::temp_dir().join(format!("aeroric-process-tree-{suffix}.pid"));
        let script = format!("sleep 30 & echo $! > '{}' && wait", pid_path.display());
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(script)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_terminable_process_tree(&mut command);
        let mut child = command.spawn().expect("spawn process tree");

        let deadline = Instant::now() + Duration::from_secs(2);
        let descendant_pid = loop {
            if let Ok(value) = fs::read_to_string(&pid_path) {
                if let Ok(pid) = value.trim().parse::<i32>() {
                    break pid;
                }
            }
            assert!(Instant::now() < deadline, "descendant pid was not written");
            thread::sleep(Duration::from_millis(20));
        };
        assert!(process_exists(descendant_pid));

        terminate_process_tree(&mut child).expect("terminate process tree");
        child.wait().expect("wait for process group leader");
        let deadline = Instant::now() + Duration::from_secs(2);
        while process_exists(descendant_pid) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }

        let _ = fs::remove_file(pid_path);
        assert!(!process_exists(descendant_pid));
    }

    #[tokio::test]
    async fn signals_the_entire_tokio_spawned_process_group() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let pid_path = std::env::temp_dir().join(format!("aeroric-tokio-tree-{suffix}.pid"));
        let script = format!("sleep 30 & echo $! > '{}' && wait", pid_path.display());
        let mut command = tokio::process::Command::new("sh");
        command
            .arg("-c")
            .arg(script)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_terminable_tokio_process_tree(&mut command);
        let mut child = command.spawn().expect("spawn tokio process tree");
        let leader = child.id().expect("tokio child pid");

        let deadline = Instant::now() + Duration::from_secs(2);
        let descendant_pid = loop {
            if let Ok(value) = fs::read_to_string(&pid_path) {
                if let Ok(pid) = value.trim().parse::<i32>() {
                    break pid;
                }
            }
            assert!(Instant::now() < deadline, "descendant pid was not written");
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        assert!(process_exists(descendant_pid));

        // 组长和后代在同一个进程组里,所以一次 kill(-pgid) 要同时带走两个;
        // helper 同时负责回收组长,避免留下 zombie。
        terminate_tokio_process_tree(&mut child)
            .await
            .expect("terminate tokio process tree");
        assert!(!process_exists(leader as i32));
        let deadline = Instant::now() + Duration::from_secs(2);
        while process_exists(descendant_pid) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        let _ = fs::remove_file(pid_path);
        assert!(!process_exists(descendant_pid));
    }
}
