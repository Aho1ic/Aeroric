# 技能审阅样例 01 — 外部执行模型交接

版本：1.0 · 2026-10-18 · 批准依据：Q50 用户回复“认可”。
对应设计：Q46–Q49；参考既有候选：M01、C02、X08、N07及各自限制。

**已确认的是独立、可交互的SwiftUI＋苹果原生Liquid Glass离线技能审阅
样例范围，不是正式布局、运行验收或真实技能引擎/安装机制批准。**
用户将本文明确交给新终端并要求执行后，执行者才依本文与Trellis门槛实施。
本线程仅设计、交接、监工/审查，本轮没有实现页面、安装技能或运行产品。
本文不是前端阶段02，不增补Q26/Q35/Q45及视觉基础执行包。
`skill-reuse-design-checkpoint-01.md` v0.1保持历史原文，覆盖Q46–Q49；
其中Q50待确认是交付时状态，本次范围确认以本文和持续决策页为准。

## 1. 目标、规范源与非目标

在**现有Aeroric原生App target**中增加DEBUG显式入口，供用户点击体验：
草稿审阅/修改/丢弃，新技能人工同意/拒绝，项目范围、任务按需选用、
可用性阻塞，版本差异与在途版本依据。全用窗口内存fixture，重置/重开复原。
“独立”指交付边界，不另建App、HTML、WebView或一套组件系统。
静态截图、不可点击稿、`#Preview`或仅测试常量不算交付。

仓库：`/Users/macbook/Downloads/同步空间/LYX/Aeroric`。必读：

- 根 `AGENTS.md`、`.trellis/workflow.md`；
- `MIGRATION_PLAN.md`顶部Apple-only基线与
  `docs/native/adr/039-distribution-pivot-macos-ios.md` §7；
- `.trellis/spec/aeroric-macos/frontend/index.md`、
  `docs/native/adr/001-native-boundary.md`；
- `docs/native/skill-reuse-design-checkpoint-01.md` v0.1全文；
- `.trellis/tasks/10-14-native-redesign-shell/research/harness-skill-reuse.md`；
- 本文及交接时点设施核对
  `.trellis/tasks/10-14-native-redesign-shell/research/skill-review-handoff-audit.md`。

**现行平台要求：最低macOS/iOS/iPadOS 26；Mac验证目标为
Apple Silicon/macOS27、Intel/macOS26，不建立Intel/macOS27组合。**
本阶段只做Mac审阅样例，不复制iOS业务。当前Package/project及旧spec中的14
是配置/规范差异，不覆盖现行基线；不顺手改deployment、签名、CI或冻结历史。
编译通过不证明平台迁移、双商店或两架构运行验收。

总库仍是 `docs/native/harness-feature-candidates.md` 的10来源、
89条观察＋U01，不是90个已批准模块。不重扫harness、不运行参考项目、
不复制其UI、技能文本或实现，不声称来源具备完整安全技能生命周期。

### 不做什么

- 不生成、下载、安装、加载或执行真实技能，不写真实`SKILL.md`、技能根、
  注册表、CLI个人配置、managed-home、数据库、任务或权限/知识配置。
- 不启动真实Agent/模型/API、MCP、PTY、反思/治理进程、网络服务或安装器；
  不导入/调用AppServices、生产Store、`AeroricClient`、UDS或真实回源。
- 不读取真实项目/历史、技能、用户资料、凭据或生产配置。来源只展开
  内嵌虚构摘录，不打开文件、URL、浏览器或外部应用。
- 不实现提炼器、分发协议、安装事务、依赖下载、版本服务、安全扫描、
  四家族适配器、持久化或撤销传播。有限fixture转换不是生产状态机/DTO。
- 不让内容确认自动等于安装/启用，不把Full Access当新技能同意、
  权限提升或技能扩围。技能不能改审批、验收、宿主硬限制。
- 不改U01任务副本工具候选的验证、安全点受控试用、治理及回退规则；
  不把普通技能更新审阅变成U01所有工具试用都要人工签字。
- 不改正式首页/主导航/TaskWorkspace、业务owner、OMP原始PTY/TUI或历史根，
  不恢复rpc-ui；不修改Rust/protocol、Tauri/React或外部workflow所有权。
