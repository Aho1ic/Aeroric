# Agent Note: 目检 1127a：Claude 会话读不到 Aeroric 工具的结果、页面打开前的警告看不到

Status: implemented

## Problem

目检报告在 `~/Documents/computer-use/2026-11-27-1217-aeroric-review-1127a/report.md`，测的是 11:13 打包的构建（[1126c](2026-11-27-review-1126c-findings.md) 的修复和 [E2 事实核查](../feature/2026-11-26-e2-fact-check-filter.md)）。

**通过的：**
- 顶栏“停止”的那一轮写“已中断，用时 51.5 秒”，第一轮仍是“已完成”，切走再回来不变；
- 全局记忆页有“新会话会带入 1211 / 1200 字（11 条），另有 9 条没带上”，和 `memory.profile` 一致；
- 技能页 `aeroric` 那一行写明没挂给任何一家；
- 会话页 `@任务` 引用：候选、筛选、胶囊、折叠的“附加的上下文”都对；
- 11-18 的旧会话写“未记录”；
- E2：评测 `wronglyRemoved` 0、`missed` 1（`refuted-unwrap` 模型没判驳倒）、`failed` 0；“核查意见”开关默认关、悬停说明、`review.get` 回读都对。

**查出的问题：**

1. **Claude 会话调用 Aeroric 的 Context 工具，拿回来的只有 `{"status":"ok","retryable":false}`。**
   - 目检 R5 里 Agent 回答“工具未返回实际答复，无法核实”。会话文件里它调了三次：`read_session`（`latest_assistant`）、`read_session`（`limit: 10`）、`search_sessions`，三次的工具结果都只有这一句。
   - `mcp-context` 每个结果同时带 `content`（正文）和 `structuredContent`（C8 加的状态）。Claude Code 在结果有 `structuredContent` 时，把它而不是正文交给模型。
   - 所以从 C8（2026-11-20）起，Claude 会话里所有 Aeroric 工具（读会话、记忆召回、委派结果、黑板……）都只看得到状态，看不到内容。Codex 读的是正文，不受影响。
2. **MCP 服务没连上的提示，新建会话里还是看不到。**
   - 宿主的更新日志里有：第 2 条就是 `notice` `error`“MCP 服务 missing-probe 没连上（Claude 报告 failed）”，在这一轮开始 0.2 秒后；
   - 新建会话的页面是在第一轮开始之后才打开的，`open()` 拿快照里的 `latestSeq` 当游标，之前的更新不再读。[1126c](2026-11-27-review-1126c-findings.md) 改的“留到下一轮结束”管不到它，它根本没进页面。重新打开会话也一样看不到。

## Decision

**Context 工具的结构化结果带上正文**（`aeroric-cli/src/mcp_context.rs` 的 `outcome`）。
- 宿主回的是 JSON 时，`structuredContent.result` 就是这份 JSON，原样，不再转成字符串；
- 回的是简报或文字（包括错误、连不上）时，`structuredContent.text` 是这段文字；
- `ask_expert` 的结果同样补上 `text`；
- `content` 照旧是文字，读正文的 Harness 不变。

**`session.state` 带上最近的警告**（`UpdateLog::recent_notices`、宿主 `state`）。
- 宿主从这个会话的更新日志里取最近 5 条 `warning` / `error` 的 `notice`，放进 `notices`；驱动的快照不管这个字段；
- 页面打开（`LiveSessionModel(state:)`）时把它们放进进行中的行：
  - 这一轮还在跑：和这一轮的其他提示一样，这一轮结束时留下，下一轮结束时清掉；
  - 这一轮已经结束：下一轮结束时清掉。
- 更新日志在内存里，宿主重启后就没了。

## Alternatives considered

**去掉 `structuredContent`，只留文字。** 最强的理由是最简单，Claude 和 Codex 都读正文。没选：C8 的状态（`retry`、`unclear`、`operationId`、`cursor`）是给会读结构化结果的客户端用的，去掉就退回 C8 之前。

**按 MCP 握手里的 `clientInfo` 区分 Claude，只对它不发 `structuredContent`。** 没选：客户端名字会变，别的 Harness 将来也可能改读结构化结果；让两边都带上正文，谁读哪一份都完整。

**结构化结果里只放 `text`，宿主的 JSON 也转成字符串放进去。** 没选：模型看到的是转义过的 JSON 字符串，更难读，也多花 token。

**页面打开时从第 0 条起重读整个更新日志，只挑出提示。** 最强的理由是不用改宿主。没选：长会话的日志有上千条（消息分片），每次打开都读一遍很浪费；宿主那边挑出几条，一次就够。

## Consequences

- **收益：**
  - Claude 会话里 Aeroric 的工具重新可用：`@任务` 引用、读会话、记忆召回、委派回报、黑板都能看到内容；
  - 新建会话、重新打开会话时，已经发生的 MCP 启动失败等警告看得见。
- **局限：**
  - 宿主的 JSON 原样放进结构化结果，体积和文字版差不多；结果很大时，模型看到的是结构化那份；
  - `session.state` 只带最近 5 条，宿主重启后清空；
  - C8 以来 Claude 会话里依赖工具内容的行为（比如记忆召回的效果、H8 的“本来会触发”统计背后的使用）都打了折扣，之前的目检没有发现。
- **验证：**
  - Rust：`mcp_context.rs` 的 `the_structured_result_carries_the_answer` 和 `results_say_how_the_call_went` 加的断言；
  - Swift：`LiveSessionModelTests.testAWarningSaidBeforeThePageOpenedIsShown`；
  - 目检 1127b（`~/Documents/computer-use/2026-11-27-1559-aeroric-review-1127b/report.md`）：
    - Claude 会话里 `read_session` 的结果带 `result.reply` 正文，Agent 原样引用；`search_sessions` 列出含 gamma 的会话标题；
    - 新建会话里 MCP 没连上的红色提示看得见，切走再回来还在，下一轮结束后消失；
    - 清单要 Agent 引用出 `beta`，没对上：那个 beta 任务在 11-26 的插话、停止测试里被接着用过，最后一条回答本来就是“我会按重新发出的请求……”，工具读得没错，是清单挑的会话不对。
