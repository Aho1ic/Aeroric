/* 编辑器内的触发式菜单:`[[` 链接补全、`#` 标签补全、`/` 斜杠命令。
 *
 * 三种触发共用一套状态机(触发态 + 选中下标 + 一份候选行),差别只在候选从哪来。
 * 合在一个 hook 里而不是各写一份,是因为它们**互斥且共用同一块屏幕位置** —— 分开写
 * 就要再造一层去裁决"谁在显示",而那层的状态正好等于现在这个 `state`。
 *
 * 五件事必须在一个地方对齐:
 *
 * 1. **显示和提交必须共用同一份候选来源**(`completionSource`)。两边各算一次的话,
 *    只要有一处输入不同(比如一边过滤了当前笔记、另一边没有),提交时按 id 就找不到
 *    那一行,表现是"选中了却没插进去"。
 *
 * 2. **换笔记要主动收起菜单**。`state.start` 是**上一篇**里的文档偏移,换篇之后它指向
 *    的位置已经没有意义 —— 新的那篇更短时甚至越界。编辑器不会替我们收:换篇走的是
 *    value prop,`setState`(CodeMirror 内部重置文档)不带 `selectionSet`,那次 update
 *    里 `docChanged` 为真但光标已经在 0,`detectTrigger` 返回 null 前提是它跑得到 ——
 *    实测菜单会原样留在屏幕上。
 *
 * 3. **替换区间以编辑器当前的光标为终点**,不用 `state.query.length` 反推 —— 反推要求
 *    「触发符 + 查询」的长度始终等于那段距离,而任何一次不成立都会吃掉前面的正文。
 *
 * 4. **候选变少时把选中下标拉回范围内**。留在原下标上会指向不存在的行。
 *
 * 5. **`onTriggerKey` 不能包 `useCallback`**。编辑器把它存进 ref、每次按键重新读,所以
 *    引用稳不稳定对那边没有意义 —— 包一层反而要把 `commitRow` 也一起稳住,而它依赖正文、
 *    笔记列表和标签扫描,稳不住。
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { moveSelection } from "./noteCommands";
import { buildCompletions, COMPLETION_LIMIT, rankCandidates } from "./noteCompletions";
import { completionRow, slashRow, type TriggerRow } from "./NoteTriggerMenu";
import { resolveSlashInsert, SLASH_ITEMS } from "./noteSlashItems";
import type { TriggerKind } from "./noteTriggers";
import type { NoteEditorHandle, TriggerKeyName, TriggerState } from "./NoteSourceEditor";
import type { NotebookNote } from "./notebookStore";
import type { Translate } from "./noteExportRun";

export type NoteTriggersOptions = {
  notes: readonly NotebookNote[];
  activeNote: NotebookNote | null;
  /** 全库标签扫描的结果。没跑过 / 还在跑时给空数组,补全就只靠正文里现写的那些。
   *  不收 `readonly` —— `CompletionSource.vaultTags` 是可变数组。 */
  vaultTagRefs: string[];
  t: Translate;
  editorRef: React.RefObject<NoteEditorHandle | null>;
  /** 用过一次 `#` 补全就通知面板(它据此打开标签扫描)。 */
  onTagCompletionUsed: () => void;
};

export type NoteTriggersApi = {
  /** null = 没触发。 */
  state: TriggerState | null;
  rows: TriggerRow[];
  selected: number;
  setSelected: (next: number) => void;
  /** 收起菜单。查询留在正文里不动 —— 用户打的字不该因为关个菜单就消失。 */
  close: () => void;
  /** 编辑器报上来一次触发态。接给 `onTriggerChange`。 */
  handleChange: (next: TriggerState | null) => void;
  /** 菜单开着时接管方向键 / 回车 / Tab / Esc。接给 `onTriggerKey`。 */
  handleKey: (key: TriggerKeyName) => boolean;
  /** 提交第 `index` 行。 */
  commitRow: (index: number) => void;
};

