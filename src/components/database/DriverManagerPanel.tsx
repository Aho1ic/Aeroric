/**
 * `workspaceMode === "drivers"` 那一屏:一张只读表,列出后端 manifest 里报上来的驱动。
 *
 * 从 `DatabaseView.tsx` 抽出时保持列顺序、DOM 结构与文案 key 逐字不变,以免影响已有的
 * `database-view-*` 用例。manifest 仍由 `DatabaseView` 负责拉取,这里只渲染。
 */

import { RefreshCcw } from "lucide-react";

import { useI18n } from "../../i18n";
import s from "../../styles";
import type { DatabaseDriverManifest } from "../../types";
import { Button as DbxButton } from "../ui/Button";

/** 能力列太长会撑爆表格,只显示前六个,余下的靠 title 兜住。 */
const CAPABILITY_PREVIEW_COUNT = 6;

export interface DriverManagerPanelProps {
  /** 还没拉到就是 null —— 这时表体只有一行占位。 */
  manifest: DatabaseDriverManifest | null;
  loading: boolean;
  onRefresh: () => void | Promise<void>;
}

export function DriverManagerPanel({ manifest, loading, onRefresh }: DriverManagerPanelProps) {
  const { t } = useI18n();

  return (
    <div style={s.databaseWorkspacePanel}>
      <div style={s.databaseWorkspaceHeader}>
        <div>
          <div style={s.databaseWorkspaceTitle}>{t("database.driverManager")}</div>
          <div style={s.databaseDialogHint}>{t("database.driverManagerHint")}</div>
        </div>
        <DbxButton
          variant="outline"
          size="sm"
          icon={RefreshCcw}
          onClick={onRefresh}
          disabled={loading}
        >
          {t("common.refresh")}
        </DbxButton>
      </div>
      <div style={s.databaseTableWrap}>
        <table style={s.databaseTable}>
          <thead>
            <tr>
              <th style={s.databaseTh}>{t("database.driver")}</th>
              <th style={s.databaseTh}>{t("database.runtime")}</th>
              <th style={s.databaseTh}>{t("database.defaultPort")}</th>
              <th style={s.databaseTh}>{t("database.supportLevel")}</th>
              <th style={s.databaseTh}>{t("database.capabilities")}</th>
            </tr>
          </thead>
          <tbody>
            {(manifest?.drivers ?? []).map((driver) => {
              const enabledCapabilities = Object.entries(driver.capabilities)
                .filter(([, enabled]) => enabled)
                .map(([key]) => key);
              return (
                <tr key={driver.dbType}>
                  <td style={s.databaseTd}>{driver.label}</td>
                  <td style={s.databaseTd}>{driver.runtimeMode}</td>
                  <td style={s.databaseTd}>{driver.defaultPort ?? "-"}</td>
                  <td style={s.databaseTd}>{driver.supportLevel}</td>
                  <td style={s.databaseTd} title={enabledCapabilities.join(", ")}>
                    {enabledCapabilities.slice(0, CAPABILITY_PREVIEW_COUNT).join(", ")}
                    {enabledCapabilities.length > CAPABILITY_PREVIEW_COUNT ? "..." : ""}
                  </td>
                </tr>
              );
            })}
            {!manifest && (
              <tr>
                <td style={s.databaseTd} colSpan={5}>
                  {loading ? t("database.loading") : t("database.empty")}
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
