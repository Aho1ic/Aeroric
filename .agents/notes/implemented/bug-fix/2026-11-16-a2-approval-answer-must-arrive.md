# Agent Note: A2 审批回应没送出去，不算已处理

Status: implemented

## Problem

[批次 A](./2026-11-15-adoption-a-truthful-state.md) 的 A2。2026-11-16 在 `main@d5126573`（加 A1 未提交的工作区改动，A1 没碰这三个驱动）上复核，位置与 `8d4391c3` 相同：

- Claude `core/crates/aeroric-core/src/sessions/live/claude/driver.rs:627–653`：actor 的 `respond` 先把请求从 `pending` 里取走；`client.respond` 失败时推一条 `Error`（"审批未送达"），随后**照常**推 `PermissionResolved`，再把任务状态改回 `running`。`self.client()` 为空时请求被静默丢掉，什么也不推。
- Codex `codex/driver.rs:669–700`：同形，失败后照常推 `PermissionResolved`，拒绝理由的 steer 也照发。`:1024`（`drop_pending`）和 `:1122`（`server_exited`）是撤回和进程退出时的清理，推 `cancelled`。
- OMP `omp/driver.rs:761–792`：同形；`AllowForSession` 的授权在发送**之前**写进 `grants`，发送失败也留着，之后同类请求会被自动批准。`:979`（omp 自己撤回）和 `:1085`（`drop_waiting`）推 `cancelled`。
- 三个驱动的 trait 方法 `respond` 只检查请求还在 `pending` 里就入队。两端同时回答同一请求时两次都返回成功，第二次在 actor 里找不到请求，静默结束。
- 消费方把 `PermissionResolved` 当事实：检查点派生 `checkpoint/derive.rs:190` 把 `allowOnce`/`allowForSession` 记进"已同意"；讨论模式 `discussion/mod.rs:924` 把成员改回"在说"；Swift `LiveSessionModel.swift:462` 移除审批卡。
- iOS `CompanionStore.swift:524` 的 `submitApproval` 在本地直接移除待审批，不等宿主结果。

三个驱动的发送都是往一个无界通道写一行（`ControlClient::respond`、`RpcClient::respond`、`OmpClient::answer_ui`）。写失败只有一种原因：写端已关闭，也就是到 Agent 的连接已经不能写了。

用户看到的是：点了批准，审批卡消失，Agent 却没收到回应；检查点里记成"已同意"；手机上点了就没了。

## Decision

**审批回应写进 Agent 的输入之后，才发布"已处理"。没写进去的，卡片留着并说明原因。**

### 驱动（Claude、Codex、OMP 同形）

1. **先占位再发送。** 待审批项加 `claimed`（有一个回答正在送往 actor）和 `tried`（用户回答过至少一次）。trait 方法 `respond` 在同一把锁里检查并占位：请求不在 → `UnknownRequest`；已被占 → `DriverError::AlreadyAnswered`；否则置 `claimed = tried = true` 再入队，入队失败时撤回占位。
2. **写成功才结算。** actor 发送时请求仍在 `pending`；写成功后才移除、推 `PermissionResolved`、按原逻辑改任务状态。Codex 的拒绝理由 steer、OMP 的拒绝 steer 和"本会话总是允许"授权都只在写成功后执行。
3. **写失败（连接仍在）。** 撤回占位，请求留在 `pending`，推 `PermissionSendFailed { id, state: failed, reason }`，原因由 `answer_not_written` 写成"审批没送出去：写不进 <Agent> 的输入（…）"，任务状态回到 `input_required`（于是留在"需要我处理"）。用户可以再答。
4. **连接或回合结束。** actor 处理时连接已不在，或进程退出、回合结束清理时这条请求 `tried` 过：推 `PermissionSendFailed { state: ended }`，原因是 `ANSWER_ENDED_WITH_CONNECTION`（"审批没送出去：这次请求已随连接结束"）或 `ANSWER_ENDED_WITH_TURN`，**不推** `PermissionResolved`。没被回答过的请求照旧推 `cancelled`；Agent 主动撤回（Claude `control_cancel_request`、omp `cancel`）和 Codex 的 `serverRequest/resolved` 照旧推 `PermissionResolved`。
5. **不自动重发。** 重发只由用户触发。

