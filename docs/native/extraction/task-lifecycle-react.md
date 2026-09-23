# Task P2-04 侦察报告：React 任务生命周期与状态持久化（证据型）

> 范围：`src/state/app/{taskLifecycle,taskLaunch,taskStatus,taskMutations,taskSwitch,taskDeleteDone,worktreeOps}.ts`、`src/{taskSession,sessionHandoff,taskCleanup,taskCompletion,taskFlush}.ts` 全文；对照 `core/crates/aeroric-protocol/src/task.rs`、`MIGRATION_PLAN.md` Task P2-04 段落。
> 方法：逐文件读全文 + 精确行号定位。每条结论后附 `文件:行号`。超出必读清单的引用（如 `src/App.tsx`、`src/types.ts`、`src/appProjectState.ts`）只用于钉住调用点/谓词定义，并在 §5 标注。本报告不提实现方案、不改代码。

## 0. 状态词表对照（先对齐口径）

| 词表 | 值 | 依据 |
|---|---|---|
| Rust `TaskStatus`（9 值，wire snake_case） | todo / pending / running / input_required / detached / interrupted / done / failed / cancelled | `core/crates/aeroric-protocol/src/task.rs:12-21` |
| Rust `is_terminal` | done、failed、cancelled | `core/crates/aeroric-protocol/src/task.rs:39-44` |
| Rust `is_live` | pending、running、input_required、detached | `core/crates/aeroric-protocol/src/task.rs:47-55` |
| Rust `can_resume` | 非 todo ∧ 非 live ∧ worktree 未丢弃 ∧ 有会话 | `core/crates/aeroric-protocol/src/task.rs:220-225` |
| TS `isTerminalTaskStatus` | done、failed、cancelled（与 Rust 一致） | `src/types.ts:414-416` |
| TS `isActiveTaskStatus` | pending、running、input_required、…（删除时用来分流 cancel） | `src/types.ts:391-396` |
| TS `isArchivableTaskStatus` | 非 running ∧ 非 pending（**比终态宽：todo/interrupted/detached 均可归档**） | `src/types.ts:409-411` |
| TS `isLiveTerminalTaskStatus` | pending、running、input_required（**比 Rust `is_live` 少 detached**） | `src/appProjectState.ts:216-218` |
| TS `shouldIgnoreTaskStatusTransition` | 仅一条：`detached` 后忽略 `running`/`input_required` | `src/appProjectState.ts:212-214` |

## 1. 状态转换表（当前状态 + 事件 → 新状态 + 副作用）

副作用列：**盘**=写持久化；**proc**=spawn/kill/reset 子进程或 git 操作；**UI**=UI 事件/视图状态/Toast。

