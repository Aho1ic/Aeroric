# ADR-043：Agent 经结构化协议驱动，不再跑在 PTY 里

- 状态：已接受（2026-10-25 用户明确指示：“学习 codeg 和 Agents-Anywhere，舍弃 pty，重构为原生结构性对话”）
- 日期：2026-10-25
- 相关：ADR-001（INV-04 / INV-12，本 ADR 修订 INV-04）、ADR-007（事件主题命名）、ADR-023（core 是唯一任务事件发射者）、ADR-038（终端回放与租约，不受影响）；任务 `.trellis/tasks/10-25-native-agent-sessions`（决定记录 `research/grill-decisions.md` Q1–Q28）

## 背景

Claude Code、Codex、omp 目前跑在真 PTY 里，用户看到的是各自的 TUI；只有 DSH 经宿主的结构化通道进入原生会话页。
INV-12 禁止从终端 ANSI 文本推断结构，所以对 PTY 里的 Agent，原生界面只能在事后读会话文件：

- 消息、工具调用、审批都不能实时呈现；UI 改版 M2 做好的气泡、工具行、内嵌审批卡对 Claude/Codex 只有历史可看；
- 审批只能在 TUI 里按键完成，原生审批卡无事可做；
- “停止”只能结束整个任务（`task.cancel`），没有“中断当前一轮”；
- 运行状态靠托管 home 里的 hook 脚本（`aeroric-hook.mjs`，需要 Node）间接得到。

## 参照依据

| 项目 | 事实 | 出处 |
|---|---|---|
| codeg | 全部 Agent 经 ACP（Agent Client Protocol）驱动，Rust crate `agent-client-protocol` 2.2；Claude 经 `claude-agent-acp`、Codex 经 `codex-acp` 适配；PTY 只用于内嵌终端 | `harness/codeg/src-tauri/Cargo.toml:157`、`src-tauri/src/acp/registry.rs:259-279` |
| Agents-Anywhere | Codex 经 `codex app-server --listen stdio://`；Claude 经 Claude Agent SDK 的 `can_use_tool` 审批回调 | `harness/backlog/Agents-Anywhere/connector/connector/runtimes/codex/sdk/client.py:1283`、`connector/connector/runtimes/claude/turns/interactions.py:141` |
| Claude Agent SDK | SDK 不实现 Agent，只以 `claude --output-format stream-json --verbose --input-format stream-json --permission-prompt-tool stdio [--include-partial-messages] [--model] [--permission-mode] [--resume]` 启动 CLI，在 stdin/stdout 上走 control_request/response：出站 `initialize`、`interrupt`、`set_model`、`set_permission_mode`、`get_context_usage`、`rewind_files`、`stop_task`、`mcp_*`；入站 `can_use_tool`、`hook_callback`、`mcp_message`、`task_started`/`task_updated`/`task_notification` | `claude_agent_sdk/_internal/transport/subprocess_cli.py:574-793`、`claude_agent_sdk/types.py:2288-2304`（Agents-Anywhere 自带的 SDK 副本） |
| Codex | `codex app-server`（标注 experimental）提供 `thread/*`、`turn/*`、`item/*`（含 `agentMessage/delta`、`reasoning/*`、`commandExecution/outputDelta`、`commandExecution`/`fileChange`/`permissions` 的 `requestApproval`、`plan/delta`）、`model/list`、`account/*`；`generate-json-schema` 可导出协议 Schema | codex-cli 0.162.1 实测（`codex app-server --help`、`generate-json-schema`） |
| orca、herdr、LiveAgent | 终端优先：多 PTY 复用，对话是 Agent 自己的 TUI | `harness/orca`、`harness/backlog/herdr`、`harness/LiveAgent` |
| Aeroric core | 已有一个持久的 `codex app-server` JSON-RPC 客户端（目前只用于用量快照） | `core/crates/aeroric-core/src/metrics/usage.rs:30-110` |

## 决定

1. **Agent 经结构化协议驱动。**
   - Claude Code：`claude` CLI 的 stream-json 控制协议（参数与控制消息见上表）。
   - Codex：`codex app-server` 的 stdio JSON-RPC，宿主内共享一个进程、多会话。
   - DSH：保持现有结构化宿主通道。
   - omp：二期评估 `--mode rpc`；在此之前仍走 PTY 与现行规则（`CLAUDE.md` omp 要点）。
2. **驱动在 Rust core 内实现，不依赖 Node / Python 运行时。** core 的会话事件模型参照 ACP `session/update`
   （消息片段、思考片段、工具调用与更新、计划、权限请求与处理、子 Agent、用量、配置变更、错误、未识别内容），
   以后若要接更多 Agent，可再加一个 ACP 驱动而不改界面。
3. **不再有“终端模式”。** 一个会话怎么呈现由 Agent 族决定，不是用户可以切换的模式。迁移期间 omp 仍是终端呈现；
   `InteractionModeController` / `SessionPresentation` 对已结构化的族固定为原生，四族迁完后删除。
4. **INV-04 改写为：** 呈现层操作（滚动、折叠、切换面板或窗口）不得改变会话进程、会话 id、权限、模型或 cwd；
   改变模型、权限、effort 只能经驱动的显式 configure，从下一轮生效；中断只停止当前一轮，不结束会话。
5. **INV-12 保留并强化：** 会话结构只来自协议消息；协议里不认识的字段或事件保留为带来源的 unknown 项并显示，不丢弃、不猜测。
6. **hook：** 已结构化的族不再安装 hook，任务状态由协议事件经 core 任务归约器产生（ADR-023 不变）；omp 二期前保留 hook。
7. **启动环境不变：** 托管 home、自定义 Agent 档案、密钥注入照旧，只换启动参数与通信方式。
8. **版本门槛：** Claude Code ≥ 2.1、Codex ≥ 0.162；不满足时明确提示升级，不回退到 PTY。

## 后果

- 会话页实时化：流式消息、工具卡、内嵌审批（批准一次 / 本会话总是允许 / 拒绝）、停止即中断、会话中途切换模型与权限。
- UI 改版 M2 的两条决定作废：“会话中不可更改 Agent、模型或权限”（改为可经 configure 切换，Claude effort 仍只读）、
  “停止 = 结束任务”（改为中断当前一轮，结束会话另设入口）。
- 新增宿主会话方法与事件主题 `session.update`（按 ADR-007 走协议变更流程）。
- PTY 终端子系统（SwiftTerm 渲染、ADR-038 回放与租约）暂留不删；原生会话稳定后另行决定删除或改作用户自己的 shell。
- `codex app-server` 是实验性接口：仓库保存其 Schema 快照，未知内容按决定 5 处理，升级时更新快照。
- Claude 控制协议没有公开规范：以 Claude Agent SDK 源码为准，用假 claude 覆盖已知消息，真 Agent 冒烟测试兜底。

## 不做

ACP 驱动、omp 迁移（二期）、删除终端子系统、粘贴图片、手机端审批。
