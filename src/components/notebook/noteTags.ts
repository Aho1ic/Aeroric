/* 标签:把「全库标签扫描」的结果折成「有哪些标签、各有几处、分别在哪」。
 *
 * 纯函数,不碰 IPC 也不碰 DOM。扫描在 Rust(`notebook/tags.rs`,读全文所以必须在
 * 后端),这里只做聚合。和反链(`noteBacklinks.ts`)是同一种分工。
 *
 * 大小写不敏感:`#Work` 和 `#work` 折成一条。分开列对用户是纯噪声(他多半根本没
 * 意识到自己两种都写过),而重命名时也必须一起改 —— 只改其中一半的话剩下那些会
 * 突然从标签云里"多出来"一条新的。归一化的 key 用小写,显示用第一次出现的原样。
 */

/** Rust 侧 `NoteTagRef`。 */
export type NoteTagRef = {
  /** 标签文本,不含 `#`,原始大小写。 */
  raw: string;
  /** 1-based 行号(按整个 `.md` 文件数,frontmatter 那几行也算)。 */
  line: number;
  /** 那一行的文本(已 trim,超长截断)。 */
  preview: string;
};

/** Rust 侧 `NoteTagSource`:一篇笔记里的全部标签出现。 */
export type NoteTagSource = {
  /** 绝对路径,与笔记列表里的 `id` 同一个值。 */
  path: string;
  tags: NoteTagRef[];
};

/** 一处标签引用。点一下跳过去。 */
export type TagRefHit = {
  /** 来源笔记的绝对路径。 */
  path: string;
  /** 来源笔记的显示标题。 */
  title: string;
  line: number;
  preview: string;
};

/** 一个标签的聚合结果。 */
export type TagEntry = {
  /** 归一化 key(小写)。重命名按它匹配。 */
  key: string;
  /** 显示用的原样文本,取第一次出现的那个大小写。 */
  label: string;
  /** 出现总处数(不是篇数)。 */
  count: number;
  /** 出现在多少篇笔记里。 */
  notes: number;
  /** 全部引用,按路径再按行号排。 */
  refs: TagRefHit[];
};

/** 归一化一个标签:去掉 `#`、去掉两端空白、摘掉末尾的 `/` 与 `-`、转小写。 */
export function normalizeTag(tag: string): string {
  return tag
    .trim()
    .replace(/^#+/, "")
    .replace(/[/-]+$/, "")
    .toLowerCase();
}

/**
 * 折出全库的标签清单。
 *
 * `titleOf` 由调用方给(它手里有链接索引 —— 那份已经合并过内存标题和扫盘标题)。
 * 这里不自己去查索引:聚合是纯计算,把索引的形状带进来会让它跟着索引一起变。
 *
 * 顺序:按处数降序,同数按字典序。**不**提供"按字母排"的开关 —— 侧栏只有一列宽,
 * 多一个控件就少一行标签,而"我常用哪些标签"是标签云回答的那个问题;要找特定的
 * 标签有筛选框。
 */
export function collectTags(
  sources: readonly NoteTagSource[],
  titleOf: (path: string) => string,
): TagEntry[] {
  const map = new Map<string, TagEntry>();
  for (const source of sources) {
    const title = titleOf(source.path);
    for (const tag of source.tags) {
      const key = normalizeTag(tag.raw);
      if (!key) continue;
      let entry = map.get(key);
      if (!entry) {
        entry = { key, label: tag.raw, count: 0, notes: 0, refs: [] };
        map.set(key, entry);
      }
      entry.count += 1;
      entry.refs.push({ path: source.path, title, line: tag.line, preview: tag.preview });
    }
  }
  const entries = [...map.values()];
  for (const entry of entries) {
    // 引用按路径再按行号 —— `sources` 已按路径排好,这里只需要稳住组内行号。
    entry.refs.sort((a, b) => (a.path === b.path ? a.line - b.line : a.path < b.path ? -1 : 1));
    entry.notes = new Set(entry.refs.map((ref) => ref.path)).size;
  }
  entries.sort((a, b) => b.count - a.count || a.key.localeCompare(b.key));
  return entries;
}

/** 按输入筛选标签。空输入返回全部。匹配的是归一化 key,所以大小写无关。 */
export function filterTags(entries: readonly TagEntry[], query: string): TagEntry[] {
  const needle = normalizeTag(query);
  if (!needle) return [...entries];
  return entries.filter((entry) => entry.key.includes(needle));
}

/** 标签总处数。标签档的标签上那个计数用它。 */
export function countTagRefs(entries: readonly TagEntry[]): number {
  return entries.reduce((sum, entry) => sum + entry.count, 0);
}
