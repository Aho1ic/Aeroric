# Agent Note: 本次带入的上下文与无新证据重试（R3-2）

Status: implemented

## Problem

[本回合账单](2026-11-09-turn-bill.md)（R3-1）能看出一轮花了多少 token、时间耗在哪个工具上，但还回答不了两件事：

- **输入都是什么**：一次调用几万 token，里面多少是每次都要带的系统提示、工具定义、技能清单、项目说明，多少是历史对话，多少是这一轮新加的。不知道这个，就没法判断该砍哪一块（[路线复核](../../proposed/architecture/2026-11-02-roadmap-recheck.md) §5 把这件事和"无新证据重试检测"一起列进 R3）。
- **Agent 在原地打转**：同一条命令反复失败、报错一样、中间什么也没改，它还在试。会话页已有的 `LiveLoop` 只看回合内同一调用是否连续出现，不管成败和结果；[失控提醒](2026-11-08-runaway-reminder.md)只看累计 token、活跃时长和空转。

## Decision

### 带入的上下文

- 宿主在 `sessions/turn_context.rs` 的 `Ledger` 里逐行记字符数，`transcript_stats` 的 Codex 和 Claude 解析器在读会话文件时喂给它，结果放在每个 `TurnStat` 的 `context`（`ContextPart`：`kind`、`tokens`、`sources`），随 `session.history` 的 `stats.turns` 到 App。
- **分哪几块**：基础部分按 `system`（系统提示）、`tools`（工具定义）、`skills`（技能清单）、`instructions`（项目说明，带 AGENTS.md / CLAUDE.md 路径）、`memory`（记忆）、`environment`（环境与模式）、`other`（其他说明）分；对话部分分 `history`（之前各轮）和 `new`（这一轮到最后一次调用为止）。
  - Codex：`session_meta.base_instructions` 是系统提示（同样内容的第一条 developer 消息不重复计）；`additional_tools` 是工具定义；developer 消息和注入的 user 消息按开头判断（`<skills_instructions>`、`# AGENTS.md instructions for <路径>`、`<permissions instructions>`、`<environment_context>`、`<collaboration_mode>`、`# Aeroric 记忆` 等）。
  - Claude：新版会话文件把系统提示（`prompt_snapshot`）、说明文件（`instructions`，`type` 含 `Mem` 或路径含 `/memory/` 的算记忆）、技能清单、延迟工具和 Agent 清单、环境信息写成 `attachment`；其余 attachment（文件、hook 输出）算对话。
- **token 怎么来**：会话文件只记每次调用的输入总数，不记各部分。各部分按字符数占比，折算到这一轮**最后一次调用**的真实输入 token，所以各项相加等于真实总数；卡里标"≈"并写明是约数。
- **压缩**：Codex 的 `compacted`、Claude 的 `compact_boundary` 之后，历史换成摘要的长度，基础部分清零重计（压缩后 Agent 会重新发送）。
- **没记基础部分的会话不拆**：OMP、DSH 和旧版 Claude 的会话文件不记系统提示这类内容，这时只拆"历史/本轮新增"会把系统提示算进历史，读起来是错的，所以不显示这一段。
- **界面**：账单卡在用量和工具瀑布之间加"带入的上下文"：一条分段的胶囊条，下面每项一行（名称、来源文件名、≈N tok · 占比），悬停看完整路径。Claude 的会话文件从不记工具定义，这部分会摊进其他各项，卡里注明。

### 无新证据重试

- `turn_context.rs` 的 `Retries` 在每次工具调用返回时记一笔：
  - 签名是工具名加完整输入；
  - 同一签名失败、输出和上次失败相同（先把数字串都换成 `#`，去掉耗时、时间戳这类会变的数字，最多比 2000 字），计数加一；输出不同就从 1 重新数；
  - 同一签名成功了就清掉它；成功改了文件（`apply_patch`、`Edit`、`Write` 等）就清掉全部，因为下一次尝试有了新东西。
  - 可以跨回合累计；连续 3 次并且最后一次在最新一轮，就是 `TranscriptStats.retry`。
- **提醒**：失控提醒横幅把它排在第一条，文字是"「cargo test」连续 3 次失败，报错都一样，中间没改过文件"，照旧可以关掉或点"请它先总结"；关掉按"工具 + 命令"记，同一条命令以后再犯不再提醒。
- **需要我处理**：宿主新增 `session.retryAttention {}`，扫描本机、未归档、24 小时内写过的会话文件，只读末尾 4 MB，按文件大小和修改时间缓存结果；App 每 5 秒随其他待处理项一起取，列为"在重试"，点开进会话。
- 不停止 Agent：失控提醒的原则是只提醒、由用户决定（用户 2026-11-09 确认）。

## Alternatives considered

- **用分词器算每部分的 token**：数字会更准，但各家模型分词不同，还要为每家带一个分词器；按字符占比折算到真实总数，总数准、各部分是约数，已经够判断该砍哪块。
- **没记基础部分的会话也拆成"历史/本轮新增"两段**：用户最初认可"没记的就只显示三段"。复现发现 OMP、DSH 一点基础内容都不记，系统提示会全部算进历史，比不显示更误导，所以不拆。
- **自动停掉在重试的 Agent**：路线复核原文写"停下"。没有采用：失控提醒约定只提醒，停不停由用户定。
- **只在会话页打开时判断重试，不进"需要我处理"**：不改宿主，但重试往往发生在用户没看着的会话里，提醒不到等于没做。

## Consequences

- **收益**：
  - 每轮能看出输入里固定开销和历史各占多少，判断压缩、精简技能清单或换短的项目说明值不值；
  - Agent 原地打转时，即使会话没打开也会出现在"需要我处理"里。
- **代价**：
  - 各部分是按字符折算的约数，中文和代码的 token/字符比不同，占比会有偏差；Claude 的工具定义摊进了其他各项。
  - 基础部分按出现次数累加：Agent 在会话中途重新注入的内容会算两次，这和它真的带进了上下文一致，但压缩前后可能不准。
  - 重试判定看输出文字：报错里有每次都变的非数字内容（随机文件名）时会漏报；不同命令报同样的错不算。
  - `session.retryAttention` 每 5 秒扫一遍最近一天写过的会话文件，变了才重新解析末尾 4 MB。
- **验证**：
  - core 单测：Codex 一轮各部分折算后等于真实输入、AGENTS.md 路径、第二轮把上一轮算进历史；没记基础部分时不拆；Claude attachment 分类（含记忆和说明文件路径）；同一失败 3 次（中间有成功的读文件）算重试，改过文件或报错变了不算。
  - Swift 单测（`TurnContextTests`）：解码、占比文字、工具定义缺失的说明、重试提醒排第一和"在重试"一行。
  - 用真实会话核对：Codex 一轮拆成系统提示、工具定义、技能清单、项目说明等 8 项，加起来等于真实输入；Claude 认出了 CLAUDE.md、AGENTS.md 和 MEMORY.md。
  - 目检（2026-11-09，Codex Computer Use，审阅版）：旧会话账单拆出 8 项约 17.4K tok；新建 CPA gpt-6.1-sol 会话连续三次 `cat` 不存在的文件后，三行工具都标为失败，提醒条首条是"「cat definitely-missing-file.txt」连续 3 次失败……"，"需要我处理"出现"在重试"并能点进会话；关掉提醒后切走再切回不再出现，"需要我处理"那一行保留到会话不再重试为止。
  - 未覆盖：Claude 端到端（这台 Mac 上 Claude 登录不可用）、OMP 和 DSH 的工具输出字段。
