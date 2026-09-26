# P3-01 侦察底稿:运行时端口与 TaskRegistry 迁移面(2026-09-26)

> 性质:实施前的机械盘点,全部数字来自 `75a39e0` 之后的真实源码,不是估计。
> 消费方式:每一节都是后续切片的输入;与计划有出入的事实在这里更正而不是悄悄改。

## 1. 起点状态:core 已经 tauri-free

P3-01 验收判据 `rg -n 'tauri::|AppHandle|tauri::State' core/crates/aeroric-core/src` **当前零命中**(core crate 的 Cargo.toml 也没有 tauri 依赖;INV-02 从 P2-01 起逐任务维持)。P3-01 的这条验收是**保持零**,不是清零。

## 2. TaskManager 容器清单与可移植性判定

宿主 `TaskManager`(`src-tauri/src/lib.rs`)共 15 个字段,全部按 `RunKey{id,generation}` 键(P2-05 重键 wave 1+2 完成,ADR-020/030/031/032):

| 容器 | 类型要点 | 可移植到 core? |
|---|---|---|
| `pty_masters` | `Box<dyn portable_pty::MasterPty + Send>` | ✅ portable-pty 是纯 Rust crate;core workspace 已声明同一 vendor 补丁(`core/Cargo.toml [patch]`),crate 依赖待加 |
| `pty_writers` | `Box<dyn io::Write + Send>` | ✅ 同上 |
| `child_handles` | `Box<dyn portable_pty::Child + Send + Sync>` | ✅ 同上 |
| `pending_pty_sizes` | `(u16,u16)` 信箱,读写走 `unminted(id)` | ✅ 纯数据 |
| `cancelled_tasks` / `manually_completed_tasks` | `HashSet<String>`,任务级账(ADR-030"最后退出者销账") | ✅ 纯数据 |
| `codex_sessions` / `claude_sessions` / `dsh_sessions` / `omp_sessions` | Info 全是纯数据(`session_id`/`session_path`/`is_placeholder`,见 `session.rs:33/39/49`、`session_dsh.rs:40`) | ✅ 纯数据,无 AppHandle/Channel/Sender |
| `claimed_session_paths` | `HashSet<String>`,路径粒度(ADR-020 §3 的例外) | ✅ 纯数据 |
| `initial_input_signals` | `pty::StartupSignalRegistry`,`std::sync::mpsc::Sender` | ✅ std 通道,无 tauri |
| `wsl_active_ids` | `HashSet<RunKey>` | ✅ 纯数据 |
| `runs` | `RunRegistry`(纯 Rust,自含锁序) | ✅ 已是可移植形状 |
| `codex_rpc` | `Arc<Mutex<Option<CodexRpcClient>>>`(usage.rs:36),**全局单例,非任务键** | ⚠️ ADR-020 已豁免其重键;迁移与否属实现决策,建议留在宿主(它是 usage 的客户端,不是任务资源) |

**迁移纪律**:Action 2 的迁移必须是一次 **move**(宿主 `TaskManager` 变成委托壳,全部 43 处 `app.state::<TaskManager>` 的读取点改道),禁止"core 先立副本再逐表切换"——那是 Action 1 明文禁止的双真相源。这也是 `core/src/runtime/mod.rs` 文档里"TaskRegistry 与 move 同刀"的原因。

## 3. `AppHandle` 的实际用途(45 个泛型化函数的表面需求)

对 `app`/`handle` 的方法调用普查(`src-tauri/src/*.rs`):

| 用途 | 次数 | 说明 |
|---|---|---|
| `state::<TaskManager>` | 43 | 主导用途。TaskRegistry 迁移后这些点改读 core registry 或经 executor 端口 |
| `emit` | 29 | 事件发射。ADR-023 后生产路径只剩 sink 一行 + 远程桥;这批多为命令级的 UI 事件(dsh/approval/终端数据),归宿主 sink 实现 |
| `manage` | 9 | 仅 setup 阶段(launch 时不调) |
| `state::<ExitAuthorization>` / `DshWebUiManager` / `CoreGateway` / `RemoteState` / `LocalRouterManager` | 5/3/3/2/1 | 宿主服务定位;`CoreGateway` 已有 core 侧抽象,其余属宿主实现细节 |

结论:`Runtime` 端口(伞形)的最小诚实表面 = `event_sink()` + `watchers()`;`state::<T>()` 这类**具体类型定位**是宿主实现的事,core 的 trait 不收它(收了就把宿主类型系统拖进 core,与 INV-02 同向违背)。

## 4. watcher / spawn 站点普查(Action 3 的迁移面)

`tauri::async_runtime::spawn` / `std::thread::spawn` / `spawn_blocking` 的站点分布(计数):`git.rs` 26、`fs.rs` 13、`agent_tools.rs` 5、`docker.rs` 5、`agent_assist.rs` 4、`config.rs` 4、`analytics.rs`/`conda.rs`/`dap.rs`/`diagnostics.rs`/`ssh.rs` 各 2、其余 1。**其中属于"任务作用域 watcher"(需接 ShutdownToken 的)是少数**:会话 watcher(`spawn_*_session_watcher`)、`event_watcher.rs` 的 hook 循环、PTY reader 线程、dsh 会话流。git/fs/docker 的大多数是命令作用域(tokio join 等在命令内),不在 Action 3 范围。切片时逐站点点名,不用总数当迁移清单。

