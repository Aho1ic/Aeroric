/* 看板视图的模型层:把一篇笔记的「标题 = 列、`- [ ]` = 卡片」折成看板结构。
 *
 * 纯函数,不碰 IPC 也不碰 DOM。
 *
 * ## 不自己扫源码
 *
 * 列来自 `analyzeNote()` 的大纲(标题 + 源码偏移),卡片来自 `noteTasks()`(marked 的
 * token 树 + 行号),归属靠"任务的偏移落在哪一列的区间里"算。两者都是随手记**已有**的
 * 词法器,所以看板和大纲、和阅读态的复选框看的是同一棵树。
 *
 * Markio 的 `parseKanban` 是第三个扫描器:一条 `/^#{1,3}\s+/` 认列头、一条
 * `/^\s*[-*]\s+\[([ xX])\]/` 认任务,**完全没有围栏状态**。于是一段用来说明看板语法的
 * 代码块:
 *
 * ```md
 * # 这是代码块里的标题
 * - [ ] 这是代码块里的例子
 * ```
 *
 * 会变成一个真的列 + 一张真的卡片 —— 而且那张卡片是**可勾的**,点一下就把复选框写进了
 * 代码块里,文档当场损坏。这不是理论风险:看板笔记里写看板语法的说明是最自然的事。
 * (实测过 Markio 那份:上面这段正文产出两列,并且卡片可写。)
 *
 * 走已有词法器之后,"围栏内不算"不需要在这里写一条规则 —— tokenizer 说什么就是什么。
 *
 * ## 与 Markio 的其余分歧
 *
 * 1. **列头 emoji 用 `\p{Extended_Pictographic}`,不是"第一个非 ASCII 字符"。**
 *    Markio 那条是 `/^([^\sA-Za-z0-9])\s*(.*)$/u`,于是 `# 本周计划` 被切成
 *    emoji = `本`、标题 = `周计划` —— 中文列头**每一个**都被啃掉第一个字。俄语、希腊语
 *    同理。见 [[cjk-has-no-word-boundaries]]。
 * 2. **列的身份是源码偏移,不是标题文本。** Markio 的 `appendTaskToColumn` 拿标题去重扫
 *    正文找列头,两列同名(`## 进行中` 出现两次是常事)时永远命中第一个 —— 用户在第二列
 *    点「添加」,任务出现在第一列。
 * 3. **写回前校验列头原文没变。** 同 `toggleTaskLine` 的 `expectChecked`:看板上的偏移
 *    来自一次渲染的快照,正文可能已经被自动保存、外部编辑或另一次操作改过。
 * 4. **列层级由文档结构定,不是"1-3 级都算列"。** Markio 把 `#`~`###` 一律当列,于是
 *    `# 本周计划` + `## 待办` + `## 完成` 变成**三**列,第一列永远是空的。
 * 5. **`bodyLineOffset` 那种长度算术不存在。** Markio 用
 *    `source.slice(0, source.length - body.length)` 反推 frontmatter 占几行 —— 正文里
 *    出现和 frontmatter 尾部相同的字符串时它就错了,而且它假设 body 一定是 source 的
 *    后缀(CRLF 归一化之后不成立)。这里全程只有**一个**坐标系:正文。
 */

import { analyzeNote, type OutlineItem } from "./noteOutline";
import { sectionSpans } from "./noteSections";
import { noteTasks } from "./noteTasks";
import { parseTaskMarks, type TaskMarks } from "./noteTaskInbox";

/** 看板上的一张卡片。 */
export type KanbanCard = TaskMarks & {
  /** 1 起的行号,按**正文**数 —— 和 `toggleTaskLine` 同一个坐标系。 */
  line: number;
  checked: boolean;
  /** 未摘标记的原文。勾选时当乐观锁的比对基准由 `toggleTaskLine` 自己做,这里只用于提示。 */
  raw: string;
};

/** 看板上的一列。 */
export type KanbanColumn = {
  /** 摘掉 emoji 之后的标题。整行只有 emoji 时退回原文,不留空标题。 */
  title: string;
  emoji?: string;
  /** 列头行在**正文**里的起始字符偏移。列的身份就是它。 */
  offset: number;
  /** 列头那一行的原文。写回前用它校验列头还在原处。 */
  headingRaw: string;
  cards: KanbanCard[];
};

