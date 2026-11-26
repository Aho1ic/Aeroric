# Agent Note: E15 MCP 没起来时，会话里写明是哪个服务、怎么了、用了多久

Status: implemented

## Problem

[1118 调研](../../proposed/architecture/2026-11-19-new-addition-1118-research.md) E15 要求：Context 工具或用户自己的 MCP 没起来时，提示写明三件事：

- 是哪个服务；
- 卡在哪一步：启动、`initialize`、`tools/list`、调用；
- 各用了多久。

来源有两个：

- OMB #3142：MCP 起不来时只说“超时”；
- CU 2026-11-13 记录过 DSH 等 `tools/list` 卡住整个回合。

动手前（`main@c4894925`）核对的事实：

- **Codex** 每个 MCP 服务启动时都会发 `mcpServer/startupStatus/updated`：
  - 内容是 `{name, status, error?, failureReason?}`；
  - `status` 从 `starting` 变成 `ready`、`failed` 或 `cancelled`；
  - `failureReason` 目前只有 `reauthenticationRequired`。

  这在 `contracts/codex/` 的 0.162.1 和 0.167.0 两份快照里都有。Aeroric 的 Codex 映射把它归为“不显示的记账通知”，直接丢掉。
- **Claude** 每一轮的 `system/init` 都带 `mcp_servers: [{name, status}]`：
  - 状态有 `connected`、`pending`、`failed`、`needs-auth`；
  - 没有错误原因，也没有时间。

  Aeroric 只从 init 里取会话 id 和模型。
- **DSH（ACP）和 OMP** 不报 MCP 的启动状态。
- **哪一步失败：** 两家都不报。Codex 的错误文字里有时写着，例如 `handshake timed out`、`tools/list timed out`。

## Decision

**新模块 `sessions/live/mcp_startup.rs`，两个驱动共用：**

**Codex（`CodexStartups`）：**

- 收到 `starting` 时记下时间。
- `failed` 记为错误级提示：“MCP 服务 X 没起来（启动 N 秒后）：<Codex 的错误原文>”。
  - 需要重新登录时，写“需要重新登录这个服务”；
  - 没有原因时，写“Codex 没有给出原因”。
- `cancelled` 记为警告：“启动被取消了（启动 N 秒后）”。
- `ready` 用时达到 5 秒，记一条信息：“用了 N 秒才准备好”。这一轮等过它，用户该知道慢在哪；快的不说。
- 每次启动各说一次；同一个服务再次启动，重新计时。

**Claude（`ClaudeStartups`）：**

- `failed` 记为错误：“没连上（Claude 报告 failed）”。
- `needs-auth` 记为警告：“需要先登录”。
- `pending` 不说，它可能还在启动。
- 一个会话里同一个服务的同一种失败只说一次；它连上过之后再失败，再说。

**Aeroric 自己的服务：** 名字显示为“Aeroric Context 工具（aeroric_context）”。失败时多写一句：这一轮用不了记忆、委派、黑板等 Aeroric 工具。

**提示怎么显示：** 用会话已有的提示行（`Notice`），按级别显示在转写里。

**哪一步失败：** 不推断，原样引用 Codex 的错误文字。

## Alternatives considered

**按错误文字推断卡在哪一步（启动、`initialize` 还是 `tools/list`）。** 最强的理由是这正是方案要的。没选：

- 错误文字是 Codex 和各个 MCP 服务自己写的，没有固定格式；
- 猜错一步比不说更误导；
- 原文里写了哪一步，用户自然看得到。

要拿到可靠的步骤和各步耗时，得让 Aeroric 自己的 Context 服务器记下每一步：进程启动、收到 `initialize`、答完 `tools/list`。这能覆盖所有 Harness，包括不报状态的 DSH 和 OMP。留作下一步。

**Codex 的 MCP 状态也像警告一样，每个会话只说一次。** 没选：同一个服务重启后又失败，是新的事实，应该再说。按“每次启动各说一次”去重，已经避免了重复。

## Consequences

- **收益：** Codex 和 Claude 会话里，MCP 没起来时能看到是哪个服务、为什么、等了多久。Aeroric 自己的工具没了时，用户知道这一轮少了什么。
- **代价：** 没有。
- **局限：**
  - DSH 和 OMP 不报 MCP 状态，这两家的会话还是看不到；
  - 不分步骤，各步耗时也没有；
  - 提示只在实时更新里，重开会话页后转写里没有，因为各家转写不记这些通知；
  - 卡住时没有提示：一直停在 `starting` 的服务，只有等它失败或就绪才说；
  - 没有在真实 Codex 和 Claude 上制造一次失败来看（NOT RUN）。
- **验证：** core 测试新增 5 个，相关的 7 个全部通过：
  - 模块 3 个：
    - Codex 失败时，写出服务名、10.4 秒和错误原文，自己的服务写明少了哪些工具；
    - 要求重新登录的写明“需要重新登录”；
    - 1 秒就绪不说，7 秒就绪说，取消是警告；
    - Claude 的失败只说一次，连上后再失败再说，`needs-auth` 是警告，`pending` 不说。
  - Codex 驱动 1 个：通知真的变成会话里的一条错误提示。
  - Claude 映射 1 个：init 带着失败的服务时出一条提示，第二次 init 不重复。
- **待目检：** 已记进路线文档 §8 待目检清单。
