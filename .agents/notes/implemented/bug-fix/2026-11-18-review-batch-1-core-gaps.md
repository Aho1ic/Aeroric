# Agent Note: 全量审查 Batch 1——批次 A 的遗漏与 core 安全边界

Status: implemented

## Problem

2026-11-17 另一个 Agent 对当前工作区做了全量审查，交接单在 `.local_docs/2026-11-17-project-review-handoff.md`（本地，不入库）。它的 Batch 1 列出十处 core 问题，多数是[批次 A](./2026-11-15-adoption-a-truthful-state.md) 落地后的边界遗漏。逐条复核后都成立：

- **B1-1 只读可被绕过（Critical）**：
  - `session.configure` 不看任务身份，对讨论成员、审查者、只读委派也照改 `permissionMode`；
  - Claude 启动时无条件带 `--allow-dangerously-skip-permissions`，改权限后可切到 bypass；
  - 讨论只自动拒绝 `ExitPlanMode`，写文件、跑命令的审批照样摆给用户，一次误点就能写盘。审查和委派连 `ExitPlanMode` 都不拒。
- **B1-2 回合撤销越界（Critical）**：
  - 写回直接拼 `top.join(path)`。回合后若某级目录被换成指向项目外的符号链接，撤销会在项目外删除或覆盖文件；
  - 恢复符号链接是先删再建；
  - "已在位"只比内容，不比 mode：只改了可执行位的回合撤不回来，普通文件换成同文本的符号链接也被当成在位。
- **B1-3 DSH 和 omp 的送达真实性**：ACP 驱动是 A2 之后合入的，审批先从待办里拿掉再写，写失败也照发"已处理"；omp 的问答回答同样如此。
- **B1-4 握手无上限**：DSH 只给 `initialize` 加了超时，`session/resume`、`session/new`、`session/set_mode` 和 Claude 的 `initialize`、omp 的 `get_state` 都无限等。等待期间命令队列读不到，停止和关闭都无效。
- **B1-5 升级锁（A11）**：
  - 安装程序锁没有 RAII，跑步骤时 panic 会让锁永不释放、任务永远"升级中"；
  - `start` 先查后插，两次快速点击都能通过检查。
- **B1-6 缓存断点（A8）**：
  - 第四个断点不看内容，空字符串的 user 消息会被提升成带标记的空文本块，Anthropic 回 400；
  - 请求顶层的 `cache_control`（自动缓存）没计入四个名额。
- **B1-7 未知结束（A3）**：
  - Claude 的 `result` 没有 `subtype` 时算成功；
  - omp 不带 `id` 的 `prompt_result` 被忽略，回合永远在跑；
  - 回合没落盘时，未知结束会落到更早、已正常结束的那一轮；
  - 重启记"没跑完"时没清掉 `product_failure`。
- **B1-8 坏行计数（A6）**：
  - 导入的 omp、DSH 会话不计坏行，版本太新时整个导入报错；
  - 超大文件按尾部读时，最后一行可能正在写，却被当成坏行；
  - 远程读取是否截断靠 `from_utf8_lossy` 之后的长度反推；
  - 带 UTF-8 BOM 的首行被算作坏行。
- **B1-9 Wiki 误删页（A9）**：包清单读不到会让整个 crate 的模块换 id；非 git 目录列不出来的子目录被当成空；模块文件全部超过大小上限时模块消失。这三种情况都会删掉仍然有效的模块页。
- **B1-10 交办（`todo.assign`）**：
  - 每次请求各开一个 `Todos`，它的锁管不到下一个请求，连点会开出两个会话；
  - 不带 `configurationId` 时完全不查目录，被阻塞的 Agent 也能交办；
  - 会话开出来后记录写失败，会话就成了孤儿。

## Decision

**按条修，每条有回归测试；交接单建议与代码实情不符的地方按代码来，下面写明。**

