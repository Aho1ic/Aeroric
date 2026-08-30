/* 随手记的查找 / 替换栏(⌘F / ⌘H)。
 *
 * 命中定位由面板负责 —— 它持有 CodeMirror 的 handle,能把选区设到命中处并滚动
 * 到可见。这个组件只管输入、三个开关与导航按钮。
 *
 * 状态一栏里要报四件事,而不是只报一个数字:命中数、正则报错、命中被截断、整词在
 * 中日韩上没生效。后三件如果不说,用户看到的都是「0 个 / 若干个」,而原因完全不同。
 */

import { ChevronDown, ChevronUp, Replace, Search, X } from "lucide-react";
import type React from "react";

export type NoteFindFlags = {
  caseSensitive: boolean;
  wholeWord: boolean;
  regex: boolean;
};

export type NoteFindBarProps = {
  /** 替换行是否展开。⌘F 只开查找,⌘H 连替换一起开。 */
  replaceOpen: boolean;
  onShowReplace: () => void;
  query: string;
  onQueryChange: (value: string) => void;
  replacement: string;
  onReplacementChange: (value: string) => void;
  /** 命中总数与当前序号(0-based)。 */
  matchCount: number;
  activeMatchIndex: number;
  flags: NoteFindFlags;
  onFlagsChange: (next: NoteFindFlags) => void;
  /** 正则不合法时的报错原文。非空时状态栏显示它,而不是「无匹配项」。 */
  error?: string | null;
  /** 命中数触顶被截断。 */
  capped?: boolean;
  /** 整词有命中因为紧贴中日韩文字而放弃了边界要求。 */
  wholeWordIgnored?: boolean;
  onMove: (direction: 1 | -1) => void;
  onReplaceOne: () => void;
  onReplaceAll: () => void;
  onClose: () => void;
  /** 打开时自动聚焦到查找框。 */
  inputRef: React.RefObject<HTMLInputElement | null>;
  t: (key: string) => string;
};

const TOGGLE_KEYS: { key: keyof NoteFindFlags; label: string; i18n: string }[] = [
  { key: "caseSensitive", label: "Aa", i18n: "notebook.findCaseSensitive" },
  { key: "wholeWord", label: "ab|", i18n: "notebook.findWholeWord" },
  { key: "regex", label: ".*", i18n: "notebook.findRegex" },
];

