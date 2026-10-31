# Aeroric 前沿能力路线（v3 增补，2026-10-30）

> 状态：**§10 Q-F1–Q-F19 已于 2026-10-30 全部按推荐确认**；实施细节待下一轮确认。本文不是实施授权。
>
> 本文在 [`backlog.md`](backlog.md)（v2，20 族）基础上增补，不改写它。backlog 中已确认的决定继续有效；本文只在 §2 列出的地方提出修订，修订待确认后再回写。
>
> 资料范围是四个文件夹，2026-10-30 全部 `git pull --ff-only` 后静态阅读，**没有运行任何来源项目**。
> - `harness/`：27 个仓库。其中新增 6 个：qunica、codex-host、nezha、Markio、AstrBot、oh-my-pi（已读 `origin/main`）。
> - `知识库/`：ragflow、WeKnora。
> - `论文/`：多 Agent 协作，13 篇。
> - `agent-memory/`：graphiti、Hy-Memory（含 MemHub）、mem0、WorldBase，另有 7 篇论文。
> - 网友方案：Obsidian《多Agent本地记忆共享方案》第四版及评论。
>
> 拉取异常：
> - `dbx` 是 Aeroric 钉住的依赖（脱离分支且带补丁），按约定不拉。
> - `oh-my-pi` 本地改了 `packages/catalog/src/provider-models/openai-compat.ts`，挡住了快进（落后 1180 个提交）。我没有动这个改动，只读了已抓取的 `origin/main`。

---

## 0. 怎么读

| 节 | 内容 |
|---|---|
| §1 | 七条前沿判断，后续所有设计的依据 |
| §2 | 对 backlog v2 的修订建议（需确认） |
| §3 | Harness 增强（F1） |
| §4 | 项目知识库（F2） |
| §5 | 多 Agent 协作（F3） |
| §6 | Agent 长期记忆（F4） |
| §7 | 统一数据模型与 owner |
| §8 | 落地批次（衔接 B1–B6） |
| §9 | 明确不做 |
| §10 | 待定问题（供 grill 逐项确认） |
| 附录 A | 来源索引 |

**新 ID 前缀**

| 前缀 | 来源 |
|---|---|
| PAP | 论文 |
| MEM | agent-memory |
| KB | 知识库 |
| QN | qunica |
| CH | codex-host |
| NET | 网友方案 |

沿用 backlog 的前缀：D（deepseek-harness）、P（paperclip）、RUN（runner）、N（opencode）。

**Aeroric 的先天优势**

Aeroric 已经用结构化协议驱动 Claude Code 和 Codex（ADR-043），omp 也有结构化 driver，并且拥有托管 home、托管 `mcp.json`、本机网关、回合改动快照（`turn_changes.rs`）和自动检查（`auto_check.rs`）。这意味着下面多数能力**不需要像 MemHub、codex-host 那样靠 hook、CDP 注入或抓终端**：会话结构、工具调用、diff 都已经在 core 手里。

---

## 1. 七条前沿判断（证据汇总）

### J1 写操作单线程，其他 Agent 贡献"智能"而不贡献"动作"

- Cognition 2026-04（PAP-COG）在生产中有效的只有三种形态：
  1. 无共享上下文的审查回环；
  2. Smart Friend（弱主模型把强模型当工具问）；
  3. manager 拆任务、子 Agent 执行、manager 汇总（map-reduce-and-manage）。
- 无结构的 swarm 被它判定为"干扰"。
- Google 对 260 个配置的对比（PAP-SCALE）：
  - 单 Agent 基线超过约 45% 后，协作收益递减；
  - 工具密集的任务被协作开销拖累；
  - 没有集中验证时，错误放大 17.2×，集中协调时为 4.4×；
  - 可分解任务 +80.8%，顺序规划 −70%。
- MS-Eval（PAP-MSE）：专用子代理（Spec）最好。"先强制规划"会过度委派；对强闭源模型，委派常常更贵也更差。
- 网友评论（NET）给出同一结论：多 Agent 只在三种情况有效——数据隔离的并行、上下文快爆窗时被迫隔离、多视角查 bug。

### J2 失败多数发生在交接边界，而不是单个 Agent 内部

- MAST（PAP-MAST）归纳了 14 种失效模式：
  - 违反规格；
  - 丢失历史；
  - 不请求澄清；
  - 信息隐瞒；
  - 过早终止；
  - 没有验证，或验证出错。
- 交接幻觉分三型（PAP-HH）。其中第 III 型是压缩时剥掉了"认知标记"（已验证 / 推断 / 未知）。
- 边界元数据塌缩（PAP-FWR）：摘要保住了操作事实，却丢了"仅供谁用"之类的约束。显式的受众白名单几乎能消除泄露。
- CFRC（PAP-CFRC）：交接不能重来，也不能直接宣告完成。要冻结一份"残差契约"：
  - 已确认的选择；
  - 有收据的副作用；
  - 未完成的义务。
  每条义务都要用实时证据逐条核销。

### J3 上下文要按角色选模式：续做用 fork，评审用 isolated

LangChain（PAP-LC）把子代理上下文分成两种：

| 模式 | 适用 | 原因 |
|---|---|---|
| fork（继承主管完整对话，吃 prompt cache） | 已诊断、等着续做的 worker | 不用重新取证 |
| isolated（只给任务） | 审查 / 验证、并行调研 | 不被主管的诊断锚定 |

Cognition 的审查回环同样要求 reviewer 不共享上下文。网友补充：fork 会让缓存树分叉，"按模块长期复用同一个专属子代理"比每次新建更省。

### J4 委派是"契约"，不只是"拆任务"

- DeepMind（PAP-DEL）提出 contract-first decomposition：子任务结果能被精确验证才委派，否则继续拆。还要求：
  - 权限衰减：再委派只能给严格子集；
  - 委派链可审计；
  - 越权时暂停，请求人重新授权。
- AOrchestra（PAP-AO）：任何子代理 = ⟨Instruction, Context, Tools, Model⟩ 四元组，由编排器每步具体化。
- 欠规格（PAP-UND）：没写出来的需求换模型就回归，概率是 2 倍。
- Grounding Gap（PAP-GG）：模型倾向于假设双方已有共同理解，很少主动澄清。

### J5 记忆的权威是"有证据的事实"，向量库只负责找候选

- WorldBase 的设计稿（MEM-WB）是这批资料里最完整的记忆治理方案：
  - 事实、证据、画像分开；
  - 画像是事实编译出来的投影；
  - 模型不能自称"已确认"；
  - 后台"做梦"只出提案，经确定性校验后才应用；
  - 遗忘屏障防止记忆复活；
  - 四种操作分开：压缩上下文、压缩画像、归档、永久遗忘。
