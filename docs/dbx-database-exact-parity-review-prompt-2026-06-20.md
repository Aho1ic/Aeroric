# Aeroric 数据库模块对齐 dbx 的完整提示词与操作步骤

生成时间：2026-06-20

## 使用说明

本文档是给下一轮实现者或 AI Agent 使用的完整提示词。当前轮只做了只读源码浏览和文档整理，没有修改数据库业务代码。

目标是：在 Aeroric 架构不同于 dbx 的前提下，把 Aeroric 数据库模块的用户可见功能、操作逻辑、按钮功能、查看方式和界面样式改到与 dbx 一模一样，尤其是按钮样式。

“一模一样”指用户可观察结果一致，不是把 dbx 的 Vue/Pinia 代码硬搬到 Aeroric。Aeroric 必须继续使用 React、hooks、Tauri invoke、当前测试结构和现有样式体系。

## 只读浏览范围

dbx 参考项目：

- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/ui/button/index.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/ui/button/Button.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/styles/globals.css`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/sidebar/ConnectionTree.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/sidebar/TreeItem.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/connection/ConnectionDialog.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/grid/DataGrid.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/editor/QueryEditor.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/redis/RedisKeyBrowser.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/redis/RedisValueViewer.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/admin/DatabaseUserAdmin.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/search/DatabaseSearchDialog.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/diagram/SchemaDiagramDialog.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/export/DatabaseExportDialog.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/components/import/TableImportDialog.vue`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/stores/connectionStore.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/stores/queryStore.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/lib/databaseCapabilities.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/lib/databaseFeatureSupport.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/lib/databaseTree.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/lib/tableTree.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/lib/treeNodeClick.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/lib/sidebarTreeSelection.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/lib/sidebarSearchTree.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/lib/connectionTransport.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/lib/createDatabaseSql.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/apps/desktop/src/lib/dbAdminSql.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/packages/node-core/src/database.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/packages/node-core/src/schema-context.ts`
- `/Users/macbook/Downloads/同步空间/LYX/dbx/packages/node-core/src/sql-safety.ts`

Aeroric 当前数据库模块：

- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/DatabaseView.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/DatabaseSidebarTree.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/DatabaseSearchPanel.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/DatabaseUserAdminPanel.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/DatabaseAdvancedTools.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/TableStructurePanel.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/RedisBrowser.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/MongoBrowser.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/ErDiagramPanel.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/components/database/databaseActions.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/hooks/useDatabaseConnections.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/hooks/useDatabaseSchema.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/hooks/useDatabaseQuery.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/hooks/useDataGrid.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/hooks/useRedisBrowser.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/hooks/useMongoBrowser.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/lib/databaseApi.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/lib/databaseUtils.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/lib/redisCommandSafety.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/lib/redisCommandSession.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/lib/redisKeyPattern.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/lib/redisKeyTree.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/types/database.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/types.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/i18n.tsx`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/styles/database.ts`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src-tauri/src/database/*.rs`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src-tauri/src/lib.rs`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/src/test/database-*.test.*`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/docs/dbx-database-parity-prompt.md`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/docs/dbx-database-parity-handoff.md`
- `/Users/macbook/Downloads/同步空间/LYX/Aeroric/docs/dbx-database-parity-pause-summary.md`

## 当前只读结论

Aeroric 已经有大量 dbx parity 增量，不能从零重写，也不能复制出第二套平行实现。当前重点不是“新增一个数据库页”，而是在已有 React/Tauri 数据库模块上继续收敛。

关键结论：