## 5. `subprocess.rs` 可移植性(331 行,Input 点名)

`configure_*` / `terminate_*_process_tree` / `signal_process_group` 全部是纯 Rust(std/tokio + libc + windows-sys),**零 tauri 引用**,自带 3 条真进程测试(`taskkill_targets_the_complete_process_tree` 为 windows-cfg)。迁入 core 的代价:core 增加两个 cfg 门控的 OS 依赖(`libc` 0.2 unix-only;`windows-sys` 0.61 带具体 feature 列表)。宿主侧改为 `pub use aeroric_core::runtime::subprocess::*;` 的薄 re-export,20+ 个调用点(`agent_tools.rs`/`conda.rs`/`agent_assist.rs`/`ssh.rs` 等)零改动。**src-tauri `--lib` 地板将随测试搬家下降**(搬 3 条),须同轮在 checks.json 的 `$note` 记录,不静默。

## 6. 切片顺序(每片自身可绿)

1. ~~runtime 模块骨架:`ShutdownToken` + `WatcherSupervisor` + `Runtime` 伞形端口 + `events.rs` 再导出~~(已落,补记八十二)。
2. ~~`subprocess.rs` 迁入 core(薄 re-export;OS 依赖进 core;地板账随迁)~~(已落,补记八十三)。连带一次守卫口径收窄:`lease-refusal-never-kills-a-foreign-pid` 原来扫 core 全部源码(写于 core 没有进程管理的 P2 时代),进程树管理迁入后改为点名租约决策面文件(storage_lock/context/location/storage/),理由与主语区分记在该 case 的 `$note`。
3. ~~`TaskRegistry` move~~(已落,补记八十四):14 张表 + `RunRegistry` 迁入 core,宿主 `TaskManager` 变 **Deref 委托壳**(`codex_rpc` 留守)——316 处直接字段访问点经 Deref 一字未改,比原估的"43 处读取点改道"便宜得多;锁序表随迁(registry.rs 模块文档)。核心回归:core 侧 `two_runs_of_one_task_hold_distinct_handle_entries`(双开共存)+ 宿主真内核验收 `a_second_operation_starts_a_second_run_without_replacing_the_first` 双层钉住;ADR-030/031 各一条 core 侧用例随迁。
4. ~~watcher 逐站点接 `ShutdownToken`~~(已落,补记八十五):10 个任务作用域站点全部经 `WatcherSupervisor`——会话 watcher(status/resume/recovery/dsh/omp/懒挂载)按 `(task, generation)` **去重登记**(同名的第二次启动被拒);PTY reader、三个退出监控、两个转录 watcher 用 `spawn_unique`(仅记账不去重——ssh 的 unminted 共享槽路径下按名去重会拒掉双开的第二个合法循环,kept-verbatim 行为保持)。token 检查点:status/transcript 循环条件、recovery/dsh/omp/attach 的 for 循环、exit monitor 与 reader 的轮询/读取循环顶。`shutdown` 升级为**有界拆除**(5s 宽限,`shutdown_within` 供测试;无视 token 的 watcher 被 detach 并计入 `ShutdownReport::detached`,不把退出挂死)。`tests/runtime_lifecycle.rs` 5 条判据全绿并注册 `pnpm native:check runtime`(minCases 5)。
5. `platform_adapter.rs`(宿主)与 `PlatformServices` 端口同刀;`ProcessSpawner` 随 §5-§6 的进程面成形。**两个 trait 名字先于消费者出现被刻意推迟**——先立没人调用的 trait 就是 grill 门禁点名的"只检查 trait 名称"。

## 7. 门禁与验收对照

- `pnpm native:check runtime`:**尚未注册**(无 suite 时不注册假门禁);`tests/runtime_lifecycle.rs` 随 §6-4 成形后同轮注册。
- core 测试基线:279 → 287(切片一)→ 290(切片二)→ **304**(切片三:+8 RunRegistry 测试迁入 +2 版本戳测试迁入 +6 TaskRegistry 新增)。
- src-tauri `--lib`:2479 → 2476(切片二)→ **2466**(切片三:−10 即上述搬家;`minCases` 与 `runner.floors` 两处同改并读回)。
- `core-no-tauri-deps` 的 forbid 从裸子串改为「包名+v」形态:portable-pty 以 path 依赖进入依赖树后,cargo tree 的 stdout 携带 .../src-tauri/vendor/... 路径字面量,裸子串会把合法 vendor 路径误判成依赖(理由在该 case 的 `$note`)。
- `cargo check --manifest-path core/Cargo.toml --locked` 与 src-tauri 同命令:均绿;core/Cargo.lock 与 src-tauri/Cargo.lock 各精确一行(`"libc",` 进入 aeroric-core 依赖列表,包本体已在两份锁里,无新包)。
