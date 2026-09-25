# Module review — `tauri-adapter` (Task P2-05)

- **Reviewed**: 2026-09-23
- **Design source**: `MIGRATION_PLAN.md` §Phase 2 · Task P2-05
- **Code（本模块目前唯一已合并部分）**: `src-tauri/src/storage_location.rs`、`src-tauri/src/storage.rs::windows_private_acl_receipt`、`src-tauri/src/lib.rs` 的 `mod storage_location;`
- **Gate**: `pnpm native:check tauri-adapter` —— **已注册并全绿**（9 case／2488 用例：`adapter-invariants-src` + `adapter-invariants-unit`（地板 11）+ `adapter-acceptance-real-kernel`（地板 6，P2-05 验收 #2 的证据本身 + 启动接线实跑一遍，串行跑）+ `src-tauri-lib-tests`（地板 2466）+ `adapter-event-payload-contract`（地板 5，Action 5 点名的 TS 侧载荷契约）+ src-tauri/core `cargo fmt --check` + src-tauri `clippy --all-targets -D warnings` + `pnpm typecheck`）。脚本：`scripts/native/adapter-invariants.mjs` / `.test.mjs`；用例：`src-tauri/src/core_adapter_tests.rs`、`src/test/native-core-adapter.test.ts`
- **Gate status**: 拷问**已闭合两轮**——Q44～Q57 → ADR-020～029、Q59～Q61 → ADR-030/031/032、Q62～Q63 → ADR-033/034（2026-09-24 第三轮，Q62 再次否掉窄路推荐）。收口提交 `1279520`/`bf57afa`/`cef9e15`/`88d3646`，不 push、不 tag。**任务仍未 accepted，但原因变了**：§1 的三条验收**现在全部有证据**（#2 见 §4c，2026-09-24 按用户拍板的 B 路补上），剩下的不是缺证据，而是还有活没干完——ADR-023 的那 8 处 legacy 发射点还在，执行器还没接进产品路径（前置是 Action 3 + ADR-022，属排期）。

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

**续轮（重键第二波前置，2026-09-23）**

| # | 问题 | 用户决定 | 落点 | 是否否掉推荐 |
| --- | --- | --- | --- | --- |
| Q59 | `cancelled_tasks`/`manually_completed_tasks` 按代记还是按任务记？（今天两者都是**读时消费**） | **任务级共享一条，最后一个退出的代销账**（否掉"意图操作 fan-out 按代写"的推荐） | ADR-030 | ✅ 否掉 |
| Q60 | 会话表按代键后，路径 claim 何时释放？ | **不新增状态**：删本次条目后**反查四张会话表**，仍有人引用就不放；锁序统一为"会话表 → claim" | ADR-031 | 采纳 |
| Q61 | `STARTUP_SIGNAL_GENERATION` 删除后 unminted 两代共享 0 会回归，怎么办？ | **保留该序号但降格为"条目版本戳"**，作用域钉死在 signal 表内，不作运行身份；其余容器仍只认 core 的代 | ADR-032（修订 ADR-020 决策 2） | 采纳 |
| Q62 | ADR-023 的移交面实测是 28 处（22+6），其中 `dsh_webui.rs` 占 9 处而执行器按 ADR-021 明确拒绝驱动 dsh。这一步交哪一部分？（推荐：只交 core 今天真能驱动的 18 处，dsh 留到 P3-05） | **全量 28 处现在就交 core**，含 dsh 那 9 处；因此必须先建 runtime→core 观察通道（把 P3-05 的通知那一半提前） | ADR-033 | ✅ 否掉 |
| Q63 | 那 3 处走 `is_task_active` 的抑制（迟到 watcher 事件不得复活已结束任务）移交后算谁的决定？ | **进 core 观察通道，成为一条规则**：来自观察的变更若该任务已无存活代则 `Dropped`；边界只覆盖转录/hook 来源 | ADR-034 | 采纳（推荐） |

本轮从源码查到、改变问题形状的四条事实：① 两个标记集合今天都是 `remove()` 返回值当判定（一次性消费）；② dsh 另有一套自己的 `cancelled_tasks`，不在 `TaskManager` 内；③ omp/dsh 的登记侧**在持有 claim 的同时插会话表**，与 `lib.rs` 锁序表第 3 条相反（今天不致死锁，但表已名不副实 → ADR-031 决策 3 定死为"会话表 → claim"）；④ `wsl_active_ids` 同时装 shell id 与 task id，判空用于"还有没有 WSL 侧活运行"，属**存活**语义，按 RunKey 集合走即可，不需要新决策。

版本口径不是问题（不必问）：ADR-009 已定"v1 内部兼容性一律由具名能力承载，绝不由版本号推断，引入 v2 需另立 ADR"，所以这是**v1 内新增可选字段 + 一个能力位**，不 bump、不升 v2。

**本轮否掉推荐共 4 次：Q44、Q53、Q55、Q56。** 共同点很一致：**要更完整、更显式、更少自动行为**——不做收窄的键改造（要全量）、不自动重放会起进程的操作（要人拍板）、不复用单个 `family` 字段（要拆成语义正确的两个）、不只在端口里拆（要落进记录钉死）。迁移期把"少动一步"当作风险而不是收益；这条口径供后续任务沿用，也意味着**在这份计划里推荐"更省事"的选项要准备好被否**。

