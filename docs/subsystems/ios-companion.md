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
- 范围是完整任务伴随与相关高级操作，不复制桌面 IDE/DAP、数据库网格、基础设施与笔记工作区。

## 测试

`swift test --package-path ios/AeroricCompanionKit`。