- 不建分支/worktree/clone、不commit/push/tag或发布，不回滚他人dirty。

有效样例和拒绝页都必须先于产品App/delegate/services构造进入；
其自身不启host。入口仅做已有只读目录边界检查，不创建样例数据目录。
完整UI runner另会预热普通App，不能把完整回归说成全程无服务/副作用，见§6。

## 2. 信息层级与可交互边界

沿用已确认A样式与现有原生组件，以下是样例层级，不冻结正式技能导航：

1. 顶部：虚构项目名，始终可见“离线技能审阅 · 全部为示例”，场景菜单、
   重置及窗口内外观预览；不放假在线/已安装成功仪表。
2. 主要列表：草稿/安装申请/项目技能/更新等按系统选择态组织，默认选有内容项；
   显示需要用户决定的事项和阻塞摘要，避免大片空白检查器。
3. 详情：方法内容、来源/适用条件/验证要求、批准对象与范围、版本差异；
   按需展开，不把整库或所有字段堆为大表格。
4. 当前任务/工作包：只展示相关技能、已选版本及可用性，解释为什么不能选；
   与项目“统一管理/已启用”分开。安装/更新申请用原生sheet或详情审阅。

可以优化间距、折叠、横纵排版，不强迫三栏。初始1260×860pt，另核对
1000×640、400×600；窄窗菜单/纵向滚动保持动作可达。记录自身窗口实测
外框与内容尺寸，屏幕不足如实标注，不缩放截图冒充目标尺寸。

### 状态必须分开，不能都写“已成功”

样例值类型至少区分以下事实；具体字段名只属于样例，不确定生产schema：

- 内容草稿待审/已确认/已丢弃；
- 安装申请未答/已拒绝/用户同意了所展示内容、版本及项目范围（示例）；
- 安装结果演示尚未载入/预置成功或失败记录；项目启用状态；
- 当前执行者可用/不可用/未知及原因；本任务是否选用、选用版本；
- 是否存在实际加载/消费证据：本样例始终**无真实加载/消费证据**。

例如“已同意（示例）”不自动显示“已安装/已加载”。可用性只能标为
“预设情景可用”，不是本机兼容性检测；模拟选用不构造prompt或发送指令。
所有列表、详情、sheet与反馈都明确示例语义，不能只在页脚加一次免责。

### 必须能操作

- 打开来源/适用条件/验证要求，修改草稿，取消不应用、应用只改内存；
  丢弃后能看到区别。单独确认内容/范围不产生安装或执行。
- 新技能申请呈现来源/内容/示例版本、拟用途、当前项目范围和能力/依赖说明；
  用户可同意、拒绝或暂不答。已同意对象与显示内容必须一致，修改后不能
  沿用不匹配的旧同意；这不选定生产签名或哈希协议。
- “演示安装结果”只载入有限记录，未答/拒绝时不能载入成功记录；
  同意后仍区分预置成功/失败/未知可用性，不能把安装成功当消费或操作成功。
  批准已涵盖同范围安装和使用时，不另弹重复“使用批准”。
- 在虚构项目内切任务/工作包，手动选用或“演示Agent按需选用”已获准/
  启用/预设可用且相关的技能，不能整库勾选或选上未知/阻塞项。
  查看另一个虚构项目时明确未获准，不自动扩围；可展示拟扩围说明，
  本阶段不做真实全局启用或完整跨项目管理器。
- 查看更新候选及差异，同意/拒绝/暂不答可区分；载入获准新版本示例后，
  新任务用项目获准版本，在途仍可见旧版依据。旧版阻塞用独立预置记录演示，
  不偷偷换版/重启，不宣称清除已消费上下文。
- 重置、关闭再打开窗口、退出重启都恢复fixture；不复用上次Store或草稿。
  外观预览及“默认/Full Access示例情景”只改内存且相互独立。

## 3. 八个离线场景与验收差异

S1–S8仅为本样例场景编号，不是新候选ID、wire枚举或协议。每个场景须有
一致的虚构来源/内容/版本/范围/记录及可见变化，不只换标题或恒定“通过”。

