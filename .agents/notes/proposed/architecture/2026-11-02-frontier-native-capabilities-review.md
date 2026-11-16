# Agent Note: Aeroric 前沿原生能力独立评审：可靠交接、项目知识、受控协作与长期记忆

Status: proposed

## Problem

用户希望把精挑的 harness、知识库、多 Agent 论文、长期记忆及 New-addition 项目中的优点，转化为 Aeroric 原生功能。本评审以 **2026-11-02 正式 Aeroric 当前工作树**为基线，包括尚未提交的 memory/skills、knowledge/wiki、协议和 SwiftUI 修改。

本篇是对 [backlog](../../../../docs/backlog.md) 和 [frontier-roadmap](../../../../docs/frontier-roadmap.md) 的独立增补评审。**不修改两份原文，不改变已批准决定，不实施产品代码。** 文中的“建议修订”需要后续评审，不能当成用户已经批准实现。`harness/Aeroric` 是另一参考副本，不是正式项目基线。

阅读导航：§1–3看当前基线与规划异议，§4–6看四类原生能力和新项目增量，§7–9看落地顺序、验收及后续取舍；附录B/C/D分别提供六个知识记忆项目、逐篇论文和全部182份副本的覆盖，附录E追踪源码证据与验证。

核心问题已经从“还缺哪些功能”变成：已有记忆、Wiki、交接、委派如何共享可靠的来源、版本、授权与恢复合同；如何在失败、并发、上下文压缩后仍准确表达实际状态；如何用本机真实任务证明新增机制有收益。

### 阅读计划、范围与证据口径

按“当前实现 → 资料清点 → 全项目入口覆盖 → 关键源码与失败路径 → 论文方法/实验/局限 → 原路线逐项评审 → 原生设计与验收”的顺序开展。跨上下文进度、原始清单及原文哈希保存在项目本地 `.local_docs/frontier-review-2026-11-02/`。

| 范围 | 覆盖 |
|---|---|
| 正式 Aeroric | 架构/子系统/规则、两份规划全文；memory、Wiki、delegation、handoff、Context MCP 的 core → host → protocol → macOS 调用链与现有测试 |
| harness | 27 份工作副本；全部入口/结构覆盖，按价值下钻源码、失败边界和测试 |
| 知识库 | WeKnora、ragflow；Lite/编译流水线/引用权限/增量任务/证据定位 |
| agent-memory | Hy-Memory、Graphiti、WorldBase、mem0；治理、时间、作用域、版本链、抽取与检索 |
| New-addition | 现存 149 份工作副本；逐项入口覆盖，重点机制下钻；不恢复手动删除项目 |
| 论文与行业文章 | 20份主材料加Hy-Memory内CMB，共21 PDF/542页；方法、主要实验、局限及关键附录；两份HTML与对应PDF为同一来源 |
| 网友方案 | [多 Agent 本地记忆共享方案及评论](/Users/macbook/Documents/GitHub/Obsidian/多Agent本地记忆共享方案.md)，第四版、旧版差异与评论 |

共 182 份参考工作副本，按规范化 remote 去重为 **177 个来源**。重复来源是 runner、AstrBot、ponytail、deepseek-harness、opencode，其中部分 HEAD 不同。不能把同名不同 owner 的 OpenBot、Pi、Orca 等合并。逐项目覆盖矩阵在附录。

“入口”表示读取 README 的功能/架构/限制选段及目录，巨大技能目录以机制和代表条目为主；“源码”表示另追了相关实现、失败路径或测试，仍不是整仓逐行审计。论文也不是542页全部逐字精读；附录记录具体阅读页段。递归盘点另见893个PDF路径，其中789个属于openai/math数学集合，另有演示、许可、产品资料、图标和测试样本；它们没有全部逐篇精读，不用于本篇的Agent能力效果论证。没有运行参考项目、论文实验、真实模型调用、GUI 或 VM，没有核验外部网站的最新状态。现有测试仅说明仓库具备哪些验证设计，**本次没有执行产品测试**。

源码能证明调用、校验和状态转换存在，不能单凭函数名、README、测试文件名或模型自述证明生产效果。文中区分“已静态接线”“待运行验收”“设计提案”“宣传或经验”。

## Proposal

保留现有 Rust core/host、四族结构化协议、SwiftUI typed 客户端、单数据根单写者的架构。原生化的重点应是 **证据与上下文基础层、可靠执行合同、可审查的增量知识、按资源隔离的协作、带适用条件的记忆**。不另造第五个 Agent 推理内核、第二套网关或第二个记忆权威。

建议优先级：先修正交接/投递/落盘的状态真实性，再扩展知识编译与上下文，之后扩大受控并行，最后开放自动整理和策略学习。新能力优先做成现有任务、检查点、Wiki、记忆和需要我处理中的纵向切片。

### 1. 当前 Aeroric 的真实起点

| 能力 | 当前工作树可追踪的实现 | 不应扩大的结论 |
|---|---|---|
| 记忆 | SQLite/WAL/FTS5、事实/画像/会话线索/经验、候选确认、证据、supersedes、遗忘屏障、抽取记录、反馈、技能提案/版本/挂载、原生界面 | 不等于语义检索、完整主体治理、跨来源语义遗忘或四族全部收到画像 |
| Wiki | 确定性编译 notes/经验/技能；决策页/操作手册/索引；段落手改保护、issues、修订、FTS、Markdown 投影、导出、UI/MCP | 不是空壳；也还不是代码模块图、PDF/网页多源编译、模型综合或 GraphRAG |
| 委派 | 真实子任务、fork/brief/isolated、权限衰减、深度/时长/token、验收项、结果卡、角色、探针、恢复 Interrupted | 不是跨父会话的工作区写锁，不是磁盘隔离、通用 DAG 或可靠结果 outbox |
| 交接 | 结构化 checkpoint、unknown effects、残余义务、模型/来源切换、首条 brief、侧支结果 | 不等于跨任意 Harness 保留原生工具/审批/KV 状态；同族 native carry 也有前提 |
| Context MCP | stdio → HostClient → task token → host 业务；历史/交接/协作，按开关列记忆/Wiki 工具 | 当前只注入本地 Claude/Codex；OMP/DSH/远端未同等接通；历史为项目内有界文本检索 |

主要落点：`core/crates/aeroric-core/src/{memory,knowledge,sessions/live}`、`aeroric-host/src/session_live_services`、`aeroric-protocol`、`aeroric-cli/src/mcp_context.rs`、`macos/AeroricKit/Sources/AeroricUI/{Memory,Wiki}`。产品中的 Notebook/RAG 已有解析/索引及中文分词能力；新增知识检索应复用可用组件，保持各业务自己的授权和生命周期，避免重新造一套同功能索引。

### 2. 先补可靠性：现有实现中需要单独处理的边界

以下是**静态代码审阅发现的风险/缺口**，未通过故障注入复现；本篇只提出后续验证与修复建议。

| 编号 | 证据与问题 | 建议及最小验收 |
|---|---|---|
| NR-01 | [delegation/mod.rs:622](/Users/macbook/Downloads/同步空间/LYX/Aeroric/core/crates/aeroric-core/src/sessions/live/delegation/mod.rs:622)、同文件 :983，先记 sent 再异步 prompt，失败只记 stderr | 结果与投递 outbox 同事务；区分 pending/submitted/confirmed/unknown；断连、进程重启、重复提交不丢结果，也不重复执行 |
| NR-02 | [session_live_services.rs:1819](/Users/macbook/Downloads/同步空间/LYX/Aeroric/core/crates/aeroric-host/src/session_live_services.rs:1819)、checkpoint `take_armed`，启动前消费交接 | 交接保留 pending，绑定目标 attempt；确认注入后才消费；启动失败可重试，未知提交先对账 |
| NR-03 | [delegation/mod.rs:450](/Users/macbook/Downloads/同步空间/LYX/Aeroric/core/crates/aeroric-core/src/sessions/live/delegation/mod.rs:450) 仅按 parent 查可写 child；redo 未见等价重检 | 用 canonical workspace/resource 身份统一准入；覆盖不同父、普通会话、父子、redo、symlink 别名与迟到结果 |
| NR-04 | [memory/run.rs:144](/Users/macbook/Downloads/同步空间/LYX/Aeroric/core/crates/aeroric-core/src/memory/run.rs:144) 等写入错误被 `.is_ok()` / `if let Ok` 吞掉，结束可记 Done；runs/extracted 非同一提交 | 定义全成/部分成功/失败，原子提交记录与游标；写入失败不可显示“已记住”，重试幂等 |
| NR-05 | [memory/store.rs:703](/Users/macbook/Downloads/同步空间/LYX/Aeroric/core/crates/aeroric-core/src/memory/store.rs:703) FTS 全库先截候选再筛项目 | scope/status/有效期在候选排名和 limit 前执行；用大量其他项目高分命中验证当前项目召全率 |
| NR-06 | [host/wiki.rs:89](/Users/macbook/Downloads/同步空间/LYX/Aeroric/core/crates/aeroric-host/src/session_live_services/wiki.rs:89)、[knowledge/notes.rs:63](/Users/macbook/Downloads/同步空间/LYX/Aeroric/core/crates/aeroric-core/src/knowledge/notes.rs:63)，读取错误可降级为空；编译可能当来源消失 | source 状态至少 present/confirmed_removed/unavailable；读取失败保留现有页并标待更新，不能触发贡献删除 |
| NR-07 | [knowledge/mod.rs:413](/Users/macbook/Downloads/同步空间/LYX/Aeroric/core/crates/aeroric-core/src/knowledge/mod.rs:413)、notes walk，导出只检字符串，遍历跟随目录链接 | 路径解析/符号链接/目录环/大小预算/检查后写前竞态由 host 校验；导出预览绑定目标版本与覆盖清单 |
| NR-08 | [memory/skills.rs:138](/Users/macbook/Downloads/同步空间/LYX/Aeroric/core/crates/aeroric-core/src/memory/skills.rs:138)、[knowledge/store.rs:112](/Users/macbook/Downloads/同步空间/LYX/Aeroric/core/crates/aeroric-core/src/knowledge/store.rs:112)，DB、版本文件、index/Markdown 投影分步提交 | DB 为权威，文件为可修复投影；outbox/原子文件替换/幂等重建；提案 ID 独立命名、revision CAS，防同名覆盖 |
| NR-09 | [executor.rs:537](/Users/macbook/Downloads/同步空间/LYX/Aeroric/core/crates/aeroric-core/src/sessions/live/executor.rs:537)，MCP 仅 Claude/Codex；画像仅系统层，OMP/DSH 未降级注入；计数可早于实际设置 | 每族/模式建立能力与实际注入收据；启动配置、工具发现、实际调用、画像有效版本分别验收，不能靠 mount 成功计“已用” |
| NR-10 | [live.rs:144](/Users/macbook/Downloads/同步空间/LYX/Aeroric/core/crates/aeroric-host/src/live.rs:144) 配置型 Codex 写项目标记技能副本；配置型 Claude/OMP 按配置共享托管 home | 如实说明仓库会出现的托管副本；会话/项目隔离挂载快照；并发启动两项目验证无技能串见，保留用户自有文件 |
| NR-11 | [host/wiki.rs:104](/Users/macbook/Downloads/同步空间/LYX/Aeroric/core/crates/aeroric-host/src/session_live_services/wiki.rs:104) 指纹依赖属性、数量及最大更新时间；[knowledge/mod.rs:238](/Users/macbook/Downloads/同步空间/LYX/Aeroric/core/crates/aeroric-core/src/knowledge/mod.rs:238) 保存判据可漏仅状态/分类变化 | 内容 hash + 元数据版本 + 编译器版本；测试同大小/同 mtime 修改、仅状态变化与删除/读取失败 |

NR-10 的跨项目技能受后一启动影响是根据共享 home 与按 cwd 替换链接推导的并发风险，**未运行证明泄漏**。已有笔记也披露了它：[memory-skills](../../implemented/feature/2026-10-31-memory-skills.md)。不能把“托管 home”直接等同于每会话隔离。

### 3. 对两份规划的独立修订意见

#### 3.1 应保留的决定

- 保留 Rust 业务边界、结构化四族、按项目控制 Context MCP、SQLite/FTS 优先、数据库权威与可导出投影、模型只提议重要记忆变更、用户审批技能、默认关闭后台整理。
- 保留主会话承担 manager，reviewer 使用隔离上下文与冻结目标；不新增 lead 人格。保留一层委派作为可控默认，动态多层与 mesh 仍是后续实验。
- 保留“未知副作用先核对，不能盲重放”“归档与遗忘分开”“原生记忆文件只读展示/候选导入”。补齐实现边界，比换术语更重要。
- Q-F/Q-B/Q-M/Q-W 已有批准结果；下面的不同意见只是新的评审建议，不使这些批准自动失效。

#### 3.2 需要修订的论据与行为承诺

| 原位置 | 评审 | 建议表述/行动 |
|---|---|---|
| backlog §4.2、B4 的 lead/PTY/hook 描述 | 与后续结构化协议及 §9 增补存在历史口径差异 | 阅读时以后续批准和实现为准；未来若另行授权整理，登记一个现行事实入口，避免把旧实现再规划一次 |
| frontier J1“写操作单线程，其他 Agent 不贡献动作” | 工程默认可保留，不能当论文普适定理 | 权威数据单写者；共享工作区单 owner；隔离文件/资源可受控并行。扩大写入并行需要变更 Q-B10，不能默认启用 |
| J1“45%”“17.2×/4.4×”“+80.8%/−70%” | 前者是特定回归经验；中者不是失败概率倍数；后者来自两格配置 | 不设运行时硬阈值。附录记录原始条件、绝对差与统计限制，架构选择由本地任务对照决定 |
| J1 Cognition 三种形态、Smart Friend | 作者经验不是穷尽定律；弱主控问强模型的例子实际未达预期 | 保留咨询工具，先做明确触发/预算/结果验收，不承诺弱主控自动获得强模型能力 |
| J2“失败多数发生交接边界” | MAST 不能支持；设计失效 41.8%、交互 36.9%、验证 21.3% | 改为“交接是需要单独测量的风险边界”；同时测需求、资源、验证和早停 |
| J2 白名单几乎消除泄露 | FWR 的 0/48 是人工 gold 标签 + 提示遵守的上界试验 | 授权由 host 确定性执行；标签自动生成仍需校验，不能从此宣称生产零泄露 |
| J3 续做 fork/评审 isolated | 是良好默认，但精选上下文有时优于 full fork | 增加 curated source pack；记录模式、来源与预算。实际 cache hit 独立测量 |
| J4 隐式需求换模型回归“2 倍” | 对象是特定要求的退步比例，不是任意任务回归概率 | 使用显式验收集测试模型变更；清晰任务不强制多轮澄清 |
| J5/§6.2 ADD-only | 自动抽取追加、显式纠正、删除/遗忘是不同合同 | 保留变更事件与 supersedes；永久遗忘允许删敏感载荷。mem0 的 update/delete API 仍存在，不能用 ADD-only 代替治理 |
| J5 mem0 新数字 | README 明说来自含专有优化的托管平台 | 不把 92.5/94.4 等当 OSS SDK 可复现承诺；2025 本地论文与2026源码分开 |
| J5 WorldBase 作为完整治理参考 | 设计很好，新增治理模块明确“尚未实现” | 采纳机制与验收，不把提案当成熟产品验证 |
| §3.1 认知状态与“已验证” | 收到 result、exit=0、存在 diff 都不足以证明目标完成 | 分 provenance、效果收据、独立验证；记录验证者、目标版本、覆盖范围、时间与 unknown |
| §3.3 开工前澄清 | 论文不支持所有任务阻塞 | 保留已批准默认关闭；仅对关键且无法本地解除的歧义提问 |
| §3.4“不需要训练”的 Context Folding | branch/return 可做交互原型，论文最佳收益依赖 FoldGRPO | UI 折叠、模型活跃上下文裁剪、KV 回滚是三层；不能互相冒充实现 |
| §3.5、§6.4 四族共享 | 目标合理，当前 MCP/画像覆盖不齐 | 改成按族 × 启动/恢复 × 本机/远端 × 能力逐格验收；未接通不显示可用 |
| §4.1 WeKnora 很重且依赖外部服务 | Lite 已有单应用、SQLite/内存队列、零外部服务 | 保留个人项目知识的定位，修正选型理由；不要用过时重服务印象否掉机制 |
| J7 ragflow 六种产物加 Skills | 当前知识编译文档与 Go 实现定位到六类，未找到同合同的 Skills 产物 | Wiki/Graph/Tree/PageIndex/Mindmap/Timeline 按实记；技能升格归 memory/skills 的验证流程 |
| §4.3 编译/增量 | 当前确定性 Wiki 已落地，不应重造 | 下一批补 source revision/贡献失效/代码与外部材料；模型调用只是可替换编译阶段 |
| §6.3 上下文构建 | 只存抽取事实会丢细节；LongMemEval 支持原 round value + fact key | 保原文证据与时间过滤，事实增强检索，不替代所有原材料 |
| Q-M19/M23 有用三次 → 高 → 技能提案 | 可以是产品启发式，不能等同三次独立验证 | 分离 usage、模型反馈、用户确认和独立 outcome；技能批准前带适用条件及可运行验收 |
| Q-M6 HMAC 防复活 | 文本签名只能挡住其匹配范围 | 增加来源谱系/命题版本/派生关系屏障，重建与恢复也检查；不承诺所有释义或第三方副本彻底抹除 |
| Q-W2/Q-W3 可改 Markdown | App 内改经数据库；磁盘投影当前没有自动回流 | 明示编辑入口；若增加外部编辑，走显式导入、revision 检查与三方合并 |
| B4′/B4/B5/B5.5 状态 | 当前代码与已记录目检各有不同状态 | 后续实施以工作树/具体构建/实际验收记录为准；本评审不重写历史勾选框 |

#### 3.3 不建议采纳的延伸

不以“最前沿”作为全自动多 Agent、无界自进化、全量知识图谱、模型内核训练或大规模重平台化的理由。建议明确反对：用 45% 阈值决定是否委派；把签名/JSON schema 当语义真值证明；把 read-only/worktree 名称当外部副作用隔离；把超时当未执行；把所有召回/好评当独立证据；把图、摘要、技能彼此互相引用造成的循环当新事实。

这些反对针对论据与设计延伸，不否定用户已批准的功能目标。已有的默认开关、角色库和用户控制可以保留。

### 4. 统一底层合同：证据、权限、版本、收据

建议抽取少量共享类型，由现有 owner 各自持有实体，**不新增囊括所有业务的“上下文超级数据库”**。

| 共享类型（建议，尚非现有 wire） | 最低字段与语义 | owner/接入点 |
|---|---|---|
| `SourceRef/SourceSpan` | 来源 ID、类型、稳定逻辑消息/工件 ID、revision/content hash、段落/页/字节位置、时间、可访问 scope；原文与译文分版本 | sessions/files/knowledge 持有原材料；memory/Wiki 引用，避免全量复制 |
| `EvidenceLink` | supports/refutes/derived_from、源版本、命题片段；区分用户原话、工具观察、模型推断、独立核验 | memory 与 knowledge 各自建关系；相同原话重抽不增加独立证据 |
| `ContextManifest` | task/attempt、读取者、目的、源 revision、预算、截断/遗漏、画像/规则版本、边界、注入结果 | context/checkpoint；记录“选了什么/为何/实际交付什么”，不存第二套事实 |
| `OperationReceipt` | operation/request/idempotency key、resource/参数摘要、attempt/generation、requested/submitted/observed/verified/unknown、核查方式 | 会话协议/host 执行；只有满足具体核查的结果才核销对应义务 |
| `ResourceLease` | data root/native session/workspace/外部资源身份、owner、generation、过期及收回依据 | host 持有；不同资源锁不能用一个“单写者”词替代 |
| `ChangeSet` | 候选修改、前置 revision、证据、权限、预算、风险、批准者、应用结果与投影修复任务 | memory/knowledge/skills 各自 validator；模型不能设置 trusted origin 或批准状态 |

读取流程：可信任务身份 → scope/受众过滤 → 分类/词法/结构检索 → 必要时可选向量候选 → 回读当前权威状态与证据 → 有界 `ContextManifest` → 适配器注入 → 实际注入收据。权限应在候选检索、证据回读和向目标外发三个边界检查，避免先取全量再在 prompt 里要求保密。

写入流程：结构化事件/明确用户动作 → 原始记录及事实状态 → 有证据的候选 → validator/CAS → 短事务提交实体与 outbox → 异步修复画像、FTS、Wiki、文件投影 → 才报告已应用。模型调用不占写锁；投影失败保留可恢复状态，不用“重做整个任务”补救。

来源位置和 hash 能证明找到哪段字，不能自动证明该段支持结论；签名能证明主体/完整性，不能证明事实为真。`verified` 必须绑定可说明的 validator 与范围。

### 5. 四类能力如何成为 Aeroric 原生功能

#### 5.1 Harness：能交接、能取消、能解释恢复行为的执行层

**H1：可靠交接与结果投递。** 保留当前 checkpoint 中的目标、决定、约束、已发生效果、未知效果和剩余义务，给“发到目标会话”增加独立持久状态。建议状态为 `prepared → pending → submitted → confirmed`，另有 `failed/unknown/cancelled`；它们描述交付，不代替任务的运行/验收状态。

核心流程是：先提交结果或交接实体及 outbox；发送者按 lease 领取；将内容绑定目标 native session、attempt 和上下文版本；驱动确认可观察的接收后，再标注对应交付阶段。对于只有“写入管道成功”证据的 driver，只能称 submitted，不能声称目标模型已读取。收到不确定错误时，先查询目标会话/消息标识；没有可核对接口就进入需要处理，不自动重发有外部效果的动作。旧 attempt 的迟到结果可以保留，但不能覆盖新 attempt 的状态。

借鉴 T3Code 的 pending/injected 与遗漏 coverage、pi-wm 的 fence/幂等校验、codex-host 的未知发送保护。CopilotKit/OpenBot 的整批续租值得采用，但其 finish 后再 enqueue relay 的窗口不能照搬。**幂等请求、可重复的只读查询、不可撤回的外部效果分开处理，不承诺跨任意工具的 exactly-once。**

原生呈现落在现有交接卡、委派结果卡和“需要我处理”：用户看到“等待发送 / 已提交 / 接收已确认 / 结果待核对”、目标会话及可用的核对/重试动作。协议细节放在详情。关闭窗口、App 重启或切换模型都不应让“尚未送达”变成“已完成”。

**H2：启动、取消和清理共用生命周期。** `admitted → preparing → running → cancelling → draining → settled` 是建议的 host 生命周期，业务结果另记 success/failure/interrupted/unknown。把 setup、工具子进程、委派树和清理纳入同一取消域；以 generation 拒绝过期回调。每阶段记录超时含义，启动等待不能无限延长。ZCode 的 inactivity watchdog、DSH 的树级 activation 准入与清理、OMP 的审查循环护栏可分别借鉴。取消请求被接受不等于进程已退出，也不等于外部操作未发生。

**H3：能力矩阵成为可查询事实。** 每族、配置、运行位置分别声明系统指令、上下文工具、工具限制、审批、原生 fork、模型切换、usage、取消、恢复等支持状态，至少区分 supported/unsupported/unknown/degraded。host 在启动前验证组合，启动后登记实际结果；不支持的强制限制应拒绝或要求明确降级，不能静默忽略。

OpenMausBot 的 catalog 与调用双检查、OpenShell 的策略保存/准入/激活分层最有价值。它们不让 macOS 自动获得 Linux Landlock，也不能证明第三方 CLI 内部的所有工具都被 Aeroric 拦截。项目级只读、审批策略、工具可见性、文件隔离、网络隔离需要各自说明实际执行者。

**H4：上下文工作台与可回读压缩。** 在现有进度页增加“本次带入的上下文”：目标/验收、当前决定、证据目录、画像和规则版本、尚未确定的效果，以及明确遗漏的材料。按任务选择三种上下文策略：

