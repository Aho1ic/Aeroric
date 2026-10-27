# Agent Note: 终端回放与控制租约是 v1 协议入口

Status: implemented

## Problem

core 的终端流中回放与活流是同一机制：`TerminalService` 持有每条流的字节环，订阅者交回字节游标，`plan_replay` 返回无缝续接或重置。这条面若只在 Rust 内部存在，原生客户端重连后无法续流；同时 core 的 `UnknownStream` 一个理由表示两件事——"这一代还没产出字节"（应挂起等活流）与"流已收尾并被 FIFO 退场"（终态），不拆开客户端就无法判断是等还是判死。

## Decision

- `terminal.plan_replay`（读，`maxBytes` 由 `MAX_TERMINAL_CHUNK_BYTES` 钳制）同时承接初始回放与活流尾巴：请求 `{taskId, generation, cursor: {kind: "bytes"|"unitsFromStart", value}, maxBytes}`，应答 `{kind: "seamless"|"reset", from, bytes(base64), reason, resumeAfter}`。客户端唯一要记的数是"已交付字节之后的 offset"。
- 协议 `reason` 在 core 四值（`compacted`/`unknownStream`/`cursorBeyondStream`/`unitsNotAnchorable`）之外拆出 `notStarted`（已登记未产出，客户端挂起等待，不得判死）与 `evicted`（无可重放）。
- `evicted` 与 `read_task_terminal_history` 分工固定：前者是"这一代无可重放"，后者是跨代历史转录；收到 `evicted` 不得把历史转录塞进屏幕缓冲冒充重放。
- 控制租约：`terminal.acquire_control`（顶替任何旧持有者，返回 `{ticket, supersededTicket?}`）与 `terminal.release_control`（过期票据静默成功）。
- `plan_replay` 声明 `requiresCapability: "terminal.replay"`；宿主不声明时客户端得到 `UNSUPPORTED_CAPABILITY` 并显示降级提示。租约方法不设能力门控。
- 轮询节奏是客户端私事，协议不定义；字节环容量决定落后多远会被重置。

## Alternatives considered

- **回放与活流分成两个方法**：语义分开、各自简单。否掉是因为 core 内部本就是同一机制，拆开后客户端要在两个游标之间做衔接，正是最容易丢字节的地方。
- **保留单一 `unknownStream` 理由**：协议更小。否掉是因为客户端无法区分"等"与"判死"，重连后一片空白。

## Consequences

- **收益**：重连无损续流；客户端对空流有确定的处置。
- **代价与已知上限**：ADR-043 之后 Agent 会话不再走 PTY，这条面主要服务内嵌终端；若终端子系统被删除或改作用户自己的 shell，需要同步修订本 Note。
