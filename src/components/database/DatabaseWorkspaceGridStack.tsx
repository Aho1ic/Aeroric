/**
 * 三元链回落到的那一叠:查询模式下的 SQL 输入框 + 运行键、网格工具栏、字段过滤条、数据网格本体、
 * 页脚。它就是 `<main>` 里「没有任何特殊面板要显示」时的默认内容。
 *
 * 从 `DatabaseView.tsx` 抽出:原文里这一叠是那条 `workspaceMode === X ? ... : (` 链最后那个
 * `: (` 分支里的 `<>...</>`,现在通过 `DatabaseWorkspacePanels` 的 `fallback` 传回去,
 * 位置与挂载时机完全不变。纯展示 —— 不持有状态、不含任何 hook。
 *
 * 逐字保留的几处:
 * - SQL 输入框那一块只在 `workspaceMode === "query"` 时出现;`DataGridView` 的 `variant`
 *   同样按这个判断在 `"query"` / `"table"` 之间切,两处的条件必须一致。
 * - 运行键的 `disabled` 是 `!activeSqlCapable || loading`,不是只看其中一个。
 * - 过滤条要 `queryResult && activeDbxConnection && activeDbxObject` 三个同时成立;
 *   页脚只要 `queryResult`。两个条件不能互抄。
 * - `onImportData` 里那句提前 return(缺连接或缺对象就什么都不做)留在回调内部,
 *   不提到外面变成按钮的 `disabled`。
 * - `onPromptPageSize` 的合法区间是 `1..10000` 闭区间,取消或不合法都返回 `null`。
 *
 * `grid` 这个 prop 在解构时改回原文里的 `dbxGrid`,好让下面整段 JSX 一字不动;对外的名字
 * 跟 `DatabaseGridToolbar` / `DataGridView` / `DataGridFooter` 那几个同层组件保持一致。
 * `dbxGridPageSize` 不另开 prop,直接从 `grid.state` 上取 —— 那就是原文里它的来源。
 */

import type { DragEvent, KeyboardEventHandler, ReactNode } from "react";
import { Play } from "lucide-react";

import { useI18n } from "../../i18n";
import { prompt } from "../../lib/appDialog";
import s from "../../styles";
import type {
  AeroricDbConnectionConfig,
  DbConnectionConfig,
  DbExecuteResult,
  DbObject,
  DbQueryResult,
  DbxObjectInfo,
} from "../../types";
import { Button as DbxButton } from "../ui/Button";
import { DatabaseGridToolbar } from "./DatabaseGridToolbar";
import type { DatabaseRow, DbxGridContextMenuState, TableExportFormat } from "./databaseGridState";
import type { DbWorkspaceMode } from "./databaseViewModel";
import { DataGridFilterBar, DataGridFooter } from "./DataGridChrome";
import { DataGridView } from "./DataGridView";
import type { DbxDataGridController } from "./useDbxDataGrid";
import type { DbxSqlPreviewDialogState } from "./useDbxSqlPreviewDialog";
import type { TableImportDialogState } from "./useTableImportDialog";

export interface DatabaseWorkspaceGridStackProps {
  workspaceMode: DbWorkspaceMode;
  grid: DbxDataGridController;
  loading: boolean;
  error: string | null;
  page: number;
  totalPages: number | null;
  sql: string;
  setSql: (sql: string) => void;
  activeSqlCapable: boolean;
  runSql: () => Promise<void>;
  handleSqlDragOver: (event: DragEvent<HTMLTextAreaElement>) => void;
  handleSqlDrop: (event: DragEvent<HTMLTextAreaElement>) => void;
  queryResult: DbQueryResult | null;
  sqlResult: DbExecuteResult | null;
  tableColumns: string[];
  showRowIdColumn: boolean;
  canInsertActiveTable: boolean;
  hideDatabaseWorkspaceTopbar: boolean;
  activeConnection: DbConnectionConfig | null;
  activeObject: DbObject | null;
  activeDbxConnection: AeroricDbConnectionConfig | null;
  activeDbxDatabase: string | null;
  activeDbxObject: DbxObjectInfo | null;
  dbxSqlPreview: DbxSqlPreviewDialogState;
  tableImport: TableImportDialogState;
  tableFooterRowCountText: string;
  tableFooterSqlText: string;
  loadActiveObjectPage: (targetPage: number) => void;
  insertRow: () => void;
  savePendingGridChanges: () => Promise<void>;
  openActiveTableProperties: () => void;
  exportActiveDbxGrid: (format?: TableExportFormat) => Promise<void>;
  copySelectedDbxRows: () => Promise<void>;
  deleteSelectedDbxRows: () => Promise<void>;
  resetActiveDbxGrid: () => Promise<void>;
  reloadActiveDbxGrid: (whereInput?: string, orderBy?: string) => Promise<void>;
  changeDbxGridPageSize: (nextPageSize: number) => Promise<void>;
  handleDbxGridKeyDown: KeyboardEventHandler<HTMLDivElement>;
  toggleDbxGridColumnSort: (column: string) => Promise<void>;
  applyDbxGridColumnFuzzyFilter: (column: string, value: string) => Promise<void>;
  setContextMenu: (menu: DbxGridContextMenuState) => void;
  updateCell: (row: DatabaseRow, column: string, value: string, original: string) => Promise<void>;
}