### 协议

- `SessionUpdateKind::PermissionSendFailed { id, state, reason }`，`state` 是 `PermissionSendState { failed, ended }`。`export-schema` 已重新生成 `session-live.schema.json` 和夹具（夹具里第 6 条是一次 `failed`，第 7 条是重答后的 `permissionResolved`）。旧客户端按未知类型解码。
- `DriverError::AlreadyAnswered` 在宿主映射为 `BUSY`；请求已不在仍是 `NOT_FOUND`。
- 宿主不发"不明"：驱动层要么写进去了，要么没写进去。说不清的只发生在客户端到宿主这一段，由客户端处理。

### 消费方

- 检查点派生：只有 `PermissionResolved` 的 `allowOnce`/`allowForSession` 进"已同意"；`PermissionSendFailed { ended }` 把意图记为结果不明（与 `cancelled` 相同）；`failed` 不改意图。
- 讨论模式：`ended` 视同结算，成员回到"在说"；`failed` 不变。

### macOS

- `LiveUpdateKind.permissionSendFailed(id:state:reason:)`、`LivePermissionSendState`。
- `LiveSessionModel.sendFailures`：`failed` 记下原因，卡片留着；`ended` 移除卡片，对话里加一行"<原因>（<命令摘要>）"；`permissionResolved` 清掉记录。用户再答时 store 调 `clearSendFailure`。
- `LiveApprovalMachine`：
  - `.failed(LiveDecision?, message:)`：调用被拒，或宿主报 `failed`（`hostFailed`，另一端的回答失败时决定为 nil；正在输入拒绝理由时不打断）。
  - `.unknown(LiveDecision, message:)`：`ConnectionError.timedOut`、`.disconnected`。卡片写"…不知道送到没有。不会自动重发；再答一次也不会答两次"，可以手动再答。
  - `.settledElsewhere(message:)`：宿主回 `BUSY`（"另一端正在回答这条审批"）或 `NOT_FOUND`（"这条审批已由另一端处理，或已经结束"），按钮禁用，等宿主结算。
  - 分类在 `SubmitFailure.classify` 里，store 只调用它。
- 回答期间任务状态保持 `input_required`，"需要我处理"照常列出，不新增队列种类。

### iOS 伴随端

- `CompanionApproval.state`（`CompanionApprovalState`）：`open`、`submitting`、`failed`、`unknown`、`answeredElsewhere`、`ended`；`canSubmit` 只对 `open`、`failed`、`unknown` 为真，否则 `submitApproval` 抛 `approvalNotAnswerable`。
- `submitApproval` 只把状态改为 `submitting`；`applyApprovalOutcome(requestId:_:)` 接 Mac 的结果（`CompanionApprovalOutcome`），只有 `resolved` 移除卡片。权威快照照旧整表替换。界面在卡片下写 `state.note`，不可答时按钮禁用。
- 手机的 `task.respond` 走宿主 `remote_headless.rs:221` 的钩子审批登记表，还没接到结构化会话的驱动；把手机接到驱动、并把 `BUSY`/`NOT_FOUND`/超时翻成 `CompanionApprovalOutcome` 是 D4 的事。

### 同主题笔记审计

| 笔记 | 分类 | 处理 |
|---|---|---|
| [批次 A](./2026-11-15-adoption-a-truthful-state.md)、[落地总纲](../../proposed/architecture/2026-11-15-research-adoption-roadmap.md) | 上级方案 | 不动；A2 状态和待目检项已回写路线文档 §8 |
| [结构化协议](../architecture/2026-10-25-structured-agent-protocols.md) | 被本篇补充 | 原地补一句"内嵌审批写成功才结算" |
| [故障注入](../feature/2026-11-04-fault-injection.md) | 被本篇补充 | 原地补上 A2 的故障场景和 `Peer::deafen` |
| [A1 委派结果卡](./2026-11-16-a1-delegation-card-follows-delivery.md) | 无关 | 不动 |
| [批次 D](../../proposed/feature/2026-11-15-adoption-d-new-surfaces.md) | 部分重叠 | 不动；D4 手机跟随依赖本篇，手机接驱动留给它 |

