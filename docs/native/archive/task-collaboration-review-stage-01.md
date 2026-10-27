# 任务协作审阅样例 01 — 外部执行模型交接

版本：1.0 · 2026-10-15 · 批准依据：Q35 用户回复“确认”。
对应设计：Q27–Q34；参考候选：P01–P06、O01–O03、F01、Z03。

**已批准范围：离线、可交互的SwiftUI原生协作审阅样例。不是协作引擎、
真实权限开关或正式导航/工作区布局的批准。**
用户把本文明确交给新终端并要求执行后，执行者才按本文及Trellis门槛实施。
主线程继续设计、交接和审查，不在当前线程实施产品。
不扩大 `frontend-redesign-stage-01.md` 或 `task-evolution-review-stage-01.md`
这两份冻结的版本1.0执行包；本文也不是整个前端重整的阶段02。

## 1. 交付目标、必读与非目标

在**现有Aeroric原生App target**内，新增DEBUG、显式开启的离线协作审阅
窗口，与产品启动路径分开。用户可点击工作包、负责人/依赖、交付证据和
恢复场景，先审阅工作流程。所有业务状态只在窗口内存中，重置/重开恢复
fixture；只交截图、`#Preview` 或不可点击稿不算交付。
“独立”指交付边界，不另建App、WebView、网页或一套组件系统。

仓库：`/Users/macbook/Downloads/同步空间/LYX/Aeroric`。先读：

- 根目录 `AGENTS.md`、`.trellis/workflow.md`；
- `.trellis/spec/aeroric-macos/frontend/index.md`、
  `docs/native/adr/001-native-boundary.md`；
- `docs/native/task-collaboration-design-checkpoint-01.md` 版本0.2；
- 本文全部、现有入口与共享设计组件；按需读
  `.trellis/tasks/10-14-native-redesign-shell/research/collaboration-review-handoff-audit.md`；
- 展示U01时，仅按需读 `docs/native/task-evolution-design-checkpoint-01.md`
  版本0.2，不改其已交接规则或另一个样例实现。

总库及来源限制见 `docs/native/harness-feature-candidates.md`；
不全量注入35KB MiMo报告，不重扫harness，不复制参考项目UI或代码。
本文依据2026-10-15的脏工作树，**开工重新核对当前文件**，不能用旧执行包
中的246例/15个UI方法替代当前有效配置。

### 不做什么

- 不启动真实Agent、PTY、治理进程、模型/API、MCP或外部workflow。
- 不读密钥、真实任务/项目/会话/统计/连接；不写真实任务、权限、
  UserDefaults业务配置或数据库，不提供“保存到全局”。
- 不导入或调用 `AeroricClient`/生产Store/UDS/网络，不从ANSI推断状态。
  仅允许启动目录边界的只读检查及自身窗口测量；测试另按§6保存证据。
- 不实现调度器、依赖推导器、权限沙箱、文件锁/租约、进程接管、真实
  重试/取消/恢复或外部操作重放；不建分支、clone/worktree。
- 不改变首页默认all-tasks、主导航、菜单业务路由、AppServices或业务owner。
- 不宣称Full Access已实际提权、四家族已兼容、系统已隔离旧写入、
  回执证明模型消费，或技术核验等于用户最终验收。

**有效样例及拒绝页都必须在构造产品App/delegate/services之前进入，
自身不启动host。** 全套UI回归的普通App预热可能另启隔离测试host，
见§6；不能把这两种行为混成“整套测试完全没有产品进程”。

## 2. 样例布局和可操作内容

沿用A样式与共享原生组件，以下是样例交付的信息层级，不冻结正式布局。
范围内允许调整排版、折叠及间距，交回真窗口让用户审阅；
不强迫固定三栏，更不能新增一个长期空白的团队/公司管理首页。

1. **顶部**：虚构任务名称、协调负责人摘要，始终可见的
   “离线协作审阅 · 非真实任务”标识；原生场景菜单、重置及外观控件。
2. **工作包列表**：用系统选择列表展示工作项、负责人、当前事实和依赖摘要，
   默认选中一个有内容的工作包。不把CLI进程状态当工作包验收状态。
