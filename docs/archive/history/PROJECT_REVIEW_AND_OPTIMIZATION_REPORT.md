# Aeroric 项目代码审查与优化报告

> 最近更新：2026-08-13
> 审查范围：React/Vite 桌面端、Expo 移动端、Tauri/Rust、远程连接、relay、共享协议、测试、构建与 CI
> 优化原则：保持业务、持久化与 wire compatibility；通过职责拆分、共享契约和统一组件降低维护成本

## 0. 2026-08-13 全仓重构交付

本次在 `codex/full-stack-maintainability` 分支完成一次全仓交付。它延续本文后续章节记录的渐进式拆分成果，并进一步覆盖此前未纳入的移动端、远程协议、relay、跨平台设计系统和实例级状态隔离。

### 0.1 工作区与共享基础设施

- 根应用、`mobile` 与 `packages/*` 已纳入单一 pnpm workspace；删除移动端重复 lockfile，CI 改为根目录一次安装。
- 新增无 React/React Native 依赖的 `@aeroric/design-system`，集中 light、dark、eyecare 语义颜色、间距、圆角、字号、阴影、状态和动效。
- 桌面 CSS variables 由设计 token 自动生成，`pnpm check:design-tokens` 检查生成物漂移；移动端直接消费同一 dark palette。
- 新增无 UI 依赖的 `@aeroric/remote-contracts`，集中 RPC v2/v3 类型、能力协商和跨语言 JSON 黄金样例。
- ESLint、Prettier、TypeScript 与设计 token 检查已覆盖桌面、移动端和共享包。
- 新增 Expo pnpm monorepo Metro 配置，iOS、Android、Web 三端 export 均已通过。

### 0.2 状态边界与组件整合

- 使用 Zustand 5 `createStore` + Context 建立实例级 scoped store；没有全局万能 store，也没有使用 persist middleware。
- Database UI 状态拆为连接/导航、workspace tab/query、grid、dialog/menu 四类同步 slice；Tauri 请求、取消、请求序列与生产操作确认仍在原 controller/service 层。
- Debug 的 session、breakpoint、variables/watch、console 状态已收敛到实例 store；Notebook 的 notes/active note 状态已收敛到实例 store。DOM selection、编辑器实例、计时器继续保留在局部 ref。
- 新增 scoped store 的纯状态、reset、函数式更新和多实例隔离测试。
- 桌面数据库域已迁移到统一 `Button`/`IconButton`/menu primitives，并删除重复的 `DbxButton` 实现；另提供 Field、Select、TextArea、Badge、Spinner、EmptyState 和 Toolbar 基础组件。
- 移动端新增 Button、IconButton、Field、Sheet、SegmentedControl、ListRow、StatusBadge、Spinner 与 EmptyState；New Task 与项目空状态已迁移到共享组件。
- 统一可见焦点环、reduced-motion 降级和紫色语义强调色；移动端共享组件交互热区下限为 44pt。

### 0.3 RPC 双栈与远程链路

- E2EE v2、terminal binary framing、relay protocol v1、PairingOffer 和 SecureStore 格式保持不变。
- 认证请求仍使用 RPC v2，新增可选 `supportedRpcVersions: [3, 2]` 与 capabilities；认证响应仍为 v2，并返回选中的 `rpcVersion`。字段缺失固定回退 v2。
- RPC v3 已实现明确的 request/response/push 判别字段、结构化错误和 push watermark；桌面按每个已认证客户端的版本分别编码 push，因此同一服务可同时承载 v2/v3 客户端。
- 移动端 `RemoteConnection` 公开 API 不变，RPC codec/协商已拆出；Aeroric 通道支持 v3，Orca 保持既有 wire shape。
- Rust 与 TypeScript 同时读取 `packages/remote-contracts/fixtures/rpc-golden.json`，覆盖序列化、结构化错误、push seq 与版本回退。
- 扩展检查时修复了 Orca 握手未实际比较已配对 desktop public key 的安全缺口；旧配对数据格式未改变。

### 0.4 Rust 与 relay 责任边界

