# Agent Note: 宿主退出时无限期等阻塞任务，占着数据目录锁，新宿主起不来

Status: implemented

## Problem

2026-11-18 12:44 重启审阅版时，旧宿主（`aeroric-cli serve`）收到退出信号后一直没退。

- 它的父进程 App 已经退了，它不再监听 `host.sock`，却占着数据目录的锁；
- 新 App 拉起的宿主每次都报"data directory … is already held by another process"，界面连不上宿主；
- 再发 SIGTERM 也没用：信号处理器已经接过一次信号，它正在退出流程里。

用 `sample` 看到主线程停在 `tokio::runtime::blocking::pool::BlockingPool::shutdown`：

- `aeroric-cli` 用 `#[tokio::main]`，`main` 返回时丢掉运行时；
- 丢运行时要等所有还在跑的阻塞任务结束，没有时限；
- 当时有三个阻塞任务卡在文件系统调用里：
  - 记忆版本链回填（`memory::backfill_chain`，`read`）；
  - 内置技能扫描项目（`builtin_skills_services::projects_with_copies`，`opendir`）；
  - 参考库列表（`references::list`，`poll`）。
- 三个任务都在读"同步空间"下的目录，当时 Mac 上正跑完整 `swift test` 和构建，读得很慢；
- 约两分钟后它们做完，旧宿主才退出，新宿主才起来。

## Decision

`aeroric-cli` 不再用 `#[tokio::main]`：

- `main` 自己建多线程运行时（与 `#[tokio::main]` 的配置相同：`new_multi_thread().enable_all()`）；
- `run_then_stop` 跑完入口后调用 `shutdown_timeout(STOP_GRACE)`，最多等 3 秒；
- 3 秒后还没结束的阻塞任务交给进程退出处理，锁随进程退出释放。

所有子命令共用这一个入口，`serve` 以外的命令本来就很少留下阻塞任务，行为不变。

## Alternatives considered

**让这三个扫描可以中途取消。** 最强的理由是退出时不丢半截工作。没选：卡住的是 `opendir`、`read` 这类系统调用本身，传取消标志也要等调用返回才看得到；而且以后新加的阻塞任务还会有同样的问题。统一给退出设时限一劳永逸。

**收到第二次 SIGTERM 时直接 `exit`。** 最强的理由是人能强行结束它。没选：App 重启时只发一次退出，靠人再发信号不现实；时限在没人管的时候也有效。

**时限设得更长（比如 30 秒）。** 最强的理由是给正在写盘的任务多些时间。没选：宿主的落盘都走临时文件加改名（`storage::atomic::write_atomic`），中途被打断只留下临时文件、下次启动清掉；App 重启期间界面一直连不上，等得越久越像坏了。3 秒够正常的阻塞任务收尾。

## Consequences

- **收益**：宿主退出最多多等 3 秒，App 重启后新宿主能马上拿到数据目录锁。
- **代价**：退出时还在跑的阻塞任务被放弃；读类任务无影响，写类任务靠原子写兜底，只可能少一次写。
- **验证**：
  - `aeroric-cli` 新增单测 `a_blocking_task_that_never_ends_does_not_hold_the_exit`：阻塞任务睡 120 秒，入口在 3 到 8 秒内返回；
  - `cargo test -p aeroric-cli` 全部通过，其中集成测试会真的拉起 `serve` 再让它退出；
  - 审阅版重建后，退出再启动一次，新宿主正常起来。
