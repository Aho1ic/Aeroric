import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Check, Columns2, Copy, Edit3, Maximize2, Plus, Server, Trash2 } from "lucide-react";
import type { FontFamily, SshConnection, TerminalFontSize, ThemeVariant } from "../../types";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { SshConnectionDialog } from "./SshConnectionDialog";
import { SshConnectionContextMenu, type SshConnectionProtocol } from "./SshConnectionContextMenu";
import { SshTerminalPanel } from "./SshTerminalPanel";
import { useCopyFeedback } from "./useCopyFeedback";
import { SshGroupContextMenu } from "./SshGroupContextMenu";
import { useSshGroups } from "./useSshGroups";
import { AnimatedSelectionTrack } from "../ui/AnimatedSelection";
import {
  SSH_TERMINAL_MAX_SESSIONS,
  closeSshTab,
  openSshTab,
  pruneSshTabsForConnection,
  toggleSshCardsView,
  type SshTab,
} from "./sshTabs";
import type { AuxiliaryWorkspaceLayout } from "../project-page/viewMode";

export type SshWorkspaceLayout = AuxiliaryWorkspaceLayout;

function connectionTarget(connection: SshConnection): string {
  return `${connection.username}@${connection.host}:${connection.port}`;
}

/**
 * 分组 → 连接。命名分组按名单顺序在前(空分组也占位),未分组的落到末尾。
 *
 * 第三项标记该分组是否为"真实分组":默认分组只是未分组连接的容器,
 * 重命名和删除都无从落地。
 */
function groupConnections(
  connections: SshConnection[],
  namedGroups: string[],
  fallbackGroup: string,
): Array<[string, SshConnection[], boolean]> {
  const buckets = new Map<string, SshConnection[]>(namedGroups.map((name) => [name, []]));
  const ungrouped: SshConnection[] = [];
  for (const connection of connections) {
    const group = connection.group?.trim();
    const bucket = group ? buckets.get(group) : undefined;
    if (bucket) bucket.push(connection);
    else if (group) buckets.set(group, [connection]);
    else ungrouped.push(connection);
  }
  const grouped: Array<[string, SshConnection[], boolean]> = Array.from(
    buckets,
    ([name, items]) => [name, items, true],
  );
  if (ungrouped.length > 0) grouped.push([fallbackGroup, ungrouped, false]);
  return grouped;
}

async function copyConnectionPassword(connection: SshConnection) {
  const password = connection.password ?? "";
  if (!password || !navigator.clipboard?.writeText) return;
  await navigator.clipboard.writeText(password);
}

