# Aeroric 原生能力借鉴候选库

**最新接续（2026-10-22）**：用户已同意先用新建会话建立原生保真闭环，
见§43及 `new-session-native-fidelity-01.md`；不新增来源、功能族或第二套UI。
短包已准备、执行者尚未ACK，原生视觉仍NOT RUN / DEFERRED，
不扩大已审页面或UI/VM权限。下方为此前接续。

**当前接续（2026-10-21）**：Q114–Q123已确认；接续V6紧凑胶囊/石墨深色
修订，项目仅名称/去框/靠近输入、Harness与输入更扁、箭头与侧栏统一；
V6新建会话主屏布局已获用户视觉确认（§25.1）；网页未完成原生玻璃滑块，
不追认其他会话面板、账号七面逐屏通过。
统一原生还原交接已按用户新要求整理为
`approved-ui-native-integration-stage-01.md` v1.0（§26），
本线程不实施、不启动App，账号与连接继续设计。
Q128可选首个Agent配置引导、Q129分项状态表达已确认；V3账号主屏已确认，
V4添加面板修订中未记通过；Q132本机识别/关联、
Q133共享模型规则/版本传播、Q134目录读取与调用测试分开已确认；
Q135自定义推理档位、Q136额度展示/自动刷新默认已确认；
Q137批量导入预检/冲突默认、Q138网页登录/回调与原版登录分流已确认；
Q139连接代理默认/失败处理与草稿返回已确认；
Q140上轮展示的可选批量创建各Harness独立Agent配置及会话可选衔接原则已确认（§40）。
当前无新问题待答。用户另以“确认通过”批准本次送审、审阅的V5浏览器视觉稿，
见§42及 `docs/native/add-connection-design-review-v5.md` §7；
不虚构十个场景各自的批准，未展示/未审状态仍待审。
其他独立面仍逐屏待审，
不扩统一原生执行包。
三来源补查见§22，页面接续见§23–25；
当前仍为**18仓库来源、
185条仓库观察＋U01＝186条**。§9保留当时168条的20族基线映射，
新增18条的唯一主归属见§22.3，共用原8闭环，不再造同义系统。
Q124按推荐分层继续出稿，不扩读为全部功能/池策略或原生实施批准。
下方“当前总览”“最新决定”及§12–22为当时记录，不作为当前计数或审批。

**当前总览（2026-10-20）**：已完成本地backlog全部7个新项目的分层源码研究，
新增78条来源观察；合计17个仓库来源、167条来源观察＋U01。
“全部项目覆盖”不表示逐文件读完、运行验证或逐项批准。
首轮10来源/89条及Q13–Q113相关决定均保留；历史计数不代表当前总量。
新增证据见§7；全库20个能力族的归并、原生落点和实现路径建议见§8–11，
**均待讨论，不是20个新页面或实施授权**。

**最新决定（2026-10-20）**：用户已确认Q114的“四层组织原则”：
工作配方库、可组合能力、独立执行/角色/权限/资源配置、执行后的任务工作面。
§11“尚待回答”为当时提问的历史状态，以§12续记为准。
深度归并及原生落地候选见
`docs/native/capability-integration-design-checkpoint-01.md`；
其中新增对象、机制、默认和Q115仍待讨论，不随Q114一并批准。

研究日期：2026-10-14 至 2026-10-15。状态：**研究与讨论材料，
不是全库实施授权；Q26、Q35、Q45、Q50分别批准独立离线原生审阅样例范围**。

这不是阶段02执行提示词。新终端的模型可以读它理解候选方向，但不能据此
改产品、启动Agent、创建工作树、扩大权限或改写阶段01。

**现行基线澄清(2026-10-16)**:现行产品最低系统以 `MIGRATION_PLAN.md`
顶部 Apple-only 基线 §B“系统与架构矩阵”为准——最低 macOS26;本次验证
目标为 Apple Silicon/macOS27 与 Intel/macOS26,不建立 Intel/macOS27 组合;
同源确认见 `docs/native/adr/039-distribution-pivot-macos-ios.md` §7。
冻结快照与既有 spec 内的 macOS14 字样是历史措辞,不覆盖本条。产品政策
不等于现有 deployment 设置已迁移。本条仅为文档澄清:不改 deployment、
签名、CI,不改变已批准样例范围,不授权实施;冻结历史文件保持原样。

**讨论进度（2026-10-19）**：本库始终是10个来源项目、89条来源观察＋U01
的总清单，不因当前先讨论自进化而缩减。U01 的 Q13–Q25 已部分收口为
设计决定；Q27–Q29另确认协作围绕任务按需组队，默认主Agent兼任协调，
不强制公司/部门或另开经理Agent，若启用U01则监督与协调职责分离。
委派用简短、可追溯工作包＋必要的获准资料引用，不等于执行授权。
Q30用户要求授权可配置：保留默认有限授权，另可手动开启Full Access，
使授权覆盖的操作/委派不逐次询问；已确认默认仅当前任务生效、不自动
影响其他任务。Q31确认协作者继承任务确认策略、只得工作包所需且不超
委派方有效授权的权限；Full Access覆盖内不重复询问，不默认复制全权/密钥。
Q32确认独立工作并行、同副本共享修改点轮流负责，主Agent协调交接与整合。
Q33确认提交后按工作包及实际证据核验，再完成工作项，不等于用户最终验收。
Q34确认保全成果、核实原执行/副作用后在原授权和预算内局部恢复，
安全交接前不启动重叠替代写入，不因沉默判停、不盲重放外部操作。
其余动作范围/执行机制仍待决定，非实际提权或执行许可。
其他候选仍保留、未逐项批准，不把整组来源功能算作已同意。
轻量覆盖/决策台账：
`.trellis/tasks/10-14-native-redesign-shell/research/harness-decision-register.md`。
后续阶段MD须回指候选ID与已确认决定；本库和台账都不是实施授权。
离线样例独立执行包：`docs/native/task-evolution-review-stage-01.md`；
用户明确交给新终端执行后按其门槛实施，不接真实任务或进化引擎。
协作Q27–Q35阶段快照：`docs/native/task-collaboration-design-checkpoint-01.md` 版本0.2；
不是协作引擎执行包。Q35已确认离线协作样例范围，独立执行包为
`docs/native/task-collaboration-review-stage-01.md`；用户交新终端明确执行后
按门槛实施，当前线程不改产品，布局/运行验收仍未通过。
Q36已确认每项任务维护可追踪主计划，工作包、协作进度和阶段反思共同
引用并保留修订关联；不因此批准计划变更/审批、存储或知识沉淀机制。
Q37另确认主Agent可在既定目标/范围、验收、授权与资源边界内自行调整
步骤/路径并留痕，改变边界先交用户决定；不绕过既有核验与监督。
Q38确认修订后保留成果、只对齐受影响在途工作，无关安全工作继续；
不全队重启、不自动回滚或重放，用户Stop与宿主硬限制优先。
Q39确认主Agent按任务复杂度先起草、关键歧义先问；需求明确且已获必要
授权时不另加计划开工签字，既有审批与监督门槛保留。
Q40确认保留原始依据、自动整理可回源的任务内摘要，区分确认/待决、
自报/核验并保留变更关联，不自动扩为跨任务记忆。
Q36–Q40快照：`docs/native/task-planning-design-checkpoint-01.md` 版本0.1，
只记录已确认设计，不是引擎或新增样例执行包。
Q41另确认自动提知识候选、用户集中审阅后项目内复用，附来源/范围/限制；
不默认扩到其他项目/全局，知识不替代权限或当前验证，不增补上述快照。
Q42另确认项目级自动入库可配置、默认关闭，与Full Access独立；
开启后预设类型/来源/范围内免逐条确认，冲突/敏感/依据不足仍留候选，
不默认跨项目/全局或直接生成可执行技能。Q43另确认在获准项目范围
按任务/工作包按需使用精简知识及来源/限制，不整库注入或升级指令，
当前要求优先于旧经验，不改权限/验收或宿主硬限制，疑点明确标注。
Q44另确认修订留痕、停用后不再推荐及已引用在途任务局部核对，
不承诺瞬时清除已消费上下文，通知/ACK不等于完成更正。
Q41–Q44独立快照：`docs/native/project-knowledge-design-checkpoint-01.md` 版本0.1，
不是实施包。Q45已确认独立SwiftUI离线知识审阅样例范围，
执行包 `docs/native/project-knowledge-review-stage-01.md` 版本1.0：
内存fixture，不接真实Agent/API、不采集历史或写真实知识/任务/权限/配置。
用户明确交新终端执行后按独立任务及文件/构建/UI隔离门槛实施；
本线程不改产品，范围认可不是布局/运行验收或知识引擎批准。
Q46已确认技能/流程先成带来源、前提和验证要求的草稿，与知识分开，
默认用户确认内容/范围后复用；生成不等于安装/启用/执行，加载不扩权限，
Full Access不自动发布或扩范围。Q47已确认获准范围内按需选用已启用且当前
可用技能；新技能须先人工同意，才按批准范围安装使用，Full Access不替代
该同意。Q48已确认统一管理、默认当前项目启用，跨项目/全局须明确扩大范围，
不擅改CLI个人配置，不改Q30任务权限作用域。Q49已确认更新先审阅、用户同意
后启用，在途不静默换版，旧版仍须获准/安全可用且宿主允许，Stop/撤销/硬拦截优先。
Q46–Q49快照 `docs/native/skill-reuse-design-checkpoint-01.md` 版本0.1非执行包；
Q50独立离线技能样例范围已确认，执行包
`docs/native/skill-reuse-review-stage-01.md` v1.0；用户交新终端后按独立任务/
审阅/start及资源隔离门槛实施，本线程不实现/安装，不扩Q26/Q35/Q45，
不改U01工具试用；范围确认不是布局/运行或引擎验收。技能完整规则：
`.trellis/tasks/10-14-native-redesign-shell/research/harness-skill-reuse.md`。
后续规划按需见
`.trellis/tasks/10-14-native-redesign-shell/research/harness-task-planning.md`。

Q51已确认参考C04/C05的三类对象关系：待办、执行任务与自动化统一关联而
不混为一种任务；创建待办不即执行，多次关联执行保留记录，规则与各次运行
分开，仍可直接建Agent任务，执行结束不等于目标验收。仅关系设计，机制未定。
按需见 `.trellis/tasks/10-14-native-redesign-shell/research/harness-work-arrangements.md`；
Q52另确认同获准范围内人工/Agent共用待办与自动化对象，保留发起者、
来源任务及后续修改；创建或修改不天然获运行许可，Full Access覆盖内不重复
询问。Q53另确认默认记录、明确交办执行、可选有范围自动接单，与Full Access
分开，不把当前任务权限推广未来任务；具体授权/预算/并发/恢复另议。
Q54另确认用户明确设置每条规则的范围/执行配置/操作与资源边界，覆盖内
不重复询问、范围外不自行扩大；可另授规则Full Access，不自动继承任务
或默认全局开放。Q55另确认暂停接单与停止本规则工作分开，保留成果/历史，
未核实不宣称已停，明确暂停/停止后不自行恢复，具体协议另议。
Q56另确认Agent待办按整个目标及必需检查/适用监督判定技术完成，不以运行
结束代替、不强制用户逐条点击；用户最终验收另记，明确亲自验收不能代签。
Q57另确认明确筛选及启用前预览当前匹配/排除与未来纳入条件，可单项排除；
有效授权内合条件新增不逐项重问，普通记录不默认执行，既有控制边界不变。
Q58另确认首次启用默认不回填积压，合条件旧待办可明确批量纳入、覆盖内
不逐项重问；不盲重放在途/未知事项，不自动重开完成/待验收项。
Q59另确认规则变更先展示差异/影响并由用户确认，默认面向后续启动，
未启动重核，在途不静默换版；明确调整按Q38局部对齐，Stop/撤权/禁令/
宿主硬限制优先，旧版不保留已撤回权限。Q60另确认定时规则复用逐规则
授权/变更/控制原则，保存/到点不授权，明确启用且覆盖内免逐次确认；
Full Access另授，新技能人工同意不变。不合并触发器、不要求每次先建待办，
不将待办积压套成定时补跑许可。Q61另确认漏跑默认不追赶历史，可逐规则明确
开启恢复后补一次当前执行，覆盖内免逐次重问，不冒充历史完成；
在途/未知先核实，暂停/停止不自行启用，逐期历史处理另定。Q62另确认同规则
默认不重叠、繁忙到点默认跳过，可独立明确开启结束后合并补一次，不继承Q61；
确认安全收束、当前条件满足后再启动，手动/恢复不绕过，未知先核实，
暂停/停止不自行恢复。Q63另确认跨规则/待办执行/手动任务实际冲突局部协调，
安全独立工作继续；不抢占/强杀/删锁，未知先核实，按当前意图/授权/依赖
重核，Full Access不绕过资源约束，不将Q62跳过变排队补跑。
Q64另确认等待后先重核，覆盖内条件满足自动继续，变化局部对齐；
不盲套补丁/全项目重做，改目标/权限/验收先问，已解决/取消/未知先核实，
不恢复暂停/停止或新增重试/补跑权。Q65另确认可配置共享资源护栏，
局部额度不绕过项目/总额，委派/监督/重试/补跑纳入消耗，到限阻止受限新准入，
不自行扩额或降低监督门槛；计量区分预估/报告/未知，不假报零/精确账单，
必需硬限制不能执行则如实受阻，Full Access不免资源边界。
Q66另确认并发/尝试机会用尽只挡新准入，不因此打断最后一次获准执行；
对在途生效的执行边界到限则挡受限扩张、按能力请求安全收束，
保全成果及未知事实，缺审不算通过，Stop/撤权/宿主硬限制优先。
只影响命中范围及必要依赖，不强杀/删锁或虚报已停。
Q67另确认补额/适用周期恢复只改变资源条件，到限收束后默认不重启，
可明确开启范围内自动续行或补额并继续；普通获准等待仍按Q64自动继续。
原执行/副作用及当前条件先重核，不恢复用户Stop/暂停、重放未知操作、
清零独立上限或复活跳过触发；未暂停规则的合法新触发按原准入重核。
Q68已确认阶段文档与独立离线原生样例范围，交接见
`work-arrangements-review-stage-01.md`；本线程不实施/开工，不批准真实引擎。
当前项目不创建VM，现行App/UI暂停不解除，不准备替代VM或继承旧VM许可。

## 1. 研究方法与可信度

来源根：`/Users/macbook/Downloads/同步空间/LYX/harness`。

先清点10个顶层项目，再读README、文档导航与关键入口，按能力追到实现、
状态所有者和必要测试。只读三组独立研究处理9个项目；主线程负责codeg、
Aeroric对照与浏览器。依赖、构建、生成物和vendor目录不作为阅读主体。
过滤后的目录含79,164个文件；**这不是逐个读过79,164个文件的声明**。
完整清点、排除规则与源码版本存在
`.artifacts/native/mockups/browser-evidence-home-v3/harness-inventory.json`。

| 项目 | 本地HEAD简写 | 过滤后文件数 | 重点 |
|---|---|---:|---|
| MiMo-Code | `6ba6a6254a6d` | 1,997 | 自进化、三角色、隔离与验收 |
| paperclip-多agents协作 | `14f961b6989d` | 10,298 | 多Agent治理、任务领取、预算与审计 |
| 1flowbase | `7abe6154891f` | 5,769 | 任务规划、对话沉淀与知识复用 |
| codeg | `4b5f942a9d06` | 2,362 | 欢迎态/新任务、工作面、协作工具 |
| deepseek-harness | `571902028a75` | 16,004 | 长任务、上下文、工作流与恢复 |
| opencode | `b474b842595c` | 8,538 | 会话与服务分层、计划、检查点 |
| LiveAgent | `85029b5611be` | 2,314 | Agent服务、事件、环境与执行 |
| minimax-code | `e798b460f4d8` | 3,455 | Agent模块、上下文与执行保护 |
| ZCode | `aac4755666d0` | 16,848 | 工作空间、控制、协作与可追溯性 |
| orca | `aa6f945001a5` | 11,579 | 并行开发、隔离、上下文与移动协同 |

本地快照不是“网上最新版本”。来源仓库没有被修改、安装、运行或测试。
本轮证据等级：

- **实现**：读到实际函数/调用/存储；不等于端到端运行通过。
- **测试源码**：读到相关断言；不等于本轮跑过测试。
- **文档/设计**：有说明但没有追到执行入口；必须单独标记。
- **建议**：为Aeroric推导的产品方案，不是来源项目或Aeroric已具备的事实。

来源路径在各项目表内均相对对应项目根。正文只摘要，不移植代码或网页UI。
许可证文件仅作来源记录；是否复用具体代码/依赖/资产必须另行逐项审查，
不能由“参考功能”推出可以复制整个项目。

## 2. Aeroric已有基础与缺口

以下基于当前脏工作树，可能包含其他执行者尚未提交的工作；不据此宣称发布。

| 基础 | 已读证据 | 可以承接什么 | 不能由此宣称什么 |
|---|---|---|---|
| 任务状态、先持久化再启动、幂等操作台账 | `core/crates/aeroric-core/src/tasks/mod.rs:1–23`；`tasks/operations.rs:128–156,281–317` | 原生工作项到执行的可靠生命周期 | 已有多Agent调度、计划DAG或自进化引擎 |
| Git隔离/合并服务 | `core/crates/aeroric-core/src/git/worktree.rs:45–66,77–116` | 将隔离、差异、合并接入任务 | worktree是安全沙箱、默认允许创建分支 |
| 会话读取与导出 | `core/crates/aeroric-core/src/sessions/mod.rs:1–20` | 历史查看、引用、交接与导出 | 每个Agent都支持fork/rewind/恢复 |
| 交接文本清理 | `core/crates/aeroric-core/src/tasks/handoff.rs:1–17,187–231` | 安全整理终端文本用于人工交接 | 可以从ANSI推导工具、审批或真实任务树 |
| 笔记检索与带预算上下文 | `core/crates/aeroric-core/src/notebook/rag/context.rs:14–29,66–93` | 带来源引用的知识调用、截断提示 | 自动跨项目/会话读取全部资料或静默记忆 |
| 外部Agent工作图 | `core/crates/aeroric-core/src/adapters/work_graph.rs:1–21,150–180` | 外部任务树/目标/子Agent事实的隔离展示 | Aeroric已拥有这些外部任务的执行权 |
| 外部工作流journal与维护候选 | `core/crates/aeroric-core/src/adapters/workflow.rs:1–28` | 校验输入/版本、有限重试、staging review | 宿主可以eval任意脚本，或本机原生工作流已交付 |
| Agent工具能力探测 | `core/crates/aeroric-core/src/adapters/mod.rs:1–23`；`macos/AeroricApp/Sources/TaskWorkspaceView.swift:366–372` | 显式能力发现、拒绝原因、外部控制入口 | 外部Agent能力=所有内置Agent都有；原生端代跑外部工具 |
| 原生资源/扩展/统计工作面 | `macos/AeroricApp/Sources/TaskWorkspaceView.swift:309–362` | 接现有设施、数据库、笔记、扩展、运维入口 | 视图装配=全部动作/数据已接线 |

最重要的区别是：**原生展示/外部适配、Aeroric自己拥有的业务对象、
实际执行与运行验收是四个层次。** 新增能力应先确认所有权，不能在UI上
改个名字就声称成为Aeroric原生能力。

## 3. 产品归并建议

推荐不按10个参考软件各建一个模块，而沿着一条工作链组织：

**目标 → 计划 → 工作项 → 执行/委派 → 证据 → 验收 → 知识 → 改进提案**

本轮共整理**89条来源观察＋1条用户三角色方案**，不是90个独立新模块。
同类的队列、恢复、规划、记忆和沙箱机制应归并，不分别造十套实现。

候选归属：

1. **任务工作区**：目标、计划、步骤、会话、交接和产物；新建任务选方法。
2. **协作中心**：角色、团队、委派、队列、依赖、审批与资源预算。
3. **知识中心**：对话沉淀、决策、笔记、引用、记忆候选与技能。
4. **演进实验室**：自进化实验、主执行/反思/治理三角色、补丁与版本。
5. **项目检查器**：Git/文件、Issue/PR、隔离环境、测试证据与合并。
6. **工具库/设置**：资源、连接、技能、MCP、能力授权、统计与诊断。

名称和模块拆分都是建议，尚未批准。首页展示项目、待回应、工作安排、
少量常用工具及摘要，不承载所有候选按钮。

### “原生功能”应有真正业务所有者

不是给参考软件包一个SwiftUI壳，而是让Aeroric拥有必要的原生对象和控制面，
再按能力调用执行者。建议如下，均待架构审阅：

| 能力域 | Aeroric原生应拥有 | 可以复用的现有基础 | 仍需新设计/实现 | 第一阶段候选 |
|---|---|---|---|---|
| 规划与工作项 | 目标、批准计划版本、工作包、依赖、验收与变更记录 | tasks/service、operation ledger | 计划/工作项实体、依赖调度、批准范围 | 先计划审阅/工作包，不直接自动拆解无限执行 |
| 团队协作 | 角色、领取租约、执行身份、委派关系、交接包、预算 | task生命周期、外部subagent登记、host executor | 原生协作控制面及单写者、持久收件箱 | 小团队、一个协调者、明确负责人 |
| 证据与成果 | 运行、命令/测试结果、文件版本/hash、审批、验收结论 | sessions/export、git、terminal事实 | 统一Evidence/Deliverable关联，独立验收入口 | 优先把“Agent说完成”与“可核验完成”分开 |
| 知识与记忆 | 决策、来源、项目范围、有效性、候选/确认/撤销版本 | notebook/RAG、带引用上下文 | 从会话提炼候选、审核与关联、不串项目 | 用户审阅后沉淀，不静默吞所有历史 |
| 原生工作流 | typed步骤、输入/输出、依赖、暂停、取消、副作用journal | task执行端口与现有外部workflow journal规则 | 原生流程owner与typed执行控制，不eval任意外部脚本 | 先固定工作配方、可观察状态，不开放无限代码编排 |
| 自进化 | 被演进对象/版本、实验、角色、隔离、评测、补丁、激活/回退 | git隔离、任务执行、证据/候选维护规则 | 三角色控制面、可演进代码对象、评测owner与版本管理 | 先定对象边界，再验证一个完整实验闭环 |
| 权限与执行环境 | 宿主身份、scope、目标共享、实际能力、授权与预算 | capability probe、既有审批/managed home | 新能力分别建立host拒绝与实际强制机制 | 未知即阻塞/解释，不把“有开关”当安全 |

对不具备结构化协议的CLI，Aeroric仍可管理自己的工作项、交接和外部测试
证据，但不能把终端ANSI解释成该CLI已完成计划、读过反馈或接受委派。
各Agent的会话/TUI保持各自真实语义，尤其不复活被移除的OMP专用rpc-ui。
原生业务实体与外部同名goal/workflow必须具有不同owner/ID/来源，不能
把外部只读事实抢成Aeroric自主执行状态。

### 原生实现边界

产品前端仍为SwiftUI/AppKit真实Material,现行最低系统macOS26(见本文顶部“现行基线澄清(2026-10-16)”);Liquid Glass增强
按项目既定macOS26 availability门控，主要用于控制/导航功能层。浏览器
示例不是WebView产品方案。原生互斥选择使用`NativeSelection`承接系统
Picker/List/Menu等状态；语义颜色、辅助功能和独立窗口状态约束不变。

业务状态归Rust core，执行归host，协议/能力契约归protocol；app注入
read model与动作，UI不拼启动命令、不解析原始会话、不从ANSI猜事实。
这些是未来落点建议，不授权创建这些新模块或替换既有实现。

### 横向约束

- 本仓库仍只允许`main`；本轮不创建分支/工作树/克隆。未来演进实验
  需要独立工作副本时，必须先明确用户授权、目标路径、数据和分支策略。
- Git隔离不是权限隔离。另需文件、网络、系统能力、凭证范围和预算。
- 无结构化证据的Agent不伪装成支持计划、委派、fork或实时反馈。
- 会话历史遵守managed home边界，尤其不得扩大OMP读取根。
- 敏感与外发动作按有效任务授权分档:已在其覆盖内(含手动开启的Full
  Access)不重复批准;超出覆盖或含糊的敏感/外发动作先确认再执行;实际
  平台/系统/宿主限制、作用范围、预算与用户Stop始终有效。服务运行、能力
  可用、目标共享、实际授权分开。本条不新增任何实际敏感数据、凭据或外发
  权限,也不建立“一切敏感/外发一律逐项批准”的绝对清单。
- 证据与评审不能由被评审者悄悄改写；角色分工不等于判断必然正确。
- 取消、重启、重试先核验副作用事实；不会用“重跑全部”掩盖未知状态。
- 无人值守与自进化都必须有硬停止条件、预算、人工入口和审计。

