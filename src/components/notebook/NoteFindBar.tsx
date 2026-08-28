/* 随手记的查找 / 替换栏(⌘F / ⌘H)。
 *
 * 从 NotebookPanel 抽出来,JSX 逐字未改。
 *
 * 命中定位由面板负责 —— 它持有 CodeMirror 的 handle,能把选区设到命中处并滚动
 * 到可见。这个组件只管输入与导航按钮。
 */

import { ChevronDown, ChevronUp, Replace, Search, X } from "lucide-react";
import type React from "react";

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
  onMove: (direction: 1 | -1) => void;
  onReplaceOne: () => void;
  onReplaceAll: () => void;
  onClose: () => void;
  /** 打开时自动聚焦到查找框。 */
  inputRef: React.RefObject<HTMLInputElement | null>;
  t: (key: string) => string;
};

export function NoteFindBar({
  replaceOpen,
  onShowReplace,
  query,
  onQueryChange,
  replacement,
  onReplacementChange,
  matchCount,
  activeMatchIndex,
  onMove,
  onReplaceOne,
  onReplaceAll,
  onClose,
  inputRef,
  t,
}: NoteFindBarProps) {
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
          border: "1px solid var(--border-medium)",
          borderRadius: 6,
          background: "var(--bg-input)",
          color: "var(--text-primary)",
          padding: "0 8px",
          fontSize: 12,
          outline: "none",
        }}
      />
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
      <span aria-live="polite" style={{ minWidth: 54, fontSize: 11, color: "var(--text-muted)" }}>
        {matchCount > 0
          ? `${Math.min(activeMatchIndex + 1, matchCount)}/${matchCount}`
          : t("notebook.noMatches")}
      </span>
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