| 当前状态 | 事件 | 新状态 | 副作用 | 依据 |
|---|---|---|---|---|
| （新建） | 提交任务，`immediate=true`，local | `pending` | 盘：persistProjectTasks；UI：setActiveProject/mountProject/selectedTaskId；proc：launchLocalTask → invoke run | `src/state/app/taskLifecycle.ts:333`、`src/state/app/taskLifecycle.ts:338-348`、`src/state/app/taskLifecycle.ts:459-465`、`src/state/app/taskLaunch.ts:56-72` |
| （新建） | 提交任务，`immediate=false` | `todo` | 盘：persistProjectTasks；UI：同上；**不 spawn**（`if (!immediate) return baseTask`） | `src/state/app/taskLifecycle.ts:333`、`src/state/app/taskLifecycle.ts:346-350` |
| （新建） | 提交任务，launchMode=worktree | `pending`/`todo` | proc：invoke `create_task_worktree`；盘：worktreePath/branch/baseBranch 落盘 | `src/state/app/taskLifecycle.ts:413-417`、`src/state/app/taskLifecycle.ts:424-434` |
| （新建，worktree） | create_task_worktree 失败 | （任务被移除） | 盘：删除并 persistProjectTasks；UI：removeTaskBuffers + toast；不 spawn | `src/state/app/taskLifecycle.ts:437-449` |
| （新建，persistBeforeLaunch） | flush 超时/失败，或 flush 期间任务被改 | （任务被移除） | 盘：删除 + persistProjectTasks；UI：清 selectedTaskId；throw，不 spawn | `src/state/app/taskLifecycle.ts:354-380` |
| （新建，SSH 无连接） | 提交任务 | 不建任务（return null） | UI：toast | `src/state/app/taskLifecycle.ts:284-287` |
| （新建，webui 模式） | 提交任务 | 不建任务 | proc：launchDshWebUi | `src/state/app/taskLifecycle.ts:296-308` |
| `todo` | runTodoTask | `pending`（清 attentionRequestedAt） | 盘：persistProjectTasks；proc：launchLocal/Ssh/Wsl；UI：resetTaskTerminal + selectedTaskId | `src/state/app/taskLifecycle.ts:478-484`、`src/state/app/taskLifecycle.ts:518-522`、`src/state/app/taskLifecycle.ts:545-546` |
| `todo`（persistBeforeLaunch） | flush 失败/任务被改 | 回滚到原态 | 盘：rollbackTaskMutation 后 persistProjectTasks；不 spawn，throw | `src/state/app/taskLifecycle.ts:495-515` |
| `todo`，SSH 项目无连接 | runTodoTask | `failed`（带 failureReason） | UI：toast；不 spawn | `src/state/app/taskLifecycle.ts:528-536` |
| settled（done/failed/cancelled/interrupted） | resumeTask 且解析出 sessionId | `pending`（清 attentionRequestedAt、failureReason） | 盘：persistProjectTasks；proc：resume* → invoke resume/run；UI：selectedTaskId、resetTaskTerminal、runCounts+1 | `src/state/app/taskLifecycle.ts:637-644`、`src/state/app/taskLifecycle.ts:652-654`、`src/state/app/taskLifecycle.ts:679-686` |
| settled | resumeTask 解析不出 sessionId | 不变 | UI：toast `running.resumeUnavailable`；不写盘不 spawn | `src/state/app/taskLifecycle.ts:630-632` |
| 任意 | resume 期间任务对象被换掉 | 不变（return false） | 无 | `src/state/app/taskLifecycle.ts:628-629` |
| settled（persistBeforeLaunch） | resume flush 失败/任务被改 | 回滚 | 盘：rollbackTaskMutation + persistProjectTasks；不 spawn | `src/state/app/taskLifecycle.ts:658-672` |
| 任意 | switchTaskConfig（校验通过） | `pending`（换 agent/model/effort/speed/permissionMode，清 attention/failureReason） | proc：invoke `reset_task_process`（杀活进程）；proc：local router `switch_target`；盘：persistProjectTasks，flush 失败回滚再写；spawn **推迟**到 pendingTaskStartsRef 回调；UI：resetTaskTerminal、runCounts+1 | `src/state/app/taskSwitch.ts:401-403`、`src/state/app/taskSwitch.ts:281`、`src/state/app/taskSwitch.ts:267-271`、`src/state/app/taskSwitch.ts:405-431`、`src/state/app/taskSwitch.ts:437-481`、`src/state/app/taskSwitch.ts:482-483` |
| 任意 | switchTaskConfig，无上下文（无结构化转录 ∧ 无终端历史 ∧ 空 prompt） | 不变（return false） | UI：toast `switchConfigNoContext`；**注意 reset_task_process 已执行、进程已杀** | `src/state/app/taskSwitch.ts:377-379`（前置 `src/state/app/taskSwitch.ts:281`） |
| 任意 | switchTaskConfig，validateAgentLaunch 失败 | 不变（return false） | UI：toast；此时尚未杀进程 | `src/state/app/taskSwitch.ts:240-247` |
| 任意 | switch 后启动回调遇 SSH 连接缺失 | `failed` | UI：toast + updateTaskStatus | `src/state/app/taskSwitch.ts:448-452` |
| 任意 | 状态事件（hook/exit/启动失败，经 updateTaskStatus） | 目标状态 | 盘：changed 时 persistProjectTasks；status=done 另触发 flushProjectTasks | `src/state/app/taskStatus.ts:44-45`、`src/state/app/taskStatus.ts:56-68`、`src/App.tsx:953-958` |
| `detached` | hook 报 `running`/`input_required` | 不变（显式忽略） | 无 | `src/state/app/taskStatus.ts:26`、`src/appProjectState.ts:212-214` |
| 同状态 | 重复事件（attention/completedAt 均不变） | 不变（changed=false） | 不写盘 | `src/state/app/taskStatus.ts:35-41`、`src/App.tsx:954` |
| live（dsh 族） | markTaskDone | （延迟）`done` | proc：invoke `complete_dsh_task`；标记 manuallyCompletedDsh；UI：stopTaskOutput；成功→scheduleForDoneTask；失败撤销标记 + resumeTaskOutput + toast | `src/state/app/taskDeleteDone.ts:104-116` |
| live（非 dsh） | markTaskDone | （延迟）`done` | proc：invoke `complete_task`；成功→scheduleForDoneTask；失败 toast | `src/state/app/taskDeleteDone.ts:119-127` |
| 非 live | markTaskDone | `done` | 盘：经 updateTaskStatus→persist；UI：scheduleForDoneTask | `src/state/app/taskDeleteDone.ts:130-131` |
| 任意（active） | deleteTasks | （移除） | proc：invoke cancelLocal（杀进程），随后 cleanupTaskWorktree（git remove，fire-and-forget）；盘：persistProjectTasks（按受影响项目）；UI：removeTaskBuffers、删终端历史、清 selectedTaskId；**starred 一律跳过** | `src/state/app/taskDeleteDone.ts:31-34`、`src/state/app/taskDeleteDone.ts:45-59`、`src/state/app/taskDeleteDone.ts:78-93` |
| 任意（非 active） | deleteTasks | （移除） | proc：仅 cleanupTaskWorktree；其余同上 | `src/state/app/taskDeleteDone.ts:62-69` |
| 任意 | reconnectTask | （reset 后走 resume 转换） | proc：invoke `reset_task_process`，成功才 resumeTask；失败 toast 不 resume | `src/state/app/taskDeleteDone.ts:134-143` |
| 终态 ∧ 非 starred ∧（无 worktree ∨ worktreeDiscarded） | 自动清理到点 | （移除，走 deleteTasks） | 调度 shouldRunCleanup；筛选 expiredTaskIds；其余同 deleteTasks | `src/taskCleanup.ts:48-58`、`src/taskCleanup.ts:79-96`、`src/state/app/useAppMaintenance.ts:81-83` |
| worktree 任务 | mergeTaskWorktree | `worktreeDiscarded=true` | proc：invoke worktree merge → remove（remove 失败仅 console.error）；盘：markDiscarded；merge 失败只 toast、不标记 | `src/state/app/worktreeOps.ts:32-43`、`src/state/app/worktreeOps.ts:16-22`、`src/state/app/worktreeOps.ts:44-46` |
| worktree 任务 | discardTaskWorktree | `worktreeDiscarded=true` | UI：confirm（拒绝则中止）；proc：invoke worktree remove；盘：markDiscarded | `src/state/app/worktreeOps.ts:54-60`、`src/state/app/worktreeOps.ts:63-68` |
| 任意 | archiveTasksInList | `archivedAt=now` | 纯函数无副作用（写盘在调用方）；仅 isArchivableTaskStatus ∧ 未归档 | `src/state/app/taskMutations.ts:5-13`、`src/types.ts:409-411` |
| 已归档 | unarchiveTasksInList | `archivedAt=undefined` | 纯函数 | `src/state/app/taskMutations.ts:15-21` |
| 任意 | toggleTaskStar / rename | `starred` 取反 / `name` 改写（空串→undefined） | 纯函数 | `src/state/app/taskMutations.ts:23-29` |
| 全局 | 退出/重启/更新前 flush | 不变 | 盘：任务+项目双队列并行落盘；单飞去重；10s 超时只限调用方等待 | `src/taskFlush.ts:68-70`、`src/taskFlush.ts:25-55`、`src/taskFlush.ts:3` |

