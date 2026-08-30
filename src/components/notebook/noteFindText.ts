/* 随手记查找的匹配器(⌘F / ⌘H 的内核)。
 *
 * 从 Markio `src/lib/findText.ts` 移植,替换 NotebookPanel 里原来那个只会
 * 「小写化 + indexOf」的实现。除了补上大小写 / 整词 / 正则三个开关,这一版还修掉
 * 两处它们各自的问题:
 *
 * 1. **小写化会改字节长度,于是偏移错位。** 原实现在 `text.toLocaleLowerCase()`
 *    上取 `indexOf`,却把拿到的下标当成**原文**偏移用去替换。可是大小写折叠不保长:
 *    `"İ".toLocaleLowerCase()` 是两个码元。所以正文 `"İstanbul 的 cat"` 里查
 *    `cat` 会命中 12,而 `cat` 其实在 11 —— 替换结果是 `"İstanbul 的 cDOG"`,
 *    吃掉了一个 `c`,而用户只看到替换「串位了」。这里改用带 `i` 标志的正则:
 *    不区分大小写由正则引擎做,偏移始终是原文坐标。
 *
 * 2. **整词在中日韩文本上会静默返回 0 命中。** Markio 的 `isWordChar` 是
 *    `/^[\p{L}\p{N}_]$/u`,而汉字属于 `\p{L}` —— 于是在 `本周计划表` 里开整词
 *    查 `计划`,前后都是「词字符」,边界检查必然失败。中日韩没有词边界,英文语境
 *    抄来的启发式在这里不成立(见 CLAUDE 记忆 cjk-has-no-word-boundaries)。
 *    这一版的做法是:命中的某一侧若紧贴中日韩表意文字,那一侧就不做边界要求,
 *    并把 `wholeWordIgnored` 报上去,由查找栏显式提示「这一侧没按整词过滤」。
 *    退化成「0 命中」和悄悄「当作整词」都是错的 —— 前者让人以为文里没有,后者
 *    让人以为过滤生效了。
 */

export type NoteFindOptions = {
  caseSensitive: boolean;
  wholeWord: boolean;
  regex: boolean;
  /** 命中上限。超过就停下并把 `capped` 报上去,不是悄悄截断。 */
  maxMatches?: number;
};

export type NoteFindMatch = {
  start: number;
  end: number;
  /** 命中处的原文。替换时当乐观锁用 —— 见 `replaceNoteMatches`。 */
  text: string;
  /** 捕获组(1 号起,可选组没匹配上就是 undefined)。正则替换的 `$1` 要它。 */
  captures: readonly (string | undefined)[];
};

export type NoteFindResult = {
  matches: NoteFindMatch[];
  /** 正则不合法时的原始报错。查找栏据此提示,而不是把它当成「无匹配」。 */
  error: string | null;
  capped: boolean;
  /** 至少有一处命中因为紧贴中日韩文字而放弃了整词边界要求。 */
  wholeWordIgnored: boolean;
};

const DEFAULT_MAX_MATCHES = 50_000;

const EMPTY: NoteFindResult = {
  matches: [],
  error: null,
  capped: false,
  wholeWordIgnored: false,
};

function escapeRegExp(pattern: string): string {
  return pattern.replace(/[\\^$.*+?()[\]{}|]/g, "\\$&");
}

/* 取边界字符要按**码位**取,不能按码元:`"𝐀b"` 里 `b` 前面那个码元是低代理项,
   单独拿出来既不是字母也不是数字,于是整词会误判成「前面不是词字符」。 */
function codePointBefore(text: string, index: number): string {
  if (index <= 0) return "";
  const prev = text.charCodeAt(index - 1);
  if (prev >= 0xdc00 && prev <= 0xdfff && index > 1) {
    const high = text.charCodeAt(index - 2);
    if (high >= 0xd800 && high <= 0xdbff) return text.slice(index - 2, index);
  }
  return text.slice(index - 1, index);
}

function codePointAt(text: string, index: number): string {
  if (index >= text.length) return "";
  const first = text.charCodeAt(index);
  if (first >= 0xd800 && first <= 0xdbff && index + 1 < text.length) {
    const low = text.charCodeAt(index + 1);
    if (low >= 0xdc00 && low <= 0xdfff) return text.slice(index, index + 2);
  }
  return text.slice(index, index + 1);
}

const WORD_CHAR_RE = /^[\p{L}\p{N}_]$/u;

/* 「没有词边界」的文字:汉字(含扩展区与兼容区)、平假名、片假名。
   谚文**不**算 —— 韩文分词写空格,整词在它上面是成立的。 */
const CJK_RE = /^[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}]$/u;

