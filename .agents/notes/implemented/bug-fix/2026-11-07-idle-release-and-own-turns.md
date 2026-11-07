# Agent Note: 空闲回收不杀后台工作，Agent 自启的回合有边界

Status: implemented

## Problem

宿主每 60 秒扫描一次（`StructuredExecutor::sweep`），把空闲超过 10 分钟的会话结束掉。这里的"空闲"只看两条：没有回合，也没有待审批。

问题在于 Agent 在回合之外还会有工作：
- Claude 的后台 Bash、Monitor、后台子代理；
- Codex 回合结束后仍在跑的命令（`unified_exec` 在 0.166 默认开启）；
- omp 尚未结算的异步作业（`prompt_result.sessionSettled=false`）。

这些都和 CLI 进程同生共死。会话一被回收，它们就随进程结束，完成通知也不会再来。

另外，各驱动只在 Aeroric 自己发出 prompt 时才开回合。可 Agent 也会自己开始一轮：Claude 在后台工作回报后自启一轮；omp 的异步作业会把会话唤醒。这类回合没有 `turnStarted`，状态不会变成运行中，回合改动不记录，审查回环也不触发，而且整轮都被当作空闲，可能被回收从中途掐断。

最后两种情况：
- 回合之间进程意外退出时，界面什么也不说，用户不知道后台任务也一起没了。
- Claude 的后台命令被显示成"子 Agent"，`skip_transcript` 的内部任务也被显示出来。

2026-11-07 用进程内协议假端复现了这些问题（`repro-results.md`，见 Mac 上的 `~/Documents/Aeroric-research/2026-11-07-New-addition-1102-recheck/`），之后把复现用例改成了现在的回归测试。

在用户 Mac 上统计 Aeroric 托管 home 里的真实记录：519 个 Claude 后台命令中，511 个收到了完成回报，其余 8 个由 Agent 自己停掉；没有发现被回收掐断的，也没有观察到自启回合。所以这次修的是一个已经确认存在、只是尚未造成损失的隐患。

## Decision

### 空闲判断

- `SESSION_IDLE_MS` 改为 1 小时（`core/crates/aeroric-core/src/sessions/live/executor.rs`）。
- 有后台工作时，`idle_since_ms()` 返回 None，`Suspend` 也会再核对一次。最后一项后台工作结束时，空闲计时从那一刻重新开始。三族各自的判据：
  - **Claude**：`ClaudeMapper` 维护任务表（`claude/map.rs`）。`task_started` 入表，`task_notification` 或 `task_updated` 报出终态后出表。只有后台任务算数：`local_bash` 或 `is_backgrounded`，`task_updated.patch.is_backgrounded` 可以把任务改到后台。回合结束（`end_turn`）只清前台任务，因为中断时 Claude 不会为它们发通知。任务 id 归会话所有，中断不会清空后台任务。
  - **Codex**：驱动记下"已 `item/started`、未 `item/completed`"的 `commandExecution`（`codex/driver.rs` 的 `commands_running`）。回合结束后仍在这张表里的，就是后台终端。没有补发完成事件的命令会让会话一直常驻；进程换代（`server_exited`）时清空。
  - **omp**：`prompt_result.sessionSettled=false` 开始算，收到 `session_settled` 结束（`omp/driver.rs` 的 `settled`）。旧版 omp 没有这个字段，按已结算处理。
- 判据只来自协议事件，不扫描进程树。
- Codex 共享 app-server 的停机逻辑不变：会话不 `unsubscribe` 就不会注销路由，有后台终端的线程会让 server 一直活着。

### 用户开关

- 设置键 `release_idle_agents`，缺省视为开（`settings/preferences.rs`）。
- 宿主装入 `SettingsIdleRelease` 策略，每次扫描都读一次；关掉后扫描就不再释放任何会话（`IdleReleasePolicy`）。
- 方法为 `session.idleRelease.get` 和 `session.idleRelease.set`，返回 `{enabled, idleMinutes}`（`aeroric-host/src/session_live_services.rs`）。
- 开关在 设置 → 通用 → "Agent 进程"（`IdleReleaseSettingsCard` 与 `IdleReleaseStore`，在 `macos/AeroricKit/Sources/AeroricUI/Settings/` 下）。App 连上宿主时先读一次；宿主回答以后才显示这张卡，没有这两个方法的旧宿主不显示。保存失败时开关回到原值，并在说明行写出原因。

### 自启回合

- **Claude**：没有进行中的回合、也没有待回结果的消息时，收到 `system/init` 就开一个自启回合，`result` 结束它。若迟迟没有 `result`，就用 `system/session_state_changed` 的 `idle` 收尾；本机启动时设置 `CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS=1` 打开这类事件（`aeroric-host/src/live.rs`）。
  - **must**：只有 `system/init` 能开回合。后台子代理的帧不带 `parent_tool_use_id`，和主会话的帧长得一样；若用它们开回合，这一轮永远等不到 `result`。
- **omp**：没有 `prompt_id`、也没有进行中的回合时，`agent_start` 开回合，`agent_end` 结束。
- 自启回合与普通回合走同一条 `begin_turn` 路径，所以回合改动、审查回环、检查点一样看得到它。

### 退出提示与卡片

