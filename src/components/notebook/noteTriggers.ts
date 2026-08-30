/* 编辑器内触发式菜单的检测层:光标前的文本是不是刚打出了一个触发序列。
 *
 * 纯函数,不碰 DOM、不碰 CodeMirror。菜单本体在 `NoteTriggerMenu.tsx`,候选表在
 * `noteCompletions.ts`。
 *
 * ## 为什么要认围栏
 *
 * Markio 那边的检测只看当前行光标之前的那一段,不判是否在代码块里。于是在代码块里
 * 写 `arr[[i]]`、写注释里的 `# TODO`、写 Python 装饰器 `@cache`、写 CSS 的 `a:hover`
 * ——每一个都会弹出补全菜单并劫持方向键和回车。这不是小概率:随手记里贴代码是主要
 * 用途之一。所以这里先判围栏和行内代码,再谈触发(见 memory
 * `markio-parsers-are-fence-blind`)。
 *
 * ## 为什么记 `start` 而不是 `triggerLen`
 *
 * Markio 提交时算的是 `deleteBeforeCursor(triggerLen + query.length)` —— 那要求
 * 「触发符长度 + 查询长度」始终等于「触发符起点到光标」的真实距离。中间只要有一处
 * 不成立(查询里带了空格、光标被别的逻辑挪过、`query` 是上一次检测留下的),删除
 * 范围就会错位,而错位的表现是吃掉用户前面的正文。这里直接给出触发符在文档里的
 * **绝对起点**,替换区间就是 `[start, cursor)`,只有一个真相来源。
 */

/** 触发种类。`slash` 是插入菜单,其余四种是补全。 */
export type TriggerKind = "slash" | "wiki" | "tag" | "mention" | "emoji";

export type TriggerMatch = {
  kind: TriggerKind;
  /** 触发符第一个字符在**文档**里的偏移。提交时的替换起点。 */
  start: number;
  /** 触发符之后到光标之间的查询串(不含触发符)。 */
  query: string;
};

/** 和 Rust `notebook/tags.rs` 的 `MAX_TAG_CHARS` 同一个值。 */
const MAX_TAG_CHARS = 64;

/**
 * 光标是否落在围栏代码块里。
 *
 * 从文档开头逐行扫到光标所在行 —— 围栏是块级状态,只看当前行永远判不出来。
 * 闭合标记不能短于开启标记,否则 ```` 包住的 ``` 会被当成提前闭合(同
 * `noteOutline.ts` 的规则)。
 */
