//! 进程组信号执行器(不等待、不持 `child_handles`)。
//!
//! ## 为什么需要它
//!
//! 退出序要杀在飞任务时,任何"拿 `child_handles` 的锁再 `kill()`"的写法都会**死锁**:
//! 退出观察线程持着那把锁走完 `child.lock().wait()`(见
//! `aeroric-host/src/executor.rs` 的 `ExitCtx` 线程),而它要等到子进程退出才松手。
//! 于是"显式杀链"只在子进程**恰好**已经退出时才有效——真正的活进程反而杀不掉。
//!
//! 复现(2026-10-13,本机):`aeroric-cli serve` 起一个 `trap '' HUP` 的假 agent,
//! SIGTERM 后卡在 `kill_run` → `child.lock()`,2/12 次挂死 30s 以上;12/12 次
//! `ps` 看到子进程在 serve 退出后仍然存活。
//!
//! ## 本模块的唯一职责
//!
//! **只发信号,不问结果**。进程组号在 spawn 时捕获(pty 子进程在 `pre_exec` 里
//! `setsid()`,所以 pgid == pid),因此不需要 `tcgetpgrp`,也就永远不碰 master 或
//! child 的锁。
//!
//! ## 安全边界(POSIX `kill` 的广播语义)
//!
//! `kill(pid)` 的 pid 取值有三个**危险特例**,必须在本模块内部一次性挡掉,绝不允许
//! 调用方"传对了就行":
//!
//! | pid | POSIX 语义 | 后果 |
//! |---|---|---|
//! | `-1` | 向**本用户可发的所有进程**发信号 | 注销当前图形会话(黑屏、所有应用退出) |
//! | `0` | 向**调用者所在进程组**全部进程发信号 | 连带杀掉宿主自己 |
//! | `< -1` | 向进程组 `\|pid\|` 发信号 | 打错组 |
//!
//! `killpg` 同样:`killpg(0, …)` 打调用者自己的组,`killpg(-1, …)` 是全局广播。
//! 因此本类型把 pid/pgid 存成 `Option<i32>`,**只有 `> 1` 才是 `Some`**,任何退化
//! 输入只会得到"什么都不发"的配置(`pid() == 0`、`pgid() == None`),而不是广播。
//! 信号路径上再做一次 `> 1` 断言,双保险。
//!
//! 这条边界不是理论洁癖:2026-10-13 本机曾在 `KillTarget::with_group(-1, None)` 上
//! 触发 `kill(-1, SIGKILL)`,当场注销了用户的 macOS 登录会话。

/// 一个 run 的信号地址。构造后只读,可跨线程共享。
///
/// 不变量:`pid` 与 `pgid` 要么是 `> 1` 的有效目标,要么分别是 `0` / `None`
/// (= 不可发送)。构造器是唯一入口,`kill_now` 只读这两个字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KillTarget {
    /// `0` = 没有可发送的 pid(平台没给 / 构造时被拒),此时整份 `kill_now` 是空操作。
    pid: i32,
    /// `None` = 没有可发送的组(未 setsid,或组号退化为 0/-1 被拒)。
    pgid: Option<i32>,
}

/// POSIX 广播语义的分界:只有 `> 1` 的 pid/pgid 才是"指名道姓杀一个进程/组"。
///
/// - `1` 是 init/launchd,任何用户态代码都不该向它发信号;
/// - `0` 与负数在 POSIX 里是"广播",正是要挡掉的形状。
const fn addressable(id: i32) -> bool {
    id > 1
}

impl KillTarget {
    /// 不可发送的地址:整份 `kill_now` 是空操作。用于拿不到 pid 的平台/时刻。
    pub fn disabled() -> Self {
        Self {
            pid: 0,
            pgid: None,
        }
    }

    /// 由子进程 pid 构造。pty 子进程在 `spawn_command` 的 `pre_exec` 里
    /// `setsid()`,所以它就是新会话的组长:pgid == pid。
    ///
    /// `pid <= 1` 视为无效(见 [`addressable`]),退化成 [`Self::disabled`]——
    /// 宁可杀不掉,也绝不广播。
    pub fn from_pid(pid: i32) -> Self {
        if !addressable(pid) {
            return Self::disabled();
        }
        Self {
            pid,
            pgid: Some(pid),
        }
    }

    /// 显式给出组号(`pgid` 也走同一道 `> 1` 门)。
    pub fn with_group(pid: i32, pgid: Option<i32>) -> Self {
        Self {
            pid: if addressable(pid) { pid } else { 0 },
            pgid: pgid.filter(|id| addressable(*id)),
        }
    }

