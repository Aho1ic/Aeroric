# ADR-007：v1 事件 topic 自成一集，38 条旧通道经显式映射表桥接

- 状态：**accepted（用户在 P2-01 协议门禁的 frontier 追问中拍板，2026-09-22）**
- 实施进度（同日）：**表已建、校验已成门禁**。`docs/native/event-topics.json` 39 条 topic 覆盖 38 条旧通道（外加宿主合成的 `terminal.gap`），由 `export-schema` 校验并生成 `contracts/native/v1/event-topics.schema.json`，纳入 `native:contracts --check`；Rust/Swift/TS 各有一条断言逐字核对覆盖与"未决不许装作已决"。**36 条仍是 `pending: P3-04`**，只有 §2.3 已定名的 3 条是 `mapped`。实测还纠正了一处我自己写错的东西：`direction` 起初由旧通道证据直接充当 v1 方向，但 `shell-output` 在旧实现里根本没有 Rust 侧 emitter（11 条属"unproven"），而 v1 的 `terminal.output` 一定要由宿主发——现在拆成 `legacyDirection`（证据）与 `plannedDirection`（v1 计划），两者不再互相冒充。
- 日期：2026-09-22
- 关联：ADR-002／ADR-006（方法面）、`MIGRATION_PLAN.md` §2.3（示例帧）、P2-01（`events.schema.json`）、P3-04（事件桥接）、P5-01/P5-02（终端与原生会话）

## 背景：同一个面上有两套拼法，谁都没写过映射

- `contracts/native/v1/events.schema.json` 现在发布的是**清点出来的 38 条旧 Tauri 通道名**（`task-status`、`shell-output`、`dsh-host-session-added`…，连字符拼法，来自 `inventory.mjs` 对 `listen<…>` / `emit(…)` 的静态抽取）。
- §2.3 的示例帧用的是 `task.updated`、`terminal.output`（点号拼法）。
- P2-01 写 fixtures 时，这两套第一次同时出现在一个可执行工件里：`terminal.output` 在 38 条里**不存在**。我当时的处理是加一张 `PLAN_SOURCED_EVENTS` 白名单（`task.updated` / `terminal.output` / `terminal.gap`，各自注明出自 §2.3 行号），并让生成器**拒绝任何没有出处的名字**进 fixtures。这把「我随口编的名字」挡住了，但没有回答「v1 的事件面到底叫什么」。

这不是命名洁癖：事件名和旧命令名一样，**改名就是协议变更**（v1 一旦冻结，第三方客户端与旧数据目录都按名字订阅）。现在只有 3 个名字有出处，返工成本是此后所有 topic 都定下来之前的最低点。

## 决策

1. **v1 事件 topic 是 v1 自己的一套点号命名空间**，规则与方法面同构：`<域>.<名>`（小写段、下划线标识符），不沿用连字符旧通道名。
2. **38 条旧通道不直接成为 v1 topic**。桥接层（P3-04）负责把旧通道事件映射到 v1 topic 后再发布，映射表是可机械校验的工件：每条 topic 必须写明
   - `legacyChannels`：它桥接哪些旧通道（可以多对一）；
   - `direction`：core→client、client→core 还是双向；
   - `payload`：是原样搬运还是由 core 重塑（重塑必须写明字段来源）；
   - `status`：`mapped` / `not-forwarded`（带原因） / `pending: <既有 Task>`。
3. **每条旧通道必须被恰好一条 topic 覆盖，或被明确标为不转发**。「忘记映射」和「决定不转发」在工件上必须长得不一样——这正是 P1-02 给命令面做 `category` 三分（`domain_rpc` / `platform_specific` / `host_local_os_ui`）时用过的同一手法。
4. 已定名的三条以 §2.3 为出处：`task.updated`、`terminal.output`、`terminal.gap`；其余 topic 的名字在 P3-04 之前逐条落入映射表，**不得由代码随手 `emit("x-y")` 出去**。
5. 未识别的 topic 仍**不得让事件流失败**（`events.schema.json` 的 `contract.unknownEvent` 已如此规定），保持前向兼容。

## 备选与被否决的理由