- dbx 的按钮样式有统一来源：`buttonVariants`。核心类包含 `rounded-lg`、`text-sm font-medium`、`focus-visible:ring-3`、`active:translate-y-px`、`disabled:pointer-events-none disabled:opacity-50`，并定义了 `default`、`outline`、`secondary`、`ghost`、`destructive`、`link` 六类 variant，以及 `default/xs/sm/lg/icon/icon-xs/icon-sm/icon-lg` 尺寸。
- Aeroric 当前按钮主要散落在 `src/styles/database.ts` 的 `databaseToolbarButton`、`databaseSmallButton`、`databaseIconButton` 等 inline style，状态和尺寸不统一，这是“按钮很丑”的主要原因。
- Aeroric 已有 `DatabaseSidebarTree.tsx`、`DatabaseSearchPanel.tsx`、`DatabaseUserAdminPanel.tsx`、Redis/Mongo browser、dbx 管理 SQL wrapper、部分菜单和测试。后续必须先搜索已有实现，再补缺口。
- dbx 的数据库体验是多 tab 工作区、树形对象浏览、右键菜单驱动、危险操作 SQL 预览确认、DataGrid 工具栏密集操作、连接弹窗分步配置。Aeroric 需要复刻用户行为和视觉，而不是改成 Vue/Pinia。
- 当前工作区存在大量数据库相关 dirty/untracked 文件，应视为用户或前序实现者工作。不要回滚，不要格式化无关文件。

## 给实现者的完整提示词

你现在在项目 `/Users/macbook/Downloads/同步空间/LYX/Aeroric` 中工作。请把 Aeroric 的数据库模块改到与 dbx 项目 `/Users/macbook/Downloads/同步空间/LYX/dbx` 的数据库模块一模一样。

这里的“一模一样”是用户可观察层面的 exact parity，包括：

- 操作逻辑一致。
- 按钮功能一致。
- 按钮视觉一致，尤其是大小、圆角、hover、active、disabled、focus、图标尺寸、文字间距。
- 侧边栏树查看方式一致。
- 右键菜单项目、顺序、分隔线、禁用状态、危险项样式一致。
- 连接弹窗流程、字段、tabs、测试连接反馈、transport layers 一致。
- 查询、表数据、表结构、对象浏览、导入导出、ER 图、搜索、Redis、Mongo 的打开方式一致。
- DataGrid 的分页、排序、过滤、搜索、编辑、导出、列显示、详情查看、SQL preview 行为一致。
- 只读连接、未连接、不同数据库类型下的能力开关一致。

不要照搬 dbx 架构。dbx 是 Vue 3 + Pinia + shadcn-vue + Tauri；Aeroric 是 React + hooks + Tauri。实现时必须保持 Aeroric 的 React 组件、hooks、`databaseApi.ts`、Tauri command、Rust backend、Vitest 测试结构。

### 最高优先级约束

1. 不回滚用户已有改动。
2. 不删除文件、不大规模重构、不改 git 历史、不推送远程、不改 CI，除非用户明确确认。
3. 所有新增 UI 文案必须补 `src/i18n.tsx` 的中英文 key。
4. 所有新增 Tauri command 必须在 `src-tauri/src/lib.rs` 注册，并通过 `src/lib/databaseApi.ts` 封装。
5. dbx 已有 SQL 构造能力的，优先在 Rust/Tauri 层做薄 wrapper，不在 React 前端拼复杂 SQL。
6. 只读连接必须禁用写操作。
7. 危险操作必须展示 SQL 或操作摘要并二次确认。
8. 每完成一组功能都要补测试或更新现有测试。
9. 所有样式修改必须检查亮色/暗色主题。
10. 不允许为了“看起来像”而牺牲可访问性：按钮必须有 title 或 aria-label，icon-only 按钮尤其如此。

## 操作步骤

### 第 0 步：差距审计，不要直接开写

开始编码前先做审计：

```bash
git status --short
rg -n "DatabaseSidebarTree|databaseToolbarButton|databaseSmallButton|databaseIconButton|dbx_build_|contextMenu|transport|DbWorkspaceMode" src src-tauri docs
```

必须读取这些现有文档：

