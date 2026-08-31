/* 全库搜索面板(⌘⇧F)。
 *
 * 只画界面与列表,搜索本身由面板发起(它持有 vault 路径和 invoke)。命中点击后交回
 * `onOpen(path, line)`,由面板走反链那条已经验证过的跳转路径。
 *
 * 结果里的行文本按纯文本渲染,不当 Markdown ——命中行常常是表格或代码,渲染出来会
 * 把列表撑破,而且用户在这里要看的就是原文长什么样。
 */

import { Loader2, Search, X } from "lucide-react";
import type React from "react";

import {
  groupSearchHits,
  hitSegments,
  type NoteSearchFlags,
  type NoteSearchHit,
} from "./noteGlobalSearch";

export type NoteSearchSheetProps = {
  query: string;
  onQueryChange: (value: string) => void;
  flags: NoteSearchFlags;
  onFlagsChange: (next: NoteSearchFlags) => void;
  hits: readonly NoteSearchHit[];
  loading: boolean;
  /** 后端报的错(正则不合法、ripgrep 不在 PATH 且回落也失败)。 */
  error?: string | null;
  /** 命中数触顶,列表不是全部。 */
  capped: boolean;
  /** 已经搜过一次了。没搜过时不显示「无结果」—— 那会让刚打开的面板看起来像搜空了。 */
  searched: boolean;
  onSubmit: () => void;
  onOpen: (hit: NoteSearchHit) => void;
  onClose: () => void;
  inputRef: React.RefObject<HTMLInputElement | null>;
  /* 全库替换条。做成插槽而不是一堆 replace* prop:替换要预览、要按文件勾选、要等
     保存落盘,那些状态全在面板里,搬进这里只会让这个组件同时管两件事。
     排在命中列表**之后** —— 上面是"搜到了什么",下面是"要把它们改成什么"。 */
  replace?: React.ReactNode;
  t: (key: string, vars?: Record<string, string | number>) => string;
};

const TOGGLES: { key: keyof NoteSearchFlags; label: string; i18n: string }[] = [
  { key: "caseSensitive", label: "Aa", i18n: "notebook.findCaseSensitive" },
  { key: "wholeWord", label: "ab|", i18n: "notebook.findWholeWord" },
  { key: "regex", label: ".*", i18n: "notebook.findRegex" },
];

