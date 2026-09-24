# ADR-023：`task-status` / `task-session` 的唯一发射者是 core，发射面不收窄到"只换数据源"

- 状态：已接受（Task P2-05 门禁，Q50）
- 日期：2026-09-23
- 相关：ADR-022（保存队列只排队意图）、ADR-007（事件 topic 命名空间）、P2-05 Action 4（禁止 executor 反过来调 command wrapper）

## 背景

移交 writer 之后，"谁发这两个事件"决定了 P2-05 验收里"没有双事件/事件回环"是否可能是真的。侦察得到的事实（并修正一次我自己早先的误记——不是 24 处）：

- `task-status`：字面量发射点 2 处，另有包装函数 `emit_task_status`（`session.rs:62-67`），其调用点 4 处（含 `emit_active_task_status` 的 3 处），全部在 `session.rs` 内。
- `task-session`：6 处，分布在 `session.rs`（3）、`session_omp.rs`、`session_dsh.rs`、`event_watcher.rs`。
- `remote/events_bridge.rs:13` 把这两个名字**原样转发**给远程客户端——它只认事件名，不认发射者。
- 终端字节流不走 `emit`（`pty.rs` 的 `OutputSink::Channel`），因此不在这条决策的范围内，也不应被顺手改道。

## 决策

1. **这两类事件由 core 唯一发射**：上述约 8 处发射点改为"调用 core 服务，由 `TauriEventSink` 统一发出既有事件名"。事件名与 payload 形状**不变**，所以 `useAppTauriEvents.ts` 与 `events_bridge.rs` 都不需要改——这是刻意的：迁移期同时改数据源和改事件契约，出问题时无法归因。
2. **范围只到这两个家族**。其余 ~83 处 `.emit(` 属于不相关子系统（数据库传输、远程请求、终端等），P2-05 不动，也不引入"通用 EventSink"去顺手统一它们。
3. **终端输出保持旁路**。原始 PTY 字节流不进 core 的事件通道，也不因为"事件都走 core 了"就跟着改道（INV-05/INV-12 同源）。

## 后果

- 被否方案② "旧发射点保持唯一发射者、core 事件只喂新宿主"其实不成立：core 自己决定的变更（崩溃对账、幂等重试、被基线拒绝的写）旧发射点无从得知，结果要么漏事件，要么让 executor 回头去调 command wrapper —— 正是 Action 4 禁止的递归方向。
- 被否方案③ "两边都发、前端按 (generation, revision) 幂等收敛"改动最小，但它把验收要防的东西变成了长期机制；幂等去重一旦成为"合法"的兜底，之后每个新事件都会先想到去重而不是找出发射者该是谁。
- 这条决策使"事件归属"变成可机械检查的东西：门禁模块可以断言 `session.rs`/`session_omp.rs`/`session_dsh.rs`/`event_watcher.rs` 里不再出现 `"task-status"`/`"task-session"` 字面量，而 core 侧必须有。漏改一处就是红的，而不是靠 review 的眼睛。

## 落实（2026-09-24，步骤 F 三批发完之后）

判据已经成立并且可核对：`contracts/native/v1/events.schema.json` 与 `command-map.json` 都如实记着
**`task-status` emitters = 1、`task-session` emitters = 1**，唯一那两行是 `core_adapter.rs:503/509`
的 sink。门禁在 `adapter-invariants.mjs` 的 ADR-023 规则里逐家族断言"恰好一行、且必须在
core_adapter.rs 里"，配 4 条负例（多一个发射点 / sink 看不见 / 走 `event_names::` 常量发 /
第二个家族同样受管），所以"两个数字各等于 1"不是某次数出来的结论而是会红的断言。

原本被否的方案②担心的递归没有发生：新增的两个入口
（`report_task_status` / `report_task_session`）都只往 core 走（observer → reducer → sink），
executor 从不回头调 command wrapper，Action 4 的方向约束由 `core-observer-does-not-write-yet`
那条 absent 守卫另外钉住（announce 路径不得提交）。

三条与"载荷形状不变"承诺相关的实况，写在这里而不是埋在 diff 里：

1. **一处键消失**：`event_watcher.rs` 过去在 `task-status` 上多发一个 `approval` 键。全仓无读取点
   （前端解构 `{task_id, status, failure_reason?}`，审批对话框走 dsh 自己的 `dsh-approval-*` 事件），
   `RemoteState.approvals` 的登记/清除照旧执行。少的是键，不是事件。
2. **`failure_reason` 必须先补端口**（补记四十二），否则照原文删就会静默丢掉失败原因。
3. **`task-session` 走的是端口上的第二个方法 `report_session`，不是 `observe`**：它没有状态转移可判，
   把"转录在哪"塞进 reducer 会造出一个假的会话状态机。本 ADR 决策 1 原话设想的
   `apply_resolved_session` 那条写路径**尚未接上**——它需要 store 切换才能提交（与状态观察的
   commit 同批，见 ADR-035 与任务 #15 的收尾说明）。今天完成的只是"唯一发射者"这一半。
