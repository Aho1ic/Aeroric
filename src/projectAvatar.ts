import type { Project, ProjectAvatarOverride } from "./types";

/**
 * 项目头像的调色板与解析逻辑。
 *
 * 存的是**键名**(`ProjectAvatarOverride.color`)而不是色值:换主题时这里可以整体
 * 重调,已定制的项目跟着变,不会卡在一个和新主题打架的硬编码颜色上。
 * 键名因此是持久化契约的一部分 —— 改名等于让老 projects.json 里的定制失效。
 */

/** 渐变色对:`[起, 止]`,直接进 `linear-gradient`。 */
export type AvatarGradient = readonly [string, string];

/**
 * 键 -> 渐变。刻意铺满色轮:上游 nezha 的 10 色全挤在蓝-青-绿一段,
 * 20 个项目排在竖条里几乎分不开,这是 NZ-2 要解决的问题本身。
 */
export const AVATAR_PALETTE: Readonly<Record<string, AvatarGradient>> = {
  slate: ["#64748B", "#475569"],
  red: ["#E1544F", "#B93B37"],
  orange: ["#E27B3A", "#BC5C22"],
  amber: ["#D9A21B", "#B07C0C"],
  lime: ["#7CAF2C", "#5C8A18"],
  green: ["#2FA36B", "#1C7C4E"],
  teal: ["#0D9488", "#0F6B64"],
  cyan: ["#0E9AC4", "#0A7391"],
  blue: ["#2563D6", "#1E4FA8"],
  indigo: ["#4F63D7", "#3F46A6"],
  violet: ["#7B4CC7", "#61369C"],
  fuchsia: ["#B646BE", "#8C2F94"],
  pink: ["#D8447F", "#AC2A5F"],
  rose: ["#DE4B60", "#B22F45"],
};

/** 自动取色的候选顺序。对象键序不该被依赖,所以显式钉一份。 */
export const AVATAR_PALETTE_KEYS: readonly string[] = [
  "slate",
  "red",
  "orange",
  "amber",
  "lime",
  "green",
  "teal",
  "cyan",
  "blue",
  "indigo",
  "violet",
  "fuchsia",
  "pink",
  "rose",
];

/** 名字推不出颜色时(空名)用它,保证渲染永远有色。 */
export const DEFAULT_AVATAR_COLOR_KEY = "blue";

/** 首字母的显示宽度上限。拉丁字符算 1,CJK / 全角算 1.5。 */
const MAX_LABEL_WIDTH = 3;

