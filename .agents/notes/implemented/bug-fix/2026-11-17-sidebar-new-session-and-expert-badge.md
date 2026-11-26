# Agent Note: 侧栏的新会话排在最上面，专家卡不写"没有投递记录"

Status: implemented

## Problem

2026-11-17 的 Codex CU 目检（报告 `~/Documents/computer-use/2026-11-17-1610-aeroric-a-batch-review/report.md`）发现两处，在 `main@1e682f44` 加未提交改动上复核：

- **新会话在侧栏里排到项目最下面。** 侧栏的项目会话按 `updatedAtMs` 倒序排（`AppServices.sidebarItems`），没有时间的当 0。`AppServices.runTask` 建会话后先放一条占位，不带 `createdAtMs`、`updatedAtMs`；`LiveSessions.applyStatus` 收到 `task-status` 只改状态，不碰时间，而且占位已经在表里，所以不会触发 `refreshWorkspace`。目检时测试项目有 61 条会话，新开的两个会话都排在最后，要靠搜索才能回去；要等别的操作触发整份刷新才回到上面。
- **新的"委派 · 专家"卡也写"已发送（没有投递记录）"。** [A1](./2026-11-16-a1-delegation-card-follows-delivery.md) 让结果卡跟随投递记录，没有记录、只有 `sent = true` 的卡被当作 A1 之前的旧卡。但专家的回答（包括失败说明）从来不走投递队列：主会话的 `ask_expert` 调用自己取走，`take_expert_answer` 这时才把 `sent` 置为真。所以 A1 之后的专家卡也永远落到"旧卡"这句话上。目检里三张 11 月 13 日的失败专家卡写"失败 · 已发送（没有投递记录）"，"已发送"属实，只是说法不对。

## Decision

- **占位带上当前时间**：`runTask` 的占位 `createdAtMs`、`updatedAtMs` 都填当前时间；`task.list` 下次刷新时换成宿主的时间。
- **状态变化算一次活动**：`applyStatus` 改状态时把 `updatedAtMs` 改成当前时间，侧栏顺序跟着最近的动静走，不用等整份刷新。
- **专家卡写"主会话已取走"**：`DelegationText.delivery` 在没有投递记录、`sent` 为真、合同角色是 `expert` 时返回"主会话已取走"（绿色）；其他没有记录的卡仍写"已发送（没有投递记录）"。旧的专家卡也一起改过来，因为它们的回答同样是工具取走的。

## Alternatives considered

**收到 `task-status` 时整份刷新 `task.list`。** 最强的理由是时间完全以宿主为准。没选它：状态事件每轮都有好几次，每次整份拉会话表（几十到上百条）太重；本地时间和宿主时间只差几毫秒，下次刷新会校正。

**宿主给专家卡也建一条投递记录。** 最强的理由是所有卡片走同一套状态。没选它：专家的回答是工具调用的返回值，主会话拿到就是送到了，再建投递记录是假的回执；在卡片上写明"取走"更直接。

## Consequences

- **收益**：新开的会话马上出现在项目的最上面；正在跑的会话随状态变化往上移。专家卡不再看起来像 A1 之前的旧卡。
- **代价**：本地填的时间和宿主记录的时间可能差一点，下次 `task.list` 刷新时顺序可能微调。
- **验证**：
  - Swift `DelegationTextTests.testAResultCardSaysWhereItsResultStands`：失败和已回报的专家卡写"… · 主会话已取走"，没取走时不加后缀；普通旧卡仍写"已发送（没有投递记录）"。
  - 侧栏的改动在 App 目标里，没有单测；靠 App 构建和下次目检（路线文档 §8 待目检清单）。
  - 2026-11-17 在 Mac 上：`swift test`（AeroricKit 961 个跳过 1 个、35 个、110 个）0 失败；App 在临时 DerivedData 里构建成功；`pnpm verify-notes` 通过。
