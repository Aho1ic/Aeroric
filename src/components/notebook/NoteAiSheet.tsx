/* 随手记的语义检索与 AI 上下文面板(P7)。
 *
 * 三块内容,自上而下:
 *   1. **索引状态**:多少篇建好了、多少待办 / 过期 / 失败,以及建索引 / 只重试失败的 /
 *      取消 / 清空四个动作。正在跑时这里变成进度条。
 *   2. **检索**:一个查询框 + 命中列表。命中点击后跳回原文那一行。
 *   3. **上下文**:装配好的那段文本 + 引用清单。给"拿这段去问 AI"用。
 *
 * 降级必须显示。向量那一路挂了(Ollama 没开、模型名写错)的表现是"结果莫名变差",
 * 用户无从判断是索引没建好还是模型没连上 —— 所以 `degraded` 非空时顶上要有一条提示,
 * 而 `vectorsMissing` 是另一回事(还没建索引),提示的是"去建索引"。
 *
 * 和其余七个 sheet 一样铺在面板内部而不是整个窗口:随手记面板可以只占项目视图的
 * 一半,盖住整个窗口会把用户正在参照的另一半也遮掉。
 */

import { type CSSProperties } from "react";
import { Ban, Loader2, RefreshCw, Search, Trash2, X } from "lucide-react";

import {
  ragProgressPercent,
  type RagContextBundle,
  type RagDegraded,
  type RagHit,
  type RagIndexProgress,
  type RagIndexStats,
} from "./noteRag";
import {
  noteSheetHeaderStyle,
  noteSheetIconButtonStyle,
  noteSheetOverlayStyle,
  useNoteSheetDismiss,
} from "./noteSheetChrome";

