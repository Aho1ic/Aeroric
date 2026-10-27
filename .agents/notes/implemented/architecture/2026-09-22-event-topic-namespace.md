# Agent Note: v1 事件 topic 自成点号命名空间，旧通道经可校验映射表桥接

Status: implemented

## Problem

旧桌面端有 38 条连字符拼法的事件通道（`task-status`、`shell-output`…），协议示例帧却用点号拼法（`task.updated`）。事件名与方法名一样，改名即协议变更；不规定 v1 事件面叫什么、每条旧通道去了哪，"忘记映射"与"决定不转发"在工件上就无法区分。

## Decision

- v1 事件 topic 是独立的点号命名空间：`<域>.<名>`，小写段与下划线，规则与方法面同构。代码不得随手 `emit("x-y")` 一个未登记的名字。
- 映射源是人工维护的 `docs/native/event-topics.json`。每条 topic 写明 `legacyChannels`、`direction`、`payload`（原样或重塑，重塑须写字段来源）与 `status`。`status` 只能是：
  - `mapped`：`source` 必须指向一处文档出处（含 `§`）；
  - `pending: <迁移任务 id>`：任务 id 形如 `P3-05` / `AP7-1`，发射点在哪个任务里，桥就归谁接；
  - `not-forwarded: <原因>`。
- 每条旧通道必须被恰好一条 topic 覆盖或显式标为不转发。`export-schema` 校验上述规则并生成 `contracts/native/v1/event-topics.schema.json`，纳入 `native:contracts --check`；Rust/Swift/TS 各有一条断言核对计数。
- 未识别的 topic 不得让事件流失败（`events.schema.json` 的 `contract.unknownEvent`），保持前向兼容。
- 映射表内的改动不是协议变更；发布或删除 topic 才是。

## Alternatives considered

- **原样沿用 38 个旧名**：省掉映射表。否掉是因为连字符名与点号方法名混在同一协议里，`shell-output` 这类名字也无法直接作为 Swift/TS 枚举 case。
- **推迟到界面重做时再定名**：先做功能。否掉是因为 topic 与方法名同类，越晚定返工越大，桥接层没有表就无法验收。
- **`pending` 行与迁移计划文档逐条对账**（要求计划里存在 `### Task <id>` 标题）：能证明任务真实存在。迁移计划文档整体退役后这条对账失去对象，现只校验任务 id 的格式；任务是否已关闭由 `event-topics.json` 自身的 `status` 演进体现。

## Consequences

- **收益**：事件面的每个名字都有出处或明确的待办归属；`pending` 不能装作已决。
- **代价与已知上限**：任务 id 只校验格式，写错一个不存在的 id 不会被门禁发现。若 `pending` 行长期不动，应在对应子系统的改动里转为 `mapped` 或 `not-forwarded`，而不是放宽校验。
