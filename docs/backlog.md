# Aeroric 原生能力候选库（v2，2026-10-23 重写）

> 调研底稿（`docs/research/`）从未入库，已于 2026-10-27 删除；文中指向它的引用已失效。旧版候选库 `e3836e62:docs/native/archive/…` 可用 `git show` 查看。
>
> 状态：**整合方案草案，待 grill-me 逐项确认**。本文不是实施授权；真实 UI 一律 NOT RUN，直到用户打开 App 验收。
> 旧版（10-22，1900 行，含 Q13–Q140 决策流水）原样归档于 `e3836e62:docs/native/archive/harness-feature-candidates-2026-10-22.md`，**其中已确认的决定继续有效**（见 §2.3），本文只在明确标注处修正它。
> 逐条证据（`path:line`）不在本文重复，见四份调研底稿：
> - G1 codeg / 1flowbase / LiveAgent / MiMo-Code：`e3836e62:docs/research/harness-full-review-2026-10-23-g1.md`
> - G2 paperclip / ZCode / orca / runner / kloop：`e3836e62:docs/research/harness-full-review-2026-10-23-g2.md`
> - G3 deepseek-harness / opencode / minimax-code / CometixCode / ponytail：`e3836e62:docs/research/harness-full-review-2026-10-23-g3.md`
> - G4 Agents-Anywhere / herdr / magpie / cc-switch / CLIProxyAPI：`e3836e62:docs/research/harness-full-review-2026-10-23-g4.md`
>
> 全部结论来自静态读码，**没有运行任何来源项目**。

---

## 0. 怎么读

- §1 来源与可信度；§2 已定约束（不再讨论）；§3 **同类整合总表**（20 个能力族，每族列出合并后的原生功能）；§4 四条主线的原生设计；§5 实现路径；§6 明确不做；§7 待定问题；§8 对旧版的勘误。
- 候选 ID 沿用旧版前缀并续编：C codeg、F 1flowbase、L LiveAgent、M MiMo-Code、P paperclip、Z ZCode、O orca、RUN runner、K kloop、D deepseek-harness、N opencode、X minimax-code、COM CometixCode、PT ponytail、**AA Agents-Anywhere（新）**、HER herdr、MAG magpie、CCS cc-switch、CPA CLIProxyAPI；U01 为用户提出的三角色进化方案。
- 成熟度：**实现**＝读到代码；**部分**＝有边界或只覆盖部分 agent；**文档**＝只有设计/提案；**骨架**＝只有状态结构无驱动。
- 优先级（建议，待确认）：**P0** 首发闭环必需；**P1** 首发差异化；**P2** 后续；**—** 不做。

## 1. 来源（20 个仓库）

| 来源 | HEAD | 许可 | 本轮新增 | 对 Aeroric 最大价值 |
|---|---|---|---|---|
| codeg | 4b5f942a | Apache-2.0 | C11–C37 | 专家/办公/科研 36 项配方、跨 agent 委派、待办看板 |
| 1flowbase | 7abe61548 | Apache-2.0 | F11–F21 | 多模型专家面板、可移植模板、计费规则 |
| LiveAgent | 85029b56 | MIT | L09–L28 | 记忆置信度降级与配额、规划日历、防休眠 |
| MiMo-Code | 6ba6a625 | MIT＋用途声明 | M11–M21 | Dream/Distill、Max 模式、卡死检测、24 个内置技能 |
| paperclip | 14f961b69 | MIT | P11–P45 | 原子领取、活性契约、审查政策、看门狗、交互卡 |
| ZCode | aac47556 | Apache-2.0 | Z10–Z21 | 子代理类型、escalate 预算、critic 回环 |
| orca | aa6f945001 | MIT | O09–O18 | 类型化消息、熔断、幂等回执、"未知"态 |
| runner | a9d3b290 | MIT（历史 GPL 区间） | RUN12–RUN22 | 单 lead 团队、PTY 注入就绪闸、观察对话 |
| kloop | 8d9c1c7 | Apache-2.0 | K14–K19 | 子代理深度限制、邮箱批次边界 |
| deepseek-harness | 571902028a | MIT | D09–D31 | Auto review、Ralph 循环、Agent Team、反馈、定时 |
| opencode | b474b84259 | MIT | N09–N16 | 按模型族提示词、最大步数硬停、explore 子代理 |
| minimax-code | e798b46 | MIT | X09–X27 | **技能自进化全管线**、verifier、失控防护、托管审查 |
| CometixCode | bad7e08 | **AGPL-3.0**（只借机制） | COM09–COM18 | memdir 记忆、/btw、agent 级记忆 |
| ponytail | 09c434a | MIT | PT11–PT15 | 子代理规则注入、可审计 judge |
| **Agents-Anywhere** | 6fb6b165 | MIT（README 声明） | **AA01–AA38** | **已上架 App Store 的远程控制先例**：配对、能力三元组、待回应卡、游标恢复、接管 |
| herdr | 143bf5fd | Apache-2.0 | HER13–HER21 | SSH 已存远端、prompt_agent、OSC 通知 |
| magpie | 75e769a9 | MIT | MAG15–MAG20 | 防休眠、SMAppService 自启、跨 agent 会话搜索 |
| cc-switch | 74cf8e6f | MIT | CCS11–CCS15 | 深链导入、菜单栏托盘、轻量模式 |
| CLIProxyAPI | 20c9095c | MIT | CPA19–CPA20 | wsrelay 反向中继 |
| U01（用户） | — | — | — | 主执行＋反思＋治理三角色进化 |

许可提醒：CometixCode 为 AGPL-3.0，只借机制、不复制代码；codeg 科研技能取自 K-Dense（MIT 子集）；MiMo 附 `USE_RESTRICTIONS.md`。配方**只借结构，正文自写或逐项核许可**。

## 2. 已定约束（不再讨论）

