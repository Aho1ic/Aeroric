/* 侧栏那一列:大纲 / 反链 / 标签三档。
 *
 * 三档共用同一列(190px),而不是各占一列:面板在项目视图里常常只有 400px 宽,再切出去
 * 一列正文就没地方了。所以这一层是"档选择器 + 当前那一档"。
 *
 * 边框和底色画在这一层,不由三个子面板各自再画一遍 —— 那样会在切换处出现双线。
 */
import { NoteOutlinePanel } from "./NoteOutlinePanel";
import { NoteBacklinksPanel } from "./NoteBacklinksPanel";
import { NoteMentionsPanel } from "./NoteMentionsPanel";
import { NoteTagsPanel } from "./NoteTagsPanel";
import type { BacklinkGroup } from "./noteBacklinks";
import type { MentionGroup, MentionHit, MentionLinkReport } from "./noteMentions";
import type { TagEntry } from "./noteTags";
import type { TagRenameDialogState } from "./TagRenameDialog";
import type { OutlineItem } from "./noteOutline";
import type { Translate } from "./noteExportRun";

export type NoteSideTab = "outline" | "backlinks" | "tags";

export type NoteSideColumnProps = {
  tab: NoteSideTab;
  onTabChange: (next: NoteSideTab) => void;
  /** 大纲档。 */
  /** 不收 `readonly` —— `NoteOutlinePanel` 的 `items` 是可变数组。 */
  outline: OutlineItem[];
  onJumpHeading: (item: OutlineItem) => void;
  onReorderHeading: (sourceIndex: number, targetIndex: number) => void;
  /** 反链档。`linksScanned` 决定标签上要不要显示计数。 */
  linksScanned: boolean;
  backlinkGroups: BacklinkGroup[];
  backlinkCount: number;
  backlinksLoading: boolean;
  backlinksError: string | null;
  onRefreshBacklinks: () => void;
  /** 未链接提及(和反链同一档,画在它下面)。 */
  mentionGroups: MentionGroup[];
  mentionCount: number;
  mentionConfidentCount: number;
  mentionsLoading: boolean;
  mentionLinking: boolean;
  mentionsError: string | null;
  mentionReport: MentionLinkReport | null;
  onLinkMention: (path: string, hit: MentionHit) => void;
  onLinkAllMentions: () => void;
  onRefreshMentions: () => void;
  /** 标签档。 */
  tags: TagEntry[];
  tagRefCount: number;
  tagsLoading: boolean;
  tagsError: string | null;
  tagQuery: string;
  onTagQueryChange: (next: string) => void;
  openTag: string | null;
  onToggleTag: (key: string) => void;
  onRenameTag: (state: TagRenameDialogState) => void;
  onRefreshTags: () => void;
  /** 三档共用:跳到"某篇的某一行"。 */
  onJump: (path: string, line: number) => void;
  t: Translate;
};

export function NoteSideColumn(props: NoteSideColumnProps) {
  const { tab, onTabChange, t } = props;
  const tabs = [
    ["outline", t("notebook.outline")],
    [
      "backlinks",
      /* 计数直接写在标签上:反链的价值在于"有没有、有几条",要点开才知道的话这一档大部分
         时候是白开的。没扫过时不显示 0 —— 那会看起来像"确实没有"。 */
      props.linksScanned
        ? t("notebook.backlinksWithCount", { count: String(props.backlinkCount) })
        : t("notebook.backlinks"),
    ],
    /* 标签这一档不带计数:它数的是全库,和当前笔记无关,而三个按钮分 190px 的时候多两个字
       就会把另外两档挤成省略号。处数写在档内的标题行里。 */
    ["tags", t("notebook.tags")],
  ] as const;

  return (
    <div
      style={{
        minWidth: 0,
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
        borderLeft: "1px solid var(--border-dim)",
        background: "var(--bg-sidebar)",
      }}
    >
      <div
        role="group"
        aria-label={t("notebook.sidePanel")}
        style={{ display: "flex", padding: "6px 6px 0" }}
      >
        {tabs.map(([value, label], index, all) => (
          <button
            key={value}
            type="button"
            aria-pressed={tab === value}
            onClick={() => onTabChange(value)}
            style={{
              flex: 1,
              minWidth: 0,
              height: 22,
              border: "1px solid var(--border-medium)",
              borderRadius:
                index === 0 ? "5px 0 0 5px" : index === all.length - 1 ? "0 5px 5px 0" : 0,
              borderLeftWidth: index === 0 ? 1 : 0,
              background: tab === value ? "var(--control-active-bg)" : "var(--bg-card)",
              color: tab === value ? "var(--control-active-fg)" : "var(--text-primary)",
              cursor: "pointer",
              padding: "0 4px",
              fontSize: 10,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {label}
          </button>
        ))}
      </div>
      {tab === "outline" ? (
        <NoteOutlinePanel
          items={props.outline}
          onJump={props.onJumpHeading}
          onReorder={props.onReorderHeading}
          t={t}
        />
      ) : tab === "backlinks" ? (
        /* 已链接在上、未链接在下:两者是同一个问题的两面("谁在说我"),而"已经链好的"是
           既成事实、"还没链的"是待办 —— 待办放在下面,翻到底就是可以动手的那一段。
           两块各自可滚(各有 `overflow: auto`),所以提及很多时不会把反链整个顶出视口。 */
        <div style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
          <NoteBacklinksPanel
            groups={props.backlinkGroups}
            count={props.backlinkCount}
            loading={props.backlinksLoading}
            error={props.backlinksError}
            onJump={props.onJump}
            onRefresh={props.onRefreshBacklinks}
            t={t}
          />
          <NoteMentionsPanel
            groups={props.mentionGroups}
            count={props.mentionCount}
            confidentCount={props.mentionConfidentCount}
            loading={props.mentionsLoading}
            linking={props.mentionLinking}
            error={props.mentionsError}
            report={props.mentionReport}
            onJump={props.onJump}
            onLink={props.onLinkMention}
            onLinkAll={props.onLinkAllMentions}
            onRefresh={props.onRefreshMentions}
            t={t}
          />
        </div>
      ) : (
        <NoteTagsPanel
          entries={props.tags}
          count={props.tagRefCount}
          loading={props.tagsLoading}
          error={props.tagsError}
          query={props.tagQuery}
          onQueryChange={props.onTagQueryChange}
          openKey={props.openTag}
          onToggle={props.onToggleTag}
          onJump={props.onJump}
          onRename={(entry, anchor) =>
            props.onRenameTag({
              x: anchor.x,
              y: anchor.y,
              key: entry.key,
              label: entry.label,
              count: entry.count,
            })
          }
          onRefresh={props.onRefreshTags}
          t={t}
        />
      )}
    </div>
  );
}
