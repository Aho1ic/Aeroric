# Agent Note: 目检 1127c：Claude 会话里 Aeroric 工具全部可用；交接清单把只读的 Aeroric 工具列成“已发生改动”

Status: implemented

## Problem

目检报告在 `~/Documents/computer-use/2026-11-27-1727-aeroric-review-1127c/report.md`，测的是 `e76d8492`（[1127a](2026-11-27-review-1127a-findings.md) 的修复）。

[1127a](2026-11-27-review-1127a-findings.md) 查出：C8（2026-11-20）起，Claude 会话里 Aeroric 工具只看得到 `{"status":"ok"}`。1127b 只验了读会话和搜会话，这一轮在一个 Claude 会话里把其余工具逐个调一遍。

**通过的（工具结果带正文，Agent 的回答用到了结果里的内容）：**
- `read_session`：`@` 引用一个新建的会话，结果里有 `"reply":"KESTREL-41"`，Agent 原样引用；
- `list_roles`：六个角色 id 和结果一致；
- `read_handoff`：复述了目标；
- `artifact_list`：0 个交付物，结果里是空列表；
- `memory_recall`：7 个记忆 ID 和结果一致；
- `board_write` / `board_read`：写入 `b1` 又读回来；
- `todo_propose`：待办页的待确认里出现这一条，丢掉后消失；
- `delegate` / `delegations`：子会话回报 21（项目根目录确实是 21 个文件，含两个隐藏文件），合并结果送回父会话；`delegations` 写 `reported`，`returnDelivery.state` 为 `delivered`。
- Wiki 工具：测试项目没有 Wiki 页面，不提供，不适用。

`list_roles`、`read_handoff` 的原始结果报告里没截到，按会话 B 的 Claude 会话文件核对过，都带正文。

**查出的问题：**

`read_handoff` 的清单在“已发生改动（不要重做）”下面列了：
- 回合 2：`mcp__aeroric_context__read_session`
- 回合 3：`mcp__aeroric_context__list_roles`

这两个工具只读，什么也没改。检查点推导（`checkpoint/derive.rs` 的 `counts`）把所有 MCP 调用都当作副作用。接手的 Agent 读到这一节，会以为前面做过了改动，真正的改动也被它们挤占：改动最多保留 `MAX_ENTRIES` 条，超出的折叠成一个数字。一轮没等到结果的只读调用还会进“结果未知（先核对）”。

## Decision

**Aeroric 自己的只读工具不算副作用**（`counts` 多看工具标题）。
- 哪些只读，查共用的工具表 `aeroric_protocol::tools::AERORIC_TOOLS`（E13 起 MCP 注解 `readOnlyHint` 也按它标）里 `ToolScope::ReadOnly` 的 13 个：`search_sessions`、`read_session`、`read_handoff`、`delegations`、`list_roles`、`board_read`、`artifact_list`、`artifact_read`、`memory_recall`、`search_references`、`read_reference`、`read_wiki`、`search_wiki`；
- 两种标题都认：`mcp__aeroric_context__<工具>`（Claude、omp、ACP）和 `aeroric_context.<工具>`（Codex）；
- 会写东西的照旧算：`board_write`、`board_decide`、`todo_propose`、`plan_propose`、`delegate`、`memory_propose`、`memory_feedback`、`flag_wiki_issue`、`ask_expert`；
- 别的 MCP 服务器的工具照旧算，即使同名（`mcp__github__read_session` 仍是副作用）：Aeroric 不知道它们改不改东西。

## Alternatives considered

**所有 MCP 调用都不算副作用，只算命令和改文件。** 最强的理由是最简单，而且 MCP 工具大多是查询。没选：外部 MCP 能发消息、建工单、改远端数据，这正是交接清单要提醒“不要重做”的东西。

**读 MCP 工具的 `readOnlyHint` 注解来判断。** 最强的理由是对外部服务器也适用。没选：检查点推导只看得到驱动发出的更新，里面没有工具注解；而且注解由服务器自己声明，不一定可信。Aeroric 自己的工具直接查共用的工具表，和注解出自同一处，不会两边不一致。

**在检查点推导里另写一份只读名单。** 最强的理由是不跨 crate。没选：以后加工具时要记得两处一起改；共用的表已经在 `aeroric-protocol` 里，核心本来就依赖它。

## Consequences

- **收益：** 交接清单的“已发生改动”只列真正改过东西的调用；Agent 查一下上下文，不会挤掉真正的改动，也不会进“结果未知”。
- **局限：**
  - 已经存下的检查点不回头改，这次修复之前记下的只读调用仍留在旧清单里，直到被新条目挤掉。
- **发现、未处理：** 会话中途调 `read_handoff`，清单标题固定写“恢复会话”（宿主 `context_handoff` 传的是 `HandoffReason::Resume`），其实这个会话没有恢复过。只是标签不准，不影响接手；要改得在协议里加一种交接原因，契约跟着变，暂不改。
- **验证：**
  - Rust：`checkpoint::derive_tests` 的 `aeroric_context_reads_are_not_effects_but_its_writes_are`：Claude、Codex 两种标题的只读调用不算，没等到结果的也不进“结果未知”；`board_write`、`todo_propose` 和外部同名工具照旧算；
  - 目检 1127d（`~/Documents/computer-use/2026-11-27-2123-aeroric-review-1127d/report.md`）：同一个 Claude 会话里依次调 `list_roles`、`board_read`、跑 `ls -1`、`board_write`，再 `read_handoff`；“已发生改动”只有回合 4 的 `ls -1` 和回合 5 的 `board_write`，没有只读工具，也没有“结果未知”一节。
