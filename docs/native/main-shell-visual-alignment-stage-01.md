# 正式主壳视觉对齐与 Git／远控布局修正 — 阶段 01 交接

版本：1.0 · 2026-10-20 · Asia/Shanghai。
**用户已确认本阶段的设计整理与交接范围。本轮只交文档；
不是产品已实现、已接入、已编译或已获视觉验收。**

本阶段与 `frontend-redesign-stage-01.md` 的“视觉基础＋隔离审阅窗”
不同：未来实施必须改到**普通产品入口实际使用的主壳和两处工作面**，
不得再以新增 DEBUG 样例、`#Preview` 或浏览器页面代替正式接入。
不修改旧阶段包、不自动启动仍为 planning 的完整外壳任务。

用户将本文交给新终端并明确要求执行后，执行者仍须获得独立任务创建
授权、完成规划审阅及 `task.py start`、取得文件和构建资源单写交接。
本次“确认”不自动创建任务、不授权本线程实施、commit/push 或发布，
也不恢复 App/UI/VM 运行。主线程继续设计与独立审查。

## 1. 确认依据与问题定性

用户先要求核对 `docs/native/ui-testing-policy.md`，并反馈普通 App
与此前浏览器确认稿相差很大；随后提供两张现成截图：

| 用户截图（仓库内副本） | 可见内容 | SHA-256 |
|---|---|---|
| `tmp/Snapzy_2026-10-20_15-41-41_253.jpg` | Git | `bf707b55c92f7aca938f980a9161811e77125e392c31a6662ec34f4544498965` |
| `tmp/Snapzy_2026-10-20_15-41-03_790.jpg` | 远控 | `7cd28f828ade621c9f2602b6c045185e6703150b7b6af80fc278cdc780e44fad` |

已只读检查图片及当前源码，没有重新启动、捕获或操作窗口。
两张图可以证明所提供画面存在裁切、挤压、重复内容和旧布局，
不能证明当前二进制身份、数据正确性、业务调用或全部原生用户旅程。

主线程建议优先收敛“正式主壳视觉对齐＋Git/远控布局修正”，用户回复：

> 确认

据此整理本文。不要重新要求用户选择已否决的 B 风格，也不要以
“原生只能如此”推翻已认可的圆润、连续材质、裸图标和系统选择态。

### 1.1 已查源码事实（2026-10-20 快照，不是运行证明）

| 位置 | 当前事实与影响 |
|---|---|
| `macos/AeroricApp/Sources/AeroricApp.swift` 的 `AeroricNativeApp.mainContent` | 普通入口挂载 `WorkspaceRootView`，ready 内容为 `TaskWorkspaceView`；不是没有接线的启动占位页 |
| `macos/AeroricKit/Sources/AeroricUI/Shell/WorkspaceRootView.swift` | 主体仍是零间距侧栏＋详情；已有材质背景，不等于已消费新版面板结构 |
| `macos/AeroricApp/Sources/TaskWorkspaceView.swift` 的 `.git` | 将自带分栏的 `GitPanel` 限制为最高400pt，又额外放置 `HistoryView` |
| `macos/AeroricKit/Sources/AeroricUI/Git/GitViews.swift` | `GitPanel` 内部左右最低宽度为240和320pt，尚未计分隔线便超过外层400pt；内部已有历史视图 |
| `TaskWorkspaceView.swift` 的 `.remote` 与 `Remote/RemoteViews.swift` | 外层又放一份 `DeviceListView`，设置页内部也有设备列表；表单被限于最高420pt的窄栏 |
| `macos/AeroricApp/Sources/AppServices.swift` | `GitStore()` 初始为空；远控为 `RemoteStore(client: nil)`。没有数据不等于已核实的干净仓库，主连接 ready 不等于远控可用 |
| `macos/AeroricApp/Info.plist` | 当前应用名称本来就是 `Aeroric Native Preview`；标题不是另一个旧程序或某个进程的指纹 |

