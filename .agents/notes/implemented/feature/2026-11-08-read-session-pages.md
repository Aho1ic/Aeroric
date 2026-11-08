# Agent Note: read_session 分段续读、带工具调用、读不了时说原因

Status: implemented

## Problem

Context MCP 的 `read_session`（宿主 `context.readSession`，见 [交接上下文](2026-10-30-handoff-context.md)）有三个缺口，2026-11-08 复现了前两个：

- 每条消息截到 4000 字，不标截断，也没有办法读后面的部分；
- 只取文字块，工具调用和结果全不进，Agent 读另一个会话时看不到它跑过什么命令；
- 会话记录超过 32 MiB 或读不出来时当作空会话，回 `total: 0`。

## Decision

- **分段**：每条消息最多 4000 字，回 `chars`（全文字数）和 `truncated`；被截断的带 `nextOffset`。新参数 `offset` 让第 `from` 条从这个字开始，回的那段带 `offset`。各段按字（Unicode 标量）切，拼起来正好是原文。
- **工具调用**：一次调用写成所在消息里的一行"[工具] 名称：参数 → 结果开头"。参数取 `cmd`、`command`、`file_path`、`path`、`pattern`、`query`、`url`、`description`、`prompt` 里第一个有值的，压成一行、最多 120 字；结果最多 200 字。结果单独成条的消息不再出现。
- **读不了**：记录超过 32 MiB 回 `IO_ERROR`"会话记录有 N MiB，超过 32 MiB，没有读取"；读失败回"会话记录读不出来：…"。还没写出记录的新会话仍是空的。
- **同一份消息**：`search_sessions` 和 `read_session` 用同一份带工具行的消息列表，命中的 `messageIndex` 直接能当 `from` 用。投递核对、记忆提取、侧支带回答案仍只取文字。
- MCP 工具说明写明分段和续读的用法。

## Alternatives considered

- **只加截断标记和超大原因，不带工具调用**：改动更小。没有采用（用户选了推荐）：读别的会话最常见的问题是"它跑了什么、结果如何"，只有文字答不上。
- **全文检索（FTS）**：研究里一并提过。没有做：搜索每次线性重读全项目转录的问题另算，不在这批小修里。

## Consequences

- **收益**：Agent 能读完一条长消息，能看到另一个会话的工具调用；读不了时知道原因，不会把超大会话当成空的。
- **代价**：消息数变多（只有工具调用的助手消息也算一条），搜索会命中工具参数和结果开头。
- **兼容**：返回里多了字段，旧字段不变；`offset` 是新增的可选参数。
- **验证**：宿主 `session_live::read_session_reads_on_shows_tools_and_says_why_it_cannot`：工具行、截断标记、续读拼回原文、33 MiB 的记录回原因。
