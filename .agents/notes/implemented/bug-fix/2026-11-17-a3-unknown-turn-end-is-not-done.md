# Agent Note: A3 没见过的结束状态不算完成

Status: implemented

## Problem

[批次 A](./2026-11-15-adoption-a-truthful-state.md) 的 A3。2026-11-17 在 `main@9603b442` 加未提交的"交办选配置""ACP 握手诊断"改动上复核：

- **Codex** `core/crates/aeroric-core/src/sessions/live/codex/map.rs:70–74`：`turn/completed` 的 `turn.status` 只认 `interrupted`、`failed`，其余值和字段缺失都记 `Completed`。Codex app-server 0.167.0 的 Schema 快照里 `TurnStatus` 是 `completed | interrupted | failed | inProgress`，`status` 必填；`inProgress` 出现在 `turn/completed` 上本身就不对，现在也记成完成。
- **Codex 子代理** `codex/driver.rs:872–918`：子代理的 `turn/completed` 缺状态时默认 `completed`，未知值也在卡片上写"完成"。
- **ACP**（DSH 和其他 ACP Agent）`acp/driver.rs:1285–1302`：`stopReason` 缺失默认 `end_turn`；`cancelled` 记中断、`refusal` 记失败，其余（含规范之外的值）都记完成。`max_tokens`、`max_turn_requests` 记完成并另发一条警告。ACP 规范里 `PromptResponse.stopReason` 必填，取值 `end_turn | max_tokens | max_turn_requests | refusal | cancelled`。
- **OMP** `omp/driver.rs:874–883`、`:1063–1078`：批次笔记的复核表说 OMP 的结束状态由驱动自己传入，没有外部未知值。再确认后**不成立**：`prompt_result.status` 来自 omp 的帧，缺失时默认 `completed`，`aborted`、`error` 以外的值都记完成。2026-11-17 在 Mac 上读了已安装 omp 二进制里打包的协议类型：`PromptStatus` 是 `'completed' | 'aborted' | 'error'`。驱动自己补的 `turn_result("completed", …)`（`agentInvoked: false`、omp 自己起的回合结束于 `agent_end`）是驱动确知的完成，不受影响。
- **Claude** `claude/map.rs:497–501`：`result` 行 `is_error` 为真或 `subtype` 不是 `success` 都算失败，没有这个兜底。

后果：记成 `Completed` 的回合会跑项目检查（`turn_changes.rs:205`）、委派验收（`delegation/mod.rs:1142`）和审查循环（`review/mod.rs:312`、`:723`），任务状态写"已完成"。回合分级（[回合分级](../feature/2026-11-11-turn-outcomes.md)）读的是 Agent 自己的会话文件，Agent 用没见过的状态结束时，文件里写的是什么我们并不知道，所以历史里这一轮多半也显示"完成"。

## Decision

**只有 Agent 明确说完成，才算完成。没见过的结束状态和缺失的结束状态记"失败"，原始值留在回合结束更新里，历史分级标"没跑成 · 结束状态不明"。**

### 映射

| 族 | 完成 | 中断 | 失败（已知） | 没见过 / 缺失 → 失败并带原值 |
|---|---|---|---|---|
| Codex | `completed` | `interrupted` | `failed` | 其他值（含 `inProgress`）、缺失 |
| ACP | `end_turn` | `cancelled`（或用户点了停止） | `refusal`、`max_tokens`、`max_turn_requests`、请求出错 | 其他值、缺失 |
| OMP | `completed` | `aborted`（或用户点了停止） | `error` | 其他值、缺失 |

- 原因文字：值不是空的写"<Agent> 返回了没见过的结束状态：<原值>"；缺失写"<Agent> 没有说这一轮是怎么结束的"。
- 用户已经点了停止的，仍记中断：停止的意图比 Agent 回的状态更确定。
- Codex 子代理卡片：只有 `completed` 写"完成"；其余写"结束状态不明（<原值>）"，卡片记失败。

### ACP 的 `max_tokens`、`max_turn_requests` 记失败，不记"没跑成"

- 这两个值在 ACP 规范之内，不是"没见过"，不带原值。
- 它们的意思是 Agent 撞到单轮上限、话没说完就停了。按完成处理会对半截的结果跑检查和验收；验收可能误判通过，这正是 A 批要挡的"把不知道算成功"。
- Claude 的同类情况（`result` 的 `subtype` 为 `error_max_turns`）已经记失败。ACP 跟它对齐。
- 不归"没跑成"：这一轮确实跑了、可能改了文件，"不算模型答错"的说法不成立。也不归"已中断"：分级里"已中断"专指用户点了停止。
- 原来那条单独的警告不再发：失败的回合结束本身带着"<Agent> 到了单轮上限（<值>），这一轮没做完"，界面会显示它。

