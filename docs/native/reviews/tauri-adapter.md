# Module review — `tauri-adapter` (Task P2-05)

- **Reviewed**: 2026-09-23
- **Design source**: `MIGRATION_PLAN.md` §Phase 2 · Task P2-05
- **Code（本模块目前唯一已合并部分）**: `src-tauri/src/storage_location.rs`、`src-tauri/src/storage.rs::windows_private_acl_receipt`、`src-tauri/src/lib.rs` 的 `mod storage_location;`
- **Gate**: `pnpm native:check tauri-adapter` —— **已注册并全绿**（8 case／2480 用例：`adapter-invariants-src` + `adapter-invariants-unit`（地板 11）+ `adapter-acceptance-real-kernel`（地板 5，P2-05 验收 #2 的证据本身，串行跑）+ `src-tauri-lib-tests`（地板 2464）+ src-tauri/core `cargo fmt --check` + src-tauri `clippy --all-targets -D warnings` + `pnpm typecheck`）。脚本：`scripts/native/adapter-invariants.mjs` / `.test.mjs`；用例：`src-tauri/src/core_adapter_tests.rs`
- **Gate status**: 拷问**已闭合两轮**——Q44～Q57 → ADR-020～029、Q59～Q61 → ADR-030/031/032（用户确认共识）。收口提交 `1279520`/`bf57afa`/`cef9e15`/`88d3646`，不 push、不 tag。**任务仍未 accepted，但原因变了**：§1 的三条验收**现在全部有证据**（#2 见 §4c，2026-09-24 按用户拍板的 B 路补上），剩下的不是缺证据，而是还有活没干完——ADR-023 的那 8 处 legacy 发射点还在，执行器还没接进产品路径（前置是 Action 3 + ADR-022，属排期）。

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

本轮从源码查到、改变问题形状的四条事实：① 两个标记集合今天都是 `remove()` 返回值当判定（一次性消费）；② dsh 另有一套自己的 `cancelled_tasks`，不在 `TaskManager` 内；③ omp/dsh 的登记侧**在持有 claim 的同时插会话表**，与 `lib.rs` 锁序表第 3 条相反（今天不致死锁，但表已名不副实 → ADR-031 决策 3 定死为"会话表 → claim"）；④ `wsl_active_ids` 同时装 shell id 与 task id，判空用于"还有没有 WSL 侧活运行"，属**存活**语义，按 RunKey 集合走即可，不需要新决策。

版本口径不是问题（不必问）：ADR-009 已定"v1 内部兼容性一律由具名能力承载，绝不由版本号推断，引入 v2 需另立 ADR"，所以这是**v1 内新增可选字段 + 一个能力位**，不 bump、不升 v2。

**本轮否掉推荐共 4 次：Q44、Q53、Q55、Q56。** 共同点很一致：**要更完整、更显式、更少自动行为**——不做收窄的键改造（要全量）、不自动重放会起进程的操作（要人拍板）、不复用单个 `family` 字段（要拆成语义正确的两个）、不只在端口里拆（要落进记录钉死）。迁移期把"少动一步"当作风险而不是收益；这条口径供后续任务沿用，也意味着**在这份计划里推荐"更省事"的选项要准备好被否**。

这条拆分把 P2-05 的实现面扩大了一圈（`AgentFamilies` 端口拆两个方法、任务记录新增两字段、`aeroric-protocol` 类型、fixtures、Swift/TS 两侧生成物、283 条 parity 期望重算），而"claude_gpt55 到底该用哪套启动参数"的真裁决留给 P3-02（缺陷 G18）。

## 3. 事实修正（侦察阶段我报错过、实现时才发现的三条）

1. **"`generation` 在 src-tauri 里根本不存在"是错的**。实际三套并行编号：`pty.rs:1205-1229` 进程全局 `AtomicU64`（作 `initial_input_signals` 值的判别字段）、`lib.rs:241-266` 的 `Arc::ptr_eq`（其注释已点名"配置切换复用 taskId"这个危害本身）、`dsh_webui.rs:217-243` 按 agent 的 `$events` 代际。这条错误会让重键被当成绿地工程，实际是**替换既有身份**。
2. **`statvfs` 在 macOS/BSD 上没有 `f_fstypename`**（编译期 E0609）；文件系统名在 `statfs` 上，Linux 也只有 `statfs::f_type`。计划原文的"`statvfs`/`getmntinfo`"改成：两平台都走 `statfs`、各读各字段，`getmntinfo` 不需要。Linux 魔数表逐条对照 `include/uapi/linux/magic.h` 核对，并加"与本地表不相交"断言（凭记忆写的两条假值已删，其中一条是 BTRFS 真值的近似抄写错）。
3. 发射点数量修正：`task-status`/`task-session` 相关 **~8 处**（包装函数 4 个调用点 + `task-session` 6 处字面量，分布于 `session.rs`/`session_omp.rs`/`session_dsh.rs`/`event_watcher.rs`），不是早先记录的 24；全仓 `.emit(` 共 91 处，与本模块无关的 ~83 处不动。
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
   - ⏳ ADR-022（队列不得携带整表快照）、ADR-024（分组导入幂等）→ 等 Action 3；ADR-023（两个 `emitters` 计数降回 1）→ 等删那 8 处发射点；ADR-026（重放上限一次）、ADR-027（只读实例写入口全拒）→ 等 `CoreGateway`/`TaskService` 进 managed state。这五条**记为未生效**并写在模块 `$note` 里，不注册成永远红或永远绿的假 case。
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
5. **未决：`is_task_active` 这道抑制闸**。`task-status` 的 4 个调用点里 3 个走 `emit_active_task_status`（先查 `is_task_active(app, task_id)` 再发）、1 个不查；`task-session` 的 6 处都不查。所以"哪些事件被抑制"是**逐调用点**的决定，不是事件级策略。`TauriEventSink` 现在**故意不带这道闸**——带上就等于替那八个点各自做一次决定，那是行为变更而不是搬运。删掉那八处发射点之前必须先定：抑制语义留在哪一侧、什么粒度。
6. **验证边界（诚实口径）**：Linux/Windows 的 `cfg` 分支只做到 `cargo check --target x86_64-unknown-linux-gnu|x86_64-pc-windows-msvc --tests` **编译通过**，运行时未测——本机无这两个平台的可执行环境。`TauriEventSink::notify` 需要 `AppHandle`，因此**只有 payload 构造器有单测**（三条用例钉住四键/五键/六键的形状），真实 emit 要到接线那一步才有覆盖。`RunRegistry` 与 `TauriEventSink` 目前都带 `#[allow(dead_code, reason = "…next step of P2-05")]`：尚未接进产品路径，这件事是写下来的，不是靠警告被忽略。
7. **ADR-023 的可核对判据（不是"看起来只有一个发射者"）**：`contracts/native/v1/events.schema.json` 逐事件统计 `emitters`。加进 sink 之后契约如实漂了两个数：`task-status` **22 → 23**、`task-session` **6 → 7**——这就是"过渡期同一事件有两个发射者"被工件记下来的样子。删除那 8 处 legacy 发射点后，这两个数必须**降回 1**（只剩 `TauriEventSink`），`tauri-adapter` 门禁应当断言这两个数字本身；否则"core 是唯一发射者"只是一句口号，而双向事件回环恰恰发生在这两个计数悄悄都大于 1 的时候。
