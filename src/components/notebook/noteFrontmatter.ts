/* 笔记 frontmatter 的读写。
 *
 * 随手记落盘后,标题存在 frontmatter 的 `title` 里而不是文件名里。原因:
 * 面板上的标题框是随时可编辑的,如果标题即文件名,每敲一个字就要 rename 一次
 * 文件 —— 既慢又容易在中途留下半路命名的文件。文件名在新建时由标题 slug 一次
 * 定好,之后只在用户显式「重命名」时才改。
 *
 * 只做最小解析:`---` 包起来的单层 `key: value`。不支持嵌套对象和数组 ——
 * 随手记不需要,而引入一个 YAML 解析器会把 bundle 撑大。无法识别的行原样
 * 保留(见 `extra`),这样第三方工具(Obsidian 等)写进去的字段不会在我们
 * 保存时被抹掉。
 */

export type NoteFrontmatter = {
  title: string | null;
  /** 除 title 外的原始行,原样保留以免覆盖第三方字段。 */
  extra: string[];
};

export type SplitNote = {
  frontmatter: NoteFrontmatter;
  /** 正文(不含 frontmatter 块)。 */
  body: string;
};

const EMPTY: NoteFrontmatter = { title: null, extra: [] };

/** 解析 YAML 双引号标量。与 `formatScalar` 互为逆操作。 */
function parseScalar(raw: string): string {
  const trimmed = raw.trim();
  if (trimmed.startsWith('"') && trimmed.endsWith('"') && trimmed.length >= 2) {
    return trimmed.slice(1, -1).replace(/\\"/g, '"').replace(/\\\\/g, "\\");
  }
  // 单引号 YAML 里 '' 表示一个单引号。
  if (trimmed.startsWith("'") && trimmed.endsWith("'") && trimmed.length >= 2) {
    return trimmed.slice(1, -1).replace(/''/g, "'");
  }
  return trimmed;
}

/** 写成 YAML 双引号标量。标题里可能有引号、冒号、`#`,裸写会产出无法解析的 YAML。 */
function formatScalar(value: string): string {
  const escaped = value.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
  // frontmatter 是单行 key: value,换行会截断这一项。
  const singleLine = escaped.replace(/\r?\n/g, " ");
  return `"${singleLine}"`;
}

/**
 * 把文件内容拆成 frontmatter + 正文。
 *
 * 没有 frontmatter 时返回空 frontmatter 和原样正文 —— 用户手写的、或第三方
 * 工具生成的裸 markdown 也要能正常打开。
 */
export function splitNote(source: string): SplitNote {
  // 兼容 CRLF:Windows 上第三方编辑器写出来的文件是 \r\n。
  const normalized = source.replace(/\r\n/g, "\n");
  if (!normalized.startsWith("---\n")) {
    return { frontmatter: { ...EMPTY }, body: source };
  }

  const rest = normalized.slice(4);
  // 结束标记必须独占一行。`\n*` 顺带吃掉块与正文之间的空行 —— 那是排版分隔,
  // 不是正文内容。`joinNote` 写回时也会加上它,两边对称,反复读写不会累积空行。
  const endMatch = /\n---[ \t]*(?:\n\n*|$)/.exec(rest);
  if (!endMatch || endMatch.index === undefined) {
    // 开了 `---` 但没闭合 —— 不是 frontmatter,当作正文的一部分。
    return { frontmatter: { ...EMPTY }, body: source };
  }

  const block = rest.slice(0, endMatch.index);
  const body = rest.slice(endMatch.index + endMatch[0].length);

  let title: string | null = null;
  const extra: string[] = [];
  for (const line of block.split("\n")) {
    const match = /^([A-Za-z][\w-]*)\s*:\s*(.*)$/.exec(line);
    if (match && match[1] === "title") {
      title = parseScalar(match[2] ?? "");
      continue;
    }
    // 认不出来的行也留着 —— 可能是别的工具写的。
    if (line.trim().length > 0) extra.push(line);
  }

  return { frontmatter: { title, extra }, body };
}

/**
 * 从 frontmatter 里读一个字段的值,没有就返回 null。
 *
 * key 折大小写(`View:` 和 `view:` 是同一个字段),和字段浏览器 / 标签同一个口径;
 * **值不折** —— 值是内容而不是标识符,见 `noteFields.ts` 的模块注释。
 *
 * 解析用的是 `splitNote` 里那条同样的 `^key\s*:\s*(.*)$`,不另写一条:两条正则对
 * "带引号的值"、"key 里能有什么字符"判定不一致的话,同一个字段在一处认得出、在另一
 * 处认不出。
 */
export function frontmatterValue(frontmatter: NoteFrontmatter, key: string): string | null {
  const want = key.toLowerCase();
  if (want === "title") return frontmatter.title;
  for (const line of frontmatter.extra) {
    const match = /^([A-Za-z][\w-]*)\s*:\s*(.*)$/.exec(line);
    if (!match || match[1]!.toLowerCase() !== want) continue;
    return parseScalar(match[2] ?? "");
  }
  return null;
}

/**
 * 把 frontmatter + 正文重新拼成文件内容。
 *
 * `title` 为空时不写 `title:` 字段 —— 一个空标题不值得往文件里塞一行噪声。
 */
export function joinNote(frontmatter: NoteFrontmatter, body: string): string {
  const lines: string[] = [];
  if (frontmatter.title && frontmatter.title.trim().length > 0) {
    lines.push(`title: ${formatScalar(frontmatter.title)}`);
  }
  lines.push(...frontmatter.extra);

  if (lines.length === 0) {
    // 没有任何字段就别写空的 `---\n---`。
    return body;
  }

  const block = `---\n${lines.join("\n")}\n---\n\n`;
  return `${block}${body.replace(/^\n+/, "")}`;
}

/**
 * 从文件内容和路径推断显示标题。
 *
 * 优先级:frontmatter title → 第一个 `# 标题` → 文件名(去扩展名)。
 * 最后那档保证了「用户从 Obsidian 拖进来的裸 md」也有个合理的名字。
 */
export function deriveTitle(source: string, filePath: string): string {
  const { frontmatter, body } = splitNote(source);
  if (frontmatter.title && frontmatter.title.trim().length > 0) {
    return frontmatter.title.trim();
  }
  const heading = /^#{1,6}[ \t]+(.+)$/m.exec(body);
  if (heading?.[1]) return heading[1].trim();
  return fileStem(filePath);
}

/** 从路径里取出不带扩展名的文件名。同时吃 `/` 和 `\`。 */
export function fileStem(filePath: string): string {
  const name = filePath.split(/[/\\]/).pop() ?? filePath;
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(0, dot) : name;
}
