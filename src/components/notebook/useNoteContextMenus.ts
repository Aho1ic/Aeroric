/**
 * 两个右键菜单(编辑区、笔记列表)的窗态与动作分派,外加剪切 / 复制 / 粘贴。
 *
 * 收在一处的理由是**这三块只有合起来才说得清"点右键之后会发生什么"** —— 菜单什么
 * 时候关、关了之后哪些坐标还要留着、哪些动作顺手切当前笔记、哪些故意不切。散开的
 * 话每一条都变成"某个 setState 后面的一句注释",没人能一次读完。
 *
 * 收集箱那条任务的菜单不在这里(它的态跟着收集箱走,见 `useNoteSheets` 的不变量 3),
 * 但"点到别处就关"这一下是三个共用的,所以那一个的 setter 要传进来。
 *
 * 不变量:
 *
 * 1. **剪贴板走 `navigator.clipboard` + CodeMirror 事务,不用 `document.execCommand`**。
 *    后者已废弃,而且它作用于 DOM 的 contenteditable 选区 —— CodeMirror 的文档状态在
 *    `EditorState` 里,execCommand 改不动它。富文本编辑器还在时那条路能用,换成
 *    CodeMirror 之后就悄悄失效了(而当时的测试只断言 execCommand 被调用过,不断言
 *    剪贴板真的发生了操作,所以没发现)。
 *
 * 2. **剪切要先复制成功、再删原文**。反过来的话写剪贴板失败就等于丢内容。
 *
 * 3. **读剪贴板走 Tauri 插件、写走 `navigator.clipboard`**。与 Aeroric 别处一致(见
 *    `terminalCopyHelper.ts`):`navigator.clipboard.readText` 在 WebView 里常因权限
 *    被拒,插件走的是系统 API。
 *
 * 4. **点到别处就关,挂 `mousedown` 而不是 `click`**。菜单项自己在 mousedown 阶段就要
 *    读到菜单状态,而 `click` 要等松手,那时候位置可能已经变了。菜单自己那棵子树带
 *    `data-notebook-context-menu`,点在里面不关。
 *
 * 5. **两个菜单互斥**。开列表菜单时先关编辑区那个 —— 两个同时挂着没有意义,而它们都是
 *    `fixed`,不会互相挤开。
 *
 * 6. **动作先关菜单、再动手**。唯一的例外是坐标:列表菜单关掉之后「改图标」的选择器还
 *    要接在同一个位置弹出,所以关之前先把 `{x, y}` 抄下来。
 *
 * 7. **`history` / `properties` 顺手切当前笔记,`icon` 故意不切**。前两者展示的是**编辑器
 *    里的当前文本**(历史面板的 diff 右侧、属性里的字数),不切就会拿 A 的正文去报 B 的
 *    数;而改图标只动列表上的一个符号,把用户手上正在编辑的那篇顶掉是纯粹的打扰。
 *
 * 8. **reveal 要把 vault 当 allowlist 根传下去**。后端的 `validate_path_within` 会拒掉
 *    vault 之外的路径,免得这个入口变成任意路径的揭示器。
 */
import { readText as readClipboardText } from "@tauri-apps/plugin-clipboard-manager";
import type React from "react";
import { useEffect, useState } from "react";

import {
  isClipboardAction,
  type NoteContextMenuAction,
  type NoteContextMenuState,
} from "./NoteContextMenu";
import type { NoteIconPickerState } from "./NoteIconPicker";
import type { NoteListContextMenuAction, NoteListContextMenuState } from "./NoteListContextMenu";
import { revealNoteInFileManager } from "./notebookApi";
import type { NotebookNote } from "./notebookStore";
import type { NoteEditorHandle } from "./NoteSourceEditor";
import type { NoteTaskContextMenuState } from "./NoteTaskContextMenu";
import type { NoteFormatting } from "./useNoteFormatting";

export type NoteContextMenusOptions = {
  editorRef: React.RefObject<NoteEditorHandle | null>;
  /** 列表菜单按 id 找那条笔记(改名要整个 note,不只是路径)。 */
  notes: NotebookNote[];
  /** reveal 的 allowlist 根,见不变量 8。 */
  vault: string | null;
  /** 加粗 / 斜体 / 列表 / 表格都走它,和工具栏是同一套。 */
  format: NoteFormatting;
  setActiveId: (noteId: string) => void;
  /** 收集箱那条任务的菜单。这里只在「点到别处」时清它,见不变量 4。 */
  setTaskMenu: (next: NoteTaskContextMenuState | null) => void;
  startRename: (note: NotebookNote) => void;
  openHistory: (noteId: string) => void;
  openProperties: (noteId: string) => void;
  openIconPicker: (state: NoteIconPickerState) => void;
  removeNote: (noteId: string) => void;
  setError: (error: string | null) => void;
  errorText: (error: unknown) => string;
  t: (key: string, vars?: Record<string, string | number>) => string;
};

