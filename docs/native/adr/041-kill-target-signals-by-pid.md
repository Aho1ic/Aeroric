# ADR-041：退出序的杀进程面按 pid/pgid 发信号，`child_handles` 不再是杀链的一环

- 状态：已接受（AP7 复核期发现并根修的真实缺陷；覆盖 P3-06 `kill_all_live` 的实装方式，不改其产品决策）
- 日期：2026-10-13
- 相关：P3-06 Q86（子进程策略「显式杀链」）、`docs/native/reviews/cli.md`（Q86 收账）、ADR-020（重键与句柄身份）、ADR-025（资源操作定址最新代）、AP7 子任务 `10-13-ap7-phase9-advanced`

## 背景

P3-06 Q86 给 serve 退出序定下的产品决策是「显式杀链」：优雅退出时**不指望** master 关闭的隐式 SIGHUP，而是逐 task 显式杀进程组。这条决策成立，且 e2e（`serve_sigterm_kills_running_task_children_even_hup_immune`）当时是绿的。

AP7 复核期在这条链上发现两件事，**一条是缺陷，一条是缺陷的成因**：

1. **退出序会挂死。** `aeroric-cli serve` 起一个 `trap '' HUP` 的假 agent 后收 SIGTERM：12 次里有 2 次阻塞 30 s 以上才退出，样本栈是 `parking_lot_core::park` ← `RawMutex::lock_slow` ← `Executor::kill_run`。
2. **子进程在 12/12 次运行里都活过了 serve。** 只有前台组号恰好取到时，`killpg` 才把壳连同后代带走；取不到时既杀不掉、还顺带把退出序卡住。

成因是同一处：`kill_run` 的兜底分支写的是 `child_for(task_id) → child.lock().kill()`，而 `child_handles` 里那个 `Mutex` **被退出观察线程跨 `child.lock().wait()` 整程持有**（`executor.rs` 的 `ExitCtx` 线程）。于是：

- 兜底分支在一台**活着**的子进程上必然阻塞到子进程自然退出——它不是兜底，它是等；
- `child.lock().wait()` 之所以松开，只是因为上一步的 `killpg` 恰好成功。**杀链只在进程已经要死的时候才有效**，这是能自洽地"看起来正常"的形状，也正是它躲过 P3-06 验收的原因；
- 前台组号来自 `tcgetpgrp(master)`，它对**刚 spawn 的 run** 常常答 `None`——而那恰好就是要杀它的时刻。

第三条：`kill_run` 的兜底路径在 `pgid` 缺失时什么也没kill（`child.lock()` 卡住了）。

## 决策

1. **进程地址在 spawn 时捕获一次，之后只读。** pty 子进程在 `portable-pty` 的 `pre_exec` 里 `setsid()`，因此它是自己会话的组长：`pgid == pid`。于是根本不需要 `tcgetpgrp`，也就不需要 master 锁。`TaskRegistry` 新增叶子表 `kill_targets: Mutex<HashMap<RunKey, KillTarget>>`，在 `register_pty_session` 里**包进 `Mutex` 之前**取 `child.process_id()`（那一刻还没有观察线程持有它），在 `remove_pty_handles*` 里删除。
2. **`KillTarget` 是唯一的信号路径，且只发信号不等待。** `killpg(pgid, SIGHUP)` → `killpg(pgid, SIGKILL)` → `kill(pid, SIGKILL)`（壳本身的兜底）。全程不取任何锁、不做有界等待：**杀不掉是 OS 的事，卡住却是我们的 bug**——卡住会连带拖住租约释放与 socket 清理，比杀不掉更糟。
3. **`kill_targets` 刻意不进 `pty_masters → pty_writers → child_handles → pending_pty_sizes` 那条锁链**，它是叶子锁。锁序表与字段注释都写明：退出序**只许**单独取它。`Executor::kill_run` 因此不再引用 `child_handles`，`kill_all_live` 枚举待杀集合也改读 `kill_targets`。
4. **POSIX 的广播形状在构造期就被拒，而不是靠调用方纪律。** `kill(pid)` 的 pid 有三个危险特例：`-1`＝向本用户**可发的所有进程**发信号（注销图形会话）、`0`＝向**调用者所在进程组**全体发信号（连带宿主自己）、`< -1`＝打到组 `|pid|`。`killpg` 同形。因此 `KillTarget` 把两个字段存成 `Option`/零值，**只有 `> 1` 才是有效目标**，`kill_now` 的每个系统调用前再断言一次。
5. **非 Unix 后端保留句柄路径**（`#[cfg(not(unix))]`）：Windows 没有这套进程组语义，`child.kill()` 仍是那里唯一可用的手段，且不共享 macOS/Linux 的锁竞争形状。这是平台分支，不是兼容壳。

## 为什么不选别的

