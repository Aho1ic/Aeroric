# Agent Note: 收到终止信号时 App 照正常退出走

Status: implemented

## Problem

1124b 以后连续四次重启 App，都用 ssh 在 MacBook 上跑 `osascript -e 'tell application id "com.aeroric.native.dev" to quit'`。命令返回 0，App 却不退出，只好改用 `kill -TERM`。笔记 `2026-11-24-host-follows-app-out` 把这件事记为“原因没查”。

**查到的原因：退出命令根本没送到 App。** 退出本身没有问题。
- 同一个 ssh 会话里，AppleScript 问 `running` 时，Aeroric、访达、程序坞都回答 false，`lsappinfo list` 什么也列不出。ssh 登录和桌面不是同一个会话，看不到桌面上的 App。
- 对一个“没在运行”的 App 发 `quit`，AppleScript 什么也不做，照样返回成功。
- App 本身能正常退出：1124b 目检里，从菜单“退出”能正常退出；代码里也没有任何拦截退出的地方。

**真正的缺口在退而求其次的 `kill -TERM`：**
- Cocoa App 收到 SIGTERM 默认当场结束，`applicationWillTerminate` 不跑：
  - 宿主不是被 App 停掉的，而是靠 `2026-11-24-host-follows-app-out` 的父进程检查在 2 秒后自己退出；
  - 文件夹访问权限（`bookmarkStore.stopAllAccess`）没有释放。
- 退出登录时，launchd 发给 App 的也是 SIGTERM，同样会这样。

## Decision

**App 把 SIGTERM 当成正常退出**（`AeroricClient.TerminationSignals`，在 `AppDelegate.applicationDidFinishLaunching` 里装）。
- **怎么接住信号：** 把 SIGTERM 设成忽略，再用 `DispatchSource` 在主线程接住，调 `NSApp.terminate(nil)`。这样 `applicationWillTerminate` 照常运行：按设置停掉或留下宿主，释放文件夹访问。
- **退出卡住时：** 正常退出没走完时再收到一次 SIGTERM，就先停宿主再 `exit(0)`。
- **对宿主的影响：** 宿主由 `HostSupervisor` 用 `POSIX_SPAWN_SETSIGDEF` 拉起，SIGTERM 恢复成默认，不会继承 App 设的“忽略”。

**以后远程重启 App，直接 `kill -TERM <App 的 pid>`。** 别用 `osascript`，它从 ssh 发不到桌面上的 App。

## Alternatives considered

**用 `launchctl asuser` 在桌面会话里跑 `osascript`。** 没选：要 root。而且 SIGTERM 是系统本来就会发给 App 的信号，App 自己处理好它，退出登录时也一样受益。

**只靠宿主的父进程检查。** 没选：宿主能自己退，但 App 的其他收尾（文件夹访问、留在后台的设置）都没走。

## Consequences

- **收益：**
  - `kill -TERM` 和菜单“退出”走同一条路；
  - 退出登录时 App 也能正常收尾；
  - 远程重启 App 有了可靠的办法。
- **局限：**
  - App 自己起的子进程如果不用 `HostSupervisor` 的方式拉起，可能继承“忽略 SIGTERM”。宿主是唯一长期运行的子进程，已核对；
  - 第二次 SIGTERM 的强制退出路径没有自动测试。
- **验证：**
  - `TerminationSignalsTests`：用 SIGUSR2 代替 SIGTERM，避免误伤测试进程；信号送到处理函数，`stop()` 后恢复成默认；
  - 下次重启 App 时用 `kill -TERM` 核对：宿主随 App 立刻停掉，不是 2 秒后自己退出。
