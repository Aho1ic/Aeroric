# Agent Note: 经验与效果回流（B5-3a）：学会的经验、置信度、检索与注入计数、有用/不对

Status: implemented

## Problem

B5-2 之后，Aeroric 能从会话里整理出线索和偏好候选，但"这次是怎么解决的"留不下来，下次遇到同样的问题还得重新摸索。另一方面，记忆存了不少，却不知道哪条真有用：被检索过多少次、带进会话后帮上忙没有。路线文档 §6.1 有经验层，§6.6 要求效果回流。owner 2026-10-31 定了 Q-M16–Q-M21，全部按推荐：B5-3 拆成 3a（本批）和 3b（提案收件箱），3b 另行设计。

## Decision

**经验**（Q-M17、Q-M18、Q-M21）

- B5-2 那次整理调用的 JSON 增加 `experiences` 字段，最多 2 条，提示词要求只在会话确实解决了问题时写。所以经验不额外花钱。
- 每条经验有：
  - 标题，存在条目正文里；
  - `ExperienceDetail`：问题、解法、检索关键词（≤8）、适用环境（≤6）。
- 校验和写入：
  - 标题、解法都不能为空，否则丢弃；
  - 经验不要求原话。它写的是"怎么做"，依据是整个会话，证据记为会话文字的 hash；
  - 写入后直接生效，kind = experience，作用域是本项目；
  - 去重、遗忘屏障、密钥扫描都照旧，解法也要过密钥扫描。
- 只在 `memory_recall` 里出现：`profile::render` 只取 profile 和 fact，`kept()` 不把经验交给分析模型。
- 全文索引的正文是"标题 + 问题 + 解法 + 关键词 + 环境"，所以按关键词或解法也能检索到。

**置信度**（Q-M19）

- 分低、中、高三档：
  - 整理时，会话检查点里的验收条款都通过（`Session.verified`），起步是"中"，否则是"低"；
  - 标"有用"：1 次至少到中，累计 3 次到高；
  - 标"不对"：降一档；已经是低档的，再标就直接归档。
- 检索排序，在 B5-1 的键（本项目、置顶、用户说的）之后，依次比较：
  - "不对"多于"有用"的排后；
  - 置信度高的在前；
  - "有用"次数多的在前；
  - 最后才是 bm25。

**效果回流**（Q-M20，所有条目都适用）

- `items` 表新增四个计数：`retrieved`、`injected`、`helpful`、`wrong`。它们和置信度一样原地更新；记忆文本仍然只追加，不改写。
- 计数怎么来：
  - `recall` 返回之前，命中的条目 `retrieved` 加 1；
  - executor 改用 `profile_for_session`：`render` 返回实际写进画像的条目 id，这些条目 `injected` 加 1。
- 投票入口：
  - App 调用 `memory.feedback {id, helpful}`；
  - Agent 用 MCP 工具 `memory_feedback`，对应宿主的 `context.memoryFeedback`，只能投本项目或全局的条目；
  - 不是生效状态的条目报 WrongState。
- `memory_recall` 的结果行会写出检索和有用次数，经验另外写置信度、解法和适用环境，末尾提示可以用 `memory_feedback` 反馈。

**存储迁移**

- schema 升到 2：只给 `items` 追加列（detail、confidence 和四个计数），`PRAGMA table_info` 发现没有的列才加。
- 旧库打开时自动升级。有测试：用 schema 1 的库打开，旧条目的计数都是 0。

**界面**

- "学会的经验"标签开放，不提供添加行（`MemoryTab.modelWritten`）。每行是：
  - 灯泡图标和标题；
  - 置信度徽标，低灰、中蓝、高绿；
  - "检索 N · 有用 M"；
  - 两个"有用 / 不对"小胶囊按钮。
- 所有生效条目的 ⋯ 菜单都加"有用""不对"；悬停提示显示问题、解法、适用环境、关键词和统计。
- 整理记录里每行多一个"经验 N 条"（`MemoryRun.experiences`）。

## Alternatives considered

- **经验也进"待确认"**：经验是做法，不是关于用户的事实，逐条确认太烦（Q-M18）。改为可以归档、可以投票降级。
- **经验注入新会话**：上下文开销大，而且不一定相关。只在检索时出现，由 Agent 按需取。
- **给计数单独建一张事件表**（每次检索一行）：现在只需要累计次数。等做梦或排序要看时间分布时再加。
- **由模型判断经验是否全局适用**：Q-M21 不让模型决定全局。

## Consequences

- B5-3b（经验升格为技能、提案收件箱、A06）还没做，占位任务是 `10-31-memory-proposals`。
- 计数是近似值：同一次检索返回的条目都会加 1，不管 Agent 有没有真的用到。
- 测试：
  - core：
    - 经验的检索、计数和投票，覆盖排序、注入计数、升降档、归档和密钥；
    - schema 1 迁移；
    - 解析经验，覆盖上限、空标题和空解法；
    - 整理流程写入经验，验收全过时起步为中；
  - 宿主端到端 1 个：检索计数、Agent 和用户投票、跨项目投票被拒；
  - CLI：工具列表；
  - Swift：经验解码、文字、统计、标签和投票。
- 没有发过真实模型请求。真实界面没有运行过（NOT RUN），等 owner 目检。
- 2026-10-31 目检报告（computer-use `2026-10-31-1303-aeroric-b5-experience-feedback`）之后的修复：
  - **memory_recall 没有审批卡却报"user rejected"**：在"每次询问"档位下，Codex 调用 MCP 工具前会发 `mcpServer/elicitation/request`。驱动不处理这个请求，回了错误，Codex 就把它当作拒绝。
    - 现在 Codex 的 `aeroric_context` 配置加上 `default_tools_approval_mode = "approve"`（已用本机 codex 0.163 在临时 CODEX_HOME 下验证这个取值合法）；
    - Claude 加上 `--allowedTools mcp__aeroric_context`；
    - 理由：这些工具只读写 Aeroric 自己的记忆、黑板和委派，委派出的子会话在它自己里面照常审批。
  - **新建会话报 revisionConflict**（问题来自更早的新建会话代码，和 B5 无关）：
    - 原因：草稿的绑定在没有草稿时返回 placeholder（`submissionID = "unopened"`），`edit` 先建好新草稿，又用整份 placeholder 把它覆盖了，于是真实提交用了 "unopened"。宿主里已经有一条 "unopened" 回执，指纹不同就会一直冲突，"核对原交办"也救不回来；
    - 修法：`NewSessionDraft.keepIdentity` 保证从 placeholder 来的编辑保留草稿本来的身份，并补了回归测试。
  - **整理记录漏写经验条数**：`MemoryRun.experiences` 现在进了界面，显示为"1 线索 · 1 候选 · 1 经验"。