| 策略 | 合适场景 | 必须记录的边界 |
|---|---|---|
| native fork | 同族继续实现、需要原会话工具上下文且 provider 支持 | 来源 thread、checkpoint、兼容条件；cache hit 取真实 usage，不预先保证 |
| curated pack | 跨模型/跨族续做、定向源码研究、咨询 | 选中来源与 revision、用途/受众、预算分配、遗漏及回读入口 |
| fresh/isolated | 独立审查、对照评测、避免实现者结论影响判断 | 冻结目标、验收合同和允许证据；不能误称磁盘或外部服务已隔离 |

把旁支探索压成摘要后，保留源事件与工件目录，允许按需回读；压缩前后检查义务、决定、权限和 unknown 是否守恒。事实证据不能仅靠摘要相互印证。先做 checkpoint/ContextManifest 层的“可逆摘要”，再对支持替换活跃上下文的 driver 做裁剪。UI 折叠只改变显示；没有 driver 能力就不宣称模型上下文已缩短；KV 回滚另属模型服务能力。

**代码落点。** 在 `sessions/live/checkpoint` 增加交付记录/上下文清单的领域类型；`delegation` 复用交付服务；host 负责持久队列、driver 调用与对账；`aeroric-protocol` 增加 typed 请求/状态，仍由生成 schema 同步 Swift。`mcp_context.rs` 提供按清单回读来源的窄工具；实际注入覆盖先补齐并验收四族，再扩到远端。

#### 5.2 知识库：从“能编 Wiki”走向“能追踪更新与撤回的项目知识”

**K1：沿用现有 Wiki，扩展来源接入。** 第一类是代码：模块、符号、调用/依赖、配置与测试入口；第二类是项目文档/决策；第三类是用户显式选入的 PDF、网页快照及研究摘录。Notebook 的解析/切片/中文检索可作为共享库复用；Wiki 自己持有项目编译与修订生命周期。不把文件夹中所有内容默认上传给模型。

最小编译流水线建议为 `discover → snapshot/parse → source spans → deterministic structure → optional synthesis → validate → merge → index/project`。确定性阶段生成可定位的结构；模型阶段是可选、可取消、有成本上限的提案器。源码按语言逐步接入 parser，不能以文本摘要冒充精确调用图。PDF 保留页码、坐标/段落或明确的定位降级；网页保留抓取时间与内容 hash；译文另作中文阅读层，引用仍可打开原文。

**K2：增量编译必须能解释“为什么变了”。** 为每份 source 建内容 revision，记录每个派生 block 的 source contribution、编译器版本和生成输入。变更队列持久化并 debounce，worker 领取 revision 后只对该版本提交；过期结果不覆盖新页。来源状态采用 present/confirmed_removed/unavailable；只有确认移除才撤回贡献。两个文档支撑同一段时，删除一个只撤掉它的贡献；剩余结论需重新评估，不能简单 union 后永久保留。

ragflow 的 dirty revision/lease 与文档级编译、集合级合并分离值得吸收，但 `source_doc_ids` 的 union 不足以实现贡献删除。ThinkRail 的稳定 ID 与悬空关系校验适合补 Wiki 健康检查；不把文件图直接当并发事务系统。WeKnora 的来源授权、明确空 source_refs 与未提供的区别、保预算多页读取和遗漏列表值得直接转化成合同。

**K3：手动修改是独立贡献。** 保留现有 block 的 user_edited 保护；编译只给建议差异和冲突，不能覆盖人工段落。编辑/采纳/回退带 expected revision；落库后 Markdown 是可修复投影。若用户以后需要直接在 Obsidian 改导出文件，增加显式“导入修改”与三方比较，不能现在就把磁盘投影当自动双向同步。保留用户文字与来源引用的关系，不因格式整理丢失脚注。

**K4：先可靠检索，再按任务验证图收益。** 第一层是项目/时间/状态过滤后的 FTS、符号和结构导航；第二层回读原文；第三层可选 embedding/rerank；只对跨文档综合有明确收益的任务启用局部关系图/社区摘要。回答展示出处、版本、冲突和未覆盖材料；“有引用”不代表已核查蕴涵，soft evidence gate 不能显示成已验证。没有证据时返回缺口和建议读取的位置。

原生页树保留决策、操作手册、索引，增模块说明与研究材料入口；侧边来源可定位具体 note、记忆、技能版本或论文页。编译预览展示改动、受影响页面、人工冲突与费用估算；修复建议进入现有 issues/需要处理。Graph、Timeline、Mindmap 应是同一来源数据的视图，按用户价值逐步加，不各建数据库。

**代码落点。** `knowledge` 增 source registry、contribution、编译 job/revision 与 validator；复用现有 store/compile/notes；host 处理扫描/解析/模型调用及安全文件操作；UI 继续经 WikiStore/SessionsClient。原文提取、SQLite 检索和中文分词从 Notebook/RAG 提取窄共享组件，不共享越权的查询入口。关系图先用现有 SQLite 表与有界遍历，达到本机基准瓶颈后才考虑额外服务。

#### 5.3 多 Agent 协作：把“并行”变成可控的任务与资源协议

**C1：分清三种写者。** 数据根单 writer 保护 Aeroric 权威存储；native-session writer 防同一 CLI thread 被多个入口同时驱动；workspace/resource owner 防几个会话覆盖同一工作区或外部资源。三者互不替代。按 canonical 路径、项目身份和资源 ID 建 lease，覆盖父子、不同父、普通会话、重做和恢复。

首批继续遵守 Q-B10：共享工作区同一时刻一个写 owner，只读研究和独立审查并行。下一阶段若另行批准，再允许独立工作副本/明确分区资源并行产出 ChangeSet；“声明只改某文件”只是协调信息，不能当 OS 权限。仓库隔离也不隔离同一端口、数据库、云凭据、网络副作用或共享模型额度。

**C2：轻量阶段图，不先做无界团队。** 在当前委派卡上增加 dependency、输入 source pack、owner、attempt、预算预留和结果工件；依赖图拒环，claim 原子校验依赖/容量/owner。主会话仍负责拆解和最终合成。显式咨询、探索、实现、验证角色可选 fresh/curated/fork，但不把每个小任务拆成固定十几位成员。

建议状态区分 queued/claimed/preparing/running/reported/awaiting_review/accepted/failed/interrupted；验收必须绑定具体 commit/diff/工件版本。结果不完整时说明剩余义务；一次工具调用成功不能自动把整节点 accepted。独立 reviewer 可读冻结目标与证据，不自动继承实现者的自评结论。涉及主观效果的界面仍按签名构建由用户审批，不绕过现有规则。

**C3：结果、应用、验收分权。** 实现者提交 patch/工件及测试记录；host 校验 base hash、允许范围和冲突；应用动作需要原有授权；验证者针对冻结验收检查，用户可退回或接受。自动检查通过只核销对应项目。Reprise 的固定起点/污染检查、web-builder-harness 的 feature hash、MiniMax worker 提议完成而 host 核 verdict，都可借鉴；单模型 verdict 仍不能当客观正确证明。

oops 的 pre/post/current 比较可支持保守撤销：后来变动的文件保留并报告，过大/未保护文件明确列出；不可宣称能识别同一回合中所有人工编辑归属。不会为“恢复原样”运行破坏性 git 丢弃未知工作。

**C4：持久 mailbox、事件唤醒与用户优先。** worker 闲置不通过模型轮询。一个结果交付触发主会话或同 retained session 的下一回合；同一批结果可合并，父忙时等安全边界，用户消息优先介入。中断权限限定委派归属并在发送前复查；不能因“我创建过某 Agent”就停掉其中混入的用户工作。pi-wm/nightly openbot 提供了细粒度参考，但后者非商业许可且 full-access 默认不可继承。

**C5：全任务预算与准入。** 将模型请求、worker 并发、内存/磁盘及远程资源纳入可解释准入；为任务树预留预算，收集迟到 usage 后结算。取消或未知账单不立即退回全部预留，避免重启绕过额度。Paperclip 的持久预算、Kueue 的 reservation/admission 分层可借鉴，桌面端不引 Kubernetes。价格过期或 usage 不完整显示估算/未知；预算准入不等于供应商账单的绝对硬封顶。

原生交互仍围绕任务树、委派卡、进度和需要处理；用户能看到当前负责者、等待原因、受影响工作区、剩余预算和待验收项。先不用一张铺满节点的图作为唯一入口。后续增团队看板，也必须读取同一状态机。

**代码落点。** `delegation` 增依赖/claim/result 合同；host 增资源准入、workspace 身份、lease 与 worker 生命周期；board 保留协调意见，不能成为另一份执行真相。protocol 输出 typed 状态与 actions；UI 不自行起子进程、合并 patch 或推断任务完成。自动多层树、agent-to-agent 任意消息、自动替换失败 worker 和无人值守合并均后置。

#### 5.4 长期记忆：有证据、有适用期、能纠错与遗忘

**M1：把“记住一句话”拆成可治理实体。** 沿用现有 profile/fact/sessionFact/experience、pending/active、supersedes 与 forget barrier，增量补齐 subject、命题、上下文条件、适用项目/分支/环境版本、observed_at、valid_from/to，以及支持/反驳证据关系。用户原话、工具观察、模型推断和他人转述保持不同 provenance。相同原话在几个会话中反复抽取不算几条独立证据。

Graphiti 的双时间/失效关系适合表达“今天知道上周配置已变”，但模型识别 contradiction 不是事实裁判。Hy-Memory 的 facts/experiences、确定性编辑链及其CMB论文的事实—经验—diff关联适合代码经验，但 Aeroric 不继承“未传 workspace 就跨项目召回”的缺省；作用域由可信 task 身份绑定。WorldBase 的事实/证据/画像投影、遗忘优先于回滚值得采纳，明确其完整治理仍是设计。

**M2：纠正、归档、遗忘分开。** 纠正创建替代版本，保留可追溯关系；归档只退出主动召回；遗忘移除受保护载荷并设置防复活屏障。屏障检查原始源片段、命题身份、派生链和投影重建；旧版本回滚、备份恢复、Wiki/skill 再编译都不能无意复活已遗忘信息。恢复流程应先合并最新屏障，再恢复允许内容。

需明确范围：Aeroric 自己的数据库、索引、缓存、投影和可控制备份可以治理；已经外发到第三方模型、用户手动复制到别处、CLI 自有历史中的内容，不能凭本地按钮宣称全面消除。敏感原文不为“可审计”永久复制，审计记录采用最少载荷；签名摘要不被当作语义去重或彻底遗忘的完整方案。

**M3：检索按任务、时间和当前状态组织。** 在 FTS 候选生成前过滤 scope；对“当时怎么配置”和“当前怎么配置”用不同时间条件；命中记忆后回读证据和当前 supersedes/forget 状态。LongMemEval 支持 fact-enhanced keys + 原 round value，不能只留抽象事实。先验证有界词法/结构检索，召回不足再加本地可选向量通道；不要每次启动灌完整记忆库。

**M4：使用反馈不直接升级为真。** 记录 loaded/selected/actually_injected/read/cited/used/verified/user_accepted 的差别。工具返回过记忆，不证明模型实际使用；“有用”票可以排序，不能单独证明正确。跨多个独立任务的可观察 outcome 才支持适用性评估；失败也要保留，不用成功样本训练一个永远自夸的记忆系统。

**M5：经验到技能是发布流程。** 当前三次有用触发提案可保留，但提案应补适用版本、反例、前提、源证据、预计成本、可执行验收与撤回条件。候选、评估结果、用户批准、挂载版本分别存；提案者不能替自己批准。ruflo 的内容寻址 receipt/统计重算可作为可复核工件，必须明确可信验证者，签名不代替语义验收。按任务渐进加载技能目录/短说明/具体正文，记录最终实际可用版本，不把全部技能常驻系统提示。

**M6：后台整理是有界维护任务。** 沿用默认关闭；开启后仅处理用户允许的源、额度和空闲窗口。模型生成 ChangeSet，validator 检查 ID 守恒、证据、作用域、屏障及人工保护，再提交。Orbital 的整理前备份、旧 ID 缺失恢复适合防丢失，但不能证明改写后的事实正确。失败显示 partial/failed，DB commit 和投影修复分开；原始会话保留期与高层记忆保留期由产品独立定义，不照抄 MemHub 的三天 raw 窗口。

原生记忆页增加“依据、适用范围、有效时间、最近核验、替代/冲突关系”；用户直接修正与忘记仍是明确动作。技能页显示候选与已启用版本、适用项目和验证记录。画像只保留稳定、短、当前有效的内容，实际注入情况在上下文详情可查看。

**代码落点。** `memory/store` 渐进迁移证据/时间/谱系；`memory/run` 输出候选 ChangeSet 并原子记录抽取进度；`memory/skills` 管提案/版本/批准；host 负责来源读取、模型 egress 与后台额度。Wiki 引用 memory/skill ID 与 revision，不复制成另一份可独立纠错的事实。MCP 调用从 token 推导项目和权限，模型参数不能伪造 trusted origin 或扩大 scope。

#### 5.5 贯穿四类能力的实际用户流程

以一个 PyTorch/YOLO 项目升级为例，这些能力应形成一次连续工作，而不是四个相互独立的入口：

1. 用户选中训练仓库和相关论文，Wiki 先生成模块/配置/测试入口及研究摘录；原文、中文阅读投影、来源版本分开保存。没有读取的论文段落不参与“已核验”结论。
2. 开始“升级推理依赖并保持精度”任务，主会话得到本次验收、当前环境、相关经验与 source pack。曾经适用于旧 PyTorch/CUDA 的建议显示适用条件，不直接当现在的事实。
3. 只读子任务并行检查依赖兼容、导出路径和论文证据；实现阶段共享目录保持一个 writer。以后批准隔离并行后，各副本交 patch，统一检查 base/冲突再应用。训练数据、GPU、端口和外部服务仍单独授权/准入。
4. 任务中换模型，交接保留已运行命令、产物 hash、未知训练/导出结果和剩余验收。若断连发生在提交外部任务之后，先核对 job ID/产物，不重复开训练。
5. evaluator 按固定输入验证精度、延迟、资源占用及失败情况；未覆盖的数据集/硬件明确列出。模型“测试通过”的文字不能替代真实记录。
6. 完成后提议一条带环境/版本/反例的经验，用户确认；Wiki 更新相关 runbook。重复任务产生足够独立证据后才提技能候选；旧结论被纠正时，派生 Wiki 和技能进入待复核。

这个流程的“原生”体现在任务身份、授权、证据、执行和记忆全程可追踪，并能在同一套原生卡片/页面里处理失败。不是把几个参考项目的 Web UI 嵌进窗口。

#### 5.6 可选扩展与核心的边界

New-addition 中 PDF 解析、论文检索、语音、生图、图像编辑、媒体 pipeline、搜索连接器、模型服务、用量观察等，可成为 typed provider、外部工具或可选配方。它们输出 SourceRef/工件与收据，复用同一取消、预算、权限和结果展示合同。大型数据库、浏览器引擎、GPU 推理引擎、Kubernetes、CRM、社媒运营、命理等不作为四类核心的必需依赖。

模型资源按本机能力选择，下载前显示大小，下载中显示进度，完成后启用，沿用当前原生约束。one search 的候选 URL 不等于抓取证据；gateway preflight 不等于远端可用；暂时错误不等于模型已下架。big-arrow 的 overlay 不激活不代表其 targeting 不激活，不能用它宣称已解决 Aeroric 抢焦点；本次没有运行 GUI 验证。

### 6. New-addition 相比既有规划带来的增量

下表聚合新资料的设计增量。项目覆盖表在附录；“融合”表示机制建议，不表示引入其代码或额外常驻服务。

| 来源组合 | 旧规划还不够具体的地方 | 建议形成的 Aeroric 原生增量 |
|---|---|---|
| pi-wm、T3Code、CopilotKit/OpenBot、nightly openbot | 已有交接/委派，但缺交付事务、跨入口准入和中断归属 | 交接投递状态机、持久 outbox、fence、按 lane 合批与用户优先；补来源本身仍有的非原子窗口 |
| OpenMausBot、OpenShell、runmesh | “只读/权限”容易混淆支持、配置与实际强制 | driver 能力表、catalog/execute 双检查、策略 effective revision；macOS 强制边界独立验收 |
| ThinkRail、Context Hub、context-mode、lossless-claw、Engram-MCP | 文档/压缩/记忆各有入口，缺可定位上下文包与关系健康 | 来源版本、受众清单、可回读压缩、知识关系校验；不凭插件 hook 名称认定四族生效 |
| Orbital、ruflo、agentdock | 经验转技能主要靠有用票，整理还缺守恒与晋级证据 | 候选/评估/批准分权、内容寻址验证工件、ID/屏障守恒与投影修复 |
| Reprise、sample-agent-cost-bench、web-builder-harness、oops | 缺同起点质量/成本对照及保守恢复合同 | 本地评测工作台、冻结目标、污染排查、费用未知态、版本绑定验收和不覆盖后续编辑的撤销 |
| GAS、Trellis、OpenSpec、superpowers、ECC、官方插件 | 规范靠模型遵守，容易把文件/勾选框当完成 | 将真正必要的不变量做成 host validator；其余作为可选流程模板，不强制给清晰任务增加仪式 |
| BrowserSkill、CLI-Anything、搜索/解析/科研工具 | 工具入口多，但证据格式与效果边界不统一 | typed 工具/provider 合同、SourceSpan、取消与收据；评估稳定接口后再选具体实现 |
| agent-session-titles、ai-todos、用量/反馈工具 | 状态与用户命名可能被自动化覆盖 | 用户编辑所有权、可解释状态、unknown 与缺失 usage；业务完成由验收决定 |

不能只因为同叫 Engram、Pi、OpenBot 就合并。bestnite/engram 是 FSRS 闪卡/Anki；XSIME 是输入法及 Capsule/Mailbox；forkiq 是 fork 同步面板；kividb 是 Redis 兼容数据库；archify 是交互图产物。它们的有用机制分别归入可选阅读/调度/视图层，不能凭名字充作 agent 长期记忆或执行内核。

#### 6.1 应从新项目新增到设计中的三个切片

**插件/技能的可审查安装与升级。** 给现有扩展管理补稳定package ID、来源repo/subdir、commit/内容摘要、许可、安装scope、受管文件与实际权限。displayName可改，身份不随标题变化；升级先生成具体预览，绑定旧文件hash和preview digest再应用，冲突时保留用户修改。Trellis的备份/冲突恢复、AAS的不可变preview和官方插件的slug/rename映射可借鉴，AAS experimental apply不当成生产模板。批准可执行脚本时必须绑定脚本字节及其执行配置；pi-mcp-adapter只hash definition/路径的做法不能覆盖脚本原地变更。

扫描报告绑定exact payload、scanner/model/policy版本与时间；若扫描调用模型，数据外发单独说明。SAFE、官方目录、MIT和无finding都不等于执行许可。技能的model/user invocation策略与底层工具权限分开；目录/短说明/正文渐进加载。hook声明事件、schema、体积/时间预算、取消、失败策略；安全关键校验异常不能静默放行，建议型检查可降级显示。ECC部分catch exit0及可禁用路径需要作为反例保留。

**工具/provider的生命周期与真实方法。** 统一prepare/start/claim/cleanup、任务归属与收据；像BrowserSkill那样区分自建/借用/仅观察的资源，清理只处理自己拥有的部分。其内存请求表不能直接提供daemon重启恢复；Aeroric应把需要恢复的操作状态持久化。CLI-Anything提供结构化后端适配的好例子，但GIMP可回退Pillow、LibreOffice在macOS可走LaunchServices激活。工具结果需带actual engine/method、fallback、输入/输出版本及工件验证，不能把包装CLI当作真实后端或无界面保证。本用户已经失败的无打扰验收仍有效，后续未经验证的激活兜底不应进入后台路径。

**原生结果视图与显示派生物。** 从answer-me-with-html、archify、diagram-design借鉴小schema/关系视图/来源标记，由SwiftUI固定组件渲染表、任务、差异、引用和timeline；模型不通过任意HTML获取App动作权。需要HTML工件时另走受限预览，桥接只允许已授权动作。Sermo/OnlyTranslate提示了原文、显示译文、实际发给模型的文本应独立管理；不能因“只用于显示”就默认把私有原文交外部翻译。标题也应有自动字段与用户锁，agent-session-titles的逐段保护可转成原生metadata，避免自动命名覆盖人工意图。

#### 6.2 上下文/记忆新资料进一步收紧的合同

**压缩以一次执行的原结果为依据。** 从context-mode的工具端检索、rtk的命令特化过滤与exit code保留、headroom的稳定前缀借机制；为原结果保受控工件、范围/截断/期限与scope，先显示索引再按需展开。不能照搬“TTL过期就重跑命令”的恢复提示，写工具须先核对效果；成功命令原文也不能假定所有参考实现都有保存。压缩比例与任务质量各自评估，删掉关键错误行即使节省99%也算失败。

**恢复必须指定确切会话。** ai-memory把owner和接收会话校验放进claim事务值得采用；context-mode按项目最新未消费snapshot回退则不能满足“继续指定任务A”的合同。恢复包绑定源/目标session、task/attempt、owner、workspace/head、义务与receipt游标，原子claim只解决消费竞争，不能替代源身份。agent-session-bridge明确conversation-only、丢工具/运行时状态与未验模型续聊，这种损失清单比“无损迁移”宣传更应原生化。

**删除进度按副本返回，缓存按完整身份失效。** 原始行删掉、检索封锁、向量/文件/导出清理、远端同步与历史备份是不同状态；ai-memory的files_failed、claude-mem的后台Chroma清理、Jieli保留附件提供了具体反例。归属应在采集时写入，不能仅按同项目最新事件推测。graphify与Ncurator启发缓存键包含源hash、parser/prompt/model资产/参数、scope；prompt不可读、旧模型版本未知或scope收缩时不能安全复用旧结果。agent-blackbox的只读回放可用来做原生事件视图，但日志截断/缺失、元数据缓存和未知价格必须保留标记，不能补造完整因果链。

layerfs可提供expected-head/分支冲突与“提交后失效失败返回unknown”的设计启发；其当前Disposable Store、synchronous=0和明确耐久限制使它不适合做Aeroric唯一检查点权威。保持现有持久业务合同，后续若评估资源分支引擎，单独验证断电、合并、权限和外部效果，不能从文件快照推导整个Agent世界已恢复。

### 7. 分批落地与依赖顺序

不按“一整个类别做完再做下一类”施工。每批都应从 core 合同走到 host、协议、原生界面和可重复验收；迁移前备份、兼容旧记录、可回退读行为，不通过回滚复活遗忘信息。下表是建议施工包，不是工期承诺或实施授权。

| 批次 | 先决条件与范围 | 可交付用户路径 | 退出条件 |
|---|---|---|---|
| P0 状态真实性 | NR-01/02/04/06/08/11；明确持久权威/部分成功/投影 | 启动失败仍能交接、回传失败看得见、来源读取失败不删知识、提取失败不假完成 | 故障注入覆盖每个提交/发送边界；重启后无静默丢失/重复提交；旧用户编辑保留 |
| P1 权限与资源 | P0；NR-03/07/09/10，native session/workspace lease、技能隔离、能力表 | 并发启动可解释排队；四族实际可用项透明；导出不越目标边界 | 跨父/redo/路径别名/迟到回调测试；逐族假 driver 集成；真实族验收另获运行授权 |
| P2 上下文与知识 | P0/P1 的身份/版本；SourceRef/Manifest；Wiki sources/contributions/jobs | 代码+研究材料进入 Wiki，来源更新有差异和冲突，中文阅读可回原文，换模型可回读遗漏 | 删除/不可读/手改/同mtime变更；引用定位；固定任务检索和 reader 分层评测 |
| P3 受控协作 | P1 writer/admission、P2 evidence；保持现有深度/共享写默认 | 只读并行、明确依赖、冻结验收、合批回传、预算和中断归属 | 同预算单 Agent 对照有收益；claim竞态、取消/重启/超额/结果未知均可恢复 |
| P4 记忆治理与技能验证 | P0 原子写；P2 source/lineage；P3 outcome | 时间/版本约束召回、纠错传播、遗忘防复活、可核查技能提案 | 时间/冲突/权限/屏障/备份恢复对照；拒绝无独立证据的自动晋级 |
| P5 选择性增强 | P2–P4 稳定且有本机评测 | 可选向量/局部图、独立副本并行、自动整理建议、科研工具配方 | 与简单基线比较质量/时延/费用；收益不足可关闭；涉及 Q-B10 等变化先单独审批 |

