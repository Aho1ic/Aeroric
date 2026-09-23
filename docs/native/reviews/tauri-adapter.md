# Module review — `tauri-adapter` (Task P2-05)

- **Reviewed**: 2026-09-23
- **Design source**: `MIGRATION_PLAN.md` §Phase 2 · Task P2-05
- **Code（本模块目前唯一已合并部分）**: `src-tauri/src/storage_location.rs`、`src-tauri/src/storage.rs::windows_private_acl_receipt`、`src-tauri/src/lib.rs` 的 `mod storage_location;`
- **Gate**: `pnpm native:check tauri-adapter` —— **尚未注册**（注册要在 `core_adapter.rs`/`legacy_task_executor.rs` 落地的同一次编辑里完成，见 §4）
- **Gate status**: 拷问**已结束**——Q44～Q54 十一问全部获答（ADR-020～027），用户已确认共识并授权先做收口提交（`1279520`，88 files，不 push、不 tag）。**任务本身仍未 accepted**：实现尚未开始、`tauri-adapter` 门禁模块尚未注册——accept 的条件是 §1 三条验收全绿，不是"决策已定"。

## 1. 验收标准（引计划原文）

1. `pnpm typecheck`、`pnpm test`、`cargo test --manifest-path src-tauri/Cargo.toml --locked --lib`；`pnpm native:check tauri-adapter`。
2. 旧 GUI 经真实 legacy executor 创建/恢复/完成一项 fake 任务，**只发生一次存储写入序列和一次 spawn**；没有双事件/事件回环/递归命令调用；四族暂未抽取的启动路径仍可达。
3. 移交前置：`StorageLocationProbe` 真实探针 + Windows ACE 回执（**已交付**）；`TaskManager` 改 `(task_id, generation)` 键；待确认闸在旧客户端续跑入口生效；分组清单合并规则；保存队列归属。

## 2. 门禁问答（Q44～Q54）

| # | 问题 | 用户决定 | 落点 | 是否否掉推荐 |
| --- | --- | --- | --- | --- |
| Q44 | `TaskManager` 14 个 taskId 键容器共 277 处引用，重键范围多大？（推荐：只改 6 个按运行的容器／104 处） | **14 个容器全量重键** | ADR-020 决策 1 | ✅ 否掉 |
| Q45 | generation 用哪个身份？legacy 已有全局 `AtomicU64`、`Arc::ptr_eq`、dsh 按 agent 三套 | **全部用 core 的 per-task generation**；全局计数删除，`Arc::ptr_eq` 降为安全网断言 | ADR-020 决策 2 | 采纳 |
| Q46 | 双开后同任务两代能否续跑同一会话文件？`claimed_session_paths` 是唯一键型非 taskId 的集合 | **保持路径粒度，写明为例外** | ADR-020 决策 3 | 采纳 |
| Q47 | writer 原子切换切到哪几个平台？Windows 回执接通后正常 NTFS 会成功 | **只在 macOS 切换，其余等 P7**；平台判断属宿主策略，不进 core | ADR-021 | 采纳 |
| Q48 | ADR-018 的确认闸接在旧客户端哪个入口？（resume 之外还有 reconnect） | **点恢复/重连时就地确认并重试**，单点覆盖 `App.tsx:616`/`631` | §3 记要（不做 ADR，可逆） | 采纳 |
| Q49 | 保存队列归属：`taskPersistence.ts` 每 350ms 合并**整表快照**发给全量覆盖的 `save_project_tasks` | **队列只排队意图，core 做读-改-写** | ADR-022 | 采纳 |
| Q50 | 谁拥有 `task-status`/`task-session` 的发射权？ | **core 唯一发射者**；事件名与 payload 不变，范围只到这 ~8 处 | ADR-023 | 采纳 |
| Q51 | 分组名单以谁为准：webview `aeroric:projectGroups`（带顺序）还是 core 独占 `project-groups.json`？（首次因连接中断未答，已重问） | **core 文件唯一权威，localStorage 一次性幂等导入**；`mergeProjectGroupNames` 的"名单落后于项目数据"那一半保留 | ADR-024 | 采纳 |
| Q52 | 只带 taskId 的请求寻址哪一代？（v1 契约不含参数形状，旧 GUI 无法传 generation） | **资源型→已注册的最大 generation；意图型（cancel/手动完成）→全部在飞代** | ADR-025 | 采纳 |
| Q53 | core 拒绝写入（`REVISION_CONFLICT`/指纹不符）时旧 GUI 怎么办？（推荐：全类自动重放一次） | **分两类：元数据重放一次，状态 op 一律报错** | ADR-026 | ✅ 否掉 |
| Q54 | 第二个实例拿不到 lease 时怎么办？（今天不会发生，旧客户端不取 lease） | **只读模式启动 + 持续可见告知**；不抢 lease、不退临时目录 | ADR-027 | 采纳 |

