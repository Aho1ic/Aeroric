# Aeroric 项目代码审查与优化报告

> 审查日期：2026-07-10  
> 审查范围：React/TypeScript 前端、Tauri/Rust 后端、测试、构建脚本、CI 与开发依赖  
> 优化原则：不改变用户可见行为、不修改 Tauri command 契约、不升级依赖、优先拆分低耦合职责

## 1. 结论摘要

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

本轮未修改 Rust 代码、Tauri command 名称、事件名称、持久化格式或前后端 JSON 字段。

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

### P1：Rust 协议模块体量较大

- `src-tauri/src/dap.rs`：4,405 行。
- `src-tauri/src/lsp.rs`：3,866 行。
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

## 6. 文件行数变化

| 文件 | 修改前 | 修改后 | 变化 |
| --- | ---: | ---: | ---: |
| `src/components/FileViewer.tsx` | 4,918 | 3,193 | -1,725 |
| `src/components/ProjectPage.tsx` | 2,588 | 2,275 | -313 |
| `src/components/database/DatabaseView.tsx` | 12,760 | 9,933 | -2,827 |
| `src/components/database/DatabaseSidebarTree.tsx` | 2,899 | 2,192 | -707 |
| `src/components/database/RedisBrowser.tsx` | 2,896 | 2,163 | -733 |
| **合计** | **26,061** | **19,756** | **-6,305** |

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
- `cargo check`：通过

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
6. 按协议内部边界渐进拆分 Rust DAP/LSP，不改变 Tauri command。
7. ~~在 CI 中加入 `pnpm format:check`，并清理 Vitest 环境警告。~~ 已完成。

## 9. 本轮未执行的事项

- 未升级 npm、Cargo 或 Tauri 依赖。
- 未修改视觉设计、交互文案或国际化资源。
- 未修改 Rust 实现和数据库持久化。
- 未执行 `cargo test`、`cargo audit` 或桌面安装包构建；这些仍由 CI/release workflow 覆盖。
- 未创建分支、提交或推送代码。