| 场景 | 必须展示的操作与区别 |
|---|---|
| S1 草稿分审 | 至少两份带来源/前提/验证要求的方法草稿；修改取消/应用、确认内容/项目范围、丢弃另一份。知识摘录不自动变指令，内容确认不自动安装/启用 |
| S2 新技能人工同意 | 默认和Full Access情景下，未答/拒绝都不出现安装成功；同意绑定所展示对象与项目范围，再可载入演示安装结果。涵盖内使用不重复问，失败不伪造可用 |
| S3 范围与权限分离 | 统一管理不等于全局启用；默认仅虚构当前项目，另一项目明确超范围。查看拟扩围说明/取消不改变范围；Full Access不扩技能范围，不改Q30任务权限默认作用域 |
| S4 任务按需选用 | 至少两个任务/工作包需求不同，相关精简技能不同；手动指定和模拟Agent选用均受已批准/启用/预设可用条件限制，已批准同范围不重复询问，不整库加载 |
| S5 可用性有依据 | 包含可用、缺依赖/不支持、未知三类预设，来源/版本/当前执行者及阻塞原因可查；覆盖Claude Code、Codex、omp、dsh名称时全标“预设，非实际兼容结论”。不自动换执行器、复制密钥或提权 |
| S6 更新先审阅 | 旧版、更新候选及来源/内容/能力/依赖/范围变化可比较；未答/拒绝不覆盖。明确同意后才载入项目新版本示例，新任务选新版、在途仍保留旧版；不自动扩散另一项目 |
| S7 在途不是永久豁免 | 载入旧版仍获准/安全/宿主允许，以及撤销/不安全/硬拦截等有限记录；后者不得再呈为可继续使用。用户Stop优先；旧引用历史保留，不自动fallback/全队重启/回放，不承诺抹除上下文 |
| S8 离线与重置 | 草稿、同意、拒绝、版本、任务选用及外观情景能重置；关闭重开及退出重启恢复初始状态。真实键盘/sheet/长文/窄窗动作和浅深色、glass/classic、辅助偏好可检查，不写真实偏好 |

S6应明确显示“安装同意/版本启用/当前可用/任务选用”差异，不能让批准直接
变成所有任务消费新版。新增能力/依赖或范围变化必须可见；一次同意不涵盖
未来所有版本或扩大范围。无来源、依赖不满足/未知时不假装已可用。

S7通过“载入预置记录”演示已知边界，不实现安全判据、撤销传播时刻、强制
停任务协议、版本切换/回退或上下文清除。技能后续使用受阻不等于整项任务
已完成/失败；Stop情景明确停止工作意图，不自造真实控制信号。
这些演示按钮不意味着正式生产流程每一步都必须人工操作。
U01任务副本工具验证/试用/治理/回退不纳入普通技能更新样例。

## 4. 原生装配、入口与隔离

### 外观与辅助功能

- 实际复用 `NativePanelFoundation`、`GlassPanel`、`NativePanelGeometry`、
  `NativeIconButton`与共享NativeSelection。连续材质基底覆盖圆角间隙，
  18pt连续面板圆角、8pt间距/内边距及弱语义边界，不贴三角补丁。
- SwiftUI/AppKit真实Material；26+交互控件使用系统原生Liquid Glass，
  必要availability门控保持。不给整个内容pane套`glassEffect`；
  方法正文、来源及差异长文用不透明`Color.dsTextBackground`。
- 使用语义accent/success/warning/danger配文字/图标，不照截图RGB、
  不只堆灰色、不只靠颜色。独立图标默认无永久圆环/托盘。
- 文字按钮、短输入和选择项沿用A样式的胶囊/圆润原生变体；面板保留
  恰当连续圆角，不把整窗/长文做成药丸，不为圆润而覆盖系统选择态或焦点。
- 邻接互斥项用系统Picker/List/Menu/Toggle和共享NativeSelection，
  不手绘选中背景/下划线；缺必要共享变体时报告，不复制或越界改Design。
- glass/classic、interactionMode、任务权限三者独立；样例不改真实设置。
  减少透明度/增强对比/减少动效必须影响实际View，注入只加强系统偏好，
  标明系统值/演示值；不改全局系统设置、不用`@AppStorage`持久化。
- AX identifier只放叶子，统一`skillReview.`前缀及可读中文名称；
  Tab/方向键/Escape、焦点、sheet取消/确认、长文滚动与窄窗操作真实可达。
  不裁切焦点/IME，不用私有窗口API、swizzling、自绘无边框窗口或事件回放。

