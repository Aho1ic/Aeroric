# 工作安排审阅样例 01 — 外部执行模型交接

版本：1.0 · 2026-10-19 · 范围批准：Q68。
设计依据：Q51–Q67，见 `work-arrangements-design-checkpoint-01.md` v0.1。
用户确认：“认可,但是注意，当前的项目要求是不创建VM”。

**只批准独立、可交互的SwiftUI＋苹果原生Liquid Glass离线样例范围。
当前项目不创建VM，App/UI执行仍暂停。本文不是立即开工或运行许可。**
用户明确将本文交给新终端并要求执行后，依§7取得独立任务/审阅/start及
资源交接，实施与经审查的非窗口检查；本线程只交文档、讨论设计和审查偏离。
本轮未实现页面、运行产品或接受布局/运行结果，不扩其他阶段包。

## 1. 不创建VM与UI暂停：开工之前的硬门槛

1. **不创建、克隆、导入、下载、恢复、启动、重建或准备替代VM；
   不安装/重装VM工具，不自动修复已退休VM路径，不继承历史VM运行许可。**
   VM不是本阶段实施、检查或交付条件，不为缺GUI环境提出新建VM作为解法。
   不维护、预订或重新占用已退休VM，不删除历史证据/退休收据。
2. 真实窗口/UI自动化继续PAUSED。不启动App、预热、运行Preview、
   XCUITest、`pnpm native:ui-test`、`pnpm native:check accessibility`、
   手动自动化、隐藏窗口或换Space/显示器绕过。不要调用runner来验证暂停。
3. 当前只允许经逐项审查的无窗口、无输入检查及compile-only，不因“unit”
   名称默认安全。使用虚构数据和临时fake-home；不读写真实业务/凭据/配置。
   编译、内存转换和离屏装配不能证明真实Liquid Glass、AX、键盘或窗口布局。
4. 窗口/交互/材质/生命周期运行验收保持**NOT RUN/DEFERRED**，不能变成
   green、skipped-to-pass或“用户已验收”。本阶段回报应允许这些明确未运行项。
5. 未来真实UI须新的明确授权、无VM方案和政策审查；本文件不授权账号/
   远程桌面/CI/权限/TCC操作，也不提前安排运行环境。恢复UI不等于允许建VM。

遇到不安全命令、缺依赖、共享资源或隔离不足时，保留证据并局部停止，
不是安装工具、强杀占用者、删锁、放宽检查或运行真实App的理由。

## 2. 目标、规范源与非目标

在**现有macOS App target**增加DEBUG显式审阅入口，演示待办、关联执行、
自动化规则、处理范围/授权、控制、变更、跳过/补一次与资源恢复的关系。
“独立”指交付边界，不新建App、target、HTML、WebView或组件系统。
交付应有真实View、可点击动作和有限内存转换，不是静态标题或`#Preview`；
但本阶段不得启动它，实际可用性保留待后续授权审阅。

仓库：`/Users/macbook/Downloads/同步空间/LYX/Aeroric`。开工必读：

- `AGENTS.md`、`.trellis/workflow.md`、`docs/native/ui-testing-policy.md`；
- `MIGRATION_PLAN.md`顶部Apple-only基线、
  `docs/native/adr/039-distribution-pivot-macos-ios.md` §7；
- `.trellis/spec/aeroric-macos/frontend/index.md`、
  `docs/native/adr/001-native-boundary.md`；
- `docs/native/work-arrangements-design-checkpoint-01.md` v0.1及本文；
- `.trellis/tasks/10-14-native-redesign-shell/research/harness-work-arrangements.md`
  Q51–Q63；
- `.trellis/tasks/10-14-native-redesign-shell/research/harness-execution-readiness.md`
  Q64–Q68。

当前产品基线最低macOS/iOS/iPadOS26，Mac运行目标为Apple Silicon/macOS27、
Intel/macOS26，不建立Intel/macOS27组合；本样例不新增iOS业务。
Package/project/旧spec的14配置不覆盖新基线，亦不借本阶段修改deployment、
签名、CI或双商店工作。构建成功不证明平台迁移或任一真机运行验收。

