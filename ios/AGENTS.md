# ios/ 规则

iOS/iPadOS 伴随端：远程查看与控制在线的 Mac，不在手机上运行 CLI。现状见 [docs/subsystems/ios-companion.md](../docs/subsystems/ios-companion.md)。

- 推送只是提示：`CompanionStore.ingestPushHint` 只置 `resyncNeeded`；审批只来自 E2EE 快照，绝不由推送或 `eventSeq` 派生。→ [推送平面](../.agents/notes/implemented/architecture/2026-10-13-push-plane.md)
- Mac 不可用时不提交执行或审批；不复制桌面 IDE/DAP、数据库网格、基础设施与笔记工作区。
- entitlements 的 `aps-environment` 保持 `development`，生产值随真实 App ID 落地，不伪造。
- `.xcodeproj` 由 XcodeGen 从 `project.yml` 生成。
- 验证：`swift test --package-path ios/AeroricCompanionKit`。
