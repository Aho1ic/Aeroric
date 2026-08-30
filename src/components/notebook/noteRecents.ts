/* 「最近打开的笔记」名单。命令面板(⌘K)空查询时列的就是它。
 *
 * 存 localStorage 而不是磁盘:它是纯 UI 便利,不是笔记数据。每打开一篇都写一次
 * `.notebook/*.json` 会让「翻笔记」这个高频动作带上一次写盘,而丢了这份名单的
 * 代价只是空查询下少几行候选。图标和排序落磁盘是因为那两样是用户的编辑结果,
 * 会期待跟着 vault 同步走(见 `noteIcons.ts` 的模块注释)。
 *
 * 键按 vault **相对路径**存,和 icons.json / order.json 一致。存绝对路径的话,
 * vault 换个位置(或同一份笔记在另一台机器上)整份名单会静默全部失效 —— 面板
 * 不报错,只是空查询下什么都不列,用户无从判断是「没打开过」还是「存坏了」。
 */

import { noteIconKey } from "./noteIcons";

/** localStorage 的键前缀。每个 vault 一份 —— 换 vault 不该看到上一个库的历史。 */
const RECENTS_KEY_PREFIX = "aeroric:notebookRecents:";

/** 名单上限。超过就丢最旧的。 */
export const NOTE_RECENTS_LIMIT = 12;

function storageKey(vault: string): string {
  // vault 路径原样进键:同一台机器上两个库不该互相串。
  return `${RECENTS_KEY_PREFIX}${vault.replace(/\\/g, "/").replace(/\/+$/, "")}`;
}

function safeStorage(): Storage | null {
  try {
    // 隐私模式 / 沙盒下访问 localStorage 本身就可能抛。
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

function normalize(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const keys: string[] = [];
  const seen = new Set<string>();
  for (const item of value) {
    if (typeof item !== "string") continue;
    const key = item.trim();
    if (!key || seen.has(key)) continue;
    seen.add(key);
    keys.push(key);
    if (keys.length >= NOTE_RECENTS_LIMIT) break;
  }
  return keys;
}

/** 读某个 vault 的名单,返回 vault 相对路径,最近打开的在前。 */
export function loadNoteRecents(vault: string): string[] {
  const store = safeStorage();
  if (!store || !vault) return [];
  try {
    return normalize(JSON.parse(store.getItem(storageKey(vault)) ?? "[]"));
  } catch {
    // 存坏了(手改过 / 上个版本的格式)当空名单,不要让面板打不开。
    return [];
  }
}

/**
 * 把一条笔记记为「刚打开」,返回新名单(vault 相对路径)。
 *
 * 笔记不在 vault 里时原样返回 —— `noteIconKey` 给空串就说明算不出相对路径,
 * 硬塞一个 `../` 开头的键会让不同库的同名文件互相串。
 */
export function touchNoteRecent(
  vault: string,
  notePath: string,
  current: readonly string[],
): string[] {
  const key = noteIconKey(vault, notePath);
  if (!key) return [...current];
  // 先去重再插到最前:重复打开同一篇只该改变它的位置,不该让名单里出现两条。
  return [key, ...current.filter((item) => item !== key)].slice(0, NOTE_RECENTS_LIMIT);
}

export function saveNoteRecents(vault: string, keys: readonly string[]): void {
  const store = safeStorage();
  if (!store || !vault) return;
  try {
    store.setItem(storageKey(vault), JSON.stringify(normalize([...keys])));
  } catch {
    // 存不下(配额 / 隐私模式)不该让「打开笔记」这个动作失败。
  }
}

/**
 * 相对路径名单 → 当前还存在的笔记 id(绝对路径)。
 *
 * 对不上的条目静默丢掉:笔记被删了、或者被外部改了名,名单里就会留下死条目。
 * 面板拿到不存在的 id 会渲染一行点了没反应的候选,那比少一行糟得多。
 */
export function resolveNoteRecents(
  vault: string,
  keys: readonly string[],
  noteIds: readonly string[],
): string[] {
  if (!vault) return [];
  const byKey = new Map<string, string>();
  for (const id of noteIds) {
    const key = noteIconKey(vault, id);
    if (key) byKey.set(key, id);
  }
  const resolved: string[] = [];
  for (const key of keys) {
    const id = byKey.get(key);
    if (id) resolved.push(id);
  }
  return resolved;
}