function SshCardPicker({
  connections,
  selectedId,
  namedGroups,
  onOpen,
  onEdit,
  onConnect,
  onDelete,
  onRenameGroup,
  onDeleteGroup,
}: {
  connections: SshConnection[];
  selectedId: string | null;
  namedGroups: string[];
  onOpen: (connection: SshConnection) => void;
  onEdit: (connection: SshConnection) => void;
  onConnect?: (connection: SshConnection, protocol: SshConnectionProtocol) => void;
  onDelete: (connection: SshConnection) => void;
  onRenameGroup: (group: string, nextName: string) => void;
  onDeleteGroup: (group: string) => void;
}) {
  const { t } = useI18n();
  const { copiedId: copiedConnectionId, markCopied } = useCopyFeedback();
  const [contextMenu, setContextMenu] = useState<{
    connection: SshConnection;
    x: number;
    y: number;
  } | null>(null);
  const [groupMenu, setGroupMenu] = useState<{ group: string; x: number; y: number } | null>(null);
  const grouped = useMemo(
    () => groupConnections(connections, namedGroups, t("ssh.defaultGroup")),
    [connections, namedGroups, t],
  );

  if (connections.length === 0 && grouped.length === 0) {
    return (
      <div style={s.sshEmptyState}>
        <div style={s.sshEmptyTitle}>{t("ssh.emptyTitle")}</div>
        <div style={s.sshSecretNote}>{t("sshProject.noConnections")}</div>
      </div>
    );
  }

  return (
    <div style={s.sshProjectConnectionPicker}>
      {grouped.map(([group, items, isNamed]) => (
        <section key={group} style={s.sshProjectGroupSection}>
          <div
            style={s.sshProjectGroupTitle}
            onContextMenu={(event) => {
              // 默认分组不是真实分组,重命名/删除都无从落地。
              if (!isNamed) return;
              event.preventDefault();
              event.stopPropagation();
              setGroupMenu({ group, x: event.clientX, y: event.clientY });
            }}
          >
            {group}
          </div>
          {items.length === 0 && <div style={s.sshSecretNote}>{t("ssh.groupEmpty")}</div>}
          <div style={s.sshProjectCardGrid}>
            {items.map((connection) => {
              const selected = selectedId === connection.id;
              const canCopyPassword = Boolean(connection.password);
              return (
                <div
                  key={connection.id}
                  style={selected ? s.sshProjectCardSelected : s.sshProjectCard}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    event.stopPropagation();
                    setContextMenu({
                      connection,
                      x: event.clientX,
                      y: event.clientY,
                    });
                  }}
                >
                  <button
                    type="button"
                    style={s.sshProjectCardSelect}
                    onClick={() => onOpen(connection)}
                    onDoubleClick={() => onOpen(connection)}
                  >
                    <span style={s.sshProjectCardIcon}>
                      <Server size={17} />
                    </span>
                    <span style={s.sshProjectCardText}>
                      <span style={s.sshProjectCardName}>{connection.name}</span>
                      <span style={s.sshProjectCardMeta}>{connectionTarget(connection)}</span>
                      {connection.remotePath && (
                        <span style={s.sshProjectCardMeta}>{connection.remotePath}</span>
                      )}
                    </span>
                  </button>
                  <button
                    type="button"
                    style={s.sshProjectCardEdit}
                    title={t("common.edit")}
                    aria-label={t("common.edit")}
                    onClick={(event) => {
                      event.stopPropagation();
                      onEdit(connection);
                    }}
                  >
                    <Edit3 size={14} />
                  </button>
                  <button
                    type="button"
                    className="ssh-copy-action"
                    style={{
                      ...s.sshProjectCardEdit,
                      opacity: canCopyPassword ? 1 : 0.35,
                      cursor: canCopyPassword ? "pointer" : "not-allowed",
                    }}
                    data-copied={copiedConnectionId === connection.id ? "true" : undefined}
                    title={canCopyPassword ? t("ssh.copyPassword") : t("ssh.noPasswordHint")}
                    aria-label={t("ssh.copyPassword")}
                    disabled={!canCopyPassword}
                    onClick={(event) => {
                      event.stopPropagation();
                      if (!canCopyPassword) return;
                      void copyConnectionPassword(connection).then(() => {
                        markCopied(connection.id);
                      });
                    }}
                  >
                    {copiedConnectionId === connection.id ? (
                      <Check size={14} />
                    ) : (
                      <Copy size={14} />
                    )}
                  </button>
                </div>
              );
            })}
          </div>
        </section>
      ))}
      {groupMenu && (
        <SshGroupContextMenu
          group={groupMenu.group}
          x={groupMenu.x}
          y={groupMenu.y}
          takenNames={namedGroups.filter((name) => name !== groupMenu.group)}
          onClose={() => setGroupMenu(null)}
          onRename={onRenameGroup}
          onDelete={onDeleteGroup}
        />
      )}
      {contextMenu && (
        <SshConnectionContextMenu
          connection={contextMenu.connection}
          x={contextMenu.x}
          y={contextMenu.y}
          onClose={() => setContextMenu(null)}
          onConnect={(connection, protocol) => onConnect?.(connection, protocol)}
          onDelete={onDelete}
        />
      )}
    </div>
  );
}

