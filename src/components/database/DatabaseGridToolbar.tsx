/**
 * 网格上方那条工具栏:刷新 / 插入 / 保存改动 / 数据工具 / 字段过滤 / 复制删除选中行。
 *
 * 从 `DatabaseView.tsx` 抽出时保持按钮顺序、`role="menu"` 结构与文案 key 逐字不变,以免影响
 * 已有的 `database-view-*` 用例。两个下拉(数据工具、字段过滤)的开合状态本来就住在
 * `useDbxDataGrid` 里,所以这里跟 `DataGridView` 一样只接一个 `grid` 控制器,不另起 hook。
 */

import {
  CheckSquare,
  ChevronLeft,
  ChevronRight,
  Copy,
  Eye,
  FileCode,
  Plus,
  RefreshCcw,
  SlidersHorizontal,
  Trash2,
  Wrench,
} from "lucide-react";

import { useI18n } from "../../i18n";
import s from "../../styles";
import type { AeroricDbConnectionConfig, DbQueryResult } from "../../types";
import type { TableExportFormat } from "./databaseGridState";
import { Button as DbxButton } from "../ui/Button";
import { DATA_TOOL_EXPORT_FORMATS } from "./databaseViewModel";
import type { DbxDataGridController } from "./useDbxDataGrid";
import type { DbxSqlPreviewDialogState } from "./useDbxSqlPreviewDialog";

export interface DatabaseGridToolbarProps {
  grid: DbxDataGridController;
  queryResult: DbQueryResult | null;
  /** 当前 dbx 连接;为空说明走的是 legacy 连接那条路,大半按钮都不出现。 */
  activeDbxConnection: AeroricDbConnectionConfig | null;
  /** legacy 或 dbx 连接的只读标记,只用来决定右侧那行状态文字。 */
  activeConnectionReadOnly: boolean;
  /** 有没有选中对象 —— 没有就连「刷新」都点不动。 */
  hasActiveObject: boolean;
  hasActiveDbxObject: boolean;
  canInsertRows: boolean;
  loading: boolean;
  tableColumns: string[];
  /** 顶栏被隐藏时错误只能挤到这里显示,这个判断留在 `DatabaseView` 里算。 */
  showInlineError: boolean;
  error: string | null;
  /** 不忙也不只读时显示的收尾信息,通常是上一条 SQL 的执行结果。 */
  statusMessage: string | undefined;
  sqlPreview: DbxSqlPreviewDialogState;
  onRefresh: () => void;
  onInsertRow: () => void;
  onSaveChanges: () => void;
  onOpenTableProperties: () => void;
  onImportData: () => void;
  onExportData: (format: TableExportFormat) => void;
  onCopySelectedRows: () => void;
  onDeleteSelectedRows: () => void;
  onResetGrid: () => void;
}

