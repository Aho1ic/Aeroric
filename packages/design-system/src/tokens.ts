export const spacing = {
  0: 0,
  1: 4,
  2: 8,
  3: 12,
  4: 16,
  5: 20,
  6: 24,
} as const;

export const radii = {
  sm: 8,
  md: 10,
  lg: 14,
  xl: 18,
  pill: 999,
} as const;

export const typography = {
  label: 11,
  meta: 12,
  body: 14,
  title: 18,
} as const;

export const motion = {
  fast: 120,
  normal: 180,
  slow: 260,
} as const;

export const palettes = {
  light: {
    canvas: "#f4f7fc",
    surface: "#ffffff",
    surfaceElevated: "#f8faff",
    surfaceMuted: "#f1f3f8",
    surfaceGlass: "rgba(255, 255, 255, 0.88)",
    surfaceGlassElevated: "rgba(248, 250, 255, 0.94)",
    surfaceGlassMuted: "rgba(241, 243, 248, 0.82)",
    border: "rgba(39, 39, 42, 0.14)",
    borderStrong: "rgba(109, 40, 217, 0.34)",
    text: "#18181b",
    textSecondary: "#3f3f46",
    textMuted: "#71717a",
    accent: "#6d28d9",
    accentHover: "#5b21b6",
    accentSoft: "rgba(109, 40, 217, 0.13)",
    onAccent: "#ffffff",
    success: "#16a34a",
    warning: "#d97706",
    danger: "#dc2626",
    focus: "#7c3aed",
  },
  eyecare: {
    canvas: "#f1e7d4",
    surface: "#fff8ea",
    surfaceElevated: "#f8eedc",
    surfaceMuted: "#eee0c6",
    surfaceGlass: "rgba(255, 248, 234, 0.9)",
    surfaceGlassElevated: "rgba(248, 238, 220, 0.94)",
    surfaceGlassMuted: "rgba(238, 224, 198, 0.84)",
    border: "rgba(101, 84, 51, 0.2)",
    borderStrong: "rgba(109, 40, 217, 0.34)",
    text: "#3f3724",
    textSecondary: "#5a4f37",
    textMuted: "#7a6c4f",
    accent: "#6d28d9",
    accentHover: "#5b21b6",
    accentSoft: "rgba(139, 92, 246, 0.14)",
    onAccent: "#fffaf0",
    success: "#2f855a",
    warning: "#b7791f",
    danger: "#c53030",
    focus: "#8b5cf6",
  },
  dark: {
    canvas: "#07080d",
    surface: "#101014",
    surfaceElevated: "#18181d",
    surfaceMuted: "#16161b",
    surfaceGlass: "rgba(16, 16, 20, 0.86)",
    surfaceGlassElevated: "rgba(24, 24, 29, 0.92)",
    surfaceGlassMuted: "rgba(22, 22, 27, 0.78)",
    border: "rgba(244, 244, 245, 0.14)",
    borderStrong: "rgba(192, 132, 252, 0.42)",
    text: "#f4f4f5",
    textSecondary: "#d4d4d8",
    textMuted: "#a1a1aa",
    accent: "#c084fc",
    accentHover: "#d8b4fe",
    accentSoft: "rgba(192, 132, 252, 0.18)",
    onAccent: "#18181b",
    success: "#3dd68c",
    warning: "#f5a623",
    danger: "#ff5555",
    focus: "#c084fc",
  },
} as const;

export type ThemeName = keyof typeof palettes;
export type SemanticPalette = (typeof palettes)[ThemeName];

export function cssVariables(theme: ThemeName): Record<string, string> {
  const palette = palettes[theme];
  return Object.fromEntries(
    Object.entries(palette).map(([key, value]) => [
      `--ds-${key.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`)}`,
      value,
    ]),
  );
}
