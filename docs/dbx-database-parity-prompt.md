# Aeroric 数据库功能对齐 dbx 的完整提示词与操作步骤

生成时间：2026-06-18

## 目标

把本项目 Aeroric 的数据库模块改到与 dbx 项目数据库模块功能、按钮行为、查看方式、交互细节和界面风格一致。

重要约束：

- 目标是“行为和视觉一模一样”，不是照搬架构。
- dbx 是 Vue + Pinia + Tauri；Aeroric 是 React + hooks + Tauri。实现时必须适配 Aeroric 现有 React/Tauri 架构。
- 不删除或回滚用户已有改动。
- 高风险操作必须先确认：删除文件、大规模重构、改 git 历史、推送远程、改 CI、数据库变更。

## 只读源码参考

dbx 项目源码：

- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/sidebar/ConnectionTree.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/sidebar/TreeItem.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/connection/ConnectionDialog.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/grid/DataGrid.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/editor/QueryEditor.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/structure/TableStructureEditor.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/sidebar/VisibleDatabasesDialog.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/export/DatabaseExportDialog.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/import/TableImportDialog.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/search/DatabaseSearchDialog.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/diagram/SchemaDiagramDialog.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/stores/connectionStore.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/stores/queryStore.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/lib/createDatabaseSql.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/lib/dbAdminSql.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/lib/databaseCapabilities.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/lib/databaseFeatureSupport.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/lib/databaseTree.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/lib/tableTree.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/lib/sidebarTreeSelection.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/lib/sidebarSearchTree.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/lib/sidebarLayout.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/lib/tauri.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/src-tauri/src/commands/query.rs`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/src-tauri/src/lib.rs`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/crates/dbx-core/src/db_admin_sql.rs`

Aeroric 当前数据库相关源码：

- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/DatabaseView.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/databaseActions.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/DatabaseAdvancedTools.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/TableStructurePanel.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/RedisBrowser.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/MongoBrowser.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/ErDiagramPanel.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/hooks/useDatabaseConnections.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/hooks/useDatabaseSchema.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/hooks/useDatabaseQuery.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/lib/databaseApi.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/types/database.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/styles/database.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/i18n.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src-tauri/src/database/mod.rs`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src-tauri/src/database/query.rs`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src-tauri/src/database/schema.rs`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src-tauri/src/database/connections.rs`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src-tauri/src/database/grid.rs`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src-tauri/src/database/import_export.rs`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src-tauri/src/lib.rs`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/test/database-api.test.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/test/database-view.test.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/test/database-workspace-actions.test.ts`

## 给下一位实现者的完整提示词

你现在在 Aeroric 项目中工作：`/Users/macbook/Downloads/同步空间/LYX/Aeroric`。

请把 Aeroric 的数据库模块改到与 dbx 项目数据库模块一模一样，包括但不限于：新建数据库、连接弹窗、按钮功能、右键菜单、查看方式、树形浏览、表格数据查看、查询编辑、结构编辑、导入导出、可见数据库选择、搜索过滤、快捷键、界面样式、状态提示、危险操作确认。

参考项目 dbx 路径：`/Users/macbook/Downloads/同步空间/LYX/dbx`。

必须遵守：

1. 只迁移功能和交互，不迁移架构。dbx 是 Vue/Pinia，Aeroric 是 React/hooks，必须用 Aeroric 的 React 组件、hooks、Tauri invoke、`src/styles/database.ts` 或局部样式实现。
2. 优先复用 Aeroric 已有数据库接入：`src/lib/databaseApi.ts`、`src/types/database.ts`、`src-tauri/src/database/*`、`src/components/database/*`。
3. dbx 中已由 `dbx_core` 实现的 SQL 构造能力，优先在 Aeroric Tauri command 层增加薄包装，不在前端手写 SQL 拼接。
4. 不改变其他模块行为。现有脏改动都视为用户改动，不要回滚。
5. 每完成一组功能都补测试或更新现有数据库测试。

### 需要实现的一致性目标

#### 1. 连接入口和顶部工具栏

Aeroric 当前顶部工具栏已有：

- 新建连接
- 新建查询
- 执行 SQL 文件
- 驱动管理
- 数据传输
- 结构对比
- 数据对比
- ER 图
- 表结构

需要对齐 dbx：

- 按钮布局、图标、禁用状态、hover/active 状态与 dbx 一致。
- 工具入口不是简单提示，而是真正进入对应工作区或弹窗。
- 连接上下文必须能正确决定按钮是否可用：SQL 数据库、Redis、MongoDB、只读连接、未连接状态。

#### 2. 新建连接弹窗

对齐 dbx `ConnectionDialog.vue`：

- 第一步是数据库类型选择。
- 类型选择支持图标视图和列表视图切换。
- 支持搜索数据库类型。
- 类型卡片视觉一致：图标、选中边框、背景、hover 动效、双击进入配置。
- 第二步是配置页。
- 配置页使用 tabs：基础、TLS、SSH/代理、高级。
- 基础页根据数据库类型显示不同字段：
  - SQLite/DuckDB/Access/H2 文件路径和文件选择按钮。
  - MySQL/PostgreSQL/SQL Server/Oracle/ClickHouse 等显示 host、port、user、password、database、URL 参数或连接串。
  - Redis 显示 standalone/sentinel/cluster 相关字段。
  - MongoDB 支持 URL 模式与表单模式。
- 支持连接 URL 解析入口。
- 支持连接颜色选择。
- 支持只读开关、连接超时、查询超时、keepalive。
- 支持 TLS 证书路径选择。
- 支持多层 SSH/代理 transport layers，而不是 Aeroric 当前一个简单开关。
- 底部按钮对齐 dbx：返回、测试连接、保存/新建、取消；测试结果显示在 footer 左侧。

#### 3. 侧边栏树形浏览

Aeroric 当前是扁平连接列表、数据库列表、对象列表。必须改为 dbx 的树形侧边栏模型：

- 根节点：连接分组和连接。
- 连接节点可展开/收起。
- 数据库节点、schema 节点、对象分组节点、表、视图、列、索引、外键、触发器、函数、过程、序列等按 dbx 逻辑显示。
- Redis 显示 `db0`、`db1` 等节点和 key 统计。
- MongoDB 显示 database 和 collection 节点。
- 支持懒加载节点 children。
- 支持刷新单个节点，不是只能刷新整个页面。
- 支持选中状态、多选、Shift 范围选择、Ctrl/Cmd 多选。
- 支持 F2 重命名、F5 刷新、Delete/Backspace 删除或 drop、Mod+C 复制名称。
- 支持拖拽表/视图引用到查询编辑器。
- 支持连接分组：新建分组、重命名分组、删除分组、移动连接到分组。
- 支持固定/取消固定节点。
- 支持侧边栏搜索：
  - 输入防抖。
  - 搜索连接、数据库、schema、表、视图。
  - 搜索范围过滤按钮。
  - 清空搜索后恢复树。
- 大量节点时使用虚拟列表或等效性能方案，避免渲染卡顿。

#### 4. 右键菜单行为

必须按 dbx `TreeItem.vue` 的节点类型实现完整菜单。不要只显示占位项。

连接节点菜单：

- 打开连接/关闭连接。
- 新建查询。
- SQL 历史。
- 用户管理。
- 复制最终代理端口。
- 执行 SQL 文件。
- 新建数据库；DuckDB 时是创建/附加 DuckDB 文件。
- 移至分组/新建分组。
- 刷新。
- 选择显示数据库。
- 编辑连接。
- 在文件管理器中显示数据库文件。
- 备份 SQLite 数据库。
- 复制连接。
- 删除连接。

数据库/schema 节点菜单：

- 复制名称。
- 打开对象浏览器。
- 新建查询。
- SQL 历史。
- 设为默认数据库/清除默认数据库。
- 新建表。
- 新建 schema。
- 执行 SQL 文件。
- 打开 ER 图。
- 数据库搜索。
- 刷新。
- 数据传输。
- 结构对比。
- 数据对比。
- 导出数据库。
- 关闭数据库连接。
- 删除数据库/drop schema。

表/视图节点菜单：

- 复制名称。
- 查看数据。
- 视图：编辑视图、查看源码、查看 DDL。
- 编辑结构。
- 表信息。
- 重命名对象。
- 新建 SQL 子菜单：查询、INSERT、UPDATE。
- SQL 历史。
- 打开 ER 图。
- 导入数据。
- 数据对比。
- 导出数据子菜单：CSV、JSON、SQL INSERT、XLSX。
- 导出数据库。
- 导出结构。
- 复制结构为 TSV/Markdown。
- 复制表结构。
- 清空表、truncate 表、drop 表。
- 刷新。

列/索引/外键/触发器菜单：

- 复制名称。
- 字段血缘。
- drop column/index/foreign key/trigger。
- drop 前必须生成 SQL 预览并确认。

过程/函数/包/序列菜单：

- 执行过程。
- 查看源码。
- 重命名对象。
- drop procedure/function。

Redis/Mongo/Elasticsearch 节点菜单也必须与 dbx 节点行为一致。

#### 5. 新建数据库必须与 dbx 一致

参考 dbx：

- 前端：`apps/desktop/src/components/sidebar/TreeItem.vue`
- SQL 构造：`apps/desktop/src/lib/createDatabaseSql.ts`
- 后端命令：`src-tauri/src/commands/query.rs`
- 核心 SQL：`crates/dbx-core/src/db_admin_sql.rs`

Aeroric 需要补齐：

- `src-tauri/src/database/query.rs`
  - 增加 `dbx_build_create_database_sql`
  - 增加 `dbx_build_duckdb_attach_database_sql`
  - 增加 `dbx_build_drop_database_sql`
  - 增加 `dbx_build_create_schema_sql`
  - 增加 `dbx_build_drop_schema_sql`
  - 增加 drop table/object/empty/truncate/duplicate structure 等 dbx 管理类 SQL 包装，命名保持 Aeroric 现有 `dbx_*` 风格。
- `src-tauri/src/lib.rs`
  - 注册新增命令。
- `src/lib/databaseApi.ts`
  - 增加对应 invoke 封装。
- `src/types/database.ts`
  - 增加 `CreateDatabaseSqlOptions`、`DatabaseNameSqlOptions`、`SchemaNameSqlOptions`、`TableAdminSqlOptions` 等类型。
- UI 行为：
  - 仅连接节点显示“新建数据库”。
  - 支持 `supportsDatabaseCreation` 的数据库类型才显示。
  - MySQL/MariaDB/TiDB/OceanBase/Doris/StarRocks/GoldenDB 兼容类型显示 charset/collation，默认 `utf8mb4` / `utf8mb4_unicode_ci`。
  - 其他类型只输入数据库名。
  - DuckDB 使用文件保存对话框创建 `.duckdb` 或 `.db` 文件，再执行 `ATTACH path AS name`，并更新连接配置中的 attached databases。
  - 创建成功后刷新数据库节点并选中新库。
  - 失败时 toast/错误提示与 dbx 一致。

#### 6. 主工作区查看方式

对齐 dbx 的多 tab 工作区思想。Aeroric 可以不引入 Pinia，但需要有等价 React 状态：

- 查询 tab。
- 数据 tab。
- 表结构 tab。
- 对象浏览器 tab。
- SQL 文件执行 tab。
- Redis 浏览器。
- Mongo 浏览器。
- 数据传输、结构对比、数据对比、ER 图、数据库搜索、数据库导出、表导入。
- 支持从树节点打开对应 tab；重复打开时按 dbx 设置或默认策略复用/新开。
- 标题、路径、副标题、错误状态、加载状态都与 dbx 视觉一致。

#### 7. 表格数据查看和编辑

Aeroric 当前 HTML table 功能较轻。需要对齐 dbx `DataGrid.vue` 的行为：

- 表格有固定表头、行号/rowid、列宽、横向滚动。
- 分页、排序、过滤、搜索、列显示隐藏。
- 单元格编辑、插入行、删除行、保存预览 SQL、回滚 SQL。
- 只读连接禁用编辑。
- 大字段预览、JSON/图片/空间数据等特殊预览能力尽量按 dbx 保留。
- 导出 CSV/JSON/Markdown/SQL INSERT/SQL UPDATE/XLSX。
- 表导入 CSV/XLSX，支持字段映射和 append/truncate 模式。

#### 8. 样式要求

视觉必须贴近 dbx，而不是保留 Aeroric 现有数据库页样式。

需要重点对齐：

- 侧边树行高、缩进、展开图标、节点图标颜色。
- 右键菜单宽度、分隔线、危险项颜色、子菜单。
- 连接弹窗宽度：
  - 类型选择约 `760px`。
  - 配置页约 `560px`。
- 弹窗圆角、边框、阴影、背景、tabs、footer。
- 类型选择卡片：
  - icon 视图为 2/4/5 列响应式布局。
  - list 视图为紧凑行。
- 搜索框、过滤按钮、空状态。
- 工具栏按钮为 icon + text，禁用态和 hover 态一致。
- 不要出现当前 Aeroric 数据库页的“占位提示型”高级功能入口。

#### 9. i18n

补齐中文和英文 key。优先参考 dbx：

- `contextMenu.*`
- `connection.*`
- `visibleDatabases.*`
- `grid.*`
- `databaseSearch.*`
- `diagram.*`
- `diff.*`
- `dataCompare.*`
- `transfer.*`

Aeroric 的 `src/i18n.tsx` 目前已有部分 key，但不完整。所有新增 UI 文案必须走 i18n。

#### 10. 测试要求

更新或新增测试：

- `src/test/database-api.test.ts`
  - 覆盖所有新增 `databaseApi` invoke 包装参数。
- `src/test/database-view.test.tsx`
  - 新建连接弹窗两步式流程。
  - 图标/列表视图切换。
  - 数据库类型搜索。
  - 连接右键菜单只显示当前能力支持的项。
  - 新建数据库弹窗、charset/collation、执行 SQL、刷新数据库列表。
  - DuckDB 创建文件/attach 行为可 mock。
  - 树节点展开、刷新、搜索、多选、快捷键。
- 新增树组件测试：
  - 连接、数据库、schema、表、视图、列节点渲染。
  - 节点右键菜单按类型出现正确操作。
  - F2/F5/Delete/Mod+C 行为。
- 新增后端 Rust 单元测试：
  - `dbx_build_create_database_sql` 调用 `dbx_core`。
  - MySQL charset/collation。
  - 非 MySQL 不带 charset。
  - drop/create schema SQL。

运行验证：

```bash
pnpm test -- src/test/database-api.test.ts src/test/database-view.test.tsx src/test/database-workspace-actions.test.ts
pnpm test -- src/test/database-advanced-tools.test.tsx src/test/database-table-structure-panel.test.tsx src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
pnpm build
```

## 推荐实施步骤

1. 建立能力映射
   - 从 dbx `databaseCapabilities.ts`、`databaseFeatureSupport.ts`、`databaseCapabilitySets.ts` 抽取 capability 判断。
   - 在 Aeroric 中新增对应能力 helper，避免 UI 到处写数据库类型 if。

2. 补后端管理 SQL 命令
   - 在 `src-tauri/src/database/query.rs` 加薄包装。
   - 在 `src-tauri/src/lib.rs` 注册。
   - 在 `src/lib/databaseApi.ts` 和 `src/types/database.ts` 暴露。
   - 先写/改 `database-api.test.ts`。

3. 抽侧边栏树组件
   - 不要继续在 `DatabaseView.tsx` 中堆全部逻辑。
   - 建议新增：
     - `src/components/database/DatabaseSidebarTree.tsx`
     - `src/components/database/DatabaseTreeItem.tsx`
     - `src/components/database/DatabaseContextMenu.tsx`
     - `src/components/database/ConnectionDialog.tsx`
     - `src/hooks/useDatabaseTree.ts`
     - `src/hooks/useDatabaseTabs.ts`
   - `DatabaseView.tsx` 只做总布局和状态组装。

4. 先实现连接弹窗 parity
   - 类型选择 icon/list/search。
   - 配置 tabs。
   - 保存、测试、返回、取消。
   - 保证现有 SQLite 和 dbx 连接都不回归。

5. 实现树和右键菜单 parity
   - 先连接节点、数据库节点、schema 节点、表/视图节点。
   - 再列/索引/外键/触发器。
   - 最后 Redis/Mongo/其他对象类型。

6. 实现新建数据库
   - 先 SQL 类型。
   - 再 DuckDB attach 文件。
   - 加错误提示、刷新和选中逻辑。

7. 替换主工作区为 dbx 式查看
   - 查询编辑器、数据表格、对象浏览、表结构、导入导出、搜索、ER 图等逐步接入。
   - 每接入一个入口，移除对应占位提示。

8. 样式收敛
   - `src/styles/database.ts` 中集中维护 dbx 风格 token。
   - 避免页面内联新增大量一次性样式。
   - 做桌面和窄屏检查，确保按钮文字不挤压或重叠。

9. 测试和验收
   - 跑上面列出的测试命令。
   - 手动验证连接右键菜单、新建库、表查看、查询、Redis/Mongo 工作区。

## 当前差距摘要

Aeroric 当前已有：

- DBX 连接保存、测试、连接、断开。
- 列数据库、列 schema、列对象、查列、查 DDL。
- 执行 SQL、多语句、取消查询、关闭结果 session。
- 表数据查询、编辑、导出、导入。
- Redis、Mongo 基础浏览。
- 数据传输、结构对比、数据对比、ER 图、表结构面板的部分入口和组件。

Aeroric 当前明显缺失或未完整实现：

- dbx 式树形侧边栏。
- 连接分组、pin、多选、快捷键、懒加载、虚拟滚动。
- 侧边栏搜索范围过滤。
- 完整节点级右键菜单。
- 新建数据库真实执行能力。
- create/drop schema、drop database、drop table、truncate、empty、duplicate、rename object 等管理 SQL 包装。
- 可见数据库选择弹窗。
- dbx 式连接弹窗完整字段和 transport layers。
- dbx 式多 tab 查询/数据/对象浏览工作区。
- 完整 DataGrid 行为。
- 大量 i18n key。

## 验收标准

完成后逐项确认：

- 新建连接弹窗外观和流程与 dbx 一致。
- 连接树的节点层级、图标、展开、选中、多选、搜索与 dbx 一致。
- 每类节点右键菜单项、顺序、图标、分隔线、危险样式与 dbx 一致。
- 连接节点能新建数据库；MySQL 兼容类型支持 charset/collation；DuckDB 支持创建并 attach 文件。
- 创建、drop、刷新后树状态正确更新。
- 查询、表数据、表结构、导入、导出、ER 图、Redis、Mongo 的入口和查看方式与 dbx 一致。
- 只读连接禁用写操作。
- 未连接、驱动缺失、执行失败时提示与 dbx 一致。
- `pnpm test`、`pnpm lint`、`pnpm build` 通过。