- Graphiti（MEM-GR）：事实带有效期窗口，被取代的事实标失效但保留；所有派生内容都能追到 episode 原文。
- LongMemEval（MEM-LME）指出最难的五种能力：跨会话推理、时间推理、知识更新、拒答，以及信息抽取。
- mem0 2026-04 新算法（MEM-M0）：
  - 改为单次 **ADD-only** 抽取，不再覆盖旧记忆；
  - agent 自己确认过的动作与用户事实同权；
  - 语义、BM25、实体三路召回融合。
  - 说明"累积 + 检索时按时间判断"比"写入时就改写"更稳。

### J6 编码 Agent 的记忆应该是"事实 + 经验 + 代码版本链"

Hy-Memory 的 coding 场景（MEM-HY）：

- 每个会话恰好 1 条事实：任务、做法、结果、文件坐标。
- 0 到 N 条可复用经验，要过价值门槛，记录 `search_keys`、适用环境 `boundary_envs` 和置信度。
- 另有一条零 LLM 的 diff 版本链，粒度是分支 × 文件 × 函数块，**连后来被回退的版本也保留**。它像装饰器一样挂在检索到的事实下面，本身不参与检索。

MemHub 再把 Claude Code、Codex、Cursor 等工具的会话汇成一份本地记忆，再授权回各工具召回。它把四种权限分开管：来源读取、同步开关、模型外发、宿主召回。

### J7 知识库从"检索切片"走向"编译知识产物"

- ragflow 1.0（KB-RF）的知识编译，把文档编译成以下产物，用于检索、问答和 Agent：
  - Graph；
  - Tree；
  - PageIndex（保留原章节结构）；
  - Mindmap；
  - Timeline；
  - Wiki；
  - Skills。
- WeKnora（KB-WK）的 Wiki 模式由 Agent 自治生成互链的实体 / 概念页，流程是：
  1. 抽取候选；
  2. 把分块的引用归属到候选；
  3. 严格去重；
  4. 增量改页；删除文档时移除它独有的贡献；
  5. 规划目录；
  6. 用 issue 工单管理问题。
- 这与 Karpathy 的 "LLM Wiki" 思路一致：知识被预先编译成可读、可链接、可溯源的页面，而不是每次临时检索。

---

## 2. 对 backlog v2 的修订建议（需确认后回写）

| # | backlog 原文 | 新证据 | 建议 |
|---|---|---|---|
| R-1 | §4.2-1：团队 = 角色模板 + **恰好一个 lead** | runner 于 2026-10-27 **删掉了 mission lead**（RUN#873）。删除原因：可选 lead 会造成两种提示形态和第三种状态；协调改由"委派工作的那条对话"在外部完成，成员通过 `mission show` 自查名单，只做自己那部分。Cognition、qunica 采用的也是"manager 是委派方会话本身"。 | 用户正在对话的主会话就是协调者（manager），不再单设 lead 槽位。团队成员共享目标和约定。见 §5.2 |
| R-2 | A08：新记忆以候选出现，无引用就降级 | mem0 改为 ADD-only；WorldBase 主张证据独立建表、画像是投影 | 记忆改为"事实 + 证据 + 画像"三件套；写入只追加，"取代"由新事实标记，不改写旧条目。见 §6 |
| R-3 | A07：检索 MCP 只做历史会话检索 | 资料普遍把检索、记忆、知识三类合并成同一个本地服务（MemHub Host Bridge、WeKnora MCP Server） | 合并成一个 **Aeroric Context MCP**：`search_sessions`、`recall_memory`、`read_wiki`、`board_*`、`delegate_*`，按任务限定作用域。见 §3.5 |
| R-4 | A06：信号来源有反馈、检查、卡死等 | MAST 的 14 类失效模式可以由 LLM-as-judge 标注 | 把 MAST 分类作为信号的标准标签，同时用作治理者的审查清单 |
| R-5 | §4.2-4：委派时每个子任务是一个普通任务 | contract-first、权限衰减、四元组 | 委派必须带"验收条款 + 权限子集 + 上下文模式"。见 §5.3 |
| R-6 | A04：待回应卡 | WeKnora 两段式挂起（交互发生在外部调用之前，才能落库挂起） | 待回应卡记录 `suspendable`：只有在外部调用之前发生的交互才能跨重启等待 |

---

## 3. F1 Harness 增强

### 3.1 交接契约（HandoffContract）——★ 首推

**问题**：以下场景都有"接力"：

- 换模型；
- 换 Harness；
- 上下文压缩后续接；
- 把任务交给子代理；
- 用户第二天恢复会话。

摘要会丢掉已确认的决定、约束和"结果未知的操作"（J2）。

**原生功能**：交接时，core 生成一份结构化的**交接契约**，作为新会话的首条注入，并在界面上显示为交接卡。契约字段如下：

| 字段 | 内容 | 来源 |
|---|---|---|
| 目标与范围 | 原始请求，以及用户确认过的需求 | 会话首条消息、确认卡 |
| 已确认决定 | 用户点过"确认"的选择（如"用方案 B"） | 待回应卡的回答记录 |
| 已发生副作用 | 提交、推送、写入的文件、执行过的命令，**附证据引用** | `turn_changes`、工具调用结构化记录 |
| 结果未知的操作 | 已发出但没有收到结果的命令或请求 | 工具调用没有 result 的条目 |
| 未完成义务 | 剩余步骤，每条带验收方式 | 计划卡 / 待办 |
| 约束与受众 | "不要推送""仅本项目"等边界，作为**结构化字段**而不是摘要文本 | 用户消息中的显式约束、任务权限档 |
| 认知状态 | 每条标注 已验证 / 推断 / 未知 | 由来源类型决定，不由模型自报 |

**规则**：

- 继任者不得重做已有收据的副作用。
- 宣告完成时，必须逐条核销义务，核销靠检查结果、diff 或命令输出（CFRC 的三原则）。
- 契约由 core 根据结构化事件生成，模型只负责补"未完成义务"的描述，不能改动已确认字段（J5 的"模型不能自称已确认"）。

**归属与界面**：

- owner：core `sessions`，新增 `handoff`。
- Mac：交接卡在会话顶部可展开，每项可点开对应证据。
- iOS：只读。

**来源**：PAP-CFRC、PAP-HH、PAP-FWR、NET（检查点字段）、MEM-WB（证据独立）。

### 3.2 会话检查点（原生化网友方案）