3. **主要内容**：所选工作包的目标/计划依据版本、交付与验证要求、
   读写范围/禁止事项、获准上下文引用、负责人/依赖、当前产物/证据、
   未完成项和下一步说明。成员简表按需展开，不另占常驻空栏。
4. **按需详情**：点成员、来源引用、证据或恢复记录，打开只读sheet/popover
   或临时检查器；窄窗可用菜单选择工作包并纵向滚动，动作仍全部可达。

初始审阅方向1260×860pt，另核对1000×640、400×600。
使用自身窗口的公开API测量外框/内容尺寸；若屏幕装不下，报告实际值和
限制，不缩放截图冒充。这里只验样例，不承诺正式工作区支持这些尺寸。

### 必须可操作

- 选择工作包、打开成员/依据/证据详情，滚动查看较长交接说明。
- 打开原生工作包sheet，仅修订**示例交接说明/汇报备注**；
  取消不应用、应用只改当前样例内存。既定目标、验收标准及授权规则不做成
  Agent自动改写动作，不要求用户重新填完整工作包。
- 使用“演示提交”“载入示例核验记录”“演示安全交接”等明确演示动作，
  观察有限、本地的状态转换；每个变化说明发生了什么、没发生什么。
  没有必要证据时动作禁用或显示具体原因，不提供“强制通过/假定已停止”。
- 查看默认授权/Full Access两种**fixture情景**，明确默认只影响当前示例任务；
  在详情中查看协作者必要权限、只读监督与人工确认策略的区别。
  本阶段不新增真实权限设置或可自选任意动作的授权面板。
- 重置和切换场景清楚提示只覆盖内存样例；退出重开不保留备注。

演示点击不是生产上每步都要用户批准。载入一份“已核验”的fixture不是
实际执行测试，也不能把成员提交按钮直接冒充技术核验。
所有连接/模型/文件/版本/检查记录都内嵌虚构，不打开真实文件/URL。

## 3. 八个离线场景：不能偷偷补齐真实机制

R1–R8只是本样例场景编号，不是新增候选ID、wire枚举或生产状态机。
每个场景都有足够具体、彼此一致的工作包、成员、版本、证据及解释；
不能只换一段标题或显示恒定的“安全/通过”文字。

| 场景 | 必须能看见和操作的结果 |
|---|---|
| R1 任务驱动交接 | 主Agent兼协调负责人，编码/研究/验证按需组合；工作包含版本化依据、范围、交付/验证、必要上下文与回报；引用只展开内嵌摘录，不全量复制历史 |
| R2 依赖未满足 | 接口交付尚待核验，依赖联调保持等待并说明原因；无关且无共享冲突的文档工作仍可演示推进，不能一项受阻就全任务暂停 |
| R3 共享修改点 | 同副本同文件/共享接口有当期写入负责人；交接前替代写入不可推进，示例停止或可靠隔离证据及归属交接满足后才可演示接手；不同路径也可共享接口依赖 |
| R4 提交不等于完成 | 成员交付后待核验，缺必需证据时不能放行依赖；载入适用核验记录后才演示完成工作项；保持“非最终用户验收”，不要求用户实际批准 |
| R5 版本变化 | 当前产物v2、证据v1时不伪报已核验；需整合时显示整合后版本及适用记录。载入对应v2示例证据后再演示解除这项限制，其他工作不重置 |
| R6 原执行未知 | 保留部分成果/失败事实，沉默或超时不显示已停；原写入未确认停止/可靠隔离并安全交接时不启动重叠替代写入。载入预置核实/交接记录后才演示原授权/预算内局部恢复 |
| R7 外部结果未知 | 已发送的外部写操作结果未知，不能直接“重试写入”；仅展示预置核实结果/受阻说明。文件/工作项恢复不代表外部副作用撤销，不执行命令/网络/数据库操作 |
| R8 Full Access与监督 | 覆盖内委派/常规恢复不新增人工权限弹窗，协作者仅得工作包必要权限；仍有交付核验。启用U01的fixture保留反思只读核对及候选必要治理评审；普通阶段满足Q21后不因治理单独待审多等签字，无实际治理进程 |