| Q55 | `claude_gpt55` 在 core 里算哪一族？（实测：启动参数按 codex、会话文件按 claude，`ProtocolFamily` 只有一个字段） | **拆成 `launch_family` 与 `session_family` 两个概念** | ADR-028、缺陷 G18 |
| Q56 | 拆分落在哪一层？（推荐：只拆宿主端口与 core 模型，v1 契约不动） | **拆到任务记录：每个任务存 `launchFamily` + `sessionFamily`**，钉死当初怎么起的 | ADR-028 决策 2 |
| Q57 | 存量记录都没有 `launchFamily`，缺失时怎么办？ | **缺失＝用与 legacy 相同的纯函数按现推导，写时补齐**；不做启动期批量重写用户数据 | ADR-028 决策 3 |

版本口径不是问题（不必问）：ADR-009 已定"v1 内部兼容性一律由具名能力承载，绝不由版本号推断，引入 v2 需另立 ADR"，所以这是**v1 内新增可选字段 + 一个能力位**，不 bump、不升 v2。

**本轮否掉推荐共 4 次：Q44、Q53、Q55、Q56。** 共同点很一致：**要更完整、更显式、更少自动行为**——不做收窄的键改造（要全量）、不自动重放会起进程的操作（要人拍板）、不复用单个 `family` 字段（要拆成语义正确的两个）、不只在端口里拆（要落进记录钉死）。迁移期把"少动一步"当作风险而不是收益；这条口径供后续任务沿用，也意味着**在这份计划里推荐"更省事"的选项要准备好被否**。

这条拆分把 P2-05 的实现面扩大了一圈（`AgentFamilies` 端口拆两个方法、任务记录新增两字段、`aeroric-protocol` 类型、fixtures、Swift/TS 两侧生成物、283 条 parity 期望重算），而"claude_gpt55 到底该用哪套启动参数"的真裁决留给 P3-02（缺陷 G18）。

## 3. 事实修正（侦察阶段我报错过、实现时才发现的三条）

1. **"`generation` 在 src-tauri 里根本不存在"是错的**。实际三套并行编号：`pty.rs:1205-1229` 进程全局 `AtomicU64`（作 `initial_input_signals` 值的判别字段）、`lib.rs:241-266` 的 `Arc::ptr_eq`（其注释已点名"配置切换复用 taskId"这个危害本身）、`dsh_webui.rs:217-243` 按 agent 的 `$events` 代际。这条错误会让重键被当成绿地工程，实际是**替换既有身份**。
2. **`statvfs` 在 macOS/BSD 上没有 `f_fstypename`**（编译期 E0609）；文件系统名在 `statfs` 上，Linux 也只有 `statfs::f_type`。计划原文的"`statvfs`/`getmntinfo`"改成：两平台都走 `statfs`、各读各字段，`getmntinfo` 不需要。Linux 魔数表逐条对照 `include/uapi/linux/magic.h` 核对，并加"与本地表不相交"断言（凭记忆写的两条假值已删，其中一条是 BTRFS 真值的近似抄写错）。
3. 发射点数量修正：`task-status`/`task-session` 相关 **~8 处**（包装函数 4 个调用点 + `task-session` 6 处字面量，分布于 `session.rs`/`session_omp.rs`/`session_dsh.rs`/`event_watcher.rs`），不是早先记录的 24；全仓 `.emit(` 共 91 处，与本模块无关的 ~83 处不动。
4. Q48 不做 ADR 的理由：它是交互层选择，改法是局部替换而非语义迁移；同时它带来一条必须记的事实——`reconnectTask`（`taskDeleteDone.ts:134-144`）是 `reset` + `resume` 的**程序化**续跑，只改 resume 按钮会让 reconnect 绕开确认闸。

## 4. 待办与阻塞（按依赖序）