## 2. 仍留在 React 里的业务决定清单（P2-04 验收核心）

| # | 决定是什么 | 现在的位置 | 为什么它不是纯视图逻辑 |
|---|---|---|---|
| 1 | 任务初始状态：`immediate ? "pending" : "todo"` | `src/state/app/taskLifecycle.ts:333` | 生命周期起点语义（排队 vs 立即执行），决定是否 spawn |
| 2 | 任务 ID 生成（createTaskId） | `src/state/app/taskLifecycle.ts:274`（引入 `src/state/app/taskLifecycle.ts:9`） | ID 是持久化主键与幂等锚点 |
| 3 | 命名规则：prompt 为空回退 agent 显示名 | `src/state/app/taskLifecycle.ts:323` | 写入 `name` 并持久化 |
| 4 | dsh preset 缺省：`dshAgentPreset ?? "standard"` | `src/state/app/taskLifecycle.ts:328-331` | 启动参数 + 持久化字段 |
| 5 | 「先持久化再启动」顺序（persistBeforeLaunch，10s flush 超时） | `src/state/app/taskLifecycle.ts:82-88`、`src/state/app/taskLifecycle.ts:352-357` | P2-04 Action 2 点名的持久化一致性契约（`MIGRATION_PLAN.md:1143`） |
| 6 | flush 期间任务被改 → 拒绝请求（对象身份比较） | `src/state/app/taskLifecycle.ts:355-356`、`src/state/app/taskLifecycle.ts:496-497`、`src/state/app/taskLifecycle.ts:659-660` | 乐观并发控制，防旧快照覆盖 |
| 7 | 失败回滚语义：只回滚「本次改动 ∧ 未被再改」的字段 | `src/state/app/taskLifecycle.ts:90-105` | 写冲突合并，直接影响数据正确性 |
| 8 | worktree 创建失败的补偿：整条任务删除 + 清缓冲 | `src/state/app/taskLifecycle.ts:437-449` | 事务性补偿，不留孤儿任务 |
| 9 | runTodo 清 `attentionRequestedAt` | `src/state/app/taskLifecycle.ts:478-481` | 状态语义，驱动提醒/清理 |
| 10 | resume 前置：解析不出 sessionId 拒绝续跑（不静默开新会话） | `src/state/app/taskLifecycle.ts:630-632` | 与 Rust `can_resume` 的 `has_session` 同一规则（`core/crates/aeroric-protocol/src/task.rs:220-225`） |
| 11 | resume 置 `pending`、清 `failureReason` | `src/state/app/taskLifecycle.ts:637-641` | 失败原因是持久化字段 |
| 12 | 会话引用解析阶梯：family 字段 → read_session_id(path) → legacy 字段/legacy path → recover_task_session（仅 local ∧ worktree 未丢弃） | `src/state/app/taskLifecycle.ts:549-615`（条件：`src/state/app/taskLifecycle.ts:562-563`、`src/state/app/taskLifecycle.ts:580-582`、`src/state/app/taskLifecycle.ts:593-596`） | 会话发现/恢复优先级协议 |
| 13 | 会话归属解析：sessionFamily > sessionCodexLike（含 dsh 特判）> 唯一存在族 > agentFamily 兜底 | `src/taskSession.ts:58-83`（特判 `src/taskSession.ts:68-69`、唯一性 `src/taskSession.ts:79-80`、兜底 `src/taskSession.ts:82`） | 决定 wire 协议族；选错族=错的 CLI 语义 |
| 14 | legacy 会话字段回退（其他族字段顶上） | `src/taskSession.ts:96-105` | 旧数据取值协议 |
| 15 | 会话字段独占写入：清四族字段 → 按族回填 + sessionAgent/sessionCodexLike/sessionFamily | `src/state/app/taskLifecycle.ts:33-80`（清空 `src/state/app/taskLifecycle.ts:42-52`，分支 `src/state/app/taskLifecycle.ts:54-79`） | 「一条任务只挂一族会话」不变量 |
| 16 | reset 快照合并：恰好一族有快照才独占收编并改 sessionFamily（`present !== 1` 仅字段覆盖） | `src/state/app/taskSwitch.ts:97-172`（判定 `src/state/app/taskSwitch.ts:117`，分支 `src/state/app/taskSwitch.ts:129`、`src/state/app/taskSwitch.ts:143`、`src/state/app/taskSwitch.ts:157`、`src/state/app/taskSwitch.ts:170`） | 会话归属仲裁 |
| 17 | 原生 resume 能力：dsh 永不；必须同族；内建间互认、自定义须同 agent | `src/taskSession.ts:113-127`（`src/taskSession.ts:121-126`） | 协议能力矩阵，决定走 resume 还是 handoff |
| 18 | 收养能力：dsh/omp 永不收养；同族 ∧ 非 native resume 才收养 | `src/taskSession.ts:136-147`（`src/taskSession.ts:144-146`） | 同上 |
| 19 | 切换续跑策略：`nativeResumeSupported=false ⇒ handoff`；否则 resume > adopt > handoff | `src/taskSession.ts:156-166` | 转录兼容性是运行时事实；不兼容会话必须绕过 resume/adopt |
| 20 | 切换前二次探测 `session_supports_native_resume` 后再定策略 | `src/state/app/taskSwitch.ts:305-323` | 数据事实探测影响机制选择 |
| 21 | 收养成功才转 resume，失败 warn 降级 | `src/state/app/taskSwitch.ts:333-348` | 副作用是磁盘上的转录迁移 |
| 22 | handoff 上下文门槛：结构化转录/终端历史/prompt 全空则拒绝切换 | `src/state/app/taskSwitch.ts:377-379` | 防「续跑」退化成失忆新会话 |
| 23 | 会话字段三方差量合并（只并入本轮真变了的字段） | `src/state/app/taskSwitch.ts:34-70`、`src/state/app/taskSwitch.ts:394-395` | 冲突合并语义 |
| 24 | 首次输入门控：spawn 注册进 pendingTaskStartsRef，terminal ready 一次性触发；删除/取消时撤销 | 注册 `src/state/app/taskLifecycle.ts:684-686`、`src/state/app/taskSwitch.ts:437-481`；撤销 `src/state/app/taskDeleteDone.ts:41-43`、`src/App.tsx:549`；触发（单发即删）`src/App.tsx:1037-1041` | P2-04 Action 2 点名的启动时序控制（`MIGRATION_PLAN.md:1143`） |
| 25 | 手动完成三路分支：dsh→complete_dsh_task；live→complete_task；否则本地置 done | `src/taskCompletion.ts:8-17`、`src/state/app/taskDeleteDone.ts:103-131` | 完成协议（是否调后端、是否停输出） |
| 26 | dsh 手动完成前置 manuallyCompletedDsh 标记、失败撤销 | `src/state/app/taskDeleteDone.ts:105`、`src/state/app/taskDeleteDone.ts:112`（消费 `src/App.tsx:368`） | 压制迟到 hook/exit 翻状态 |
| 27 | 完成后 scheduleForDoneTask 延迟落地 done | `src/state/app/taskDeleteDone.ts:109`、`src/state/app/taskDeleteDone.ts:122`、`src/state/app/taskDeleteDone.ts:131` | 排序约束（等真正退出再落 done） |
| 28 | 迁移守卫：detached 后忽略 running/input_required | `src/state/app/taskStatus.ts:26`（定义 `src/appProjectState.ts:212-214`） | 防迟到 hook 翻回 live |
| 29 | completedAt 终态规则：进终态首值不刷新，离开终态清空 | `src/state/app/taskStatus.ts:31-33` | P2-04 Action 2 点名（`MIGRATION_PLAN.md:1143`）；保留期以它计龄 |
| 30 | attentionRequestedAt 仅 input_required 置值 | `src/state/app/taskStatus.ts:28-29` | 「等你输入」提醒语义 |
| 31 | failed 写 `failureReason` | `src/state/app/taskStatus.ts:45` | 持久化错误原因 |
| 32 | done 落盘后立即 flush | `src/state/app/taskStatus.ts:65-68` | 防退出丢完成态 |
| 33 | 删除保护：starred 永不删 | `src/state/app/taskDeleteDone.ts:31-34` | 数据保护 |
| 34 | 删除顺序：active 先 cancel 再清 worktree；非 active 只清 worktree | `src/state/app/taskDeleteDone.ts:45-69` | 资源回收顺序 |
| 35 | 删除伴随清理：缓冲、终端历史、selectedTaskId 复位（isNewTask=true） | `src/state/app/taskDeleteDone.ts:89-93`、`src/state/app/taskMutations.ts:39-41` | 磁盘清理 + 引用完整性 |
| 36 | worktree 终局顺序：merge 成功才 remove（remove 可容忍失败）；成功后才 worktreeDiscarded=true | `src/state/app/worktreeOps.ts:32-43`、`src/state/app/worktreeOps.ts:16-22` | 资源终局判定 |
| 37 | worktreeDiscarded 禁 resume/恢复探测、清理跳过 | Rust `core/crates/aeroric-protocol/src/task.rs:220-225`；TS `src/state/app/worktreeOps.ts:79`、`src/state/app/taskLifecycle.ts:593` | P2-04 点名的跨端不变量（`MIGRATION_PLAN.md:1143`） |
| 38 | 自动清理门槛：正向 isTerminalTaskStatus（排除法会误删 interrupted/todo）、跳过 starred、跳过 worktree 未处理、`completedAt ?? createdAt` 计龄 | `src/taskCleanup.ts:87-94` | 保留期策略，直接删数据 |
| 39 | 清理调度：weekly 用本地日历分量取「最近应执行槽」、interval 用整天毫秒；lastRunAt 缺失不跑 | `src/taskCleanup.ts:26-38`、`src/taskCleanup.ts:48-58`（缺失规则 `src/taskCleanup.ts:50-51`） | 含时区/夏令时正确性的调度语义 |
| 40 | 归档资格：非 running/pending 且未归档 | `src/state/app/taskMutations.ts:8`（谓词 `src/types.ts:409-411`） | 状态机约束 |
| 41 | rename 空串→undefined | `src/state/app/taskMutations.ts:28` | 归一化进持久化数据 |
| 42 | 取消任务撤销 pendingTaskStarts 注册 | `src/App.tsx:549` | 防已取消任务被迟到触发 spawn |
| 43 | 重连顺序：reset 成功才 resume | `src/state/app/taskDeleteDone.ts:137-143` | 进程重置-续跑顺序约束 |
| 44 | 启动失败→failed + failureReason | `src/state/app/taskLaunch.ts:13-17`（接线 `src/App.tsx:467`） | 错误是业务状态 |
| 45 | dsh 续跑空 prompt 重连（不重放原消息） | `src/state/app/taskLaunch.ts:174-181` | 会话语义 |
| 46 | 每次启动/续跑清 manuallyCompletedDsh 标记 | `src/state/app/taskLifecycle.ts:146`、`src/state/app/taskLifecycle.ts:191` | 一次性标记生命周期 |
| 47 | flush 单飞：同一时刻一个落盘操作；超时不取消底层写 | `src/taskFlush.ts:25-55` | 写盘串行化/去重契约 |
| 48 | 退出前双队列并行 flush，任一失败整体失败以保活应用 | `src/taskFlush.ts:58-70` | 落盘完整性契约 |
| 49 | 每次 resume/switch runCounts+1 并复位终端 | `src/state/app/taskLifecycle.ts:679-680`、`src/state/app/taskSwitch.ts:482-483` | 运行代际锚点（旧输出作废） |
| 50 | 续跑上下文谓词：prompt 非空 ∨ 任一族会话字段存在 | `src/taskSession.ts:168-180` | 「有没有东西可续」业务谓词 |