总库仍10来源、89条观察＋U01；只回指C04/C05/C07、P05/M06/L08等既有观察
及其限制，不重扫harness、不运行/复制参考项目，不新增“来源已经实现”断言。

### 明确不做

- 不连接真实Agent/模型/API、MCP、PTY、反思/治理进程、host/网络服务；
  不导入或构造AppServices、生产Store、`AeroricClient`、UDS连接或执行器。
- 不读写真实项目、Git、会话历史、待办/规则/账本、权限、凭据、
  UserDefaults/偏好域、CLI配置或managed-home；不打开文件/URL/外部应用。
- 不安装/加载/执行技能或VM工具，不生成真实prompt、Agent argv或系统任务。
- 不实现真实调度器、队列、定时器/后台轮询、权限/预算/恢复引擎、锁/租约、
  持久化、数据库迁移、生产DTO/schema、四家族接线或外部副作用回放。
- 不改正式首页/主导航/TaskWorkspace、Rust core/host/protocol、共享业务owner、
  OMP原始PTY/TUI/managed-home/历史根，不恢复rpc-ui，不读用户`~/.omp`。
- 不改既有Review和冻结的Q26/Q35/Q45/Q50、视觉基础阶段包、设计快照或MiMo报告。
  历史执行包的UI/VM流程不继承到本阶段。
- 不建分支/worktree/clone、commit/push/tag或发布，不覆盖/回滚他人dirty。

## 3. 建议的原生信息层级

以下只安排样例，正式产品命名/首页/布局仍供用户审查：

1. **顶部**：虚构项目名与始终可见“离线工作安排示例 · 全部为虚构数据”、
   场景菜单、重置及窗口内外观预览，不放假在线、真实计费或成功仪表。
2. **工作列表**：待办/关联执行/自动化规则按系统选择态组织，默认选有内容项；
   简要展示来源、当前事实、阻塞及下一步。无需堆满空检查器或仿codeg皮肤。
3. **主要详情**：目标与相关执行、规则范围/策略、差异/影响、核验记录。
   复杂说明按需展开，处理范围预览与批量选择用原生详情或sheet。
4. **上下文区**：当前在等什么、规则依据、资源状态/完整性和事件记录，
   与当前选择关联；独立工作仍有自己的状态，不把整个项目画成统一受阻。

可调整横纵排版，不强制三栏；初始1260×860pt、兼顾1000×640及400×600。
使用系统窗口，不自绘无边框外壳或私有窗口圆角。这里只写目标尺寸，
当前不运行窗口、不声称实测；真实几何和窄窗可达性均待后续授权。

### 动作与事实必须分开

- “新建/保存（示例）”只改内存记录；不自动等于交办/启用/运行。
- “交给Agent（示例）”“确认并启用（示例）”只记录模拟意图/策略。
  有效覆盖内的后续模拟操作不重复弹同一许可，但并非取得任何真实权限。
- 实际触发、原执行结果、停止回执、资源释放、用量报告、监督/检查结果，
  只能由明确的**“载入示例事件/记录”**呈现，不能装成从本机采集到的事实。
- 请求停止、未知、已核实安全收束分别呈现；运行结束、技术完成、
  用户验收分开。新技能人工同意不被Full Access绕过。
- 消耗显示“示例预估/示例已报告/未知”，缺值不归零；数字只是fixture，
  不作为真实账单或本机API/模型能力结论。

这不是要求生产流程每个事件由用户点按钮，而是避免有限演示冒充真实执行。

## 4. 八组离线场景与必须可观察的差异

W1–W8是样例场景编号，不是来源ID、wire状态或新增八个产品模块。
实现有限值类型和fixture转换，不建立通用规则解释器或生产状态机。
必须有实际Store方法、View消费和可见前后差异，不只修改标题/恒定“通过”。

### W1 目标、来源与完成依据（Q51/Q52/Q56）

