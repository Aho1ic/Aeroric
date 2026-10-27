# 项目知识审阅样例 01 — 外部执行模型交接

版本：1.0 · 2026-10-16 · 批准依据：Q45 用户回复“认可”。
对应设计：Q41–Q44；参考候选：F09、M01、F08、M03、L06。

**已批准范围：独立、可交互的SwiftUI原生离线知识审阅样例。
不是知识引擎、真实自动入库/权限设置、正式布局或运行验收。**
用户将本文明确交给新终端并要求执行后，执行者才按本文及Trellis门槛实施。
主线程只设计、交接和审查，本次没有产品实现或运行。
本文独立于Q26/Q35和视觉基础执行包，不是整个前端重整的阶段02。
已交付的 `project-knowledge-design-checkpoint-01.md` 版本0.1仍只覆盖
Q41–Q44；其中Q45待确认是当时快照，不改原文来追加本轮授权。

**现行基线澄清(2026-10-16)**:现行产品最低系统以 `MIGRATION_PLAN.md`
顶部 Apple-only 基线 §B“系统与架构矩阵”为准——最低 macOS26;验证目标
Apple Silicon/macOS27 与 Intel/macOS26,不建立 Intel/macOS27 组合;同源
确认见 `docs/native/adr/039-distribution-pivot-macos-ios.md` §7。冻结快照
与既有 spec 内的 macOS14 字样是历史措辞,不覆盖本条。产品政策不等于现有
deployment 设置已迁移。本条不改 deployment、签名、CI,不改变 Q45 已确认
的独立离线样例范围与状态,不构成阶段02或知识引擎授权;冻结历史文件保持
原样。

## 1. 目标、必读与不做什么

在**现有Aeroric原生App target**内新增DEBUG、显式开启的知识审阅窗口，
让用户点击体验候选集中审阅、项目自动入库配置、带来源的按需参考、
修订/停用及在途影响。只用窗口内存fixture；重置/退出再启动恢复样例。
“独立”是交付边界，不另建App、HTML/WebView或一套组件系统。
只交截图、不可点击稿、`#Preview` 或测试常量不算交付。

仓库：`/Users/macbook/Downloads/同步空间/LYX/Aeroric`。先读：

- 根 `AGENTS.md`、`.trellis/workflow.md`；
- `.trellis/spec/aeroric-macos/frontend/index.md`、
  `docs/native/adr/001-native-boundary.md`；
- `docs/native/project-knowledge-design-checkpoint-01.md` 版本0.1全文；
- 本文全文及实际入口/共享组件；设施事实按需读
  `.trellis/tasks/10-14-native-redesign-shell/research/knowledge-review-handoff-audit.md`。

总库仍为 `docs/native/harness-feature-candidates.md` 的10来源、
89条观察＋U01，不是90个批准模块。本阶段只参考上述五项及其限制，
不重扫harness，不安装/运行参考项目，不复制其UI、代码或技能。

### 非目标和隔离边界

- 不启动真实Agent、模型/API、MCP、PTY、治理进程、工作流或网络服务。
- 不读取真实历史、任务、项目、知识、权限、连接、密钥或生产配置；
  不写真实知识/任务/权限、UserDefaults业务配置、数据库或索引。
- 不导入/调用 `AeroricClient`、生产Store、AppServices、UDS或真实回源；
  来源详情只展开内嵌虚构摘录，不打开文件、URL、个人历史或外部应用。
- 不实现提炼器、检索/RAG、规则引擎、知识生命周期/传播协议、权限沙箱、
  版本服务或跨项目记忆。有限fixture转换不是生产数据模型/状态机。
- 不生成、安装、加载或执行技能，不新增真实权限开关、审批流程或验收标准。
  Full Access、外观与interactionMode分别表达，不相互替代。
- 不改首页默认all-tasks、主导航、菜单业务路由、业务owner、OMP历史根
  或原生PTY/TUI；不复活rpc-ui，不改Tauri/React或外部workflow所有权。
- 不建分支/worktree/clone、不提交推送或发布，不覆盖或回滚他人dirty改动。

样例和拒绝页都须**先于任何产品App/delegate/services构造进入，自身不启host**。
仅允许入口目录边界的只读检查和自身窗口测量；测试设施另按§6保存证据。
完整UI回归会预热普通产品App，也会执行旧审阅页，不能把它说成全程无副作用；
特别注意§4/§6的旧delegate清偏好风险。

