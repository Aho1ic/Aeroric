/* 面板的笔记模型 ⇄ vault 层的笔记模型。
 *
 * 从 NotebookPanel 抽出来,逐字未改。面板和自动保存 hook 都要用。
 *
 * 两边字段几乎一一对应,差别只在主键的叫法:面板用 `id`(它不关心那是什么),
 * vault 用 `path`(它就是文件路径)。留两个模型而不是合成一个,是为了让"面板
 * 不需要知道笔记存在哪"这件事在类型上成立。
 */

import type { NotebookNote } from "./notebookStore";
import type { VaultNote } from "./notebookVault";

/** vault 层的笔记 → 面板的笔记。`id` 用文件路径,天然唯一。 */
export function toPanelNote(note: VaultNote): NotebookNote {
  return {
    id: note.path,
    title: note.title,
    body: note.body,
    updatedAt: note.modifiedMs,
    sig: note.sig,
    frontmatter: note.frontmatter,
    loaded: note.loaded,
  };
}

/** 面板的笔记 → vault 层的笔记。 */
export function toVaultNote(note: NotebookNote): VaultNote {
  return {
    path: note.id,
    title: note.title,
    body: note.body,
    frontmatter: note.frontmatter,
    sig: note.sig,
    modifiedMs: note.updatedAt,
    loaded: note.loaded,
  };
}