新建待办后只出现记录；至少一条关联两次不同结果的执行，旧记录不被重试
覆盖。来源可区分“用户（示例）”和“获准Agent/来源任务（示例）”，都在同一列表。
保留无需待办的直接执行示例，不要求每次定时运行先造待办。

载入“执行结束”记录，目标仍可缺整合、检查或适用监督；载入满足整体目标的
核验记录后才显示“技术完成（示例）”，用户验收仍单独显示。
可显式“记录用户验收（示例）”，不把技术完成自动当用户已签字，
不将失败、停止/未知或缺监督计为成功。

### W2 明确交办、范围与存量（Q53/Q54/Q57/Q58）

默认只记录。选择一项/一批“交给Agent（示例）”与启用范围规则分开。
自动接单启用前展示虚构项目/清单/标签等有限条件、执行配置别名、
允许操作和资源说明、当前匹配/排除及未来新增边界；支持单项排除和取消。
规则“启用”与有限授权/Full Access示例独立，不从某次执行继承。

首次启用默认不接旧待办；另可批量纳入用户明确选择的合条件旧项。
载入范围内合条件“新增事件”后可模拟接单，不为覆盖内每项重复确认；
排除、仅记一下/目标待澄清、未授权/未知或已有在途事实的条目不被偷接。
预览与筛选不授运行权；Agent来源也不天然获准或被一律排除。
取消批量/范围编辑不应用草稿，不实现任意筛选语法或后台扫描。

### W3 暂停新启动与停止在途（Q55）

预置一个在途、一个尚未启动的相关执行。
“暂停接单（示例）”挡本规则新启动，仍合规的在途不因此被打断。
“停止本规则工作（示例）”先挡新启动再进入请求停止/待核实，
不得点击即报“全部已停止”。可载入未知回执或已核实安全收束的示例记录。

保全历史与成果；不呈现“停止即回滚外部操作”。用户暂停/Stop后，
事件、重试、原执行结束或补额不自行恢复。
明确恢复只恢复该范围意图并重核，不自动复活已跳过的触发或未知外部操作。
本场景不实现真实停止级联、超时强杀、回执或撤权传播协议。

### W4 规则差异与在途依据（Q59/Q60）

可编辑规则草稿、查看变更前后及影响、取消或明确应用。
后续启动按当前有效依据重核；在途仍显示原目标与配置/API别名，
不因规则改了静默换模型/API、验收或整批重启。
扩围/提权的差异须明确，规则Full Access不继承其他任务授权。

载入“撤权/显式禁令/宿主硬限制”示例时，不以“在途沿用旧版”继续不被允许的
操作；Stop优先且停止事实仍可未知。用固定安全别名，不读模型列表/API配置。
定时规则可共用策略审阅与控制原则，但触发类型独立，
保存、确认启用、到点事件与启动不是同一事实。

### W5 已核实漏跑与恢复补一次（Q61）

以有限按钮载入3次已核实“错过且未启动”和“恢复可用”事件。
默认只记录跳过，不补3次；另可预先明确开启本规则“恢复后补一次”，
当前条件满足时仅出现一次面向当前状态的后续检查。
历史3次仍未运行，不显示各期成功；需要逐期处理的内容仅解释未定策略。

已有在途/失联/副作用未知不能当“确定漏跑”重放。
暂停/Stop/未启用不因恢复事件自行启用，缺授权/资源不假装已运行。
不使用真实时间、通知、cron、后台轮询或唤醒/网络检测。

### W6 繁忙跳过与结束后补一次（Q62）

上一轮在途时载入多次新到点，默认显示繁忙跳过，不打断原运行或排历史长队。
“结束后补一次”是与W5独立的设置，不能隐含继承；明确开启后，
同段繁忙最多保留一次面向当前状态的意图。

手动“立即运行（示例）”、定时与恢复补一次均不能绕同规则不重叠。
停止中/未知不判空闲；载入原执行与相关占用已核实安全收束事件后，
仍检查当前意图/规则/授权/依赖/资源，暂停后不自行继续。
不额外要求用户先验收上一轮，但适用监督/依赖仍须满足。

