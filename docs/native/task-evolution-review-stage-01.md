# 任务内进化审阅样例 01 — 外部执行模型交接

版本：1.0 · 2026-10-15 · 候选：U01 · 批准依据：Q26 用户回复“认可”。

**已批准范围：离线、可交互的 SwiftUI 原生审阅样例。不是进化引擎、
真实任务接线或正式工作区布局的批准。** 用户将本文明确交给新终端并
要求执行后，执行者才按下述流程实施；本线程只交接、讨论和审查。
本文独立于 `docs/native/frontend-redesign-stage-01.md`，不扩写冻结阶段01，
也不是整个前端重整的阶段02。不要再次询问同一范围，不自行扩大范围。

## 1. 交付目标与必读材料

在现有 Aeroric 原生 App target 内，新增一个 **DEBUG、显式开启、独立于
产品启动路径** 的任务进化审阅窗口。用户能真实点击配置、阶段、意见、
证据与工具候选，观察离线场景；所有状态仅在内存中，退出/重置即恢复。
仅 `#Preview`、截图或不可点击静态稿不算交付。

仓库：`/Users/macbook/Downloads/同步空间/LYX/Aeroric`。必须先读：

- `AGENTS.md`、`.trellis/workflow.md`；
- `.trellis/spec/aeroric-macos/frontend/index.md`；
- `docs/native/adr/001-native-boundary.md`；
- `docs/native/task-evolution-design-checkpoint-01.md`，尤其Q13–Q26与三条闭环；
- 本文全部，以及现有共享视觉组件和原生入口；按需读
  `.trellis/tasks/10-14-native-redesign-shell/research/evolution-review-handoff-audit.md`。

需要澄清既有规则才按需回读角色路由、阶段复核与家族兼容研究；不要
全量注入候选库、35KB MiMo报告或重扫harness项目。原项目只读，不复制实现。
文档描述的现有代码基于2026-10-15脏工作树，开工必须重新核对。

### 不做什么

- 不调用任何真实模型/API，不读取密钥、连接配置、真实任务、会话或统计。
- 不创建Agent任务、PTY、治理进程、工具副本、clone/worktree；不执行候选。
- 不接 `AeroricClient`、UDS、HTTP、MCP、外部workflow或真实消息投递。
- 不写真实任务/连接、UserDefaults业务配置或数据库；不提供“保存到全局”。
- 不改变主导航、首页默认all-tasks、菜单业务路由、AppServices或宿主生命周期。
- 不实现证据判据、依赖推导、治理IPC、审批强制或四家族适配；不称已兼容。

**样例进程本身不得构造产品App/AppDelegate/AppServices或启动宿主。**
既有全套UI回归设施另有普通App预热/宿主副作用，见§6；两者不可混称。

## 2. 可实施的样例布局，不冻结正式产品布局

沿用A样式、共享原生材料与圆角，不重新问已经确认的造型。
推荐以下信息层级；允许在本范围内调整间距、折叠与排版，交回真窗口审阅，
不能趁样例改正式shell。不要强迫四个常驻面板。

1. **顶部**：示例任务名称、始终可见的“离线审阅 · 非真实任务”标识，
   场景菜单、重置样例，以及独立的外观/材质选择。
2. **角色摘要**：主执行、反思、治理的示例API/模型与继承来源；
   “配置角色”打开原生sheet或popover的Form，不另建生产设置页。
3. **主体**：左侧系统选择列表展示阶段，主区展示所选阶段的状态、
   既定要求、版本、意见/问题、证据及依赖影响。项目交付与能力改进
   用系统分段选择分开，不能把换模型或修项目bug计为工具自进化。
4. **按需详情**：点意见、证据或候选查看只读详情；右检查器默认关闭，
   窄窗用原生sheet/popover，不靠永远空白的大右栏占位。

初始以1260×860pt为审阅方向，另检查1000×640和400×600窄窗场景；
适应真实屏幕可用区域，报告实测外框和内容尺寸，不能偷偷缩放截图冒充。
窄窗可改为纵向、按需打开阶段选择/详情，全部动作可滚动到达。
这些只验本样例，不声称正式主工作区已支持这些尺寸。

### 角色配置必须能操作

- 使用明确虚构的“示例连接A/B”“示例模型1/2”等；无密钥/URL输入、
  联网测试或真实模型能力推断。允许三角色同配置或不同配置。
- 展示一份只读的**示例全局三角色模板**；任务各角色可独立选择继承或覆盖。
  覆盖时更换连接/模型只影响当前角色；恢复继承回到该角色的模板值。
