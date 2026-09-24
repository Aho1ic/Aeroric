# ADR-035：观察的"决策"与"落盘"解耦——core 在所有平台都是事件权威，只在递交了写权的平台是数据权威

- 状态：已接受（Task P2-05 门禁，Q64 —— 用户拍板，采纳推荐）。**本 ADR 解开 ADR-033 与 ADR-021 之间的一处硬冲突**，并撤销 ADR-034 落实 §3 那条"已声明的不等价"（见决策 6）。
- 日期：2026-09-24
- 相关：ADR-021（writer 只在 macOS 递交）、ADR-023（core 唯一发射者）、ADR-033（28 处一次性全交）、ADR-034（迟到观察是 core 的规则）、ADR-015/ADR-011（覆盖基线与拒写）、INV-03

## 背景

ADR-033 把发射面移交定为"28 处现在就全交 core"。真要动手前量到四件事，合起来让"现在就删"在三个平台中的两个上**必然做成 ADR-033 §4 明令禁止的静默不发**：

1. `TaskService::observe` 的提交走 `commit → task_storage::save_covering → compat::require_writer`（`core/crates/aeroric-core/src/storage/tasks.rs:73`），非 writer 直接 `Err`。
2. `writer_handover_allowed()` 就是 `cfg!(target_os = "macos")`（`src-tauri/src/core_adapter.rs`），Windows/Linux 上 core 是 `HandoverDeferred`，`CoreGateway::startup` 连租约都不尝试，拿到的是 `CoreContext::read_only`。
3. **但读不需要写权**：`load_tasks`（`tasks.rs:16`）与 `load_stamped`（`tasks.rs:53`）都是纯读，读的还是 `compat` 那套旧客户端布局。也就是说非 writer 平台上 core 完全看得见真实的行，只是不能改。
4. 今天 src-tauri 里没有任何一处构造 `TaskEvent`（全仓 `\.observe\(` 零命中），`TauriEventSink::notify` 也没有调用者——28 处 legacy `.emit` 是这两个事件唯一的产地。

于是：把 28 处改成"进 core 的 observe"，在 macOS 上成立，在 Windows/Linux 上 `observe` 一律 `Err`，`task-status`/`task-session` 从此不再发出，任务卡永远不再变色。而 Windows/Linux 是**在架产品**（`AGENTS.md` 的 release 资产清单里 msi/exe/deb/rpm 都在）。

三个备选：① 只 macOS 先删、其余平台留 legacy 发射（源码里两套路径共存到 P7，`emitters == 1` 这条静态门禁得改成按平台可核对）；② 推到 P7 与写权跨平台一次切（ADR-023 在 P2-05 不收口）；③ **把"决策+发射"与"落盘"拆开**——core 的 reducer 和两道守卫不需要写权就能跑，需要写权的只有最后那次 `save_covering`。用户选 ③。

## 决策

1. **`observe` 拆成"决策"与"提交"两段**，决策段只依赖读（`load_stamped`）＋账本，提交段仍是 `commit`。非 writer 平台走"决策 + 发射 + 不提交"；writer 平台在同一份决策后面接 `commit`。这不是新增一条平行实现，而是把现有的 `observe` 内部分层后暴露两个入口，规则只有一份。
2. **权威分工按"事件/数据"切，不按平台切**：事件权威永远是 core（唯一决定"这条状态用不用户该看到"的地方），数据权威按 ADR-021 逐平台递交。过渡期 Windows/Linux 上磁盘仍由旧客户端的保存路径写——**这件事今天就是这样**：28 处发射点里没有一处负责落盘，落盘是 React 侧 `updateTaskStatus` 之后的保存队列做的。所以本决策不新增写入者，也不违反 INV-03。
3. **准入即发射**：一条被 reducer **准入**的观察必须发事件，即使算出的 `changed == false`。今天 28 处里 25 处是无条件发射（ADR-033 实测），把"没变化"折叠成"不发"就是在移交里夹带行为变更。只有 `EventOutcome::Dropped` 才不发。
4. **禁止的绕道**：不许为了绕开本决策的复杂度而在 core 之外补 `.emit`；也不许把非 writer 平台的观察"先收下、等切换再补发"——补发队列就是第二个事件权威。
5. **可核对**：门禁要断言非 writer 的决策路径不触到 `require_writer`，并且断言它在只读 context 上确实发出了事件（用真 `CoreContext::read_only` ＋录制 sink，而不是 mock）。
6. **顺带修正 ADR-034 的一处实现超纲**：该 ADR 决策 2 明写规则的边界是"转录与 hook 观察"，启动/退出/取消/手动完成/DSH 不在其列；落实 §3 却把判据写成"这一行是不是终态"，覆盖面比 ADR 允许的宽。本轮按 ADR-034 **后果**最后一条要求的口径改回 **"`live_for(task_id)` 为空才丢"**（账本里这个查询已经存在，`operations.rs:273`）。
   这条修正不是收缩保护，而是同时满足三件事：与旧客户端逐处等价、仍然挡住那个真实事故（进程退出后 claim 已结算 → 无存活代 → 迟到的转录行照旧被丢）、并且**取消窗口的不等价自动消失**（取消时 claim 还开着 → 存活代非空 → 迟到的 `running` 照旧通过，即今天的行为）。ADR-034 落实 §3 因此作废，以本节为准。

## 后果

- **发射面移交与 store 切换解耦**：28 处的删除不再需要等写权移交，跨平台单一发射者当场可成立。这是本决策买到的东西。
- **代价写清楚**：core 的 `observe` 有两种持久化行为，各自都要有用例；而且非 mac 平台上 core 会"知道"一个它没有写下的状态——如果旧客户端的保存路径最终写的词与 core 决策的不同，磁盘与事件的短暂分歧是可能的。今天这条分歧已经存在（发射点与保存路径本来就是两件事），本决策不制造它，只是不许掩盖它。
- **DSH 那 9 处的新前置暴露出来**：dsh 的运行从未经过 core 的账本（`LegacyTaskExecutor::launch` 对 dsh 返回 `Unsupported`），所以 `live_for(dsh_task)` 恒为空——按决策 6 的判据，dsh 的 9 处一旦删除会**全部静默丢弃**。因此观察通道必须先有一条"宿主登记的代"：dsh 起进程时向 core 登记一个存活代，退出时结算。这与 ADR-033 §3 那条"runtime → core 观察通道"是同一条前置，只是现在它的内容更明确了（不只是转发状态，还要转发存活）。
- 还需要一条 core 侧没有的东西：**task → project 解析**。`observe(project_id, event)` 要的 project，28 处发射点全都拿不到（`emit_task_status(app, task_id, status)`、`pty.rs:1970`、`dsh_webui.rs:1473`、`event_watcher.rs:319` 都只有 `task_id`），`Claim` 里也没有这个字段（`operations.rs:51`）。
- 一条顺带量到的事实：`event_watcher.rs:315-319` 在 `task-status` 载荷里多发了一个 `approval` 键，而前端 `src/state/app/useAppTauriEvents.ts:43` 声明的是 `{ task_id, status, failure_reason? }`——没有任何读取点。走 core 的 sink 之后这个键会消失；它不是"少发一个事件"，但要在删除那一步逐处点名，不许混在"载荷不变"的笼统承诺里。