这条拆分把 P2-05 的实现面扩大了一圈（`AgentFamilies` 端口拆两个方法、任务记录新增两字段、`aeroric-protocol` 类型、fixtures、Swift/TS 两侧生成物、283 条 parity 期望重算），而"claude_gpt55 到底该用哪套启动参数"的真裁决留给 P3-02（缺陷 G18）。

## 3. 事实修正（侦察阶段我报错过、实现时才发现的三条）

1. **"`generation` 在 src-tauri 里根本不存在"是错的**。实际三套并行编号：`pty.rs:1205-1229` 进程全局 `AtomicU64`（作 `initial_input_signals` 值的判别字段）、`lib.rs:241-266` 的 `Arc::ptr_eq`（其注释已点名"配置切换复用 taskId"这个危害本身）、`dsh_webui.rs:217-243` 按 agent 的 `$events` 代际。这条错误会让重键被当成绿地工程，实际是**替换既有身份**。
2. **`statvfs` 在 macOS/BSD 上没有 `f_fstypename`**（编译期 E0609）；文件系统名在 `statfs` 上，Linux 也只有 `statfs::f_type`。计划原文的"`statvfs`/`getmntinfo`"改成：两平台都走 `statfs`、各读各字段，`getmntinfo` 不需要。Linux 魔数表逐条对照 `include/uapi/linux/magic.h` 核对，并加"与本地表不相交"断言（凭记忆写的两条假值已删，其中一条是 BTRFS 真值的近似抄写错）。
3. **发射点数量：本文件曾记 `task-status`/`task-session` 相关 ~8 处（包装函数 4 个调用点 + `task-session` 6 处字面量），这个数字是错的**（2026-09-24 按 `command-map.json` 的 `events[].emitters` 重数）。真值：`task-status` 23 个 emitter（含 core 的 sink ⇒ legacy 22）、`task-session` 7 个（含 sink ⇒ legacy 6），legacy 22 处分布 `dsh_webui.rs` 9 / `pty.rs` 5 / `ssh.rs` 3 / `wsl.rs` 3 / `session.rs` 1 / `event_watcher.rs` 1。漏数的原因是只数了包装函数的调用点，没数绕过包装函数的直接 `.emit`。全仓 `.emit(` 共 91 处，与移交无关的约 63 处不动。详见 ADR-033。
4. Q48 不做 ADR 的理由：它是交互层选择，改法是局部替换而非语义迁移；同时它带来一条必须记的事实——`reconnectTask`（`taskDeleteDone.ts:134-144`）是 `reset` + `resume` 的**程序化**续跑，只改 resume 按钮会让 reconnect 绕开确认闸。

## 4. 待办与阻塞（按依赖序）

1. **`TaskManager` 全量重键（ADR-020 + ADR-025）——第一波已完成（2026-09-23）**：`RunKey{ id, generation }` 进 `run_registry.rs`，`generation == RunKey::UNMINTED(0)` 是"这次运行不是 core 起的"的保留常量（不是第四个计数器；core 的编号从 1 起，所以未 minted 永不会赢得寻址竞争）。已重键：`pty_masters`/`pty_writers`/`child_handles`/`pending_pty_sizes` + `TaskManager.runs`；`register_pty_handles` 收 `&RunKey` 并在三张表都插好后才 `runs.register`；`remove_pty_handles*` 收 `&RunKey` 并 retire；`Arc::ptr_eq` 从判定依据降为 `debug_assert` 安全网。两条重键逼出的语义区别已写成断言/helper：**存活判断看"任意一代"（`has_live_run`），资源判断看"最新一代"（`resource_run`/`writer_for`/`master_for`）**；`pending_pty_sizes` 是"投递给这个 id 的下一次运行"的信箱，读写一律 `unminted`。**wave 2 已完成（2026-09-23，Q59～Q61 → ADR-030/031/032）**：取消/完成保持任务级一条账、由最后一个退出者销账（三处消费点收敛到 `settle_marks_for_exit`）；四张会话表按 `RunKey` 键，claim 走 `release_session_path_if_unreferenced` 反查释放；`initial_input_signals` 按 `RunKey` 键且 `STARTUP_SIGNAL_VERSION` 降格为条目版本戳；`wsl_active_ids` 按 `RunKey`；锁序表第 3 条重写为"谁嵌套谁定序"并新增 3a（先 retire 再读 `runs`）。计数安全网：src-tauri `--lib` **2459 passed**、clippy `-D warnings` 与 fmt 干净、两次变异反例成立。
2. **已完成（2026-09-23）**：`core_adapter.rs::CoreGateway`（writer 交接判定 + 只读回退，9 条测试）与 `SettingsFamilies`（ADR-028 的宿主侧）。**关键事实修正**：`launch_family` 不能查表——`app_settings/launch_spec.rs:400-406` 是**读用户 start 脚本内容**动态推断的，写死一张表会覆盖真实配置并改变实际执行的命令行；因此宿主委托给 `get_agent_launch_spec()`（`pty.rs` 选 `build_codex_cmd`/`build_claude_cmd` 用的就是它）。`launch_family` 因此**没有单测**（它会读活的 `~/.aeroric`），可测部分抽成三个纯函数。core 的 `tasks/mod.rs` 补了 `ProtocolFamily`/`PermissionMode`/`TaskStatus` 再导出——src-tauri 只依赖 `aeroric-core`，此前宿主根本无法命名端口参数类型。
   门禁实测（本轮全绿）：`skeleton` 20/113、`protocol` 5/88、`storage` 5/184、`projects` 4/18、`tasks` 7/106、`doctor` 18/18、`contracts --check` 12 份逐字节一致、`inventory --check` consistent、core 233 passed、src-tauri **2433 passed / 0 warning**、`clippy -D warnings` 零错误。
