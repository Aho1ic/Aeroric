# Agent Note: D4 第三步：手机上的审批，Agent 收到了才算“已生效”

Status: implemented

## Problem

[批次 D](../../proposed/feature/2026-11-15-adoption-d-new-surfaces.md) D4 第二阶段要求，用户在手机上批准或拒绝后：

- 按钮变成“发送中”，直到 Mac 上的宿主确认；
- 确认后显示“已生效”；
- 超时显示“不确定是否生效，回到 Mac 上确认”；
- Mac 上已经处理过的，显示“已在另一端处理”。

做法第 4 条：审批回执复用 A2，宿主只有在回应真正送达 Harness 后，才回复手机“已生效”。完成条件：手机上显示“已生效”的审批，Harness 一定收到了。

动手前（`main@4c902898`）核对的事实：

- **手机的审批没接到驱动。** `task.respond` 进的是宿主 `remote_headless.rs` 的钩子审批登记表（`take_waterfall_answer`），只要登记表里还有这条，就回 `accepted: true`。结构化会话（Claude、Codex、OMP 的驱动）的待审批不在那张表里。[A2 笔记](../bug-fix/2026-11-16-a2-approval-answer-must-arrive.md)也写明，手机接驱动留给 D4。
- **驱动的 `respond` 是同步的，只负责入队。** 结果靠会话事件报出来：写进 Agent 输入的是 `PermissionResolved`；没写进的是 `PermissionSendFailed`，`failed` 表示请求还开着，`ended` 表示随连接结束。另一端正在答，驱动返回 `AlreadyAnswered`；请求已经不在，返回 `UnknownRequest`。
- **会话事件经一串状态汇转发**（`LiveStatusSink::update`）。宿主没有现成的入口，可以“等某个请求结算”。
- **远程句柄拿不到结构化执行器。** `CliRemoteHandle` 不持有 `StructuredExecutor`，但组装它的 `aeroric-cli serve` 手里有。
- **手机端已有现成的状态。** `CompanionApprovalOutcome` 和卡片状态已经有：发送中、失败、不确定、已在另一端处理、已结束。缺的是把宿主的回复翻成这些状态。

## Decision

**审批回执层**（`sessions/live/receipts.rs::PermissionReceipts`）：

- 一层状态汇，放在执行器事件链的最外面，什么都照样往里转发；
- 看到 `PermissionResolved` 或 `PermissionSendFailed`，就按“任务 + 请求”记下结论：已送达、没写进、已结束，并唤醒等待者；
- 最多记 1000 条，超了先丢最旧的；
- `forget` 在回答前清掉这条请求的旧结论，免得把上一次的失败当成这一次的；
- `wait` 等到这条请求的结论，或者等到时限；
- 执行器经 `receipts()` 提供它。

**宿主**（`remote_headless.rs`）：

- `CliRemoteHandle` 加 `live`，即结构化执行器。`serve` 传入，测试里为空。
- `task.respond`：任务有驱动，并且驱动的待审批里有这个请求号，就走结构化路径 `answer_structured`；否则照旧进钩子登记表，回复里补上 `outcome: resolved`。
- `answer_structured` 按顺序：
  1. 清掉旧结论；
  2. 批准按“允许一次”交给驱动，拒绝按“拒绝”交给驱动，只交一次，不重发；
  3. 驱动直接拒绝时，马上回复：`AlreadyAnswered` 回“另一端正在回答这条审批”，`UnknownRequest` 回“这条审批已由另一端处理，或已经结束”，都记为 `answeredElsewhere`；其他错误记为 `sendFailed`；
  4. 否则等驱动结算，最多 10 秒：
     - 已送达 → `resolved`，`accepted: true`，只有这一种算已生效；
     - 没写进 → `sendFailed`，带驱动给的原因；
     - 已结束 → `ended`；
     - 时限内没结论 → `unconfirmed`：“Mac 还没确认送到 Agent，回到 Mac 上看”。

**手机**（`CompanionStore`）：

- `CompanionApprovalOutcome.fromReply` 读宿主的回复。`outcome` 对应到：已生效、没写进、已结束、已在另一端处理，以及新增的 `unconfirmed(reason)`。旧版宿主只回 `accepted: true`，按已生效处理；读不懂的不改卡片。
- `applyApprovalReply(requestId:, resultJSON:)`：
  - 把这条审批的命令移出待回复列表（第二步的 `outbox`）；
  - 按结论更新卡片：只有已生效移除卡片；没写进的卡片留着、可以再答；不确定的写宿主给的原因；已在另一端处理的不能再答。

## Alternatives considered

**宿主收下回答就回复手机，由手机再去查会话状态。** 最强的理由是宿主不用等待。没选：

- 手机就得订阅会话事件，自己判断哪一条是这次回答的结果；
- 完成条件要的是手机上的“已生效”一定可靠。在宿主这一侧等驱动结算，A2 的判断只做一次，手机收到的就是结论。

**用轮询驱动快照里的待审批列表代替事件。** 最强的理由是不用加状态汇。没选：请求从待审批里消失，有两种可能：写进去了，或者随连接结束了。光看快照分不出来，而这两种情况手机上要显示的完全不同。

**超时后宿主继续等，等到了再推给手机。** 最强的理由是用户最终能看到结果。没选，这一步不做：

- 要做一条宿主到手机的推送消息；
- 手机重连后的权威快照，本来就会按当时的状态替换卡片；
- 超时时如实说“不确定，回到 Mac 上看”，正是方案要的。

## Consequences

- **收益：**
  - 手机上的审批对结构化会话也有效了；
  - 手机上显示“已生效”时，Agent 一定收到了回应；
  - 写不进、随连接结束、另一端已处理、迟迟不确认，各有说法；
  - 只交一次，不重发。
- **代价：**
  - 每个手机审批最多占一个请求 10 秒；
  - 执行器的事件链多一层，记最近 1000 条结论。
- **局限：**
  - 真实连接上的收发还没接（第二步留下的）；
  - 手机只能“允许一次”或“拒绝”，没有“本会话总是允许”；
  - 手机拒绝时不带理由；
  - Orca 协议不经过这里；
  - 按 `generation` 和修订号判旧还没做。
- **验证：**
  - core：回执层 1 个测试。等待被这条请求的结论唤醒，别的任务的同号请求不算；没写进和已结束分开记；清掉之后等不到。
  - 宿主：`answer_structured` 1 个测试，用一个按驱动方式行事的假驱动，先收下回答，稍后经回执层发结论。已送达（批准、拒绝各一次）、没写进、已结束、另一端在答、请求已不在、迟迟没结论，各回对应的结论；每次都只交给驱动一次。
  - 手机：`ApprovalReceiptTests` 2 个，手机端共 32 个测试全部通过：
    - 宿主回复的几种结论、旧版宿主、读不懂的回复各自翻成什么；
    - 提交后是“发送中”，收到回复后命令移出待回复列表；只有已生效移除卡片；没写进的能再答；不确定的写原因；另一端处理过的不能再答。
- **没测：** 真实 Agent 上的手机审批；真实连接上的往返。
