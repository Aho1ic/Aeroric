/**
 * 「选择要显示的数据库」对话框的展示层。状态与动作全部来自 `useVisibleDatabasesDialog`,
 * 这里不持有任何 state —— 从 `DatabaseView.tsx` 抽出时保持 DOM 结构、aria-label
 * 与文案 key 逐字不变,以免影响已有的 `database-view-*` 用例。
 */

import { CheckSquare, Search, Square } from "lucide-react";

import { useI18n } from "../../i18n";
import s from "../../styles";
import { DialogFooterButton as DbxDialogFooterButton } from "../ui/Button";
import type { VisibleDatabasesDialogState } from "./useVisibleDatabasesDialog";

const LINK_BUTTON_STYLE = {
  border: "none",
  background: "transparent",
  color: "var(--text-secondary)",
  fontSize: 11.5,
  cursor: "pointer",
  padding: "0 2px",
} as const;

export function VisibleDatabasesDialog({ state }: { state: VisibleDatabasesDialogState }) {
  const { t } = useI18n();
  const { connection } = state;
  // 连接解析不出来时不渲染:这也是原来 `visibleDatabaseConnection && (...)` 的守卫。
  if (!connection) return null;

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
        aria-label={t("database.visibleDatabasesTitle")}
        style={{ ...s.databaseDialog, width: 460 }}
      >
        <div style={s.databaseDialogHeader}>{t("database.visibleDatabasesTitle")}</div>
        <div style={s.databaseDialogBody}>
          <div style={s.databaseDialogHint}>
            {t("database.visibleDatabasesDescription", { connection: connection.name })}
          </div>
          <label style={s.databaseSearchBox}>
            <Search size={13} />
            <input
              aria-label={t("database.visibleDatabasesSearch")}
              style={s.databaseSearchInput}
              value={state.search}
              onChange={(event) => state.setSearch(event.target.value)}
              placeholder={t("database.visibleDatabasesSearch")}
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
              {t("database.visibleDatabasesSelectedCount", {
                selected: state.selection.size,
                total: state.listedNames.length,
              })}
            </span>
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <button
                type="button"
                style={LINK_BUTTON_STYLE}
                onClick={() => state.setSelection(new Set(state.listedNames))}
                disabled={state.loading}
              >
                {t("database.visibleDatabasesSelectAll")}
              </button>
              {state.search.trim() && (
                <button
                  type="button"
                  style={LINK_BUTTON_STYLE}
                  onClick={() => state.setSelection(new Set(state.filteredNames))}
                  disabled={state.loading}
                >
                  {t("database.visibleDatabasesSelectFiltered")}
                </button>
              )}
              <button
                type="button"
                style={LINK_BUTTON_STYLE}
                onClick={() => state.setSelection(new Set())}
                disabled={state.loading}
              >
                {t("database.visibleDatabasesClear")}
              </button>
              <button
                type="button"
                style={LINK_BUTTON_STYLE}
                onClick={() => {
                  void state.showAll();
                }}
                disabled={state.loading || !state.hasConfiguredSelection}
              >
                {t("database.visibleDatabasesShowAll")}
              </button>
            </div>
          </div>
          {!state.loading && !state.error && !state.canSave && (
            <div style={{ color: "var(--danger)", fontSize: 12 }}>
              {t("database.visibleDatabasesEmptySelection")}
            </div>
          )}
          {state.hasSystemNames && (
            <label style={s.databaseSwitchRow}>
              <input
                type="checkbox"
                checked={state.showSystem}
                onChange={(event) => state.setShowSystem(event.target.checked)}
                disabled={state.loading || Boolean(state.error)}
              />
              <span>{t("database.visibleDatabasesShowSystem")}</span>
            </label>
          )}
          <div
            style={{
              height: 288,
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
                {t("database.visibleDatabasesLoadFailed", { message: state.error })}
              </div>
            ) : state.filteredNames.length === 0 ? (
              <div style={s.databaseEmptyCompact}>{t("database.sidebarSearchNoResults")}</div>
            ) : (
              state.filteredNames.map((database) => {
                const selected = state.selection.has(database);
                return (
                  <button
                    key={database}
                    type="button"
                    style={{
                      ...s.databaseListButton,
                      minHeight: 30,
                      padding: "5px 8px",
                      color: selected ? "var(--text-primary)" : "var(--text-secondary)",
                    }}
                    onClick={() => state.toggleSelection(database)}
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
                      {database}
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
              void state.save();
            }}
            disabled={state.loading || Boolean(state.error) || !state.canSave}
          >
            {t("database.visibleDatabasesSave")}
          </DbxDialogFooterButton>
        </div>
      </div>
    </div>
  );
}