Git 当前调用处未提供 `onAction`，该回调默认可为 nil。执行者必须
重新核对最新代码，不能因为已有 Button 就宣称 Fetch/Pull/Push 可用。
本文不批准为补齐这些按钮而新增整套 Git 或远控业务执行链。

已有启动留档还记载过小尺寸窗口和另一安装实例；这与用户提供的
完整工作区截图不是同一个已证实的观察。不得据此断言截图来自某个
PID/安装路径，也不得为了确认身份擅自重启、附加进程或操作窗口。

## 2. 优先规则与必读源

1. `AGENTS.md`、`.trellis/workflow.md`、
   `docs/native/ui-testing-policy.md`。
2. `MIGRATION_PLAN.md` 顶部 Apple-only 基线、
   `docs/native/adr/039-distribution-pivot-macos-ios.md`。
3. `docs/native/frontend-redesign-prompt.md`；
   `docs/native/frontend-redesign-stage-01.md` 只读参考既有造型与边界。
4. `.trellis/tasks/10-14-native-redesign-shell/review-round-07.md` 至
   `review-round-12.md`，按相关决定窄读，不整库注入。
5. `.trellis/spec/aeroric-macos/frontend/index.md`，以及涉及 Shell、
   Design、Git、Remote 的现有源码和定向测试。该索引引用的
   `native-ui.md` 当前缺席，执行规划记录这个缺口，不杜撰已读结果、
   不创建空指南；有效约束以现行 AGENTS、Apple-only 基线及ADR为准。

平台目标仍为 SwiftUI macOS＋iOS/iPadOS、Rust core/host、双 App Store。
最低系统原生支持 Liquid Glass，以 Apple-only 基线为准；
旧阶段包和旧 spec 的 macOS14 描述不能覆盖它。本阶段不负责修改
deployment、依赖、签名、沙箱、CI、发布或 iOS 配置，不把当前预览包
的旧配置/本地 ad-hoc 构建当作目标平台或商店验收。

UI 是 SwiftUI，必要时复用现有 AppKit 桥接；不替换为 React/WebView。
业务数据和操作仍经既有客户端边界，不在 View 中运行 Git/SSH 命令、
构造 Agent argv、解析原始会话或直接写共享业务数据。

## 3. 本阶段做到哪里，哪里仍未完成

### 纳入本阶段

- 正式 `WorkspaceRootView` 的连续底座、面板、留白、标题/状态区域
  和系统控件视觉一致性；使用共享原语，不复制另一套设计系统。
- 正式 `.git`、`.remote` 两分支的容器职责、宽度预算、内容去重、
  表单布局及真实可用性呈现。
- 与上述变化直接相关的窗口内呈现状态、AX 叶子名称、系统选择态、
  辅助功能偏好消费与纯值/fixture 回归。
- 普通产品路径的消费证据；只改 Review 目录不算本阶段接入。

### 不纳入本阶段，但必须保留在差异台账

- 新首页 home-v3、首页替换默认入口、完整工具库及其生产数据接线。
- 项目→任务嵌套树、统一工具入口/文件与Git移入检查器的完整路由、
  新检查器所有权、多窗口恢复和选择/草稿/过滤器迁移。
- 进化、知识、技能、工作安排、沟通、成果、工具连接、使用统计等
  离线 Review 转为生产能力；不把这些样例塞进正式菜单冒充可用。
- Git/远控真实客户端接线、权限、凭据、网络服务、迁移及运行验收。
- 所有其他业务工作面的内部重设计与400×600正式窄窗支持。

这是**正式主壳形态＋两页修正**，不是完整导航重构。项目→任务、
统一工具入口等已确认的产品方向继续有效，不重新否决；
只是相关状态/路由契约不在本文中默默补定。当前侧栏工具较多这一
差异不能靠圆角消除，回报中必须明确“仍待后续导航集成”，不可称
“整壳已完全等同浏览器稿”。