## 2. 首批信息层级与操作

沿用A样式和项目原生组件。以下是样例的信息层级，不冻结正式导航/布局；
可以优化间距、折叠和排版，不强迫固定三栏或保留长期空白检查器。

1. **顶部**：虚构项目名，始终可见“离线知识审阅 · 非真实资料”；
   原生场景菜单、重置和窗口内外观预览，不显示假在线服务状态。
2. **主要列表**：候选/可复用内容/历史按系统选择控件组织；
   默认选有内容项，能批量选择候选、查看状态及不能入库的具体原因。
3. **详情**：简明内容、来源摘录、适用范围/限制、候选或已确认的区别，
   版本/替代关联和相关任务影响。长文按需展开，不把整库堆成大表格。
4. **按需配置/详情**：项目自动入库配置用原生sheet或检查器；
   来源/版本/任务影响只在需要时展开。窄窗用菜单和纵向滚动保持所有动作可达。

初始方向1260×860pt，另核对1000×640、400×600。
用自身窗口公开API记录实际外框/内容尺寸；屏幕不够大如实说明，不缩放截图
冒充实际尺寸。这里只验证样例，不承诺正式工作区支持这些压缩范围。

### 必须能点击，而非只有说明

- 批量选择、确认/丢弃安全候选；编辑内容/适用说明，取消不应用，
  应用只改样例内存。危险候选显示待处理原因，不设“忽略全部限制入库”。
- 打开自动入库配置：默认关闭，可预览固定的类型/来源/项目范围组合；
  取消不应用、应用只改内存。任务授权另以“默认/Full Access示例情景”
  切换，不伪装为真的提权或全局设置。
- 用“演示处理新候选”载入有限fixture结果：关闭时留候选，开启且符合
  预设条件时免逐条确认；冲突、敏感或依据不足仍留候选并说明原因。
- 选择虚构任务/工作包，查看按需精简知识及来源/限制；切换当前明确要求、
  来源缺失或冲突情景，看到不同解释，而非整库注入或自动执行。
- 修订一条已确认知识，取消/应用可区分；应用生成样例新版本并保留关联。
  停用后从“后续新上下文推荐”中排除，历史仍可明确按旧版本回看。
- 查看引用旧版本的在途工作、演示通知/回执与局部核对结果；安全无关
  文档工作可继续。通知/ACK按钮不能直接变成“已纠正/已核验”。
- 重置、切换场景明确只影响内存；窗口重建不复用上次的Store，
  退出重启不保留草稿或配置。测试验证取消/应用及重开，不只检查文案。

所有来源、引用、版本、任务、配置与检查记录均内嵌虚构，无凭据或敏感原文。
“已核验”只可用于明确标注的预置示例记录，不表示本次真跑过验证。
演示点击不意味着生产流程每步都需用户批准。

## 3. 八个离线场景及不可推导的机制

K1–K8仅是样例场景编号，不是新候选ID、wire枚举或产品协议。
每个场景须有一致的内容/来源/范围/版本/记录及实际可见变化，
不能只换标题或显示恒定的“安全/通过”。

| 场景 | 必须展示的差异和有限操作 |
|---|---|
| K1 集中审阅 | 至少两条安全候选批量确认，并能修改取消/应用、丢弃另一条；来源/范围/限制随内容保留，自报未核验不变成成功经验，默认只在虚构项目内复用 |
| K2 自动入库与授权独立 | 自动入库默认关闭；与默认/Full Access情景的四种组合都能区分。开启且符合预设条件时免逐条确认，Full Access本身不打开入库，也不扩大知识范围 |
| K3 例外仍留候选 | 冲突、敏感、依据不足各有具体安全fixture及原因；用脱敏占位而非真实秘密；开启自动入库/切Full Access不能强行收纳。超出预设范围的内容不扩到全局或生成技能 |
| K4 按需参考 | 当前工作只展示相关精简内容与来源/条件；历史SQLite选择不强制新任务继续使用。切换当前要求/缺来源/过时或冲突情景时明示限制，不升级为系统指令，不替代权限或当前验证 |
| K5 纠正与版本 | 用户纠正生成有依据/替代关联的示例v2，仍能查看获准v1历史及其旧状态；取消不生成版本，Agent新猜测不自行覆盖用户确认 |
| K6 停用与后续引用 | 显式停用不适用知识，后续新上下文推荐不再含它；历史与已引用旧版本仍可回看且明确状态，不能声称抹除模型已消费内容 |
| K7 在途局部核对 | 已引用旧接口的相关联调显示待核对，独立安全文档工作可继续；演示通知/ACK仍未纠正。载入适用的局部核对示例记录后只更新受影响项，不全队重启、回滚或重放外部操作 |
| K8 无持久化与重置 | 批量选择、草稿、配置和版本变化都可重置；退出再启动恢复初始fixture，窄窗/键盘/外观变化不写真实偏好或改变权限 |

