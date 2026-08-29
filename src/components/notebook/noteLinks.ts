/* `[[wikilink]]` 的解析与解析目标(P4 双链的地基)。
 *
 * 纯函数,不碰 DOM 也不碰 IPC —— 渲染那一半在 `enhanceWikiLinks.ts`,反链索引
 * 在 Rust 侧。这样解析规则只有一处,前端渲染、反链面板、跨文件改名三家共用。
 *
 * 与 Markio 的实质差异:**这里必须同时按文件名 stem 和 frontmatter 标题解析。**
 *
 * Markio 里文件名就是标题,一个 stem 索引够用。Aeroric 不是:标题存 frontmatter,
 * 文件名只在新建时由标题 slug 定一次,之后改标题**不改文件名**(见 notebookVault
 * 的 renameNoteToTitle 注释)。只认 stem 的话,用户把「草稿」改成「周报」之后
 * 写 `[[周报]]` 会解析不到自己那篇笔记 —— 而那正是双链最常见的用法。
 */

/** 一条 wikilink 拆开之后的三段。 */
export type WikiLinkParts = {
  /** 目标笔记(未归一化,保留用户写的原样)。 */
  target: string;
  /** 显示文本。有 `|别名` 时是别名,否则是目标含 `#小节` 的那一段。 */
  display: string;
  /** `#小节` 锚点。没有就是 undefined。 */
  heading?: string;
};

/**
 * `[[...]]` 的匹配式。
 *
 * 上限 200 字符不是排版洁癖:没有上限时,一段带很多方括号的正文(代码片段、
 * BibTeX、正则)会让回溯代价爆掉。真实链接远短于 200。
 *
 * `[^\]\n]` 排除换行,因为 wikilink 不跨行 —— 允许跨行会让一段孤立的 `[[`
 * 一路吃到几百行之后的某个 `]]`。
 */
const WIKI_LINK_RE = /\[\[([^\]\n]{1,200})\]\]/g;

/** 按首次出现的分隔符切两段。找不到时第二段是 undefined(而不是空串)。 */
function splitOnce(input: string, token: string): [string, string | undefined] {
  const index = input.indexOf(token);
  if (index < 0) return [input, undefined];
  return [input.slice(0, index), input.slice(index + token.length)];
}

/**
 * 归一化到可比形式:trim → `\`换`/` → 解百分号编码 → 去尾部 `.md` → 去首尾 `/`
 * → 小写。
 *
 * 大小写不敏感是刻意的:macOS 默认文件系统本身不区分大小写,让 `[[foo]]` 和
 * `[[Foo]]` 指向不同笔记会造出一种"看起来一样却打不开"的链接。
 */
export function normalizeLinkTarget(input: string): string {
  let next = input.trim().replace(/\\/g, "/");
  try {
    next = decodeURIComponent(next);
  } catch {
    // 目标里有孤立的 `%`(`50%完成`)时 decodeURIComponent 会抛。保留原样比
    // 丢掉整个链接好。
  }
  next = next.replace(/\.md$/i, "");
  next = next.replace(/^\/+|\/+$/g, "");
  return next.toLowerCase();
}

/**
 * 拆一条 wikilink 的内容(不含两侧的方括号)。
 *
 * 语法:`target`、`target#小节`、`target|别名`、`target#小节|别名`。
 * 目标为空(`[[]]`、`[[#只有小节]]`、`[[|只有别名]]`)时返回 null —— 那不是一条
 * 能指向任何东西的链接,当成普通文本保留比渲染出一个死链好。
 */
export function parseWikiLinkBody(body: string): WikiLinkParts | null {
  const trimmed = body.trim();
  if (!trimmed) return null;

  // 顺序:先按首个 `|` 切别名,再把前半按首个 `#` 切小节。反过来的话
  // `[[a|b#c]]` 里别名带的 `#` 会被当成小节。
  const [targetWithHeading, alias] = splitOnce(trimmed, "|");
  const [targetRaw, headingRaw] = splitOnce(targetWithHeading, "#");
  const target = targetRaw.trim();
  if (!target) return null;

  const heading = headingRaw?.trim();
  // 没有别名时显示目标**含小节**的那一段(`a#b` 显示成 `a#b`),与 Obsidian 一致。
  const display = alias?.trim() || targetWithHeading.trim();
  return { target, display, ...(heading ? { heading } : {}) };
}