3. **已完成（同日，ADR-029）**：`native_resume_family` 端口方法 + `resumeFamily` 记录字段 + 契约 `familySplit` 三字段三方断言 + 变异测试（换回 `session_family` 立即 2 条红）。**同一次错误在三个地方各活了一次**：parity 表 45 条、`task_lifecycle.rs` 的切换集成测试（断言过 `Resume`）、`AgentFamilies` 端口文档（拿 claude↔gpt55 当同族互信的例子），三处均已按 ADR-029 纠正。一处已知未覆盖：`apply_resolved_session` 不写 `resumeFamily`（加了会破坏 parity 的 `applyResolvedTaskSession` 期望），故转录跨族迁移时该字段可能落后，靠推导兜着——留给 P3-02 连同 G18 一起收。
4. **下一步（原口径已过时，见 §4.8）**：`TauriEventSink` + legacy DTO 转换、`legacy_task_executor.rs`、`TaskManager` 全量重键、注册 `tauri-adapter` 门禁。
   **已完成（2026-09-23，补记三十五～三十六）**：七条命令体的 `*_core` 抽取（local/SSH/WSL/DSH 的 launch+resume+cancel，加 `reset_task_process_core`）、`TauriEventSink`（补记三十三）、`legacy_task_executor.rs`（`OutputSinkMailbox` + 四目标分发 + 8 条用例）。端口为此长出 `operation_key`、`WorkingDirectory::Ssh.connection_id`、`permission_mode_wire` 三个字段——都是"端口装不下的宿主事实"，不是设计偏好。DSH 的 launch 仍走它自己的命令，因为 `workspace_id`/`prompt_mode`/`client_time_zone` 进不了端口（执行器对此返回点名理由的 `Unsupported`）。
5.5. **接线前必须先定的两件事（本轮新记录，别再靠记忆）**：
   - **spawn 之后的失败没有回音路**。端口是同步的，内核是 async，`Err` 无处可还；按 ADR-023 它应当作为 `TaskEvent` 经 observer 回来。**"旧 GUI 经真实 executor 完成一项 fake 任务"这条验收必须包含一条失败注入**，否则回音路是空的而测试仍是绿的。
   - **执行器尚未接进产品路径**（`LegacyTaskExecutor` 带 `#[allow(dead_code, reason = ...)]`）。接线的真正前置不是代码而是 ADR-021/022：旧客户端仍在写任务表，切过去之前必须先让 React 侧交出写权（Action 3 的 store 改造 + 保存队列改成只排队意图），否则 core 每次写都撞 ADR-011 的指纹拒绝。**这是一条排期事实，不是缺口**：P2-05 的"移交"包含它，Phase 2 Gate 不放行就不进 P3。
4. **`tauri-adapter` 门禁已注册（2026-09-23，7 case／2470 用例）**：`scripts/native/adapter-invariants.mjs`（对真实源码树）+ `.test.mjs`（11 条负例，地板 11）+ `src-tauri-lib-tests`（地板 2459）+ src-tauri/core `cargo fmt --check` + src-tauri `clippy --all-targets -D warnings` + `pnpm typecheck`。逐条 ADR 的落实状态：
   - ✅ ADR-020/025/032：`STARTUP_SIGNAL_GENERATION` 不得复现；`STARTUP_SIGNAL_VERSION` 只允许 `pty.rs` 的登记/清除两处被读（含"版本戳没人读＝规则没在检查"的反口号守卫）。
   - ✅ ADR-021：`writer_handover_allowed()` 体必须是 `cfg!(target_os = "macos")`，出现 windows/linux 即红。
   - ✅ ADR-031：claim 的 remove 只准出现在 `release_session_path_if_unreferenced` 内（helper 里若不 remove 也报错）。
   - ✅ ADR-030：靠 `--lib` 用例计数地板 + 3 条专项测试（含两次变异验证其可失败性）。
   - ✅ **Action 4 调用方向（2026-09-25 新增，第五条规则）**：`legacy command → TaskService → executor → *_core` 不得回头。可判定的两种破坏面各有一条负例——回头调 command wrapper（命令名取自 `command-map.json` 的 660 条注册表，因此**新增命令自动纳入**，无需改脚本）与未委派就 `return Ok(`（无声 no-op）；`ExecutorFailure::Unsupported` 必须以 `Err(...)` 出门，否则等于拒绝被吞掉。扫描前过 `codeOnly()`：清空注释但保留字节长度与行数，所以报出的行号仍指向真代码，而字符串里的 `//` 不当注释起点。`run_task_core(` 不被 `run_task\(` 误伤有一条近亲正控。真实树判定：5 个端口方法全部委派到位。单元负例 15 → **23**。
   - ⏳ ADR-022（队列不得携带整表快照）、ADR-024（分组导入幂等）→ 等 Action 3，而 Action 3 卡在 **Q67 未拍板**（intents 在哪执行，见计划补记五十七）；ADR-026（重放上限一次）→ 同因，且**全仓尚无重放实现**。这三条仍**记为未生效**并写在模块 `$note` 里，不注册成永远红或永远绿的假 case。
   - ✅ **ADR-023 已生效（2026-09-24，补记五十二～五十四）**：28 处 legacy 发射点全部删除，`task-status`/`task-session` 的 `emitters` 都是 1，由上面那条 ADR-023 规则逐家族断言"恰好一行且必须在 `core_adapter.rs`"。**ADR-027 决策 4 的常驻指示也已生效（补记五十五）**：`ReadOnlyStorageBadge` 挂在侧边栏底部，toast 之外那份不过期的告知补上了。