### 2.1 本轮（2026-10-23）新定
| 编号 | 决定 |
|---|---|
| 平台 | 只做 macOS 与 iOS（含 iPadOS），不考虑 Windows/Linux 客户端。 |
| Q10-c 分发 | macOS 先走 Developer ID＋公证官网分发；iOS 作为远程伴随端上 App Store；host 保持独立进程，为日后 Mac App Store 版留拆分空间。依据：AP0 探针实测沙盒下 exec 用户 CLI 与读 `~/.claude` 等均被内核拒绝。ADR-039 §7 Q1 已标注被取代。 |
| Q11-a | ADR-039 按 Q10-c 更新，旧文保留。 |
| Q12-b（待修订细化） | 改许可；具体范围见 §7 Q12'/Q13。 |
| IDE | 取消独立 IDE 页：诊断（tsc/eslint/cargo）与测试（vitest/pytest）并入 A05 质量证据；运行配置＋Web 预览并入任务运行区；LSP/DAP/补全/大纲冻结。 |
| Q3-a | 沿用 20 个能力族，新功能并入；本轮无需新开族。 |
| Q5-a | 允许改 `core/`（protocol/host/core），须向后兼容并补 Rust 测试。 |
| Q6-a / Q7-a | 新建 Trellis 任务承载实施；每批构建＋测试＋签名＋`AeroricNative-redesign-vN.zip`，用户验收后再进下一批。 |

### 2.2 架构硬约束（沿用）
1. 四族（Claude Code / Codex / oh-my-pi / DeepSeek Harness）只经 **PTY、hook 事件、会话文件、托管配置**（托管 home、mcp.json、skills 目录、指令文件）施加影响；不改内核、不合成或重渲染 TUI；omp 绝不传 `--mode`。

> **已被 ADR-043 修订（2026-10-25）：** Claude Code、Codex 改为经结构化协议驱动（`claude` stream-json 控制协议、`codex app-server`），不再跑在 PTY；omp 的 PTY 与“绝不传 `--mode`”规则在二期迁移前仍有效。
2. 业务真相在 Rust core；host 负责能力核验与执行；SwiftUI 只读 read model、只发 typed 动作。不建第五编码内核、不建第二网关。
3. 托管 home 与用户自己的 `~/.claude`/`~/.omp` 隔离；会话发现不扫用户目录（对照 MAG19 反例）。
4. iOS 不运行 agent、不下载执行代码；执行一律在用户自己的 Mac。

### 2.3 旧版已确认、继续有效的决定（摘要）
- **U01 / Q13–Q26**：开启进化时，主执行、阶段反思、独立治理三角色职责分离；反思/治理只读、模型独立；必需监督不可关闭；只改任务专属副本，经评测与受控试用。
- **Q27–Q40**：协作与主计划设计快照；普通协作者继承有限授权（Q31）；Full Access 不替代技能人工同意（Q30）。
- **Q41–Q50**：项目知识独立 owner；技能保存≠部署≠授权。
- **Q51–Q68**：工作安排与 typed 流程；Q65–Q67 资源护栏。
- **Q69–Q75**：反馈与人工问题。
- **Q114**：四层组织——配方库 / 可组合能力 / 执行·角色·权限·资源配置 / 任务工作面。
- **Q115–Q123**：配方只带方法与产物建议、不改模型权限；冲突不静默替代；工作方案可存配方＋能力＋角色配置＋资源引用、版本化、不自动套用不提权；首次普通会话只启用主 Agent；同项目沿用上次配方与能力为可见偏好；首批目录五组十六项；内置配方可另存为用户方法。
- **Q124–Q140**：账号与连接分层（统一账号→工具专属配置→任务角色绑定快照）及其后续确认（Q124 无单独回执，按"推荐分层继续出稿"处理）。

## 3. 同类整合总表（20 族）

每族一行"整合后原生功能"，把多个来源的同类机制合成一个 Aeroric 功能，并给出 owner、两端形态与建议优先级。来源 ID 只列代表项，全量见底稿。

### A01 工作配方库
- **整合**：codeg 三 Tab（C01/C03，36 项）＋自动化模板 7 个（codeg X1）＋MiMo 24 个内置技能（M17）＋minimax 内置技能（X27）＋paperclip 团队模板（P33）＋opencode/MiMo 命令模板（N15/M20）＋1flowbase 可移植模板（F13）。
- **原生功能**：
  1. **配方目录**按"代码开发 / 日常办公 / 科学研究 / 构思研究 / 能力沉淀"分组（与 Q122 五组对齐，见 §4.1）。
  2. 每个配方 = manifest（id、分类、方法步骤、注入模板、依赖技能、`needs_env`/`needs_key`、推荐能力、产物类型）。
  3. **诚实启用状态**：可用 / 未启用（可一键启用）/ 缺依赖（具名：缺 Python、缺 officecli、缺 key）/ 未知。**不学 codeg 加载前乐观可用**。
  4. 导出/导入为可移植包（含技能依赖声明），导入走预览确认。
- **owner**：core `recipes`（RecipeRevision）。**Mac**：新建会话加号菜单 → 配方面板；设置 → 配方 × Agent 启用矩阵。**iOS**：新建会话 sheet 只读选择；依赖显示"需在 Mac 上安装"。
- **优先级**：P0（目录＋注入＋状态）；导入导出 P2。

### A02 主计划与工作包
- **整合**：D01 计划审阅、P12 结构/依赖/归属/执行四分、P35 计划拆解精确一次、P30 首计划审批门、O01 依赖晋升、C04 待办看板状态、D29 todo 投影。
- **原生功能**：计划卡（批准 / 继续规划）→ 拆解为工作包（单负责人、parent 与 blocks 分离、状态 backlog/todo/in_progress/in_review/blocked/done/cancelled）；拆解按 (计划, 修订) 指纹精确一次；最后一个 blocker 完成才晋升 ready。
- **owner**：core `tasks`（PlanRevision、WorkPackage）。**Mac**：会话页计划卡＋工作包看板。**iOS**：批准计划、看看板。
- **优先级**：P1（与 A03 一起）。

### A03 任务团队与委派
- **整合**：RUN12 单 lead、P01 角色、P03/P11 原子领取＋单负责人、C11/C12 跨族异步委派与 @提及、D11/D12 具名成员＋可换后端、Z10/Z11 子代理类型与后台、K15 深度限制、O10 派发熔断、P21 子树控制、P16 横向 courier、PT11 子代理规则注入、M11 Max 多候选、F04 多模型专家面板。
- **原生功能**：见 §4.2。
- **owner**：core `tasks`＋`work_graph`。**Mac**：新建会话"团队协作"能力卡展开角色绑定；会话页团队栏＋成员终端切换。**iOS**：团队名册、与 lead 对话、暂停整树。
- **优先级**：P1。

