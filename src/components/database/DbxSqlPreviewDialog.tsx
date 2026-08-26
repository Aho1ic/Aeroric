/**
 * 「预览 SQL」对话框的展示层。状态来自 `useDbxSqlPreviewDialog.ts`,这里不持有任何
 * state —— 从 `DatabaseView.tsx` 抽出时保持 DOM 结构与文案 key 逐字不变,以免影响
 * 已有的 `database-view-*` 用例。
 */

import { useI18n } from "../../i18n";
import s from "../../styles";
import { Button as DbxButton } from "../ui/Button";
import type { DbxSqlPreviewDialogState } from "./useDbxSqlPreviewDialog";

/** 两块语句预览共用的等宽代码块样式,只有高度上限和文字颜色不同。 */
const SQL_BLOCK_STYLE = {
  margin: 0,
  padding: "8px 10px",
  background: "var(--surface-alt)",
  borderRadius: 6,
  fontSize: 12,
  fontFamily: "var(--font-mono)",
  whiteSpace: "pre-wrap",
  wordBreak: "break-word",
  overflow: "auto",
  border: "1px solid var(--border-dim)",
} as const;

const SECTION_LABEL_STYLE = {
  fontSize: 11,
  fontWeight: 650,
  color: "var(--text-muted)",
  marginBottom: 4,
} as const;

export function DbxSqlPreviewDialog({ state }: { state: DbxSqlPreviewDialogState }) {
  const { t } = useI18n();
  if (!state.visible) return null;

  return (
    <div
      style={s.databaseDialogOverlay}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) state.close();
      }}
    >
      <div style={{ ...s.databaseConnectionDialog, width: 560, maxWidth: "min(92vw, 560px)" }}>
        <div style={s.databaseDialogHeader}>{t("database.previewSql")}</div>
        <div style={s.databaseDialogBody}>
          {state.description && (
            <div style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 10 }}>
              {state.description}
            </div>
          )}
          <div style={SECTION_LABEL_STYLE}>{t("database.gridPreviewStatements")}</div>
          <pre style={{ ...SQL_BLOCK_STYLE, maxHeight: "30vh" }}>{state.statements.join("\n")}</pre>
          {state.rollback.length > 0 && (
            <>
              <div style={{ ...SECTION_LABEL_STYLE, marginTop: 10 }}>
                {t("database.gridRollbackSql")}
              </div>
              <pre style={{ ...SQL_BLOCK_STYLE, maxHeight: "20vh", color: "var(--danger)" }}>
                {state.rollback.join("\n")}
              </pre>
            </>
          )}
        </div>
        <div style={s.databaseDialogFooter}>
          <DbxButton variant="outline" size="sm" onClick={state.close}>
            {t("common.close")}
          </DbxButton>
        </div>
      </div>
    </div>
  );
}