export function NoteFindBar({
  replaceOpen,
  onShowReplace,
  query,
  onQueryChange,
  replacement,
  onReplacementChange,
  matchCount,
  activeMatchIndex,
  flags,
  onFlagsChange,
  error,
  capped,
  wholeWordIgnored,
  onMove,
  onReplaceOne,
  onReplaceAll,
  onClose,
  inputRef,
  t,
}: NoteFindBarProps) {
  const status = error
    ? t("notebook.findInvalidRegex")
    : matchCount > 0
      ? `${Math.min(activeMatchIndex + 1, matchCount)}/${matchCount}${capped ? "+" : ""}`
      : t("notebook.noMatches");
  return (
    <div
      role="search"
      aria-label={t("notebook.findReplace")}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 6,
        padding: "5px 8px",
        borderBottom: "1px solid var(--border-dim)",
        background: "var(--bg-sidebar)",
        flexWrap: "wrap",
      }}
    >
      <Search size={13} color="var(--text-muted)" />
      <input
        ref={inputRef}
        aria-label={t("notebook.find")}
        value={query}
        onChange={(event) => {
          onQueryChange(event.currentTarget.value);
        }}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            onClose();
          } else if (event.key === "Enter") {
            event.preventDefault();
            onMove(event.shiftKey ? -1 : 1);
          }
        }}
        placeholder={t("notebook.findPlaceholder")}
        style={{
          width: 180,
          height: 26,
          /* 正则不合法时把边框染红:状态栏那行字在最右边,而眼睛在输入框上。 */
          border: `1px solid ${error ? "var(--danger)" : "var(--border-medium)"}`,
          borderRadius: 6,
          background: "var(--bg-input)",
          color: "var(--text-primary)",
          padding: "0 8px",
          fontSize: 12,
          outline: "none",
        }}
      />
      {TOGGLE_KEYS.map((toggle) => {
        const on = flags[toggle.key];
        return (
          <button
            key={toggle.key}
            type="button"
            aria-label={t(toggle.i18n)}
            title={t(toggle.i18n)}
            /* 开关得让读屏报出「按下 / 未按下」。光靠背景色变化,只用键盘和读屏的人
               根本不知道大小写敏感现在是开还是关。 */
            aria-pressed={on}
            onClick={() => onFlagsChange({ ...flags, [toggle.key]: !on })}
            style={{
              height: 24,
              minWidth: 26,
              border: `1px solid ${on ? "var(--accent)" : "var(--border-medium)"}`,
              borderRadius: 5,
              background: on ? "color-mix(in srgb, var(--accent) 18%, transparent)" : "transparent",
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
      {replaceOpen && (
        <>
          <Replace size={13} color="var(--text-muted)" />
          <input
            aria-label={t("notebook.replace")}
            value={replacement}
            onChange={(event) => onReplacementChange(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                onClose();
              } else if (event.key === "Enter") {
                event.preventDefault();
                onReplaceOne();
              }
            }}
            placeholder={t("notebook.replacePlaceholder")}
            style={{
              width: 150,
              height: 26,
              border: "1px solid var(--border-medium)",
              borderRadius: 6,
              background: "var(--bg-input)",
              color: "var(--text-primary)",
              padding: "0 8px",
              fontSize: 12,
              outline: "none",
            }}
          />
        </>
      )}
      <span
        aria-live="polite"
        title={error ?? undefined}
        style={{
          minWidth: 54,
          fontSize: 11,
          color: error ? "var(--danger)" : "var(--text-muted)",
        }}
      >
        {status}
      </span>
      {wholeWordIgnored && !error && (
        /* 中日韩没有词边界,整词在这一侧卡不住。说清楚比悄悄放行或悄悄返回 0 都好。 */
        <span style={{ fontSize: 11, color: "var(--warning)" }} title={t("notebook.findCjkHint")}>
          {t("notebook.findCjkBadge")}
        </span>
      )}
      <button
        type="button"
        aria-label={t("notebook.previousMatch")}
        title={t("notebook.previousMatch")}
        disabled={matchCount === 0}
        onClick={() => onMove(-1)}
        style={{
          width: 24,
          height: 24,
          border: "none",
          borderRadius: 5,
          background: "transparent",
          color: "var(--text-muted)",
          cursor: matchCount > 0 ? "pointer" : "default",
        }}
      >
        <ChevronUp size={13} />
      </button>
      <button
        type="button"
        aria-label={t("notebook.nextMatch")}
        title={t("notebook.nextMatch")}
        disabled={matchCount === 0}
        onClick={() => onMove(1)}
        style={{
          width: 24,
          height: 24,
          border: "none",
          borderRadius: 5,
          background: "transparent",
          color: "var(--text-muted)",
          cursor: matchCount > 0 ? "pointer" : "default",
        }}
      >
        <ChevronDown size={13} />
      </button>
      {replaceOpen && (
        <>
          <button
            type="button"
            disabled={matchCount === 0}
            onClick={onReplaceOne}
            style={{
              height: 24,
              border: "1px solid var(--border-medium)",
              borderRadius: 5,
              background: "var(--bg-card)",
              color: "var(--text-secondary)",
              padding: "0 7px",
              cursor: matchCount > 0 ? "pointer" : "default",
              fontSize: 11,
            }}
          >
            {t("notebook.replace")}
          </button>
          <button
            type="button"
            disabled={matchCount === 0}
            onClick={onReplaceAll}
            style={{
              height: 24,
              border: "1px solid var(--border-medium)",
              borderRadius: 5,
              background: "var(--bg-card)",
              color: "var(--text-secondary)",
              padding: "0 7px",
              cursor: matchCount > 0 ? "pointer" : "default",
              fontSize: 11,
            }}
          >
            {t("notebook.replaceAll")}
          </button>
        </>
      )}
      {!replaceOpen && (
        <button
          type="button"
          title={t("notebook.showReplace")}
          onClick={onShowReplace}
          style={{
            height: 24,
            border: "1px solid var(--border-medium)",
            borderRadius: 5,
            background: "var(--bg-card)",
            color: "var(--text-secondary)",
            padding: "0 7px",
            cursor: "pointer",
            fontSize: 11,
          }}
        >
          {t("notebook.replace")}
        </button>
      )}
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
  );
}