### A04 反馈、消息与人工问题
- **整合**：C08/C15 steer 与排队、N02 持久 inbox、AA06/AA07 待回应卡与结构化提问、P19 六种交互卡＋应答人政策、P20 旧问题自动失效、O09 九类消息、O13 决策门、Z12 显式消息、Z13 escalate ≤3、RUN15 PTY 注入就绪闸、RUN16 on_behalf_of、K16 批次边界、D15 消息级反馈七类、D23 问答回注。
- **原生功能**：
  1. **统一消息模型**（§7 Q14 待定）：消息挂在工作包上，独立投递状态 已存 / 已投递 / 已读 / 已采纳。
  2. **PTY 注入就绪闸**：仅在 hook 判定空闲且无人输入时注入；结果三态 Confirmed / Unsubmitted（提示"按 Return"）/ Unverified。
  3. **运行中排队**：agent 忙时消息入队，等 Stop 事件后发送；"接受≠已处理"。
  4. **待回应卡**：审批、问题（多题、单/多选、自填）、决策门；提交态 sending / accepted / uncertain；schema 变化作废旧答。
  5. **结构化反馈**：每条回复可点评（七类），只入 core 库，不进模型历史；作为 A06 信号源。
- **owner**：core 消息/问题聚合（带 generation）。**Mac**：会话页卡片＋输入框排队提示。**iOS**：首页待回应 dock。
- **优先级**：待回应卡＋排队 P0；团队消息 P1；反馈 P1。

### A05 质量与交付证据（含"产物"子 owner）
- **整合**：D18/AA23 每回合改动汇总、X18 独立 verifier、X21 托管审查锚定 diff、P17 审查政策＋退修 3 轮升级、P34 完成契约、P22 看门狗、Z16 critic 回环、PT08 测试过时、PT12 可审计 judge、**IDE 的诊断与测试**、C03 办公/科研产物预览、D07 成果声明。
- **原生功能**：
  1. **回合改动**：UserPromptSubmit 与 Stop 之间做 git 快照，回合脚注显示 +/- 行数与逐文件 diff（与族无关）。
  2. **自动检查**：任务结束自动跑项目诊断与测试；结果 未运行 / 通过 / 失败 / 已过时（后续有编辑即过时）。
  3. **独立验收**：agent 声明完成后，可另起只读 verifier 任务核对证据；结论入证据。
  4. **审查政策**：任何人 / 非创建者 / 仅人；退修超过 3 轮升级给用户。
  5. **产物**：Office/PDF/图像产物登记，Mac 用 QuickLook、iOS 用 QLPreviewController 预览。
- **owner**：core `git`/`files`/`sessions` 上的 EvidenceRef、Finding、Deliverable。**Mac**：辅助面板"改动"页＋回合脚注＋检查卡。**iOS**：只读 diff 与检查结论。
- **优先级**：回合改动＋自动检查 P0；verifier/审查政策 P1；产物预览 P1。

### A06 任务进化（自进化）
- **整合**：U01＋X09–X15（技能进化管线、会话结束反思、生命周期、refiner、creator、记忆提醒退避）＋M01 Dream/Distill＋COM12 auto-dream（骨架）＋X17 retro＋PT12 judge＋N09 按模型族指令＋D15 反馈。
- **原生功能**：见 §4.3。
- **owner**：core `evolution`（Signal / Proposal / Evaluation / Revision）；托管物存储归 A09。
- **优先级**：P1（信号＋提案＋审批＋回滚）；对照评测与夜间整合 P2。

### A07 任务上下文与历史
- **整合**：C13 会话导入与 @session、M15、D19 跨会话引用＋检索工具、MAG17 跨 agent 会话搜索、C14/N04/D05 fork、D30 标题生成、AA 标题推导、C29 标签大纲导出、AA22 导出、COM13 /btw、N14 transfer。
- **原生功能**：托管 home 内会话发现与增量指纹索引；全文搜索；会话标题自动推导（首条消息折叠后 48 字，或模型生成）；把"历史会话检索"做成 Aeroric 自带 MCP 写入托管 mcp.json，四族都能引用旧会话；导出 Markdown/JSON（含终端转录与 hook 事件）；fork 只对支持的族开放，其余如实不可用。
- **owner**：core `sessions`。**Mac**：侧栏搜索、会话菜单导出。**iOS**：搜索与查看。
- **优先级**：标题＋导出 P0；搜索 P1；检索 MCP P1；fork P2。

### A08 项目知识与记忆
- **整合**：COM09 memdir 四类记忆、COM10 agent 级记忆、L09 置信度降级、L10 配额阶梯、L11 回合后提取、L12 日记、M18 写入总开关、X16 记忆工具、Z15 子代理记忆、F09、D31 笔记归档。
- **原生功能**：记忆浏览/编辑/审计界面（覆盖 Claude 原生 MEMORY.md 与 Aeroric 托管记忆）；新记忆以**候选**出现，带置信度与原文引用，无引用自动降级；每范围上限与五级配额横幅，不静默归档；全局"禁止自动写记忆"开关；外发或共享前密钥扫描。
- **owner**：独立知识 owner（沿用 Q41–Q45）。**Mac**：设置 → 记忆。**iOS**：审核候选。
- **优先级**：P1。

### A09 工具与扩展资源
- **整合**：C02 中央技能仓＋按 agent 链接四态、D26 多来源技能注册表、L20/X26 技能管理与 Hub、C34/L16 MCP 管理与 OAuth、D25/PT13 一份规则投影多宿主、CCS11 深链导入、AA25 斜杠命令目录、X12 生命周期。
- **原生功能**：技能与 MCP 的统一库；按族投影到托管 home（Claude `.claude/skills`、Codex、omp、dsh 各自目录）；链接状态四态（已链接 / 他处已有 / 被阻 / 损坏）；MCP OAuth 用 ASWebAuthenticationSession、token 进 Keychain；`aeroric://` 深链导入（预览确认、不自动启用）。
- **owner**：core `settings`/`extensions`/`agents`（ResourceRevision、DeploymentPlan/Receipt）。**Mac**：设置 → 技能与 MCP。**iOS**：只读。
- **优先级**：P0（配方依赖它）。

### A10 角色配置与环境方案
- **整合**：旧 CCS/MAG/CPA 账号与连接（Q124–Q140）、D24 权限预设、Z10/K18 角色工具白名单、N12 explore 子代理、COM16 agent 目录、RUN14 三层提示词、P41 主 agent、AA26/AA35 会话内切换与多实例、模型网关与自定义模型。
- **原生功能**：**内建 Harness 的真实配置目录**（模型、权限、思考档，当前前端发送被阻断的根因）；角色库（只读调研 / 实现 / 审查 / 反思 / 治理），每个角色绑定族＋模型＋权限档；提示词分平台层 / 团队约定层 / 角色层组装，按族投影到 CLAUDE.md / AGENTS.md / 位置参数。
- **owner**：RoleBinding（凭据只存受保护引用）。**Mac**：新建会话配置胶囊＋设置 → Agent。**iOS**：只读；改权限档需 Mac 确认。
- **优先级**：内建配置目录 P0；角色库 P1。