网友方案的核心是让 agent 自己维护 `.agent-memory/active/<task>.md`，内容是可恢复的快照，而不是流水账。Aeroric 不需要 agent 自己写：

- 检查点由 core 在以下时刻自动生成：
  - 每次回合结束；
  - 每次有副作用的工具调用之前，也就是"拟执行操作 + 核查方式"。
- 检查点的结构与 §3.1 的交接契约相同。交接契约就是"最近一次检查点 + 未完成义务"。
- 恢复会话或压缩后，先注入检查点，并把"结果未知的操作"列成待核对清单，再继续。
- 结案时自动从 active 移入 done，再按 §6 的规则沉淀经验。
- 对用户自己仓库里已有的 `.agent-memory/` 只读、不写，避免和用户手写的规则冲突。也可以提供"导出为 .agent-memory 格式"，供不经过 Aeroric 的 agent 使用。

**来源**：NET 第四版的组织、检查点、并发、Git 排除四节，以及 PAP-CFRC。

### 3.3 需求澄清与验收条款（开工前）

- 新建任务时，用户可开启"先澄清"（或由配方默认开启）。模型的第一轮只做两件事：
  1. 列出它推断的隐含需求；
  2. 把需求写成可检查的验收条款。
- 结果出成一张卡，由用户勾选或修改，**不直接开工**。
- 勾选后的条款进入交接契约的"未完成义务"，A05 自动检查和独立验收逐条对照。
- 已有的 grilling 技能可以作为配方的实现方式。

**来源**：PAP-UND、PAP-GG、PAP-DEL（contract-first）。

### 3.4 子会话折叠与侧支

- **侧支（/btw，或从任意消息分叉）**：起一个子会话处理局部问题。结束后回到主线，主线只收到"结果卡"（结论 + 证据链接），过程被折叠。这就是 Context-Folding 的 branch / return，不需要训练模型。
- **分叉位置**：可选当前工作区，或新 worktree，与 codex-host、WeKnora 相同。
- 主会话里可以展开看子会话全文，也可以打开子会话直接对话。

**来源**：PAP-FOLD、CH、D（trajectory subcall folding）、KB-WK（分叉 / 回滚）。

### 3.5 Aeroric Context MCP（统一上下文服务）

Aeroric 往托管 `mcp.json` 写入一个本地 MCP 服务，四个 Harness 族都能调用：

| 工具组 | 工具 | 说明 |
|---|---|---|
| 历史 | `search_sessions`、`read_session` | 跨 Harness 的会话全文检索（A07） |
| 记忆 | `recall_memory`、`propose_memory` | 只能**提议**，不能直接写入（§6） |
| 知识 | `read_wiki`、`search_wiki`、`flag_wiki_issue` | 项目 Wiki（§4） |
| 协作 | `board_read`、`board_update`、`delegate`、`delegate_status`、`ask_user` | 团队协作（§5） |
| 交接 | `read_handoff` | 读取当前交接契约 |

**权限**：

- 每个工具按"任务 × 角色"限定作用域，令牌只在本次运行期间有效（参考 qunica 的 notes MCP）。
- 四类授权分开，参考 MemHub：
  - 读哪些来源；
  - 是否同步；
  - 内容能否发给模型；
  - 哪个 Harness 能召回。

### 3.6 回合下的可选动作卡

- agent 可以调用 `propose_action` 提出一个可选动作，例如：
  - "把这次经验存成技能"；
  - "为这条发现开工作包"；
  - "让审查员复核"。
- 卡片在回合结束后显示，**用户点了才执行**。队列里有待发消息时不出卡。

**来源**：D（action-cards）、P（Coordinator 的任务提案）。

### 3.7 回合性能与成本透视

- 每一步显示：
  - 耗时；
  - 上下文增量；
  - 缓存命中率；
  - 重试与审批；
  - 是哪个事件触发了这一轮。
- 按会话汇总。为 A13 用量提供逐步粒度。

**来源**：D（client-ui-stats）、CH（缓存命中率 / 成本 / 上下文实时）。

### 3.8 定时任务的前置检查

定时 prompt 先跑一条 shell 检查（例如"有新提交吗"），检查不通过就不唤醒模型，省钱。这一条并入 A12。

**来源**：N（schedule gate）。

---

## 4. F2 项目知识库（Project Wiki）

### 4.1 定位

**不做通用企业知识库**——ragflow、WeKnora 已经很重，而且依赖 Docker、向量库和图数据库。Aeroric 做的是**面向编码项目的"编译型知识"**：

- 来源只有三类：
  1. 项目自身（代码、README、docs、ADR 笔记）；
  2. 所有 Harness 在这个项目上的会话（事实与经验，见 §6）；
  3. 用户手动加入的资料（PDF、网页、论文）。
- 产物是项目 Wiki：互相链接的 Markdown 页，存在托管目录，**用户可读、可改、可导出**。
- 所有 Harness 经 Context MCP 读取，用户在 App 里浏览。

### 4.2 页类型（编译模板）

| 页型 | 内容 | 对应来源 |
|---|---|---|
| 模块页 | 每个模块 / 子系统的职责、入口、关键类型、依赖、常见坑 | ragflow PageIndex / Tree |
| 概念页 | 领域概念、术语、不变量 | WeKnora concept |
| 决策页 | 从 `.agents/notes/`、ADR、会话中确认的决定编译，带状态 proposed / implemented / rejected / archived | qunica 群组笔记（四段模板正是 write-notes-like-deepseek） |
| 时间线 | 项目里程碑、重大改动、回退 | ragflow Timeline，以及 Hy-Memory 的 diff 链 |
| 操作手册 | 构建、测试、发布、排障步骤，从成功会话的经验沉淀 | Hy-Memory Experiences |
| 索引页 | 目录（≤3 层）与简介，自动维护 | WeKnora index |

### 4.3 编译流水线（增量）

```
来源变更（git 提交 / 新会话结案 / 新资料）
  → 解析与切块（代码按符号，文档按章节，会话按事实/经验）
  → 候选实体与概念（带 chunk 引用归属）
  → 严格去重（同一实体判定，不确定则新建并标"可能重复"）
  → 页面增量修改：新增信息合并；来源被删时移除其独有贡献
  → 确定性校验：每句话都有 source_ref；链接目标存在；目录深度
  → 写入新 revision（可比较、可回滚）
  → 有冲突/过时 → 开 wiki issue（不静默改写）
```

**规则**：

- **用户手改过的段落受保护**：编译器只能对它提出修改建议，不能覆盖（与 backlog 的写入基线不变量一致）。
- 页面过时的判定：引用的代码坐标（文件 / 符号的 hash）变了，就标"可能过时"，并开 issue。
- 编译由空闲调度触发（与 §6.5 共用调度器），有 token 与费用上限，模型可以单独配置（WorldBase 主张对话、分析、embedding 三种模型分开）。