K2/K3只是固定fixture预设与有限结果，不实现可执行表达式、通用规则编辑器、
采集/提炼引擎或真实定时自动化。**关闭自动入库对既有知识/在途任务的具体
影响仍未设计**：界面明确提示本样例不演示该传播规则，不做“关闭即停用/
撤回/清上下文”动作，也不把样例状态保留方式定义成正式产品语义。

K5–K7不选定身份/存储owner、失效判据、生效时刻、删除/保留、恢复启用、
引用传播、实际消费或纠正协议。不以“消息已发/已收到”代替完成更正，
不伪称四家族均能热替换知识或可靠撤销上下文。用户Stop与宿主硬限制优先。
U01反思/治理保持既定只读职责；此样例不新增每条知识的治理签字或写权限。

## 4. 原生组件、辅助功能和安全入口

### 复用而非另造

- 实际复用 `NativePanelFoundation`、`GlassPanel`、`NativePanelGeometry`、
  `NativeIconButton`、`NativeSelectionList`/`NativeSegmentedPicker`，
  按真实需要装配，不为凑名字加入无关控件。
- SwiftUI/AppKit真实Material,现行最低系统macOS26(见本文顶部“现行基线
  澄清(2026-10-16)”);26+真实Liquid Glass按
  availability门控,只增强可交互控件,不给整个内容pane套 `glassEffect`。
- 连续材料基底覆盖面板圆角间隙，18连续圆角、8pt间距/内边距、
  弱语义边界；长知识/来源正文用不透明 `Color.dsTextBackground`。
- 用语义accent/success/warning/danger配文字/图标区分内容/状态，
  不只堆灰色、不抄截图RGB、不只靠颜色。独立图标默认无永久圆环/托盘。
- 邻接互斥控件使用系统选择态和共享NativeSelection；不手绘active底/
  下划线。详情焦点复用当前单选 `NativeSelectionList`；批量勾选用独立的
  系统checkbox Toggle，与详情选择分开，不自造选中卡片或扩改共享组件。
  缺必要共享变体时回报主线程，不越界复制或修改Design。
- glass/classic、interactionMode、权限模式独立；样例不改真实模式。
  减少透明度/增强对比/减少动效必须实际影响View；注入只加强，
  明示系统值/演示值，不修改全局系统设置。
- AX identifier只放叶子，统一 `knowledgeReview.` 前缀及中文可读名称。
  验证Tab/方向键/Escape、sheet、长文滚动、窄窗动作与可读状态；
  不裁切焦点/IME，不用私有窗口API、swizzling或无边框窗口。

### 新入口契约：显式请求才分流

新增 `AERORIC_NATIVE_KNOWLEDGE_REVIEW=1`，**当前尚未支持**。
新样例/fixture/入口/拒绝页整体DEBUG限定，使用已有只读
`NativeVisualReviewEntry.isIsolatedDataDirectory` 检查安全Caches子目录；
要求 `AERORIC_ENV=ui-test` 及显式数据路径。不另写宽松前缀判断，
不创建样例数据目录，不把检查称为OS沙箱或偏好域隔离。

在 `AeroricEntryPoint.main` DEBUG部分新增最早的只读决策，先于已有全部
审阅分流，更先于任何产品App/delegate/services：

1. **新flag不存在**：inactive，原链原样继续，包括旧入口对非法值的解释；
   不借此修复、重排或统一旧入口。
2. **新flag存在且为1，环境/目录有效，无其他审阅请求冲突**：
   只进入知识样例Scene。
