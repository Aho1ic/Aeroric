/**
 * 把五份全库扫描结果折成各视图要显示的东西:反链、未链接提及、标签、字段、任务收集箱,
 * 外加引用图谱。
 *
 * 收在一起的理由是**标题口径**:五处里有四处都要把「vault 相对路径」换成「用户看到的
 * 标题」,而这四处必须给出同一个答案 —— 不一致的话同一篇笔记在标签档里显示真标题、
 * 在字段浏览器里显示文件名,用户会以为点进去会到两个不同的地方。原来那份口径是四段
 * 一模一样的表达式抄在四个 `useMemo` 里,靠注释("标题口径和标签档共用同一条")互相
 * 提醒;这里收成一个 `titleOf`,口径只有一份。
 *
 * 不变量:
 *
 * 1. **标题口径只有一份**(`titleOf`)。索引里有就用索引里那份(frontmatter 里的真标题),
 *    没有的退回路径 stem。退回而不是显示空:扫描比标题索引快,刚建的笔记会有一小段
 *    时间不在索引里,那时显示文件名总比显示空白好。
 *
 * 2. **反链按当前笔记筛,标签 / 字段 / 任务不筛**。那正是它们的分工:反链讲"谁指向了
 *    这一篇",另外三档讲"全库有什么"。所以只有反链要 `activeNoteId`。
 *
 * 3. **反链跟着链接索引重算**。同一批扫描结果在标题改过之后指向的可能已经不是这一篇了
 *    (`[[某标题]]` 解析成哪一篇由索引决定),所以 `linkIndex` 进依赖。
 *
 * 4. **图谱关着时不折**。整库 BFS + 布局不该在没人看的时候每次重扫都跑一遍;关着时
 *    返回同一个 `EMPTY_GRAPH` 常量,而不是每次新建一个空对象(它进 memo 的返回值)。
 *
 * 5. **「整个库」是没有焦点,不是跳数很大**。模型层只要有焦点,连不到它的笔记就一律
 *    算在范围外(那是"焦点这一团"的语义,不管跳数给多大),而那些互不相连的孤岛恰恰
 *    是切到「整个库」想看的东西 —— 所以 `DEPTH_ALL` 时连 `focusPath` 一起去掉。当前
 *    这篇仍然会高亮:那走 sheet 的 `focusPath` prop,和这里无关。
 */
import { useMemo } from "react";

import {
  collectBacklinks,
  countBacklinks,
  type BacklinkGroup,
  type NoteLinkSource,
} from "./noteBacklinks";
import { collectFields, type FieldEntry, type NoteFieldSource } from "./noteFields";
import { buildNoteGraph, type NoteGraph } from "./noteGraph";
import {
  collectMentions,
  countConfident,
  countMentions,
  type MentionGroup,
  type MentionSource,
} from "./noteMentions";
import type { VaultLinkIndex } from "./noteLinks";
import {
  collectTags,
  countTagRefs,
  filterTags,
  type NoteTagSource,
  type TagEntry,
} from "./noteTags";
import { collectInboxTasks, type InboxTask, type NoteTaskSource } from "./noteTaskInbox";
import { DEPTH_ALL } from "./NoteGraphSheet";

/** 图谱关着时给出的空图。见不变量 4。 */
const EMPTY_GRAPH: NoteGraph = { nodes: [], edges: [], deadLinks: 0, orphans: 0, hidden: 0 };

export type VaultAggregatesOptions = {
  /** 当前笔记的 vault 相对路径。反链筛它,图谱以它为焦点。 */
  activeNoteId: string | null;
  /** 标题索引:vault 相对路径 → frontmatter 里的真标题。 */
  indexedTitles: Map<string, string>;
  /** 链接索引。反链和图谱共用同一份,见 `noteGraph` 的模块注释。 */
  linkIndex: VaultLinkIndex;
  /** `vaultLinks` 的扫描结果。反链和图谱都读它。 */
  links: readonly NoteLinkSource[];
  /** `vaultMentions` 的扫描结果。 */
  mentions: readonly MentionSource[];
  /** `vaultTags` 的扫描结果。 */
  tags: readonly NoteTagSource[];
  /** `vaultFields` 的扫描结果。 */
  fields: readonly NoteFieldSource[];
  /** `vaultTasks` 的扫描结果。 */
  tasks: readonly NoteTaskSource[];
  /** 标签档的过滤词。 */
  tagQuery: string;
  /** 图谱那一档开着没有。见不变量 4。 */
  graphOpen: boolean;
  /** 图谱跳数,`DEPTH_ALL` = 整个库。见不变量 5。 */
  graphDepth: number;
};

export type VaultAggregates = {
  /** 指向当前这一篇的引用,按来源笔记分组。 */
  backlinkGroups: BacklinkGroup[];
  backlinkCount: number;
  /** 提到了当前这一篇的标题但没链过来的地方。 */
  mentionGroups: MentionGroup[];
  mentionCount: number;
  /** 其中判定为"确实是在说这一篇"的那些。 */
  mentionConfidentCount: number;
  /** 全库标签被引用的总次数。 */
  tagRefCount: number;
  /** `tagQuery` 过滤之后的标签。未过滤的那份不外露 —— 面板只画过滤后的列表。 */
  visibleTags: TagEntry[];
  /** 全库 frontmatter 字段。 */
  fieldEntries: FieldEntry[];
  /** 全库任务。 */
  inboxTasks: InboxTask[];
  /** 引用图谱。关着时是空图。 */
  noteGraph: NoteGraph;
};

export function useVaultAggregates({
  activeNoteId,
  indexedTitles,
  linkIndex,
  links,
  mentions,
  tags,
  fields,
  tasks,
  tagQuery,
  graphOpen,
  graphDepth,
}: VaultAggregatesOptions): VaultAggregates {
  /* 见不变量 1。跟着索引变 —— 改标题之后四档要一起换成新标题。 */
  const titleOf = useMemo(
    () =>
      (path: string): string =>
        indexedTitles.get(path) ?? path.replace(/^.*[/\\]/, "").replace(/\.md$/i, ""),
    [indexedTitles],
  );

  const backlinkGroups = useMemo(
    () => (activeNoteId ? collectBacklinks(links, linkIndex, activeNoteId) : []),
    [activeNoteId, linkIndex, links],
  );

  const mentionGroups = useMemo(() => collectMentions(mentions, titleOf), [mentions, titleOf]);
  const tagEntries = useMemo(() => collectTags(tags, titleOf), [tags, titleOf]);
  const fieldEntries = useMemo(() => collectFields(fields, titleOf), [fields, titleOf]);
  const inboxTasks = useMemo(() => collectInboxTasks(tasks, titleOf), [tasks, titleOf]);

  const noteGraph = useMemo(
    () =>
      graphOpen
        ? buildNoteGraph(
            links,
            linkIndex,
            // 见不变量 5。
            graphDepth === DEPTH_ALL ? {} : { focusPath: activeNoteId, maxDepth: graphDepth },
          )
        : EMPTY_GRAPH,
    [graphOpen, links, linkIndex, activeNoteId, graphDepth],
  );

  const visibleTags = useMemo(() => filterTags(tagEntries, tagQuery), [tagEntries, tagQuery]);

  return {
    backlinkGroups,
    backlinkCount: countBacklinks(backlinkGroups),
    mentionGroups,
    mentionCount: countMentions(mentionGroups),
    mentionConfidentCount: countConfident(mentionGroups),
    tagRefCount: countTagRefs(tagEntries),
    visibleTags,
    fieldEntries,
    inboxTasks,
    noteGraph,
  };
}