1. **`TaskManager` 全量重键（ADR-020 + ADR-025）——第一波已完成（2026-09-23）**：`RunKey{ id, generation }` 进 `run_registry.rs`，`generation == RunKey::UNMINTED(0)` 是"这次运行不是 core 起的"的保留常量（不是第四个计数器；core 的编号从 1 起，所以未 minted 永不会赢得寻址竞争）。已重键：`pty_masters`/`pty_writers`/`child_handles`/`pending_pty_sizes` + `TaskManager.runs`；`register_pty_handles` 收 `&RunKey` 并在三张表都插好后才 `runs.register`；`remove_pty_handles*` 收 `&RunKey` 并 retire；`Arc::ptr_eq` 从判定依据降为 `debug_assert` 安全网。两条重键逼出的语义区别已写成断言/helper：**存活判断看"任意一代"（`has_live_run`），资源判断看"最新一代"（`resource_run`/`writer_for`/`master_for`）**；`pending_pty_sizes` 是"投递给这个 id 的下一次运行"的信箱，读写一律 `unminted`。**剩下（wave 2，先问后做）**：三个 taskId 集合 + 四张会话表 + `initial_input_signals`/删 `STARTUP_SIGNAL_GENERATION` + 锁序表重写。三题：① 取消/手动完成是任务级意图还是运行级条目；② 会话表按代键后与 `claimed_session_paths` 的"先取条目再释放占用"如何重排；③ 旧命令路径没有 core 代，`UNMINTED` 当"这一代"是否与 ADR-020 决策 2 相容。**安全网计数**：src-tauri `--lib` 基线 2412 → 前置① 2424 → CoreGateway 2430 → ADR-029 2433 → TauriEventSink 2436 → RunRegistry 2441 → 执行器 2450 → 重键第一波 **2453**。
2. **已完成（2026-09-23）**：`core_adapter.rs::CoreGateway`（writer 交接判定 + 只读回退，9 条测试）与 `SettingsFamilies`（ADR-028 的宿主侧）。**关键事实修正**：`launch_family` 不能查表——`app_settings/launch_spec.rs:400-406` 是**读用户 start 脚本内容**动态推断的，写死一张表会覆盖真实配置并改变实际执行的命令行；因此宿主委托给 `get_agent_launch_spec()`（`pty.rs` 选 `build_codex_cmd`/`build_claude_cmd` 用的就是它）。`launch_family` 因此**没有单测**（它会读活的 `~/.aeroric`），可测部分抽成三个纯函数。core 的 `tasks/mod.rs` 补了 `ProtocolFamily`/`PermissionMode`/`TaskStatus` 再导出——src-tauri 只依赖 `aeroric-core`，此前宿主根本无法命名端口参数类型。
   门禁实测（本轮全绿）：`skeleton` 20/113、`protocol` 5/88、`storage` 5/184、`projects` 4/18、`tasks` 7/106、`doctor` 18/18、`contracts --check` 12 份逐字节一致、`inventory --check` consistent、core 233 passed、src-tauri **2433 passed / 0 warning**、`clippy -D warnings` 零错误。
3. **已完成（同日，ADR-029）**：`native_resume_family` 端口方法 + `resumeFamily` 记录字段 + 契约 `familySplit` 三字段三方断言 + 变异测试（换回 `session_family` 立即 2 条红）。**同一次错误在三个地方各活了一次**：parity 表 45 条、`task_lifecycle.rs` 的切换集成测试（断言过 `Resume`）、`AgentFamilies` 端口文档（拿 claude↔gpt55 当同族互信的例子），三处均已按 ADR-029 纠正。一处已知未覆盖：`apply_resolved_session` 不写 `resumeFamily`（加了会破坏 parity 的 `applyResolvedTaskSession` 期望），故转录跨族迁移时该字段可能落后，靠推导兜着——留给 P3-02 连同 G18 一起收。
4. **下一步（原口径已过时，见 §4.8）**：`TauriEventSink` + legacy DTO 转换、`legacy_task_executor.rs`、`TaskManager` 全量重键、注册 `tauri-adapter` 门禁。
   **已完成（2026-09-23，补记三十五～三十六）**：七条命令体的 `*_core` 抽取（local/SSH/WSL/DSH 的 launch+resume+cancel，加 `reset_task_process_core`）、`TauriEventSink`（补记三十三）、`legacy_task_executor.rs`（`OutputSinkMailbox` + 四目标分发 + 8 条用例）。端口为此长出 `operation_key`、`WorkingDirectory::Ssh.connection_id`、`permission_mode_wire` 三个字段——都是"端口装不下的宿主事实"，不是设计偏好。DSH 的 launch 仍走它自己的命令，因为 `workspace_id`/`prompt_mode`/`client_time_zone` 进不了端口（执行器对此返回点名理由的 `Unsupported`）。