- `docs/dbx-database-parity-prompt.md`
- `docs/dbx-database-parity-handoff.md`
- `docs/dbx-database-parity-pause-summary.md`
- 本文档

输出本轮只处理的 1-3 个缺口。每个缺口写清：

- 对应 dbx 源码文件和函数/组件。
- Aeroric 当前对应文件。
- 缺失点。
- 本轮会改哪些文件。
- 自动化测试命令。
- 手工/截图验收点。

禁止未审计直接新增组件、API、command 或状态。

### 第 1 步：先做 dbx 风格按钮基元，这是最高优先级

dbx 按钮参考：

- `dbx/apps/desktop/src/components/ui/button/index.ts`
- `dbx/apps/desktop/src/components/ui/button/Button.vue`
- `dbx/apps/desktop/src/styles/globals.css`

Aeroric 当前问题：

- `src/styles/database.ts` 中存在多套 button inline style。
- `DatabaseView.tsx`、`RedisBrowser.tsx`、`MongoBrowser.tsx`、`TableStructurePanel.tsx`、`DatabaseSidebarTree.tsx` 中大量 `<button style={...}>` 风格不统一。
- hover、active、disabled、focus-visible 状态缺失或不一致。

建议实现方式：

1. 在 Aeroric 数据库模块内新增 React 版按钮基元，例如 `src/components/database/DbxButton.tsx` 或在 `src/styles/database.ts` 中建立明确的 `dbxButton*` 样式函数。
2. 推荐组件化，不推荐继续复制 inline style。组件至少支持：
   - `variant`: `default`、`outline`、`secondary`、`ghost`、`destructive`、`link`
   - `size`: `default`、`xs`、`sm`、`lg`、`icon`、`icon-xs`、`icon-sm`、`icon-lg`
   - `active`
   - `disabled`
   - `title`
   - `aria-label`
   - `icon`
   - `iconPosition`
3. 如果项目没有 class-variance-authority，就不要强行引入新依赖。可以用轻量 className/string builder 或局部 CSS module 实现等价样式。

按钮规格必须对齐 dbx：

- 基础：`inline-flex shrink-0 items-center justify-center whitespace-nowrap select-none transition-colors outline-none`。
- 默认高度：32px，对应 dbx `h-8`。
- 小号高度：28px，对应 dbx `h-7`。
- 极小高度：24px，对应 dbx `h-6`。
- 默认图标按钮：32x32。
- 小图标按钮：28x28。
- 极小图标按钮：24x24。
- 默认 gap：6px。
- 小号 gap：4px。
- 默认左右 padding：10px。
- 字号：默认 14px，小号约 12.8px，极小 12px。
- 字重：500。
- 圆角：默认约 10px，sm/xs 不超过 10-12px。
- active：轻微 `translateY(1px)`。
- disabled：`pointer-events: none` + opacity 0.5。
- focus-visible：border/ring 高亮。
- icon-only 必须使用 lucide-react 图标，不能用裸文字冒充。

替换顺序：

1. `DatabaseView.tsx` 顶部工具栏按钮。
2. 连接弹窗 footer：返回、测试连接、保存、取消、复制测试结果。
3. 连接类型选择中的 icon/list toggle、文件选择、颜色选择、transport layer 控制。
4. `DatabaseSidebarTree.tsx` 的搜索、scope、展开、pin、load more、节点 action。
5. 右键菜单项和危险菜单项。
6. `RedisBrowser.tsx` 和 `MongoBrowser.tsx` 工具栏、分页、过滤、删除。
7. DataGrid/表结构/ER/search/admin 面板的所有数据库按钮。

按钮验收标准：

- 同一页面不再出现多套高度、圆角、字体、边框风格。
- icon-only 按钮有 tooltip 或 title/aria-label。
- disabled 状态不可点击，视觉明显但不突兀。
- hover 背景接近 dbx 的 muted/accent 效果。
- destructive 按钮采用浅红背景和红色文字，不用高饱和整块红。
- 所有按钮文字不溢出、不挤压图标。