### 4.4 检索

- **第一版不引入向量数据库**：SQLite FTS5 + 链接图 + 符号索引就够用。项目 Wiki 规模小，页面本身已是编译过的摘要。
- 可选的 embedding 放到后续批次，作为派生索引；换模型时整批重建（WorldBase 的 `embedding_generations`）。
- Agentic 检索：先读索引页，再按链接读页，不足时回读原文或代码。不做 GraphRAG 社区摘要。

### 4.5 界面

- **Mac**：项目侧栏新增 "Wiki" 入口，包括：
  - 页面树；
  - 页面（来源脚注可点开代码或会话）；
  - 关系图；
  - 修订历史；
  - issue 列表；
  - "重新编译此页"。
- **iOS**：只读浏览。

**来源**：KB-RF、KB-WK、QN（群组笔记）、MEM-WB、MEM-HY。

---

## 5. F3 多 Agent 协作

### 5.1 原则（取代 backlog §4.2 的前提）

1. **写单线程**：同一工作区同一时刻只有一个写者。需要并行写时，每个写者用独立 worktree，合并由用户或主会话完成（J1、qunica、Cognition）。
2. **默认单 Agent + 技能**：只在以下情况推荐团队：
   - 可并行、可分解的检索或排查；
   - 多视角审查；
   - 上下文快爆窗时的隔离。
   新建会话时给出"建议用团队吗？"的提示，并说明理由（J1，PAP-SCALE 的架构匹配）。
3. **必须集中验证**：团队产物由主会话或独立 verifier 汇总核对，不允许成员互相宣告完成（PAP-SCALE 的错误放大）。
4. **交接即契约**：所有委派与回传都走 §3.1 的结构（J2）。

### 5.2 三种协作形态（按落地顺序）

**形态 A：审查回环**（最先做，收益最确定）

- coder 完成回合后，自动起一个 **isolated** 的 reviewer：
  - 可以是另一个 Harness，或另一个模型；
  - 只给 diff 和验收条款，不给对话。
- reviewer 的意见回到 coder，coder 用全局上下文**过滤**：
  - 采纳、驳回（写理由）或上报用户；
  - 防止循环、越界、违背用户意愿。
- 有轮数上限（沿用 P17 的"退修超过 3 轮升级给用户"）。
- 跨 Harness 天然合适，例如 Codex 写、Claude 审。这是 Aeroric 独有的组合能力。

**形态 B：Smart Friend（求助强模型）**

- 主会话用便宜模型，遇到难点时调用 `ask_expert`：
  - 强模型拿到**主会话上下文的 fork**；
  - 回答宽泛问题；
  - 如果缺信息，应该指示"先去读某文件再来问"，而不是编造。
- 给出强制调用点作为兜底：合并冲突、连续失败 2 次、改动安全相关文件。
- 本质是能力路由：可以按子任务路由到更擅长的模型（如调试、视觉、写测试），与网关的模型路由联动。

**形态 C：Manager 委派（map-reduce-and-manage）**

- **manager 就是用户正在对话的主会话**（R-1），不再设 lead 槽位。
- manager 用 `delegate` 起子任务。每个子任务是一个独立的原生会话，出现在侧栏，可以随时打开接手，就像 codex-host 的 `#agent`。
- 输入框里键入 `#` 选择 Harness 或角色，就是显式委派，例如 "#claude-code 独立审查这个改动"。
- 子任务结束时只回传结果卡（§3.4 的折叠）。manager 汇总后再交给用户。
- 深度为 1：子任务不能再建团队（K15）。

> 不做：无结构的 mesh 群聊、成员之间自由协商（J1）。qunica 的 mesh、ring 拓扑留作 P2 实验，只开放给"讨论 / 头脑风暴"类配方，且**禁止写**。

### 5.3 委派契约（Delegation）

`delegate` 的参数就是一份契约，对应 AOrchestra 四元组加 DeepMind 契约：

| 字段 | 说明 |
|---|---|
| instruction | 任务描述 |
| context_mode | `fork`（续做）/ `isolated`（审查、调研）/ `brief`（只给交接契约） |
| context_refs | 额外附带的文件、Wiki 页、记忆、会话片段 |
| harness + model | 四族之一 + 模型配置，或"按角色默认" |
| tools / permission | **必须是父任务权限的子集**（权限衰减），由 core 校验 |
| workspace | 共享只读 / 独立 worktree |
| acceptance | 可执行的验收条款（命令、测试、检查点）。缺失时提示"无法验证，建议继续拆分" |
| budget | token、时长、轮数上限 |

**规则**：

- 委派链（谁、何时、给谁、什么权限）写入不可改的审计记录。
- 子任务需要越权时：暂停，并向用户发待回应卡。
- **信誉统计**：按"Harness × 模型 × 角色"记录完成率、返工率、验收通过率。新建委派时显示这些统计，帮助用户选择；不自动选择。

### 5.4 共享黑板（Board）

团队与工作包共享一块结构化黑板，取代"笔记流"：

- 字段：
  - 目标；
  - 假设（附待验证标记）；
  - 认领（工作包 ↔ 成员）；
  - 决策；
  - 证据；
  - 待解问题。
- 字段级写入并留审计；注入时只注入"上次读取以来的 diff"。
- 决策状态区分 proposed 和 accepted："一个成员的建议不等于团队决定"（qunica）；accepted 需要 manager 或用户确认。
- 团队结束后，黑板上的决策和经验进入 §6 的沉淀流程。

**来源**：WorldBase 的 SharedBoard PRD、qunica 群组笔记、paperclip 的 Coordinator 决策卡。

### 5.5 失效探针（MAST 化）

core 对团队运行做轻量探针。命中时生成"需要我处理"项，或触发 A06 信号，**不自动终止**：

| MAST 模式 | 探针 |
|---|---|
| 步骤重复 | 精确重复的工具调用 / ABAB 振荡（A13 已有失控检测） |
| 不知停止条件 | 超过预算 80% 仍无验收进展 |
| 过早终止 | 宣告完成但验收条款未核销 |
| 忽略他人 / 信息隐瞒 | 黑板上的待解问题被认领后长期无更新 |
| 无验证 | 子任务回传没有证据引用 |
| 推理-行动不一致 | （P2）由 judge 抽样标注 |

### 5.6 Needs You 队列

Mac 与 iOS 共用（即 backlog A19）。新增两类条目：

