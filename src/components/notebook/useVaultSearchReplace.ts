/* 全库搜索(⌘⇧F)与全库替换。
 *
 * 两件事收在同一个 hook 里,因为它们**共用同一个查询和同一组开关** —— 替换栏画在
 * 搜索面板里,替换用的就是搜索那一栏的 query/flags。拆成两个 hook 的话这几个状态
 * 得由面板持有再对穿传回去,那样面板并没有变薄,只是多了一层。
 *
 * 三件事必须在一个地方对齐:
 *
 * 1. **两个 run 序号各自独立**(`globalRunRef` / `replaceRunRef`)。用户改条件重搜时
 *    前一次的 promise 可能后回来,不带序号就会把旧结果盖在新结果上,而列表看不出这
 *    一点。搜索和预览是两条并行的链,共用一个序号会互相作废。
 *
 * 2. **预览和落笔前都要 `settleSave` 全部笔记**。后端算的偏移和乐观锁比对的都是磁盘
 *    上的内容;内存里改了没落盘时两边不是同一份正文。预览之后用户还可能再编辑,所以
 *    落笔前要再等一次,不能只在预览时等。
 *
 * 3. **落笔后一切按替换前文本算出来的东西都过期**:预览的偏移、命中列表的 lineText、
 *    以及当前这篇在编辑器里的受控 value。前两个清掉,第三个靠 `bumpEditorEpoch` 重建。
 *
 * 模型层(选项换算、路径 → noteId、勾选集合求差)在 `noteGlobalSearch.ts` 和
 * `noteVaultReplace.ts`,都是纯函数;这里只管状态和 IPC 编排。 */
import { useEffect, useRef, useState } from "react";
import {
  NOTE_SEARCH_LIMIT,
  noteSearchOptions,
  resolveHitNoteId,
  type NoteSearchFlags,
  type NoteSearchHit,
} from "./noteGlobalSearch";
import {
  buildReplacements,
  resolvePreviewNoteIds,
  vaultReplaceOptions,
  type VaultReplacePreview,
  type VaultReplaceSummary,
} from "./noteVaultReplace";
import {
  applyVaultReplacements,
  peekNote,
  previewVaultReplace,
  searchNotesText,
} from "./notebookApi";
import { deriveTitle, splitNote } from "./noteFrontmatter";
import type { Translate } from "./noteExportRun";
import type { NotebookNote } from "./notebookStore";

export type VaultSearchReplaceOptions = {
  vault: string | null;
  notes: readonly NotebookNote[];
  activeNoteId: string | null;
  t: Translate;
  errorText: (error: unknown) => string;
  /** 没有 vault 时把话说在面板顶部那条全局错误里 —— 面板还没开,没有别的落点。 */
  setPanelError: (message: string) => void;
  /** 落盘挂起的保存。预览和落笔都要先等它。 */
  settleSave: (noteId: string) => Promise<void>;
  /** 把重读回来的正文写进笔记列表。 */
  setNotes: (update: (current: NotebookNote[]) => NotebookNote[]) => void;
  /** 当前这篇的正文被替换改过时重建编辑器。 */
  bumpEditorEpoch: () => void;
  /** 开全库搜索时把当前这篇的查找栏收掉:两个都开着 Escape 该关谁没有直觉答案。 */
  closeNoteFind: () => void;
  /** 关掉后把焦点还给编辑器。 */
  focusEditor: () => void;
  /** 点一条命中后的跳转(按文件行号换算正文偏移,和反链共用)。 */
  jumpToBacklink: (noteId: string, line: number) => void;
};

export type VaultSearchReplaceApi = {
  open: boolean;
  query: string;
  setQuery: (value: string) => void;
  flags: NoteSearchFlags;
  setFlags: (value: NoteSearchFlags) => void;
  hits: readonly NoteSearchHit[];
  loading: boolean;
  error: string | null;
  /** 命中数已经顶到后端上限,列表不完整。 */
  capped: boolean;
  /** 搜过至少一次。用来区分「没有结果」和「还没搜」。 */
  searched: boolean;
  inputRef: React.RefObject<HTMLInputElement | null>;

  openSheet: () => void;
  closeSheet: () => void;
  /** 只关不抢焦点。命令面板要盖到最上面,收掉这层时焦点该归它。 */
  dismiss: () => void;
  runSearch: () => void;
  openHit: (hit: NoteSearchHit) => void;

  replaceValue: string;
  setReplaceValue: (value: string) => void;
  preview: VaultReplacePreview | null;
  excluded: ReadonlySet<string>;
  toggleFile: (path: string) => void;
  busy: boolean;
  summary: VaultReplaceSummary | null;
  canPreview: boolean;
  runPreview: () => void;
  apply: () => void;
};

