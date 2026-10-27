# Agent Note: 退出序按 pid/pgid 发信号，杀链不经过子进程句柄

Status: implemented

## Problem

退出时杀进程曾需要取 `child_handles` 的锁，而观察线程在 `portable_pty::Child::wait()` 期间持有该句柄，于是宿主退出会卡在杀链上，连带拖住租约释放与 socket 清理。另外 POSIX `kill(pid)` 的 pid 有三个危险特例：`-1` 向本用户所有进程发信号（注销图形会话）、`0` 打到调用者进程组（连带宿主自己）、`< -1` 打到进程组 `|pid|`——这类事故实际发生过一次，只靠调用方纪律挡不住。

## Decision

- 进程地址在 spawn 时捕获一次，之后只读。pty 子进程在 `pre_exec` 里 `setsid()`，所以 `pgid == pid`，不需要 `tcgetpgrp`。注册表新增叶子表 `kill_targets: Mutex<HashMap<RunKey, KillTarget>>`（`core/crates/aeroric-core/src/runtime/kill.rs`）。
- `KillTarget` 是唯一的信号路径，只发信号不等待：`killpg(pgid, SIGHUP)` → `killpg(pgid, SIGKILL)` → `kill(pid, SIGKILL)`。全程不取别的锁、不做有界等待——杀不掉是 OS 的事，卡住是我们的 bug。
- `kill_targets` 是叶子锁，不进 `pty_masters → pty_writers → child_handles → pending_pty_sizes` 那条锁链；退出序只许单独取它。
- 广播形状在构造期就被拒：只有 `> 1` 的 pid/pgid 是有效目标，每个系统调用前再断言一次。

## Alternatives considered

- **观察线程不跨 `wait()` 持锁**：消除锁竞争的直观做法。否掉是因为 `Child::wait()` 取 `&mut self`，同时承担收尸与退出码语义，换成裸 `waitpid` 会丢掉这两点。
- **`try_lock` 失败就当杀过**：不再卡住。否掉是因为它把"卡住"换成"漏杀"。
- **保留 `tcgetpgrp` 优先**：拿到前台组更准确。否掉是因为它答不出刚 spawn 的运行的前台组，而 setsid 组长号在那一刻一定有值。

## Consequences

- **收益**：退出序不会被观察线程卡住；向 `-1`/`0` 发信号在类型层面不可能。
- **代价与已知上限**：依赖 `setsid` 使 `pgid == pid` 这一前提；若某类子进程不再 setsid，必须重访。
