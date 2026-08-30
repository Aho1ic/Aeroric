/* 任务收集箱的模型层:把全库扫来的任务行折成一张可分组、可筛选的清单。
 *
 * 纯函数,不碰 IPC 也不碰 DOM。扫描在 Rust(`notebook/tasks.rs`,读全文所以必须在
 * 后端),这里做标记解析与聚合 —— 和标签(`noteTags.ts`)、字段(`noteFields.ts`)
 * 是同一种分工。
 *
 * ## 和 `noteTasks.ts` 的分工
 *
 * `noteTasks.ts` 是**写回**用的:它走 marked 的 token 树,行号按正文数,负责"勾选
 * 不能勾错行"。这一份是**只读视图**用的:行号按整个 `.md` 文件数(和标签/反链同一个
 * 坐标系,好共用跳转),只把用户送到那一行。两个坐标系不能混,见 `tasks.rs` 的模块
 * 注释。
 *
 * ## 和 Markio 的三处分歧
 *
 * 1. **`dueBucket` 不用 `toISOString()` 取今天。** Markio 那份是
 *    `now.toISOString().slice(0, 10)` —— 那是 **UTC** 日期。UTC+8 在早上 08:00 之前,
 *    它给的是昨天:今天到期的任务被分到「明天」,已过期的分到「今天」。整个分组
 *    偏移一天,而这是每天早上都会发生的事。这里按**本地**日期算。
 * 2. **裸 ISO 日期只在首尾算截止。** Markio 的兜底是"行内任意位置出现的 ISO 日期也
 *    认",于是 `- [ ] 复盘 2026-08-01 那次故障` 会凭空长出一个截止日期。截止日期是
 *    **元数据**,写在首尾;夹在句子中间的日期是正文在说事。
 * 3. **排序是全序。** Markio 的比较器以 `text.localeCompare` 收尾,两篇笔记里同名的
 *    任务永远打平 —— 那时候顺序由 `sort` 的实现和输入顺序决定,两次扫描之间会跳。
 *    这里以路径 + 行号收尾。
 */

import { normalizeTag } from "./noteTags";

/** Rust 侧 `NoteTaskRef`。 */
export type NoteTaskRef = {
  /** 1-based 行号,按整个 `.md` 文件数(frontmatter 那几行也算)。 */
  line: number;
  checked: boolean;
  /** 任务原文,标记(`#tag`、`@日期`、`!优先级`)都还在里面。 */
  text: string;
};

/** Rust 侧 `NoteTaskSource`:一篇笔记里的全部任务。 */
export type NoteTaskSource = {
  /** 绝对路径,与笔记列表里的 `id` 同一个值。 */
  path: string;
  tasks: NoteTaskRef[];
};

export type TaskPriority = "high" | "med" | "low";

/** 从任务原文里解析出的标记。 */
export type TaskMarks = {
  /** 摘掉标记、折叠空白之后的显示文本。可能是空串(整行只有标记)。 */
  text: string;
  /** 标签,原样大小写,按归一化 key 去重后保持出现顺序。不含 `#`。 */
  tags: string[];
  /** 截止日期,`YYYY-MM-DD`。 */
  due?: string;
  priority?: TaskPriority;
};

/** 收集箱里的一条任务。 */
export type InboxTask = TaskMarks & {
  /** 来源笔记的绝对路径。 */
  path: string;
  /** 来源笔记的显示标题。 */
  title: string;
  /** 1-based 行号,按整个 `.md` 文件数。 */
  line: number;
  checked: boolean;
  /** 未摘标记的原文。悬浮提示和「复制任务文本」用它 —— 用户写的是这个。 */
  raw: string;
};

/**
 * 标签正文的字符上限,和 Rust 侧 `MAX_TAG_CHARS` 对齐。
 *
 * 两边不一致的后果:某条超长"标签"在收集箱里是 200 字、在标签云里是 64 字,于是同
 * 一个筛选词能命中其中一处、命中不了另一处。
 */
