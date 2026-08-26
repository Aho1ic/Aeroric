/**
 * 单元格详情面板的展示层 —— 网格下方那块「列信息 + 值 + 置空/还原」的浮层。
 *
 * 从 `DatabaseView.tsx` 抽出时保持 DOM 结构与文案 key 逐字不变,以免影响已有的
 * `database-view-*` 用例。内容住在 `useDbxDataGrid` 的 `dbxCellDetail` 里,这里只读。
 *
 * 值输入框用的是 `defaultValue` 而不是 `value` —— 面板不卸载时切到另一个单元格并
 * 不会刷新框里的文字,这是原有行为,所以这里也不能加 `key` 去强制重挂。
 */

import { useI18n } from "../../i18n";
import { valueToText } from "../../lib/databaseUtils";
import s from "../../styles";
import type { DbQueryResult } from "../../types";
import { Button as DbxButton } from "../ui/Button";
import type { DatabaseRow } from "./databaseGridState";
import { dbxDataTypeStyle } from "./databaseViewModel";
import type { DbxDataGridController } from "./useDbxDataGrid";

const CLOSE_BUTTON_STYLE = {
  background: "transparent",
  border: "none",
  cursor: "pointer",
  color: "var(--text-hint)",
  fontSize: 18,
  padding: "0 4px",
} as const;

const VALUE_TEXTAREA_STYLE = {
  width: "100%",
  minHeight: 60,
  marginTop: 4,
  fontFamily: "var(--font-mono)",
  fontSize: 12,
  padding: 6,
  background: "var(--bg-input)",
  color: "var(--text-primary)",
  border: "1px solid var(--border-dim)",
  borderRadius: 4,
  resize: "vertical",
} as const;

export interface DbxCellDetailPanelProps {
  grid: DbxDataGridController;
  /** 「置空」和「还原」都要按当前结果集定位到那一行。 */
  queryResult: DbQueryResult | null;
  /** 借用网格既有的单元格编辑链路,连带走它的只读 / 可编辑判断。 */
  onUpdateCell: (
    row: DatabaseRow,
    column: string,
    value: string,
    original: string,
  ) => void | Promise<void>;
}

export function DbxCellDetailPanel({ grid, queryResult, onUpdateCell }: DbxCellDetailPanelProps) {
  const { t } = useI18n();
  const { dbxCellDetail, setDbxCellDetail } = grid.state;
  if (!dbxCellDetail) return null;

  /** 「置空」和「还原」共用同一段定位逻辑:越界或行已经不在结果里就什么都不做。 */
  const withDetailRow = (apply: (row: DatabaseRow) => void) => {
    const rows = queryResult?.rows;
    if (rows && dbxCellDetail.rowIndex >= 0 && dbxCellDetail.rowIndex < rows.length) {
      const row = rows[dbxCellDetail.rowIndex];
      if (row) apply(row);
    }
  };

  return (
    <div style={s.databaseCellDetailPanel}>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          marginBottom: 4,
        }}
      >
        <div style={{ fontWeight: 700, fontSize: 13 }}>{t("database.cellDetail")}</div>
        <button type="button" onClick={() => setDbxCellDetail(null)} style={CLOSE_BUTTON_STYLE}>
          ×
        </button>
      </div>
      <div style={s.databaseCellDetailGrid}>
        <div style={s.databaseCellDetailField}>
          <span style={s.databaseCellDetailLabel}>{t("database.columnName")}</span>
          <span style={{ ...s.databaseCellDetailValue, fontWeight: 700 }}>
            {dbxCellDetail.column}
          </span>
        </div>
        <div style={s.databaseCellDetailField}>
          <span style={s.databaseCellDetailLabel}>{t("database.rowNumber")}</span>
          <span style={s.databaseCellDetailValue}>{dbxCellDetail.rowIndex + 1}</span>
        </div>
        <div style={s.databaseCellDetailField}>
          <span style={s.databaseCellDetailLabel}>{t("database.columnType")}</span>
          <span
            style={{
              ...s.databaseCellDetailValue,
              ...dbxDataTypeStyle(dbxCellDetail.columnInfo?.data_type ?? ""),
            }}
          >
            {dbxCellDetail.columnInfo?.data_type ?? "-"}
          </span>
        </div>
        <div style={s.databaseCellDetailField}>
          <span style={s.databaseCellDetailLabel}>NULL</span>
          <span style={s.databaseCellDetailValue}>
            {dbxCellDetail.columnInfo?.is_nullable ? "YES" : "NO"}
          </span>
        </div>
        <div style={s.databaseCellDetailField}>
          <span style={s.databaseCellDetailLabel}>{t("database.length")}</span>
          <span style={s.databaseCellDetailValue}>
            {dbxCellDetail.columnInfo?.character_maximum_length ?? "-"}
          </span>
        </div>
        <div style={s.databaseCellDetailField}>
          <span style={s.databaseCellDetailLabel}>{t("database.columnComment")}</span>
          <span style={s.databaseCellDetailValue}>{dbxCellDetail.columnInfo?.comment ?? "-"}</span>
        </div>
      </div>
      <div style={{ marginTop: 4 }}>
        <span style={s.databaseCellDetailLabel}>{t("database.cellValue")}</span>
        <textarea
          style={VALUE_TEXTAREA_STYLE}
          defaultValue={valueToText(dbxCellDetail.value)}
          readOnly
        />
      </div>
      <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
        <DbxButton
          variant="outline"
          size="sm"
          onClick={() => {
            navigator.clipboard?.writeText(valueToText(dbxCellDetail.value));
          }}
        >
          {t("common.copy")}
        </DbxButton>
        <DbxButton
          variant="outline"
          size="sm"
          onClick={() =>
            withDetailRow((row) => {
              onUpdateCell(row, dbxCellDetail.column, "", valueToText(dbxCellDetail.value));
            })
          }
        >
          {t("database.setNull")}
        </DbxButton>
        <DbxButton
          variant="outline"
          size="sm"
          onClick={() =>
            withDetailRow((row) => {
              const original = valueToText(row.values[dbxCellDetail.columnIndex]);
              onUpdateCell(row, dbxCellDetail.column, original, original);
            })
          }
        >
          {t("database.restore")}
        </DbxButton>
      </div>
    </div>
  );
}