- 任务提案（来自 action card）；
- 决策卡（来自黑板上待确认的决定）。

**来源**：P（Coordinator workspace）。

---

## 6. F4 Agent 长期记忆

### 6.1 分层（Aeroric 记忆模型）

| 层 | 内容 | 作用域 | 注入方式 | 生命周期 |
|---|---|---|---|---|
| 工作上下文 | 当前回合 | 会话 | 原生 | 回合结束 |
| 检查点 / 交接 | §3.1 / §3.2 | 任务 | 恢复时注入 | 任务结案后归档 |
| 用户画像 | 语言、交互习惯、明确偏好、限制 | 全局 / 项目覆盖 | 300–600 token 常驻 | 明确修改或遗忘前保留 |
| 项目事实与决策 | 约束、已采用方案、环境版本 | 项目 / 分支 | 按任务召回 | 随版本过期，需复查 |
| 会话事实 | 每个结案会话 1 条：任务 / 做法 / 结果 / 文件坐标 | 项目 | 检索 | 长期，可归档 |
| 经验 | 可复用的解法，带适用环境、置信度、search_keys | 项目 / 全局 | 检索，或升格为技能 | 验证、修订、过时 |
| 代码版本链 | 分支 × 文件 × 函数块的每版真实 diff | 项目 | 挂在事实下（装饰器） | 跟随仓库 |
| 证据 | 原始消息、产物、命令输出的稳定引用 + hash | — | 按需回读 | 依保留策略 |

**来源**：MEM-WB（分层与画像）、MEM-HY（事实、经验、diff 链）、NET（检查点）。

### 6.2 写入：事实只追加，取代用标记，模型只能提议

写入链路：

```
结构化会话事件（core 已有，四族统一）
  → 候选抽取（会话结案 / 用户说"记住" / 验收通过）
      · 每条候选带 evidence 引用（消息 ID + hash + 片段）
      · assertion_type：user_explicit / tool_verified / agent_confirmed / model_inferred
        —— 由 core 根据来源设置，模型不能自报
  → 合并判定：新增 / 取代（旧条目 superseded，保留）/ 重复（只加证据关联）/ 冲突（pending）
  → 风险分流：低风险自动生效；画像变更、推断与明确事实冲突 → 待确认
  → 写入（短事务）→ 异步更新画像投影与索引
```

**规则**：

- 用户明确说"记住 / 改成 / 忘掉"：前台立即生效，在下一次模型调用前就体现。
- 模型推断出的偏好只能进入 pending，不能进入画像。
- 多值偏好不判为冲突（"喜欢 A"并不否定"喜欢 B"）。
- 凭据和密钥在写入前做扫描；命中就拒绝写入（A17）。

### 6.3 检索：上下文构建器

按顺序：

1. 校验权限：哪个 Harness、哪个项目、哪个受众。
2. 注入核心画像和项目规则。
3. 属性类问题直接读画像类别，不走向量。
4. 项目事实、会话事实、经验三类做 FTS + 实体匹配召回，按以下维度重排：
   - 时间（当前状态 / 过去 / 计划）；
   - 适用环境；
   - 置信度；
   - 是否已被取代。
5. 命中的事实下挂代码版本链。
6. 如果仍然不够，回读证据原文。
7. 输出一个带 ID、来源、有效期的证据包；**找不到就明说找不到**（LongMemEval 的拒答能力）。

### 6.4 跨 Harness 共享

- Aeroric 是唯一写者。Claude Code、Codex、omp、DSH 的会话都由 core 统一沉淀，各族通过 Context MCP 召回。这正是 MemHub 想做却只能靠 hook 做到的事。
- 来源标签：每条记忆带来源 Harness 和模型。召回时可以按来源过滤，比如"只用 Claude 会话的经验"。
- 原生记忆文件：
  - Claude 的 `MEMORY.md`、Codex memories 等原生记忆文件，Aeroric 只读并展示（A08）；
  - 可选择"导入为候选"；
  - 不双向同步，避免两个权威。

### 6.5 后台整理（做梦）

- **触发**：空闲约 10 分钟且积累了约 20 条新证据，或者会话结案。没有新证据就不调用模型。正在对话时、预算不足时，或用户关闭了后台学习时，延后。
- **流程**：
  1. 取租约和快照；
  2. 收集增量；
  3. 聚类，判断冲突；
  4. 产出变更计划，附证据和理由；
  5. **确定性校验**：ID、revision、作用域、证据、保护级别；
  6. 低风险的自动应用，高风险的进入收件箱；
  7. 写报告。
- 快照之后被用户改过的目标，这次计划作废，重新评估。
- **权限矩阵**（采纳 WorldBase）：
  - 不因"长期未使用"删除用户偏好；
  - 置顶或用户确认过的约束，不能被自动改义；
  - 经验升格为技能，走 A06 的提案、评测、版本流程。
- 与 A06 的关系：做梦整理"记忆和经验"，A06 进化"技能和指令"，两者共用信号、提案收件箱和调度器。

### 6.6 状态与遗忘

- **状态机**：`candidate → pending → active → superseded / archived / expired → deleted`
- **四种操作分开**，界面上是四个不同入口，而不是一个"清理"按钮：
  1. 压缩上下文；
  2. 压缩画像；
  3. 归档；
  4. 永久遗忘。
- **遗忘屏障**：用户遗忘一条记忆后，保存一个不含原文的命题签名（HMAC）和来源谱系。以下操作都要检查屏障，防止记忆复活：
  - 重新抽取；
  - 重建索引；
  - 备份恢复。
  另外，撤销一次做梦，不能撤销遗忘。
- **效果回流**：`retrieved → injected → used → outcome`，其中 outcome 为 confirmed / corrected / verified / unknown。用于排序，以及"这条记忆真的有用吗"的统计。

### 6.7 界面

**Mac 的"记忆"页**（设置 → 记忆，或项目侧栏），五个入口：

1. 关于你：画像；
2. 项目上下文：事实与决策；
3. 历史线索：会话事实与证据；
4. 学会的经验：经验与技能候选；
5. 整理记录：每次做梦改了什么、待确认什么、花了多少钱、能撤销什么。

**iOS**：审核待确认项。

---

## 7. 统一数据模型与 owner（草案）