### 协议（只加不改）

- `SessionUpdateKind::TurnEnded` 新增可选 `raw_status`（线上 `rawStatus`）：只在结束状态没见过或缺失时出现；缺失记空字符串，不是字符串的值记它的 JSON 文本。`reason` 仍是 `failed`，`error` 是上面的原因文字。
- 不给 `TurnEndReason` 加第四个值（owner 2026-11-15 定的，见[总纲](../../proposed/architecture/2026-11-15-research-adoption-roadmap.md) §7 第 3 条）。
- `NotRunReason` 新增 `unknownEnd`。它只出现在 `session.history` 的统计里（不在 `contracts/` 的 schema 中），Swift 对没见过的原因显示"出错"，旧客户端不受影响。

### 分级要读得到：记在任务记录上

实时更新只在内存里，宿主重启就没了；分级读的是会话文件、回合账本和任务记录。做法照 `hostRestarts`（宿主重启打断的回合）：

- 宿主的 `TaskServiceSink` 看到带 `rawStatus` 的 `TurnEnded`，就在任务记录上追加一条 `unknownTurnEnds: [{at, status, message}]`，最多留 20 条。写入走 `TaskService` 的单写者路径。
- `session.history` 组装统计时，`turn_outcome::mark_unknown_ends` 把 `at` 之前最后开始的那一轮标成 `notRun { reason: unknownEnd, message }`，同时置 `failed`、清掉 `interrupted`。这一轮已经从会话文件里读出 API 错误的，保留更具体的那个原因。
- 回合分级的"产品失败"判断照旧跳过没跑成的回合。

### macOS

- `LiveUpdateKind.turnEnded` 带上 `rawStatus`。实时界面照旧把失败回合的原因显示成一条提示。
- `TurnBillText.notRunReason("unknownEnd")` 写"结束状态不明"；悬停说明改为"Agent 报了 Aeroric 不认识的结束状态，这一轮不算完成，不跑检查和验收"，并带原因文字。

### 同主题笔记审计

| 笔记 | 分类 | 处理 |
|---|---|---|
| [批次 A](./2026-11-15-adoption-a-truthful-state.md) | 上级方案 | 原地补：OMP 复核结论更正；A3 状态回写路线文档 §8 |
| [回合分级](../feature/2026-11-11-turn-outcomes.md) | 被本篇补充 | 原地补"结束状态不明"这一类和来源 |
| [ACP 方言](../feature/2026-11-08-acp-dialects.md) | 被本篇修改 | 原地改 `stopReason` 映射那一条 |
| [A1](./2026-11-16-a1-delegation-card-follows-delivery.md)、[A2](./2026-11-16-a2-approval-answer-must-arrive.md) | 无关 | 不动 |

## Alternatives considered

**给 `TurnEndReason` 加 `Unknown`。** 最强的理由是语义最准，消费方能分清"失败"和"不知道"。没选：owner 已定；它被驱动、检查、委派、审查、讨论、登录状态多处穷举匹配，而这些地方对"不知道"的处理和失败一样（不检查、不验收、可重试）。区分靠 `rawStatus` 就够。

**只改映射，不把未知结束写进任务记录。** 最强的理由是改动只在驱动里，没有新的落盘字段。没选：分级读会话文件，Agent 用没见过的状态结束时文件里写什么不可知；不落盘的话，实时界面写"失败"，重新打开会话却显示"完成"，两处说法打架。任务记录上已经有同样用途的 `hostRestarts`，照着加不引入新的存储。

**新开一个文件记"非正常结束"。** 最强的理由是不碰任务记录的写路径。没选：总纲要求新字段加在现有对象上；任务记录已有单写者和修订号，另开文件要再处理一遍并发和清理。

**ACP 的 `max_tokens` 维持完成加警告。** 最强的理由是这一轮确实产出了内容，可能大半已经做完，跑检查还能给出信息。没选：检查和验收的前提是 Agent 认为做完了；撞上限是 Agent 没做完，验收误判通过的代价高于少跑一次检查。用户可以让它继续，下一轮正常结束后检查照常跑。

