# 阶段 01：原生视觉基础 — 外部执行模型交接

版本：1.0 · 2026-10-14

**状态：已批准，可交给外部模型按阶段 01 范围实施。v10 视觉修订暂时通过，
原生实施仍须完成本文验收，不代表正式主外壳或全软件已验收。**

批准日期：2026-10-14。用户回复上一轮“确认这版修订，并将第一阶段
原生视觉基础（暂不改业务路由）交给其他模型实施”的问题：

> 可以,这个页面我审查后觉得没什么问题,暂时通过,确认这版修订

本版由草案0.1发布为1.0，**只更新批准状态，不扩大文件白名单、
实现范围或技术决策**。主会话继续设计与监督，不在这里代替外部模型
启动产品实施。执行者无需重复询问已确认的A/B或同一阶段范围；
遇到实质性变更、范围扩展或安全前置条件不满足时停止并报告。
本阶段不是完整外壳或所有工作面的实施许可。

## 1. 给执行模型的任务

你是本阶段执行者，不是重新设计者。根据下列已确认的设计方向，交付
**可复用 SwiftUI 原生视觉基础 + 使用这些真实组件的隔离原生审阅窗口**。
主会话继续与用户讨论其他界面，并审查你的实际改动和证据。

- 仓库：`/Users/macbook/Downloads/同步空间/LYX/Aeroric`
- UI：`macos/AeroricKit/Sources/AeroricUI/`
- App：`macos/AeroricApp/Sources/`
- 只读主参照：`/Users/macbook/Downloads/同步空间/LYX/harness/codeg`

必须先读：

- `AGENTS.md`
- `.trellis/workflow.md`
- `.trellis/spec/aeroric-macos/frontend/index.md`
- `docs/native/adr/001-native-boundary.md`
- `docs/native/frontend-redesign-prompt.md`（本文件是最新分阶段范围补充）
- `.trellis/tasks/10-14-native-redesign-shell/review-round-07.md`
- `.trellis/tasks/10-14-native-redesign-shell/research/shape-policy-mapping.md`

本地最新浏览器审阅页：`.artifacts/native/mockups/shell-v10.html`；
已有服务可用时地址为 `http://127.0.0.1:1437/shell-v10.html`。
审阅证据：`.artifacts/native/mockups/browser-evidence-q7-refinement/`。
这些是布局/造型示意，**不是产品 WebView、真实材质或最终任务页**。
代码仓库之外的 codeg 只读，取层级与交互意图，不复制实现。

## 2. 设计冻结范围

### 已确认，不重新问、不自行推翻

1. 主导航是项目 → 任务，保留全部任务和全部工具。
2. 一条工具栏；左导航、中工作区、可选右检查器，右栏默认关闭。
3. 全局系统字体基线：正文/导航14pt、标题18pt、次级11–12pt，
   导航行32pt；间距按现有4pt刻度。
4. 外壳方向：初始1260×860pt、默认左右320pt；压缩时不自动关右栏；
   小于768pt使用按需原生浮层。400×600pt是适配目标，不是现有能力。
5. 底栏32pt：连接、当前宿主全部项目的运行/待输入分别汇总、详情；
   任务状态/项目/分支留在任务。不把缓存或未确认数量标成实时。
6. 工具统一入口：文件/Git → 项目检查器；IDE、笔记、数据库、SSH、
   设施、Agent工具、远控、运维 → 独立中间工作区；扩展与Agent默认项
   → 设置；当前会话控制留在任务。“独立”不意味着数据范围全局化。
7. **选 A，拒绝 B**：应用自有小控件胶囊，大面板/卡片/弹层/多行输入
   用现有 `DSRadius.xl`（18）；不是整个窗口或大内容区域裁成药丸。
8. **单独图标按钮默认只显示图标**，不要常驻圆形边框或背景。
   仍需 Button 语义、原有命中范围、help、禁用、hover 和原生键盘焦点。
9. 三栏18圆角已经认可；优化圆角相接处的小三角背景，而非删掉圆角。
10. **产品前端全局 SwiftUI + 苹果原生材质 / Liquid Glass**，不是某个
    页面的可选装饰，不是 CSS 模糊、半透明色或 WebView。

### 已暂时通过并冻结的 v10 细节