- **不改成"观察线程不跨 `wait()` 持锁"**：`portable_pty::Child::wait()` 取 `&mut self`，持有所有权是必须的，而 `wait()` 同时承担收尸与退出码语义。把 `wait()` 换成裸 `waitpid` 会丢掉这两点，把一处锁竞争换成两处语义缺口。让观察线程持有句柄、让杀链绕开句柄，是改动面更小且不依赖 `wait` 内部行为的那种选择。
- **不做"`try_lock` 失败就当杀过"**：那会让 `pgid` 缺失时彻底没有杀链（当前真实情形的 1/6），把"卡住"的 bug 换成"漏杀"的 bug。
- **不保留 `tcgetpgrp` 优先**：它答不出刚 spawn 的 run 的前台组，而这正是唯一需要的时刻；setsid 组长号在同一时刻**一定有值**。

## 后果

- **兜底语义变实。** 旧注释承诺"组号取不到时至少杀到壳"，实现兑现不了（那条路会阻塞）；现在 `kill(pid)` 是真正的兜底。
- **退出时延不再依赖子进程行为。** `serve` 的退出预算不再被子进程的 `sleep` 长度绑架。
- **`pgid == pid` 成为一条被文档化的前提**，它由 `portable-pty` 的 `pre_exec` `setsid()` 保证。若将来换 pty 后端且不再 `setsid`，`KillTarget::from_pid` 的组假设会失效——届时必须改走 `with_group(pid, None)`，这一点写进了类型的文档注释。
- **迁移期桥接面不在本次范围。** 待退休的 Tauri 前端（`src-tauri/src/pty.rs`/`ssh.rs`/`wsl.rs`）的杀进程点取句柄后立刻 `kill()` + `wait()`，不跨 `wait()` 持锁，因此**没有**同一个缺陷。它的句柄表由 core 的 `TaskRegistry` 提供，`kill_targets` 自动随之维护；本次不动它，符合 AP10 才清退旧前端的边界。
- **门禁**：新增 `runtime` 模块用例 `runtime-kill-target-rust`（floor 5，含广播形状拒绝 4 例与真 setsid 子进程组回收 1 例）；`host` 模块的 `executor-pty-rust` 用例加强为「cancel 后**组内零残留**＋cancel 立即返回（<5 s，脚本是 `sleep 30`）」——旧判据只看壳与 `cancelled` 终局，两者在坏实现下都能成立。

## 事故记录（为什么第 4 条必须是代码而不是纪律）

本 ADR 的第一版实装在 core 单测里放了 `KillTarget::with_group(-1, None)` 这条"已死进程是空操作"的用例，实现当时对 pid 不做任何校验，于是 `kill_now` 发出 `kill(-1, SIGKILL)`——POSIX 语义是"本用户可发的所有进程"，当场注销了开发机的 macOS 登录会话（黑屏、全部应用退出、回到登录界面）。同一版里 `from_pid(0)` 的路径会发出 `kill(0, …)`，打的是调用者自己的进程组。

这不只是一条测试写错了：**信号广播是"调用方传对了就行"挡不住的一类输入**，所以防线必须落在类型的构造器与信号路径上，并由门禁钉住（第 4 条、`runtime-kill-target-rust`）。宁可漏杀，绝不广播。

## 修正案（2026-10-13，同一轮的地板后果）

本 ADR 改了 `master_for` 的消费者数量，而 `scripts/native/terminal-lease-invariants.mjs`
的 C3 断言原先写的是"恰两处：resize 与 kill_run"。这条断言**必须**跟着改，且改法是
**加强**而不是放松：新的规则是"`master_for` 只许有一处消费者——那个过闸的 resize"。

原因是杀链离开 `master_for` 之后，master 就只剩控制写这一个用途了。于是这条不变量
从"记账两个性质不同的消费者"变成了"任何新增的 master 消费者都必须显式记账"——
后者更强，且正好把这次改动想守住的东西（master 的接触面应当只有一个过闸点）钉住。

同一轮还发现 C5 的漏斗计数**在 AP6 之后就已经是错的**：`remote_headless.rs` 除了
JSON-RPC 的 `task_input`，还多了一条二进制终端帧入口（`TerminalOpcode::Input`/`Resize`，
手机 E2EE 终端流走这条），于是计数是 4 个入口而不是 3 个，而断言上的 1/1 自 AP6 落盘
起就不可能成立。这**不是**本 ADR 引入的——查证：`git show HEAD:...remote_headless.rs`
`grep -c self.executor.input` = 2，而该文件在本轮**零改动**；引入时刻是 AP6 的提交
`9561e090`。已如实改成 2/2 并把"四个入口、闸都在 executor 内"写进注释。

这两条都不是"让门禁变绿"：第一条是形状确实变了（按 ADR 决策改），第二条是**门禁自己
早就红了而没人跑**（`pnpm native:check cutover` 是完整模块，而 AP6 落盘时只跑了 `push`
模块）。后者值得单独记一笔：**改一处共享文件的调用形状，要跑的不只是那个模块的门禁。**