### W7 局部冲突、等待重核与独立工作（Q63/Q64）

用虚构共享修改点说明两项工作冲突等待，另一独立文档工作仍能推进。
展示在等什么及可披露的依据，不显示越权项目细节。
未知、超时、主进程退出或停止请求不能自动视为资源已安全可用，
不抢占/强杀/删锁，也不创建工作副本/worktree/VM。

已有明确交办且允许等待的工作，在载入安全释放并满足现行条件后，
覆盖内自动继续，不每次再问；若输入/接口示例已变化，只对齐相关步骤，
不盲套旧补丁/重做全项目。已取消、已由其他成果解决或意图/结果未知先核实。
不能将W5/W6已跳过机会转成等待队列，不因释放产生新重试权或恢复用户Stop。

### W8 共享护栏、到限类型与补额续行（Q65–Q67）

展示规则/任务局部与项目/共同边界，关联委派/反思/治理/重试/补一次消耗，
同一笔不重复累计；全为固定虚构记录，不连接计量服务或实现预算账本。
包括预估、已报告和未知，以及“必需硬限制在本情景不可可靠执行→受阻”。
Full Access不免资源，预算有余也不产生新权限或技能安装同意。

- 载入并发占满/尝试机会用尽：只挡相关新准入/下一次重试，
  不因此终止最后一次仍合规的获准尝试。
- 载入对在途生效的消耗/时长边界到限：挡扩张、请求安全收束、保全成果，
  未核实仍显示收束中/未知，必需监督未完仍待核对。
  独立且未命中限制的工作继续，不借“收尾”新开付费调用/无限跑完。
- “补额（示例）”仅恢复对应条件，默认不重启到限收束的旧执行；
  可预先单独开启范围自动续行，或明确“补额并继续（示例）”。
  先核对原执行/副作用、当前目标/依赖/权限与全部适用上限，
  原占用未知不启动重叠替代、不重放已完成/未知外部操作。
- 普通获准等待仍按W7自动继续；不因补额清零独立时长/重试上限、
  恢复用户Stop/暂停、复活跳过机会或让缺监督通过。
  未暂停规则的合法新触发仍按原准入重核，不与重启旧执行混同。

重置/切换场景应清楚说明“载入初始示例”，而非恢复真实进程。
所有编辑、选择、事件、外观与同意都只在本窗口内存；
关闭重开/退出重启预期恢复fixture，但当前仅验证可测的内存部分，
真实窗口生命周期运行验收仍DEFERRED。

## 5. 原生装配、内存owner与入口隔离

### 原生样式与辅助偏好

- 实际复用 `NativePanelFoundation`、`GlassPanel`、`NativePanelGeometry`、
  `NativeIconButton`及共享NativeSelection，不另造主题系统。
  连续材质基底覆盖圆角空隙，不贴三角补丁；沿用恰当面板连续圆角/间距。
- SwiftUI/AppKit真实Material；26+交互控件用苹果原生Liquid Glass，
  沿用availability门控，不用低透明颜色假玻璃，不给整个内容pane套glassEffect。
  正文/来源/差异长文用语义不透明阅读面。
- 沿用A样式：短按钮/输入/选择项圆润胶囊，独立图标默认无永久圆环；
  不把整窗/长文药丸化，不覆盖系统选择态或裁切焦点/IME。
- 邻接互斥项用系统Picker/List/Menu/Toggle及NativeSelection；
  不手绘active背景/下划线。共享变体不足先报告，不复制或越白名单改Design。
- 使用语义accent/success/warning/danger配文字/图标，不照截图RGB，
  不只依赖颜色或堆灰。glass/classic、interactionMode和权限相互独立。
- 减少透明度/增强对比/减少动效必须消费到实际View；内存注入只加强系统偏好，
  明示系统值/演示值，不改系统设置或用`@AppStorage`持久化。
