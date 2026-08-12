/**
 * 深色主题 token(石墨灰基调,对齐 orca 移动端设计语言)。
 * 颜色 key 名与旧版保持一致,现存 StyleSheet 无需改动即可换肤;
 * 新增 spacing / radii / typography 供新界面统一取值,避免继续硬编码。
 */
export const theme = {
  canvas: "#070b14",
  bg: "rgba(8, 12, 22, 0.82)",
  bgCard: "rgba(24, 30, 45, 0.68)",
  bgElevated: "rgba(38, 46, 65, 0.74)",
  border: "rgba(226, 232, 240, 0.14)",
  glassHighlight: "rgba(255, 255, 255, 0.08)",
  orbBlue: "rgba(37, 99, 235, 0.24)",
  orbPurple: "rgba(139, 92, 246, 0.2)",
  text: "#e0e0e0",
  textSecondary: "#888888",
  textHint: "#555555",
  /** 主行动按钮的亮色填充(orca surfaceBright)。 */
  surfaceBright: "#f5f5f5",
  accent: "#3b82f6",
  accentSoft: "rgba(29, 78, 216, 0.28)",
  accentPressed: "#2563c7",
  accentBorder: "rgba(96, 165, 250, 0.52)",
  onAccent: "#ffffff",
  info: "#3b82f6",
  success: "#22c55e",
  warning: "#f59e0b",
  danger: "#ef4444",
  purple: "#a78bfa",
} as const;

export const spacing = { xs: 4, sm: 8, md: 12, lg: 16, xl: 24 } as const;

/** 圆角比 orca 更圆润(按钮/输入 10,卡片 14)。 */
export const radii = { row: 10, card: 14, button: 10, input: 10, pill: 999 } as const;

export const typography = {
  titleSize: 18,
  bodySize: 14,
  metaSize: 12,
  labelSize: 11,
} as const;