**OMP 的 `status` 缺失仍按完成处理（兼容老版本）。** 最强的理由是老版本 omp 若不带 `status`，改后每一轮都会变成失败。没选：已安装的 omp 协议类型里 `status` 是 `prompt_result` 的字段，Aeroric 对 omp 有最低版本检查；真遇到老版本，界面会写"没有说这一轮是怎么结束的"，用户看得见、能追，比静默记完成好。

## Consequences

- **收益**：
  - 只有 Agent 明确说完成的回合才跑项目检查、委派验收和审查循环；三族（Codex、ACP、omp）都不再把"不知道怎么结束的"算成完成。
  - 原值保留在 `rawStatus` 和任务记录的 `unknownTurnEnds` 里，上游新增状态时能直接看到是哪个值。
  - 实时界面和重新打开后的历史说法一致：都是失败 / 没跑成，原因相同。
- **代价**：
  - 上游新增表示成功的状态会先被记成"没跑成 · 结束状态不明"，要加一行映射才恢复。
  - 老版本 omp 若不带 `status`，每一轮都会显示"omp 没有说这一轮是怎么结束的"。
  - DSH 撞上单轮上限的回合不再跑检查，用户要让它继续，下一轮正常结束后才检查。
  - 未知结束按"结束时间之前最后开始的一轮"对回合，依赖 Aeroric 和 Agent 用同一台机器的时钟。
- **验证**：
  - core 单测：
    - `codex::map` 映射覆盖 `completed`、`failed`、`interrupted`、缺失、空字符串、`null`、`cancelled`、`inProgress`、任意新值、数字，以及 Codex 自带错误时原因的拼接；
    - Codex 子代理卡片：`paused` 写"结束状态不明（paused）"，缺失写"结束状态不明"，卡片记失败；
    - ACP `turn_outcome` 覆盖 `end_turn`、`cancelled`、`refusal`、`max_tokens`、`max_turn_requests`、缺失、空字符串、任意新值、数字、请求出错、用户已点停止；另有一条驱动测试：`stopReason: paused` 让任务失败、`TurnEnded` 带 `rawStatus`；
    - omp `prompt_outcome` 覆盖 `completed`、`aborted`、`error`、缺失、`cancelled`、`cost_cap`、用户已点停止；另有一条驱动测试：`status: paused` 让任务失败；
    - `turn_changes`：真实 git 仓库里改了文件的两轮未知结束（`paused`、空）都不启动检查，紧接着明确完成的一轮启动检查；
    - `turn_outcome::mark_unknown_ends`：对上正确的回合、清掉中断和产品失败、不覆盖会话文件读出的 API 错误；任务记录字段解析跳过坏条目；
    - reducer：`unknownTurnEnds` 追加、最多 20 条、随后的失败状态不清掉它。
  - host 集成测试 `an_unknown_end_state_fails_the_turn_and_its_history_says_not_run`：假 Codex 回 `status: paused`，任务失败、原因如上、任务记录有一条 `unknownTurnEnds`；会话文件写的是 `task_complete`，`session.history` 仍把那一轮标成 `notRun.reason = unknownEnd`。
  - Swift：契约夹具多一条带 `rawStatus` 的 `turnEnded` 并能解码；`TurnBillTests` 覆盖"没跑成 · 结束状态不明"的文字、颜色和悬停说明。
  - 2026-11-17 在 Mac 上：`cargo test --workspace --no-fail-fast` 3730 个通过、1 个失败（`aeroric-cli --test agent_config_roots` 的 `serve ready timeout`，5 秒内宿主没就绪；单独重跑 3 次过 2 次，与本改动无关）；`cargo fmt --check`、`cargo clippy --workspace --all-targets -D warnings` 通过；`swift test --package-path macos/AeroricKit` 1099 个用例 0 失败（1 个跳过）；`pnpm verify-notes` 通过。`export-schema` 只改了 `session-live.schema.json`（加 `rawStatus`）和它的夹具；顺带出现的 `session-submission.schema.json` 既有漂移已还原。
  - 界面没有目检，记在路线文档 §8 待目检清单。
- **2026-11-18 补强**：Claude 缺 `subtype`、omp 不带 `id` 的 `prompt_result`、未知结束落错回合、重启没清 `product_failure` 四处补上（B1-7），见 [全量审查 Batch 1](./2026-11-18-review-batch-1-core-gaps.md)。