- AX identifier只放叶子，统一`workArrangementsReview.`前缀及可读名称。
  设计保留Tab/方向键/Escape、原生sheet取消/确认和窄窗滚动；
  真实键盘/VoiceOver/材质效果不可在本次声称已验证。

### 单窗口内存owner

样例Store为`@MainActor @Observable`，由样例根持有，每次新窗口新建；
值类型/fixture、编辑草稿、选择、示例事件和资源数均局部内存，不导出到业务。
取消不应用，重置恢复fixture；不使用单例、持久化、后台Task/Timer、
真实clock/scheduler、网络、进程或业务回调。
内存有限转换只解释已确认关系，不能作为生产权限或资源安全保证。

### DEBUG显式入口契约（尚未实施）

新增键 `AERORIC_NATIVE_WORK_ARRANGEMENTS_REVIEW`。全部新源/fixture/Store、
入口、拒绝页与专属生命周期整体DEBUG限定。
在 `AeroricEntryPoint.main` 最早DEBUG分支，先于当前
Skill → Knowledge → Evolution → Visual → product链，进行只读决策：

| 请求 | 决策 |
|---|---|
| 新键缺席 | inactive；原链语义不变，不重排、不解释/修复旧键 |
| 新键为`1`，`AERORIC_ENV=ui-test`，显式`AERORIC_DATA_DIR`通过既有严格目录helper，其他审阅键无`1` | 仅新样例，先于产品App/delegate/services构造 |
| 新键存在但`0`/空/非法，或环境/目录不符，或任一其他审阅键为`1` | 新只读拒绝页，绝不回落产品或按顺序挑另一样例 |

其他审阅键：`AERORIC_NATIVE_SKILL_REVIEW`、
`AERORIC_NATIVE_KNOWLEDGE_REVIEW`、`AERORIC_NATIVE_EVOLUTION_REVIEW`、
`AERORIC_NATIVE_VISUAL_REVIEW`、`AERORIC_NATIVE_COLLABORATION_REVIEW`。
新键存在时其他键`0`不冲突；缺席不等于赋值`0`。
协作当前只是冲突预留键，不因列入就承诺其已有入口或实现。

复用 `NativeVisualReviewEntry.isIsolatedDataDirectory` 的只读Caches子目录
检查，可注入fake-home；不造宽松prefix、不创建数据目录，不回显完整路径/
环境/密钥。它不是OS沙箱、偏好域隔离或权限授予。
`ui-test`只是入口上下文标签，**不是绕过UI暂停的许可**。

样例和拒绝页均用新的无业务副作用生命周期及唯一窗口标识，仅管理自身
不恢复状态的标准窗口；不复用产品delegate/业务菜单、全局窗口扫描或焦点补丁，
不清UserDefaults、不激活/关闭其他实例。关闭丢弃根/Store，重开新建。
应用业务代码不写偏好；不能由此承诺绝无OS窗口系统持久痕迹，运行证据仍未取得。
当前只实施/静态核对这些路径，不调用窗口显示方法或启动App来试验。
ReleaseLocal排除全部新实现并走原产品链；仅compile-only/产物检查，
不为检验flag启动Release。

## 6. 产品/测试文件白名单

以下是最大范围，不要求全改；新增文件开工时若已存在，先核对owner，
不能覆盖别人的成果。类型可在这5份新源内拆小，不另造目录树：

| 路径 | 允许职责 |
|---|---|
| `macos/AeroricKit/Sources/AeroricUI/WorkArrangementsReview/WorkArrangementsReviewModels.swift`（新） | 仅样例值类型、来源/目标/执行/规则/资源事实，不定义生产协议 |
| 同目录 `WorkArrangementsReviewFixtures.swift`（新） | W1–W8安全虚构内容与有限事件/记录 |
| 同目录 `WorkArrangementsReviewStore.swift`（新） | 窗口内存状态、有限转换、编辑取消/重置 |
| 同目录 `NativeWorkArrangementsReviewView.swift`（新） | 原生装配、详情/sheet、View消费与交互 |
| 同目录 `NativeWorkArrangementsReviewEntry.swift`（新） | 只读决策、拒绝页、专属无业务窗口生命周期 |
| `macos/AeroricApp/Sources/AeroricApp.swift` | 仅最早DEBUG新分流/新样例App装配，保留旧链和产品逻辑 |
| `macos/AeroricKit/Tests/AeroricUIUnitTests/NativeWorkArrangementsReviewTests.swift`（新） | 经审查的无窗口入口/内存/安全装配检查，不显示窗口 |