- 连续真实材质基座覆盖整个应用内容轮廓；面板间留白也属于同一基座。
- 面板内边距8pt、栏间距8pt、18pt连续圆角、弱语义描边；不再用零
  间距圆角描边拼接出孤立三角口袋。不加三角补丁、负间距或截图配色。
- 新预算必须入账：1260样本内部1258，三栏减去内边距16+间距16后，
  为320/586/320；1000样本是284/409/273；小窗内部中心382。
  **后两组只是审阅样本，不是已批准的真实压缩下限或持久化算法。**
- “四外角更圆润”是用户意图；浏览器24pt只是示意，**不能承诺等于
  NSWindow轮廓**。保留标准窗口、交通灯、拖动、resize、全屏行为。
  不使用私有视图/swizzling/无边框窗口强行匹配。不新增全局24 token。

### 后续阶段才能做

真实三栏主外壳、导航树/排序、工具与检查器路由、状态详情、实时汇总
read model、项目/宿主范围、返回路径、草稿/筛选/多窗口/PTY所有权、
每个工作面的内部布局与窄窗适配。本阶段只在隔离审阅窗验证样式。

## 3. 原生实现契约

- 部署目标保持macOS14；真实基座为现有 `NativeMaterial` 的
  `NSVisualEffectView` 或 SwiftUI `Material`。`glassEffect`/原生玻璃
  按钮样式等26+ API必须availability门控，**仅用于交互控件**。
- 内容面板的连续底层不要整块套 `glassEffect`；终端、编辑器、数据
  内容使用不透明 `Color.dsTextBackground`，不逐行叠材质。
  原生Liquid Glass承载操作/导航层，不能牺牲内容阅读来追求特效。
- 裸图标按钮不因“全局玻璃”重新获得常驻圆形玻璃托盘。优先原生
  plain/toolbar语义，交互反馈按平台支持；带文字操作可使用支持的
  胶囊玻璃样式，不用自绘状态或特效替代系统按钮。
- 颜色只用 `Color.ds*` 语义角色。减少透明度优先切classic；增强对比
  提高必要边界；减少动效去掉非必要动画。实际View要读取系统偏好，
  不能只写一个没人消费的策略函数。`glass/classic`与
  `native/advanced`仍是两枚独立开关。
- 邻接互斥选项必须复用 `NativeSelection.swift` 的系统Picker/List/Menu
  语义；不手绘active背景/下划线。系统选中态的实际形态优先于浏览器
  胶囊像素，不用包一层mask破坏焦点/选中状态。
- AX identifier只放叶子，名称必须可读；容器不要吞掉子元素。
  示例也遵守。输入、光标、IME、滚动条与焦点不能被圆角切掉。
- UI不依赖client或解析wire；审阅窗只用本地fixture。不得生成agent
  参数、读取原始会话或改变业务状态/宿主/终端生命周期。

## 4. 文件所有权与允许改动

**这是白名单，不是要求每个文件都必须改。超出白名单先回主会话审查。**

| 文件 | 阶段职责 |
|---|---|
| `macos/AeroricKit/Sources/AeroricUI/Design/GlassPanel.swift` | 修正现有整panel26+ glassEffect；复用真实材质和18面板造型，保留现有公开调用兼容性 |
| 同目录新 `NativePanelGeometry.swift` | 用既有token表达面板18、内边距/间距8及可测预算；不是重新定义整个设计系统 |
| 同目录新 `NativeIconButtonStyle.swift` | 默认裸图标共享样式，保留原生Button语义和辅助功能反馈 |
| 同目录新 `NativeVisualFoundationReviewView.swift` | DEBUG限定的隔离原生样式样本，真实复用上述组件，不复制一套只在样本里存在的实现 |
| `macos/AeroricApp/Sources/AeroricApp.swift` | **仅**DEBUG+显式review flag的隔离窗口入口，普通路径保持原样；此文件已有其他人的改动，必须单写协调 |
| `macos/AeroricKit/Tests/AeroricUIUnitTests/NativeVisualFoundationTests.swift`（新） | 几何、材质/辅助功能策略、兼容性与组件装配回归 |
| `macos/AeroricUITests/NativeVisualFoundationUITests.swift`（新） | 真窗口的图标命名/焦点、输入与系统选项可达性、审阅隔离 |
| `scripts/native/ui-test.mjs` | 仅登记新增UI test案例；保持原有隔离/预热/13案例及执行机制 |
| `scripts/native/checks.json` | 仅新增案例登记或已证实的floor上调；不缩filter、不降floor，已有dirty需协调 |

