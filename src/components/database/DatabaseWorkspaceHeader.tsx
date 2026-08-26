/**
 * 工作区顶部的两块:标签条(有标签时)与标题栏(没标签时)。
 *
 * 从 `DatabaseView.tsx` 抽出时保持 DOM 结构、`role` / `aria-*` 与文案 key 逐字不变,
 * 以免影响已有的 `database-view-*` 用例。两者在 `DatabaseView` 里仍是两个独立的渲染
 * 分支 —— 谁出现由 `workspaceTabs.length` 决定,这个判断留在原处更好读。
 */

import type { MouseEvent } from "react";

import { useI18n } from "../../i18n";
import s from "../../styles";
import type { AeroricDbConnectionConfig, DbEndpoint } from "../../types";
import { AnimatedSelectionTrack } from "../ui/AnimatedSelection";
import { endpointLabel, type DatabaseContextMenuState } from "./databaseViewModel";
import type { WorkspaceTab } from "./databaseWorkspaceStore";

/** 标签里的文字:被挤窄的标签(shortTabIds)只给 72px。 */
const TAB_LABEL_BASE_STYLE = {
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
} as const;

/** 标签主体是个撑满高度的透明按钮,外层 div 负责选中态与右键。 */
const TAB_BUTTON_STYLE = {
  minWidth: 0,
  height: "100%",
  display: "inline-flex",
  alignItems: "center",
  border: 0,
  background: "transparent",
  color: "inherit",
  cursor: "pointer",
} as const;

export interface DatabaseWorkspaceTabBarProps {
  tabs: WorkspaceTab[];
  activeTabId: string;
  /** 需要缩短显示的标签 id;标签一多就靠它省地方。 */
  shortTabIds: Set<string>;
  onActivate: (tab: WorkspaceTab) => void;
  onClose: (tabId: string) => void;
  onOpenContextMenu: (menu: DatabaseContextMenuState) => void;
}

export function DatabaseWorkspaceTabBar({
  tabs,
  activeTabId,
  shortTabIds,
  onActivate,
  onClose,
  onOpenContextMenu,
}: DatabaseWorkspaceTabBarProps) {
  const { t } = useI18n();

  return (
    <AnimatedSelectionTrack
      value={activeTabId}
      ariaLabel={t("database.workspaceTabs")}
      role="tablist"
      variant="underline"
      style={s.databaseTabBar}
    >
      {tabs.map((tab) => (
        <div
          key={tab.id}
          data-animated-selection-item
          data-selection-value={tab.id}
          style={{
            ...s.databaseTab,
            ...(activeTabId === tab.id ? s.databaseTabActive : undefined),
          }}
          onContextMenu={(event: MouseEvent) => {
            event.preventDefault();
            onOpenContextMenu({
              x: event.clientX,
              y: event.clientY,
              tabId: tab.id,
              kind: "workspace-tab",
            });
          }}
        >
          <button
            role="tab"
            aria-selected={activeTabId === tab.id}
            tabIndex={activeTabId === tab.id ? 0 : -1}
            type="button"
            title={tab.label}
            onClick={() => {
              onActivate(tab);
            }}
            style={TAB_BUTTON_STYLE}
          >
            <span
              style={{
                ...TAB_LABEL_BASE_STYLE,
                maxWidth: shortTabIds.has(tab.id) ? 72 : 160,
              }}
            >
              {tab.label}
            </span>
          </button>
          {tab.closable && (
            <button
              type="button"
              aria-label={t("database.closeWorkspaceTab", { name: tab.label })}
              style={s.databaseTabClose}
              onClick={(e) => {
                e.stopPropagation();
                onClose(tab.id);
              }}
            >
              ×
            </button>
          )}
        </div>
      ))}
    </AnimatedSelectionTrack>
  );
}

export interface DatabaseWorkspaceTopbarProps {
  /** 当前工作区模式对应的标题,由 `DatabaseView` 算好传进来。 */
  title: string;
  /** 优先显示 legacy 连接的 endpoint,其次是 dbx 连接,都没有就提示先选连接。 */
  endpoint: DbEndpoint | null;
  connection: AeroricDbConnectionConfig | null;
  error: string | null;
}

export function DatabaseWorkspaceTopbar({
  title,
  endpoint,
  connection,
  error,
}: DatabaseWorkspaceTopbarProps) {
  const { t } = useI18n();

  return (
    <div style={s.databaseTopbar}>
      <div style={{ minWidth: 0 }}>
        <div style={s.databaseTitle}>{title}</div>
        <div style={s.databasePath}>
          {endpoint
            ? endpointLabel(endpoint)
            : connection
              ? `${connection.dbType}: ${connection.name}`
              : t("database.chooseConnection")}
        </div>
      </div>
      {error && (
        <div style={s.databaseError} title={error}>
          {error}
        </div>
      )}
    </div>
  );
}
