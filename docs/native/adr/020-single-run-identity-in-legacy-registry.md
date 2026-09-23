# ADR-020：运行身份只有一个——core 的 per-task generation，TaskManager 全量按它重键

- 状态：已接受（Task P2-05 门禁，Q44 + Q45 + Q46）
- 日期：2026-09-23
- 相关：ADR-016（generation 权威在 core 独占账本）、ADR-017（重试身份是调用方的 operation key；"允许同任务双开"由此而来）、ADR-018（截断运行需确认）

## 背景

ADR-017 决定同一任务允许多代在飞之后，计划把"legacy 注册表改成 `(task_id, generation)` 键"写成 P2-05 的移交前置。侦察时我给出过一个**错误事实**："`generation` 在 `src-tauri` 里根本不存在，最接近的是 `remove_pty_handles_if_current` 用 `Arc::ptr_eq`"。实现前置①时核对代码，实际情况是这里已经有**三套**并行答案：

1. `pty.rs:1205-1229` 的 `STARTUP_SIGNAL_GENERATION` —— 一个**进程全局** `AtomicU64`，`fetch_add` 之后作为值里的判别字段塞进 `initial_input_signals`，并用 `clear_initial_input_signal_if_current` 比对。
2. `lib.rs:241-266` 的 `Arc::ptr_eq` —— 子进程句柄的"还是不是当前这代"。它的注释已经点名了这个危害本身："A manual configuration switch reuses the task ID for a new process, so an old exit monitor must not clean up the new PTY"。
3. `dsh_webui.rs:217-243` 的 `DshStartAttempt.generation` —— 按 **agent** 而非按任务，管的是 `$events` 代际。

`TaskManager`（`lib.rs:113-129`）的 14 个容器共 277 处引用。其中 12 个以 taskId 为键（4 张 HashMap 句柄表、3 个 taskId 集合、4 张 session 表、1 张已带全局计数器的 signal 表），`claimed_session_paths` 是**会话路径**集合，`codex_rpc` 是一个与任务无关的全局单例客户端。

## 决策

1. **全量重键**：14 个容器一律按"运行"这一维度重新表达，不挑 6 个"真正会被覆盖的"改。被否决的收窄方案是"只改句柄三件套 + pending_pty_sizes + wsl_active_ids + initial_input_signals（约 104 处）"，理由是它会留下一个双心智模型——同一张 `TaskManager` 里一半容器按任务、一半按运行，而"某个容器按不按运行"这种知识只能靠读注释维持。
2. **键里的 generation 用 core 的 per-task generation**，即 `OperationLedger::next_generation` 的产出，经 P2-04 的 `LaunchRequest`/`ResumeRequest` 传进来。不新建第四个编号。
   - `STARTUP_SIGNAL_GENERATION` 这个全局计数器**删除**，改用同一个 per-task 数。它今天只被用来判断"我注册的那次启动还在不在"，换成 per-task 编号后语义更强（原来两个不同任务的号会互相增长，"还是不是我这代"要靠全局单调性间接成立）。
   - `Arc::ptr_eq` 从**判定依据**降级为**安全网断言**：句柄身份仍可用来发现"同一个 generation 被注册了两次"这种实现错误，但不再用来决定要不要清理。
3. **两个非任务键容器的处理**（不是"重键"，是它们本来就没有 taskId 键）：
   - `codex_rpc` 显式豁免：它是跨任务复用的单个 `codex app-server` 进程，与"哪一代运行"无关，保持全局。
   - `claimed_session_paths` **保持路径粒度**（Q46）。它存的是会话文件路径，`claim_session_path`（`session.rs:105-113`）今天就已经是"一个会话文件只能被一个持有者看着"的跨任务互斥；把它改成 `(task_id, generation)` 键会让每代各自能认领同一路径，**互斥消失**，后果是两个 agent 进程往同一份 jsonl 里写——比句柄被覆盖更难排查。所以双开之后同任务两代也不能同时续跑同一个会话文件，这条由现有机制原样保证，只需在文档里写明它是"全量重键"的例外而不是漏掉了。
     被否的第二个方案是"把认领上移到 core 独占文件、跨宿主互斥"（Swift 与 Tauri 不能同时 attach）：语义更强，但 `attach/release` 会变成落盘操作，而 `session.rs:2064` 那里有"猜测路径→真实路径"的频繁换 claim，还要另设崩溃后残留认领的回收；本任务不做。

## 后果

- 改动面是 277 处而不是 104 处，且要重看 `lib.rs:105-110` 那张全局锁序表：容器键的形状变了，跨容器的"同一运行"操作需要同时持有两把锁，锁序约束从"哪个容器先锁"变成"哪个容器的哪一代先锁"。
- 安全网是**用例计数**：src-tauri `--locked --lib` 基线 2412，前置① 后 2424。重键期间任何一次"测试没跑也不报错"的退化都会让这个数变小——这不是推测，本任务内已经真实发生过一次（一次看似只删空行的编辑把 `storage.rs` 的 `#[test]` 合进了上一行的文档注释，该测试停止运行且无任何失败信号，是 `dead_code` 警告暴露的）。
- "允许同任务双开"从此在**宿主侧也成立**：core 的账本能力与 legacy 注册表的表达能力对齐，P3-01 抽 `TaskRegistry` 时不需要再翻一次键。
