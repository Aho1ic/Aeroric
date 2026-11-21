# Agent Note: D4 第二步：手机的命令带上编号和启动标识，跳了就重新同步

Status: implemented

## Problem

[D4 第一步](2026-11-21-d4-command-identity.md)让宿主按 `commandId` 去重、按 `hostEpoch` 判旧（`main@fad7b1f8`），但手机端还没带上这两项。方案 D4 第 1–3 条在手机这一侧要做到：

- 命令带设备生成的稳定编号，重发不变；
- 命令带上它所基于的宿主启动；
- 发现事件序号不连续，或宿主告知缓冲已溢出时，丢弃本地状态，重新拉取完整快照；
- 第一阶段只修同步的正确性，不改变手机能做的事。

动手前核对 `ios/AeroricCompanionKit`：

- **命令只是记了一笔。** `CompanionStore` 是手机端的状态机：在线才能发命令，离线不排队；审批卡等 Mac 回话（A2）；按 `hostEpoch` 判断审批是否过期。审批（`submitApproval`）和新建任务（`createTask`）现在只往 `sentFramesLog` 记一笔，没有编号，也没有真正走网络。真正的收发由 App 接，测试里不涉及。
- **重连：** `handleReconnect` 每次都套用快照，并清掉不属于新启动的审批。可是已经发出、还没回答的命令，没有地方记。
- **推送提示：** 推送提示里的序号有跳变时，会置 `resyncNeeded`。宿主推来的事件，以及 `events.since` 回答的“溢出要重置”，都还没有入口。
- **界面：** 状态横幅只写连接状态，不提重新同步。
- **编码：** `RemoteRpcCodec.encodeRequest` 能编码请求，没有带身份的命令。

## Decision

**命令对象** `CompanionCommand`：

- 方法、参数（字符串键值）、`commandId`（建立时生成一个 UUID，之后不变）、`hostEpoch`（建立时的宿主启动）；
- `wireParams` 是发到线上的参数：原参数加上 `commandId` 和 `hostEpoch`；
- `RemoteRpcCodec.encodeCommand` 用它编码请求。

**发出和回答：**

- `submitApproval`、`createTask` 在原来的检查都通过后，各建一条命令，放进 `outbox`（已发出、未回答）。`sentFramesLog` 照旧记。
- `applyCommandReply(commandId:, failure:)`：Mac 回答了，就把这条移出 `outbox`。拒绝原因以 `STALE_HOST_EPOCH` 开头时，说明本地看到的状态是旧的，置 `resyncNeeded`。

**重连：**

- 回到同一次宿主启动：`commandsToResend()` 给出 `outbox` 里的命令，编号不变。Mac 收到重复的，会返回第一次的结果。离线时调用会抛错，不排队。
- 宿主换了一次启动：`outbox` 里的命令全部移到 `droppedCommands`，不再发。它们是按旧状态决定的，Mac 也会拒绝，用户应该先看到新状态再决定。

**事件：**

- `ingestHostEvent(seq:)`：重复的或更早的序号忽略；跳了号就置 `resyncNeeded`；
- `ingestEventsReset()`：宿主的事件缓冲溢出（`events.since` 回答要重置），置 `resyncNeeded`。

**撤销：** 设备被撤销时，`outbox` 和 `droppedCommands` 一起清空。

**界面：** 状态横幅多一个参数 `resyncing`。在线且需要重新同步时写“正在和 Mac 重新同步，之前显示的状态可能已过期”，不再只写“Mac 在线”。根视图传入 `store.resyncNeeded`。

## Alternatives considered

**宿主换了启动后，把旧命令带上新的启动标识重发。** 最强的理由是用户不用再点一次。没选：命令是按旧状态做的决定，比如批准一个当时还开着的请求。Mac 重启后，那个请求可能已经结束或换了一个，原样重发等于替用户做新的决定。丢掉并记下来，让用户在新状态下再决定。

**每次重发都生成新的编号。** 没选：那样 Mac 去重不了，正好是方案要防的“重连后重复执行”。

**序号跳变时立刻清空本地的任务和审批。** 最强的理由是方案说“丢弃本地状态”。没选：快照到来之前，清空会让列表闪成一片空白。标记 `resyncNeeded`，横幅写明状态可能过期，审批照旧受启动标识和 A2 回执的约束；快照一到就整体替换，`handleReconnect` 本来就是这样做的。

## Consequences

- **收益：**
  - 手机端的命令有了稳定编号和启动标识，加上第一步的宿主去重，同一条审批或新建任务在重连后不会执行两次；
  - Mac 重启后，旧决定不会被发出去；
  - 漏了事件，界面会如实说明“正在重新同步”。
- **代价：** 状态机里多了两个列表。
- **局限：**
  - 真实连接上的收发还没接：App 何时调 `commandsToResend`，以及把回答和事件喂进 `applyCommandReply`、`ingestHostEvent`，都是后续接线；
  - 终端输入是二进制帧，不走命令，没有编号；
  - 会话控制、写文件等其他会改东西的操作还没建命令；
  - 第二阶段的审批回执（第 4 条）没做。
- **验证：** `swift test --package-path ios/AeroricCompanionKit` 共 30 个测试，全部通过，其中新增 `CommandIdentityTests` 3 个：
  - 新建任务进入待发列表；编码后的参数带编号、启动标识和原参数；离线时不给重发；回到同一次启动后连取三次，都是同一个编号；回答后移出列表；
  - 审批进入待发列表；Mac 换了启动后旧命令被丢下、不重发；新命令带新的启动标识；收到过期拒绝后要求重新同步，横幅照实写；
  - 重复的事件序号忽略，跳号或溢出要求重新同步；撤销后两个列表都清空。

  Mac 上的全量验证不包含手机端的包，这次是单独跑的。
