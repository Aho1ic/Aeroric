# Module review — `tauri-adapter` (Task P2-05)

- **Reviewed**: 2026-09-23
- **Design source**: `MIGRATION_PLAN.md` §Phase 2 · Task P2-05
- **Code（本模块目前唯一已合并部分）**: `src-tauri/src/storage_location.rs`、`src-tauri/src/storage.rs::windows_private_acl_receipt`、`src-tauri/src/lib.rs` 的 `mod storage_location;`
- **Gate**: `pnpm native:check tauri-adapter` —— **尚未注册**（注册要在 `core_adapter.rs`/`legacy_task_executor.rs` 落地的同一次编辑里完成，见 §4）
- **Gate status**: **IN PROGRESS —— 未通过，P2-05 未 accepted**。`grill-me [tauri-adapter]` 已真实调用，Q44～Q54 十一问全部获答并落成 ADR-020～027；**等待用户确认共识**后方可开始实现，在此之前不写实现代码。

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

**两次否掉推荐的共同点**：否掉的方向都是"更少自动行为"（不做收窄的键改造＝要更完整；不自动重放会起进程的操作＝要更保守）。迁移期把自动行为当作风险而不是便利，这条口径供后续任务沿用。

## 3. 事实修正（侦察阶段我报错过、实现时才发现的三条）

1. **"`generation` 在 src-tauri 里根本不存在"是错的**。实际三套并行编号：`pty.rs:1205-1229` 进程全局 `AtomicU64`（作 `initial_input_signals` 值的判别字段）、`lib.rs:241-266` 的 `Arc::ptr_eq`（其注释已点名"配置切换复用 taskId"这个危害本身）、`dsh_webui.rs:217-243` 按 agent 的 `$events` 代际。这条错误会让重键被当成绿地工程，实际是**替换既有身份**。
2. **`statvfs` 在 macOS/BSD 上没有 `f_fstypename`**（编译期 E0609）；文件系统名在 `statfs` 上，Linux 也只有 `statfs::f_type`。计划原文的"`statvfs`/`getmntinfo`"改成：两平台都走 `statfs`、各读各字段，`getmntinfo` 不需要。Linux 魔数表逐条对照 `include/uapi/linux/magic.h` 核对，并加"与本地表不相交"断言（凭记忆写的两条假值已删，其中一条是 BTRFS 真值的近似抄写错）。
3. 发射点数量修正：`task-status`/`task-session` 相关 **~8 处**（包装函数 4 个调用点 + `task-session` 6 处字面量，分布于 `session.rs`/`session_omp.rs`/`session_dsh.rs`/`event_watcher.rs`），不是早先记录的 24；全仓 `.emit(` 共 91 处，与本模块无关的 ~83 处不动。
4. Q48 不做 ADR 的理由：它是交互层选择，改法是局部替换而非语义迁移；同时它带来一条必须记的事实——`reconnectTask`（`taskDeleteDone.ts:134-144`）是 `reset` + `resume` 的**程序化**续跑，只改 resume 按钮会让 reconnect 绕开确认闸。

## 4. 待办与阻塞（按依赖序）

1. **`TaskManager` 全量重键**（ADR-020 + ADR-025）尚未开始：277 处引用、需重看 `lib.rs:105-110` 全局锁序表；容器必须支持"按任务聚合所有代"（ADR-025 的取消语义反过来约束键形态）。安全网是用例计数：src-tauri `--lib` 基线 2412，前置① 后 **2424**；重键期间任何一次"测试没了也不报错"都会让这个数变小。**本次已真实发生过一次**：一次看似只删空行的编辑把 `storage.rs` 的 `#[test]` 合进上一行文档注释，测试停止运行且无任何失败信号，由 `dead_code` 警告暴露。
2. `core_adapter.rs`（`CoreGateway`/`TauriEventSink`/DTO 转换）与 `legacy_task_executor.rs` 尚未开始；调用方向固定为 `legacy command → TaskService → LegacyTaskExecutor → low-level launch body`。
3. **`tauri-adapter` 门禁模块未注册**。注册时按每条 ADR 配机械检查（并在同一次编辑里写 `runner.floors`）：
   - ADR-020/025：src-tauri 内 generation 编号只有一处来源；`STARTUP_SIGNAL_GENERATION` 不得复活。
   - ADR-021：非 macOS 构建不得递交 writer 角色。
   - ADR-022：队列类型不再携带整表快照（`save_project_tasks` 不得出现在宿主的写路径上）。
   - ADR-023：`session*.rs`/`event_watcher.rs` 内不再出现 `"task-status"`/`"task-session"` 字面量，**并配正控**证明扫描真读到内容。
   - ADR-024：分组导入幂等（导入前后名单逐字节相同）。
   - ADR-026：`REVISION_CONFLICT` 的重放次数上限为一次，且状态 op 不重放。
   - ADR-027：只读实例的所有写入口被拒；`list_startup_degradations` 报告该状态。
4. **验证边界（诚实口径）**：Linux/Windows 的 `cfg` 分支只做到 `cargo check --target x86_64-unknown-linux-gnu|x86_64-pc-windows-msvc --tests` **编译通过**，运行时未测——本机无这两个平台的可执行环境。macOS 侧 12 条全绿。
