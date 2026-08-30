/* 随手记的笔记列表侧栏。
 *
 * 从 NotebookPanel 抽出来的,JSX 逐字未改 —— 面板当时 1857 行,已远超计划书
 * §6.2 定的「单个 UI 文件 ≤ 800 行」。
 *
 * 这个组件只负责渲染和把交互往上抛;拖拽排序、改名的实际逻辑仍在面板里
 * (它们要改 store,而 store 的写入口集中在面板)。
 */

import type React from "react";
import { Braces, FileText, GripVertical, Plus, Share2, Trash2 } from "lucide-react";
import { noteIconComponent } from "./NoteIconPicker";
import type { NoteIconName } from "./noteIcons";
import type { NotebookNote } from "./notebookStore";
import { normalizeEnglishPunctuation } from "./notePunctuation";

export type NoteListProps = {
  notes: NotebookNote[];
  activeNote: NotebookNote | null;
  /** 首次加载 vault 期间为 true —— 「还在读」和「真的没有」要分开显示。 */
  loading: boolean;
  /** 最近一次后台失败。必须可见,静默降级会让用户以为笔记丢了。 */
  loadError: string | null;
  renamingNoteId: string | null;
  renamingTitle: string;
  onRenamingTitleChange: (value: string) => void;
  onCommitRename: () => void;
  onCancelRename: () => void;
  onStartRename: (note: NotebookNote) => void;
  onSelect: (id: string) => void;
  onCreate: () => void;
  /** 打开回收站。放在列表头而不是标题栏:回收站属于整个 vault,不属于某条笔记
   *  —— 挂在标题栏上会让人以为它列的是"这条笔记的历史版本"。 */
  onOpenTrash: () => void;
  onOpenFields: () => void;
  onOpenGraph: () => void;
  /** 行上右键。菜单本身由面板渲染(它持有 vault 和后端调用)。 */
  onNoteContextMenu: (event: React.MouseEvent<HTMLDivElement>, noteId: string) => void;
  /** 拖拽命中检测需要每行的 DOM。 */
  setNoteItemRef: (noteId: string) => (element: HTMLDivElement | null) => void;
  onPointerDown: (event: React.PointerEvent<HTMLButtonElement>, noteId: string) => void;
  onPointerMove: (event: React.PointerEvent<HTMLButtonElement>) => void;
  onPointerUp: (event: React.PointerEvent<HTMLButtonElement>) => void;
  onPointerCancel: (event: React.PointerEvent<HTMLButtonElement>) => void;
  draggedNoteId: string | null;
  dragOverNoteId: string | null;
  /** 拖拽结束后要吞掉紧随的 click,否则会误切换笔记。 */
  suppressNextClickRef: React.MutableRefObject<boolean>;
  /** 一条笔记的自定义图标。返回 undefined 表示用默认图标。 */
  iconOf?: (noteId: string) => NoteIconName | undefined;
  /** 列表底部的附件分区。由面板构造(它持有 vault 和插入逻辑)。 */
  attachmentSection?: React.ReactNode;
  t: (key: string, vars?: Record<string, string>) => string;
};

