/* 随手记的全库搜索(⌘⇧F)。
 *
 * 后端直接用 Aeroric 已有的 `search_text` / `search_structured`(ripgrep,失败时回落到
 * 自带的逐行扫描),不搬 Markio 的那一套 grep。这个模块只负责把后端结果整理成列表能用
 * 的形状,以及三件后端结果里不明说、但用错就会出错的事:
 *
 * 1. **`column` 是字节偏移,不是 JS 字符串下标。** 两条路径都来自 Rust:ripgrep 的
 *    `submatch.start` 和回落路径的 `str::find`,给的都是**行内字节**位置。直接拿它去
 *    `lineText.slice()` 高亮,只要命中前面有任何非 ASCII 字符就会切错 —— 中文笔记里
 *    这是常态,不是边角情况:`标题 abc` 里查 `abc`,字节列是 10,而 JS 下标是 3,
 *    高亮框会跑到行尾之外。所以要按 UTF-8 字节换算回来。
 *
 * 2. **命中所在文件要按路径找回笔记,而路径两头不一定长得一样。** 后端
 *    `validate_project_root` 会 canonicalize 根目录,macOS 上 `/tmp` 会变成
 *    `/private/tmp`;而面板里的笔记 id 是 `listNotes` 给的原始路径。字符串直接相等
 *    就会一条都对不上(而且是静默的:搜到了、点了没反应)。所以先试全等,再退回按
 *    「相对 vault 的尾段」比。
 *
 * 3. **行号是文件行号**,frontmatter 那几行算在内。跳转要走面板里反链那条路
 *    (`jumpToBacklink`),它已经做了「文件行 → 正文偏移」的换算。
 */

export type NoteSearchHit = {
  /** 绝对路径,由后端 canonicalize 过的根拼出来。 */
  path: string;
  /** 文件名(带扩展名)。 */
  name: string;
  /** 1 起的文件行号,frontmatter 也算。 */
  line: number;
  /** 1 起的**字节**列。用 `byteColumnToIndex` 换成 JS 下标再用。 */
  column: number;
  lineText: string;
  matchText: string;
};

export type NoteSearchFlags = {
  caseSensitive: boolean;
  wholeWord: boolean;
  regex: boolean;
};

/** 传给后端的搜索选项。字段名是 camelCase —— Tauri 会转成 Rust 侧的 snake_case。 */
export type NoteSearchRequestOptions = {
  caseSensitive: boolean;
  wholeWord: boolean;
  regex: boolean;
  includeGlob: string;
  limit: number;
};

/** 结果上限。够大到不像截断,又不至于一次渲染上万行。 */
export const NOTE_SEARCH_LIMIT = 500;

export function noteSearchOptions(
  flags: NoteSearchFlags,
  limit = NOTE_SEARCH_LIMIT,
): NoteSearchRequestOptions {
  return {
    caseSensitive: flags.caseSensitive,
    wholeWord: flags.wholeWord,
    regex: flags.regex,
    /* 只搜 .md。vault 里还有 `.notebook/`(order.json、icons.json、图标表)和用户拖进来的
       附件 —— 搜到 JSON 配置里的匹配,点过去是打不开的,那不是笔记。 */
    includeGlob: "*.md",
    limit,
  };
}

/**
 * 1 起的字节列 → JS 字符串下标。
 *
 * 越界一律夹到行长:后端和前端拿到的行文本理论上一致,但换行裁剪、编码回落这类差异
 * 一旦出现,夹住比抛异常好 —— 搜索结果列表不该因为一行对不齐就整块崩掉。
 */
export function byteColumnToIndex(lineText: string, column: number): number {
  const target = Math.max(0, column - 1);
  if (target === 0) return 0;
  let bytes = 0;
  for (let index = 0; index < lineText.length; ) {
    if (bytes >= target) return index;
    const code = lineText.codePointAt(index)!;
    // 手算 UTF-8 长度,不用 TextEncoder:这个函数每条命中都要跑,而结果最多几百条。
    bytes += code < 0x80 ? 1 : code < 0x800 ? 2 : code < 0x10000 ? 3 : 4;
    index += code > 0xffff ? 2 : 1;
  }
  return lineText.length;
}