Q11无当前项目时“全部任务”的方向不被首页候选替换；
不得因空列表、切到无项目工具或重新包裹 View 而重置当前明确目标。
本阶段沿用现有路由标识、项目范围和初始化机制，不新增选择恢复规则。

## 4. 视觉基准与正式主壳契约

### 4.1 基准的身份

- 已暂时认可的造型：`.artifacts/native/mockups/shell-v10.html`、
  `shell-v10-refined-light.jpg`、`shell-v10-refined-dark-launcher.jpg`。
- 首页最新候选：`.artifacts/native/mockups/home-v3.{html,tsx,css}` 及
  `browser-evidence-home-v3/`。它不是已批准的新默认首页或生产功能表。
- 用户本次两张截图是“当前差异”资料，不是要继续沿用的外观基准。

浏览器图只传达布局、密度、层级和造型；不复制 CSS 玻璃、背景色
采样或手绘选择态。原生系统材质与选中外观允许平台差异，
不允许把整体层级、裁切或内容重复归咎于这种差异。
保全旧图、旧HTML/TSX/CSS和来源记录，不覆写成“修订后证据”。

### 4.2 正式主壳

1. 复用 `NativePanelFoundation` 形成覆盖内容轮廓及栏间留白的连续
   基座。主导航壳和主工作区壳之间留8pt，外围内边距8pt，
   大面板使用既有 `DSRadius.xl` 18pt连续圆角。
   不以零间距描边拼接三角空隙，不加补丁或负间距掩盖问题。
2. 导航/操作层使用苹果原生材质及原生 Liquid Glass 控件。
   不把整个窗口或大段正文套 `glassEffect`；文本、表单、diff、终端、
   编辑器等阅读区域用稳定语义底色，避免每层都透明导致桌面透入正文。
3. 复用 `GlassPanel`、`NativeIconButtonStyle` 和 `NativeSelection`。
   单独图标默认无常驻圆形边框/托盘，保留 help、禁用、hover、
   原生命中范围和键盘焦点；短文字操作承接胶囊语言，多行/大区域18圆角。
   系统控件的选中/焦点形态优先，不用 mask 裁掉它们以强行变药丸。
4. 正文/导航14pt、标题18pt、次级11–12pt，导航行32pt基线、
   状态区域32pt；复用已有语义角色。系统可访问性需要更大空间时
   允许增长，不压缩字号、命中区域或截掉焦点去凑样本。
5. 保持标准窗口、交通灯、拖动、resize及全屏行为。
   浏览器24pt外轮廓是示意；不私有API、swizzling或无边框窗口仿造。
   不为清理 `Native Preview` 字样顺手修改产品身份/签名/Info.plist。
6. 初始尺寸/侧栏320pt沿已确认的1260×860方向处理，仅适用于没有
   恢复几何的初始呈现；不得覆盖用户既有窗口状态。
   本阶段不降低正式窗口当前1000×640下限，保留已有侧栏调整范围和
   56pt折叠轨道行为，不借本轮另定持久化、自动收栏或窄窗规则。
7. 为正式窗口重算预算：可用内容宽度减外内边距、可见栏宽、
   实际分隔/间距后才分给主区。`NativePanelGeometry` 的 review
   `outlineInset=1`、320/586/320、284/409/273等是样本预算，
   不能原样作为生产压缩算法或凭空扣掉系统窗口边框。
   最小宽度冲突必须调整容器职责或回报，不把负值夹为0后声称可用。
8. `glass/classic` 与 `interactionMode` 分开。减少透明度优先转
   不透明语义底色，增强对比强化必要边界，减少动效移除非必要动画。
   偏好必须被正式 View 消费，不仅存在于没人使用的策略函数。

`WorkspaceRootView` 应继续持有每窗口选择/呈现状态并调用既有
`readyContent`，不能把业务 Store 搬进壳。保持 focused values、
命令面板、侧栏键盘顺序、项目范围和普通生命周期。
视觉切换、切换下面的页内标签不得重启 Agent、切换模型/权限、
重建会话或改变 PTY generation。