- 既有 DAP/LSP protocol 子模块、App Settings、Local Router 与数据库后端模块继续作为稳定内部边界；所有 Tauri command 名称、参数、返回 JSON 和事件名保持不变。
- Git 的进程启动、超时、pipe 收集与错误转换已迁入 `git/transport.rs`；Session 的导出路径验证、Markdown 渲染和流式文件写入已迁入 `session/export.rs`，父模块继续提供原 command façade。
- `remote/protocol.rs` 负责版本协商和领域 envelope，`server.rs` 保持 command/server façade，event/session push 只调用版本感知的广播接口。
- relay 单文件入口已拆为 `config`、`registry`、`rate_limit`、`splice` 与 WebSocket façade；splice 仍只盲转发字节，不解析业务 payload。
- 本地同级 DBX checkout 未被修改。Rust 验证在隔离临时目录使用固定提交 `8559aec8bce4efeb4f52080da8ab1839733ef45b` 和仓库补丁完成。

### 0.5 验证与包体

最终验收结果：

- 桌面：lint、Prettier、typecheck、token 漂移检查、生产构建全部通过；160 个 Vitest 文件、1,194 个测试通过。
- 桌面 coverage：statements 65.59%、branches 62.63%、functions 64.51%、lines 68.58%。
- 移动端：typecheck、22 个 Vitest 文件/150 个测试、iOS/Android/Web Expo export 全部通过。
- Tauri/Rust：固定 DBX 提交隔离环境全量 707 个测试通过，fmt 与 Clippy 通过。
- remote-protocol：fmt、Clippy、2 个测试通过；remote-relay：fmt、Clippy、9 个测试通过。
- 浏览器视觉回归覆盖桌面首页 light/dark、设置 dark/eyecare 和移动端 390×844 首页/配对页；无横向溢出，移动端最小可交互高度实测 45px。

根据改动前保留的生产构建采样，主要 chunk 均未超过 5% 阈值：入口 gzip 约 125.1 KB，基本持平；DatabaseView gzip 由约 85.1 KB 降至 83.0 KB；ProjectPage raw 基本持平；主 CSS raw 由约 79.8 KB 增至 82.4 KB（约 +3.2%）。

以下第 1–9 节保留上一阶段的审查过程和拆分依据；其中“未执行”与旧测试数量以本节最新交付记录为准。

## 1. 结论摘要（上一阶段记录）

本轮完成了全仓静态扫描、基线验证、超长文件识别和一轮低风险重构。项目整体工程基础较好：TypeScript、ESLint、Prettier、Vitest、Rust 检查和 GitHub Actions 均已建立，前端主要功能也有较丰富的行为测试。

当前最主要的维护风险不是缺少测试，而是少数核心文件承担过多职责。最高优先级热点仍是 `src/components/database/DatabaseView.tsx`，但经过连接对话框和 DataGrid 迁移后已由 12,760 行降至 9,933 行；其次是 Rust DAP/LSP 模块和 `FileViewer.tsx`。

本轮已经在不改变对外组件导出和行为的前提下完成以下优化：

- 将 `ProjectPage` 的面板注册、懒加载、预加载和 Suspense fallback 迁移到独立基础设施模块。
- 将 `FileViewer` 的 Markdown 渲染、目录/代码大纲、本地历史对话框、SQLite 预览以及 references/rename/quick-fix 浮层迁移到独立模块。
- 将 `DatabaseView` 的 SQL token 展示、密码输入框、引导面板和连接 URL 解析迁移到独立模块。
- 为数据库连接 URL 解析增加 5 个纯函数测试，覆盖标准 URL、JDBC、SQL Server、Oracle 和无效输入。
- 将 6,176 行、85 个测试的数据库主测试按连接管理、工作区与数据网格、对象操作、树操作和 NoSQL 工作流拆成 5 个测试文件。
- 将数据库测试的连接 fixture、DataTransfer、菜单标签和默认 Tauri mock 收敛到共享支持模块。
- 导出明确的 `RightPanel` 类型，避免面板基础设施通过 hook 返回值反推公共类型。
- 将连接草稿纯逻辑和完整连接对话框迁出 `DatabaseView`，保留既有 API 契约和交互。
- 将 DataGrid 纯状态计算、展示/交互 hook 和共享网格组件迁出 `DatabaseView`。
- 将应用设置导航、平台过滤、面板注册与预加载集中到单一注册表，低频面板形成独立 chunk。
- 将 Rust `app_settings.rs` 的模型探测、Agent wrapper、配置包和版本/升级逻辑迁到四个内部子模块，保留原 command 门面。
- 在 CI 中加入 `format:check`，并清理 jsdom canvas 与 Node 25 Web Storage 测试警告。

