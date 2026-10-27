# Agent Note: Agent 经结构化协议驱动，会话结构只来自协议消息

Status: implemented

## Problem

Claude Code、Codex、omp 跑在真 PTY 里时，原生界面只能事后读会话文件：消息、工具调用、审批不能实时呈现，审批只能在 TUI 里按键，"停止"只能结束整个任务，运行状态靠托管 home 里的 hook 脚本间接得到（需要 Node）。同时项目禁止从终端 ANSI 文本推断结构，所以 PTY 路线下原生会话页无事可做。

## Decision

- 四族 agent 经结构化协议驱动，驱动在 Rust core 内实现（`core/crates/aeroric-core/src/sessions/live/`），不依赖 Node/Python：
  - Claude Code：`claude` CLI 的 stream-json 控制协议（`--output-format stream-json --input-format stream-json --permission-prompt-tool stdio`，control_request/response）。协议无公开规范，以 Claude Agent SDK 源码为准。
  - Codex：`codex app-server` 的 stdio JSON-RPC，宿主内共享一个进程、多会话；仓库在 `contracts/codex/app-server-<版本>/` 保存其 Schema 快照。
  - omp：`omp --mode rpc`（`agents::launch::omp_rpc_args`），提示词走协议，不作位置参数。
  - DSH：宿主结构化通道。
- core 的会话事件模型参照 ACP `session/update`（消息片段、思考、工具调用与更新、计划、权限请求、子 Agent、用量、配置变更、错误、未识别内容），经事件主题 `session.update` 发给客户端。
- 不再有"终端/原生模式"切换；会话怎么呈现由 Agent 族决定。
- **呈现层操作**（滚动、折叠、切换面板或窗口）不得改变会话进程、会话 id、权限、模型或 cwd。改变模型、权限、effort 只能经驱动的显式 configure，从下一轮生效；中断只停止当前一轮，不结束会话。
- **会话结构只来自协议消息**，不从终端文本推断；协议里不认识的字段或事件保留为带来源的 unknown 项并显示，不丢弃、不猜测，更不自动批准。
- 已结构化的族不安装 hook，任务状态由协议事件经 core 任务归约器产生；omp 托管 hook 已删除，宿主启动时清理旧版装下的 hook。
- 启动环境不变：托管 home（omp 经 `PI_CODING_AGENT_DIR` 注入 `~/.aeroric/agent-homes/omp`，并在环境组合后清掉 `PI_PROFILE`/`OMP_PROFILE`）、自定义 Agent 档案、密钥注入照旧。
- 版本门槛：Claude Code ≥ 2.1、Codex ≥ 0.162；不满足时明确提示升级，不回退到 PTY。
- SSH 远程会话：Claude/Codex/omp 经 ssh 在远端以同样的结构化协议运行（`sessions/live/remote.rs`、`aeroric-host/src/remote_sessions.rs`），仅限内建 Agent。

## Alternatives considered

- **继续用 PTY，读会话文件补结构**：保留各家原生 TUI，零协议适配成本。否掉是因为实时消息、内嵌审批、中断当前一轮都无从实现，且用户明确要求舍弃 PTY、改为原生结构化对话。
- **统一经 ACP 适配器（`claude-agent-acp`、`codex-acp`）驱动**：一套协议接所有 Agent，参照项目 codeg 即如此。否掉是因为适配器需要 Node 运行时与额外分发，Aeroric 要求宿主不依赖 Node/Python；core 事件模型对齐 ACP 形状，日后可再加一个 ACP 驱动而不改界面。
- **omp 继续留在 PTY**：omp 的 rpc 模式较新。该路线在一期保留过，`--mode rpc` 驱动落地后不再需要。

## Consequences

- **收益**：会话页实时化——流式消息、工具卡、内嵌审批（批准一次 / 本会话总是允许 / 拒绝）、中断当前一轮、会话中途切换模型与权限。
- **代价与已知上限**：`codex app-server` 是实验性接口，升级时要更新 Schema 快照；Claude 控制协议无公开规范，靠假 claude（`scripts/native/fake-claude.py`）覆盖已知消息、真 Agent 冒烟兜底。PTY 终端子系统（SwiftTerm 渲染、[终端回放与租约](2026-10-02-terminal-replay-and-lease.md)）暂留；`agents/managed_home/` 中的 Claude/Codex hook 构造代码目前没有调用方，删除前需确认不再需要卸载旧版 hook。
