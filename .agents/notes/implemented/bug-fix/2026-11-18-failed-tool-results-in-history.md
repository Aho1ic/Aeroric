# Agent Note: 被拒或失败的工具，读历史后显示成"完成"

Status: implemented

## Problem

2026-11-18 第五轮目检（`~/Documents/computer-use/2026-11-18-1405-aeroric-review-round5/report.md`）验证了只读讨论成员的审批自动拒绝：

- Claude Code 成员调用 `ExitPlanMode`；
- 宿主的 `ReadOnlyGuard` 在 53 毫秒内替用户拒绝；
- 成员随后改用文字作答。

宿主记录是对的（`toolCall` → `permissionRequest` → `permissionResolved: deny` → `toolCallUpdate: failed`），会话页却先显示"失败"，十几秒后变成"完成"和绿点。拒绝原因要展开工具行才看得到。

原因：

- 会话页运行中用实时更新，状态是对的；
- 这一轮结束后改从历史重建，历史来自 Claude 自己的会话记录：
  - 记录里这一条 `tool_result` 带着 `is_error: true`；
  - 但 `SessionContent::ToolResult` 只有 `id` 和 `output`，解析时丢掉了这个标记；
- App 的 `ToolCallState` 只有运行中、完成、未完成三种，有结果就算"完成"。

## Decision

工具结果带上"失败"标记，一路传到界面：

- core：
  - `SessionContent::ToolResult` 加 `failed: bool`，序列化时为 false 就省略，旧数据和旧读者不受影响；
  - Claude 会话记录按 `is_error` 填它；
  - Codex、DSH、omp、导入来源暂时都填 false。
- 宿主：`tool_result` 块的 payload 在失败时多一个 `"failed": true`。
- App：
  - `SessionContent.toolResult` 多一个 `failed`（默认 false，原有构造不用改）；
  - `ToolCallState` 加 `failed`：行首红点，标签"失败"；
  - 不并进"已调用 n 个工具"的折叠组，单独一行显示。

## Alternatives considered

**历史重建时拿实时更新里的工具状态覆盖。** 最强的理由是不改历史格式，Codex 等也能一起对。没选：实时更新只存在宿主内存和最近的页里，旧会话和导入会话没有；会话记录里本来就有失败标记，丢在解析这一步，补上最直接。

**失败行显示"已拒绝"而不是"失败"。** 最强的理由是更贴切。没选：会话记录只说出错，分不出是被拒、命令报错还是超时；说"失败"不会说错，具体原因在展开后的输出里（这次是"这是只读会话：…"）。

## Consequences

- **收益**：被拒的计划、报错的命令在历史里显示"失败"，与运行中一致。
- **代价与未做**：Codex、DSH、omp 的会话记录也有各自的失败标记，没接，仍按"完成"显示；以后按需要逐家补。
- **验证**：
  - core 新增 `a_refused_claude_tool_stays_failed_in_history`：`is_error` 的结果解析为失败，正常结果的 JSON 里没有 `failed`、读回来是 false；
  - Swift 新增 `testAFailedResultMakesAFailedToolRow`：带 `failed` 的块解析出失败，单独成行、不折叠；
  - 第六轮目检已过（`2026-11-18-1514-aeroric-review-round6`）：上一轮成员会话里的 `ExitPlanMode` 显示红点和"失败"、单独成行，切走再切回不变；同一回合的 Bash、Write 仍是"完成"。
