/* 笔记列表的右键菜单。
 *
 * 和编辑区那个 `NoteContextMenu` 是两回事:那个作用在选中的文字上(加粗、粘贴),
 * 这个作用在**文件**上(改名、揭示、进回收站)。分成两个组件而不是合成一个带
 * 模式开关的,是因为两边的菜单项、可用条件、目标对象没有一项是共用的。
 *
 * 关掉菜单复用面板里那个 outside-click 监听 —— 它认的是
 * `data-notebook-context-menu` 属性,所以这里也带上同一个属性。
 */

import type { LucideIcon } from "lucide-react";
import { Copy, FolderOpen, History, Info, Pencil, Smile, Trash2 } from "lucide-react";
import { zLayers } from "../../styles/zLayers";

export type NoteListContextMenuState = {
  x: number;
  y: number;
  /** 右键点中的那条笔记的路径(= 它的 id)。 */
  noteId: string;
};

export type NoteListContextMenuAction =
  | "rename"
  | "icon"
  | "history"
  | "properties"
  | "reveal"
  | "copyPath"
  | "trash";

export type NoteListContextMenuProps = {
  state: NoteListContextMenuState;
  onAction: (action: NoteListContextMenuAction) => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

export function NoteListContextMenu({ state, onAction, t }: NoteListContextMenuProps) {
  const items: [NoteListContextMenuAction, string, LucideIcon, boolean][] = [
    ["rename", t("notebook.renameMemo"), Pencil, false],
    ["icon", t("notebook.iconChange"), Smile, false],
    ["history", t("notebook.historyOpen"), History, false],
    ["properties", t("notebook.propertiesOpen"), Info, false],
    ["reveal", t("file.openInSystemFolder"), FolderOpen, false],
    ["copyPath", t("file.copyFullPath"), Copy, false],
    // 进 vault 自己的回收站,能原位恢复 —— 所以不弹二次确认,但配色上仍标成危险操作。
    ["trash", t("notebook.moveToTrash"), Trash2, true],
  ];
  return (
    <div
      role="menu"
      aria-label={t("notebook.noteActions")}
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
      {items.map(([action, label, Icon, danger]) => (
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
            color: danger ? "var(--danger, var(--text-primary))" : "var(--text-primary)",
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