export type HitSegments = {
  before: string;
  match: string;
  after: string;
};

/**
 * 把命中行切成「前 / 命中 / 后」三段,供列表高亮。
 *
 * 按字节列换算出下标之后**再核对**那一段是不是 `matchText`:对不上就退回在整行里找一次,
 * 还找不到就整行当普通文本给出去。宁可不高亮,也不要高亮错的位置 —— 后者会让人以为
 * 命中在别处,点进去发现不是,却不知道是列表画错了。
 */
export function hitSegments(hit: NoteSearchHit): HitSegments {
  const { lineText, matchText } = hit;
  const start = byteColumnToIndex(lineText, hit.column);
  if (matchText && lineText.slice(start, start + matchText.length) === matchText) {
    return {
      before: lineText.slice(0, start),
      match: matchText,
      after: lineText.slice(start + matchText.length),
    };
  }
  const fallback = matchText ? lineText.indexOf(matchText) : -1;
  if (fallback >= 0) {
    return {
      before: lineText.slice(0, fallback),
      match: matchText,
      after: lineText.slice(fallback + matchText.length),
    };
  }
  return { before: lineText, match: "", after: "" };
}

export type NoteSearchGroup = {
  path: string;
  name: string;
  hits: NoteSearchHit[];
};

/**
 * 按文件分组,保持后端给出的顺序。
 *
 * 不按路径排序:ripgrep 的输出顺序在同一次搜索里是稳定的,而重排会让「刚改过的那篇
 * 排在前面」这类直觉失效。同一文件的多处命中聚在一起,是因为分开列会让用户反复在
 * 同一个文件名之间跳。
 */
export function groupSearchHits(hits: readonly NoteSearchHit[]): NoteSearchGroup[] {
  const groups: NoteSearchGroup[] = [];
  const index = new Map<string, NoteSearchGroup>();
  for (const hit of hits) {
    const existing = index.get(hit.path);
    if (existing) {
      existing.hits.push(hit);
      continue;
    }
    const group: NoteSearchGroup = { path: hit.path, name: hit.name, hits: [hit] };
    index.set(hit.path, group);
    groups.push(group);
  }
  return groups;
}

/** 路径规整:去掉尾部斜杠,Windows 的反斜杠统一成正斜杠。 */
function normalizePath(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "");
}

/**
 * 把命中的绝对路径对回面板里的笔记 id。
 *
 * 先全等,再按「相对 vault 的尾段」比 —— 后端 canonicalize 过根目录,两边的前缀可能
 * 不一样(macOS 的 `/tmp` → `/private/tmp`、Windows 的盘符大小写、软链的 vault)。
 * 对不上返回 `null`,由调用方决定怎么说,而不是悄悄跳到一条不相干的笔记上。
 */
export function resolveHitNoteId(
  hitPath: string,
  notePaths: readonly string[],
  vault: string,
): string | null {
  const wanted = normalizePath(hitPath);
  for (const path of notePaths) {
    if (normalizePath(path) === wanted) return path;
  }
  const root = normalizePath(vault);
  for (const path of notePaths) {
    const candidate = normalizePath(path);
    // 笔记相对 vault 的尾段(含子目录);不在 vault 下就退回文件名。
    const tail = candidate.startsWith(`${root}/`)
      ? candidate.slice(root.length + 1)
      : candidate.slice(candidate.lastIndexOf("/") + 1);
    /* 要求以 `/尾段` 结尾,而不是 `endsWith(尾段)` —— 后者会让 `Notes.md` 命中
       `MyNotes.md`,把用户送到另一篇笔记上。 */
    if (tail && wanted.endsWith(`/${tail}`)) return path;
  }
  return null;
}