### 第 2 步：连接弹窗完全对齐 dbx

参考：

- `dbx/apps/desktop/src/components/connection/ConnectionDialog.vue`
- `dbx/apps/desktop/src/lib/connectionTransport.ts`
- `dbx/apps/desktop/src/lib/connectionUrl.ts`
- `dbx/apps/desktop/src/lib/mongoConnectionOptions.ts`

必须实现：

- 两步式流程：数据库类型选择 -> 配置页。
- 类型选择支持 icon view 和 list view。
- 类型选择支持搜索。
- 类型卡片包含图标、名称、描述、选中态、hover、双击进入。
- 配置页包含基础、TLS、SSH/代理、高级等 tabs。
- footer 左侧显示测试连接结果，成功绿色，失败红色，并支持复制测试结果。
- footer 右侧按钮顺序和 dbx 一致。
- 支持连接颜色、只读、连接超时、查询超时、keepalive。
- 支持 URL 解析入口。
- 支持 SQLite/DuckDB 文件选择和 DuckDB attach 行为。
- 支持 PostgreSQL/MySQL TLS mode，并保存到 `url_params`。
- 支持 Oracle Service Name/SID 和 SYSDBA。
- 支持 Redis standalone/sentinel/cluster。
- 支持 MongoDB 表单模式和 URL 模式。
- 支持多层 transport layers：SSH、代理、复制、删除、上移、下移、启用/禁用。

验收：

- 对照 dbx 弹窗逐项点击。
- 新建、编辑连接都能回显字段。
- 测试连接结果在 footer，而不是只弹 toast。
- 只读开关会影响后续写操作可用性。

### 第 3 步：侧边栏树形浏览对齐 dbx

参考：

- `dbx/apps/desktop/src/components/sidebar/ConnectionTree.vue`
- `dbx/apps/desktop/src/components/sidebar/TreeItem.vue`
- `dbx/apps/desktop/src/lib/sidebarTreeSelection.ts`
- `dbx/apps/desktop/src/lib/treeNodeClick.ts`
- `dbx/apps/desktop/src/lib/sidebarTreeItemLayout.ts`
- `dbx/apps/desktop/src/lib/sidebarNodeOrdering.ts`
- `dbx/apps/desktop/src/lib/sidebarSearch.ts`

Aeroric 已有 `DatabaseSidebarTree.tsx`，不要另建平行树。继续补齐：

- 连接分组、连接、数据库、schema、对象分组、表、视图、列、索引、外键、触发器、函数、过程、序列、package。
- Redis：db 节点、key 节点、key type badge、分页 load more。
- MongoDB：database、collection、document preview、分页 load more。
- 节点懒加载。
- 单节点刷新。
- 选中态、多选、Shift 范围选择、Ctrl/Cmd 多选。
- F2 重命名。
- F5 刷新。
- Delete/Backspace 删除或 drop。
- Mod+C 复制名称。
- 拖拽表/视图引用到查询编辑器。
- 侧边栏搜索 debounce、scope filter、清空恢复树。
- 定位当前 active tab。
- 大量节点下使用虚拟列表或等效性能方案。

视觉要求：

- 行高、缩进、展开图标、对象图标、badge、hover、active、pinned 状态对齐 dbx。
- 树节点不要做大卡片；保持 dbx 的紧凑列表密度。

### 第 4 步：右键菜单逐节点完全对齐

参考 dbx `TreeItem.vue` 的菜单构造逻辑。实现时先列出菜单矩阵，再编码。

连接节点菜单必须包含：

- 打开连接/关闭连接。
- 新建查询。
- SQL 历史。
- 用户管理。
- 复制最终代理端口。
- 执行 SQL 文件。
- 新建数据库，DuckDB 为创建/附加 DuckDB 文件。
- 移至分组/新建分组。
- 刷新。
- 选择显示数据库。
- 编辑连接。
- 在文件管理器中显示数据库文件。
- 备份 SQLite 数据库。
- 复制连接。
- 删除连接。

