import { FileText, Plus, Terminal as TerminalIcon, X } from "lucide-react";

import type { EditorGroupId } from "../../hooks/useProjectPanels";
import { AnimatedSelectionTrack } from "../ui/AnimatedSelection";

/** 一个文件页签:来自某个编辑器分组里的一个打开文件。 */
export interface WorkspaceFileTab {
  groupId: EditorGroupId;
  path: string;
  name: string;
}

/**
 * 一个终端页签。
 *
 * `remote: true` 的那种恒定只有一条(WSL / SSH 项目各一条),且**不可关闭** ——
 * 远端终端跟着项目走,关掉它没有「再开一个」的入口。本地 shell 则是每个会话一条,
 * 都可关闭,并可通过尾部的加号新建。
 */
export interface WorkspaceTerminalTab {
  id: string;
  title: string;
  label: string;
  remote: boolean;
}

interface ProjectWorkspaceTabsProps {
  fileTabs: WorkspaceFileTab[];
  terminalTabs: WorkspaceTerminalTab[];
  /** 终端层当前是否可见:决定选中态落在文件页签还是终端页签上。 */
  terminalVisible: boolean;
  activeTabValue: string;
  activeEditorGroupId: EditorGroupId;
  activeFilePath: string | null;
  activeShellId: string | null;
  /** 只有本地项目能新建 shell;远端项目不显示加号。 */
  canAddTerminal: boolean;
  addTerminalDisabled: boolean;
  t: (key: string, params?: Record<string, string | number>) => string;
  onCloseAllFileTabs: () => void;
  onFileTabSelect: (groupId: EditorGroupId, path: string) => void;
  onFileTabClose: (path: string, groupId: EditorGroupId) => void;
  onTerminalTabSelect: (terminalId: string) => void;
  onTerminalTabClose: (terminalId: string) => void;
  onAddTerminal: () => void;
}