4b. **验收 #2（真 executor 跑 fake 任务）不可写，原因不是存储根，是 Wry 具体类型**（2026-09-23 实测，不是推测）：
   - 先解决了一个次要障碍：`platform::home_dir()` 在 unix 上就是读 `$HOME`，加一个 `#[cfg(test)]` 专用重定向确实可行，且现存三处读 `home_dir()` 的测试（`config.rs`、`sftp.rs`、`notebook/export.rs`）都是"与 `home_dir()` 自比"，重定向不会让它们断言到假东西。
   - **真正的墙**：`LegacyTaskExecutor` 持有的必须是 `AppHandle<Wry>`，因为 `launch` 要一路调进 `run_task_core → spawn_pty_reader → send_pty_chunk`，这条链上的签名是 Wry 具体的；而 `tauri::test` 只产出 `App<MockRuntime>`。编译器直接把这条路堵死：`error[E0308]: expected AppHandle, found App<MockRuntime>`。`complete_dsh_task_core` 那类泛型先例救不了它——同一次尝试里 `run_dsh_task_core<R: Runtime>` 就已经因为体内要调具体 `AppHandle` 的函数而编译失败（补记三十五记过）。
   - 因此这不是"我没找到办法"，而是**任务本身的前提**：能在无 webview 环境里驱动宿主启动路径，等价于 P3-01 的标题——"提取运行时端口，替代 `AppHandle` 与全局查找"。P2-05 的验收 #2 写在 P3-01 之前，是计划里的一处顺序缺陷。
   - 两条出路（需用户拍板，§1 的三条验收目前只有 #1 与 #3 有证据）：**(A)** 把 #2 随 P3-01 一起做（端口化之后测试自然可写，代价是 P2-05 只能标 "accepted-with-deferred-criterion"，Phase 2 Gate 要显式放行）；**(B)** 现在就把 `send_pty_chunk`/`spawn_pty_reader`/`run_task_core` 这条链做成 `R: Runtime` 泛型（约 6 个函数 + 若干调用点，属于提前支付 P3-01 的一部分成本）。**没有 (C)**：用记录型假执行器代替只能证明"假执行器被调用"，那正是 P2-04 关掉这条验收时给过的理由。
4c. **验收 #2 已按 (B) 路拿到证据（2026-09-24，实测不是推测）**：
   - **泛型化实际范围 = 45 个函数 ×7 文件**（`pty/session/ssh/wsl/session_dsh/session_omp/event_watcher`），不是本节估的「约 6 个」。签名加了 `<R: tauri::Runtime>` 之后，参数 `app: AppHandle` 必须逐个跟改成 `AppHandle<R>`；批处理还漏掉一类只有编译器能抓到的：**函数体内嵌套的 `fn`**（`session.rs` 的 `write_to_pty`）不在「签名区间」里，正则扫不到。
   - **存储根**：`platform::install_scratch_home` + RAII Guard（`#[cfg(test)]` 的 `Mutex<Option<PathBuf>>`）。**刻意不用 `set_var("HOME")`** —— `permissions/mod.rs:291` 与 `permissions/linux.rs:55` 记着本仓库已因并行测试把该做法移除，给出的替代是喂参数；但宿主侧所有路径都从 `platform::home_dir()` 起、没有入参可喂，所以我们自己的加锁槽是唯一同时满足「能改根」与「不碰 ambient env」的做法。
   - **两条只有跑起来才知道的事实**（P3-01 同样需要知道）：
     1. **版本探测在启动路径上是同步等待的** —— 而且"写 marker 再等闸门"的脚本会把每一次探测变成一个**永久孤儿进程**：`ps` 实测 10 个挂了 1h45m，其临时 home 已被 Drop 删除，所以测试全绿也看不见这个泄漏。`[ -t 1 ]` 同时解决误计数与泄漏（之后四次全量/门禁跑零新增孤儿）。 —— `run_task_core` 走到 `register_pty_handles` 之前会跑 `<agent> --version`。fake agent 只要「写 marker 然后等闸门」，探测进程就会**把整条启动路径钉死**（症状：`spawns=1` 但 `runs=[]`，看着像注册逻辑坏了）。判别器是 `[ -t 1 ]`：真启动持有终端，探测的 stdout 是管道。
     2. **`load_settings_unlocked()` 在 `settings.json` 缺失时会创建并缓存默认值** —— 先装根再写 fixture 的话，本二进制里任何一条读设置的测试都会把默认设置落进我的临时 home，fake agent 查不到档案，按 `unknown_session_family` 退成 Codex 族。顺序因此是硬约束：**整个 home 先建好再安装**，析构时**先清槽再删目录**。
   - **五条用例**（`src-tauri/src/core_adapter_tests.rs`，门禁 case `adapter-acceptance-real-kernel`，`--test-threads=1`）：一次 start ⇒ 一个进程 + 宿主 maps 恰为 `RunKey::minted(task,1)` 且不含 `unminted` + `running` 只发一次 + 表 digest 只在 start 变一次；换 key ⇒ 第二代共存且 `resource_run`/`intent_keys` 按 ADR-025 分流；同 key 重放 ⇒ `AlreadyClaimed`、0 额外 spawn、不预留代、不重写表；`cancel` ⇒ 注册与句柄全清；**失败注入**（§4.5.5 要求的）⇒ 端口处拒绝的 launch 记为 `failed` 并释放 claim，之后真重试能起来。
   - **这次写作纠正的两条不实**：① 我原以为被拒的 launch 会**回滚**状态，实测 `abandon_launch` 记的是 `failed`（对齐 `reportLaunchFailure` → `updateTaskStatus(failed)`），后者才是与旧客户端一致的语义；② `Applied.generation` 的注释写着「switch or start」，实测只有 `switch_config` 会填 —— start/resume 的代已经在 `LaunchRequest.generation` 里给了宿主，返回值再带一份就是第二个可互相矛盾之处，注释已改。
   - **共存性**：默认并行全量 `--lib` 连续两跑 **2464 passed / 0 failed**。中途 `remote::lan_roundtrip_tests::m3_…` 出现过一次 WS 超时，按记录流程归因（`cp` 备份 → 用 `git show 88d3646:` 覆盖回 HEAD 跑通 → 校验 sha256 还原 → 带改动单跑 3/3 通过），结论是那条测试本就有的超时敏感，不是本次回归；全程未用 `git checkout`/`git stash`。
   - ADR-020/025：src-tauri 内 generation 编号只有一处来源；`STARTUP_SIGNAL_GENERATION` 不得复活。
   - ADR-021：非 macOS 构建不得递交 writer 角色。
   - ADR-022：队列类型不再携带整表快照（`save_project_tasks` 不得出现在宿主的写路径上）。
   - ADR-023：`session*.rs`/`event_watcher.rs` 内不再出现 `"task-status"`/`"task-session"` 字面量，**并配正控**证明扫描真读到内容。
   - ADR-024：分组导入幂等（导入前后名单逐字节相同）。
   - ADR-026：`REVISION_CONFLICT` 的重放次数上限为一次，且状态 op 不重放。
   - ADR-027：只读实例的所有写入口被拒；`list_startup_degradations` 报告该状态。
