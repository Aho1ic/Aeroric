/* 随手记编辑区的右键菜单。
 *
 * 从 NotebookPanel 抽出来,JSX 逐字未改。
 *
 * 菜单项分两类:剪贴板操作(剪切/复制/粘贴)始终可点,格式操作要有选区才可点
 * —— 没选中任何文字时"加粗"没有作用对象。这条规则由 `canFormat` 表达,判定在
 * 面板里做(它持有 CodeMirror 的 handle)。
 *
 * 关掉菜单的 outside-click 监听留在面板里:`data-notebook-context-menu` 这个
 * 属性就是给那个监听认自己用的。
 */

import { zLayers } from "../../styles/zLayers";

export type NoteContextMenuState = {
  x: number;
  y: number;
  /** 有选区时才允许格式化。 */
  canFormat: boolean;
};

export type NoteContextMenuAction =
  | "cut"
  | "copy"
  | "paste"
  | "bold"
  | "italic"
  | "underline"
  | "strike"
  | "bullet"
  | "numbered"
  | "table";

/** 剪贴板操作不需要选区(粘贴本来就没有),所以不受 `canFormat` 限制。 */
export function isClipboardAction(action: NoteContextMenuAction): boolean {
  return action === "cut" || action === "copy" || action === "paste";
}

export type NoteContextMenuProps = {
  state: NoteContextMenuState;
  onAction: (action: NoteContextMenuAction) => void;
  t: (key: string) => string;
};

export function NoteContextMenu({ state, onAction, t }: NoteContextMenuProps) {
  const items: [NoteContextMenuAction, string][] = [
    ["cut", t("notebook.cut")],
    ["copy", t("notebook.copy")],
    ["paste", t("notebook.paste")],
    ["bold", t("notebook.bold")],
    ["italic", t("notebook.italic")],
    ["underline", t("notebook.underline")],
    ["strike", t("notebook.strike")],
    ["bullet", t("notebook.bulletList")],
    ["numbered", t("notebook.numberedList")],
    ["table", t("notebook.table")],
  ];
  return (
    <div
      role="menu"
      data-notebook-context-menu
      style={{
        position: "fixed",
        left: state.x,
        top: state.y,
        zIndex: zLayers.contextMenu,
        minWidth: 148,
        padding: "4px 0",
        border: "1px solid var(--border-dim)",
        borderRadius: 7,
        background: "var(--bg-sidebar)",
        boxShadow: "var(--shadow-popover)",
      }}
    >
      {items.map(([action, label]) => {
        const disabled = !isClipboardAction(action) && !state.canFormat;
        return (
          <button
            key={action}
            type="button"
            role="menuitem"
            disabled={disabled}
            onClick={() => onAction(action)}
            style={{
              width: "calc(100% - 8px)",
              height: 28,
              margin: "1px 4px",
              padding: "0 10px",
              border: "none",
              borderRadius: 5,
              background: "transparent",
              color: disabled ? "var(--text-muted)" : "var(--text-primary)",
              display: "flex",
              alignItems: "center",
              textAlign: "left",
              cursor: disabled ? "not-allowed" : "pointer",
              opacity: disabled ? 0.55 : 1,
              fontSize: 13,
            }}
          >
            {label}
          </button>
        );
      })}
    </div>
  );
}
