# Agent Note: C8 第三步：工具触发评测集进仓库，假 Harness 跑通，真实 Harness 未跑

Status: implemented

## Problem

[批次 C 方案](../../proposed/feature/2026-11-15-adoption-c-memory-gateway-skills.md) C8 第三步要求：

- 给每个 Aeroric 工具写 3–5 条"应该触发"和"不应该触发"的提示；
- 用假 Harness 跑通流程；
- 真实 Harness 的评测记为 NOT RUN，并写明需要的条件；
- 评测集放在仓库里，以后改工具描述时作回归输入。

动手前（`main@dd1707bc`），仓库里没有这样的评测集。以前的工具评测都是用真 Harness 跑完、把结果写进笔记，例如 [Wiki](2026-11-10-wiki-agent-eval.md)、[参考库](2026-11-13-reference-library-eval.md)、[Smart Friend](2026-11-13-smart-friend-eval.md)。题目散在各篇笔记里，不能重复跑。改了某个工具的描述，也无法知道别的工具会不会被抢走或误触发。

## Decision

**评测集**（`core/crates/aeroric-cli/evals/tool-triggers.json`）：

- [C8 第一步](../feature/2026-11-20-c8-effective-tool-catalog.md)共享工具表里的 21 个工具，每个都有：
  - `should`：3–4 条应该触发的用户请求；
  - `shouldNot`：3 条不应该触发的。每条写明 `instead`，也就是该用哪个工具，或 `none`（直接干活，不用 Aeroric 工具）。
- 共 127 条。
- "不应该触发"专门挑容易混的相邻工具，例如：
  - `board_write` 和 `board_decide`；
  - `memory_propose` 和 `memory_feedback`；
  - `search_wiki` 和 `read_wiki`；
  - `delegate` 和 `ask_expert`；
  - 读交付物和读项目文件。

  也包括不该用 Aeroric 工具的情况，例如"把这个密钥记下来"应该拒绝，不该调用 `memory_propose`。

**运行器**（`aeroric-cli/src/tool_eval.rs`，只在测试里编译）：

- `Harness` 接口：给一条请求和全部工具的名字与描述，返回调用哪个工具，或不调用。
- 运行时把 `mcp-context` 服务器开在一根管道上，按 Agent 的方式先 `initialize`、`tools/list`，再对每条请求把选中的工具真的 `tools/call` 一次，记下返回的 `structuredContent.status`。
- 报告按工具统计：
  - 应触发命中几条；
  - 误触发几条；
  - 选择与预期（本工具、`instead` 指的工具，或不调用）完全一致几条。

**假 Harness**（`KeywordHarness`）：

- 按请求和工具描述共有的词打分：拉丁字母的单词，加上汉字两两相邻的字对；请求里直接写出工具名的，算最高分。
- 取最高分的工具，分数低于下限（2）就不调用。
- 它不是模型，只用来证明流程跑得通，并给出一个基线。

**测试**：

- 评测集恰好覆盖共享工具表，每边 3–5 条，`instead` 必须是存在的另一个工具或 `none`，没有重复的请求；
- 假 Harness 跑完全部 127 条：选中的都是列出的工具，每次调用都有带状态的回应（这里没有宿主，状态是 `retry`），没选的没有发调用；
- 报告的计数规则。

**基线**（关键词假 Harness，2026-11-20）：

- 应触发命中 45/64；
- 误触发 7/63；
- 与预期完全一致 83/127。
- 最弱的几个：
  - `board_write` 命中 0/3：请求里写的是"黑板""认领"，描述里也有，但被 `board_decide` 和 `board_read` 抢走；
  - `delegations` 命中 1/3；
  - `search_wiki` 命中 1/3。
- 误触发最多的是 `board_decide`，2/3。
- 这些数字说明哪些工具的描述靠关键词难以区分，不代表模型的表现。

**真实 Harness：NOT RUN。** 需要的条件：

1. 一个有项目的测试宿主，并打开全部工具组：项目开启记忆，Wiki 有页面且允许 Agent 读，登记至少一个参考库，设置专家模型。否则 `tools/list` 只有基础工具，记忆、Wiki、参考库、专家几组工具测不到。
2. 一个真 Harness 和模型。按评测约定是 Codex 配置走 Magpie，模型 `codex/gpt-6.2-sol`。
3. 每条请求开一个新会话，记下 Agent 第一次调用的 Aeroric 工具，或者没有调用。Context 服务器那一侧已经能看到每次调用，但还没有把"这个会话第一次调用了什么"汇总给评测脚本的出口。
4. 一次跑 127 个会话的时间和花费。

UI 自动化和测试虚拟机目前暂停，这一步不跑。

## Alternatives considered

**直接用真 Harness 跑一遍再提交。** 最强的理由是只有模型的选择才说明描述写得好不好。没选：

- 测试环境暂停；
- 每次改描述都要花一轮钱和时间；
- 先把题目和流程固定下来，真跑时只需换掉 `Harness`。

**假 Harness 按 `instead` 字段作弊，直接返回预期答案。** 最强的理由是流程测试完全确定。没选：那样报告永远是满分，看不出任何东西。关键词匹配至少能暴露"两个工具的描述太像"的问题，例如 `board_write` 和 `board_decide`，而流程测试本身不依赖它的分数。

**评测集写成 Rust 代码里的常量。** 没选：JSON 文件改起来直观，真 Harness 的脚本也能直接读，不用编译。

## Consequences

- **收益**：
  - 工具描述以后有了固定的回归题目；
  - 任何人加新工具时，测试会要求先补上评测题，因为评测集必须覆盖共享工具表；
  - 跑真 Harness 时只需实现一个 `Harness`，其余流程已经验证过。
- **局限**：
  - 只测第一次选哪个工具，不测参数是否正确；
  - 题目是中文的，与产品界面一致，没有英文题；
  - 基线来自关键词匹配，不能当作模型能力的估计；
  - 真实 Harness 评测没有跑。
- **验证**：命令行新增 3 个测试（见上），全部通过；基线数字来自测试的输出（`--nocapture`）。
