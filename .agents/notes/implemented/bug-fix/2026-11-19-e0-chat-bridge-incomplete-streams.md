# Agent Note: E0 Chat 桥接如实传递"没说完"

Status: implemented

## Problem

[1118 调研](../../proposed/architecture/2026-11-19-new-addition-1118-research.md) 的 E0。和 LinkSense #79 是同一个缺陷。

`core/crates/aeroric-core/src/local_router/chat_bridge.rs` 的 `ChatSseTransformer` 负责把 Chat Completions 的流转成 Codex 要的 Responses 事件，但它不区分回答是怎么结束的：

- `handle_event` 不读 `finish_reason`；
- 收到 `[DONE]`，或者上游连接关闭时调 `finish()`，都一律走 `emit_finish`，发出 `response.completed`；
- 非流式的 `chat_response_to_responses` 也总是写 `"status":"completed"`。

后果：

- 上游因为长度上限截断（`finish_reason=length`）、被内容过滤拦下（`content_filter`）、或者连接中途断开，Codex 都收到一个"完整的回合"：回答说到一半就结束了，却显示成功；
- 截断在工具参数中间时，半截参数也被标成完成的工具调用，可能被执行；
- 回合分级和自动检查都把它当成正常完成。

[换模型重跑](../feature/2026-11-11-rerun.md) 和日常使用都会经过这条桥（例如经 Magpie 用 `42/deepseek-flash` 跑 Codex）。另一条桥 `pivot_bridge.rs` 已经映射了 `stop_reason`，没有这个问题。

## Decision

- 流里记下最后一个 `finish_reason`（`choices[0].finish_reason`）。
- 流的结束分三种：
  - **正常结束**：收到 `[DONE]`，或者见过结束原因后连接关闭，而且原因不是下面两类之一。照旧发 `response.completed`。
  - **没说完**：原因是 `length`、`max_tokens`、`max_output_tokens`（记为 `max_output_tokens`），或者 `content_filter`、`sensitive`（记为 `content_filter`）。发 `response.incomplete`，带 `incomplete_details.reason` 和用量。已经开始的文本和工具调用一律不发 done 事件，半截的工具调用不会执行。
  - **中途断开**：连接关闭时既没有结束原因，也没有 `[DONE]`。发 `response.failed`，原因是"上游在回复结束前断开了连接（没有收到结束标记）"。
- Codex 对 `response.incomplete` 和 `response.failed` 按它自己的逻辑处理（报错，可以重试的就重试）。Aeroric 不重放回合，也不补造用量。
- 兼容：
  - 只发 `[DONE]`、不带 `finish_reason` 的供应商，照常算完成（原有测试 `converts_chat_sse_to_responses_events`）；
  - 带结束原因但不发 `[DONE]` 的供应商，也算完成；
  - 其他没见过的结束原因一律按完成处理，只有上面两类算"没说完"。
- 非流式转换按同样规则，把响应和其中的条目标为 `incomplete`，并写上 `incomplete_details`。

## Alternatives considered

**中途断开时什么也不发，让 Codex 自己发现流断了。** 最强的理由是这和直连 Responses API 时断线的表现完全一样。没选：网关和服务端日志里就看不出这次尝试为什么失败；发一个带原因的 `response.failed`，Codex 同样会走它的错误处理，记录里还能留下原因。

**把截断前已经写出的文本作为已完成的条目交给 Codex。** 最强的理由是用户能看到写到一半的内容。没选：Codex 会把它当成这一轮的正式输出写进历史，下一轮在半句话的基础上继续，比明确报"没说完"更难察觉。

## Consequences

- **收益**：被截断、被过滤、断线的回答不会再被当成成功的回合；半截的工具调用不会执行。
- **代价**：以前"看起来成功"的截断回合，现在会在 Codex 里报错。这是如实暴露，不是新故障。
- **验证**：
  - `chat_bridge.rs` 新增 5 个测试：
    - `a_stream_that_drops_before_its_end_fails_instead_of_completing`：只有文本增量就断开，结果是 `response.failed`；
    - `a_length_stop_is_incomplete_and_its_half_tool_call_is_not_done`：`length` 截断在工具参数中间，没有任何 done 事件，结束于 `response.incomplete`，原因是 `max_output_tokens`，用量保留；
    - `a_content_filter_stop_is_incomplete_even_without_done`：`content_filter` 之后不发 `[DONE]` 就断开；
    - `a_normal_end_completes_with_either_marker`：只有结束原因、只有 `[DONE]`、`tool_calls` 结束这三种都算完成，关闭后不重复发终止事件；
    - `a_non_streaming_length_stop_is_incomplete`：非流式同理。
  - 网关模块 186 个测试全部通过。
  - 没有用真实上游和真实 Codex 跑过（NOT RUN）：还没确认 Codex 当前版本对 `response.incomplete` 的提示文案在 Aeroric 界面上怎么显示。
