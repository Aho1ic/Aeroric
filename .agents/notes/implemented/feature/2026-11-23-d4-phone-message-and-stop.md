# Agent Note: D4 后续：手机给任务发消息、停止这一轮

Status: implemented

## Problem

backlog §4.4-1 定了手机能发的几种意图：输入、批准或拒绝、中断、打开文件。手机任务页能看会话、改动和待审批，也能答审批，但不能给任务说话，也不能让它停下。

动手前（`main@71ae67cf`）核对的事实：
- **`task.input` 只对终端任务管用：** 宿主（`CliRemoteHandle::task_input`）把文字包成粘贴序列写进 PTY。Aeroric 现在主要用的是 Codex、Claude、DSH、OMP 这类结构化会话，没有 PTY，这条消息送不到。
- **桌面端怎么发、怎么停：**
  - 发消息走 `session.prompt`：消息进投递队列（R1-2，有回执，回合中发的排到下一轮），暂停所有 Agent 时拒收（E6），处理 `@` 引用（E12）；
  - “停止”走 `session.interrupt`：先暂停还没发出的消息（停止屏障），再让驱动中断回合。
- **远程宿主调不到这两个服务：** 它手里只有 `StructuredExecutor`，不能直接调用。
- **没有停一轮的远程方法：** `task.cancel` 走 `TaskService::cancel`，它的语义是取消任务，不是停这一轮。
- **能力清单不对：** `hello` 回给手机的能力清单写着 `files.write`，但远程写文件上一步已默认关闭。

## Decision

**宿主：结构化会话的消息和停止，和桌面端走同一条路。**
- **进程内调用：** `CliRemoteHandle` 多一个 `host_services`（宿主自己的 `ServiceSet`），由 `aeroric-cli serve` 传入。`call_in_process` 以一个已认证的客户端身份在进程内调用其中的方法。手机的请求已经过远程面的认证，这里不再认证一次。
- **判断是不是结构化会话：** 抽成公开函数 `session_live_services::runs_structured`，和 `session_live_services` 判断同一件事用的是同一个函数。驱动开着，或者任务的运行会交给结构化驱动，都算。
- **`task.input`：**
  - 结构化会话：调 `session.prompt`，回 `{sent, accepted, deliveryId}`；空消息拒绝；
  - 终端任务：照旧写 PTY。
- **新方法 `task.interrupt {projectId, taskId}`：**
  - 结构化会话：调 `session.interrupt`，回 `stopped: "turn"`；
  - 终端任务：发 Ctrl-C，回 `stopped: "terminal"`；
  - 任务本身不取消。
  - 它会改 Mac 上的东西，所以进了命令表：带 `commandId` 时只执行一次。
  - 实现方法有默认拒绝，别的宿主不实现就如实回答“不能中断”。
- **能力清单：**
  - 加 `tasks.interrupt`；
  - `hello` 和认证回答里的清单改由 `rpc_capabilities(state)` 给出，远程写文件没打开时不列 `files.write`。

**手机：任务页加“发给这个任务”一节。**
- **输入：** 多行输入框、“发送”、“停止这一轮”。
- **命令：** `sendMessage` 发 `task.input`，`stopTask` 发 `task.interrupt`，都是命令（带 `commandId`、`hostEpoch`，进 outbox，重连到同一次启动原样重发）。
- **结果按任务记在 `taskActions`，写在按钮下面：**
  - 正在发 / 正在停；
  - Mac 收下了这条消息 / Mac 已停止这一轮；
  - 没送出、没能停止，写 Mac 给的原因；
  - Mac 重启后没发出的，写“看过最新状态再发”。
  - “收下”只说明消息进了队列，Agent 什么时候收到，看会话。
- **拒绝的情况：** 空消息、不认识的任务、Mac 不在线，都在发出前拒绝。
- **退出时：** 取消配对或设备被撤销时清掉。

**顺带修的测试替身问题：** 假 Mac（`FakeMac`）在锁里加密回答、在锁外投递。手机同时发两个读取时（任务页的会话和改动），帧可能不按序号顺序送到，手机按“解不开的帧”断开，任务页那条会话测试因此偶发失败（12 次里约 1 次）。现在加密和入队在同一个锁里完成，和 `server.rs` 按顺序写出一致。Mac 这一侧的产品代码没有这个问题；手机发往 Mac 的方向有同样的乱序，见 [1126a](../bug-fix/2026-11-26-review-1126a-findings.md)。

## Alternatives considered

**`task.cancel` 对结构化会话改成中断。** 没选：取消任务和停这一轮是两件事。改了以后，用 `task.cancel` 的地方（包括旧客户端）意思就变了。

**远程宿主自己往驱动里 `deliver`。** 没选：这会绕过投递队列、停止屏障、暂停所有 Agent、引用检查。手机发的消息应该和桌面端发的一模一样。

**手机等 Agent 真收到再写“已送达”。** 这次没做：投递回执在会话里已经能看到。按钮下面只说 Mac 收没收下，不冒充 Agent 收到了。

## Consequences

- **收益：** 手机能给正在跑的 Codex、Claude 等会话补一句话，能停下跑偏的一轮，和在 Mac 上操作效果一样。
- **局限：**
  - 发消息不能带 `@` 引用、附件，也不能选“排队”或“插话”，都按默认的“现在发”；
  - 终端任务的停止只是一个 Ctrl-C；
  - 真机、真实 Mac 上 NOT RUN，界面没有目检。
- **验证：**
  - core 新增 1 个测试：`task.interrupt` 是命令，缺参数报错，宿主不实现时如实拒绝。远程写文件那条测试加了能力清单的断言：没打开不列 `files.write`，有 `tasks.interrupt`。
  - 宿主新增 1 个测试：进程内调用把参数交给方法，成功回结果，拒绝回原因，没注册的方法报错。
  - 手机新增 2 个测试：
    - store：不在线、空消息、不认识的任务都在发出前拒绝；消息和停止的命令和参数；收下、拒绝、Mac 重启各自的写法；取消配对后清掉；
    - 经假 Mac：消息作为带 `commandId` 的 `task.input` 发出并收下；停止被拒时写出 Mac 的原因。
  - MacBook 上：
    - core 的 remote 测试 127 个、`remote_headless` 集成测试 10 个、宿主 `remote_headless` 和 `session_live` 测试 28 个、宿主的两套远程集成测试都过；
    - `aeroric-cli` 编过，`cargo fmt --check` 过；
    - iOS 包 71 个测试都过，连接相关五套连跑 12 次都过；
    - iOS App 在模拟器上编过。