P0/P1 应拆成小的行为修复和合同笔记；本篇只是设计研究，不能用一份大路线评审掩盖未来跨模块实施的具体取舍。P2 的来源治理可以先支持 notes/现有经验，再增源码和 PDF，避免首次就支持所有格式。P4 的主体/时间 schema 可提前设计，但不阻塞 P0 的可靠性修复。

### 8. 本地评测与“前沿能力”的可证伪标准

优先建立固定起点、隔离数据根、fake driver 的可靠性测试，再用明确授权的真实模型任务评估效果。记录项目/输入 hash、模型/driver/提示与策略版本、冷/热缓存、预算、重复次数、失败样本和价格来源。将最终产物/答案与预任务起点隔开，避免 Reprise 指出的答案污染；不要为重跑而覆盖用户工作树。

| 维度 | 最小实验 | 主要指标/失败判据 |
|---|---|---|
| 交接/恢复 | 启动前后、发送前后、确认前后断连/崩溃；重复请求；旧attempt迟到 | 已提交/已确认/未知区分正确；义务守恒；已确认交付不重复；不确定效果不盲重放 |
| 写入与投影 | SQLite/文件写失败、磁盘满、CAS冲突、进程被中断、索引重建 | 成功/部分失败准确；DB权威不损坏；投影可重建；用户编辑与屏障不丢 |
| 权限与隔离 | 两项目、不同父、symlink别名、不同home、过期token、catalog缓存 | 未授权来源/工具/技能不可见且不可调用；不支持的强制限制明确拒绝 |
| 知识检索 | 中文/英文、符号名、多文档、时效/删除/不可读、原文与译文 | 召回@k、来源定位准确率、过期贡献率、读预算遗漏；检索与生成分别计分 |
| 记忆 | 当前/历史事实、矛盾更正、跨项目同名、同源重复、遗忘后重抽/恢复；CMB式交错任务恢复与代码测试 | scope误召回、时间选错、无证据断言、防复活漏检、适用版本误用 |
| 协作 | 单Agent、只读并行、咨询/manager、后续隔离并行，同预算对照 | 成功率、真实墙钟、总token/费用、协调开销、过度/不足委派、人工介入次数 |
| 技能 | 无技能/当前技能/候选技能对照，独立任务留出集 | 成功/回归/适用性；使用次数不代正确；候选不能自动替代已批准版本 |

统计先按 task 聚合，再对 task 做成对比较/重采样；同任务多次运行不直接当独立任务扩大显著性。展示 pass@1、至少一次成功和全部一致成功时注明重复次数及定义；同时保留每次失败与未知费用。硬门禁针对确定性不变量；模型效果报告置信区间/样本范围，不以某篇论文或排行榜的相对提升设置生产承诺。

模型 judge 可做离线筛查，但需人工标注小样本校准及独立可观察结果；MAST 标签不自动阻断用户任务。科学/训练/推理任务使用具体精度、性能、导出兼容性和产物验证，界面视觉效果仍由用户在签名构建上验收。GUI/VM 现阶段按项目规则 NOT RUN，文档校验不能替代这些验收。

### 9. 后续真正需要用户拍板的取舍

目前没有问题阻塞这份研究；以下只为后续实现评审保留，不在本轮推翻已批准决定。

| 取舍 | 建议默认 | 何时需要再讨论 |
|---|---|---|
| 是否允许多个可写 worker | 共享目录继续单writer，只读可并行 | 有隔离副本/外部资源配额和可靠合并后，单独评审变更 Q-B10 |
| 资料是否可外发分析 | 先本地解析/检索，明确选择模型和来源范围 | 增加后台 synthesis/embedding 或跨提供商上下文时 |
| 自动整理/技能晋级 | 整理默认关；模型提案、用户批准 | 留出集和回归/撤回机制完成后，才讨论低风险自动接受范围 |
| 磁盘 Markdown 是否双向编辑 | DB权威、导出/显式导入 | 用户明确需要 Obsidian 双向工作流后设计冲突与删除语义 |
| 记忆、原文和备份保留期 | 延续已批准行为，按源类别区分 | 引入外部材料缓存、备份恢复或清理策略时 |
| 图/向量/本地模型是否常驻 | 可选按需，不增默认常驻服务 | 固定基准证明简单检索不足，并确认资源/下载成本后 |

## Alternatives considered

1. **直接集成一个成熟平台的全部运行时。** 最强理由是能较快获得现成界面、插件和连接器，减少从零组合的工作。未选为主路线：参考平台的权威存储、模型循环、权限和生命周期会与 Aeroric 四族/host 并列，许可与部署成本也不同。保留 typed 工具/provider 集成，核心合同独立实现。
2. **全部用 Markdown + hooks + 技能约定。** 最强理由是透明、可人工修改、迁移方便，用户已积累的仓库也最容易复用。未选为运行权威：文件存在/模型承诺无法确保投递、并发、权限、原子性和恢复，hook还有绕过/失效边界。保留 Markdown 作为来源、可审查提案和可修复投影，关键不变量由 host 校验。
3. **先上全量向量/图数据库与自动多 Agent。** 最强理由是跨文档关联与并行探索潜力大，多个论文在对应任务上显示收益。未选为首批：现有丢交付/误状态/作用域问题会被放大，图构建、模型调用和维护成本难以归因；论文也未证明普适胜出。保留可插拔检索及局部图，用固定任务证明增益再扩大。
4. **只修已有功能，不扩展知识和记忆。** 最强理由是范围最小、可靠性收益直接，短期风险低。未选为完整结论：用户明确需要四类原生能力，新材料也提供了来源贡献、可回读压缩和验证晋级等实际增量。因此前两批聚焦可靠性，后续通过纵向切片扩展，而不是永久停在修补阶段。
5. **照抄论文拓扑/门槛作为默认策略。** 最强理由是可以快速得到貌似客观的自动选路规则。未选：模型、任务、样本、指标与预算条件不同，一些关键统计并不稳健。使用同预算本地基线、可解释启发式和用户选择，后续再评估策略学习。

## Acceptance criteria

本次文档交付的验收：

- 覆盖正式项目当前真实调用链、两份规划全文、182份参考副本/177来源、21PDF及评论；每项区分入口/源码/测试条目/论文实读范围，不宣称整仓或全部542页逐字审计；递归PDF清单单独披露未读材料。
- 对旧规划给出保留、不同意见、论据纠正和新增；四类能力各有用户路径、当前代码接入点、失败边界、依赖和可执行验收。
- 关键结论有本地来源与版本/页码，可在本篇及持久附件追踪；不以 `/tmp` 为唯一交付。
- 两份原文的 SHA256 与初始 baseline 一致；产品代码不由本次修改；现存用户改动保留。报告通过 `pnpm verify-notes`，本次链接/覆盖检查结果有记录。

后续产品实施的共同门槛：先通过 P0/P1 的故障注入、作用域和权限验收；任何新增自动化不能把 unknown 变 success；没有 source/版本/适用条件的推断不能自动升为事实；验收通过绑定具体目标工件；公开产品宣称与已验证 driver/平台一致。真实模型、GUI与性能效果需要另有执行证据，本篇不替代。

## Risks

- **研究广度与深度不同。** 全项目覆盖不等于全源码审计；文档级项目只作为候选，重点机制也未运行。路线中的可靠性风险是静态发现，需后续复现；论文效果不能直接外推本机产品。
- **工作树与来源会变化。** 本篇以2026-11-02本地HEAD及正式工作树为快照；行号可能漂移，HEAD和函数/页名用于重新定位。未联网验证最新上游状态，引用不代表持续维护承诺。
- **统一合同过度抽象。** 若SourceRef/Receipt变成万能事件平台，会拖慢交付。只抽共享语义，各业务保留owner，先做一条真实用户路径。
- **状态数量与原生体验。** 更准确的内部状态可能造成界面负担；默认只展示用户能行动的状态，完整时序放详情。不能靠隐藏unknown让界面显得简单。
- **迁移、并发与遗忘。** 多版本记录、投影和备份可能复活旧信息或覆盖人工编辑；需要屏障优先、CAS、迁移/恢复演练。跨文件与DB不能伪装成一个原子事务。
- **隐私与提示注入。** 文档/网页/论文/项目规则均是有来源的数据；被检索内容不能提升工具权限或覆盖用户指令。外发需沿用用户授权范围，凭据不进入来源快照/trace。
- **许可与供应链。** 本地项目包含GPL/AGPL/LGPL、PolyForm、限制条款、商业闭源入口和无LICENSE项目；可讨论机制不意味着可直接复制/打包。下游实施前按具体文件/依赖核许可，宽松项目也保留notice。
- **成本与效果不可保证。** 并行、向量、图和整理会增加维护与模型开销；按配置可关、保留简单基线。供应商usage不完整与迟到计费只能如实标注，不能承诺绝对预算封顶。

## 研究附录与证据

以下附录是本次研究的组成部分。来源路径均为本机快照；表中的测试表示看过测试条目/断言，不表示执行通过。

### A. 既有笔记的归属审计

本篇部分重叠于下列已实施决定，但不吸收、改写、归档或翻转它们：

| 既有笔记 | 归属与本篇关系 |
|---|---|
| [native boundary](../../implemented/architecture/2026-09-21-native-boundary-and-single-writer.md)、[data-dir writer](../../implemented/architecture/2026-09-22-data-dir-writer-eligibility.md) | 继续约束Rust业务/单数据根writer；本篇补区分workspace和native-session资源租约 |
| [structured protocols](../../implemented/architecture/2026-10-25-structured-agent-protocols.md)、[protocol version](../../implemented/architecture/2026-09-22-protocol-version-boundary.md) | 继续约束四族/typed wire；本篇提出后续能力与收据扩展，未变更协议 |
| [handoff/context](../../implemented/feature/2026-10-30-handoff-context.md) | 保留checkpoint/用户批准；补两阶段交付、遗漏清单与论文论据纠偏 |
| [delegation](../../implemented/feature/2026-10-31-delegation.md)、[board/probes](../../implemented/feature/2026-10-31-board-probes.md) | 保留现有角色/权限/默认深度和探针；补跨父准入、outbox和后续依赖图 |
| [memory store](../../implemented/feature/2026-10-31-memory-store.md)、[extract](../../implemented/feature/2026-10-31-memory-extract.md)、[experience](../../implemented/feature/2026-10-31-memory-experience.md)、[skills](../../implemented/feature/2026-10-31-memory-skills.md) | 保留用户记住/候选/经验/技能版本闭环；补失败原子性、证据独立性、时间和隔离 |
| [Wiki](../../implemented/feature/2026-10-31-wiki-skeleton.md) | 已有确定性知识切片；本篇扩来源/贡献/健康，不把它重新列为空白功能 |
| [本篇保护的两份规划](../../../../docs/frontier-roadmap.md) | 独立评审，不使其Q系列批准失效，不修改原文 |

已检索 active proposed/implemented 中同主题笔记；本篇是跨资料研究的新提案，未发现需要完整吸收的另一份独立同主题提案。没有为清理而删除任何已有笔记。互链在本篇单向登记，原笔记保留原状。

### B. 知识库与长期记忆六项目的源码证据

| index / 来源 | HEAD / 本地许可 | 阅读深度及结论 |
|---|---|---|
| 27 Tencent/WeKnora | `40192ace214c`；MIT，含第三方许可 | README/Lite、挂起恢复设计、Wiki读写/权限/预算测试；借来源授权、持久挂起和多页预算；不引整套企业平台 |
| 28 infiniflow/ragflow | `2fc8373d4cca`；Apache-2.0 | 知识编译文档、Go编译/merge/dirty/evidence；借增量贡献和job revision；图/union不保证删除或事实真值 |
| 29 Tencent-Hunyuan/Hy-Memory | `988667cd876f`；Apache-2.0 | MemHub架构、coding场景、chain_store与scope测试；借SDK权威/事实经验/编辑链；不继承宽scope缺省 |
| 30 getzep/graphiti | `72b5c59c0660`；Apache-2.0 | README、edge模型、失效/矛盾操作与测试；借双时间和谱系；不为此引Neo4j或已deprecated Kuzu |
| 31 RZfive/WorldBase | `da96914a8186`；Apache-2.0 | 治理设计主要章节、当前delete/tombstone；借事实证据/画像分离和forget屏障；完整新增治理尚未实现 |
| 32 mem0ai/mem0 | `26cdefb46600`；Apache-2.0 | README新算法说明、抽取/搜索/更新删除与identity过滤；借有界抽取/混合候选/身份绑定；托管数字不当本地OSS成绩 |

**WeKnora。** [Lite](/Users/macbook/Downloads/同步空间/LYX/知识库/WeKnora/docs/LITE.md) 支持单应用SQLite与内存队列、零外部服务，故不认可旧路线把它一概视为重部署。挂起设计在 [限制:17](/Users/macbook/Downloads/同步空间/LYX/知识库/WeKnora/docs/design/agent-hitl-suspend-resume.md:17) 明示只能在外部调用前等可持久边界恢复，进行中的连接不能序列化；[CAS/lease:213](/Users/macbook/Downloads/同步空间/LYX/知识库/WeKnora/docs/design/agent-hitl-suspend-resume.md:213) 描述 `answered → executing → done`，未知效果不重放，不能外推所有入口都可挂起。

[wiki_write_page:92](/Users/macbook/Downloads/同步空间/LYX/知识库/WeKnora/internal/agent/tools/wiki_write_page.go:92) 先对source授权，再检查目标KB路由与页类型；省略source_refs和显式空数组语义不同，Agent不能自行创建system summary。[预算测试:49](/Users/macbook/Downloads/同步空间/LYX/知识库/WeKnora/internal/agent/tools/wiki_read_page_budget_test.go:49) 等覆盖多页预算、公示遗漏、不静默丢中间页；[link mutation:38](/Users/macbook/Downloads/同步空间/LYX/知识库/WeKnora/internal/agent/tools/wiki_link_mutation_test.go:38) 有失败补偿检查。借的是授权和结果合同，测试本次NOT RUN。

**ragflow。** [知识编译概览](/Users/macbook/Downloads/同步空间/LYX/知识库/ragflow/docs/guides/knowledge_compilation/overview.md) 给出Parser/Chunker/Compiler/Indexer与Graph/Tree/PageIndex/Mindmap/Timeline/Wiki六类产物，当前主干为Go，legacy Python不是当前接入建议。文档级产物与Dataset Wiki合并分离，暂时缺某产物不必然等于整次失败。[wiki_merge:26](/Users/macbook/Downloads/同步空间/LYX/知识库/ragflow/internal/ingestion/knowledge_compile/wiki_merge.go:26) 做稳定页ID/Markdown/source IDs union，不能由此推断能准确撤回已删源贡献。[wiki_dirty:37](/Users/macbook/Downloads/同步空间/LYX/知识库/ragflow/internal/ingestion/knowledge_compile/wiki_dirty.go:37) 提供20秒debounce、revision与lease/CAS的持久增量工作机制。

[evidence:12](/Users/macbook/Downloads/同步空间/LYX/知识库/ragflow/internal/ingestion/component/knowledge_compiler/structure/evidence.go:12) 默认soft evidence gate，原始字节offset处理中文定位；找到quote只能证明字面出现，不能自动证明语义蕴涵。Aeroric应把可定位与已核验分开。

**Hy-Memory/MemHub。** [ARCHITECTURE](/Users/macbook/Downloads/同步空间/LYX/agent-memory/Hy-Memory/memhub/docs/ARCHITECTURE.md) 把SDK设为数据面唯一权威，管理面不镜像记忆。四类授权是源读取、同步、模型外发、host召回，不是逐memory跨Harness ACL；默认能跨Harness搜索。一次job对SDK只做一次add，queue/outcome分工；raw只保真人user/assistant三天、tool不进raw，高层记忆不受此窗口约束，不能混写为“三天后全部忘记”。

[coding场景](/Users/macbook/Downloads/同步空间/LYX/agent-memory/Hy-Memory/docs/scene_coding_usage.md) 使用facts/experiences与workspace/branch；不传workspace不自动隔离，branch还保NULL历史。[chain_store:32](/Users/macbook/Downloads/同步空间/LYX/agent-memory/Hy-Memory/hy_memory/coding/chain_store.py:32) 以user+workspace为owner，后续确定性diff/编辑轨迹值得借鉴；observe seq不是因果证明。chain默认关闭且best-effort。[scope测试](/Users/macbook/Downloads/同步空间/LYX/agent-memory/Hy-Memory/tests/coding/test_search_scope_filter.py) 提供检索过滤验证意图，本次未执行。

**Graphiti。** [edges:267](/Users/macbook/Downloads/同步空间/LYX/agent-memory/graphiti/graphiti_core/edges.py:267) 包含episode、expired_at、valid_at、invalid_at；[失效操作:433](/Users/macbook/Downloads/同步空间/LYX/agent-memory/graphiti/graphiti_core/utils/maintenance/edge_operations.py:433)、同文件:634/:722 描述invalidated_by、双时间和contradiction处理，相关 [测试](/Users/macbook/Downloads/同步空间/LYX/agent-memory/graphiti/tests/utils/maintenance/test_edge_operations.py) 有same_pair等过滤。确定性的时间运算与模型提议的矛盾分开；“被图记为矛盾”不是事实判决。Aeroric先映射到SQLite关系，不需要为借这个机制采用完整图服务。

**mem0。** [README](/Users/macbook/Downloads/同步空间/LYX/agent-memory/mem0/README.md) 新算法数字92.5/94.4明确来自含专有优化的托管平台。当前 [main.py:881](/Users/macbook/Downloads/同步空间/LYX/agent-memory/mem0/mem0/memory/main.py:881) 单次LLM抽取ADD-only、batch embedding/hash去重；LLM错误抛出而非伪装空结果。identity先构建scope，metadata不能伪造identity；[search:1393](/Users/macbook/Downloads/同步空间/LYX/agent-memory/mem0/mem0/memory/main.py:1393)、[混合候选:1642](/Users/macbook/Downloads/同步空间/LYX/agent-memory/mem0/mem0/memory/main.py:1642) 可对照词法/entity/semantic分支。update/delete仍存在，2025论文还是ADD/UPDATE/DELETE/NOOP，不能拿旧论文证明新路径普适更优。

**WorldBase。** [治理设计:4](/Users/macbook/Downloads/同步空间/LYX/agent-memory/WorldBase/docs/memory-user-model-and-consolidation-design.md:4) 明示新增治理模块尚未实现。fact/evidence/profile分离、独立证据、投影重编、forget先于回滚、有效与记录时间值得借鉴。当前 [delete:1930](/Users/macbook/Downloads/同步空间/LYX/agent-memory/WorldBase/harness-rs/crates/memory/src/lib.rs:1930)、同文件:1943/:1962 有硬删、tombstone和forget事件，但不等于第三方、聊天历史和备份彻底遗忘。AI整理可生成并应用计划也不等于已经实现完整独立审批维护器。

### C. 论文与网友材料逐篇覆盖及证据

页码均指本地PDF物理页。A=有给定条件的对照实验；B=小样本或产业经验；C=概念/架构建议；A也不是生产保证。正文提取与关键数字复核不等于全页图像视觉QA。

| 材料（文件） | 年份/版本；页数 | 实读覆盖与剩余限制 | 等级 |
|---|---|---|---|
| [AOrchestra_Automating-Sub-Agent-Creation_arXiv-2602.03786.pdf](/Users/macbook/Downloads/同步空间/LYX/论文/AOrchestra_Automating-Sub-Agent-Creation_arXiv-2602.03786.pdf) | 2026 v2；26 | 主文1–9、实验细节11、关键prompt12–13；其余附录prompt未逐字读 | A |
| [Cognition_Multi-Agents-Whats-Actually-Working.pdf / .html](/Users/macbook/Downloads/同步空间/LYX/论文/Cognition_Multi-Agents-Whats-Actually-Working.pdf) | 2026-04-22；11 | HTML正文完整，PDF首页及文字核对；重复来源 | B |
| [MS-Eval_Sub-Agent-Delegation-and-Orchestration_arXiv-2610.22869.pdf](/Users/macbook/Downloads/同步空间/LYX/论文/MS-Eval_Sub-Agent-Delegation-and-Orchestration_arXiv-2610.22869.pdf) | 2026 v1；48 | 主文1–10、关键metrics/config附录16–18；其余长prompt/case未逐字读 | A |
| [Intelligent-AI-Delegation_arXiv-2602.11865.pdf](/Users/macbook/Downloads/同步空间/LYX/论文/Intelligent-AI-Delegation_arXiv-2602.11865.pdf) | 2026 v1；42 | 首页、框架/委派/权限/验证8–20完整，21–28关键词上下文及结论；大量背景/参考文献未逐字读 | C |
| [Towards-a-Science-of-Scaling-Agent-Systems_arXiv-2512.08296.pdf](/Users/macbook/Downloads/同步空间/LYX/论文/Towards-a-Science-of-Scaling-Agent-Systems_arXiv-2512.08296.pdf) | 初稿2025，v3 2026-04-08；44 | 主文1–27、关键统计/配置附录41–44；28–40参考/其他附录未全读 | A |
| [What-Prompts-Dont-Say_Underspecification_arXiv-2505.13360.pdf](/Users/macbook/Downloads/同步空间/LYX/论文/What-Prompts-Dont-Say_Underspecification_arXiv-2505.13360.pdf) | 初稿2025，v3；30 | 首页、主文3–9完整，10参考；其他附录未逐字读 | A |
| [Context-Folding_Scaling-Long-Horizon-LLM-Agent_arXiv-2510.11967.pdf](/Users/macbook/Downloads/同步空间/LYX/论文/Context-Folding_Scaling-Long-Horizon-LLM-Agent_arXiv-2510.11967.pdf) | 2025 v1；22 | 主文1–11完整，实现/算法15–16，工具定义21–22；其他prompt未逐字读 | A |
| [Facts-Without-Rules_Boundary-Metadata-Collapse_arXiv-2608.29028.pdf](/Users/macbook/Downloads/同步空间/LYX/论文/Facts-Without-Rules_Boundary-Metadata-Collapse_arXiv-2608.29028.pdf) | 2026 v1；23 | 主文2–10、关键设计/allowlist/外部验证附录14–23；部分附录表为定向摘读 | A |
| [Handoff-Hallucinations_Taxonomy-Benchmark-Mitigation_Springer-2026.pdf](/Users/macbook/Downloads/同步空间/LYX/论文/Handoff-Hallucinations_Taxonomy-Benchmark-Mitigation_Springer-2026.pdf) | 2026；16 | 主文1–14，15–16参考未细读 | B |
| [LangChain_Organizing-Context-in-a-Multi-Agent-Harness.pdf / .html](/Users/macbook/Downloads/同步空间/LYX/论文/LangChain_Organizing-Context-in-a-Multi-Agent-Harness.pdf) | 2026-09-08；8 | PDF1–8完整，HTML提取核对；重复来源 | B |
| [Do-Not-Restart_Residual-Completion-for-Handoffs_arXiv-2609.13800.pdf](/Users/macbook/Downloads/同步空间/LYX/论文/Do-Not-Restart_Residual-Completion-for-Handoffs_arXiv-2609.13800.pdf) | 2026 v4；11 | 主文1–9完整，10–11参考未细读 | A（受mock环境/前提限制） |
| [Grounding-Gaps-in-Language-Model-Generations_arXiv-2311.09144.pdf](/Users/macbook/Downloads/同步空间/LYX/论文/Grounding-Gaps-in-Language-Model-Generations_arXiv-2311.09144.pdf) | 初稿2023，修订2024-04；18 | 主文2–10核心完整，附录未逐字读 | A |
| [MAST_Why-Do-Multi-Agent-LLM-Systems-Fail_arXiv-2503.13657.pdf](/Users/macbook/Downloads/同步空间/LYX/论文/MAST_Why-Do-Multi-Agent-LLM-Systems-Fail_arXiv-2503.13657.pdf) | 2025 v3；47 | 主文3–10、taxonomy17–18、干预/边界25–29；其他case/prompt未逐字读 | A |
| [2410.10813v2.pdf（LongMemEval）](/Users/macbook/Downloads/同步空间/LYX/agent-memory/2410.10813v2.pdf) | ICLR2025，修订2025-03；28 | 主文2–10、伦理11完整；其余附录未逐字读 | A |
| [2502.14802v2.pdf（HippoRAG 2）](/Users/macbook/Downloads/同步空间/LYX/agent-memory/2502.14802v2.pdf) | ICML2025，v2 2025-06-19；19 | 1–9核心、15–19案例/资源/超参；参考/部分附录未逐字读 | A |
| [2504.19413v1.pdf（Mem0）](/Users/macbook/Downloads/同步空间/LYX/agent-memory/2504.19413v1.pdf) | 2025-04 v1；23 | 3–15方法/实验/讨论完整；其余附录未逐字读 | A |
| [2502.12110v11.pdf（A-Mem）](/Users/macbook/Downloads/同步空间/LYX/agent-memory/2502.12110v11.pdf) | 2025 v11；28 | 3–9全部方法/实验/局限；其余附录未逐字读 | A |
| [2404.16130v2.pdf（GraphRAG）](/Users/macbook/Downloads/同步空间/LYX/agent-memory/2404.16130v2.pdf) | 初稿2024，修订2025-02；26 | 4–12方法/实验/局限完整；1–3开篇及附录未全读 | A |
| [2410.05779v3.pdf（LightRAG）](/Users/macbook/Downloads/同步空间/LYX/agent-memory/2410.05779v3.pdf) | 初稿2024，v3 2025-04-28；16 | 1–10正文完整；11–16参考/prompt附录未逐字读 | A（judge偏好指标） |
| [2507.03724v4.pdf（MemOS）](/Users/macbook/Downloads/同步空间/LYX/agent-memory/2507.03724v4.pdf) | v4 2025-12-03；37 | 开篇1–9提取/定向读、核心13–18/20–28、结论31；10–12/19/29–30未完整逐字读，32–37参考 | A（具体评测）+C（完整治理愿景） |
| Obsidian网友方案及评论 | 第四版及旧版；265行 | 第四版、旧版核心差异、评论240–265及130–164完整核对 | B/C，不能升格为实验共识 |