- **B1-1**：
  - 新增 `sessions/live/read_only.rs` 的 `ReadOnlyGuard`，放在 sink 链最外层。凡是权限模式为 `plan` 的会话发来的权限请求，宿主都立即拒绝，并告诉 Agent"这是只读会话：不能改文件、执行命令或交计划。请直接用文字回答。"；
  - 用 `plan` 判断，是因为只有讨论成员、审查者和只读委派用它：用户能选的只有 `ask`、`auto_edit`、`full_access`。讨论原来那段只拒 `ExitPlanMode` 的逻辑删掉，由守卫统一处理；
  - Claude 以 `plan` 启动时不再带 `--allow-dangerously-skip-permissions`；
  - `session.configure` 对带 `discussionOf`、`reviewOf` 的任务，以及 `permissionMode` 为 `plan` 的委派子任务，拒绝任何权限变更。
- **B1-2**：
  - 新增 `inside(top, path)`：拒绝绝对路径、`..`、`.git`。已存在的每一级上层必须是真目录，不能是符号链接或文件。计划时不过的记为跳过（`SKIP_UNSAFE`），写回时再查一次，建目录后还要再查；
  - 是否在位改成比 `(对象 id, mode)`：
    - 快照一侧用 `git ls-tree`；
    - 工作区一侧，符号链接用链接文本经 `hash-object --stdin` 求 id，普通文件用 `hash-object <路径>`，可执行位看 `0o100`；
    - 目录或读不到的东西给一个不会等于任何快照的值，不再当成"不存在"；
  - 符号链接的恢复改成在旁边建临时链接再改名。
- **B1-3**：
  - ACP 照 Claude 的 A2 做法：`Approval.claimed` 保证先到先得；写成功后才移出待办、发 `PermissionResolved`、把拒绝理由排进下一条消息；
  - 写失败时释放认领，发 `PermissionSendFailed { failed }`，状态回到等用户；
  - 连接已断时发 `{ ended }`；
  - 回合结束清理待办时，已被回答但没送到的发 `{ ended }`，没人回答的仍是"已取消"。
  - omp 的回答写成功后才移出问题、发 `QuestionResolved`；写失败只报一条可恢复的错误，问题留着可以再答。
- **B1-4**：
  - ACP 握手每一步（含 `initialize`）走 `step()`：最多等 `HANDSHAKE`（30 秒），同时等会话的 `stop`（`tokio::sync::Notify`）。`interrupt` 和 `close` 会 `notify_waiters`，只唤醒正在等的握手，不留许可；
  - Claude 的 `initialize`、omp 的 `get_state` 加同样 30 秒的上限。
- **B1-5**：
  - `Installers::take` 返回 `HeldInstallers`，drop 时归还并唤醒；
  - 任务线程放一个 `JobEnds`：线程结束时任务还是 `Running`，就改成 `Failed`："升级中途出错，没有完成；可以重试"；
  - `start` 在同一把锁里检查并插入占位任务。规划失败时恢复原来的任务（或删掉占位），返回错误。
  - 实现时踩到一个死锁：线程最后的 `if let ... = jobs.lock()` 是闭包的尾表达式，它的临时锁比局部变量 `JobEnds` 活得久，`JobEnds` 再拿锁就卡住。现在显式 `drop` 锁，并写了注释。
- **B1-6**：
  - 判断"能放标记"时排除空文本（`is_empty_text`），第三、第四个断点都跳到前一个可放的块；空字符串内容不提升成数组，空的 `system` 也不提升；
  - 第四个断点只在有内容的 user 消息里选；
  - `existing_markers` 先计请求顶层的 `cache_control`。
- **B1-7**：
  - Claude 的 `result` 只有 `subtype == "success"` 才算成功；缺 `subtype` 时报"Claude 结束这一轮时没有说结果（缺少 subtype），按失败记"，`result` 字段里的回复文字不当错误显示；
  - omp 不带 `id` 的 `prompt_result`，在有等待中的 prompt 时就结算它；
  - `mark_restarts` 同时清掉 `product_failure`；
  - "未知结束落到更早一轮"**不改**，理由见 Alternatives。
