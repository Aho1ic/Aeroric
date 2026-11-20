---
name: aeroric
description: "需要以前的讨论、项目约定、设计决定，或者想把一块工作交给别人时用。通过 aeroric-cli ctx 命令查 Aeroric 的历史会话、记忆和 Wiki，读写共享黑板，发起委派。只在没有 Aeroric MCP 工具的会话里提供。"
---

# Aeroric 上下文（命令行）

Aeroric 是启动这个会话的应用。它记着这个项目以前的会话、你和用户积累下来的记忆、项目 Wiki，还管着多个 Agent 共用的黑板和委派。这个会话没有 Aeroric 的 MCP 工具，同样的事用命令行做。

## 怎么调用

```sh
"$AERORIC_CLI" --data-dir "$AERORIC_DATA_DIR" ctx <工具名> --json '<参数 JSON>'
```

- 三个环境变量由 Aeroric 启动会话时给出：`AERORIC_CLI`（命令路径）、`AERORIC_DATA_DIR`（数据目录）、`AERORIC_CONTEXT_TOKEN`（令牌，只能访问这个任务的项目）。不要把令牌写进文件或输出。
- 参数长的时候用 `--json -`，从标准输入传。
- 输出一行 JSON：`content` 是文字结果；`structuredContent.status` 是 `ok`、`partial`、`retry`、`rejected` 或 `unclear`。`retry` 可以原样再调一次；`unclear` 时带上同一个 `idempotency_key` 再调，做过的不会重复做。
- 退出码：0 成功；2 参数不对或没有令牌；3 连不上 Aeroric；4 被拒绝或结果不明。
- `"$AERORIC_CLI" --data-dir "$AERORIC_DATA_DIR" ctx --help` 列出全部工具和参数。

## 什么时候用

- 用户提到"上次""之前那个方案""我们说过"：先 `search_sessions` 或 `memory_recall`，再 `read_session` 看原话。
- 要遵守项目约定、了解模块设计：`search_wiki`、`read_wiki`。
- 接手别人做了一半的事：`read_handoff` 看交接清单。
- 一块工作适合交给另一个 Agent：`list_roles` 看有哪些角色，`delegate` 发出去，`delegations` 看进度。
- 和其他 Agent 共享发现、做决定：`board_read`、`board_write`、`board_decide`。
- 学到了以后还用得上的事实或经验：`memory_propose` 提议，用户确认后才生效。

只在确实需要时调用，不要每一轮都先查一遍。

## 例子

```sh
"$AERORIC_CLI" --data-dir "$AERORIC_DATA_DIR" ctx memory_recall --json '{"query": "登录流程"}'
"$AERORIC_CLI" --data-dir "$AERORIC_DATA_DIR" ctx search_sessions --json '{"query": "OAuth", "limit": 5}'
```