- 不能用“主Agent切换模型”隐式改变反思/治理绑定；取消表单不应用草稿。
  应用只更新当前样例内存，并提示“仅本次样例，未保存真实配置”。
- 治理显示“独立进程为产品设计要求；本样例未启动进程”，不能显示
  假的运行中服务或用三个API标签冒充三个独立进程。

## 3. 样例数据、状态和交互边界

业务owner是**本窗口内存中的审阅Store**，不是新的生产业务真相源。
可用 `@MainActor @Observable` 持有样例、表单草稿和选择；模型/fixture
与View分开。命名明确带Review，不向protocol/core导出或增加wire枚举。
新样例代码整体以 `#if DEBUG` 排除于ReleaseLocal。

fixture应有阶段/问题/候选ID、产物版本、证据版本、来源角色和影响范围，
足以解释当前状态；具体字段只是视图样例，不冻结生产协议。
证据内容全部内嵌、只读、标明虚构，不读取“示例路径”对应的本机文件。
不得用定时器伪造真实流式执行、网络连接、token消耗或测试通过日志。

最少提供以下可切换场景；点击动作均明确以“演示/载入示例”表述。
需要门槛的动作禁用时给可读原因，而非只变灰。

| 场景 | 必须可见、可操作验证的行为 |
|---|---|
| S1 接口待核对/待整改 | 依赖该接口的联调等待；不依赖、无工作区冲突且获准的独立文档可继续；反思反馈标为机器来源示例、未真实投递 |
| S2 普通阶段通过、治理待审 | 既定验证及反思满足后可关闭并推进依赖；治理仍显示待审，不自动变通过/免审 |
| S3 工具候选缺必要评审 | 即便普通阶段通过、候选验证通过，也不能试用；补入同版本必要评审和既有审批满足的示例后才可演示试用 |
| S4 监督API失败/超时 | 显示失败/待审、缺审候选不启用；不自审替代、静默换模型或回退有效工具；恢复只载入当前版本的新评审示例 |
| S5 迟到实质漏检 | 保留原通过记录/证据和下游成果，局部重开；只限制实际受影响依赖，未获暂停证据显示仍执行/待收束；安全独立路径保留 |
| S6 漏检整改/争议 | 展示新版整改、反思、既定验证及原问题定向复审再解对应限制；另可查看Q24用户争议裁决示例，不能用其替代候选必需评审 |
| S7 版本不匹配/风格建议 | 旧版证据不能批准新版候选；纯风格建议不自动阻断阶段；无证据的模型头衔/投票不决定事实 |
| S8 工具试用运行故障 | 明确候选v2停用、回退获准安全工具v1；项目成果仍保留，不显示项目/外部副作用已撤销；区别于S4服务故障 |

可以用有限的本地状态转换演示上述路径，不要写通用调度器或自动依赖引擎。
重置/切换场景应清楚提示覆盖的是样例状态；关闭重开后不保留表单改动。
不能提供可绕过评审的“强制通过”“忽略所有问题”或实质业务Stop按钮。
发送、入队、消费、整改、验证是不同事实；若展示收据，一律标明样例，
不能把“反馈已送出”的演示动作直接等同主Agent已停止或已修复。

## 4. 原生视觉、辅助功能和启动契约

### 实际消费现有组件

- 复用 `NativePanelFoundation`、`GlassPanel`、`NativePanelGeometry`、
  `NativeIconButton`、`NativeSegmentedPicker`/`NativeSelectionList`；
  不复制一套只在样例中存在的视觉系统。
- SwiftUI/AppKit真实材料。macOS14基线，26+ Liquid Glass仅增强操作/
  导航控件并做availability门控；不要整块内容pane套 `glassEffect`。
- 连续材料基座覆盖面板间隙；面板18连续圆角、8间距/内边距、弱语义边界。
  密集证据正文用不透明 `Color.dsTextBackground`，不逐行叠材料。
- 小应用控件沿用胶囊方向，标准系统控件的选中态/焦点优先；裸图标按钮
  默认无圆环/托盘。保留真实Button语义、命中范围、help、禁用和焦点。
- 使用 `Color.ds*` 语义色：accent、success、warning、danger配合文字/
  图标表达状态，避免一片灰，也不以颜色作为唯一信息。不新增截图RGB。
- 邻接互斥导航/段选只用共享系统选择控件；禁止自绘active底/下划线。
  若确需新增共享变体，先回主线程审批，不擅改 `NativeSelection.swift`。
- glass/classic与interactionMode分离；样例不改变真实interactionMode。
  系统减少透明度、增强对比、减少动效被实际View消费。测试注入只可加强
  可访问性并显式标记，不能覆盖真实系统限制或更改用户全局系统设置。