- **B1-8**：
  - 导入的 omp、DSH 会话在 core 里读行、计坏行；解析器整份拒绝时把原因放进 `unreadable.reason`，消息为空，不再让导入报错。DSH 按行读，不知道文件是否以换行结尾，最后一行不判；
  - 超大文件按尾部读时，最后一行同样不判；
  - 远程读取命令在只打印尾部时以退出码 `READ_CUT_EXIT`（4）结束，宿主按退出码知道是否截断；
  - 判断坏行前去掉行首的 BOM。
- **B1-9**：
  - `CodeIndex` 加 `unreadable_dirs`，记两种目录：非 git 遍历时列不出来的子目录（以 `dir/` 前缀记），以及包清单读不到的那个包目录。读不到的清单本身记入 `unreadable`；
  - 新增 `unsure_of(path)`，覆盖 `unreadable`、`too_large` 和前缀；`incomplete()` 作为 `unread_module` 的总开关；
  - 模块不见了，但它的任一来源"可能还在、这次没看到"时，页面保留原样。
- **B1-10**：
  - 进程内 `ASSIGNING` 集合加上 `Claim` 守卫：同一条待办正在交办时，第二次请求返回 `Busy`"这条待办正在交办"；
  - 一律读项目的会话目录。不带配置时，只要求目录里有这个 Agent，否则报"这个项目里没有这个 Agent"。配置上的 `blockedReasons` 说的是"按这个配置提交"不行，不代表按名字（`task.run`）启动 Agent 不行，所以不拿它拒绝；
  - 会话开出来但记录写失败时，`close_session` 停掉它，错误里写明"刚开的会话已停下"。

### 同主题笔记审计

| 笔记 | 分类 | 处理 |
|---|---|---|
| [批次 A](./2026-11-15-adoption-a-truthful-state.md) | 上级方案 | 迁入 implemented（交接单 B0-5），末尾补一条指向本篇 |
| [A2](./2026-11-16-a2-approval-answer-must-arrive.md)、[A3](./2026-11-17-a3-unknown-turn-end-is-not-done.md)、[A4](./2026-11-17-a4-discussion-read-only-must-be-enforced.md)、[A6](./2026-11-17-a6-earlier-history-and-unreadable-lines.md)、[A7](./2026-11-17-a7-undo-in-place-and-whole-writes.md)、[A8](./2026-11-17-a8-cache-breakpoints-count-existing.md)、[A9](./2026-11-17-a9-wiki-sources-deleted-or-unreadable.md)、[A11](./2026-11-17-a11-upgrades-share-an-installer-take-turns.md) | 被本篇补强 | 结论不变，末尾各补一条"2026-11-18 补强"指向本篇 |

## Alternatives considered

**B1-1：在讨论、审查、委派三处各自拒绝。** 最强的理由是每处能给贴合场景的理由，例如讨论原来的"不需要交计划"。没选：三处各写一遍，漏一处就是漏洞（审查和委派原来就漏了）；权限模式本身已经标出了只读身份，一处守卫覆盖所有角色。

**B1-1：守卫不转发请求，让界面连卡片都看不到。** 最强的理由是彻底消除误点的窗口。没选：界面读的是驱动自己的更新日志，sink 不转发也挡不住；守卫在另一个线程立即回拒，配合 configure 拒绝和去掉 bypass 标志，误点只剩毫秒级窗口，而且只能在 `plan` 档内生效。

**B1-2：按交接单用 `hash-object --no-filters`。** 最强的理由是不受 clean 过滤器影响，比较的是原始字节。没选：快照是 `git add -A` 写进临时索引的，会经过 clean 过滤；工作区一侧不过滤，带过滤器的文件永远对不上。两侧都按 git 的方式求值才一致。

**B1-3：omp 也加 `QuestionSendFailed` 更新。** 最强的理由是和审批对称，界面能精确标出那一张卡。没选：要改线协议和 Swift 解码；问题留在原处、报一条可恢复错误，用户已经能再答一次。

