import type React from "react";

import { zLayers } from "./zLayers";

export const common = {
  errorBoundaryWrap: {
    flex: 1,
    display: "flex",
    flexDirection: "column",
    alignItems: "center",
    justifyContent: "center",
    padding: "24px 32px",
    gap: 12,
    color: "var(--text-muted)",
    fontSize: 13,
    textAlign: "center" as const,
  },
  errorBoundaryIcon: { fontSize: 28, lineHeight: 1 },
  errorBoundaryTitle: { fontWeight: 600, color: "var(--text-secondary)" },
  errorBoundaryMessage: {
    maxWidth: 320,
    fontSize: 12,
    color: "var(--text-hint)",
    wordBreak: "break-word" as const,
    lineHeight: 1.5,
  },
  errorBoundaryActions: { display: "flex", gap: 8 },
  errorBoundaryBtn: {
    padding: "5px 16px",
    background: "var(--bg-hover)",
    border: "1px solid var(--border-dim)",
    borderRadius: 6,
    color: "var(--text-secondary)",
    fontSize: 12,
    cursor: "pointer",
    marginTop: 4,
  },
  usagePopoverContent: {
    width: 204,
    padding: "10px 12px",
    borderRadius: 9,
    border: "1px solid var(--border-medium)",
    background: "var(--bg-card)",
    boxShadow: "var(--shadow-md)",
    zIndex: zLayers.popover,
    backdropFilter: "var(--glass-blur-compact)",
    WebkitBackdropFilter: "var(--glass-blur-compact)",
  },
  usagePopoverHeader: {
    padding: "0 0 7px",
    borderBottom: "1px solid var(--border-dim)",
    marginBottom: 8,
  },
  usagePopoverTitle: {
    fontSize: 11.5,
    fontWeight: 700,
    color: "var(--text-primary)",
  },
  // 同上:名字避开 styles/usage.ts 的 grid 版,popover 里装的是 SourceCard 竖列。
  usagePopoverSourceList: {
    display: "flex",
    flexDirection: "column",
    gap: 9,
  },
  usageSourceSection: {
    display: "flex",
    flexDirection: "column",
    gap: 4,
  },
  usageSourceHead: {
    marginBottom: 3,
  },
  usageSourceTitle: {
    fontSize: 11,
    fontWeight: 700,
    color: "var(--text-secondary)",
  },
  usageSourceSubtitle: {
    fontSize: 9.5,
    color: "var(--text-hint)",
    lineHeight: 1.35,
    wordBreak: "break-word" as const,
    marginTop: 1,
  },
  usageMetricList: {
    display: "flex",
    flexDirection: "column",
    gap: 4,
  },
  usageMetricRow: {
    display: "flex",
    alignItems: "center",
    gap: 6,
  },
  // Popover 专属:label 与 value 在 usageMetricRow 里左右并排(flex + alignItems:center),
  // 所以 label 要 flex:1 把 value 顶到右侧,且 value 不能有 marginTop。
  // 不要改回 usageMetricLabel/usageMetricValue —— 那两个名字被 styles/usage.ts
  // 的卡片版(grid + marginTop:11)占用,styles/index.ts 的 spread 顺序会让本文件被覆盖。
  usagePopoverMetricLabel: {
    flex: 1,
    fontSize: 11,
    fontWeight: 500,
    color: "var(--text-secondary)",
  },
  usagePopoverMetricValue: {
    fontSize: 11,
    fontWeight: 700,
    whiteSpace: "nowrap" as const,
  },
  usageMetricMeta: {
    fontSize: 10,
    color: "var(--text-hint)",
    flexShrink: 0,
  },
  usageUnavailableText: {
    fontSize: 10.5,
    color: "var(--text-muted)",
    lineHeight: 1.5,
  },
  usageStatusText: {
    fontSize: 11,
    color: "var(--text-muted)",
    lineHeight: 1.5,
    padding: "2px 0",
  },
  usageInlineWindow: {
    display: "flex",
    gap: 3,
    alignItems: "center",
  },
  usageInlineWindowLabel: {
    fontSize: 10,
    color: "var(--text-hint)",
  },
  usageInlineWindowValue: {
    fontSize: 10.5,
    fontWeight: 700,
  },
} satisfies Record<string, React.CSSProperties>;
