/**
 * 铺满型 sheet 的「开」与「从里面跳出去」:开一档 sheet、点语义检索的命中、点收集箱
 * 里的一条任务,以及收集箱那条任务的右键菜单。
 *
 * 四条收在一起是因为它们共用同一条身体语言 —— **先把铺满面板的那一层收掉,再把落点
 * 交给编辑器**,见不变量 1;而"哪些档要显式关"这件事只有一处答案,见不变量 2。
 *
 * 不变量:
 *
 * 1. **从 sheet 里跳走之前必须先关 sheet**。它铺满整个面板,不关的话光标落在编辑器里
 *    而用户还盯着 sheet —— 看起来像点了没反应。字段浏览器那里点笔记**不**关是另一回事
 *    (见 `NoteOverlays`):那一档常常要连着点好几篇来比较,而点一条任务、点一条检索
 *    命中的意思就是"我现在要去改它"。
 *
 * 2. **`openSheet` 要显式关掉历史 / 回收站 / 属性那三档**。它们不在 `sheets` 的单槽里,
 *    所以不会被 `sheets.open` 顺手关掉。而它们和这几档在 JSX 里有排在前面的、也有排在
 *    后面的:排在后面的会盖住新开的这一档,排在前面的会留在底下继续接键盘事件 ——
 *    一次 Esc 关掉两个,而用户只看见一个消失。
 *
 * 3. **没有 vault 就不开**。这几档全是全库范围的东西,没有库时打开只会是一个空窗。
 *
 * 4. **各档的数据不在这里备**。字段 / 收集箱走 `useVaultScan` 的 `enabled`,图谱和反链
 *    共用同一份反链扫描,语义检索走 `useNoteRag`,同步那一路自己一直在轮询 —— 都是按
 *    `sheets` 的当前档自己去取。
 *
 * 5. **跳转只记落点,不自己滚**。真正的滚动和放光标由编辑器接 `cursorOffset` 完成,
 *    见 `useNoteJump` 的不变量 2。所以这里算出来的行号即使暂时偏了(见 `openAiHit`),
 *    正文到位后还会重算一次。
 */
import { toVaultNote } from "./noteConverters";
import { resolveHitNoteId } from "./noteGlobalSearch";
import { fileLineOfBodyScalar, type RagHit } from "./noteRag";
import type { NoteTaskContextMenuAction } from "./NoteTaskContextMenu";
import { revealNoteInFileManager } from "./notebookApi";
import type { NotebookNote } from "./notebookStore";
import { noteFileContent } from "./notebookVault";
import type { NoteSheetName, NoteSheetsApi } from "./useNoteSheets";

export type NoteSheetNavOptions = {
  /** 没有库时 `openSheet` 什么都不做,见不变量 3。 */
  vault: string | null;
  /** 语义检索的命中要按它把路径对回面板里的笔记 id。 */
  notes: NotebookNote[];
  sheets: NoteSheetsApi;
  /* 那三档不在 `sheets` 的单槽里,得一个一个关。分开传而不是合成一个
     `closeOthers` —— 见不变量 2,要关哪几个本身就是这条不变量的内容。 */
  closeHistory: () => void;
  closeTrash: () => void;
  closeProperties: () => void;
  /** 命中对不上笔记时刷一次索引状态,让用户看到"索引是旧的"。 */
  refreshAiStats: () => void;
  /** 记下"要落在哪一篇的哪一行"。见不变量 5。 */
  jumpTo: (path: string, line: number) => void;
  setError: (error: string | null) => void;
  errorText: (error: unknown) => string;
  t: (key: string, vars?: Record<string, string | number>) => string;
};

export type NoteSheetNavApi = {
  /** 开一档铺满型 sheet。见不变量 2、3。 */
  openSheet: (name: NoteSheetName) => void;
  /** 点语义检索的一条命中。 */
  openAiHit: (hit: RagHit) => void;
  /** 点收集箱里的一条任务。 */
  jumpToInboxTask: (path: string, line: number) => void;
  /** 收集箱右键菜单的四项操作。 */
  runTaskMenuAction: (action: NoteTaskContextMenuAction) => void;
};

export function useNoteSheetNav({
  vault,
  notes,
  sheets,
  closeHistory,
  closeTrash,
  closeProperties,
  refreshAiStats,
  jumpTo,
  setError,
  errorText,
  t,
}: NoteSheetNavOptions): NoteSheetNavApi {
  // 见不变量 2、3、4。
  const openSheet = (name: NoteSheetName) => {
    if (!vault) return;
    closeHistory();
    closeTrash();
    closeProperties();
    sheets.open(name);
  };

  /**
   * 点一条命中:关掉 sheet,跳到那一块在原文里的位置。
   *
   * 两次坐标换算都躲不掉:命中给的是**正文**里的**标量**偏移,而跳转要的是按整个
   * `.md` 文件数的行号。`fileLineOfBodyScalar` 一并做掉,理由见它的注释。
   *
   * 笔记还没读进来时 `body` 是空串,那时算出来的行号一律是 1。这是可接受的,见
   * 不变量 5:正文到位后 `jump.cursorOffset` 会按文件行号重算一次。
   */
  const openAiHit = (hit: RagHit) => {
    const noteId = resolveHitNoteId(
      hit.path,
      notes.map((note) => note.id),
      vault ?? "",
    );
    if (!noteId) {
      // 对不上就明说。静默 return 会让用户以为面板坏了 —— 理由同 `openGlobalSearchHit`。
      refreshAiStats();
      setError(t("notebook.aiUnresolved"));
      return;
    }
    const note = notes.find((item) => item.id === noteId);
    const line = note
      ? fileLineOfBodyScalar(noteFileContent(toVaultNote(note)), note.body, hit.charStart)
      : 1;
    sheets.close();
    jumpTo(noteId, line);
  };

  // 见不变量 1。
  const jumpToInboxTask = (path: string, line: number) => {
    sheets.close();
    jumpTo(path, line);
  };

  const runTaskMenuAction = (action: NoteTaskContextMenuAction) => {
    const target = sheets.taskMenu?.task;
    sheets.setTaskMenu(null);
    if (!target) return;
    if (action === "open") {
      jumpToInboxTask(target.path, target.line);
      return;
    }
    if (action === "copyText") {
      /* 复制**原文**而不是显示文本:`#标签` 和 `@截止` 通常正是用户想带走的那部分。 */
      void navigator.clipboard
        ?.writeText(target.raw)
        .catch((error: unknown) => setError(t("file.copyPathFailed", { error: errorText(error) })));
      return;
    }
    if (action === "copyPath") {
      // 带行号,和 Markio 一致 —— 粘到别处能直接定位。
      void navigator.clipboard
        ?.writeText(`${target.path}:${target.line}`)
        .catch((error: unknown) => setError(t("file.copyPathFailed", { error: errorText(error) })));
      return;
    }
    // reveal:vault 当作 allowlist 根传下去,理由同笔记列表那个菜单。
    if (!vault) {
      setError(t("notebook.vaultUnavailable"));
      return;
    }
    void revealNoteInFileManager(target.path, vault).catch((error: unknown) =>
      setError(errorText(error)),
    );
  };

  return { openSheet, openAiHit, jumpToInboxTask, runTaskMenuAction };
}