### A11 连接弹性与协议适配
- **整合**：AA04 能力三元组、AA08 游标恢复、AA13 发送状态分离、HER14 版本不兼容策略、D22 重试、CPA 路由、F11 协议互转、F15 外部事件桥。
- **原生功能**：**能力三元组**（supported / available / allowed＋稳定原因码＋revision）定义在 aeroric-protocol，Mac 与 iOS 所有按钮的可用性只看它（顺带消除四族 switch 穷尽点）；远程协议版本协商，低于下限提示升级 Mac App。
- **owner**：aeroric-protocol＋已有 local_router。
- **优先级**：能力三元组 P0；其余随 A18。
- **Agent 配置与网关**：详见 §4.5（Q23–Q32）。

## 4.5 Agent 配置、模型路由与号池（Q23–Q32，2026-10-23）

证据：`e3836e62:docs/research/agent-config-review-2026-10-23.md`（cc-switch / magpie / CLIProxyAPI）、`e3836e62:docs/research/agent-config-aeroric-state-2026-10-23.md`（Aeroric 现状）。Q128–Q140 的连接界面决定继续有效。

**五层对象**
1. **连接**：一个账号或一个 API 端点。来源：官方 OAuth、本机识别后关联（Q132）、手填 key＋base URL。协议类型三选一：**Responses / Messages / Chat Completions**。凭据只进 Keychain，settings 只存引用（Q28：首次启动迁移现有明文后清除）。
2. **模型目录**：挂在连接上，单份带版本（Q133）；上游 ID、映射名、显示名、上下文、输出、输入类型、自定义推理档位分开（Q134/Q135）。
3. **模型路由（Q25）**：以**映射名**为键，汇集所有映射到它的渠道（连接＋上游模型 ID）。例：三个 API 都把各自模型映射为 `deepseek-v4.1-flash`，选这个模型即得到三条渠道；一条不可用自动切下一条。切换判据参考 cc-switch / magpie：只在上游开始输出前切；400/405/413/422 等请求错误不切；熔断（连续失败、半开探测）；流式首包前缓冲。
4. **号池（Q24-b / Q26-b，参考 CLIProxyAPI）**：独立对象、全局共享冷却状态（Q31）；可含第三方 key **与官方账号**。策略全做：优先级分桶、轮询、加权轮询、先用满、最少使用、按节奏、免费优先；会话粘滞（子代理继承父会话账号）；分错误类型冷却（429 按 Retry-After、401/403 30 分钟、404 12 小时、invalid_grant 30 分钟）；401 先尝试刷新 token。
5. **Agent 配置（Q30-a）**：Harness ＋ 连接 / 模型路由 / 号池 ＋ 推理档映射。连接与 Harness 无关；新建会话选 Harness 后列出全部配置：协议原生兼容可直接选，需要网关转换的标"经网关转换"，无法转换的禁用并写原因。

**网关（Q23-a / Q29-a）**：复用 core 的 `local_router`，改由原生宿主按需启动；只绑 127.0.0.1，每任务独立 token；补齐三协议互转（Responses ⇄ Messages ⇄ Chat Completions）。直连单一上游的任务不经过网关。

**任务快照（Q27-a）**：创建时固定配置快照，修改只影响新任务；撤销 key、删除账号立即生效。

**首发范围（Q32-a）**：第三方配置、模型路由、号池首发 Claude Code 与 Codex；omp、dsh 下一批接入网关，期间只支持直连配置。

**不照搬**：CPA 默认绑全部网卡、cc-switch 普通路由无鉴权、magpie 的 `--dangerously-skip-permissions` 订阅桥接。官方账号入池的伪装方式见 §7 Q33。

### A12 工作安排与 typed 流程
- **整合**：C05 自动化、P10 并发策略、D16 定时（一次/间隔/每日/每周/cron＋补发最近一次）、M14 cron/loop、X23 活跃时段与忙时排队、L13 规划日历、P36 外部等待 monitor、Z17 动态并发、D13（改为 typed 流程，不执行模型写的 JS）。
- **原生功能**：定时任务由 Mac host 触发（iOS 只编辑与看历史）；错过只补最近一次；会话忙时排队；可选写入系统日历（EventKit）；7 个自动化模板作为配方。
- **owner**：core `tasks`（AutomationRule/Run）。
- **优先级**：P2。

### A13 统计与资源护栏
- **整合**：C23 token 用量与热力图、F12 计费规则、P05 多作用域预算（公司/Agent/项目，未计价默认阻断）、P25 每 agent 并发、P26 无进展冷却、D14/X20 重复调用与失控防护、N11 最大步数硬停＋收尾总结、D20 上下文压力、M16 按模型压缩点、AA20 日志中心。
- **原生功能**：用量按项目/族/模型/会话聚合，费用标"按规则估算"；订阅额度与金额分开；失控检测（精确重复、轮询、ABAB 振荡）只提醒并可一键 steer；到预算上限先注入"收尾总结"再停；诊断日志一键脱敏导出。
- **owner**：core `metrics`（BudgetAdmission 与 UsageObservation 分开）。
- **优先级**：日志导出 P0；失控提醒 P1；预算 P1（与 A03 同批，准入护栏须在多 agent 上线前到位）。

### A14 连续执行、活性与恢复
- **整合**：P14 非终态活性契约、P13 blocked 可路由、P23/O14 静默不判死、M12 Try-best 卡死检测、X19 无进展熔断、D02 Goal（重启后 disarm）、D10 Ralph 全新上下文循环、D17 先落盘再生效、AA18 Claude 空闲回收与 Cron 保活、AA19 单实例租约（pid＋启动时间）、MAG15/L26 防休眠、MAG16/CCS14 登录自启、RUN18 Stop 可恢复与 Archive 只读、P43 暂停语义、O12 "未知"态、AA17 Codex 写锁判据。
- **原生功能**：
  1. 每个未结束任务必须处于"在跑 / 明确在等谁 / 有恢复动作"之一，否则标"需要处理"——直接解决"永远 running"。
  2. 静默 5 分钟可疑、15 分钟严重，只提示，不自动杀、不换人。
  3. 卡死检测（重复编辑、重复重试、动作连击）→ 生成待回应项并推送。
  4. 运行期间 IOPMAssertion 防休眠，任务卡显示"正在阻止睡眠"；登录项自启用 SMAppService（显示待批准状态）。
  5. host 单实例租约；导入外部 Codex 会话时按 thread-writer-lock 判断，找不到 owner 就只读。
  6. 停止：SIGINT → 宽限 → SIGKILL；Stop 可恢复，归档只读。
  7. Ralph 循环（P2）：每轮新 PTY 任务，只交接有界报告。