## 4. 按来源列举的候选

### 4.1 codeg — 工作入口与工作空间

| ID | 借鉴亮点与价值 | 来源证据（实现，未运行） | Aeroric原生完善建议 | 前置/风险 |
|---|---|---|---|---|
| C01 | 新任务欢迎态按工作目的挑选方法 | `src/components/chat/quick-actions.tsx:508–590`；`src/components/conversations/conversation-detail-panel.tsx:2394–2427` | 新建任务内分构思/文档/科研；方法与项目/目标同一流程 | 方法不是新任务实体；选择不提交 |
| C02 | 技能与提示注入、按Agent检测可用性 | `src/components/chat/quick-actions.tsx:382–473`；`src/components/conversations/conversation-detail-panel.tsx:1785–1787` | 方法显示依赖、启用范围与阻塞原因，确认后才交执行适配器 | 不复制“加载中先乐观可用”策略；未知先明确 |
| C03 | 文档、表格、演示与科研工作配方 | `src/lib/office-actions.ts:37–54`；`src/lib/research-actions.ts:43–60` | 为9种方法定义输入、产物、格式与验证；复用技能目录 | 不伪称已交付原生Office编辑器或科学有效结论 |
| C04 | 待办到执行、输入、审阅、合并的闭环 | `src-tauri/src/work_task/engine.rs:1–17` | 工作项看板与Agent任务分开，运行记录带generation与验收证据 | late event/CAS/重复启动防护；复用已有task台账 |
| C05 | 自动化计划与运行实例分离、防重叠 | `src-tauri/src/automation/engine.rs:233–273,306–345` | 自动化原生页管理计划、手动运行、实例与错过/跳过原因 | 时区/离线/重启策略需另定；不能无限补跑 |
| C06 | 每次运行隔离与可恢复合并 | `src-tauri/src/automation/engine.rs:600–640`；`src-tauri/src/work_task/engine.rs:13–17` | 隔离环境、差异、冲突、合并意图和恢复记录归项目检查器 | worktree不等于沙箱；分支/合并都需先授权 |
| C07 | 服务健康、会话、委派、工具组分层 | `src/components/layout/status-bar-mcp.tsx:133–172,317–363`；`src-tauri/src/commands/mcp_service.rs:158–224` | 协作摘要与能力目录；会话数/运行数/深度策略分开 | Broker运行不等于每个目标已授权 |
| C08 | 执行中反馈与结构化提问 | `src/hooks/use-session-feedback.ts:85–96,487–494`；`src/components/chat/conversation-shell.tsx:252–263` | 任务中区分立即steer、下回合队列、待用户回答 | PTY/结构化协议能力不同，不能假装全部即时 |
| C09 | 画布把会话、笔记、文件、终端联系起来 | `src/components/canvas/canvas-model.ts:722–753,891`；`src/components/canvas/canvas-view.tsx:671–705` | SwiftUI/AppKit原生可视工作图，卡片引用真实对象而非复制状态 | 视口虚拟化、坐标持久化、PTY单owner；不是照搬ReactFlow |
| C10 | 仓库Issue/PR工作面区别于本地Git | `src/components/forge/forge-page.tsx:190–238,543,1445–1453` | 仓库中心分本地变更/远端Issue/PR，工作项可关联但不混淆状态 | 凭证、远端分页/平台差异、发帖/合并授权 |

### 4.2 paperclip — 多Agent协作与治理

重点借机制而非照搬“AI公司”。以下均读到实现，P04/P06另读到测试源码，
没有运行；完整适配器、Telemetry/外发路径和每条权限分支未遍历。

| ID | 借鉴亮点与价值 | 来源证据 | Aeroric原生完善建议 | 前置/风险 |
|---|---|---|---|---|
| P01 | 角色职责与汇报链，明确负责人和升级对象 | `packages/db/src/schema/agents.ts:23–46`；`server/src/services/agents.ts:206–241` | 角色册、团队模板、负责人/审阅人，角色与Agent品牌解耦 | 角色描述不等于强制权限或能力保证 |
| P02 | 委派携带计划，避免交接失去上下文 | `server/src/services/native-runtime/paperclip-runner-tool-authority.ts:1292–1327`；`server/src/services/issues.ts:10432–10448` | 交接包引用目标、计划版本、依赖、证据及禁止事项 | 接受交接不授权扩范围；Agent Chat有实验性边界 |
| P03 | 原子领取与run锁，避免多人抢同一工作项 | `server/src/services/issues.ts:11714–11750,11879–11901` | core管理领取租约/运行归属/幂等重试；UI显示谁在负责 | 不等于外部副作用exactly-once，不能只靠任务树去重 |
| P04 | 审批、退修、重交，决定可追踪 | `server/src/services/approvals.ts:82–122,175–199`；`server/src/__tests__/approvals-service.test.ts:82–108` | 审批收件箱、范围预览、返修意见和版本绑定 | 只覆盖受治理动作，不代表全部工具默认审批 |
| P05 | 项目/Agent/团队多作用域预算准入 | `server/src/services/budgets.ts:562–598`；`server/src/services/heartbeat/queue.ts:2533–2548` | 真实用量账本、预算解释、启动拒绝、在途取消状态 | 未计价也要标明；拒绝新运行不等于瞬时停止在途工作 |
| P06 | 重启先保护已完成结果，再判断恢复 | `server/src/services/heartbeat/recovery.ts:909–916`；`server/src/services/native-runtime/native-runner-restart-recovery.integration.test.ts:998–1011` | 恢复检查器，分别核验进程、结果、存储与清理 | 终态、provider停止、文件保存不是同一事实 |
| P07 | 归责审计关联actor、Agent、run和用户 | `server/src/services/activity-log.ts:157–183,215–225` | 运行证据时间线、过滤与脱敏导出 | 来源并非防篡改账本，也未证明每条写路径完整覆盖 |
| P08 | 身份与租户授权隔离 | `server/src/routes/authz.ts:75–118` | 宿主、工作区、操作者和执行身份的明确授权映射 | 公司作用域不能机械等同项目；trusted-local有例外 |
| P09 | 要求隔离时不静默降级执行环境 | `server/src/services/heartbeat/environment-selection.ts:223–234,286–307` | 新任务显示隔离要求、可用环境与不能执行的原因 | worktree不是OS沙箱；具体环境provider未运行验证 |
| P10 | 例行工作产出可追踪工作项，处理并发策略 | `server/src/services/routines.ts:1883–1929` | 自动化引用同一工作项/执行对象，跳过与合并有记录 | 错过触发、重试、时区与并发需逐项设计 |

### 4.3 LiveAgent — 执行监督与可审查记录

以下均读到实现；L03/L05另有测试源码。服务、远控和实际沙箱未运行。

| ID | 借鉴亮点与价值 | 来源证据 | Aeroric原生完善建议 | 前置/风险 |
|---|---|---|---|---|
| L01 | 长进程session/cursor监督与退出事实 | `docs/features/tools.md:30–43`；`crates/agent-gui/src-tauri/src/runtime/managed_process.rs:358–398,657–677` | 后台进程检查器、增量日志、停止范围及退出证据 | 验证PID身份；“请求停止”不能涂成“已停止” |
| L02 | 子Agent默认只读、显式候选补丁回灌 | `crates/agent-gui/src/lib/subagents/validate.ts:299–317`；`crates/agent-gui/src/lib/subagents/policy.ts:175–197` | 子Agent权限摘要、改动路径预览、审阅后应用 | 工具只读不是OS隔离，自动应用必须显式策略 |
| L03 | 断线事件按seq/epoch恢复，缺口重同步 | `crates/agent-gateway/internal/session/conversation_stream.go:401–451`；`crates/agent-gateway/internal/session/conversation_stream_test.go:209–244` | 远控会话显示重同步/快照/缺失区间 | 有界窗口不是永久事件库，不能静默丢事件 |
| L04 | 多宿主执行凭证与浏览器角色分离 | `crates/agent-gateway/internal/auth/agenttoken/store.go:213–225`；`crates/agent-gateway/internal/protocol/pbws/handshake.go:35–55` | 宿主配对、执行身份、撤销/过期与审计 | 来源浏览器共享token不等于细粒度用户RBAC |
| L05 | 上下文压缩有提交点，避免恢复旧segment | `crates/agent-gui/src/lib/chat/compaction/controller.ts:440–452`；`crates/agent-gui/test/chat/compaction-controller.test.mjs:475–506` | 压缩预览、原文引用、版本与取消/恢复状态 | 摘要可能丢信息，不做原始历史或任务状态真源 |
| L06 | 记忆有引用、置信标记和接受动作 | `crates/agent-gui/src-tauri/src/services/memory/mutations/evidence.rs:20–34`；`crates/agent-gui/src-tauri/src/services/memory/mutations/methods.rs:392–409` | 记忆候选审核区、来源、项目范围与失效说明 | 有引用不保证正确；跨项目召回需授权 |
| L07 | Bash/HTTP/Prompt自动化分型并拒绝重叠 | `crates/agent-gui/src-tauri/src/services/automation/scheduler.rs:317–341,402–423,508–514` | 自动化动作类型、能力/风险摘要、运行实例 | 进程互斥非分布式exactly-once；休眠补偿未核实 |
| L08 | 运行轨迹统计标注完整性与近似 | `crates/agent-ui/src/lib/trajectory/eventLog.ts:104–123`；`crates/agent-ui/src/lib/trajectory/useConversationStats.ts:255–268` | 用量/耗时/工具轨迹显示来源、范围、截断 | 缺事件不能造精确耗时，近似统计非审计账本 |

### 4.4 minimax-code — 交互控制与执行保护

以下均读到实现；X07另读到测试源码。根MIT声明不覆盖全部第三方，
例外见`LICENSE-STATUS.md:3–11`。公开投影明确排除旧team循环引擎、
Desktop HTTP入口和cloud-only实现：
`docs/architecture.md:14`、`docs/open-source-status.md:21`。

| ID | 借鉴亮点与价值 | 来源证据 | Aeroric原生完善建议 | 前置/风险 |
|---|---|---|---|---|
| X01 | 无工具轻会话，普通问答不背整个执行上下文 | `packages/local-runtime-v2/src/service/turn-system/agent-host/execution/prompt.ts:25–42`；`README.md:159–169` | 新任务可选轻问答用途，明确“不能操作项目” | 来源仅root/普通turn适用；不能暗中升级成执行任务 |
| X02 | 工具执行前权限裁决与授权来源 | `packages/local-runtime-v2/src/service/turn-system/agent-host/runner/policy/local-turn-permission-gate.ts:292–306,471–509` | 审批展示实际目标、权限来源、期限及撤销 | Hook自动允许/始终允许不能绕过Aeroric边界 |
| X03 | 每次调用核验沙箱后端能否落实策略 | `packages/local-runtime-v2/src/service/sandbox/local-sandbox-service.ts:352–393`；`packages/local-runtime-v2/src/service/sandbox/backend/selector.ts:46–82` | 执行环境探测与拒绝原因，不假装“开启=已隔离” | 来源明确禁用时走native；跨平台实现未运行 |
| X04 | 排队、即时steer、持久交付分状态 | `packages/local-runtime-v2/src/application/conversation/queue-steer-workflow.ts:50–61,95–140` | 用户补充说明标注排队/已交付/实际消费 | 权限快照、claim释放、幂等域与迟到事件 |
| X05 | 用户Stop级联所属后台工作，不与切会话混淆 | `packages/local-runtime/src/background-task/user-stop-cascade.ts:214–243`；`docs/tui-capabilities.md:132–157` | 停止范围确认、后台列表、逐项退出证据 | 来源best-effort且只本进程owner，不保证跨进程全停 |
| X06 | 非交互执行cursor去重与终态协调 | `packages/tui/src/headless/supervisor.ts:85–132`；`packages/tui/src/headless/runner.ts:146–148` | 批处理运行记录、断点与重复消息过滤 | 最终事件不等于turn所有权释放；非远程daemon证明 |
| X07 | Hook通知与回答/导出隔离 | `packages/local-runtime-v2/src/service/turn-system/agent-host/assembly/local-turn-plugin-hooks.ts:83–95`；`packages/tui/test/unit/tui-hook-system-message.test.ts:159–174` | 插件通知区和结构化运行事件分层 | 展示隔离不是Hook进程安全，additionalContext另有语义 |
| X08 | 会话作用域发现技能、过滤冲突与不可用项 | `packages/tui/src/acp/commands.ts:45–70`；`packages/tui/src/acp/agent.ts:124–141` | 方法/技能目录按Agent与workspace显示可用性 | 技能入口非独立工作面，不证明完整IDE互通 |

### 4.5 1flowbase — 规划边界与对话资产

目录缩写：FC=`api/crates/control-plane/src`；
FR=`api/crates/orchestration-runtime/src`；
FP=`api/crates/storage/durable/postgres/src/orchestration_runtime_repository`；
FU=`web/app/src/features/agent-flow/components/debug-console/trajectory`；
FM=`api/apps/api-server/src/routes/mcp_protocol`。

**F01/F09是开发规范启发，不是用户自动规划或长期记忆产品的实现。**
其余读到实现，全部未运行。

| ID | 借鉴亮点与价值 | 来源证据 | Aeroric原生完善建议 | 前置/风险 |
|---|---|---|---|---|
| F01 | 两层计划、一次批准、有界工作包，降低交接反复 | `AGENTS.md:32–37`；`.agents/skills/problem-framing/references/long-running-work.md:72–92`（规范） | 批准目标/交付计划，工作包列依赖、可写/禁写路径、验收与停止条件 | 不是已实现的自动规划器；范围变化重新审阅 |
| F02 | 工作流依赖可编译、端点检查、拓扑排序拒循环 | `FR/compiler/topology.rs:240–369` | 计划依赖图、阻塞原因和节点诊断 | 工作流节点不等于任务树，也不证明自动拆解 |
| F03 | 已读调试/执行链的人工输入暂停与恢复checkpoint | `FR/execution_engine.rs:1301–1330`；`FC/orchestration_runtime/persistence.rs:288–335` | 待回答/待决定卡、历史答案与实际恢复位置 | 未证明所有运行模式；human_input非完整计划审批，不能撤销旧副作用 |
| F04 | 有界多模型专家面板，意见并列比较 | `FR/execution_engine/visible_internal_llm_tools/panel_execution.rs:5–76` | 评审时按角色展示不同模型意见、依据与成本 | 来源并发上限4；多数同意不保证正确 |
| F05 | 会话按workspace/application/actor续读，未配对工具调用不重执行 | `FP/application_run_logs/assistant_conversation_methods.rs:2–56`；`FC/application_public_api/run_service/conversation_history.rs:29–95` | 来源化会话档案、继续/引用/交接分开 | 补错误不证明原操作没有部分发生，历史非记忆 |
| F06 | 外部日志验证来源、幂等接收、内容冲突可见 | `FC/agent_logs.rs:18–61`；`FP/agent_logs.rs:20–55` | 日志收件箱、来源/接收/重复/冲突状态 | 写日志授权不等于执行授权；采集插件未覆盖 |
| F07 | 对话—请求—模型调用可回溯到原轨迹 | `FU/ProviderTrajectory.tsx:53–73`；`FU/TrajectoryStepDetail.tsx:80–98` | 每个结论/产物有“查看依据”与原始记录返回路径 | 来源可追溯不等于推理正确，缺元数据保留未知 |
| F08 | 作用域轨迹关键词搜索与游标分页 | `FP/log_query/conversation_trajectory.rs:27–130` | 历史搜索命中直接跳步骤，按页而非整段装上下文 | 非向量/语义RAG，规模性能未运行 |
| F09 | 记忆分类型、限展开预算、过期回源验证 | `.memory/AGENTS.md:3–28`（规范，仅规则） | 保存偏好/纠正/决策/工具解法，引用来源、范围、有效性 | 未读私有记忆正文，不声称产品自动记忆已实现 |
| F10 | MCP list/get/call/result渐进发现，完成后凭receipt续读 | `FM/virtual_ui.rs:817–842`；`FM/result_delivery.rs:220–322` | 大结果按引用展开，已执行与已读结果分别记录 | 缓存失效不等于操作未发生，不能直接重试写操作 |

### 4.6 ZCode — 协作连续性与控制契约

目录缩写：ZC=`apps/zcode-cli/packages/core/src`；
ZD=`apps/zcode-cli/packages/dynamic-workflow/src`；
ZB=`apps/zcode-cli/packages/bootstrap/src`。

以下读到实现，未运行。`NOTICE.md:9–21`明确没有默认OS沙箱；
worktree、工作目录、页面隔离和REPL不能充当系统级隔离。

| ID | 借鉴亮点与价值 | 来源证据 | Aeroric原生完善建议 | 前置/风险 |
|---|---|---|---|---|
| Z01 | 协作脚本先分析、诊断、确认，再经执行port提交 | `ZC/tool/handlers/create-workflow.ts:100–171,367–440` | 工作流预览、编译错误、能力/副作用摘要 | 不让宿主eval外部脚本；来源可本会话免后续确认 |
| Z02 | journal恢复完成节点、产物版本及输入hash | `ZD/engine/engine-resume.ts:17–43`；`ZD/engine/scheduler.ts:127–165` | 恢复检查器核验脚本/输入/适配器版本，显示已完成事实 | running节点可重派，不是全部副作用exactly-once |
| Z03 | 批准计划在上下文压缩后仍有引用 | `ZC/tool/handlers/plan-mode.ts:60–103,153–178`；`ZC/runtime/methods/compact-active.ts:486–508` | 批准计划固定版本引用、变更提示、恢复后对齐目标 | 来源文件仍可能变更/删除；非永久记忆 |
| Z04 | 后台委派结果标代际，拒旧分支通知 | `ZC/runtime/methods/background-notifications.ts:19–71` | 成果收件箱显示对应运行/分支及过时标记 | 来源内存队列不保证崩溃后必达 |
| Z05 | 多客户端会话FIFO与command ID受理去重 | `ZB/zcode-protocol-v4/command-inbox.ts:81–185` | 命令显示草稿/已受理/执行中/完成，不串窗口 | ACK不等于完成，受理去重不等于业务幂等 |
| Z06 | 手机/桌面附着同一执行Host | `packages/desktop/src/host/index.ts:2083–2128,2844–2854` | 显示执行位置、连接身份、所附会话；刷新不另spawn | 展示设备非执行设备，配对/传输未运行验收 |
| Z07 | 文件撤销先预检，多文件失败尝试补偿 | `ZC/runtime/methods/file-rewind.ts:94–191`；`docs/file-summary-safe-rewind.md:12–31` | 撤销预览、当前版本冲突、风险文件与补偿结果 | 不覆盖Shell/网络/数据库回滚，不是全局原子事务 |
| Z08 | 工作区Hook信任绑定身份和声明摘要 | `ZC/hooks/workspace-hook-runtime-admission.ts:116–206` | 来源/声明变化/重新信任/故障时拒绝的统一说明 | 此门不统一覆盖全部插件Hook、MCP与项目脚本 |
| Z09 | MCP页面工具按scope/generation注册与撤销 | `ZC/mcp/app-tools.ts:47–103`；`ZB/zcode-protocol/mcp-ui/appTools.ts:97–143` | typed原生工具表单，实例失效可见，不移植HTML产品页面 | readOnlyHint仅声明，页面隔离非OS沙箱 |

### 4.7 orca — Run协作、终端连续性与指认式反馈

目录缩写：OR=`src/main/runtime`；OU=`src/renderer/src/components`。
以下读到实现；O02另有未运行测试源码。
`ORCHESTRATION_IMPLEMENTATION_CHECKLIST.md:40–61`明确未包含新Run看板、
自动placement、基于沉默的自动重试、强制文件只读/单写者或Run-home自动
failover；worker报告不被标为Orca已验证正确。

| ID | 借鉴亮点与价值 | 来源证据 | Aeroric原生完善建议 | 前置/风险 |
|---|---|---|---|---|
| O01 | 同Run父子任务、依赖完成后晋升ready | `OR/orchestration/db.ts:3662–3709,3784–3819` | 协作任务树、阻塞依赖、负责人和验收状态 | completed状态非验收正确；完整环检测/并发未覆盖 |
| O02 | 协作邮箱delivery ACK与消费者代际隔离 | `OR/rpc/methods/orchestration.ts:732–805`；`OR/orchestration/orchestration-run-delivery-db.test.ts:133–191` | 持久成果收件箱、消费记录、旧消费者隔离 | ACK只证明消费，不证明成果正确 |
| O03 | 指定跨服务器worker，dispatch可追踪 | `OR/rpc/methods/orchestration-federated-worker-start.ts:28–124` | 执行位置/worker生命周期/失败事实，按显式能力分配 | 单Run home，非自动调度/failover；worktree非沙箱 |
| O04 | Host-owned终端快照恢复与provider fallback | `OR/orca-runtime.ts:10315–10377` | PTY重新附着与输出恢复；host权威序列接既有terminal机制 | 恢复输出非重启进程或验证结果，不搬xterm实现 |
| O05 | 点选页面元素成为结构化批注附件 | `src/main/browser/browser-grab-payload.ts:18–114`；`OU/browser-pane/BrowserPane.tsx:4373–4389` | 用户可指认问题，附元素/意图/批注/来源 | 文本/截图仍可能敏感，URL过滤非完整脱敏 |
| O06 | Diff/Markdown注释发给明确Agent并记录交付 | `OU/editor/DiffNotesSendMenu.tsx:54–101`；`OU/editor/ReviewNotesSendMenuContent.tsx:98–176` | 差异行评论、当前文件/全部评论、目标与落实反馈 | 行号可过期；已发送非已落实/已验收 |
| O07 | 手机RPC绑定设备身份和方法allowlist | `OR/runtime-rpc.ts:203–234,1385–1415` | 配对范围、设备授权、撤销和危险操作确认 | 来源allowlist含写/commit/discard，不能称只读遥看 |
| O08 | 桌面能力、观察/操作与TCC分层 | `OR/rpc/methods/computer.ts:31–83`；`src/main/computer/macos-computer-use-permissions.ts:23–79` | 系统权限、目标共享与逐动作授权分别说明 | TCC非逐动作同意；sidecar目标门未完整追踪 |

### 4.8 MiMo-Code — 记忆、工作流与演进候选

目录缩写：MC=`packages/cli`。以下M01–M09读到实现，部分还有提示契约/
测试源码；M10仅技能契约。全部未运行。MIT文件之外另有
`USE_RESTRICTIONS.md:5–19`用途声明，本轮不判断法律关系或复制代码。

**三角色证据缺口**：在当前HEAD的README/docs/packages/隐藏技能中暂未
定位id/ego/superego定义、启动或独立克隆链。`MC/src/id/id.ts:4–20,37–53`
是标识符工具；`README.zh.md:344–347,521`将Dream/Distill称作自我进化。
不把build/dream/distill硬映射成用户的三角色，也不宣称Harness自主升级、
模型训练或独立克隆已获证明。用户方案作为U01单列，仍值得深入设计。
以上限定于该开源快照；用户新增的桌面 `evolve` 证据见§5补充，
不可混归为MiMo-Code仓库中的实现。

| ID | 借鉴亮点与价值 | 来源证据 | Aeroric原生完善建议 | 前置/风险 |
|---|---|---|---|---|
| M01 | Dream沉淀知识、Distill提炼可复用流程资产 | `MC/src/command/index.ts:130–170`；`MC/src/agent/agent.ts:350–426` | 记忆候选与技能/子Agent/命令候选分开审阅 | 来源允许write/edit/bash/exec，指令纪律不是隔离 |
| M02 | 手动/自动复盘触发与运行位置明确 | `MC/src/session/auto-dream.ts:110–133`；`MC/src/session/prompt.ts:4628–4657,6130–6192` | 显示触发者、输入来源、运行位置和结果候选 | 手动在原session，自动另建session；都不等于克隆隔离 |
| M03 | 分域记忆搜索与预算化上下文重建 | `MC/src/memory/service.ts:52–95,115–137`；`MC/src/session/checkpoint.ts:262–333,1336–1358` | 上下文来源面板、注入范围/预算/截断，接既有RAG | 相关性非事实保证；消息尾部预算软上限；非Git回滚 |
| M04 | 持久任务树、Actor owner与可选继承上下文 | `MC/src/task/registry.ts:130–167,278–307`；`MC/src/actor/spawn.ts:210–244` | 子Agent进度、owner、父子关系、终态与取消 | cwd/上下文分叉非文件隔离；ID需带session/adapter |
| M05 | Goal另行裁判与有限再进入 | `MC/src/session/goal.ts:64–77,111–141`；`MC/src/session/prompt.ts:243,4095–4178` | 完成条件、裁判依据、尝试数和错误分开显示 | 来源Goal在内存；异常停止非验收通过；12次非费用预算 |
| M06 | 工作流并发/深度/生命周期预算与授权预检 | `MC/src/workflow/runtime.ts:29–37,600–620,716–736`；`MC/src/workflow/workspace.ts:5–24` | 协作/演进执行策略、硬限额、到限/拒绝原因 | 不照搬12h/16并发/1000Agent/深度8；词法路径检查有symlink风险 |
| M07 | 研究补缺、冲突记录与对抗式事实检查 | `MC/src/workflow/builtin/deep-research.js:124–176`；`MC/src/workflow/builtin/fact-check.js:291–332` | 研究结果附证据缺口、反证、弃权和引用审阅 | 模型投票不保证来源独立或引文准确 |
| M08 | 实验指标账本、keep/revert与失败转向 | `MC/src/workflow/builtin/research-experiment.js:27–164` | 演进实验绑定固定评测、机器证据、差异及撤销核验 | 来源metric由Agent报告，未核验撤销就记reverted，作弊审计不自动否决；不复制破坏性reset |
| M09 | 显式worktree隔离并返回成果目录/分支 | `MC/src/workflow/runtime.ts:884–949,995–1040`；`MC/test/workflow/runtime-worktree.test.ts:31–75` | 经批准的隔离代码实验、差异审查后整合 | 基于HEAD不带dirty；非OS隔离/Harness克隆；清理失败可能被吞 |
| M10 | Compose Next需求→验收→验证→独立review闭环 | `MC/src/skill/builtin/.bundle/compose-next/SKILL.md:8–30,130–158,184–192`（契约） | 原生工作包与阶段交付证据清单，而非复制技能文本 | 不是runtime强制状态机；参考分支政策不能覆盖main规则 |