数据库/schema 节点菜单必须包含：

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

表/视图节点菜单必须包含：

- 复制名称。
- 查看数据。
- 编辑视图、查看源码、查看 DDL。
- 编辑结构。
- 表信息。
- 重命名对象。
- 新建 SQL 子菜单：SELECT/INSERT/UPDATE。
- SQL 历史。
- 打开 ER 图。
- 导入数据。
- 数据对比。
- 导出数据子菜单：CSV、JSON、SQL INSERT、XLSX。
- 导出数据库。
- 导出结构。
- 复制结构为 TSV/Markdown。
- 复制表结构。
- 清空表。
- truncate 表。
- drop 表。
- 刷新。

列/索引/外键/触发器菜单必须包含：

- 复制名称。
- 字段血缘。
- drop column/index/foreign key/trigger。
- drop 前必须生成 SQL 预览并确认。

过程/函数/包/序列菜单必须包含：

- 执行过程。
- 查看源码。
- 重命名对象。
- drop procedure/function。

Redis/Mongo 菜单必须继续补齐 dbx 对应行为：

- Redis db/key 的复制、刷新、打开 workspace、删除 key、查看/编辑 value、命令执行安全提示。
- Mongo database/collection/document 的复制、刷新、打开 workspace、插入 document、删除匹配 documents、删除单 document。
- 只读连接禁用删除/写入。

菜单视觉要求：

- 行高、图标尺寸、文字大小、hover、危险项颜色、disabled opacity、分隔线顺序与 dbx 对齐。
- 不允许出现占位菜单项。

### 第 5 步：主工作区和查看方式对齐

参考：

- `dbx/apps/desktop/src/stores/queryStore.ts`
- `dbx/apps/desktop/src/components/layout/AppTabBar.vue`
- `dbx/apps/desktop/src/components/layout/ContentArea.vue`
- `dbx/apps/desktop/src/components/editor/QueryEditor.vue`
- `dbx/apps/desktop/src/components/objects/ObjectBrowser.vue`

Aeroric 需要形成 dbx 等价的 workspace/tab 模型：

- 新建查询 tab。
- 表数据 tab。
- 表结构 tab。
- 对象浏览器 tab。
- SQL 文件执行 tab/dialog。
- SQL 历史。
- ER 图。
- 数据库搜索。
- 数据传输。
- 结构对比。
- 数据对比。
- 用户管理。
- Redis workspace。
- Mongo workspace。
- 关闭 tab、切换 tab、从 active tab 定位到侧边栏。

如果 Aeroric 架构暂时不适合完整多 tab，必须实现等价用户行为：用户从树节点或菜单打开某个对象后，能以 dbx 相同方式查看、刷新、返回、定位和关闭。

### 第 6 步：DataGrid 对齐 dbx

参考：

- `dbx/apps/desktop/src/components/grid/DataGrid.vue`
- `dbx/apps/desktop/src/composables/useDataGridSelection.ts`
- `dbx/apps/desktop/src/composables/useDataGridEditor.ts`
- `dbx/apps/desktop/src/composables/useDataGridColumnResize.ts`
- `dbx/apps/desktop/src/lib/dataGridSaveUi.ts`
- `dbx/apps/desktop/src/lib/dataGridColumnVisibility.ts`
- `dbx/apps/desktop/src/lib/dataGridColumnFilter.ts`
- `dbx/apps/desktop/src/lib/tableDataExport.ts`
- `dbx/apps/desktop/src/lib/binaryCellDownload.ts`
- `dbx/apps/desktop/src/lib/cellDetailPresentation.ts`

必须对齐：

