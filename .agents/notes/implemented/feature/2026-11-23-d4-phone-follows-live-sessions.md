# Agent Note: D4 第八步：手机跟上实时会话

Status: implemented

## Problem

D4 前七步做完，手机能配对、连上、读快照、答审批。但用实际场景走一遍，有两处接不上。

**一、手机看不到实时会话的待审批。** 快照里的审批来自 `tasks.list` 的 `approval` 字段，这个字段由 core 的 `RemoteState.approvals` 填，只登记钩子审批（旧的终端式会话）。Aeroric 现在主要用的是结构化会话：Codex、Claude、DSH、OMP。它们的待审批在驱动里（`driver.snapshot().pending`），从来没进过快照。所以第三步做的审批回执（`answer_structured`）在主要场景里用不上：手机上根本没有那张卡。

**二、手机连着的时候，Mac 一条推送都不发。**
- 核心的 `RemoteState.clients.broadcast_push` 在生产代码里没有调用者；
- 宿主的 `RemoteHostHandle::emit` 是空函数，注释说“任务状态经 task-status 事件桥到达手机”。这个桥是 Tauri 时代的，原生宿主里没有；
- 宿主的任务事件（`task-status`）写进的是宿主自己的事件日志（`core::events::EventLog`），手机读的是 `RemoteState.event_log`，两者之间没有连线。

结果是：手机只在连上、或判出过期重读时才知道状态，连着的时候 Mac 上来了新审批，手机看不到。

## Decision

**快照带上实时会话的待审批。** `CliRemoteHandle::load_project_tasks` 读出任务记录后：
- 对每个有实时驱动的任务，加一个 `pendingApprovals` 数组，每项是 `{requestId, kind, summary, subagent?}`；
- 原来的 `approval`（钩子审批）照旧保留；
- 拼装放在纯函数 `attach_live_pending` 里，好单独测。

手机读快照时，把 `approval` 和 `pendingApprovals` 都变成审批卡：摘要取 `summary`，没有就取 `kind`；按 `requestId` 去重。

**推送接到手机：** core 新增 `remote::bridge::PhoneEventBridge`，它是宿主事件日志的观察者（`EventObserver`）。
- 观察者只有一个位子，原来那个（推送派发器，没启用推送时为空）由它转交，行为不变；
- 事件名是 `task-status` 或 `task-changed` 时，写进 `RemoteState.event_log` 拿到序号，再用 `clients.broadcast_push` 发给在线的手机；
- 写入、广播都不阻塞（广播是 `try_send`），满足观察者“立即返回”的约定；
- `aeroric-cli serve` 在远程服务启用、建好 `RemoteState` 之后挂上它。

**实时会话的变化记为 `task-changed`。** 宿主的 `TaskServiceSink::update` 收到以下更新时，往宿主事件日志写 `task-changed {taskId, change}`：
- 审批请求（`PermissionRequest`）；
- 审批结束（`PermissionResolved`）、审批没送到（`PermissionSendFailed`）；
- 回合开始（`TurnStarted`）、回合结束（`TurnEnded`）。

`task-status` 原本就在写，只是没人转给手机。推送派发器只认 `task-status`，所以 `task-changed` 不会多发一条系统通知。

**手机收到推送就重读：** 连接循环收到 `task-status` 或 `task-changed` 推送后，按第六步的办法重读快照。
- 重读期间又来了推送，记一笔，读完再读一次；
- 不会叠成多次并发的读。

## Alternatives considered

**推送里直接带上审批内容，手机按增量改。** 没选：手机端的规则是“快照是权威，推送只是提示”（AP6 和 D4 第一阶段）。重读一次快照只要几个请求，状态不会因为漏了某条增量而走偏。

**把宿主的事件日志换成 `RemoteState.event_log`，合成一份。** 没选：两份日志的用途和生命周期不同。宿主那份给桌面 App 和推送派发器，一直存在；远程那份只在远程服务启用时才有，序号是手机的水位线。桥接只转手机关心的两种事件，改动最小。

**在 `emit` 里实现推送。** 没选：`emit` 只在少数 RPC 里被调用（例如置顶项目），任务和会话的变化不经过它。

## Consequences

- **收益：**
  - 手机上能看到并回答 Codex、Claude 等实时会话的审批，回执走第三步的 `answer_structured`；
  - 手机连着时，Mac 上任务状态变化、来了新审批、审批结束，都会及时同步到手机。
- **局限：**
  - 推送只到“任务变了”这一级，手机每次都重读整份快照，项目和任务很多时请求会多；
  - 会话消息、终端、文件和 Git 仍不随推送更新；
  - 真机、真实 Relay、真实 Mac 上 NOT RUN。
- **验证：**
  - core 新增 1 个测试（`remote::bridge`）：宿主日志里的 `task-status`、`task-changed` 进了手机的日志并按序编号，别的事件不进；被替换的观察者照样收到全部事件。
  - 宿主新增 2 个测试：
    - `attach_live_pending` 给有实时会话的任务挂上待审批（带子代理名），钩子审批照旧，没有会话、没有待审批的任务不动；
    - `TaskServiceSink::update` 收到审批请求、审批结束时写 `task-changed`，消息片段不写。
  - iOS 新增 1 个测试：快照里的 `pendingApprovals` 变成审批卡（子代理名在前），`task-changed` 推送后重读出新卡，`task-status` 推送后卡片消失。
  - 假 Mac 的默认推送改为不触发重读的事件名，原有按序号的测试不受影响。
  - MacBook 上：
    - core 的 remote 测试 125 个都过，宿主的 remote_headless 测试 3 个都过，`aeroric-cli` 编过，`cargo fmt --check` 过；
    - iOS 包 57 个测试都过，连接两套测试连跑 5 次都过；iOS App 在模拟器上编过。
