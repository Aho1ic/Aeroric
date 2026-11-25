# Agent Note: App 没正常退出时，宿主跟着退出

Status: implemented

## Problem

1124b 和 1124c 两次重启 App，旧宿主都留了下来：
- App 正常退出时，`AppDelegate.applicationWillTerminate` 调 `HostSupervisor.terminateChildForProcessExit` 停掉宿主。
- 两次都是用 `osascript` 发退出没有反应，只好用 SIGTERM 停 App。SIGTERM 不经过 `applicationWillTerminate`，宿主就被留下，父进程变成 launchd。
- 留下的宿主一直拿着数据目录的锁。新 App 起来以后连上的是这个旧宿主，跑的是旧构建的代码：1124b 那次，新 App 用的 Claude 用量统计还是旧的。
- 留下来的宿主还带着它的 Agent 子进程：1124c 那次是一个卡在“运行中”的 Claude。

App 崩溃、被强制退出，也是同样的结果。

`osascript` 退出为什么没有反应，没有查出来：App 里没有拦截退出的代码。当时 App 正在跑目检，不能再退一次来复现。

## Decision

**App 拉起的宿主看着父进程，父进程没了就自己退出**（`aeroric-cli` 的 `serve`、`wait_for_parent_exit`）。
- **只看 App 拉起的宿主：** 带 `--ready-fd` 的才是 App 拉起的。手动跑的 `serve` 不看父进程。
- **父进程什么时候记：** 在 `serve` 一开始就记。最初是在监听起来以后才记，App 如果在宿主启动途中就没了，记下的父进程已经是 launchd，以后再也看不出变化。进程测试第一次就是因此超时的。
- **怎么发现父进程没了：** 每 2 秒看一次父进程号，变了就说明 App 没了。
- **留不留：** 父进程没了的那一刻，读 `background-policy.json` 的 `keepAliveOnUIQuit`。
  - 用户开了“退出 App 后宿主留在后台”，就留着，之后不看父进程；
  - 否则走和 SIGTERM 一样的收尾：停服务、杀掉在跑的 Agent 进程，然后退出。
  - 这个文件读不出来或没有，按关处理，和 App 的默认值一致。

## Alternatives considered

**用 kqueue 的 `NOTE_EXIT` 等父进程退出。** 没选：要加平台相关代码。2 秒轮询一次 `getppid` 的开销可以忽略，晚 2 秒退出也没有影响。

**App 启动时把 `keepAliveOnUIQuit` 当参数传给宿主。** 没选：这个开关运行中可以切换，宿主不会重启，传进去的值会过时。在父进程没了的那一刻读文件，读到的总是最新的。

**App 启动时先杀掉占着锁的旧宿主。** 没选：锁模块有意只报告持有者、从不按 pid 杀进程（pid 可能已被复用）。开了后台保留时，旧宿主也本来就该被连上。

## Consequences

- **收益：** App 怎么没的都不会留下旧宿主。下次启动的 App 起的是自己这一版的宿主。
- **局限：**
  - App 没了以后，宿主最多晚约 2 秒退出；
  - 宿主在 `serve` 开始之前（进程刚起来时）父进程就没了，这种情况看不出来；
  - `osascript` 发退出没反应的原因还没查。
- **验证：**
  - 单元测试：`the_host_stays_after_the_app_only_when_asked`、`a_host_without_a_parent_to_watch_waits_for_a_signal`；
  - 进程测试：`a_host_the_app_started_leaves_when_the_app_dies`。用一个 shell 当 App，SIGKILL 掉它：没开后台保留时宿主自己退出；开了时 5 秒后仍在，SIGTERM 能停。
