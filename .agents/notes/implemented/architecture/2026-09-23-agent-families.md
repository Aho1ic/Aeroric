# Agent Note: Agent 的启动族、会话族、原生续跑族是三个字段

Status: implemented

## Problem

一个 agent 的"用哪套 CLI 参数启动"、"会话文件是什么格式、住在哪个 home"、"哪一族 CLI 的 `--resume` 能直接读这份转录"是三个独立问题。内置 `claude_gpt55` 在这三个问题上答案不同；用单一 `ProtocolFamily` 字段回答，就必然在其中一处答错——要么改了真实用户的启动命令，要么读不到 `~/.claude/projects` 下的历史。

## Decision

- 任务记录存三个字段：`launchFamily`（启动参数）、`sessionFamily`（会话格式与 home）、`resumeFamily`（原生续跑）。每个任务钉死自己当初怎么起的，之后修改自定义 profile 不追溯改变存量任务。
- `AgentFamilies` 端口分别提供三者；`launch_plan` 读 `launch_family`，`session_owner` 读 `session_family`，`can_native_resume` / `can_adopt` / `resolve_config_switch_strategy` 一律读 `native_resume_family`，**不得**拿另两个字段顶替。
- 内置取值：`claude→claude`、`codex→codex`、`dsh→dsh`、`omp→omp`、`claude_gpt55` 的续跑族为 `codex`（所以 claude↔gpt55 切换走 handoff）。
- 缺字段时 core 用与旧行为相同的纯函数推导，并在下一次写该记录时补齐；不做启动期批量重写用户数据。
- 契约仍在 v1 内：新增可选字段 + 能力位，不 bump 主版本。

## Alternatives considered

- **保留单一 family 字段，当场裁决 gpt55 属于哪族**：字段最少。否掉是因为任一裁决都会破坏一个既有事实（启动命令或历史可读性）。
- **续跑能力从会话族推导**：少一个字段。否掉是因为 gpt55 的续跑结论与会话族不同，推导只是把理由藏成巧合。

## Consequences

- **收益**：每个"为什么这样起/这样续"都是可评审的显式字段。
- **代价与已知上限**：字段越多，"某处读错字段"的风险越高；新增 agent 族时必须同步 protocol、core match、Swift 枚举、设置与统计的全部穷尽点。