### 4.9 opencode — 会话控制与恢复

目录缩写：NC=`packages/core`。以下读到实现，多项有测试源码，
全部未运行；根许可证MIT。Plan的已核实约束主要是编辑权限，不是所有
Shell/外部副作用只读。fork/revert/move都不等于隔离完整执行环境。

| ID | 借鉴亮点与价值 | 来源证据 | Aeroric原生完善建议 | 前置/风险 |
|---|---|---|---|---|
| N01 | Plan/Build分层及计划目录编辑规则 | `NC/src/plugin/plan.ts:14–65`；`NC/test/plugin/plan.test.ts:249–267` | 计划审阅与执行权限分别可见；变模式不偷换权限 | 不把Plan标签当所有操作只读保证 |
| N02 | 持久inbox区分steer与queue、输入ID去重 | `NC/src/session/inbox.ts:154–190,496–528`；`NC/test/session-prompt.test.ts:1028–1080` | 输入投递状态、队列编辑、重复提交解释 | steer不立即打断在执行工具；首次payload胜出需用户理解 |
| N03 | 崩溃恢复claim与持久次数上限 | `NC/src/session/execution/restart.ts:23–98`；`NC/test/session-execution.test.ts:291–338` | 恢复通知、attempt、阻塞原因及人工重试 | 来源默认10次非费用预算；at-least-once可能重复副作用 |
| N04 | 按消息边界fork并建立父子会话 | `NC/src/session.ts:319–358` | 比较方案、从指定点分叉、引用基线/指令版本 | 来源使用父会话最新指令，不自动隔离文件 |
| N05 | 按消息快照预览文件撤销与恢复 | `NC/src/session/revert.ts:21–73,84–113`；`NC/test/session-revert.test.ts:94–114` | 会话撤销与文件撤销分开，diff预览、确认及并发检查 | 不覆盖网络/数据库；同文件并发安全未证明 |
| N06 | 摘要压缩与provider-native压缩分路 | `NC/src/session/compaction.ts:195–237,312–342`；`NC/test/session-native-compaction.test.ts:334–380` | 上下文meter、压缩来源、可关闭和原文跳转 | 摘要有损；native加密项可能绑定endpoint，不能任意换路由 |
| N07 | 技能加载授权与所属prompt附件定位 | `NC/src/tool/plugin/skill.ts:46–66`；`NC/test/session-skill.test.ts:98–163` | 技能来源、注入到哪条任务输入、能力与权限状态 | 批准加载技能不批准其中全部操作 |
| N08 | 会话工作目录迁移先检查再边界交接 | `NC/src/session/move.ts:74–101,118–154`；`NC/test/session-move.test.ts:207–230` | 切换工作区预览、当前工具边界、目标能力说明 | 不是搬文件/复制PTY/即时改变当前工具cwd |

### 4.10 deepseek-harness — 长任务、成果与扩展发现

目录缩写：DC=`packages`。以下读到实现，多项有测试源码，未运行；
README标developer preview，`SAFETY.md:7–15`提示未安全审计且沙箱不能作为
唯一安全控制。许可证MIT。这些外部工具不能变成宿主eval入口。

| ID | 借鉴亮点与价值 | 来源证据 | Aeroric原生完善建议 | 前置/风险 |
|---|---|---|---|---|
| D01 | 计划原文审阅、反馈、pending到边界生效 | `DC/plan/plan-mode/src/index.ts:306–359`；`DC/plan/plan-mode/README.md:12–32` | 待批准/已批准/待生效分别显示，绑定被批准原文 | 来源模式是指导，工具仍可用，非写保护 |
| D02 | 持久Goal、revision校验、重启disarm | `DC/goal/goal/src/index.ts:247–267,364–381,456–462`；`DC/goal/goal-round-driver/src/index.ts:172–203,364–378` | 长任务目标、暂停/继续、确认恢复、预算与阻塞原因 | 默认256轮不照搬；Goal phase非工具权限 |
| D03 | PTC工作流固定cwd/file policy、取消等待清理 | `DC/workflow/workflow-ptc/src/host.ts:282–316`；`DC/workflow/workflow-ptc/README.md:57–61,154–158` | 外部工作流能力摘要与停止状态；独立原生流程另定owner | VM非安全边界，file policy不控网络，整体deadline未实现 |
| D04 | 后台Jobs owner隔离与观察cursor分离 | `DC/jobs/jobs-local/src/index.ts:309–342,410–481`；`DC/jobs/jobs-local/README.md:57–67` | 人和模型可各自续读后台日志，不相互消费 | 活作业/ring不随重启恢复，非持久调度器 |
| D05 | 精确事件前缀fork，未知工具结果不伪造成功 | `DC/core/session/src/fork.ts:10–29`；`DC/core/session/tests/fork.spec.ts:324–365` | 分叉前标出未开始/已派发结果未知/已完成 | unknown outcome不盲重试，会话修复非文件回滚 |
| D06 | 图片预算失败后持久offload可重放 | `DC/compaction/compaction-image-offload/src/index.ts:25–39`；`DC/compaction/compaction-image-offload/tests/projection.spec.ts:63–81` | 附件是否仍进入请求可见，重新读取是明确动作 | 预算后来增加不会自动恢复旧图片，不假装全部历史仍可见 |
| D07 | 路径校验后的deliverable声明，成果脱离聊天泡沫 | `DC/deliverables/tool-present/src/index.ts:77–108`；`DC/deliverables/tool-present/README.md:90–92` | 成果卡、文件版本/hash、类型、来源任务与打开入口 | 来源存路径非版本副本，删除/编辑/竞态影响交付；需补原生完善 |
| D08 | 扩展服务/事件/schema只读发现与临时定义分离 | `DC/extensions/tool-cordis/src/index.ts:21–69`；`DC/extensions/cordis-host-runner/README.md:54–58` | 原生能力目录、真实schema表单、安装/启用审批 | 临时definition重启消失，非持久安装或自动演进 |

## 5. 最优先的三条深入设计线

### 自进化：不是普通的三Agent轮流聊天

需要把执行任务、对行为反思、对长期方向治理、对可运行Agent代码产生
改进补丁这四件事分清。建议原生演进实验记录：被改进对象、基线版本、
角色职责、隔离环境、证据、补丁、测试、预算、用户批准、激活与回退。
先明确“允许改什么”，再讨论自主级别和循环触发，不能反过来先开自动。

| ID | 用户提出的能力 | Aeroric原始完善建议（后续设计确认见下） | 关键边界 |
|---|---|---|---|
| U01 | 主Agent执行、反思角色核对行为、治理角色监督长期方向，在独立克隆内改进Agent代码 | 建立专属“演进实验”，三角色有不同输入/写入权；实验产生可复核代码补丁和版本，而非只有自评文字 | 当前MiMo快照未证实此三角色链；被改进者不能同时改评测/审批规则；真实代码演进需要可演进对象与独立执行控制面 |

只切换模型、改提示词或调用现成技能，不能据此贴上“代码自进化”标签。
Aeroric可演进的代码对象由下述 Q13 确认为任务专属工具/执行逻辑，
不要求改写已安装CLI。普通项目迭代与Agent能力改进仍须分开取证。

**2026-10-15独立来源补充**：用户提供
`docs/research/xiaomi-mimo-self-evolution.md`，分析的是桌面包
`26.1010.101546` 的 `evolve-seed`，不是上述MiMo-Code快照。
这份静态研究及留存源码支持“会话Harness自修改＋反思自动反馈＋超我
元监督/观测”；未发现超我判决自动纠正并验证的完整链，也没有统一的
测试准入/失败回退或长期提升证明。本轮未启动App或调用模型。
轻量补充见
`.trellis/tasks/10-14-native-redesign-shell/research/mimo-desktop-evolution-supplement.md`；
保留原研究覆盖与完整性限制，不改变89条仓库观察＋U01的历史计数。
当时 Q13 仍待确认；推荐讨论受管理的专属Harness副本，不默认改安装的CLI、
Aeroric宿主或本轮评测/审批规则。此为研究与设计建议，不是执行许可。
用户随后回复“符合”，并强调围绕布置的任务、在coding过程中自进化。
当时高层意图已确认；项目代码与任务专属工具/执行逻辑的具体可改面仍待澄清，
不由此批准自动生效、跨任务继承或独立的“演进实验室”入口。

**Q13最新明确确认**：用户已同意项目代码＋任务专属副本的工具实现/
执行逻辑可改，不可自行修改权限、审批和验收标准；三角色还须支持
独立 API／模型绑定（允许相同或不同），不能仅改名称、实际共用隐式全局配置。
MiMo常规监督API快路共用解析器，主调用另有覆盖，不是可直接照搬的
三角色独立路由或不可变安全内核。后续 Q14–Q25 已确认的设计边界见
`.trellis/tasks/10-14-native-redesign-shell/research/evolution-role-model-routing.md`。

- **Q14**：全局三角色默认模板＋各任务分别覆盖，角色独立绑定API/模型。
- **Q15**：Aeroric统一控制面＋Claude Code/Codex/OMP/DSH四家族适配；
  不是四个CLI本体自改，也不是已实现的兼容认证。
- **Q16**：主执行者在授权内改代码/工具与执行；反思者和治理者只读获准
  证据、提出意见/补丁草案，不直接改工作区或执行命令。
- **Q17**：工具候选副本修改→宿主验证→安全点自动试用→失败停用并回退
  工具版本，仍遵守既有审批；不自动撤销项目代码或外部副作用。
- **Q18**：治理意见由宿主按授权规则阻断对应候选，不任意强停主任务；
  旧版安全可用且宿主允许时可继续。既有安全拦截、用户Stop及Q17故障处理不变。
- **Q19**：监督API失败/超时记失败/待审，不当作通过或模型否决；
  缺审候选不启用，安全的普通任务可继续。不静默换API/模型或以主Agent
  自审替代；有限重试，恢复后按当前候选/证据复审，不因服务故障自动回退有效工具。
- **Q20**：日常编码每阶段反思必核对、可主动反馈主执行者；治理为三角色
  最高监督/复审层，仍低于用户授权和宿主固定规则。保留Q16–Q19，不自行
  改权限/审批/验收或任意强停任务；具体消息协议与阶段门槛未因此批准。
- **Q21**：阶段核对及既定验证满足后再关闭；待核对/待整改仅阻断依赖
  路径，安全独立工作可继续。治理须及时审查、独立进程运行，不等于
  更高权限或已有OS隔离，不顺带决定反思者进程形式或治理每阶段终审。
- **Q22**：阶段反思有新结论及反复失败、角色冲突、方向偏离、工具候选
  待启用等关键事件，立即通知独立治理侧、异步及时复审；不等长周期或
  主Agent空闲，不每条命令同步双审。排队/API失败明确待审，不算完成或
  静默跳过；缺必要评审的候选不启用。未决定治理逐阶段终审或数值时限。
- **Q23**：普通编码阶段满足Q21后可关闭、推进依赖工作，不因治理单独
  待审额外等待逐阶段签字；治理仍及时异步复审，待审不等于通过或免审。
  工具候选须完成必要治理评审及既定门禁后启用；未批准分歧裁决、
  已关闭阶段重开/依赖收束协议或任意强停整任务。
- **Q24**：治理负责有据的最高模型复审，须引用既定要求、当前版本证据
  与影响范围；主执行/反思可举反证有限复审，无法收敛的实质争议交用户。
  有据的验收问题由宿主约束相关依赖/候选，安全独立工作继续；风格偏好
  不自动阻断阶段，不得自改标准或任意强停整任务。复审额度/强制判据另议。
- **Q25**：当前仍适用的实质漏检，保留原通过记录并局部重开，复核实际
  受影响的依赖链；相关新步骤暂缓，已开工工作在支持的安全点收束并如实
  标记，无关安全工作继续。整改/反思、既定验证及原问题定向复审满足后仅
  解除对应限制；争议按Q24处理，不自动回滚项目或增加普遍治理签字。
- **Q26**：首批外部交付为SwiftUI可交互原生审阅样例，以明确离线数据
  展示角色配置、阶段/评审证据和工具候选；不接真实模型、不写真实任务/
  配置、不启动治理进程。此范围已确认，布局与运行验收仍待审。

阶段/依赖机制、分歧处理细则、预算/触发细则、具体进程/权限/验证/切换机制与跨任务继承
仍待决定。上述确认不等于长期提升已证明、整组M01–M10已获批、其他
来源候选已获批、真实进化引擎已授权或stage01扩围；Q26只另批离线样例。

### 多Agent协作：团队分工加可靠任务领取

重点不是同时开多个终端，而是责任、任务依赖、单一领取、上下文交接、
工作副本、预算、审批、结果验收和失败恢复。推荐先小团队，避免直接把
“AI公司”全部组织制度搬进个人开发工具。角色不是Agent品牌；同一品牌
可以承担不同职责，不同品牌也可以互相交接。

### 规划与知识沉淀：让讨论变成可执行、可追溯资产

讨论里的目标、决定、约束、步骤、产物和验收要有独立身份；不能只有
一段“总结”。推荐先由用户审阅计划与记忆候选，再允许工作项引用其
特定版本。记录原文链接、生成者、确认者、版本、范围和失效条件。

## 6. 阶段文档与后续讨论

研究清单 → grill-me确认一个分支 → 样例审阅该界面 → 冻结一个有边界的
阶段MD → 外部模型执行 → 本线程核对diff与新证据。未经确认的分支仍留在
候选库，不偷偷进入执行MD。Q26已明确选择原生交互样例，不额外做网页替代。

每个执行MD需要目标/非目标、允许文件、业务owner、依赖、危险动作门槛、
状态与失败处理、接口、验证命令、必须证据和回退；外部模型不能因“列表
很长”一次实施全库。本轮不扩大`frontend-redesign-stage-01.md`版本1.0。

当前自进化分支 Q13–Q25 已部分确认设计边界。Q21阶段门槛、治理独立进程、
Q22及时异步复审、Q23放行门槛、Q24分歧升级及Q25局部重开复核原则已确认；
不把具体进程/消息协议、分歧/依赖强制判据或复审额度一并视作通过。
MiMo超我并无现成的最高指挥/自动纠正闭环，不能拿设计认可冒充来源事实。
阶段设计快照已整理为 `docs/native/task-evolution-design-checkpoint-01.md`，
供用户/监工/新终端审查；不是整套功能实施授权，也不是frontend阶段02。
Q26已确认首批外部交付先做SwiftUI可交互原生审阅页，以离线样例展示
角色配置、阶段/评审证据与工具候选；不接真实模型、不写真实任务/配置、
不启动治理进程。独立执行包为 `docs/native/task-evolution-review-stage-01.md`，
用户明确交给新终端执行后按其门槛实施；布局与运行验收仍未通过。
具体依赖判据、在途调用处置、预算/触发细则、进程/隔离/验证/消息协议及
继承仍待设计，不因先做审阅样本而补成产品默认值。
详见轻量研究中的
`evolution-role-model-routing.md` §11–17、
`evolution-phase-revalidation.md` 与MiMo补充“Q20 定向复核”节；
不改原报告或原始来源观察。
这是当前分支，不替代其他两条优先线及全库候选；多Agent协作、规划与
对话/知识沉淀及其余功能仍须回到各自来源ID逐项审阅，不能被遗漏或
静默视为放弃。首页是否取代Q11默认入口也仍单独待定。
Q27已确认协作默认围绕任务按需组队、不强制预建公司/部门，成员不固定
为U01三角色；并未批准自动委派、并发读写、预算/审批或协作调度器。
Q28已确认每个协作任务有明确协调负责人，默认主Agent兼任，负责
组织工作项、提出委派及汇总；若启用U01，治理保持独立监督，不做默认
日常项目经理。委派启动条件与权限等另议。
Q29已确认每次委派形成可追溯的有界工作包，带目标/依据、交付/验证、
范围/禁止事项、依赖/上下文引用及回报要求；主Agent起草，用户可查看修订。
此信息组织原则不是启动授权、固定协议或持久化/界面实现。
Q30经用户修订为可配置：默认有限协作授权，另可由用户手动开启Full Access，
对其授权覆盖的任务操作/委派免逐次人工确认，不能仅保留原受限模式。
已确认默认仅当前任务生效，不自动影响其他任务；持久化/恢复与全局默认另议。
Q31已确认：协作者沿用人工确认策略，只获工作包所需且不超委派方有效授权的
能力，Full Access覆盖内委派不重复询问，不默认复制全部权限/密钥。
反思/治理保留已确认的只读职责；不证明隔离或四家族适配已实现。
Q32已确认：独立工作并行，同一工作副本的共享修改点轮流负责，
主Agent协调交接、技术冲突、整合及验证；不预设分支/worktree或锁实现。
Q33已确认：区分成员提交、工作包技术核验与用户最终验收；核对当前
产物和实际证据，满足既定检查及适用监督门槛后再作已核验依赖。
不要求用户逐项点击验收，不替代U01阶段监督或强加全员三角色。
Q34已确认：保全成果、核对原执行/副作用、在原授权和预算内局部恢复；
安全交接前不启动重叠替代写入，不因沉默判停或盲重放外部操作。
具体故障/超时判据、重试额度、强制隔离及恢复协议另议。
Q27–Q35已汇总为 `docs/native/task-collaboration-design-checkpoint-01.md` 版本0.2，
只是阶段设计快照。Q35已确认首批SwiftUI可交互离线协作审阅样例，
展示工作包、负责人/依赖、交付核验和局部恢复；内存数据，
不启动真实Agent/API、不写真实任务或权限。独立执行包
`docs/native/task-collaboration-review-stage-01.md` 版本1.0已列文件、场景和
验证/失败边界，用户明确交新终端执行后按门槛实施，不扩大Q26交付，
不表示正式导航、布局/运行验收或真实协作引擎通过。
Q36已确认任务主计划供工作包、协作进度和阶段反思共同引用，
保留修订关联；具体计划模型、生效/审批流程与知识沉淀机制未定。
Q37已确认主Agent在既定边界内自主修订步骤/路径并留痕，
改变目标/范围、验收、授权或资源边界先交用户决定，不绕开核验/监督。
Q38已确认修订后保留成果、仅对齐受影响在途工作，无关安全工作继续；
不全队重启、不自动回滚或重放，用户Stop与宿主硬限制优先。
Q39已确认主Agent按任务复杂度先起草、关键歧义先问；需求明确且已获必要
授权时不另加计划开工签字，保留既有审批与监督门槛。
Q40已确认保留原始依据、自动整理可回源的任务内摘要，区分确认/待决、
自报/核验并保留变更关联，不扩为跨任务记忆。
Q36–Q40已汇总为 `docs/native/task-planning-design-checkpoint-01.md` 版本0.1，
只是阶段设计快照。Q41另确认自动提知识候选、用户集中审阅后项目内复用，
附来源/范围/限制，不默认跨项目/全局，不替代权限或当前验证。
Q42另确认项目级自动入库可配置、默认关闭，与Full Access独立；
关闭保留候选审阅，开启后在预设类型/来源/范围内免逐条确认，
冲突/敏感/依据不足仍留候选，不默认跨项目/全局或直接生成可执行技能。
规则字段、冲突判据、历史候选处理和关闭/撤回传播等具体机制仍未定。
Q43另确认按任务/工作包在获准项目范围按需使用精简知识及来源/限制，
必要时按权限回源，不整库注入或升级成系统指令；当前明确要求优先于
旧经验，权限/审批/验收与宿主硬限制不变，过时/冲突/缺来源明确标注，
不冒充当前已验证事实。参考F08/M03限制，不宣称新机制已经实现。
Q44另确认修订保留获准历史及替代关联、用户可停用知识；
有依据的过时/冲突先待复核，不再作确定结论推荐，已停用内容不再供
后续新上下文推荐。已引用在途任务局部核对，安全无关工作继续，
不全队重启、自动回滚或重放，不承诺瞬时清除旧上下文；
通知/ACK不等于完成更正，用户/宿主边界不变。仅参考F09规范、
L06可审阅/引用观察及Q38原则，不证明来源已有完整失效/撤回闭环。
Q41–Q44已汇总为 `docs/native/project-knowledge-design-checkpoint-01.md` 版本0.1，
只是设计快照，不是实施包。Q45已确认独立SwiftUI可交互离线知识样例范围，
候选审阅、配置独立、按需引用、修订/停用及在途影响仅用内存fixture；
不接真实Agent/API、不采集历史或写真实知识/任务/权限/配置。
执行包 `docs/native/project-knowledge-review-stage-01.md` 版本1.0已列白名单、
K1–K8、六UI方法及失败出口；交接时点设施核对见
`.trellis/tasks/10-14-native-redesign-shell/research/knowledge-review-handoff-audit.md`，
开工须重核，禁止复用清偏好delegate，不断言旧类仍存在。
用户交新终端明确执行后按独立任务及文件/构建/桌面和偏好域隔离门槛实施，
当前线程不改产品，布局/运行验收及具体知识机制未因此批准。
Q46已确认技能/流程先成带来源、适用条件和验证要求的草稿，与知识分开，
用户可修改/丢弃，默认确认内容/范围后复用。生成不等于安装/启用/执行，
聊天/知识不自动升级指令；加载不改权限/审批/验收或宿主硬限制，
Full Access不自动发布或扩范围。自动发布配置、分发/版本和具体验证另议。
Q47已确认用户可指定、Agent可在获准范围按需选用已启用且当前可用的技能，
不整库加载或重复询问，明示来源/可用性和阻塞原因；未知不冒充可用，
缺能力不静默换执行器或扩权限。用户补充新技能先人工同意才按批准范围
安装使用，Full Access不替代该同意；批准已涵盖安装/使用时不重复询问。
选用/安装不等于加载、消费或操作成功，不改任务权限/审批/验收或宿主限制。
Q48已确认Aeroric统一管理、默认当前项目启用，任务按需选用；跨项目/全局
须用户明确扩大范围，不擅改用户CLI个人配置，不改Q30任务权限作用域。
统一保存不等于全局启用/整库注入，不证明四家族已兼容；机制/适配另议，
不增加Q45范围或由本线程生成/安装具体技能。Q49已确认更新先审阅、
用户同意后启用，不静默覆盖或在途热替换；新任务按项目获准版本按需使用。
一次同意不包未来版本/扩大范围，旧版仅仍获准/安全可用且宿主允许时继续，
Stop/撤销/硬拦截优先，不承诺瞬时清除已消费上下文，不改U01工具受控试用/验证/回退。
Q46–Q49快照 `docs/native/skill-reuse-design-checkpoint-01.md` 版本0.1不是执行包；
Q50独立SwiftUI/苹果原生Liquid Glass离线技能样例范围已确认，
执行包 `docs/native/skill-reuse-review-stage-01.md` v1.0列白名单、S1–S8、
验证和失败出口；设施核对见
`.trellis/tasks/10-14-native-redesign-shell/research/skill-review-handoff-audit.md`。
用户交新终端后按独立任务/审阅/start及资源隔离门槛实施；本线程不实现或
安装技能，不扩Q26/Q35/Q45，不是布局/运行或真实机制验收。技能完整规则：
`.trellis/tasks/10-14-native-redesign-shell/research/harness-skill-reuse.md`。
按需研究：
`.trellis/tasks/10-14-native-redesign-shell/research/harness-task-planning.md`。
新规划讨论不修改已交接的Q26/Q35执行包及设计快照。
不突破真实能力/未撤销硬限制等其余边界仍待细化；协作核验与适用监督
已由Q33确认。动作覆盖集/继承执行机制、额度/人数/层级、执行器兼容、
撤回与原生设置另议；
不是本线程实际提权、启动执行或四家族已兼容的证明。
见轻量决策台账§5–29及配套按需研究：
`.trellis/tasks/10-14-native-redesign-shell/research/harness-task-collaboration.md`；
两条工作线独立，协作设计不纳入Q26离线样例执行包。