三个主要父文件共减少 4,865 行。迁出的代码保留在职责明确的生产模块中，目标是减少单文件认知负担和冲突面，而不是人为减少项目总代码量。

## 2. 审查范围与基线

当前工作区扫描覆盖：

- 314 个 TypeScript/TSX 文件。
- 49 个 Rust 源文件。
- 110 个 Vitest 测试文件。
- `.github/workflows/`、`package.json`、`src-tauri/Cargo.toml`、README 和相关开发文档。

修改前基线全部通过：

- `pnpm lint`
- `pnpm format:check`
- `pnpm build`
- `pnpm test`：105 个测试文件、804 个测试通过
- `cargo check`

应用设置域重构前的最新基线为前端 1,301 个测试、Rust 690 个测试全部通过；`AppSettingsDialog` 主 chunk 为 216,982 字节。本轮修改了 Rust 内部组织，但未修改 Tauri command 名称/参数/返回值、事件名称、持久化格式或前后端 JSON 字段。

## 3. 架构评价

### 3.1 做得较好的部分

- 前端 API 边界较清晰，数据库调用集中在 `src/lib/databaseApi.ts`。
- 数据库共享契约已集中到 `src/types/database.ts`，避免组件各自定义跨层结构。
- 大量用户可见行为已有 Vitest 覆盖，重构可以依靠 characterization tests 验证。
- `ProjectPage` 已使用按面板懒加载，生产构建能形成独立功能 chunk。
- Rust 后端按 Git、LSP、DAP、Session、Database、SFTP 等能力分模块。
- CI 已执行 Rust audit/fmt/test、ESLint、Vitest 和前端生产构建。
- README 已说明本地构建所需的 Node、pnpm、Rust、Tauri 系统依赖和 DBX 同级仓库。

### 3.2 核心问题

少数“工作区总控组件”持续吸收新功能，导致状态、派生数据、副作用、命令调用和 JSX 混在同一个文件中。此类文件虽然测试充分，但修改时需要理解过大的上下文，容易产生合并冲突和局部回归。

本轮采取渐进式拆分：先迁移纯展示、纯解析和注册表等稳定边界，不在同一轮重写状态模型或跨层协议。

## 4. 风险与建议优先级

### P1：`DatabaseView.tsx` 仍是最高风险热点（已完成前两项迁移）

- 当前行数：9,933。
- 连接配置和 DataGrid 展示/交互已迁出；文件仍承担连接树、数据库/Schema/Object 浏览、SQL 编辑与执行、结构编辑、上下文菜单、导入导出、数据迁移、Redis、Mongo 和多类对话框编排。
- 大量状态和回调集中在同一组件，局部功能修改需要跨越较长的状态依赖链。

建议后续按用户工作流拆分，而不是按代码长度机械切割：

1. ~~`DatabaseConnectionDialog`：连接草稿、URL 导入、SSH/Proxy transport。~~ 已完成。
2. `DatabaseWorkspace`：工作区 mode 和主内容路由。
3. `DatabaseQueryEditor`：SQL 输入、执行、取消、Explain、历史。
4. ~~`DatabaseDataGrid`：分页、排序、筛选、编辑、选择和复制。~~ 展示与交互状态迁移已完成；API 编排保留在父组件。
5. `DatabaseObjectMenus`：数据库、表、列、索引、过程等菜单矩阵。
6. `DatabaseImportExportDialogs`：导入预览、导出格式和进度。