    /// 实际会被发送信号的 pid;`0` 表示无。
    pub fn pid(&self) -> i32 {
        self.pid
    }

    /// 实际会被发送信号的组号;`None` 表示无。
    pub fn pgid(&self) -> Option<i32> {
        self.pgid
    }

    /// 是否有任何有效目标。`false` 时 [`Self::kill_now`] 保证一个信号都不发。
    pub fn is_addressable(&self) -> bool {
        addressable(self.pid) || self.pgid.is_some()
    }

    /// 立即杀掉。非阻塞、幂等:进程已消失时内核返回 ESRCH,无副作用。
    ///
    /// 顺序:先 SIGHUP(让守规矩的壳自己清理),立刻 SIGKILL。
    /// 不给"优雅退出"留等待窗口——退出序没有时间预算去做这件事。
    pub fn kill_now(&self) {
        #[cfg(unix)]
        unsafe {
            // 双保险:构造器已经挡过一遍,这里再挡一遍。签名路径上出现 0/-1 是
            // 不可接受的失败模式(广播),所以宁可漏杀也不放行。
            if let Some(pgid) = self.pgid.filter(|id| addressable(*id)) {
                libc::killpg(pgid, libc::SIGHUP);
                libc::killpg(pgid, libc::SIGKILL);
            }
            // 壳本身:组号无效(或已回收)时的兜底,也是唯一在 pgid 缺失时生效的路径。
            if addressable(self.pid) {
                libc::kill(self.pid, libc::SIGKILL);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pid_becomes_its_own_group_after_setsid() {
        let target = KillTarget::from_pid(4242);
        assert_eq!(target.pid(), 4242);
        assert_eq!(target.pgid(), Some(4242));
        assert!(target.is_addressable());
    }

    /// 反广播防线:POSIX 里 `kill(-1, …)` 是"本用户所有进程",`kill(0, …)` 是
    /// "调用者所在进程组"。两者都曾在真实事故里注销过登录会话,必须在构造期
    /// 变成空操作,而不是靠调用方小心。
    #[test]
    fn broadcast_shaped_pids_are_refused_at_construction() {
        for hostile in [-1, 0, 1, -2, i32::MIN] {
            let target = KillTarget::from_pid(hostile);
            assert_eq!(target.pid(), 0, "pid {hostile} 必须以 0(不可发送)落地");
            assert_eq!(target.pgid(), None, "pid {hostile} 不得带出组号");
            assert!(!target.is_addressable());
        }
    }

    /// `with_group` 的 pid 与 pgid 走同一道门:任一侧退化都不得变成广播。
    #[test]
    fn group_and_pid_share_the_same_guard() {
        let no_group = KillTarget::with_group(4242, Some(0));
        assert_eq!(no_group.pgid(), None, "pgid 0 是调用者自己的组,必须拒绝");
        assert_eq!(no_group.pid(), 4242, "pid 仍然有效");

        let no_pid = KillTarget::with_group(-1, Some(4242));
        assert_eq!(no_pid.pid(), 0, "pid -1 是全局广播,必须拒绝");
        assert_eq!(no_pid.pgid(), Some(4242));

        let nothing = KillTarget::with_group(-1, Some(-1));
        assert!(!nothing.is_addressable());
    }

    #[test]
    fn a_disabled_target_signals_nothing() {
        KillTarget::disabled().kill_now();
        // 已回收/不存在的 pid:ESRCH,不得 panic、不得阻塞。
        KillTarget::with_group(999_999, None).kill_now();
    }

    #[test]
    fn kill_now_reaps_the_group_of_a_real_session_leader() {
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};
        // 真起一个忽略 SIGHUP 的壳,里面再挂一个睡眠后代:这正是退出序要杀的形状。
        // 必须让子进程自己 `setsid()`,否则它继承测试进程的组,`killpg(pid)` 就指向
        // 一个不属于我们的组——测试要么毫无意义,要么打到别处。
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c")
            .arg("trap '' HUP; sleep 30 & wait")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = cmd.spawn().expect("spawn shell");
        let pid = child.id() as i32;
        assert!(pid > 1, "测试子进程必须拿到真实 pid");

        // 组号 == pid:壳在 pre_exec 里 setsid 了,它是自己会话的组长。
        KillTarget::from_pid(pid).kill_now();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Ok(Some(_)) = child.try_wait() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "SIGKILL 之后进程仍不退出:信号没到"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