function inFencedBlock(text: string, cursor: number): boolean {
  let fence: string | null = null;
  let at = 0;
  while (at < cursor) {
    const lineEnd = text.indexOf("\n", at);
    // 光标所在的这一行本身不算:围栏的开启行上打字不该被当成在块内。
    if (lineEnd < 0 || lineEnd >= cursor) return fence !== null;
    const line = text.slice(at, lineEnd);
    const mark = /^\s*(`{3,}|~{3,})/.exec(line)?.[1];
    if (fence !== null) {
      if (mark && mark[0] === fence[0] && mark.length >= fence.length) fence = null;
    } else if (mark) {
      fence = mark;
    }
    at = lineEnd + 1;
  }
  return fence !== null;
}

/**
 * 光标是否落在行内代码(`` ` ``)里。
 *
 * 数当前行光标之前有几个未转义的反引号,奇数就在里面。不做跨行配对:markdown 的
 * 行内代码不跨行,而跨行的那种是围栏,已经由 `inFencedBlock` 管了。
 */
function inInlineCode(before: string): boolean {
  let ticks = 0;
  for (let i = 0; i < before.length; i += 1) {
    if (before[i] === "\\") {
      i += 1;
      continue;
    }
    if (before[i] === "`") ticks += 1;
  }
  return ticks % 2 === 1;
}

/** 光标是否落在 frontmatter 里。那几行是结构化数据,补全在里面没有意义。 */
function inFrontmatter(text: string, cursor: number): boolean {
  if (!/^---\r?\n/.test(text)) return false;
  const close = /\r?\n---(\r?\n|$)/.exec(text.slice(3));
  // 没闭合的话整篇都还在 frontmatter 里(用户正在写第一行)。
  if (!close) return true;
  return cursor <= 3 + close.index + close[0].length;
}

/**
 * `#` / `@` / `:` 的前一个字符是否允许触发。
 *
 * 跟 Rust `tags.rs` 的 `ok_prefix` 一致:只有行首或空白。放宽到括号引号(Markio 那样)
 * 会让 `(@ts-ignore)`、`{#id}` 这类写法弹菜单,而更要紧的是**写进去的标签后端扫不
 * 出来** —— 前端说这是标签、`tag_hits` 不认,标签云里就永远看不到它。
 */
function canTriggerAfter(ch: string | undefined): boolean {
  return ch === undefined || /\s/.test(ch);
}

/** 纯数字不是标签(`#42` 是条目编号)。和 Rust `normalize_tag` 同一条规则。 */
function isNumericOnly(query: string): boolean {
  return query.length > 0 && /^\d+$/.test(query);
}

/**
 * 检测光标前的触发序列。没有就返回 null。
 *
 * `text` 是**整篇**正文(不是当前行):围栏和 frontmatter 都是块级状态。`cursor`
 * 是文档偏移,调用方保证它落在正文内且没有选区 —— 有选区时不该弹菜单,那是在选中
 * 一段而不是在打字。
 */
export function detectTrigger(text: string, cursor: number): TriggerMatch | null {
  if (cursor < 0 || cursor > text.length) return null;
  if (inFrontmatter(text, cursor)) return null;
  if (inFencedBlock(text, cursor)) return null;

  const lineStart = text.lastIndexOf("\n", cursor - 1) + 1;
  const before = text.slice(lineStart, cursor);
  if (inInlineCode(before)) return null;

  /* `/` 只在行首(或列表 / 引用标记之后)算插入菜单。行中间的 `/` 绝大多数是路径和
     日期,弹菜单纯属打扰。允许列表标记在前,是因为"在列表项里插个代码块"很常见。 */
  const slash = /^(\s*(?:[-*+]\s+|\d+[.)]\s+|>\s*)*)\/([^\s/]*)$/.exec(before);
  if (slash) {
    return { kind: "slash", start: lineStart + slash[1]!.length, query: slash[2]! };
  }

  /* `[[` 优先于 `#`:`[[note#heading]]` 是合法的段内链接写法,两个模式都能命中
     `[[#head`,先判 wiki 才不会把它当标签。
     查询里除了 `[` `]` 什么都收 —— 笔记标题可以带空格、括号、中文标点、emoji,
     按"允许哪些字符"列白名单一定会漏,而漏的表现是标题里带个逗号就补不出来。 */
  const wiki = /\[\[([^[\]\r\n]{0,120})$/.exec(before);
  if (wiki) {
    return { kind: "wiki", start: cursor - wiki[1]!.length - 2, query: wiki[1]! };
  }

  const tag = /#([\p{L}\p{N}_/-]{1,64})$/u.exec(before);
  if (tag && canTriggerAfter(before[before.length - tag[0].length - 1])) {
    const query = tag[1]!;
    // 长度到顶就不再是"正在打一个标签",而是已经打满了。
    if ([...query].length < MAX_TAG_CHARS && !isNumericOnly(query)) {
      return { kind: "tag", start: cursor - tag[0].length, query };
    }
  }

  const mention = /@([\p{L}\p{N}_-]{0,64})$/u.exec(before);
  if (mention && canTriggerAfter(before[before.length - mention[0].length - 1])) {
    return { kind: "mention", start: cursor - mention[0].length, query: mention[1]! };
  }

  /* emoji 的 `:` 同样要求前面是空白 —— 不要求的话 `http://`、`12:30`、YAML 的
     `key: value` 全都会弹。 */
  const emoji = /:([\p{L}\p{N}_+-]{0,32})$/u.exec(before);
  if (emoji && canTriggerAfter(before[before.length - emoji[0].length - 1])) {
    return { kind: "emoji", start: cursor - emoji[0].length, query: emoji[1]! };
  }

  return null;
}