5. **`is_task_active` 抑制闸——已于 2026-09-24 定案（Q63 → ADR-034）**：成为 core 观察通道上的一条规则（迟到的转录/hook 观察在任务已无存活代时 `Dropped`）。下面这段是定案前的原始记录，保留供对照。`task-status` 的 4 个调用点里 3 个走 `emit_active_task_status`（先查 `is_task_active(app, task_id)` 再发）、1 个不查；`task-session` 的 6 处都不查。所以"哪些事件被抑制"是**逐调用点**的决定，不是事件级策略。`TauriEventSink` 现在**故意不带这道闸**——带上就等于替那八个点各自做一次决定，那是行为变更而不是搬运。删掉那八处发射点之前必须先定：抑制语义留在哪一侧、什么粒度。
6. **验证边界（诚实口径）**：Linux/Windows 的 `cfg` 分支只做到 `cargo check --target x86_64-unknown-linux-gnu|x86_64-pc-windows-msvc --tests` **编译通过**，运行时未测——本机无这两个平台的可执行环境。`TauriEventSink::notify` 需要 `AppHandle`，因此**只有 payload 构造器有单测**（三条用例钉住四键/五键/六键的形状），真实 emit 要到接线那一步才有覆盖。`RunRegistry` 与 `TauriEventSink` 目前都带 `#[allow(dead_code, reason = "…next step of P2-05")]`：尚未接进产品路径，这件事是写下来的，不是靠警告被忽略。
6b. **端口曾经装不下 `failure_reason`——这是删发射点前必须先修的前提（2026-09-24 实测，见补记四十二）**：`useAppTauriEvents.ts` 解构 `{ task_id, status, failure_reason? }` 并把 reason 一路喂到 `storage.rs` 的持久字段，而 5 处 legacy 发射点在 failed 路径上都带这个键；`TaskNotification::Status` 当时只有 `{task_id, status}`，所以照 ADR-023『payload 不变』去删那 8 处会**在删除的当下静默丢掉失败原因**且无任何东西变红。已按 `TaskSessionFacts::family` 的同一口径补 `Option<String>` 并"只在 Some 时带上键"。守卫是新登记的 `adapter-event-payload-contract`（TS 侧，三方键集全部从源码扫出：客户端 `listen<{…}>` 类型字面量 × `src-tauri` 各 `.emit(...)` 的 `json!` 字面量 × core 两个构造函数），断言方向 legacy ⊆ core。可失败性已用变异验证（改回不带键 → 立刻红），正控断言的是**键集合非空**——第一版只数发射点个数，于是一个把键扫成空的括号 bug 让最关键那条真空通过。
7. **ADR-023 的可核对判据（不是"看起来只有一个发射者"）**：`contracts/native/v1/events.schema.json` 逐事件统计 `emitters`。加进 sink 之后契约如实漂了两个数：`task-status` **22 → 23**、`task-session` **6 → 7**——这就是"过渡期同一事件有两个发射者"被工件记下来的样子。删除那 8 处 legacy 发射点后，这两个数必须**降回 1**（只剩 `TauriEventSink`），`tauri-adapter` 门禁应当断言这两个数字本身；否则"core 是唯一发射者"只是一句口号，而双向事件回环恰恰发生在这两个计数悄悄都大于 1 的时候。

## 5. 发射面逐站点清单与它的存活代前置（2026-09-24，ADR-033 §4 要求的工件）

ADR-033 §4：「每一处删除都必须能指出它现在经由哪个观察进入哪个 reducer 转移」。逐站点量出来的第一列
就是那个"哪个观察"。状态词是从源码的 `json!` 字面量里读的，不是记忆。