| 新 owner（core） | 主要实体 | 依赖 |
|---|---|---|
| `handoff` | Checkpoint、HandoffContract、Obligation、EffectReceipt | sessions、turn_changes、auto_check |
| `knowledge` | WikiPage、PageRevision、SourceRef、WikiIssue、CompileRun | files、git、sessions、memory |
| `team`（扩展 `work_graph`） | Delegation、Board、BoardEntry、DelegationAudit、Reputation | tasks、sessions、handoff |
| `memory` | Fact、Evidence、EvidenceLink、Experience、CodeChain、ProfileSnapshot、ChangeSet、UsageEvent、MaintenanceRun、Tombstone | sessions、settings |
| `context_mcp`（host） | 工具注册、作用域令牌、四类授权 | 以上全部 |

**约束**（沿用架构不变量）：

- 业务真相在 core；Swift 只读 read model、只发 typed 动作。
- 每个写入都覆盖 core 上一次发布的字节（REVISION_CONFLICT）。
- 存储使用 SQLite（FTS5），不引入向量服务器或图数据库；embedding 只作派生索引（P2）。
- 托管目录下的 Wiki Markdown 是导出投影，不是第二个权威。用户在 App 里的修改经 core 写回数据库。

---

## 8. 落地批次（建议，衔接 backlog §5）

现状：B1 已部分完成（turn_changes、auto_check 已接通），B2 的内置技能已提交（04f38b2c）。

| 批次 | 内容 | 依据 |
|---|---|---|
| **B4′ 交接与上下文**（插在 B4 之前；2026-10-30 已实现并提交 61f5d669，偏差见笔记 `2026-10-30-handoff-context`） | §3.1 交接契约 + §3.2 自动检查点；§3.3 澄清与验收条款；§3.5 Context MCP 的历史与交接部分；§3.4 侧支折叠 | 地基：后面的协作、记忆都依赖"证据 + 契约" |
| **B4 协作**（按 R-1 修订；Q-B1 拆为 B4-1 审查回环 → B4-2 委派与契约 → B4-3 黑板、探针与 Needs You；B4-1 于 2026-10-31 目检通过并提交，见笔记 `2026-10-30-review-loop`；B4-2、B4-3 于 2026-10-31 目检通过并提交，见笔记 `2026-10-31-delegation`、`2026-10-31-board-probes`） | §5.2 形态 A 审查回环 → 形态 C manager 委派与 `#` 委派；§5.3 委派契约；§5.4 黑板；§5.5 探针；Needs You 新条目 | 先做收益最确定的审查回环 |
| **B5 记忆与进化**（Q-M1 拆为 B5-1 地基 → B5-2 结案抽取与收件箱 → B5-3 经验与提案收件箱；B5-1、B5-2、B5-3a 于 2026-10-31 目检通过并提交，见笔记 `2026-10-31-memory-store`、`2026-10-31-memory-extract`、`2026-10-31-memory-experience`；B5-3b 提案收件箱待讨论） | §6.1–6.3 事实、经验、证据、画像与上下文构建器；§6.4 跨族召回；§6.6 状态与遗忘；B5 原有的 A06 提案收件箱合流 | — |
| **B5.5 项目 Wiki** | §4 编译流水线（模块、决策、操作手册三类页先行）、Wiki 浏览器、issue | 依赖 B5 的事实与经验 |
| **B6 及以后** | §5.2 形态 B Smart Friend（与网关路由联动）；§6.5 后台做梦自动应用；代码版本链；可选 embedding；mesh / ring 讨论实验；§3.7 性能透视；§3.8 定时前置检查 | — |

---

## 9. 明确不做

- 无结构的多 Agent swarm、成员自由协商的写操作（J1）。
- 通用企业知识库：多租户 RBAC、IM 渠道、飞书 / Confluence 同步、OCR 管线、Docker 沙箱（ragflow / WeKnora 的主体）。
- GraphRAG 式社区摘要、图数据库、向量服务器（第一版）。
- 训练或微调：Hy-Memory-30B、FoldGRPO、编排 SFT。Aeroric 只做 harness 层。
- 自动删除用户偏好；后台物理删除用户事实。
- 用 CDP 或注入改别人的 App（codex-host 的做法），以及 hook 抓屏、剪贴板、键鼠采集。
- 双向同步各 Harness 的原生记忆文件。

---

## 10. 决定（原待定问题）

| # | 问题 | 推荐 |
|---|---|---|
| Q-F1 | 接受 R-1（去 lead，主会话即 manager）吗？ | 接受 |
| Q-F2 | 记忆写入"只追加 + 取代标记"（R-2）？ | 接受 |
| Q-F3 | Context MCP 合并为一个服务（R-3）？默认对四族都开吗？ | 合并；默认只开"历史 + 交接"，记忆 / 知识 / 协作按项目开启 |
| Q-F4 | 交接契约在哪些时机生成？ | 换模型、换 Harness、压缩后续接、委派、恢复会话五种全做 |
| Q-F5 | 审查回环默认 reviewer 用哪个？ | 与 coder 不同的族；用户可在角色库改 |
| Q-F6 | 项目 Wiki 的存放位置？ | 托管目录（`~/.aeroric/knowledge/<project>`），可导出到仓库；不默认写入用户仓库 |
| Q-F7 | 记忆抽取用哪个模型？费用归谁？ | 独立"分析模型"配置，默认用便宜模型，计入 A13，单独显示 |
| Q-F8 | 后台做梦默认开吗？ | 默认关，按项目开（与 Q19 自进化一致） |
| Q-F9 | 用户仓库里已有 `.agent-memory/`、`MEMORY.md` 怎么办？ | 只读展示，可"导入为候选" |
| Q-F10 | 信誉统计显示给用户吗？ | 显示，不自动选 |
| Q-F11 | 是否首发 Smart Friend？ | 不首发，放 B6，与网关的模型路由一起做 |
| Q-F12 | mesh / ring 讨论实验做不做？ | 只做只读讨论配方，P2 |
| Q-F13 | 批次顺序 | B4′ 交接与上下文 → B4 协作 → B5 记忆 → B5.5 项目 Wiki |

| Q-F14 | backlog 怎么回写 | 只在 backlog 末尾追加 §9 v3 增补，原文不改 |
| Q-F15 | 交接契约注入方式 | 支持系统级指令的族放系统层（Claude append system prompt、Codex developer instructions），omp/DSH 放首条消息；界面统一为交接卡 |
| Q-F16 | 自动检查点时机 | 每回合结束（全族）；有审批事件的族在危险操作前补存"拟执行＋核查方式"；其余按能力三元组如实标不支持 |
| Q-F17 | 开工前澄清默认 | 默认关，新建会话可开，配方可设默认开 |
| Q-F18 | 历史检索范围 | 默认只查当前项目，手动勾选才跨项目 |
| Q-F19 | 开工方式 | 新建 B4′ Trellis 任务（prd/design/implement），先出交接卡与检查点界面草图确认再实现 |