function isWordChar(ch: string): boolean {
  return ch !== "" && WORD_CHAR_RE.test(ch);
}

function isCjk(ch: string): boolean {
  return ch !== "" && CJK_RE.test(ch);
}

/**
 * 整词判定。返回「是否收下这处命中」以及「这处有没有放弃某一侧的边界要求」。
 *
 * 两侧分别判:`TODO计划` 这种混排查询,左边是 ASCII 该按词边界卡,右边贴着汉字
 * 就卡不住 —— 逐侧决定比整条命中一刀切准。
 */
function checkWholeWord(text: string, from: number, to: number): { ok: boolean; relaxed: boolean } {
  const before = codePointBefore(text, from);
  const after = codePointAt(text, to);
  let relaxed = false;
  for (const side of [before, after]) {
    if (isCjk(side)) {
      relaxed = true;
      continue;
    }
    if (isWordChar(side)) return { ok: false, relaxed: false };
  }
  return { ok: true, relaxed };
}

export function findNoteTextMatches(
  text: string,
  query: string,
  options: NoteFindOptions,
): NoteFindResult {
  if (!query) return EMPTY;

  let re: RegExp;
  try {
    re = new RegExp(
      options.regex ? query : escapeRegExp(query),
      options.caseSensitive ? "gu" : "giu",
    );
  } catch (error) {
    // 边打边查时半截正则(`(`、`[a-`)必然抛,这是常态而非异常,所以把报错**报出来**
    // 让查找栏显示,而不是让它冒到 render 里去。
    return { ...EMPTY, error: error instanceof Error ? error.message : String(error) };
  }

  const maxMatches = options.maxMatches ?? DEFAULT_MAX_MATCHES;
  const matches: NoteFindMatch[] = [];
  let capped = false;
  let wholeWordIgnored = false;
  let match: RegExpExecArray | null;
  while ((match = re.exec(text))) {
    const start = match.index;
    const end = start + match[0].length;
    if (end === start) {
      // 零宽命中(`a*`、`^`)不推进 lastIndex,不手动挪一格就是死循环。
      re.lastIndex = start + 1;
      continue;
    }
    if (options.wholeWord) {
      const verdict = checkWholeWord(text, start, end);
      if (!verdict.ok) continue;
      if (verdict.relaxed) wholeWordIgnored = true;
    }
    matches.push({ start, end, text: match[0], captures: match.slice(1) });
    if (matches.length >= maxMatches) {
      capped = true;
      break;
    }
  }
  return { matches, error: null, capped, wholeWordIgnored };
}

/**
 * 把替换文本按当前模式算成实际要写进去的串。
 *
 * 正则模式下要支持 `$1` / `$&` 这些引用,普通模式下 `$` 必须是字面量 —— 用户查
 * `price` 想替换成 `$9.99`,不能被当成第 9 个捕获组。所以这里不走
 * `String.replace` 的模板语义,而是自己按模式决定:普通模式直接返回原串。
 */
export function expandReplacement(
  replacement: string,
  match: NoteFindMatch,
  useRegex: boolean,
): string {
  if (!useRegex) return replacement;
  return replacement.replace(/\$(\$|&|\d{1,2})/g, (whole, token: string) => {
    if (token === "$") return "$";
    if (token === "&") return match.text;
    const index = Number(token);
    // 越界的 `$3`(只有两个组)保留原样,和 `String.replace` 的行为一致 ——
    // 别把它变成空串,那会让用户以为组号写对了。
    if (index < 1 || index > match.captures.length) return whole;
    return match.captures[index - 1] ?? "";
  });
}

/**
 * 一次性替换文本里的全部命中,返回新正文。
 *
 * 从后往前写,这样前面的命中偏移不会被已写入的长度差挪动。
 *
 * `match.text` 当乐观锁:算出命中时的正文和真正落笔时的正文可能已经不是同一份
 * (自动保存、外部改动、看板 / 收集箱的并发写)。所以每处都要核对该区间的原文是否
 * 还是当时那一段,不一致就**整体**放弃并返回 `null` —— 半篇替换过、半篇没替换,
 * 比什么都不做糟得多,而用户看不出停在了哪。
 */
export function replaceNoteMatches(
  text: string,
  matches: readonly NoteFindMatch[],
  replacement: string,
  useRegex: boolean,
): string | null {
  let next = text;
  for (let i = matches.length - 1; i >= 0; i -= 1) {
    const match = matches[i]!;
    if (match.start < 0 || match.end > next.length) return null;
    if (next.slice(match.start, match.end) !== match.text) return null;
    const value = expandReplacement(replacement, match, useRegex);
    next = `${next.slice(0, match.start)}${value}${next.slice(match.end)}`;
  }
  return next;
}
