/* 任务收集箱里一条任务的右键菜单。
 *
 * 和 `NoteListContextMenu` 分开而不是加一个模式开关:那个作用在**文件**上(改名、
 * 进回收站),这个作用在**一行**上 —— 目标对象和可用操作没有一项是共用的。
 *
 * 「复制任务文本」复制的是**原文**(`raw`),不是摘掉标记后的显示文本:用户要粘到别处
 * 去的时候,`#标签` 和 `@截止` 通常正是他想带走的那部分。
 *
 * 关掉菜单复用面板里那个 outside-click 监听 —— 它认 `data-notebook-context-menu`
 * 属性,所以这里也带上同一个属性。
 */

import type { LucideIcon } from "lucide-react";
import { Copy, ExternalLink, FolderOpen } from "lucide-react";
import { zLayers } from "../../styles/zLayers";
import type { InboxTask } from "./noteTaskInbox";

export type NoteTaskContextMenuState = {
  x: number;
  y: number;
  task: InboxTask;
};

export type NoteTaskContextMenuAction = "open" | "reveal" | "copyText" | "copyPath";

export type NoteTaskContextMenuProps = {
  state: NoteTaskContextMenuState;
  onAction: (action: NoteTaskContextMenuAction) => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

export function NoteTaskContextMenu({ state, onAction, t }: NoteTaskContextMenuProps) {
  const items: [NoteTaskContextMenuAction, string, LucideIcon][] = [
    ["open", t("notebook.taskInboxOpenSource"), ExternalLink],
    ["reveal", t("file.openInSystemFolder"), FolderOpen],
    ["copyText", t("notebook.taskInboxCopyText"), Copy],
    ["copyPath", t("notebook.taskInboxCopyPath"), Copy],
  ];
  return (
    <div
      role="menu"
      aria-label={t("notebook.taskInboxActions")}
      data-notebook-context-menu
      style={{
        position: "fixed",
        left: state.x,
        top: state.y,
        zIndex: zLayers.contextMenu,
        minWidth: 172,
        padding: "4px 0",
        border: "1px solid var(--border-dim)",
        borderRadius: 7,
        background: "var(--bg-sidebar)",
        boxShadow: "var(--shadow-popover)",
      }}
    >
      {items.map(([action, label, Icon]) => (
        <button
          key={action}
          type="button"
          role="menuitem"
          onClick={() => onAction(action)}
          style={{
            width: "calc(100% - 8px)",
            height: 28,
            margin: "1px 4px",
            padding: "0 10px",
            border: "none",
            borderRadius: 5,
            background: "transparent",
            color: "var(--text-primary)",
            display: "flex",
            alignItems: "center",
            gap: 8,
            textAlign: "left",
            cursor: "pointer",
            fontSize: 13,
          }}
        >
          <Icon size={13} />
          {label}
        </button>
      ))}
    </div>
  );
}