export type NoteKanban = {
  columns: KanbanColumn[];
  /**
   * 不在任何列里的任务数(第一个列头之前的那些)。
   *
   * 如实报出来而不是丢掉:看板上看不见它们,但它们在文件里、也在任务收集箱里。数字对不上
   * 的时候用户至少知道"有 2 条没显示",而不是以为自己记错了。
   */
  unplaced: number;
  done: number;
  total: number;
  /** 完成度百分比,四舍五入。`total` 为 0 时是 0。 */
  percent: number;
};

/**
 * 列头开头的 emoji。
 *
 * 三条分支:区域指示符对(国旗 `🇨🇳` —— 它**不是** Extended_Pictographic,少这一条会
 * 匹配失败)、keycap(`1️⃣` 的基字符是 ASCII 数字)、其余图形字符 + 可选的 VS16。
 * 后面跟任意多个 ZWJ 连接的同类,这样 `👨‍👩‍👧`、`🏳️‍🌈` 是**一个** emoji 而不是被切成两半。
 */
const EMOJI_ATOM =
  "(?:\\p{Regional_Indicator}{2}|[0-9#*]\\uFE0F?\\u20E3|\\p{Extended_Pictographic}\\uFE0F?)";
const HEADING_EMOJI_RE = new RegExp(
  `^(${EMOJI_ATOM}(?:\\u200D${EMOJI_ATOM})*)\\s*([\\s\\S]*)$`,
  "u",
);

/** 从 `📥 收件箱` 里分出 emoji 和标题。没有 emoji 就原样返回。 */
export function splitHeadingEmoji(text: string): { emoji?: string; title: string } {
  const match = HEADING_EMOJI_RE.exec(text);
  if (!match) return { title: text };
  const rest = match[2]!.trim();
  // `# 📥` 这种整行只有 emoji:标题退回原文,否则列头是空的、也点不出是哪一列。
  return rest ? { emoji: match[1]!, title: rest } : { title: text };
}

/**
 * 哪一级标题当列。
 *
 * 取**最浅的、至少有两个标题的那一级**;每一级都只有一个标题时取最浅的那一级。
 *
 * 为什么不像 Markio 那样"1-3 级都算列":`# 本周计划` 当大标题、`## 待办` / `## 完成`
 * 当列是最常见的写法,按它的规则会得到三列而第一列永远空着。而纯 `## A` / `## B` 的
 * 平铺写法在这个规则下同样正确 —— 最浅一级有两个标题,就是它。
 */
function columnLevel(outline: readonly OutlineItem[]): number | null {
  const counts = new Map<number, number>();
  for (const item of outline) counts.set(item.level, (counts.get(item.level) ?? 0) + 1);
  const levels = [...counts.keys()].sort((a, b) => a - b);
  if (levels.length === 0) return null;
  for (const level of levels) {
    if ((counts.get(level) ?? 0) >= 2) return level;
  }
  return levels[0]!;
}

/** 每一行的起始字符偏移。行号 → 偏移的换算只在这里做一次。 */
function lineOffsets(body: string): number[] {
  const offsets = [0];
  for (let index = 0; index < body.length; index += 1) {
    if (body.charCodeAt(index) === 10) offsets.push(index + 1);
  }
  return offsets;
}

/** 从偏移取出那一行的原文(不含换行)。 */
function lineAt(body: string, offset: number): string {
  const end = body.indexOf("\n", offset);
  const raw = end < 0 ? body.slice(offset) : body.slice(offset, end);
  return raw.endsWith("\r") ? raw.slice(0, -1) : raw;
}

/**
 * 把正文折成看板。
 *
 * 传的是**正文**(不含 frontmatter):行号和偏移都按正文数,和 `toggleTaskLine`
 * 一致 —— 两个坐标系混起来就是勾错行。
 */
