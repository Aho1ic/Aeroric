import { fileIconOf } from "./lib/fileIcons";
import { AVATAR_PALETTE, AVATAR_PALETTE_KEYS, autoAvatarColorKey } from "./projectAvatar";
import type { AvatarGradient } from "./projectAvatar";

/**
 * 调色板的渐变对列表。唯一数据源是 `projectAvatar.AVATAR_PALETTE` ——
 * 这里只是给不关心键名的老调用点保留的视图。
 */
export const AVATAR_COLORS: readonly AvatarGradient[] = AVATAR_PALETTE_KEYS.map(
  (key) => AVATAR_PALETTE[key],
);

/** 按名字自动取渐变。定制过的项目要走 `resolveProjectAvatar`,它认 `Project.avatar`。 */
export function getAvatarGradient(name: string): AvatarGradient {
  return AVATAR_PALETTE[autoAvatarColorKey(name)];
}

export function shortenPath(p: string) {
  return p.replace(/^\/Users\/[^/]+/, "~");
}

export function load<T>(key: string, fallback: T): T {
  try {
    const r = localStorage.getItem(key);
    return r ? JSON.parse(r) : fallback;
  } catch {
    return fallback;
  }
}
export function save<T>(key: string, val: T) {
  localStorage.setItem(key, JSON.stringify(val));
}

// ── Usage 颜色工具 ────────────────────────────────────────────────────────────

export function getUsageColor(remainingPercent: number): string {
  if (remainingPercent > 70) return "var(--usage-good)";
  if (remainingPercent >= 20) return "var(--usage-warn)";
  return "var(--usage-danger)";
}

// ── Git 状态工具 ──────────────────────────────────────────────────────────────

export function getGitStatusColor(status: string): string {
  switch (status) {
    case "A":
      return "#3fb950";
    case "D":
      return "#f85149";
    case "M":
      return "#e3b341";
    case "R":
      return "#79c0ff";
    case "?":
      return "#79c0ff";
    case "U":
      return "#f85149";
    default:
      return "var(--text-muted)";
  }
}

export function getGitStatusLabel(status: string): string {
  switch (status) {
    case "A":
      return "A";
    case "D":
      return "D";
    case "M":
      return "M";
    case "R":
      return "R";
    case "?":
      return "U";
    case "U":
      return "!";
    default:
      return status;
  }
}

// ── 文件颜色工具 ──────────────────────────────────────────────────────────────

/**
 * 文件名 → 图标颜色 token。
 *
 * 委托给 `lib/fileIcons` 的那张表:此前这里有一份独立的 switch,和文件树的字形表
 * 各说各话(`.wasm` 有专属颜色但字形是通用文件,`Makefile` 有 build 颜色但字形是
 * 通用文件,`.env` 走 config 颜色而字形按后缀落到 text)。现在颜色和字形同源。
 */
export function getFileColor(name: string, ext?: string): string {
  return fileIconOf(name, ext).color;
}

// ── 文件类型扩展名集合 ────────────────────────────────────────────────────────

export const CODE_EXTS = new Set([
  "ts",
  "tsx",
  "js",
  "jsx",
  "rs",
  "py",
  "go",
  "java",
  "c",
  "cpp",
  "h",
  "css",
  "html",
  "vue",
  "svelte",
  "swift",
  "kt",
]);