`NativeMaterial.swift`、`NativeSelection.swift`、`WindowAppearance.swift`可
只读复用；如确需修改它们，先给主会话具体原因和最小diff取得扩展批准。
`ThemeTokens.swift`、`packages/design-system/src/tokens.ts`及其数值锁定
测试只读。`WorkspaceRootView.swift`及所有业务工作面/Store/AppServices、
protocol/core、Tauri/React产品、签名/发布配置均不在修改范围内。

原生审阅入口建议 `AERORIC_NATIVE_VISUAL_REVIEW=1`，仅在DEBUG且
`AERORIC_ENV=ui-test`、显式隔离Caches数据目录下生效；release忽略。
这是本阶段要新增的入口，不是声称现有App已支持该变量。
审阅分支不启动宿主、不运行真实任务/Agent、不挂载PTY，不执行生产
command路由；默认未设flag的App逻辑逐字保持语义不变。
使用现有Xcode/SPM/native构建链，不新增临时独立App/WebView。

**交付口径：**共享组件与原生审阅窗已接线；正式主外壳集成未完成。
仅加`#Preview`或策略单测，不能算真窗口交付，更不能称全软件已改造。

## 5. 执行流程与协作保护

1. 核对本文1.0的批准记录与冻结范围；本阶段批准已记录，不需再次
   请求同一范围的批准。实质改计划或使用后续未批准版本时止步只读。
2. `git branch --show-current`必须为`main`。不创建/切换新分支、
   worktree，不commit/push。用户需要时才进行后续Git操作。
3. 保存开始时的`git status --short`、相关已有diff及文件SHA-256到
   `.artifacts/native/stage-01/<run-id>/`。本仓库已有大量dirty/untracked，
   不要stash/reset/clean/整文件覆盖。以开工快照而非HEAD区分你的diff。
4. 向主会话报告白名单的写入所有权；已有dirty共享文件先单写协调。
   主会话继续写设计文档，不与执行者并发写这些产品文件。
5. 不重新创建已有父任务/外壳任务。批准后建立独立
   `native-redesign-visual-foundation`子任务（若已存在则复用），父为
   `10-14-native-frontend-redesign`。PRD/design/implement与context记录
   本阶段批准范围，再遵循Trellis进入执行；不启动未收敛的外壳任务。
6. 先记录现有约束测试结果，再做最小组件改动；共享样式被审阅窗
   实际消费。发现本计划无法做到/遇到冲突，报告问题，不自行换技术。
7. 新UI测试必须登记到`ui-test.mjs`逐例列表；对应门禁必须实际执行
   它们。禁止只创建测试文件而没有runner消费。
8. 自查diff、完成以下验证、交回完整回报后停止。由主会话独立审核；
   执行模型自报“完成”不代表接受，也不能擅自开始阶段02。

## 6. 验收清单与命令

必须展示：

- 真SwiftUI窗口浅/深色的面板连续底与18圆角，三栏接缝无孤立高对比
  三角；裸图标默认无边框/托盘，hover/禁用/键盘焦点反馈正常。
- 真实系统选择态、单行/多行输入和滚动可达，无文字/IME/焦点裁切；
  AX叶子可独立查找、名称可读。纯DOM/源码检查不能替代。
- glass/classic、减少透明度、增强对比、减少动效确实被View消费。
  单测验证策略，真窗口验证实际效果；不要修改用户全局系统设置来
  静默完成测试，可在DEBUG样本明确标记测试注入并另核对系统默认。
- macOS26+原生Liquid Glass控件和macOS14兼容路径分别报告证据。
  如果缺少某系统环境，写“未验证”，不能把编译通过写成运行验收。
- 普通未设review flag的App仍走原有shell/菜单/宿主路径，现有测试
  保留；审阅窗使用隔离fixture，不触碰生产目录/任务/Agent/会话。
- review入口必须在实际 `ReleaseLocal` 配置中不可用。补充该配置的
  编译与build-settings证据，确认没有DEBUG条件；入口及fixture用
  编译条件排除，不能仅运行时读环境变量。DEBUG下缺flag、错误环境、
  未提供隔离目录等负向用例也必须执行。仅Debug全绿不证明release安全。
- 1260/1000/400样本只验证共享几何；不声称正式主工作面已支持400。
  标准窗口四角实际结果截图回报；无法精确匹配示意24时单列限制，
  不用私有方案“解决”。

