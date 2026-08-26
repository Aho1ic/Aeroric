/**
 * 网格的上下两条「外框」:上面一条 where / order by / 本页搜索,下面一条行数 + SQL + 翻页。
 *
 * 从 `DatabaseView.tsx` 抽出时保持 DOM 结构、`role` / `aria-label` 与文案 key 逐字不变,以免
 * 影响已有的 `database-view-*` 用例。三个输入框与每页条数都住在 `useDbxDataGrid` 里,所以这里
 * 跟 `DataGridView` 一样只接一个 `grid` 控制器;两条各自由外层的条件表达式决定是否出现。
 */

import { ChevronLeft, ChevronRight } from "lucide-react";

import { useI18n } from "../../i18n";
import s from "../../styles";
import { Button as DbxButton } from "../ui/Button";
import { DBX_GRID_PAGE_SIZE_OPTIONS, type DbxDataGridController } from "./useDbxDataGrid";

const PAGE_SIZE_SELECT_STYLE = {
  ...s.databaseDialogInput,
  width: 82,
  height: 28,
  padding: "0 6px",
} as const;

const PAGE_SIZE_LABEL_STYLE = {
  display: "inline-flex",
  alignItems: "center",
  gap: 6,
  fontSize: 12,
  color: "var(--text-muted)",
  whiteSpace: "nowrap",
} as const;

export interface DataGridFilterBarProps {
  grid: DbxDataGridController;
  /** 回车即重跑当前对象 —— 带 shift / meta / ctrl / alt 的回车不算。 */
  onReload: () => void;
}

export function DataGridFilterBar({ grid, onReload }: DataGridFilterBarProps) {
  const { t } = useI18n();
  const {
    dbxGridWhereInput,
    setDbxGridWhereInput,
    dbxGridOrderByInput,
    setDbxGridOrderByInput,
    dbxGridSearch,
    setDbxGridSearch,
  } = grid.state;

  return (
    <div
      role="group"
      aria-label="Table filters"
      style={s.databaseGridFilterBar}
      onKeyDown={(event) => {
        if (
          event.key === "Enter" &&
          !event.shiftKey &&
          !event.metaKey &&
          !event.ctrlKey &&
          !event.altKey
        ) {
          event.preventDefault();
          onReload();
        }
      }}
    >
      <label style={{ ...s.databaseDialogField, minWidth: 220, flex: "1 1 240px" }}>
        <span style={s.databaseDialogLabel}>{t("database.gridWhere")}</span>
        <input
          style={s.databaseDialogInput}
          value={dbxGridWhereInput}
          onChange={(event) => setDbxGridWhereInput(event.target.value)}
          placeholder={t("database.gridWherePlaceholder")}
          aria-label={t("database.gridWhere")}
        />
      </label>
      <label style={{ ...s.databaseDialogField, minWidth: 180, flex: "1 1 220px" }}>
        <span style={s.databaseDialogLabel}>{t("database.gridOrderBy")}</span>
        <input
          style={s.databaseDialogInput}
          value={dbxGridOrderByInput}
          onChange={(event) => setDbxGridOrderByInput(event.target.value)}
          placeholder={t("database.gridOrderByPlaceholder")}
          aria-label={t("database.gridOrderBy")}
        />
      </label>
      <label style={{ ...s.databaseDialogField, minWidth: 180, flex: "1 1 220px" }}>
        <span style={s.databaseDialogLabel}>{t("database.gridSearchCurrentPage")}</span>
        <input
          style={s.databaseDialogInput}
          value={dbxGridSearch}
          onChange={(event) => setDbxGridSearch(event.target.value)}
          placeholder={t("database.gridSearchPlaceholder")}
          aria-label={t("database.gridSearchCurrentPage")}
        />
      </label>
    </div>
  );
}

export interface DataGridFooterProps {
  grid: DbxDataGridController;
  /** 页脚左边两块只读文字,由 `DatabaseView` 算好传进来。 */
  rowCountText: string;
  sqlText: string;
  page: number;
  /** 总页数未知时显示 "?",上一版就是这样。 */
  totalPages: number | null;
  /** 没选中对象时两个翻页按钮都点不动。 */
  hasActiveObject: boolean;
  /** 只有 dbx 连接才有「每页条数」这个选择器。 */
  showPageSize: boolean;
  loading: boolean;
  onGoToPage: (page: number) => void;
  onChangePageSize: (size: number) => void;
  /** 选「自定义」时弹应用内输入框问一个数字;取消或不合法就返回 null。 */
  onPromptPageSize: () => Promise<number | null>;
}

export function DataGridFooter({
  grid,
  rowCountText,
  sqlText,
  page,
  totalPages,
  hasActiveObject,
  showPageSize,
  loading,
  onGoToPage,
  onChangePageSize,
  onPromptPageSize,
}: DataGridFooterProps) {
  const { t } = useI18n();
  const { dbxGridPageSize } = grid.state;
  const presetPageSize = (DBX_GRID_PAGE_SIZE_OPTIONS as readonly number[]).includes(
    dbxGridPageSize,
  );

  return (
    <div style={s.databaseGridFooter}>
      <div role="status" aria-label={t("database.tableRowCount")} style={s.databaseGridFooterRows}>
        {rowCountText}
      </div>
      <div
        role="status"
        aria-label={t("database.currentSql")}
        style={s.databaseGridFooterSql}
        title={sqlText}
      >
        {sqlText || "-"}
      </div>
      <div
        role="group"
        aria-label={t("database.tablePagination")}
        style={s.databaseGridFooterPager}
      >
        <DbxButton
          variant="ghost"
          size="icon-sm"
          icon={ChevronLeft}
          disabled={!hasActiveObject || page <= 1 || loading}
          onClick={() => onGoToPage(Math.max(1, page - 1))}
        />
        <span style={{ fontSize: 12, color: "var(--text-muted)" }}>
          {t("database.page", { page, total: totalPages ?? "?" })}
        </span>
        {showPageSize && (
          <label style={PAGE_SIZE_LABEL_STYLE}>
            <span>{t("database.gridRowsPerPage")}</span>
            <select
              style={PAGE_SIZE_SELECT_STYLE}
              value={presetPageSize ? dbxGridPageSize : "custom"}
              disabled={loading}
              aria-label={t("database.gridRowsPerPage")}
              onChange={(event) => {
                const val = event.currentTarget.value;
                if (val === "custom") {
                  // 应用内输入框是异步的,而 onChange 是同步回调 —— 包一层 void async。
                  void (async () => {
                    const size = await onPromptPageSize();
                    if (size != null) onChangePageSize(size);
                  })();
                } else {
                  onChangePageSize(Number(val));
                }
              }}
            >
              {DBX_GRID_PAGE_SIZE_OPTIONS.map((option) => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))}
              {!presetPageSize && <option value="custom">{dbxGridPageSize}</option>}
              <option value="custom">{t("database.gridCustomPageSize")}</option>
            </select>
          </label>
        )}
        <DbxButton
          variant="ghost"
          size="icon-sm"
          icon={ChevronRight}
          disabled={!hasActiveObject || loading || (totalPages != null && page >= totalPages)}
          onClick={() => onGoToPage(page + 1)}
        />
      </div>
    </div>
  );
}