export function NoteList({
  notes,
  activeNote,
  loading,
  loadError,
  renamingNoteId,
  renamingTitle,
  onRenamingTitleChange,
  onCommitRename,
  onCancelRename,
  onStartRename,
  onSelect,
  onCreate,
  onOpenTrash,
  onOpenFields,
  onOpenGraph,
  onNoteContextMenu,
  setNoteItemRef,
  onPointerDown,
  onPointerMove,
  onPointerUp,
  onPointerCancel,
  draggedNoteId,
  dragOverNoteId,
  suppressNextClickRef,
  iconOf,
  attachmentSection,
  t,
}: NoteListProps) {
  return (
    <aside
      style={{
        minWidth: 0,
        borderRight: "1px solid var(--border-dim)",
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
      }}
    >
      <div
        style={{
          height: 38,
          display: "flex",
          alignItems: "center",
          gap: 6,
          padding: "0 8px",
          borderBottom: "1px solid var(--border-dim)",
        }}
      >
        <FileText size={14} />
        <strong style={{ fontSize: 12, flex: 1 }}>{t("notebook.title")}</strong>
        <div style={{ position: "relative", display: "inline-flex" }}>
          <button
            type="button"
            aria-label={t("notebook.graphOpen")}
            title={t("notebook.graphOpen")}
            onClick={onOpenGraph}
            style={{
              border: "none",
              background: "transparent",
              color: "var(--text-muted)",
              cursor: "pointer",
              padding: 3,
            }}
          >
            <Share2 size={14} />
          </button>
          <button
            type="button"
            aria-label={t("notebook.fieldsOpen")}
            title={t("notebook.fieldsOpen")}
            onClick={onOpenFields}
            style={{
              border: "none",
              background: "transparent",
              color: "var(--text-muted)",
              cursor: "pointer",
              padding: 3,
            }}
          >
            <Braces size={14} />
          </button>
          <button
            type="button"
            aria-label={t("notebook.trashOpen")}
            title={t("notebook.trashOpen")}
            onClick={onOpenTrash}
            style={{
              border: "none",
              background: "transparent",
              color: "var(--text-muted)",
              cursor: "pointer",
              padding: 3,
            }}
          >
            <Trash2 size={14} />
          </button>
          <button
            type="button"
            aria-label={t("notebook.newMemo")}
            title={t("notebook.newMemo")}
            onClick={onCreate}
            style={{
              border: "none",
              background: "transparent",
              color: "var(--text-muted)",
              cursor: "pointer",
              padding: 3,
            }}
          >
            <Plus size={14} />
          </button>
        </div>
      </div>
      <div
        style={{
          flex: 1,
          overflowY: "auto",
          padding: 6,
          display: "flex",
          flexDirection: "column",
          gap: 6,
        }}
      >
        {loadError && (
          // 后台失败必须可见 —— 静默降级会让用户以为笔记丢了。
          <div
            role="alert"
            style={{
              padding: "6px 8px",
              borderRadius: 6,
              background: "var(--danger-subtle, var(--bg-card))",
              color: "var(--danger, var(--text-primary))",
              fontSize: 11,
              lineHeight: 1.4,
              wordBreak: "break-word",
            }}
          >
            {loadError}
          </div>
        )}
        {notes.length === 0 ? (
          <div style={{ padding: 10, fontSize: 12, color: "var(--text-hint)", lineHeight: 1.4 }}>
            {/* 「还在读」和「真的没有」要分开说,否则加载期间看着像空的。 */}
            {loading ? t("notebook.loading") : t("notebook.empty")}
          </div>
        ) : (
          notes.map((note) =>
            renamingNoteId === note.id ? (
              <input
                key={note.id}
                aria-label={t("notebook.renameMemo")}
                value={renamingTitle}
                autoFocus
                onFocus={(event) => event.currentTarget.select()}
                onChange={(event) =>
                  onRenamingTitleChange(normalizeEnglishPunctuation(event.currentTarget.value))
                }
                onBlur={onCommitRename}
                onKeyDown={(event) => {
                  if (event.key === "Enter") onCommitRename();
                  if (event.key === "Escape") onCancelRename();
                }}
                style={{
                  minHeight: 30,
                  border: "1px solid var(--border-focus)",
                  borderRadius: 6,
                  background: "var(--bg-input)",
                  color: "var(--text-primary)",
                  padding: "5px 7px",
                  fontSize: 12,
                  outline: "none",
                }}
              />
            ) : (
              <div
                key={note.id}
                ref={setNoteItemRef(note.id)}
                data-notebook-note-row
                onContextMenu={(event) => onNoteContextMenu(event, note.id)}
                style={{
                  minHeight: 30,
                  display: "flex",
                  alignItems: "center",
                  gap: 4,
                  border: "1px solid transparent",
                  borderRadius: 6,
                  background:
                    dragOverNoteId === note.id
                      ? "var(--bg-hover)"
                      : note.id === activeNote?.id
                        ? "var(--bg-selected)"
                        : "transparent",
                  color: "var(--text-primary)",
                  padding: "3px 5px",
                  fontSize: 12,
                  fontWeight: 700,
                  opacity: draggedNoteId === note.id ? 0.55 : 1,
                  transform:
                    draggedNoteId === note.id
                      ? "scale(0.985)"
                      : dragOverNoteId === note.id
                        ? "translateY(2px)"
                        : "none",
                  boxShadow: dragOverNoteId === note.id ? "inset 0 0 0 1px var(--accent)" : "none",
                  transition:
                    "background 0.14s ease, opacity 0.14s ease, transform 0.16s ease, box-shadow 0.16s ease",
                }}
              >
                <button
                  type="button"
                  aria-label={t("notebook.dragMemo", {
                    name: note.title || t("notebook.untitled"),
                  })}
                  title={t("notebook.dragMemo", { name: note.title || t("notebook.untitled") })}
                  onPointerDown={(event) => onPointerDown(event, note.id)}
                  onPointerMove={onPointerMove}
                  onPointerUp={onPointerUp}
                  onPointerCancel={onPointerCancel}
                  style={{
                    width: 20,
                    height: 22,
                    border: "none",
                    borderRadius: 5,
                    background: "transparent",
                    color: "var(--text-hint)",
                    display: "inline-flex",
                    alignItems: "center",
                    justifyContent: "center",
                    flexShrink: 0,
                    padding: 0,
                    cursor: draggedNoteId === note.id ? "grabbing" : "grab",
                    touchAction: "none",
                    userSelect: "none",
                  }}
                >
                  <GripVertical size={14} strokeWidth={2} />
                </button>
                {(() => {
                  /* 自定义图标。`aria-hidden` + 不可聚焦:它是装饰,行的可及名
                     由标题按钮给出。加进可及名会让屏读把"书 周报"读成一个整体,
                     而"书"只是用户挑的一个符号。 */
                  const iconName = iconOf?.(note.id);
                  if (!iconName) return null;
                  const Icon = noteIconComponent(iconName);
                  return (
                    <span
                      aria-hidden="true"
                      style={{
                        display: "inline-flex",
                        alignItems: "center",
                        flexShrink: 0,
                        color: "var(--text-muted)",
                      }}
                    >
                      <Icon size={13} />
                    </span>
                  );
                })()}
                <button
                  type="button"
                  title={note.title}
                  onClick={(event) => {
                    if (suppressNextClickRef.current) {
                      suppressNextClickRef.current = false;
                      event.preventDefault();
                      return;
                    }
                    onSelect(note.id);
                  }}
                  onDoubleClick={() => onStartRename(note)}
                  style={{
                    flex: 1,
                    minWidth: 0,
                    border: "none",
                    background: "transparent",
                    color: "var(--text-primary)",
                    textAlign: "left",
                    padding: "2px 2px",
                    cursor: "pointer",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                    fontSize: 12,
                    fontWeight: 700,
                    fontFamily: "var(--font-ui)",
                  }}
                >
                  {note.title || t("notebook.untitled")}
                </button>
              </div>
            ),
          )
        )}
      </div>
      {/* 附件在笔记之后:它是笔记的附属物,而且默认折叠 —— 放在上面会先占掉
          一行,把真正要找的笔记挤下去。 */}
      {attachmentSection}
    </aside>
  );
}
