# ADR-007：v1 事件 topic 自成一集，38 条旧通道经显式映射表桥接

- 状态：**accepted（用户在 P2-01 协议门禁的 frontier 追问中拍板，2026-09-22）**
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

- 交付物：`docs/native/event-topics.json`（人工维护的映射源）→ 由 `export-schema` 生成 `contracts/native/v1/event-topics.schema.json`，并纳入 `native:contracts --check` 的漂移门禁。
- `events.schema.json` 的角色收窄为「旧通道清点」（emitters/listeners/orphan/`disposition`），v1 的对外 topic 由新工件承担；两者都要过覆盖检查。
- 归属：P3-04 之前由 P2-01 起持续补全；`pending` 条目在进入 P3-04 时必须清零或转为 `not-forwarded` + 原因。
- 与 ADR-006 同一原则：**映射表内的改动不是协议变更，发布/删除 topic 才是**。