/** 建索引时需要的最小笔记形状。用结构类型而不是 `VaultNote`,便于测试与复用。 */
export type LinkableNote = {
  /** 绝对路径,同时是身份。 */
  path: string;
  /** 显示标题(来自 frontmatter,可能与文件名无关)。 */
  title: string;
};

/**
 * 建索引时该用哪个标题:内存里那份,还是扫盘索引里那份。
 *
 * 两边都可能是过期的,方向相反:
 * - 内存里未读入的笔记只有文件名 stem(列表只读目录项),真标题在 frontmatter 里,
 *   得靠索引;
 * - 刚在列表里改过标题的笔记,内存是新的、索引是上一次扫盘的旧值,得用内存。
 *
 * 判据是"内存里那份是不是只是文件名顶着的占位":是就采信索引,否则内存优先。
 * 只看 `loaded` 不够 —— 改名走的是列表那条路,并不会把正文读进来。
 */
export function linkTitleOf(
  note: { path: string; title: string },
  indexedTitles: ReadonlyMap<string, string>,
): string {
  const indexed = indexedTitles.get(note.path);
  if (!indexed) return note.title;
  const placeholder = normalizeLinkTarget(note.title) === normalizeLinkTarget(stemOf(note.path));
  return placeholder ? indexed : note.title;
}

/** 一条命中,连同它是靠哪一路匹配上的。 */
export type LinkMatch = {
  note: LinkableNote;
  /** 命中来源。歧义提示要用它区分"改过标题"和"真的重名"。 */
  via: "stem" | "title" | "path";
  /** 同一归一化键下是否还有别的笔记。UI 据此提示歧义。 */
  ambiguous: boolean;
};

/**
 * vault 的链接索引。
 *
 * 四张表各自负责一种写法。用 `LinkableNote[]` 而不是单个:同一个键下可能有多篇
 * (改过标题撞上别人的文件名、或者真的两篇同名),把它们都留着才能报歧义 ——
 * 静默取第一篇会让用户以为链接指向的是另一篇。
 */
export type VaultLinkIndex = {
  byStem: Map<string, LinkableNote[]>;
  byTitle: Map<string, LinkableNote[]>;
  byPath: Map<string, LinkableNote>;
  /** 路径式链接的尾段匹配用。归一化路径 + 笔记。 */
  paths: { norm: string; note: LinkableNote }[];
};

/** 从路径取不带扩展名的文件名。同时吃 `/` 和 `\`(Windows 路径)。 */
export function stemOf(path: string): string {
  const name = path.split(/[/\\]/).pop() ?? path;
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(0, dot) : name;
}

function push(map: Map<string, LinkableNote[]>, key: string, note: LinkableNote): void {
  if (!key) return;
  const list = map.get(key);
  if (list) {
    // 同一篇笔记的 stem 和 title 归一化后可能相同(标题没改过),那时候不要
    // 把它记两遍 —— 否则它自己会把自己判成歧义。
    if (!list.some((item) => item.path === note.path)) list.push(note);
    return;
  }
  map.set(key, [note]);
}

/**
 * 建索引。
 *
 * 顺序即优先级:入参顺序决定同键下谁排第一。调用方传的是面板里的笔记列表,
 * 它按用户排序 / 修改时间排 —— 于是歧义时优先命中的是"更近的那篇",这比按
 * 路径字典序更符合直觉。
 */
export function buildLinkIndex(notes: readonly LinkableNote[]): VaultLinkIndex {
  const index: VaultLinkIndex = {
    byStem: new Map(),
    byTitle: new Map(),
    byPath: new Map(),
    paths: [],
  };
  for (const note of notes) {
    push(index.byStem, normalizeLinkTarget(stemOf(note.path)), note);
    push(index.byTitle, normalizeLinkTarget(note.title), note);
    const norm = normalizeLinkTarget(note.path);
    if (norm) {
      // 路径是唯一的,第一个赢就行 —— 真出现两条同归一化路径只能是大小写差异,
      // 那在不区分大小写的文件系统上本来就是同一个文件。
      if (!index.byPath.has(norm)) index.byPath.set(norm, note);
      index.paths.push({ norm, note });
    }
  }
  return index;
}

