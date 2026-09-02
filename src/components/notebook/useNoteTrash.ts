/* 回收站面板:列出被删的、恢复一条、彻底删一条、清空。
 *
 * 四件事必须在一个地方对齐:
 *
 * 1. **恢复成功后把那条加回列表,而不是重扫整个 vault** —— 重扫会丢掉别的笔记里未落盘的
 *    编辑。同路径已经在列表里就不重复加:恢复期间用户可能已经新建了同名笔记(后端会拒),
 *    或者这条其实没真的离开过列表。
 *
 * 2. **目录恢复不往列表里加**。笔记列表只放笔记,目录里的那些笔记要重扫才拿得到路径和
 *    内容。用户重开面板就能看到。
 *
 * 3. **彻底删要确认**。载荷进系统回收站,历史快照一起清 —— 这是不可逆的。
 *
 * 4. **清空失败要重新拉列表**。清空是逐条走的,失败时可能已经清掉一部分;原样留着的话
 *    用户看到的是一份已经不准的清单。连列表都拉不回来时保留原来那条错误,别用第二个错误
 *    盖掉它。
 *
 * 状态形状(`NoteTrashState`)在 `NoteTrashSheet.tsx` —— 它是那个面板的 props 形状。 */
import { useState } from "react";
import { freshTrashState, type NoteTrashState } from "./NoteTrashSheet";
import { listTrash, purgeAllTrash, purgeTrashItem, restoreTrashItem } from "./notebookApi";
import { loadNoteByPath, type VaultNote } from "./notebookVault";
import { confirm } from "../../lib/appDialog";
import type { NotebookNote } from "./notebookStore";
import type { Translate } from "./noteExportRun";

export type NoteTrashOptions = {
  vault: string | null;
  t: Translate;
  errorText: (error: unknown) => string;
  setNotes: (update: (current: NotebookNote[]) => NotebookNote[]) => void;
  toPanelNote: (note: VaultNote) => NotebookNote;
  /** 开回收站时要收掉的其它铺满型 overlay。 */
  closeOtherSheets: () => void;
};

export type NoteTrashApi = {
  /** null = 没开。开着时它铺满面板。 */
  state: NoteTrashState | null;
  open: () => void;
  close: () => void;
  restore: (id: string) => void;
  purge: (id: string) => void;
  purgeAll: () => void;
};

export function useNoteTrash(options: NoteTrashOptions): NoteTrashApi {
  const { vault, t, errorText, setNotes, toPanelNote, closeOtherSheets } = options;

  const [state, setState] = useState<NoteTrashState | null>(null);

  const close = () => {
    setState(null);
  };

  /** 打开回收站并拉列表。 */
  const open = () => {
    if (!vault) return;
    closeOtherSheets();
    setState(freshTrashState());
    void (async () => {
      try {
        const items = await listTrash(vault);
        setState((current) => (current ? { ...current, items, loading: false } : current));
      } catch (error) {
        setState((current) =>
          current ? { ...current, loading: false, error: errorText(error) } : current,
        );
      }
    })();
  };

  const restore = (id: string) => {
    if (!vault) return;
    setState((current) => (current ? { ...current, busyId: id, error: null } : current));
    void (async () => {
      try {
        const restored = await restoreTrashItem(vault, id);
        setState((current) =>
          current
            ? { ...current, items: current.items.filter((item) => item.id !== id), busyId: null }
            : current,
        );
        if (restored.isDir) return;
        const note = await loadNoteByPath(restored.path);
        setNotes((current) =>
          current.some((existing) => existing.id === note.path)
            ? current
            : [toPanelNote(note), ...current],
        );
      } catch (error) {
        setState((current) =>
          current ? { ...current, busyId: null, error: errorText(error) } : current,
        );
      }
    })();
  };

  const purge = (id: string) => {
    if (!vault) return;
    const target = state?.items.find((item) => item.id === id);
    if (!target) return;
    void (async () => {
      const ok = await confirm(t("notebook.trashPurgeMessage", { name: target.name }), {
        title: t("notebook.trashPurgeTitle"),
        kind: "warning",
        okLabel: t("notebook.trashPurgeConfirm"),
        cancelLabel: t("notebook.trashPurgeCancel"),
      });
      if (!ok) return;
      setState((current) => (current ? { ...current, busyId: id, error: null } : current));
      try {
        await purgeTrashItem(vault, id);
        setState((current) =>
          current
            ? { ...current, items: current.items.filter((item) => item.id !== id), busyId: null }
            : current,
        );
      } catch (error) {
        setState((current) =>
          current ? { ...current, busyId: null, error: errorText(error) } : current,
        );
      }
    })();
  };

  const purgeAll = () => {
    if (!vault) return;
    const count = state?.items.length ?? 0;
    if (count === 0) return;
    void (async () => {
      const ok = await confirm(t("notebook.trashPurgeAllMessage", { count: String(count) }), {
        title: t("notebook.trashPurgeAllTitle"),
        kind: "warning",
        okLabel: t("notebook.trashPurgeAllConfirm"),
        cancelLabel: t("notebook.trashPurgeCancel"),
      });
      if (!ok) return;
      setState((current) => (current ? { ...current, purgingAll: true, error: null } : current));
      try {
        await purgeAllTrash(vault);
        setState((current) => (current ? { ...current, items: [], purgingAll: false } : current));
      } catch (error) {
        setState((current) =>
          current ? { ...current, purgingAll: false, error: errorText(error) } : current,
        );
        try {
          const items = await listTrash(vault);
          setState((current) => (current ? { ...current, items } : current));
        } catch {
          // 连列表都拉不回来时保留上面那条错误,别用第二个错误盖掉它。
        }
      }
    })();
  };

  return { state, open, close, restore, purge, purgeAll };
}