| [CMB / TriLink](/Users/macbook/Downloads/同步空间/LYX/agent-memory/Hy-Memory/docs/papers/CMB_A_Coding_Memory_Benchmark_for_Long_Horizon_and_Interleaved_Coding_Agents.pdf) | 本地稿；19页，未联网核版本日期 | 主文1–9、10声明、17–19实现附录；13–16表格定向，11–12参考未逐字读 | A，固定运行、合成历史，未复现 |


#### C.1 多Agent、交接与上下文逐篇证据


##### 论文 3.1 Towards a Science of Scaling Agent Systems（SCALE）

- 机制与条件：PDF3/10/12，260 configurations、6任务基准、5拓扑、9个旧模型，主团队3–4agent、固定总推理预算，平均4800 reasoning tokens/trial。PDF41 SWE/Terminal每配置仅20题，其他50–100。PDF43 Table16每格成功率CI常约±20pp。
- 原始结果：PDF13 Finance centralized .631 vs SAS .349，+80.8%相对/+28.2pp；PlanCraft independent .170 vs .568，−70.0%相对/−39.8pp。PDF14跨6bench总体MAS平均变化−0.3%，95%CI[−58.7%,+77.2%]，异质性很大。SWE/Terminal部分多agent拓扑退步，不能只选Finance报喜。
- 指标核准：PDF8任务级error amplification=(1−P_MAS)/(1−P_SAS)，约1.1–1.3；trace级4–17衡量传播/额外工作。PDF20 Table5的17.2/4.4为按架构赋的常数，跨benchmark套用；不是逐任务错误概率实测倍数。PDF18 error main effect p=.658、×tools p=.332均不显著。
- 45%：PDF19由标准化回归系数比值反标准化得到，PDF25–27限定已知任务regimes。PDF43 baseline×agents naive p=.004→cluster-robust .105，Holm .084；efficiency×tools .002→.205（Holm .026）；single baseline .001→.004（Holm .018）。多个“规律”仅方向性。87%是域内held-out配置架构选择，不是新任务域成功预测；LODO绝对成功率泛化弱。
- 局限：未逐模型调prompt、任务/样本有限；异构仅13配置初步；不覆盖真实长期依赖/多模态/生产反馈。PDF43 Table16模型清单与主文模型数量、PDF22/PDF42异构anchor存在口径不一致，勿包装为普适law。
- 可迁移：同模型单agent基线、固定预算比较、协调开销与任务并行度测量、拓扑选择试验；不迁移45%硬门槛或全局单writer定理。

##### 论文 3.2 Cognition: Multi-Agents: What's Actually Working

- 产业经验（HTML标题2026-04-22，同文PDF）：clean reviewer、Smart Friend、manager→children→synthesis三类；不是穷尽分类或随机对照研究。
- clean reviewer平均2bugs/PR、约58%severe为作者生产报告，无公开随机对照/样本口径，不当普遍收益。
- Smart Friend明确弱SWE1.5主控不知何时/如何问Sonnet4.5，存在quality ceiling；SWE1.6改善但未达预期。有效更接近frontier↔frontier互补；路线写“弱主控问强已有效”错误。
- full-context fork是80/20工程经验；会要求主控补看缺文件。swarm mostly distraction、single writer均为工程判断，未证明所有场景不能并行写。
- 可迁移：独立reviewer、受约束的咨询工具、主会话承担协调、源证据可补取；不能省掉本地收益/成本评估。

##### 论文 3.3 MS-Eval

- PDF3–7：166 Toolathlon-Gym tasks/8 MCP工作域，Kimi-Code单harness，禁内置skills；人工标77“不需要”/89“需要”委派（parallel/offload gold prior）。Gen、Spec、Plan-1st、Oracle，variant对比只在DS-V4-Flash0731。
- PDF7 Spec MSQ20.8 vsGen11.6/Plan17.6，但Spec ODR57.1%、Plan61%，仍显著过度委派。Oracle F1hit92.5%，quality utility49.6（50平手）、eff53.5、cost23.6，最优拆法也不必然提高质量。
- 同页八模型Spec：Sol ODR93.5%，utility performance42.6/eff44.0/cost27.4；KimiK3是唯一三项>50。不能推成“专用agent对任何模型最佳”。
- PDF8–9高收益里48.8%是未委派主agent行为改变，不能归功并行；强闭源本设置成本高/质量步数差不等于所有版本的固有性质。
- PDF16–17 CSteps是critical path tool-step代理，非真实墙钟；API token价格估成本；效率/成本utility在成功subset计算；MSQ=F1hit×三utility均值，gold拆法偏好进入评分。
- 可迁移：over/under delegation、真实墙钟、成本、成功质量联合测；只读探索比共享编辑更易并行。限制：单harness、无skills、合成环境、模型/prompt/价格快照。

##### 论文 3.4 Intelligent AI Delegation

- PDF8–20/28为规范性框架，无自身基准或消融实验证明收益。五要求：动态评估、适应执行、透明审计、市场协调、系统韧性。
- PDF9 contract-first：目标能被验收才委派，太主观/复杂继续拆；PDF12明确协议/验证有成本，低关键性、确定且短的任务可直接执行，并非所有任务都要厚合同/用户批准。
- PDF14–19过程/结果监测、black/white-box、传递责任、权限衰减、风险/可逆性区分，permission不是简单“拿到工具即可全权”。低风险standing permissions与高风险JIT/必要时人工批准均讨论。
- 密码学证明证明特定计算按输入运行，不证明抽取的需求完整、外部事实为真。签名能证明主体/完整性，不能证明签名陈述的语义正确。论文自己指出验证并非无误。
- PDF28 checkpoint schema/A2A extensions只是illustrative，并非最终标准。市场、区块链、zk等对Aeroric本地单用户产品并非必要依赖。
- 可迁移：轻量delegate spec、有限权限、结果证据、预算/取消/恢复、适度反馈；不过度迁移agent economy/强制审批。

##### 论文 3.5 AOrchestra

- PDF3–5 I/C/T/M四元组；orchestrator只有Delegate/Finish，不直接环境动作，executor给精选上下文和最小工具，返回summary/artifacts/errors。是策略接口，不证明动态agent必优静态模板。
- PDF6/11：GAIA165 validation，Terminal2 89中随机70，SWEVerified500中随机100；max10delegations×50substeps vsbaseline500。主控Gemini3Flash固定，executor模型变化；弱Qwen3-8B用约2k Gemini轨迹SFT2epochs；ICL每轮6trajectory共5轮优化。不是全500 SWE测试。
- PDF7 curated context ablation50GAIA：NoContext86/Full84/Curated96；说明不能“续做必full fork”，不是各产品通用收益。PDF8混合routing72.12→75.15、均cost .70→.57，SFT56.97→68.48但cost .36→.68，质量/成本有tradeoff。
- PDF6–9文本有明显数字口误：Terminal52.86 vs34.29绝对18.57pp却写64.29pp；摘要/结论16.28%与正文22.13%平均提升口径不同。引用原始格即可，避免复述宣传数。
- PDF12–13 Terminal prompt每次fresh container丢前次工作，要重演有效步骤；与CFRC保留实时状态的交接条件不同。
- 可迁移：把上下文/工具/模型作为显式委派输入、精选source pack、错误返回结构；不直接复制最大10子代理或全动态agent工厂。

##### 论文 3.6 What Prompts Don’t Say: Underspecification

- PDF3–4：code explanation/trip/productgen三任务、60要求、每任务200例、15/35/50split；script+LLM validators，human agreement95.6%，每prompt10要求。
- PDF5 2×=模型族升级后accuracy降>20%的requirement比例，隐式5.9% vs显式3.0%；多数升级稳或改善，22.9%case regression，不能变成所有换模型回归概率翻倍。
- PDF6 19要求全塞prompt，GPT4o均85%、Llama79.7 vs单要求98.7%。因此“穷尽要求+每次阻塞确认”也有成本/干扰。
- PDF7 COPRO-R逐要求validator+Bayesian挑子集，改善效果同时减少prompt token；PDF9仅60要求/240 synthetic prompts、judge偏差、无真实workflow用户研究。
- 可迁移：需求登记、按风险选显式约束、模型切换回归集和验收；不强制所有任务grill。

##### 论文 3.7 Grounding Gaps

- 2023/2024旧模型研究，PDF2–10：教育TSCC、情绪ESConv、劝捐三个英语域，各100dialogs；GPT4 fewshot acts classifier macroF1 .89、人标κ .72。不是当前coding-agent研究。
- PDF5–8 GPT3.5几乎不clarify，GPT4也低于人；Mistral7B SFT不改善、DPO更差，训练语料追问少/被偏好压制。但不能推当前所有模型固有不澄清。
- PDF6/8 mitigation followup10.76→42.32%，人27.87%；clarification κ反而负，单纯命令“多问问题”造成过度澄清。
- 限制：英语、部分speech acts、crowdworker、classifier域适用性。可迁移：只在重要歧义不可本地解除时问、追问质量而非数量。

##### 论文 3.8 Context Folding

- PDF3–5 branch(description,prompt)/return(message)：分支过程退出主活跃context，只留结果；同前缀KV回滚；主要主/子交错串行，执行态禁嵌套分支，不是平行swarm。
- FoldGRPO包含主线超窗50%惩罚、out-of-scope（GPT5nano judge）、failed tool penalty。PDF6 Seed-OSS36B、RL50steps约2epochs、32K active×最多10branches、BC680train/150test、SWE740train/500Verified。
- PDF7原始格（BC/SWE）：untrained fold42.0/49.2；32K ReAct28.6/43.6；summary38.6/48.8；327K长ReAct47.8/55.2；fold+standardGRPO56.7/56.4；fold+FoldGRPO62.0/58.0。
- 因此branch/return可无训练做原型，但无训练fold低于长窗ReAct，不能把训练后最佳质量赋给UI折叠。PDF8–9约7752/8885主线tokens、完成率 .935/.962衡量active main context，不是总token省90%；SWE工具调用96.5次反而增多。
- PDF10–11 parallel61.33% vsserial62%，读页110 vs80，不证明并行更优。PDF15 baseline prompt本身BC从.08提高.478，prompt敏感。
- 原生落点：可展开source-backed结果卡、活跃context预算、branch生命周期/恢复/权限；必须实测质量非劣、总费用/缓存与证据回看成本。

##### 论文 3.9 MAST

- PDF4–6 1642traces/7framework/4model families；初150traces/5系统grounded theory，6专家20h+；14类/3大组，人IAAκ .88，OODκ .79。
- PDF6 o1 fewshot annotator accuracy .94/recall .77/precision .833/F1 .80/κ .77，不能当确定性在线运行时判据。
- PDF8 Fig4 210traces的设计41.8%、交互36.9%、验证21.3%。跨系统任务不同，不宜按图排名；不支持“多数在交接边界”。
- PDF17–18：交互失配含reset、不澄清、跑题、withhold、忽略、推理动作不一致；验证含早停/缺验证/错验证。可用人工复盘分类，不同label可共存。
- PDF26–27干预200 GSM-Plus题×6repeats，GPT4 improved prompt84.75→89.75，而新topology85.50且p=.4；GPT4o新topology84.25→88.83 p=.03。ChatDev ProgramDev32题25→34.4/40.6，HumanEval89.6→90.3/91.5，有限小增益且依赖模型。
- PDF29成功trace也有verification错误，label不是task failure等价物。结构策略是开放研究方向，不是已证治理方案。

##### 论文 3.10 Handoff Hallucinations

- PDF6–7 taxonomy：I原发送者推理就错；IIhandoff变换新错；III压缩去掉hedges/uncertainty（事实可能没错但认知强度被强化）。区分有价值。
- PDF7–9 HVL=claim extraction→持久source grounding→本地NLI→calibration→“logit authenticity”（API问得的proxy，非直接logit）→风险压缩。
- PDF9–13仅Sonnet4-6/OpenRouter、5合成场景、n=1–2/方法、深度2–3，无CI/显著性、无human judge agreement、无组件消融，不能生产外推。
- PDF10 Table2 Vanilla hall10%/fidelity85%/tokens2846/38.2s；HVL .1%/79%/3945/97.6s；Reflexion fidelity90%/hall5%。HVL非全面占优。
- PDF11–12数学有错：6363/3945=1.61却写2.24×；10%→.1%=100×却写10×；Reflexion tokens4467>HVL还称cheaper。勿复述宣传数。
- 原生可保认知标记/source锚点，不可承诺“已验证近零幻觉”。

##### 论文 3.11 Facts Without Rules

- PDF3–6 36合成场景×7域、4handoff surfaces×5conditions=720design cells；E1采180，E2 hard25/40words；GPT5mini、DSR1-32B主测，Qwen3-32B部分复验；judge humanκ .74，exact+semantic detectors并报。
- PDF6 σb与σop相关近0；25word GPT σb .690/σop .997、DS .580/.972、Qwen .557/.977，事实保住不意味着边界保住。threshold描述性，非安全硬界线。
- PDF7–9明确audience/prohibition优于vague hedges；prompt-only仍58/288、42/288泄露。exact redaction字面0/288但semantic约10.8/13.5%，独立双标确认10/130、κ .39，不能称无泄漏。
- PDF9/19 E9每fact有gold `allowed_audiences`，提示agent仅在受众属于名单时提及；**不是确定性runtime过滤**，hand-constructed upper-bound probe，主模型0/48。PDF20/23 clean stress DS1/48、Qwen4/48；30%错标签11/48、15/48；negation flip44/48、38/48、35/48。schema正确性/可解析性比有schema本身重要。
- PDF21–23额外493 PrivacyLens独立半合成单agent工具轨迹、2465handoffs，structured σb .73→compressed .28，σop .94–.99，41%落低边界/高操作象限。强化机制外部效度，但非生产多agentlogs，operational facts从user instruction派生较弱，单norm无法估计真实norm频率。
- 原生落点：typed boundary附provenance、权限层确定性执行、自动抽取置信/拒绝策略、读写路径测paraphrase漏；不能以人工gold标签证明自动治理已经解决。

##### 论文 3.12 Do Not Restart / CFRC

- PDF3–7冻结bindings、receipt证实effects、残余义务；先于继任proposal冻结contract。ERC将proposal关闭为DAG，可在isolated replica试跑；动态值绑定source receipt，非沙箱具体ID。
- CE写前查coverage/权限/依赖/grounded参数，写后live receipt核销；plan或sandbox成功不等于live成功。失败⊥不等于回滚或无副作用，已有live写后失败需重建contract。
- PDF7条件正确性前提包含schema sound、evidence valid、tenure isolation、**全部义务正确抽取**；不保证从自然语言自动提全原目标。
- PDF7–9 5个stateful mock工具bench（STATE、τ2retail/airline、ToolSandbox、Agent-Diff）、GPT5.4mini/5.5/Luna/Sol等、5完整runs；macro70.8vs69.6、mean cost34.6%；Luna/Sol70.8vs70.0、cost22%。
- PDF9 audit50binding100%；write义务recall62/68=91.2%；ungrounded writes0/50但95%upper5.8%；falsecomplete2/50=4%；43rejection可重放、124interventions在写前。
- 限制单次cheap→strong交接、冻结合同、mock schemas，不是任意代码/GUI可完整闭图证明。先迁移receipt/unknown状态/义务核销，不许诺whole-task DAG/exactly-once。

##### 论文 3.13 LangChain: Organizing Context in a Multi-Agent Harness

- PDF1–8产业设计说明：fork完整supervisor state、删末尾调用、task追加user message、返回为tool result。fork适合已诊断worker、memorizer（对话是材料）；isolated适合独立reviewer/research。
- 没有受控实验/量化cache收益；前缀缓存需provider/model兼容和实际命中测量。isolated也需要最小source pack，不能让agent仅凭偏见摘要评审；fork也不是永远更省或更好。

#### C.2 记忆与知识库逐篇证据

##### 论文 4.1 LongMemEval

- PDF2–6 500人工问题、五能力IE/MR/TR/KU/ABS、7题型，S约115k tokens/M500sessions约1.5M。evidence由LLM自聊再人工改，30false-premise abstention，judge>97%人一致。商业UI测试仅97题/3–6sessions/2024年8月，不是当前产品排名。
- PDF7–10三阶段index/retrieve/read、value/key/query/reader四控制点。PDF8仅summary/facts替换原session/round通常QA下降，multi-session例外；保round原文value、fact扩充key有效。
- PDF9 Table3 round K=V recall@10 .692→V+fact .784、GPT4oQA .670→.720；onlyfact key .654/.664，不应扔原文。
- 时间query filter依赖强LLM，8B会误判反伤；时间排序+JSON+Chain-of-Note有用，但oracle reader也能差10pp。PDF11明确缺删除算子损害可信度、memory poisoning风险。
- 原生：事实可追原文/时间/版本，retrieval和reader分开测；不把facts store当truth，不把五能力叫最难排序。

##### 论文 4.2 Mem0

- PDF4明确extract+ADD/UPDATE/DELETE/NOOP，m=10recent、s=10neighbors、GPT4omini；summary+recent+newpair提候选。与2026源码ADD-only不能混淆。
- Mem0g实体边，冲突标invalid可按时序检索；检索实体邻居+triples semantic。图不是永远胜文本。
- PDF6–8 LoCoMo10dialogs约600turn/26k tokens，排除adversarial所以没测unknown/abstention；judge10次+lexical，商业UI手工加入与provider实现不一致。
- PDF11 Table2 fullcontext judge72.90最高，Mem0 66.88、graph68.44；retrieved1764/3616 vsfull26031tokens，p95 1.440/2.590 vs17.117秒，是效率换部分准确率，不是全面更优。singlehop/multihop纯Mem0更高，temporal graph更高。
- PDF14对Zep慢/冗余是当时provider配置；不是当前Graphiti排名。无中文coding、删除屏障、并发治理实验。

##### 论文 4.3 A-Mem

- PDF4 note=原content/timestamp＋LLMkeywords/tags/context＋embedding＋links；topk邻居后LLM linking/evolution，派生字段会替换旧版，适合可重建projection而非权威fact。
- PDF5–7 LoCoMo7512QA+DialSim、6foundationmodels、MiniLM embedding，k可按category调。F1/BLEU等主测，不是治理安全实验。
- PDF7多跳去link+evolution9.65、onlylink21.35、full27.02；DialSimF1 3.45vs2.55/1.18绝对仍低。
- PDF8百万向量3.7µs只是retrieval微基准，不是LLM抽取/演化/端到端生产吞吐。PDF9限LLM依赖/textonly，tSNE团簇不证明记忆真。不同paper LoCoMo类别/评估口径不能硬跨表比。

##### 论文 4.4 HippoRAG 2

- PDF3–5 OpenIE triples＋phrase/passage nodes＋synonym edges、query→triple retrieval＋LLM recognition filter＋PPR，返回原passages给reader；图帮助找证据，不把生成summary升真。
- PDF6–9七bench：NQ/PopQA/MuSiQue/2Wiki/Hotpot各1000、LVEval124、Narrative293（10docs）；Llama3.3-70B extract/filter＋NVEmbed2，reader Llama/GPT4omini；baseline尽量同extractor/retriever。
- PDF7/16 QA avgF1 59.8vsdense57.0；recallavg78.2vs73.4；2Wiki+9.5pp，不是每任务显著提升。LightRAG avg6.6复现用local模式/short phrase统一QA prompt（PDF19），不能推翻它的global summary原任务结果。
- PDF8 ablation passage nodes multi-hop recall87.1→81.0，queryNER74.6/querynode59.6，filter87.1vs86.4增量小。continual仅文档分4段扩容，多跳仍随规模衰减，未测冲突更新/删除/多agent写。
- PDF16误检/recognition filter错误/PPR都有失败；空triples回退dense。PDF19 11656 passages上dense vsHippo2：index12.1vs99.5min、QA .3vs1.2s、GPU1.7vs9.9GB（不计共享weights），图有明显额外成本。Table11第二案例query与展示triples不相干，样例存在编排瑕疵，勿只引其定性说明。
- 可迁移：保passage provenance、图/向量混合/空图回退，索引成本与增量维护单独计量；脑类比不构成可控遗忘证据。

##### 论文 4.5 GraphRAG

- PDF4–6 chunk→实体关系claims→Leiden社区层级→summary→query map answer/helpfulness→reduce；是预编译知识产物明确实例。claims/relations可LLM推断非原文明说，entity exactstring合并，派生必须有原文/重建/校验。
- PDF7–10两个约1M/1.7M tokens语料（podcast/news）、每语料125 LLM生成globalquestions、GPT4turbo/8k context、head-to-head judge每题5次，全面性/多样性/empowerment/directness，无gold answer。
- global比vector全面性win72–83%、多样性62–82%，vector更direct、empowerment混合。是global sensemaking偏好，不是精确factQA/代码引用必优。
- PDF10 root社区相较全source mapreduce减>97%querycontext（9–43×），不是比vector便宜97%；PDF9 podcast预索引281min要摊销。PDF11更多verifiable claims/cluster不等于claimtruth已验证，许多global vsTS差异不显著。
- PDF11–12限制两语料/LLMjudge、需fabrication和跨域验证。原生宜把原文、索引、wiki/graph产物版本与依赖分开，不让派生自证真。

##### 论文 4.6 LightRAG