每次只迁移一个工作流，并先运行 `database-view-*.test.tsx` 及对应子组件测试。

### 已处理：数据库主测试文件过长

- 原 `src/test/database-view.test.tsx` 为 6,176 行、85 个测试。
- 现已按连续行为域拆成 5 个测试文件，最大文件为 1,861 行，85 个测试正文和断言数量保持不变。

当前文件：

- `database-view-connections.test.tsx`
- `database-view-workspace-grid.test.tsx`
- `database-view-object-actions.test.tsx`
- `database-view-tree-actions.test.tsx`
- `database-view-nosql.test.tsx`

共享 mock 和 fixture 已迁入 `src/test/databaseViewTestUtils.ts`。后续若各文件继续增长，可再按 query/data-grid 或 connection/user-management 二级行为域拆分。

### P1：Rust 协议模块体量较大（已完成第一阶段内部协议拆分）

- `src-tauri/src/dap.rs`：4,405 行，第一阶段后 4,142 行。
- `src-tauri/src/lsp.rs`：3,866 行，第一阶段后 3,730 行。
- `src-tauri/src/session.rs`：2,836 行。
- `src-tauri/src/git.rs`：2,685 行。

这些模块涉及协议、子进程、异步 I/O 和共享状态，不适合仅按行数拆分。建议先保持 command 签名不变，再按内部边界迁移：

- 协议请求/响应类型与序列化。
- Transport、reader/writer 和进程生命周期。
- Session registry 与并发状态。
- DAP breakpoint/evaluate/stack 处理。
- LSP diagnostics、workspace edit、symbol/hover/completion 解析。
- Git status/diff/history/worktree 子域。

现有模块内已有测试标记，后续拆分前应先把关键私有解析函数变为可独立测试的内部模块。

本轮已保持所有 Tauri command 在原入口模块中，仅迁出：

- DAP Content-Length framing、adapter response/stack/variable/evaluate 解析和 CDP remote object 解析。
- LSP file URI、hover、location、range、position、completion 与 Markdown 文本解析。

Transport、session actor、进程生命周期与 workspace edit 写入仍保留原位，避免一次性扩大异步和并发重构范围。

### 已处理：应用设置域责任与包体

- `AppSettingsDialog.tsx` 已不再同时维护导航数组和面板条件分支；导航 key、分组、图标、平台可见性、组件和预加载入口均由 `panelRegistry.tsx` 定义。
- 低频面板通过 `React.lazy` 拆包，当前面板立即加载，其他可用面板在首帧后预取；WSL 仅在 Windows 注册和预取。
- `app_settings.rs` 已将四个业务域迁出，父模块仍保留公共类型、持久化协调、公共函数门面和全部 `#[tauri::command]`。

仍保留的高风险热点为 `DatabaseView.tsx`、DAP/LSP transport 与 session 生命周期、`DebugPanel.tsx` 和 `NotebookPanel.tsx`。本轮未扩展到这些范围，也未处理 `mobile`。

### P2：其他超长前端文件

- `src/i18n.tsx`：3,573 行。建议按 locale 和业务域拆分资源，再由入口聚合。
- ~~`src/components/database/DatabaseSidebarTree.tsx`：2,899 行。建议拆树节点展示、节点操作菜单、过滤/排序派生逻辑。~~ 已迁出状态派生、树基础展示和纯逻辑，入口降至 2,192 行。
- ~~`src/components/database/RedisBrowser.tsx`：2,896 行。建议拆 key tree、value viewer、编辑器和命令会话。~~ 已迁出 key tree、JSON viewer、命令会话和纯状态派生，入口降至 2,163 行。
- `src/components/debug/DebugPanel.tsx`：2,052 行。建议按 sessions、breakpoints、variables、console 拆分。
- `src/components/notebook/NotebookPanel.tsx`：1,783 行。建议拆 cell renderer、toolbar 和持久化协调层。

### P2：外部路径依赖需要持续明确

`src-tauri/Cargo.toml` 使用：

```toml
dbx-core = { path = "../../dbx/crates/dbx-core", default-features = false }
```

