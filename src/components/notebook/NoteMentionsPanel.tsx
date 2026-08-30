/* 侧栏「反链」档下半部分的「未链接的提及」:提到了当前这篇、却没写成 `[[链接]]` 的
 * 地方,以及把它们包成链接。
 *
 * 和反链同一档而不是新开一个 tab:两者回答的是同一个问题的两面("谁在说我"),
 * 而侧栏那一列只有 190px,再加一档会把标签挤没。上面是已链接、下面是未链接。
 *
 * 这是随手记里**唯一会批量改别人文件**的只读视图,所以三件事写在界面上:
 *
 * - 「全部链接」只动 confident 的那些,按钮上的数就是它 —— 不是总处数。
 * - ambiguous 的单独一段,带一个说明,逐条点。
 * - 改完报的是**处数**(后端算的 `linked`),不是文件数。
 *
 * 扫描由父面板发起(它知道 vault、当前笔记的名字、什么时候该重扫),这个组件只负责画。
 */

import { Link2, Loader2, RefreshCw } from "lucide-react";
import type { MentionGroup, MentionHit, MentionLinkReport } from "./noteMentions";

export type NoteMentionsPanelProps = {
  groups: MentionGroup[];
  /** 总处数(不是来源篇数)。 */
  count: number;
  /** 其中 confident 的处数 —— 「全部链接」会动的就是这些。 */
  confidentCount: number;
  loading: boolean;
  /** 正在写盘。写的时候不能再点,也不该重扫。 */
  linking: boolean;
  /** 扫描失败时的文案。就地显示,不占用面板那条错误条。 */
  error: string | null;
  /** 上一次链接的结果。null = 这一档还没动过手。 */
  report: MentionLinkReport | null;
  /** 跳到某一篇的某一行。 */
  onJump: (path: string, line: number) => void;
  /** 包某一处。 */
  onLink: (path: string, hit: MentionHit) => void;
  /** 包全部 confident 的。 */
  onLinkAll: () => void;
  onRefresh: () => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

export function NoteMentionsPanel({
  groups,
  count,
  confidentCount,
  loading,
  linking,
  error,
  report,
  onJump,
  onLink,
  onLinkAll,
  onRefresh,
  t,
}: NoteMentionsPanelProps) {
  const busy = loading || linking;
  return (
    <section
      aria-label={t("notebook.mentions")}
      style={{
        minWidth: 0,
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
        borderTop: "1px solid var(--border-subtle, var(--border))",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 4,
          padding: "5px 6px 5px 8px",
          fontSize: 10,
          color: "var(--text-muted)",
          textTransform: "uppercase",
          letterSpacing: 0.4,
        }}
      >
        <span style={{ flex: 1, minWidth: 0 }}>
          {t("notebook.mentionCount", {
            count: String(count),
            notes: String(groups.length),
          })}
        </span>
        {linking ? (
          <Loader2
            size={11}
            aria-hidden="true"
            style={{ flexShrink: 0, color: "var(--text-muted)" }}
          />
        ) : null}
        <button
          type="button"
          aria-label={t("notebook.mentionRefresh")}
          title={t("notebook.mentionRefresh")}
          onClick={onRefresh}
          disabled={busy}
          style={{
            border: "none",
            background: "transparent",
            color: "var(--text-muted)",
            cursor: busy ? "progress" : "pointer",
            opacity: busy ? 0.45 : 1,
            padding: 2,
            display: "flex",
            flexShrink: 0,
          }}
        >
          <RefreshCw size={11} />
        </button>
      </div>

      {error ? (
        <div
          role="alert"
          style={{
            margin: "0 8px 8px",
            padding: "6px 8px",
            borderRadius: 6,
            background: "var(--danger-subtle, var(--bg-card))",
            color: "var(--danger, var(--text-primary))",
            fontSize: 11,
            lineHeight: 1.4,
            wordBreak: "break-word",
          }}
        >
          {error}
        </div>
      ) : null}

      {report ? <MentionReportLine report={report} t={t} /> : null}

      {/* 「全部链接」只在真有 confident 的时候出现。ambiguous 不进批量,所以全是
          ambiguous 时这个按钮不该在 —— 摆一个点了什么都不会变的按钮更糟。 */}
      {confidentCount > 0 ? (
        <div style={{ padding: "0 8px 6px" }}>
          <button
            type="button"
            onClick={onLinkAll}
            disabled={busy}
            style={{
              width: "100%",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              gap: 4,
              border: "1px solid var(--border)",
              borderRadius: 6,
              background: "var(--bg-card)",
              color: "var(--text-primary)",
              cursor: busy ? "progress" : "pointer",
              opacity: busy ? 0.6 : 1,
              padding: "4px 6px",
              fontSize: 11,
            }}
          >
            <Link2 size={11} aria-hidden="true" />
            {t("notebook.mentionLinkAll", { count: String(confidentCount) })}
          </button>
        </div>
      ) : null}

      {/* 加载中仍然显示上一次的结果,和反链同一个理由:清空会让「刷新」变成一次闪烁。 */}
      {groups.length === 0 && !loading && !error ? (
        <div style={{ padding: "4px 10px 12px", fontSize: 11, color: "var(--text-hint)" }}>
          {t("notebook.mentionEmpty")}
        </div>
      ) : (
        <div
          data-testid="note-mentions-list"
          style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "0 6px 10px" }}
        >
          {groups.map((group) => (
            <div key={group.path} style={{ marginBottom: 8 }}>
              <div
                title={group.path}
                style={{
                  padding: "0 2px 2px",
                  fontSize: 11,
                  fontWeight: 600,
                  color: "var(--text-secondary)",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
              >
                {group.title}
              </div>
              {group.hits.map((hit) => (
                <MentionRow
                  /* key 用 start:同一行可以有多处,行号不唯一。 */
                  key={hit.start}
                  group={group}
                  hit={hit}
                  busy={busy}
                  onJump={onJump}
                  onLink={onLink}
                  t={t}
                />
              ))}
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

/* 上一次链接的那张账:改了几**处**、跳过几处、几篇没成。
 *
 * 处数用后端算好的 `report.linked`,不在这里把 `changed` 加起来 —— 前端自己挑一个数
 * 来报正是 Markio 那份把文件数说成处数的成因。跳过和失败只在真有的时候显示:恒定挂着
 * 「跳过 0 处」会让人以为出了什么事。
 */
function MentionReportLine({
  report,
  t,
}: {
  report: MentionLinkReport;
  t: (key: string, vars?: Record<string, string>) => string;
}) {
  return (
    <div
      role="status"
      style={{
        margin: "0 8px 6px",
        padding: "5px 8px",
        borderRadius: 6,
        background: "var(--bg-card)",
        color: "var(--text-secondary)",
        fontSize: 11,
        lineHeight: 1.45,
        wordBreak: "break-word",
      }}
    >
      {report.linked
        ? t("notebook.mentionLinked", {
            count: String(report.linked),
            notes: String(report.changed.length),
          })
        : t("notebook.mentionLinkedNone")}
      {report.skipped.length
        ? ` ${t("notebook.mentionLinkSkipped", { count: String(report.skipped.length) })}`
        : ""}
      {report.failed.length
        ? ` ${t("notebook.mentionLinkFailed", { notes: String(report.failed.length) })}`
        : ""}
    </div>
  );
}

type MentionRowProps = {
  group: MentionGroup;
  hit: MentionHit;
  busy: boolean;
  onJump: (path: string, line: number) => void;
  onLink: (path: string, hit: MentionHit) => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

function MentionRow({ group, hit, busy, onJump, onLink, t }: MentionRowProps) {
  const ambiguous = hit.confidence === "ambiguous";
  return (
    <div style={{ display: "flex", alignItems: "flex-start", gap: 2 }}>
      <button
        type="button"
        /* 可及名里带上来源标题和行号:列表按来源分组,而屏读逐个念按钮时听不到
           上面那个分组标题。ambiguous 的额外报出来 —— 那是"点下去要自己判断"的信号,
           而颜色和图标对屏读用户不存在。 */
        aria-label={t(ambiguous ? "notebook.mentionJumpAmbiguous" : "notebook.mentionJump", {
          title: group.title,
          line: String(hit.line),
        })}
        title={hit.preview}
        onClick={() => onJump(group.path, hit.line)}
        style={{
          flex: 1,
          minWidth: 0,
          display: "flex",
          alignItems: "flex-start",
          gap: 3,
          border: "none",
          borderRadius: 4,
          background: "transparent",
          color: "var(--text-primary)",
          cursor: "pointer",
          textAlign: "left",
          padding: "2px 4px",
          fontSize: 11,
          lineHeight: 1.35,
        }}
      >
        <span
          aria-hidden="true"
          style={{
            marginTop: 3,
            flexShrink: 0,
            width: 5,
            height: 5,
            borderRadius: "50%",
            /* ambiguous 空心、confident 实心。形状差异不只是颜色 —— 色觉障碍下
               两者仍然分得开。 */
            background: ambiguous ? "transparent" : "var(--text-muted)",
            border: ambiguous ? "1px solid var(--warning, var(--text-muted))" : "none",
            boxSizing: "border-box",
          }}
        />
        <span
          style={{
            minWidth: 0,
            overflow: "hidden",
            display: "-webkit-box",
            WebkitBoxOrient: "vertical",
            WebkitLineClamp: 2,
          }}
        >
          {hit.preview}
        </span>
      </button>
      <button
        type="button"
        aria-label={t("notebook.mentionLinkOne", {
          text: hit.text,
          line: String(hit.line),
        })}
        title={t("notebook.mentionLinkOne", { text: hit.text, line: String(hit.line) })}
        onClick={() => onLink(group.path, hit)}
        disabled={busy}
        style={{
          border: "none",
          background: "transparent",
          color: "var(--text-muted)",
          cursor: busy ? "progress" : "pointer",
          opacity: busy ? 0.45 : 1,
          padding: "3px 2px",
          display: "flex",
          flexShrink: 0,
        }}
      >
        <Link2 size={11} />
      </button>
    </div>
  );
}
