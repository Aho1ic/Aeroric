# Agent Note: E13 Context 工具带上 MCP 注解

Status: implemented

## Problem

[1118 调研](../../proposed/architecture/2026-11-19-new-addition-1118-research.md) E13：给 Context 工具标上 MCP 注解 `readOnlyHint`、`destructiveHint`、`openWorldHint`。Codex 等客户端可以据此决定一个工具要不要审批。来源是 agent-knowledge-mcp。

动手前（`main@774623e0`）核对的事实：

- 方案写的是 21 个工具，现在是 22 个，D5 加了 `plan_propose`。
- Context 服务器（`aeroric-cli` 的 `mcp_context.rs`）的 `tools/list` 只回名字、说明和参数结构，没有注解。
- 共用工具表 `AERORIC_TOOLS` 已经给每个工具记了范围：只读、写、发到外部。H2 的 OMP 放行规则和 C8 的评测集都用这张表。

## Decision

**注解从工具表的范围推出，不在工具定义里逐个手写：**

| 范围 | `readOnlyHint` | `destructiveHint` | `openWorldHint` |
|---|---|---|---|
| 只读（`search_sessions`、`read_*`、`board_read` 等） | 是 | 否 | 否 |
| 写（`delegate`、`board_write`、`todo_propose`、`plan_propose` 等） | 否 | 否 | 否 |
| 发到外部（`ask_expert`） | 是 | 否 | 是 |

- **写的工具都不标破坏性。** 它们只改 Aeroric 自己的记录：黑板、记忆、待办、计划、委派，不删用户的文件，也不改仓库。
- **`ask_expert` 标只读加开放世界。** 它不改本机任何东西，但会把内容发给另一个模型。
- `tools/list` 和 `all_tools()` 走同一个函数加注解，所以 `ctx --help` 和评测看到的清单与 Agent 拿到的一样。
- 表里找不到的名字不加注解。

## Alternatives considered

**在每个工具的 JSON 定义里手写注解。** 最强的理由是一眼就能看到。没选：

- 范围在表里已经记过一次，手写就是第二份；
- 新加工具时容易漏写，或者和表对不上。

现在新工具只要进了表，注解就跟着有。

**写的工具标 `destructiveHint: true`，让客户端都去审批。** 没选：

- MCP 的定义里，破坏性指会删除或覆盖已有的东西，这些工具不是；
- 标成破坏性，Codex 会对每次写黑板、提待办都要人批，Agent 协作就停住了。

## Consequences

- **收益：** 支持注解的客户端能分清只读和写，只读工具可以免审批。
- **代价：** 没有。
- **局限：**
  - 注解只是提示，客户端怎么用由客户端决定；
  - 没有在真实 Codex 上看它是否据此改变审批（NOT RUN）。
- **验证：** 命令行测试 27 个全部通过，其中新增 1 个、延伸 1 个：
  - 新增：22 个工具都有注解，且与表里的范围一致，没有一个是破坏性的，只有 `ask_expert` 是开放世界；
  - 延伸：没有宿主时 `tools/list` 回的每个工具都带注解。