Q51另确认C04/C05相关的待办/执行任务/自动化对象关系，不批准其整套实现；
Q52另确认人工/Agent同获准范围内共用对象与发起来源追溯、权限独立，
不是已实现工具接线或授权强制。Q53另确认默认记录、明确交办执行及可选
有范围自动接单，与Full Access分开、任务授权不默认外扩。
Q54另确认规则独立取得用户授权，覆盖内免重复询问；规则Full Access须
另授、不自动继承或默认全局开放，既有门槛保留。Q55另确认暂停/停止分开，
保全成果/历史、未核实不宣称已停，明确暂停/停止后不自行恢复，协议另议。
Q56另确认Agent待办按目标核验技术完成，区别于运行结束及用户最终验收，
不强制每项人工签字、不代用户确认。Q57另确认明确筛选/预览与单项排除，
有效范围内新增免逐项确认。Q58另确认首次启用不自动回填积压，合条件旧项
可明确批量纳入，不盲重放或自动重开完成/待验收项。Q59另确认执行相关规则
变更先展示差异/影响并确认，面向后续启动、未启动重核，在途保留启动依据
不静默换模型/API或目标；明确调整按Q38局部对齐，Stop/撤权/禁令/宿主限制
优先，旧版不保留已撤回权限。Q60另确认定时规则复用逐规则授权/变更/
执行控制原则，保存/到点不授权，明确启用且覆盖内免逐次确认；
Full Access另授，新技能人工同意不变。不合并触发器或强制每次先建待办，
不将待办积压套成定时补跑策略。Q61另确认已核实错过且未启动的定时机会
默认不追赶历史，可明确逐规则开启恢复后补一次当前执行，覆盖内免逐次重问；
不冒充历史完成，在途/未知先核实，暂停/停止不自行启用，逐期历史处理另定。
Q62另确认同规则默认不重叠、繁忙到点默认跳过，可独立明确开启结束后补一次，
多次繁忙触发最多保留一次当前执行意图，不继承Q61；安全收束、当前条件
满足后才启动，手动/恢复不绕过，未知先核实，暂停/停止不自行恢复。
Q63另确认跨任务实际冲突局部协调，安全独立工作继续；不抢占/强杀/删锁，
未知先核实，交接后仍核对当前意图/授权/依赖，不自行恢复暂停/停止，
不将Q62跳过变排队补跑，Full Access不绕过资源约束。完整已确认规则见
`.trellis/tasks/10-14-native-redesign-shell/research/harness-work-arrangements.md`。
Q64另确认已有交办且允许等待的工作重核后覆盖内自动继续，变化局部对齐，
不盲套补丁/全项目重做，变目标/权限/验收先问，取消/已解决/未知先核实，
不恢复暂停/停止或新增重试/补跑权。Q65另确认可配置共享资源护栏，
局部不绕过项目/总额，相关委派/监督/重试/补跑纳入消耗，到限阻止受限新准入；
不自行扩额或降低监督，预估/报告/未知分开，不假报零/精确账单，
必需硬限制不能执行则如实受阻，Full Access不免资源边界。
Q66另确认到限分类处置：并发/尝试机会用尽只挡新准入，不为此打断在途；
对在途生效的执行边界到限则挡受限扩张、按能力请求安全收束，保全成果/
未知事实，缺审不算通过，用户控制及宿主硬限制优先，不强杀/删锁。
Q67另确认补额与恢复执行分开，到限收束后默认不重启、可明确开启范围内
自动续行；普通获准等待仍按Q64自动继续，重核原执行/副作用与当前条件，
不恢复用户暂停/Stop、清零独立上限或复活跳过触发；合法新触发仍按原准入重核。
Q68已确认阶段文档与离线原生样例范围，当前项目不创建VM，App/UI暂停不解除。
快照 `work-arrangements-design-checkpoint-01.md` v0.1，
独立执行MD `work-arrangements-review-stage-01.md` v1.0；本线程不实施或开工。
完整续页见
`.trellis/tasks/10-14-native-redesign-shell/research/harness-execution-readiness.md`。
创建待办不即执行、规则不等于各次运行、结束不等于验收，直接Agent任务保留；
授权/预算/离线触发与原生呈现另议，不扩既有样例或冻结交付。

Q69–Q75另完成C08/X04/N02执行中沟通分支的原则与阶段范围确认：
统一输入区分当前补充/下一轮/原问题回答；经确认仍可控未交出才可编辑/
撤回，已交出追加关联更正、未知不冒充撤回成功；默认协调负责人，可明确
成员定向，回答锚定原问题，共享影响局部同步而非广播扩权。
“待你决定”集中保留来源/推荐/阻塞，未答不视同意，独立工作继续，答案
不是审批。下一轮发送即交办、条件满足自动接续，同收件人按顺序，
暂停/Stop不被队列唤醒；普通补充保留旧要求，明确更正只替代对应部分，
含糊实质冲突局部澄清，不机械执行已明确过时要求或伪装已撤销旧操作。

Q75已确认两份文档：`docs/native/task-communication-design-checkpoint-01.md`
v0.1与 `docs/native/task-communication-review-stage-01.md` v1.0。
首批仅交接独立虚构内存SwiftUI/苹果原生Liquid Glass样例，TC1–TC8是五类
关系的有限细分，不是八个新模块或生产协议；本线程不实施/开工。
用户交新终端后仍按独立任务创建授权/审阅/start、文件与构建资源单写
交接及逐项非窗口验证门槛实施，不接真实Agent/服务或改正式首页/旧包。
不创建/恢复/准备替代VM、不安装VM工具；App/Preview/真实UI暂停保持，
运行验收仍NOT RUN/DEFERRED。快照/样例不证明真实投递、采纳或权限强制，
四类执行器＋Aeroric控制面与OMP边界保持，不新增第五个编码执行器。
完整确认与历史建议见
`.trellis/tasks/10-14-native-redesign-shell/research/harness-inflight-communication.md`。
总库仍10来源、89观察＋U01，不因本分支全部获准实施。

## 7. 2026-10-20：backlog 来源增补

**本节及后续归并均为研究/待讨论建议，不是全库实施授权。**
§1–6 的10来源、89观察＋U01保留为首轮历史，不删旧ID、不覆盖已确认决定。
新来源根：`/Users/macbook/Downloads/同步空间/LYX/harness/backlog`。
路径在各来源表中相对该项目根；代码只是阅读，来源测试和程序均未运行。

用户最新反馈：新建会话浏览器 V1 **总体样式大致可接受，但功能不完整**；
不能记成整屏最终批准、全部功能通过或原生实施许可。工作方法/高级配置
不能由几项虚构样例充当最终功能边界，待本轮研究及讨论后再修订浏览器稿；
每屏仍须用户审批。Q107–Q113、原生 SwiftUI/Liquid Glass、UI/VM 暂停不变。

### 7.1 清点、方法与可信度

七项目清点时均 clean，共12,436个过滤后文件，不表示逐个阅读。
采用与首轮相同的“README/目录 → 重要入口 → 状态/控制实现 →
必要测试源码 → Aeroric对照”方法，不按文件总数耗尽上下文。
三组独立只读研究覆盖五项目；主线程研究ponytail/kloop并负责整合。
无联网更新、依赖安装、第三方执行、真实Agent/API/UI或VM操作。

| 新来源 | 本地HEAD简写 | 过滤后文件数 |
|---|---|---:|
| magpie | `75e769a9f9b4` | 5,541 |
| runner | `a9d3b2902695` | 1,296 |
| ponytail | `09c434ab73ca` | 214 |
| herdr | `143bf5fd4781` | 1,739 |
| kloop | `8d9c1c716e2b` | 481 |
| cc-switch | `74cf8e6f0b12` | 1,647 |
| CometixCode | `bad7e08c2c4d` | 1,518 |

完整HEAD、排除规则、覆盖限制、主线程细节及Aeroric已有基础对照：
`docs/research/harness-backlog-2026-10-20.md`。
**实现 ≠ 运行通过；测试源码 ≠ 已跑测试；技能规范 ≠ 宿主强制；原生化建议 ≠ 已交付。**
许可证仅记录本地声明；本轮不复制源码/资产，不作复用许可结论。

### 7.2 ponytail — 最小完整改动、审查配方与质量反馈

根LICENSE为MIT。主体是技能/规则与薄适配器，不是新Agent内核。
PT01–PT04为技能规范，其余读到实际代码；均未运行。
来源作者的跨宿主验证和节省比例不等于Aeroric认证或收益承诺。

| ID | 借鉴亮点与机制 | 来源证据 | Aeroric原生完善建议 | 前置/不能照搬 |
|---|---|---|---|---|
| PT01 | 先查影响面，再按已有实现/平台/依赖选择最小完整方案 | `skills/ponytail/SKILL.md:19–44`（规范） | “复用优先”质量策略与现有组件导航；补强C03/M10 | 不削减用户功能、安全、无障碍或验收；不是自动删代码 |
| PT02 | diff审查追调用者，具体问题/修复/不修后果分级报告 | `skills/ponytail-review/SKILL.md:16–67,74–101`（规范） | 变更审查配方，Finding绑定当前版本与证据；补强U01/M10 | 配方的“Ship”不是宿主验收事实；不只看改动行 |
| PT03 | 全库审计先画入口/数据流/负载，明确未读范围 | `skills/ponytail-audit/SKILL.md:16–28,57–103`（规范） | 项目体检任务，按风险抽样、逐项追证据 | 不是每次发消息全仓扫描；不自动修复全库 |
| PT04 | 收集技术债注释里的上限/回访触发，缺触发单列 | `skills/ponytail-debt/SKILL.md:13–44`（规范） | “改进清单”关联代码位置/负责人/复核触发，与待办引用 | 不是已实现常驻扫描器，注释不自动变执行授权 |
| PT05 | 复用地图以顶层名称和目录输出，文件/字符预算明确 | `hooks/ponytail-map.js:16–34,37–106`（实现） | 项目上下文的“已有能力地图”，按需回源；补强F08/M03 | 正则非AST；fallback不完整遵循gitignore，不越获准目录 |
| PT06 | 一次审查、会话级别、持久默认分开；会话切换恢复 | `hooks/ponytail-mode-tracker.js:59–76,105–155`；`pi-extension/index.js:197–252`（实现） | 可版本化编码/审查策略，按四族真实能力适配；补强X08/N07 | “严格/精简”不是模型推理强度/权限；不改个人CLI配置 |
| PT07 | 跟踪新增函数、已有函数调用及删除后疑似残留调用 | `hooks/band/metrics.ts:12–89,298–300`；`hooks/band/register.tsx:260–277,297–310`（实现） | 审查辅助提示和复用建议，与PT05共享索引 | 名字/正则启发非语义质量评分；不设复用率KPI或据此阻断 |
| PT08 | 上次测试成功在后续编辑后标为过时 | `hooks/band/register.tsx:242–277`；`tests/band.test.tsx:415–427`（实现/测试源码） | 统一验证卡显示未运行/失败/通过/已过时，绑定输入版本；补强D07/U01 | 来源只识别部分命令和Edit/Write；Aeroric不从ANSI正则造权威结果 |
| PT09 | 当前任务质量/用量摘要，项目账本与Markdown导出 | `hooks/band/metrics.ts:214–295,304–316`；`hooks/band/register.tsx:282–320,411–436`（实现） | 会话紧凑摘要＋统计页下钻，复用Q94–Q103口径；补强L08 | 恢复分类有损/费用可估算；不扫外部homes或默认发webhook |
| PT10 | 多实验臂、固定条件、质量与成本分开计算 | `benchmarks/agentic/run.py:34–52,69–77,400–417`；`benchmarks/results/2026-10-07-agentic.md`（实现/作者报告） | U01候选工具/技能的对照评测，报告失败/超时/未检查 | 本轮未复跑；特定样本不泛化，不能自行改验收门槛或只按便宜胜出 |

### 7.3 kloop — 上下文连续性、能力绑定与恢复事实

根LICENSE、`rust/Cargo.toml:16–20`为Apache-2.0，NOTICE有派生代码归属。
KC=`rust/crates/core/src`；KS=`rust/crates/server/src`；KL=`rust/crates/cli/src`。
以下均追到实现，部分另读测试源码，未运行；不引入第五个Agent内核。
`docs/capability-report.md`是来源自评，当前代码细节优先，不能继承“反超”结论。

| ID | 借鉴亮点与机制 | 来源证据 | Aeroric原生完善建议 | 前置/不能照搬 |
|---|---|---|---|---|
| K01 | 实际只读计划模式、全文审阅、拒绝后保留计划状态 | `KC/tools/plan_mode.rs:24–49,79–123` | 可选规划配方与权限事实分开；补强D01/N01 | 不覆盖Q39：明确获准的普通任务不统一加开工签字；外部CLI须有能力 |
| K02 | 主会话TODO整表校验、revision与有界提醒 | `KC/tools/todo.rs:56–69,101–124,131–143` | 轻步骤列表引用Aeroric主计划；补强F01/O01 | 当前无行ID/持久DAG，旧计划文件名不作实现证明；不再造任务系统 |
| K03 | 用户steer/同伴消息/后台结果/定时输入分型，边界claim后commit | `KC/inbox.rs:26–64,89–169`；`KC/agent_mailbox.rs:818–844` | 与Q69–Q75同一收件箱，来源、送达、采纳、完成分离 | 来源活体内存并非持久必达；同伴文本不是用户授权 |
| K04 | 摘要外独立继承用户原话，预算内经权限重读近期文件 | `KC/compact/anchors.rs:26–75`；`KC/compact/restore.rs:35–99`；`KC/compact.rs:361–364,521–529` | 任务上下文包保留确认依据/遗漏，补强Z03/M03/L05 | 有截断，不叫“永不遗忘”；不改外部CLI历史/压缩或绕权读文件 |
| K05 | 仅请求视图瘦身，冷缓存时冻结stub，原始历史及回源保留 | `KC/request_reduction.rs:1–28,229–269`；`KC/rollout.rs:324–328` | 能力支持时接压缩统计与附件按需展开；补强N06/D06/F10 | 无接口时只管理自身上下文包，不宣称控制四族缓存/节省比例 |
| K06 | 证明重复读当前上下文行区间后提醒，外部文件变化另提示 | `KC/tools/fs.rs:55–89,104–140`；`KC/file_state.rs:259–293,325–369`；`KC/tools/plan151_acceptance_tests.rs:1–11,72–104` | 上下文诊断/成本提示、文件依据过时标记，补强PT08/M03 | 不是通用死循环判定，不因沉默/重复自动杀进程或否决任务 |
| K07 | 模型/网关事实分开；请求冻结路由、revision和提交回执 | `KC/provider_route.rs:23–77,340–369,779–801,847–895` | 扩现有Agent配置/连接与路由owner，记录角色/模型快照 | 来源pin强度跨配置默认不照搬；遵守Q111/Q112与监督独立绑定 |
| K08 | deferred工具凭据绑定workspace/权限epoch/Agent，执行时复核动态来源 | `KC/tools/tool_search.rs:30–73`；`KC/tools/mod.rs:1117–1153` | 统一能力目录的按需发现、代际失效及host拒绝；补强D08/Z09 | 搜到/显示不等于授权调用，不开放任意未广告方法 |
| K09 | MCP就绪回执区分认证/目录代际/健康/失败与陈旧 | `KL/mcp/lifecycle.rs:29–106,132–158` | 补强C07及Q76–Q83工具状态，保留最后已知与当前不可用 | 服务在线≠认证可用≠目标获准；目录失败不伪装空工具或成功 |
| K10 | 路径锁、句柄相对提交、当前字节核对，失败明确重读范围 | `KC/tools/fs.rs:701–755,1245–1285,1353–1366` | 原生补丁应用/交付检查器复用版本预检与最小冲突说明；补强Z07/N05 | 来源Edit可容忍读后变化并依赖唯一anchor，不是全部要求fresh read；不承诺外部CLI写入可拦 |
| K11 | 工作流持久run/临时执行分开，拓扑ID＋完整输入匹配journal | `KC/tools/workflow.rs:111–176`；`KC/tools/codemode/journal.rs:1–7,28–59,141–171` | 固定typed工作配方、稳定节点与恢复核对；补强Z02/F02/D03 | 缓存不是副作用exactly-once；不eval第三方脚本，不创建VM |
| K12 | 已开始但结果未知/未开始分开，取消后核验进程树收束 | `KC/rollout/sessions.rs:684–705`；`KC/tools/bash.rs:375–420` | 原生恢复/停止检查器与Q34/Q66共用实际运行身份 | 请求停止≠已停；不强杀他人持有者、不删锁、不盲重放 |
| K13 | 用量按provider/model/路由修订/采样或压缩记录；重连按代际cursor补快照 | `KC/usage.rs:8–40,75–118`；`KC/rollout.rs:310–328`；`KS/events.rs:454–510` | 统计来源归属＋设备read model重同步，补强L03/L08/F07 | reported usage非真实账单/所有失败成本；事件ring非永久审计/执行授权 |

### 7.4 magpie — 连接控制、资源部署与可解释路由

核心是跨Agent配置/模型连接和网关，不是独立编码Agent。根LICENSE为MIT，
另有第三方归属及可选模型制品，未审计全部许可。下列追到实现；
MAG09/MAG11另读测试源码，均未运行。入口 `main.go:15`、
`plugins/all/all.go:5–19`；源码不是全适配器/协议矩阵通过证明。

| ID | 借鉴亮点与机制 | 来源证据（实现，未运行） | Aeroric原生完善建议 | 前置/不能照搬 |
|---|---|---|---|---|
| MAG01 | 记录自己应用的字段和漂移，命名方案恢复配置/模型/资源 | `internal/agent/applied.go:96–146`；`internal/profile/profile.go:174–183,227–301`；`internal/edit/file.go:99–165,320–367` | 配置方案＋实际生效差异、部署预览，补强X08 | 跨Agent首错停止不是整体事务；不自动接管个人配置或覆盖外部修改 |
| MAG02 | 账号可见模型、厂商目录、上下文/图片/推理/价格分来源；失败不清空选择 | `internal/provider/models.go:31–106,223–280`；`internal/agent/route.go:145–199` | 连接/模型能力目录与草稿失效说明，补强C02/K07 | 目录声明不等于当前账户可调用，混合协议组不冒称原生兼容 |
| MAG03 | 多协议请求/流式IR转换，结构化工具call/result桥按run/call ID配对 | `internal/gateway/gateway.go:5708–5758,5949–5971,5997–6025`；`internal/gateway/ir.go:23–78`；`internal/gateway/claude_subscription.go:3468–3552` | 按矩阵扩现有local_router，保留未知内容/调用身份 | 非通用无损转换，sealed reasoning可绑定协议；订阅桥含跳审批参数，不照搬 |
| MAG04 | 配额窗口、余额、共享池、重置与缓存/延迟/错误分开 | `internal/provider/quotas.go:27–47,55–115`；`internal/provider/quota_alert.go:261–288` | 连接页配额摘要与告警，统计页回源；补强P05/L08 | 供应商余额不是Aeroric任务预算，读取失败不补0 |
| MAG05 | 账号池顺序/轮转/低用量/重置节奏，循环检测，失败接替和缓存亲和 | `internal/routes/strategies.go:57–161`；`internal/provider/group.go:1335–1355`；`internal/gateway/fallback.go:1727–1767`；`internal/gateway/affinity.go:460–551` | 可解释、用户预设范围内的连接弹性策略 | 路由接替不是重跑任务；不暗换监督API/模型或越预算，严格亲和可能重加被cap排除账号 |
| MAG06 | 条件/意图首匹配，同turn保持路由，分类超时/缓存/在途合并可追踪 | `internal/provider/grouprule.go:334–435`；`internal/gateway/rules.go:26–98,163–198`；`internal/gateway/classify.go:35–38,106–137,320–332` | “为什么用这个连接/模型”决策轨迹；高级候选 | 意图分类器不是权限/验收裁判；默认不隐式换绑，费用/外发范围先定 |
| MAG07 | 请求—尝试—实际账号/模型关联，接替失败不重复算整请求 | `internal/usage/usage.go:26–198`；`internal/gateway/gateway.go:2953–2968`；`internal/otlp/otel.go:164–185` | 扩现有metrics，保留requested/sent/reported及计时/缓存来源 | 不是官方对账，正文采集/外发另控，历史不按今日配置重归属 |
| MAG08 | 拆系统/工具/记忆/文件/对话构成，估算后按报告总量校准 | `internal/gateway/prompt.go:43–89,234–294,331–408`；`internal/gui/context.go:199–283` | 上下文检查器与缓存/增长诊断，补强M03/K05 | 各项比例仍估算；token骤降推断压缩不是CLI确认 |
| MAG09 | 跨Agent资源编译与Applied台账，子代理文件hash保护用户修改 | `internal/library/sync.go:80–116,125–165,200–255`；`internal/library/subagents.go:259–365`；`internal/library/subagents_test.go:130–187` | 一个资源库、各执行器部署适配器，逐项成功/冲突/不支持；补强D08/X08/N07 | 不同资源保护强度不同；资源发现不等于安装/启用，技能仍人工同意 |
| MAG10 | Provider插件和middleware分层，IPC信用窗/帧/总预算回压 | `internal/plugin/host.go:130–179`；`internal/middleware/middleware.go:1–8`；`internal/middleware/run.go:129–189`；`internal/plugin/store.go:362–410,730` | 有界扩展生命周期/健康诊断，补强D08/Z08 | 不构成OS沙箱；middleware失败可放行，不能承担不可绕过的安全门 |
| MAG11 | 远端地址/共享开关/key分层；命名key作用域与在途预算预占 | `internal/gateway/lan.go:246–283,407–445`；`internal/gateway/key_limit.go:36–95`；`internal/budget/budget.go:338–426`；`internal/gateway/key_limit_test.go:63–111,166–208` | 宿主连接身份和任务预算关联，补强L04/P05/O07 | 估算准入可单次超额，非硬费用封顶/完整RBAC；各工具入口强制性须另核 |
| MAG12 | 共享搜索/fetch显式schema、后端接替；媒体异步job与产物回执 | `internal/gateway/search_mcp.go:117–163,190–213`；`internal/gateway/search_api.go:107–163`；`internal/imagemcp/imagemcp.go:155–204,484–494` | 原生工具任务与交付卡共享持久job/receipt，补强D04/D07 | 断线仍后台运行≠Stop；补路径/版本/去重，不盲重发媒体生成 |
| MAG13 | 秘密映射成稳定可逆占位符；要求脱敏却不可用时拒绝外发 | `internal/redact/redact.go:305–326`；`internal/gateway/redact.go:17–31,103–137` | 可配置外发预检/脱敏范围与残余风险；连接、工具和导出共用策略 | 识别会漏，映射非租户隔离，脱敏不代替授权/目标校验 |
| MAG14 | 分部分加密备份、条件同步、冲突输家留存 | `internal/backup/backup.go:44–46,65–106,266–351`；`internal/davsync/davsync.go:1042–1099,1170–1223` | 原生配置迁移包，逐项diff/来源/凭据单选/冲突保留 | 非强事务/语义合并；来源同步能安装插件，Aeroric不能自动传播新技能或Full Access |

重点反证：`internal/gateway/claude_subscription.go:1919–1938` 的桥使用
`--dangerously-skip-permissions`，不能替换本项目交互PTY/审批路径。
来源账号自动发现并非默认关闭，不能照搬去扫个人homes/钥匙串；
`internal/settings/accounts.go:14–36`、`internal/provider/discovery.go:34–38`。
亲和策略可能重加被本地cap排除的账号：
`internal/gateway/affinity.go:379–412`，安全硬限额应在最终准入独立强制。
媒体任务输出及后台策略须重设计：
`internal/imagemcp/imagemcp.go:225–249,631–679`。

### 7.5 cc-switch — 配置投影、发布恢复与连接可靠性

本地清单版本4.0.16，定位多CLI配置/Provider/代理/资源管理，不是Agent内核。
根LICENSE声明MIT；未全面审计依赖、OAuth或订阅服务使用条件。
ST=`src-tauri/src`；以下均读到实现，另标测试源码，均未运行。