### 新入口契约（尚待实施）

新增 `AERORIC_NATIVE_SKILL_REVIEW=1`。全部新View/fixture/Store/入口、
拒绝页及专属生命周期DEBUG限定。在 `AeroricEntryPoint.main` 的DEBUG内
**先于当前Knowledge → Evolution → Visual链**做新的只读决策：

1. 新flag**不存在**：inactive，原链逐字保留其语义，不重排/统一旧入口。
   缺席不是赋值0；当前Knowledge/Evolution对其自身0/空值也会拒绝。
2. 新flag存在且为1，`AERORIC_ENV=ui-test`、显式`AERORIC_DATA_DIR`
   通过已有 `NativeVisualReviewEntry.isIsolatedDataDirectory`只读检查，
   其他审阅flag无1：只进入技能样例。
3. 新flag存在但为0/空/其他非法值，或环境/目录不合法，或任一其他审阅
   flag为1：只进入新只读拒绝页，不回落产品、不按先后挑一个样例。

其他键：`AERORIC_NATIVE_KNOWLEDGE_REVIEW`、
`AERORIC_NATIVE_EVOLUTION_REVIEW`、`AERORIC_NATIVE_VISUAL_REVIEW`、
`AERORIC_NATIVE_COLLABORATION_REVIEW`。协作当前是预留契约，开工重核。
新flag存在时，其他键0不算冲突；新flag缺席时不解释/修复旧键。
沿用路径helper，不另造宽松prefix，不创建目录；该检查不是OS沙箱、
偏好域隔离或文件访问权限授予。拒绝原因可读但不回显完整路径/环境/密钥。

新样例/拒绝必须使用自己的无业务副作用生命周期，唯一窗口标识，
仅管理自身标准窗口；不得复用产品delegate、业务菜单、openWindow路由，
也不得清UserDefaults或操纵其他实例。可在新Entry文件中定义
`NativeSkillReviewWindowLifecycle`，沿用现有原生窗口托管模式；
关闭后丢弃Store，重新打开创建新的View/Store。
不可复制旧输入补丁、子视图扫描或抢夺焦点来绕开系统选择态。

当前Evolution/Knowledge已经采用各自生命周期，本轮未见旧清域delegate；
这是本时点静态事实，**不证明全部启动/回归无偏好副作用或已有隔离许可**。
ReleaseLocal排除全部新实现，忽略新flag走原产品链；只编译/检查排除，
不为测变量而启动生产Release。

## 5. 白名单、所有权与开工门槛

最大产品/测试修改范围如下，不要求全改；额外文件或机制先申请最小扩围。
所有新源文件都整体DEBUG限定，可在这些文件内拆小类型/View：

| 路径 | 允许职责 |
|---|---|
| `macos/AeroricKit/Sources/AeroricUI/SkillReview/SkillReviewModels.swift`（新） | 仅样例的草稿/请求/范围/可用性/版本/任务值类型，无生产协议 |
| 同目录 `SkillReviewFixtures.swift`（新） | S1–S8安全虚构内容和有限记录 |
| 同目录 `SkillReviewStore.swift`（新） | `@MainActor @Observable`窗口内存状态及有限转换，不接业务 |
| 同目录 `NativeSkillReviewView.swift`（新） | 原生装配、详情/sheet与交互 |
| 同目录 `NativeSkillReviewEntry.swift`（新） | 只读策略、拒绝页、技能专属窗口生命周期 |
| `macos/AeroricApp/Sources/AeroricApp.swift` | 仅最早DEBUG技能分流及技能专属App装配，不改旧链/产品 |
| `macos/AeroricKit/Tests/AeroricUIUnitTests/NativeSkillReviewTests.swift`（新） | 入口、内存转换/取消/重置、fixture边界及实际组件装配测试 |
| `macos/AeroricUITests/NativeSkillReviewUITests.swift`（新） | §6六个真窗口方法与自身安全helper |
| `scripts/native/ui-test.mjs` | 仅向TESTS追加六方法；不改预热/kill/重试、witness或环境机制 |
| `scripts/native/checks.json` | 按实测同步上调相关floor/minCases及相邻说明，不缩集合/降门槛 |