## Alternatives considered

**发送失败时自动重发。** 最强的理由是用户不必再点一次。没选：审批回应不幂等，第一次可能其实到了，重发会被 Harness 当成下一个请求的回答。

**驱动层加超时：trait 的 `respond` 等 actor 的发送结果，超时记为不明。** 最强的理由是"超时"在驱动这一层就有了，测试最直接。没选：trait 方法是同步的，宿主在分发线程上调用；阻塞等 actor 要么占住运行时线程，要么在单线程运行时里死锁。actor 忙时请求其实还没发出，那是"还在发"，不是"说不清"；客户端到宿主这一段的超时才是真正说不清，放在客户端处理。

**连接结束时把回答过的请求也推 `cancelled`，界面只看 `cancelled`。** 最强的理由是不加新的结算值。没选：`cancelled` 也用于 Agent 主动撤回，界面分不出"Agent 撤回了"和"你的回答没送到、请求随连接结束了"。

**把"正在回答"放进 `PendingPermission` 快照字段，让另一端提前看到。** 最强的理由是两端都能实时看到对方在答。没选：多一个要同步的字段，`BUSY` 已经能在第二个回答到达时说清；留给 D4 再看。

## Consequences

- **收益**：任何路径都不会在回应没写给 Agent 时产生 `PermissionResolved`；检查点里的"已同意"只来自写成功的回应；omp 不会因为一次没送到的"总是允许"而自动批准后续请求；两端同时回答时后到的一端看到原因。
- **代价**：
  - 写失败后卡片留着，直到用户再答或连接结束清理；连接已坏但宿主还没察觉时，再点会再失败一次。
  - actor 卡在别的等待里（例如等中断的回复）时，第二个回答被拒为"另一端正在回答"。这是真实状态：第一个回答还没发出。
  - 旧版 Swift 把 `permissionSendFailed` 当未知更新显示一行；卡片仍在，可以再答。
  - 手机还不能真正回答结构化会话的审批（见上），iOS 的状态机只有单测。
- **验证**（2026-11-16，Mac 上跑）：
  - core 驱动：Claude `an_answer_that_cannot_be_written_keeps_the_request_open`（返回错误）、`an_answer_is_settled_only_once_written_and_the_first_answer_wins`（actor 忙时回答被扣住、第二个回答 `AlreadyAnswered`、写成功才结算）、`an_answer_whose_connection_closed_ends_unsent`（连接关闭，没回答过的仍 `cancelled`）；Codex `an_answer_codex_cannot_take_keeps_the_request_open`、`a_held_answer_wins_once_and_ends_unsent_when_codex_exits`；OMP `an_answer_omp_cannot_take_keeps_the_request_open_and_grants_nothing`、`a_held_answer_wins_once_and_ends_unsent_when_omp_exits`。连接关闭的几个有调度竞争，连跑 10 遍都通过。故障注入用 Codex 假对端新加的 `Peer::deafen`（对端不再读输入）和 Claude、OMP 测试对端的 `from_client.close()`。
  - core 检查点：`an_answer_that_did_not_reach_the_agent_is_no_consent`。
  - 宿主端到端 `session_live`：`an_answer_codex_never_got_is_not_settled_and_the_second_answer_is_busy`（经 RPC 两次回答，第二次 `BUSY`；Codex 退出后请求以 `ended` 结束，没有 `permissionResolved`，检查点意图 `outcomeUnknown`），连跑 3 遍通过。
  - protocol：夹具含 `permissionSendFailed`。
  - Swift：`LiveSessionModelTests.testAnAnswerThatDidNotArriveKeepsTheCardOrSaysItEnded`、`testApprovalMachineTellsNotSentFromNotKnownFromAnsweredElsewhere`；合同测试解码 30 条更新。iOS `testAnApprovalWaitsForTheMacsWordAndTheFirstAnswerWins`，原有流程测试改为先 `submitting` 再 `resolved`。
  - 界面行为未目检，记在路线文档 §8 待目检清单。
- **2026-11-18 补强**：DSH（ACP）驱动和 omp 的问答回答补上同样的送达规则（B1-3），见 [全量审查 Batch 1](./2026-11-18-review-batch-1-core-gaps.md)。
