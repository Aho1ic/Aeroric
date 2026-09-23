# ADR-018：被对账截断的运行需要用户确认，确认前只挡"续跑"

- 状态：已接受（Task P2-04 门禁 Q42、Q43）
- 日期：2026-09-22
- 相关：ADR-017（重叠运行）、ADR-013（`interrupted` 不是终态）、P2-04 Action 3（崩溃夹在步骤中间时对账）

## 背景

计划要求"崩溃夹在步骤中时下次启动对账为 interrupted/可确认状态，不能无条件重新 spawn"。第一版实现对账后直接把任务交给用户续跑——用户可以点"继续"，core 就从上次认领的世代接着起进程，**没有任何一次交互说明上一次是被截断的**。

提问后用户选择：**需要确认**。

## 形状约束（事实，不是偏好）

"待确认"不能做成一个新的 `TaskStatus`：9 值状态词表同时写在
`contracts/native/v1/envelope.schema.json`、`macos/AeroricKit/Sources/AeroricProtocol/ProtocolModels.swift:11`、`src/lib/nativeHostProtocol.ts:49` 三处，并按 ADR-009 视为 v1 冻结面——加值是协议变更。

因此确认状态不占词汇表，而是账本里的一个条目：

```
task-operations.json → awaitingAcknowledgement: [{ taskId, generation, sinceMs }]
```

与认领、世代计数同文件、同一次写发布（core 独占，旧客户端不读，见 `legacy-client-ignores-core-exclusive-files` 门禁）。

## 决策

1. 对账把未决 start/resume 改成 `interrupted`，并留下确认条目；**不**重新 spawn。
2. **确认前只挡 `resume`**，返回 `TaskError::AwaitingAcknowledgement`。`start`、`cancel`、`complete`、改配置、删除都照旧可行——Q43 的取舍。
3. `start` 会顺手清掉该任务的确认条目：**从头开始本身就是对"上次被打断"的确认**。继续卡住只会让用户连下一次续跑都做不了。
4. 删除任务时一并清掉条目，不留悬空标记。
5. 条目必须**跨进程存活**：它是账本的一部分，重启后仍然挡得住 resume（`an_unresolved_start_becomes_interrupted_..._restarted` 覆盖了崩溃→重启→拒续→新起→再崩溃这条链）。
6. 顺带修掉实现缺陷：对账**不再**给 `interrupted` 盖 `completedAt`。P2-03 的规则是"离开终态要清空结束时间"，而 `interrupted` 从来不是终态；盖上去等于给一条没结束的任务写了结束时间。

## 后果

- 这是对**当前产品行为的一处有意改变**：旧客户端今天允许直接续跑 `interrupted` 任务。因此它是一条声明式分歧，不只是 core 内部决定；在旧客户端仍自己走路径的窗口里该规则不生效，等 P2-05 把 mutations 收到 `TaskService` 后面才真正贯通——这条写进 P2-05 的验收。
- UI 侧需要一个确认入口（Swift/React 各一个），并把 `awaiting_acknowledgement()` 的读取挂到列表上：归 P5-03（React）与 Phase 4/5 的原生侧；core 已提供读取与 `acknowledge()` 两个端口，宿主不需要重新发明。
- 契约面（wire）不变：状态词表不动、错误码不加——`AwaitingAcknowledgement` 由 adapter 映射到既有 `REVISION_CONFLICT`/`BUSY` 之外的哪一个，属 P2-05 的映射决定，本 ADR 不预先指定。