## 3. 幂等与竞态

检索结论：`idempotencyKey`、`revision`、`expected_revision`、`generation` 在 12 个必读文件中**零命中**（近邻仅有 runCounts：`src/state/app/taskLifecycle.ts:680`）。Rust 契约已有 `revision`/`generation` 字段与 expected_revision 语义（`core/crates/aeroric-protocol/src/task.rs:159-161`、`core/crates/aeroric-protocol/src/task.rs:148-150`）。React 现行防线是**对象身份 + 字段级合并 + 一次性标记**：

| # | 机制 | 位置 | 同一事件重复到达会发生什么 |
|---|---|---|---|
| 1 | 同状态重复事件短路（status/attention/completedAt 全等→changed=false） | `src/state/app/taskStatus.ts:35-41`、门控 `src/App.tsx:954` | 不产生新对象、不写盘；无害 |
| 2 | completedAt 首值不刷新 | `src/state/app/taskStatus.ts:31-33` | 迟到第二终态事件（failed→cancelled）不改结束时间 |
| 3 | detached 后忽略 running/input_required | `src/state/app/taskStatus.ts:26`、`src/appProjectState.ts:212-214` | 迟到 hook 被静默丢弃，任务不翻回 live |
| 4 | 对象身份乐观锁：flush 后身份变了→抛「Task changed while saving」 | `src/state/app/taskLifecycle.ts:355-356`、`src/state/app/taskLifecycle.ts:496-497`、`src/state/app/taskLifecycle.ts:659-660` | 并发修改/重复触发不拿旧快照发请求；第二次因身份已变被拒 |
| 5 | 异步解析后复查身份 | `src/state/app/taskLifecycle.ts:628-629` | 解析期间任务被改/删→resume 作废，不 spawn |
| 6 | rollbackTaskMutation 字段级回滚（仅回滚本次改 ∧ present==applied 的字段） | `src/state/app/taskLifecycle.ts:90-105` | 重复回滚无效果；期间迟到事件写入的字段被保留 |
| 7 | mergeChangedTaskSessionFields 三方差量合并 | `src/state/app/taskSwitch.ts:34-70`、`src/state/app/taskSwitch.ts:394-395` | 切换重复提交第二次差量为空，不覆盖迟到会话字段 |
| 8 | manuallyCompletedDsh 一次性标记（前置、失败撤销） | `src/state/app/taskDeleteDone.ts:105`、`src/state/app/taskDeleteDone.ts:112` | 迟到 dsh hook/exit 被识别为手动完成而压制（消费点 `src/App.tsx:368`）；重复 markTaskDone 会再 invoke 一次 complete（后端幂等性见 §5） |
| 9 | pendingTaskStartsRef 一次性启动回调（触发即删） | 注册 `src/state/app/taskLifecycle.ts:684-686`、`src/state/app/taskSwitch.ts:437`；触发/删除 `src/App.tsx:1038-1041` | 重复的 terminal-ready 只 spawn 一次；第二次找不到回调直接 return |
| 10 | 删除/取消撤销 pendingTaskStarts 注册 | `src/state/app/taskDeleteDone.ts:41-43`、`src/App.tsx:549` | 迟到 ready 不会对已删/已取消任务 spawn |
| 11 | deleteTasks 二次到达短路（无匹配任务即 return） | `src/state/app/taskDeleteDone.ts:38-39` | 重复删除不重复写盘/删历史；但第一次的 cancel/remove 是 fire-and-forget，重复到达若与第一次并发则可能双 invoke（无锁） |
| 12 | flush 单飞去重（inFlightOperation 复用） | `src/taskFlush.ts:26-49` | 重复 flush 共享同一操作；超时后重试不会与未竟写重叠 |
| 13 | markDiscarded 幂等 + cleanupTaskWorktree 对已丢弃任务短路 | `src/state/app/worktreeOps.ts:20`、`src/state/app/worktreeOps.ts:79` | 重复合并/删除不再二次 `git worktree remove` |
| 14 | runCounts 每次 resume/switch 自增 | `src/state/app/taskLifecycle.ts:680`、`src/state/app/taskSwitch.ts:483` | 旧代际终端输出作废（generation 参数透传 `src/App.tsx:1037`、`src/AppOpsProvider.tsx:98`）；重复 resume 每次都自增=新代际 |
| 15 | resume/switch 对同一任务重复触发 | `src/state/app/taskLifecycle.ts:637-644`、`src/state/app/taskSwitch.ts:401-415` | 无状态门槛（settled 判定在调用方 UI），重复触发会重复 spawn——**唯一的重复 spawn 防线是 pendingTaskStarts/身份比较，不是 operation key**（P2-04 要求的 operation key 见 `MIGRATION_PLAN.md:1144`） |