## 5. Git：一个工作面，一个布局所有者

### 5.1 目标组织

```text
Git 工作面（填满主工作区，不再塞进外层最高400pt的窄栏）
  项目/仓库上下文＋一处操作区
  系统选择器：更改 | 历史 | 贮藏（Stash）
  更改：变更列表＋diff正文（只在这里分栏）
  历史：一份历史内容，沿用已有能力
  贮藏：一份stash内容，沿用已有能力，不与stage暂存混称
```

系统选择器复用 `NativeSelection.swift`；页内选择是窗口局部呈现，
不自动拉取仓库、切分支、暂存、提交或重放操作。
缺少提交详情能力时不造可点击的假详情；切标签不丢尚有价值的输入。

- `TaskWorkspaceView` 的 `.git` 不再给完整 `GitPanel` 外套第二层
  “窄栏＋另一份历史”。布局职责集中到一处，复用现有叶子 View。
- 更改模式只有一条分隔，预算包含两侧真实最小内容宽度与分隔线；
  必须能在主壳1000×640和1260×860的可用内容区内成立。
- 历史、stash 不再同时挤占半屏制造重复空态。正常空态简洁居中于
  它负责的区域，不让一个空态或裸 `Spacer` 推走真实表单/操作区。
- 长路径/提交信息可读、diff允许横纵滚动；不截断原始 diff，
  不用增大整个窗口的最小宽度掩盖内层560pt/外层400pt冲突。

### 5.2 数据与动作不能伪装

区分至少以下可解释情况，不要求照此冻结生产schema：

| 已知依据 | 呈现要求 |
|---|---|
| 未明确仓库、没有读取链或从未成功加载 | 显示“尚未加载仓库状态／本页尚未接入”，不显示已核实的干净、无提交或0变更 |
| 有真实加载中的依据 | 显示加载中，不触发第二次读取来修外观 |
| 明确成功快照且该快照的变更列表为空 | 才能针对该仓库/快照显示“工作区干净” |
| 明确成功历史快照为空 | 才能显示“还没有提交” |
| 失败或旧快照 | 保留来源/错误/旧值限制，不当作当前成功空态 |

允许在 UI read model 增加最小的“是否有已加载依据／能力是否接入”
呈现信息，由真实已有结果或显式 fixture 提供；缺少依据默认未知/
未接入。不能仅因数组非空就认证当前仓库，也不能为了 UI 置一个
永久成功布尔值。当前产品若没有 Git 快照链，诚实显示未接入；
新增业务采集、跨项目绑定、客户端方法或执行内核须另行审查。

`onAction == nil` 或缺少上下文/可用能力时，对应操作必须禁用并解释，
不能显示正常可执行却静默无事发生。已有合法回调保留其语义，
不借禁用逻辑删掉既有可用能力。Push/丢弃/冲突覆盖等既有确认不减少，
本阶段不提交、推送或操作用户真实仓库。

## 6. 远控：可读的设置页，不是窄栏里的整页表单

### 6.1 目标组织

```text
远程访问工作面（使用整个主工作区，内容自顶向下）
  本页能力/连接状态
  服务：开关、状态、配对入口
  连接：自托管 Relay / 公网端点配置
  设备：一份已配对设备列表
  适用的错误/限制说明
```

- 移除 `.remote` 外层将整页设置塞到最高420pt、再并排展示相同设备的
  组合。由一个工作面负责滚动和分区，设备表只保留一处 owner。
- 表单用原生 `Form`/分组与稳定的标签、输入宽度；容器需要时纵向
  滚动。长标签可换行或上下排列，不让标签伸进侧栏、输入只剩几字宽。
- 宽窗可并列不重复的分组，剩余宽度给可读正文；最小窗口自动采用
  同页纵向布局，不新增全局路由或400pt窗口能力承诺。
- Relay 口令保持 `SecureField`，不在截图说明、日志、错误摘要、
  fake fixture 或复制/导出中塞真实秘密。不读取现有真实配置作样本。