可用有限fixture转换表达，不写通用授权/依赖/恢复引擎。
例如R6里的“停止证据”是预置虚构记录，须标注“示例”；
点击载入记录不表示向真实进程发出了停止命令。
R8是已确认原则的表达，不确定Full Access真实动作覆盖集、API映射、
持久化/撤回、预算/人数/层级、超时/重试额度或强制隔离协议。
U01 API故障不算通过、不静默换模/自审替代；本样例不重做三角色配置页。

## 4. 原生视觉、辅助功能与启动契约

### 实际使用共享组件

- 复用 `NativePanelFoundation`、`GlassPanel`、`NativePanelGeometry`、
  `NativeIconButton`、`NativeSelectionList`/`NativeSegmentedPicker`。
  按实际需要使用，不为凑名字塞无关控件；不复制样例专属视觉系统。
- SwiftUI/AppKit真实材质；macOS14基线，26+真实Liquid Glass按availability
  门控且仅增强可交互控件，不给整个内容pane套 `glassEffect`。
- 连续材料基座覆盖面板间隙，18连续圆角、8pt间距/内边距、弱语义边界；
  长证据/交接正文用不透明 `Color.dsTextBackground`，不逐行叠材料。
- `Color.ds*` 的accent/success/warning/danger结合文字/图标表达状态，
  不一片灰、不用截图RGB或仅靠颜色；独立图标按钮默认不加圆环/托盘。
- 邻接互斥选择用共享系统选择控件；不自绘active底/下划线。
  标准焦点和选中态优先于胶囊外观；组件缺变体时回主线程，不另造选中态。
- glass/classic、interactionMode、权限模式独立；样例不改变真实后两者。
  系统减少透明度/增强对比/减少动效须被实际View消费；演示注入只加强，
  明示系统值与注入值，不修改用户全局系统设置。
- AX identifier只放叶子，统一 `collaborationReview.` 前缀和中文可读名称。
  Tab/方向键/Escape、sheet取消/应用、长文滚动和窄窗动作可达；
  不裁切焦点/IME，不用私有窗口API、swizzling或无边框窗口。

### 新入口：显式请求才分流

新增 `AERORIC_NATIVE_COLLABORATION_REVIEW=1`，**当前尚未支持**。
样例/fixture/入口/拒绝页整体DEBUG限定。
复用现有、只读的 `NativeVisualReviewEntry.isIsolatedDataDirectory`，
要求 `AERORIC_ENV=ui-test` 和显式安全Caches子目录；
不另写宽松前缀判断，不把目录检查称为OS沙箱。

在 `AeroricEntryPoint.main` 的DEBUG部分，**先于现有evolution/visual分流，
更先于任何产品App/delegate/services构造**，调用新的只读入口决策：

1. **新flag不存在**：inactive，原evolution→visual→产品链原样继续，
   包括原有无效请求的行为；不顺手重构旧路由。
2. **新flag存在且为1，环境/目录有效且无冲突**：只进协作样例Scene。
3. **新flag存在但值/环境/目录无效，或旧evolution/visual任一flag为1**：
   进入新的只读“审阅未启动”拒绝页，不靠顺序静默选一个，不落回产品。
   新flag为0/空值也属于无效显式请求；不等于未提供。

旧两个flag是 `AERORIC_NATIVE_EVOLUTION_REVIEW`、
`AERORIC_NATIVE_VISUAL_REVIEW`。在新flag存在时，它们取1才视为冲突；
新flag不存在时，旧入口自己解释旧值，语义不能变化。
新Scene不挂业务命令、AppDelegate或生产openWindow路由；
拒绝原因可读但不含敏感路径，不能把拒绝页显示为样例已启动。

ReleaseLocal通过编译条件排除全部新实现，忽略审阅变量并走原产品路径。
只验证Release编译/有效编译设置，不为测flag实际启动生产Release窗口。

## 5. 文件白名单、所有权与实施顺序

白名单是最大范围，不要求全改。只新增下列五份源文件，可在文件内拆小View；
需要增加源文件或跨界修改，先回主线程说明原因和最小差异。