/**
 * 解析一个链接目标。
 *
 * 优先级:stem → title → 完整路径 → 路径尾段。
 *
 * stem 排在 title 前面,因为文件名是稳定的身份,而标题随时可改 —— 用户写
 * `[[foo]]` 时若正好有个 `foo.md`,他要的几乎一定是那个文件,而不是某篇后来把
 * 标题改成 `foo` 的笔记。
 *
 * 带 `/` 的目标跳过 stem/title:`[[notes/foo]]` 明显是在指路径,拿它去撞一个
 * 标题里带斜杠的笔记只会误命中。
 */
export function resolveLink(index: VaultLinkIndex, target: string): LinkMatch | null {
  const needle = normalizeLinkTarget(target);
  if (!needle) return null;

  if (!needle.includes("/")) {
    const byStem = index.byStem.get(needle);
    if (byStem?.length) {
      return { note: byStem[0]!, via: "stem", ambiguous: byStem.length > 1 };
    }
    const byTitle = index.byTitle.get(needle);
    if (byTitle?.length) {
      return { note: byTitle[0]!, via: "title", ambiguous: byTitle.length > 1 };
    }
    return null;
  }

  const exact = index.byPath.get(needle);
  if (exact) return { note: exact, via: "path", ambiguous: false };

  // 尾段匹配:`[[sub/foo]]` 命中 `/vault/notes/sub/foo.md`。前缀那个 `/` 不能省,
  // 否则 `[[b/foo]]` 会命中 `/vault/ab/foo.md`。
  const tail = `/${needle}`;
  const hits = index.paths.filter((entry) => entry.norm.endsWith(tail));
  if (hits.length) {
    return { note: hits[0]!.note, via: "path", ambiguous: hits.length > 1 };
  }
  return null;
}

/** 正文里一条 wikilink 出现的位置与内容。 */
export type WikiLinkOccurrence = WikiLinkParts & {
  /** `[[` 在源码里的下标(嵌入语法算上前面那个 `!`)。 */
  from: number;
  /** `]]` 之后的下标。 */
  to: number;
  /** 方括号里的原始内容,改写时按它定位。 */
  raw: string;
  /** 是不是 `![[...]]` 嵌入。 */
  embed: boolean;
};

/**
 * 扫出正文里所有 wikilink。
 *
 * **不排除代码块。** 调用方按用途自己决定:渲染那一路走 DOM,天然不会进
 * `<pre>`;反链统计要的是"这篇笔记提到过谁",代码块里的 `[[foo]]` 也算提到。
 * 在这里一刀切会让反链漏掉一部分,而那种漏是静默的。
 */
export function scanWikiLinks(source: string): WikiLinkOccurrence[] {
  const out: WikiLinkOccurrence[] = [];
  WIKI_LINK_RE.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = WIKI_LINK_RE.exec(source))) {
    const raw = match[1] ?? "";
    const parts = parseWikiLinkBody(raw);
    if (!parts) continue;
    const embed = match.index > 0 && source[match.index - 1] === "!";
    out.push({
      ...parts,
      raw,
      embed,
      from: embed ? match.index - 1 : match.index,
      to: match.index + match[0].length,
    });
  }
  return out;
}

/**
 * 这篇笔记指向了哪些笔记(去重后的路径列表)。
 *
 * 解析不到的目标直接丢掉 —— 反链是"谁指向我",一个指不到任何笔记的目标没有
 * "我"可言。死链的提示是渲染那一层的事。
 */
export function outgoingLinks(index: VaultLinkIndex, source: string): string[] {
  const seen = new Set<string>();
  for (const link of scanWikiLinks(source)) {
    const hit = resolveLink(index, link.target);
    if (hit) seen.add(hit.note.path);
  }
  return [...seen];
}
