/**
 * 「导入文件到表」对话框的展示层。状态与动作全部来自 `useTableImportDialog`,
 * 这里不持有任何 state —— 从 `DatabaseView.tsx` 抽出时保持 DOM 结构、aria-label
 * 与文案 key 逐字不变,以免影响已有的 `database-view-*` 用例。
 */

import { useI18n } from "../../i18n";
import s from "../../styles";
import { Button as DbxButton, DialogFooterButton as DbxDialogFooterButton } from "../ui/Button";
import type { TableImportMode } from "../../types";
import type { TableImportDialogState } from "./useTableImportDialog";

export function TableImportDialog({ state }: { state: TableImportDialogState }) {
  const { t } = useI18n();
  const { target, connection, preview } = state;
  // 连接解析不出来时不渲染:此时 submit 也会拒绝执行,渲染出来只会给一个点不动的框。
  if (!target || !connection) return null;

  return (
    <div
      style={s.databaseDialogOverlay}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) state.close();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={t("database.tableImport")}
        style={{ ...s.databaseDialog, width: 720 }}
      >
        <div style={s.databaseDialogHeader}>{t("database.tableImport")}</div>
        <div
          style={{
            ...s.databaseDialogBody,
            maxHeight: "calc(100vh - 180px)",
            overflowY: "auto",
          }}
        >
          <div style={s.databaseDialogHint}>
            {t("database.tableImportHint", {
              table: target.object.schema
                ? `${target.object.schema}.${target.object.name}`
                : target.object.name,
            })}
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <DbxButton
              variant="outline"
              size="sm"
              onClick={() => {
                void state.chooseFile();
              }}
              disabled={state.loading}
            >
              {t("database.tableImportChooseFile")}
            </DbxButton>
            <span
              style={{
                minWidth: 0,
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
                fontSize: 12,
                color: "var(--text-muted)",
              }}
            >
              {preview ? preview.fileName || preview.filePath : t("database.tableImportNoFile")}
            </span>
          </div>
          <div style={s.databaseDialogFormGrid}>
            <label style={s.databaseDialogField}>
              <span style={s.databaseDialogLabel}>{t("database.tableImportMode")}</span>
              <select
                aria-label={t("database.tableImportMode")}
                style={s.databaseDialogInput}
                value={state.mode}
                onChange={(event) => state.setMode(event.target.value as TableImportMode)}
              >
                <option value="append">{t("database.tableImportAppend")}</option>
                <option value="truncate">{t("database.tableImportTruncate")}</option>
              </select>
            </label>
            <label style={s.databaseDialogField}>
              <span style={s.databaseDialogLabel}>{t("database.tableImportBatchSize")}</span>
              <input
                aria-label={t("database.tableImportBatchSize")}
                style={s.databaseDialogInput}
                value={state.batchSize}
                onChange={(event) => state.setBatchSize(event.target.value)}
                inputMode="numeric"
              />
            </label>
          </div>
          {state.error && <div style={{ color: "var(--danger)", fontSize: 12 }}>{state.error}</div>}
          {state.loading ? (
            <div style={s.databaseEmptyCompact}>{t("common.loading")}</div>
          ) : preview ? (
            <>
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  gap: 10,
                }}
              >
                <div style={s.databaseDialogLabel}>
                  {t("database.tableImportMappedColumns", {
                    mapped: state.mappedColumns.length,
                    total: preview.columns.length,
                  })}
                </div>
                <button
                  type="button"
                  style={{
                    border: "none",
                    background: "transparent",
                    color: "var(--text-secondary)",
                    fontSize: 11.5,
                    cursor: "pointer",
                    padding: "0 2px",
                  }}
                  onClick={() => state.autoMap()}
                >
                  {t("database.tableImportAutoMap")}
                </button>
              </div>
              <div
                style={{
                  display: "grid",
                  gridTemplateColumns: "repeat(2, minmax(0, 1fr))",
                  gap: 8,
                }}
              >
                {preview.columns.map((sourceColumn) => (
                  <label key={sourceColumn} style={s.databaseDialogField}>
                    <span style={s.databaseDialogLabel}>{sourceColumn}</span>
                    <select
                      aria-label={t("database.tableImportTargetColumn", {
                        column: sourceColumn,
                      })}
                      style={s.databaseDialogInput}
                      value={state.mappings[sourceColumn] ?? ""}
                      onChange={(event) => state.setMapping(sourceColumn, event.target.value)}
                    >
                      <option value="">{t("database.tableImportSkipColumn")}</option>
                      {state.targetColumnNames.map((targetColumn) => (
                        <option key={targetColumn} value={targetColumn}>
                          {targetColumn}
                        </option>
                      ))}
                    </select>
                  </label>
                ))}
              </div>
              <div style={s.databaseDialogLabel}>
                {t("database.tableImportPreviewRows", { rows: preview.totalRows })}
              </div>
              <div
                style={{
                  overflow: "auto",
                  border: "1px solid var(--border-dim)",
                  borderRadius: 8,
                  background: "var(--bg-subtle)",
                }}
              >
                <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12 }}>
                  <thead>
                    <tr>
                      {preview.columns.map((column) => (
                        <th
                          key={column}
                          style={{
                            padding: "6px 8px",
                            textAlign: "left",
                            borderBottom: "1px solid var(--border-dim)",
                            color: "var(--text-muted)",
                          }}
                        >
                          {column}
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {preview.rows.map((row, rowIndex) => (
                      <tr key={rowIndex}>
                        {preview.columns.map((column, columnIndex) => (
                          <td
                            key={column}
                            style={{
                              padding: "6px 8px",
                              borderBottom: "1px solid var(--border-dim)",
                              color: "var(--text-secondary)",
                            }}
                          >
                            {row[columnIndex] === null || row[columnIndex] === undefined
                              ? "NULL"
                              : String(row[columnIndex])}
                          </td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </>
          ) : (
            <div style={s.databaseEmptyCompact}>{t("database.tableImportSelectFileHint")}</div>
          )}
        </div>
        <div style={s.databaseDialogFooter}>
          <DbxDialogFooterButton type="button" onClick={state.close} disabled={state.loading}>
            {t("common.cancel")}
          </DbxDialogFooterButton>
          <DbxDialogFooterButton
            type="button"
            variant="default"
            onClick={() => {
              void state.submit();
            }}
            disabled={state.loading || Boolean(state.error) || !state.canRun}
          >
            {t("database.tableImport")}
          </DbxDialogFooterButton>
        </div>
      </div>
    </div>
  );
}