3. **新flag存在但非1（含0/空值）、环境/目录不合法，或其他审阅flag为1**：
   只进新只读拒绝页，不回落产品，不靠顺序挑一个。

冲突键：`AERORIC_NATIVE_EVOLUTION_REVIEW`、
`AERORIC_NATIVE_VISUAL_REVIEW`、`AERORIC_NATIVE_COLLABORATION_REVIEW`。
最后一个是Q35已交接但本次静态核对尚未实现的预留契约，仍要拒绝同时为1。
只有新flag存在时才由新入口解释冲突；其他flag为0不是1，不因此冲突。
若开工协作样例已落地，重新核对现有链/交接，缺新flag时必须保留其行为，
不覆盖外部新实现；超出本白名单先协调。
新Scene用各自唯一id，无业务菜单、生产delegate/openWindow路由；
拒绝原因可读但不泄露路径，拒绝页不冒充样例已启动。

### 不可复用当前旧ReviewAppDelegate

2026-10-16只读看到 `AeroricApp.swift` 中旧 `ReviewAppDelegate` 会调用
`UserDefaults.standard.removePersistentDomain` 清本bundle偏好。
新知识样例和拒绝Scene**不得挂载、继承或调用这个delegate/清理路径**，
也不得通过 `@AppStorage`、显式defaults写入或“修复窗口”的清偏好动作落盘。
不在本阶段修旧delegate或旧样例；将风险交还其owner。

如确需激活/关闭窗口，可在新 `NativeKnowledgeReviewEntry.swift` 内定义
知识专属的无业务副作用delegate，仅管理自身窗口生命周期，
不枚举操纵其他实例、不清偏好、不接AppServices。也可不用delegate。
正常系统窗口/自动化设施的系统行为与应用业务写入分别报告，
不声称绝无任何OS级持久痕迹。

ReleaseLocal排除全部新实现，忽略新变量走原产品链。
只验实际Release编译/有效设置与排除，不为测flag启动生产Release。

## 5. 文件白名单、所有权与实施顺序

白名单是最大范围，不要求全改。只新增以下五份样例源文件，可在其中拆
小View；增加文件、跨界修改或修共享组件须先回主线程说明最小差异。

| 路径 | 允许职责 |
|---|---|
| `macos/AeroricKit/Sources/AeroricUI/KnowledgeReview/NativeKnowledgeReviewView.swift`（新） | 样例View/小View，实际复用原生组件 |
| 同目录 `KnowledgeReviewModels.swift`（新） | 仅样例的候选、来源、配置预览、修订/引用/影响值类型 |
| 同目录 `KnowledgeReviewFixtures.swift`（新） | K1–K8安全虚构内容与有限预设记录 |
| 同目录 `KnowledgeReviewStore.swift`（新） | `@MainActor @Observable`窗口内存状态、草稿与有限转换 |
| 同目录 `NativeKnowledgeReviewEntry.swift`（新） | DEBUG只读入口/拒绝页；必要时知识专属无副作用delegate |
| `macos/AeroricApp/Sources/AeroricApp.swift` | 只加最早DEBUG知识分流/独立Scene与拒绝Scene，不动旧链/delegate |
| `macos/AeroricKit/Tests/AeroricUIUnitTests/NativeKnowledgeReviewTests.swift`（新） | 入口矩阵、fixture约束、内存转换/取消/重置及实际组件装配测试 |
| `macos/AeroricUITests/NativeKnowledgeReviewUITests.swift`（新） | §6六个真窗口方法，逐一登记 |
| `scripts/native/ui-test.mjs` | 只向TESTS追加六方法，不改预热/清理/重试/环境机制 |
| `scripts/native/checks.json` | 按实测必要上调相关floor和相邻计数说明，不降floor、不缩filter/集合 |

共享Design/ThemeTokens、旧EvolutionReview及其测试、未来CollaborationReview、
Package.swift、project.yml/xcconfig、build/check/sign/release脚本只读。
生成工程只允许现有generate工具生成派生变化，不手改、不新增target。
正式shell、TaskWorkspaceView、业务/core/protocol、Tauri/React均不在白名单。

### 外部开工门槛

