/**
 * 「执行 SQL 文件」面板的展示层。表单状态来自 `useSqlFilePanel.ts`,执行动作由
 * `DatabaseView` 传进来 —— 从 `DatabaseView.tsx` 抽出时保持 DOM 结构与文案 key
 * 逐字不变,以免影响已有的 `database-view-*` 用例。
 */

import { FileCode, Play } from "lucide-react";

import { useI18n } from "../../i18n";
import s from "../../styles";
import { Button as DbxButton } from "../ui/Button";
import type { SqlFilePanelState } from "./useSqlFilePanel";

export function SqlFilePanel({
  state,
  busy,
  canExecute,
  onExecute,
}: {
  state: SqlFilePanelState;
  /** DatabaseView 的全局忙碌态。 */
  busy: boolean;
  /** 当前选中的连接能不能跑 SQL;不能就只禁用执行按钮,面板照旧可填。 */
  canExecute: boolean;
  onExecute: () => void;
}) {
  const { t } = useI18n();

  return (
    <div style={s.databaseWorkspacePanel}>
      <div style={s.databaseWorkspaceHeader}>
        <div>
          <div style={s.databaseWorkspaceTitle}>{t("database.executeSqlFile")}</div>
          <div style={s.databaseDialogHint}>{t("database.sqlFileHint")}</div>
        </div>
        <DbxButton
          variant="outline"
          size="sm"
          icon={FileCode}
          onClick={() => void state.chooseFile()}
          disabled={busy}
        >
          {t("database.chooseSqlFile")}
        </DbxButton>
      </div>
      <div style={s.databaseDialogFormGrid}>
        <label style={s.databaseDialogField}>
          <span style={s.databaseDialogLabel}>{t("database.filePath")}</span>
          <input
            style={s.databaseDialogInput}
            value={state.path}
            onChange={(event) => state.setPath(event.target.value)}
            placeholder={t("database.placeholder.sqlScriptPath")}
          />
        </label>
        <label style={s.databaseDialogField}>
          <span style={s.databaseDialogLabel}>{t("database.timeoutSecs")}</span>
          <input
            style={s.databaseDialogInput}
            value={state.timeoutSecs}
            onChange={(event) => state.setTimeoutSecs(event.target.value)}
          />
        </label>
      </div>
      <pre style={s.databaseSqlPreview}>{state.preview || t("database.sqlFilePreviewEmpty")}</pre>
      <div style={s.databaseDialogFooter}>
        <DbxButton
          variant="default"
          size="sm"
          icon={Play}
          onClick={onExecute}
          disabled={busy || !state.path.trim() || !canExecute}
        >
          {t("database.executeSqlFile")}
        </DbxButton>
      </div>
    </div>
  );
}