- 保存、启停、配对、撤销等已有业务意图和安全边界不改变；
  不为了演示而启动监听、暴露公网、生成真配对码或连接 Relay。

### 6.2 主连接与远控能力分开

底栏明确它描述的是“宿主连接”，不是全软件健康灯。
远控页单独说明客户端是否接入、有没有服务快照、快照是否有效。
主连接 ready 与远控尚未接入可以同时为真，应说明范围，不把主连接
改成断连，也不把远控涂绿冒充可用。

当前 `client: nil` 时应显示“本页尚未接入远控通道”等准确提示，
而不是将它当作整个宿主不可用；禁止展示默认 `.offline()` 为
“刚核实服务已停止／正在监听某端口”。默认端口若需保留，只能明确
作为尚未核实的配置值，不是运行事实。

缺客户端的开关、保存、配对等操作禁用并说明原因，不能静默无效。
有真实客户端/成功快照的原有路径保持；不靠 UI 显示补出真实能力。
仅新增最少的呈现状态，不把客户端、凭据或服务生命周期搬入壳。

## 7. 文件白名单与单写边界

以下是**未来执行候选白名单，不是每个文件都必须改**。开始前重核
实际调用关系、dirty和同期其他终端；共享文件必须获得单写交接。

| 文件（相对仓库） | 允许职责 |
|---|---|
| `macos/AeroricKit/Sources/AeroricUI/Shell/WorkspaceRootView.swift` | 正式主壳消费连续底座、面板与预算，保留状态/路由边界 |
| 同目录 `ProjectSidebar.swift`、`WorkspaceDetail.swift`、`TitleBar.swift`、`StatusBar.swift` | 直接相关的密度、边界、页头/底栏范围说明，不重构导航模型 |
| `macos/AeroricApp/Sources/TaskWorkspaceView.swift` | 仅 Git/remote 两工作面装配与所需局部呈现状态；其他分支语义保持 |
| `macos/AeroricApp/Sources/AeroricApp.swift` | 如确需，只改普通主窗口的初始视觉几何；旧 Review 分流、拒绝路径和生命周期逐项保全 |
| `macos/AeroricKit/Sources/AeroricUI/Git/GitViews.swift` | 单一布局 owner、页内选择、空态和动作禁用，不新增执行内核 |
| 同目录 `GitStore.swift` | 仅最少UI可用性/快照依据，不自行读取仓库 |
| `macos/AeroricKit/Sources/AeroricUI/Remote/RemoteViews.swift` | 去重、原生表单布局、作用域明确的可用性，不新增客户端/联网 |
| `macos/AeroricKit/Sources/AeroricUI/Design/NativeMainShellLayout.swift`（如需新增） | 正式壳宽度纯计算/策略，不混入Review样本或新token |
| `macos/AeroricKit/Tests/AeroricUIUnitTests/MainShellVisualAlignmentTests.swift`（新） | 纯值几何/外观策略回归 |
| 同目录 `GitWorkspaceLayoutTests.swift`、`RemoteWorkspaceLayoutTests.swift`（新） | 布局策略、数据/动作可用性、去重投影的内存fixture回归 |

`AppServices.swift`、`AppCommands.swift`、全部 Design 既有原语和
token、既有测试/门禁注册、Package/project配置先只读复用。
若现有接口确实无法表达已获准的最小呈现变化，带具体符号、理由、
最小diff及受影响调用方回主线程扩审；不能自行把它们加进白名单。
既有单测如需随合法行为变更更新，单列断言迁移方案申请，不删断言、
降floor、缩filter或改测试期望来遮住回归。

不改 core/protocol/AeroricClient、旧客户端、iOS、发布、签名和
VM/UI政策。不改各 Review 目录或其入口契约、不加入新的审阅flag；
不改冻结阶段MD、截图与浏览器样本。越界先停在相关子项，
独立且不冲突的文档工作可继续。

## 8. 新终端执行顺序