| ID | 借鉴亮点与机制 | 来源证据 | Aeroric原生完善建议 | 前置/不能照搬 |
|---|---|---|---|---|
| CCS01 | 切换Provider只投影归属字段，保留无关配置/注释 | `ST/commands/provider.rs:148–180`；`ST/services/provider/mod.rs:6552–6644`；`ST/services/provider/claude_direct.rs:30–88`；测试源码 `ST/live/patch/toml.rs:290–315` | 配置差异预览、字段所有权与外改保护，补强MAG01/X08 | 来源将subagent/fallback/advisor模型也纳入Provider字段，不能联动Aeroric独立监督绑定 |
| CCS02 | 暂存/hash/pending journal及崩溃前滚，检测到的外改目标会跳过 | `ST/live/engine.rs:8–10,128–183,199–213`；`ST/mode/operation.rs:65–204,208–292`；测试源码同文件`:857–878` | 复用core原子发布/操作台账，逐目标显示已应用/冲突/待恢复；另审外部写入竞争 | 来源非全局事务，部分跳过仍提交指针；发布/恢复存在检查到写入竞态（TOCTOU），不能保证并发零覆盖；进程内锁不替代单写者 |
| CCS03 | 认证来源、受管理账号绑定、删除竞争保护，摘要不返回token | `ST/provider.rs:359–382,668–684`；`ST/commands/auth.rs:18–40,372–393`；`ST/proxy/providers/codex_oauth_auth.rs:1956–1990,2036–2047` | 角色绑定明确account/credential引用，不把凭据写进草稿或快照 | 空account_id跟随默认会隐式改绑；来源0600明文JSON不是Keychain，不照搬认证存储 |
| CCS04 | 协议适配器、显式模型映射与流式工具参数完整性处理 | `ST/proxy/handlers.rs:171–214,307–414`；`ST/proxy/providers/adapter.rs:16–68`；`ST/proxy/providers/transform_codex_anthropic.rs:72–99,229`；测试源码 `ST/proxy/providers/streaming_codex_anthropic.rs:963–1007` | 在既有local_router扩明确兼容矩阵，保留unknown/截断/未完成 | 非无损通转；completed事件名可能承载incomplete，不以事件名判任务成功；广泛矩阵文档不能作全实现证明 |
| CCS05 | 有序fallback、错误分类、熔断Closed/Open/HalfOpen及切换记录 | `ST/proxy/provider_router.rs:59–186`；`ST/proxy/circuit_breaker.rs:133–174,203–283`；`ST/proxy/forwarder.rs:781–901,3306–3351`；`ST/proxy/failover_switch.rs:38–100` | 补现有路由的可解释候选队列/错误分类，受用户预设范围约束 | API重试不是任务幂等，上游可能已收到；不暗跨账号/角色/外发域，来源官方Codex路由也有禁跨卡分支 |
| CCS06 | Profile引用Provider/MCP/Skill/Prompt，差异应用及缺项告警 | `ST/services/profile.rs:197–241,334–460`；测试源码 `src-tauri/tests/profile_roundtrip.rs:396–448` | “环境方案”保存引用及版本约束，预览完整/部分/失配 | 来源4项缺失仍应用其余并标当前，非事务；不能把部分成功显示为完整就绪或当成工作配方 |
| CCS07 | 资源单源、目标部署及部分路径hash/symlink外改保护 | `ST/services/skill.rs:849–884,2436–2453,2520–2564,2578–2643`；`ST/services/prompt.rs:150–228`；`ST/services/mcp.rs:121–159` | 同一技能/MCP/提示库编译到各受管理执行器，补强MAG09/RUN08 | Pi保护不泛化全部客户端、Pi不等于OMP；MCP可能先DB enabled后写失败；新技能/更新仍人工同意 |
| CCS08 | 代理和日志用量归并、offset/tail指纹增量去重、Decimal与缺价未知 | `ST/services/session_usage.rs:137–168,601–689,1225–1291`；`ST/proxy/usage/calculator.rs:44–128`；`ST/services/usage_cache.rs:13–46,105–115`；`ST/commands/provider.rs:506–546` | 扩现有metrics的来源去重、新鲜度和价格版本，补强L08/MAG07 | 额度脚本可非GET且custom绕HTTPS同源检查，见`ST/usage_script.rs:247–269,502–540`；不能把任意脚本当无害读数 |
| CCS09 | 结构化历史tool/result配对、指纹缓存、内容引用与分块返回 | `ST/commands/session_manager.rs:12–109`；`ST/session_manager/providers/claude.rs:274–313`；`ST/session_manager/cache.rs:17–20,212–230,286–308`；`ST/session_manager/content.rs:79–147` | 保来源的会话检索/引用，与Aeroric既有sessions共用owner | 先全解析再分块非真惰性，坏行可跳过非完整审计；恢复入口接受任意command/cwd不可借，host仅收typed session引用 |
| CCS10 | 恢复前暂存校验、SQL authorizer、确定性ZIP与hash manifest | `ST/database/backup.rs:39–82,155–205,1004–1076`；`ST/services/sync_protocol.rs:170–230,334–402`；`ST/services/webdav_sync.rs:53–92` | 配置迁移/恢复收据，复用已有storage备份边界，补强MAG14 | hash不是加密/签名，来源可上传含凭据SQL/ZIP；上传后ETag不是CAS，不复制明文同步策略 |

关键反证：`ST/live/floor.rs:28–80` 的字段归属含监督相关模型；
`ST/commands/session_manager.rs:125–190` 的恢复命令由renderer传入；
`ST/database/backup.rs:86–104` 未排Provider，`ST/database/schema.rs:27–31`
的配置可含敏感字段。这些均不能成为Aeroric实现范例。
未覆盖全部Provider、OAuth、转换流、安装路径、入站鉴权和同步并发。

### 7.6 CometixCode — 结构化控制、消息边界与保守工具编排

实际产品是Rust编码Agent/TUI：`Cargo.toml:1–19` 声明
cometix-code 0.2.0，二进制cometix，入口 `src/bin/cometix.rs`。
`README_CN.md:15–34` 自述非官方复刻且未完整，不将自述作权属结论。
本地声明 **AGPL-3.0-only**（Cargo.toml、LICENSE），本轮不复制源码/资产、
不引入第五内核。以下是实现/测试源码静态观察，不是SDK完整性或安全认证。

| ID | 借鉴亮点与机制 | 来源证据 | Aeroric原生完善建议 | 前置/不能照搬 |
|---|---|---|---|---|
| COM01 | 查询/显示/控制分离，权限request ID，EOF终结pending，无resolver拒绝 | `src/query.rs:269–343,414–446`；`src/query_engine.rs:1048–1084`；`src/cli/structured_io.rs:195–258,539–619`；测试源码同文件`:2473–2503` | 四执行器能力广告、控制关联ID与断线pending处理，补强RUN06/HER07 | 取消不等ACK；部分功能未移植；不能用ANSI补造控制事实或复活OMP RPC UI |
| COM02 | 权限结果保留来源/实际context，子Agent局部拒绝状态 | `src/types/permissions.rs:8–53,71–90`；`src/hooks/use_can_use_tool.rs:341–417,584–619`；`src/utils/permissions/permissions.rs:861–935,1097–1109,1170–1183` | 有效授权摘要、角色/工具裁决回执，补强X02/P04 | 来源Plan在特定条件可Allow，并非天然只读；AI Auto或角色名称不能暗提权 |
| COM03 | 沙箱配置/可用性与真实shell包装分开 | `src/tools/bash_tool/should_use_sandbox.rs:16–33`；`src/utils/sandbox/sandbox_adapter.rs:277–346,403–424`；`src/utils/shell/mod.rs:269–282` | 环境预检显示实际约束/不能落实原因，补强P09/X03 | 来源依赖缺失会降成普通shell；Aeroric要求隔离时不能静默降级；不创建VM、worktree不是沙箱 |
| COM04 | Now/Next/Later FIFO，Agent目标隔离、同mode批次、安全边界消费 | `src/utils/message_queue_manager.rs:17–75,156–231`；`src/utils/queue_processor.rs:31–43`；测试源码同文件`:59–91`；`src/query.rs:3833–3858` | 与既有Q69–Q75合并为一种反馈/队列控制，补强K03/RUN02 | 来源Mutex/Vec不持久；入队/送达/采纳/完成分开，不能承诺全CLI立即steer |
| COM05 | 压缩成功才commit，失败保原消息、有界连续尝试 | `src/services/compact/auto_compact.rs:198–289`；`src/services/compact/compact.rs:582–742`；测试源码`src/services/compact/auto_compact.rs:476–532` | 上下文视图保原文、提交点/取消和失败状态，补强L05/K04 | `src/services/compact/reactive_compact.rs:36–39,74–76`恒禁用/no-op，不算溢出恢复已实现；不能改外部CLI原历史 |
| COM06 | 会话恢复重建Agent/Skill/已读/TODO元数据，文件checkpoint另行维护 | `src/utils/session_restore.rs:325–374,414–468`；`src/utils/file_history.rs:294–312,429–470` | 恢复计划分别列会话、文件和外部副作用，补强N04/N05 | 来源rewind逐文件吞错仍Ok，不复制其成功策略；会话恢复≠文件回滚≠撤销外部副作用 |
| COM07 | 子Agent共享/隔离按字段，父取消链接、cache克隆、局部tool/deny状态 | `src/utils/forked_agent.rs:64–98,119–192`；`src/tools/agent_tool/run_agent.rs:533–582,708`；`src/tools/agent_tool/fork_subagent.rs:108–119` | 工作包实例快照、显式继承/覆盖与局部失败，补强P02/RUN01 | 来源父权限可优先子模式，不证明独立三角色API/受限监督；普通协作者遵Q31，监督仍Q16只读 |
| COM08 | 保守工具并发分批：未知/无效/不安全串行，permission-blocked独立 | `src/services/tools/tool_orchestration.rs:20–24,177–229,286–339`；测试源码同文件`:714–732`；`src/query/config.rs:75–80` | Aeroric拥有的typed工具job可按资源/副作用约束并行 | MCP readOnlyHint仅声明；不重排外部CLI内部工具，不移植Agent loop；流式路径另有feature gate |

未审全部插件、团队/远控、feature组合、网络沙箱和工具权限路径；
“有对应文件”不等于可独立使用或已完整接入。未运行来源程序/测试。

### 7.7 runner — 团队、持久收件箱与执行实例连续性

Rust workspace 0.16.6，含core/daemon/cli/terminal/app；daemon持PTY，
客户端GPUI不是Aeroric前端方案。当前HEAD根LICENSE声明MIT；
`README.zh-CN.md:225` 另注明2026-08-22至2026-09-22版本仍按GPL-3.0-only，
不能把当前声明套到历史。未审全部第三方/打包组件。
RD=`crates/runner-daemon/src`；RC=`crates/runner-core/src`；
RA=`crates/runner-app/src`。以下均追到实现，另标测试源码，均未运行。

| ID | 借鉴亮点与机制 | 来源证据 | Aeroric原生完善建议 | 前置/不能照搬 |
|---|---|---|---|---|
| RUN01 | 角色—团队—任务配方，单lead校验和成员执行器/model覆盖 | `RD/ops/mission.rs:165–205`；`RD/ops/mission/roster.rs:21–54,87–113` | 角色模板与工作包执行实例分开，补强P01/P02 | 恢复仍读取当前Role，非完整不可变快照；`RC/protocol/permissions.rs:62–68`默认Bypass不采用，角色提示不是权限强制 |
| RUN02 | 文件消息流、定向收件箱、单调读水位，投递校验generation/input revision | `RC/event_log/log.rs:125–138,180–225`；`RD/event_bus/mod.rs:447–508`；`RD/router/dispatch.rs:205–292,321–390` | 由core单写者持久化因果消息及交接引用，补强O02/N02/K03 | 来源CLI也直接追加且append无fsync；“输入清空”不证明Agent已读/接受；草稿/人工输入不能被投递覆盖 |
| RUN03 | 人工问题按question_id归原提问者、关联source_event，失败恢复pending | `RD/router/handlers.rs:28–79`；`RD/router/mod.rs:200–236` | 原生待回应问题卡、回答路由与任务来源，补强C08/P04 | 普通问答不是工具审批，回答“是”不自动授权扩大范围或安装技能 |
| RUN04 | 团队先注册、逐槽启动，单槽失败其余继续，取消挡未起槽；动态成员有补偿 | `RD/ops/mission.rs:718–795`；`RD/ops/mission/roles.rs:325–388`；测试源码`RD/session/manager/tests/mission_lifecycle.rs:476–579` | 团队/成员执行生命周期与局部失败journal，补强Q34 | 旧注释“启动失败全回滚”与当前实现不符；Running/idle/Archive都不等于工作项验收 |
| RUN05 | UI/CLI共享daemon，视图detach不结束PTY，重接带seq/输出/尺寸事实 | `RA/bootstrap.rs:16–40`；`RD/daemon/server.rs:399–412`；测试源码`crates/runner-cli/tests/daemon_process.rs:446–508` | 扩现有host重接，不另建第二daemon，补强Z06/O04 | 活host重接≠host崩溃仍同进程；不承诺iOS常驻CLI，不授权自动拉起/清进程 |
| RUN06 | Terminal/Hook/Skill/Usage能力拆分，状态带来源，hook按session/generation隔离 | `RD/runtimes/mod.rs:159–229`；`RD/session/state/agent/mod.rs:172–201`；`RD/session/hook_queue.rs:116–159`；测试源码同文件`:338–353` | 四族＋custom的能力契约/过时代际拒绝，补强COM01/HER05 | `RC/protocol/runtime.rs:98–150`能力不在线序列化，非完整远端协商；不搬ANSI/title判工具/审批/完成，Pi不等于OMP |
| RUN07 | 恢复记录实际launch定义，fork检查会话键/能力及归档状态 | `RD/session/launch_definition.rs:5–34`；`RD/session/manager/spawn.rs:1870–1952`；`RD/runtimes/codex/mod.rs:225–240`；测试源码`RD/session/manager/tests/fork.rs:310–338` | core保存脱敏有效配置引用，host按真实能力构恢复计划，补强N04/D05 | 来源Codex fork先headless执行，不是纯复制；潜在消耗/副作用不得静默触发，SwiftUI不构argv |
| RUN08 | MCP局部编辑保未知字段/注释，技能按runtime能力与marker管理 | `RD/ops/mcp.rs:350,438,648–655`；`RD/ops/skills.rs:115–166`；测试源码`RD/ops/mcp.rs:885–919` | 与MAG09/CCS07同一个资源部署owner，呈现目标差异/冲突 | 最终truncate＋write非事务；`RA/app_store/skill_defaults.rs:34–61,91–105`默认安装不采用，新技能仍人工同意 |
| RUN09 | 随客户端寿命启停账户配额轮询，限制inflight，失败保上次成功 | `RD/usage.rs:83–122,147–168,308–324`；测试源码同文件`:607–677` | 配额采集时间/来源/新鲜度与当前失败分开，补强MAG04 | 来源有缓存时吞新失败，Aeroric不能伪装新鲜成功；账户窗口不是任务费用或预算强制，采集另受授权 |
| RUN10 | 布局树与session身份分离，IME组合态/UTF-16选区单独处理 | `RA/pane_layout.rs:60–81,112–153`；`RA/terminal_ime.rs:89–126,176–203`；测试源码`crates/runner-app/tests/terminal_ime.rs:4–25,29–71` | 复用现有AppKit/UIKit终端桥，分栏只改视图、尺寸单owner | 不移植GPUI自绘材质/选择态；不因面板重排重启Agent，每屏仍需浏览器审批 |
| RUN11 | 按首次baseline/新interaction ID/失败/未读完成边沿通知，目标区分会话/槽位 | `RA/notifications.rs:21–81,97–149`；测试源码`RA/notifications/tests.rs:21–74,147–194` | 与HER11归并待回应/通知中心，稳定回跳真实对象 | 收到/显示/看过/回合结束/目标验收不同；正文隐私和跨端去重需设计 |

覆盖mission/roster、消息总线/router、启动/hook、配置/用量及上述限定测试。
未覆盖全部执行器/平台、真实账号采集和配置并发；不由这些观察推出
已有长期记忆、依赖DAG、预算强制或完整任务领取租约。

### 7.8 herdr — 终端与远端Host连续性、代际和订阅契约

Rust 0.9.5终端工作空间管理器，server持PTY，Ratatui/Crossterm客户端，
使用ghostty-vt；不是新的推理内核。`Cargo.toml:1–10,23–54`及根LICENSE
声明Apache-2.0，未全面审计vendor、第三方及分发产物。
以下均追到实现，另标测试源码，均未运行；路径相对herdr根。

| ID | 借鉴亮点与机制 | 来源证据 | Aeroric原生完善建议 | 前置/不能照搬 |
|---|---|---|---|---|
| HER01 | server脱离客户端存活，布局与可选历史分开；暂存/sync/replace发布 | `src/server/headless/bootstrap.rs:3–105`；`src/persist/io.rs:49–66`；`src/persist/writer.rs:22–58`；测试源码`tests/detach_reattach.rs:431–518` | 复用现有host/core恢复journal及保留策略，补强RUN05 | 活server重接与新PTY resume不同；多文件非整体事务，终端历史非长期知识或无限保留许可 |
| HER02 | Unix活PTY跨server传FD、restored/ready后commit，提交前失败回滚 | `src/server/headless/lifecycle.rs:25–224`；`src/server/headless/bootstrap.rs:173–205`；测试源码`tests/live_handoff.rs:1096–1138,1959–2027` | 后续host维护候选：明确FD/单写者所有权交接、故障阶段 | 客户端仍断连、仅Unix；PID/时间token非密码学认证，不承诺无缝安全升级，不作iOS本地方案 |
| HER03 | 每SSH endpoint独立重连/取消/generation，method与capability分别协商 | `src/client/endpoint/supervisor.rs:54–103,180–210`；`src/client/endpoint/registry.rs:23–66`；`src/remote/attach.rs:1543–1599` | 沿用Aeroric SSH/SFTP/relay，补命名空间/独立失败域 | 来源远端安装须交互确认，后台不兼容要求人工；保存连接≠新部署/执行授权，不静默安装 |
| HER04 | 每客户端焦点/几何owner，surface revision floor挡旧投影，失活释持键 | `src/server/headless/surface_interest.rs:9–97`；`src/server/headless/client_views.rs:63–80,128–160`；测试源码`src/client/endpoint/activation_tests.rs:502–511` | 多窗口/iPad观察同一执行但不抢尺寸/焦点，补强RUN10/Z06 | surface lease不是任务领取租约、业务写锁或安全沙箱 |
| HER05 | hook状态带来源/seq/session锚；OMP续跑不结算，旧代际拒绝 | `src/app/api/panes.rs:1554–1642`；`src/terminal/state.rs:1526–1595,2074–2098`；`src/integration/assets/omp/herdr-agent-state.ts:439–463` | 复用Aeroric共享OMP hook与task/main-session guard，补强RUN06 | 来源仍有`src/detect/mod.rs:1–38`屏幕/标题启发，不能迁为工具/审批/完成事实；来源标签不是认证 |
| HER06 | resume身份含source/Agent/cwd/参数，限大小/控制字符，去重且不重放ANSI | `src/agent_resume.rs:38–88,199–240`；`src/persist/restore.rs:813–861` | host持typed恢复引用/来源/拒绝理由，补强RUN07 | 来源把报告argv输入shell，不照搬；报告不是执行授权，参数校验非沙箱，不扩大OMP历史根 |
| HER07 | 等待绑定原身份/共同deadline，先见活动再等idle；有界ring明确Lost/Unavailable | `src/api/wait.rs:196–289`；`src/api/event_hub.rs:18–66`；测试源码`src/api/server/subscription_socket_tests.rs:273–303` | 订阅cursor缺口→重读快照，等待/受理/结果不同状态，补强L03/COM01 | 512项内存ring非持久journal；文本命中/idle不是验收，不伪造全CLI统一完成语义 |
| HER08 | 插件安装预览/固定commit，构建后复核manifest，运行保留版本lease | `src/cli/plugin.rs:268–337`；`src/app/api/plugins/runtime.rs:82–158`；测试源码`src/plugin_installations.rs:285–318` | 扩现有extensions的审阅/激活/回退与在用版本保留 | `--yes`可跳确认、启用hook自动执行，普通子进程未见沙箱/显式超时；build有副作用，不能替代技能人工同意 |
| HER09 | 展示元数据按source/seq更新，可删除/TTL过期，不改真实Agent身份/状态 | `src/metadata_tokens.rs:15–65,91–102`；`src/app/api/agent_view.rs:7–88`；测试源码`src/terminal/metadata.rs:615–640` | 原生角色标签、摘要/健康自报带来源与有效期，补强协作态势 | tokens是键值非模型计量；自报非核验，TTL非长期记忆，标签不能改权限或执行器身份 |
| HER10 | worktree打开前重核路径/repo，异步operation去重/陈旧结果拒绝/失败恢复 | `src/app/api/worktrees.rs:69–90`；`src/app/api/worktrees/deferred.rs:315–395,527–564`；`src/worktree.rs:175–185` | 扩现有Git/操作台账，复用版本预检与明确失败收据，补强C06/Z07 | 不授权本轮建分支/worktree；force恢复可能remove_dir_all，不复制自动清理、不删dirty/锁 |
| HER11 | 服务端语义通知，客户端延迟复核/前台抑制，稳定对象定位 | `src/server/headless/notifications.rs:40–108`；`src/client/shell/notification_policy.rs:99–196,231–289`；测试源码`src/client/shell/tests/agents_worktrees_notifications.rs:1531–1616` | 与RUN11同一原生通知业务，待回应直达问题/执行实例 | 系统通知endpoint/boot/pane校验受cfg(windows)限制，不算macOS已交付；Finished非验收、状态可能含启发 |
| HER12 | 配置发布拒多硬链接、私有暂存/权限、提交前重核，保留symlink | `src/integration/config_file.rs:22–79,87–145`；`src/integration/targets.rs:124–151`；测试源码`src/integration/config_file/tests.rs:34–103,204–236` | 补RUN08薄弱写入，归现有配置部署owner，显示逐目标发布/失败 | 非跨文件事务、未证并发内容CAS；symlink仍须核对批准管理目标，不自动写真实homes |

覆盖server/persist、handoff、endpoint/surface、hook/resume、API等待与订阅、
插件、worktree/元数据/通知。未覆盖ghostty/vendor内部、全部平台分支、
真实SSH部署/休眠恢复、恶意插件或商店sandbox兼容性，不能泛化各Agent能力。

## 8. 全覆盖后的结论：借能力机制，不堆软件模块

### 8.1 本轮新增价值

新增观察计数：PT10、K13、MAG14、CCS10、COM8、RUN11、HER12，共78条。
首轮89条＋本轮78条＝167条仓库来源观察，U01另计。
用户提供的MiMo桌面包补充研究仍单独回指§5及
`docs/research/xiaomi-mimo-self-evolution.md`，不冒充第18个仓库调查。
以下是**综合建议**，不是来源项目自称或Aeroric现有交付：

- **ponytail补质量闭环**：最小完整改动、追调用者的审查、技术债回访、
  验证过时和对照评测。最值得带走的是“依据仍适用吗”，不是精简代码KPI。
- **kloop补任务连续性**：用户原话锚、预算化上下文、能力代际、结果未知、
  恢复journal。不要把压缩/缓存技巧误做外部CLI都能控制的功能。
- **magpie补连接与资源控制面**：能力目录、路由解释、配额、部署归属、
  外发预检。沿用Aeroric已有local_router，不再造一个全权网关。
- **cc-switch补配置工程**：字段投影、应用计划、外改保护、部分成功、
  崩溃恢复。把“所选”和“实际生效”分开，比增加更多配置按钮更重要。
- **CometixCode补控制边界**：结构化请求关联、消息队列、压缩提交点、
  保守并发。只借契约，不把复刻内核或提示权限当宿主强制。
- **runner补协作业务**：角色/团队配方、持久收件箱、人工问题、
  局部失败与运行连续性。是对paperclip协作方向的补强，不再造公司模型。
- **herdr补原生工作台的可靠底座**：多host重连、代际/光标缺口、
  多窗口焦点/尺寸所有权、精确恢复。热交接属高风险后续候选，不优先承诺。

### 8.2 归并规则

1. **按用户目的和业务owner归并，不按来源名字分栏。** 同一能力可吸收多个
   来源的不同机制；每条来源ID仍保留，以便追证据和明确未采用的风险。
2. **配方、叠加能力、执行配置和工作面分层。** “系统化调试”是配方，
   “多Agent”是协作能力，“Claude/Codex/OMP/DSH”是执行器，
   “API/模型/推理强度”是角色配置，“仓库/画布”是工作面，不能同级单选。
