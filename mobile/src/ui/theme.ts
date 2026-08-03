/**
 * 深色主题 token(石墨灰基调,对齐 orca 移动端设计语言)。
 * 颜色 key 名与旧版保持一致,现存 StyleSheet 无需改动即可换肤;
 * 新增 spacing / radii / typography 供新界面统一取值,避免继续硬编码。
 */
export const theme = {
  bg: "#111111",
  bgCard: "#1a1a1a",
  bgElevated: "#242424",
  border: "#2a2a2a",
  text: "#e0e0e0",
  textSecondary: "#888888",
  textHint: "#555555",
  /** 主行动按钮的亮色填充(orca surfaceBright)。 */
  surfaceBright: "#f5f5f5",
  accent: "#3b82f6",
  accentSoft: "#1d355f",
  accentPressed: "#2563c7",
  accentBorder: "#376fc7",
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
