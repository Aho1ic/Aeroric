/**
 * 「导出数据库」对话框的展示层。状态与动作全部来自 `useDatabaseExportDialog`,
 * 这里不持有任何 state —— 从 `DatabaseView.tsx` 抽出时保持 DOM 结构、aria-label
 * 与文案 key 逐字不变,以免影响已有的 `database-view-*` 用例。
 */

import { CheckSquare, Search, Square } from "lucide-react";

import { useI18n } from "../../i18n";
import s from "../../styles";
import { DialogFooterButton as DbxDialogFooterButton } from "../ui/Button";
import type { DatabaseExportDialogState } from "./useDatabaseExportDialog";

const LINK_BUTTON_STYLE = {
  border: "none",
  background: "transparent",
  color: "var(--text-secondary)",
  fontSize: 11.5,
  cursor: "pointer",
  padding: "0 2px",
} as const;

export function DatabaseExportDialog({ state }: { state: DatabaseExportDialogState }) {
  const { t } = useI18n();
  const { target, connection } = state;
  // 原来的守卫是 `databaseExportTarget && databaseExportConnection && (...)`。
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
        aria-label={t("database.databaseExport")}
        style={{ ...s.databaseDialog, width: 500 }}
      >
        <div style={s.databaseDialogHeader}>{t("database.databaseExport")}</div>
        <div style={s.databaseDialogBody}>
          <div style={s.databaseDialogHint}>
            {t("database.databaseExportHint", {
              database: target.schema ? `${target.database}.${target.schema}` : target.database,
            })}
          </div>
          <div style={s.databaseDialogFormGrid}>
            <label style={s.databaseSwitchRow}>
              <input
                type="checkbox"
                checked={state.includeStructure}
                onChange={(event) => state.setIncludeStructure(event.target.checked)}
              />
              <span>{t("database.exportIncludeStructure")}</span>
            </label>
            <label style={s.databaseSwitchRow}>
              <input
                type="checkbox"
                checked={state.includeData}
                onChange={(event) => state.setIncludeData(event.target.checked)}
              />
              <span>{t("database.exportIncludeData")}</span>
            </label>
            <label style={s.databaseSwitchRow}>
              <input
                type="checkbox"
                checked={state.includeObjects}
                onChange={(event) => state.setIncludeObjects(event.target.checked)}
              />
              <span>{t("database.exportIncludeObjects")}</span>
            </label>
            <label style={s.databaseSwitchRow}>
              <input
                type="checkbox"
                checked={state.dropTableIfExists}
                onChange={(event) => state.setDropTableIfExists(event.target.checked)}
              />
              <span>{t("database.exportDropTableIfExists")}</span>
            </label>
          </div>
          <label style={s.databaseSearchBox}>
            <Search size={13} />
            <input
              aria-label={t("database.exportSearchTables")}
              style={s.databaseSearchInput}
              value={state.search}
              onChange={(event) => state.setSearch(event.target.value)}
              placeholder={t("database.exportSearchTables")}
              disabled={state.loading || Boolean(state.error)}
            />
          </label>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              gap: 10,
              fontSize: 11.5,
              color: "var(--text-muted)",
            }}
          >
            <span>
              {t("database.exportSelectedTables", {
                selected: state.selection.size,
                total: state.tables.length,
              })}
            </span>
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <button
                type="button"
                style={LINK_BUTTON_STYLE}
                onClick={() => state.setSelection(() => new Set(state.filteredTables))}
                disabled={state.loading}
              >
                {t("database.visibleDatabasesSelectAll")}
              </button>
              <button
                type="button"
                style={LINK_BUTTON_STYLE}
                onClick={() => {
                  // 只清掉当前过滤结果里的表,搜索词之外的勾选保持不动。
                  const removing = new Set(state.filteredTables);
                  state.setSelection(
                    (current) => new Set([...current].filter((table) => !removing.has(table))),
                  );
                }}
                disabled={state.loading}
              >
                {t("database.visibleDatabasesClear")}
              </button>
            </div>
          </div>
          {!state.loading && !state.error && state.selection.size === 0 && (
            <div style={{ color: "var(--danger)", fontSize: 12 }}>
              {t("database.exportEmptySelection")}
            </div>
          )}
          {!state.loading &&
            !state.error &&
            !state.includeStructure &&
            !state.includeData &&
            !state.includeObjects && (
              <div style={{ color: "var(--danger)", fontSize: 12 }}>
                {t("database.exportEmptyOptions")}
              </div>
            )}
          <div
            style={{
              height: 240,
              overflowY: "auto",
              border: "1px solid var(--border-dim)",
              borderRadius: 8,
              background: "var(--bg-subtle)",
              padding: 4,
            }}
          >
            {state.loading ? (
              <div style={s.databaseEmptyCompact}>{t("common.loading")}</div>
            ) : state.error ? (
              <div style={{ ...s.databaseEmptyCompact, color: "var(--danger)" }}>
                {t("database.exportLoadFailed", { message: state.error })}
              </div>
            ) : state.filteredTables.length === 0 ? (
              <div style={s.databaseEmptyCompact}>{t("database.sidebarSearchNoResults")}</div>
            ) : (
              state.filteredTables.map((table) => {
                const selected = state.selection.has(table);
                return (
                  <button
                    key={table}
                    type="button"
                    style={{
                      ...s.databaseListButton,
                      minHeight: 30,
                      padding: "5px 8px",
                      color: selected ? "var(--text-primary)" : "var(--text-secondary)",
                    }}
                    onClick={() => state.toggleTable(table)}
                  >
                    {selected ? <CheckSquare size={14} /> : <Square size={14} />}
                    <span
                      style={{
                        flex: 1,
                        minWidth: 0,
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                      }}
                    >
                      {table}
                    </span>
                  </button>
                );
              })
            )}
          </div>
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
            {t("database.databaseExport")}
          </DbxDialogFooterButton>
        </div>
      </div>
    </div>
  );
}