任务私有PRD/design/implement/research、context和本次证据报告另按Trellis管理；
这不授权改整壳/迁移任务状态或自动注入全库。
共享Design/ThemeTokens、既有Review及其测试、Package.swift、project.yml、
xcconfig、build/check/sign/release脚本均只读。生成工程仅允许现有generate工具
产生派生变化，不手改、不新增target。业务/core/protocol及个人配置不在白名单。

### 按顺序开工

1. 用户在新终端明确要求执行本文后，依workflow取得任务创建/规划同意，
   创建/复用独立`native-skill-review`子任务，package为aeroric-macos，
   parent为 `10-14-native-frontend-redesign`。复杂任务完成PRD/design/
   implement、相应context与最终审阅，再依实施确认/start门槛只启动子任务。
   不启动仍planning的整壳；无会话身份时报告，不手改metadata绕过。
2. main上保存开工status、白名单原文/diff/SHA-256至
   `.artifacts/native/skill-review/<run-id>/`，以当时脏树为基线，不能用HEAD
   冒充。冻结所有既有阶段包/设计快照及MiMo研究，不重写历史来加本轮授权。
3. 取得App入口、UI清单、checks等共享文件的明确单写交接；每次编辑前核对
   hash/diff，同期变化先协调，不stash/reset/clean或整文件覆盖。
4. 首次swift test/generate/build前取得SPM/core/工程/DerivedData/bundle及
   签名产物独占窗口。建立针对性基线；先写入口/值类型测试和安全fixture，
   再Store、View、最小App分流和UI登记，不从真实技能接线开始。
5. UI前另取得**本次**隔离账号/VM、桌面与进程资源许可，核对§6当前witness
   依赖；别的任务的VM或controller许可不自动继承。缺条件只做独立/静态
   工作并报告UI阻塞，不越界修共享runner或清偏好。
6. 交回证据后停止，等待主线程审查及用户看窗口，不继续真实安装/引擎。
   失败保留诊断，只处置自己相对开工快照的具体hunk；先核对无后续协作者
   修改，不回滚整文件、不擅自关闭用户App或删除锁。

## 6. 后续验证与证据要求

以下均为**外部执行者后续验证**，本轮文档交接没有运行。
逐命令保留原始输出、退出码、passed/failed/skipped及逐方法结果；
0例不算通过，保留失败/重试，不用最后成功掩盖前面失败。
配置计数/类存在/源码常量不证明真实旅程或无副作用。

### 构建/测试资源与当前静态基数

单写文件不等于构建独占。测试前交接SPM `.build`、core target、生成工程、
固定DerivedData、Debug bundle/helper/签名；UI另共享Caches scratch/预热目录、
`ui-test-last.log`及gate日志。不改写正在运行的bundle，不通过新worktree绕过。

2026-10-18静态配置：shell/native-design均跑整个`AeroricUIUnitTests`，
两处floor/minCases均350；swift-client为33/28/2；UI清单27方法，无CLI筛选
参数。开工重核，不照抄旧包277/21；新增后登记数为**开工实际数＋6**。
新增单测按实测同步提高两组floor/minCases及说明，不缩filter/旧回归。

取得独占后串行执行并各自保存日志：

```sh
RUN_ID="$(date '+%Y%m%d-%H%M%S')"
RUN_ARTIFACT="$PWD/.artifacts/native/skill-review/$RUN_ID"
mkdir -p "$RUN_ARTIFACT"
swift test --package-path macos/AeroricKit --filter NativeSkillReviewTests
swift test --package-path macos/AeroricKit --filter NativeKnowledgeReviewTests
swift test --package-path macos/AeroricKit --filter NativeEvolutionReviewTests
swift test --package-path macos/AeroricKit --filter NativeVisualFoundationTests
swift test --package-path macos/AeroricKit --filter ThemePolicyTests
pnpm native:build
node --check scripts/native/ui-test.mjs
pnpm native:check shell
pnpm native:check native-design
pnpm native:check swift-client
```