function SshWorkspaceHeader({
  layout,
  showingCards,
  canReturnToTerminal,
  onToggleLayout,
  onToggleCards,
  onNewConnection,
}: {
  layout: SshWorkspaceLayout;
  showingCards: boolean;
  canReturnToTerminal: boolean;
  onToggleLayout: () => void;
  onToggleCards: () => void;
  onNewConnection: () => void;
}) {
  const { t } = useI18n();
  return (
    <div className="ssh-workspace-header">
      <div className="ssh-workspace-title">
        <Server size={15} />
        {t("ssh.title")}
      </div>
      <div className="ssh-workspace-actions">
        <button
          type="button"
          className="ssh-workspace-icon-btn"
          title={t("ssh.newConnection")}
          onClick={onNewConnection}
        >
          <Plus size={15} />
        </button>
        <button
          type="button"
          className={`ssh-workspace-icon-btn${showingCards ? " active" : ""}`}
          // 同一个按钮两态:开着卡片且有终端可回时说「还原终端」,否则说「显示连接」。
          // 没有标签时它只是个已按下的状态指示,不该承诺一个回不去的动作。
          title={
            showingCards && canReturnToTerminal ? t("ssh.backToTerminal") : t("ssh.showConnections")
          }
          aria-pressed={showingCards}
          onClick={onToggleCards}
        >
          <Server size={15} />
        </button>
        <button
          type="button"
          className="ssh-workspace-icon-btn"
          title={layout === "full" ? t("ssh.splitView") : t("ssh.fullView")}
          onClick={onToggleLayout}
        >
          {layout === "full" ? <Columns2 size={15} /> : <Maximize2 size={15} />}
        </button>
      </div>
    </div>
  );
}

/**
 * 标签条。只在有标签时出现 —— 一条空轨道会让卡片视图凭空矮一截。
 *
 * `+` 是「同主机再开一个会话」的唯一入口:点卡片走聚焦语义,不新建。
 */
function SshTabStrip({
  tabs,
  activeTabId,
  atLimit,
  onSelect,
  onClose,
  onDuplicate,
}: {
  tabs: readonly SshTab[];
  activeTabId: string | null;
  atLimit: boolean;
  onSelect: (tabId: string) => void;
  onClose: (tabId: string) => void;
  onDuplicate: () => void;
}) {
  const { t } = useI18n();
  return (
    <AnimatedSelectionTrack
      value={activeTabId ?? ""}
      ariaLabel={t("ssh.title")}
      role="tablist"
      className="ssh-tab-strip"
    >
      {tabs.map((tab) => {
        const selected = tab.id === activeTabId;
        return (
          <div
            key={tab.id}
            className="ssh-tab"
            data-animated-selection-item
            data-selection-value={tab.id}
            data-selected={selected ? "true" : "false"}
          >
            <button
              type="button"
              role="tab"
              aria-selected={selected}
              className="ssh-tab-label"
              title={tab.title}
              onClick={() => onSelect(tab.id)}
            >
              <Server size={11} />
              <span className="ssh-tab-text">{tab.title}</span>
            </button>
            <button
              type="button"
              className="ssh-tab-close"
              aria-label={t("ssh.closeTab", { title: tab.title })}
              title={t("ssh.closeTab", { title: tab.title })}
              onClick={() => onClose(tab.id)}
            >
              <Trash2 size={9.5} />
            </button>
          </div>
        );
      })}
      <button
        type="button"
        className="ssh-tab-add"
        disabled={atLimit}
        title={atLimit ? t("terminal.limitReached") : t("ssh.newSession")}
        aria-label={atLimit ? t("terminal.limitReached") : t("ssh.newSession")}
        onClick={onDuplicate}
      >
        <Plus size={12} />
      </button>
      <span className="ssh-tab-count">
        {tabs.length}/{SSH_TERMINAL_MAX_SESSIONS}
      </span>
    </AnimatedSelectionTrack>
  );
}