1. 用户在新终端明确要求执行本文后，按Trellis创建/复用独立
   `native-knowledge-review`子任务，package为aeroric-macos、
   parent为 `10-14-native-frontend-redesign`。先完成PRD/design/implement、
   按需context和最终审阅，再按审阅/start门槛只启动此子任务；
   不启动仍planning的整壳，不修改其他执行者的任务状态。
   无会话身份时报告，不手改元数据绕开。
2. main上保存开工status、白名单原文/diff/SHA-256到
   `.artifacts/native/knowledge-review/<run-id>/`，不用HEAD冒充脏树基线。
   冻结Q26/Q35/视觉执行包与知识/规划等既有快照不增补。
3. 取得App入口、UI登记及checks等共享文件的明确单写交接；
   视觉基础/进化样例有in_progress任务和dirty，能读不等于能覆盖。
   每次编辑共享文件前核对hash/diff，同期变化先协调。
4. **首次swift test/generate/build之前**取得§6构建产物独占窗口。
   先记针对性基线；再写值类型/安全fixture和入口单测、内存Store、
   原生View、DEBUG分流及UI测试登记。不从真实知识接线开始。
5. UI前另取得本次桌面授权和偏好域隔离条件；不能借其他任务的VM许可。
   无共享资源交接时只做独立/静态工作，回报阻塞，不抢跑。
6. 交回证据后停止，不继续真实引擎、技能、权限或其他候选。
   不stash/reset/clean/整文件重写。失败时保留诊断，只处理自己相对开工
   快照的具体hunk，先确认无后续协作者改动，不回滚整个文件或关闭用户App。

## 6. 验证、资源风险与必须证据

以下是**外部执行者后续要跑**的验证，本次文档交接未运行。
逐命令保存原始输出、退出码、passed/failed/skipped和逐方法结果。
0例不算通过；失败区分既有/本次，保留前面失败，不以最后成功覆盖；
不为修既有问题越过白名单、降低门禁或缩减回归集合。

### 构建资源与当前有效基线

文件单写不等于构建独占。首次测试/生成/构建前明确SPM `.build`、
core target、生成工程、固定DerivedData、Debug bundle/签名和运行实例的
归属；覆盖针对性基线、构建及UI核验，不改写运行中的bundle/helper。
UI还共享固定Caches scratch/预热目录、`ui-test-last.log`及gate日志。
无安全窗口就只做静态工作并报告，不用新worktree绕开交接。

2026-10-16静态配置：shell/native-design均跑整个AeroricUIUnitTests，
floor各277；swift-client unit/TSan/schema为33/28/2；UI登记21方法。
开工重新核对，不用旧包246/15或过时注释代替。
本次新增六方法后为**开工实际登记数+6**，不删除旧项凑数；
新增单测按实测必要提高两组floor/计数，不缩filter。

取得独占后先建立证据目录，以下命令**逐条串行**执行并保存各自日志：

```sh
RUN_ID="$(date '+%Y%m%d-%H%M%S')"
RUN_ARTIFACT="$PWD/.artifacts/native/knowledge-review/$RUN_ID"
mkdir -p "$RUN_ARTIFACT"
swift test --package-path macos/AeroricKit --filter NativeKnowledgeReviewTests
swift test --package-path macos/AeroricKit --filter NativeEvolutionReviewTests
swift test --package-path macos/AeroricKit --filter NativeVisualFoundationTests
swift test --package-path macos/AeroricKit --filter ThemePolicyTests
pnpm native:build
node --check scripts/native/ui-test.mjs
node scripts/native/check.mjs shell
node scripts/native/check.mjs native-design
node scripts/native/check.mjs swift-client
```

新测试尚不存在的开工基线要写“未存在”，不能报0例通过。
build会generate、cargo Debug、Xcode Debug、拷host、ad-hoc签名/验证；
runner仅bundle缺失才build，不能靠它自动刷新已有bundle。
这不等于样例启动host，也不授权改host源码或签名配置。

入口单测覆盖：新flag缺席保持旧链、存在的0/空/其他非法值、环境/目录
缺失及非法、Caches根/前缀碰瓷/相对/`..`/symlink、最终安全子目录未存在、
三种旧/预留flag分别冲突及组合。危险路径只测纯策略，不实际启动。
内存单测覆盖K1–K8，包括四种配置/授权组合、批量/草稿取消应用、例外、
按需引用差异、修订替代/停用、新上下文推荐与旧引用、ACK不等于纠正、
安全无关工作继续、重置与新Store；不能只断言静态文案常量。