| 站点 | 事件 | 发的状态 | 移交后进入的观察 |
| --- | --- | --- | --- |
| `dsh_webui.rs:1473` | task-status | `running` | `Status(running)`（turn/start） |
| `dsh_webui.rs:1488` | task-status | `cancelled` | `Status(cancelled)` |
| `dsh_webui.rs:1507` | task-status | `input_required` | `Status(input_required)` |
| `dsh_webui.rs:1931/1962` | task-status | `input_required` | 同上（两条不同的等待边） |
| `dsh_webui.rs:1937/2029` | task-status | `pending` | `Status(pending)` |
| `dsh_webui.rs:1973` | task-status | `failed` | `Status(failed, reason)` |
| `dsh_webui.rs:2108` | task-status | `done` | `Exit(code 0)` |
| `pty.rs:232` | task-status | 包装函数体 | 见下方"共享出口" |
| `pty.rs:1970` / `2734` | task-status | `running` | `Status(running)`（launch / resume） |
| `pty.rs:2153` | task-status | `cancelled` | `Status(cancelled)` |
| `pty.rs:2203` | task-status | `done` | `Exit` |
| `ssh.rs:1388` / `wsl.rs:1022` | task-status | `running` | `Status(running)`（launch） |
| `ssh.rs:1347` / `wsl.rs:972` | task-status | 包装函数调用点 | 见下方"共享出口" |
| `ssh.rs:1933` / `wsl.rs:1736` | task-status | `cancelled` | `Status(cancelled)` |
| `session.rs:63` | task-status | 参数 `status` | **共享出口**：`emit_task_status` 自己，删它一处即覆盖其全部调用点 |
| `event_watcher.rs:319` | task-status | hook 写的状态（另带一个 `approval` 键，前端无读取点） | `Status`（hook 观察） |
| `session.rs:1839/2018/2097`、`event_watcher.rs:264`、`session_dsh.rs:695`、`session_omp.rs:53` | task-session | — | **注意**：这不是观察，是"宿主治好了一个路径事实"。core 里对应的入口是 `apply_resolved_session`，不是 `observe` |

### 5.1 删除的前置不是 dsh 独有——是全部 legacy 启动（本轮实测）

`TaskEvent` 的存活判断按 ADR-034 的定案口径是"这个任务还有没有存活代"，而 core 只在 `start`/`resume`
时取 claim。**产品路径今天一条 claim 都不产生**：`lib.rs:672` 的注释自己写着"仍由 `storage.rs` 保存，
没有任何 mutation 走 `TaskService`"。所以上表里 5 处 launch 报的 `running`（`pty.rs:1970/2734`、
`ssh.rs:1388`、`wsl.rs:1022`、`dsh_webui.rs:1473`）一经删除就会全部被判"无存活代"而静默。

因此"宿主报告的存活代"（任务 #14）是**整张表的前置**，不是 dsh 那一格的前置。两条机制约束：

1. **不得伪造 `Claim`**。claim 承载的是"一次尝试只 spawn 一次"的幂等账（ADR-016/ADR-017），宿主报的
   运行没有 `operation_key`，塞进同一张表会让"core 是否已为这个 key 起过进程"变成两个来源。
   存活代要独立一份，`observe` 的准入判据取 **两者之并**。
2. **`RunKey::UNMINTED(0)` 与真实代同等参与**。今天 `is_task_active` 问的就是"这个 id 有没有任意存活代"，
   按 0 登记恰好等价；把它当成"没有代"就是把 ADR-034 的判据再次写宽。

### 5.1a 这条前置已于同轮关闭（Q65 → ADR-036）

上面第 1 条推荐的形态**被采纳并落地了**：core 不存镜像，而是持一个 `LiveRuns` 端口去问宿主，
`is_task_live = 有 core 自己的 claim ∨ 端口答"活"`。宿主侧实现就是 `TaskManager::has_live_run`
（`core_adapter.rs::HostLiveness`），generation 0 与真实代同等参与，因此 §5.1 第 2 条约束是
天然满足的，不需要额外机制。可核对性落在两条用例上：core 侧 `a_legacy_runs_report_is_held_up_by_
the_port_and_by_nothing_else`（含"端口被真的调用过"的计数断言），宿主侧
`the_wired_service_asks_the_host_about_a_run_core_never_minted`（唯一驱动 `install_core_services`
真建出来的那一对的用例——其余验收场景都喂 `NoLiveRuns`，接线接错在它们那里是隐形的）。

任务 #14 因此不是"再做一份登记"，而是**已经被端口取代**；剩下的只有 §5.2 的会话入口与 #12 的
task→project 解析。

### 5.1b 第二条前置也已关闭（补记五十：`project_of`）

§5.1 点的两件事里，"task→project 解析"已经落地：`TaskService::project_of(task_id)` 先查进程内
记忆、miss 时按 `projects.json` 扫每个项目的 `tasks.json`，**答不出是 `Ok(None)` 而不是猜一个项目**。
缓存之所以合法：`projectId` 创建时定死、之后不迁，`delete` 是唯一能让条目变错的操作，它在提交成功后
forget。用例 #32 要求盘上真有两个项目，两处变异（扫成"第一个非空项目"、去掉 delete 的 forget）分别验过。

现在 28 处删除只缺一样东西：宿主侧那条 `TaskRuntimeObserver` 实现，以及 `EventOutcome →
TaskNotification → sink` 的映射。机制层没有未关的前置了。

### 5.2b 落地实况（2026-09-24）：本节预测的入口没有用上，原因是时机而不是设计

