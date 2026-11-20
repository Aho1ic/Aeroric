# Agent Note: C8 第二步：工具调用的结果分得清，每次调用都有回应，重试不做两次

Status: implemented

## Problem

[C8 第一步](2026-11-20-c8-effective-tool-catalog.md)让会话看得到有哪些工具。方案的第二步要求 Agent 调用 Aeroric 工具之后，结果分得清成功、部分成功、需要重试、被拒绝。动手前 `aeroric-cli mcp-context`（`main@610b34f7`）的情况：

- **结果只有一个布尔值。** `call_tool` 把宿主的回复压成一段文字，失败时 `isError: true`。Agent 分不出三种情况：
  - 宿主说参数不对，重试没用；
  - 宿主忙，可以再试；
  - 连接断了，不知道做没做成。
- **读入没有上限。** 标准输入按行读，一行多长就读多长，一个异常的超长行会被整个读进内存。
- **坏行被静默丢掉。** 解析失败的行直接 `continue`，即使那行里看得出 `id`，调用方也收不到任何回应，只能等到超时。
- **重试可能做两次。** `delegate`、`board_write`、`board_decide`、`memory_propose` 都有副作用。宿主做完了，但回复在路上丢了，Agent 重试就会再委派一个子会话，或再写一条黑板。

## Decision

**每个工具结果都带 `structuredContent`**（MCP 2025-06-18 的字段），文字部分照旧，旧 Harness 不受影响：

- `status` 有五种：
  - `ok`；
  - `partial`：宿主回复里标了 `partial`；
  - `retry`：可以原样再调，比如没连上宿主，请求根本没发出，或宿主返回 `Busy` 这类可重试的错误码；
  - `rejected`：宿主拒绝，比如参数不对、没有权限、没有这个工具，原样再调没用；
  - `unclear`：有副作用的调用已经发出，但宿主没回，不知道做没做成。
- 另外几个字段：
  - `retryable`：`retry` 和 `unclear` 为真；
  - `code`：宿主的错误码；
  - `operationId`：有副作用的调用都有；
  - `cursor`：读会话时还有后文，取宿主的 `nextOffset`；
  - `artifactRefs`：回复里列出的交付物 id；
  - `replayed`：这是第一次调用的结果。
- `isError` 只在 `ok` 和 `partial` 以外为真。`ask_expert` 的结果也补上状态：失败记为 `unclear`，可以重试。

**单行上限与坏行回应**（`read_line` / `serve_with`）：

- 每行最多 1 MiB（`MAX_LINE_BYTES`）。超过时继续读到行尾、丢掉，只留前 4 KiB 用来找 `id`。找到 `id` 就回 JSON-RPC 错误 `-32600`，`data` 里写 `limitKind: lineBytes`、`allowed`、`observed`、`retryable: false`；文字说明"请把内容拆小，或写进文件再传路径"。
- 解析失败的行，能从文字里读出顶层 `id`（第一个 `"id":` 后面的数字或字符串）就回 `-32700`。读不出 `id` 的，写一行 stderr，不回。按 JSON-RPC，回应要对得上请求的 id，没有 id 就没有可回的对象。
- 一个坏行不会让服务停下，后面的请求照常处理。主循环抽成 `serve_with(reader, writer)`，测试可以直接喂数据。

**幂等键**：

- 四个有副作用的工具（`delegate`、`board_write`、`board_decide`、`memory_propose`）在 `tools/list` 里多一个可选参数 `idempotency_key`，说明是"超时或断线要重试时带上同一个键，只执行一次"。
- 命令行进程把键从工具参数里取出（宿主各方法的参数拒绝未知字段），放进请求信封自带的 `idempotencyKey`。Agent 没给时，自己生成一个 `auto-…`，作为 `operationId` 返回，这样 Agent 也能拿它重试。
- 宿主（`session_live_services/once.rs`）在这四个方法外面套一层：
  - 按"方法、令牌对应的任务、键"查；
  - 见过的直接返回第一次的结果，加 `replayed: true`，不再执行；
  - 没见过的照常执行，成功的结果记下。失败不记，因为改了参数还可以再试。
- 记在 `<data>/context-once.json`，保留一天，最多 2000 个，宿主重启后仍有效。键限定在任务内，两个会话用同一个键互不影响。
- `ServiceSet` 加了 `take`，这一层在所有方法注册之后统一套上，各服务模块不用改。

## Alternatives considered

**幂等记录放在命令行进程里。** 最强的理由是不用改宿主。没选，它恰好挡不住最需要挡的情况："宿主做完了，回复丢了"：

- 进程没拿到结果，记不下来；
- 会话重启后进程也是新的。

记在宿主那边，做完就记下，与连接无关。

**读不出 id 的坏行也回一个 `id: null` 的解析错误。** JSON-RPC 规范允许这样做。没选：有的客户端收到对不上号的回应会报错或断开，而这类行本来就没有调用方在等。stderr 留了记录，可以排查。

**把五种状态压成 `isError` 加文字前缀。** 最强的理由是任何 Harness 都看得懂。没选：Agent 要靠解析文字决定重不重试，很脆弱。`structuredContent` 是 MCP 规定的位置，文字照旧给，不支持的 Harness 不受影响。

## Consequences

- **收益**：
  - Agent 能分清该重试、该改参数，还是先用同一个键确认；
  - 每个带 id 的请求都有回应，不再干等到超时；
  - 有副作用的工具重试不会重复委派、重复写黑板；
  - 超长输入不会撑爆进程内存。
- **代价**：
  - 宿主多一个小文件，每次带键的成功写入写一次盘；
  - 每个结果多几十字节。
- **局限**：
  - 同一个键的两次调用几乎同时到达时，第二次还没看到第一次的记录，仍可能执行两次；这种情况要 Agent 并发重试才会出现。
  - `todo_propose`、`memory_feedback`、`flag_wiki_issue` 没有加键：
    - `memory_feedback` 已按会话去重（C3）；
    - 另外两个重复一次只是多一条待确认的提议或问题，由用户处理。
  - `partial` 目前只在宿主回复标了 `partial` 时出现，现有方法还没有这样标的。
  - C8 第三步（触发评测）还没做。
- **验证**：
  - 命令行新增 4 个测试：
    - 超长行、`id` 是字符串的坏行、没有 `id` 的坏行、空行、正常的 ping 依次送入，前两个得到带 `id` 的结构化错误，第三、四个没有回应，ping 照常回答；
    - 从坏行里读出 `id`，嵌套的 `id` 不会被误认；
    - 结构化结果：没有这个工具是 `rejected`；连不上宿主是 `retry`，带上 Agent 给的 `operationId`，没给时自动生成；只读工具没有 `operationId`；`tools/list` 只给四个写工具加了键参数；
    - 用一个假宿主握手后收下调用就断开：结果是 `unclear`、可重试，文字说明要带同一个键重试；键在请求信封里，不在方法参数里。
  - 宿主新增 2 个测试：
    - 存储：重开后仍记得，一天后忘掉，别的任务不命中，超过上限时先删最旧的；
    - 集成：同一个键写两次黑板，返回同一条并标 `replayed`；换一个键、不带键各写一条，最后黑板上是 3 条，不是 4 条；记录写到了磁盘。
