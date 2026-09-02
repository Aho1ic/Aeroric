/* 导出窗的状态与动作。
 *
 * 抽成 hook 而不是留在 NotebookPanel(那个文件 4052 行)。这里有四件必须一处做对的事:
 *
 * 1. **正文优先取内存里那份。** 那才是用户眼下看到的内容,含还没落盘的编辑。只有没读入过的
 *    笔记(列表只拿了元数据)才回落到读盘 —— 而不是当成「没有可导出的笔记」:用户从列表里点
 *    一条就导出,那篇很可能还没被读进来。
 *
 * 2. **`exportBusy` 既是进度指示也是并发闸门。** 后端在读盘写盘,并发两条只会互相拖慢,所以
 *    在跑的时候直接忽略新的请求。
 *
 * 3. **解禁写在 `finally` 里,不写在每条分支末尾。** 抛出来的时候按钮也必须解禁,否则面板永久
 *    卡在「导出中」。同一处还要清掉进度和 abort 句柄。
 *
 * 4. **只有整库站点导出可取消。** 单篇导出没有可中断的中间态(要么写完要么没写),给它一个取消
 *    按钮只会让人以为按了有用。`abortRef` 因此只在站点那条路上装。
 *
 * 读盘失败当导出失败报,不退化成「没有可导出的笔记」—— 后者会让用户以为是没选中笔记,而真正的
 * 原因(权限、文件被删)就丢了。
 */

import { useRef, useState } from "react";

import { defaultExportDeps, pickExportDir } from "./noteExport";
import {
  runSingleExport,
  runSiteExportAction,
  vaultSiteTitle,
  type ExportAction,
  type ExportRunOutcome,
  type Translate,
} from "./noteExportRun";
import { defaultSiteExportDeps, type SiteExportProgress } from "./noteSiteExportRun";
import type { NotebookNote } from "./notebookStore";

export type NoteExportOptions = {
  /** 当前这篇。`null` 时单篇导出报「没有可导出的笔记」。 */
  activeNote: NotebookNote | null;
  /** 站点导出要按全库出页,所以要整份列表而不只是当前这篇。 */
  notes: readonly NotebookNote[];
  /** 站点导出的落点根;`null` 时那条路直接报错。 */
  vault: string | null;
  t: Translate;
  language: string;
  /** 与面板同一份错误文案口径,靠参数传(与 `useVaultScan` 一致)。 */
  errorText: (error: unknown) => string;
  /** 读一篇的正文,不登记「打开」。没读入过的笔记走这条。 */
  peek: (path: string) => Promise<{ content: string }>;
};

export type NoteExportApi = {
  open: boolean;
  /** 正在跑的那条动作;`null` = 空闲。同时是并发闸门。 */
  busy: ExportAction | null;
  /** 只有站点导出会产进度。 */
  progress: SiteExportProgress | null;
  notice: string | null;
  error: string | null;
  /** 开窗时清掉上一次的结果文案 —— 那是上一次的事了。 */
  openSheet: () => void;
  closeSheet: () => void;
  run: (action: ExportAction) => void;
  /** 取消正在跑的站点导出。不在跑或不是站点导出时是空操作。 */
  cancelSite: () => void;
};

export function useNoteExport(options: NoteExportOptions): NoteExportApi {
  const { activeNote, notes, vault, t, language, errorText, peek } = options;
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState<ExportAction | null>(null);
  const [progress, setProgress] = useState<SiteExportProgress | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const abortRef = useRef<AbortController | null>(null);

  /**
   * 拼出要导出的那篇笔记。
   *
   * 正文优先取内存里的 —— 那才是用户眼下看到的内容,含还没落盘的编辑。没读入过的
   * 笔记(列表只拿元数据)才回落到读盘,而不是当成"没有可导出的笔记":用户从列表里
   * 点一条就导出,笔记很可能还没被读进来。
   */
  const buildSource = async () => {
    if (!activeNote) return null;
    const body = activeNote.loaded ? activeNote.body : (await peek(activeNote.id)).content;
    return {
      path: activeNote.id,
      title: activeNote.title || t("notebook.untitled"),
      body,
    };
  };

  const runSingleFlow = async (action: ExportAction): Promise<ExportRunOutcome> => {
    let source: Awaited<ReturnType<typeof buildSource>>;
    try {
      source = await buildSource();
    } catch (err) {
      // 读盘失败要当导出失败报,不能退化成"没有可导出的笔记" —— 后者会让用户以为
      // 是没选中笔记,而真正的原因(权限、文件被删)就丢了。
      return { notice: null, error: t("notebook.exportFailed", { message: errorText(err) }) };
    }
    return runSingleExport(action, source, defaultExportDeps(language), t);
  };

  const runSiteFlow = async () => {
    if (!vault) return { notice: null, error: t("notebook.exportNoNote") };
    const controller = new AbortController();
    abortRef.current = controller;
    return runSiteExportAction(
      {
        vault,
        // 站点标题用 vault 目录名:它就是用户给这个库起的名字。
        siteTitle: vaultSiteTitle(vault),
        notes: notes.map((note) => ({
          path: note.id,
          title: note.title || t("notebook.untitled"),
        })),
        pickDir: () => pickExportDir(t("notebook.exportSitePickDir")),
        deps: defaultSiteExportDeps(
          (count) => t("notebook.exportSitePageCount", { count: String(count) }),
          t("notebook.exportSiteEmbedPrefix"),
        ),
      },
      t,
      setProgress,
      controller.signal,
    );
  };

  /** 跑一条导出动作。面板的 state 全在这里收口。 */
  const run = (action: ExportAction) => {
    // 已经在跑就忽略:后端在读盘和写盘,并发两条只会互相拖慢。
    if (busy) return;
    setNotice(null);
    setError(null);
    setBusy(action);
    void (async () => {
      let outcome: ExportRunOutcome;
      try {
        outcome = action === "site" ? await runSiteFlow() : await runSingleFlow(action);
      } finally {
        // finally 而不是每条分支末尾:抛出来的时候按钮也必须解禁,否则面板永久卡住。
        setBusy(null);
        setProgress(null);
        abortRef.current = null;
      }
      setNotice(outcome.notice);
      setError(outcome.error);
    })();
  };

  return {
    open,
    busy,
    progress,
    notice,
    error,
    openSheet: () => {
      setNotice(null);
      setError(null);
      setOpen(true);
    },
    closeSheet: () => setOpen(false),
    run,
    cancelSite: () => abortRef.current?.abort(),
  };
}
