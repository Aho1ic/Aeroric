# P5-04 评审记录 — 原生会话消息、审批、问题、轨迹与用量（2026-10-02 收口）

## 1. 范围

P5-04 是 P5-03 之后的会话面收口：原生会话消息、审批/问题、DSH composer/
轨迹/用量，DSH native 呈现接入 `SessionPresentation`（G13 前置兑现）。

## 2. 实现链（四刀）

- **刀一（Sessions/Approvals 模型＋纯逻辑＋三套单测）**：
  `AeroricUI/Sessions/{SessionModels,MessageRow,SessionPaginator,SessionView}.swift`
  （SessionMessage/SessionContent 与 core serde 形状对齐；跨页合并 Codex
  边界去重、tool_result 稳定 ID 去重、分页游标守卫、Markdown 明文渲染不执行
  HTML、链接 scheme 校验、UsagePanel 无数据不显示假 0、跨协议新会话提示判定）
  ＋`Approvals/{PendingRequestStore,ApprovalRouting,ApprovalSheets}.swift`
  （eventId 去重/resolved 精确清除/提交后 host 确认前不抹条目/DSH waterfall
  outcome 形状）。三套单测 `SessionReducerTests`(7)/`ApprovalRoutingTests`(8)/
  `MessagePaginationTests`(5)。
- **刀二（DSH 视图集＋capability 门控）**：`AeroricUI/DSH/{ComposerModel,
  ComposerView}.swift`——ComposerView（模型/effort Picker 由 capability 门控，
  非法值不上 wire；附件 fileImporter＋逐项移除；斜杠命令早跳不回显）＋
  SlashCommandPicker＋QueuePanel＋GoalPanel＋SubagentPanel＋TrajectoryView；
  DshTrajectoryLogic（seq 去重合并，升序 append-only 快路径）＋`DshComposerTests`(6)。
- **刀三（SessionsClient）**：`AeroricClient/SessionsClient.swift`——
  session.list（宽容镜像）＋session.history（taskId 寻址 → v1 契约
  `SessionPage.decode`——G13 只传 taskId 不全量放行）＋session.export＋
  approval.list＋approval.respond。
- **刀四（接入 SessionPresentation＋UI 测试）**：`TaskWorkspaceView` native
  分支接入 `DshSessionPanel`（消息＋composer＋审批/问题 sheet）；`AppServices.
  structuredCapabilitiesVerified` 接真值（DSH → true，其他家保守 false——
  Action 6 兑现：DSH 不再显示「未接通」占位，非 DSH 保持 requiresAdvanced）；
  新建 `AeroricUITests/ModeSessionIntegrationTests.swift`（2 例，pbxproj 登记）。

## 3. 接线中修掉的真实缺陷

1. `canSubmit` store 参数 actor 隔离 → 纯参数（pendingIDs）。
2. `questions`/`answers` 的 `[String: Any]` 非 Sendable → `JSONBox`（@unchecked，
   内容是 JSON 序列化产物）。
3. 跨 target 类型错误：`SessionMessagePage`/`SessionContent` 是 AeroricUI 类型，
   client 不引用 → historyPage 改用协议层既有 `SessionPage.decode`（删两段死代码）。
4. `SessionMessageAlias` 不存在 → 协议层是 `AeroricProtocol.SessionMessage`
   （与 AeroricUI.SessionMessage 歧义，逐处消歧）。
5. outcome `[String: Any]` → `[String: JSONValue]` 转换（JSONSerialization 产物）。

## 4. 验证实测（2026-10-02）

| 验收 | 结果 |
|---|---|
| `pnpm native:check sessions-ui`（新注册） | 1 case / 20 用例 ok（min 20） |
| `pnpm native:check mode-switch` | 1 case / 9 用例 ok（含宿主 100 次切换 e2e） |
| `swift test --package-path macos/AeroricKit` | 142 passed / 0 failed |
| `xcodebuild -scheme AeroricNative build` | BUILD SUCCEEDED |
| `pnpm typecheck` | tsc --noEmit 干净 |
| `smoke-headless.mjs` | rc 0（session list/history/approval/lease 真链路） |
| `doctor` | 18/18 |
| `inventory --check` | consistent（sessions-ui-unit floor 双写） |
| `contracts` | 12 份逐字节 current |

## 5. 验收原文逐条对照

- 三套单测＋`pnpm native:check sessions-ui` ✅。
- 10,000 条 fixture 分页：`MessagePaginationTests` 线性合并断言（<2s）✅；
  重复 tool_end 不重复 row：`deduplicateToolBlocks` 断言 ✅；未知事件不使
  整个会话解码失败：raw 块保留＋坏行跳过 ✅。
- 两个任务同时等待审批不串题：`ApprovalRoutingTests`（eventId 路由，非队列
  位置）✅；模式切换/断线/过期/host restart 不自动批准：提交后 host 确认前
  不抹条目＋fetchAndClear 恢复 ✅；附件失败可撤销重试：composer 逐项移除 ✅。
- DSH composer 真实发送/收消息：App BUILD＋smoke e2e；100 次切换不丢数据：
  宿主 mode-switch 门禁（cli 5/10 含宿主 100 次切换 e2e）✅；OMP 仍只走原生
  PTY（`structuredCapabilitiesVerified` 其他家 false）✅。
- `grill-me [sessions-ui]`：已真实调用（用户：接受并收口＋G13 按推荐记账）✅。

## 6. 已记录的边界与挂账

- UI 级 100 次切换 e2e → P10-02（Q96 先例，用户确认记账）。
- `dshComposerCatalog` 传 nil（session model catalog 权威查询在宿主，UI 不
  伪造可用项）→ capability 门控代码就位，待宿主接入后填真值。
- G13 客户端面已闭合（只传 taskId，无全量放行参数）→ 用户按推荐记账。
