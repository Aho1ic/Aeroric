/* 笔记的自定义图标:可选图标集 + 落盘键的计算。
 *
 * 纯逻辑,不碰 DOM 也不碰 IPC —— 图标名到组件的映射在 `NoteIconPicker.tsx`,
 * 读写在 `notebookApi.ts`。
 *
 * 与 Markio 的差异在存放位置。那边用 zustand persist 存进 tauriStorage,按归一化
 * **绝对路径**索引;这里存进 vault 的 `.notebook/icons.json`,按 vault **相对
 * 路径**索引。两条理由:
 * - 随手记刚刻意从 localStorage 迁到磁盘,图标该跟着笔记走 —— 用户同步或搬走
 *   整个 vault 时,图标不该留在原来那台机器上。
 * - 相对路径让 vault 换个位置之后图标还在(`order.json` 已经是这个决定)。
 */

/** 可选的图标名。存进 icons.json 的就是这些字符串。 */
export const NOTE_ICON_NAMES = [
  "note",
  "book",
  "calendar",
  "target",
  "checkSquare",
  "list",
  "table",
  "image",
  "link",
  "tag",
  "hash",
  "lightbulb",
  "palette",
  "archive",
  "database",
  "cloud",
  "sparkle",
  "message",
  "code",
  "clock",
  "flame",
  "star",
] as const;

export type NoteIconName = (typeof NOTE_ICON_NAMES)[number];

/** 图标名 → i18n 键。选择器的 tooltip 用。 */
export function noteIconLabelKey(name: NoteIconName): string {
  return `notebook.icon.${name}`;
}

export function isNoteIconName(value: string | undefined): value is NoteIconName {
  return value !== undefined && (NOTE_ICON_NAMES as readonly string[]).includes(value);
}

/**
 * 笔记路径 → icons.json 里的键(vault 相对路径,`/` 分隔)。
 *
 * 笔记不在 vault 里时返回空串,调用方据此跳过 —— 给一个 `../` 开头的键会让
 * 同名文件在不同 vault 之间互相串图标。
 */
export function noteIconKey(vault: string, notePath: string): string {
  const root = normalizeSeparators(vault).replace(/\/+$/, "");
  const note = normalizeSeparators(notePath);
  if (!root || !note) return "";
  // 大小写:macOS / Windows 的文件系统通常不敏感,但键本身保留原样。只有前缀
  // 比对用不敏感的形式,否则 vault 路径大小写和笔记路径不一致时会判成"不在库里"。
  if (note.toLowerCase() === root.toLowerCase()) return "";
  if (!note.toLowerCase().startsWith(`${root.toLowerCase()}/`)) return "";
  return note.slice(root.length + 1);
}

/** 取一条笔记的图标。没设过、或表里存的是已经下线的图标名时返回 undefined。 */
export function noteIconOf(
  icons: Readonly<Record<string, string>>,
  vault: string,
  notePath: string,
): NoteIconName | undefined {
  const key = noteIconKey(vault, notePath);
  if (!key) return undefined;
  const name = icons[key];
  // 认不出来的名字当没设 —— 表可能是新版本写的,或者被手改过。渲染一个不存在的
  // 图标会让整行崩掉,而回落到默认图标只是少一点装饰。
  return isNoteIconName(name) ? name : undefined;
}

/**
 * 设置或清除一条笔记的图标,返回新表。
 *
 * `icon` 为 null 表示恢复默认 —— 那时把键**删掉**而不是存空串:空串会在表里
 * 一直占着位置,而且下一版如果给空串赋了含义就会解释成别的东西。
 *
 * 返回同一个对象引用表示"没有变化",调用方据此跳过一次写盘。
 */
export function withNoteIcon(
  icons: Readonly<Record<string, string>>,
  vault: string,
  notePath: string,
  icon: NoteIconName | null,
): Record<string, string> {
  const key = noteIconKey(vault, notePath);
  if (!key) return icons;
  const current = icons[key];
  if (icon === null) {
    if (current === undefined) return icons;
    const next = { ...icons };
    delete next[key];
    return next;
  }
  if (current === icon) return icons;
  return { ...icons, [key]: icon };
}

function normalizeSeparators(input: string): string {
  return input.replace(/\\/g, "/");
}
