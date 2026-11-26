# Agent Note: 交办待办时选配置和模型

Status: implemented

## Problem

[待办第一批](./2026-11-15-todos.md)的交办只选 Agent 一族，会话用这一族的默认配置和模型。当时的理由是和定时任务保持一致，需要别的模型可以进会话再换。2026-11-16 复测时，交办出去的会话默认落到 `auto/gpt-6.1-sol`，想换只能进会话、停掉这一轮、换模型再发：交办的那一轮已经白跑了。

当时的代码（`main@1e682f44`）：

- 协议 `todo.assign {id, agent, model?}` 已经收 `model`，但 App 的交办弹窗只给选 Agent，从不传模型。
- 宿主 `schedule::open_session` 走 `task.run`，带 `selectedModel`；`task.run` 不认配置，`NewTask.agent_config_id` 写死 `None`。
- 讨论模式和换模型重跑已经有"配置 → 模型"的选择：配置用会话目录（`session_catalog::for_project`）的 id，宿主用 `launch::config_id_from_catalog` 换成任务记录里的 `agentConfigId`。

## Decision

**交办时选会话目录里的一个配置和它的一个模型；不选配置时行为不变。**

- **协议**：`todo.assign {id, agent?, model?, configurationId?}`。`Assignment` 加可选字段 `configurationId`、`label`（"配置名 · 模型"）。都是新增可选项；todo 不在 `contracts/native/v1` 里。
- **宿主**（`session_live_services/todo.rs`）：带 `configurationId` 时，按待办所在项目读会话目录，由纯函数 `pick_configuration` 校验：
  - 目录里没有 → `INVALID_ARGUMENT`"<id>：配置不存在"；
  - 配置被挡（`blockedReasons` 不空）→ `INVALID_ARGUMENT`"<配置>现在不能用：<原因>"；
  - 模型表不空而模型不在表里 → "不支持模型"；
  - 同时传了 `agent` 而与配置的 Agent 不符 → 拒绝，不猜。
  校验不过不开会话、不记交办。通过后 Agent 取配置的 `agent`，`agentConfigId` 取 `config_id_from_catalog`（内置配置为空，会话用这一族自己的来源）。
- **开会话**（`session_live_services/schedule.rs`）：`open_session` 改收 `Runner { agent, model, agent_config_id }`；`run_params` 拼 `task.run` 的参数，有配置时带 `agentConfigId`。`task.run`（`services.rs`）读这个参数填 `NewTask.agent_config_id`；启动时找不到这个配置照旧报错，不回退。定时任务传 `None`，行为不变。
- **App**：
  - `TodoPage` 新增 `targets`、`loadTargets` 两个闭包，App 用 `sessionConfigurations` 过滤掉被挡的配置、`loadSessionConfigurations` 按待办所属项目加载。
  - 交办弹窗有"配置""模型"两个胶囊选择；配置没有模型表时模型是"默认模型"且不可选。配置列表拿不到（远程项目、还没加载到）时退回只选 Agent。配置列表在弹窗打开后才到时，按默认规则补上选择。
  - 默认值（`TodoText.defaultPick`）：上次交办的配置还在就用它，上次的模型它还提供就沿用，否则取它的第一个模型；没有配置记录时取上次 Agent（或第一个 Agent）的第一个配置。
  - 列表上的"交办 N 次 · 上次 …"优先显示 `label`；弹窗下方的说明改为"会话用选的配置和模型，以及这个 Agent 的默认权限模式"。

## Alternatives considered

**只在弹窗里加模型选择，不选配置。** 最强的理由是改动最小：协议已经收 `model`。没选它：同一个模型可以由不同配置（本机登录、网关、第三方连接）提供，只选模型说不清走哪条连接；复测里换的恰恰是来源。

**交办时让用户选完整的会话设置（权限、思考强度、工作区）。** 最强的理由是和新会话页一致。没选它：这次要解决的是"模型和来源选错只能重跑"，权限沿用 Agent 的默认值是待办第一批定下的；一次加太多选项会让交办这一步变重。

**定时任务一并改。** 最强的理由是两处保持一致。没选它：定时任务是无人值守的规则，配置在保存规则时就该定死，属于另一个改动（规则编辑弹窗、规则格式迁移）；这次只做复测碰到的交办。

## Consequences

- **收益**：交办时选的配置和模型就是新会话的配置和模型（任务记录的 `agentConfigId`、`selectedModel`）；选不了的配置在开会话之前就被拒绝并说明原因。
- **代价**：交办弹窗多了两个选择；远程项目仍然只能选 Agent（会话目录拿不到时的退路）。定时任务仍然只选 Agent。
- **验证**：
  - 宿主单测 `todo::tests`：配置给出 Agent、来源和标签；内置配置没有 `agentConfigId`；不存在、被挡、模型不支持、Agent 不符都被拒；不带配置时和以前一样。`schedule::tests`：`task.run` 参数带上模型和 `agentConfigId`。
  - 宿主端到端 `a_todo_is_handed_out_only_with_a_configuration_that_can_run`：不存在的配置、被挡的 `omp` 都返回 `INVALID_ARGUMENT` 且没开出会话；`task.run` 带的 `agentConfigId` 写进任务记录。原有的待办端到端测试照常通过。
  - Swift `TodoTests`：默认选择的几种情况、请求参数、解码 `configurationId`/`label`、列表文字和说明。
  - 没有在真实网关配置上走通一次"交办 → 会话用这个配置跑起来"（测试宿主里没有可用的网关配置）；界面未目检，记在路线文档 §8 待目检清单。