- **owner**：aeroric-host、runtime/terminal（ExecutionRef、ResumePlan）。
- **优先级**：活性＋静默提示＋防休眠＋单实例 P0；卡死检测 P1；Goal/Ralph P2。

### A15 项目检查与安全变更
- **整合**：C06/D27 worktree 隔离、C10 Forge（Issue/PR）、N05 快照撤销、L22 子代理 worktree 合并决策、C27 三栏合并、AA24 Git 动作徽标。
- **原生功能**：任务启动可选独立 worktree；两段合并（先持久化合并意图）；Git 工具页接线（宿主已有 40 个 git 方法）。
- **优先级**：Git 页接线 P1；worktree 选项 P1；Forge P2。

### A16 原生工作台与事实同步
- **整合**：AA10 revision 栅栏、O11 幂等回执、AA12 本地缓存、AA27 IME 安全输入、AA28 流式性能预算、HER04 终端尺寸单一 owner、HER18/19 复制与剪贴板、CCS12/13 菜单栏与轻量模式、C30 URL scheme、RUN22 直接对话、L14 语音输入。
- **原生功能**：菜单栏常驻（运行中/待回应数、远程在线、阻止睡眠），关窗 host 继续；`aeroric://` 深链直达会话；所有远程动作带 request_id 幂等回执；中文输入安全（marked text 阻断发送）；系统 Speech 语音输入（P2）。
- **优先级**：菜单栏＋深链 P1。

### A17 权限、审计与外发
- **整合**：D09 Auto review（三档风险，高危恒拒）、P04 审批、P31 分派授权、P42 外发调用审查、O17 信任预设、COM11 密钥扫描、AA05 接管（默认只读）、AA09 一次性 WS 票据、X24 诊断包脱敏。
- **原生功能**：远程默认观察，显式接管才能写；Auto review 仅在 hook 可拦截的族上作为实验开关，高危恒拒；记忆、日志、分享外发前密钥扫描。
- **优先级**：接管 P0（随 A18）；Auto review P2。

### A18 远端 Host 与移动端控制（扩展：配对 / 连接同步 / 远程终端 / 远程审批）
- **整合**：AA01–AA03 配对与设备管理、AA08 游标恢复、AA14 文件、AA15 终端协议、AA29 本地网络、AA32 前后台生命周期、HER13 SSH 已存远端、CPA19 反向中继、L15 隧道、C22 聊天渠道、Aeroric 已有 `remote/`（E2EE、两阶段配对、push）。
- **原生功能**：见 §4.4。
- **优先级**：P0（iOS 上架的前提）。

### A19 待回应与态势通知
- **整合**：P37 "需要我处理"视图、AA06 卡片 dock、RUN17 观察对话、O15 移动通知（重连补发、水位）、X22 问卷到期自动回复、HER16 OSC 9/99、C35 提示音、Aeroric `remote/push.rs` 白名单载荷。
- **原生功能**：Mac 与 iOS 共用一个"需要我处理"队列；APNs 只推 done / failed / input_required / 可能卡住，载荷不含内容，点开深链到卡片，批准必须回到已认证连接；agent 发出的 OSC 9 被 host 捕获转为通知；Live Activity 显示长任务（P2）。
- **优先级**：P0。

### A20 配置迁移与备份恢复
- **整合**：C24 加密备份（Argon2id＋AES-GCM）与分域同步、F14 系统备份、L24 WebDAV、MAG20 last-good、N14 transfer、P44 导入导出、HER20 版本公告已读。
- **原生功能**：分域导出（供应商、Agent 设置、配方、快捷消息、任务模板）带回滚快照；传输走 iCloud Drive 或文件；写坏配置一键回到上次可用。App Store 版不做自更新。
- **优先级**：P2。

## 4. 四条主线的原生设计

### 4.1 配方库：专家 / 日常办公 / 科学研究

**来源事实（codeg）**：三类都是同一机制——SKILL.md 技能包＋欢迎页卡片＋点击注入"技能徽标＋本地化提示词模板"（专家只注入徽标）。技能先进中央仓，再按 agent 链接启用。

**Aeroric 首批目录草案**（把 Q122 的五组十六项与 codeg 36 项合并；条目名为暂定，正文自写）：

| 组 | 配方 | 来源 | 依赖 |
|---|---|---|---|
| 代码开发（专家） | 头脑风暴与设计、编写计划、按计划执行、子代理驱动开发、并行派发、TDD、完成前验证、系统化调试、请求审查、处理审查意见、收尾分支、编写技能 | codeg E1–E14（superpowers） | 无 |
| 代码开发（例行） | 代码审查、依赖更新、测试覆盖、TODO 清理、CI 排查、发布说明、安全审计、项目体检（含技术债） | codeg 自动化模板、Q122 | 无 |
| 日常办公 | Word 文档、Excel 工作簿、PPT 演示、融资路演、学术论文、财务模型、数据仪表盘、Morph 动画 PPT | codeg O1–O9、MiMo docx/pptx/xlsx/pdf | **办公引擎（见 Q16）** |
| 科学研究 | 科学头脑风暴、假设生成、实验设计、统计功效、统计分析、探索性数据分析、科学可视化、批判性思维、论文检索、同行评审、引用管理、学术评估、科学示意图 | codeg R1–R13（K-Dense MIT）、MiMo deep-research/arxiv | 11 项需 Python/uv；示意图需模型 key |
| 能力沉淀 | 复盘（retro）、技能编辑（refiner）、新建技能（creator） | X13/X14/X17、Q122 | 无 |
| 轻量问答 | 直接提问 | Q122 | 无 |

**设计**：
1. manifest 在 core；注入方式按族区分（Codex `$id`、其他 `/id` 或位置参数首条消息），模板只预填输入框，用户可改，不自动发送。
2. 环境依赖由 host 探测（Python、uv、officecli、key），显示具名缺失与"一键安装"（Mac 官网版可执行安装；以后 MAS 版只显示说明）。
3. 配方不改模型和权限（Q115）；与已选能力冲突时提示，不静默替代（Q116）。
4. Mac 入口：新建会话**加号菜单**里的"工作配方"→ 按组 Tab 的卡片面板；首页可以放 3 张高频大卡。