### 六个真窗口方法必须逐一登记

类/文件名：`NativeKnowledgeReviewUITests`；登记前缀
`AeroricNativeUITests/NativeKnowledgeReviewUITests/`：

1. `testCandidatesBatchReviewAndDraftStayOffline` — K1，批量确认/丢弃、
   来源/范围、草稿取消/应用，只有内存变化。
2. `testAutoAdmissionAndFullAccessRemainIndependent` — K2/K3，默认关闭、
   四种组合、配置取消/应用、覆盖内免逐条问、三种例外仍留候选。
3. `testScopedReferencesRespectSourcesAndCurrentRequirements` — K4，
   精简参考/来源详情、当前要求优先、缺来源/过时冲突不冒充事实。
4. `testRevisionsRetirementAndAffectedTasksStayLocal` — K5/K6/K7，
   v1/v2/停用与后续推荐、旧引用仍标明，通知/ACK非更正、
   局部核对前后与安全独立文档继续。
5. `testNativeAppearanceKeyboardAndCompactLayout` — K8及真实操作，
   重置/退出重开、浅深色/glass/classic、辅助偏好实际消费、
   键盘/sheet/长文/三种实测尺寸。
6. `testInvalidKnowledgeRequestsNeverReachProduct` — 安全Caches路径下，
   错环境、新flag与其他三个flag分别/组合冲突时只显示拒绝页，
   不构造产品服务，也不清偏好。

用真实点击和可读AX断言结果，不以源码存在、fixture自报或恒定host=0替代。
helper用实际账户home而非sandboxed runner容器home，选唯一、原先不存在的
安全Caches子路径；读取已存在父目录核对子路径启动前后仍未创建，
不由helper先建目录掩盖写入。只清理归属明确的本次实例，失败也不例外。
新helper显式设knowledge=1，evolution/visual/collaboration=0，
控制各flag及TEST_RUNNER继承污染；不复制只设旧两个flag的private helper。
无新flag语义用纯入口测试证明，不为负向测试启动产品/不安全路径。

### 完整UI链的额外安全门槛

UI只走项目 `pnpm native:ui-test` 链；accessibility gate就是完整runner，
不另加重复整套UI、不发明不存在的筛选参数，也不运行runner来检查登记。
其文件守卫在预热/清理之后，且只能查漏文件，不能查同文件漏方法。

- runner删除固定scratch、终止匹配该路径的host，`open`预热普通产品App；
  随后以 `MacOS/AeroricNative$` 广匹配SIGKILL，重试还匹配
  `MacOS/AeroricNative|aeroric-cli serve`，没有进程归属锁。
- UI前只读核对匹配PID、完整命令、数据目录与归属，取得**本次**
  桌面/测试环境授权；有用户/其他终端/未知实例就不跑，不先kill腾位。
- **当前完整集合还会进入旧ReviewAppDelegate清偏好路径**。
  安全Caches目录、清flag或构建独占都不能隔离UserDefaults偏好域。
  在该路径仍存在时，只能在本次明确获准、无真实Aeroric配置的隔离测试
  账号/VM中跑完整UI；不得在用户日常账号冒险运行或顺手修旧delegate。
  若其owner另行修复，先复核修复和资源交接，不能据口头自报放行。
- 隔离测试域中的新样例/拒绝页要单独核对偏好域前后无业务写入/清除；
  只用无敏感测试标记/摘要，不读取输出真实用户偏好或凭据，
  不为验证在日常账号种植/删除配置。旧回归副作用与新样例证据分开记录。
- 预热/普通回归可能构造产品服务并启隔离测试host，只允许既有测试
  Caches/fixture，不导入真实数据、跑Agent或真实模型；无法保证就停止。
  不抢鼠标/焦点，不并发build/UI/App/serve，不重置TCC或改全局设置。
- check超时只杀直接child，不证明进程树已停；结束后核对本次实例与目录，
  只收拾归属明确的实例，记录未知/遗留，不碰无关进程。

满足以上条件后，清全部审阅flag及TEST_RUNNER转发再执行一次完整gate：

