/**
 * 导出进度浮层的展示层。
 *
 * 从 `DatabaseView.tsx` 抽出时保持 DOM 结构与文案 key 逐字不变,以免影响已有的
 * `database-view-*` 用例。进度本身是 `DatabaseView` 的一次性状态(选好路径时置起、
 * 导出结束时清掉),所以这里不配 hook,只按传进来的对象渲染。
 */

import { useI18n } from "../../i18n";
import s from "../../styles";

export interface DatabaseExportProgress {
  active: boolean;
  format: string;
  filePath: string;
}

/** 转圈用的是全局 keyframes `spin`,和其它加载指示一致。 */
const SPINNER_STYLE = {
  width: 16,
  height: 16,
  border: "2px solid var(--border-dim)",
  borderTopColor: "var(--accent)",
  borderRadius: "50%",
  animation: "spin 1s linear infinite",
} as const;

export function DatabaseExportProgressOverlay({
  progress,
}: {
  progress: DatabaseExportProgress | null;
}) {
  const { t } = useI18n();
  if (!progress?.active) return null;

  return (
    <div style={s.databaseDialogOverlay}>
      <div style={{ ...s.databaseConnectionDialog, width: 400, maxWidth: "min(90vw, 400px)" }}>
        <div style={s.databaseDialogHeader}>{t("database.exportProgress")}</div>
        <div style={s.databaseDialogBody}>
          <div style={{ fontSize: 13, color: "var(--text-secondary)", marginBottom: 8 }}>
            {t("database.exportProgressFormat", { format: progress.format.toUpperCase() })}
          </div>
          <div style={{ fontSize: 12, color: "var(--text-muted)", wordBreak: "break-all" }}>
            {progress.filePath}
          </div>
          <div style={{ marginTop: 16, display: "flex", alignItems: "center", gap: 8 }}>
            <div style={SPINNER_STYLE} />
            <span style={{ fontSize: 12, color: "var(--text-muted)" }}>
              {t("database.exportProgressRunning")}
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}
