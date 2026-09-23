# ADR-017：重试的身份是调用方的 operation key，不是 core 的世代计数

- 状态：已接受（Task P2-04 门禁 Q38、Q39）
- 日期：2026-09-22
- 相关：P2-04 验收"同 ID 重试只 spawn 一次"、ADR-016（世代权威与镜像）、INV-03、`src-tauri/src/lib.rs:110-124`

## 背景

第一版把认领键定成 `(taskId, kind, generation)`，其中 generation 是 core 自己的计数。后果是"重试"和"另起一次"在机制上无法区分：每次新调用都会先领一个新号，于是防重试等于防并发——两个窗口点同一个 todo，只有一个能起进程。

把它当成"实现细节"是错的：这是一条产品语义决定。提问后用户明确选择**允许同一任务双开**，并接受先改 `TaskManager`。

## 冲突与代价（实测，不是推测）

`src-tauri/src/lib.rs:110-124` 的 `TaskManager` 全部以 task_id 为键：`pty_masters`、`pty_writers`、`child_handles`、`pending_pty_sizes`、四族 session、`cancelled_tasks`。同一 taskId 起第二个进程会 `insert` 覆盖第一个的句柄——**前一个进程变得无法读写、也无法 kill**（句柄丢了，进程还在跑），两个进程的输出灌进同一个终端缓冲。

因此"双开"在 legacy executor 上不是删一条判断就行，需要先把注册表改成按 `(task_id, generation)` 键。本轮的处置：

- core 侧的认领语义现在就改（改动小、且是纯端口层）；
- **P2-05 的前置阻塞项加一条**：`LegacyTaskExecutor` 接入之前，`TaskManager`（及 P3-01 的 `TaskRegistry`）必须以 `(task_id, generation)` 为键，且取消/退出清理按代回收。在那之前双开只在 core 的测试里成立，接到旧运行时上会漏进程。

## 决策

1. **认领键 = `(taskId, kind, operationKey)`**，operation key 由调用方提供（就是计划 §2.3 请求信封里的 `idempotencyKey`）。同一个键重复投递 = 一次尝试：`AlreadyClaimed`，不写盘、不起进程、不烧号——验收原文"同 ID 重试只 spawn 一次"由此成立，且它约束的是"重试"而不是"并发"。
2. **世代计数仍然每次尝试前进一格**，它是"哪一次运行"的标识：事件带 generation，账本按 generation 判旧。计数不再兼任重试身份。
3. 释放认领有两条路：调用方按自己的 key 释放（`settle`），事件按 generation 释放（`settle_by_generation`）——因为 hook/退出只知道自己属于哪一代，不知道当初是谁提交的。
4. 崩溃对账、终态规则、`is_live` 判定都不因本决策改变；改变的只有"同一个任务能不能有两个在飞的尝试"。

## 决策 5（Q40）：重叠时不落终态，守卫改为"那一代还在不在飞"

一条任务记录只有一个 `status`，而双开后同一任务可以两代同时在跑。规则定为：

- 某一代上报终止事件（exit / done / failed / cancelled）时，先结清**它自己**的认领；
- 只要账本里该任务还有**别的代**在飞，任务行的状态写回 `running`，不写终态、不盖 `completedAt`；
- 最后一代结束时，才落它带来的终态与原因。

这带出一条必要的连带修正：**"比最新号小"不足以判定事件陈旧**。run 1 的退出在 run 2 已开始之后仍然是真事件。守卫改成"那一代已经没有在飞的认领"才算陈旧——已经报过一次结局的代再来事件，照旧丢弃（`a_late_exit_from_the_previous_run_does_not_move_the_task` 仍然绿，因为它在 cancel 时已结清认领）。

`two_distinct_operation_keys_on_one_task_both_reach_the_executor` 钉住全链：两把 key → 两个进程；run 2 退出 → 任务仍是 `running` 且没有结束时间；run 1 退出 → `failed` + 原因 + 结束时间，两条认领都清空。

## P2-05 的阻塞项（本决策新增，不可在本轮悄悄做掉）

`LegacyTaskExecutor` 接入之前，`src-tauri/src/lib.rs:110-124` 的 `TaskManager`（以及 P3-01 的 `TaskRegistry`）必须以 `(task_id, generation)` 为键，取消与退出清理按代回收。不满足这条，双开会在旧运行时上漏出无法 kill 的子进程——这是结构后果，不是偶发 bug。
