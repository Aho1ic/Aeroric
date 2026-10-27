# Agent Note: 任务运行身份是 per-task generation，重试身份是调用方的 operation key

Status: implemented

## Problem

同一个任务可以被重试、被配置切换重新拉起，也允许双开（两个窗口点同一个 todo）。若运行身份只有 `taskId`：旧一代的退出会清理新一代的句柄，同 ID 第二个进程会覆盖第一个的句柄（进程还在跑却无法读写、无法 kill），重试与并发在机制上无法区分，取消与清理也不知道该作用于哪一代。

## Decision

- **世代权威**：`task-operations.json` 中 `OperationLedger` 的 `generations` 表是唯一权威，每次尝试前进一格。任务记录里的 `generation` 只是镜像，`TaskService::commit` 每次写前把它改写成权威值（包括改小、改成 0）。账本不认识的任务从 0 起算；带着账本从未发放过的代号的事件得到 `FutureGeneration` 报错，不静默丢弃。未决认领不镜像。
- **重试身份**：认领键是 `(taskId, kind, operationKey)`，operation key 由调用方提供（请求信封里的 `idempotencyKey`）。同键重复投递 = 同一次尝试，返回 `AlreadyClaimed`，不写盘、不起进程、不烧号。释放有两条路：调用方按 key（`settle`），事件按 generation（`settle_by_generation`）。
- **宿主侧注册表**按 `RunKey = (task_id, generation)` 键（`core/crates/aeroric-core/src/runtime/registry.rs`），运行身份只有这一个来源；`RunKey::UNMINTED` 是保留值。`initial_input_signals` 表内部的序号只是"我登记的那条还在不在"的条目版本戳，其它容器不得把它当运行身份。
- **只带 taskId 的请求**：资源型操作（写输入、resize、会话挂接、启动输入投递）寻址该任务**已注册**的最大 generation；意图型操作（`cancel`、手动完成）作用于全部在飞代。"最新一代"取已注册集合，不取账本计数器当前值。
- **取消/手动完成标记**按任务记一条（`cancelled_tasks` / `manually_completed_tasks` 是 `taskId` 集合），由**最后一个退出的代**销账：某代退出时先从注册表 retire 自己，再判断是否还有别的代在飞；retire 与"读剩余代"在同一把叶子锁内完成。附件目录、事件目录等清理副作用也随销账挪到最后一代。读并消费标记的逻辑收敛为一个宿主函数。
- 崩溃对账把未决 start/resume 改为 `interrupted`，见 [任务写入是意图](2026-09-23-task-writes-are-intents.md)。

## Alternatives considered

- **认领键用 core 自己的 generation**：实现最简单。否掉是因为每次调用都先领新号，"防重试"变成"防并发"，同一任务无法双开。
- **只给句柄三件套等"真会被覆盖的"容器重键**：改动面约小一半。否掉是因为同一张注册表一半按任务、一半按运行，"某容器按不按运行"只能靠注释维持。
- **取消也只寻址最大代**：规则统一。否掉是因为用户点完取消后仍有一代在跑，且没有界面能解释。
- **多代在飞时拒绝只带 taskId 的请求**：歧义显式化。否掉是因为只认 taskId 的客户端会突然冒出按不动的按钮。
- **标记按代记账、意图操作 fan-out 写入**：语义最直白。否掉是因为与"用户撤销的是任务"这一事实的粒度不符；而原样保留"第一个退出者消费"会让后退出那代的正常收尾被判为 `failed`，并删掉仍在运行那代正在写的附件。
- **删掉条目版本戳、统一用 RunKey**：只剩一种身份。否掉是因为该表按 task_id 单值覆盖，序号真正保护的是"迟到的清除别删掉顶替者的登记"，硬替换会造成 prompt 静默丢失。

## Consequences

- **收益**：同任务双开在 core 与宿主两侧都成立；旧代退出不会清理新代；重试至多起一次进程。
- **代价与已知上限**：注册表容器键变宽，跨容器的同一运行操作要按锁序表同时持锁；销账顺序（先 retire、后判断剩余代）是硬约束，写反会让任务停在 running。导出/导入后世代从 0 重计；若将来要求导出带走完整世代，应把 `task-operations.json` 纳入导出格式，而不是让镜像变成权威。