## 4. 不可搬运的部分（必须留在客户端）

判据：P2-04 Action 4「UI 通知、选中哪个任务、Toast 留客户端」（`MIGRATION_PLAN.md:1145`）+ Acceptance「fake runtime 没有 GUI listener 时 start/resume/cancel 全部可执行」（`MIGRATION_PLAN.md:1149`）——凡去掉 GUI 仍必须成立的语义都该进核心，反之留下。

| 项 | 位置 | 留客户端的判据 |
|---|---|---|
| 选中任务/新建态：selectedTaskId、isNewTask | `src/state/app/taskMutations.ts:39-41`、`src/state/app/taskLifecycle.ts:348`、`src/state/app/taskLifecycle.ts:519-522` | 明确列为客户端保留（`MIGRATION_PLAN.md:1145`）；但「删除后引用不得悬空」的复位动作伴随删除发生，边界见 §5 |
| Toast 通知 | `src/state/app/taskDeleteDone.ts:52`、`src/state/app/taskDeleteDone.ts:57`、`src/state/app/worktreeOps.ts:45` 等 | 明确留客户端（`MIGRATION_PLAN.md:1145`）；对应业务错误应转 `RpcError`（同出处） |
| confirm 对话框（丢弃 worktree 前确认） | `src/state/app/worktreeOps.ts:54-60` | 用户决策交互属 UI；但「未确认不得执行丢弃」的门槛应由核心拒绝表达 |
| 项目导航：setActiveProject、mountProject | `src/state/app/taskLifecycle.ts:346-347`、`src/state/app/taskLifecycle.ts:652-653` | 纯导航，去 GUI 后无意义 |
| 终端视图缓冲：resetTaskTerminal、removeTaskBuffers、stopTaskOutput、resumeTaskOutput | `src/state/app/taskLifecycle.ts:382`、`src/state/app/taskLifecycle.ts:448`、`src/state/app/taskDeleteDone.ts:89`、`src/state/app/taskDeleteDone.ts:106`、`src/state/app/taskDeleteDone.ts:113` | 是渲染缓冲不是任务状态；核心只需发事件 |
| runCounts 显示计数 | `src/state/app/taskLifecycle.ts:680`、`src/state/app/taskSwitch.ts:483` | 展示层派生态（但其背后的代际语义属核心，见 §3#14） |
| 启动错误写进终端：writeErrorToTerminal | `src/state/app/taskLaunch.ts:8`、`src/state/app/taskLaunch.ts:15` | 展示；同一处的 onFailed→failed 状态是业务（`src/App.tsx:467`） |
| window 级设置变更广播 dispatchAppSettingsChanged(window) | `src/state/app/taskSwitch.ts:271` | 浏览器 window 事件，纯客户端 |
| 展示名 agentDisplayLabel | `src/state/app/taskLifecycle.ts:323`、`src/state/app/taskSwitch.ts:383` | 标签是 i18n 展示（但 #3 的「空名回退」规则是业务） |
| 终端尺寸 cols/rows 采集 | `src/state/app/taskLaunch.ts:9`、`src/state/app/taskLaunch.ts:69-70` | 来自 UI 的运行时输入，客户端采集、作为参数传核心 |
| handoff prompt 注入终端（injectPromptIntoTerminal/forcePromptInjection） | `src/state/app/taskLaunch.ts:68`、`src/state/app/taskSwitch.ts:443` | 注入目标是前端 PTY 管道；但「何时注入、注入什么」的决定属核心（#22、#23） |