上面写"这 6 处的移交判据是经由 `apply_resolved_session`"。**实际落地的是端口的第二个方法
`TaskRuntimeObserver::report_session`**，`apply_resolved_session` 至今没有被发射路径调用。
理由与状态家族的 commit 完全同构：那条函数会写记录，而写记录属于 Action 3 的 store 切换；
在 `storage.rs` 仍是保存者的今天从 announce 路径提交，就是 INV-03 要防的双写。
所以今天的移交只完成"唯一发射者"这一半（两个家族 emitters 各 = 1），会话字段落盘那一半
与状态提交同批接上。**本节保留原文不删**，因为它对"会话事实不是状态转移"的判断是对的，
错的只是我以为这一步就能同时把写路径接进去。

### 5.2 `task-session` 走的不是观察通道

6 处 `task-session` 报的是"这份转录现在在哪"，不是"某个运行发生了什么"。它在 core 里已有的对应入口是
`apply_resolved_session`（P2-04 落地，带 `SESSION_FIELDS` 合并），所以这 6 处的移交判据是"经由
`apply_resolved_session` 之后由 sink 发 `TaskNotification::Session`"，与 `observe` 无关。把它们塞进
观察通道会造出一个假的"会话状态机"。
### 5.3 发射面实测结果（2026-09-24，第一、二批发完之后）

`command-map.json` 的 `events[].emitters` 现在如实记着：**`task-status` = 1**
（`core_adapter.rs:503` 的 sink）、**`task-session` = 1**（`core_adapter.rs:509` 的同一行 sink）。§5.1 的"存活代登记"
前置被两条更准的机制取代：ADR-036 的 `LiveRuns` 端口回答宿主 `is_task_active` 的**全部**问题
（含 codex/claude 会话表，只答 run registry 会漏掉 lazy-attach 窗口），以及 ADR-034 更正二补上的
`ObservationOrigin`——dsh 那 9 处的正确分类是 `Runtime`，本来就不该过那道闸。

判据已进门禁：`adapter-invariants.mjs` 的 ADR-023 规则要求"已移交家族必须由 `core_adapter.rs`
的恰好一行发出"，配三条负例（多一个发射点 / sink 看不见 / 走 `event_names::` 常量发）。
两个家族都在规则里（`task-session` 是在它的 6 处删完之后并入的，没有先登记成一条永远红的 case），
负例 4 条，其中一条专门证明第二个家族也受管。

顺带记两条扫描器的错，都是门禁第一遍运行就报出来的：按行匹配看不见 sink 那种 `emit(` 与事件名
换行写的调用（会误报"没有 sink"），以及 `remote/lan_roundtrip_tests.rs` 里测试自己发的
`task-status` 被算成生产发射点（现在 `*_tests.rs` 显式排除，理由写在代码里而不是靠巧合）。

## 6. `src-tauri --lib` 的"负载敏感"是一条我造成的真缺陷（2026-09-25 结题）

补记五十三、五十四把两次红记成"并行负载敏感、单跑不复现"。那个归因是错的，本轮按退出码追到根因：

- **症状**：`exit 101`，`app_settings::tests::updating_a_generated_claude_profile_refreshes_the_
  wrapper_and_sidecar` 在 `mod.rs:854` 对 `fs::read_to_string(credential_path).unwrap()` 拿到
  `NotFound`。门禁只打印 `FAIL (exit 101)`，真实输出留在 `.artifacts/native/tauri-adapter--src-tauri-lib-tests.log`
  ——这条日志是被保留的，先前几轮没人去读它，所以"查不到"其实是"没去查"。
- **判据**：写 sidecar 与读 sidecar 用的是同一个 `agent_api_key_path()`，而它每次现算
  `aeroric_dir()` → `platform::home_dir()`；同一次运行里两次解析之间根换了，就是写在一处、读在另一处。
  决定性证据是 `~/.aeroric/agent-credentials/` 里留着 9 个 `claude-refresh-*`（测试 panic 在
  自己的清理之前），其中一个 mtime = 01:55:08 正对着那次失败——**文件确实写进了 live home**，
  所以读失败只能是读侧解析到了别的根。
- **根因**：`platform::install_scratch_home`（补记四十一为真内核验收引入）换的是**进程全局**的 home 槽。
  `core_adapter_tests` 的 10 个场景各自安装/卸载它；`SCENARIO` 互斥量只把这 10 个彼此隔开，
  其余 2460 条完全不参与，于是任何"写一次路径、稍后再算一次路径"的测试都在赌运气。
- **对照组**：把 app_settings 那条改成自带 scratch root、并给槽加上 `SERIAL`（`Guard` 持有到场景结束）
  之后，15 次并行全量跑仍红 **4 次**，受害者换成了 `agent_scripts::tests::
  resets_codex_chat_bridge_wrappers_when_setting_is_off`（3 次）和场景自己的
  `an_observation_announces_once_and_writes_nothing`（1 次，多收到一个 `done`）。
  这一步价值在于它证明了**逐条给受害者加根不是解**：类是"任何经 `aeroric_dir()` 的测试 vs 任何持槽场景"，
  成员数量是未知的，加锁只挡住了"根被别人换掉"这一个方向。
