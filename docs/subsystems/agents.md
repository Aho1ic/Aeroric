# Agent 驱动

四族内建 Agent（Claude Code、Codex、omp、DSH）与自定义 Agent 都经结构化协议驱动，驱动在 `core/crates/aeroric-core/src/sessions/live/`。决策依据见 [结构化协议](../../.agents/notes/implemented/architecture/2026-10-25-structured-agent-protocols.md)。

| 族 | 协议 | 驱动 | 版本门槛 |
|---|---|---|---|
| Claude Code | `claude` stream-json 控制协议（stdin/stdout control_request/response） | `sessions/live/claude/` | ≥ 2.1 |
| Codex | `codex app-server` stdio JSON-RPC，宿主内共享一个进程多会话 | `sessions/live/codex/` | ≥ 0.162 |
| omp | `omp --mode rpc`（`agents::launch::omp_rpc_args`） | `sessions/live/omp/` | — |
| DSH | 宿主结构化通道 | `agents/dsh_host/` | — |

## 启动环境

- 托管 home：按配置生成的在宿主数据目录的 `agent-homes/` 下；内建 omp 是 `<数据目录>/agent-homes/omp`，宿主启动时注入（`settings::configure_omp_home`），会话、设置、模型探测和升级都用它。经 `settings::aeroric_dir()` 的自定义 Agent home、凭据文件、旧 `settings.json`、远程密钥仍在 `~/.aeroric`：宿主进程里有两个数据根，见 [两个数据根](../../.agents/notes/implemented/architecture/2026-11-18-two-data-roots.md)。omp 经 `PI_CODING_AGENT_DIR` 注入（`core/crates/aeroric-core/src/settings/omp_home.rs`），环境组合后清掉 `PI_PROFILE`/`OMP_PROFILE`；绝不读写用户的 `~/.omp`。
- 家族字段：任务记录钉死 `launchFamily`、`sessionFamily`、`resumeFamily`，见 [Agent 家族](../../.agents/notes/implemented/architecture/2026-09-23-agent-families.md)。
- 已结构化的族不安装 hook。旧版装下的 hook 由两处清理：omp 托管 home 在每次准备时移除旧脚本（`settings/omp_home.rs`）；Claude/Codex 的用户配置注入与 `~/.aeroric/hooks/` 在宿主启动时移除（`agents/managed_home/legacy_cleanup.rs`，见 [清理旧 hook](../../.agents/notes/implemented/simplification/2026-10-27-remove-legacy-hook-pipeline.md)）。

## 会话事件

驱动把协议消息归一为 core 的会话事件（参照 ACP `session/update`：消息与思考片段、工具调用、计划、权限请求、子 Agent、用量、配置变更、错误、未识别内容），经 topic `session.update` 发给客户端。未识别内容带来源保留并显示，不丢弃、不猜测、不自动批准。

审批回应写进 Agent 的输入之后，驱动才发 `permissionResolved`。写不进去时发 `permissionSendFailed`：`failed` 表示请求仍开着、任务回到 `input_required`，用户可以再答；`ended` 表示请求随连接或这一轮结束，回答没送到。被回答过的请求在连接或回合结束时以 `ended` 结束，没回答过的仍以 `cancelled` 结束。同一请求同时只接受一个回答，第二个回答返回 `BUSY`（`DriverError::AlreadyAnswered`），请求已不在返回 `NOT_FOUND`。审批回应不自动重发；omp 的"本会话总是允许"在写成功后才记下。检查点只把写成功的批准记为同意。

回合结束（`turnEnded`）只有 Agent 明确说完成才记 `completed`：Codex 的 `turn.status` 为 `completed`，ACP 的 `stopReason` 为 `end_turn`，omp 的 `prompt_result.status` 为 `completed`。ACP 的 `max_tokens`、`max_turn_requests`（撞上单轮上限）记 `failed`。没见过的结束状态和缺失的结束状态记 `failed`，原值放在 `rawStatus`（缺失为空字符串），原因写"<Agent> 返回了没见过的结束状态：<原值>"或"<Agent> 没有说这一轮是怎么结束的"；宿主把它追加到任务记录的 `unknownTurnEnds`（最多 20 条），`session.history` 据此把那一轮记为"没跑成 · 结束状态不明"（`notRun.reason = unknownEnd`）。自动检查、委派验收和审查循环只在 `completed` 时启动。

## SSH 远程

Claude/Codex/omp 可经 ssh 在远端运行（`sessions/live/remote.rs`、`aeroric-host/src/remote_sessions.rs`）：登录 shell、逐词转义、`StrictHostKeyChecking=yes` 与 Aeroric 自己的 known_hosts；只有密钥的连接加 `BatchMode=yes`，密码连接经 `SSH_ASKPASS = aeroric-cli askpass` 兑换一次性票据（60 秒、3 次）。仅限内建 Agent。

## 改动四族分支

同步全部穷尽点：`aeroric-protocol`、core 的 match、Swift 枚举、设置与统计；以 `cargo check` + `swift build` 兜底。测试用 `scripts/native/fake-claude.py`、`scripts/native/fake-codex-app-server.py` 与假 ssh。