## 5. 无法证实的条目（不混入前四节）

1. `idempotencyKey` 在 React 侧是否存在：12 个必读文件零命中，但未搜全仓库；无法证实别处是否持有幂等键。近邻事实：Rust `expected_revision` 语义 `core/crates/aeroric-protocol/src/task.rs:148-150`。
2. PTY `generation` 的分配/比较：必读文件只见 runCounts 自增（`src/state/app/taskLifecycle.ts:680`）与 generation 参数透传（`src/App.tsx:1037`、`src/AppOpsProvider.tsx:98`）。「旧 generation exit 不覆盖新 generation」（`MIGRATION_PLAN.md:1148`）的现行为实现位于终端管理层，未核对。
3. `shouldIgnoreTaskStatusTransition`/`isTerminalTaskStatus`/`isActiveTaskStatus`/`isArchivableTaskStatus`/`isLiveTerminalTaskStatus` 定义在必读清单外（`src/appProjectState.ts:212-214`、`src/types.ts:391-396`、`src/types.ts:409-411`、`src/types.ts:414-416`、`src/appProjectState.ts:216-218`），本报告已核对其文本，但是否随 P2-04 迁走需主代理定夺。
4. `persistProjectTasks`/`flushProjectTasks`/`flushAllProjectTasks`/`flushProjects` 的队列内部（防抖/链式串行）只核对到接口面（`src/appProjectState.ts:137-164`），未逐行读实现。
5. `scheduleForDoneTask` 来自 useWorktreeDiffStats（`src/App.tsx:262-265`）：延迟多久、迟到 done 如何合并，未核对。
6. `manuallyCompletedDshTasks` 消费逻辑只确认到注入点 `src/App.tsx:368`（`isManuallyCompletedDsh`）；它具体压制哪个事件分支在 listener 内部，未核对。
7. pendingTaskStarts 触发点确认为 terminal ready（`src/App.tsx:1037-1041`），但「terminal ready」与 P2-04 所称「首次输入门控」（`MIGRATION_PLAN.md:1143`）是否同一时刻，未证实。
8. hook/exit 事件的接收链路（`useAppTauriEvents`、`remote-task-request` 监听；记载见 `MIGRATION_PLAN.md:963`——`src-tauri/src/remote/tasks_rpc.rs:483` → `src/App.tsx:498`）不在必读清单，迟到 hook 的最终落点与去重未核对。
9. `src-tauri/src/remote/tasks_rpc.rs` 在 P2-04 Input 清单内（`MIGRATION_PLAN.md:1140`），但不在本次必读清单，**未读**。
10. `sessionHandoffPrompt.ts`（`formatSessionHandoff`/`hasStructuredSessionTranscript`，被 `src/state/app/taskSwitch.ts:12-15` 引入）不在必读清单；仅核对到导出符号位置（`src/sessionHandoffPrompt.ts:101`、`src/sessionHandoffPrompt.ts:166`）。
11. `sessionHandoff.ts` 的清洗规则（转义剥离 `src/sessionHandoff.ts:19-27`、spinner/状态栏过滤 `src/sessionHandoff.ts:71-99`、400 行去重窗 `src/sessionHandoff.ts:103`、`src/sessionHandoff.ts:125-134`）已逐行读完；它只被 sessionHandoffPrompt 消费（`src/sessionHandoffPrompt.ts:1`），handoff 组装的最终格式在后者，未核对。
12. `createTaskId` 实现于 `src/taskId.ts`（引入见 `src/state/app/taskLifecycle.ts:9`），唯一性/排序保证未核对。
13. 重复 `markTaskDone` 时后端 `complete_dsh_task`/`complete_task` 的幂等性在后端命令实现中，未核对；前端无防重（`src/state/app/taskDeleteDone.ts:104-131`）。
14. 滚动位置：必读文件中无任何命中，无法给出位置。
15. `tasksRef.current.find(...) !== task` 这类身份比较在 React 重渲染/状态重建后是否恒假（即乐观锁是否退化），取决于 setTasks 的不可变更新纪律；未做运行时验证。