3. **一个真源、多处投影。** 首页摘要、任务详情、工具库和统计页可以同时
   展示同一对象，但不各存一份任务、权限、用量或MCP状态。
4. **能力可组合，不等于开关自由相加。** 有依赖/不支持/不兼容就明确解释；
   保存选择不是部署/启动/授权，Full Access也不替代技能人工同意。
5. **归并不删用户要求。** 科研、Office、头脑风暴、技能编写、调试等仍可
   分别形成有输入/产物/检查的配方，不因底层共用owner就只剩几种入口。
6. **已有确认优先。** 下文新组合与命名均待讨论，不能反向覆盖Q13–Q113，
   也不把全库研究、旧样例通过或界面“总体可以”算作引擎实施授权。

### 8.3 相似但绝不能混成一个状态

| 看起来相似 | 必须保留的差别 |
|---|---|
| 工作配方 / 环境Profile / 原生-高级显示模式 | 分别回答做什么、绑定哪些执行资源、如何呈现；显示切换不重启Agent、不改模型/权限 |
| 独立审查 / 阶段反思 / 任务自进化 | 审查可单次使用；启用U01后按Q20–Q25运行阶段反思/及时治理；自进化还须有任务专属可执行代码候选、评测和受控试用 |
| 普通协作者 / 反思者 / 治理者 | 普通成员按Q31继承有限工作包授权；反思/治理按Q16只读且模型独立，不能套一条“团队继承”规则 |
| 上下文包 / 会话历史 / 项目知识 / 可执行技能 | 任务内输入、事实原记录、获准复用经验和可执行资产分别管理；摘要不升级成指令/权限 |
| 消息已存 / 已投递 / 模型已消费 / 已采纳 / 已整改 | 各有证据来源，无接口则未知；输入框清空、PTY字节送出或ACK不能串推后续状态 |
| Provider配额 / 用量估算 / 官方账单 / Aeroric资源上限 | 供应商窗口、观测值、对账和准入策略不同；未知不补零，估算不能冒充硬费用封顶 |
| 网络重试 / 路由接替 / 任务重跑 / 规则补跑 | 各自授权、幂等域和副作用不同；不能以请求失败证明未执行或自动跨角色改绑 |
| 重新附着 / resume / fork / 文件rewind / 撤销外部副作用 | 活进程重接、启动新执行、另建会话、恢复文件和外部撤销分开；必须有各自计划与收据 |
| Agent回合结束 / 进程退出 / 技术完成 / 用户验收 | 按结构化来源、退出码、适用检查与用户决定分别记；Finished通知不等于验收 |
| 文件锁 / 任务领取租约 / 窗口surface owner / 数据根单写者 | 解决不同竞争；任何一个都不能证明其他层已安全，也都不是OS沙箱 |

## 9. Aeroric原生能力族与实现路径（待讨论）

### 9.1 全库主归属：20个能力族，不是20个侧栏按钮

每条来源观察在下表有一个**主归属**，跨域依赖可再引用但不重复计数。
A01–A20仅为本轮归并索引，不是新增来源观察、已批准需求或任务ID。
表中“补强”指新增观察给原方向增加机制，旧ID的已确认/待决状态不变。

| 能力族 | Aeroric应交付的原生能力 | 主归属来源ID |
|---|---|---|
| A01 工作配方库 | 按目的搜索/收藏；配方定义输入、阶段建议、产物、依赖及检查；轻问答明确不可操作项目；专家面板有界 | `C01 C02 C03 X01 F04 PT06` |
| A02 主计划与工作包 | 一个可修订主计划，步骤/依赖/负责人/验收引用；只对齐受影响工作，不统一加开工签字 | `D01 D02 F01 F02 Z03 O01 M04 N01 K01 K02` |
| A03 任务团队与委派 | 任务按需组队，角色模板/实例快照、领取租约、授权子集、并行/局部失败/交接 | `P01 P02 P03 L02 M06 RUN01 RUN04 COM07` |
| A04 反馈与人工问题 | steer/下轮队列/同伴结果分型，定向持久收件箱、问题因果、投递及消费状态 | `C08 X04 N02 O02 O06 Z04 K03 COM04 RUN02 RUN03` |
| A05 质量与交付证据 | 复用优先、审查/体检/债务配方、版本化Finding、命令/测试/产物证据、技术完成与用户验收 | `D07 M05 M07 M10 PT01 PT02 PT03 PT04 PT07 PT08` |
| A06 任务进化 | 以已确认U01为根：任务专属工具代码候选、对照评测、阶段反思、独立治理进程、受控试用/停用 | `U01 M02 M08 PT10` |
| A07 任务上下文与历史 | 原话/决定锚、预算化资料包、代码能力地图、作用域历史检索、附件/压缩投影、来源及遗漏 | `F05 F08 D06 L05 N06 M03 PT05 K04 K05 K06 MAG08 CCS09 COM05` |
| A08 项目知识与流程草稿 | 从任务提候选，集中审核、项目内按需引用、冲突/过期/撤销；流程草稿与可执行技能分开 | `M01 F09 L06` |
| A09 工具与扩展资源 | 技能/MCP/Hook/插件统一目录，发现/安装/启用/部署/调用分开，按执行器显示能力和版本 | `C07 D08 F10 X08 N07 Z08 Z09 K08 K09 MAG09 MAG10 CCS07 RUN08 HER08` |
| A10 角色配置与环境方案 | 各角色独立执行器/API/账号/模型/强度绑定；版本化方案、字段归属、漂移及部分应用可见 | `K07 MAG01 MAG02 CCS01 CCS02 CCS03 CCS06 HER12` |
| A11 连接弹性与协议适配 | 明确兼容矩阵、用户预设fallback、熔断/亲和、请求尝试轨迹；不影响角色授权 | `MAG03 MAG05 MAG06 CCS04 CCS05` |
| A12 工作安排与typed流程 | 待办/执行/自动化分开；明确触发/启用/跳过/补跑；typed节点、后台job和产物收据 | `C04 C05 P10 L07 F03 Z01 Z02 D03 D04 K11 MAG12 COM08` |
| A13 统计与资源护栏 | 请求/尝试/任务/角色用量归属，金额精度/新鲜度/缺口；配额窗口、预算和并发准入分别实现 | `P05 L08 F07 F06 PT09 K13 MAG04 MAG07 CCS08 RUN09` |
| A14 连续执行与恢复 | 重接、resume/fork、停止/未知结果核实、局部恢复、有效启动快照；热交接保留后续研究 | `P06 L01 X05 X06 N03 N04 D05 O04 K12 COM06 RUN05 RUN07 HER01 HER02 HER06` |
| A15 项目检查与安全变更 | 本地Git/远端Issue/PR、差异/批注、文件版本预检/撤销收据、获准隔离副本及迁移 | `C06 C10 M09 N05 N08 Z07 K10 HER10` |
| A16 原生工作台与事实同步 | 会话/文件/笔记/终端可视关联，原生分栏与IME；typed能力/控制ID/代际/cursor、窗口焦点与尺寸 | `C09 O05 Z05 Z06 L03 RUN10 HER04 HER07 RUN06 HER05 COM01` |
| A17 权限、审计与外发 | 有效授权/审批来源/身份/范围/期限/撤销、受保护规则、实际隔离报告、外发预检/脱敏 | `P04 P07 P08 P09 X02 X03 X07 COM02 COM03 MAG11 MAG13` |
| A18 远端Host与共享目标 | 多host能力/身份、SSH/SFTP/relay重接、设备配对、目标共享与观察/操作权限分开 | `L04 O03 O07 O08 HER03` |
| A19 待回应与态势通知 | 基于原生业务事件的未读/问题/失败提醒、稳定回跳、自报标签来源/TTL、跨端去重 | `RUN11 HER09 HER11` |
| A20 配置迁移与备份恢复 | 可预览/过滤的版本化迁移包，暂存校验、敏感项单独选择、冲突保留和恢复收据 | `MAG14 CCS10` |

A08保留独立知识owner；A09承接获准发布后的技能，不因归并偷渡执行资产。
A13内也保留“计量、供应商配额、资源准入”三个子契约，不共用一个余额字段。
A16收纳同步/能力是横向依赖，不表示布局模型有权修改执行状态。
用户原先要求的Docker、SSH、SFTP、存储、数据库、随手记和使用统计继续保留：
分别接既有基础设施/连接/笔记/统计owner，在工具与资源库分类、按需引用；
不是因为它们未出现在本次78条观察中就取消，也不重复包装成新工作方法。

### 9.2 不另造系统：复用位置、候选对象与必须补的闭环

以下路径为本轮核对的**已有基础**；候选对象名只是说明所需增量，
不是声称该类型/接口已经存在，也不授权修改这些文件。
源码依据和当前缺口详见 `docs/research/harness-backlog-2026-10-20.md` §4。

| 覆盖族 | 现有owner/边界 | 建议的最小增量与失败处理 |
|---|---|---|
| A01/A02/A12 | `core/crates/aeroric-core/src/tasks/`的service/operations；protocol task | 候选RecipeRevision、PlanRevision、WorkPackage、AutomationRule/Run引用既有task执行；expected_revision挡外改、operation key去重；不再造平行任务库 |
| A03/A04/A19 | tasks生命周期＋既有`adapters/work_graph.rs`外部只读边界、events/host订阅 | 新增原生协作关系/消息/问题持久聚合；成员/投递有generation，普通问答与审批不同对象；成员失败局部恢复，不从PTY输入清空补消费ACK |
| A05/A15 | `git/`、`files/`、sessions导出＋tasks | EvidenceRef/Finding/Deliverable绑定内容版本、检查环境/退出事实；代码改变使受影响证据过时；外改先重核、不盲套补丁，多文件逐项结果而非笼统Ok |
| A06 | 现有tasks/executor端口、extensions信任＋A02/A05/A17 | 原生EvolutionCandidate、EvaluationRun、ReviewRecord/角色绑定和激活记录；治理独立进程；保护权限/审批/验收面，缺必要评审不启用，不修改四CLI安装本体 |
| A07/A08 | `sessions/`、`notebook/rag/context.rs`和knowledge设计快照 | ContextBundle区分原话/事实/摘要/候选、来源/截断/范围；知识引用不成为高优先指令；不修改CLI原始历史或扫描外部OMP home |
| A09/A10 | `settings/`、`extensions/`、`agents/`、`CoreContext::SecretStore` | RoleBinding、ResourceRevision、DeploymentPlan/Receipt；凭据只用受保护引用；目标预检/字段diff/hash、外改冲突、逐项发布与恢复，技能同意/版本切换独立记录 |
| A11 | **已有**`local_router/`及circuit_breaker/usage | 扩兼容矩阵、RouteDecision/Attempt与失败原因，不再造第二网关；当前RouterAgent只列Claude/Codex，OMP/DSH须另证能力，不宣称四族已同路由 |
| A13 | **已有**`metrics/`、local_router/usage | 补任务/角色/运行/请求关联和去重、价格/采集来源；BudgetAdmission与UsageObservation分开；先核对历史发现根，不能整接现有外部OMP扫描；未知/失价/过期明示，必需硬限制不能落实就受阻 |
| A14/A16 | **已有**`aeroric-host`、runtime/terminal、capability/session_controls、protocol/原生终端桥 | 不另建daemon；ExecutionRef含host/boot/generation，ResumePlan与ViewAttachment分开；丢cursor重读快照，取消等待真实结果，主机死后不声称原PTY续活 |
| A17/A18 | host鉴权/共享目标、remote/connections、显式adapter拒绝及core单写者 | 统一有效授权与来源，目标共享≠执行授权；凭据/路由/日志外发按范围预检；要求隔离时失败关闭，不把“无VM”解释为默认无约束全权shell |
| A20 | `storage/atomic.rs`、`storage/backup.rs`及settings配置包 | 迁移manifest、逐项diff、secret默认排除/单独同意、解密/验证/暂存后发布收据；未知版本/冲突不覆盖；加密不等于可传播技能或Full Access |

`providers/`当前主要是存储服务OAuth，不能只因名字像模型Provider就塞入
模型账号域。应沿settings/agents/local_router厘清现有配置owner后加增量。
核心聚合负责业务真源，host负责能力核验及执行；SwiftUI只收read model/
typed动作，不构argv、不直接写共享业务，也不解析原始JSONL。
具体反例：`metrics/analytics.rs:1084–1116`目前通用根发现仍会枚举`.omp`；
“模块已上收”不证明原生调用满足managed-root要求。未来统计接线必须先
审清调用可达性/限定来源，本轮没有运行扫描或顺手改代码，也不删Tauri例外。

### 9.3 一条可实现的纵向链，而不是先做二十套页面

以下为可行性拆分，**不是执行清单或开工顺序授权**：

1. **先把来源、状态和配置说真。** 梳理现有配置ID/secret引用/能力广告，
   给草稿、资源目录、有效配置和未知状态建立可复用read model；先用离线
   fixture呈现“可用/不支持/缺依赖/漂移/部分成功”，不以mock按钮声称闭环。
2. **打通最小任务链。** 版本化配方引用既有tasks，主计划/工作包→执行
   实例→成果/证据；复用操作去重，先做单执行者，不阻断普通输入直接发送。
3. **在同一链上加协作与沟通。** 角色实例/领取→持久收件箱/人工问题→
   定向反馈/局部失败→受影响工作重核。先明确真实适配器能力，再决定
   哪种反馈可即时、哪种只能排队/人工交接；不能用三套邮箱各存一份状态。
4. **把监督和可执行候选接到证据链。** 保留Q20–Q25阶段反思/异步治理，
   用同一EvidenceRef与当前版本审查；任务专属工具候选另有评测/门禁/
   安全点试用。质量配方不是简化成三模型互投“通过”。
5. **在有真实归属后完善知识、安排和护栏。** 整理获准知识候选、规则引用
   配方和角色方案、计量/配额/预算及未知副作用恢复，避免自动化先行而
   既无证据也无可落实边界。实际准入护栏须在对应执行上线前满足，不能后补。
6. **连接高级能力独立验证。** 扩协议转换/路由策略/迁移包、多host和
   多端视图；Unix活PTY热交接、自动意图路由等高风险项单独设计，
   不是为了凑齐候选库就作为首版默认。保留它们，不等于都要现在实现。

全程不建第五编码内核，不将外部Agent的goal/workflow改名抢为Aeroric所有。
新数据结构先定义owner/版本/授权/恢复边界；完整实施包再逐项列协议往返、
非法状态、重复请求、陈旧事件、缺权限/技能同意、预算耗尽、外改/部分失败、
取消后未知、断线与敏感日志的验证门槛。不得默认“unit”目标无窗口；
具体检查仍须审查为非窗口、隔离数据和fake agent后才能运行。

### 9.4 四执行器的可行性：统一业务，不伪造相同运行能力

| 实现层级 | 可以怎样做成Aeroric原生 | 没有真实支持时怎么办 |
|---|---|---|
| Aeroric自有业务 | 配方/主计划/工作包/问题/知识候选/成果证据由core拥有，四族及custom均可作为执行者引用；不依赖解析其内部TODO | 记录宿主事实和原始来源；Agent自报单独标注，不把内部CLI计划抢作原生状态 |
| 启动与配置适配 | 沿既有host families/executor分别落实目录、managed home、模型/强度/权限、恢复引用 | 不支持的值具名受阻，不改成别的模型/权限；OMP保持真实PTY、flags在`--`前、prompt在后，清profile覆盖，不恢复RPC UI |
| 运行中控制 | steer/结构化工具/审批/compact/fork/会话恢复逐能力、版本、方法广告，host独立拒绝未支持请求 | 可提供原生排队/人工交接等明确不同语义的入口；必需能力缺失则阻断依赖，不以ANSI/屏幕猜出“已消费/已完成” |
| 监督与任务进化 | 主执行可继续由四族承担；独立监督绑定API/模型、获准只读证据，治理独立进程；进化候选仅在任务专属工具/执行逻辑副本 | 不是另写Aeroric编码推理loop；无法落实必需只读、候选验证或安全切换就不启用，不以只读prompt或进程分离冒充隔离 |
| 多端和远程执行 | macOS/iOS/iPadOS以SwiftUI读同一host的业务事实和能力，远端沿SSH/SFTP/自托管relay | iOS不承诺本地运行CLI或常驻host；断线/丢史/主机更换明确显示，不把活PTY重接和新进程resume混同 |

无VM是固定要求，不是取消文件、网络、凭据和系统能力边界。
隔离/Store sandbox及目标平台限制须在具体实施设计中证明；
不引入Docker或worktree充当自动替代VM，也不把已有Docker管理入口当作
默认允许创建执行环境。部分功能可分阶段交付，界面必须如实显示支持范围。

## 10. 新建会话该如何容纳这些能力（待讨论，不是视觉定稿）

推荐保留用户已认可的中央输入/发送骨架，改变**功能组织方式**，
而不是把所有能力塞进“更多配置”或堆成互斥大按钮：

### 10.1 工作方法升级为可扩展的工作配方库

主输入保持“告诉Aeroric你要做什么”，不强制先选配方。
常用少量推荐＋搜索/全部配方入口，库内可按目的分组并显示依赖/预期产物：

- **软件与项目**：构建功能、系统化调试、变更审查、项目体检、技术债梳理。
- **构思与研究**：头脑风暴、科学头脑风暴、资料研究、假设生成、实验设计。
- **文档与成果**：Word文档、Excel工作簿、演示文稿、报告/交付整理。
- **能力与复用**：编写专家技能、流程草稿、知识整理；生成不等于发布安装。
- **轻量问答**：可明确不赋予项目操作能力，不因提到文件就隐式升成执行任务。

这些是配方候选，不假称已接原生Office编辑器或所有科学工具；
每个配方需补齐输入/阶段/产物/检查及实际依赖，再单独审阅。
“普通编码”可以不选；多Agent、自进化、Full Access不作为其互斥替代项。
配方可提出能力建议，但不能因为点了配方自动安装技能、扩权限、改绑定或启动。

### 10.2 可叠加能力：告诉用户如何协助，不强制全部开启

围绕已确认规则，候选可有“团队协作、阶段核验/独立审查、任务工具进化、
项目知识、所选技能/MCP”等摘要，点开才看细节和角色配置。
这里的“叠加”不是重新把Q20阶段反思或必要验证变成随意可关闭：
**启用U01后按既有监督规则运行；其他有效要求和宿主硬限也不受配方影响。**
阶段核验/独立审查的具体开关、默认值和启停影响须随后逐项讨论。
画布、仓库、数据库、终端、随手记等是任务中工作面/可引用资源，
不要求用户每新建一条会话都先勾选。

### 10.3 执行配置：角色、权限、资源各自独立

- 输入区保持主Agent配置、模型、推理强度和有效权限可见；
  Full Access及关键启用能力摘要不埋进折叠层。
- 协作/监督启用后，分角色绑定执行器/API/账号/模型；改主配置不联动监督。
  全局模板与本任务覆盖区分，缺项/不支持/不可用就地说明，不静默fallback。
- 环境方案可引用配置/资源版本，但不能偷带一次性授权、凭据或技能安装许可；
  来源Profile缺项也不能在UI显示为全就绪。
- 高级配置按“上下文、工具资源、执行环境、资源护栏、路由策略”等具名分组，
  仅展示与当前目标/能力有关内容；不适用项有原因，不假装所有CLI都支持。
- 权限、连接、工具可用性和预算是不同维度；新技能依旧须人工同意。
  Full Access免覆盖内反复权限询问，但不取消监督、资源边界或用户Stop。

### 10.4 任务开始后：能力有自己的工作面，不滞留在创建表单

任务工作区显示计划/步骤、协作者/问题、成果/验证、上下文/知识和事件，
资源/连接/统计/项目检查复用既有页，互相用对象引用跳转。
首页仍是项目、待回应、工作安排、常用入口及摘要，不搬回“发起工作”大展板。
“固定入口移中区”的已确认方向不变；上述归并不另批准新侧栏分组。

例：用户要做一个软件，可选“构建功能”配方，以Codex主执行；
按已确定规则配置协作和U01三角色，反思/治理可分别选择不同API/模型。
点击发送即开始、原地进入会话；计划、待回应、证据和进化候选在任务中
按需出现。不是先建空任务再二次发送，也不是把所有设置填完才准输入。
若必需依赖/授权/可落实边界不满足，应保留草稿并明确受阻，
不后台安装或悄悄关闭用户要求后开跑。

### 10.5 草稿与发送链不被新配方破坏

项目级未发送草稿继续保留提示词/附件、模型/配置/权限及已选能力，
切页后返回同项目仍恢复；项目间隔离，不拿当前全局默认覆盖已选值。
草稿引用的配方/配置/资源若变化或失效，应显示原选择与当前差异，
不静默升级、迁移或启动。主动切主配置仍依Q111采用目标明示默认，
监督不联动；单纯恢复草稿依Q112不重新套默认。

可行的候选数据流：
**项目草稿 → 解析配方/角色/资源版本 → 准入结果与具名问题 →
既有typed TaskIntent/操作去重 → core持久化 → host执行及真实回执**。
角色/资源的有效快照不包含明文secret；未得到确定发送成功回执前，
状态不明先核对，不自动重发。成功后按Q113清内容/附件、保可见配置偏好，
不复制单次授权；这不自行决定“成功”的新协议语义。

关窗/重启/跨窗持久化、跨设备同步、改项目时的草稿迁移与冲突仍未决定；
本轮不因来源有磁盘持久化就擅自替用户拍板。

## 11. 下一轮grill-me：先定组织原则，再逐项收口

**Q114（尚待回答）**：是否按§10推荐，把“工作方法”做成可扩展、
可搜索的配方库，把协作/审查/任务进化等做成可组合能力，
执行器/各角色API模型、权限与资源配置独立，过程功能进入任务工作面？
推荐采用：避免只增加几个预设按钮后再次遗漏功能，也避免把78条观察
都塞成新建会话的高级开关。用户对默认值、命名和具体位置仍可继续调整。
这条原则若确认，也不等于每项候选采纳、版式批准或实施授权。

依赖Q114的后续讨论分别处理：首批配方与搜索分类；能力组合/默认和
禁用影响；角色模板/环境方案的版本与失配；已有草稿迁移/冲突；
依赖缺失时的交互；任务内各工作面与验证状态。不得在Q114未答时
把下游方案预记“已确认”，也不把未决事实交给用户代查。

阶段MD仍按“来源ID→已确认决定→边界→独立实施/非窗口验证门槛”输出给
新终端。本轮仅更新候选库与研究证据，未修改浏览器稿、SwiftUI、Rust、
测试政策、任务元数据或旧执行包；没有运行来源测试、编译或真实模型。
真实App/Preview/窗口UI检查 **NOT RUN / DEFERRED**，不创建/准备/启动VM。

## 12. Q114已确认：从功能归类进入原生业务归并（2026-10-20）

用户回复：

> 确认，你要深度思考怎么合并相似的功能，怎么在Aeroric中原生实现

随后要求“继续”。据此确认§10的四层组织，不重开Q36共同主计划、
Q51待办/执行/规则分层等已决定事项。**确认不代表168项全部采纳、20族
分别实施、新建会话整屏批准、能力默认开启或任何真实引擎开工。**

本轮深化不再新增来源观察，改为区分三种归并：

1. **同一业务对象**：任务主计划及其工作项/工作包/分工关系，各页面共用；
   不让计划、团队看板、阶段监督分别维护互相冲突的完成状态。
2. **共用小机制、保留独立契约**：消息传递/来源引用/版本/操作回执可以复用；
   问题、权限审批、审查、验收和通知仍各有业务状态与授权处理。
3. **只统一入口或投影**：工具库汇集技能/MCP及Docker/SSH/SFTP/数据库/
   存储/随手记；既有资源、连接、笔记和执行owner不被目录接管。

深化文档 `capability-integration-design-checkpoint-01.md` v0.1包含：
八组用户工作闭环、任务与运行身份的兼容边界、配方/流程/技能的区别、
版本化证据与局部阻断、配置部署及凭据缺口、typed数据流、失败恢复、
原生呈现与分段验证建议。八组是解释产品关系，不是八套新服务/页面。
原167条仓库观察＋U01及20族映射全部保留，不用新分类抹去来源与风险。

**Q115（待用户回答）**：选择/更换工作配方时，是否只应用清楚可见的工作
方法、建议步骤与产物建议，并把额外协作、独立审查、任务进化或工具依赖
列为“必需/推荐/可选”，而不静默改变已有模型/API、权限或能力选择？
推荐如此：已明确启用的能力保留；选择配方本身不新增执行授权、安装技能
或启动角色。若用户确实选择的配方缺必需条件，保留草稿、具名说明，
不能悄悄关闭条件后发送；推荐/可选项未启用不应阻断普通任务。
“必需”须有可审阅依据，不能由模型借配方修改已确认验收标准。
这问不设定初始默认或增加每次发送前的第二道确认页。

