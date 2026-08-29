/* 反链:把「全库链接扫描」的结果折成「谁指向了当前这篇」。
 *
 * 纯函数,不碰 IPC 也不碰 DOM。扫描在 Rust(`notebook/links.rs`,读全文所以必须
 * 在后端),解析规则在 `noteLinks.ts`(那份有测试),这里只做第三件事:拿解析
 * 规则去筛扫描结果。
 *
 * 三层分开的好处是各自可测:Rust 那层测"词法提取和前端正则等价",`noteLinks`
 * 测"`[[foo]]` 该指向谁",这里测"折叠、去重、排序、自引用"。
 */

import {
  normalizeLinkTarget,
  parseWikiLinkBody,
  resolveLink,
  type VaultLinkIndex,
} from "./noteLinks";

/** Rust 侧 `NoteLinkRef`。 */
export type NoteLinkRef = {
  /** 方括号里的原始内容,未解析。 */
  raw: string;
  /** 1-based 行号。 */
  line: number;
  /** 那一行的文本(已 trim,超长截断)。 */
  preview: string;
  /** 是不是 `![[...]]` 嵌入。 */
  embed: boolean;
};

/** Rust 侧 `NoteLinkSource`:一篇笔记里的全部链接。 */
export type NoteLinkSource = {
  /** 绝对路径,与笔记列表里的 `id` 同一个值。 */
  path: string;
  links: NoteLinkRef[];
};

/** 一条反链命中:某篇笔记的某一行指向了目标。 */
export type BacklinkHit = {
  line: number;
  preview: string;
  embed: boolean;
};

/** 一篇来源笔记贡献的全部反链。 */
export type BacklinkGroup = {
  /** 来源笔记的绝对路径。点一下跳过去。 */
  path: string;
  /** 来源笔记的显示标题。取自链接索引 —— 它已经合并过内存标题和扫盘标题。 */
  title: string;
  hits: BacklinkHit[];
};

/** 从索引里取一篇笔记的显示标题。索引里没有(刚被删掉)时回落到路径。 */
function titleOf(index: VaultLinkIndex, path: string): string {
  const note = index.byPath.get(normalizeLinkTarget(path));
  return note?.title ?? path;
}

/**
 * 折出指向 `targetPath` 的全部反链。
 *
 * - **自引用不算。** 一篇笔记里写 `[[自己]]` 是排版手法(目录、模板),把它列进
 *   "谁引用了我"只会让每篇笔记的反链里都有它自己。
 * - **解析不到的链接不算。** 反链是"谁指向我",一个指不到任何笔记的目标没有
 *   "我"可言 —— 死链的提示是渲染那一层的事。
 * - **嵌入(`![[...]]`)算。** 它是一种更强的引用,不是别的东西。
 * - 同一行出现两次(`见 [[周报]] 和 [[周报#本周]]`)只留一条:反链列表是给人扫的,
 *   同一行重复两遍没有增量信息,而点进去也是同一个位置。
 *
 * 顺序:来源按 `sources` 给的顺序(Rust 已按路径排好),组内按行号。
 */
export function collectBacklinks(
  sources: readonly NoteLinkSource[],
  index: VaultLinkIndex,
  targetPath: string,
): BacklinkGroup[] {
  const key = normalizeLinkTarget(targetPath);
  if (!key) return [];
  const groups: BacklinkGroup[] = [];
  for (const source of sources) {
    if (normalizeLinkTarget(source.path) === key) continue;
    const hits: BacklinkHit[] = [];
    const seenLines = new Set<number>();
    for (const link of source.links) {
      const parts = parseWikiLinkBody(link.raw);
      if (!parts) continue;
      const match = resolveLink(index, parts.target);
      if (!match) continue;
      if (normalizeLinkTarget(match.note.path) !== key) continue;
      if (seenLines.has(link.line)) continue;
      seenLines.add(link.line);
      hits.push({ line: link.line, preview: link.preview, embed: link.embed });
    }
    if (!hits.length) continue;
    hits.sort((a, b) => a.line - b.line);
    groups.push({ path: source.path, title: titleOf(index, source.path), hits });
  }
  return groups;
}

/** 反链总条数(不是来源篇数)。标题上那个计数用它。 */
export function countBacklinks(groups: readonly BacklinkGroup[]): number {
  return groups.reduce((sum, group) => sum + group.hits.length, 0);
}

/**
 * 文件里的 1-based 行号 → **编辑器正文**里的偏移。
 *
 * 两个坐标系不一样:反链的行号是按整个 `.md` 文件数的(frontmatter 那几行也算),
 * 而编辑器里装的是拆掉 frontmatter 之后的正文。直接把行号喂给 `offsetOfLine(body)`
 * 会稳定地偏几行 —— 而且偏多少取决于那篇笔记的 frontmatter 有多长,看起来像是
 * "有时候准有时候不准"。
 *
 * `fileContent` 由调用方用 `noteFileContent` 拼(和保存、和版本历史 diff 同一个
 * 函数),所以这里的换行数和落盘的文件一致。
 *
 * 落在 frontmatter 里的行号(理论上不会有:那几行里的 `[[...]]` 也会被扫出来)
 * 收敛到正文开头 —— 把光标放进一个编辑器里根本看不到的位置更糟。
 */
export function bodyOffsetOfFileLine(fileContent: string, body: string, line: number): number {
  const bodyStart = fileContent.length - body.length;
  const fileOffset = offsetOfLine(fileContent, line);
  return Math.max(0, fileOffset - bodyStart);
}

/**
 * 1-based 行号 → 文档偏移(那一行的行首)。
 *
 * 反链给的是行号,而 CodeMirror 的 `revealOffset` 要偏移。行号越界(来源笔记在
 * 上一次扫描之后被删短了)时返回最后一行的行首,而不是 0 或者文末:滚到顶部
 * 会让人以为跳错了笔记,而"尽量靠近"至少落在同一篇的末尾。
 *
 * 换行按 `\n` 数,和 Rust 侧 `content.lines()` 对得上 —— 那边把 `\r` 当行尾的一
 * 部分去掉了,所以 `\r\n` 文本的行号在两边一致。
 */
export function offsetOfLine(source: string, line: number): number {
  if (line <= 1) return 0;
  let offset = 0;
  let current = 1;
  while (current < line) {
    const next = source.indexOf("\n", offset);
    if (next < 0) break;
    offset = next + 1;
    current += 1;
  }
  return offset;
}