- AX identifier仅放可操作/可读叶子，统一前缀 `evolutionReview.`，
  中文可读名称；Tab、方向键、Escape、表单取消/应用和长证据滚动可达。
  不裁切焦点/IME，不使用私有窗口API、swizzling或无边框窗口。

### 新的显式入口

新增 `AERORIC_NATIVE_EVOLUTION_REVIEW=1`，**不是现有已支持变量**。
只在DEBUG、`AERORIC_ENV=ui-test`、显式安全Caches子目录时显示样例。
复用只读的 `NativeVisualReviewEntry.isIsolatedDataDirectory` 路径检查，
不另造宽松前缀判断；测试覆盖相对路径、`..`、根目录、前缀碰瓷和符号链接。
这一目录限制不是OS沙箱或系统级零副作用证明。

在 `AeroricEntryPoint.main` 内、构造任何产品App/delegate/services之前分流：

1. **未提供新flag**：完全保持原有visual-review与产品启动语义。
2. **明确请求新审阅且条件有效**：仅进入新隔离审阅App/Scene。
3. **新flag存在但值/环境/目录无效，或同时请求旧visual-review**：
   显示DEBUG限定、只读的“审阅未启动”拒绝页，列出不含敏感路径的原因；
   不显示样例已启动，不回落产品，不启动宿主。

新flag与旧visual flag均为`1`时必须拒绝，不靠代码顺序静默选一个。
没有新flag时，不顺手改变旧入口的失败行为。新Scene不挂业务命令、
AppDelegate或生产openWindow路由。
ReleaseLocal通过编译条件移除新入口/fixture/拒绝页，忽略审阅变量并走原来
产品路径；不要为验证flag而实际用生产数据启动Release窗口。

## 5. 文件白名单、协作与实施顺序

白名单是最大允许范围，不要求每份都修改。目录下只新增这五份源文件，
可在文件内部拆小View；若需要更多文件或跨目录修改，先说明原因与最小diff。

| 路径 | 允许职责 |
|---|---|
| `macos/AeroricKit/Sources/AeroricUI/EvolutionReview/NativeEvolutionReviewView.swift`（新） | 主样例及小视图，实际复用共享设计组件 |
| 同目录 `EvolutionReviewModels.swift`（新） | 仅样例的值类型、版本/意见/候选/依赖显示数据 |
| 同目录 `EvolutionReviewFixtures.swift`（新） | 内嵌虚构连接、模板、场景、证据 |
| 同目录 `EvolutionReviewStore.swift`（新） | 窗口内存状态、表单草稿及有限演示转换，无真实I/O |
| 同目录 `NativeEvolutionReviewEntry.swift`（新） | DEBUG入口判定和只读拒绝页，无生产服务构造 |
| `macos/AeroricApp/Sources/AeroricApp.swift` | 仅新DEBUG分流与隔离Scene；保留现有visual-review与产品路径 |
| `macos/AeroricKit/Tests/AeroricUIUnitTests/NativeEvolutionReviewTests.swift`（新） | 入口/版本/角色独立/场景门槛/内存重置与组件装配测试 |
| `macos/AeroricUITests/NativeEvolutionReviewUITests.swift`（新） | 下述六个真实窗口测试，逐方法登记 |
| `scripts/native/ui-test.mjs` | 仅向TESTS追加六个方法，不改清理/预热/重试/环境机制 |
| `scripts/native/checks.json` | 必要时按实测上调对应floor；不降低、不改filter/扫描和其他case |

设计组件、ThemeTokens、Package.swift、project.yml、xcconfig、构建/签名/
发布脚本均只读。Xcode工程只允许既有generate工具产生的派生变化，不手改。
正式shell、TaskWorkspaceView、AppServices、所有生产Store、protocol/core、
Tauri/React、OMP历史与PTY、DSH消息路径均不得修改。

### 开工门槛

1. 用户明确要求在新终端执行本文；仅收到设计快照或读文件请求时保持只读。
   不自动委派给DeepSeek，不要求配置真实模型服务来演示。
2. branch必须main；记录开始时的status、白名单已有diff和文件SHA-256，
   存 `.artifacts/native/evolution-review/<run-id>/`。不用HEAD冒充脏树基线。
3. 此刻共享视觉基础任务仍记录为in_progress，App入口、runner和checks有
   协作者改动。**先取得这些文件的明确单写交接**，再编辑；dirty本身不
   代表可以覆盖。不能确认所有权则继续只读/独立分析，不碰共享写入。