export type NoteContextMenusApi = {
  /** 编辑区那个菜单的态。`null` = 没开。 */
  contextMenu: NoteContextMenuState | null;
  /** 笔记列表那个菜单的态。`null` = 没开。 */
  listMenu: NoteListContextMenuState | null;
  /** 在编辑区点右键。`canFormat` 由这一刻有没有选区决定。 */
  openEditorMenu: (at: { x: number; y: number; canFormat: boolean }) => void;
  /** 在列表某条笔记上点右键。见不变量 5。 */
  openListMenu: (at: { x: number; y: number; noteId: string }) => void;
  runContextMenuAction: (action: NoteContextMenuAction) => void;
  runListMenuAction: (action: NoteListContextMenuAction) => void;
};

export function useNoteContextMenus({
  editorRef,
  notes,
  vault,
  format,
  setActiveId,
  setTaskMenu,
  startRename,
  openHistory,
  openProperties,
  openIconPicker,
  removeNote,
  setError,
  errorText,
  t,
}: NoteContextMenusOptions): NoteContextMenusApi {
  const [contextMenu, setContextMenu] = useState<NoteContextMenuState | null>(null);
  const [listMenu, setListMenu] = useState<NoteListContextMenuState | null>(null);

  // 见不变量 4。
  useEffect(() => {
    const close = (event: MouseEvent) => {
      const target = event.target;
      if (target instanceof Element && target.closest("[data-notebook-context-menu]")) return;
      setContextMenu(null);
      setListMenu(null);
      setTaskMenu(null);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
    // `setTaskMenu` 是 useState 的 setter,恒定 —— 列进去不会让这次监听反复重挂。
  }, [setTaskMenu]);

  const openEditorMenu = (at: { x: number; y: number; canFormat: boolean }) => setContextMenu(at);

  const openListMenu = (at: { x: number; y: number; noteId: string }) => {
    // 见不变量 5。
    setContextMenu(null);
    setListMenu(at);
  };

  /** 剪切 / 复制 / 粘贴。见不变量 1、2、3。 */
  const runClipboardAction = async (action: "cut" | "copy" | "paste") => {
    const editor = editorRef.current;
    if (!editor) return;
    try {
      if (action === "paste") {
        const text = await readClipboardText();
        if (!text) return;
        editor.replaceRange(editor.selectionStart(), editor.selectionEnd(), text, "after");
        return;
      }
      const selected = editor.selectedText();
      if (!selected) return;
      await navigator.clipboard?.writeText(selected);
      if (action === "cut") {
        // 复制成功之后才删,见不变量 2。
        editor.replaceRange(editor.selectionStart(), editor.selectionEnd(), "", "after");
      }
    } catch (error) {
      setError(errorText(error));
    }
  };

  const runContextMenuAction = (action: NoteContextMenuAction) => {
    const menu = contextMenu;

    const clipboard = isClipboardAction(action);
    if (!clipboard && !menu?.canFormat) return;
    setContextMenu(null);
    if (clipboard) {
      void runClipboardAction(action as "cut" | "copy" | "paste");
      return;
    }
    if (action === "bold") format.applyWrap("**", "**");
    if (action === "italic") format.applyWrap("*", "*");
    if (action === "underline") format.applyWrap("<u>", "</u>");
    if (action === "strike") format.applyWrap("~~", "~~");
    if (action === "bullet") format.applyList(false);
    if (action === "numbered") format.applyList(true);
    if (action === "table") format.applyTable();
  };

  const runListMenuAction = (action: NoteListContextMenuAction) => {
    const noteId = listMenu?.noteId;
    // 菜单马上关掉,但坐标要留着,见不变量 6。
    const menuAt = listMenu ? { x: listMenu.x, y: listMenu.y } : null;
    setListMenu(null);
    if (!noteId) return;
    const target = notes.find((note) => note.id === noteId);
    if (!target) return;

    if (action === "rename") {
      startRename(target);
      return;
    }
    if (action === "history") {
      // 顺手切到这条笔记,见不变量 7。
      setActiveId(noteId);
      openHistory(noteId);
      return;
    }
    if (action === "properties") {
      /* 同上,理由更硬:属性里的字数 / 标题数算的是编辑器里的当前文本,而列表里的笔记
         正文往往还没读入 —— 不切的话那个数会是 0。 */
      setActiveId(noteId);
      openProperties(noteId);
      return;
    }
    if (action === "icon") {
      // 沿用右键那一刻的坐标(见不变量 6);不切当前笔记(见不变量 7)。
      openIconPicker({ x: menuAt?.x ?? 0, y: menuAt?.y ?? 0, noteId });
      return;
    }
    if (action === "trash") {
      removeNote(noteId);
      return;
    }
    if (action === "copyPath") {
      // `note.id` 就是绝对路径(见 notebookStore 的注释)。
      void navigator.clipboard
        ?.writeText(noteId)
        .catch((error: unknown) => setError(t("file.copyPathFailed", { error: errorText(error) })));
      return;
    }
    // 见不变量 8。
    if (!vault) {
      setError(t("notebook.vaultUnavailable"));
      return;
    }
    void revealNoteInFileManager(noteId, vault).catch((error: unknown) =>
      setError(errorText(error)),
    );
  };

  return {
    contextMenu,
    listMenu,
    openEditorMenu,
    openListMenu,
    runContextMenuAction,
    runListMenuAction,
  };
}
