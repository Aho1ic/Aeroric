# Aeroric 数据库模块完全对齐 dbx 的提示词与操作步骤

生成时间：2026-06-20

## 使用方式

把本文档中“给实现者的完整提示词”整段交给下一位实现者或 AI Agent。目标不是照搬 dbx 架构，而是在 Aeroric 当前 React/hooks/Tauri 架构下，把数据库模块的操作逻辑、按钮功能、查看方式、界面样式，尤其按钮样式，做到与 dbx 一模一样。

## 本次只读浏览范围

dbx 参考项目：

- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/ui/button/index.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/ui/button/Button.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/sidebar/ConnectionTree.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/sidebar/TreeItem.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/connection/ConnectionDialog.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/grid/DataGrid.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/editor/QueryEditor.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/crates/dbx-core/src/db_admin_sql.rs`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/src-tauri/src/commands/query.rs`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/docs/content/docs/*.mdx`

Aeroric 当前数据库模块：

- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/DatabaseView.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/DatabaseSidebarTree.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/DatabaseSearchPanel.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/DatabaseUserAdminPanel.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/RedisBrowser.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/MongoBrowser.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/TableStructurePanel.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/ErDiagramPanel.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/lib/databaseApi.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/types/database.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/styles/database.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src-tauri/src/database/*.rs`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/docs/dbx-database-parity-prompt.md`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/docs/dbx-database-parity-handoff.md`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/docs/dbx-database-parity-pause-summary.md`

## 当前判断

Aeroric 已经有大量 dbx 数据库对齐增量，不要从零重写。下一步应该继续在现有基础上收敛：

- 保留 Aeroric 的 React 组件、hooks、Tauri invoke、现有测试结构。
- dbx 只作为行为、状态、菜单顺序、视觉密度、按钮样式和交互细节的金标准。
- 优先拆小组件和样式基元，避免继续把所有逻辑堆在 `DatabaseView.tsx`。
- 高风险数据库写操作必须预览 SQL 或操作摘要，并要求确认。
- 当前工作区已有数据库相关脏改动，全部视为用户/前序实现者工作，不要回滚。

### 当前完成度矩阵

根据 `docs/dbx-database-parity-prompt.md`、`docs/dbx-database-parity-handoff.md`、`docs/dbx-database-parity-pause-summary.md` 以及当前代码核对结果，整体 exact parity **尚未完成**。后续实现者必须按下面状态继续推进，不要把“测试通过”误判为“完全对齐完成”。

| 模块 | 当前状态 | 说明 |
| --- | --- | --- |
| 连接入口和顶部工具栏 | 部分完成 | 工具栏动作和能力禁用逻辑已存在，但按钮视觉、hover/active/disabled、入口行为仍需按 dbx 精确审查。 |
| 新建连接弹窗 | 部分完成 | 已有两步式类型选择、icon/list/search、配置 tabs、TLS、Redis/Mongo/Oracle、transport layers、footer 测试结果；仍需补 dbx 全 profile 字段、复制测试结果、transport layer 复制/上移/下移和视觉细节。 |
| 侧边栏树形浏览 | 部分完成 | 已有 `DatabaseSidebarTree`、连接组、SQL/Redis/Mongo 层级、懒加载、多选、快捷键、拖拽和定位能力；仍需补虚拟列表或等效性能方案、完整节点语义和精确视觉密度。 |
| 右键菜单 | 部分完成 | 连接、SQL 节点、NoSQL 节点已有大量菜单；仍需按 dbx 源码逐项核对顺序、分隔线、禁用态、危险项、submenu 和剩余动作。 |
| 新建数据库 | 基本完成 | create database、DuckDB attach、MySQL charset/collation、创建后执行和刷新已有覆盖；后续以补视觉/错误提示/边缘类型为主。 |
| 后端管理 SQL wrapper | 基本完成 | create/drop database、create/drop schema、drop/truncate/empty/rename/duplicate/table child drop/search 等 wrapper 已存在；不要重复新增。 |
| 主工作区查看方式 | 部分完成 | 查询、数据、结构、搜索、ER、传输、Redis/Mongo 等模式已有；仍未形成与 dbx 等价的完整多 tab 工作区模型。 |
| DataGrid | 部分完成 | 已有分页、排序、过滤、搜索、列显示、编辑、SQL preview、行增删、选中行导出等；仍需补列宽拖拽、XLSX/import mapping、特殊数据预览和完整 UX。 |
| 样式 exact parity | 未完成 | 目前不能判定与 dbx 视觉一模一样，必须截图审查按钮、菜单、弹窗、树、DataGrid。 |
| i18n | 部分完成 | 中英文 key 已大量补齐；所有新增菜单、按钮、状态、错误提示仍必须补中英文。 |
| 测试 | 部分完成 | 聚焦数据库测试已通过，但 exact parity 缺少视觉截图验收和部分深层交互测试。 |

### 已完成能力不要重复实现

开始编码前必须先搜索当前实现。下面能力在当前工作区已经有实现或 substantial coverage，后续只做缺口修补、行为补全、样式收敛和测试增强，不要重复创建另一套平行 API 或组件：

- DBX 管理 SQL wrapper：
  - `dbx_build_create_database_sql`
  - `dbx_build_duckdb_attach_database_sql`
  - `dbx_build_drop_database_sql`
  - `dbx_build_create_schema_sql`
  - `dbx_build_drop_schema_sql`
  - `dbx_build_drop_table_sql`
  - `dbx_build_truncate_table_sql`
  - `dbx_build_empty_table_sql`
  - `dbx_build_rename_object_sql`
  - `dbx_build_drop_object_sql`
  - `dbx_build_drop_table_child_object_sql`
  - `dbx_build_duplicate_table_structure_sql`
  - `dbx_build_database_search_sql`
- SQLite 文件备份：`dbx_backup_sqlite_database`。
- 前端 API wrapper：优先检查 `src/lib/databaseApi.ts`。
- 后端 command 注册：优先检查 `src-tauri/src/lib.rs`。
- 类型定义：优先检查 `src/types/database.ts` 和 `src/types.ts`。
- 侧边栏树组件：`src/components/database/DatabaseSidebarTree.tsx`。
- 数据库搜索面板：`src/components/database/DatabaseSearchPanel.tsx`。
- 用户管理面板：`src/components/database/DatabaseUserAdminPanel.tsx`。
- Redis/Mongo 浏览器：`RedisBrowser.tsx`、`MongoBrowser.tsx`。

如果发现已有能力不完整，应在原实现上收敛，不要新增重复命名、重复状态或重复 command。

### 当前验证基线

最近一次核对后，当前工作区通过了以下验证：

```bash
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
cargo check --manifest-path src-tauri/Cargo.toml
pnpm build
```

结果：

- 数据库聚焦测试：37 个测试文件通过，385 个测试通过。
- `pnpm lint`：通过。
- `git diff --check`：通过。
- `cargo check --manifest-path src-tauri/Cargo.toml`：通过。
- `pnpm build`：通过，只有预期的大 chunk warning。

后续实现者不能复用这个结果声称新改动通过；每轮改动后必须重新运行相关验证。

## 给实现者的完整提示词

你现在在项目 `/Users/macbook/Downloads/同步空间/LYX/Aeroric` 中工作。请把 Aeroric 数据库模块改到与 dbx 项目 `/Users/macbook/Downloads/同步空间/LYX/dbx` 的数据库模块一模一样。

“一模一样”指用户可观察行为和视觉一致，包括：

- 操作逻辑一致
- 按钮功能一致
- 右键菜单项目、顺序、分隔线、禁用状态、危险项样式一致
- 侧边栏树查看方式一致
- 查询、表数据、表结构、对象浏览、导入导出、ER 图、搜索、Redis、Mongo 的打开方式一致
- 连接弹窗流程、字段、Tabs、测试连接反馈一致
- 按钮大小、圆角、hover、active、disabled、图标尺寸、文字间距、焦点态一致

不要照搬 dbx 架构。dbx 是 Vue 3 + Pinia + shadcn-vue + Tauri；Aeroric 是 React + hooks + Tauri。实现时必须使用 Aeroric 的架构习惯，并在行为和视觉上等价还原 dbx。

### 最高优先级约束

1. 不回滚用户已有改动。
2. 不删除文件、不大规模重构、不改 git 历史、不推送远程，除非用户明确确认。
3. 所有新增 UI 文案必须写入 `src/i18n.tsx` 的中英文 key。
4. 所有新增 Tauri command 必须在 `src-tauri/src/lib.rs` 注册，并通过 `src/lib/databaseApi.ts` 封装。
5. dbx 中已有 SQL 构造能力的，优先在 Rust/Tauri 层做薄包装，不在 React 前端拼接复杂 SQL。
6. 只读连接必须禁用所有写操作；危险操作必须确认。
7. 每做一个功能组，补测试或更新现有测试。

## 操作步骤

### 第 0 步：差距审计，不要直接开写

每次开始新一轮 exact parity 工作前，必须先做差距审计：

1. 运行 `git status --short`，确认当前脏改动，所有既有改动都视为用户/前序实现者工作。
2. 读取：
   - `docs/dbx-database-parity-prompt.md`
   - `docs/dbx-database-parity-handoff.md`
   - `docs/dbx-database-parity-pause-summary.md`
   - 本文档
3. 对照 dbx 源码和 Aeroric 当前实现，列出本轮只处理的 1-3 个缺口。
4. 每个缺口必须写清：
   - 对应 dbx 源码文件和函数/组件。
   - 当前 Aeroric 对应文件和缺失点。
   - 本轮会改哪些文件。
   - 验证命令和手工验收点。
5. 先搜索已有实现，再决定是否新增代码。禁止在未确认现状前重复实现 API、组件或状态。

建议搜索入口：

```bash
rg -n "目标 command 或 UI 文案" src src-tauri docs
rg -n "DatabaseSidebarTree|dbx_build_|DbWorkspaceMode|contextMenu|transport_layers" src src-tauri
```

审计完成后再进入下面步骤。

### 第 1 步：先建立 dbx 风格 UI 基元，重点修按钮

当前 Aeroric 按钮主要来自 `src/styles/database.ts` 的 inline style，例如 `databaseToolbarButton`、`databaseSmallButton`、`databaseIconButton`，视觉偏散、状态不完整。先做统一基元，再替换数据库模块按钮。

参考 dbx：

- `apps/desktop/src/components/ui/button/index.ts`
- `apps/desktop/src/components/ui/button/Button.vue`

需要在 Aeroric 建立等价 React 样式或组件，例如：

- `DbxButton`
- `DbxIconButton`
- `DbxButtonGroup`
- `DbxMenuItem`
- `DbxDialogFooterButton`
- `DbxSegmentedButton`

按钮视觉规格必须对齐 dbx：

- 默认按钮：`h-8`，文字 `text-sm`，font weight 约 500，gap 约 6px，左右 padding 约 10px。
- 小按钮：`h-7`，文字约 `0.8rem`，图标 14px。
- 极小按钮：`h-6`，文字 `text-xs`，图标 12px。
- 图标按钮：默认 `32x32`，小号 `28x28`，极小 `24x24`。
- 圆角：默认接近 `rounded-lg`，小按钮不超过 10-12px。
- 状态：
  - default：主色背景、主色前景。
  - outline：边框、背景、hover muted。
  - secondary：secondary 背景。
  - ghost：透明背景、hover muted。
  - destructive：浅红背景、红色文字、hover 更深。
  - disabled：`pointer-events: none` + 透明度降低。
  - focus-visible：ring/border 高亮。
  - active：轻微下压，不要夸张动画。
- 图标：
  - 使用 `lucide-react`。
  - 工具类按钮优先 icon-only 或 icon + text。
  - 不要用纯文字大按钮替代 dbx 的图标按钮。

替换顺序：

1. 顶部工具栏按钮。
2. 侧边栏搜索/过滤/定位按钮。
3. 树节点展开、pin、加载更多按钮。
4. 右键菜单项。
5. 弹窗 footer 按钮。
6. 连接弹窗中的 view toggle、文件选择、transport layer 控制按钮。
7. DataGrid 工具栏按钮。

验收标准：

- 数据库页不再出现“高度、圆角、边框不一致”的按钮。
- disabled、hover、active、focus-visible 状态肉眼与 dbx 接近。
- 按钮文字不溢出、不挤压图标。
- 所有图标尺寸与按钮尺寸匹配。

### 第 2 步：连接弹窗完全对齐 dbx

参考：

- `dbx/apps/desktop/src/components/connection/ConnectionDialog.vue`
- Aeroric 当前连接弹窗在 `DatabaseView.tsx` 约 `wizardStep`、`configTab`、`transport` 相关区域。

必须实现两步式流程：

1. 数据库类型选择页。
2. 连接配置页。

类型选择页必须包括：

- Dialog 宽度约 760px。
- icon/list 两种视图切换。
- 右上搜索框。
- 数据库类型按 dbx 分组。
- icon 视图：2/4/5 列响应式网格、卡片 `min-h` 约 96px、图标容器、选中边框、主色浅背景、hover 轻微上移。
- list 视图：紧凑行、图标、名称、分类。
- 双击类型进入配置页。
- footer 左侧显示当前选择的数据库类型，右侧 Next 按钮。

配置页必须包括：

- Dialog 宽度约 560px。
- Tabs：连接、TLS、SSH/代理、高级。
- 连接页：
  - 连接 URL 输入与解析按钮。
  - 名称。
  - 当前数据库类型 mini card，点击返回类型选择。
  - 颜色选择。
  - 文件型数据库路径选择和创建按钮。
  - MySQL/PostgreSQL/SQL Server/Oracle/ClickHouse 等 host、port、user、password、database、url params。
  - Redis standalone/sentinel/cluster。
  - MongoDB URL 模式和表单模式切换。
  - Oracle Service Name/SID 与 SYSDBA。
  - PostgreSQL/MySQL TLS mode 下拉。
  - JDBC 驱动路径和 driver class。
- TLS 页：
  - ssl 开关。
  - CA、client cert、client key 文件路径。
  - PostgreSQL/MySQL 专属 TLS mode。
- SSH/代理页：
  - 多层 transport layers，不是一个简单开关。
  - 支持 SSH、Proxy 两类 layer。
  - 支持新增、复制、删除、上移、下移、启用/禁用。
  - 显示 transport path segments。
  - 支持 ssh agent、key passphrase、expose LAN、connect timeout。
- 高级页：
  - connect timeout。
  - query timeout。
  - idle timeout。
  - keepalive。
  - read only。

footer 必须对齐 dbx：

- 左侧显示测试连接结果，成功绿色、失败红色，单行 truncate。
- 测试结果旁有复制按钮。
- 配置页 footer 有返回、选择可见数据库、测试连接、保存/创建。
- 测试结果不要只放全局 error banner。

### 第 3 步：侧边栏树完全对齐 dbx

参考：

- `dbx/apps/desktop/src/components/sidebar/ConnectionTree.vue`
- `dbx/apps/desktop/src/components/sidebar/TreeItem.vue`
- Aeroric 当前 `DatabaseSidebarTree.tsx`

侧边栏必须是 dbx 树模型：

- 顶部 sticky 搜索区。
- 搜索输入 `h-6`、左侧 Search 图标、右侧清空按钮。
- 定位当前工作区按钮 Crosshair。
- 范围过滤按钮 ListFilter。
- 搜索 300ms 防抖。
- 支持搜索连接、数据库、schema、表、视图。
- 搜索时保留可展开/折叠行为。
- 大量节点使用虚拟列表或等效方案。

树行视觉必须对齐：

- 行高和 padding 紧凑，近似 dbx `py-1 px-2`。
- 展开按钮 `16x16`，图标 `14px`。
- 节点图标 `14px`。
- label 单行截断。
- 默认数据库 badge 高度约 16px。
- 只读连接 badge 小尺寸，带锁图标。
- 已连接小绿点。
- pin 图标默认隐藏，hover 或已 pin 时显示。
- 选中态、multi-select 态、hover 态使用 dbx 的 muted/accent 视觉。

树功能必须包括：

- 连接分组。
- 连接展开/收起。
- 数据库、schema、对象分组。
- 表、视图、物化视图、列、索引、外键、触发器。
- 过程、函数、序列、包。
- Redis db、Redis key。
- Mongo database、collection、document preview。
- 懒加载 children。
- 节点级 refresh。
- 多选：
  - 普通点击：选中并按设置激活。
  - Ctrl/Cmd 点击：切换选择。
  - Shift 点击：范围选择。
- 快捷键：
  - F2 重命名。
  - F5 刷新。
  - Delete/Backspace 删除或 drop。
  - Mod+C 复制名称。
  - Mod+V 粘贴表结构复制。
- 表/视图拖拽到 SQL 编辑器。
- 连接/数据库/schema/table 定位当前工作区。

### 第 4 步：右键菜单项目、顺序和样式完全对齐 dbx

参考 `TreeItem.vue` 的 `treeItemMenuItems()`，不要凭感觉重排。

菜单视觉要求：

- 菜单项高度、图标、gap、hover 背景对齐 dbx。
- 有分隔线。
- destructive 项为红色。
- submenu 有箭头。
- disabled 状态可见但不可点。
- 菜单打开时，节点先变为当前选择。

连接节点菜单顺序：

1. Pin/Unpin。
2. 打开连接或关闭连接。
3. 新建查询。
4. SQL 历史。
5. 用户管理。
6. 复制最终代理端口。
7. 执行 SQL 文件。
8. 新建数据库；DuckDB 显示创建 DuckDB 文件。
9. 移至分组或新建分组。
10. 刷新。
11. 选择显示数据库。
12. 编辑连接。
13. 在文件管理器中显示数据库文件。
14. 备份 SQLite 数据库。
15. 复制连接。
16. 删除连接。

连接分组菜单：

1. 复制名称。
2. 新建连接。
3. 新建分组。
4. 重命名分组。
5. 删除分组。

数据库/schema 菜单顺序：

1. Copy name。
2. Open object browser。
3. New query。
4. SQL history。
5. Set/Clear default database。
6. Create table。
7. Create schema。
8. Execute SQL file。
9. Open ER diagram。
10. Database search。
11. Refresh。
12. Data transfer。
13. Schema diff。
14. Data compare。
15. Export database。
16. Close database connection。
17. Drop database/schema。

Redis/Mongo database 菜单：

- New query。
- Set/Clear default database。
- Redis db 支持 Flush DB，危险项。
- Copy/open/refresh 等 Aeroric 已有项若 dbx 没有，应按 dbx 为准；如保留必须确认不会破坏一致性。

表/视图/物化视图菜单顺序：

1. Copy name。
2. View data。
3. View DDL。
4. View/Edit source。
5. Edit structure。
6. Table info。
7. Rename object。
8. Drop view/object。
9. Generate SQL 子菜单：SELECT、INSERT、UPDATE、DELETE、DDL。
10. SQL history。
11. Open ER diagram。
12. Import data。
13. Data compare。
14. Export data 子菜单：CSV、JSON、SQL INSERT、SQL UPDATE、XLSX 等。
15. Export database。
16. Export structure。
17. Copy structure as TSV/Markdown。
18. Duplicate structure。
19. Truncate table。
20. Empty table。
21. Drop table。
22. Refresh。

列、索引、外键、触发器菜单：

- Copy name。
- Column 支持 Field lineage。
- Drop column/index/foreign key/trigger。
- drop 前必须显示 SQL 预览并确认。

过程/函数/包/序列菜单：

- Execute procedure。
- View source。
- Rename object。
- Drop procedure/function。
- Copy name。

### 第 5 步：主工作区查看方式对齐 dbx

不要把所有功能塞在一个页面状态里。即使不引入 Pinia，也要在 React 中建立等价工作区状态。

工作区模式至少包括：

- query
- table data
- SQL file execution
- query history
- object browser
- table structure
- table info
- field lineage
- user admin
- ER diagram
- database search
- database export
- table import
- data transfer
- schema diff
- data compare
- Redis browser
- Mongo browser

要求：

- 从树节点或右键菜单打开对应工作区。
- 重复打开时按 dbx 行为复用或激活已有工作区。
- 标题、副标题、路径、加载态、错误态对齐 dbx。
- “高级工具入口”不能只是占位提示，必须进入真实功能。

### 第 6 步：DataGrid 对齐 dbx

参考：

- `dbx/apps/desktop/src/components/grid/DataGrid.vue`
- Aeroric 当前 `DatabaseView.tsx` 中 grid 相关逻辑。

必须逐步补齐：

- 固定表头。
- 行号/rowid。
- 列宽、拖拽调整、自动适配。
- 分页。
- 刷新按钮。
- WHERE / ORDER BY 输入。
- 排序。
- 结构化过滤。
- 右键单元格过滤：
  - Filter by this value。
  - Exclude this value。
  - Contains。
  - Does not contain。
  - Less than。
  - Greater than。
  - NULL / NOT NULL。
  - Clear filter。
- 列显示隐藏。
- 搜索。
- 单元格编辑。
- 新增行。
- 删除行。
- 保存前 SQL 预览和 rollback SQL。
- SQL Preview 侧栏。
- 大字段/JSON/二进制/图片/空间数据预览。
- 选中行导出。
- CSV、JSON、SQL INSERT、SQL UPDATE、XLSX 导出。
- CSV/XLSX 导入预览、字段映射、append/truncate 等模式。

### 第 7 步：后端和 API 对齐

优先复用 dbx-core 已有逻辑。Aeroric Rust 层应该新增薄包装，但必须先确认当前是否已经存在对应 wrapper。已存在的 wrapper 只补缺失参数、行为、测试或调用入口，不要重复实现。

需要核对的 wrapper 类型包括：

- create database SQL。
- DuckDB attach database SQL。
- drop database SQL。
- create/drop schema SQL。
- drop table/object SQL。
- truncate/empty table SQL。
- duplicate structure SQL。
- rename object SQL。
- table child object drop SQL。
- data grid save SQL。
- context filter SQL。
- export/import helper。

每个 Tauri command 都要：

1. 在 `src-tauri/src/database/*.rs` 实现。
2. 在 `src-tauri/src/lib.rs` 注册。
3. 在 `src/lib/databaseApi.ts` 封装。
4. 在 `src/types/database.ts` 或 `src/types.ts` 定义类型。
5. 在 `src/test/database-api.test.ts` 覆盖 payload。

执行前必须搜索：

```bash
rg -n "dbx_build_create_database_sql|dbx_build_drop_database_sql|dbx_build_create_schema_sql|dbx_build_drop_schema_sql|dbx_build_drop_table_sql|dbx_build_truncate_table_sql|dbx_build_empty_table_sql|dbx_build_rename_object_sql|dbx_build_duplicate_table_structure_sql|dbx_build_database_search_sql|dbx_backup_sqlite_database" src src-tauri
```

如果搜索结果已覆盖当前需求，转为补 UI 调用、错误处理、确认流程或测试，不要新增同义 command。

### 第 8 步：样式收敛，不要只“看起来差不多”

必须建立数据库模块统一样式，不要继续散落大量一次性 inline style。

建议：

- 在 `src/styles/database.ts` 中新增 dbx 风格 token，或拆成 `src/components/database/dbxUi.tsx`。
- 先统一按钮、菜单、弹窗、输入框、Tabs、Badge。
- 再统一树、工具栏、DataGrid。

重点检查：

- 顶部工具栏高度约 40px，但按钮自身按 dbx Button size。
- 侧边栏宽度约 284px 可以保留，但内部密度按 dbx。
- 弹窗最大宽度：
  - 类型选择：760px。
  - 配置页：560px。
  - 可见数据库：460px。
- 输入框高度：
  - 搜索小输入：24px。
  - 普通输入：36px。
- 菜单项图标 13-14px。
- 树节点图标 14px。
- 不使用过大的圆角、厚重边框、纯文字块状按钮。
- 不使用一堆同色块造成“后台模板感”。

视觉验收必须有截图或等效证据，不能只靠主观描述。建议至少检查这些状态：

- 数据库首页/工作区整体。
- 新建连接类型选择页。
- 新建连接配置页的连接/TLS/SSH/高级四个 tab。
- 侧边栏树：普通节点、hover、selected、multi-selected、pinned、default badge、read-only badge。
- 右键菜单：连接、数据库/schema、表/视图、列、Redis、Mongo。
- DataGrid：工具栏、表头、行选中、单元格菜单、SQL preview。
- 小宽度窗口下按钮文字是否溢出或遮挡图标。

如条件允许，使用 Playwright 或等效截图工具做桌面尺寸截图检查；否则必须在最终报告中明确说明只做了代码/测试验证，没有做视觉截图验收。

### 第 9 步：测试与验收

每个增量至少跑相关测试。完整验证建议：

```bash
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
pnpm build
```

如果改 Rust：

```bash
cargo check
```

最终手工验收清单：

- 新建连接弹窗流程和视觉与 dbx 一致。
- 按钮样式不再丑，尺寸、圆角、hover、disabled、图标与 dbx 一致。
- 连接树层级、搜索、过滤、定位、多选、快捷键与 dbx 一致。
- 每类节点右键菜单项、顺序、分隔线、危险项样式与 dbx 一致。
- 连接节点能新建数据库；MySQL 兼容类型支持 charset/collation；DuckDB 支持创建并 attach 文件。
- 数据库/schema/table/drop/truncate/empty/rename/duplicate 等危险操作都有 SQL 预览和确认。
- 查询、表数据、表结构、导入、导出、ER 图、搜索、Redis、Mongo 的入口和查看方式与 dbx 一致。
- 只读连接禁用写操作。
- 未连接、驱动缺失、执行失败时提示与 dbx 一致。
- 所有新增文案中英文都存在。

### 阶段完成定义

每一轮只能声明“本阶段完成”，不能因为某组测试通过就声明“数据库模块完全对齐完成”。阶段完成必须满足：

1. 本阶段的 dbx 参考文件、Aeroric 修改文件和差距点已列明。
2. 本阶段新增或修改的 UI 行为有测试，或明确说明为何只能手工验收。
3. 已运行本阶段相关测试、`pnpm lint`、`git diff --check`；涉及 Rust 时运行 `cargo check --manifest-path src-tauri/Cargo.toml`。
4. 视觉相关阶段必须提供截图验收结论，或明确标注未完成视觉验收。
5. 剩余未完成 exact parity 项必须写入 handoff 或最终报告。

禁止使用这些表述，除非已经逐项通过完整手工和自动验收：

- “数据库模块已完全对齐 dbx”
- “所有菜单都已完成”
- “视觉已经一模一样”
- “DataGrid 已完整 parity”

## 推荐拆分顺序

1. UI 基元和按钮样式对齐。
2. 连接弹窗样式与 footer 行为对齐。
3. 侧边栏搜索区和树行样式对齐。
4. 右键菜单样式与菜单顺序对齐。
5. 补齐缺失菜单动作。
6. DataGrid 工具栏和右键菜单对齐。
7. DataGrid 编辑、预览、导入导出补齐。
8. 工作区多 tab/多视图模型收敛。
9. Redis/Mongo 剩余菜单和查看方式补齐。
10. 全量测试、lint、build、手工截图审查。

## 需要特别避免的错误

- 不要把 dbx Vue 代码直接翻译成一个巨大的 React 文件。
- 不要只复制菜单文案，不实现真实动作。
- 不要只改颜色，忽略 hover/active/disabled/focus。
- 不要把危险操作直接执行。
- 不要因为架构不同就改变用户操作路径。
- 不要保留“占位提示型”高级工具入口。
- 不要在前端手写复杂 SQL 代替后端 SQL builder。
- 不要删除或回滚当前脏工作区改动。