export function parseNoteKanban(body: string): NoteKanban {
  const { outline } = analyzeNote(body ?? "");
  const level = columnLevel(outline);
  const tasks = noteTasks(body ?? "");
  const offsets = lineOffsets(body ?? "");

  const empty: NoteKanban = {
    columns: [],
    unplaced: tasks.length,
    done: tasks.reduce((sum, task) => sum + (task.checked ? 1 : 0), 0),
    total: tasks.length,
    percent: 0,
  };
  if (level === null) return { ...empty, percent: percentOf(empty.done, empty.total) };

  /* 列的区间借 `sectionSpans` 算,不自己找"下一个列头"。它按大纲算 `[from, to)`,
     子标题连同正文一起算进父章节 —— 也就是列里嵌的 `### 小节` 归这一列,正是要的语义。
     自己再写一遍找边界的循环,就又多了一个可能和大纲不一致的地方。 */
  const spans = sectionSpans([...outline], (body ?? "").length);
  const columns: KanbanColumn[] = [];
  const ranges: Array<{ from: number; to: number; column: KanbanColumn }> = [];
  for (let index = 0; index < outline.length; index += 1) {
    const item = outline[index]!;
    if (item.level !== level) continue;
    const span = spans[index]!;
    const { emoji, title } = splitHeadingEmoji(item.text);
    const column: KanbanColumn = {
      title,
      ...(emoji ? { emoji } : {}),
      offset: item.offset,
      headingRaw: lineAt(body ?? "", item.offset),
      cards: [],
    };
    columns.push(column);
    ranges.push({ from: span.from, to: span.to, column });
  }

  let done = 0;
  let unplaced = 0;
  for (const task of tasks) {
    if (task.checked) done += 1;
    const offset = offsets[task.line - 1];
    const hit =
      offset === undefined
        ? undefined
        : ranges.find((range) => offset >= range.from && offset < range.to);
    if (!hit) {
      unplaced += 1;
      continue;
    }
    hit.column.cards.push({
      ...parseTaskMarks(task.text),
      line: task.line,
      checked: task.checked,
      raw: task.text,
    });
  }

  return { columns, unplaced, done, total: tasks.length, percent: percentOf(done, tasks.length) };
}

function percentOf(done: number, total: number): number {
  return total > 0 ? Math.round((done / total) * 100) : 0;
}

/**
 * 在某列末尾插入一条任务,返回新正文;插不进去时返回 `null`。
 *
 * `column` 必须是 `parseNoteKanban` 给出的那一个:它的 `offset` + `headingRaw` 一起构成
 * 乐观锁 —— 列头原文和现在的正文对不上就整个放弃,不猜。Markio 那份是拿标题重扫正文找
 * 列头,同名两列时永远命中第一个(用户在第二列点添加、任务落到第一列),而正文已经变过
 * 的情况它根本不检查。
 */
export function appendCardToColumn(
  body: string,
  column: Pick<KanbanColumn, "offset" | "headingRaw">,
  text: string,
): string | null {
  const trimmed = text.trim();
  if (!trimmed) return null;
  const source = body ?? "";
  // 乐观锁:那一行现在不是当初那个列头,说明正文变过,拒绝写入。
  if (lineAt(source, column.offset) !== column.headingRaw) return null;

  const { outline } = analyzeNote(source);
  const index = outline.findIndex((item) => item.offset === column.offset);
  if (index < 0) return null;
  const span = sectionSpans([...outline], source.length)[index];
  if (!span) return null;

  const lines = source.split("\n");
  const offsets = lineOffsets(source);
  // 列头之后的第一行,和列尾之后的第一行 —— 都换算成行下标。
  const headingLine = offsets.indexOf(column.offset);
  if (headingLine < 0) return null;
  let insertAt = span.to >= source.length ? lines.length : offsets.indexOf(span.to);
  if (insertAt < 0) return null;
  // 往前跳过列尾的空行:新任务要贴着最后一条,不是隔着空行浮在下一列前面。
  while (insertAt > headingLine + 1 && (lines[insertAt - 1] ?? "").trim() === "") insertAt -= 1;

  lines.splice(insertAt, 0, `- [ ] ${trimmed}`);
  return lines.join("\n");
}