- PDF3–5实体/关系抽取→LLM profile key-values→dedupe；local实体与global关系关键词向量检索＋一跳邻居，返回描述与原chunk。增量PDF4只是新旧nodes/edges集合并集，**没有由此解决冲突修订/删除/撤回/来源贡献清理**。
- PDF5–7 UltraDomain四域（Agriculture/CS/Legal/Mix，.6–5M tokens）、每域125 LLM生成globalquestions、GPT4omini所有LLM操作/同一judge、chunk1200、gleaning1；交替answer顺序减positionbias但无gold correctness。
- PDF7 vsGraphRAG overall54.8/52.0/52.8/49.6胜率，很多接近50；最大优势是diversity，不等于事实准确率提升。PDF8移除原文不明显下降只在此global偏好评估成立，不能用来删除证据。
- **PDF9 Table3案例query问“哪些方法归一化特征”，两答案全在讲推荐系统评估指标；judge仍因覆盖更多指标选LightRAG。** 这直接说明全面性judge可奖励答非所问，不能替代正确性/grounding测试。
- PDF9–10 “<100tokens/1call”仅关键词生成/检索环节，对比GraphRAG处理610社区report，并非完整回答/索引总成本；增量成本是同等大新增语料的特定重建假设，不代表当前GraphRAG所有实现。
- 无独立limitations节；缺factQA、citationfaithfulness、冲突删除、长期维护和人工独立judge等。原生可试双层召回，但必须带原文引用与坏案例回归。

##### 论文 4.7 MemOS

- 本地是v4 **2025-12-03**，含MemOS-1031更新实验；不要只按初稿2025-07或误写2026。
- PDF13–22三类memory：plaintext、activation/KV、parameter；MemCube payload+metadata（origin/version/TTL/access/usage），reader/operator/scheduler/lifecycle/governance/vault/load-dump。这些是有启发的资源建模，完整权限/遗忘/回滚正确性不能仅由架构描述得到。
- 生命周期口径内部不统一：PDF18五态含Expired，PDF21四核心态再加Frozen。缓存失效PDF20只写usage/contextual drift heuristic，没有给可证明的跨模型兼容/撤回失效协议。PDF31 cross-LLM参数/activation共享及MIP兼容规则仍是未来工作。
- PDF23–25同GPT4omini，LoCoMo/LME/PreFEval/PersonaMem，自称按各baseline validation最优配，H80080GB；没有足够逐任务配置/种子/CI说明可独立复制全治理栈。
- PDF24 LoCoMo judge75.80/1589tokens，但Memobase overallF1 50.18高于MemOS45.27；LME77.8整体，但knowledge update74.3低于Memobase89.7/Zep74.4。不能称各维全面最佳，尤其更新正是原生记忆关键。
- PDF25 PersonaMem61.2%四选一；PreFEval0turn personalized77.2%，10turn71.9%；仍有preference hallucination14.5/18.6%。
- PDF25–27 network API robustness 10/40QPS外部服务，100QPS只有MemOS行；未说明所有远端服务配额、region、重试/样本等，不能直接宣称100QPS胜全部系统或无错误。成功HTTP/API操作不等于语义正确。
- PDF26–28 KV加速假定memory已预处理且**驻GPU**，单H800/HF transformers、Qwen3-8B/32B/Qwen2.5-72B、583/2773/6064context、167/302.7/952.7query；Qwen2.5-72B长context短query TTFT .15vs1.79s=91.4%降低，另有build1.27s。不是总成本/总延时91.4%降低，不是跨provider/model任意复用KV。
- 原生可借MemCube身份/来源/版本/namespace/TTL，但先做plaintext/source投影与删除传播，再谈activation/LoRA；高频不等于真，归档不等于遗忘，签名不等于事实正确。

#### C.3 网友方案与评论

路径：`/Users/macbook/Documents/GitHub/Obsidian/多Agent本地记忆共享方案.md`。

- 第四版核心是本地**可恢复snapshot规范**，不是量化证明的记忆算法：MEMORY只index（4KB优先目标非硬删）、active单任务、done先写核验再删active/updateindex、topics放复用经验；index与单任务writer管理并发；未知sideeffects先查现场不盲重放。
- 明确记忆不是规则/授权/当前状态证明；冲突/过时需核对；不可存凭据；worktree正确解析git排除，已tracked时停写。这些边界比具体目录名更值得原生化。
- 130–164旧版有更强“实质执行前建档”要求、按阶段落盘等流程；最新用户/项目规则明确只读时不适用，不能把资料里的规则当本次授权。
- 4KB预算、目录布局、每阶段强制落盘/新建任务是个人工程启发，无baseline、成本/恢复率评估。原生core已有事件则宜事件驱动checkpoint，避免模型重复写文件制造成本/陈旧双真源。
- 评论240–265存在分歧：有人主张强single-agent，有人认为长context主控漂移更糟；不可只摘反多agent意见当共识。
- “跨模型摘要必严重损失”“同模型参数才能cache”“fork缓存树分叉失效”“模块专属agent长期复用一定更省”“仅三种有效场景”“astra思考/ds执行”的效益均个人经验，没有公开provider命中率/控制实验；必须标经验/待测。
- 文内Anthropic等外链未联网阅读，不冒称外部证实。


#### C.4 补充：CMB / TriLink（Hy-Memory仓库内论文）

来源：[19页本地PDF](/Users/macbook/Downloads/同步空间/LYX/agent-memory/Hy-Memory/docs/papers/CMB_A_Coding_Memory_Benchmark_for_Long_Horizon_and_Interleaved_Coding_Agents.pdf)。本次读取主文1–9和关键实现附录17–19、声明及统计表选段；全文提取保存于本地研究目录。未复现、未核实外部出版版本，不把本地稿里的模型名或数字当成当前供应商能力保证。

- **方法与规模（物理页1–6、18–19）。** 九仓库/四语言、216个可执行任务、750个诊断问题、5595回合、823万冻结轨迹token。使用SWE-bench Pro的issue/patch/tests构造技术决策、3–5轮不同实现和交错任务，所有记忆方案回放同一冻结流；参考patch不直接放进记忆，但它参与构造指令/目标，属于合成、特意强调修订的场景。文件不重叠不证明语义独立；QA与Code评分单位不同。
- **三层不是三个孤立库（页5–6）。** Fact记录任务目标/方法/结果/文件及active/superseded；Experience保存方案、适用条件和来源fact；Diff按branch/file/object/symbol/region保有序patch与checkpoint。fact继承experience关联，检索先fact再相关experience并附diff，仍需query相关性过滤。论文明确这些关系表示共同provenance而非因果；更新不是全域原子证明。Diff维护不需额外LLM，不代表整套记忆构建零成本。
- **结果需要同时报反例（页7–9、17）。** CMB Sonnet-5：TriLink43.1%，最强其他记忆36.6%，但FullContext58.8%更高；flash35.6% vs最强记忆28.2%。外部99任务SWE-ContextBench：47.5/28.3 vs最强记忆45.5/25.3，仅+2/+3pp；部分插件低于NoMemory。作者明确固定运行比较不能建立统计显著性。CMB要求F2P与P2P一起过，外部harness的Resolved看F2P、P2P另报，两个榜分数不能硬合并。
- **消融与成本限制（页8–9）。** flash去Diff/Experience/Fact后35.6%降至20.8/19.9/19.9%，但改变了检索和上下文，未单独隔离三层link本身；qutebrowser三个消融都胜full，表示收益非普适。外部记忆注入TriLink690token vsMemOS479/25.3%、Mem0257/25.3%；是未匹配预算的观测点，未计所有Agent输入、构建和执行费用。不能据此宣称完整系统更省。
- **可原生化与反证。** 将Aeroric的task/checkpoint、experience、版本化diff/receipt关联，保留目标commit、branch和源span；用交错恢复+实际仓库测试评价记忆，不仅问答judge。当前Hy-Memory代码的chain默认关/尽力而为，不能因论文评估TriLink就称本地SDK已按该配置启用。CMB给了code-object元数据帮助diff grouping，真实会话未必具备，必须测试无元数据、中文指令、人工并发编辑、版本漂移、冲突和遗忘。

#### C.5 递归PDF盘点与排除边界

五组目录中`rg --files -g '*.pdf'`列出893个路径：20份主材料、CMB1份、openai/math数学集合789份、59个按路径归类的样本/资源/产品资料、其余24个嵌入PDF。后两类的归类是文件路径启发式，不是内容鉴定；完整路径保存在 [递归PDF清单](/Users/macbook/Downloads/同步空间/LYX/Aeroric/.local_docs/frontier-review-2026-11-02/recursive-pdf-census.json)。

数学集合在其项目README/结构层覆盖，未逐篇评判数学正确性；演示稿、图标、许可证、测试fixture与产品brief不作为Agent收益实验。layerfs愿景文与claude-mem演示属于各项目设计背景，实际能力仍以本地源码为准。这些材料没有全部逐字精读，本篇不声称遍历到文件就等于阅读理解。

### D. 182份参考工作副本的逐项覆盖矩阵

编号为原始清单index，非排序序号。HEAD仅标识提交基线，原有未提交修改仍可能存在；完整HEAD/remote/目录/许可入口保存在[覆盖清单](/Users/macbook/Downloads/同步空间/LYX/Aeroric/.local_docs/frontier-review-2026-11-02/review-coverage.json)。入口是README语义选段与目录，源码/测试表示额外定向阅读；均非整仓逐行审计，测试全部未执行。

同源复用并未删除不同HEAD的副本记录；同名不同owner保持独立。New-addition的手动删除记录见[排除清单](/Users/macbook/Downloads/同步空间/LYX/New-addition/_import-report/manually-deleted-repositories.json)，未恢复其中项目。

#### D.harness：27份

| index | 精确来源与阅读入口 | HEAD | 本地许可 | 阅读范围 | 借鉴、成熟度与取舍 |
|---:|---|---|---|---|---|
| 26 | [git@github.com:Aho1ic/orca](/Users/macbook/Downloads/同步空间/LYX/harness/orca/README.md:1) | `aa6f945001a5` | MIT | D2；README、实现 ledger、orchestration CLI、legacy read-only tests | Run-home/consumer generation/ack 语义；当前不含旧 scheduler/自动替代系统。 |
| 0 | [Aho1ic/Aeroric](/Users/macbook/Downloads/同步空间/LYX/harness/Aeroric/README_ZH.md:1) | `08d57aa32d8e` | Proprietary | D0；README、LICENSE、SwiftUI/Rust/iOS 结构 | 仅参考副本；不据此断言正式项目现状，不混同本地正式源码。 |
| 25 | [anomalyco/opencode](/Users/macbook/Downloads/同步空间/LYX/harness/opencode/README.zh.md:1) | `92b2ca0dd727` | MIT | D1；README、core subagent plugin | fresh child、自包含 prompt、直系归属与后台通知；提示词规则需 host 强制。 |
| 5 | [anywhere-labs/Agents-Anywhere](/Users/macbook/Downloads/同步空间/LYX/harness/Agents-Anywhere/README.zh-CN.md:1) | `64c3f67ee3b2` | README 称 MIT；根无 LICENSE，Python manifests 未见许可 | D1；README、connector/server 结构、`state_cache.py`、`runtime_sync.py` | native/server identity 重绑定与重同步；能力矩阵有价值，许可尚不清晰。 |
| 8 | [AstrBotDevs/AstrBot](/Users/macbook/Downloads/同步空间/LYX/harness/AstrBot/README_zh.md:1) | `abf683071952` | AGPL-3.0 | D1；README、`core/subagent_orchestrator.py` | 配置驱动 handoff 注册较轻；它不是完整调度、预算或恢复实现。 |
| 9 | [BytePioneer-AI/codex-host](/Users/macbook/Downloads/同步空间/LYX/harness/codex-host/README.md:1) | `33759b75f8f7` | LGPL-3.0 | D2；README、host-runtime coordinator/watch、broker server/writer tests | 幂等委派、单 writer、未知 send 不重放；watch 易失部分需重新设计。 |
| 2 | [can1357/oh-my-pi](/Users/macbook/Downloads/同步空间/LYX/harness/oh-my-pi/README.md:1) | `0838d2ab4407` | MIT | D2；README、task/structured-subagent、advisor guards、review command/tests | 去重、噪声预算、循环熔断和冻结审查目标；不默认每 turn 派 reviewer。 |
| 7 | [chenqi92/Markio](/Users/macbook/Downloads/同步空间/LYX/harness/Markio/README.md:1) | `7b2ae4e8f89e` | 根无 LICENSE，package/Cargo 未见许可 | D1；README、walker_io 原子保存 | 工件编辑与文件保存边界；不是协作主干，不能声称强 power-loss durable。 |
| 13 | [deepseek-ai/deepseek-harness](/Users/macbook/Downloads/同步空间/LYX/harness/deepseek-harness/README.zh.md:1) | `ce7073d51254` | MIT | D2；README、subagent activation/manager/index | 树共享 activation 准入、ownership、清理生命周期；无需引入其整套插件架构。 |
| 10 | [DietrichGebert/ponytail](/Users/macbook/Downloads/同步空间/LYX/harness/ponytail/README.md:1) | `96608e078b2d` | MIT | D1；README、review SKILL、subagent hook | 审查角色提示与上下文注入；主要是 prompt+hook，不能视为 host 强制或发现率证明。 |
| 17 | [farion1231/cc-switch](/Users/macbook/Downloads/同步空间/LYX/harness/cc-switch/README_ZH.md:1) | `fcd8f3bd4503` | MIT | D1；README、session_manager/providers/omp.rs | provider 配置、历史与恢复环境适配；不是多 Agent 执行 authority。 |
| 18 | [Haleclipse/CometixCode](/Users/macbook/Downloads/同步空间/LYX/harness/CometixCode/README_CN.md:1) | `bad7e08c2c4d` | AGPL-3.0 | D1；README、team_create、mailbox、forked_agent Rust 实现 | mailbox/团队概念可比较；逐文件移植且 WIP，不当作 Claude 官方协议或稳定完整实现。 |
| 14 | [hanshuaikang/nezha](/Users/macbook/Downloads/同步空间/LYX/harness/nezha/README.md:1) | `49a1c642e05b` | GPL-3.0 | D1；README、hooks.rs | hook marker、版本门槛与环境守卫；不是结构化团队协议。 |
| 12 | [herdrdev/herdr](/Users/macbook/Downloads/同步空间/LYX/harness/herdr/README.zh-CN.md:1) | `ee5bfb5041bb` | Apache-2.0 | D1；README、detect/mod.rs、codex manifest、agent CLI | daemon/终端存活与检测置信度；只作 fallback/诊断，不能压过结构化协议。 |
| 16 | [lizc2003/kloop](/Users/macbook/Downloads/同步空间/LYX/harness/kloop/README.md:1) | `50931a49ab5f` | Apache-2.0 | D1；README、Rust subagent tool | 严格输入、限深、隔离失败关闭、执行回执；不默认开 worktree 或自动合并。 |
| 21 | [MiniMax-AI/minimax-code](/Users/macbook/Downloads/同步空间/LYX/harness/minimax-code/README_ZH.md:1) | `50448abe3c75` | MIT，第三方例外见 LICENSE-STATUS | D2；README、goal evaluator/coordinator/verifier、contract tests | worker 完成提议与 host 完成授权分离；verdict 仍须可验证证据支持。 |
| 4 | [paperclipai/paperclip](/Users/macbook/Downloads/同步空间/LYX/harness/paperclip-多agents协作/README.md:1) | `7a7ade4b9360` | MIT | D2；README、budget reservations/enforcement/review wait/heartbeat | 账本、预留、重启恢复和治理；不引入公司/org chart/多租户业务模型。 |
| 20 | [router-for-me/CLIProxyAPI](/Users/macbook/Downloads/同步空间/LYX/harness/CLIProxyAPI/README_CN.md:1) | `90c447d3a872` | MIT | D0+；README、AGENTS、协议/代理目录 | 提供账号与协议兼容层；非 manager，代理网络超时策略不能直接移植成 Agent 活性策略。 |
| 11 | [sligter/qunica](/Users/macbook/Downloads/同步空间/LYX/harness/qunica/README.zh-CN.md:1) | `52258cbeac15` | backend-rs/Cargo.toml 明确 UNLICENSED；根无 LICENSE | D2；README、group_scheduler budget/state/store/topology/concurrency/recovery | 有界调度状态机和事务 fencing 思想有价值；不能直接复制源码。 |
| 22 | [spacering-net/codeg](/Users/macbook/Downloads/同步空间/LYX/harness/codeg/README.md:1) | `ddd1f09fc5dd` | Apache-2.0 | D2；README、ACP delegation types/broker/depth/parent_watcher | 配置快照、关联身份、终态竞态和缓存结果；适合契约设计参考。 |
| 19 | [Stack-Cairn/LiveAgent](/Users/macbook/Downloads/同步空间/LYX/harness/LiveAgent/README.zh-CN.md:1) | `85029b5611be` | MIT | D2；README、subagents agentTool/scheduler/policy/run | 输出白名单、失败 worktree 保留；其普通 readonly 仍可写 MCP，不能照抄安全语义。 |
| 23 | [t8y2/dbx](/Users/macbook/Downloads/同步空间/LYX/harness/dbx/README.zh-CN.md:1) | `790423156613` | Apache-2.0 | D1；README、dbx-mcp server、依赖钉住与工作区状态 | DB 工具契约和结果窗口；非协作主干。已有大量本地修改，不应 pull 或归并重构。 |
| 15 | [taichuy/1flowbase](/Users/macbook/Downloads/同步空间/LYX/harness/1flowbase/README.md:1) | `b5d26a22cf11` | Apache-2.0 | D2；README、Rust API/React 结构；`virtual_ui.rs`、MCP discovery/call tests | list/get/call 的渐进工具发现、结果投影；工具规模大时可借鉴，非团队调度器。 |
| 24 | [XiaomiMiMo/MiMo-Code](/Users/macbook/Downloads/同步空间/LYX/harness/MiMo-Code/README.zh.md:1) | `7bc7dc3a9150` | MIT，另有 USE_RESTRICTIONS.md | D2；README、actor schema/spawn、session prefix/prompt、fork test | 活性与 outcome 分离、fork 显式边界；缓存 parity 测试 skip，不能当跨 provider 保证。 |
| 1 | [yetone/magpie](/Users/macbook/Downloads/同步空间/LYX/harness/magpie/README.zh-CN.md:1) | `8f0d21a009d0` | MIT | D1；README、budget.go | 多窗口预留/网关预算；允许单次超额且未知价格为零，不是账单硬上限。 |
| 6 | [yicheng47/runner](/Users/macbook/Downloads/同步空间/LYX/harness/runner/README.zh-CN.md:1) | `40e96fde3a2b` | 当前 MIT；README 提到部分历史版本 GPL | D2；README、roster/event_log/msg/mission signal、投递文档、lead 删除回归 | durable inbox/读确认/daemon 边界；lead 已删除，PTY 注入只作兼容 fallback。 |
| 3 | [zai-org/ZCode](/Users/macbook/Downloads/同步空间/LYX/harness/ZCode/README.md:1) | `a60ba6a7e5a0` | Apache-2.0 | D2；README、CLI subagent runner/runtime、超时和废弃 join 文档 | setup 全生命周期取消与 inactivity watchdog；旧强制 join 策略已废弃。 |

#### D.知识库：2份

| index | 精确来源与阅读入口 | HEAD | 本地许可 | 阅读范围 | 借鉴、成熟度与取舍 |
|---:|---|---|---|---|---|
| 28 | [infiniflow/ragflow](/Users/macbook/Downloads/同步空间/LYX/知识库/ragflow/README_zh.md:1) | `2fc8373d4cca` | Apache-2.0 | 文档、关键源码/测试（见附录B） | 编译产物、dirty revision/lease、evidence；union不等于贡献可撤回 |
| 27 | [Tencent/WeKnora](/Users/macbook/Downloads/同步空间/LYX/知识库/WeKnora/README_CN.md:1) | `40192ace214c` | MIT/第三方例外 | 文档、关键源码/测试（见附录B） | Lite/来源授权、多页预算与持久挂起；不引企业整栈 |

#### D.agent-memory：4份

| index | 精确来源与阅读入口 | HEAD | 本地许可 | 阅读范围 | 借鉴、成熟度与取舍 |
|---:|---|---|---|---|---|
| 30 | [getzep/graphiti](/Users/macbook/Downloads/同步空间/LYX/agent-memory/graphiti/README.md:1) | `72b5c59c0660` | Apache-2.0 | 文档、关键源码/测试（见附录B） | 双时间、失效与谱系；不把模型矛盾判断当事实 |
| 32 | [mem0ai/mem0](/Users/macbook/Downloads/同步空间/LYX/agent-memory/mem0/README.md:1) | `26cdefb46600` | Apache-2.0 | 文档、关键源码/测试（见附录B） | 有界抽取、identity scope、混合搜索；托管数字不当OSS效果 |
| 31 | [RZfive/WorldBase](/Users/macbook/Downloads/同步空间/LYX/agent-memory/WorldBase/README.md:1) | `da96914a8186` | Apache-2.0 | 文档、关键源码/测试（见附录B） | 事实/证据/画像与forget屏障；完整治理尚为设计 |
| 29 | [Tencent-Hunyuan/Hy-Memory](/Users/macbook/Downloads/同步空间/LYX/agent-memory/Hy-Memory/README.zh.md:1) | `988667cd876f` | Apache-2.0 | 文档、关键源码/测试（见附录B） | SDK权威、事实经验/diff链、CMB；scope缺省不可继承 |

#### D.New-addition：149份