4. 复用已有同范围任务；没有时，在用户执行本文的授权下创建独立
   `native-evolution-review`子任务，package为aeroric-macos、父为
   `10-14-native-frontend-redesign`。不启动完整外壳planning任务。
   先写该子任务的PRD/design/implement及按需context，回指本文。
   若这些材料仅映射本文已批准范围，完成审阅门槛后只start该子任务；
   增加范围/改变规则必须先回用户。无会话身份时如实报告，不能手改状态。
5. 在首次测试/构建前先满足§6的构建产物独占门槛，再记录既有针对性
   测试基线，按顺序新增模型/fixture与入口纯测试、内存Store、原生View、
   DEBUG分流、UI测试和登记；不能先接真实业务。缺独占窗口时可继续
   不写共享构建缓存的静态工作，记录测试阻塞，不冒险抢跑。
6. 每次共享文件编辑前重核SHA/差异；出现同期改动停止合并并协调。
   不stash/reset/clean/整文件覆盖，不改其他任务元数据，不创建分支/
   worktree、提交、推送或发布。
7. 按§6验证并交回报告后停止；执行者自报完成不等于视觉/功能验收，
   不自行开始真实模型接线或下一个候选功能。

失败出口：保留诊断证据；只撤销自己相对开工快照的具体hunk，确认没有
后续协作者改动才处理。不得回滚整个文件/项目、删共享artifact或关闭用户App。

## 6. 验收、命令与测试设施的真实副作用

以下是执行者的后续验收，本次交接未运行构建/UI测试。
每条保存完整日志、退出码和passed/failed/skipped，不用编译代替真窗口验收。
命令逐条执行；失败先停下定位、区分既有失败，不以最后一条的退出码
掩盖前面失败，也不在安全前置条件失效后继续后续命令。

**在下面第一条swift test及任何generate/build之前**，先与其他终端明确
取得原生构建/测试独占窗口，核对共享SPM `.build`、Xcode生成工程、
DerivedData、目标Debug bundle及其使用中实例的归属。文件单写交接不等于
构建产物独占；不能在别人的运行中bundle上重新构建/拷helper/签名。
独占持续覆盖基线、实现后构建及UI验收，不能等到UI阶段才补检查。
无法取得则不启动这些共享写缓存命令，只交静态结果与阻塞说明。

取得上述窗口后，先建立本次 `RUN_ARTIFACT`，按既有工具链**串行**运行：

```sh
RUN_ID="$(date '+%Y%m%d-%H%M%S')"
RUN_ARTIFACT="$PWD/.artifacts/native/evolution-review/$RUN_ID"
mkdir -p "$RUN_ARTIFACT"
swift test --package-path macos/AeroricKit --filter NativeEvolutionReviewTests
swift test --package-path macos/AeroricKit --filter NativeVisualFoundationTests
swift test --package-path macos/AeroricKit --filter ThemePolicyTests
pnpm native:build
node --check scripts/native/ui-test.mjs
node scripts/native/check.mjs shell
node scripts/native/check.mjs native-design
node scripts/native/check.mjs swift-client
```

构建会编译并拷入host二进制，但这不等于样例启动了host。已有core/构建失败
须与本次差异分开报告，不获准顺手修复非白名单代码。
当前shell/native-design均过滤整个AeroricUIUnitTests，配置floor各246；
swift-client unit为33、TSan为28、schema为2。它们是2026-10-15静态配置，
不是本轮实跑证明，开工重核，不能照旧注释把floor降回219或28。
新增单测数量按实际执行报告/必要时上调，不缩范围让门禁通过。

### 六个UI测试必须逐方法登记

类名/文件名统一 `NativeEvolutionReviewUITests`，登记前缀
`AeroricNativeUITests/NativeEvolutionReviewUITests/`：

1. `testRoleBindingsAreIndependentAndOffline`
2. `testOrdinaryPhaseAndCandidateGatesDiffer`
3. `testLateFindingKeepsHistoryAndScopesDependencies`
4. `testServiceAndToolFailuresRemainDistinct`
5. `testNativeAppearanceKeyboardAndCompactLayout`
6. `testInvalidReviewRequestNeverFallsIntoProduct`

通过真实点击验证场景与状态，不以读源码/常量作断言。入口无flag等测试
可用纯策略测试验证原分支保持；错误目录/环境的矩阵不应为了负向测试
实际启动生产路径。第六例使用安全Caches根，分别演示错误环境或两flag
冲突进入拒绝页，不进产品；明确清除其他继承flag污染。
UI helper可借鉴旧样例测试的账户home解析、唯一未创建目录、启动前后
可靠检查父目录、前台PID、失败也清理自身实例，但不复制其固定旧flag。