任务私有PRD/design/implement、context及本次证据另按Trellis管理；
不改整壳/迁移任务metadata/context，不把总库自动注入每个执行Agent。

**只读/不在白名单**：共享Design/ThemeTokens/NativeSelection、所有既有Review/
测试、`Package.swift`、`project.yml`、xcconfig、业务/core/protocol、iOS、
`macos/AeroricUITests/`、`scripts/native/`（含UI清单/checks/floors/runner/policy）、
签名/CI及个人配置。不得降低门槛、删用例或为扩大白名单顺手改这些文件。
生成Xcode工程仅允许现有generate工具的派生产物，不手改、不新增target。
若必要的非窗口验证/接入确实需要额外hunk，先回主线程审查最小扩围，
不以文档中的一般构建说明作为修改权限。

## 7. 新终端开工顺序与共享资源

1. 用户明确交本文要求执行后，先按workflow取得任务创建/规划同意，
   创建/复用独立有边界的`native-work-arrangements-review`任务，
   package为aeroric-macos，parent为 `10-14-native-frontend-redesign`。
   完成独立PRD/design/implement、对应context、安全命令清单与审阅，
   再按实施确认/start门槛仅启动子任务。本文不替代用户同意，
   无会话身份就报告，不手改metadata或激活整壳任务绕过。
2. main上保存开工status、白名单实际原文/diff/SHA-256及缺席文件记录至
   `.artifacts/native/work-arrangements-review/<run-id>/`。
   以当时dirty树为基线，不用HEAD冒充，不stash/reset/clean或全文件覆盖。
   冻结所有既有阶段包/快照及MiMo研究，本文和设计快照本身也不由执行者改。
3. 先取得App入口等共享文件的明确单写交接；每次编辑前重核hash/diff，
   同期改动先协调，不将他人成果纳入自己的通过声明。
4. 编译/单测前核对工具与依赖已在本地、可用且冻结，审查命令及继承环境。
   不自动安装/下载/更新依赖或工具，不调用新VM、真实配置/账号或网络服务补缺。
   缺依赖/工具只报告；修改HOME不等于完整隔离，fake-home须显式传入测试API。
5. 取得SPM `.build`、core target、生成工程、固定DerivedData、Debug bundle/
   helper/签名产物的独占交接。存在用户/未知App或其他执行者持有产物时，
   不重写运行中bundle，不强杀/删锁，不用新worktree绕过；相关编译DEFERRED。
   不为等待资源而创建/预订VM或桌面运行窗口。
6. 先写入口和值类型/fixture的无窗口测试，再内存转换、View、最小App分流。
   逐项检查新suite不会显示窗口/调用NSApplication.shared、初始化生产服务/
   偏好Store、启动进程/socket、读真实home。只构造未显示的生命周期策略对象。
7. 按§8分别记录编译、定向单测与静态证据；本阶段不添加/运行真实UI用例、
   不注册UI清单、不碰runner/floor。需要额外回归先列出实际副作用供审查。
8. 交报告与相对开工快照patch后停止，等待主线程审查，不继续真实引擎、
   正式首页或真实UI。失败只处置自己仍有owner的具体hunk，
   先确认无后续协作者修改；不整文件回滚、不删历史证据。

## 8. 验证清单：当前仅允许经审查的无窗口检查

以下是**外部执行者后续候选检查**，不是本次已运行命令或产品通过记录。
每条执行前仍审查当时源码、环境、依赖、资源与实际目标；
命令名称、已有通过数和测试floor都不能替代安全核对。

### 8.1 静态交接核对（2026-10-19，只读）