### 4.2 多 Agent 协作

**取法**：runner 的团队形态 ＋ orca 的协作原语 ＋ paperclip 的活性/审查/预算契约。

1. **团队** = 角色模板快照＋**恰好一个 lead**；lead 是用户唯一对口，iOS 默认只和 lead 与待回应卡打交道。成员绑定四族之一＋模型＋权限档；成员不能再建团队（深度 1）。
2. **工作包**是唯一协作单元（A02），单负责人。
3. **领取**：core 单写者 CAS `(工作包, 预期状态, 负责人∈{空,我}, run 锁∈{空,我的 run})`；冲突返回当前 owner，提示词规定"冲突即停、不重试"；run 结束只清仍指向自己的锁。
4. **委派**：host 提供 Aeroric MCP 工具（delegate / status / cancel / complete / progress），写入托管 mcp.json，四族都能调；子任务就是普通 PTY 任务，带 parent 链；子任务卡在用户那里时向上冒泡 `blocked_on`；消息里 @Agent 即显式委派。
5. **消息**：挂在工作包上，独立投递状态（Q14 待定）；注入走 A04 就绪闸。
6. **写边界**：成员只改自己工作包及子项，向上只发评论或完成信号，横向协作新建指派工作包。
7. **审查**：政策三选一，退修 3 轮升级给用户；可选看门狗角色复核"假完成"。
8. **护栏**：每成员并发上限、无进展冷却、派发失败 3 次熔断、预算硬停（与 A13 同批上线）。
9. **人工介入**：审批卡、问题卡、"升级给你"、暂停/取消整棵任务树，全部带幂等回执。
10. 进阶（P2）：多候选方案由裁判或用户挑选（M11/F04），成本高，须预算。

**与新建会话的关系**：现有"团队协作"能力卡开启后，展开 lead＋成员的角色绑定（沿用主配置或单独指定）。

### 4.3 自进化（U01 ＋ minimax 管线）

**原则**：只改托管物（托管技能目录、按族按模型分版本的托管指令文件、角色定义、托管记忆、MCP 配置），**绝不改四族内核或用户自己的目录**；任何变更必须经用户批准，可回滚。

1. **信号层**：统一 Signal 记录，来源为 hook 事件（重复调用、失败、权限拒绝、Stop 时状态）、会话文件（重试、报错、用户纠正）、结构化反馈（A04 七类）、verifier 与检查结论（A05）、卡死与失控检测（A14/A13）。每条带证据引用与指纹，同指纹冷却 24h。
2. **提案层**，两条触发：
   - 会话结束反思：用户开启后，Stop 后经 PTY 让原 agent 写 `proposal.md` 到托管目录（会话够长才触发，参考 X11 的 ≥40 条消息）；不在回合内频繁提醒（minimax v3 已删除此做法的反面经验）。
   - 空闲整合：Mac 空闲时起独立"技能编辑"任务批量处理信号，产出 patch（X09 夜间 / M01 Dream / COM12）。
3. **编辑规则**作为内置配方（能力沉淀组）：先归因（技能写错 vs agent 没遵守）、最小 patch、问题/证据/理由三段、禁止自指、新建前先查重。
4. **评估门**（P2）：同一 prompt 起两个任务对照"启用/不启用"；评估器先过自测（PT12）；报告失败、超时、未检查项。
5. **审批与版本**：提案收件箱（Mac）与推送（iOS）；批准后生成新 revision，可一键回滚（补上 minimax revert 未实现的缺口）。
6. **生命周期**：使用计数来自 hook 的技能加载事件；闲置 15 天建议标过期、30 天建议归档，不自动删除。
7. **记忆沉淀**：候选带置信度与原文引用（A08），无引用降级。
8. **U01 三角色**：启用"任务工具进化"时，主执行 / 阶段反思（只读、模型独立）/ 治理（独立进程）分离，监督不可关闭（Q13–Q26 原样保留）；上面 1–7 是三角色的工具链。
9. 四族差异如实呈现：Claude 可经 hook additionalContext 注入提醒，其他族降级为 UI 提示，靠能力三元组表达。

**补充：Xiaomi MiMo 桌面版三角色实现的借鉴**（`e3836e62:docs/research/xiaomi-mimo-self-evolution.md`，静态核验，未运行）：
10. **代码与状态分离**（germline / soma）：托管物版本库与运行状态分目录；每次进化在任务专属克隆里进行，克隆先建在临时目录、配置完成后原子 rename。
11. **反思者只看宿主采样的有界证据**：git status、最近提交、错误日志尾部、规格检查摘要，各有字符上限；无工具；输出固定 `criticize / praise / quiet`，每次最多一个主问题，`quiet` 不打断主执行。
12. **反馈投递链**：待投递 → 注入 → 送达标记，并明确"送达 ≠ 采纳 ≠ 修复"，与 Q14 的投递状态一致。
13. **调度**：反思有冷却（90s）、空闲触发与兜底（300s）；治理约每小时，都只在主循环检查点触发，不当作严格定时服务。
14. **治理者审查清单**：麻木（有问题却长期 quiet）、空转（同一批评只换措辞）、错误处方（照做后没改善）、自我污染、证据不对称、规格漂移。
15. **监督管道自检**：治理结果丢失、截断、长期沉默时发健康告警。
16. **补上 MiMo 的缺口**（反面经验）：
    - 治理意见在 MiMo 没有自动落实闭环 → Aeroric 把每条治理处方转为提案，带"处方回执"（是否执行、效果是否验证）。
    - 改后检查只发警告、覆盖失败会丢旧版本、崩溃重启仍加载坏代码 → Aeroric 采用事务式应用：评估不过不生效，失败回到 last-known-good revision。
    - 凭据有回退进入子进程的路径 → 进化任务只经宿主代理拿凭据，不留回退。
    - 授权在主进程校验（`consentAt`），不只是 UI 隐藏 → Aeroric 由 core 校验开关。

### 4.4 iOS 远程控制（App Store 主线）

