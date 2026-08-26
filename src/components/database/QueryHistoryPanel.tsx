/**
 * `workspaceMode === "query-history"` 那一屏:一列历史 SQL,点一条就把它填回编辑器。
 *
 * 从 `DatabaseView.tsx` 抽出时保持 DOM 结构与文案 key 逐字不变,以免影响已有的
 * `database-view-*` 用例。历史记录本身仍存在 `DatabaseView` 的 state 里 —— 写入它的是
 * 执行 SQL 的那条路径,这里只读。
 */

import { Trash2 } from "lucide-react";

import { useI18n } from "../../i18n";
import s from "../../styles";
import { Button as DbxButton } from "../ui/Button";
import type { QueryHistoryEntry } from "./databaseViewModel";

/** 每条记录是个纵向排列的按钮:整块可点,点了就恢复这条查询。 */
const HISTORY_BUTTON_STYLE = {
  ...s.databaseListButton,
  alignItems: "stretch",
  flexDirection: "column",
  gap: 6,
  minHeight: 0,
  padding: "10px 12px",
} as const;

const HISTORY_META_ROW_STYLE = {
  display: "flex",
  gap: 8,
  width: "100%",
  alignItems: "center",
} as const;

const HISTORY_STATS_ROW_STYLE = {
  display: "flex",
  gap: 10,
  color: "var(--text-hint)",
  fontSize: 11,
} as const;

export interface QueryHistoryPanelProps {
  entries: QueryHistoryEntry[];
  onClear: () => void;
  onRestore: (entry: QueryHistoryEntry) => void;
}

export function QueryHistoryPanel({ entries, onClear, onRestore }: QueryHistoryPanelProps) {
  const { t } = useI18n();

  return (
    <div style={s.databaseWorkspacePanel}>
      <div style={s.databaseWorkspaceHeader}>
        <div>
          <div style={s.databaseWorkspaceTitle}>{t("database.queryHistory")}</div>
          <div style={s.databaseDialogHint}>{t("database.queryHistoryHint")}</div>
        </div>
        <DbxButton
          variant="outline"
          size="sm"
          icon={Trash2}
          onClick={onClear}
          disabled={entries.length === 0}
        >
          {t("database.clearQueryHistory")}
        </DbxButton>
      </div>
      {entries.length === 0 ? (
        <div style={s.databaseEmpty}>{t("database.queryHistoryEmpty")}</div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          {entries.map((entry) => (
            <button
              key={entry.id}
              type="button"
              style={HISTORY_BUTTON_STYLE}
              onClick={() => onRestore(entry)}
            >
              <div style={HISTORY_META_ROW_STYLE}>
                <span style={{ color: "var(--text-secondary)", fontWeight: 600 }}>
                  {entry.connectionName}
                </span>
                {entry.database && (
                  <span style={{ color: "var(--text-hint)" }}>{entry.database}</span>
                )}
                {entry.schema && <span style={{ color: "var(--text-hint)" }}>{entry.schema}</span>}
                <span style={{ marginLeft: "auto", color: "var(--text-hint)", fontSize: 11 }}>
                  {new Date(entry.executedAt).toLocaleString()}
                </span>
              </div>
              <pre style={{ ...s.databaseSqlPreview, margin: 0, maxHeight: 86 }}>{entry.sql}</pre>
              <div style={HISTORY_STATS_ROW_STYLE}>
                {entry.rowsAffected != null && (
                  <span>{t("database.historyRowsAffected", { rows: entry.rowsAffected })}</span>
                )}
                {entry.executionTimeMs != null && (
                  <span>{t("database.historyExecutionTime", { ms: entry.executionTimeMs })}</span>
                )}
                <span>{t("database.restoreQuery")}</span>
              </div>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
