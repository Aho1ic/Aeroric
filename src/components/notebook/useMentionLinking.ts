/* 把若干处未链接提及包成 `[[..]]`。
 *
 * 这是随手记里唯一一处**批量改别人文件**的操作,所以四件事都不能省:
 *
 * - **`linking` 是并发闸门**。重复提交会让第二次的每一处都报 `alreadyLinked`;期间也不
 *   该重扫,扫到的是改了一半的状态。
 *
 * - **重扫提及**:改过的那几处已经是链接了,留在列表里点第二次只会报 `alreadyLinked`。
 *
 * - **重扫链接**:刚写进去的是真链接,反链档和图谱读的是同一份 `linkScan`。
 *
 * - **让改过的、已读入内存的笔记重新读盘**:它们在内存里的正文还是旧的。不重读的话
 *   用户切到那个 tab 看到的是没有链接的旧正文,而下一次自动保存会拿旧基线去比 ——
 *   后端会报冲突(不会静默覆盖,见 `save_note`),但用户看到的是一次莫名的冲突提示。
 *   正在保存 / 待保存的跳过:那些有用户还没落盘的编辑,清掉 `loaded` 会把它们丢掉。 */
import { useState } from "react";
import { linkVaultMentions } from "./notebookApi";
import type { MentionLinkReport, MentionTarget } from "./noteMentions";
import type { NotebookNote } from "./notebookStore";
import type { NoteSaveState } from "./useNoteAutosave";

export type MentionLinkingOptions = {
  vault: string | null;
  errorText: (error: unknown) => string;
  /** 每篇的保存状态。`pending` / `saving` 的不清 `loaded`。 */
  saveStates: Record<string, NoteSaveState>;
  setNotes: (update: (current: NotebookNote[]) => NotebookNote[]) => void;
  refreshMentions: () => void;
  refreshLinks: () => void;
};

export type MentionLinkingApi = {
  /** 正在写盘。按钮要禁掉,也不该重扫。 */
  linking: boolean;
  /** 上一次链接的结果:改了几处、跳过几处、几篇没成。 */
  report: MentionLinkReport | null;
  /** 整次请求失败(vault 读不动、路径越界)。单篇失败在 `report.failed` 里,不走这里。 */
  error: string | null;
  link: (targets: MentionTarget[]) => void;
};

export function useMentionLinking(options: MentionLinkingOptions): MentionLinkingApi {
  const { vault, errorText, saveStates, setNotes, refreshMentions, refreshLinks } = options;

  const [linking, setLinking] = useState(false);
  const [report, setReport] = useState<MentionLinkReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  const link = (targets: MentionTarget[]) => {
    if (!vault || !targets.length || linking) return;
    setLinking(true);
    setReport(null);
    setError(null);
    void (async () => {
      try {
        const next = await linkVaultMentions(vault, targets);
        setReport(next);
        const rewritten = new Set(next.changed.map((change) => change.path));
        if (rewritten.size) {
          setNotes((current) =>
            current.map((note) => {
              if (!rewritten.has(note.id) || !note.loaded) return note;
              const state = saveStates[note.id];
              if (state === "pending" || state === "saving") return note;
              /* `sig` 一起清掉:留着旧指纹会让下一次保存拿它当基线,而那个基线已经
                 不是盘上的了。清掉之后按需读入那一路会重新登记。 */
              return { ...note, loaded: false, body: "", sig: null };
            }),
          );
        }
        refreshMentions();
        refreshLinks();
      } catch (err) {
        setError(errorText(err));
      } finally {
        setLinking(false);
      }
    })();
  };

  return { linking, report, error, link };
}