1. **执行只在 Mac**，Mac host 是唯一单写者；iOS 只发 typed 意图（输入、批准/拒绝、中断、打开文件），绝不下发任意 shell。
2. **配对**：Mac 显示二维码（版本化载荷：host 公钥、地址、中继、nonce、≤120s），iPhone 扫码 → 握手 → **Mac 端弹窗确认设备名与指纹** → 入设备表；支持吊销、轮换；配对码用后作废。已有 `remote/` 的两阶段配对与 E2EE 直接复用。无账号体系。
3. **连接**：同一套 E2EE 协议三档——局域网 Bonjour 发现（需声明 `NSBonjourServices`，手填地址为降级）；外网经中继（Q15 待定）；自托管公网可选。
4. **同步**：每任务单调 seq 事件日志；回前台先 `recover(after=cursor)`，缺口或 host 重启则重拉快照；列表用 revision 栅栏；按钮可用性只看能力三元组。
5. **审批/提问**：卡片只呈现 host 从 hook 得到的事实；按钮由 host 映射为按键注入 PTY；提交三态含 uncertain；多端竞争以 host 为准，后到者显示"已由其他设备处理"。
6. **终端**：默认镜像观察，尺寸 owner 在 Mac，iOS 缩放显示；接管后才可输入，并提供 ESC/TAB/CTRL/方向键快捷行；断线按 fromSeq 续传。
7. **通知**：APNs 白名单载荷（已有 `remote/push.rs`），通知只做唤醒与深链。AA 的 iOS 没有推送，这是 Aeroric 的差异化。
8. **文件与改动**：浏览、QuickLook 预览、回合 diff；远程写文件默认关闭。
9. **审核准备**：审核备注说明控制的是用户自己配对的 Mac；提供演示模式或演示 host；本地网络、相机、推送用途说明；隐私标签申报缓存；终端交互输入放在接管之后。

## 5. 实现路径（建议，待确认）

按"先能真正用起来，再叠加差异化"切成纵向批次；每批独立可验收（Q7-a）。

| 批次 | 内容 | 主要族 |
|---|---|---|
| **B1 闭环可用** | 内建 Harness 真实配置目录并解除发送阻断；能力三元组进 protocol；未接通能力的显示方式（Q8）；回合改动＋自动检查；会话标题与导出；活性提示＋防休眠＋单实例租约；诊断日志导出；Git/文件/运维工具页与 Git 日志接线 | A10 A11 A05 A07 A14 A13 A15 |
| **B2 配方库** | 技能/MCP 统一库与按族投影；配方 manifest 与首批目录；诚实启用状态；依赖探测；加号菜单配方面板 | A09 A01 |
| **B3 iOS 远程 MVP** | 扫码配对＋Mac 确认；局域网连接；游标恢复；任务列表与观察；待回应卡；APNs；接管 | A18 A19 A17 |
| **B4 协作** | 工作包＋计划拆解；团队与 lead；原子领取；Aeroric MCP 委派工具；消息与就绪闸；审查政策；预算与并发护栏 | A02 A03 A04 A05 A13 |
| **B5 自进化与记忆** | 信号；会话结束反思提案；提案收件箱、审批、版本、回滚；记忆候选与置信度；U01 三角色接入 | A06 A08 |
| **B6 及以后** | 外网中继、iOS 终端输入、定时与日历、对照评测、Ralph/Goal、worktree 选项、备份同步、Forge、数据库/笔记工具页 | A12 A14 A15 A18 A20 |

## 6. 明确不做（或冻结）

- Computer Use（C21）、任意 JS 工作流（D13/M19）、JSX Studio（F19）、桌面宠物（C31）、推测执行（COM14）、自定义 CSS 主题（C32）。
- 独立 IDE：LSP 会话、DAP 调试器、补全、大纲（冻结）。
- iOS 上的任意 shell 任务（AA16）与默认开启的远程写文件。
- 账号体系、公开分享链接服务（AA21 降级为导出＋系统分享）、Codex IPC 私有协议 follower（AA17 只借写锁判据）。
- 内核层能力：工具入参修复（N10）、多级压缩（COM17）、工具结果修剪（D21）——四族自理，Aeroric 不碰。
- App Store 版的自更新（HER20）。

## 7. 本轮决定（2026-10-23）

| 问题 | 决定 |
|---|---|
| Q8 未接通能力的显示 | 禁用并标"即将支持"，不阻断发送 |
| Q12′ 许可范围 | 原生端（macos/ios/core/协议）改许可；旧 Tauri 端不再保留，直接清理（前提：原生代码非逐行移植旧端） |
| Q13 许可类型 | **专有（闭源）** |
| Q14 Agent 间消息 | 消息挂在工作包上，独立记录投递状态（已存/已投递/已读/已采纳） |
| Q15 外网中继 | 首发局域网（Bonjour）＋用户自建中继入口；产品方无状态中继留作上架后的**订阅收费项** |
| Q16 办公引擎 | Python 库（python-docx / openpyxl / python-pptx），与科研共用 Python/uv 环境 |
| Q17 批次顺序 | 按 §5：B1 → B2 → B3 → B4 → B5 → B6 |
| Q18 首批配方 | 首发代码开发（专家＋例行）与轻量问答；办公、科研随环境依赖进第二批 |
| Q19 自进化默认 | 会话结束反思默认关闭，按项目开启 |
| Q20–Q22 清理 | 删除旧 Tauri/移动端与杂项；docs 归档被取代稿件（已执行，未提交） |
| Q23 网关 | 内置本机网关，按需启动，127.0.0.1＋每任务 token |
| Q24 官方账号入池 | **允许**（用户决定；已提示条款与封号风险），号池参考 CLIProxyAPI |
| Q25 故障转移 | 按映射名聚合多渠道自动切换，参考 cc-switch / magpie |
| Q26 号池策略 | 全部策略首发 |
| Q27 配置变更 | 任务快照；只影响新任务，安全撤销立即生效 |
| Q28 凭据 | 迁入 Keychain，清除明文 |
| Q29 网关实现 | 复用扩展 `local_router` |
| Q30 组合 | 连接与 Harness 解耦；三协议 Responses / Messages / Chat Completions，可经网关转换 |
| Q31 号池归属 | 独立对象，冷却全局共享 |
| Q32 首发族 | Claude Code、Codex；omp、dsh 下一批 |
| Q33 官方账号登录与入池 | **照搬 CLIProxyAPI**（官方 client ID 的 PKCE 登录＋请求伪装）；用户在已被告知条款与封号风险后决定；默认关闭，按账号显式开启并在界面标注风险 |
| Q34 跨协议切换 | 允许，网关在 Responses / Messages / Chat Completions 间转换，会话粘滞优先 |
| Q36 `git/service.rs` 未提交改动 | 不动，等原工作收尾 |
| Q37 旧自定义 agent 明文 key 文件 | 一次性迁移为新连接＋Agent 配置（key 入 Keychain）；迁移并验证成功后再删启动脚本与 key 文件；失败条目原样保留并标原因；排在 4c/4d 之后、v4 打包之前 |
| Q35 开工方式 | Trellis 任务 `10-23-native-feature-wiring-01`；后端线直接开始；**界面不出浏览器稿，用户直接审 App 效果**（取代 2026-10-20 逐屏浏览器稿规则） |