| 路径 | 允许职责 |
|---|---|
| `macos/AeroricKit/Sources/AeroricUI/CollaborationReview/NativeCollaborationReviewView.swift`（新） | 样例主界面/小View，实际使用共享设计组件 |
| 同目录 `CollaborationReviewModels.swift`（新） | 仅样例的工作包、成员、版本、证据/依赖/恢复记录值类型 |
| 同目录 `CollaborationReviewFixtures.swift`（新） | R1–R8、虚构上下文摘录与记录，无外部读取 |
| 同目录 `CollaborationReviewStore.swift`（新） | `@MainActor @Observable`窗口内存状态、备注草稿与有限演示转换 |
| 同目录 `NativeCollaborationReviewEntry.swift`（新） | DEBUG入口决策、拒绝页，无产品服务构造 |
| `macos/AeroricApp/Sources/AeroricApp.swift` | 只加最早的DEBUG协作分流/隔离Scene与拒绝Scene，保留旧链 |
| `macos/AeroricKit/Tests/AeroricUIUnitTests/NativeCollaborationReviewTests.swift`（新） | 入口/场景约束/证据版本/内存状态与组件装配测试 |
| `macos/AeroricUITests/NativeCollaborationReviewUITests.swift`（新） | §6六个真窗口测试，逐方法登记 |
| `scripts/native/ui-test.mjs` | 只向TESTS追加六个方法，不改预热/清理/重试/环境机制 |
| `scripts/native/checks.json` | 必要时按实际执行量上调对应floor及相邻数量说明，不降低、不改filter/执行集合 |

共享Design、ThemeTokens、旧EvolutionReview及其测试、Package.swift、
project.yml/xcconfig、构建/签名/发布脚本只读。Xcode工程只允许既有
generate工具生成派生变化，不手改，不新增target。
正式shell、TaskWorkspaceView、AppServices/生产Store、protocol/core、
Tauri/React、OMP历史与PTY、DSH消息路径都不在白名单。

### 外部执行开工顺序

1. 收到用户在新终端**明确执行本文**的指令后，再按Trellis创建/复用
   `native-collaboration-review`独立子任务（package为aeroric-macos，
   parent为 `10-14-native-frontend-redesign`）。先形成PRD/design/implement、
   按需context及最终审阅摘要，再按审阅/start门槛只启动此子任务；
   不启动仍planning的整套外壳。新设计或扩大范围先问用户。
   无会话身份时报告，不手改任务状态，不修改其他执行者任务元数据。
2. main上记录开工status、白名单既有差异及SHA-256，保存
   `.artifacts/native/collaboration-review/<run-id>/`；不用HEAD冒充脏树基线。
3. 取得App入口、ui-test、checks等共享文件的明确单写交接。
   当前视觉基础与进化样例均有in_progress任务/dirty改动；
   dirty不等于可覆盖，进化样例已存在也不等于本线程验收过。
   没交接时只能继续独立/只读工作。
4. **首次swift test/generate/build之前**取得§6构建产物独占窗口。
   先记录针对性基线，再做模型/fixture与入口测试、内存Store、原生View、
   DEBUG早分流、UI测试及登记；不从真实业务接线开始。
5. 每次共享文件编辑前重核hash/diff。同期修改即停止覆盖、协调交接；
   不stash/reset/clean/整文件重写，不创建分支/worktree、提交推送或发布。
6. 按§6交回证据后停止，不自行继续引擎、权限、规划或其他候选实施。

失败时保留诊断，只处理自己相对开工快照的具体hunk，且先确认无后续
协作者修改；不能回滚整文件/项目、删共享artifact或关闭用户App。

## 6. 验证：命令、真窗口与现有工具的副作用

以下是**执行者后续要跑**的验证，本次文档交接未运行。
逐命令保存原始日志、退出码、passed/failed/skipped；失败即停下区分既有/
本次问题，不用最后一条成功掩盖前面失败，也不顺手修复非白名单代码。

### 构建与测试前先取得独占窗口

文件单写交接不等于构建产物独占。首次测试/生成/构建前，明确共享SPM
`.build`、core target、生成工程、DerivedData、Debug bundle及其运行实例的
归属；不能在别人的使用中bundle上编译、拷helper或重签。
独占覆盖基线、实现后构建与UI验收。无法取得则只做静态工作并记录阻塞，
不冒险抢跑或擅自创建worktree来绕开交接。