5.5. **接线前必须先定的两件事（本轮新记录，别再靠记忆）**：
   - **spawn 之后的失败没有回音路**。端口是同步的，内核是 async，`Err` 无处可还；按 ADR-023 它应当作为 `TaskEvent` 经 observer 回来。**"旧 GUI 经真实 executor 完成一项 fake 任务"这条验收必须包含一条失败注入**，否则回音路是空的而测试仍是绿的。
   - **执行器尚未接进产品路径**（`LegacyTaskExecutor` 带 `#[allow(dead_code, reason = ...)]`）。接线的真正前置不是代码而是 ADR-021/022：旧客户端仍在写任务表，切过去之前必须先让 React 侧交出写权（Action 3 的 store 改造 + 保存队列改成只排队意图），否则 core 每次写都撞 ADR-011 的指纹拒绝。**这是一条排期事实，不是缺口**：P2-05 的"移交"包含它，Phase 2 Gate 不放行就不进 P3。
4. **`tauri-adapter` 门禁模块未注册**。注册时按每条 ADR 配机械检查（并在同一次编辑里写 `runner.floors`）：
   - ADR-020/025：src-tauri 内 generation 编号只有一处来源；`STARTUP_SIGNAL_GENERATION` 不得复活。
   - ADR-021：非 macOS 构建不得递交 writer 角色。
   - ADR-022：队列类型不再携带整表快照（`save_project_tasks` 不得出现在宿主的写路径上）。
   - ADR-023：`session*.rs`/`event_watcher.rs` 内不再出现 `"task-status"`/`"task-session"` 字面量，**并配正控**证明扫描真读到内容。
   - ADR-024：分组导入幂等（导入前后名单逐字节相同）。
   - ADR-026：`REVISION_CONFLICT` 的重放次数上限为一次，且状态 op 不重放。
   - ADR-027：只读实例的所有写入口被拒；`list_startup_degradations` 报告该状态。
5. **未决：`is_task_active` 这道抑制闸**。`task-status` 的 4 个调用点里 3 个走 `emit_active_task_status`（先查 `is_task_active(app, task_id)` 再发）、1 个不查；`task-session` 的 6 处都不查。所以"哪些事件被抑制"是**逐调用点**的决定，不是事件级策略。`TauriEventSink` 现在**故意不带这道闸**——带上就等于替那八个点各自做一次决定，那是行为变更而不是搬运。删掉那八处发射点之前必须先定：抑制语义留在哪一侧、什么粒度。
6. **验证边界（诚实口径）**：Linux/Windows 的 `cfg` 分支只做到 `cargo check --target x86_64-unknown-linux-gnu|x86_64-pc-windows-msvc --tests` **编译通过**，运行时未测——本机无这两个平台的可执行环境。`TauriEventSink::notify` 需要 `AppHandle`，因此**只有 payload 构造器有单测**（三条用例钉住四键/五键/六键的形状），真实 emit 要到接线那一步才有覆盖。`RunRegistry` 与 `TauriEventSink` 目前都带 `#[allow(dead_code, reason = "…next step of P2-05")]`：尚未接进产品路径，这件事是写下来的，不是靠警告被忽略。
7. **ADR-023 的可核对判据（不是"看起来只有一个发射者"）**：`contracts/native/v1/events.schema.json` 逐事件统计 `emitters`。加进 sink 之后契约如实漂了两个数：`task-status` **22 → 23**、`task-session` **6 → 7**——这就是"过渡期同一事件有两个发射者"被工件记下来的样子。删除那 8 处 legacy 发射点后，这两个数必须**降回 1**（只剩 `TauriEventSink`），`tauri-adapter` 门禁应当断言这两个数字本身；否则"core 是唯一发射者"只是一句口号，而双向事件回环恰恰发生在这两个计数悄悄都大于 1 的时候。