只追加设计候选与决定记录，不改冻结快照/执行包、产品代码、浏览器V1、
测试政策、任务metadata/context、忽略规则或发布配置。不启动任何真实
Agent/API、App/Preview、浏览器/窗口自动化或VM，不提交/推送。
本轮文档静态核对结果见深化文档§12；运行/UI验收仍NOT RUN / DEFERRED。

## 13. Q115配方应用范围已确认，Q116组合冲突待回答（2026-10-20）

用户回复“确认”，确认§12的Q115：
配方带入可见方法、步骤和产物建议，额外能力列必需/推荐/可选；
保留提示词、附件、归属及明确选择的能力、模型/API、权限，不静默修改。
缺有依据的必需条件时保留草稿、具名受阻；未启用推荐/可选项不强迫开启。
选择配方不启动角色、不安装技能、不新增执行授权，发送才交办，
不增加发送前第二确认页。其他默认、机制、视觉和产品实施并未获批。

**Q116（待回答）**：用户已明确选择的能力若与配方或执行方案不兼容，
是否保留选择和草稿、解释具体影响，由用户明确调整后再按新要求发送，
而不自动关闭能力或用弱替代冒充？例如主Agent自检不能冒充独立审查。
配方标“可选”不能抹去用户的明确选择；未选择的推荐项仍不阻断普通任务。
涉及其他已选功能的必需条件时，仍须守住关联门槛，不能自行绕过。
这是发送前组合规则的提案，不决定运行中启停、恢复、默认或技术协议。

完整确认范围与提案见
`docs/native/capability-integration-design-checkpoint-01.md` §13；
既有167条观察＋U01及20族/八闭环关系未增删。保留§12历史提问和检查结果，
本轮新存基线，只做文档静态核对；未改产品、浏览器稿、政策或冻结包，
未运行App/UI/模型/VM，真实UI验收仍NOT RUN / DEFERRED。

## 14. Q116已确认，Q117工作方案复用待回答（2026-10-20）

用户回复“认可”，确认Q116：已选能力不能静默关闭/降级或弱替代冒充；
发送前组合不兼容时保留草稿与选择，说明具体冲突和调整建议，由用户
明确调整后再发送。不静默改执行器/模型/API/权限，不新增发送确认页；
未选择的推荐项不阻断，关联必需监督/验收条件仍须满足。
此确认不决定运行中失效、恢复、默认、技术协议或视觉。

**Q117（待回答）**：建议在四层内用工作方案保存常用组合引用：
配方、已选能力、各角色独立执行配置及常用工具/资源，可由用户明确
另存为项目或全局可见。衔接Q14/Q84，不另造配置/资源真源；
权限独立，不随方案自动提权。应用时原位显示配置变化、明确应用，
保留当前提示词/附件/归属；缺项与冲突遵Q115–Q116，发送才交办。
不保存本次任务内容/路径、运行记录、凭据或单次授权，不安装或启动。
更新/版本、分享、自动套用和运行中应用另议；尚未批准。

完整范围见 `docs/native/capability-integration-design-checkpoint-01.md` §14。
仅文档续记与静态核对，不新增来源ID，不改产品/浏览器稿/政策或冻结包，
不运行真实Agent/API/App/UI/VM，真实UI验收仍NOT RUN / DEFERRED。

## 15. Q117已确认，Q118工作方案更新关系待回答（2026-10-20）

用户回复“认可”，确认Q117：常用配方/能力、各角色独立执行配置/模型/
强度及工具/资源引用可保存为项目专用或全局可见工作方案，不自动套用/
启动。应用前原位显示配置变化、明确应用，保留提示词/附件/归属，
权限独立、不随方案提权；不复制凭据、单次授权或上一次任务内容，
缺项/冲突遵Q115–Q116，发送才交办，不安装技能。
版本、分享、初始默认、运行中应用及视觉等不随之获批。

**Q118（待回答）**：建议方案修改形成新版本，旧草稿保留已应用的配置，
方案及其引用定义更新只提示差异，由用户主动应用，不静默覆盖手选。
运行任务记录实际生效绑定，不因模板更新改绑/重启。通过已有配置引用及
有效非敏感取值追溯，不复制secret或另造配置真源；远端不可锁版如实说明。
旧版仍可用且符合当前条件时可继续；撤权/Stop/安全禁用/宿主硬限仍优先，
失效保留选择、具名提示，不以快照越权或暗换新版，不新增发送确认页。
这不是运行中主动改绑、持久化/同步、分享或版本保留期限的定案。

完整范围见 `docs/native/capability-integration-design-checkpoint-01.md` §15。
仅文档续记/静态核对，不新增来源观察，不改产品/浏览器稿/政策/冻结包；
未运行构建/产品测试/真实Agent/API/浏览器/App/UI/VM，真实UI仍NOT RUN / DEFERRED。

## 16. Q118已确认，Q119首次普通会话默认待回答（2026-10-20）

用户认可Q118：更新形成新版本，既有草稿保留原配置、提示更新，明确
查看差异并应用才改变；不覆盖其他手选，不自动改绑/重启运行任务。
旧版当前有效可继续，但不能绕过撤权、安全禁用或凭据失效等现时限制。
这不批准具体版本存储/协议、运行中主动改绑、持久化/同步或视觉。

**Q119（待回答）**：无草稿/明确偏好且未主动应用方案的首次普通会话，
建议只启用主Agent，团队协作/独立审查/任务工具进化由用户按需叠加。
不取消普通任务的既定验证；启用U01后必需反思/治理不可单独关闭。
角色已有配置不等于已启用；恢复草稿/主动应用方案不重套首次默认。
本问不决定后续草稿的配方/能力偏好继承或具体Agent/模型/权限默认。

确认范围及提案见 `capability-integration-design-checkpoint-01.md` §16。
仅四文档静态核对，不增删来源观察，不改产品/浏览器稿/政策或冻结包，
未运行真实Agent/API/App/UI/VM，真实UI验收仍NOT RUN / DEFERRED。

## 17. Q119已确认，Q120同项目后续能力偏好待回答（2026-10-20）

用户认可Q119：无草稿/明确偏好且未主动应用方案时，首次只启用主Agent，
协作/独立审查/任务工具进化按需开启；普通验证不免除，U01必需监督
不可关闭，恢复草稿/应用方案不重套默认。具体模型/权限/视觉未获批。

**Q120（待回答）**：建议同项目成功发送后，下一份新草稿保留当次配方/
协作/独立审查/任务工具进化选择作为可见可改偏好；不串项目、不覆盖
既有草稿、不自动重套方案或升级。不复制旧成员/会话/工作包/监督结果、
任务专属进化工具或授权，内容/附件仍清空；发送才交办并重核条件。
不决定资源自动继承、全局入口偏好、重启/跨端保存。

见 `capability-integration-design-checkpoint-01.md` §17；仅文档静态核对，
未改产品/浏览器稿，未运行真实Agent/API/App/UI/VM，UI仍NOT RUN / DEFERRED。

## 18. Q120已确认，Q121起步配方与阶段方法待回答（2026-10-20）

用户认可同项目下次新建沿用成功发送时的配方及协作/独立审查/进化选项，
作为可见可改偏好；清空提示词/附件，不复制旧运行/结论/任务专属工具/
授权，既有草稿优先、项目隔离，不重套/升级方案，重核当前可用性与授权。
工具/资源及角色覆盖的跨任务继承、全局入口偏好、跨窗口/设备保存仍未定。

**Q121（待回答）**：建议新建不选或选一个起步配方，复杂任务不同阶段
在同一主计划引用不同方法，不简单拼接多个配方指令，也不强制换任务/
会话/模型。普通任务不强制复杂流程，范围内选方法不新增逐阶段开工签字；
方法关联不安装技能、扩权或接管外部workflow，不授权通用流程引擎。
例如研究→假设→实验→报告，均保留同一任务的产物/证据关联。

见 `capability-integration-design-checkpoint-01.md` §18；仅文档静态核对，
不增删来源观察，不改产品/浏览器稿，未运行真实Agent/API/App/UI/VM。

## 19. Q121已确认，Q122首批配方目录待回答（2026-10-21）

用户认可Q121：新建可不选或选一个起步配方，复杂任务在同一主计划
分阶段使用不同方法，产物/证据归同任务，不简单拼指令或强制换任务/
会话/模型；简单任务不强制流程，目标/授权内不加阶段开工确认。
重复认可不扩及未提出事项；具体清单、协议及视觉仍未获批。

**Q122（待回答）**：沿§10.1建议五组十六项可扩展目录：
软件与项目4项（开发与改进、系统化调试、变更审查、项目体检）；
构思与研究5项（通用/科学头脑风暴、资料研究、假设生成、实验设计）；
文档与成果3项（文档与报告、表格与数据、演示文稿）；
能力与沉淀3项（编写专家技能、工作流程设计、知识整理）；轻量问答1项。
技术债梳理并入体检子目标/别名，Word/报告合并入口但保留格式要求；
每项须有输入/方法/产物/检查及真实依赖说明，不是新增空按钮。
协作/进化仍是能力，工具资源保留owner；不承诺Office编辑器、所有真实
文件格式、十六项已就绪、首发必交或十六套独立实现。

详见 `capability-integration-design-checkpoint-01.md` §19；不增删观察ID，
只做文档静态核对，未改产品/浏览器稿，未运行真实Agent/API/App/UI/VM。

## 20. Q122已确认，Q123用户工作配方待回答（2026-10-21）

用户认可§19五组十六项目录：软件与项目4、构思与研究5、文档与成果3、
能力与沉淀3、轻量问答1；完整确认清单见检查点§20.1。
技术债归项目体检子目标/搜索入口，Word/报告共用入口并保留格式要求，
普通/科学头脑风暴共用机制且保留科研证据要求；组合能力不混成配方。
每项有输入/方法/产物/检查及真实依赖，不是空按钮；不承诺全部就绪、
首发必交、十六套引擎或Office编辑器，内容草稿不冒充真实Office文件。

**Q123（待回答）**：建议内置配方可另存为项目专用或全局可见的用户方法，
保留原版/来源/版本，用户可改目标、输入、步骤、产物、检查建议和依赖。
如“项目体检”另存为“发布前体检”；修改不静默回写旧草稿/运行任务。
配方管方法，Q117工作方案管配方/能力/各角色执行配置及资源引用；
保存不等于安装、执行、启用角色、改模型或授予权限，也不改既定验收。
模型可提草案，不自行采用为已认可方法或新标准；具体编辑器/分享/协议另议。

见 `capability-integration-design-checkpoint-01.md` §20；仅四文档续记/静态核对，
不增删来源观察，不改产品/浏览器V1，未运行真实Agent/API/浏览器/App/UI/VM。
每屏仍须浏览器稿及用户视觉审批，真实UI验收仍NOT RUN / DEFERRED。

## 21. Q123已确认，新建会话V2等待逐屏视觉审阅（2026-10-21）

用户回复“允许”，确认内置配方可另存为项目专用或全局可见的用户方法，
保留原版/来源/版本，可编辑目标/输入/步骤/产物/检查建议及依赖。
保存不自动应用，不改变旧草稿/运行任务或既定验收；工作方案仍管组合，
不因配方保存安装技能、执行流程、启用角色、改模型/API或扩权。
Q123待答为历史，不批准分享/导入、生产协议或编辑器视觉。

新增独立`.artifacts/native/mockups/new-session-v2/`与阶段审阅MD
`new-session-design-review-v2.md`：中央输入保留，完整十六项配方与
我的方法、三种组合能力、独立角色、方案差异、工具资源分组及异常路径
进入假数据浏览器稿；各主屏/面板视觉仍待用户逐屏批准，不交产品实施。
V1及其服务不变，全部任务/工具库仅占位；资源/角色覆盖跨任务继承未定。
167条观察＋U01及20能力族/8闭环映射不变，不把示例覆盖当全部能力实现。

预览操作范围见V2 README：独立loopback只读四资源，用户手动打开；
不启动浏览器自动化、原生App/Preview或VM，不改政策。验证限静态/内存/
document桩与只读HTTP，不证明浏览器真实渲染、原生材质或产品旅程。
详见检查点§21；正式客户端仍SwiftUI/原生Liquid Glass，UI仍NOT RUN / DEFERRED。

## 22. 2026-10-21：工具/配置分离与三来源账号能力归并

### 22.1 范围、来源与当前总量

用户要求新建会话输入框更圆、删除两段导语、路径行改四个等宽带图标的
Codex / Claude Code / oh-my-pi / DeepSeek Harness 胶囊；胶囊选工具，
左下主配置选对应 Agent 配置。独立 V3 及阶段说明见
`new-session-design-review-v3.md`，**仍待用户逐屏视觉批准**。
路径字段移入更多配置，不删除项目上下文；custom 身份继续保留。

用户同时要求全面参考 cc-switch、magpie、CLIProxyAPI 的官方授权、
API、模型目录、额度、上下文、协议、映射、推理、故障转移及账号池等。
本轮对 cc-switch / magpie 各补查18个机制簇，归回原 CCS / MAG 观察，
不把同一机制再次加号计数；CLIProxyAPI 新增18条来源观察。
累计 **18仓库来源、185条仓库观察＋U01＝186条**；旧§7–9的17/168
口径保留为当时基线，以本节主归属增量接续，不声称旧表已覆盖新来源。

本地 CLIProxyAPI HEAD `20c9095cac089c6a8e40554debe4dd8a19d4c14f`，
HEAD 路径2,028；根 LICENSE 声明 MIT，不代表供应商授权或整个依赖/资产
均可移植。完整三来源机制补充、证据与限制见
`docs/research/agent-connections-source-review-2026-10-21.md`。
仅静态读取源码，不读 auths / 真实凭据、不安装/运行/登录/测 API；
本地快照不是网上最新，未穷尽 provider×协议×工具×模型组合。

### 22.2 CLIProxyAPI — 新来源观察

以下路径相对 CLIProxyAPI 根；建议不是已采纳、已实现或运行通过。

| 编号 | 亮点机制与证据 | Aeroric 原生化候选 | 不能照搬的边界 |
|---|---|---|---|
| CPA01 | Provider/OAuth矩阵、独立SIWC注册/host/issued client及刷新；`internal/cmd/auth_manager.go:13–25`；`internal/auth/codex/siwc_auth.go:59–94,176–227`；`internal/api/handlers/management/oauth_siwc.go:138–180` | 认证方式/账号/注册分型，传统Codex与新SIWC分开；补A10 | 来源管理绑定要求已有file-backed Codex身份；不能据此要求Aeroric导入真实auths、复用client ID或声称四CLI全覆盖；源码不是供应商授权 |
| CPA02 | API-key group字段继承、类型化profile、合成运行Auth；`internal/config/config_v8.go:64–68,353–420`；`internal/watcher/synthesizer/config.go:36–60,291–389` | 连接单源、工具专属配置、来源/覆盖显式；补A10 | 上游Key与下游准入Key不同；OpenAI-compatible非万能协议，base URL不是每Key都可任意覆盖 |
| CPA03 | 多Codex身份、priority/RR/weighted/fill-first、独立并发准入；`sdk/auth/codex_device.go:254–303`；`sdk/cliproxy/auth/selector.go:364–398,613–636,782–820`；`sdk/cliproxy/auth/home_concurrency.go:23–40,155–170` | 用户自有账号池及可解释调度；补A11 | 文件名plan非实时entitlement；Home外部控制面算法未覆盖；refresh并发不是请求并发，不以池绕供应商限制 |
| CPA04 | 刷新队列/per-auth job、mutex/epoch fence、singleflight、锁内轮换patch；`sdk/cliproxy/auth/auto_refresh_loop.go:16–60,265–327`；`sdk/cliproxy/auth/conductor_refresh.go:809–868`；`internal/auth/codex/refresh_patch.go:61–80,126–146`；`internal/auth/codex/openai_auth.go:197–217`；`internal/runtime/executor/codex_executor_auth.go:131–158` | 单owner凭据轮换、版本预检与失败恢复；补A10 | 不证明全部provider有同级保障，文件锁不是加密；不可删锁抢占或让CLI/host同时刷新同会话 |
| CPA05 | TTL亲和、显式identity优先、subagent继承和前缀/Merkle推断；`sdk/cliproxy/auth/selector.go:1052–1204,1765–1781`；`config.example.yaml:154–178` | 会话上游账号亲和及推断来源；补A11 | hash不是可靠身份；亲和不修改真实CLI session/model/cwd/权限/PTY代际；pi header非OMP端到端证明 |
| CPA06 | 配额窗口/瞬态限流/cooldown按账号与模型区分、被动观察不直接调度；`internal/runtime/executor/codex_executor_terminal.go:317–329,403–444`；`sdk/cliproxy/auth/quota_signals.go:15–53`；`internal/api/handlers/management/quota.go:27–69` | 额度观察、健康冷却和用量分型；补A13 | ResetQuota只清本地冷却，不能叫重置供应商额度；未知/过期不能编造为可用 |
| CPA07 | credential重试、401刷新、下游首个有效输出前的bootstrap缓冲及WS重连分支；`sdk/cliproxy/auth/conductor_selection.go:1119–1153,1370–1437`；`sdk/cliproxy/auth/conductor_stream.go:91–125`；`sdk/cliproxy/auth/conductor_execution.go:596–615`；`internal/runtime/executor/codex_websockets_stream.go:155–177,209–233` | 明确未发送/已受理/未知与attempt恢复；补A11 | 无下游输出不等于上游没收到；不可默认换号重放，流/工具副作用不以该源码证明exactly-once |
| CPA08 | registry目录、plan来源、alias上游池及响应model反写；`sdk/api/handlers/openai/openai_handlers.go:55–108`；`sdk/cliproxy/service_models.go:158–183`；`sdk/cliproxy/auth/oauth_model_alias.go:140–188,263–271`；`sdk/cliproxy/auth/response_model_rewriter.go:11–44` | 模型能力目录、客户端别名与真实上游ID；补A10 | unknown plan可落pro目录，不是账号授权证明；OAuth alias规则不直接适用于API-key group，响应改名不能遮实际模型 |
| CPA09 | 多端点handler→AuthManager→有向translator；`internal/api/server_routes.go:63–135`；`sdk/api/handlers/handlers_execution.go:47–117`；`internal/translator/init.go:4–35`；`sdk/translator/registry.go:98–164` | 有向协议/端点/流模式能力矩阵；补A11 | 不是协议笛卡尔积；缺pair原样返回不等于兼容或语义无损，更不代表四CLI旅程全通 |
| CPA10 | tool/image/call_id及SSE增量转换、stop/usage/签名；`internal/translator/codex/claude/codex_claude_request.go:212–281,376–412`；`internal/translator/codex/claude/codex_claude_response.go:77–177,267–278,358–459`；`internal/api/server_routes.go:74–108` | 保来源的工具/多模态事件流与限制；补A11 | 端点存在非全部provider支持，优化开关需审阅；不能从输出自动批准工具、猜权限或替代交互PTY |
| CPA11 | level/budget/none/auto规范化、suffix优先、验证/夹限及provider落地；`internal/thinking/types.go:39–77`；`internal/thinking/apply.go:131–239`；`internal/thinking/validate.go:38–108`；`internal/thinking/provider/codex/apply.go:44–84` | 原始/规范/实际推理参数和模型能力；补A10 | high/max跨厂商不等价；推理摘要展示非effort，后续payload仍可能覆盖，不静默丢参 |
| CPA12 | 有序payload default/override/filter及finalizer；响应另有chunk插件/header过滤；`internal/runtime/executor/helps/payload_helpers.go:83–224,269–309`；`internal/runtime/executor/helps/payload_finalizer.go:14–35`；`sdk/api/handlers/handlers_stream.go:209–239`；`internal/runtime/executor/codex_executor_execute.go:64–93`；`internal/runtime/executor/codex_executor_request.go:139–144,203–219` | 可预览、有版本、受合规终检限制的高级连接规则；补A11 | 不是通用响应JSON规则；用户override可改先前SIWC字段，Aeroric须不可覆盖的最终校验；未审全executor末屏障 |
| CPA13 | 逐请求/账号/全局/context代理、HTTP/SOCKS/direct、WS及ALPN分支；`internal/runtime/executor/helps/proxy_helpers.go:18–58,130–142`；`sdk/proxyutil/proxy.go:41–68,102–146`；`internal/runtime/executor/helps/utls_client.go:116–151,350–356,418–465` | 显式transport/出站代理与诊断；补A11 | 不保证全链路h2；fingerprint/cloaking不是授权依据；WebRTC仅读配置边界，不直接搬入原生客户端 |
| CPA14 | 同listener管理/API、独立管理鉴权、空access provider放行和任意URL注token工具；`internal/api/server.go:248–265`；`internal/api/handlers/management/handler.go:293–419`；`internal/access/config_access/provider.go:19–22`；`sdk/access/manager.go:44–51`；`internal/api/handlers/management/api_tools.go:61–85,145–216` | 默认loopback/拒绝、管理与业务权限分开；补A17 | 不称独立管理端口；空鉴权可能公开代理，凭据下载/URL注token有泄漏与SSRF风险，不能复制为通用测试功能 |
| CPA15 | watcher/hash/debounce、串行reload、坏配置保旧与generation；`internal/watcher/events.go:67–138`；`internal/watcher/config_reload.go:31–149`；`internal/api/handlers/management/handler.go:244–260` | 版本配置提交、last-good/漂移/部分结果；补A10 | 并非全部网络/TLS参数热更新；表现模式切换或保存配置不重启Agent/更换现有会话 |
| CPA16 | 请求/实际模型账号、token/cache/reasoning及TTFT；内存队列与destructive pop、raw Key/body日志；`sdk/cliproxy/usage/manager.go:24–94`；`internal/redisqueue/queue.go:10–34`；`internal/api/handlers/management/usage.go:23–42`；`internal/redisqueue/plugin.go:128–176`；`internal/logging/request_logger_format.go:117–158` | privacy-first的持久usage/去重与任务归属；补A13 | 非可靠内置账单/持久统计；多消费者竞争，header遮罩不代表body/Key安全，外部SQLite看板不计本仓内置 |
| CPA17 | file/Postgres/object/git后端、local mirror、分路径锁/rename与普通truncate；`cmd/server/main.go:672–680`；`sdk/auth/filestore.go:74–153`；`internal/auth/codex/token.go:45–95`；`internal/store/postgresstore.go:120–149,568–582` | Keychain引用、受控迁移和明确远端同步范围；补A20 | 0600/bcrypt/远端store都非上游token加密；mirror/备份/git历史扩大泄漏，删当前文件不等于历史消失 |
| CPA18 | SDK builder/hooks、受管服务/可选TUI/mDNS、trusted进程内插件与部署；`sdk/cliproxy/builder.go:67–98,214–231`；`internal/api/server.go:235–238`；`internal/pluginhost/loader_unix.go:1–44`；`Dockerfile:1–37`；`docker-compose.yml:17–29` | 工具/扩展适配和受管生命周期机制；补A09 | 动态库非沙箱，来源Compose/Docker非本项目执行许可；EasyCLIProxyAPI GUI/托盘/自动更新不是CPA本仓内置，Apple分发未验证 |

### 22.3 唯一主归属增量与功能去重

本表只分配新18条；与§9.1基线相加恰好覆盖186条，每条一个主归属。
仍为20能力族，按原I01–I08八闭环组织，不新增第九闭环/第五Agent/第二网关。
表内来源不重复计数，其他族可引用依赖但不重复主归属。

| 现有能力族 | 新来源的唯一主归属 |
|---|---|
| A09 工具与扩展资源 | `CPA18` |
| A10 角色配置与环境方案 | `CPA01 CPA02 CPA04 CPA08 CPA11 CPA15` |
| A11 连接弹性与协议适配 | `CPA03 CPA05 CPA07 CPA09 CPA10 CPA12 CPA13` |
| A13 统计与资源护栏 | `CPA06 CPA16` |
| A17 权限、审计与外发 | `CPA14` |
| A20 配置迁移与备份恢复 | `CPA17` |

归并而不缩水：OAuth/Key/CLI状态共同组成账号连接入口；模型发现/映射/
上下文/推理为同一能力目录的不同字段；池/亲和/熔断/重试共用local_router
但状态分开；额度/用量/预算各自有来源及契约；资源/方案/同步复用原owner。
完整十二簇及Rust落点见`agent-connections-design-checkpoint-01.md` §3/5。

两项补查收窄旧风险表述：magpie所查普通生成路径先做Key许可筛选，严格亲和恢复
的是本地软cap排除项，不由此断言权限绕过；MCP若干工具也会局部预算
预占，不可扩大称所有MCP绕预算。内部代调用也可能清模型范围而保留账号
约束，不能据此保证所有入口同一模型白名单或全部工具已覆盖硬预算。
cc-switch同步包不含独立托管OAuth JSON，却仍可能在Provider SQL含秘密；
“没有同步登录”不等于备份无敏感信息。