- 回合之间进程退出，如果还有后台工作，在会话里插一条可恢复错误：
  - "Claude 进程已退出，N 个后台任务随之结束"
  - "Codex 进程已退出，N 个后台命令随之结束"
  - "omp 进程已退出，未结算的后台作业随之结束"
  
  任务状态不变。没有后台工作时，退出不出任何提示，与原来一致。
- `local_bash` 卡片的标题以 `BACKGROUND_COMMAND_PREFIX`（"后台命令："）开头；`skip_transcript` 的任务只计入后台，不出卡片。
- 会话信息卡按这个前缀分组：带前缀的放进"后台命令"组，显示时去掉前缀；其余放进"子 Agent"组（`LiveSubagent.isBackgroundCommand`）。这个前缀是 core 与 Swift 之间的约定，两边必须同步修改。

## Alternatives considered

- **扫描进程树判断后台工作**：最强的理由是它不依赖各家协议，任何留下来的子进程都能看到。没有采用：
  - 区分不了"该留的 dev server"和"已经没用的孤儿"；
  - 远程 SSH 会话看不到对方的进程表；
  - paseo 也只用 provider 自己报告的信号，并注明这些信号不是任意进程的清单。
- **Claude 用主会话回复帧开自启回合（PRD 原定的第二条路）**：理由是即使某个 CLI 版本不发 `init`，也能接住自启回合。没有采用：paseo 在 `providers/claude/agent.ts` 的 `opensAutonomousTurn` 里明确只认 `init`，因为后台子代理的帧开出的回合等不到 `result`，会话会永远停在运行中。代价是：如果真有不发 `init` 的自启回合，它会维持现状（没有边界）。
- **给后台否决设上限（比如 4 小时后照样回收）**：理由是一个漏发完成通知的任务会让进程永远常驻。没有采用：常驻的代价只是内存，而误杀会丢掉用户的工作；用户也可以关闭会话，或者在设置里调整释放策略。paseo 同样不设覆盖否决的期限。
- **维持 10 分钟，只加后台否决**：理由是改动更小。没有采用：用户选择了 1 小时（与 paseo 一致）。

## Consequences

- **收益**：
  - Agent 留在后台的 dev server、长构建、后台子代理不会被回收掐断；
  - 自启回合可以撤销、审查，也有状态；
  - 进程意外退出带走后台工作时，用户能看到。
- **代价与已知上限**：
  - 常驻进程会变多。一个永远不结束的后台命令（比如 dev server）会让对应 CLI 一直驻留，直到用户关闭会话。
  - Codex 没有后台终端列表 RPC。如果它对某个命令不补发 `item/completed`（例如中断时），会话会一直常驻。
  - Claude 后台子代理的帧仍会混进主会话的消息流，本批没有处理。
  - 回合结束后后台作业写入的文件，会被算进下一回合的改动，本批也没有处理。
  - 远程 SSH 的 Claude 不设 `CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS`，自启回合只靠 `result` 收尾。
- **需要重访的信号**：Codex 增加后台终端列表或线程级"已结算"字段；Claude 的 `init` 不再括住回合；用户反馈常驻进程占用过多。

## Verification

- 回归测试：
  - `claude/driver_tests.rs`：`a_running_background_task_keeps_the_session_from_idling`、`a_turn_claude_starts_itself_has_its_own_boundary`、`frames_outside_a_turn_open_none_and_idle_closes_a_turn_without_result`、`an_exit_between_turns_*`
  - `claude/map_tests.rs`：`background_commands_hidden_tasks_and_what_still_runs`
  - `codex/driver_tests.rs`：`a_background_command_keeps_the_session_until_it_completes`、`a_server_exit_between_turns_says_its_background_commands_ended`
  - `omp/driver_tests.rs`：`an_unsettled_session_stays_up_and_its_own_turn_has_a_boundary`、`an_exit_while_unsettled_says_the_async_work_ended`
  - `executor_tests.rs`：`the_sweep_keeps_idle_sessions_when_release_is_off`
  - `aeroric-host/tests/session_live.rs`：`idle_release_is_on_until_switched_off`
  - `macos/AeroricKit/Tests/AeroricUIUnitTests/IdleReleaseTests.swift`：应答解码、旧宿主不显示、保存失败回滚、后台命令分组
- 签名构建目检（Codex Computer Use，2026-11-07）：
  - 设置 → 通用 → "Agent 进程"卡片：位置、文案、开关样式、关闭后保存均已通过（报告见 Mac 上的 `~/Documents/computer-use/2026-11-07-1919-aeroric-idle-release-review/report.md`，只验了深色外观）。
  - 信息卡的"后台命令"分组、后台命令结束后 Claude 的自启回合：**NOT RUN**。测试用的 Claude 配置返回 403，Agent 没有跑起来（报告见 `2026-11-07-2210-aeroric-idle-release-review2/report.md`）。这两项目前只有上面的单测与协议假端用例覆盖，Claude 2.1.318 真机上自启回合的帧序还没核对。
- 相关的旧判断："空闲 1 小时回收、已有同类机制、不用新做"，见 [路线复核](../../proposed/architecture/2026-11-02-roadmap-recheck.md)。当时的机制不看后台工作，那条判断已就地更正。