顺序运行并保留完整日志、退出码、通过/失败/skip数：

```sh
swift test --package-path macos/AeroricKit --filter NativeVisualFoundationTests
swift test --package-path macos/AeroricKit --filter ThemePolicyTests
swift test --package-path macos/AeroricKit --filter ShellPolicyTests
pnpm native:build
node scripts/native/check.mjs shell
node scripts/native/check.mjs native-design
node scripts/native/check.mjs swift-client
node scripts/native/check.mjs accessibility
xcodebuild -project macos/AeroricNative.xcodeproj -scheme AeroricNative \
  -configuration ReleaseLocal -destination 'platform=macOS' \
  -derivedDataPath .artifacts/native/stage-01/ReleaseDerivedData \
  CODE_SIGNING_ALLOWED=NO build
```

另外用同一project/scheme/configuration的 `-showBuildSettings` 保存有效
`SWIFT_ACTIVE_COMPILATION_CONDITIONS`；检查App和相关包的Release编译
记录，不人为添加DEBUG来让review代码通过。上述Release命令只验编译，
不是打包/签名/发布许可，也不等于已运行Release窗口；分别报告证据。

XCTest UI只能通过项目`pnpm native:ui-test`链执行；当前accessibility的
`accessibility-ui-test`已调用同一 `scripts/native/ui-test.mjs` 入口，
无需再无意义重复全套。开工时重核配置，若入口有变化先报告。
原生`native-design-unit`与`shell-unit`目前均过滤整个UI unit target，
floor目前219；不假设上述单独filter就代表整个模块通过。
不要降低floor/缩减测试以让门禁变绿；新增数量依据实际执行登记。
环境阻塞、已有失败、opt-in skip和本阶段失败分别报告，不隐瞒。

真窗口截图和AX文本一起存放到阶段artifact目录。使用
`~/Library/Caches/`下独立数据目录，保留现有LaunchServices预热。
测试runner有共享缓存/日志，应串行运行，不与其他终端native gate争用。
**UI runner安全前置条件（未满足必须停止，不能冒险运行）：**
现有预热按 `MacOS/AeroricNative$` 终止进程，重试还会按
`MacOS/AeroricNative|aeroric-cli serve` 广泛终止App及宿主。Caches隔离
不隔离这些进程。执行accessibility/UI链之前，只读列举匹配的PID、
命令和数据目录、核对所有权；存在任何非本次测试实例时停止并报告，
不得启动runner，也不得为了测试先kill它。与其他终端约定独占原生
测试时间窗，整个期间不得启动业务App/serve。无法取得独占窗口则把
UI验收标为阻塞，仍可完成不启动该runner的其他工作，不假称通过。
本阶段不改这个既有清理机制；如需安全修复，另交主会话批准具体范围。
真窗口UI链运行时，主会话仍可讨论设计和写文档，但暂停浏览器/桌面
点击与截图操控，避免抢焦点导致测试命中失败；不因此关闭用户窗口。

## 7. 交回主会话的 Markdown 回报

保存为 `.artifacts/native/stage-01/<run-id>/report.md`，告诉用户路径。

```text
# Stage 01 implementation report
## 批准依据：文件版本/日期、用户阶段批准原话
## 开工基线：branch、已有dirty快照、写入所有权
## 改动文件与目的：各文件只列本阶段diff；提供patch路径
## 设计逐项映射：A/裸图标/18圆角/连续底/原生玻璃/AX
## 验证：实际命令、退出码、passed/failed/skipped、日志路径
## 真窗口证据：截图、AX、系统版本、SDK、尺寸、外观/偏好
## 隔离和未改项：生产入口、任务/Agent/PTY、dirty与参照保护
## 尚未验证/偏离计划/需要主会话决定
```

**主会话审查规则：**看开工基线与实际patch，核对白名单、材料/选择/
AX边界和截图；确认测试实际被runner执行，不靠标题或成功声明。
未经确认的设计变化退回；通过审查后才请用户接受本阶段和开启下一段。

## 8. 官方材质依据（只读参考）

Apple官方SwiftUI文档标识：`adopting-liquid-glass`、
`applying-liquid-glass-to-custom-views`；
Apple Human Interface Guidelines：`materials`、`components`。
用于核对原生API/控件支持，不替代本项目更严格的compatibility、
interactive-control-only与辅助功能契约。