1. 确认用户已明确交办本文的执行及独立任务创建。复用已存在的匹配
   子任务；若无且获准，可在既有 frontend-redesign 父任务下建
   `native-main-shell-visual-alignment`，不创建同名平行任务，
   不激活范围更大的完整 shell 任务。
2. 留在 `main`，不开分支/worktree，不commit/push。保存开工原文、
   hash、相关dirty diff、截图/样本引用和文件缺席基线；不按HEAD
   反推自己的改动，不stash/reset/clean或覆写他人的dirty。
3. 先交 PRD/design/implement、context和“浏览器意图→正式符号→差异
   →改法→未完成项”矩阵。列出本阶段允许的几何、可用性和View职责，
   不默定后续树/工具库/检查器契约。经规划审阅后才 `task.py start`。
4. 协调白名单文件及 SwiftPM scratch、Xcode工程生成、DerivedData、
   Cargo target/签名产物等资源的唯一写入者。争用就报告；
   不强杀占用者、删锁、覆盖别的终端产物或重装工具。
5. 先建立能暴露现有400pt/560pt冲突、重复面板、未加载误报成功、
   缺客户端动作可点问题的定向回归，再最小修改正式主壳和两工作面。
   纯值策略必须被实际View使用，不能只测试另一套不会被产品消费的模型。
6. 按下一节逐条审查并执行无窗口检查。每步记录原始命令、日志、
   退出码、匹配suite/case、passed/failed/skipped和实际产物；
   遇到既有失败、依赖不足或权限/资源问题如实分开，不修无关范围。
7. 对比开工基线交回自己的patch、正式入口消费证据与逐行差异矩阵。
   主线程独立审查，不自行宣布用户验收或开下一阶段。

## 9. 检查候选、运行禁令与完成口径

### 9.1 本阶段必须覆盖的定向断言

- 正式预算使用可见栏/实际分隔，1000和1260宽度不出现重叠或负主区；
  默认与折叠轨道都守恒，不套Review的1pt outline。
- 一个Git布局owner；历史和stash仅在各自模式出现一次；
  模式切换不触发业务回调或改会话。
- 未加载/加载/成功空/失败/旧值区分；nil操作回调不能呈现为可执行，
  真实成功空快照仍可显示正确空态，不以“一律隐藏”回避状态问题。
- 远控页只有一份设备列表；缺客户端/未知快照不显示已核实运行事实，
  动作禁用；已有可用客户端路径和敏感输入语义保留。
- 壳的连接、远控能力、数据加载分别说明；不会以全局ready授予局部可用性。
- 正式View使用共享外观/系统选择原语，读取减少透明度/对比/动效策略；
  不改interactionMode、任务/Agent/PTY、项目范围或旧Review生命周期。
- 长标签、长路径、多行输入和错误内容的布局策略有回归；
  纯值断言不冒充实际文字排版或焦点可达性。

### 9.2 只允许审查后的非窗口检查

先检查每个测试、setUp/tearDown、静态初始化、测试发现及构建脚本的
副作用。仅使用假客户端、内存fixture和独立临时根；
不访问真实Agent home、业务库、仓库工作区、凭据或用户偏好。
不能因test target名为unit就整组运行。
依赖缺失时报告，不自动联网下载/安装、更新锁文件或换依赖来补环境。

未来新增suite确认无窗口/无输入/无真实业务副作用后，可用以下候选
命令；这些suite在本文形成时尚未实现或执行，命令不是当前通过记录：

```sh
# CHECK_ROOT 由执行者新建为专用临时目录，先完成资源和测试副作用审查。
: "${CHECK_ROOT:?须先设置经审查的专用临时目录}"
swift test --package-path macos/AeroricKit --scratch-path "$CHECK_ROOT/swift" --filter MainShellVisualAlignmentTests
swift test --package-path macos/AeroricKit --scratch-path "$CHECK_ROOT/swift" --filter GitWorkspaceLayoutTests
swift test --package-path macos/AeroricKit --scratch-path "$CHECK_ROOT/swift" --filter RemoteWorkspaceLayoutTests
```

