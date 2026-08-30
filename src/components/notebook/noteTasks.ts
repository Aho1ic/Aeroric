/* 笔记正文里的 `- [ ]` 任务项:定位、勾选、结构化解析。
 *
 * 阅读态的复选框要能点。Markio 那边(`markdownTasks.ts`)的做法是用一条手写正则从源码
 * 里挑出任务行,再按**出现顺序**和 DOM 里第 N 个 `<input type=checkbox>` 对齐。它自己的
 * 注释就写着这个风险:"必须与渲染器实际产出的复选框一一对应,否则 DOM 第 N 个与源码第 N
 * 个错位,点击会勾错行" —— 于是那条正则要手工追平渲染器的全部行为(围栏内不算、有序
 * 列表算、blockquote 前缀算……),而追不平的表现是**勾错别人那一行**,一种静默的数据
 * 损坏。
 *
 * 这里换成:任务项的定义只有一个来源 —— marked 自己的 tokenizer。深度优先走一遍 token
 * 树,用一个只前进的游标在源码里定位每个 token 的 `raw`,就能拿到每个任务项的行号。
 * 渲染器和这里看的是同一棵树,所以"哪些是任务项、什么顺序"由构造保证一致,不靠追平。
 *
 * 而且这一趟走在**原始源码**上,不是 `extractMath` 之后的文本:那一步会把多行的 `$$`
 * 块压成一行哨兵,后面每一行的行号都会往前挪。
 */

import { Marked, type Token, type Tokens } from "marked";

/** 一个任务项。`line` 是 1 起的行号,对着**正文**(不含 frontmatter)数。 */
export type NoteTask = {
  line: number;
  checked: boolean;
  /** 任务文本(markdown 原文,不含 `- [ ]` 前缀)。 */
  text: string;
};

/**
 * 找出正文里所有任务项,按文档顺序。
 *
 * 围栏里的 `- [ ]` 不算(marked 不会为它产复选框),有序列表 `1. [ ]` 和 blockquote 里的
 * `> - [ ]` 算(它会产)—— 这些都不用在这里写规则,tokenizer 说什么就是什么。
 */
export function noteTasks(source: string): NoteTask[] {
  if (!source) return [];
  // 每次新建:Marked 实例持有状态,复用会让相邻两次互相影响(和 noteRender 同一个理由)。
  const tokens = new Marked().lexer(source);
  const found: NoteTask[] = [];
  /** 只前进的游标。token 按源码顺序出现,所以从上一次的位置往后找就够。 */
  let cursor = 0;

  const walk = (list: Token[]): void => {
    for (const token of list) {
      const raw = typeof token.raw === "string" ? token.raw : "";
      const at = raw ? source.indexOf(raw, cursor) : -1;

      if (at < 0) {
        /* 找不到 `raw`:blockquote 之类会剥掉行首前缀,子 token 的 raw 不是父 raw 的字面
           子串。不动游标继续往下走 —— 孙子层通常还能对上(`> - [ ]` 里那个 list_item 的
           raw 就是 `- [ ]  …`,在同一行里找得到)。 */
        walkChildren(token);
        continue;
      }

      if (token.type === "list_item" && (token as Tokens.ListItem).task) {
        const item = token as Tokens.ListItem;
        found.push({
          line: countLines(source, at),
          checked: item.checked === true,
          // 只要第一行的文本:多行任务项后面几行是它的子内容,不是任务本身。
          text: taskTextOf(raw),
        });
      }

      /* 先递归再前进。父 token 的 `raw` 把子 token 整个包在里面(嵌套列表项就在其中),
         直接跳到 `at + raw.length` 会把嵌套的任务项整片漏掉。 */
      cursor = at;
      walkChildren(token);
      cursor = Math.max(cursor, at + raw.length);
    }
  };

  const walkChildren = (token: Token): void => {
    const asList = token as Tokens.List;
    if (Array.isArray(asList.items)) walk(asList.items);
    const nested = (token as { tokens?: Token[] }).tokens;
    if (Array.isArray(nested)) walk(nested);
  };

  walk(tokens);
  return found;
}

/** `at` 之前有几个换行,+1 就是 1 起的行号。 */
function countLines(source: string, at: number): number {
  let lines = 1;
  for (let index = 0; index < at; index += 1) {
    if (source.charCodeAt(index) === 10) lines += 1;
  }
  return lines;
}

/** 从 `- [ ] 写周报` 里取出 `写周报`。 */
function taskTextOf(raw: string): string {
  const firstLine = raw.split("\n", 1)[0] ?? "";
  return firstLine.replace(/^\s*(?:>\s*)*(?:[-*+]|\d+[.)])\s+\[[ xX]\]\s*/, "").trim();
}

/** 勾选框那一格,连同它前面的列表标记。 */
const TASK_MARK_RE = /^((?:\s*>)*\s*(?:[-*+]|\d+[.)])\s+\[)([ xX])(\])/;

/**
 * 翻转某一行的勾选状态,返回新正文;那一行不是任务项就返回 `null`。
 *
 * `expectChecked` 是**乐观锁**:调用方传它渲染那一刻看到的状态,和当前源码不符就拒绝
 * 写入。阅读态的复选框来自一次渲染的快照,而正文可能已经被自动保存、外部编辑或另一次
 * 勾选改过 —— 那时候按旧行号写下去就是勾错别人那一行。这是本模块唯一防得住"静默勾错"
 * 的地方,所以不能省。
 */
export function toggleTaskLine(
  source: string,
  line: number,
  expectChecked?: boolean,
): string | null {
  const lines = source.split("\n");
  const target = lines[line - 1];
  if (target == null) return null;
  const match = TASK_MARK_RE.exec(target);
  if (!match) return null;
  const checked = match[2]!.toLowerCase() === "x";
  if (expectChecked !== undefined && checked !== expectChecked) return null;
  lines[line - 1] = `${match[1]}${checked ? " " : "x"}${match[3]}${target.slice(match[0].length)}`;
  return lines.join("\n");
}