- `AeroricApp.swift` 当前在构造产品之前进行Skill → Knowledge → Evolution →
  Visual分流；产品`AeroricNativeApp.init`会构造AppServices。
  新分支须置于其前，不能在产品View内加“离线”按钮后仍初始化真实服务。
- 当前Skill Entry有只读策略、专属生命周期、缺席inactive/存在非法拒绝模式；
  目录helper可注入home且只读元数据。复用模式/公共helper，不修改旧Review。
- 当前 `build.mjs` 是generate、cargo Debug、Xcode Debug、拷helper、开发签名/
  bundle核验链，静态未见启动App/serve；会改共享产物，不是无副作用命令。
  开工须核对脚本、构建hooks及当时产物owner，开发签名不等于商店签名/验收。
- `shell`/`native-design`当前unit case跑整个`AeroricUIUnitTests`（floor411），
  含离屏View和测试偏好域写入，不是仅新suite；**不能默认运行整组**。
  `swift-client`含临时UDS及Python假宿主进程，不能写成“无服务”检查。
  `check.mjs`继承环境并写固定日志，不提供新suite专用过滤。
- doctor静态包含工具探针和邻接DBX的git状态，不自动列为必跑；
  工具shim是否会下载安装、邻接读取是否获准，须当时核对。
- 本机仅查询过Swift test/Xcodebuild的`--help`，支持下列依赖冻结参数；
  没有运行构建/单测/产品。这些参数不保证离线，缺本地依赖仍可能请求下载，
  不把它们当网络沙箱。开工先确认缓存完整；无安全离线条件就不跑。
- frontend spec index指向的`native-ui.md`当前缺失；不创建占位或顺手修规范，
  以可读index、AGENTS/ADR和当前基线核对，冲突/不足交主线程决定。

### 8.2 入口、转换与实际View消费

定向新suite至少覆盖：

- 新键缺席，旧键各种值/组合均不被新决策接管；存在`0`/空/非法拒绝，
  错环境、缺/非法目录拒绝，不回落产品。
- 目录用临时fake-home：Caches根/相对/点段/前缀碰瓷/符号链接拒绝，
  合法未存在子目录接受但不创建，父目录前后清单不变。
  不对真实home/配置/权限做负向破坏或调用实际App。
- 五个其他审阅键分别冲突及31种非空组合，新键存在时其他键`0`不冲突。
  用纯决策/源码接入核对证明旧链不变，不为测试缺席而启动产品。
- W1–W8真实Store方法和前后投影：取消/批量/排除/启用、有限授权/Full Access、
  目标核验、暂停/请求停/未知、在途依据、独立补一次、局部等待和变化对齐、
  到限分类、普通等待/到限续行分开、补额不复活Stop/跳过/独立上限。
  测失败/未知/未答分支，不只用固定成功记录或自证常量。
- 预估/报告/未知、同笔不重复算、能力缺口/缺监督待核对。
  重置与新Store恢复fixture，各实例不共享草稿/同意/事件。
- 静态核对真实View消费这些状态与动作、原生选择/语义色/辅助偏好，
  以及App分流、DEBUG/Release排除和依赖边界。
  若做离屏装配须另逐项审查，不显示/创建NSWindow、不触发生产初始化/
  NSApplication.shared/输入/服务；无法确认则该项DEFERRED，不为“装配”启动App。

这证明的是有限样例行为和静态接入，不是生产调度/权限/恢复引擎安全。

### 8.3 条件满足后可采用的命令

先建立独立日志目录，逐条保存完整命令、环境差异（不含密钥）、退出码、
实际passed/failed/skipped。suite尚未存在记“未存在”，零匹配不算通过。
以下不得一键盲跑，需先满足§7、§8.1及新suite逐例安全审查：