2026-10-15静态核对：shell/native-design都执行整个AeroricUIUnitTests，
floor各277；swift-client unit为33、TSan为28、schema为2；
UI显式登记21个方法。开工再核对，不能照旧注释/执行包降回246或15。
新增六个UI方法后应为**开工实际清单+6**，不删除其他测试来凑数；
新增单测按实际执行量必要时提高对应floor，不缩filter。

取得独占后建立证据目录，按现有工具链**逐条、串行**执行：

```sh
RUN_ID="$(date '+%Y%m%d-%H%M%S')"
RUN_ARTIFACT="$PWD/.artifacts/native/collaboration-review/$RUN_ID"
mkdir -p "$RUN_ARTIFACT"
swift test --package-path macos/AeroricKit --filter NativeCollaborationReviewTests
swift test --package-path macos/AeroricKit --filter NativeEvolutionReviewTests
swift test --package-path macos/AeroricKit --filter NativeVisualFoundationTests
swift test --package-path macos/AeroricKit --filter ThemePolicyTests
pnpm native:build
node --check scripts/native/ui-test.mjs
node scripts/native/check.mjs shell
node scripts/native/check.mjs native-design
node scripts/native/check.mjs swift-client
```

开工尚无新测试时应如实记录“未存在”，不能把0例当通过。
build会生成工程、编译并拷入host、ad-hoc签名；这不等于样例启动host，
也不授权修改签名配置或host源码。

### 六个UI方法必须真实登记/执行

文件/类名：`NativeCollaborationReviewUITests`；TESTS登记前缀
`AeroricNativeUITests/NativeCollaborationReviewUITests/`：

1. `testPackagesOwnersAndTaskAuthorizationStayOffline` — R1/R8，查看工作包/
   获准摘录、成员与授权策略，备注取消/应用；Full Access不重复权限弹窗。
2. `testDependenciesAndSharedWriteHandoff` — R2/R3，联调等待而文档继续；
   未满足交接前不能替代写入，预置交接记录后只演示局部接手。
3. `testSubmissionVerificationAndSupervisionGates` — R4/R5/R8，缺证据/错版本
   不完成依赖；载入适用记录后变化；U01普通阶段与候选门槛不混淆。
4. `testRecoveryPreservesResultsAndAvoidsBlindReplay` — R6/R7，未知不判停、
   部分成果保留、外部未知不重放，演示恢复不宣称副作用撤销。
5. `testNativeAppearanceKeyboardAndCompactLayout` — 真窗口浅/深色、glass/
   classic、辅助功能实际消费、键盘/sheet/长文与三种尺寸可达。
6. `testInvalidCollaborationRequestsNeverReachProduct` — 安全Caches根下，
   错误环境、新flag与旧两个flag分别冲突/三者冲突进入拒绝页，不进产品。

用真实点击和可读AX断言结果，不以源码/常量或fixture自称成功替代。
入口无flag、非法值/路径等完整矩阵在单测覆盖；不为负向测试真的启动
生产路径/不安全数据目录。新helper应解析**实际账户home**，不是sandboxed
test runner的容器home；选用唯一、原先不存在的安全Caches子目录路径，
不由helper预先创建，启动前后读取现存父目录核对该路径仍不存在；
核验前台PID，失败也只清理自身实例。
不原样复制旧helper的固定flag；新样例用新flag=1、旧两个flag=0，
无新flag语义在纯入口测试中覆盖。

UI只通过项目 `pnpm native:ui-test` 链；accessibility gate正是此链。
先满足下面安全条件，再清除shell继承的审阅flag及其TEST_RUNNER转发值，
避免污染普通回归：

```sh
env -u AERORIC_NATIVE_COLLABORATION_REVIEW \
  -u AERORIC_NATIVE_EVOLUTION_REVIEW -u AERORIC_NATIVE_VISUAL_REVIEW \
  -u TEST_RUNNER_AERORIC_NATIVE_COLLABORATION_REVIEW \
  -u TEST_RUNNER_AERORIC_NATIVE_EVOLUTION_REVIEW \
  -u TEST_RUNNER_AERORIC_NATIVE_VISUAL_REVIEW \
  node scripts/native/check.mjs accessibility
```

