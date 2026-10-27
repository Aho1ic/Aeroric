# Agent 驱动

四族内建 Agent（Claude Code、Codex、omp、DSH）与自定义 Agent 都经结构化协议驱动，驱动在 `core/crates/aeroric-core/src/sessions/live/`。决策依据见 [结构化协议](../../.agents/notes/implemented/architecture/2026-10-25-structured-agent-protocols.md)。

| 族 | 协议 | 驱动 | 版本门槛 |
|---|---|---|---|
| Claude Code | `claude` stream-json 控制协议（stdin/stdout control_request/response） | `sessions/live/claude/` | ≥ 2.1 |
| Codex | `codex app-server` stdio JSON-RPC，宿主内共享一个进程多会话 | `sessions/live/codex/` | ≥ 0.162 |
| omp | `omp --mode rpc`（`agents::launch::omp_rpc_args`） | `sessions/live/omp/` | — |
| DSH | 宿主结构化通道 | `agents/dsh_host/` | — |

## 启动环境

- 托管 home：`~/.aeroric/agent-homes/<family>`。omp 经 `PI_CODING_AGENT_DIR` 注入（`core/crates/aeroric-core/src/settings/omp_home.rs`），环境组合后清掉 `PI_PROFILE`/`OMP_PROFILE`；绝不读写用户的 `~/.omp`。
- 家族字段：任务记录钉死 `launchFamily`、`sessionFamily`、`resumeFamily`，见 [Agent 家族](../../.agents/notes/implemented/architecture/2026-09-23-agent-families.md)。
- 已结构化的族不安装 hook；宿主启动时清理旧版装下的 omp hook。

## 会话事件

驱动把协议消息归一为 core 的会话事件（参照 ACP `session/update`：消息与思考片段、工具调用、计划、权限请求、子 Agent、用量、配置变更、错误、未识别内容），经 topic `session.update` 发给客户端。未识别内容带来源保留并显示，不丢弃、不猜测、不自动批准。

## SSH 远程

Claude/Codex/omp 可经 ssh 在远端运行（`sessions/live/remote.rs`、`aeroric-host/src/remote_sessions.rs`）：登录 shell、逐词转义、`StrictHostKeyChecking=yes` 与 Aeroric 自己的 known_hosts；只有密钥的连接加 `BatchMode=yes`，密码连接经 `SSH_ASKPASS = aeroric-cli askpass` 兑换一次性票据（60 秒、3 次）。仅限内建 Agent。

## 改动四族分支

同步全部穷尽点：`aeroric-protocol`、core 的 match、Swift 枚举、设置与统计；以 `cargo check` + `swift build` 兜底。测试用 `scripts/native/fake-claude.py`、`scripts/native/fake-codex-app-server.py` 与假 ssh。