新suite尚不存在时，开工基线写“未存在”，不报0例通过。
build走现有generate/cargo Debug/Xcode Debug、host拷贝及开发签名链；
runner仅bundle缺失才build，已有bundle须显式重建，不能用旧产物证明本次代码。
这些构建步骤不代表样例启动了host，也不授权改host源码或签名配置。

入口单测覆盖新flag缺席、存在的0/空/非法值，环境/目录缺失及非法，
Caches根/相对/路径穿越/前缀碰瓷/symlink、有效未存在子目录；
四个旧/预留flag分别冲突及全部15种非空组合，其他flag0不冲突。
危险路径仅纯策略测试，不实际启动；缺新flag保持原链用纯策略/源码接入核对，
不为负向测试启动产品。内存/装配测试必须覆盖S1–S8真实转换和实际View消费，
包括未答/拒绝与Full Access、内容修改后同意对象、任务相关性/不可用/未知、
更新不覆盖/在途固定条件、Stop/撤销优先、取消/重置及新Store。

### 六个真窗口方法须逐一登记

文件/类：`NativeSkillReviewUITests`；清单前缀：
`AeroricNativeUITests/NativeSkillReviewUITests/`。

1. `testDraftReviewEditsAndDiscardStayOffline` — S1，查看来源/条件、
   修改取消/应用、内容确认不自动安装、丢弃。
2. `testNewSkillConsentAndProjectScopeRemainExplicit` — S2/S3，默认/Full Access、
   未答/拒绝/同意区别，演示安装失败/成功与可用性分离，另一项目未扩围。
3. `testTaskSelectionRespectsAvailabilityAndSources` — S4/S5，两个任务差异、
   手选/模拟按需、阻塞/未知/来源及不重复询问；不整库注入。
4. `testUpdatesKeepInFlightVersionsWithinSafetyLimits` — S6/S7，差异/拒绝/
   同意、新任务新版/在途旧版、旧版继续条件与Stop/撤销/硬拦截优先。
5. `testNativeAppearanceKeyboardAndCompactLayout` — S8，重置/关闭重开/
   退出重启、浅深/glass/classic、辅助偏好、键盘/sheet/长文及三种实测尺寸。
6. `testInvalidSkillRequestsNeverReachProduct` — 安全Caches路径下，新flag
   0/空/非法值、错环境、四个旧键分别及组合冲突只到只读拒绝页，不启产品。

必须真实点击并读AX结果，不只断言固定文本。helper仅读取实际账户home
（不能用sandboxed runner容器home），选唯一且未存在的安全Caches子路径，
启动前后读取已存在父目录证明该子目录没被创建；不由helper先mkdir掩盖写入。
先拒绝已有同bundle实例，再启动/记录自身PID和窗口；失败也只处理自己拥有
的实例，不让XCUIApplication顺便终止其他进程。
显式设skill=1及knowledge/evolution/visual/collaboration=0，控制两种拼写的
继承污染；纯策略证明缺席语义，运行测试不把skill=0当“关闭”回落产品。

### 完整UI链：已知依赖与停止条件

UI只经项目`pnpm native:ui-test`链；accessibility gate调用同一完整runner，
只跑一次完整集合，不发明`--filter`，不运行runner来检查方法登记。
当前预热/清理先于清单守卫；守卫只识别漏文件，不能发现同文件漏方法。

- runner删除固定scratch、终止匹配该目录的host，`open`预热普通App后
  广匹配`MacOS/AeroricNative$`发送SIGKILL；重试匹配
  `MacOS/AeroricNative|aeroric-cli serve`。没有进程归属锁。
  跑前只读核对PID/完整命令/目录/归属并取得本次独占；有用户/未知/其他
  终端实例就不跑，不能先kill腾位。
- 完整集合包含普通产品App/host及既有Review，必须在明确获准、没有真实
  Aeroric配置的隔离账号/VM运行；安全数据路径不隔离UserDefaults偏好域。
  不因旧清域delegate已消失就取消隔离门槛，不重置TCC或改全局偏好。
- **现有runner的偏好witness控制器专属于Knowledge suite**：
  `AERORIC_KNOWLEDGE_WITNESS_CONTROLLER`指向外部获准控制器，缺它时旧
  Knowledge用例会失败。本包不提供/审核/运行该控制器，不删除旧用例或
  绕过witness，不把旧VM许可带入新任务。