这意味着本地开发和 CI 都依赖 Aeroric 同级目录中的 DBX checkout。README 和 CI 当前已经处理该前置条件，但应避免在不更新文档和 workflow 的情况下改变目录关系。若未来需要可复现发布或独立 checkout 构建，可评估固定 Git revision、workspace vendor 或发布 crate。

### 已处理：CI 前端格式检查

`.github/workflows/checks.yml` 已在 ESLint 后执行 `pnpm format:check`，避免只在开发者机器上发现 Prettier 差异。

### 已处理：测试环境警告

`src/test/setup.ts` 已统一提供 canvas 2D context stub；`vitest.config.ts` 对 worker 禁用 Node 25 实验性 Web Storage，测试继续使用 jsdom 隔离的 `localStorage`，两类警告均已消除。

## 5. 本轮已实施的优化

### 5.1 `ProjectPage` 面板基础设施

新增 `src/components/project-page/ProjectPanelInfrastructure.tsx`，集中管理：

- IDE dock shell。
- 面板 lazy import 注册。
- 单面板和常用面板预加载。
- 面板反馈文案映射。
- center/dock Suspense fallback。

`ProjectPage` 继续负责工作区状态和布局编排，不再同时维护面板模块注册表。

### 5.2 `FileViewer` 可视子系统

新增：

- `src/components/file-viewer/FileOutline.tsx`
  - 合并 Markdown 目录和代码大纲的折叠容器。
  - 承载 breadcrumbs 和 sticky symbols。
- `src/components/file-viewer/markdownPreview.ts`
  - Markdown 渲染、消毒和 TOC 提取。
- `src/components/file-viewer/LocalHistoryDialog.tsx`
  - 本地历史列表、快照比较、恢复操作 UI。
- `src/components/file-viewer/SqlitePreviewPane.tsx`
  - SQLite object 列表、列信息和结果表格展示。
- `src/components/file-viewer/LspActionDialogs.tsx`
  - references 结果与异步预览状态。
  - rename 输入、workspace edit 预览和应用确认。
  - quick-fix 列表以及 rename/code-action 状态栏摘要。

文件读取、SQLite 请求、远程 endpoint、编辑器状态、LSP 请求、workspace edit 应用和保存时序仍保留在 `FileViewer`，因此没有改变数据流。

### 5.3 `DatabaseView` 稳定边界

新增：

- `src/components/database/DatabaseViewPrimitives.tsx`
  - SQL token 展示。
  - 密码输入框。
  - 工作区引导面板。
- `src/components/database/databaseConnectionUrl.ts`
  - 标准连接 URL、JDBC、SQL Server 和 Oracle URL 解析。

连接 URL 解析现在是无 UI 依赖的纯函数，可以独立测试和复用。

### 5.4 类型边界

`src/hooks/useProjectPanels.ts` 现在显式导出 `RightPanel` 类型。面板基础设施不再通过 `ReturnType<typeof useProjectPanels>` 推断公共面板类型，降低了 hook 实现与 UI 注册模块之间的类型耦合。

### 5.5 数据库主测试拆分

原 `src/test/database-view.test.tsx` 已拆成：

- `database-view-connections.test.tsx`：29 个测试。
- `database-view-workspace-grid.test.tsx`：18 个测试。
- `database-view-object-actions.test.tsx`：11 个测试。
- `database-view-tree-actions.test.tsx`：23 个测试。
- `database-view-nosql.test.tsx`：4 个测试。

`src/test/databaseViewTestUtils.ts` 统一提供连接 fixture、DataTransfer mock、菜单标签读取和 `beforeEach` 初始化。拆分后测试总数仍为 85，没有通过删除或合并断言降低覆盖。

### 5.6 连接对话框迁移

新增：

- `src/components/database/databaseConnectionDraft.ts`
  - 承载连接 profile、transport draft 和连接配置构建纯逻辑。
- `src/components/database/ConnectionDialog.tsx`
  - 自持连接草稿、向导、URL 导入、测试和保存状态。
