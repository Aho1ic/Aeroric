# Agent Note: D4 第六步：按任务的运行次数判旧，判旧后手机自动重新同步

Status: implemented

## Problem

D4 计划的第 2 条：手机发出的命令要带上它所依据的状态，宿主发现依据已经过期就拒绝。前几步只做了 `hostEpoch` 一层，宿主重启过的命令会被拒，但同一次启动里任务本身变了还看不出来。例如：
- 手机上显示一张审批卡，用户点“批准”之前，任务在 Mac 上被停掉又重新运行了；
- 手机这条 `task.respond` 依据的是上一次运行，宿主照样收下。

任务记录里已经有现成的版本号：`generation`。它是这个任务的第几次运行，每次启动或恢复加一。宿主读任务时，`TaskService::stamp_generation_copies` 会把它写进每条记录，手机读快照时就能拿到。

还有一处缺口：手机把 `resyncNeeded` 置上以后，连接循环并不会去重新读。宿主拒绝、事件跳号、缓冲溢出都只会让横幅写“正在和 Mac 重新同步”，实际要等下次断线重连才真的同步。

## Decision

**命令带上任务的运行次数。** `task.respond` 除了 `taskId`，还带：
- `projectId`：宿主要按项目读任务；
- `basedOnGeneration`：手机快照里这个任务的 `generation`，快照里没有就不带。

**宿主核对。** 新增 `commands::check_task_generation`：
- **什么时候查：** 命令带了 `basedOnGeneration`、`taskId`、`projectId` 才查。查的是任务当前的 `generation`。
- **拒绝：** 对不上时拒绝，原因以 `STALE_TASK_GENERATION` 开头，写明当前第几次、命令依据第几次。任务已经不在了，也按这个前缀拒绝。
- **放行：** 记录里没有 `generation` 的任务（旧数据）照常放行。
- **核对在哪一步：** 在 `dispatch_once` 里，放在“同一个 `commandId` 只执行一次”之内。第一次执行才核对，结果记下来；之后用同一个编号重发，拿到的还是第一次的结果，不会因为任务后来又跑了一次而改口。

**手机识别并重新同步：**
- `CompanionTask` 加 `generation`，从快照读；
- `applyCommandReply` 把 `STALE_TASK_GENERATION` 和 `STALE_HOST_EPOCH` 一样看待：置 `resyncNeeded`；
- 连接循环每处理完一帧，看到 `resyncNeeded` 而自己没在同步，就重读一次：`events.since` 加项目、任务快照，交给 `handleReconnect`。快照是全量的，读完 `resyncNeeded` 就清掉。重读期间到的推送照第四步的办法先攒着。同一时间只跑一次重读；重读失败就留着 `resyncNeeded`，等下一帧或下次重连再试。

## Alternatives considered

**用任务文件的修订号（`StampedTasks.revision`）判旧。** 没选：它是整个项目任务列表的修订号，同项目里任何一个任务变了都会变。拿它判，几乎每条命令都会被拒。`generation` 只跟这个任务的运行有关，正好对应“这张审批卡还是不是这次运行的”。

**把判旧放在 `admit` 里，和 `hostEpoch` 一起查。** 没选：`admit` 是同步的，读任务要走宿主的异步接口。而且放在 `admit` 里，会在“只执行一次”之前查，重发时可能得到和第一次不同的结论。

**拒绝时把当前任务记录一起带回。** 没选：现在的 v3 错误只有 `code` 和 `message`，带对象要改协议。手机反正要整份重读快照，拒绝原因里写明当前第几次就够了。

## Consequences

- **收益：**
  - 任务在 Mac 上重新跑过以后，手机按旧卡片点的批准会被拒，不会答到新一次运行上；
  - 手机端判出状态过期以后会马上重新同步，不用等断线。
- **局限：**
  - 只有 `task.respond` 带 `basedOnGeneration`。别的会改东西的命令手机端还没发（会话控制、写文件），等它们接上时再带；
  - 拒绝里只有当前次数，不带记录；
  - 真机、真实 Mac 上 NOT RUN。
- **验证：**
  - core 新增 1 个测试：依据对得上就执行；对不上就拒绝，且 Mac 那边一次都没执行；同一个编号在任务又跑过一次之后重发，拿到第一次的结果；任务不在了也拒绝。
  - iOS 新增 2 个测试：
    - 审批命令带上 `projectId` 和 `basedOnGeneration`；被拒为 `STALE_TASK_GENERATION` 以后，连接循环在同一条连接上自动重读，卡片换成新的，横幅清掉；
    - 推送跳号也当场重读。
  - 原有的跳号测试改为断言“重读一次”：原来断言 `resyncNeeded` 一直为真，现在它会被这次重读清掉。
  - MacBook 上：
    - core 的 remote 测试 124 个都过；
    - iOS 包 54 个测试都过，两套连接测试连跑 5 次都过；
    - iOS App 在模拟器上编过。