export function SshWorkspace({
  connections,
  onConnectionsChange,
  onDeleteConnection,
  active,
  themeVariant,
  terminalFontSize,
  monoFontFamily,
  onOpenSftp,
  layout,
  onLayoutChange,
}: {
  connections: SshConnection[];
  onConnectionsChange: (connections: SshConnection[]) => void;
  onDeleteConnection?: (connectionId: string) => void | Promise<void>;
  active: boolean;
  themeVariant: ThemeVariant;
  terminalFontSize: TerminalFontSize;
  monoFontFamily: FontFamily;
  remoteConnection?: SshConnection;
  onOpenSftp?: (connection: SshConnection) => void;
  layout: SshWorkspaceLayout;
  onLayoutChange: (layout: SshWorkspaceLayout) => void;
}) {
  const { t } = useI18n();
  const [tabs, setTabs] = useState<SshTab[]>([]);
  const [activeTabId, setActiveTabId] = useState<string | null>(null);
  const tabsRef = useRef<SshTab[]>([]);
  // State updaters can be batched when two cards are clicked before React
  // renders again. Keep the latest selection available to the tab state
  // machine so a limit hit never restores a stale tab.
  const activeTabIdRef = useRef<string | null>(null);
  const commitTabs = useCallback((next: SshTab[]) => {
    tabsRef.current = next;
    setTabs(next);
  }, []);
  const commitActiveTabId = useCallback((next: string | null) => {
    activeTabIdRef.current = next;
    setActiveTabId(next);
  }, []);
  const [showCards, setShowCards] = useState(true);
  const [editingConnection, setEditingConnection] = useState<SshConnection | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [limitNotice, setLimitNotice] = useState(false);
  // 与右侧栏 SSH 列表同源(store 订阅):那里建的空分组在这里也要看得到、删得掉。
  const {
    groups,
    createGroup,
    deleteGroup: deleteGroupName,
    renameGroup,
  } = useSshGroups(connections, onConnectionsChange);
  const hasOpenTabs = tabs.length > 0;
  // 没有标签时终端侧无内容可显示,只能回落到卡片。
  const rightShowsCards = showCards || !hasOpenTabs;
  const activeTab = tabs.find((tab) => tab.id === activeTabId) ?? null;

  const openConnection = useCallback(
    (connection: SshConnection, forceNew = false) => {
      const result = openSshTab({
        tabs: tabsRef.current,
        connection,
        forceNew,
        activeTabId: activeTabIdRef.current,
        now: Date.now(),
      });
      commitTabs(result.tabs);
      commitActiveTabId(result.activeTabId);
      setLimitNotice(result.limitReached);
      if (!result.limitReached) {
        // A successful open/focus enters the terminal. When the limit is hit,
        // keep the current view and explain why nothing moved.
        setShowCards(false);
      }
    },
    [commitActiveTabId, commitTabs],
  );

  /**
   * 连接从名单里消失了(在别处删的、或远端同步下来的),清掉指向它的标签。
   *
   * 不能只靠 `deleteConnection` —— 那条路只覆盖"在这个工作区里删"。名单是外部 prop,
   * 右侧栏、手机端同步都能改它,标签留着就会指向一条不存在的连接:自动重连拿不到
   * connection,标签变成一个永远连不上的空终端。
   */
  useEffect(() => {
    const live = new Set(connections.map((connection) => connection.id));
    const currentTabs = tabsRef.current;
    const orphan = currentTabs.find((tab) => !live.has(tab.connectionId));
    if (!orphan) return;
    let next = currentTabs;
    let nextActive = activeTabIdRef.current;
    for (const tab of currentTabs) {
      if (live.has(tab.connectionId)) continue;
      const result = pruneSshTabsForConnection({
        tabs: next,
        activeTabId: nextActive,
        connectionId: tab.connectionId,
      });
      next = result.tabs;
      nextActive = result.activeTabId;
    }
    commitTabs(next);
    commitActiveTabId(nextActive);
    if (next.length === 0) setShowCards(true);
  }, [commitActiveTabId, commitTabs, connections]);

  const handleCloseTab = useCallback(
    (tabId: string) => {
      const result = closeSshTab({
        tabs: tabsRef.current,
        activeTabId: activeTabIdRef.current,
        tabId,
      });
      commitTabs(result.tabs);
      commitActiveTabId(result.activeTabId);
      if (result.tabs.length === 0) setShowCards(true);
      setLimitNotice(false);
    },
    [commitActiveTabId, commitTabs],
  );

  const saveConnection = (connection: SshConnection) => {
    const exists = connections.some((item) => item.id === connection.id);
    const next = exists
      ? connections.map((item) => (item.id === connection.id ? connection : item))
      : [connection, ...connections];
    onConnectionsChange(next);
    // 对话框里手输的新分组也要进名单,否则移走最后一条连接它就消失了。
    createGroup(connection.group ?? "");
    setEditingConnection(null);
    setDialogOpen(false);
    // 编辑已存在的连接只是改配置,不该顺手把它连起来 —— 新建才自动开会话。
    if (!exists) openConnection(connection);
  };

  const deleteConnection = (connectionId: string) => {
    const next = connections.filter((connection) => connection.id !== connectionId);
    if (onDeleteConnection) {
      void onDeleteConnection(connectionId);
    } else {
      onConnectionsChange(next);
    }
    // 同一台主机可能开着多个标签,全部清掉。SshTerminalPanel 卸载时会 kill 后端 shell。
    const result = pruneSshTabsForConnection({
      tabs: tabsRef.current,
      activeTabId: activeTabIdRef.current,
      connectionId,
    });
    commitTabs(result.tabs);
    commitActiveTabId(result.activeTabId);
    if (result.tabs.length === 0) setShowCards(true);
  };

  const renderChooserOrTerminal = () => (
    <div className="ssh-workspace-pane fill">
      <SshWorkspaceHeader
        layout={layout}
        showingCards={rightShowsCards}
        canReturnToTerminal={hasOpenTabs}
        onToggleLayout={() => onLayoutChange(layout === "full" ? "split" : "full")}
        onToggleCards={() =>
          setShowCards(toggleSshCardsView({ showingCards: rightShowsCards, hasOpenTabs }))
        }
        onNewConnection={() => {
          setEditingConnection(null);
          setDialogOpen(true);
        }}
      />
      {hasOpenTabs && (
        <SshTabStrip
          tabs={tabs}
          activeTabId={rightShowsCards ? null : activeTabId}
          atLimit={tabs.length >= SSH_TERMINAL_MAX_SESSIONS}
          onSelect={(tabId) => {
            commitActiveTabId(tabId);
            // 点标签就是要看那个终端,顺手从卡片视图退出来。
            setShowCards(false);
          }}
          onClose={handleCloseTab}
          onDuplicate={() => {
            const connection = activeTab
              ? connections.find((item) => item.id === activeTab.connectionId)
              : null;
            if (connection) openConnection(connection, true);
          }}
        />
      )}
      {limitNotice && <div style={s.sshErrorBanner}>{t("terminal.limitReached")}</div>}
      <div className="ssh-workspace-body">
        <div
          className="ssh-workspace-card-scroll"
          aria-hidden={!rightShowsCards}
          style={{ display: rightShowsCards ? "block" : "none" }}
        >
          <SshCardPicker
            connections={connections}
            selectedId={activeTab?.connectionId ?? null}
            namedGroups={groups}
            onRenameGroup={renameGroup}
            onDeleteGroup={deleteGroupName}
            onOpen={(connection) => openConnection(connection)}
            onEdit={(connection) => {
              setEditingConnection(connection);
              setDialogOpen(true);
            }}
            onConnect={(connection, protocol) => {
              if (protocol === "sftp") {
                onOpenSftp?.(connection);
                return;
              }
              openConnection(connection);
            }}
            onDelete={(connection) => deleteConnection(connection.id)}
          />
        </div>
        {/*
          每个标签的终端都常挂,非活动的用 display:none 压住 —— 与 ShellTerminalInstance
          同一手法。切标签不能卸载:卸载会 kill 后端 shell(见 SshTerminalPanel 的 cleanup),
          回来就是一个空终端,而用户预期是"接着刚才那条会话"。
        */}
        {tabs.map((tab) => {
          const visible = !rightShowsCards && tab.id === activeTabId;
          return (
            <div
              key={tab.id}
              className="ssh-workspace-terminal-layer"
              aria-hidden={!visible}
              style={{ display: visible ? "flex" : "none" }}
            >
              <SshTerminalPanel
                connections={connections}
                onConnectionsChange={onConnectionsChange}
                onDeleteConnection={onDeleteConnection}
                active={active && visible}
                width="100%"
                themeVariant={themeVariant}
                terminalFontSize={terminalFontSize}
                monoFontFamily={monoFontFamily}
                initialConnectionId={tab.connectionId}
                autoConnect
                hideConnectionList
              />
            </div>
          );
        })}
      </div>
    </div>
  );

  return (
    <div className="ssh-workspace">
      {renderChooserOrTerminal()}

      {dialogOpen && (
        <SshConnectionDialog
          connection={editingConnection}
          groups={groups}
          onClose={() => {
            setDialogOpen(false);
            setEditingConnection(null);
          }}
          onSave={saveConnection}
        />
      )}
    </div>
  );
}