| index | 精确来源与阅读入口 | HEAD | 本地许可 | 阅读范围 | 借鉴、成熟度与取舍 |
|---:|---|---|---|---|---|
| 54 | [333-wg/pi-wm](/Users/macbook/Downloads/同步空间/LYX/New-addition/333-wg/pi-wm/README.md:1) | `38b28aa1e676` | MIT | 关键源码、测试源码（S+T） | 持久化team独立运行时、原子claim、scope冲突调度、幂等与writer fence最匹配；不迁整套Web/Pi产品，且声明writePaths不等于OS隔离。 关键证据：[apps/gateway/src/agent-teams.ts:369](/Users/macbook/Downloads/同步空间/LYX/New-addition/333-wg/pi-wm/apps/gateway/src/agent-teams.ts:369) |
| 93 | [affaan-m/ECC](/Users/macbook/Downloads/同步空间/LYX/New-addition/affaan-m/ECC/README.zh-CN.md:1) | `b38be6a7f635` | MIT | 源码、失败边界/测试（D2） | 多host安装manifest、hook runtime/profiles、Hookify约束、tests；可借受控生命周期与规则状态。部分hook异常fail-open/可禁用，不能视为系统权限墙。 |
| 110 | [agegr/pi-web](/Users/macbook/Downloads/同步空间/LYX/New-addition/agegr/pi-web/README.md:1) | `d128bae47cd3` | MIT | 入口（R） | 区分新独立session和当前session分支，并有空闲关停/deadline；可参考生命周期语义，非原生宿主或多provider治理实现。 关键证据：[README.md:13](/Users/macbook/Downloads/同步空间/LYX/New-addition/agegr/pi-web/README.md:13) |
| 50 | [agentscope-ai/QwenPaw](/Users/macbook/Downloads/同步空间/LYX/New-addition/agentscope-ai/QwenPaw/README_zh.md:1) | `c26d31c0688a` | Apache-2.0 | 架构（A） | 按Agent隔离workspace并把memory、skills、connector与governance接入运行时；可借鉴资源边界，整栈AgentScope常驻助理范围过宽。 关键证据：[website/public/docs/architecture.zh.md:11](/Users/macbook/Downloads/同步空间/LYX/New-addition/agentscope-ai/QwenPaw/website/public/docs/architecture.zh.md:11) |
| 68 | [airhunter/OnlyTranslate](/Users/macbook/Downloads/同步空间/LYX/New-addition/airhunter/OnlyTranslate/README.md:1) | `d2958536f1ba` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/airhunter/OnlyTranslate/LICENSE:1) | 入口 | 原文/译文对照，中文阅读投影不覆盖原证据。 |
| 107 | [AIsa-team/agent-foyer](/Users/macbook/Downloads/同步空间/LYX/New-addition/AIsa-team/agent-foyer/README.zh-CN.md:1) | `ee2723a60ef7` | Apache-2.0 | 入口（R） | 身份、上下文、资源发布、护栏作为MCP服务边界可参考；偏企业容器平台，非当前本地会话工作台优先依赖。 关键证据：[README.md:1](/Users/macbook/Downloads/同步空间/LYX/New-addition/AIsa-team/agent-foyer/README.md:1) |
| 78 | [akitaonrails/ai-memory](/Users/macbook/Downloads/同步空间/LYX/New-addition/akitaonrails/ai-memory/README.md:1) | `1c506228b701` | MIT，[许可文本](</Users/macbook/Downloads/同步空间/LYX/New-addition/akitaonrails/ai-memory/LICENSE:1>) | D3：README 20–120、597–729；handoff 类型/SQL/owner 测试，wiki purge、session-management、tombstone/重索引入口。 | 3.3.1；多用户记忆服务，有明确 writer/归属/清理合同及测试资产；单 data dir 单 server/SQLite writer，非通用执行检查点。 |
| 66 | [aloneio/runmesh](/Users/macbook/Downloads/同步空间/LYX/New-addition/aloneio/runmesh/README.zh-CN.md:1) | `882179c24ae4` | PolyForm Noncommercial | 架构（A） | 权限与能力分开、Runner主动WSS连接、执行时重验授权/路径；可作为边界设计对照，非商业许可且Cloudflare控制面不直接引入。 关键证据：[docs/architecture.md:5](/Users/macbook/Downloads/同步空间/LYX/New-addition/aloneio/runmesh/docs/architecture.md:5) |
| 139 | [Amiyadesi/agent-session-bridge](/Users/macbook/Downloads/同步空间/LYX/New-addition/Amiyadesi/agent-session-bridge/README.md:1) | `731766ad782c` | MIT，[许可文本](</Users/macbook/Downloads/同步空间/LYX/New-addition/Amiyadesi/agent-session-bridge/LICENSE:1>) | D3：README 295 行的关键合同/平台证据；core prepare/apply、stores remove_source、损坏/变更/回滚测试。 | 0.1.0；跨 harness 会话转换，安全复制合同较具体；parser read-back 不等于真实 GUI/模型续聊成功。 |
| 95 | [anomalyco/opencode](/Users/macbook/Downloads/同步空间/LYX/New-addition/anomalyco/opencode/README.zh.md:1) | `69269e4ebe30` | MIT | 入口、复用同源研究（R+H） | 保留原生harness/provider适配研究，与已有同remote来源复用；不把其产品内部state再复制成Aeroric第二事实源。 关键证据：[README.md:1](/Users/macbook/Downloads/同步空间/LYX/New-addition/anomalyco/opencode/README.md:1) |
| 39 | [anthropics/claude-code](/Users/macbook/Downloads/同步空间/LYX/New-addition/anthropics/claude-code/README.md:1) | `80c6959d28c4` | 商业条款/All rights reserved | 入口、复用同源研究（R+H） | 公开仓库是安装、插件和文档入口；保留官方CLI适配与边界研究，不能当开放的Agent loop实现。 关键证据：[LICENSE.md:1](/Users/macbook/Downloads/同步空间/LYX/New-addition/anthropics/claude-code/LICENSE.md:1) |
| 41 | [anthropics/claude-plugins-official](/Users/macbook/Downloads/同步空间/LYX/New-addition/anthropics/claude-plugins-official/README.md:1) | `a7ba5b93db0c` | 根Apache-2.0；各插件独立 | 源码、失败边界/测试（D2） | 读marketplace、plugin-structure、code-review、Hookify配置；不可变slug、重命名映射、sha钉住有价值。官方目录含第三方，不能作信任证明。 |
| 40 | [anthropics/knowledge-work-plugins](/Users/macbook/Downloads/同步空间/LYX/New-addition/anthropics/knowledge-work-plugins/README.md:1) | `2574e29377c7` | Apache-2.0；连接插件另核 | 代表材料/实现（D1） | 文件式领域插件、manifest/MCP/command/skill分层；抽读engineering code-review。可借领域包契约，不内置所有业务角色或连接器。 |
| 94 | [Asheblog/dsh-ollama-cloud](/Users/macbook/Downloads/同步空间/LYX/New-addition/Asheblog/dsh-ollama-cloud/README.zh.md:1) | `27605b91d11e` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/Asheblog/dsh-ollama-cloud/LICENSE:1) | 入口 | 按真实端点发现模型与思考档位，暂时错误不当作下架。 |
| 138 | [AstrBotDevs/AstrBot](/Users/macbook/Downloads/同步空间/LYX/New-addition/AstrBotDevs/AstrBot/README_zh.md:1) | `d203f84b7dfc` | AGPL-3.0 | 入口、复用同源研究（R+H） | 多消息渠道与插件生态可参考未来接入层；重复remote复用，bot平台/AGPL整栈不作为本轮原生会话核心。 关键证据：[README.md:1](/Users/macbook/Downloads/同步空间/LYX/New-addition/AstrBotDevs/AstrBot/README.md:1) |
| 125 | [aws-samples/sample-agent-cost-bench](/Users/macbook/Downloads/同步空间/LYX/New-addition/aws-samples/sample-agent-cost-bench/README.md:1) | `270c6f1bdb2b` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/aws-samples/sample-agent-cost-bench/LICENSE:1) | 源码 | 重复运行成功率、一致性、费用来源；相关样本需修正统计口径。 |
| 161 | [badlogic/pi-subagent](/Users/macbook/Downloads/同步空间/LYX/New-addition/badlogic/pi-subagent/README.md:1) | `4fc1fe5178d1` | 未见LICENSE | 入口（R） | tmux+独立JSONL可inspect/wait/steer，父退出后suspend保存；机制可参考，许可缺失且环境依赖tmux，不直接包进产品。 关键证据：[README.md:73](/Users/macbook/Downloads/同步空间/LYX/New-addition/badlogic/pi-subagent/README.md:73) |
| 91 | [BerriAI/litellm](/Users/macbook/Downloads/同步空间/LYX/New-addition/BerriAI/litellm/README.md:1) | `60fc121ce8ab` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/BerriAI/litellm/LICENSE:1) | 入口 | 协议一致性与限流合同参考；不新增 Python 网关权威。 |
| 101 | [bestnite/engram](/Users/macbook/Downloads/同步空间/LYX/New-addition/bestnite/engram/README.zh.md:1) | `95617430dc79` | AGPL-3.0，[许可文本](</Users/macbook/Downloads/同步空间/LYX/New-addition/bestnite/engram/LICENSE:1>) | D3：README.zh.md 核心/导入导出；scheduler 参数入口、MCP 每请求身份合同、跨用户劫持测试。 | FSRS v6 闪卡学习服务，有多用户与复习回归资产；不是同名 LLM memory 库。 |
| 102 | [bestnite/skills](/Users/macbook/Downloads/同步空间/LYX/New-addition/bestnite/skills/README.zh-CN.md:1) | `0d9c337f3208` | 未见根LICENSE | 代表材料/实现（D1） | 11技能；抽读xianyu技能/模板边界；明确业务副作用与站点调用失败模式。社交账号操作不进通用默认工具，许可与授权逐项解决。 |
| 105 | [cathrynlavery/diagram-design](/Users/macbook/Downloads/同步空间/LYX/New-addition/cathrynlavery/diagram-design/README.md:1) | `0cf73c3fcc56` | MIT；第三方例外见THIRD_PARTY_LICENSES | 代表材料/实现（D1） | README渐进加载/导出边界、references结构；可借单入口按需加载与来源说明。HTML/SVG发布图领域插件，不替代原生UI，不泛化为可编辑图源保证。 |
| 58 | [caulif/Reprise](/Users/macbook/Downloads/同步空间/LYX/New-addition/caulif/Reprise/README.zh-CN.md:1) | `0b86d5db6b15` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/caulif/Reprise/LICENSE:1) | 源码 | 显式 commit 与输入 hash 恢复、答案污染检查和证据对照。 |
| 42 | [ClickHouse/ClickBench](/Users/macbook/Downloads/同步空间/LYX/New-addition/ClickHouse/ClickBench/README.md:1) | `7276a62f955d` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/ClickHouse/ClickBench/LICENSE:1) | 入口 | 固定数据、冷/热缓存、失败结果披露用于原生评测。 |
| 83 | [cloudflare/security-audit-skill](/Users/macbook/Downloads/同步空间/LYX/New-addition/cloudflare/security-audit-skill/README.md:1) | `c1c8a8c14710` | MIT | 源码、失败边界/测试（D2） | 六阶段审计SKILL、coverage/findings验证器、tests；候选反证、needs_validation和覆盖缺口有价值。沙箱是外部要求，仓库并未提供OS隔离执行器。 |
| 122 | [coldingcode/web-builder-harness](/Users/macbook/Downloads/同步空间/LYX/New-addition/coldingcode/web-builder-harness/README.zh-CN.md:1) | `b8bd32f50cd5` | AGPL-3.0 | 入口（R） | 冻结feature_list哈希、独立evaluator与有限返工循环能防实现者自验；场景限定web-builder且AGPL，借验收机制而非整栈。 关键证据：[README.md:1](/Users/macbook/Downloads/同步空间/LYX/New-addition/coldingcode/web-builder-harness/README.md:1) |
| 72 | [Comfy-Org/ComfyUI](/Users/macbook/Downloads/同步空间/LYX/New-addition/Comfy-Org/ComfyUI/README.md:1) | `6d352802d8f8` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/Comfy-Org/ComfyUI/LICENSE:1) | 入口 | 可取消 DAG 与产物参数溯源；扩散引擎作外部任务适配。 |
| 81 | [CopilotKit/OpenBot](/Users/macbook/Downloads/同步空间/LYX/New-addition/CopilotKit/OpenBot/README.md:1) | `c396a5f68c81` | MIT；Intelligence另有许可/依赖 | 关键源码、测试源码（S+T） | handoff队列lease全批续租、调用前复查、签名run身份与正常runner留痕可借鉴；持久会话依赖外部Intelligence，不能当全本地自包含。 关键证据：[server/src/agents/handoff-runner.ts:258](/Users/macbook/Downloads/同步空间/LYX/New-addition/CopilotKit/OpenBot/server/src/agents/handoff-runner.ts:258) |
| 120 | [dani-garcia/vaultwarden](/Users/macbook/Downloads/同步空间/LYX/New-addition/dani-garcia/vaultwarden/README.md:1) | `e056359a4265` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/dani-garcia/vaultwarden/LICENSE.txt:1) | 入口 | 凭据边界与兼容接口参考，Keychain 保持凭据 owner。 |
| 117 | [debpalash/VoiceStudio](/Users/macbook/Downloads/同步空间/LYX/New-addition/debpalash/VoiceStudio/README.md:1) | `40c5e3ce6f5a` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/debpalash/VoiceStudio/LICENSE:1) | 入口 | 本地语音任务与模型资源管理作可选 provider。 |
| 87 | [deepseek-ai/DeepInfer](/Users/macbook/Downloads/同步空间/LYX/New-addition/deepseek-ai/DeepInfer/README.md:1) | `553741557a57` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/deepseek-ai/DeepInfer/LICENSE:1) | 入口 | 推理确定性是服务端能力；只记录端点声明，不承诺 KV 回滚。 |
| 86 | [deepseek-ai/deepseek-harness](/Users/macbook/Downloads/同步空间/LYX/New-addition/deepseek-ai/deepseek-harness/README.zh.md:1) | `ce7073d51254` | MIT | 入口、复用同源研究（R+H） | Cordis插件化harness已有研究可复用；README明示developer preview与破坏兼容，不能视稳定协议契约。 关键证据：[README.md:13](/Users/macbook/Downloads/同步空间/LYX/New-addition/deepseek-ai/deepseek-harness/README.md:13) |
| 56 | [denvey/ai-todos](/Users/macbook/Downloads/同步空间/LYX/New-addition/denvey/ai-todos/README.zh-CN.md:1) | `977d9130b236` | 未见LICENSE | 入口（R） | hooks观察运行状态与业务验收分离，不可观察即unknown；适合避免UI假完成，缺许可且不是执行/验收引擎。 关键证据：[README.md:44](/Users/macbook/Downloads/同步空间/LYX/New-addition/denvey/ai-todos/README.md:44) |
| 43 | [deqiying/onesearch](/Users/macbook/Downloads/同步空间/LYX/New-addition/deqiying/onesearch/README.zh-CN.md:1) | `ef67de430194` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/deqiying/onesearch/LICENSE:1) | 源码 | 候选 URL 与已抓取证据分开，按 operation 标就绪状态。 |
| 133 | [DietrichGebert/ponytail](/Users/macbook/Downloads/同步空间/LYX/New-addition/DietrichGebert/ponytail/README.md:1) | `96608e078b2d` | MIT | 入口、复用同源研究（R+H） | 本地任务cost/test band与审查提示可参考；已同remote复用，提示词工作流不能当运行时强制能力。 关键证据：[README.md:1](/Users/macbook/Downloads/同步空间/LYX/New-addition/DietrichGebert/ponytail/README.md:1) |
| 73 | [Disdjj/intelligent-ui-demo](/Users/macbook/Downloads/同步空间/LYX/New-addition/Disdjj/intelligent-ui-demo/README.md:1) | `33297b8629c8` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/Disdjj/intelligent-ui-demo/LICENSE:1) | 入口 | 声明式结果界面参考；研究复刻不作为正式上游协议。 |
| 142 | [duckdb/duckdb](/Users/macbook/Downloads/同步空间/LYX/New-addition/duckdb/duckdb/README.md:1) | `71f47d410f6a` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/duckdb/duckdb/LICENSE:1) | 入口 | 后续只读 trace 分析候选，不替代 SQLite 事务存储。 |
| 53 | [earendil-works/pi](/Users/macbook/Downloads/同步空间/LYX/New-addition/earendil-works/pi/README.md:1) | `c09c98d7d2ca` | MIT | 入口、复用同源研究（R+H） | 最小Agent内核与extension能力可继续作为可控基础；上游刻意不内建subagent/plan/权限，原生协作治理须由Aeroric负责。 关键证据：[README.md:19](/Users/macbook/Downloads/同步空间/LYX/New-addition/earendil-works/pi/README.md:19) |
| 61 | [elayadesign/ai-design-skills](/Users/macbook/Downloads/同步空间/LYX/New-addition/elayadesign/ai-design-skills/README.md:1) | `c920b7d5511c` | MIT | 代表材料/实现（D1） | 2个技能；landing-page-design、frontmatter validator及tests结构。可借结构校验，不迁入面向全部web任务的宽触发或营销审美硬规则。 |
| 90 | [Ephemeral-AI-Lab/layerfs](/Users/macbook/Downloads/同步空间/LYX/New-addition/Ephemeral-AI-Lab/layerfs/README.md:1) | `35d36575c1eb` | MIT，[许可文本](</Users/macbook/Downloads/同步空间/LYX/New-addition/Ephemeral-AI-Lab/layerfs/LICENSE:1>) | D3：README 核心/benchmark/限制；Store profile/init、branch publish、FUSE invalidation、并发提交测试。 | 1.1.0 源码发布；workspace/branch 引擎，API breaking，Disposable/非完整 POSIX；实验性基础设施。 |
| 108 | [EthanO-O/outclip](/Users/macbook/Downloads/同步空间/LYX/New-addition/EthanO-O/outclip/README.md:1) | `9dc6580d7c78` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/EthanO-O/outclip/LICENSE:1) | 入口 | 有界终端输出复制；剪贴板是外部效果，需明确动作。 |
| 104 | [expo/skills](/Users/macbook/Downloads/同步空间/LYX/New-addition/expo/skills/README.md:1) | `05754e71d02a` | MIT | 代表材料/实现（D1） | 官方Expo域路由、overview、scripts/tests与brownfield fixtures；按用户目标+项目依赖双门槛，叶技能权威。Aeroric为SwiftUI/Rust，不迁移Expo运行栈。 |
| 92 | [finereli/whatsapp-mcp](/Users/macbook/Downloads/同步空间/LYX/New-addition/finereli/whatsapp-mcp/README.md:1) | `67b6c5e583eb` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/finereli/whatsapp-mcp/LICENSE:1) | 入口 | 只读连接器及来源边界参考，通信接入需单独产品设计。 |
| 88 | [Finnyhudson/my-personal-vibecoding-skills](/Users/macbook/Downloads/同步空间/LYX/New-addition/Finnyhudson/my-personal-vibecoding-skills/README.md:1) | `c3fd524c709f` | 未见根LICENSE/许可声明 | 代表材料/实现（D1） | README、PRINCIPLES、配置快照结构；真实模型参数/职责/失败边界比角色名可靠。个人OpenCode+固定插件配置，不照搬模型映射、自动轮换或强制只委派。 |
| 76 | [Fission-AI/OpenSpec](/Users/macbook/Downloads/同步空间/LYX/New-addition/Fission-AI/OpenSpec/README.md:1) | `27c0515724de` | MIT | 源码、失败边界/测试（D2） | artifact graph/schema/state/instructions与blocked tests；产物依赖可视化好。文件存在性不证明质量，不默认创建spec/tasks/change目录。 |
| 154 | [franzenzenhofer/big-arrow-on-the-screen](/Users/macbook/Downloads/同步空间/LYX/New-addition/franzenzenhofer/big-arrow-on-the-screen/README.md:1) | `e42ce23f7be9` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/franzenzenhofer/big-arrow-on-the-screen/LICENSE:1) | 源码 | 不激活面板只覆盖绘制路径，AppRaiser 明确 activate；不宣称解决抢焦点。 |
| 62 | [FrigadeHQ/hob](/Users/macbook/Downloads/同步空间/LYX/New-addition/FrigadeHQ/hob/README.md:1) | `74a09d6a69b8` | MIT | 入口（R） | 按本机硬件自动选择本地权重的上手流程可参考；重点本地聊天/文件/PDF，不是多Agent控制面核心。 关键证据：[README.md:1](/Users/macbook/Downloads/同步空间/LYX/New-addition/FrigadeHQ/hob/README.md:1) |
| 47 | [Geruyang/GAS-Agent-Skills](/Users/macbook/Downloads/同步空间/LYX/New-addition/Geruyang/GAS-Agent-Skills/README.md:1) | `220a40162677` | MIT | 源码、失败边界/测试（D2） | 三模式协议、中央gas_runtime、runtime-evidence、组合复用模板与测试；事实/接受/交付分离。离线校验器非调度器，不默认强制团队席位或30分钟逐人弹窗。 |
| 159 | [gezipppp-ux/devloom](/Users/macbook/Downloads/同步空间/LYX/New-addition/gezipppp-ux/devloom/README.zh-CN.md:1) | `7f9777228465` | MIT | 架构（A） | 需求/分派/独立worktree/验收返工闭环可参考；当前Codex适配已明确，其他provider仍计划，不据计划承诺多引擎完成。 关键证据：[docs/engineering/architecture.md:15](/Users/macbook/Downloads/同步空间/LYX/New-addition/gezipppp-ux/devloom/docs/engineering/architecture.md:15) |
| 163 | [Git4Agent/mallard-app](/Users/macbook/Downloads/同步空间/LYX/New-addition/Git4Agent/mallard-app/README.zh-CN.md:1) | `43e95da4ff87` | MIT，[许可文本](</Users/macbook/Downloads/同步空间/LYX/New-addition/Git4Agent/mallard-app/LICENSE:1>) | D1：README 1–155 及结构/限制；Rust session_detail/pull_review/sync/budget 测试资产清点。 | 1.0.5；Codex/Claude 会话同步应用，有 review/push/pull 产品合同；非执行状态与组织知识权威引擎。 |
| 127 | [gitroomhq/postiz-app](/Users/macbook/Downloads/同步空间/LYX/New-addition/gitroomhq/postiz-app/README.md:1) | `4d13c6a2a847` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/gitroomhq/postiz-app/LICENSE:1) | 入口 | 计划与发布状态机参考，不接社媒运营业务。 |
| 75 | [google/styleguide](/Users/macbook/Downloads/同步空间/LYX/New-addition/google/styleguide/README.md:1) | `70d70c93c9cc` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/google/styleguide/LICENSE:1) | 入口 | 按项目选择语言规范，不覆盖 AGENTS 规则。 |
| 150 | [Graphify-Labs/graphify](/Users/macbook/Downloads/同步空间/LYX/New-addition/Graphify-Labs/graphify/README.md:1) | `f5155badddaa` | Apache-2.0 当前；历史 MIT/第三方另保留，[许可文本](</Users/macbook/Downloads/同步空间/LYX/New-addition/Graphify-Labs/graphify/LICENSE:1>) | D3：README extraction/cache/query/bench；cache version/prompt/stat、deleted/excluded pruning 与同 mtime 改写测试。 | 0.9.100；代码 AST＋文档语义图，有大量细粒度回归资产；语义边是模型产物，索引新鲜度需证据。 |
| 137 | [headroomlabs-ai/headroom](/Users/macbook/Downloads/同步空间/LYX/New-addition/headroomlabs-ai/headroom/README.md:1) | `fc1ba891bff2` | Apache-2.0，[许可文本](</Users/macbook/Downloads/同步空间/LYX/New-addition/headroomlabs-ai/headroom/LICENSE:1>) | D3：README compression/proof/privacy/learn；compression_cache/store 的 hash/TTL/replay，conversation scope 测试。 | 0.42.0；有多格式压缩与回归资产；TTL 恢复、模型/格式兼容及外部 proxy 仍需专项验收。 |
| 152 | [hellobertrand/zxc](/Users/macbook/Downloads/同步空间/LYX/New-addition/hellobertrand/zxc/README.md:1) | `d904467ba0bc` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/hellobertrand/zxc/LICENSE:1) | 入口 | 格式规范和一致性向量参考；无损字节压缩不等于语义压缩。 |
| 128 | [HKUDS/CLI-Anything](/Users/macbook/Downloads/同步空间/LYX/New-addition/HKUDS/CLI-Anything/README_CN.md:1) | `34f519533bc1` | Apache-2.0 | 源码、失败边界/测试（D2） | HARNESS SOP、GIMP backend/export/真实E2E、LibreOffice backend；结构化CLI+实际工件检查有价值。Pillow fallback与LaunchServices激活反证，不能声称所有CLI均真实后端且无界面。 |
| 44 | [hugohe3/ppt-master](/Users/macbook/Downloads/同步空间/LYX/New-addition/hugohe3/ppt-master/README_CN.md:1) | `5e8856be0da8` | MIT；PyMuPDF可选依赖AGPL | 代表材料/实现（D1） | README/路由权威/生成与编辑边界；三种artifact生命周期清晰。PPT转换器属业务插件，不进通用harness主干，local不等于模型服务不出网。 |
| 146 | [Inginnng/EditHere](/Users/macbook/Downloads/同步空间/LYX/New-addition/Inginnng/EditHere/README.md:1) | `c7198d54c21c` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/Inginnng/EditHere/LICENSE:1) | 入口 | 原图、坐标、时间戳、布局变更构成反馈包，接人工验收卡。 |
| 167 | [JasperPengC/agent-blackbox](/Users/macbook/Downloads/同步空间/LYX/New-addition/JasperPengC/agent-blackbox/README.md:1) | `6c58eb9611d8` | MIT，[许可文本](</Users/macbook/Downloads/同步空间/LYX/New-addition/JasperPengC/agent-blackbox/LICENSE:1>) | D3：README 关键机制/基准/限制；event/snapshot cache、session-source、event-cache/live-reload 测试。 | 0.1.0；本地日志只读事件回放，有缓存/增量回归资产；日志缺失、尾窗截断限制事实完整性。 |
| 166 | [JasperPengC/dsh-agent-blackbox](/Users/macbook/Downloads/同步空间/LYX/New-addition/JasperPengC/dsh-agent-blackbox/README.md:1) | `c9d644c4bcb4` | MIT，[许可文本](</Users/macbook/Downloads/同步空间/LYX/New-addition/JasperPengC/dsh-agent-blackbox/LICENSE:1>) | D2：README 全部关键合同；lib/index.js 的同源宿主/构建选择；host 冒烟测试定位。 | 0.1.2；DSH adapter 包，core/web 是主项目编译副本，不能当独立机制证据再计一次。 |
| 168 | [JasperPengC/dsh-mcp-skills-board](/Users/macbook/Downloads/同步空间/LYX/New-addition/JasperPengC/dsh-mcp-skills-board/README.md:1) | `eddf4acba485` | MIT | 源码、失败边界/测试（D2） | Host lib、四态frontmatter更新、managed block、host tests；控制面与来源分层清晰。无OAuth/headers可明文落盘且需特定host版本，不照搬秘钥存储。 |
| 113 | [JetBrains/thinkrail](/Users/macbook/Downloads/同步空间/LYX/New-addition/JetBrains/thinkrail/README.md:1) | `09f0489a1ed9` | Apache-2.0 | 关键源码、测试源码（S+T） | 文件frontmatter形成spec图，校验悬空/重ID/parent环，图工具与prose编辑分离；可借知识wiki结构校验，不能宣称已有事务/CAS防并发覆盖。 关键证据：[packages/spec-graph/core/validate.ts:56](/Users/macbook/Downloads/同步空间/LYX/New-addition/JetBrains/thinkrail/packages/spec-graph/core/validate.ts:56) |
| 111 | [jhao0413/parrot-agent-extensions](/Users/macbook/Downloads/同步空间/LYX/New-addition/jhao0413/parrot-agent-extensions/README.md:1) | `7791a2192252` | 未见根LICENSE；package无license | 代表材料/实现（D1） | 当前名Sermo；读Pi README、tests结构。显示翻译不污染上下文与取消晚到结果有价值；默认外部翻译和出站prompt改写不能默认启用，许可待查。 |
| 57 | [Jiao77/LLMflare](/Users/macbook/Downloads/同步空间/LYX/New-addition/Jiao77/LLMflare/README.md:1) | `44eb837eb744` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/Jiao77/LLMflare/LICENSE:1) | 入口 | 流式取消传播与仅元数据统计；补足现有路由观测。 |
| 176 | [jieliapp/Jieli](/Users/macbook/Downloads/同步空间/LYX/New-addition/jieliapp/Jieli/README.md:1) | `1497a55b853d` | Apache-2.0，[许可文本](</Users/macbook/Downloads/同步空间/LYX/New-addition/jieliapp/Jieli/LICENSE:1>) | D3：README 产品/部署；upload metadata/hash/事务、cache hooks、DeleteThread、附件保留测试。 | 会话云端同步/备份/分享服务，有归属/增量/清理测试资产；同步插件另仓，不应当零出站本地 memory。 |
| 36 | [JosssphZhou/seesee](/Users/macbook/Downloads/同步空间/LYX/New-addition/JosssphZhou/seesee/README.zh-CN.md:1) | `e201949858cb` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/JosssphZhou/seesee/LICENSE:1) | 入口 | 时间锚点与原始、纠正、译文版本分离；播放器业务暂缓。 |
| 156 | [K-Dense-AI/scientific-agent-skills](/Users/macbook/Downloads/同步空间/LYX/New-addition/K-Dense-AI/scientific-agent-skills/README.md:1) | `5f2bc0341354` | MIT；单技能/上游依赖另核 | 代表材料/实现（D1） | 177技能目录；抽读statistical-analysis及scanner hash/model/version/caching。领域能力宜按需安装；扫描调用LLM且SAFE非安全证明，不默认安装所有依赖。 |
| 59 | [k2-fsa/sherpa-onnx](/Users/macbook/Downloads/同步空间/LYX/New-addition/k2-fsa/sherpa-onnx/README.md:1) | `935e5ded3d94` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/k2-fsa/sherpa-onnx/LICENSE:1) | 入口 | 可选离线 ASR/TTS provider，模型下载不进入四类核心首批。 |
| 67 | [KatrielMoses/voidaccess](/Users/macbook/Downloads/同步空间/LYX/New-addition/KatrielMoses/voidaccess/README.md:1) | `e53b1cff0b01` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/KatrielMoses/voidaccess/LICENSE:1) | 入口 | 带来源实体图及抓取失败状态；不纳入 Tor 调查业务。 |
| 134 | [kividbio/kividb](/Users/macbook/Downloads/同步空间/LYX/New-addition/kividbio/kividb/README.md:1) | `7a3f682b4e3b` | AGPL-3.0-only；另有商业许可，[许可文本](</Users/macbook/Downloads/同步空间/LYX/New-addition/kividbio/kividb/LICENSE:1>) | D1：README 数据库/性能配置/安全/许可关键节；src/vector 与 expiry/eviction/recovery/index-persistence 测试清点。 | 1.2.0-rc3；Redis/RESP 数据库＋搜索/vector；不是 agent 记忆治理产品，不为本地需求默认引入重服务。 |
| 96 | [KrushnaVardhanReddy/PaperPilot](/Users/macbook/Downloads/同步空间/LYX/New-addition/KrushnaVardhanReddy/PaperPilot/README.md:1) | `1e6ea3646706` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/KrushnaVardhanReddy/PaperPilot/LICENSE:1) | 入口 | 本地 PDF 解析 provider 候选，需另验版面与引用定位。 |
| 45 | [kubernetes-sigs/kueue](/Users/macbook/Downloads/同步空间/LYX/New-addition/kubernetes-sigs/kueue/README.md:1) | `4a2bb59f9b3c` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/kubernetes-sigs/kueue/LICENSE:1) | 源码 | 准入检查、资源预留、公平队列；不引入 Kubernetes。 |
| 116 | [lexmount/moli](/Users/macbook/Downloads/同步空间/LYX/New-addition/lexmount/moli/README.md:1) | `28701350d919` | Apache-2.0 OR MIT；含第三方许可 | 入口（R） | 结构语义优先、必要时像素的浏览器交互思路可参考；是浏览器引擎/渲染器实现，跨平台移植与供应链成本高，暂不纳控制面。 关键证据：[README.md:1](/Users/macbook/Downloads/同步空间/LYX/New-addition/lexmount/moli/README.md:1) |
| 147 | [liangdabiao/Octop-muse-esp-mosaico](/Users/macbook/Downloads/同步空间/LYX/New-addition/liangdabiao/Octop-muse-esp-mosaico/README.md:1) | `36b380a0926d` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/liangdabiao/Octop-muse-esp-mosaico/LICENSE:1) | 入口 | 代次丢弃过期响应与有界队列可参考，嵌入式业务暂缓。 |
| 148 | [liangdabiao/whiteboard-teacher-video](/Users/macbook/Downloads/同步空间/LYX/New-addition/liangdabiao/whiteboard-teacher-video/README.md:1) | `db9aeadd5d67` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/liangdabiao/whiteboard-teacher-video/LICENSE:1) | 入口 | 确定性渲染与增量缓存，lint 不等于视觉验收；可选科研配方。 |
| 99 | [lintsinghua/oops](/Users/macbook/Downloads/同步空间/LYX/New-addition/lintsinghua/oops/README.md:1) | `59af9f32deaa` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/lintsinghua/oops/LICENSE:1) | 源码 | pre/post/current 比较保留后续编辑；无法证明回合内全部人工改动归属。 |
| 178 | [luongnv89/claude-howto](/Users/macbook/Downloads/同步空间/LYX/New-addition/luongnv89/claude-howto/README.md:1) | `b968f429866a` | MIT | 入口与结构（D0） | 教程README、10类目录/示例；适合入门帮助、术语和可复制案例。教程不是权威协议/执行引擎，具体CLI语法依当前adapter验证。 |
| 109 | [macro-inc/macro](/Users/macbook/Downloads/同步空间/LYX/New-addition/macro-inc/macro/README.md:1) | `25e31075da1b` | AGPL-3.0 | 架构（A） | 文档订阅逐资源授权与上游成功后ACK可参考；邮件/消息/CRM统一记忆是大平台，AGPL且迁栈成本高，暂不吸收核心。 关键证据：[docs/architecture/sync-router.md:19](/Users/macbook/Downloads/同步空间/LYX/New-addition/macro-inc/macro/docs/architecture/sync-router.md:19) |
| 179 | [Mag1cFall/AIStudio2API](/Users/macbook/Downloads/同步空间/LYX/New-addition/Mag1cFall/AIStudio2API/README.md:1) | `0302e7737005` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/Mag1cFall/AIStudio2API/LICENSE:1) | 入口 | 渠道失效及内置系统提示披露，不宣称网页通道等价于标准 API。 |
| 140 | [makecindy/cindy](/Users/macbook/Downloads/同步空间/LYX/New-addition/makecindy/cindy/README.zh-CN.md:1) | `abb138beb3fc` | Apache-2.0 | 架构（A） | worker生命周期、协同运行时契约、MCP/IPC分层有针对性；架构显式分当前/未来，先借责任与投递边界，不整套复刻。 关键证据：[docs/dev-rules/orca-team-architecture.md:31](/Users/macbook/Downloads/同步空间/LYX/New-addition/makecindy/cindy/docs/dev-rules/orca-team-architecture.md:31) |
| 35 | [malisper/pgrust](/Users/macbook/Downloads/同步空间/LYX/New-addition/malisper/pgrust/README.md:1) | `38634b408dea` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/malisper/pgrust/LICENSE:1) | 入口 | 统一资源预算可参考；beta 数据库不替换 SQLite。 |
| 46 | [mattpocock/skills](/Users/macbook/Downloads/同步空间/LYX/New-addition/mattpocock/skills/README.md:1) | `4ae2317ddb22` | MIT | 代表材料/实现（D1） | 39个SKILL文件；抽读code-review固定基线、Standards/Spec双轴；小技能可组合。避免为简单请求强制tracker/setup和持续盘问。 |
| 65 | [mengbaba3316/vimora](/Users/macbook/Downloads/同步空间/LYX/New-addition/mengbaba3316/vimora/README.md:1) | `80ed81bb7755` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/mengbaba3316/vimora/LICENSE:1) | 入口 | 参数、提示、素材快照与异步队列；生图只作可选配方。 |
| 79 | [milind-soni/OpenMausBot](/Users/macbook/Downloads/同步空间/LYX/New-addition/milind-soni/OpenMausBot/README.md:1) | `ac08cf72a70f` | Apache-2.0 | 关键源码、测试源码（S+T） | 每bot独立工具scope，模型发现与执行均限制，不支持native限制的engine显式拒绝；不把schema筛选等同执行审批或沙箱。 关键证据：[shared/tool-scope-support.ts:11](/Users/macbook/Downloads/同步空间/LYX/New-addition/milind-soni/OpenMausBot/shared/tool-scope-support.ts:11) |
| 164 | [mindfold-ai/Trellis](/Users/macbook/Downloads/同步空间/LYX/New-addition/mindfold-ai/Trellis/README_CN.md:1) | `f089cb328607` | AGPL-3.0 | 源码、失败边界/测试（D2） | README、manifest上下文注入、realpath/trusted roots、ablation事务/冲突与测试；按任务上下文和可逆配置有价值。独立实现思想，不复制AGPL；不默认PRD/journal/全套自动流程。 |
| 63 | [mitmproxy/mitmproxy](/Users/macbook/Downloads/同步空间/LYX/New-addition/mitmproxy/mitmproxy/README.md:1) | `b08148b35d76` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/mitmproxy/mitmproxy/LICENSE:1) | 入口 | 隔离协议测试与脱敏 trace；不默认截取 TLS 或安装证书。 |
| 145 | [mksglu/context-mode](/Users/macbook/Downloads/同步空间/LYX/New-addition/mksglu/context-mode/README.md:1) | `a35dd45f3976` | Elastic License 2.0；source-available，[许可文本](</Users/macbook/Downloads/同步空间/LYX/New-addition/mksglu/context-mode/LICENSE:1>) | D3：README 关键工具/检索/连续性/安全/许可节；URL cache、resume claim、containment、purge 源码与测试。 | 1.0.169；上下文隔离/FTS/continuity，有跨平台能力差异与 fail-open 边界；不是 OS 沙箱。 |
| 74 | [mosttt/ternilo](/Users/macbook/Downloads/同步空间/LYX/New-addition/mosttt/ternilo/README.zh-CN.md:1) | `49c87e869b97` | Apache-2.0 | 架构（A） | Local/Server/Worker三种权责与事件事实源、队列注入层划分可借鉴；多租户托管模型远超当前桌面核心，先保留机制。 关键证据：[docs/zh-CN/architecture.md:5](/Users/macbook/Downloads/同步空间/LYX/New-addition/mosttt/ternilo/docs/zh-CN/architecture.md:5) |
| 149 | [Mouseww/Prometheus](/Users/macbook/Downloads/同步空间/LYX/New-addition/Mouseww/Prometheus/README.md:1) | `1083b2e4a05f` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/Mouseww/Prometheus/LICENSE:1) | 入口 | 统一会话展示与隐私面罩；背景迁移不等于原生工具状态迁移。 |
| 98 | [msitarzewski/agency-agents](/Users/macbook/Downloads/同步空间/LYX/New-addition/msitarzewski/agency-agents/README.md:1) | `b37b5bb33b78` | MIT | 代表材料/实现（D1） | 大角色catalog；抽读Agents Orchestrator与Codex映射；可作角色说明素材。TOML映射主要name/description/developer_instructions，不提供调度权限或持久状态。 |
| 112 | [multica-ai/andrej-karpathy-skills](/Users/macbook/Downloads/同步空间/LYX/New-addition/multica-ai/andrej-karpathy-skills/README.zh.md:1) | `2c606141936f` | SKILL及README声明MIT，根无LICENSE | 代表材料/实现（D1） | 单一行为准则：小改动、显式假设、成功条件；适合精简默认指导。无执行引擎，不把谨慎理解为逢事追问或强制流程。 |
| 119 | [NapNeko/NapCatQQ](/Users/macbook/Downloads/同步空间/LYX/New-addition/NapNeko/NapCatQQ/README.md:1) | `98d781d19815` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/NapNeko/NapCatQQ/LICENSE:1) | 入口 | 通道适配参考；限制许可且非主线，不复制或新增 QQ 渠道。 |
| 52 | [Nick-Hogo/AgentSeed](/Users/macbook/Downloads/同步空间/LYX/New-addition/Nick-Hogo/AgentSeed/README.md:1) | `521b6767210f` | 未见LICENSE | 入口（R） | 教学式Agent概念分层可用于解释设计；多章仍TODO，不能作为成熟运行时或商业代码来源。 关键证据：[README.md:53](/Users/macbook/Downloads/同步空间/LYX/New-addition/Nick-Hogo/AgentSeed/README.md:53) |
| 129 | [nicobailon/pi-mcp-adapter](/Users/macbook/Downloads/同步空间/LYX/New-addition/nicobailon/pi-mcp-adapter/README.md:1) | `3fc01da22289` | MIT | 源码、失败边界/测试（D2） | trust/provenance/deferred/cache/output guard与trust/TTL tests；项目源执行门槛、按需schema、输出窗口很有价值。脚本批准hash不含文件字节；不可照搬“批准一次永远有效”。 |
| 153 | [nightly-labs/openbot](/Users/macbook/Downloads/同步空间/LYX/New-addition/nightly-labs/openbot/README.md:1) | `26f9b8fd7d06` | PolyForm Noncommercial | 关键源码、测试源码（S+T） | mailbox按lane串行，群发结果合批、用户可立即介入；interrupt只可停止调用者委派的全部delivery。preview/full-access默认与非商业许可限制直接采用。 关键证据：[src/backend/agent/agent-interrupt-tool.ts:41](/Users/macbook/Downloads/同步空间/LYX/New-addition/nightly-labs/openbot/src/backend/agent/agent-interrupt-tool.ts:41) |
| 180 | [Nutlope/hallmark](/Users/macbook/Downloads/同步空间/LYX/New-addition/Nutlope/hallmark/README.md:1) | `4f6a46c6dc32` | MIT；照片各自CC/PD许可 | 代表材料/实现（D1） | README/SKILL、floor/reflex/finish、sloplint脚本/tests结构；可借硬检查/可解释例外/建议分级。网页风格禁令不搬进SwiftUI默认，fresh reviewer仍需host预算。 |
| 123 | [NVIDIA/OpenShell](/Users/macbook/Downloads/同步空间/LYX/New-addition/NVIDIA/OpenShell/README.md:1) | `42671c2dd2c0` | Apache-2.0 | 关键源码、测试源码（S+T） | 网关准入与runtime实际激活分别确认，策略版本/哈希留痕；Linux Landlock/seccomp落实边界，不能称macOS现成等价，best_effort还可降级。 关键证据：[crates/openshell-supervisor-process/src/supervisor_session.rs:82](/Users/macbook/Downloads/同步空间/LYX/New-addition/NVIDIA/OpenShell/crates/openshell-supervisor-process/src/supervisor_session.rs:82) |
| 103 | [obra/superpowers](/Users/macbook/Downloads/同步空间/LYX/New-addition/obra/superpowers/README.md:1) | `2ce8aa15d05b` | MIT | 源码、失败边界/测试（D2） | SessionStart注入、SDD skill/review-package/cannot-verify与测试结构；fresh上下文、固定review范围有价值。方法学核心仍提示词，不强制worktree/计划文件/自动commit。 |
| 85 | [omnara-ai/omnara](/Users/macbook/Downloads/同步空间/LYX/New-addition/omnara-ai/omnara/README.md:1) | `7f14db99b940` | Apache-2.0 | 架构（A） | 机器daemon主动连接、control plane/worker/memory FS分权清晰；托管平台需PG/Redis/blob等，原生端仅借持久上下文与远程执行边界。 关键证据：[docs/self-hosting/architecture.mdx:7](/Users/macbook/Downloads/同步空间/LYX/New-addition/omnara-ai/omnara/docs/self-hosting/architecture.mdx:7) |
| 171 | [openai/codex](/Users/macbook/Downloads/同步空间/LYX/New-addition/openai/codex/README.md:1) | `67f0994b532b` | Apache-2.0 | 入口、复用同源研究（R+H） | 官方app-server是进程/协议/审批/恢复边界主要参考；复用已有harness源码研究并核HEAD，不重复深入整仓。 关键证据：[README.md:1](/Users/macbook/Downloads/同步空间/LYX/New-addition/openai/codex/README.md:1) |
| 172 | [openai/math](/Users/macbook/Downloads/同步空间/LYX/New-addition/openai/math/README.md:1) | `4eaee8754825` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/openai/math/LICENSE:1) | 入口 | 分级验证与形式证明产物模板；论文集合不是 Agent 内核。 |
| 37 | [panda-z519/agent-session-titles](/Users/macbook/Downloads/同步空间/LYX/New-addition/panda-z519/agent-session-titles/README.md:1) | `dba70f3ee19c` | MIT | 关键源码、测试源码（S+T） | 三段标题按用户改动逐段锁定，无法可靠解析时保护整名；值得原生实现用户所有权，保留provider命名能力差异，不直接装hook。 关键证据：[plugins/codex-session-titles/hooks/session-title.py:602](/Users/macbook/Downloads/同步空间/LYX/New-addition/panda-z519/agent-session-titles/plugins/codex-session-titles/hooks/session-title.py:602) |
| 121 | [Panniantong/Agent-Reach](/Users/macbook/Downloads/同步空间/LYX/New-addition/Panniantong/Agent-Reach/README.md:1) | `7a38047acf26` | MIT | 入口与结构（D0） | README渠道fallback/只读/专用浏览器与安全段、Python结构；可借provider能力诊断。登录复制/站点账号和外部工具依赖需单独明确授权，不默认同步认证。 |
| 130 | [pbakaus/impeccable](/Users/macbook/Downloads/同步空间/LYX/New-addition/pbakaus/impeccable/README.md:1) | `e89e3ada3e87` | Apache-2.0 | 代表材料/实现（D1） | README、Rust rule registry/RuleHit、tests结构；deterministic质量规则与advisory分层。设计品味不是普适正确性，外部catalog请求与telemetry需区分。 |
| 106 | [php/php-src](/Users/macbook/Downloads/同步空间/LYX/New-addition/php/php-src/README.md:1) | `b9b231f75c2f` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/php/php-src/LICENSE:1) | 入口 | 语言运行时与测试集参考，无四类核心新增。 |
| 124 | [pingdotgg/t3code](/Users/macbook/Downloads/同步空间/LYX/New-addition/pingdotgg/t3code/README.md:1) | `82b10d546c10` | MIT | 关键源码、测试源码（S+T） | normalized domain、lineage与可恢复handoff有代码：pending/injected/inline、遗漏coverage、模糊投递换native thread；不把v2全部文档自动视已交付。 关键证据：[apps/server/src/orchestration-v2/ContextHandoffDelivery.ts:19](/Users/macbook/Downloads/同步空间/LYX/New-addition/pingdotgg/t3code/apps/server/src/orchestration-v2/ContextHandoffDelivery.ts:19) |
| 170 | [Proa-Labs/proa](/Users/macbook/Downloads/同步空间/LYX/New-addition/Proa-Labs/proa/README.md:1) | `5594c00039c7` | Apache-2.0 | 入口（R） | Rust SSR模板/stream/islands可能帮助Web渲染；与Agent编排/记忆主题不符，明确排除原生治理核心。 关键证据：[README.md:1](/Users/macbook/Downloads/同步空间/LYX/New-addition/Proa-Labs/proa/README.md:1) |
| 173 | [QingYunA/answer-me-with-html](/Users/macbook/Downloads/同步空间/LYX/New-addition/QingYunA/answer-me-with-html/README.zh-CN.md:1) | `aab9ea19f50f` | MIT | 源码、失败边界/测试（D2） | Markdown parser、component registry、raw HTML/CSP、local server与tests；模型输出小DSL、宿主渲染可借鉴。任意HTML/SVG不是可信原生UI，需schema与隔离桥接。 |
| 131 | [QuantumNous/new-api](/Users/macbook/Downloads/同步空间/LYX/New-addition/QuantumNous/new-api/README.zh_TW.md:1) | `70e6af642982` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/QuantumNous/new-api/LICENSE:1) | 入口 | 渠道权重与异步产物观测参考，不引入订阅计费后台。 |
| 162 | [QwenAudio/SenseVoice](/Users/macbook/Downloads/同步空间/LYX/New-addition/QwenAudio/SenseVoice/README_zh.md:1) | `586bbdb2b052` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/QwenAudio/SenseVoice/LICENSE:1) | 入口 | ASR 可选；checkpoint 语种与组合 pipeline 能力需分别声明。 |
| 136 | [Renhuai123/ziwei-doushu](/Users/macbook/Downloads/同步空间/LYX/New-addition/Renhuai123/ziwei-doushu/README.md:1) | `495b79ecb352` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/Renhuai123/ziwei-doushu/LICENSE:1) | 入口 | 数据与解释分离参考，命理引擎及数据集不纳入。 |
| 175 | [Ricardo121380/LeanFlip](/Users/macbook/Downloads/同步空间/LYX/New-addition/Ricardo121380/LeanFlip/README.md:1) | `9c2bfd46091f` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/Ricardo121380/LeanFlip/LICENSE:1) | 入口 | 只读迁移与实际路由展示，不另建配置权威。 |
| 151 | [rtk-ai/rtk](/Users/macbook/Downloads/同步空间/LYX/New-addition/rtk-ai/rtk/README_zh.md:1) | `b88d5492971b` | core Apache-2.0；engine/proxy BUSL-1.1，[许可文本](</Users/macbook/Downloads/同步空间/LYX/New-addition/rtk-ai/rtk/LICENSE:1>) | D3：英文 README 过滤/edition；runner/tee/retriever、pytest fallback 与测试场景、engine/proxy LICENSE。 | 0.49.0；命令特化压缩器，有错误/召回合同；有损过滤，默认不存每条成功命令全文。 |
| 82 | [ruvnet/ruflo](/Users/macbook/Downloads/同步空间/LYX/New-addition/ruvnet/ruflo/README.zh-CN.md:1) | `e580bbc82048` | MIT | 关键源码、测试源码（S+T） | receipt模块真实实现内容寻址、字段白名单、统计重算和Ed25519；同时remote team sync有显式模拟、claims示例store在内存，按模块采纳，暂不接受整套成熟宣传。 关键证据：[v3/@claude-flow/cli/src/services/flywheel-receipt.ts:595](/Users/macbook/Downloads/同步空间/LYX/New-addition/ruvnet/ruflo/v3/@claude-flow/cli/src/services/flywheel-receipt.ts:595) |
| 100 | [scholay/rimes](/Users/macbook/Downloads/同步空间/LYX/New-addition/scholay/rimes/README.md:1) | `6c657da3013a` | Apache-2.0 自有代码；第三方/历史许可另列，[许可文本](</Users/macbook/Downloads/同步空间/LYX/New-addition/scholay/rimes/LICENSE:1>) | D1：README 1–202 的产品、焦点、版本/路线图/许可节；Sources/Shared/platforms/Catalog/validation 结构。 | XSIME 多端输入法与 Buffer/Capsule/Mailbox；平台实现/设备验收分别跟踪，管理窗正常取键盘焦点。 |
| 71 | [shadcn-ui/ui](/Users/macbook/Downloads/同步空间/LYX/New-addition/shadcn-ui/ui/README.md:1) | `c0d63c339d6c` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/shadcn-ui/ui/LICENSE.md:1) | 入口 | 组件组合性作参考，继续 SwiftUI 设计原语。 |
| 55 | [Sharktheone/yavashark](/Users/macbook/Downloads/同步空间/LYX/New-addition/Sharktheone/yavashark/README.md:1) | `5aaa927bd975` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/Sharktheone/yavashark/LICENSE:1) | 入口 | 开发中 JS 引擎，不选作不可信模型代码的默认沙箱。 |
| 84 | [sickn33/agentic-awesome-skills](/Users/macbook/Downloads/同步空间/LYX/New-addition/sickn33/agentic-awesome-skills/README.md:1) | `66f1301eb4c8` | 工具MIT；原始非代码CC BY 4.0；第三方例外 | 源码、失败边界/测试（D2） | 主catalog及打包副本规模大；读AAS Core预览、manifest/plan实现、aas-compose-stack和skill-security-audit。仅选定子集，不能整库灌上下文或把结构有效当安全。 |
| 118 | [stablyai/orca](/Users/macbook/Downloads/同步空间/LYX/New-addition/stablyai/orca/README.md:1) | `0646d26ff002` | MIT | 架构（A） | 桌面与CLI共用worktree/terminal/browser能力，适合原生可编程控制面；必须按stablyai精确remote区分其他同名orca，不迁整套Electron/浏览器。 关键证据：[docs/site/content/docs/cli/overview.mdx:15](/Users/macbook/Downloads/同步空间/LYX/New-addition/stablyai/orca/docs/site/content/docs/cli/overview.mdx:15) |
| 141 | [Stonewuu/agenteam](/Users/macbook/Downloads/同步空间/LYX/New-addition/Stonewuu/agenteam/README.md:1) | `01ecfe30f8ca` | Apache-2.0 | 入口（R） | 知识/技能/IM/待办围绕数字员工统一入口可参考；Java自托管服务体系范围大，未源码核恢复与隔离，暂不核心引入。 关键证据：[README.md:1](/Users/macbook/Downloads/同步空间/LYX/New-addition/Stonewuu/agenteam/README.md:1) |
| 70 | [storytold/artcraft](/Users/macbook/Downloads/同步空间/LYX/New-addition/storytold/artcraft/README.md:1) | `1f969a922493` | Apache-2.0 OR MIT（双许可文件） | 入口与结构（D0） | README与Rust/frontend/native目录；图像视频IDE的素材/模型选择与编辑工件有启发。领域体量大、非技能治理或协作harness，不纳入主干。 |
| 69 | [storytold/photocraft](/Users/macbook/Downloads/同步空间/LYX/New-addition/storytold/photocraft/README.md:1) | `84732d09bcc9` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/storytold/photocraft/LICENSE-APACHE:1) | 入口 | 同一 Rust 引擎驱动 GUI/CLI/MCP；不嵌入完整图像编辑器。 |
| 114 | [supabase/supabase](/Users/macbook/Downloads/同步空间/LYX/New-addition/supabase/supabase/README.md:1) | `a05e7ec1d79e` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/supabase/supabase/LICENSE:1) | 入口 | 事件与权限参考，不把本地应用改成 Postgres 平台。 |
| 169 | [sylearn/AIUsage](/Users/macbook/Downloads/同步空间/LYX/New-addition/sylearn/AIUsage/README.zh-CN.md:1) | `a85f5714089e` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/sylearn/AIUsage/LICENSE:1) | 入口 | 用量来源与额度趋势界面，不修改真实 CLI 登录态。 |
| 144 | [Tencent/BrowserSkill](/Users/macbook/Downloads/同步空间/LYX/New-addition/Tencent/BrowserSkill/README.zh-CN.md:1) | `1b395fd07a04` | MIT | 源码、失败边界/测试（D2） | SessionManager、agent-window、borrow、Rust session_requests、cleanup与tests；任务所有权/claim/取消回收清晰。默认background无全程无焦点证据，内存token不等于重启durable。 |
| 157 | [thedotmack/claude-mem](/Users/macbook/Downloads/同步空间/LYX/New-addition/thedotmack/claude-mem/README.md:1) | `b80b29bc49d0` | Apache-2.0 当前；子目录另核 notice，[许可文本](</Users/macbook/Downloads/同步空间/LYX/New-addition/thedotmack/claude-mem/LICENSE:1>) | D3：README 210–305/许可等关键节；privacy、ResponseProcessor、DataRoutes 删除、server-beta generator、Chroma/隐私测试。 | 15.8.0；本地记忆 worker＋可选 cloud/team/server-beta；多路径成熟度应分开，不因版本大即推定完整来源/删除保证。 |
| 158 | [tianma-if/edgeever](/Users/macbook/Downloads/同步空间/LYX/New-addition/tianma-if/edgeever/README.zh-TW.md:1) | `c58e1f29ab8e` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/tianma-if/edgeever/LICENSE:1) | 入口 | 增量同步、附件生命周期与摘录模式参考，不引入云笔记账号系统。 |
| 48 | [TinyNano/TinyChat](/Users/macbook/Downloads/同步空间/LYX/New-addition/TinyNano/TinyChat/README.md:1) | `8be13cce4e0c` | 未见LICENSE；README称MIT | 入口（R） | 设置逐键合并和删除墓碑有借鉴价值；主体是PHP/SQLite聊天与计费服务，许可文件缺失，暂不吸收代码。 关键证据：[README.md:86](/Users/macbook/Downloads/同步空间/LYX/New-addition/TinyNano/TinyChat/README.md:86) |
| 51 | [TreeX-X/JanusX](/Users/macbook/Downloads/同步空间/LYX/New-addition/TreeX-X/JanusX/README.md:1) | `23f9ea4e4c7b` | MIT | 入口（R） | 项目/PTY/蓝图/分支工作台可参考导航；README标二测并依赖同级janus-agentX/WorkFlowX，单仓证据不足以判完整可交付。 关键证据：[README.md:24](/Users/macbook/Downloads/同步空间/LYX/New-addition/TreeX-X/JanusX/README.md:24) |
| 165 | [tt-a1i/archify](/Users/macbook/Downloads/同步空间/LYX/New-addition/tt-a1i/archify/README_ZH.md:1) | `a4065941f12b` | Apache-2.0 当前；历史 MIT 保留，[许可文本](</Users/macbook/Downloads/同步空间/LYX/New-addition/tt-a1i/archify/LICENSE:1>) | D1：README 产品/来源链接/IR/validate-deliver/范围/许可；schema/viewer/scripts/gating 测试结构。 | README 3.2.0；交互 HTML 解释作品，输入事实仍由作者/agent 提供；validator 证明结构不证明语义正确。 |
| 174 | [twentyhq/twenty](/Users/macbook/Downloads/同步空间/LYX/New-addition/twentyhq/twenty/README.md:1) | `aca6c8163ab2` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/twentyhq/twenty/LICENSE:1) | 入口 | typed 对象、字段、视图扩展参考，不增加 CRM 业务。 |
| 89 | [tywr/Nordwand-Mono](/Users/macbook/Downloads/同步空间/LYX/New-addition/tywr/Nordwand-Mono/README.md:1) | `bddc5d192536` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/tywr/Nordwand-Mono/LICENSE.txt:1) | 入口 | 可选字体按需下载与 OpenType 配置，非核心批次。 |
| 135 | [u14app/neo-chat](/Users/macbook/Downloads/同步空间/LYX/New-addition/u14app/neo-chat/README.zh-CN.md:1) | `cc2fafe2e6db` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/u14app/neo-chat/LICENSE:1) | 入口 | 研究引用与恢复流程；浏览器存储不成为原生权威。 |
| 126 | [uvwt/agentdock](/Users/macbook/Downloads/同步空间/LYX/New-addition/uvwt/agentdock/README.zh-CN.md:1) | `43b80c72fbd6` | Apache-2.0 | 架构（A） | 即时提案/实践验证/独立晋级可用于memory到skill的门禁；自进化架构文首明确非全部已实现，设备MCP网关主体不纳桌面核心。 关键证据：[docs/self-evolution-architecture.md:3](/Users/macbook/Downloads/同步空间/LYX/New-addition/uvwt/agentdock/docs/self-evolution-architecture.md:3) |
| 49 | [vastsa/PI-Desktop](/Users/macbook/Downloads/同步空间/LYX/New-addition/vastsa/PI-Desktop/README.zh-CN.md:1) | `0ba8cfd57cdc` | LGPL-3.0 | 架构（A） | Rust Host负责工具/权限/持久化，Node Pi负责turn与stream，Electron只是壳；可对照进程职责，远程控制含post-MVP设计且许可需单独评估。 关键证据：[docs/spec/02-architecture/01-architecture.md:5](/Users/macbook/Downloads/同步空间/LYX/New-addition/vastsa/PI-Desktop/docs/spec/02-architecture/01-architecture.md:5) |
| 115 | [VenLac/kaguya-code](/Users/macbook/Downloads/同步空间/LYX/New-addition/VenLac/kaguya-code/README.md:1) | `c635b7e67d9a` | Apache-2.0 | 入口、复用同源研究（R+H） | ZCode fork沿内部包/协议组织，可复用已有来源研究；不因改名另计一套独立成熟架构。 关键证据：[README.md:1](/Users/macbook/Downloads/同步空间/LYX/New-addition/VenLac/kaguya-code/README.md:1) |
| 181 | [vllm-project/vllm](/Users/macbook/Downloads/同步空间/LYX/New-addition/vllm-project/vllm/README.md:1) | `5d55ef1590c9` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/vllm-project/vllm/LICENSE:1) | 入口 | 外部推理端点与缓存计量，不把 vLLM 搬进桌面 harness。 |
| 97 | [web3claw/ReinAgent](/Users/macbook/Downloads/同步空间/LYX/New-addition/web3claw/ReinAgent/README.md:1) | `b5732fedd164` | MIT | 入口（R） | Rust工作台、RAG/记忆/子Agent声明覆盖广；本轮仅文档级，未核算法与恢复链，暂不据宣传认定具备可复用成熟机制。 关键证据：[README.md:1](/Users/macbook/Downloads/同步空间/LYX/New-addition/web3claw/ReinAgent/README.md:1) |
| 77 | [Wei-Shaw/sub2api](/Users/macbook/Downloads/同步空间/LYX/New-addition/Wei-Shaw/sub2api/README_CN.md:1) | `d851a3847759` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/Wei-Shaw/sub2api/LICENSE:1) | 入口 | 渠道亲和与配额观测；不纳入支付或转售系统。 |
| 60 | [wnzzer/tendly](/Users/macbook/Downloads/同步空间/LYX/New-addition/wnzzer/tendly/README.zh-CN.md:1) | `027a5d7d06b1` | MIT | 架构（A） | 统一ClientAdapter命令/事件及PTY帧序号续传；当前架构已Rust直连Claude/Codex/Pi，不再Node聊天桥。可借鉴协议壳，不复制CLI loop。 关键证据：[docs/architecture.md:15](/Users/macbook/Downloads/同步空间/LYX/New-addition/wnzzer/tendly/docs/architecture.md:15) |
| 132 | [xerj-org/xerj](/Users/macbook/Downloads/同步空间/LYX/New-addition/xerj-org/xerj/README.zh-CN.md:1) | `720b9990a2f8` | Apache-2.0，[许可文本](</Users/macbook/Downloads/同步空间/LYX/New-addition/xerj-org/xerj/LICENSE:1>) | D2：中英 README 核心、memory/embedding、MCP 验证说明；SECURITY_MODEL 关键边界与 Known limits；engine/tools 结构。 | ES 兼容 Rust 搜索/记忆命名空间，RC 演进；词法 embedding 默认、neural 显式启用；非完整 ES/RBAC 等价。 |
| 160 | [xz-dev/pi](/Users/macbook/Downloads/同步空间/LYX/New-addition/xz-dev/pi/README.md:1) | `4fbb2a239c15` | MIT | 入口、复用同源研究（R+H） | 上游最小patch stack加响应checkpoint/retry，适合窄对照恢复补丁；不重复泛读Pi、不另引一份fork依赖。 关键证据：[README.md:36](/Users/macbook/Downloads/同步空间/LYX/New-addition/xz-dev/pi/README.md:36) |
| 155 | [yicheng47/runner](/Users/macbook/Downloads/同步空间/LYX/New-addition/yicheng47/runner/README.zh-CN.md:1) | `b59ddf2a7db5` | 当前MIT；历史曾GPL | 架构、复用同源研究（A+H） | Rust daemon独占PTY/SQLite/terminal事实源，UI为mirror，进程脱离视图；同remote复用且当前许可与历史代码来源仍需按所取版本审查。 关键证据：[docs/arch/arch.md:9](/Users/macbook/Downloads/同步空间/LYX/New-addition/yicheng47/runner/docs/arch/arch.md:9) |
| 80 | [YILING0013/AI_NovelGenerator](/Users/macbook/Downloads/同步空间/LYX/New-addition/YILING0013/AI_NovelGenerator/README.zh-CN.md:1) | `a61674fad4dd` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/YILING0013/AI_NovelGenerator/LICENSE:1) | 入口 | 计划、候选审核、确认保存；小说业务与 MongoDB 栈暂缓。 |
| 177 | [Yoan98/Ncurator](/Users/macbook/Downloads/同步空间/LYX/New-addition/Yoan98/Ncurator/README.md:1) | `37ab787e27c7` | AGPL-3.0-only，[许可文本](</Users/macbook/Downloads/同步空间/LYX/New-addition/Yoan98/Ncurator/LICENSE:1>) | D2：README 能力/roadmap；WorkspaceKnowledgeCache 的 pipeline/model identity/checksum 实现；其余结构清点。 | 1.0.12；办公 assistant＋本地 KB；英文/多模态 KB、伙伴长期记忆在 README 中仍为计划。 |
| 34 | [ZhenHuangLab/mcp-web-tools](/Users/macbook/Downloads/同步空间/LYX/New-addition/ZhenHuangLab/mcp-web-tools/README.zh-CN.md:1) | `ef361978eb87` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/ZhenHuangLab/mcp-web-tools/LICENSE:1) | 入口 | 正文分段与 HTTP 能力分离；作为外部资料适配器，不新增研究后端。 |
| 33 | [ZhiAo1120/pi-opencode-zen](/Users/macbook/Downloads/同步空间/LYX/New-addition/ZhiAo1120/pi-opencode-zen/README.zh-CN.md:1) | `4d95dde06388` | MIT | 入口（R） | 免费端点请求形状兼容与无tools压缩修补可帮助识别provider脆弱点；协议指纹和免费服务策略依赖不纳入原生产品核心。 关键证据：[README.zh-CN.md:9](/Users/macbook/Downloads/同步空间/LYX/New-addition/ZhiAo1120/pi-opencode-zen/README.zh-CN.md:9) |
| 64 | [zqiren/Orbital](/Users/macbook/Downloads/同步空间/LYX/New-addition/zqiren/Orbital/README.zh-CN.md:1) | `049efbdd9366` | GPL-3.0 | 关键源码、测试源码（S+T） | 四份项目记忆、编辑前备份、entry ID守恒与归档修复值得独立实现；daemon重启自动恢复仍有路线图项，GPL代码不直接搬迁。 关键证据：[agent_os/agent/memory_editor.py:250](/Users/macbook/Downloads/同步空间/LYX/New-addition/zqiren/Orbital/agent_os/agent/memory_editor.py:250) |
| 38 | [zyycn/codex-proxy-rs](/Users/macbook/Downloads/同步空间/LYX/New-addition/zyycn/codex-proxy-rs/README.md:1) | `62516d976972` | 见[本地许可](/Users/macbook/Downloads/同步空间/LYX/New-addition/zyycn/codex-proxy-rs/LICENSE:1) | 入口 | 账号组与用量观测；复用现有 local_router。 |
| 143 | [zzusec/forkiq](/Users/macbook/Downloads/同步空间/LYX/New-addition/zzusec/forkiq/README.md:1) | `d8a08e6c7c6c` | MIT，[许可文本](</Users/macbook/Downloads/同步空间/LYX/New-addition/zzusec/forkiq/LICENSE:1>) | D1：README 同步/异常删除/备份/不确定写/结构；engine 租约定位与 tests 资产清点。 | 单管理员 GitHub fork 管理器；精确 SHA/HEAD 租约、备份先行可借鉴；与 context fork 无直接同义关系。 |