export function NoteSearchSheet({
  query,
  onQueryChange,
  flags,
  onFlagsChange,
  hits,
  loading,
  error,
  capped,
  searched,
  onSubmit,
  onOpen,
  onClose,
  inputRef,
  replace,
  t,
}: NoteSearchSheetProps) {
  const groups = groupSearchHits(hits);
  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={t("notebook.globalSearch")}
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        event.preventDefault();
        onClose();
      }}
      style={{
        position: "absolute",
        inset: 0,
        zIndex: 30,
        display: "flex",
        flexDirection: "column",
        background: "var(--bg-panel)",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 6,
          padding: "7px 9px",
          borderBottom: "1px solid var(--border-dim)",
          background: "var(--bg-sidebar)",
        }}
      >
        <Search size={13} color="var(--text-muted)" />
        <input
          ref={inputRef}
          aria-label={t("notebook.globalSearch")}
          value={query}
          onChange={(event) => onQueryChange(event.currentTarget.value)}
          onKeyDown={(event) => {
            if (event.key !== "Enter") return;
            event.preventDefault();
            onSubmit();
          }}
          placeholder={t("notebook.globalSearchPlaceholder")}
          style={{
            flex: 1,
            minWidth: 0,
            height: 26,
            border: `1px solid ${error ? "var(--danger)" : "var(--border-medium)"}`,
            borderRadius: 6,
            background: "var(--bg-input)",
            color: "var(--text-primary)",
            padding: "0 8px",
            fontSize: 12,
            outline: "none",
          }}
        />
        {TOGGLES.map((toggle) => {
          const on = flags[toggle.key];
          return (
            <button
              key={toggle.key}
              type="button"
              aria-label={t(toggle.i18n)}
              title={t(toggle.i18n)}
              aria-pressed={on}
              onClick={() => onFlagsChange({ ...flags, [toggle.key]: !on })}
              style={{
                height: 24,
                minWidth: 26,
                border: `1px solid ${on ? "var(--accent)" : "var(--border-medium)"}`,
                borderRadius: 5,
                background: on
                  ? "color-mix(in srgb, var(--accent) 18%, transparent)"
                  : "transparent",
                color: on ? "var(--accent)" : "var(--text-muted)",
                padding: "0 5px",
                cursor: "pointer",
                fontSize: 11,
                fontFamily: "var(--font-mono, monospace)",
              }}
            >
              {toggle.label}
            </button>
          );
        })}
        <button
          type="button"
          aria-label={t("common.close")}
          title={t("common.close")}
          onClick={onClose}
          style={{
            width: 24,
            height: 24,
            border: "none",
            borderRadius: 5,
            background: "transparent",
            color: "var(--text-muted)",
            cursor: "pointer",
          }}
        >
          <X size={13} />
        </button>
      </div>
      <div
        aria-live="polite"
        style={{ padding: "5px 10px", fontSize: 11, color: "var(--text-muted)" }}
      >
        {loading ? (
          <span style={{ display: "inline-flex", alignItems: "center", gap: 5 }}>
            <Loader2 size={11} />
            {t("notebook.globalSearchRunning")}
          </span>
        ) : error ? (
          <span style={{ color: "var(--danger)" }}>{error}</span>
        ) : hits.length > 0 ? (
          <span>
            {t("notebook.globalSearchSummary", { hits: hits.length, files: groups.length })}
            {capped ? ` · ${t("notebook.globalSearchCapped")}` : ""}
          </span>
        ) : searched ? (
          <span>{t("notebook.globalSearchEmpty")}</span>
        ) : (
          <span>{t("notebook.globalSearchHint")}</span>
        )}
      </div>
      <div style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "0 6px 8px" }}>
        {groups.map((group) => (
          <section key={group.path} aria-label={group.name} style={{ marginBottom: 8 }}>
            <div
              style={{
                padding: "3px 4px",
                fontSize: 11,
                fontWeight: 600,
                color: "var(--text-secondary)",
              }}
            >
              {group.name}
              <span style={{ marginLeft: 5, fontWeight: 400, color: "var(--text-muted)" }}>
                {group.hits.length}
              </span>
            </div>
            {group.hits.map((hit) => {
              const segments = hitSegments(hit);
              return (
                <button
                  key={`${hit.line}:${hit.column}`}
                  type="button"
                  /* 可及名字带上文件名和行号:读屏用户逐个按下来时,只念行文本分不出
                     这一条属于哪篇笔记 —— 而分组标题只念一次。 */
                  aria-label={t("notebook.globalSearchHit", {
                    name: group.name,
                    line: hit.line,
                    text: hit.lineText.trim() || hit.matchText,
                  })}
                  onClick={() => onOpen(hit)}
                  style={{
                    display: "flex",
                    gap: 7,
                    width: "100%",
                    border: "none",
                    borderRadius: 5,
                    background: "transparent",
                    color: "var(--text-secondary)",
                    padding: "3px 5px",
                    cursor: "pointer",
                    textAlign: "left",
                    fontSize: 11,
                    lineHeight: 1.5,
                  }}
                >
                  <span style={{ color: "var(--text-muted)", flex: "0 0 auto" }}>{hit.line}</span>
                  <span
                    style={{
                      flex: 1,
                      minWidth: 0,
                      whiteSpace: "nowrap",
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      fontFamily: "var(--font-mono, monospace)",
                    }}
                  >
                    {segments.before}
                    {segments.match ? (
                      <mark
                        style={{
                          background: "color-mix(in srgb, var(--accent) 30%, transparent)",
                          color: "var(--text-primary)",
                        }}
                      >
                        {segments.match}
                      </mark>
                    ) : null}
                    {segments.after}
                  </span>
                </button>
              );
            })}
          </section>
        ))}
      </div>
      {replace}
    </div>
  );
}
