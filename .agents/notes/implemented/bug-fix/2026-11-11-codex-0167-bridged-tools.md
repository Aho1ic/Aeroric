# Agent Note: Codex 0.167 经路由转 Chat 时拿不到工具，还会报 Lite 400

Status: implemented

## Problem

Codex 会话选带前缀的模型（如 `codex/gpt-6.1-sol`）时，Aeroric 的本地路由把 Responses 请求转成 Chat Completions 发给网关（这里是 Magpie）。[R4-3 评测](../testing/2026-11-10-wiki-agent-eval.md)在这条路上撞到两个问题：

1. **模型没有任何工具**。8 个会话都是 0 步工具调用，模型自己说"当前会话没有可调用的代码读取或搜索工具"。原因有三处：
   - Codex 0.167 不再把工具放在请求的 `tools` 字段，而是放在输入里一条 `{"type":"additional_tools","role":"developer","tools":[…]}`，按命名空间分组。`chat_bridge` 只读 `tools`，结果一个工具都没转过去；这条条目带着 `role`，还被当成一条空的 developer 消息。
   - code mode 的 `exec` 是带 lark 语法的自定义工具（`type: custom`），放在 `functions` 命名空间里。Codex 把这个命名空间当顶层：它记录的调用是不带 `namespace` 的 `custom_tool_call`，`input` 是原始代码。桥接却把模型的调用还原成 `function_call`。
   - 历史里的 `custom_tool_call` 转成 Chat 时，参数写成了原始代码；而这个工具在 Chat 里声明的参数是 `{"input": …}`，两边对不上。
2. **上游返回 400**：`X-OpenAI-Internal-Codex-Responses-Lite requires reasoning.context to be all_turns`。转 Chat 后请求体里已经没有 `reasoning.context`，Codex 带的 Lite 头却原样转发了，网关再转回 Responses 时被上游拒绝。zzzcoding 上的 `gpt-6-astra` 也报过同样的错。

## Decision

**Lite 头**（`local_router/server.rs` 的 `outbound_headers`）

- 走协议转换（转 Chat 或 Messages）时删掉 `x-openai-internal-codex-responses-lite`；直通 Responses 时保留。

**工具清单与调用**（`local_router/chat_bridge.rs`）

- `offered_tools` 合并 `tools` 和所有 `additional_tools` 条目里的工具；`responses_input_to_chat` 跳过 `additional_tools` 条目。
- `functions` 命名空间（`TOP_LEVEL_NAMESPACE`）里的工具按顶层处理：转 Chat 时不加前缀，还原调用时也不带 `namespace`。其他命名空间沿用[委派时定下的规则](../feature/2026-10-31-delegation.md)，展开成 `命名空间__工具名`，回来再拆开。
- `ToolNamespaces` 从 `HashMap` 别名改成结构体，每个工具记下命名空间、原名，以及是不是自定义工具。自定义工具的调用这样还原：
  - 非流式：`restore_tool_namespaces` 把它改成 `custom_tool_call`，`input` 取参数里的 `input`；模型没按 `{"input": …}` 包一层时，用原样的参数。
  - 流式：`ChatSseTransformer` 对它不发 `function_call_arguments` 增量，在 `output_item.done` 里一次给出完整的 `custom_tool_call`。参数是 JSON 包着的，要等 JSON 完整才能取出 `input`。
- 历史里的 `custom_tool_call` 转 Chat 时，参数写成 `{"input": …}`。

## Alternatives considered

- **只修 Lite 头，工具问题另说**：改动小。没选：不修工具，这条路上的 Codex 会话什么都做不了，修好 400 也没有意义。
- **自定义工具调用仍按 `function_call` 回给 Codex，指望它兼容**：不用改流式转换。没选：Codex 记录的 `exec` 调用一直是 `custom_tool_call`，换成 `function_call` 时，它很可能拒绝，或者把参数当成 JSON 去解析。
- **流式时把 `input` 边解析边发增量**：界面上能看到代码逐字出现。没选：要在不完整的 JSON 里增量解出字符串，复杂又容易错；Codex 以 `output_item.done` 为准，一次给出就够了。

## Consequences

- **收益**：
  - 经网关的 Codex 会话又能用 `exec` 和 Aeroric 上下文工具了；R4-3 的 W3 里 8 题都正常调用了工具。
  - 网关不再因为 Lite 头返回 400。
- **代价**：
  - 自定义工具的调用在流式回复里没有中间增量，界面要等整段生成完才显示代码。
  - 以后 Codex 再换工具清单的放法，这里还得跟着改；`additional_tools` 是从会话记录里看出来的，没有正式文档。
- **验证**：
  - 路由单测 `bridges_codex_responses_to_chat_completions`、`pivots_codex_responses_stream_to_a_messages_upstream`：转换后不带 Lite 头、不带 `reasoning` 和 `client_metadata`；直通时 Lite 头和 `reasoning.context` 原样保留。先在修复前的代码上确认这两个测试会失败。
  - 路由单测 `additional_tools_reach_chat_and_custom_calls_come_back_raw`，覆盖三条路径：请求、非流式回复、流式回复。先在修复前的代码上确认会失败。路由测试共 174 个，全部通过。
  - 实测：R4-3 W3/W3b 走 Codex 配置下的 `codex/gpt-6.1-sol`，8 个会话都正常结束，工具调用 3–6 步，其中两次在 `exec` 里调了 `search_wiki`。