### E. 可追溯证据、版本与验证记录

#### E.1 持久研究记录

本篇已经合并覆盖表、修订意见与21篇论文论证；以下记录保留更详细的关键源码链、测试条目与反证，供后续实现定向回读。它们是本次研究附件，均已从临时位置复制到项目本地目录；其中complete仅指各自研究覆盖完成，不代表运行通过。

| 记录 | 内容 |
|---|---|
| [native-baseline](/Users/macbook/Downloads/同步空间/LYX/Aeroric/.local_docs/frontier-review-2026-11-02/evidence/native-baseline.md) | 正式项目五模块调用链、现有测试与技能home边界 |
| [harness-research](/Users/macbook/Downloads/同步空间/LYX/Aeroric/.local_docs/frontier-review-2026-11-02/evidence/harness-research.md) | 27项来源的13个机制、源码/失败边界及旧文反证 |
| [new-native](/Users/macbook/Downloads/同步空间/LYX/Aeroric/.local_docs/frontier-review-2026-11-02/evidence/new-native.md) | 45项工作台/协作来源、12个具体机制与许可 |
| [new-workflows](/Users/macbook/Downloads/同步空间/LYX/Aeroric/.local_docs/frontier-review-2026-11-02/evidence/new-workflows.md) | 30项工作流/技能来源、12个机制、hook/工具与供应链边界 |
| [new-context](/Users/macbook/Downloads/同步空间/LYX/Aeroric/.local_docs/frontier-review-2026-11-02/evidence/new-context.md) | 19项上下文/记忆来源、11个机制及删除/恢复/缓存反证 |
| [papers-comments](/Users/macbook/Downloads/同步空间/LYX/Aeroric/.local_docs/frontier-review-2026-11-02/evidence/papers-comments.md) | 20份主论文与评论的原始逐篇记录；CMB补充见本篇C.4 |