const MAX_TAG_CHARS = 64;

/**
 * 标签:`#` 前面只允许行首或空白,标签字符是 `字母数字 | _ | - | /`。
 *
 * 这一套照 Rust 侧 `is_tag_char` / `line_tags` 抄的,**不是** Markio 那条
 * `[\w一-鿿/.-]`:口径不一致的话,任务上那些标签和标签云里的对不上(Markio 的含 `.`,
 * 而 Aeroric 的不含 —— 于是 `#v1.2` 在一处是一个标签、在另一处是两个)。
 */
const TASK_TAG_RE = /(^|\s)#([\p{L}\p{N}_/-]+)/gu;

const EMOJI_PRIORITY: ReadonlyArray<[string, TaskPriority]> = [
  ["🔴", "high"],
  ["🟡", "med"],
  ["🟢", "low"],
];

/**
 * 解析一条任务原文里的标记。
 *
 * 支持的写法(在文本里出现的顺序任意):
 * - `#tag` → 标签
 * - `📅 YYYY-MM-DD` / `@YYYY-MM-DD` / 末尾 `(YYYY-MM-DD)` / 首尾的裸 `YYYY-MM-DD` → 截止
 * - `!high` / `!med` / `!low`、`🔴` / `🟡` / `🟢` → 优先级(emoji 优先)
 *
 * 取到的标记都从文本里**摘掉**:留着的话同一个信息会显示两遍(一遍在日期徽标上,
 * 一遍在任务文本里)。
 */
export function parseTaskMarks(raw: string): TaskMarks {
  let text = raw;

  const tags: string[] = [];
  const seen = new Set<string>();
  text = text.replace(TASK_TAG_RE, (_match, lead: string, name: string) => {
    const clipped = [...name].slice(0, MAX_TAG_CHARS).join("");
    const key = normalizeTag(clipped);
    // 归一化后空掉的(比如 `#-`)不算标签,原文留在文本里 —— 那多半是别的意思。
    if (!key) return `${lead}#${name}`;
    if (!seen.has(key)) {
      seen.add(key);
      tags.push(clipped);
    }
    return lead;
  });

  let priority: TaskPriority | undefined;
  for (const [emoji, level] of EMOJI_PRIORITY) {
    if (!text.includes(emoji)) continue;
    priority = level;
    text = text.split(emoji).join("");
    break;
  }
  if (!priority) {
    const word = /(^|\s)!(high|med|low)\b/.exec(text);
    if (word) {
      priority = word[2] as TaskPriority;
      text = text.replace(/(^|\s)!(?:high|med|low)\b/, "$1");
    }
  }

  let due: string | undefined;
  for (const pattern of [
    /📅\s*(\d{4}-\d{2}-\d{2})/,
    /(?:^|\s)@(\d{4}-\d{2}-\d{2})\b/,
    /\((\d{4}-\d{2}-\d{2})\)\s*$/,
    /* 裸日期只认首尾。夹在句子中间的是正文在说事,不是截止日期 —— 见模块注释。 */
    /^\s*(\d{4}-\d{2}-\d{2})\b/,
    /(?:^|\s)(\d{4}-\d{2}-\d{2})\s*$/,
  ]) {
    const hit = pattern.exec(text);
    if (!hit) continue;
    if (!isRealDate(hit[1]!)) continue;
    due = hit[1];
    text = text.replace(pattern, " ");
    break;
  }

  return { text: text.replace(/\s+/g, " ").trim(), tags, due, priority };
}

/**
 * `YYYY-MM-DD` 是不是真实存在的日期。
 *
 * `2026-02-30` 形状对但不存在。放进去的话它会排在 2 月末之后、3 月之前的某个位置,
 * 而分桶时 `new Date` 会把它折成 3 月 2 日 —— 于是同一条任务在"排序"和"分到哪一桶"
 * 两件事上是两个日期。
 */
