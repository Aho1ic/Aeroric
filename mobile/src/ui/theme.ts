import {
  palettes,
  radii as sharedRadii,
  spacing as sharedSpacing,
  typography as sharedTypography,
} from "@aeroric/design-system";

/** React Native adapter for the renderer-free shared design tokens. */
const dark = palettes.dark;

export const theme = {
  canvas: dark.canvas,
  bg: dark.surfaceGlass,
  bgCard: dark.surfaceGlassElevated,
  bgElevated: dark.surfaceGlassMuted,
  border: dark.border,
  glassHighlight: "rgba(255, 255, 255, 0.08)",
  text: dark.text,
  textSecondary: dark.textSecondary,
  textHint: dark.textMuted,
  /** 主行动按钮的亮色填充(orca surfaceBright)。 */
  surfaceBright: dark.text,
  accent: dark.accent,
  accentSoft: dark.accentSoft,
  accentPressed: dark.accentHover,
  accentBorder: dark.borderStrong,
  onAccent: dark.onAccent,
  info: dark.accent,
  success: dark.success,
  warning: dark.warning,
  danger: dark.danger,
  purple: "#a78bfa",
} as const;

export const spacing = {
  xs: sharedSpacing[1],
  sm: sharedSpacing[2],
  md: sharedSpacing[3],
  lg: sharedSpacing[4],
  xl: sharedSpacing[6],
} as const;

/** 圆角比 orca 更圆润(按钮/输入 10,卡片 14)。 */
export const radii = {
  row: sharedRadii.md,
  card: sharedRadii.lg,
  button: sharedRadii.md,
  input: sharedRadii.md,
  pill: sharedRadii.pill,
} as const;

export const typography = {
  titleSize: sharedTypography.title,
  bodySize: sharedTypography.body,
  metaSize: sharedTypography.meta,
  labelSize: sharedTypography.label,
} as const;