UI测试只能通过项目 `pnpm native:ui-test` 链。
当前accessibility gate调用的正是这条链，故安全前置满足后运行：

```sh
node scripts/native/check.mjs accessibility
```

**不要另外无意义重复整套UI测试，也不要发明runner尚不支持的筛选参数。**
当前runner显式登记15个方法，新增六个后应是开工实际清单+6；它的
文件名守卫不保证每个新方法都被运行，必须看逐例日志。

### 运行UI链之前必须满足

- 现有runner会删除固定Caches scratch，预热普通App，并按
  `MacOS/AeroricNative$`广泛终止App；重试还按
  `MacOS/AeroricNative|aeroric-cli serve`广泛终止。它没有进程所有权锁；
  目录隔离不等于进程隔离。
- 延续首次构建前取得的独占窗口，并在UI链开始前再次只读列出匹配PID、
  完整命令、数据目录与所有权，确认用户/其他终端的桌面测试时间窗；
  任何非本次测试实例存在或所有权不明，都停止，不启动runner，
  不为测试先kill用户实例。
- 整段测试期间不并发build/UI runner、不启动业务App/serve，主线程不抢
  桌面焦点/截图。不能取得独占窗口则UI验收记阻塞，继续其他安全检查。
- 预热未带审阅flag，会进入普通App并可能启动隔离测试宿主；原有shell
  测试也不是新样例。只允许既有测试Caches与fixture回归，不导入真实
  任务/连接、不调用真实模型或运行Agent。不能保证此边界时停止报告。
- 本阶段不获准改上述清理机制、扩大kill范围、重置TCC或降低门禁。
  check超时不证明整个进程树已退出；结束后只核对/清理确认属于本次的
  实例，记录遗留/未知，不碰无关进程。

### 原生真窗口与隔离证据

- 新样例窗口的浅/深色、glass/classic、真实系统偏好及显式样例增强注入；
  原生材料、状态语义色、圆角接缝、裸图标、焦点、长证据与窄窗可达。
- S1–S8逐项映射到截图/AX/操作结果；包括版本不匹配、拒绝原因和重置。
  无完整屏幕空间的尺寸单列限制，不伪造窗口测量。
- 有效审阅与拒绝页均不构造产品服务；源代码分流、进程树观察、隔离
  数据目录前后检查相互佐证。常量“host=0”、没有sidebar或未建目录，
  **都不能单独证明无host/RPC/外部副作用**。不为证明而读取生产业务数据。
- macOS14兼容路径、26+Liquid Glass的运行证据分别报告；缺系统即未验证，
  不改部署目标、不用单机编译冒称双系统通过。
- 保存实际ReleaseLocal编译及有效build-settings，确认App与相关包没有
  DEBUG，新入口/fixture已编译排除；不能仅看配置名或Debug测试全绿：

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

Release只验编译/排除，不授权打包、签名配置变更、发布或生产运行。
若需用户手动打开样例，交回经过本轮验证的**Debug bundle路径**及
完整显式env启动命令：新flag=1、旧visual flag=0、env=ui-test、唯一安全
Caches子目录。不能只说“直接打开App”，也不能靠普通启动后切业务路由。

## 7. 给主线程的回报与停止条件

保存 `.artifacts/native/evolution-review/<run-id>/report.md`，提供：

```text
# 任务内进化原生审阅样例01：实施回报
## 批准依据、执行指令、任务路径、main与共享文件所有权
## 开工快照、实际修改文件、相对快照patch与未触碰边界
## S1–S8与角色配置：动作、预期、实际、截图/AX索引
## 真实材料/选择/辅助功能/窄窗：系统、SDK、实测尺寸与限制
## 入口隔离、拒绝矩阵、ReleaseLocal排除证据及其证明范围
## 测试：原始命令、退出码、passed/failed/skipped、逐例登记日志
## UI独占窗口与测试宿主副作用、进程/目录前后核对
## 可复现的Debug样例启动命令、手动审阅路径
## 未验证、偏离计划、已有失败与需主线程决定的问题
```

主线程按开工快照和真实diff审查白名单、规则门槛、真实材料/AX与测试消费，
再请用户看窗口；不凭成功声明验收。视觉通过不表示真实API、独立治理
进程、宿主强制、四家族兼容或长期进化收益已经交付。
后续真实接线必须另行设计并批准；其余89条来源观察和协作/规划/知识等
候选继续讨论，不由本执行包一并批准或排除。