function isRealDate(iso: string): boolean {
  const [year, month, day] = iso.split("-").map(Number) as [number, number, number];
  if (month < 1 || month > 12 || day < 1) return false;
  const probe = new Date(Date.UTC(year, month - 1, day));
  return probe.getUTCMonth() === month - 1 && probe.getUTCDate() === day;
}

/**
 * 今天的**本地**日期,`YYYY-MM-DD`。
 *
 * 不用 `toISOString().slice(0, 10)` —— 那是 UTC 日期,见模块注释第 1 条。
 */
export function todayIso(now: Date = new Date()): string {
  const month = String(now.getMonth() + 1).padStart(2, "0");
  const day = String(now.getDate()).padStart(2, "0");
  return `${now.getFullYear()}-${month}-${day}`;
}

/**
 * ISO 日期加天数,仍返回 ISO。
 *
 * 算术走 UTC:那里没有夏令时。用本地时间的话,跨过夏令时切换的那一天"加一天"会
 * 得到 23 或 25 小时后 —— 落在同一天或跳过一天,于是「明天」那一桶在每年两天里是空的。
 */
function addDays(iso: string, days: number): string {
  const [year, month, day] = iso.split("-").map(Number) as [number, number, number];
  const next = new Date(Date.UTC(year, month - 1, day) + days * 86_400_000);
  const nextMonth = String(next.getUTCMonth() + 1).padStart(2, "0");
  const nextDay = String(next.getUTCDate()).padStart(2, "0");
  return `${next.getUTCFullYear()}-${nextMonth}-${nextDay}`;
}

export type DueBucket = "overdue" | "today" | "tomorrow" | "thisWeek" | "later" | "none";

/**
 * 把截止日期分到哪一桶。`today` 由调用方给(`todayIso()`),测试里可以直接钉住。
 *
 * `thisWeek` 是「今天起 7 天内」,不是「本周」—— 自然周的边界在周日晚上会让一整批
 * 任务无缘无故换桶,而用户关心的是"还剩几天"。
 */
export function dueBucket(due: string | undefined, today: string): DueBucket {
  if (!due) return "none";
  if (due < today) return "overdue";
  if (due === today) return "today";
  if (due === addDays(today, 1)) return "tomorrow";
  if (due <= addDays(today, 7)) return "thisWeek";
  return "later";
}

const PRIORITY_RANK: Record<TaskPriority | "_", number> = { high: 0, med: 1, low: 2, _: 3 };

/**
 * 排序:未完成优先 → 优先级 → 截止日期(有的在前)→ 文本 → 路径 → 行号。
 *
 * 未完成排在前面:开着「显示已完成」时,做完的那些是参考资料,不该插在待办中间。
 *
 * 以路径 + 行号收尾而不是以文本收尾(Markio 那样):同名任务打平的话顺序由 `sort`
 * 的实现和输入顺序决定,两次扫描之间会跳。
 */
export function compareTasks(a: InboxTask, b: InboxTask): number {
  if (a.checked !== b.checked) return a.checked ? 1 : -1;
  const rank = PRIORITY_RANK[a.priority ?? "_"] - PRIORITY_RANK[b.priority ?? "_"];
  if (rank !== 0) return rank;
  if (a.due !== b.due) {
    if (!a.due) return 1;
    if (!b.due) return -1;
    return a.due < b.due ? -1 : 1;
  }
  const byText = a.text.localeCompare(b.text);
  if (byText !== 0) return byText;
  if (a.path !== b.path) return a.path < b.path ? -1 : 1;
  return a.line - b.line;
}

/**
 * 折出全库的任务清单。
 *
 * `titleOf` 由调用方给(它手里有链接索引 —— 那份已经合并过内存标题和扫盘标题),和
 * 标签档、字段浏览器共用同一条口径:三处不一致的话同一篇笔记在一个视图里显示文件名、
 * 在另一个里显示真标题。
 */
