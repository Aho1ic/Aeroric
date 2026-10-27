# Agent Note: 顶层导航是分组侧栏

Status: implemented

## Problem

macOS App 有十余个工作面。平铺 tab 条放不下，也表达不出"项目是任务的限定维度"这层关系；开发脚手架（bootstrap 行、环境标签）还常驻在日常交互运行的界面里。

## Decision

- 顶层导航是分组侧栏：工作面是 `SidebarItem`，按渲染序落在四组——`工作`（任务）→ `项目`（用户项目）→ `工具`（文件、Git、IDE、数据库、笔记、Agent 工具）→ `系统`（SSH、设施、扩展、远控、运维）。项目组紧跟工作组，选中一个项目即任务列表只看它。分组是行模型的一部分（`SidebarGroup`），"哪个面属于哪组"只有一处答案。
- 选择权在 shell，含义在 app：`WorkspaceRootView` 持有 `selection`，经 `readyContent` 交给 app，app 用 `WorkspaceSection.resolve(sidebarID:)` 解析；shell 不知道行 id 的含义。
- 折叠是 56pt 图标栏而不是隐藏；图标栏仍是 `List(selection:)`，由系统画选中态。行 id 与展开态同一命名空间，折叠不改变选择。
- 开发脚手架只在 `DiagnosticEnvironment.isDiagnostic` 为真时渲染（`ui-test` 环境，以及连接未就绪时的连接信息）。
- 每个面板有空态（共用 `PaneEmptyState`），空列表与渲染失败可区分。
- 状态栏 24pt、11pt 弱化文字，承载连接相位与上下文（进行中任务数、当前项目），不承载构建环境标签。

## Alternatives considered

- **平铺 tab 条**：实现简单。否掉是因为面板数量超出一行，且表达不出项目与任务的从属关系。
- **折叠时完全隐藏侧栏**：内容区更宽。否掉是因为会丢掉导航本身；参照实现一致选择图标栏。

## Consequences

- **收益**：导航层级与产品概念对齐；选中态由系统绘制，无障碍行为一致。
- **代价与已知上限**：新增工作面必须决定所属分组；分组超过四个或某组过长时需要重访。