## 7.1 上游复查后的补充决定（2026-10-26，R1–R12）

证据：`e3836e62:docs/research/agent-config-upstream-recheck-2026-10-26-{cc-switch,cliproxyapi,magpie}.md`（对照 cc-switch e8e1cfbf、CLIProxyAPI 30780351、magpie c158b043）。§4.5 主体不变，以下为补充；实现见 Trellis `10-26-gateway-pool-fixes`（H）。

| 编号 | 决定 |
|---|---|
| R1 缺陷修复 | 直接修：Responses 流的 metadata/keepalive 不算已提交；`model_context_window_exceeded` → length/incomplete、超长 prompt 补 `context_length_exceeded`；Retry-After 支持 HTTP 日期；桥接保留 cached_tokens；tool_call 缺 index 按 id/位置分配；推理摘要尾巴补发、工具输出图片抽为图片部分；`compatible-mode/v1/models` 候选、Codex 会话头、Claude Keychain 先 `-a $USER`、导入 Codex 读 `openai_base_url`；第三方/网关 Codex 托管 home 无 ChatGPT 登录的不变量测试；宿主启动抬 `RLIMIT_NOFILE`；Key 输入框关自动大写/更正 |
| R2 流内错误 | 按 type/code 分类：请求错误、安全拒绝不切不冷却；限流/额度按 RateLimited |
| R3 冷却粒度 | 冷却键为 连接＋可选模型：404/模型不服务、首包超时冷却 (连接, 模型)；鉴权、整体限流冷却连接；首包超时第一次只切换，30 分钟内第二次才冷却，1 分钟起翻倍 |
| R4 粘滞改绑 | 只在限流/额度/鉴权类错误后改绑；瞬时错误仅本次换号；子代理继承父会话账号但不覆盖 |
| R5 改 key 后 | 清鉴权类与 404 类冷却，保留限流冷却 |
| R6 号池策略 | 402 冷却 30 分钟；平滑加权轮询、权重上限 100；轮询只在最高可用优先级内 |
| R7 跨厂商切换 | 目标为 OpenAI/ChatGPT 时去掉非本家 reasoning；其他上游被拒一次后记住并去掉（Q34 的前提） |
| R8 跨映射名降级 | 首发不做；下一批：Agent 配置可设有序备用模型，界面标“降级” |
| R9 限流就暂停 | 连接级 `rest_on_limit`，默认开 |
| R10 按模型协议 | ModelEntry 可选 `protocols` 覆盖，路由先模型后连接 |
| R11 Codex tool_mode | 先核实 bundled 模板；如有，同一映射名只在同 tool_mode 组内自动切换 |
| R12 补全与档位 | 模型信息补全只朝“支持”方向、不覆盖用户值、不借别家条目；推理档位收窄对已知模型只影响选择器，未知模型按用户声明适配 |
| Q33 补充 | 同一账号只有一个刷新方（Aeroric 登录的号由 Aeroric 独占刷新；本机关联的号只读或在 CLI 锁下刷新）；刷新加文件锁、拿锁后重读、以失效 token 为键；读 auth.json 失败判“未知”；身份按 `sub`；Codex 登录浏览器 PKCE 为主、device code 兜底，1455 端口冲突报错不抢，state 严格校验；401 按 token 值记录、刷新回同一 token 不算成功、凭据变化才清 401/invalid_grant；可按账号设代理 |

## 8. 对旧版的勘误

- codeg 三 Tab 是"代码开发 / 日常办公 / 科学研究"，不是"构思 / 文档 / 科研"；实际 36 项（专家 14、办公 9、科研 13），旧版漏列专家，且未写 officecli 与 needs_env/needs_key 依赖。
- paperclip 预算作用域是公司 / Agent / 项目，**没有"团队"**；旧版漏了"未计价用量默认阻断"、"409 即停不重试"与终态条件清锁。
- deepseek-harness "默认 256 轮"属于 Ralph 循环，不是 Goal。
- minimax-code 的整套自进化（skill-evolve、refiner、creator、retro、记忆提醒、生命周期）、verifier、失控防护、托管审查旧版全部缺失；其本地 revert **未实现**。
- CometixCode 的 auto-dream 只是骨架，无生产调用者。
- Agents-Anywhere 旧版完全未收录；其 iOS 端**没有终端、没有推送**。
- 旧版对"Mac 常驻 host、iOS 远程、App Store 沙盒"几乎没有覆盖；防休眠（MAG15）、SMAppService 自启（MAG16）、菜单栏（CCS12）均漏。
- runner RUN11 原生通知条目与其 vision"原生 OS 通知未发布"冲突，待复核。

## 9. v3 增补（2026-10-30）

前沿调研与决定见 [`frontier-roadmap.md`](frontier-roadmap.md)（§10 Q-F1–Q-F19 已确认）。本节只登记它对上文的修订，上文原文不改。

| 修订 | 被修订处 | 新决定 |
|---|---|---|
| R-1 | §4.2-1「恰好一个 lead」 | 不设 lead：用户正在对话的主会话即协调者（manager），成员共享目标与约定，自查名单（依据 runner #873 去 lead） |
| R-2 | A08 记忆候选 | 记忆=事实＋证据＋画像；写入只追加，取代以标记表示，模型只能提议 |
| R-3 | A07 检索 MCP | 合并为一个 Aeroric Context MCP（历史/记忆/知识/协作/交接），默认只开历史＋交接 |
| R-4 | A06 信号 | 以 MAST 14 类失效模式为信号标签与治理审查清单 |
| R-5 | §4.2-4 委派 | 委派必须带验收条款＋权限子集＋上下文模式（fork/isolated/brief） |
| R-6 | A04 待回应卡 | 记录 `suspendable`：仅外部调用之前的交互可跨重启等待 |
| 顺序 | §5 批次 | B4 之前插入 **B4′ 交接与上下文**；之后 B4 协作 → B5 记忆 → B5.5 项目 Wiki |