**2026-10-30 决定：Q-F1–Q-F19 全部采纳“推荐”列；R-1–R-6 已登记到 backlog §9。**

B4-1 审查回环（2026-10-30，全部按推荐采纳；Trellis 任务 `10-30-review-loop`）：

| # | 问题 | 决定 |
|---|---|---|
| Q-B1 | B4 怎么拆 | 三批：B4-1 审查回环；B4-2 委派、契约、权限衰减、审计、结果卡（含角色库）；B4-3 黑板、探针、Needs You |
| Q-B2 | 何时触发 | 会话级开关，默认关；开时每个改了文件的回合结束（有自动检查则等检查结束）自动审；改动卡上随时可"请人审查" |
| Q-B3 | 谁来审 | 默认"自动"：本机不同族的 Claude Code / Codex，没有则同族换模型；可手动指定 |
| Q-B4 | 审查者看到什么 | 隔离：只给 diff、验收条款、约束；只读权限，同一工作目录 |
| Q-B5 | 审查会话放哪 | 真实会话，侧栏挂在主会话下；主会话只显示审查卡 |
| Q-B6 | 意见如何回到 coder | 自动发，coder 逐条采纳 / 驳回（写理由）/ 需要你定；可暂停本轮；最多 3 轮，超限或"需要你定"进需要我处理 |
| Q-B7 | 何时算通过 | 无阻断问题即通过；coder 驳回全部阻断问题也结束并提示用户 |

B4-2 委派与契约（2026-10-31 夜间，owner 授权"尽可能多做"后按推荐自主采纳，owner 2026-10-31 目检通过并提交时确认；Trellis 任务 `10-31-delegation`）：

| # | 问题 | 决定 |
|---|---|---|
| Q-B8 | 谁能发起 | 主会话模型调用 Context MCP 的 `delegate` 工具；用户在输入框打 `#角色` / `#harness`；进度页"新委派" |
| Q-B9 | 子会话看到什么 | `fork`（带对话，仅同族）/ `brief`（交接清单）/ `isolated`（只给指令和文件），默认由角色定 |
| Q-B10 | 在哪写 | 第一版：共享目录只读或可写；同一时刻最多一个可写子会话；独立 worktree 等新建会话接通后再做 |
| Q-B11 | 权限 | 不高于主会话（plan < ask < auto_edit < full_access），core 校验；角色默认值自动降级，显式越权直接拒绝；深度 1 |
| Q-B12 | 结果回传 | 结果卡；模型发起的委派全部结束、主会话空闲时合并成一条消息自动发回；用户发起的由用户点"发给主会话" |
| Q-B13 | 预算 | 时长（默认 30 分钟）与 token；超出即打断，标"超出预算"，进需要我处理 |
| Q-B14 | 审计 | 只追加的 `audit.jsonl`，进度页可展开 |
| Q-B15 | 角色库 | 内置调研员、审查员、实现者、测试员（可改、可恢复），用户可增删自己的角色 |
| Q-B16 | 信誉 | 按 Harness × 模型 × 角色统计，新委派时显示，不自动选 |

B4-3 黑板、探针与 Needs You（2026-10-31 夜间，同样按推荐自主采纳，owner 2026-10-31 目检通过并提交时确认；Trellis 任务 `10-31-board-probes`）：

| # | 问题 | 决定 |
|---|---|---|
| Q-B17 | 黑板归属 | 每个主会话一块，它委派出的子会话共用 |
| Q-B18 | 记什么 | 目标、假设（待验证/成立/不成立）、认领（含认领人）、决定（提议/采纳/否决）、证据、问题（待解/已答） |
| Q-B19 | 谁能写 | 用户、主会话、子会话都能写；Agent 的决定一律"提议"，只有用户或主会话能采纳/否决；每次写入记变更 |
| Q-B20 | 注入 | `board_read` 只给该读者上次读取后的变化；子会话首条消息提示可用黑板 |
| Q-B21 | 探针 | 只在子会话上跑、只标记不打断：重复 3 次 / ABAB、八成预算无验收、完成未核销、无证据、认领 30 分钟未更新 |
| Q-B22 | Needs You | 新增"黑板要你定"（决定待确认、认领久未更新）；探针并入"委派要你定"，点"知道了"清除；任务提案等 §3.6 行动卡 |
| Q-B23 | 沉淀 | 黑板决定与经验进长期记忆留到 B5 |

B5 记忆（2026-10-31，owner 确认；Trellis 父任务 `10-31-memory`）：

| # | 问题 | 决定 |
|---|---|---|
| Q-M1 | 分批 | B5-1 地基（不额外调模型）→ B5-2 会话结案抽取与待确认收件箱、原生记忆导入为候选 → B5-3 经验、置信度、效果回流、并入 A06 提案收件箱；做梦留在 B6 |
| Q-M2 | 存储 | SQLite + FTS5，`<data>/memory/memory.db`，复用 `notebook/rag/cjk` 中文分词 |
| Q-M3 | 作用域 | 全局 + 项目，项目覆盖全局；分支随代码版本链（B6） |
| Q-M4 | 注入 | 原生会话启动时把画像放进系统层（Claude `--append-system-prompt`、Codex `developerInstructions`）；项目事实走 `memory_recall`；不改仓库文件 |
| Q-M5 | 记住 | 用户 `/记住` 由 Aeroric 直接写入、立即生效；Agent 用 `memory_propose`，一律待确认 |
| Q-M6 | 遗忘 | 永久遗忘删原文、留 HMAC 签名防复活；密钥在 data 目录 0600；归档与遗忘分开 |
| Q-M7 | 入口 | 侧栏单独的"记忆"入口（owner 选 b） |
| Q-M8 | 原生记忆文件 | B5-1 只读展示；导入为候选在 B5-2 |
| R2-1 | 按项目开启 | "关于你"所有原生会话都注入；项目事实与记忆工具只在开启记忆的项目里提供；项目默认关闭，首次 `/记住` 自动开启 |
| R2-2 | `/记住` 写到哪 | 默认当前项目，"已记住"卡可改为全局或撤销 |
| R2-3 | 入口位置 | 侧栏顶部导航；项目右键菜单"项目记忆…" |
| Q-M9 | 分析模型 | 用户在"整理设置"里从已启用、支持 chat 的连接里选连接加模型；没选不自动整理 |
| Q-M10 | 何时整理 | 已归档，或已完成且安静 30 分钟（原生会话每回合都会变成已完成，所以加了安静期）；只整理打开自动整理之后结束的；每个会话一次，会话菜单可强制整理；用户发言少于 2 条跳过 |
| Q-M11 | 产出 | 会话线索直接生效（模型整理，只检索不注入）；偏好和约定一律待确认 |
| Q-M12 | 发什么 | 只发对话文字，最近 24000 字，脱敏；附上已生效记忆用于去重和提议取代（取代要确认） |
| Q-M13 | 费用与上限 | 每次记模型、token、估算费用、产出，显示在整理记录；每天默认 30 次；记在 memory.db |
| Q-M14 | 原生导入 | 按行导入为待确认，不调用模型，已有的和遗忘过的跳过 |
| Q-M15 | 开关 | 自动整理默认关，选好模型才能开；只整理开启了记忆的项目 |
| Q-M16 | B5-3 拆分 | B5-3a 经验与效果回流；B5-3b 提案收件箱（经验升格为技能，接 A06）另行讨论 |
| Q-M17 | 经验来源 | B5-2 同一次整理调用顺带产出 0–2 条（标题、问题、解法、关键词、适用环境），只在确实解决了问题时写 |
| Q-M18 | 经验确认 | 直接生效，只在 memory_recall 出现、不注入；可归档、修改、遗忘 |
| Q-M19 | 置信度 | 低、中、高；验收全过时起步中；有用 1 次到中、3 次到高；不对降一档，低档再标归档；检索按置信度与有用次数排序 |
| Q-M20 | 效果回流 | 自动计检索与注入次数；Agent 用 memory_feedback、用户在记忆页标有用/不对；所有条目适用 |
| Q-M21 | 经验作用域 | 默认本项目，用户可改为全局 |

