/* 属性面板:文件元数据 + 全库那一组(标签、被引用的篇数与链接条数)。
 *
 * 四件事必须在一个地方对齐:
 *
 * 1. **大小和修改时间只能来自磁盘**。内存里那份笔记的 `updatedAt` 是**打开时**的时间戳,
 *    而且它不带字节数。
 *
 * 2. **两条链分开跑,分开报错**。全库扫描比 `stat` 慢得多,串在一起会让"文件多大"跟着
 *    全库扫描一起等。侧栏那两档的结果不能拿来用 —— 它们只在对应档可见时才扫,而属性面板
 *    不要求侧栏开着,多数时候那两份是空的。
 *
 * 3. **两条链都要按 noteId 对账**。请求飞行途中用户可能已经关掉面板或换看另一条笔记的
 *    属性;回来的不是当前那条就丢掉,否则慢的响应会盖掉快的,数字和标题对不上。
 *
 * 4. **目标笔记没了就把状态清掉**。不清的话文件名被回收利用后,同路径的新笔记会顶着上一条
 *    的大小和修改时间显示出来。
 *
 * 反链那两个数走和侧栏完全同一条路(`collectBacklinks`),不另写一份计数:属性面板说
 * "3 篇"而反链档列出 4 篇的话,没人知道该信哪个。 */
import { useEffect, useState } from "react";
import { freshPropertiesState, type NotePropertiesState } from "./NotePropertiesSheet";
import { statNote, vaultLinks, vaultTags } from "./notebookApi";
import { collectBacklinks, countBacklinks } from "./noteBacklinks";
import { tagsInNote } from "./noteTags";
import type { VaultLinkIndex } from "./noteLinks";
import type { NotebookNote } from "./notebookStore";

export type NotePropertiesOptions = {
  vault: string | null;
  notes: readonly NotebookNote[];
  /** 全库链接索引。反链计数要走它,和侧栏共用同一份。 */
  linkIndex: VaultLinkIndex;
  errorText: (error: unknown) => string;
  /** 开属性面板时要收掉的其它 overlay。 */
  closeOtherSheets: () => void;
};

export type NotePropertiesApi = {
  /** null = 没开。 */
  state: NotePropertiesState | null;
  /** 面板针对的那条笔记。找不到时为 null(状态由内部的 effect 负责清)。 */
  note: NotebookNote | null;
  open: (noteId: string) => void;
  close: () => void;
};

export function useNoteProperties(options: NotePropertiesOptions): NotePropertiesApi {
  const { vault, notes, linkIndex, errorText, closeOtherSheets } = options;

  const [state, setState] = useState<NotePropertiesState | null>(null);

  const note = state ? (notes.find((entry) => entry.id === state.noteId) ?? null) : null;

  const close = () => {
    setState(null);
  };

  useEffect(() => {
    if (!state) return;
    if (notes.some((entry) => entry.id === state.noteId)) return;
    setState(null);
  }, [notes, state]);

  const open = (noteId: string) => {
    closeOtherSheets();
    setState(freshPropertiesState(noteId));
    void (async () => {
      try {
        const stat = await statNote(noteId);
        setState((current) =>
          current?.noteId === noteId ? { ...current, stat, loading: false } : current,
        );
      } catch (error) {
        setState((current) =>
          current?.noteId === noteId
            ? { ...current, loading: false, error: errorText(error) }
            : current,
        );
      }
    })();
    if (!vault) return;
    void (async () => {
      try {
        const [tagSources, linkSources] = await Promise.all([vaultTags(vault), vaultLinks(vault)]);
        const groups = collectBacklinks(linkSources, linkIndex, noteId);
        const facts = {
          tags: tagsInNote(tagSources, noteId),
          mentionNotes: groups.length,
          mentionLinks: countBacklinks(groups),
        };
        setState((current) =>
          current?.noteId === noteId ? { ...current, vault: facts, vaultLoading: false } : current,
        );
      } catch (error) {
        setState((current) =>
          current?.noteId === noteId
            ? { ...current, vaultLoading: false, vaultError: errorText(error) }
            : current,
        );
      }
    })();
  };

  return { state, note, open, close };
}