- `src/test/database-connection-draft.test.ts`
  - 覆盖 PostgreSQL/MySQL、MongoDB、Redis、Oracle、transport 和编辑保留字段。

父组件仅保留对话框开关、编辑目标和保存成功后的连接列表/工作区协调。

### 5.7 DataGrid 三阶段迁移

新增：

- `src/components/database/databaseGridState.ts`
  - 承载排序、筛选、列可见性、行选择和 pending edit 等纯计算。
- `src/components/database/useDbxDataGrid.ts`
  - 收敛网格展示与交互状态、DOM resize 副作用和派生数据。
- `src/components/database/DataGridView.tsx`
  - 共享渲染 table/query 两种网格模式。
- `src/test/database-grid-state.test.ts`
  - 8 个测试覆盖全部导出的纯函数。

数据库请求、保存/回滚确认和上下文菜单命令编排继续由 `DatabaseView` 持有，没有修改 Tauri command 契约。

### 5.8 CI 与测试日志

- `.github/workflows/checks.yml` 新增 `pnpm format:check`。
- `src/test/setup.ts` 增加 canvas 2D context stub。
- `vitest.config.ts` 禁用 Node worker 的实验性 Web Storage，消除无路径 `--localstorage-file` 警告。

### 5.9 `FileViewer` LSP action controller

新增 `src/components/file-viewer/useFileViewerLspActions.ts`，集中管理：

- references 请求、异步预览和打开目标。
- rename 预览、保存前同步、workspace edit 应用和当前文件刷新。
- quick-fix 请求、workspace edit/command 执行和结果状态。
- 编辑器命令事件监听以及切换文件/内容变化时的 action 状态清理。

`FileViewer` 继续持有编辑器内容、保存计时和 CodeMirror 实例，只通过明确回调向 controller 提供保存与当前文件刷新能力。

### 5.10 数据库侧边树与 Redis 浏览器边界

`DatabaseSidebarTree` 新增：

- `databaseSidebarTreeState.ts`：节点 key、对象分组/去重/排序、badge、搜索和 Mongo preview 等纯逻辑。
- `useDatabaseSidebarTreeDerived.ts`：集中管理连接、数据库、schema、对象过滤以及可见节点序列派生。
- `DatabaseTreePrimitives.tsx`：连接 badge 与展开 glyph。

`RedisBrowser` 新增：

- `redisBrowserState.ts`：JSON、member row、stream 分组、列宽和插入语句派生。
- `RedisKeyTreePane.tsx`：数据库切换、key 搜索、树导航、多选与分页加载展示。
- `RedisJsonTree.tsx`：可折叠 JSON viewer。
- `RedisCommandSessionView.tsx`：受控命令历史与输入会话。

API 调用、确认流程、连接切换重置、编辑保存和 command safety 编排仍保留在 `RedisBrowser`，组件间仅通过显式 props/callbacks 协作。

### 5.11 DAP/LSP 内部协议解析模块

新增：

- `src-tauri/src/dap/protocol.rs`：DAP framing、adapter response/variable/stack/evaluate 和 CDP value parser。
- `src-tauri/src/lsp/protocol.rs`：file URI 编解码以及 hover/location/range/completion parser。

`dap.rs` 与 `lsp.rs` 通过私有 `mod protocol` 使用这些实现；Tauri command 名称、参数、返回类型、状态持有和前端调用契约均未改变。已有模块测试继续从父模块覆盖迁出的内部函数。

### 5.12 应用设置前后端边界

前端新增 `src/components/app-settings/panelRegistry.tsx`，集中管理面板注册、平台过滤和预加载。`SettingsPanelHost` 提供统一 Suspense 加载态和面板级错误边界；预加载失败不会关闭设置弹窗。General、Theme、Fonts 和 Shortcuts 保持直接加载，其余低频面板均形成独立 chunk。

Rust 新增：