- 分页、页大小、自定义页大小。
- 刷新当前页。
- WHERE 输入和 ORDER BY 输入。
- 结构化过滤器。
- 单元格右键过滤：等于、不等于、包含、不包含、大于、小于、NULL、非 NULL、清除过滤。
- 列排序。
- 列显示/隐藏、全部显示、反选、隐藏全 NULL 列。
- 列宽拖拽。
- 行选择、多选、复制。
- 单元格编辑、行插入、行删除、批量删除。
- 保存按钮状态：无变更、可保存、保存中、失败。
- SQL preview panel：展示生成 SQL 和 rollback SQL。
- JSON/大文本/二进制/图片/几何数据预览。
- CSV、JSON、SQL INSERT、XLSX 导出。
- 导出选中行。
- 导出进度弹窗。
- 导入数据 preview、字段映射和完整预览行。

危险写入必须确认，优先通过后端 wrapper 生成 SQL。

### 第 7 步：新建数据库、管理 SQL 和危险操作

参考：

- `dbx/apps/desktop/src/lib/createDatabaseSql.ts`
- `dbx/apps/desktop/src/lib/dbAdminSql.ts`
- `dbx/apps/desktop/src/lib/tableSqlTemplates.ts`
- `dbx/packages/node-core/src/sql-safety.ts`

Aeroric 已有大量 `dbx_build_*` wrapper，先搜索后补缺：

```bash
rg -n "dbx_build_|drop_database|create_database|truncate|duplicate|rename|search" src src-tauri
```

必须保证：

- create/drop database。
- DuckDB attach database。
- create/drop schema。
- drop/truncate/empty table。
- rename object。
- drop view/procedure/function。
- drop column/index/foreign key/trigger。
- duplicate table structure。
- database search SQL。
- SQLite backup。

所有危险操作必须：

1. 根据当前 database type 生成 SQL。
2. 显示 SQL preview。
3. 用户确认。
4. 执行。
5. 刷新对应节点，不要粗暴刷新整个页面。
6. toast 或状态提示与 dbx 一致。

### 第 8 步：Redis/Mongo 对齐 dbx

Redis 参考：

- `dbx/apps/desktop/src/components/redis/RedisKeyBrowser.vue`
- `dbx/apps/desktop/src/components/redis/RedisValueViewer.vue`
- `dbx/apps/desktop/src/lib/redisCommandSafety.ts`
- `dbx/apps/desktop/src/lib/redisCommandSession.ts`
- `dbx/apps/desktop/src/lib/redisCommandTable.ts`
- `dbx/apps/desktop/src/lib/redisKeyPattern.ts`

Mongo 参考：

- `dbx/apps/desktop/src/components/document/DocumentBrowser.vue`
- `dbx/apps/desktop/src/lib/mongoConnectionOptions.ts`
- `dbx/packages/node-core/tests/mongo-query.test.ts`

Aeroric 已有 Redis/Mongo 浏览器和 hook，继续补齐：

- Redis key tree、key pattern、scan 分页。
- Redis value 的 string/hash/list/set/zset/stream 展示。
- Redis 命令执行安全策略。
- Redis pub/sub 如 dbx 有入口则补入口。
- Mongo database/collection/document sidebar。
- Mongo document/table 两种查看方式。
- Mongo filter/sort JSON。
- Mongo structured filter。
- Mongo insert/delete 确认。
- Mongo document preview 分页和选中态。

### 第 9 步：样式 exact parity

必须对齐 dbx 的视觉语言，而不是“差不多”：

- 字体：优先使用 Aeroric 现有 font 变量，但密度和字号参考 dbx Geist 风格。
- 页面背景、sidebar 背景、panel 背景、border、muted、accent、destructive 需要映射到 Aeroric 主题变量。
- 数据库页不要出现营销式大卡片。
- 树、菜单、DataGrid 采用紧凑工具型布局。
- 按钮圆角不要过大。
- toolbar 高度和按钮高度固定，避免 hover 或文字变化造成布局跳动。
- 所有固定格式元素使用稳定尺寸。
- 暗色主题必须可读。