export function useNoteTriggers(options: NoteTriggersOptions): NoteTriggersApi {
  const { notes, activeNote, vaultTagRefs, t, editorRef, onTagCompletionUsed } = options;

  const [state, setState] = useState<TriggerState | null>(null);
  const [selected, setSelected] = useState(0);

  const completionSource = useCallback(
    (kind: Exclude<TriggerKind, "slash">, query: string) => ({
      kind,
      query,
      /* 不把当前这篇列出来:自链没有意义,而它的标题恰恰是用户此刻脑子里的那个词,
         模糊匹配下往往排第一,把真正想链的那篇挤下去。段内跳转用 `[[#标题]]`,
         不需要先写自己的篇名。 */
      notes: notes
        .filter((note) => note.id !== activeNote?.id)
        .map((note) => ({ id: note.id, title: note.title || t("notebook.untitled") })),
      vaultTags: vaultTagRefs,
      body: activeNote?.body ?? "",
    }),
    [notes, activeNote?.id, activeNote?.body, vaultTagRefs, t],
  );

  /** 菜单要显示的行。触发种类决定候选从哪来,排序两条路共用 `rankCandidates`。 */
  const rows: TriggerRow[] = useMemo(() => {
    if (!state) return [];
    if (state.kind === "slash") {
      const candidates = SLASH_ITEMS.map((item) => slashRow(item, t));
      if (state.query === "") return candidates;
      return rankCandidates(candidates, state.query, COMPLETION_LIMIT).map(({ item, spans }) => ({
        ...item,
        spans,
      }));
    }
    return buildCompletions(completionSource(state.kind, state.query)).map(completionRow);
  }, [state, completionSource, t]);

  /* 候选变少时把选中项拉回范围内(不变式 4)。 */
  useEffect(() => {
    setSelected((current) => moveSelection(current, 0, rows.length));
  }, [rows.length]);

  const close = useCallback(() => setState(null), []);

  /* 换笔记就收起菜单(不变式 2)。 */
  useEffect(() => {
    setState(null);
  }, [activeNote?.id]);

  const handleChange = useCallback(
    (next: TriggerState | null) => {
      setState(next);
      // 换了一次触发就从第一条开始选。同一次触发里打字缩短列表由上面那个 effect 夹。
      setSelected(0);
      if (next?.kind === "tag") onTagCompletionUsed();
    },
    [onTagCompletionUsed],
  );

  /** 提交选中的那一行:把 `[start, cursor)` 换成候选文本(不变式 3)。 */
  const commitRow = (index: number) => {
    const row = rows[index];
    const editor = editorRef.current;
    if (!state || !row || !editor) return;
    const cursor = editor.selectionEnd();
    /* 光标跑到触发符前面去了:这次提交没有意义,`replaceRange(start, cursor)` 的
       区间是反的。
       这一条杀不掉(变异掉它测试全绿),留着是因为它防的是**偏移过期**这一类,而那
       一类确实发生过 —— 换笔记时 `start` 会指向上一篇的位置。真正的闸门是上面那个
       挂在 `activeNote?.id` 上的 effect(有回归测试守着),这里只是最后一道:
       任何新的"文档在菜单开着时被换掉"的路径都会先撞到它,而不是写坏正文。 */
    if (cursor < state.start) {
      close();
      return;
    }
    if (state.kind === "slash") {
      const item = SLASH_ITEMS.find((candidate) => candidate.id === row.id);
      if (!item) return;
      const { text, cursor: offset } = resolveSlashInsert(item);
      close();
      editor.replaceRange(state.start, cursor, text, "after");
      // 落点在插入文本中间的(代码块、`[[]]`)要再挪一次 —— `replaceRange` 只会放到末尾。
      if (offset !== text.length) editor.setSelection(state.start + offset, state.start + offset);
      return;
    }
    const item = buildCompletions(completionSource(state.kind, state.query)).find(
      (candidate) => candidate.id === row.id,
    );
    if (!item) return;
    close();
    editor.replaceRange(state.start, cursor, item.insert, "after");
  };

  /**
   * 菜单开着时接管方向键 / 回车 / Tab / Esc。返回 false 就让 CodeMirror 照常处理。
   *
   * Tab 也接:补全菜单里 Tab 选中是通行习惯(编辑器、shell 都是),而正文里的 Tab
   * 在菜单开着时插一个缩进几乎肯定不是用户想要的。
   *
   * 不包 `useCallback`,见不变式 5。
   */
  const handleKey = (key: TriggerKeyName): boolean => {
    if (!state) return false;
    if (key === "Escape") {
      close();
      return true;
    }
    if (key === "ArrowDown" || key === "ArrowUp") {
      setSelected((current) => moveSelection(current, key === "ArrowDown" ? 1 : -1, rows.length));
      return true;
    }
    // 没有候选时回车该照常换行,而不是被菜单吃掉。
    if (rows.length === 0) return false;
    commitRow(selected);
    return true;
  };

  return { state, rows, selected, setSelected, close, handleChange, handleKey, commitRow };
}