export function DatabaseGridToolbar({
  grid,
  queryResult,
  activeDbxConnection,
  activeConnectionReadOnly,
  hasActiveObject,
  hasActiveDbxObject,
  canInsertRows,
  loading,
  tableColumns,
  showInlineError,
  error,
  statusMessage,
  sqlPreview,
  onRefresh,
  onInsertRow,
  onSaveChanges,
  onOpenTableProperties,
  onImportData,
  onExportData,
  onCopySelectedRows,
  onDeleteSelectedRows,
  onResetGrid,
}: DatabaseGridToolbarProps) {
  const { t } = useI18n();
  const {
    dbxDataToolsOpen,
    setDbxDataToolsOpen,
    dbxDataToolsMode,
    setDbxDataToolsMode,
    dbxFieldFilterOpen,
    setDbxFieldFilterOpen,
    dbxGridColumnSearch,
    setDbxGridColumnSearch,
    dbxGridHiddenColumns,
    setDbxGridExportFormat,
    dbxGridSelectedRows,
  } = grid.state;
  const { visibleTableColumns, filteredDbxGridColumnOptions, dbxPendingChangeCount } = grid.derived;
  const { toggleDbxGridColumnVisibility, invertDbxGridColumnVisibility, showAllDbxGridColumns } =
    grid.actions;

  return (
    <div style={s.databaseToolbar}>
      {showInlineError && error && (
        <div style={s.databaseError} title={error}>
          {error}
        </div>
      )}
      <DbxButton
        variant="outline"
        size="sm"
        icon={RefreshCcw}
        disabled={!hasActiveObject || loading}
        onClick={onRefresh}
      >
        {t("database.refresh")}
      </DbxButton>
      <DbxButton
        variant="outline"
        size="sm"
        icon={Plus}
        disabled={!canInsertRows || loading}
        onClick={onInsertRow}
      >
        {t("database.insert")}
      </DbxButton>
      <DbxButton
        variant="outline"
        size="sm"
        icon={CheckSquare}
        disabled={loading || dbxPendingChangeCount === 0}
        onClick={onSaveChanges}
      >
        {t("database.saveGridChanges")}
      </DbxButton>
      {sqlPreview.hasStatements && (
        <DbxButton variant="outline" size="sm" icon={FileCode} onClick={sqlPreview.show}>
          {t("database.previewSql")}
        </DbxButton>
      )}
      {queryResult && activeDbxConnection && hasActiveDbxObject && (
        <>
          <DbxButton
            variant="outline"
            size="sm"
            icon={SlidersHorizontal}
            disabled={loading}
            onClick={onOpenTableProperties}
          >
            {t("database.tableProperties")}
          </DbxButton>
          <div style={s.databaseToolbarMenuAnchor}>
            <DbxButton
              variant="outline"
              size="sm"
              icon={Wrench}
              disabled={loading}
              onClick={() => {
                setDbxDataToolsOpen((open) => !open);
                setDbxFieldFilterOpen(false);
                setDbxDataToolsMode("root");
              }}
            >
              {t("database.dataTools")}
            </DbxButton>
            {dbxDataToolsOpen && (
              <div role="menu" aria-label={t("database.dataTools")} style={s.databaseToolbarMenu}>
                {dbxDataToolsMode === "root" ? (
                  <>
                    <button
                      type="button"
                      role="menuitem"
                      style={s.databaseToolbarMenuButton}
                      disabled={!canInsertRows || loading}
                      onClick={() => {
                        setDbxDataToolsOpen(false);
                        onInsertRow();
                      }}
                    >
                      {t("database.generateData")}
                    </button>
                    <button
                      type="button"
                      role="menuitem"
                      style={s.databaseToolbarMenuButton}
                      disabled={activeDbxConnection.readOnly || loading}
                      onClick={() => {
                        setDbxDataToolsOpen(false);
                        onImportData();
                      }}
                    >
                      {t("database.importData")}
                    </button>
                    <button
                      type="button"
                      role="menuitem"
                      style={s.databaseToolbarMenuButton}
                      disabled={visibleTableColumns.length === 0 || loading}
                      onClick={() => setDbxDataToolsMode("export")}
                    >
                      {t("database.exportData")}
                      <ChevronRight size={14} aria-hidden="true" />
                    </button>
                  </>
                ) : (
                  <>
                    <button
                      type="button"
                      style={s.databaseToolbarMenuButton}
                      onClick={() => setDbxDataToolsMode("root")}
                    >
                      <ChevronLeft size={14} aria-hidden="true" />
                      {t("common.back")}
                    </button>
                    {DATA_TOOL_EXPORT_FORMATS.map((item) => (
                      <button
                        key={item.format}
                        type="button"
                        role="menuitem"
                        style={s.databaseToolbarMenuButton}
                        disabled={visibleTableColumns.length === 0 || loading}
                        onClick={() => {
                          setDbxGridExportFormat(item.format);
                          setDbxDataToolsOpen(false);
                          onExportData(item.format);
                        }}
                      >
                        {item.label}
                      </button>
                    ))}
                  </>
                )}
              </div>
            )}
          </div>
          <div style={s.databaseToolbarMenuAnchor}>
            <DbxButton
              variant="outline"
              size="sm"
              icon={Eye}
              disabled={tableColumns.length === 0}
              onClick={() => {
                setDbxFieldFilterOpen((open) => !open);
                setDbxDataToolsOpen(false);
              }}
            >
              {t("database.fieldFilter")}
            </DbxButton>
            {dbxFieldFilterOpen && (
              <div
                role="menu"
                aria-label={t("database.fieldFilter")}
                style={{ ...s.databaseToolbarMenu, ...s.databaseToolbarMenuWide }}
              >
                <input
                  style={s.databaseDialogInput}
                  value={dbxGridColumnSearch}
                  onChange={(event) => setDbxGridColumnSearch(event.target.value)}
                  placeholder={t("database.gridSearchColumns")}
                  aria-label={t("database.gridSearchColumns")}
                />
                <div style={s.databaseFieldFilterList}>
                  {filteredDbxGridColumnOptions.length > 0 ? (
                    filteredDbxGridColumnOptions.map((column) => {
                      const hidden = dbxGridHiddenColumns.has(column);
                      // 最后一列不许再藏 —— 全藏光的网格没法用。
                      const visibleCount = tableColumns.filter(
                        (item) => !dbxGridHiddenColumns.has(item),
                      ).length;
                      return (
                        <label key={column} style={s.databaseFieldFilterItem}>
                          <input
                            type="checkbox"
                            checked={!hidden}
                            disabled={!hidden && visibleCount <= 1}
                            onChange={() => toggleDbxGridColumnVisibility(column)}
                          />
                          <span
                            style={{
                              overflow: "hidden",
                              textOverflow: "ellipsis",
                              whiteSpace: "nowrap",
                            }}
                          >
                            {column}
                          </span>
                        </label>
                      );
                    })
                  ) : (
                    <span style={{ color: "var(--text-hint)", fontSize: 12 }}>
                      {t("database.gridNoSearchResults")}
                    </span>
                  )}
                </div>
                <div style={s.databaseFieldFilterFooter}>
                  <DbxButton
                    variant="outline"
                    size="xs"
                    disabled={tableColumns.length <= 1}
                    onClick={invertDbxGridColumnVisibility}
                  >
                    {t("database.gridInvertColumnVisibility")}
                  </DbxButton>
                  <DbxButton
                    variant="outline"
                    size="xs"
                    disabled={dbxGridHiddenColumns.size === 0}
                    onClick={showAllDbxGridColumns}
                  >
                    {t("database.gridShowAllColumns")}
                  </DbxButton>
                </div>
              </div>
            )}
          </div>
        </>
      )}
      {queryResult && activeDbxConnection && (
        <>
          <DbxButton
            variant="outline"
            size="sm"
            icon={Copy}
            disabled={dbxGridSelectedRows.size === 0 || loading}
            onClick={onCopySelectedRows}
          >
            {t("database.copySelectedRowsCount", { count: dbxGridSelectedRows.size })}
          </DbxButton>
          <DbxButton
            variant="destructive"
            size="sm"
            icon={Trash2}
            disabled={
              !queryResult.editable ||
              activeDbxConnection.readOnly ||
              dbxGridSelectedRows.size === 0 ||
              loading
            }
            onClick={onDeleteSelectedRows}
          >
            {t("database.deleteSelectedRowsCount", { count: dbxGridSelectedRows.size })}
          </DbxButton>
          <DbxButton
            variant="outline"
            size="sm"
            icon={RefreshCcw}
            disabled={loading}
            onClick={onResetGrid}
          >
            {t("database.gridReset")}
          </DbxButton>
        </>
      )}
      <span style={{ color: "var(--text-hint)", fontSize: 12 }}>
        {loading
          ? t("database.loading")
          : activeConnectionReadOnly
            ? t("database.readOnlyBadge")
            : statusMessage}
      </span>
    </div>
  );
}