不额外重复整套UI、不发明runner未实现的筛选参数。
文件守卫不能证明同文件所有方法都登记，必须看逐方法日志和结果。

### UI链真实副作用与停止条件

- 现有runner会删除固定Caches scratch，预热普通App，再按
  `MacOS/AeroricNative$`广泛终止；重试还按
  `MacOS/AeroricNative|aeroric-cli serve`广泛终止。没有进程归属锁，
  安全数据目录不等于进程隔离。不能运行runner来“只检查注册”。
- 延续构建独占，并在UI前只读核对匹配PID、完整命令、数据目录及归属，
  取得**本次**桌面/测试环境授权；不可借其他任务的VM/UI运行许可抢资源。
  用户实例、其他终端实例或所有权未知时不启动runner，不先kill来腾位置。
- 测试期间不并发构建、UI runner、业务App/serve；主线程不抢鼠标、
  焦点或截图。无安全时间窗记UI阻塞，不冒充验收完成。
- 预热和原有shell测试可能构造普通App并启动隔离测试host；仅允许既有
  Caches/fixture回归，不导入真实数据、不运行Agent或真实模型。
  不能保证此边界就停下报告，不为通过而改预热/清理机制、扩大kill范围、
  重置TCC、改系统设置或降低门禁。
- check超时不证明整个进程树已停；结束后核对并只清理确定属于本次的
  实例，保留未知/遗留说明，不碰无关进程。

### 隔离、视觉与Release证据

- R1–R8逐项映射操作、截图、AX和实际结果；含拒绝原因、重置/重开、
  草稿取消/应用、材料接缝/语义色/裸图标、系统与注入偏好、长文与实测尺寸。
- 有效样例和拒绝页不构造产品服务：用早分流源码、进程树观察、隔离目录
  前后检查相互佐证。恒定“host=0”、无sidebar或未建目录，不能单独证明
  无host/RPC/外部副作用；不为证明而读取生产数据。
- macOS14兼容路径与26+Liquid Glass运行证据分别报告；缺系统即未验证，
  不改部署目标或用单机编译冒称双系统通过。
- 保存ReleaseLocal实际编译和有效build-settings，核对App及相关包无DEBUG、
  新入口/fixture被排除；配置名、源码有if或Debug测试全绿都不单独充分：

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

Release只验编译/排除，不打包、发布、改签名或生产运行。
交回本次已验证的Debug bundle路径及完整启动命令：新flag=1、
旧两个flag=0、env=ui-test、唯一安全Caches子目录；指明实际使用的测试机器。
不能只说“直接打开App”，也不从普通业务路由切入演示。

## 7. 给监工线程的回报与停止点

保存 `.artifacts/native/collaboration-review/<run-id>/report.md`：

```text
# 任务协作原生审阅样例01：实施回报
## 用户执行指令、任务路径、main、共享文件交接及构建/桌面独占
## 开工快照、实际文件/相对快照patch、未触碰边界
## R1–R8：操作、预期/实际、截图/AX及未通过项
## 真材质/系统选择/键盘/辅助功能/尺寸：系统、SDK、实测及限制
## 入口拒绝矩阵、原有入口保持、隔离与Release排除证据
## 原始验证命令、退出码、passed/failed/skipped、逐例登记日志
## UI普通App/host副作用、进程与目录前后状态、遗留实例
## 已验证Debug bundle路径、显式env启动命令、手动审阅顺序
## 已有失败、未验证、偏离计划和需监工/用户决定的问题
```

主线程按开工快照和真实diff核对范围、规则、组件/AX实际消费及新证据，
再请用户看窗口；执行者自报完成不等于验收。视觉通过也不代表调度、
权限强制、真实恢复、模型兼容、治理进程或外部操作幂等已实现。
真实接线须另行设计批准。规划/对话与知识沉淀以及其余全库候选继续保留，
不能由本执行包一并实施或排除。
