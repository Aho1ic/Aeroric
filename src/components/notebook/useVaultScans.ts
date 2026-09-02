/**
 * 全库扫描:一共五次,以及"什么时候扫、谁和谁共用一次"。
 *
 * 收在一处的理由是**这几次扫描之间的共用关系是成对的约束** —— 反链档和图谱要读同一份
 * 链接、标签档和 `#` 补全要读同一份标签。写在两处的话,"反链里有这条、图里没有"就成了
 * 可能,而那种偏差没人会往取数上想。
 *
 * 不变量:
 *
 * 1. **只在那一档可见时扫**。每一次都读全库每个文件的全文,是整个面板里最贵的 IO,
 *    而绝大多数时候用户根本没打开侧栏。这条(以及"报错留住旧结果""换笔记不重扫")
 *    由 `useVaultScan` 保证,五次共用同一份实现。
 *
 * 2. **刻意不合并成一次扫描**。侧栏三档共用那一列(互斥),合起来只会让每次多做一半
 *    没人看的提取。共享的是遍历那半边 —— 在 Rust 的 `vault_walk` 里。
 *
 * 3. **反链档和图谱共用 `linkScan`**;**标签档和 `#` 补全共用 `tagScan`**。所以这两个的
 *    `enabled` 都是"或"。同一份数据扫两遍会让两个消费者的刷新时机错开。
 *
 * 4. **提及那一路要 `resetKey`**。它的扫描参数里有当前笔记的名字,换笔记之后旧结果讲的
 *    是另一篇 —— 不清空的话新笔记的标题下面会先显示上一篇的提及,而那些条目点下去会
 *    改错地方的正文。
 *
 * 5. **`vaultTagRefs` 要 memo**。它进的是触发菜单里 `completionSource` 的依赖,每次渲染
 *    换一个新数组会让那个补全每次都失效。
 *
 * 6. **`tagCompletionUsed` 由外面拿着**。`tagScan` 的 `enabled` 要读它,而产生它的触发
 *    菜单又要读 `tagScan` 的结果 —— 搬进来就成环,所以它作为入参进来。
 */
import { useCallback, useMemo } from "react";

import type { NoteLinkSource } from "./noteBacklinks";
import { vaultFields, vaultLinks, vaultMentions, vaultTags, vaultTasks } from "./notebookApi";
import type { NotebookNote } from "./notebookStore";
import type { NoteFieldSource } from "./noteFields";
import { mentionNamesOf, type MentionSource } from "./noteMentions";
import type { NoteTagSource } from "./noteTags";
import type { NoteTaskSource } from "./noteTaskInbox";
import { useVaultScan, type VaultScan } from "./useVaultScan";

export type VaultScansOptions = {
  vault: string | null;
  /** 当前这一篇。提及那一路要它的路径和标题,见不变量 4。 */
  activeNote: NotebookNote | null;
  /** 「路径 → frontmatter 里的真实标题」,提及要按它算出该找哪几个名字。 */
  indexedTitles: ReadonlyMap<string, string>;
  /** 侧栏展开了没有,以及当前是哪一档。三档的 `enabled` 都要它们,见不变量 1。 */
  outlineOpen: boolean;
  sideTab: "outline" | "backlinks" | "tags";
  /** 图谱那一档开着没有。见不变量 3。 */
  graphOpen: boolean;
  fieldsOpen: boolean;
  taskInboxOpen: boolean;
  /** 用过 `#` 补全没有。见不变量 3、6。 */
  tagCompletionUsed: boolean;
  errorText: (error: unknown) => string;
};

export type VaultScansApi = {
  /** 全库链接。反链档和图谱共用,见不变量 3。 */
  linkScan: VaultScan<NoteLinkSource>;
  /** 全库标签。标签档和 `#` 补全共用,见不变量 3。 */
  tagScan: VaultScan<NoteTagSource>;
  fieldScan: VaultScan<NoteFieldSource>;
  taskScan: VaultScan<NoteTaskSource>;
  /** 当前这一篇的未链接提及。见不变量 4。 */
  mentionScan: VaultScan<MentionSource>;
  /** 补全能用的全库标签原文。见不变量 5。 */
  vaultTagRefs: string[];
};

export function useVaultScans({
  vault,
  activeNote,
  indexedTitles,
  outlineOpen,
  sideTab,
  graphOpen,
  fieldsOpen,
  taskInboxOpen,
  tagCompletionUsed,
  errorText,
}: VaultScansOptions): VaultScansApi {
  const linkScan = useVaultScan(
    vault,
    (outlineOpen && sideTab === "backlinks") || graphOpen,
    vaultLinks,
    errorText,
  );
  const tagScan = useVaultScan(
    vault,
    (outlineOpen && sideTab === "tags") || tagCompletionUsed,
    vaultTags,
    errorText,
  );
  // 见不变量 5。
  const vaultTagRefs = useMemo(
    () => tagScan.data.flatMap((source) => source.tags.map((ref) => ref.raw)),
    [tagScan.data],
  );
  /* 字段浏览器和任务收集箱的 `enabled` 是"sheet 开着"而不是某一档可见 —— 它们不在侧栏里
     (三档已经占满那 190px,见 `NoteFieldsSheet` 的模块注释)。 */
  const fieldScan = useVaultScan(vault, fieldsOpen, vaultFields, errorText);
  const taskScan = useVaultScan(vault, taskInboxOpen, vaultTasks, errorText);

  /* 提及那一路和上面几档有两处不同,都来自"它的结果只对当前这一篇成立":
     - `scan` 是闭在当前笔记名字上的闭包,所以换笔记 / 改标题会自然重扫;
     - 传 `resetKey`,让换笔记时**清空**而不只是重扫,见不变量 4。 */
  const mentionNames = useMemo(
    () =>
      activeNote
        ? mentionNamesOf({ path: activeNote.id, title: activeNote.title }, indexedTitles)
        : [],
    [activeNote, indexedTitles],
  );
  const mentionNamesKey = mentionNames.join("\u0000");
  const scanMentions = useCallback(
    (target: string): Promise<MentionSource[]> =>
      activeNote && mentionNames.length
        ? vaultMentions(target, activeNote.id, mentionNames)
        : Promise.resolve([]),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 按名字的内容而不是数组身份,免得每次渲染都重扫
    [activeNote?.id, mentionNamesKey],
  );
  const mentionScan = useVaultScan(
    vault,
    outlineOpen && sideTab === "backlinks",
    scanMentions,
    errorText,
    activeNote?.id ?? null,
  );

  return { linkScan, tagScan, fieldScan, taskScan, mentionScan, vaultTagRefs };
}