- **原样沿用 38 个旧名**：省掉映射表，但 §2.3 的示例帧立刻变成非法名字（要反过来改文档），而且带连字符的通道名与点号方法名混在一个协议里；`shell-output` 这类名字在 Swift/TS 里也不能作为枚举 case 直接落地。
- **留给 P5-01 再定**：被否决。事件 topic 与方法名同类——越晚定返工越大，而且 P3-04 的桥接层没有表就无法验收。

## 后果

- 交付物：`docs/native/event-topics.json`（人工维护的映射源）→ 由 `export-schema` 生成 `contracts/native/v1/event-topics.schema.json`，并纳入 `native:contracts --check` 的漂移门禁。**已完成**；另外两条 §2.3 定名的 topic 其 payload 实测**不是**原样搬运（`shell-output` 是 `{shell_id, data}`，v1 要 `{taskId, generation, offset, nextOffset, bytesBase64}`；`task-status` 是 `{task_id, status}`，v1 要 `{taskId, generation, revision}`），所以两行都写成 `reshaped:` 并点名由哪个任务补上 generation/revision，校验器禁止 `mapped` 行用 `verbatim` 蒙过去。
- `events.schema.json` 的角色收窄为「旧通道清点」（emitters/listeners/orphan/`disposition`），v1 的对外 topic 由新工件承担；两者都要过覆盖检查。
- 归属：P3-04 之前由 P2-01 起持续补全；`pending` 条目在进入 P3-04 时必须清零或转为 `not-forwarded` + 原因。
- 与 ADR-006 同一原则：**映射表内的改动不是协议变更，发布/删除 topic 才是**。

## 修正案（2026-09-28，T1 由用户拍板 A：按发射点改派）

上面「后果」一节最后一条把 36 个 `pending` 全押给 P3-04（"进入 P3-04 时必须清零或转为 not-forwarded"）。
这条**前提被实测推翻**：按 `command-map.json` 的 `events[].emitters[]` 实际发射点统计，真属 sessions/usage 面的
只有 2 条（`task.session` @ `core_adapter.rs:538`、`usage.statistics_updated` @ `usage_index.rs:1047`），
其余 34 条的发射点分布在 `dsh_webui`(11) / `dsh-host-*` 动态发射(8) / `remote`(5) / `database`(2) /
`notebook`(2) / `report`(1) / `lsp`(1) / `agent_ops`+`agent_tools`(2) / `app-*` 动态(2)。逐条证据与可重跑探针见
`docs/native/extraction/event-topics-pending.md`。

改派后的形状：**每行 `pending:` 的 Task 就是它的接线责任人——发射点在哪，桥就谁接**。
`event-topics.json` 的 `derivation.ownerOfPending` 已从"P3-04 wires the bridge"改成这条规则，
`status`/`plannedDirection`/`payload` 三列同批改写，`contracts/native/v1/event-topics.schema.json` 随之重生成
（`--write` 报 `1 changed`，`--check` 逐字节复绿；三方断言的计数 `topics 39 / mapped 3 / pending 36 /
legacyDirectionUnproven 11` 全部不变，所以 `protocol` 门禁 6 case／98 用例仍绿——**改派不是重新计数**）。

不变的部分，逐条强调：topic 词表、派生规则、`mapped` 行必须带文档行号 + `verbatim|reshaped:` 的判定、
以及"改映射表不是协议变更、发布/删除 topic 才是"（ADR-006 同一原则）**一律未动**。校验器仍禁止 `pending`
行提前装作已决，所以改派**不会**把未决伪装成闭合；它只把"谁欠这笔"写对。

一条连带后果：`G01` 的 `task_ids` 原写 `P2-01, P3-05`，而其 `agent-tool-install-progress` 一行的发射点在
`agent_tools.rs:606`（属 P8-02 的 Input 面 `agent_tools/`），事件级 note 又点名 P3-02 处置——两份工件各指一个
Task。本次按新规则把 `agent.operation_changed` / `agent.tool_install_progress` 两条改派 **P8-02**，
`G01` 的"要么有订阅者要么删除"仍需在 P8-02 出口逐条交代；该矛盾已在 `MIGRATION_PLAN.md` 补记一百十二 记账。