既有 `ShellPolicyTests`、`GitStoreTests` 等先审查最新源码再选纯值用例，
相关既有覆盖不能删减。`NativeVisualFoundationTests` 含
`NSHostingView` 装配，不自动将整suite列入本阶段运行许可；
离屏项若另经非窗口审查，也只证明装配，不能替代真实视觉验收。

需要验证App侧消费时，可在独占现有构建资源且重新审查脚本/生成步骤
后使用 `pnpm native:build` **纯构建**，不跟随 `open` 或任何预热/runner。
当前脚本会重生成工程、编译Rust helper/Xcode、复制并本地ad-hoc签名；
不是无写入的静态检查，也不是双商店、沙箱或运行验收。
本阶段不修改该脚本、构建配置或安装缺失工具来绕开阻塞。

### 9.3 继续禁止的操作

- 不新增启动 App/Preview/真实窗口、自动截图/AX/输入、手动自动化、
  XCUITest、`pnpm native:ui-test`、`pnpm native:check accessibility`。
- **不创建、克隆、导入、下载、恢复、启动或重建任何测试VM**，
  不准备替代VM或重装VM工具；保留已有证据与退休回执。
- 不以环境变量、隐藏窗口、另一Space/显示器/账号、远程或旧helper
  绕过暂停；不强改TCC/系统偏好、真实连接/数据或真实Agent运行。

`ui-testing-policy.md` 的2026-10-20例外只覆盖已记录的一次手动启动，
不继承为本阶段或第二次启动许可。用户自行提交的两张截图可以继续
只读比较，不能标为修改后新验收；未来真UI运行仍需新的明确授权、
资源/权限审查和经审查的政策更新。当前真实材质、窗口布局、键盘/
焦点/IME、AX/VoiceOver、多窗口与用户旅程均为 **NOT RUN / DEFERRED**。

### 9.4 三层交付，禁止混报

| 层级 | 可支持的结论 | 不能据此声称 |
|---|---|---|
| 设计/文档确认 | 本阶段范围、规则与执行边界有记录 | 代码已改、正式入口已接入 |
| 源码接入＋定向静态/编译/非窗口检查 | 普通入口引用实际实现及对应检查结果 | 真Liquid Glass、文字未裁切、键盘/AX或产品业务已验收 |
| 后续另获授权的真实画面/旅程审查 | 按届时实际证据逐项记录 | 用旧图或编译全绿替代未观察的场景 |

## 10. 回报与监工停止点

至少提供一份这样的差异闭环表，状态不得只写“已完成”：

| 对照项 | 基准/当前问题 | 正式消费符号与改动 | 实际检查结果 | 真画面验收/遗留 |
|---|---|---|---|---|
| 主壳连续底座/圆角/间距 | v10对照截图 | 实际Root/子View | 命令、退出码、断言 | NOT RUN / DEFERRED |
| 图标/控件/正文材质与辅助偏好 | A规则与系统语义 | 正式View消费 | 同上 | NOT RUN / DEFERRED |
| Git宽度/重复内容/状态/动作 | 用户Git截图 | 普通`.git`入口 | 同上 | NOT RUN / DEFERRED |
| 远控表单/设备去重/能力范围 | 用户远控截图 | 普通`.remote`入口 | 同上 | NOT RUN / DEFERRED |
| 新首页、任务树、统一工具/检查器 | 仍属后续集成 | 本阶段不实现 | 不冒充通过 | 明确未完成 |

另附任务/规划授权、main、开工dirty及相对patch、单写资源交接、
新增测试及实际覆盖、无关工作保护、未运行项和扩审问题。
普通产品入口仍可触达、旧Review链保持，并不自动证明真实业务可用。

发现范围需要扩大、来源变化、可用性无法诚实表达或文件冲突时，
回主线程决定。不得另起“好看的孤立样例”替代正式修正，
也不得为了交付截图偷偷恢复App/UI/VM运行。
