// 预览里的 ```notebook-query 块:按 frontmatter 字段查全库,渲染成一张表。
//
// 语法是逐行 `指令: 值`,四个指令:
//   key    要匹配的 frontmatter 字段名(必填)
//   value  该字段需要包含的值(可选;省略 = 只要有这个字段就算)
//   sort   name(默认) | value
//   limit  最多显示多少行(可选)
// 以 `#` 开头的行是注释,空行忽略。
//
// 和 Markio 的 dataview-lite 有三处刻意不同,每处都对应一个"看起来正常、其实是错的"场景:
//
//   1. **写错的指令要报错,不能静默忽略。** Markio 只认这四个键、其余一概跳过,于是
//      `keys: status`(多打一个 s)会变成"没写 key"、`sort: naem` 会静默退回按名字排。
//      查询块的可怕之处在于错的结果和对的结果长得一模一样 —— 用户没有办法发现。
//   2. **`limit` 截断之后要说清总数。** Markio 表头写的是截断**后**的条数:100 条匹配、
//      `limit: 5`,表头就写"5 条",剩下 95 条无声消失。这里两个数都给。
//   3. **排序是全序。** Markio 只按 name 比,同名笔记(不同目录下的 `index.md`)永远打平,
//      顺序由 sort 实现和输入顺序决定,两次扫描之间会跳。这里一律以 path 收尾。

import { normalizeFieldKey, type NoteFieldSource } from "./noteFields";
import { compareNotebookPath, compareNotebookText } from "../../lib/notebookSort";

/** 一个解析好的查询。 */
export interface NoteQuery {
  key: string;
  /** 省略 = 只要有这个字段就算,不比值。 */
  value?: string;
  sort: "name" | "value";
  limit?: number;
}

/** 解析失败的原因。带上原文,让提示能指出到底哪里写错了。 */
export type QueryProblem =
  | { code: "missingKey" }
  | { code: "unknownDirective"; name: string }
  | { code: "badSort"; value: string }
  | { code: "badLimit"; value: string };

export type ParsedQuery = { ok: true; query: NoteQuery } | { ok: false; problems: QueryProblem[] };

/** 认识的指令。写错的指令要报错,所以这份名单必须是唯一的判据。 */
const DIRECTIVES = new Set(["key", "value", "sort", "limit"]);

/** 一次查询最多渲染多少行。再多的话表格本身就成了性能问题。 */
export const MAX_QUERY_ROWS = 500;

/** 一篇笔记里最多渲染多少个查询块。 */
export const MAX_QUERY_BLOCKS = 20;

/**
 * 解析查询源码。
 *
 * 同一个指令写两遍时后写的生效(和大多数配置格式一致),这一点没有报错 —— 它不会让
 * 结果变得可疑,只是啰嗦。真正会骗人的是"写错的指令被当成没写",所以那个报错。
 */
export function parseNoteQuery(src: string): ParsedQuery {
  const problems: QueryProblem[] = [];
  let key: string | undefined;
  let value: string | undefined;
  let sort: "name" | "value" = "name";
  let limit: number | undefined;

  for (const raw of src.split("\n")) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const at = line.indexOf(":");
    // 没有冒号的行不是指令。按"写错的指令要报错"的同一个理由,它也不能被静默吞掉。
    if (at < 0) {
      problems.push({ code: "unknownDirective", name: line });
      continue;
    }
    const name = line.slice(0, at).trim().toLowerCase();
    const rest = line.slice(at + 1).trim();
    if (!DIRECTIVES.has(name)) {
      problems.push({ code: "unknownDirective", name: name || line });
      continue;
    }
    if (name === "key") {
      key = rest || undefined;
    } else if (name === "value") {
      // 空值 = 不比值。写 `value:` 而不写内容是"任意值"的意思,不是"值等于空串"。
      value = rest || undefined;
    } else if (name === "sort") {
      const lowered = rest.toLowerCase();
      if (lowered === "name" || lowered === "value") {
        sort = lowered;
      } else {
        problems.push({ code: "badSort", value: rest });
      }
    } else {
      // limit:必须是正整数。`0` / 负数 / 非数字都报错 —— Markio 是静默忽略,
      // 那会让 `limit: 0` 表现成"没写 limit",和用户的意图正好相反。
      const parsed = Number(rest);
      if (!Number.isInteger(parsed) || parsed <= 0) {
        problems.push({ code: "badLimit", value: rest });
      } else {
        limit = Math.min(parsed, MAX_QUERY_ROWS);
      }
    }
  }

  if (key === undefined) problems.push({ code: "missingKey" });
  if (problems.length > 0) return { ok: false, problems };
  return { ok: true, query: { key, value, sort, limit } as NoteQuery };
}

/** 结果里的一行。 */
export interface QueryRow {
  path: string;
  title: string;
  /** 这一篇里这个 key 的值,逗号连接。没有值时是空串。 */
  value: string;
}

/** 一次查询的结果。`total` 是**截断前**的条数。 */
export interface QueryResult {
  rows: QueryRow[];
  total: number;
}

/**
 * 在全库字段扫描结果上跑一次查询。
 *
 * **key 大小写不敏感,value 大小写敏感** —— 跟 `noteFields.ts` 同一条规则,理由也是同一个:
 * key 是标识符(同一个人先写 `Status` 后写 `status` 是常事),而值是内容,把 `Done` 和
 * `done` 折起来等于替用户改数据。这条一致性不是小事:字段面板把它们显示成两行,查询要是
 * 一次捞回两行的总和,同一个库就会给出两个互相矛盾的答案。Markio 两边都小写,而它自己的
 * 属性面板是分开显示的 —— 那份矛盾正是这里要避开的。
 */
export function runNoteQuery(
  sources: readonly NoteFieldSource[],
  query: NoteQuery,
  titleOf: (path: string) => string,
): QueryResult {
  const wantKey = normalizeFieldKey(query.key);
  const rows: QueryRow[] = [];
  for (const source of sources) {
    // 同一篇里同一个 key 可能出现多次(大小写不同),值要合起来看:字段面板也是这么折的。
    const values: string[] = [];
    let hasKey = false;
    for (const field of source.fields) {
      if (normalizeFieldKey(field.key) !== wantKey) continue;
      hasKey = true;
      values.push(...field.values);
    }
    if (!hasKey) continue;
    // 指定了 value 时,"有这个 key 但没有值"的笔记不算命中 —— 它没有值可以拿来比。
    if (query.value !== undefined && !values.includes(query.value)) continue;
    rows.push({ path: source.path, title: titleOf(source.path), value: values.join(", ") });
  }

  // 以 path 收尾,保证是全序:同名笔记(不同目录下的 `index.md`)不能靠输入顺序决定先后,
  // 否则两次扫描之间顺序会跳。
  rows.sort((a, b) => {
    const primary =
      query.sort === "value"
        ? compareNotebookText(a.value, b.value)
        : compareNotebookText(a.title, b.title);
    return primary || compareNotebookPath(a.path, b.path);
  });

  const total = rows.length;
  const capped = Math.min(query.limit ?? MAX_QUERY_ROWS, MAX_QUERY_ROWS);
  return { rows: rows.length > capped ? rows.slice(0, capped) : rows, total };
}