#### E.2 评测、恢复与外部工具的补充源码定位

| 来源 | 定向证据与结论 |
|---|---|
| Reprise | [recovery-git-start.ts:23](/Users/macbook/Downloads/同步空间/LYX/New-addition/caulif/Reprise/src/environment/recovery-git-start.ts:23) 只使用确认commit祖先与hash过的输入，拒symlink/submodule/LFS；[recovery-pre-task.ts:35](/Users/macbook/Downloads/同步空间/LYX/New-addition/caulif/Reprise/src/environment/recovery-pre-task.ts:35) 核查起点和污染。适合固定起点评测，不自动复制整个当前工作树作为历史基线 |
| oops | [restore.go:33](/Users/macbook/Downloads/同步空间/LYX/New-addition/lintsinghua/oops/internal/restore/restore.go:33) 对expected/desired/current比较，后续改变保留，过大文件标unprotected；不保证识别同回合内所有人工编辑 |
| sample-agent-cost-bench | [stats.py:91](/Users/macbook/Downloads/同步空间/LYX/New-addition/aws-samples/sample-agent-cost-bench/agent_cost_bench/stats.py:91) 重复运行统计；直接所有run的Wilson区间不足以处理同任务相关性。 [usage.py:1178](/Users/macbook/Downloads/同步空间/LYX/New-addition/aws-samples/sample-agent-cost-bench/agent_cost_bench/usage.py:1178) 不把缺usage的零上报当真实零成本；原生按task聚合/成对比较并区分reported/estimated/unknown |
| Kueue | [scheduler.go:1020](/Users/macbook/Downloads/同步空间/LYX/New-addition/kubernetes-sigs/kueue/pkg/scheduler/scheduler.go:1020)、同文件:1133 的quota reservation/admission与版本冲突处理，提示预留不等于所有准入条件已满足；只借调度合同，不引Kubernetes |
| onesearch | [catalog.go:134](/Users/macbook/Downloads/同步空间/LYX/New-addition/deqiying/onesearch/internal/mcpserver/catalog.go:134) 明示候选URL不是已抓取证据、submitted crawl不是已完成页面；status只本地预检，不能证明网络/凭据/额度有效 |
| big-arrow-on-the-screen | [OverlayPanel.swift:10](/Users/macbook/Downloads/同步空间/LYX/New-addition/franzenzenhofer/big-arrow-on-the-screen/Sources/BigArrowOverlay/OverlayPanel.swift:10) 的不激活面板只覆盖该路径；[AppRaiser.swift:28](/Users/macbook/Downloads/同步空间/LYX/New-addition/franzenzenhofer/big-arrow-on-the-screen/Sources/BigArrowTargeting/AppRaiser.swift:28) 明确activate。不能据它声称全程无焦点影响 |

#### E.3 重复来源的精确处理

| 来源 | 本地副本/HEAD | 处理 |
|---|---|---|
| anomalyco/opencode | harness/opencode `92b2ca0dd727`；New-addition/opencode `69269e4ebe30` | 来源合并计数，版本差异保留，不混写当前实现 |
| astrbotdevs/astrbot | harness/AstrBot `abf683071952`；New-addition/AstrBot `d203f84b7dfc` | 来源合并计数，版本差异保留，不混写当前实现 |
| deepseek-ai/deepseek-harness | harness/deepseek-harness `ce7073d51254`；New-addition/deepseek-harness `ce7073d51254` | 同HEAD复用阅读，保留两条副本记录 |
| dietrichgebert/ponytail | harness/ponytail `96608e078b2d`；New-addition/ponytail `96608e078b2d` | 同HEAD复用阅读，保留两条副本记录 |
| yicheng47/runner | harness/runner `40e96fde3a2b`；New-addition/runner `b59ddf2a7db5` | 来源合并计数，版本差异保留，不混写当前实现 |

#### E.4 本次验证范围

本次只修改独立研究文档和本地研究缓存，不实施产品逻辑。文档门禁、覆盖完整性、引用存在/行号及受保护原文哈希按最终稿重新检查；结果记录在[验证记录](/Users/macbook/Downloads/同步空间/LYX/Aeroric/.local_docs/frontier-review-2026-11-02/verification.json)。产品/来源测试、构建、真实模型、网络、GUI和VM均NOT RUN；阅读到测试代码不算运行通过。未提交、推送、发布或变更任何分支。

文档门禁通过48篇笔记的树、格式与相对链接检查；本篇和六份持久研究附件的本地引用存在性/行号检查无缺失或越界，182份来源HEAD与盘点一致。两份受保护文档哈希与初始baseline一致。门禁首次因当前shell的PATH无node而未启动，使用App已有Node运行时后通过；最终稿再用相同Node直接执行package脚本中的三项校验。pnpm校验期间新生成的114字节空锁文件已完整保留到[研究附件](/Users/macbook/Downloads/同步空间/LYX/Aeroric/.local_docs/frontier-review-2026-11-02/verification-generated-pnpm-lock.yaml)，没有留在项目根目录；未改变依赖声明，无产品测试通过声明。