**B1-4：给 Claude、omp 也接上 stop 信号。** 最强的理由是三族行为一致。没选：Claude 和 omp 握手只有一步，30 秒上限已经把卡死变成有限等待；ACP 有三四步串行，最坏要等两分钟，才需要立即打断。

**B1-5：用 `catch_unwind` 包住任务。** 最强的理由是能拿到 panic 信息写进任务。没选：要求闭包里所有东西都 `UnwindSafe`。RAII 守卫不管线程怎么结束都会执行，更简单。

**B1-7：未知结束比转录里最后一轮的结束晚 2 分钟以上就不覆盖它（交接单建议）。** 最强的理由是：那一轮没落盘时，未知结束不会错算到更早、已经正常结束的一轮上。试过后没选：未知结束的时间是宿主时钟，转录里的时间是 Agent 那台机器的时钟。SSH 远程会话两边时钟可能差几分钟，测试夹具也用固定时间，按时间窗反而会把真正的未知结束漏掉（宿主测试 `an_unknown_end_state_fails_the_turn_and_its_history_says_not_run` 就因此失败）。错算只在"那一轮完全没落盘"时发生，比漏掉更少见。

**B1-10：不带配置时也按 `blockedReasons` 拒绝（交接单建议）。** 最强的理由是被阻塞的 Agent 不该被交办。没选：宿主测试里 Codex 的内建配置带着"托管执行装配尚未完整采用"这类原因，按名字启动却是正常的。`blockedReasons` 说的是配置，不是 Agent。

**B1-10：在 `TodoBook` 落盘里加"交办中"状态。** 最强的理由是宿主重启也知道有过一次没完成的交办。没选：交办只持续一个请求那么久，重启后不该还挂着；进程内认领就够了，而且不改落盘格式。

## Consequences

- **收益**：
  - 只读角色的写入口全部收口：不能改权限，Claude 拿不到 bypass，写审批由宿主自动拒绝；
  - 撤销不会碰项目外的东西，能撤回只改了权限位的回合；
  - 四族审批和问答都只在真正送到后才算处理过。
- **代价**：
  - `plan` 会话里只读的 MCP 工具若也要审批，会被一并拒掉；
  - 守卫拒绝前，卡片会在界面上闪现一下；
  - B1-4 只对 DSH 做了立即打断，Claude 和 omp 握手卡住时最多等 30 秒；
  - B1-10 的认领只在本进程内有效；`schedule::open_session` 里 `task.run` 自己失败时留下的空记录没有处理，交接单这一小项留着。
- **验证**：
  - 新增回归测试：
    - `read_only` 守卫一条，宿主 `read_only_role_tests` 一条；
    - `turn_ledger` 三条（符号链接目录、只改可执行位、文件与同文本链接互换）；
    - ACP 两条（审批没写进去、握手被停止打断），omp 两条（无 id 的 `prompt_result`、回答没写进去），Claude `map` 一条；
    - `upgrade` 三条（panic 后归还锁、并发点两次只装一次、规划失败不留任务），`cache_injector` 三条；
    - `turn_outcome` 一条，导入 omp/DSH 一条，远程读取命令一条，Wiki 一条，交办三条。
  - 2026-11-18 在 Mac 上：
    - 只格式化本次改动的文件；`cargo clippy --workspace --all-targets -D warnings` 通过；
    - 第一次 `cargo test --workspace --no-fail-fast`：3774 通过、6 失败。其中 3 个是新测试本身写错，另外 3 个是 B1-7、B1-10 收得过紧。改正后重跑出错的几组：core 的 acp/omp/turn_outcome 52 个、宿主 `session_live` 48 个、交办 5 个全部通过。改正后没有再跑一遍完整的 workspace；
    - 中途发现并修掉一个死锁：B1-5 引入 `JobEnds` 后，`upgrade` 的测试全部挂住（见 Decision）。
