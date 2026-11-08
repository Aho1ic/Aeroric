# Agent Note: 映射 Codex 的 exitedReviewMode 正文

Status: rejected — 真 Codex 0.167 在 exitedReviewMode 之后会发一条正文相同的 agentMessage，落盘记录里也有；再映射会让审查结果显示两遍

## Problem

New-addition-1102 复核研究（参照 codex-plugin-cc 的 `exitedReviewMode.review → reviewText`）认为 `/review` 的审查正文被 Aeroric 丢了：`codex/map.rs` 把 `enteredReviewMode`、`exitedReviewMode` 都映射为空，而 0.162.1 的契约里 `review` 是必填字段。

## Proposal

把 `exitedReviewMode.review` 映射成一条助手文字，另写一个只发 `exitedReviewMode` 的假端用例，断言正文出现且只出现一次。

## Alternatives considered

- **开始审查时显示一行"开始审查：当前改动"**（映射 `enteredReviewMode`）：可选的小改进。没有做：`/review` 发出后界面已有回合进行中，这一行信息量小。

## Risks

2026-11-08 用真 Codex 0.167（npm 和 ChatGPT 内置两个二进制）加假模型跑 `review/start`，目标是未提交的改动：

- 线上依次有 `enteredReviewMode`、`userMessage`、`exitedReviewMode`，以及一条正文与 `exitedReviewMode.review` 完全相同的 `agentMessage`，都在父线程上，Aeroric 的实时界面会显示这条 `agentMessage`；
- 落盘记录里有同文的 assistant 消息（历史显示它），以及一条以 `<user_action>` 开头的注入消息（被 `is_injected_context` 过滤）。

所以正文没有丢，按提议映射会重复。重提的条件：某个 Aeroric 支持的 Codex 版本（≥0.162）不再发这条 `agentMessage`，或者想把审查结果做成结构化卡片（那时取 `ExitedReviewMode.review_output`）。