export function DatabaseWorkspaceGridStack({
  workspaceMode,
  grid: dbxGrid,
  loading,
  error,
  page,
  totalPages,
  sql,
  setSql,
  activeSqlCapable,
  runSql,
  handleSqlDragOver,
  handleSqlDrop,
  queryResult,
  sqlResult,
  tableColumns,
  showRowIdColumn,
  canInsertActiveTable,
  hideDatabaseWorkspaceTopbar,
  activeConnection,
  activeObject,
  activeDbxConnection,
  activeDbxDatabase,
  activeDbxObject,
  dbxSqlPreview,
  tableImport,
  tableFooterRowCountText,
  tableFooterSqlText,
  loadActiveObjectPage,
  insertRow,
  savePendingGridChanges,
  openActiveTableProperties,
  exportActiveDbxGrid,
  copySelectedDbxRows,
  deleteSelectedDbxRows,
  resetActiveDbxGrid,
  reloadActiveDbxGrid,
  changeDbxGridPageSize,
  handleDbxGridKeyDown,
  toggleDbxGridColumnSort,
  applyDbxGridColumnFuzzyFilter,
  setContextMenu,
  updateCell,
}: DatabaseWorkspaceGridStackProps): ReactNode {
  const { t } = useI18n();
  const { dbxGridPageSize } = dbxGrid.state;

  return (
    <>
      {workspaceMode === "query" && (
        <div style={s.databaseSqlPanel}>
          <textarea
            style={s.databaseSqlInput}
            value={sql}
            onChange={(event) => setSql(event.target.value)}
            onDragOver={handleSqlDragOver}
            onDrop={handleSqlDrop}
            spellCheck={false}
            placeholder={t("database.sqlPlaceholder")}
          />
          <DbxButton
            variant="default"
            size="sm"
            icon={Play}
            onClick={runSql}
            disabled={!activeSqlCapable || loading}
            style={{ width: 86, height: "auto" }}
          >
            {t("database.run")}
          </DbxButton>
        </div>
      )}

      <DatabaseGridToolbar
        grid={dbxGrid}
        queryResult={queryResult}
        activeDbxConnection={activeDbxConnection}
        activeConnectionReadOnly={Boolean(activeConnection?.readOnly)}
        hasActiveObject={Boolean(activeObject)}
        hasActiveDbxObject={Boolean(activeDbxObject)}
        canInsertRows={canInsertActiveTable}
        loading={loading}
        tableColumns={tableColumns}
        showInlineError={hideDatabaseWorkspaceTopbar}
        error={error}
        statusMessage={sqlResult?.message}
        sqlPreview={dbxSqlPreview}
        onRefresh={() => loadActiveObjectPage(page)}
        onInsertRow={insertRow}
        onSaveChanges={() => void savePendingGridChanges()}
        onOpenTableProperties={openActiveTableProperties}
        onImportData={() => {
          if (!activeDbxConnection || !activeDbxObject) return;
          void tableImport.open(activeDbxConnection, activeDbxDatabase, activeDbxObject);
        }}
        onExportData={(format) => void exportActiveDbxGrid(format)}
        onCopySelectedRows={() => void copySelectedDbxRows()}
        onDeleteSelectedRows={() => void deleteSelectedDbxRows()}
        onResetGrid={() => void resetActiveDbxGrid()}
      />

      {queryResult && activeDbxConnection && activeDbxObject && (
        <DataGridFilterBar grid={dbxGrid} onReload={() => void reloadActiveDbxGrid()} />
      )}

      <DataGridView
        variant={workspaceMode === "query" ? "query" : "table"}
        queryResult={queryResult}
        activeDbxConnection={activeDbxConnection}
        activeConnectionReadOnly={Boolean(activeConnection?.readOnly)}
        activeObject={activeObject}
        tableColumns={tableColumns}
        showRowIdColumn={showRowIdColumn}
        canInsertRows={canInsertActiveTable}
        loading={loading}
        grid={dbxGrid}
        onKeyDown={handleDbxGridKeyDown}
        onSortColumn={toggleDbxGridColumnSort}
        onFilterColumn={applyDbxGridColumnFuzzyFilter}
        onOpenContextMenu={setContextMenu}
        onUpdateCell={updateCell}
      />
      {queryResult && (
        <DataGridFooter
          grid={dbxGrid}
          rowCountText={tableFooterRowCountText}
          sqlText={tableFooterSqlText}
          page={page}
          totalPages={totalPages}
          hasActiveObject={Boolean(activeObject)}
          showPageSize={Boolean(activeDbxConnection)}
          loading={loading}
          onGoToPage={loadActiveObjectPage}
          onChangePageSize={(size) => void changeDbxGridPageSize(size)}
          onPromptPageSize={async () => {
            const custom = await prompt(t("database.gridCustomPageSize"), {
              title: t("database.gridRowsPerPage"),
              defaultValue: String(dbxGridPageSize),
            });
            if (!custom) return null;
            const num = Number(custom);
            return Number.isFinite(num) && num >= 1 && num <= 10000 ? num : null;
          }}
        />
      )}
    </>
  );
}