export type NoteAiSheetProps = {
  stats: RagIndexStats | null;
  /** 正在跑的那一轮。null 表示没在跑。 */
  progress: RagIndexProgress | null;
  query: string;
  onQueryChange: (value: string) => void;
  hits: readonly RagHit[];
  /** 已经搜过一次。没搜过时不显示「无结果」—— 那会让刚打开的面板像是搜空了。 */
  searched: boolean;
  searching: boolean;
  degraded: readonly RagDegraded[];
  vectorsMissing: boolean;
  context: RagContextBundle | null;
  contextBusy: boolean;
  /** 上下文刚被复制走。 */
  copied: boolean;
  error: string | null;
  onSearch: () => void;
  onBuildContext: () => void;
  onCopyContext: () => void;
  onIndex: (scope: "all" | "failedOnly") => void;
  onCancelIndex: () => void;
  onClearIndex: () => void;
  onOpenHit: (hit: RagHit) => void;
  onClose: () => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

const headerStyle = noteSheetHeaderStyle(8);

const sectionStyle: CSSProperties = {
  padding: "6px 8px",
  borderBottom: "1px solid var(--border-dim)",
  fontSize: 11.5,
};

const labelStyle: CSSProperties = { color: "var(--text-hint)" };

const buttonStyle: CSSProperties = {
  height: 22,
  padding: "0 8px",
  border: "1px solid var(--border)",
  borderRadius: 4,
  background: "transparent",
  color: "var(--text-secondary)",
  fontSize: 11.5,
  cursor: "pointer",
};

function disabledButton(disabled: boolean): CSSProperties {
  return disabled ? { ...buttonStyle, color: "var(--text-hint)", cursor: "default" } : buttonStyle;
}

/** 命中正文里的高亮。区间来自后端(相对 `body`),这里按 JS 下标切。 */
function highlight(hit: RagHit): React.ReactNode {
  if (hit.bodySpans.length === 0) return hit.body;
  const parts: React.ReactNode[] = [];
  let cursor = 0;
  hit.bodySpans.forEach((span, index) => {
    /* 区间来自 Rust 的标量计数。预览里的偏差只影响高亮位置(不会跳错行),而
       为了一段预览把整块正文再扫一遍代理对不值得 —— 有 emoji 时高亮可能差一两个
       字符,这是有意接受的。跳转那一路走 `scalarToUtf16`,不受影响。 */
    const start = Math.max(cursor, Math.min(span.start, hit.body.length));
    const end = Math.max(start, Math.min(span.end, hit.body.length));
    if (start > cursor) parts.push(hit.body.slice(cursor, start));
    if (end > start) {
      parts.push(
        <mark key={index} style={{ background: "var(--accent-dim, #3b82f640)", color: "inherit" }}>
          {hit.body.slice(start, end)}
        </mark>,
      );
    }
    cursor = end;
  });
  if (cursor < hit.body.length) parts.push(hit.body.slice(cursor));
  return parts;
}

export function NoteAiSheet({
  stats,
  progress,
  query,
  onQueryChange,
  hits,
  searched,
  searching,
  degraded,
  vectorsMissing,
  context,
  contextBusy,
  copied,
  error,
  onSearch,
  onBuildContext,
  onCopyContext,
  onIndex,
  onCancelIndex,
  onClearIndex,
  onOpenHit,
  onClose,
  t,
}: NoteAiSheetProps) {
  const { closeRef, overlayProps } = useNoteSheetDismiss(t("notebook.aiTitle"), onClose);
  const running = progress !== null;
  const percent = ragProgressPercent(progress);

  return (
    <div style={noteSheetOverlayStyle} {...overlayProps}>
      <div style={headerStyle}>
        <span>{t("notebook.aiTitle")}</span>
        <button
          ref={closeRef}
          type="button"
          aria-label={t("notebook.aiClose")}
          onClick={onClose}
          style={{ ...noteSheetIconButtonStyle, marginLeft: "auto", color: "var(--text-hint)" }}
        >
          <X size={13} aria-hidden />
        </button>
      </div>

      {/* 索引状态 */}
      <div style={sectionStyle}>
        {running ? (
          <div>
            <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <Loader2 size={12} aria-hidden style={{ flexShrink: 0 }} />
              <span style={labelStyle}>
                {t(`notebook.aiPhase.${progress.phase}`)}
                {progress.total > 0 ? ` ${progress.done}/${progress.total}` : ""}
                {progress.failed > 0
                  ? ` · ${t("notebook.aiIndexFailed", { count: String(progress.failed) })}`
                  : ""}
              </span>
              <button
                type="button"
                onClick={onCancelIndex}
                style={{ ...buttonStyle, marginLeft: "auto" }}
              >
                <Ban size={11} aria-hidden style={{ marginRight: 4, verticalAlign: -1 }} />
                {t("notebook.aiCancel")}
              </button>
            </div>
            {/* 进度条。`total` 为 0(还在扫描)时宽度是 0,不显示成满格。 */}
            <div
              role="progressbar"
              aria-valuenow={percent}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-label={t("notebook.aiIndexProgress")}
              style={{
                marginTop: 5,
                height: 3,
                borderRadius: 2,
                background: "var(--border-dim)",
                overflow: "hidden",
              }}
            >
              <div style={{ width: `${percent}%`, height: "100%", background: "var(--accent)" }} />
            </div>
            {progress.current && (
              <div
                style={{
                  marginTop: 4,
                  color: "var(--text-hint)",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
                title={progress.current}
              >
                {progress.current}
              </div>
            )}
          </div>
        ) : (
          <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
            <span style={labelStyle}>
              {stats
                ? t("notebook.aiIndexStats", {
                    docs: String(stats.indexed),
                    chunks: String(stats.chunks),
                  })
                : t("notebook.aiIndexUnknown")}
            </span>
            {stats && (stats.pending > 0 || stats.stale > 0) && (
              <span style={labelStyle}>
                · {t("notebook.aiIndexPending", { count: String(stats.pending + stats.stale) })}
              </span>
            )}
            {stats && stats.failed > 0 && (
              <span style={{ color: "var(--warning)" }}>
                · {t("notebook.aiIndexFailed", { count: String(stats.failed) })}
              </span>
            )}
            <button
              type="button"
              onClick={() => onIndex("all")}
              style={{ ...buttonStyle, marginLeft: "auto" }}
            >
              <RefreshCw size={11} aria-hidden style={{ marginRight: 4, verticalAlign: -1 }} />
              {t("notebook.aiIndexRun")}
            </button>
            {/* 只重试失败的。没有失败项时不画 —— 一个永远禁用的按钮只会让人猜它干什么。 */}
            {stats && stats.failed > 0 && (
              <button type="button" onClick={() => onIndex("failedOnly")} style={buttonStyle}>
                {t("notebook.aiIndexRetry")}
              </button>
            )}
            <button
              type="button"
              aria-label={t("notebook.aiIndexClear")}
              onClick={onClearIndex}
              style={{ ...noteSheetIconButtonStyle, color: "var(--danger, #f85149)" }}
            >
              <Trash2 size={12} aria-hidden />
            </button>
          </div>
        )}
      </div>

      {/* 失败清单。列出来才能知道该改配置还是该改笔记。 */}
      {!running && stats && stats.failures.length > 0 && (
        <div style={{ ...sectionStyle, maxHeight: 90, overflowY: "auto" }}>
          {stats.failures.map((failure) => (
            <div key={failure.path} style={{ display: "flex", gap: 6, color: "var(--text-hint)" }}>
              <span
                style={{
                  minWidth: 0,
                  flexShrink: 0,
                  maxWidth: "40%",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
                title={failure.path}
              >
                {failure.path.split("/").pop()}
              </span>
              <span style={{ minWidth: 0, flex: 1, color: "var(--warning)" }} title={failure.error}>
                {failure.error}
              </span>
            </div>
          ))}
        </div>
      )}

      {/* 检索 */}
      <div style={sectionStyle}>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            onSearch();
          }}
          style={{ display: "flex", alignItems: "center", gap: 6 }}
        >
          <Search size={12} aria-hidden style={{ flexShrink: 0, color: "var(--text-hint)" }} />
          <input
            value={query}
            onChange={(event) => onQueryChange(event.target.value)}
            placeholder={t("notebook.aiSearchPlaceholder")}
            aria-label={t("notebook.aiSearchPlaceholder")}
            style={{
              minWidth: 0,
              flex: 1,
              height: 22,
              padding: "0 6px",
              border: "1px solid var(--border)",
              borderRadius: 4,
              background: "var(--bg-input, transparent)",
              color: "var(--text-primary)",
              fontSize: 11.5,
            }}
          />
          <button type="submit" disabled={searching} style={disabledButton(searching)}>
            {searching ? t("notebook.aiSearching") : t("notebook.aiSearch")}
          </button>
          <button
            type="button"
            disabled={contextBusy || query.trim().length === 0}
            onClick={onBuildContext}
            style={disabledButton(contextBusy || query.trim().length === 0)}
          >
            {contextBusy ? t("notebook.aiAssembling") : t("notebook.aiAssemble")}
          </button>
        </form>
      </div>

      {/* 降级与错误 */}
      {vectorsMissing && (
        <div style={{ ...sectionStyle, color: "var(--text-hint)" }}>
          {t("notebook.aiVectorsMissing")}
        </div>
      )}
      {degraded.map((item) => (
        <div
          key={item.stage}
          style={{ ...sectionStyle, color: "var(--warning)" }}
          title={item.detail}
        >
          {t(`notebook.aiDegraded.${item.stage}`)}
        </div>
      ))}
      {error && <div style={{ ...sectionStyle, color: "var(--warning)" }}>{error}</div>}

      {/* 命中列表 */}
      <div style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: 5 }}>
        {hits.length === 0 ? (
          <div style={{ margin: "auto", padding: 10, color: "var(--text-hint)", fontSize: 11.5 }}>
            {searched ? t("notebook.aiNoHits") : t("notebook.aiHint")}
          </div>
        ) : (
          hits.map((hit) => (
            <button
              key={`${hit.path}:${hit.charStart}`}
              type="button"
              data-testid="note-ai-hit"
              onClick={() => onOpenHit(hit)}
              style={{
                display: "block",
                width: "100%",
                marginBottom: 4,
                padding: "5px 6px",
                border: "1px solid var(--border-dim)",
                borderRadius: 4,
                background: "transparent",
                color: "var(--text-secondary)",
                fontSize: 11.5,
                textAlign: "left",
                cursor: "pointer",
              }}
            >
              <div style={{ display: "flex", gap: 6, color: "var(--text-hint)" }}>
                <span
                  style={{
                    minWidth: 0,
                    flex: 1,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                  title={hit.path}
                >
                  {hit.title}
                  {hit.heading ? ` › ${hit.heading}` : ""}
                </span>
                <span style={{ flexShrink: 0 }}>{hit.source}</span>
              </div>
              {/* 正文按纯文本渲染,不当 Markdown:命中块常常是表格或代码,渲染出来
                  会把列表撑破,而用户在这里要看的就是原文长什么样。 */}
              <div style={{ marginTop: 3, whiteSpace: "pre-wrap", wordBreak: "break-word" }}>
                {highlight(hit)}
              </div>
            </button>
          ))
        )}
      </div>

      {/* 装配好的上下文 */}
      {context && (
        <div
          style={{
            ...sectionStyle,
            borderBottom: "none",
            borderTop: "1px solid var(--border-dim)",
          }}
        >
          <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
            <span style={labelStyle}>
              {t("notebook.aiContextSummary", {
                tokens: String(context.tokens),
                count: String(context.citations.length),
              })}
            </span>
            {context.truncated && (
              <span style={{ color: "var(--text-hint)" }}>
                · {t("notebook.aiContextTruncated")}
              </span>
            )}
            <button
              type="button"
              onClick={onCopyContext}
              style={{ ...buttonStyle, marginLeft: "auto" }}
            >
              {copied ? t("notebook.aiContextCopied") : t("notebook.aiContextCopy")}
            </button>
          </div>
          <pre
            data-testid="note-ai-context"
            style={{
              margin: "5px 0 0",
              maxHeight: 120,
              overflowY: "auto",
              whiteSpace: "pre-wrap",
              wordBreak: "break-word",
              color: "var(--text-secondary)",
              fontFamily: "inherit",
              fontSize: 11,
            }}
          >
            {context.text}
          </pre>
        </div>
      )}
    </div>
  );
}