export function useVaultSearchReplace(options: VaultSearchReplaceOptions): VaultSearchReplaceApi {
  const {
    vault,
    notes,
    activeNoteId,
    t,
    errorText,
    setPanelError,
    settleSave,
    setNotes,
    bumpEditorEpoch,
    closeNoteFind,
    focusEditor,
    jumpToBacklink,
  } = options;

  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [flags, setFlags] = useState<NoteSearchFlags>({
    caseSensitive: false,
    wholeWord: false,
    regex: false,
  });
  const [hits, setHits] = useState<NoteSearchHit[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searched, setSearched] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);
  /* 只认最后一次发起的搜索。用户改条件重搜时,前一次的 promise 可能后回来
     ——不带序号就会把旧结果盖在新结果上,而列表看不出这一点。 */
  const searchRunRef = useRef(0);

  /** 空串是合法的替换目标(= 删掉命中),所以不能用空串当"没填"。 */
  const [replaceValue, setReplaceValue] = useState("");
  /** null = 还没预览过。全库替换必须先预览再落笔,不给"直接全替换"的入口。 */
  const [preview, setPreview] = useState<VaultReplacePreview | null>(null);
  /** 用户取消勾选的文件(预览给的路径口径)。 */
  const [excluded, setExcluded] = useState<ReadonlySet<string>>(new Set());
  const [busy, setBusy] = useState(false);
  /** 上一次落笔的结果。用来显示"改了 N 处 / 跳过 M 处"。 */
  const [summary, setSummary] = useState<VaultReplaceSummary | null>(null);
  const replaceRunRef = useRef(0);

  useEffect(() => {
    if (!open) return;
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [open]);

  /**
   * 打开全库搜索。不要求先有 activeNote —— 空库/没选中笔记时正是最需要它的时候。
   * 但要有 vault,不然没有可搜的根。
   */
  const openSheet = () => {
    if (!vault) {
      setPanelError(t("notebook.vaultUnavailable"));
      return;
    }
    closeNoteFind();
    setOpen(true);
  };

  const closeSheet = () => {
    setOpen(false);
    /* 结果留着不清:关掉再开常常是"我刚才搜的那批还想再点一条"。改条件会重搜,
       所以留着的结果不会变成过期数据被误当成新搜的。 */
    focusEditor();
  };

  const dismiss = () => {
    setOpen(false);
  };

  const runSearch = () => {
    const trimmed = query.trim();
    if (!vault || !trimmed) {
      // 空查询不发请求,但要把上一批结果清掉 —— 留着会像是"清空了还搜得到"。
      setHits([]);
      setError(null);
      setSearched(false);
      return;
    }
    const run = searchRunRef.current + 1;
    searchRunRef.current = run;
    setLoading(true);
    setError(null);
    void searchNotesText(vault, trimmed, noteSearchOptions(flags))
      .then((matches) => {
        if (searchRunRef.current !== run) return;
        setHits(matches);
        setSearched(true);
      })
      .catch((err: unknown) => {
        if (searchRunRef.current !== run) return;
        // 后端的错要原样给出来:正则不合法时它带着位置信息,比我们自己编一句有用。
        setError(errorText(err));
        /* 上一批结果要清掉:报错时列表还留着旧命中的话,状态行说"出错了"而下面列着
           三条结果,用户没法判断哪个是真的。这里**不**动 `searched` —— 状态行里
           error 优先于"没有结果",而下一次搜索无论成败都会重设它。 */
        setHits([]);
      })
      .finally(() => {
        if (searchRunRef.current !== run) return;
        setLoading(false);
      });
  };

  /** 点一条命中:关面板,再走反链那条跳转路径(它按文件行号换算正文偏移)。 */
  const openHit = (hit: NoteSearchHit) => {
    const noteId = resolveHitNoteId(
      hit.path,
      notes.map((note) => note.id),
      vault ?? "",
    );
    if (!noteId) {
      /* 对不上就明说。静默 return 是最坏的选择:用户点了没反应,只会以为面板坏了,
         而真实原因(文件刚被移走/删掉,或列表还没刷新)他无从得知。 */
      setError(t("notebook.globalSearchUnresolved"));
      return;
    }
    setOpen(false);
    jumpToBacklink(noteId, hit.line);
  };

  /**
   * 全库替换的预览。
   *
   * 先把**所有**挂起 / 在飞的保存等落完,再让后端读盘 —— 后端算出的偏移和乐观锁比对的
   * 都是磁盘上的内容,内存里改了没落盘时两边不是同一份正文,预览会按旧文本给出偏移。
   * 这和回滚前必须 `settleSave` 是同一个道理。
   */
  const runPreview = () => {
    const trimmed = query.trim();
    if (!vault || !trimmed) {
      setPreview(null);
      setSummary(null);
      return;
    }
    const run = replaceRunRef.current + 1;
    replaceRunRef.current = run;
    setBusy(true);
    setError(null);
    setSummary(null);
    void (async () => {
      try {
        await Promise.all(notes.map((note) => settleSave(note.id)));
        const next = await previewVaultReplace(
          vault,
          trimmed,
          replaceValue,
          vaultReplaceOptions(flags, NOTE_SEARCH_LIMIT),
        );
        if (replaceRunRef.current !== run) return;
        setPreview(next);
        // 重新预览就清掉上一次的勾选:文件集合可能已经变了,留着会按旧路径排除。
        setExcluded(new Set());
      } catch (err) {
        if (replaceRunRef.current !== run) return;
        setError(errorText(err));
        setPreview(null);
      } finally {
        if (replaceRunRef.current === run) setBusy(false);
      }
    })();
  };

  /**
   * 落笔,然后把改过的笔记从磁盘重读回内存。
   *
   * 重读是必须的:替换是后端直接改文件,内存里那份还是替换前的正文。不重读的话下一次
   * 自动保存会把旧正文整篇写回去,替换静默消失 —— 而且替换可能命中 frontmatter(标题
   * 就在里面),所以要走 `splitNote` 重新拆一遍,不能只把 `body` 换掉。
   */
  const apply = () => {
    const current = preview;
    if (!vault || !current) return;
    const replacements = buildReplacements(current, excluded);
    if (replacements.length === 0) return;
    const touched = [...new Set(replacements.map((entry) => entry.path))];
    setBusy(true);
    setError(null);
    void (async () => {
      try {
        // 落笔前再等一次:预览之后用户可能又编辑过(面板盖住编辑器,但命令面板等入口
        // 仍能改内容),那些改动必须先落盘,否则乐观锁比的还是旧文本。
        await Promise.all(notes.map((note) => settleSave(note.id)));
        const nextSummary = await applyVaultReplacements(vault, replacements);
        const noteIds = resolvePreviewNoteIds(
          current,
          notes.map((note) => note.id),
          vault,
        );
        const reloaded = await Promise.all(
          touched.map(async (path) => {
            const noteId = noteIds.get(path);
            if (!noteId) return null;
            try {
              const opened = await peekNote(noteId);
              return { noteId, opened };
            } catch {
              /* 单篇读失败不该让整次替换看起来失败 —— 文件已经改好了。跳过它,那条
                 笔记的内存副本仍是旧的,而它的 `sig` 也旧,下次保存会被乐观锁挡下。 */
              return null;
            }
          }),
        );
        setNotes((list) =>
          list.map((note) => {
            const hit = reloaded.find((entry) => entry?.noteId === note.id);
            if (!hit) return note;
            const { frontmatter, body } = splitNote(hit.opened.content);
            return {
              ...note,
              title: deriveTitle(hit.opened.content, note.id),
              body,
              frontmatter,
              sig: hit.opened.sig,
              updatedAt: hit.opened.sig.mtimeMs,
            };
          }),
        );
        /* 当前这篇的正文被换掉了,编辑器必须重建:受控 value 变了但 CodeMirror 里
           可能有挂起的更新闭包,它捕获的是替换前的 value(见 `editorEpoch` 的注释)。 */
        if (activeNoteId && touched.some((path) => noteIds.get(path) === activeNoteId)) {
          bumpEditorEpoch();
        }
        setSummary(nextSummary);
        // 预览已经过期(偏移全变了)。清掉,逼用户重新预览再改第二轮。
        setPreview(null);
        setExcluded(new Set());
        // 命中列表也过期了:那批 lineText 是替换前的。
        setHits([]);
        setSearched(false);
      } catch (err) {
        setError(errorText(err));
      } finally {
        setBusy(false);
      }
    })();
  };

  /** 勾掉/勾回预览里的一个文件。落笔时被勾掉的文件一条都不提交。 */
  const toggleFile = (path: string) => {
    setExcluded((set) => {
      const next = new Set(set);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  return {
    open,
    query,
    setQuery,
    flags,
    setFlags,
    hits,
    loading,
    error,
    capped: hits.length >= NOTE_SEARCH_LIMIT,
    searched,
    inputRef,
    openSheet,
    closeSheet,
    dismiss,
    runSearch,
    openHit,
    replaceValue,
    setReplaceValue,
    preview,
    excluded,
    toggleFile,
    busy,
    summary,
    canPreview: query.trim().length > 0,
    runPreview,
    apply,
  };
}
