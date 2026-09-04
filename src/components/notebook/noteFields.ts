/* frontmatter 字段:把「全库字段扫描」的结果折成「有哪些 key、各自被写成过哪些值、
 * 每个值命中哪几篇」。
 *
 * 纯函数,不碰 IPC 也不碰 DOM。扫描在 Rust(`notebook/fields.rs`,读全文所以必须在
 * 后端),这里只做聚合。和标签(`noteTags.ts`)、反链(`noteBacklinks.ts`)是同一种
 * 分工。
 *
 * key 大小写不敏感,和标签同一个理由:同一个人先写 `Status` 后写 `status` 是常事,
 * 分成两条对他是纯噪声。**值大小写敏感** —— 值是内容而不是标识符,`done` 和 `Done`
 * 在界面上是两个不同的取值,折起来就等于替用户改数据。
 */

import { compareNotebookPath, compareNotebookText } from "../../lib/notebookSort";

/** Rust 侧 `NoteField`:一篇笔记里的一个字段。 */
export type NoteField = {
  /** key 的原始文本,保持大小写。 */
  key: string;
  /** 这个 key 在这一篇里的值。空数组表示"有这个 key、没有值"。 */
  values: string[];
};

/** Rust 侧 `NoteFieldSource`:一篇笔记的全部 frontmatter 字段。 */
export type NoteFieldSource = {
  /** 绝对路径,与笔记列表里的 `id` 同一个值。 */
  path: string;
  fields: NoteField[];
};

/** 一篇引用了某个 key(或某个 key=value)的笔记。 */
export type FieldNoteHit = {
  path: string;
  title: string;
};

/** 一个值的聚合结果。 */
export type FieldValue = {
  /** 值的原文。 */
  value: string;
  /** 写了这个值的笔记,按路径排。 */
  notes: FieldNoteHit[];
};

/** 一个 key 的聚合结果。 */
export type FieldEntry = {
  /** 归一化 key(小写)。 */
  key: string;
  /** 显示用的原样文本,取第一次出现的那个大小写。 */
  label: string;
  /** 有这个 key 的笔记数。 */
  notes: number;
  /** 这个 key 的全部取值,按篇数降序、同数按值字典序。 */
  values: FieldValue[];
  /**
   * 有这个 key 但没给值的笔记(`k:` 或 `k: []`),按路径排。
   *
   * 给的是笔记而不是一个计数:界面上它和别的取值是同一种行(点开看是哪几篇),
   * 只给数字的话那一行点不开,而"哪些笔记漏填了 status"正是要点开才有用的。
   * 也刻意不把它塞进 `values` 里当一个空串取值 —— 用值域里的哨兵表示"没有值",
   * 迟早会有人把它当成一个真的取值显示出来。
   */
  emptyNotes: FieldNoteHit[];
};

/** 归一化一个 key:去掉两端空白、转小写。 */
export function normalizeFieldKey(key: string): string {
  return key.trim().toLowerCase();
}

/**
 * 折出全库的字段清单。
 *
 * `titleOf` 由调用方给(它手里有链接索引 —— 那份已经合并过内存标题和扫盘标题),
 * 和 `collectTags` 同一个约定:聚合是纯计算,把索引的形状带进来会让它跟着索引变。
 *
 * 顺序:按篇数降序,同数按 key 字典序。回答的是"我这个库里主要在用哪些字段"。
 */
export function collectFields(
  sources: readonly NoteFieldSource[],
  titleOf: (path: string) => string,
): FieldEntry[] {
  type Building = {
    entry: FieldEntry;
    /** 值 → 笔记路径集合。同一篇同一个值只算一次。 */
    values: Map<string, Map<string, FieldNoteHit>>;
    /** 有这个 key 的笔记路径,用来数篇数 —— 一篇写了三个值也只是一篇。 */
    paths: Set<string>;
    /** 有这个 key 无值的笔记,按路径去重。 */
    emptyNotes: Map<string, FieldNoteHit>;
  };
  const map = new Map<string, Building>();
  for (const source of sources) {
    const title = titleOf(source.path);
    for (const field of source.fields) {
      const key = normalizeFieldKey(field.key);
      if (!key) continue;
      let building = map.get(key);
      if (!building) {
        building = {
          entry: { key, label: field.key, notes: 0, values: [], emptyNotes: [] },
          values: new Map(),
          paths: new Set(),
          emptyNotes: new Map(),
        };
        map.set(key, building);
      }
      building.paths.add(source.path);
      if (field.values.length === 0) {
        building.emptyNotes.set(source.path, { path: source.path, title });
        continue;
      }
      for (const value of field.values) {
        let notes = building.values.get(value);
        if (!notes) {
          notes = new Map();
          building.values.set(value, notes);
        }
        notes.set(source.path, { path: source.path, title });
      }
    }
  }
  const entries: FieldEntry[] = [];
  for (const building of map.values()) {
    const values: FieldValue[] = [...building.values.entries()].map(([value, notes]) => ({
      value,
      // 这一次排序是实打实起作用的,不是防御性的:Map 保的是**到达**顺序,而
      // `sources` 只在走真后端时才按路径排好(Rust 侧 `scan_vault_fields` 排了)。
      // 换句话说,顺序的承诺归这一层,不归上游。
      notes: [...notes.values()].sort((a, b) => compareNotebookPath(a.path, b.path)),
    }));
    values.sort((a, b) => b.notes.length - a.notes.length || compareNotebookText(a.value, b.value));
    building.entry.values = values;
    building.entry.notes = building.paths.size;
    building.entry.emptyNotes = [...building.emptyNotes.values()].sort((a, b) =>
      compareNotebookPath(a.path, b.path),
    );
    entries.push(building.entry);
  }
  entries.sort((a, b) => b.notes - a.notes || compareNotebookText(a.key, b.key));
  return entries;
}

/** 按输入筛选字段。空输入返回全部。匹配归一化 key,所以大小写无关。 */
export function filterFields(entries: readonly FieldEntry[], query: string): FieldEntry[] {
  const needle = normalizeFieldKey(query);
  if (!needle) return [...entries];
  return entries.filter((entry) => entry.key.includes(needle));
}

/** 字段总数(有多少个不同的 key)。标题栏那个计数用它。 */
export function countFieldKeys(entries: readonly FieldEntry[]): number {
  return entries.length;
}
