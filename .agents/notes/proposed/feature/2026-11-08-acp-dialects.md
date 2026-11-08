# Agent Note: 通用 ACP 驱动的方言表与权限钉法

Status: proposed

## Problem

路线图 Q-R5 已定：做一个通用 ACP 驱动，每家一份方言，排在 R3 之后（见 [路线复核](../architecture/2026-11-02-roadmap-recheck.md)）。core 的会话事件本来就对齐 ACP 形状（[结构化协议](../../implemented/architecture/2026-10-25-structured-agent-protocols.md)），`adapters/external_protocols.rs` 也有版本和能力协商的部分代码。但各家 ACP 实现在权限模式上差别很大，最常见的坑是：用户自己的配置里开了自动批准（yolo），驱动不钉住就会继承下来，Aeroric 的"每次询问"形同虚设。

New-addition-1102 复核研究读了 papilot 的 `src/server/acp.ts`，它有一张现成的方言表。

## Proposal

- **一个驱动，每家一条方言记录**：启动命令、要钉住的会话参数和环境变量、可接受的模式、工具种类怎么识别、恢复会话用哪个方法。
- **方言表初稿**（来自 papilot `acp.ts:1-60`，Gemini 是新增，需实测）：

| Agent | 启动 | 权限钉法 | 备注 |
|---|---|---|---|
| Grok | `grok agent stdio`（本机 help 里只写"Run the agent over stdio"，是否就是 ACP 待实测） | `sessionMeta` 里 `yoloMode` 和 `autoMode` 都钉成 false；它另有 `--always-approve`，不能带 | 本机已装 |
| Copilot | 各家 ACP 入口 | `allow_all` 关掉，同时设环境变量 `COPILOT_ALLOW_ALL=false`，两处都要钉 | |
| Kimi | 同上 | 用 `default` 模式 | |
| Qwen | 同上 | `auto-edit` 或 `default` | |
| DSH | `dsh acp`（本机 help：`dsh --profile acp`，"Serve automation clients over Agent Client Protocol stdio"） | 没有模式可选；钉住沙箱预设，防止环境里更宽松的 `DSH_PERMISSION_MODE` 生效 | 所有工具都报 `kind=other`，要按工具名认出 shell 和 edit（`acp.ts:5-15`）；本机已装 |
| Gemini | `gemini --acp`（`--experimental-acp` 已弃用） | 待实测：确认不继承用户的自动批准设置 | 本机已装 |

- **通用规则**：
  - 从不继承用户配置里的自动批准；
  - 一家 Agent 给不出 Aeroric 能接受的模式时，直接拒绝连接，不降级；
  - 按能力表选 `session/resume`，其次 `session/load`；没有声明、失败、或返回了不同的会话 id，就明确失败，不另开一个新会话（`acp.ts:132,163`）；
  - 模型目录拉取失败要保留错误，不把空目录当成"连上了但没有模型"。
- **先接哪家**：路线图写的是 Kimi 或 OpenCode，但本机两个都没装；建议先用本机已有、入口明确的 Gemini（`--acp`）验证驱动，Grok 确认 `agent stdio` 是 ACP 后再接，最后补 Kimi。
- DSH 现在走 webui 宿主通道（不是 `SessionDriver`），能力表里 live 特性全是 No；`dsh acp` 也许能让 DSH 拿到 live 会话能力，单独评估。

## Alternatives considered

- **每家写一个专用驱动**：方言差异最好处理。没有采用：Q-R5 已定为通用驱动加方言；差异集中在启动参数和权限上，一张表能装下。
- **继承用户自己的 ACP 配置**：最省事，用户在别处怎么用这里就怎么用。不采用：会把用户在别处开的 yolo 带进 Aeroric，"每次询问"失效。

## Acceptance criteria

- 驱动在 Gemini 或 Grok 上跑通一轮对话、一次审批（允许和拒绝各一次）、一次恢复会话。
- 每家方言都有一个用例，证明用户配置里开了自动批准时，Aeroric 的会话仍会先问。
- 给不出可接受模式的 Agent 被拒绝连接，界面说清原因。

## Risks

- 各家 ACP 实现还在快速变化，方言表要跟着版本走；需要像 Codex 那样留契约快照。
- Gemini 的 ACP 入口还是实验性的。
- DSH 的工具种类全是 `other`，靠工具名识别，改名就会漏判。