- `app_settings/models.rs`：模型 URL 安全策略、认证重试、目录解析和余额请求。
- `app_settings/agent_scripts.rs`：Shell/PowerShell wrapper、凭据恢复和旧 wrapper 刷新。
- `app_settings/config_bundles.rs`：单 Agent/全量配置包、便携转换和 CC Switch SQL 解析。
- `app_settings/versions.rs`：版本解析/缓存、安装方式识别、升级命令选择与执行。

共49 个领域测试随实现迁入子模块；持久化、代理、迁移和跨域启动行为测试保留在父模块。

生产构建实测：

| 产物 | 重构前 | 重构后 | 变化 |
| --- | ---: | ---: | ---: |
| `AppSettingsDialog-*.js` | 216,982 B | 39,059 B | -82.00% |

独立面板 chunk 包括 Proxy、Local Router、Remote Access、MCP、WSL、Usage、Agent Updates、Hooks、Skills、All Agent Configs 和 About。

## 6. 文件行数变化

| 文件 | 修改前 | 修改后 | 变化 |
| --- | ---: | ---: | ---: |
| `src/components/FileViewer.tsx` | 4,918 | 3,193 | -1,725 |
| `src/components/ProjectPage.tsx` | 2,588 | 2,275 | -313 |
| `src/components/database/DatabaseView.tsx` | 12,760 | 9,933 | -2,827 |
| `src/components/database/DatabaseSidebarTree.tsx` | 2,899 | 2,192 | -707 |
| `src/components/database/RedisBrowser.tsx` | 2,896 | 2,163 | -733 |
| `src-tauri/src/dap.rs` | 4,405 | 4,142 | -263 |
| `src-tauri/src/lsp.rs` | 3,866 | 3,730 | -136 |
| `src-tauri/src/app_settings.rs` | 7,978 | 3,765 | -4,213 |
| `src/components/AppSettingsDialog.tsx` | 409 | 298 | -111 |
| **合计** | **42,719** | **31,691** | **-11,028** |

新增生产模块行数：

| 模块 | 行数 |
| --- | ---: |
| `ProjectPanelInfrastructure.tsx` | 334 |
| `FileOutline.tsx` | 263 |
| `markdownPreview.ts` | 32 |
| `LocalHistoryDialog.tsx` | 274 |
| `SqlitePreviewPane.tsx` | 429 |
| `LspActionDialogs.tsx` | 567 |
| `DatabaseViewPrimitives.tsx` | 119 |
| `databaseConnectionUrl.ts` | 122 |
| `databaseConnectionDraft.ts` | 442 |
| `ConnectionDialog.tsx` | 1,625 |
| `databaseGridState.ts` | 237 |
| `useDbxDataGrid.ts` | 426 |
| `DataGridView.tsx` | 478 |
| `useFileViewerLspActions.ts` | 406 |
| `databaseSidebarTreeState.ts` | 307 |
| `useDatabaseSidebarTreeDerived.ts` | 604 |
| `DatabaseTreePrimitives.tsx` | 36 |
| `redisBrowserState.ts` | 403 |
| `RedisKeyTreePane.tsx` | 289 |
| `RedisJsonTree.tsx` | 102 |
| `RedisCommandSessionView.tsx` | 127 |
| `src-tauri/src/dap/protocol.rs` | 288 |
| `src-tauri/src/lsp/protocol.rs` | 147 |
| `src/components/app-settings/panelRegistry.tsx` | 299 |
| `src-tauri/src/app_settings/models.rs` | 704 |
| `src-tauri/src/app_settings/agent_scripts.rs` | 2,213 |
| `src-tauri/src/app_settings/config_bundles.rs` | 791 |
| `src-tauri/src/app_settings/versions.rs` | 597 |

数据库主测试拆分后行数：

| 文件 | 行数 | 测试数 |
| --- | ---: | ---: |
| `database-view-connections.test.tsx` | 1,626 | 29 |
| `database-view-workspace-grid.test.tsx` | 1,861 | 18 |
| `database-view-object-actions.test.tsx` | 900 | 11 |
| `database-view-tree-actions.test.tsx` | 1,386 | 23 |
| `database-view-nosql.test.tsx` | 415 | 4 |
| `databaseViewTestUtils.ts` | 108 | - |