- **处置（围堵，不是治愈）**：`src-tauri-lib-tests` 这条 case 加 `--test-threads=1`。串行之没有两个测试
  同时活着，全局槽就不可能被子读到；实测 66.3s／2470 passed（并行 18s），门禁 11/11、2500 用例全绿。
  代价写进 case 的 `$note`：**开发者本地 `cargo test --lib` 默认并行仍会竞争**，这不是被解决了，只是被
  门禁挡住了。真正的修法是 Action 3 的数据根注入——让宿主存储路径不再只能从进程全局推导，那时这个
  测试专用槽应当整体删除。
- **顺手关掉的一条卫生问题**：那条测试此前一直往开发者的 live `~/.aeroric/agent-credentials` 写凭据
  （失败时留下孤儿文件），现在它拥有自己的 scratch root，写入落进临时目录。`SERIAL` 的注释也改成了
  它实际能保证的东西——它保护"自己装了根的测试"，不保护"没装根的读者"，早先那句"别的测试落到同一个
  目录也改变不了答案"是错的，它没算到反向：别人的写会落进**我的**根。


## 7. Action 3 的一条硬前置：宿主装不下 core 写的启动族（2026-09-25）

量 ADR-022 意图词汇表时撞上的，不是设计歧义而是会静默丢数据的事实链：

| 环节 | 证据 |
| --- | --- |
| core 的 `create` 往任务记录写 `launchFamily`/`sessionFamily`/`resumeFamily` | `core/crates/aeroric-core/src/tasks/service.rs:437-441` |
| 宿主 `Task` 只有 `session_family` | `src-tauri/src/storage.rs:191` |
| TS `Task` 只有 `sessionFamily` | `src/types.ts:349` |
| serde 静默忽略未知字段（无 `deny_unknown_fields`），且整份覆盖写 | `src-tauri/src/storage.rs:397-410` |

结论：**写权一递交，客户端每次保存都会把 ADR-028/029 钉死的启动族抹掉**——任务下次被换成另一族的 CLI 参数与会话文件格式，而两侧都不会报错。今天没有生产调用方走 `TaskService`（`src-tauri/src/lib.rs:672` 明写"仍由 storage.rs 保存"），所以是隐性缺陷；但它属于 Action 3 的前置，不是收尾项。

**为什么既有门禁看不见**：`src/test/task-model-parity.test.ts` 守的是 TS↔Rust 两侧字段集合**相等**，两侧同时缺一个键时它完美通过。缺的是第三边——"core 写的键 ⊆ 宿主装得下的键"。已补，扫描范围是 `core/crates/aeroric-core/src/tasks/` 的生产段，正控要求至少扫到 `id`/`status`/`agent`/三个族键。

**豁免按判据给，不按名单给**：`generation` 同样不在宿主结构体里，但它是 `stamp_generation_copies`（`service.rs:373-383`）每次写时从账本重盖的派生副本，丢了能自愈；`launchFamily`/`resumeFamily` 不可重算，所以必须声明。测试里另有一条"白名单键若 core 已不写就报错"，防豁免面只增不减。

**已落地的修复**：两侧各加两个可选字段 + `core_bridge.rs` 的逐字段 fixture 同步；v1 契约不变（`contracts --check` 仍 12 份逐字节一致）。可失败性：注释掉 `launch_family` 声明后两条守卫同时红并点名 `launchFamily`。

**顺带查到的意图词汇表规模**（Q67 的输入）：21 个持久化调用点，其中 7 处 core 无具名 op（todo 编辑、会话归属、worktree 挂载/丢弃、4 处回滚补偿写），但全部可由现成的 `TaskService::mutate` 表达；4 处回滚是补偿写而非用户意图，倾向留在宿主侧做字段级回退。

## 8. Action 3 的两块地基（2026-09-25）+ Q67 的结案方式

**Q67 不是开放问题。** 连着两轮想问用户"intents 在哪执行"，读 ADR 后发现早已判定：ADR-022 决策 2 明写读-改-写归 `TaskService`，其"后果"第 28 行把"继续发快照＋字段级重放"和"状态走 op、元数据走快照的混合"两个方案点名否掉——那正是我要问的两项。非递交平台的落盘问题则是 ADR-035 的 decide/commit 解耦换个调用点复用。**教训：把已闭的庭重新开成"待用户拍板"，看起来谨慎，实际是把决策成本推回去换取自己不动手的安全感。**

已落下的两块：

| 地基 | 内容 | 证据 |
| --- | --- | --- |
| 队列合一（补记六十三） | `projects.json` 单文件整份覆盖，却挂着 `appProjectState.ts:124` 与 `projectsStore.ts:9` 两条 promise 链，`AppOpsProvider.tsx:164-175` 的回退 ops 走后者 ⇒ `flushProjects()` 等不到 store 那份写，且两链互相覆盖 | `src/projectSaveQueue.ts` 单例；`src/test/project-save-single-queue.test.ts` 3 条；双实例下精确红 2 条、修好后绿，且**修好后又把双实例还原重跑一次**确认判别力 |
| 元数据 op（补记六十四） | `TaskService::{rename,toggle_star,archive,unarchive}`（`load → reducer → commit`） | ADR-022:17 声称这些"服务已有"，实际当时只是 `reducer.rs` 的纯函数；5 条新用例（含 reader context 拒写、批量缺一即整条 `NotFound`、重复归档不烧 revision） |

钉住的语义：空串改名删 `name` 键；批量归档不允许"只归档一半"；unstar 是一次真实写入（`starred:false`）而不是回到键不存在——这条我先写错在注释里，被自己的用例推翻后改的**是注释不是测试**。

下一步：mutation 的 decide/commit 拆分（否则非 macOS 上意图无处落盘），然后才是 21 处调用点改为声明 `op + payload + operationKey`。
