# ADR-042：顶层导航是分组侧栏，不是平铺 tab 条

- 状态：已接受（2026-10-13 用户明确指示：以 `LYX/harness/` 下各客户端前端为参照，只取样式/排版/交互，实现仍是 Apple 原生 SwiftUI + Liquid Glass）
- 日期：2026-10-13
- 相关：ADR-001 §4（相邻互斥控件的原生等价规则）、`AGENTS.md` 原生选择规则、G50（信息架构缺陷）、AP8 子任务 `10-13-ap8-full-journey-faults`

## 背景

原生客户端此前把 12 个工作面平铺成**一条分段控件**（`TaskWorkspaceView` 的 `WorkspaceSection`），
横跨详情列顶部。实测（2026-10-13，真窗口 1516pt）：

- 12 段 + 被渲染成可见标签的 VoiceOver 组名「工作区视图」，占满整条；
- 日常动作（跑任务、看任务）与机器级配置（运维、远控）**视觉权重完全相同**；
- 用户原话：「界面非常杂乱，与我的设想有非常大的差别」。

这不是"控件放错了位置"，是**信息架构缺失**：把所有能力都画出来不等于产品布局。
G50 已把它登记为缺陷，但当时只有"测量更正"，没有决策。

## 参照依据（只取样式/排版/交互）

`LYX/harness/` 下七个独立客户端在这一点上**收敛**，这是最强的信号：

| 项目 | 事实 | 出处 |
|---|---|---|
| codeg | 侧栏分组为 `folders / chats / recent` + `pinned` 桶；可选导航行 `automations/tasks/forge/canvas` 是**侧栏行**不是 tab | `src/lib/sidebar-view-mode-storage.ts:30,39,74-79` |
| codeg | 文档 tab 条在标题栏，导航在侧栏——两者不混 | `src/components/tabs/tab-bar.tsx`、`src/components/layout/sidebar.tsx` |
| codeg | 辅助面板 4 个 tab 有**可见顺序表**，桌面分段控件与折叠下拉共用同一张表，保证两处不漂移 | `src/components/layout/aux-panel.tsx:38-45` |
| opencode | 折叠侧栏是 **64px 图标栏**，不是隐藏抽屉 | `sidebar-shell.tsx:47-48` `w-16 shrink-0` |
| deepseek-harness | 三栏 264–420 / ≥640 / 300–520，折叠常量 **56** | `columns.ts:21-39` |
| ZCode | 有专门的折叠栏文件 | `WorkspaceSidebarCollapsedRail.tsx` |
| 全部 | 列表**分组**，从不平铺；行高 34–40、字号 11–14 | `deepseek Rows.tsx:386`、`opencode home-sessions-view.tsx:430` |
| 全部 | **没有一个是空白面板**：空列表带真实文案（+ 动作） | `opencode en.ts:855-856`、`MiMo-Code en.ts:522-524`、`orca WorktreeList.tsx:6639-6665` |
| opencode | 调试信息由 `import.meta.env.DEV` 门控，不是常驻 | `layout.tsx:2252-2259` |

## 决策

1. **顶层导航 = 分组侧栏。** 12 个工作面变成 `SidebarItem`，落在四个组里（渲染序）：
   `工作`（任务）→ `项目`（用户项目）→ `工具`（文件、Git、IDE、数据库、笔记、Agent 工具）→
   `系统`（SSH、设施、扩展、远控、运维）。
   项目组紧跟工作组，因为**项目是任务的限定维度**（选中一个项目＝任务列表只看它），
   把它放在工具之后会让这层关系断掉。
   分组是**行模型的一部分**（`SidebarGroup`），不是各调用点各自决定的东西——
   这样"哪个面属于哪一组"只有一处答案。
2. **选择权在 shell，含义在 app。** `WorkspaceRootView` 持有 `selection: String?`，
   通过 `readyContent: (Binding<String?>) -> AnyView` 交给 app；app 用
   `WorkspaceSection.resolve(sidebarID:)` 把行 id 映射到工作面。
   shell **不知道**任何行 id 的含义（INV-02 归属不变），app 也不持有导航状态。
3. **折叠 = 56pt 图标栏，不是隐藏。** 隐藏会丢掉导航本身；参照实现一致选择图标栏。
   图标栏仍是 `List(selection:)`——系统画选中态，不手绘（ADR-001 §4 第 2 条）。
   行 id 与展开态**同一命名空间**，所以折叠/展开不改变选择。
4. **开发者脚手架只在测试环境渲染。** `bootstrap.*` 五行块与状态栏的环境标签由
   `DiagnosticEnvironment.isDiagnostic` 单点判定，只有 `ui-test` 为真
   （外加"连接未就绪"这一条用户真正需要 `connection` 的场景）。
   `dev` 曾被算作诊断环境，但 `native:dev` 正是**交互式运行应用的方式**，
   于是那五行成了正在被重做的界面里的常驻家具。UI 测试全部跑在 `ui-test`
   （`scripts/native/ui-test.mjs:63`），收窄不损失任何测试面。
5. **每个面板都有空态。** 共用 `PaneEmptyState`（图标 + 标题 + 下一步提示）。
   空列表与"渲染失败"必须可区分——这是用户报的缺陷类别之一。
6. **状态栏回到参考实现的密度与内容**：24pt、11pt 弱化文字，承载连接相位与
   **上下文**（进行中任务数、当前项目），不承载构建环境标签。

## 后果

- **正面**：日常动作回到导航的第一组，机器配置沉到第三组；窗口顶部不再有横贯的条；
  折叠后窗口边缘稳定（不跳动）；空面板一律可解释。
- **代价**：`WorkspaceStateProviding` 新增 `statusContext`；`readyContent` 由
  `() -> AnyView` 变为 `(Binding<String?>) -> AnyView`（调用点只有 app 一处）。
- **不变**：`interactionMode`（native/advanced）与 `glass/classic` 外观偏好仍是两枚
  独立开关；材质基座仍是 `NSVisualEffectView`/`Material`，`glassEffect` 仍只在
  macOS 26+ 且只用于可交互控件（ADR-001 §3）。

## 验证

- `shell` 门禁：`shell-no-hand-drawn-selection`（侧栏与命令面板的选中态仍由系统绘制）、
  `shell-no-web-navigation`、`shell-unit`。
- `native-design` 门禁：`native-selection-no-hand-drawn-state`、语义色。
- 新增 UI 用例 `ShellLifecycleTests.testEveryNavigationRowSwitchesTheDetailSurface`：
  逐个点击 11 个工作面行，断言 `detail.selection` 的值等于被点行的标题——
  覆盖"行点了没反应"（选中态与内容脱节）这一类缺陷。
- 新增单元用例 `ShellPolicyTests.testOnlyTheTestEnvironmentRendersDeveloperScaffolding`。
- `AccessibilityTests.testWorkspaceNavigationIsAddressableWithReadableRows`：导航行
  必须从 AX 树可寻址且读得出文字（不是只靠颜色区分）。
- 真窗口核对：分组侧栏、默认落在「任务」、`dev` 下无开发者信息、状态栏只剩相位与上下文。