强制截图验收：

1. dbx 数据库页：连接弹窗、侧边栏树、右键菜单、DataGrid toolbar。
2. Aeroric 对应页面同尺寸截图。
3. 并排对比按钮高度、圆角、图标、间距、hover、active、disabled。
4. 发现明显不一致，先修按钮基元，再修局部。

### 第 10 步：i18n、测试和验证

每个新增文案都补：

- 中文。
- 英文。

测试优先更新：

- `src/test/database-view.test.tsx`
- `src/test/database-sidebar-tree.test.tsx`
- `src/test/database-api.test.ts`
- `src/test/database-redis-browser.test.tsx`
- `src/test/database-mongo-browser.test.tsx`
- `src/test/database-table-structure-panel.test.tsx`
- `src/test/database-advanced-tools.test.tsx`

每轮改动后至少运行：

```bash
pnpm test -- src/test/database-sidebar-tree.test.tsx src/test/database-view.test.tsx src/test/database-api.test.ts src/test/database-redis-browser.test.tsx src/test/database-mongo-browser.test.tsx
pnpm lint
git diff --check
pnpm build
```

如果改了 Rust 或 Tauri command，额外运行：

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

如果改了 UI 样式，必须启动开发服务器并截图检查：

```bash
pnpm dev
```

然后使用浏览器/Playwright 检查：

- 1280x800 桌面视口。
- 1440x900 桌面视口。
- 窄宽度下按钮文字不溢出。
- 亮色/暗色主题。

## 推荐实施顺序

1. 按钮基元和数据库按钮全量替换。
2. 连接弹窗样式和 footer 行为。
3. 侧边栏树视觉密度和搜索/定位/节点状态。
4. 右键菜单矩阵缺口。
5. DataGrid 工具栏、列显示、过滤、SQL preview、导入导出细节。
6. Redis/Mongo 剩余菜单和查看方式。
7. 真正多 tab 工作区或等价工作区模型。
8. ER、搜索、用户管理、结构对比、数据对比、传输等深层入口。
9. 截图对比和测试补齐。

## 审查清单

实现完成后逐项自查：

- [ ] 没有回滚用户已有 dirty worktree。
- [ ] 没有重复创建平行数据库 API/状态/组件。
- [ ] 所有数据库按钮都使用统一 dbx 风格基元。
- [ ] icon-only 按钮都有 `title` 或 `aria-label`。
- [ ] disabled/hover/active/focus-visible 状态完整。
- [ ] 连接弹窗流程与 dbx 一致。
- [ ] 右键菜单项目、顺序、禁用态、危险项与 dbx 一致。
- [ ] 只读连接禁用写操作。
- [ ] 危险操作有 SQL 或操作摘要确认。
- [ ] 侧边栏树支持懒加载、局部刷新、多选、快捷键、搜索。
- [ ] DataGrid 的分页、过滤、排序、编辑、导出、预览与 dbx 一致。
- [ ] Redis/Mongo 的查看方式、菜单和删除确认与 dbx 一致。
- [ ] 新增文案中英文齐全。
- [ ] 测试通过。
- [ ] lint 通过。
- [ ] build 通过。
- [ ] Rust 改动时 cargo check 通过。
- [ ] 截图对比通过，尤其是按钮样式。

## 不要做的事

- 不要把 dbx 的 Vue 组件复制进 Aeroric。
- 不要引入 Pinia 或 shadcn-vue。
- 不要在前端拼复杂危险 SQL。
- 不要因为架构不同就改变用户可见行为。
- 不要只改颜色而不统一按钮状态。
- 不要把右键菜单做成占位项。
- 不要省略 disabled 状态。
- 不要把所有新逻辑继续堆进一个巨大函数，能局部抽小组件就抽小组件。
- 不要把旧验证结果当成本轮验证结果。