## 7. 验证结果

修改后实际执行并通过：

- `pnpm lint`
- `pnpm format:check`
- `git diff --check`
- `pnpm build`
- 第一阶段定向 Vitest：6 个文件、122 个测试通过
- 数据库拆分定向 Vitest：5 个文件、85 个测试通过
- FileViewer LSP 定向 Vitest：5 个文件、15 个测试通过
- FileViewer LSP controller 扩展定向 Vitest：9 个文件、22 个测试通过
- DataGrid 定向 Vitest：2 个文件、26 个测试通过
- 连接对话框定向 Vitest：2 个文件、40 个测试通过
- 数据库侧边树与 Redis 浏览器定向 Vitest：4 个文件、61 个测试通过
- `pnpm test`：114 个测试文件、836 个测试通过
- `cargo fmt --check`：通过
- DAP 模块定向测试：28 个测试通过
- LSP 模块定向测试：26 个测试通过
- `cargo check`：通过
- `cargo test`：339 个测试通过

应用设置域本轮追加验收：

- 设置面板定向 Vitest：3 个文件、17 个测试通过。
- `AppSettingsDialog` 主 chunk 为 39,059 字节，相对 216,982 字节下降 82.00%。
- Rust `app_settings` 定向测试：73 个测试通过，其中 49 个已迁入四个子模块。
- 全量验收结果：`pnpm test` 1,306 个测试全部通过，`cargo test --lib` 690 个测试全部通过。

新增测试文件：

- `src/test/database-connection-url.test.ts`
- `src/test/database-connection-draft.test.ts`
- `src/test/database-grid-state.test.ts`
- `src/test/database-view-connections.test.tsx`
- `src/test/database-view-workspace-grid.test.tsx`
- `src/test/database-view-object-actions.test.tsx`
- `src/test/database-view-tree-actions.test.tsx`
- `src/test/database-view-nosql.test.tsx`
- `src/test/database-sidebar-tree-state.test.ts`
- `src/test/redis-browser-state.test.ts`
- `src/test/app-settings-panel-registry.test.tsx`

覆盖：

- PostgreSQL 标准 URL 与编码字段。
- MySQL JDBC URL 归一化。
- SQL Server 分号参数。
- Oracle service name 和 SID JDBC 格式。
- 空值和非法 URL。

自动化验证未发现行为回归。由于本轮是结构重构，没有进行桌面端人工逐项 UI 点击验证；现有组件行为测试、完整测试、类型检查和生产构建作为主要回归依据。

## 8. 推荐后续执行顺序

1. ~~从 `DatabaseView` 迁出连接对话框及其草稿状态，保持 `databaseApi` 调用不变。~~ 已完成。
2. ~~迁出 DataGrid 展示和交互状态，并保留现有保存/回滚请求结构。~~ 已完成。
3. ~~将 `FileViewer` 的 LSP 请求、保存前同步和 workspace edit 刷新编排收敛到 controller hook。~~ 已完成。
4. ~~拆分 `DatabaseSidebarTree` 与 `RedisBrowser` 的展示和状态派生逻辑。~~ 已完成。
5. 若数据库测试继续增长，按 query/data-grid 和 connection/user-management 做二级拆分；本轮复核后未继续增长，暂不做无收益拆分。
6. ~~按协议内部边界渐进拆分 Rust DAP/LSP，不改变 Tauri command。~~ 已完成第一阶段协议 parser 拆分。
7. ~~在 CI 中加入 `pnpm format:check`，并清理 Vitest 环境警告。~~ 已完成。

## 9. 上一阶段未执行的事项（现状以第 0 节为准）

- 未升级 npm、Cargo 或 Tauri 依赖。
- 未修改视觉设计、交互文案或国际化资源。
- 未修改 Tauri command 契约和数据库持久化。
- 未执行 `cargo audit` 或桌面安装包构建；这些仍由 CI/release workflow 覆盖。
- 未扫描或修改 `mobile`，未执行桌面端人工逐项点击验收。
- 工作区位于 `main`，未创建其他分支；本轮未提交或推送。
