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
- 命令带身份（D4）：审批和新建任务各生成一个 `CompanionCommand`，带设备生成的 `commandId` 和当时的 `hostEpoch`，发出后留在 `outbox` 直到 Mac 回答；重连到同一次启动时原样重发（同一个 id，Mac 只执行一次），Mac 换了启动就丢掉、记进 `droppedCommands`，不重发；Mac 以 `STALE_HOST_EPOCH` 拒绝、事件序号跳变或缓冲溢出时置 `resyncNeeded`，状态横幅写“正在和 Mac 重新同步”。
- 连接循环（D4 第四步）：`CompanionLink` 照 `core/.../remote/server.rs` 说话：明文 `hello` 握手（钉配对时的公钥）、v2 `auth`、之后 v3；连上读 `hello`（`hostEpoch`）、`events.since`（水位线）、`projects.list` 和各项目的 `tasks.list` 组成快照交给 `handleReconnect`，读的时候到的推送攒到之后再按序号交；`flush()` 发出 `outbox` 里这条连接没发过的命令，回答按请求 id 交给 `applyCommandReply` / `applyApprovalReply`；断开时等回答的审批记为不确定，命令留着等同一次启动重发；令牌不认识按设备撤销处理。网络藏在 `CompanionSocket` 后面，真机用 `URLSessionCompanionSocket`。
- 配对与重连（D4 第五步）：`CompanionPairingOffer.parse` 读 Mac 的 `aeroric://pair?code=…`；`CompanionLink.pair` 用邀请认证，凭据先 `stagePending` 再发 `pairing.confirm`，`confirmed` 后转正、被拒丢掉、没回答留着（下次启动先拿它去连）；凭据在钥匙串（`KeychainCredentialStore`，本机解锁后可读、不同步）。`CompanionConnector` 由 App 持有：进前台 `start()`、进后台 `stop()`，按顺序试每个地址（局域网在前、Relay 在后），连不上退避到最多 30 秒，断了重连，令牌被拒清凭据；store 的 `onCommandIssued` 接到 `flush()`，`push.register` 走已认证的通道。“配对与连接”一页可粘贴配对链接、看连接状态、取消配对。扫码和点链接见下一条。
- 扫码和链接（D4 第七步）：Mac 的配对页用 CoreImage 把配对链接画成二维码（`PairingQRCode`，带静区）；手机“配对与连接”一页可用 VisionKit 扫码（`CompanionPairingScanner`，设备或相机不可用时写明原因），App 注册了 `aeroric` scheme（`project.yml` 的 `info:` 合并进自动生成的 Info.plist），相机说明在 `INFOPLIST_KEY_NSCameraUsageDescription`。扫到的、点开的链接都只经 `CompanionConnector.offerPairingLink` 填进配对页并显示链接里的 Mac 名，已配对时写明会替换哪台，配对仍要用户点。
- 按任务的运行次数判旧（D4 第六步）：`task.respond` 带 `projectId` 和快照里的 `basedOnGeneration`，宿主（`commands::check_task_generation`，在只执行一次之内）发现任务已重新运行或不在了，以 `STALE_TASK_GENERATION` 拒绝；手机把它和 `STALE_HOST_EPOCH` 一样置 `resyncNeeded`，连接循环看到 `resyncNeeded` 就在同一条连接上重读快照（推送跳号、缓冲溢出也一样）。
- 范围是完整任务伴随与相关高级操作，不复制桌面 IDE/DAP、数据库网格、基础设施与笔记工作区。

## 测试

`swift test --package-path ios/AeroricCompanionKit`。