```sh
RUN_ID="$(date '+%Y%m%d-%H%M%S')"
RUN_ARTIFACT="$PWD/.artifacts/native/work-arrangements-review/$RUN_ID"
mkdir -p "$RUN_ARTIFACT"

# 依赖本地已齐且无自动下载/更新；只执行新suite，不显示窗口。
swift test --package-path macos/AeroricKit \
  --disable-automatic-resolution --skip-update \
  --filter NativeWorkArrangementsReviewTests

# 既有原生工具链，先核对hooks/共享产物；只编译/开发签名，不launch。
pnpm native:build
```

默认SPM缓存/SPM `.build`仍是共享资源；依赖锁定参数不隔离缓存或保证无网络。
可在独立任务中审查scratch/cache离线方案，但不得自动下载或复制真实业务数据。
定向单测可能编译整个package，编译范围不等于执行所有suite；
其他suite/整组gate的实际运行与否必须单列，不伪称已全回归。

复用现有generate工具后，ReleaseLocal只compile-only核对新实现排除：

```sh
xcodebuild -project macos/AeroricNative.xcodeproj -scheme AeroricNative \
  -configuration ReleaseLocal -destination 'platform=macOS' \
  -derivedDataPath "$RUN_ARTIFACT/ReleaseDerivedData" \
  -disableAutomaticPackageResolution -onlyUsePackageVersionsFromResolvedFile \
  CODE_SIGNING_ALLOWED=NO -showBuildSettings
xcodebuild -project macos/AeroricNative.xcodeproj -scheme AeroricNative \
  -configuration ReleaseLocal -destination 'platform=macOS' \
  -derivedDataPath "$RUN_ARTIFACT/ReleaseDerivedData" \
  -disableAutomaticPackageResolution -onlyUsePackageVersionsFromResolvedFile \
  CODE_SIGNING_ALLOWED=NO build
```

不运行Release/Debug App、`swift run`、Xcode Run、Preview、GUI snapshot或UI runner。
检查有效编译设置和本次产物符号/链接，不以源码`#if`或配置名字代替排除证据；
无法可靠确认则报告未验证，不启动Release验证flag。

### 8.4 必须保留的未运行项

真实按钮/菜单/sheet/键盘/焦点/IME/AX/VoiceOver、浅深/glass/classic材质接缝、
辅助偏好的真实输出、窗口尺寸/重开/退出重启、Intel与Apple Silicon真机旅程
都保持NOT RUN/DEFERRED。本次不得登记虚假UI通过或截图；
View存在、单测数、方法注册和compile-only不是这些验收的替代品。
不降低既有floors/用例，不因政策暂停宣告首发、双商店或真实引擎已完成。

## 9. 回报与监工停止点

保存 `.artifacts/native/work-arrangements-review/<run-id>/report.md`，至少：

```text
# 原生工作安排审阅样例01：实施回报
## 用户执行指令、独立任务审阅/start、main与文件/构建资源交接
## 开工dirty原文/hash/diff、实际文件、相对快照patch及同期他人增量
## W1–W8实现和View消费、预期/实际、失败/未知与未覆盖项
## 创建/启用/授权/触发/执行/技术完成/用户验收的分离
## 暂停/停止、规则变更、两类补一次与等待/补额续行边界
## 入口矩阵、旧链保持、fake-home/目录只读、无业务依赖/写入证据
## 原始命令/退出码、passed/failed/skipped、真实执行suite及编译产物
## Debug/Release有效设置、排除证据、实际OS/SDK/架构和平台缺口
## 无VM执行声明；App/Preview/UI未运行，真实交互/材质/AX等DEFERRED
## 共享资源/依赖阻塞、既有失败、偏离及需主线程/用户决定的问题
```

主线程按开工快照、本次diff、实现/真实View消费和新鲜日志审查。
自报、文档一致或单测通过不等于用户验收。交回源码位置及已编译产物
（如实际生成）时标“未启动/未经UI验收”，**不提供立即launch命令**，
不让用户“直接打开App”绕过当前暂停。
如缺编译条件，可交局部成果与明确阻塞，但不得声称实现/验收全部完成。

下一步只由用户决定是否明确交新终端执行；本线程不实施、不创建/激活任务，
不运行产品/App/UI/VM/真实Agent/API，不安装技能，不提交推送。