### 22.4 官方授权差异与Q124

2026-10-21官方核查：OpenAI已公开合资格开源/本地应用SIWC订阅路径，
按应用注册/用户workspace/host及Responses限制接入，不能沿旧印象一刀切
说第三方绝不支持；同时不能把旧Codex backend桥或源码client ID当正式授权。
Claude订阅只走用户在未修改原版Claude Code的官方登录，不把Claude.ai
token收集、中转给四工具。API计费/订阅窗口/上下文/预算不混作余额。
OMP/DSH对特定接入的兼容需逐版本验证，模型列表也不是推理权限证明。

建议「统一账号与连接 → 工具专属Agent配置 → 任务角色独立绑定快照」。
**Q124待回答**；账号中心/添加连接/模型/池等新屏尚未制作或批准。
本轮只提供V3离线稿和研究文档，不授权原生接线、真实登录/API或账号池。
不自动打开浏览器、不启动App/Preview/UI/VM；原生UI仍NOT RUN / DEFERRED。

## 23. 新建会话V4与账号连接V1 — 不新增重复候选（2026-10-21）

用户要求保留输入框左上项目目录下拉，右侧小巧四Harness；
本轮独立V4用等宽32px图标radio、宋体艺术标题、暖白青色层次和短动效。
虚构资料支持拖/点添加，账号池支持拖/上移/下移和撤销；不读真实文件。
主配置继续对应所选工具，独立监督/权限/草稿规则及旧方法版本回归保留。

账号连接V1进入总览/详情、添加连接、官方授权分流、模型覆盖、
只读Agent配置、账号池草稿、诊断迁移七面审阅。
基于Q124推荐分层继续制作不等于批准生产schema、授权注册、
默认池策略、真实连接、全部功能或未见屏幕。
完整十二簇的首版覆盖/后续字段表见`agent-connections-design-review-v1.md`，
不因本页不露所有高级字段而丢弃Stack、代理、usage、扩展与同步机制。
总量仍18仓库、185观察＋U01＝186，唯一主归属仍§9＋§22.3，
20能力族/8闭环不变，不另造第五Agent/第二网关或重复观察ID。

各屏/重大修订均待用户视觉审批；独立稿和新建会话说明见
`new-session-design-review-v4.md`。手动预览`http://127.0.0.1:51285/`，
只读八资源/CSP，无真实Key输入、OAuth/API/Agent/持久化/正式发布。
账号均待授权，模型/兼容性/额度不伪造通过，未知接收不自动换号重放。
正式产品仍SwiftUI/原生Liquid Glass，本稿不加网页/Swift依赖；
未运行浏览器渲染/输入/截图、App/Preview/UI/VM或真实业务，
原生UI仍NOT RUN / DEFERRED，不改政策或产品实施状态。
最终检查与保全观察定位`/tmp/aeroric-q125-v4-location`。

## 24. 新建会话选择栏V5 — 框外独立、选中全名（2026-10-21）

用户认可总体设计方向，另要求项目下拉和Harness选择离开输入框内部。
独立`.artifacts/native/mockups/new-session-v5/`将二者放在框外上方；
四工具顺序固定，当前图标＋完整名称，其他仅图标、不显示名称。
浏览器真实radio负责选择态，名称轻展开/收拢，不手绘活动玻璃滑块。
局部更新保留选择器节点/焦点，草稿/权限/监督角色与账号V1原逻辑不变。
正式产品仍SwiftUI与原生Liquid Glass，动态标签系统样式能力待原生验证；
不把网页过渡当已完成的原生动画。

选择栏V5待本次视觉审批；总体认可不追认此前十三面逐屏通过。
见`new-session-design-review-v5.md`和检查点§24；手动地址
`http://127.0.0.1:51878/`，证据定位`/tmp/aeroric-q126-v5-location`。
18仓库/185观察＋U01＝186、20族/8闭环均不变，不新增候选/工具内核。
V1–V4/服务及账号源码保全；无浏览器自动化、App/Preview/UI/VM，
无真实OAuth/API/Agent/凭据/持久化，UI仍NOT RUN / DEFERRED。
不改产品/政策/冻结实施包/任务状态，不create/start或提交发布。

## 25. 新建会话V6 — 轻薄胶囊与石墨深色（2026-10-21）

独立`new-session-v6/`响应项目仅名称/去边框/6px间距、扁Harness/输入框、
柔和单值下拉箭头、灰黑深色与组合能力/侧栏胶囊要求。
项目ID及归属提议、主配置/独立角色/权限/草稿/配方规则不变。
侧栏真radio表达当前页，不自绘active底；正式仍SwiftUI系统Picker/List、
苹果原生Liquid Glass。网页透明外壳未实现玻璃选中滑块，原生核验候选/
API限制见`new-session-design-review-v6.md` §3，不引入第三方或假接口。
账号V1源码/CSS沿用，但共享外观改变，七面仍逐屏待审。

出稿时V6待用户视觉审批（后续确认见§25.1），不追认其他面已通过，
也不授权产品实施。
手动`http://127.0.0.1:60883/`，新证据`/tmp/aeroric-q127-v6-location`；
V1–V5/旧服务保全。18仓库/186观察、20族/8闭环不变，不重研或加号。
只离线/静态/内存/HTTP，不自动浏览器、不App/Preview/UI/VM或真实业务，
UI仍NOT RUN / DEFERRED；不改产品/政策/冻结包/任务状态，不提交发布。

### 25.1 V6主屏视觉确认（2026-10-21）

用户明确回复“好的，我确认了页面，下一步做什么”，据此记录本次
新建会话V6主屏布局及共享样式方向已确认，见
`new-session-design-review-v6.md` §6。其他会话面板、账号V1七面仍沿原
逐屏审批状态；原出稿、测试与来源观察不追改，不新增功能采纳或开工批准。
下一步建议整理本屏原生实施交接，并继续账号总览/详情审阅；
交接包尚未制作，原生玻璃/动画未验收，真实UI检查NOT RUN / DEFERRED。
UI/VM暂停保持，不改产品/政策/凭据/任务状态，不提交发布。

## 26. 统一原生界面交接，不新增候选或追认批准（2026-10-21）

用户要求先写一份MD，覆盖此前确认范围、严格对照浏览器预览，交其他
Agent实施，完成后用户自己看App。交接为
`approved-ui-native-integration-stage-01.md` v1.0。
A01–A04普通入口视觉/导航/草稿、F01–F02限定布局修正、B01–B09历史
离线样例核对保全、C类未审页面分开，不把原则/范围批准追认为正式
整屏视觉通过。账号与连接七面及home/完整工具库仍需后续逐屏审阅。

高保真是交付标准，不能以Review/HTML假壳/模拟成功代替；系统材质/
窗口/选择差异具名交监工，不静默回退。冻结旧包/稿件，保全所有候选、
18仓库/185＋U01＝186、20族/8闭环，不重研或加号。
本轮仅文档与静态核对，不改产品、政策、凭据/任务状态，不开App/
Preview/浏览器/Agent/VM；真实UI检查NOT RUN / DEFERRED。

## 27. Q128已确认，账号总览状态提案Q129待回答（2026-10-21）

用户认可连接保存成功后提供可跳过的首个Agent配置引导，不自动设默认、
替换旧配置、改变运行任务或付费推理；兼容性未核实明示。
Q129建议总览行只突出最需处理的一项，详情分别显示授权、工具/模型
适配、额度及最近检测的真实范围/来源/时间；未知不为0，未检测不等于
失效或已通过。Q129待回答，自动发现/采集/检测授权/账号池策略另议。
权威续记：`agent-connections-design-checkpoint-01.md` §8–9；
检查点§27、review-round-19§50。

本轮不新增来源观察或同义能力族；18仓库、185＋U01＝186及20族/8闭环
不变。不追认账号七面/引导页视觉，不改冻结执行包/产品/稿件/政策/
任务状态，不create/start或提交发布；未运行真实业务、App/UI/VM，
原生UI仍NOT RUN / DEFERRED。

## 28. Q129已确认，账号与连接V2出稿待视觉审批（2026-10-21）

用户认可总览突出当前重点、详情分授权/工具与模型适配/额度/最近检测。
独立`.artifacts/native/mockups/agent-connections-v2/`落实简洁目录、
分项来源/时间/范围、失效和未知额度离线例，以及Q128可跳过配置引导。
主动创建的配置只存在本页未发布草稿，可只读查看，不进入新建会话，
不设默认、不改原任务；未知不为0，未检测不当通过或失败。

见`agent-connections-design-review-v2.md`与账号检查点§10；
手动预览`http://127.0.0.1:65257/`。总览/详情、添加连接、配置引导
及配置只读页分别待审，其余面沿V1待审。原则确认不追认视觉批准。
自动采集/检测授权/账号池策略仍待议，不扩冻结原生执行包。
V6及旧稿/服务保全，18仓库、185＋U01＝186、20族/8闭环不变。
仅离线/静态/只读HTTP，无自动浏览器、App/Preview/UI/VM或真实
Key/OAuth/API/Agent/持久化/发布；UI仍NOT RUN / DEFERRED。
不改产品/政策/任务metadata/context，不create/start或提交发布。

## 29. 账号V3：同排顶栏与轻盈滚动（2026-10-21）

按用户修订，独立`agent-connections-v3/`去重复账号标题/版本行与中文大标题，
保留英文艺术字；四分区同一透明胶囊、与添加连接同排置顶，
正文保留滚动但隐藏滚动条。浏览器真实radio不冒充苹果玻璃滑块，
正式端仍SwiftUI系统选择＋原生Liquid Glass及系统动画，无第三方或自绘active。
见`agent-connections-design-review-v3.md`、账号检查点§11；手动
`http://127.0.0.1:63549/`，各面仍待用户视觉审批，不扩冻结实施包。
Q128/Q129不变，V2/V6原稿/服务保全；不新增调查计数或能力主归属，
仍18仓库/185＋U01＝186、20族/8闭环。仅离线/静态/只读loopback检查，
证据`/tmp/aeroric-q130-accounts-v3-location`；真实UI仍NOT RUN / DEFERRED，
无自动浏览器/App/Preview/UI/VM/真实业务，不改产品/政策/任务状态或提交发布。

### 29.1 V3账号主页面已获视觉确认（2026-10-21）

用户回复“确认，我比较满意”，批准本次展示的V3主页面总览/详情布局
及顶栏/艺术字/滚动修订；后续以该版为视觉基线，重大修订重新审批。
回执`agent-connections-design-review-v3.md` §5、账号检查点§11.1；
其他独立管理面/弹层仍逐屏待审，不扩冻结实施包或真实业务授权。
来源18仓库/186观察、20族/8闭环不变，正式SwiftUI/原生Liquid Glass
及NOT RUN / DEFERRED运行边界不变。本次仅记录，不修改稿件或执行界面/业务。

## 30. Q131添加连接V4出稿，不新增重复能力（2026-10-21）

用户认可基础简洁/高级折叠、API与官方分流、保存检测分开、连接配置分步。
独立`agent-connections-v4/`表达面板，基础四项/高级代理与超时文本，
Key/请求头敏感输入及官方登录禁用；检测仅解释，保存不等于可用。
成功后Q128可跳过引导不变，不默认发布/入池/改任务；各分支保留各自草稿。
审阅`add-connection-design-review-v4.md`、账号检查点§12，手动
`http://127.0.0.1:62147/#add`；新面板及其他独立管理面仍待视觉审批。
V3视觉基线/原稿/服务与冻结实施包保全；SwiftUI/原生Liquid Glass要求不变。
18仓库/185＋U01＝186观察、20族/8闭环不变，高级候选未删减，不另造协议或网关。
仅离线/静态/只读loopback，证据`/tmp/aeroric-q131-add-v4-location`；
真实UI仍NOT RUN / DEFERRED，不自动浏览器或运行App/Preview/UI/VM/真实业务，
不改产品/政策/任务状态，不create/start或提交发布。

## 31. 全局代理/账号导入/模型编辑补查，不另计重复观察（2026-10-21）

用户修订添加连接V4：代理在设置统一管理，连接勾选；本机账号识别、
网页回调、CPA多账号导入；API主动获取/勾选/手填模型，映射名、
上下文/输出/输入类型及自定义推理档位投影到新建任务。
补查归回原MAG02/CCS模型与配置观察、CPA01/02/08/11/13/17，
不新增来源/观察主归属，仍18/186、20族/8闭环。

事实与边界见`docs/research/agent-connections-proxy-auth-models-2026-10-21.md`、
账号检查点§13；结构提案`connection-model-catalog-design-v5.md`待讨论，
无新浏览器稿/视觉批准。CPA多JSON不等于通用token许可/安全迁移；目录不等于推理
实测；手工能力与自定义档位需真实参数映射/四工具适配，不照搬固定枚举。
Q132本机身份识别与授权关联边界待答，V4未记视觉通过，下一版尚未出稿。
本轮仅只读研究/文档；V3基线、旧稿/服务和冻结包保全，SwiftUI/Apple
Liquid Glass要求不变。无真实账号/登录/API/UI/VM/任务状态/提交发布操作；
真实UI保持NOT RUN / DEFERRED。

## 32. Q132确认与Q133模型规则归并提案（2026-10-21）

用户“可以”确认首次授权后的限定本机身份识别、明确关联、安全复用
不成立时重登录；不自动复制/覆盖个人认证文件，合资格迁移另确认。
不把此确认记成全部连接默认、V5视觉或实施批准。
见账号检查点§14、`connection-model-catalog-design-v5.md` §8–9。

Q133仅推荐一份连接级版本化模型/能力/推理定义，配置按Harness引用，
任务各角色固定版本；更新明确应用，草稿/运行任务不静默改，
旧版本不绕过撤销/安全终检。**待用户回答**，不是新增功能观察。
来源18仓库、185＋U01＝186观察、20族/8闭环及原能力owner不变。
V3基线、旧稿/服务、政策/冻结包保全；V5尚无浏览器稿或视觉批准，
真实UI保持NOT RUN / DEFERRED。本次仅文档，无真实账号/登录/API/
Agent/自动浏览器/App/Preview/UI/VM，不改任务状态或提交发布。

## 33. Q133共享规则确认，Q134目录/测试动作待答（2026-10-21）

用户“认可”确认一份模型规则、工具配置引用及任务版本固定，
更新明确应用、不覆盖草稿/运行任务/其他角色绑定，不兼容明确处理。
完整回执与下问见`connection-model-catalog-design-v5.md` §10–11、
账号检查点§15。Q134仅推荐目录读取、手工声明与调用测试分开：
获取不顺带推理/余额/外部资料；未知可填写、刷新保留覆盖、失败可存连接；
测试单独说明目标/内容/潜在费用，结果不夸大为全部能力通过。**待回答**。

本次不新增来源/观察或owner，仍18仓库、185＋U01＝186、20族/8闭环。
V5无浏览器稿/视觉批准；V3基线、旧稿/服务、政策及冻结包保全。
只更新文档，未操作真实账号/登录/API/Agent/浏览器/App/Preview/UI/VM，
未改任务状态或提交发布，真实UI仍NOT RUN / DEFERRED。

## 34. Q134确认，Q135自定义推理映射待答（2026-10-21）

用户“认可”确认目录读取/编辑/调用测试分开，未知可手填、失败可保存、
刷新保留覆盖；额度入口另议。回执见V5 §12、账号检查点§16，
不视为真实请求、视觉或实施批准。
Q135建议每模型可增删/排序/命名档位，名称与实际枚举/预算映射分开，
按协议/Harness适配，关闭自定义配置不等于关闭模型推理。
**待答**，正文`connection-model-catalog-design-v5.md` §13。

不增计来源/观察/owner，仍18仓库、185＋U01＝186、20族/8闭环。
V5无浏览器稿/视觉批准；旧稿/服务、政策/冻结包不改。
仅文档，无真实账号/登录/API/Agent/自动浏览器/App/Preview/UI/VM、
任务状态或提交发布操作，真实UI保持NOT RUN / DEFERRED。

## 35. Q135确认与Q136额度分型/刷新提案（2026-10-21）

用户“认可”确认档位自由命名/排序、实际映射与适配校验，
关闭自定义配置不等于关闭推理。回执V5 §14、账号检查点§17。
Q136建议手动查询及默认关闭的自动刷新，开启时明确范围；
余额/Key限额、订阅窗口、本机用量/估算分开，未知/零/过期/失败明确。
**待答**，正文`connection-model-catalog-design-v5.md` §15。

仍18仓库、185＋U01＝186、20族/8闭环；复用原统计/连接owner，不增观察/账本。
V5无浏览器稿/视觉批准；旧稿/服务、政策/冻结包保全。
本轮只文档，无真实账号/登录/额度/API/Agent/自动浏览器/App/Preview/
UI/VM、任务状态或提交发布操作；真实UI保持NOT RUN / DEFERRED。

## 36. Q136确认，Q137多账号导入流程待答（2026-10-21）

用户“认可”确认自动刷新默认关闭、开启后按设运行，额度分型且失败不清零。
回执V5 §16、账号检查点§18；不批准真实采集、页面视觉或实施。
Q137建议多JSON预检→统一预览→一次确认→逐项结果，
无冲突合资格新增项可预选，重复/冲突默认跳过，不按文件名/邮箱覆盖；
解析成功不冒充账号可用，不自动入池/设默认/测试。**待答**，V5 §17。

仍18仓库、185＋U01＝186、20族/8闭环，不新增来源/观察或owner；
V5无浏览器稿/视觉批准，旧稿/服务/政策/冻结包保全。
仅文档，无真实凭据导入/登录/API/Agent/自动浏览器/App/Preview/UI/VM、
任务状态或提交发布操作，真实UI保持NOT RUN / DEFERRED。

## 37. Q137导入默认确认，Q138网页登录交互待答（2026-10-21）

用户“认可”确认多JSON先预检/脱敏统一预览、批量确认逐项结果，
无冲突合资格新增可预选，重复/冲突默认跳过；更新明确确认，
不按文件名/邮箱覆盖，不自动入池/设默认/启动任务。
回执V5 §18、账号检查点§19；不扩为未展示细节、视觉或实施批准。
Q138将既有授权候选细化为受支持的自动回跳优先、手动入口兜底，
回调接收/授权完成分开，完成核对身份再保存，原版登录另行引导；
取消/过期不复用旧回调、不后台登录。**待答**，正文V5 §19。

仍18仓库、185＋U01＝186、20族/8闭环，不新增来源/观察或owner。
V5无浏览器稿/视觉批准；旧稿/服务/政策/冻结包不改。
仅文档，无真实凭据导入/登录/API/Agent/自动浏览器/App/Preview/UI/VM、
产品/任务状态或提交发布操作；真实UI保持NOT RUN / DEFERRED。

## 38. Q138登录交互确认，Q139代理默认与失败处理待答（2026-10-21）

用户“认可”确认自动回跳优先/适用的手动兜底、核对账号后保存，
原版登录分流，取消/过期保留非敏感表单并重开流程。
回执V5 §20、账号检查点§20，不是接入资格、真实登录或视觉/实施批准。
Q139沿原集中代理候选提出新连接默认直连，勾选才用设置代理，
设置返回保留草稿，未配置可存待配但不发相关请求；
失败不暗降直连，不重启Agent/暗改运行任务，实际支持范围明确。
**待答**，正文V5 §21，不增计新观察或owner。

仍18仓库、185＋U01＝186、20族/8闭环；
V5无浏览器稿/视觉批准，旧稿/服务/政策/冻结包保全。
仅文档，无真实账号/导入/登录/代理配置/网络检查/API/Agent/
自动浏览器/App/Preview/UI/VM，不改产品/任务状态或提交发布；
真实UI保持NOT RUN / DEFERRED。

## 39. Q139代理默认确认，Q140批量配置创建交互待答（2026-10-21）

用户“认可”确认默认直连、勾选统一代理、失败不自动直连，
设置返回保留草稿，不打断/暗改运行任务。回执V5 §22、账号检查点§21，
不是代理实施、真实操作或视觉批准。
Q140沿已有配置引用/Q128引导提出可选多Harness一次创建：
逐行确认模型/档位，工具配置独立、目录版本共用，冲突不覆盖，
逐项结果保留成功项，不自动选进草稿/设默认/改权限或启动任务。
**待答**，正文V5 §23；不是新增来源观察或第二模型/配置系统。

仍18仓库、185＋U01＝186、20族/8闭环；
V5无浏览器稿/视觉批准，旧稿/服务/政策/冻结包保持。
仅文档，无真实配置创建/发布、账号/登录/网络/API/Agent/
自动浏览器/App/Preview/UI/VM，不改产品/任务状态或提交发布；
真实UI保持NOT RUN / DEFERRED。

## 40. Q140上轮展示原则确认，V5制作中且视觉待审（2026-10-21）

用户“认可”仅确认保存连接后可跳过引导，按需选一个或多个Harness、
不自动全选四个；分别确认每工具名称/模型/推理，独立配置共用模型目录。
一次确认创建，同名不覆盖/不自动改名，逐项结果、部分失败不影响成功项；
成功项只进入对应Harness的Agent配置可选列表，不自动默认、不覆盖会话草稿、
不启动任务。回执V5 §24、账号检查点§22；§39待答保留为历史，
不将V5 §23未展示实现细节、正式schema、配置发布或真实创建一并批准。
不是新增来源观察、第二配置/模型系统或产品实施/视觉批准。

当前无新问题待答。V5稿制作中、尚无视觉批准，详见预期的
`docs/native/add-connection-design-review-v5.md`（主代理随后创建）。
18仓库、185条观察＋U01＝186、20族/8闭环不变，不改索引统计。
SwiftUI/Apple原生Liquid Glass及Rust core/host业务owner不变，新面逐屏视觉待审；
旧稿/政策/冻结交接保持。仅文档，无真实Home/账号/凭据/Keychain/API/
Agent/网络/登录/浏览器/App/Preview/UI/VM操作，不运行产品测试/服务；
真实UI仍NOT RUN / DEFERRED，不改产品/任务metadata/context/status，
不create/start或commit/push。

## 41. V5出稿，不新增候选来源或审批（2026-10-21）

Q140回执后，独立V5参考稿已出：
`.artifacts/native/mockups/agent-connections-v5/`，
手动地址 `http://127.0.0.1:51911/#models`。
逐屏清单、待审状态和明确未覆盖项见
`docs/native/add-connection-design-review-v5.md`。本次是既有需求的设计表达，
不是新来源观察、全部功能完成或视觉批准；§40旧制作状态保留为历史。
仍18仓库、185＋U01＝186、20族/8闭环，不改统计/冻结交接或V3主屏。
仅隔离fixture与本机只读静态服务/离线核对，无真实账号/授权/API/Agent/发布；
不打开或操控浏览器、不启动App/Preview/UI/VM。真实UI NOT RUN / DEFERRED，
产品架构和任务状态不变，不提交发布。

## 42. V5送审视觉稿确认，不新增候选或全功能批准（2026-10-21）

用户回复“确认通过”，记录本次送审、审阅的V5浏览器视觉稿通过。
范围见 `docs/native/add-connection-design-review-v5.md` §7、
V5设计§26、账号检查点§24；不是十条独立逐屏批准，未展示/未审状态、
其他管理面及省略流程仍待审。§40–41为此前制作/送审阶段记录。

本次不新增来源观察或配置/模型系统，不批准完整Agent配置管理、账号池/
故障转移、诊断迁移，不扩冻结交接。V5稿件15文件和旧稿/政策均保持。
仍18仓库、185＋U01＝186、20族/8闭环；正式端仍SwiftUI/Apple原生
Liquid Glass，Rust core/host单writer。仅8份文档审批记账，
证据`/tmp/aeroric-v5-visual-approval-location`。
无真实账号/API/Agent/浏览器/App/Preview/UI/VM操作，
真实UI仍NOT RUN / DEFERRED；不改任务状态，不create/start或commit/push。

## 43. 原生保真试点方法确认，不新增功能候选（2026-10-22）

用户“可以”及随后“继续”确认先以新建会话跑通保真闭环，短包
`new-session-native-fidelity-01.md`及其审计/指纹已准备，尚待现有执行者ACK。
这是交付方法确认，不是新页面/状态批准或已还原完成；
先对账V6与现有MSR01后续修订、复用同源SwiftUI组件，不创建竞争实现。
18仓库、185＋U01＝186、20族/8闭环不变，不新增观察、owner或模型/配置系统。
仅文档与静态核对，证据`/tmp/aeroric-new-session-fidelity-location`；
旧稿/政策/冻结包和任务状态不改，无浏览器/App/Preview/UI/VM/
真实账号/API/Agent操作，真实UI仍NOT RUN / DEFERRED，不提交发布。
