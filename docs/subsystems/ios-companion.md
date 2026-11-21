# iOS/iPadOS 伴随端

远程查看与控制在线 Mac 上的任务；不在手机上运行 CLI，也没有直连执行宿主。Mac 不可用时手机不提交执行或审批。工程由 XcodeGen 从 `ios/project.yml` 生成（部署 iOS 26.0）；`AeroricCompanionKit` 包支持 iOS 17 / macOS 14 以便在 Mac 上跑单测。

## 结构

| 路径 | 内容 |
|---|---|
| `ios/AeroricCompanionApp` | App 入口与 entitlements（`aps-environment` 当前为 `development`） |
| `ios/AeroricCompanionKit/Sources/AeroricMobileTransport` | `E2eeTransport`（经 relay 的端到端加密通道）、`RemoteRpcCodec`、`CompanionStore`（快照与待审批）、`PushBridge` |
| `ios/AeroricCompanionKit/Sources/AeroricMobileUI` | 界面 |

## 规则

- 推送只是提示：`CompanionStore.ingestPushHint` 只置 `resyncNeeded`，审批只来自 E2EE 快照。见 [推送平面](../../.agents/notes/implemented/architecture/2026-10-13-push-plane.md)。
- 审批卡等 Mac 的结果：`submitApproval` 只把卡片改为 `submitting`，`applyApprovalOutcome` 按 Mac 的结果移除（`resolved`）或改为 `failed`、`unknown`（没等到回复，不自动重发）、`answeredElsewhere`、`ended`。手机的 `task.respond` 对结构化会话的待审批交给它的驱动，宿主等驱动结算（A2）再回复：`resolved`（Agent 收到了）、`sendFailed`、`ended`、`answeredElsewhere`、`unconfirmed`（10 秒内没结算，回 Mac 上看）；`CompanionApprovalOutcome.fromReply` 把它变成卡片状态，只有 `resolved` 移除卡片。其余（钩子审批）仍走钩子审批登记表。
- 命令带身份（D4）：审批和新建任务各生成一个 `CompanionCommand`，带设备生成的 `commandId` 和当时的 `hostEpoch`，发出后留在 `outbox` 直到 Mac 回答；重连到同一次启动时原样重发（同一个 id，Mac 只执行一次），Mac 换了启动就丢掉、记进 `droppedCommands`，不重发；Mac 以 `STALE_HOST_EPOCH` 拒绝、事件序号跳变或缓冲溢出时置 `resyncNeeded`，状态横幅写“正在和 Mac 重新同步”。线上收发还没接到真实连接。
- 范围是完整任务伴随与相关高级操作，不复制桌面 IDE/DAP、数据库网格、基础设施与笔记工作区。

## 测试

`swift test --package-path ios/AeroricCompanionKit`。
