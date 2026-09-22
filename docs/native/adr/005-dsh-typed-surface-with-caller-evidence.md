# ADR-005：DSH 能力面全量 typed 入表，零调用者逐条交代证据

- 状态：**accepted（用户在 Phase 1 Gate 拷问中拍板）**
- 日期：2026-09-22
- 关联：ADR-002（命名）、ADR-003（平台面入协议 + 能力门控）、`MIGRATION_PLAN.md` P3-05 Action 4、G17、§6.1（区分 mock 通过 vs 真实可用）、§4.2（不得把未回答的问题写成已通过）

## 原先的问法是错的（留档，因为它已经进过别的文档）

G17 被表述为"42 个 typed core 方法 vs 一个 mux/gateway 服务"。实测否定了这个二选一：

1. `remote_mux.rs`（991 行）**没有任何 `#[tauri::command]`，也不是按方法名分发的路由**——它的公开面是客户端 `connect / is_alive / open / next / remote_mux_url / parse_remote_event_item / remote_event_result_args`。它不能"替代"typed 方法，因为它根本不持有那张分发表。
2. 41 条零证据命令与上游方法名**零匹配**（41 条的尾名对上游 117 个不重复名字做后缀比对，命中 0），所以它们不是"上游 RPC 的具名包装"，而是 Aeroric 自己的 DSH 宿主能力：sessions（archive/fork/rename/search/list/move/select-preset）、workspaces、goals（create/edit/pause/resume/complete/clear）、subagents（prompt/interrupt/history）、settings、credentials、LLM 发现、webui 生命周期。
3. `MIGRATION_PLAN.md` P3-05 Action 4 本来就要求"把 DSH command inventory 每条映射为 RPC/capability：history/search/fork/archive/preset/queue/goal/jobs/questions/attachments/log export 等不能只迁 prompt"——一个 mux passthrough 无法满足这条。

真正待决的不是形状，而是**证据要求**。

## 决策

1. **66 条 `dsh_webui::*` 全部按 ADR-002 规则冻结为 typed 方法 + capability 条目**（服务面 `dsh`）。
2. 其中 **41 条零证据方法**在 `command-map.json` 上带 `caller_evidence` 字段，取值只有两种：
   - `proven: <测试或调用点证据>` —— 由 SwiftUI/CLI/mobile 实际消费并有测试；
   - `pending: <task id>` —— 尚未有调用者，**挂在具体 Task 上**。
   `inventory --check` 断言：任何 typed 方法不得两样皆无；`pending` 必须指向存在的 Task。
3. P3-05 的验收口径变为逐条交代：`pending` 项要么转成 `proven`，要么在评审后**显式删除**（删除需要单独批准，且是协议变更）。**不允许**以"schema 里有、门禁绿"充当已验收。
4. 计数口径统一措辞：上游清单是 **198 条目 / 117 个不同名字**（另有 48 项显式 RETIRED，不计能力面）。此前文档写"198 names/235 names"都不够准确。

## 被否决的方案

- **typed 全量但不追证据**：41 条会以"有 schema、无调用者、门禁全绿"的长期状态存在，正是本次一路在修的假绿形状。
- **只冻结 25 条有证据的**：协议面只含被证明的部分，但每次补充都是协议变更，客户端与 relay 的能力声明随之抖动，且直接违反 P3-05 Action 4。

## 后果

- `methods.schema.json` 冻结时 DSH 面完整（66 条），`AgentCapabilities` 需要为每条给出可声明粒度。
- G17 从"待定形态"变为"逐条证据待补"，可以进 Task 清单被跟踪，而不是悬在决策层。
- `caller_evidence` 成为第四个受门禁约束的归属类字段（`category`、`rpc_method`、`check_module` 之后）。