/** 用 FNV-1a:老的 `hash * 31` 对 `project-alpha` / `project-beta` 这类同前缀名区分度差。 */
function hashName(name: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < name.length; i++) {
    hash ^= name.charCodeAt(i);
    // Math.imul 保证 32 位回绕,不会掉进 double 精度。
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

/** 按名字自动选调色板键。同名恒定同色。 */
export function autoAvatarColorKey(name: string): string {
  if (!name) return DEFAULT_AVATAR_COLOR_KEY;
  return AVATAR_PALETTE_KEYS[hashName(name) % AVATAR_PALETTE_KEYS.length];
}

/** 按 grapheme 切分,让 emoji(含 ZWJ 序列、肤色修饰)算一个字符。 */
function graphemesOf(value: string): string[] {
  // Safari 16 之前没有 Intl.Segmenter;缺了就退化到 code point 切分。
  const Segmenter = (
    Intl as typeof Intl & {
      Segmenter?: new (
        locale?: string,
        options?: { granularity: "grapheme" },
      ) => { segment: (input: string) => Iterable<{ segment: string }> };
    }
  ).Segmenter;
  if (!Segmenter) return Array.from(value);
  const segmenter = new Segmenter(undefined, { granularity: "grapheme" });
  return Array.from(segmenter.segment(value), (part) => part.segment);
}

/** CJK / 全角占两格宽,按 1.5 计:两个汉字正好压在上限 3 以内。 */
function displayWidthOf(grapheme: string): number {
  const code = grapheme.codePointAt(0) ?? 0;
  const isWide =
    (code >= 0x1100 && code <= 0x115f) || // 韩文字母
    (code >= 0x2e80 && code <= 0xa4cf) || // CJK 部首 / 假名 / 汉字
    (code >= 0xac00 && code <= 0xd7a3) || // 韩文音节
    (code >= 0xf900 && code <= 0xfaff) || // CJK 兼容汉字
    (code >= 0xfe30 && code <= 0xfe6f) || // CJK 兼容形式
    (code >= 0xff00 && code <= 0xff60) || // 全角
    (code >= 0xffe0 && code <= 0xffe6) ||
    (code >= 0x20000 && code <= 0x3fffd); // CJK 扩展 B+
  return isWide ? 1.5 : 1;
}

/** 截到 `MAX_LABEL_WIDTH` 显示宽度为止,不切断 grapheme。 */
function clampLabel(value: string): string {
  let width = 0;
  let out = "";
  for (const grapheme of graphemesOf(value)) {
    const next = width + displayWidthOf(grapheme);
    if (next > MAX_LABEL_WIDTH) break;
    width = next;
    out += grapheme;
  }
  return out;
}

/**
 * 按名字自动取首字母。
 *
 * 拉丁名取「首字母 + 分隔符后的首字母」(`agent-config` -> `ac`);
 * CJK 没有分隔符习惯,取前两个字(`语音助手` -> `语音`)。
 */
export function autoAvatarLabel(name: string): string {
  const trimmed = name.trim();
  if (!trimmed) return "";
  const graphemes = graphemesOf(trimmed);
  const first = graphemes[0];
  if (displayWidthOf(first) > 1) return clampLabel(graphemes.slice(0, 2).join(""));
  const afterSeparator = trimmed.match(/[-_.\s]+([\p{L}\p{N}])/u)?.[1];
  if (afterSeparator) return first + afterSeparator;
  return clampLabel(graphemes.slice(0, 2).join(""));
}

/** 渲染头像所需的全部信息:定制项与自动值已经合并完。 */
export interface ResolvedProjectAvatar {
  /** 生效的调色板键。 */
  colorKey: string;
  gradient: AvatarGradient;
  /** 设了 emoji 就不显示 `label`。 */
  emoji?: string;
  /** emoji 缺席时显示的首字母。 */
  label: string;
}

/** 定制项覆盖自动值,逐字段回落。非法调色板键当作未定制。 */
export function resolveProjectAvatar(
  name: string,
  override?: ProjectAvatarOverride,
): ResolvedProjectAvatar {
  const overrideKey = override?.color;
  const colorKey =
    overrideKey && Object.hasOwn(AVATAR_PALETTE, overrideKey)
      ? overrideKey
      : autoAvatarColorKey(name);
  const emoji = override?.emoji ? clampEmoji(override.emoji) : undefined;
  return {
    colorKey,
    gradient: AVATAR_PALETTE[colorKey],
    emoji,
    // emoji 在位时 label 不显示,但仍解析出来:去掉 emoji 就立刻回到它。
    label: override?.label ? clampLabel(override.label) : autoAvatarLabel(name),
  };
}

/** 只留第一个 grapheme:粘贴一串 emoji 时不该把头像撑破。 */
function clampEmoji(value: string): string {
  return graphemesOf(value.trim())[0] ?? "";
}

/**
 * 把用户输入收敛成可持久化的定制项。
 *
 * 全空时返回 `undefined`,这样 `projects.json` 里不会长出 `"avatar": {}`,
 * 「清空定制」也就是存一个 `undefined`,与从未定制过完全同构。
 */
export function normalizeAvatarOverride(value: unknown): ProjectAvatarOverride | undefined {
  if (!value || typeof value !== "object") return undefined;
  const raw = value as Record<string, unknown>;
  const next: ProjectAvatarOverride = {};

  const color = typeof raw.color === "string" ? raw.color.trim() : "";
  if (color && Object.hasOwn(AVATAR_PALETTE, color)) next.color = color;

  const emoji = typeof raw.emoji === "string" ? clampEmoji(raw.emoji) : "";
  if (emoji) next.emoji = emoji;

  const label = typeof raw.label === "string" ? clampLabel(raw.label.trim()) : "";
  if (label) next.label = label;

  return next.color || next.emoji || next.label ? next : undefined;
}

/** 便利入口:直接吃 `Project`。 */
export function projectAvatarOf(project: Pick<Project, "name" | "avatar">): ResolvedProjectAvatar {
  return resolveProjectAvatar(project.name, project.avatar);
}
