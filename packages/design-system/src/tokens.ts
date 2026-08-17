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
    canvas: "#050607",
    surface: "#0c0f13",
    surfaceElevated: "#13171d",
    surfaceMuted: "#11151a",
    surfaceGlass: "rgba(12, 15, 19, 0.82)",
    surfaceGlassElevated: "rgba(19, 23, 29, 0.88)",
    surfaceGlassMuted: "rgba(17, 21, 26, 0.76)",
    border: "rgba(171, 178, 191, 0.18)",
    borderStrong: "rgba(97, 175, 239, 0.46)",
    text: "#d7dae0",
    textSecondary: "#abb2bf",
    textMuted: "#7f8797",
    accent: "#61afef",
    accentHover: "#79c0ff",
    accentSoft: "rgba(82, 139, 255, 0.18)",
    onAccent: "#f7fbff",
    success: "#98c379",
    warning: "#e5c07b",
    danger: "#e06c75",
    focus: "#61afef",
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