```sh
env -u AERORIC_NATIVE_KNOWLEDGE_REVIEW \
  -u AERORIC_NATIVE_COLLABORATION_REVIEW \
  -u AERORIC_NATIVE_EVOLUTION_REVIEW -u AERORIC_NATIVE_VISUAL_REVIEW \
  -u TEST_RUNNER_AERORIC_NATIVE_KNOWLEDGE_REVIEW \
  -u TEST_RUNNER_AERORIC_NATIVE_COLLABORATION_REVIEW \
  -u TEST_RUNNER_AERORIC_NATIVE_EVOLUTION_REVIEW \
  -u TEST_RUNNER_AERORIC_NATIVE_VISUAL_REVIEW \
  node scripts/native/check.mjs accessibility
```

无隔离测试域/安全时段就如实报告UI阻塞，不绕开runner或冒充验收通过。

### 隔离、视觉与Release证据

- K1–K8映射实际操作、预期/实际、截图/AX；含重置/重开、草稿取消/
  应用、材料接缝/语义色/裸图标、系统与注入偏好、长文和实测几何。
- 有效样例/拒绝不构造产品服务，用早分流源码、delegate依赖、进程树、
  安全目录和隔离测试偏好域前后相互佐证。无sidebar、目录未创建或
  恒定“未启host”不能单独证明没有host/RPC/配置副作用。
- 按现行系统与架构矩阵报告实际运行证据:实际OS/SDK/架构逐项记录,目标
  组合 Apple Silicon/macOS27 与 Intel/macOS26(无 Intel/macOS27);不可用
  组合明确记“未验证”,交叉编译不算 Intel 真机运行。历史 macOS14 运行证据
  缺失不是当前验收阻塞;现有构建配置不构成改 deployment、签名或 CI 的
  许可,不用单机编译冒充多系统运行通过。
- ReleaseLocal实际编译及有效build-settings核对App/相关包无DEBUG、
  新入口/fixture排除；配置名、源码有if或Debug全绿都不单独充分：

```sh
xcodebuild -project macos/AeroricNative.xcodeproj -scheme AeroricNative \
  -configuration ReleaseLocal -destination 'platform=macOS' \
  -derivedDataPath "$RUN_ARTIFACT/ReleaseDerivedData" \
  CODE_SIGNING_ALLOWED=NO -showBuildSettings > "$RUN_ARTIFACT/release-build-settings.txt"
xcodebuild -project macos/AeroricNative.xcodeproj -scheme AeroricNative \
  -configuration ReleaseLocal -destination 'platform=macOS' \
  -derivedDataPath "$RUN_ARTIFACT/ReleaseDerivedData" \
  CODE_SIGNING_ALLOWED=NO build
```

Release只编译/排除，不运行生产窗口、打包、改签名或发布。
交回本次已验证Debug bundle路径、机器/隔离账号说明和完整显式启动命令：
knowledge=1，其他三个审阅flag=0、env=ui-test、唯一安全Caches子路径。
命令只用于获准测试环境，不只说“直接打开App”或从普通业务路由进入。

## 7. 回报格式与监工停止点

保存 `.artifacts/native/knowledge-review/<run-id>/report.md`：

```text
# 项目知识原生审阅样例01：实施回报
## 用户执行指令、独立任务/main、共享文件及构建/桌面资源交接
## 开工快照、实际文件和相对快照patch、冻结边界
## K1–K8操作、预期/实际、截图/AX及失败/未完成
## 原生组件、键盘/辅助偏好/尺寸、系统与SDK、材质运行证据
## 入口矩阵、原链保持、专属delegate、目录/进程/隔离偏好域前后
## 原始命令与退出码、passed/failed/skipped、六方法登记和执行日志
## 完整UI普通App/host及旧清偏好路径的隔离、遗留实例/风险
## Release编译/有效设置/排除证据
## 已验证Debug bundle、测试机器和显式env启动命令、审阅顺序
## 已有失败、未验证、偏离计划及需用户/监工决定的问题
```

主线程按开工快照/diff、新鲜日志和实际组件/AX消费审查，再请用户看窗口；
执行者自报、文档一致性或截图好看不等于验收。
视觉通过也不证明真实采集、检索、入库、权限过滤、知识纠正传播、
模型消费或四家族适配可用。真实接线、技能/流程资产及其他候选继续单独设计，
不能由本执行包一并实施或排除。
