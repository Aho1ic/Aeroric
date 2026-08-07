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

#[cfg(not(windows))]
pub(crate) fn configure_background_tokio_command(_cmd: &mut tokio::process::Command) {}

#[cfg(unix)]
pub(crate) fn configure_terminable_process_tree(cmd: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;

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
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        child.kill()
    }
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn terminate_process_tree(child: &mut std::process::Child) -> std::io::Result<()> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }
    child.kill()
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
}