export function collectInboxTasks(
  sources: readonly NoteTaskSource[],
  titleOf: (path: string) => string,
): InboxTask[] {
  const out: InboxTask[] = [];
  for (const source of sources) {
    const title = titleOf(source.path);
    for (const task of source.tasks) {
      out.push({
        ...parseTaskMarks(task.text),
        path: source.path,
        title,
        line: task.line,
        checked: task.checked,
        raw: task.text,
      });
    }
  }
  return out.sort(compareTasks);
}

/**
 * 按输入筛选。空输入返回全部(仍受 `showDone` 约束)。
 *
 * 匹配任务文本、标签、笔记标题。**不**匹配原文 —— 那样 `@2026` 这种输入会命中一批
 * 文本里根本看不到那个词的任务,用户会以为筛选坏了。
 */
export function filterInboxTasks(
  tasks: readonly InboxTask[],
  query: string,
  showDone: boolean,
): InboxTask[] {
  const needle = query.trim().toLowerCase();
  return tasks.filter((task) => {
    if (task.checked && !showDone) return false;
    if (!needle) return true;
    return (
      task.text.toLowerCase().includes(needle) ||
      task.title.toLowerCase().includes(needle) ||
      task.tags.some((tag) => tag.toLowerCase().includes(needle))
    );
  });
}

export type TaskGroupMode = "time" | "priority" | "note";

/** 一个分组。`key` 用于 React key 与文案查表,`label` 由调用方翻译。 */
export type TaskGroup = {
  key: string;
  /** 分组的语义值:时间桶 / 优先级 / 笔记路径。 */
  kind: TaskGroupMode;
  /** 「按笔记」时是笔记标题;另两档由调用方按 `key` 查 i18n。 */
  title: string;
  tasks: InboxTask[];
};

const TIME_ORDER: DueBucket[] = ["overdue", "today", "tomorrow", "thisWeek", "later", "none"];
const PRIORITY_ORDER: Array<TaskPriority | "_"> = ["high", "med", "low", "_"];

/**
 * 分组。空组不出现 —— 六个时间桶里通常只有两三个有内容,把空的画出来会让清单看起来
 * 全是标题。
 *
 * 组内顺序一律 `compareTasks`;组间顺序按预定义序列(时间 / 优先级)或标题字典序
 * (笔记)。后者不用文件系统顺序:那个会随目录项排列漂移。
 */
export function groupInboxTasks(
  tasks: readonly InboxTask[],
  mode: TaskGroupMode,
  today: string,
): TaskGroup[] {
  const buckets = new Map<string, InboxTask[]>();
  const keyOf = (task: InboxTask): string => {
    if (mode === "time") return dueBucket(task.due, today);
    if (mode === "priority") return task.priority ?? "_";
    return task.path;
  };
  for (const task of tasks) {
    const key = keyOf(task);
    const list = buckets.get(key);
    if (list) list.push(task);
    else buckets.set(key, [task]);
  }

  const order =
    mode === "time"
      ? TIME_ORDER.map(String)
      : mode === "priority"
        ? PRIORITY_ORDER.map(String)
        : [...buckets.keys()].sort((a, b) => {
            const left = buckets.get(a)?.[0]?.title ?? a;
            const right = buckets.get(b)?.[0]?.title ?? b;
            return left.localeCompare(right) || (a < b ? -1 : a > b ? 1 : 0);
          });

  const groups: TaskGroup[] = [];
  for (const key of order) {
    const list = buckets.get(key);
    if (!list || list.length === 0) continue;
    groups.push({
      key,
      kind: mode,
      title: mode === "note" ? (list[0]?.title ?? key) : key,
      tasks: [...list].sort(compareTasks),
    });
  }
  return groups;
}

/** 未完成的条数。收集箱按钮上那个角标用它 —— 已完成的不该让角标一直亮着。 */
export function countOpenTasks(tasks: readonly InboxTask[]): number {
  return tasks.reduce((sum, task) => sum + (task.checked ? 0 : 1), 0);
}