/** 中间区域上方的工作区标签条:文件页签与终端页签并排,共享一条选中轨道。 */
export function ProjectWorkspaceTabs({
  fileTabs,
  terminalTabs,
  terminalVisible,
  activeTabValue,
  activeEditorGroupId,
  activeFilePath,
  activeShellId,
  canAddTerminal,
  addTerminalDisabled,
  t,
  onCloseAllFileTabs,
  onFileTabSelect,
  onFileTabClose,
  onTerminalTabSelect,
  onTerminalTabClose,
  onAddTerminal,
}: ProjectWorkspaceTabsProps) {
  return (
    <AnimatedSelectionTrack
      value={activeTabValue}
      ariaLabel="Workspace tabs"
      role="tablist"
      variant="underline"
      className="terminal-session-tabs"
      dataTestId="workspace-tabs"
      style={{
        minHeight: 34,
        height: 34,
        flexShrink: 0,
        display: "flex",
        alignItems: "center",
        gap: 5,
        padding: "4px 8px",
        borderBottom: "1px solid var(--border-dim)",
        background: "color-mix(in srgb, var(--bg-root) 72%, var(--bg-sidebar))",
        overflowX: "auto",
      }}
    >
      {fileTabs.length > 0 && (
        <button
          type="button"
          aria-label={t("file.closeAllTabs")}
          title={t("file.closeAllTabs")}
          onClick={onCloseAllFileTabs}
          style={{
            width: 24,
            height: 24,
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            padding: 0,
            border: "1px solid var(--border-dim)",
            borderRadius: 6,
            background: "transparent",
            color: "var(--text-secondary)",
            cursor: "pointer",
            flexShrink: 0,
          }}
        >
          <X size={12} />
        </button>
      )}
      {fileTabs.map((tab) => {
        const selected =
          !terminalVisible && tab.groupId === activeEditorGroupId && tab.path === activeFilePath;
        return (
          <div
            key={`file:${tab.groupId}:${tab.path}`}
            data-animated-selection-item
            data-selection-value={`file:${tab.groupId}:${tab.path}`}
            style={{
              height: 24,
              maxWidth: 220,
              display: "inline-flex",
              alignItems: "center",
              border: "1px solid var(--border-dim)",
              borderRadius: 6,
              background: "transparent",
              flexShrink: 0,
            }}
          >
            <button
              type="button"
              role="tab"
              aria-selected={selected}
              tabIndex={selected ? 0 : -1}
              title={tab.path}
              onClick={() => onFileTabSelect(tab.groupId, tab.path)}
              style={{
                minWidth: 0,
                height: "100%",
                display: "inline-flex",
                alignItems: "center",
                gap: 6,
                padding: "0 7px 0 8px",
                border: "none",
                background: "transparent",
                color: selected ? "var(--control-active-fg)" : "var(--text-muted)",
                cursor: "pointer",
                fontSize: 11,
                fontWeight: selected ? 650 : 560,
              }}
            >
              <FileText size={12} />
              <span
                style={{
                  minWidth: 0,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
              >
                {tab.name}
              </span>
            </button>
            <button
              type="button"
              aria-label={t("file.closeTab", { name: tab.name })}
              title={t("file.closeTab", { name: tab.name })}
              onClick={() => onFileTabClose(tab.path, tab.groupId)}
              style={{
                width: 20,
                height: 20,
                display: "inline-flex",
                alignItems: "center",
                justifyContent: "center",
                padding: 0,
                border: "none",
                background: "transparent",
                color: "var(--text-hint)",
                cursor: "pointer",
              }}
            >
              <X size={11} />
            </button>
          </div>
        );
      })}
      {terminalTabs.map((terminal) => {
        const selected = terminalVisible && (terminal.remote || terminal.id === activeShellId);
        return (
          <div
            key={`terminal:${terminal.id}`}
            className="terminal-session-tab"
            data-animated-selection-item
            data-selection-value={`terminal:${terminal.id}`}
            data-selected={selected ? "true" : "false"}
            style={{
              height: 24,
              maxWidth: 150,
              display: "inline-flex",
              alignItems: "center",
              border: "1px solid var(--border-dim)",
              borderRadius: 6,
              background: "transparent",
              flexShrink: 0,
            }}
          >
            <button
              type="button"
              role="tab"
              aria-selected={selected}
              tabIndex={selected ? 0 : -1}
              title={terminal.title}
              onClick={() => onTerminalTabSelect(terminal.id)}
              style={{
                minWidth: 0,
                flex: 1,
                height: "100%",
                display: "inline-flex",
                alignItems: "center",
                gap: 6,
                padding: "0 7px 0 8px",
                border: "none",
                background: "transparent",
                overflow: "hidden",
                whiteSpace: "nowrap",
                color: selected ? "var(--control-active-fg)" : "var(--text-muted)",
                cursor: "pointer",
                fontSize: 11,
                fontWeight: selected ? 650 : 560,
              }}
            >
              <span className="terminal-session-tab__cursor" aria-hidden="true" />
              <TerminalIcon size={12} />
              <span className="terminal-session-tab__label">{terminal.label}</span>
            </button>
            {!terminal.remote && (
              <button
                type="button"
                aria-label={t("terminal.closeShell", { title: terminal.title })}
                title={t("terminal.closeShell", { title: terminal.title })}
                onClick={() => onTerminalTabClose(terminal.id)}
                style={{
                  width: 20,
                  height: 20,
                  display: "inline-flex",
                  alignItems: "center",
                  justifyContent: "center",
                  padding: 0,
                  border: "none",
                  background: "transparent",
                  color: "var(--text-hint)",
                  cursor: "pointer",
                }}
              >
                <X size={11} />
              </button>
            )}
          </div>
        );
      })}
      {canAddTerminal && (
        <button
          type="button"
          aria-label={t("terminal.newTerminal")}
          title={addTerminalDisabled ? t("terminal.limitReached") : t("terminal.newTerminal")}
          disabled={addTerminalDisabled}
          onClick={onAddTerminal}
          style={{
            width: 24,
            height: 24,
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            padding: 0,
            border: "1px solid var(--border-dim)",
            borderRadius: 6,
            background: "transparent",
            color: "var(--text-secondary)",
            cursor: addTerminalDisabled ? "not-allowed" : "pointer",
            flexShrink: 0,
          }}
        >
          <Plus size={12} />
        </button>
      )}
    </AnimatedSelectionTrack>
  );
}