---

## 附录 A：来源索引（静态阅读，HEAD 为 2026-10-30 拉取后）

**论文（`论文/`，多 Agent 协作）**

| ID | 来源 | 本文用到的要点 |
|---|---|---|
| PAP-COG | Cognition, *Multi-Agents: What's Actually Working* (2026-04) | 单写多智、审查回环无共享上下文、Smart Friend、map-reduce-and-manage |
| PAP-LC | LangChain, *Organizing Context in a Multi-Agent Harness* (2026-09) | fork / isolated 上下文模式 |
| PAP-CFRC | *Do Not Restart: Residual Completion for Handoffs*, arXiv 2609.13800 | 承诺前沿、残差契约、实时收据核销 |
| PAP-HH | *Handoff Hallucinations*, Springer 2026 | 交接幻觉三型、认知标记 |
| PAP-FWR | *Facts Without Rules*, arXiv 2608.29028 | 边界元数据塌缩、受众白名单 |
| PAP-UND | *What Prompts Don't Say*, arXiv 2505.13360 | 欠规格脆弱性 |
| PAP-GG | *Grounding Gaps*, arXiv 2311.09144 | 模型少做澄清 |
| PAP-MAST | *Why Do Multi-Agent LLM Systems Fail?*, arXiv 2503.13657 | 14 种失效模式 |
| PAP-SCALE | *Towards a Science of Scaling Agent Systems*, arXiv 2512.08296 | 能力饱和、错误放大、架构-任务匹配 |
| PAP-AO | *AOrchestra*, arXiv 2602.03786 | 子代理四元组 |
| PAP-MSE | *MS-Eval*, arXiv 2610.22869 | 专用子代理最好、过度委派 |
| PAP-FOLD | *Context-Folding*, arXiv 2510.11967 | branch / return 折叠 |
| PAP-DEL | *Intelligent AI Delegation*, arXiv 2602.11865 | contract-first、权限衰减、委派链 |

**agent-memory（`agent-memory/`）**

| ID | 来源 | 本文用到的要点 |
|---|---|---|
| MEM-WB | WorldBase da96914 的设计稿：`docs/memory-user-model-and-consolidation-design.md`、`group-conversation-prd.md`、`shared-memory-embedding-architecture.md`（**提案未实现**） | 事实 / 证据 / 画像、做梦、遗忘屏障、共享黑板 |
| MEM-HY | Hy-Memory 988667c（README、memhub/docs/ARCHITECTURE.md） | 三场景、事实与经验两层、diff 链、MemHub 四类权限 |
| MEM-M0 | mem0 26cdefb4 README | ADD-only、三路融合、时间感知 |
| MEM-GR | graphiti 72b5c59 README | 时间有效期、episode 溯源 |
| MEM-LME 等 | 2410.10813 LongMemEval、2502.12110 A-Mem、2502.14802 HippoRAG 2、2504.19413 Mem0、2507.03724 MemOS、2404.16130 GraphRAG、2410.05779 LightRAG | 记忆能力维度、Zettelkasten 链接、MemCube 元数据、图检索 |

**知识库（`知识库/`）**

| ID | 来源 | 本文用到的要点 |
|---|---|---|
| KB-RF | ragflow 2fc8373d4（README_zh、`docs/guides/knowledge_compilation/overview.md`） | 知识编译与 Graph、Tree、PageIndex、Mindmap、Timeline、Wiki、Skills 等产物 |
| KB-WK | WeKnora 40192ace2（README_CN、`internal/agent/prompts_wiki.go`、`internal/agent/tools/wiki_*`、`docs/design/agent-hitl-suspend-resume.md`） | Wiki 编译流程、issue 工单、两段式挂起 |

**harness（`harness/`）**

| ID | 来源 | 本文用到的要点 |
|---|---|---|
| QN | qunica 52258cb（README、`backend-rs/crates/backend/src/docs/guide/groups.md`） | 四拓扑、bounded / automatic 调度与预算、call / handoff、群组笔记 MCP |
| CH | codex-host 33759b75 README | `#agent` 委派、子代理可见、fork 到 worktree |
| RUN#873 | runner 40e96fde，`docs/features/873-remove-mission-lead.md`（在该提交中） | 去掉 lead |
| D | deepseek-harness ce7073d512 | subagent-oar、trajectory 折叠、action-cards、liveshare、forge-session、client-ui-stats |
| P | paperclip 7a7ade4b9 | Coordinator workspace（`2026-10-26-coordinator-experimental-workspace.md`） |
| N | opencode 92b2ca0dd7 | 定时 prompt 前置 shell 检查 |
| NET | Obsidian《多Agent本地记忆共享方案》第四版及评论 | `.agent-memory` 结构、检查点、并发、评论中的多 Agent 适用场景 |

其余仓库（magpie、cc-switch、CLIProxyAPI、codeg、MiMo、minimax、kloop、Agents-Anywhere、herdr、1flowbase、AstrBot、nezha、Markio 等）本轮的增量主要是网关、用量、ACP 适配，不改变上述方向；必要时用 upstream-sync 技能单独复查。