- Skill样例/拒绝页还需自己的隔离测试偏好证据：优先由获准隔离环境的
  外部测试操作者按启动/结束采集非敏感标记/域变化，XCTest只读。
  在独立子任务计划中审阅具体证据方案；Knowledge的suite/方法/账号/domain/
  marker约束不能直接冒充Skill证据。若需要新增控制器代码或修改runner
  超出TESTS，先申请最小扩围；未获审阅或缺隔离条件时**UI明确阻塞**。
  禁止为通过而删除断言、放宽现有验证、在日常账号种植或清理偏好。
- 无敏感测试标记只能证明该标记未被清除，不证明零偏好写入。
  结合早分流、依赖/调用核对、进程树、安全目录及测试域前后证据判断；
  AppKit窗口系统痕迹与应用业务写入分开报告，不能声称绝无OS级持久痕迹。
- 普通回归仅使用获准测试Caches/fixture，不跑真实Agent/API；结束后核对
  本次进程/目录。check超时只杀直接child，不代表进程树已停；未知遗留交还
  owner，不删锁、抢焦点或重置环境。没有安全时段就只报阻塞。

满足上述前提、保留获准controller/证据配置，同时清除全部审阅污染后：

```sh
env -u AERORIC_NATIVE_SKILL_REVIEW \
  -u AERORIC_NATIVE_KNOWLEDGE_REVIEW \
  -u AERORIC_NATIVE_EVOLUTION_REVIEW -u AERORIC_NATIVE_VISUAL_REVIEW \
  -u AERORIC_NATIVE_COLLABORATION_REVIEW \
  -u TEST_RUNNER_AERORIC_NATIVE_SKILL_REVIEW \
  -u TEST_RUNNER_AERORIC_NATIVE_KNOWLEDGE_REVIEW \
  -u TEST_RUNNER_AERORIC_NATIVE_EVOLUTION_REVIEW \
  -u TEST_RUNNER_AERORIC_NATIVE_VISUAL_REVIEW \
  -u TEST_RUNNER_AERORIC_NATIVE_COLLABORATION_REVIEW \
  pnpm native:check accessibility
```

### Release、平台与用户审阅证据

ReleaseLocal实际编译、有效设置/产物确认全部新样例和入口排除；仅源码
`#if`、配置名称或Debug通过都不充分。只编译/检查，不运行生产Release：

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

逐项记录实际OS/SDK/架构；Apple Silicon/macOS27、Intel/macOS26缺哪项就写
“未验证”，交叉编译不算Intel真机运行。不给整个产品或商店发布盖章。
保存S1–S8预期/实际、操作、截图/AX、材料接缝/语义色/裸图标、辅助偏好
消费和几何证据。无sidebar/未创建目录/界面写“未启host”不能单独证明隔离。
交回**本次已验证**Debug bundle路径、隔离环境和完整显式env启动命令：
skill=1，其他四审阅键=0，env=ui-test及唯一安全Caches子路径；
不能只让用户“直接打开App”。本轮尚无已验证bundle或可用的新入口。

## 7. 回报与监工停止点

保存 `.artifacts/native/skill-review/<run-id>/report.md`，至少包括：

```text
# 原生技能审阅样例01：实施回报
## 用户执行指令、独立任务/main、共享文件/构建/桌面交接
## 开工脏树快照、实际文件、相对快照patch与冻结边界
## S1–S8实际操作/变化、预期/实际、截图/AX及失败
## 内容确认/安装同意/演示结果/可用性/版本/任务选用的分离
## 入口矩阵、原链保持、生命周期及目录/进程/偏好域证据
## 原始命令/退出码、passed/failed/skipped、逐方法登记/重试
## 现有Knowledge控制器依赖和Skill证据方案的审阅/授权/执行
## 原生组件/键盘/辅助偏好/尺寸、实际OS/SDK/架构及缺口
## Release编译/有效设置/排除，不运行生产Release
## 本次已验证Debug bundle、隔离环境、显式启动命令和审阅顺序
## 已有失败、未验证、偏离及需要主线程/用户决定的问题
```

主线程依据开工快照、完整差异、新鲜日志及真实View/AX消费审查，再请用户
看窗口。执行者自报、测试数量、文档一致或截图好看都不等于用户验收。
