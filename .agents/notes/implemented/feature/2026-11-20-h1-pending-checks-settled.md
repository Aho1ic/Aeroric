# Agent Note: H1 第二步：七条待实测逐条定论，能力表三格随之改准

Status: implemented

## Problem

[各 Harness 原生融合](../../proposed/architecture/2026-11-16-harness-native-capabilities.md) §2 末尾列了七条静态阅读定不了的事。H1 第 4 条改法要求逐条做掉：

- 能用假进程或固定版本的上游源码定的，就定下来；
- 必须跑真实 Harness 的，记为 NOT RUN 并写明条件；
- 结论写进 `docs/subsystems/agent-capabilities.md`。

[H1 第一步](2026-11-20-h1-surfaces-per-launch.md) 留下三格"还没核对"或依据不足：

- Codex 的事件入口；
- DSH 的事件入口；
- DSH 的系统层和子代理。后两格只写了"没接"，没写为什么接不了。

## Decision

依据是本地克隆的上游源码，只读不切换：

- OMP `v18.12.0` 标签（Aeroric 要求的最低版本）；
- Codex `fe8e540a`；
- DSH `fcc0ad4621`；
- ZCode `a9fe401e`。

另外读了 ACP 规范 session-setup 页的原文，看了本机 Claude Code 2.1.327 的 `--help`。没有启动任何真实 Harness 会话。逐条结论在 `agent-capabilities.md` §6 的"七条待实测的结论"。要点如下：

- **OMP 18.12.0** 已有 `--append-system-prompt`、`mcp.json` 的变量展开、`agents/` 目录，不用抬高最低版本。H2 要注意一点：没设的 `${VAR}` 会原样保留，`${VAR:-}` 才变成空串。所以没有 token 的启动应该写 `${AERORIC_CONTEXT_TOKEN:-}`。
- **Codex** 请求里的 `config` 落在会话层，Hook 和自定义角色都能按次下发。会话层的 Hook 要有信任哈希才运行，信任状态也会从会话层读，所以可以同一份 `config` 一起给。子代理的配置是父回合配置的副本，Aeroric 服务器连同 token、开场内容一起继承。实际效果 NOT RUN。
- **Claude** 插件里的 Hook 和代理在 stream-json 模式下是否生效 NOT RUN。另外找到两条不碰用户目录的按次入口：`--settings`、`--agents`。
- **DSH** 的 ACP 没有声明 `systemPrompt`，用的 SDK 也没有这个字段；ACP 不发子代理事件；没有能从外部配置的 Hook。
- **远端**：系统层已由第一步的测试断言；技能不挂。
- **ZCode** 没有 ACP，可以每轮起一次 stream-json 进程。ACP 的 `systemPrompt` 按原文：可选，需要 Agent 声明，`append` 或 `override`。

**能力表随之改的三格**：

- Codex 事件：从"还没核对"改为"没有：可以经 config 按次下发，未接（H4）"；
- DSH 事件：从"还没核对"改为"没有：没有能从外部配置的 Hook"；
- DSH 的系统层和子代理：说明里写出原因（没声明 `systemPrompt`；ACP 不发子代理事件）。

入口记录的测试同步改了期望值。

## Alternatives considered

**Codex 事件那格留"还没核对"，等 H4 实测后再改。** 最强的理由是"可以下发"还没有跑过。没选：这一格回答的是"Aeroric 现在有没有用这个入口"，答案确定是没有；"能不能"写在说明里，并标明未实测。继续写"还没核对"，会让人以为连现状都不清楚。

**用真实 Codex、Claude 当场跑掉 NOT RUN 的几条。** 最强的理由是一次定论。没选：真实 Harness 会话要用账号和网络，项目规则里真实评测要单独授权（原融合笔记 §8 第 7 问只授权在 H2、H3 之后做一次小规模评测）。本片把条件写清楚，留给那次评测。

## Consequences

- **收益**：
  - 七条每条都有结论，或者是带条件的 NOT RUN，满足 H1 的完成条件；
  - 能力表里入口项没有"还没核对"了；
  - H2 拿到了 OMP 变量展开的准确规则，H4 拿到了 Codex 和 Claude 按次下发 Hook 的路径。
- **局限**：
  - NOT RUN 四处：Codex 的 Hook 和角色实际生效；Claude 插件的 Hook 和代理在 stream-json 模式下生效；Claude 子代理调用 `aeroric_context`；三家子代理的真实工具调用；
  - Claude Code 不开源，它的结论只能靠真实会话。
- **验证**：
  - 入口记录的测试改了两家事件的期望值；
  - 文档核对通过；
  - 没有界面改动。
