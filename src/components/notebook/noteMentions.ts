/* 未链接的提及:把「全库提及扫描」的结果折成按来源笔记分组的列表,并决定
 * 「一键全部链接」到底该动哪几处。
 *
 * 纯函数,不碰 IPC 也不碰 DOM。分层和反链完全一致:扫描在 Rust
 * (`notebook/mentions.rs`,读全文所以必须在后端),**候选名字**的口径在
 * `noteLinks.ts`(那份有测试),这里只做第三件事 —— 折叠、排序,以及"批量该动谁"。
 *
 * ## 候选名字为什么由前端给
 *
 * 一篇笔记的可链接名字有哪些,答案在 `noteLinks.ts` 的解析规则里:stem 和
 * frontmatter 标题都能解析到同一篇(`resolveLink` 的 byStem → byTitle 顺序)。在
 * Rust 里再判一次会得到两套会各自漂移的"名字",而漂移的表现是"提及列表里有它、
 * 点了却包出一条死链"—— 用户看不出那是两处规则不一致。
 *
 * ## 批量只动 confident
 *
 * 中文没有词边界,「计划」出现在「原计划表」里时判不出该不该包。Rust 侧把这类邻字
 * 判成 `ambiguous`,这里让「全部链接」跳过它们 —— 猜错的代价是用户正文里多出一条谁
 * 都没写过的链接,而他不会立刻发现。ambiguous 那些仍然列出来,逐条点得动。
 */

import { linkTitleOf, normalizeLinkTarget, stemOf } from "./noteLinks";

/** Rust 侧 `MentionConfidence`。 */
export type MentionConfidence = "confident" | "ambiguous";

/** Rust 侧 `MentionHit`。 */
export type MentionHit = {
  /** 命中的候选名字(传下去的那一个)。显示用。 */
  needle: string;
  /**
   * 命中处的**原文**。「链接」那一步的校验依据。
   *
   * 和 `needle` 分开是必须的:匹配大小写不敏感,正文里的 `PLAN` 会命中候选名 `Plan`,
   * 而后端校验的是"这个区间里的原文还是不是当时那段"。拿 `needle` 去校验的话,每一个
   * 大小写不同的命中都会被报成 `vanished`。
   */
  text: string;
  /** 1-based 行号,按**整个 `.md` 文件**数 —— 和反链 / 标签 / 任务同一个坐标系。 */
  line: number;
  /** 这处字样在**整篇内容里**的字节区间。链接那一步按它原地包。 */
  start: number;
  end: number;
  preview: string;
  confidence: MentionConfidence;
};

/** Rust 侧 `MentionSource`:一篇笔记里的全部未链接提及。 */
export type MentionSource = {
  /** 绝对路径,与笔记列表里的 `id` 同一个值。 */
  path: string;
  mentions: MentionHit[];
};

/** Rust 侧 `MentionTarget`:要包的那一处。 */
export type MentionTarget = {
  path: string;
  start: number;
  end: number;
  /** 扫描时那一处的原文。后端重读后对不上就报 `vanished`。 */
  text: string;
};

/** Rust 侧 `MentionLinkReport`。 */
export type MentionLinkReport = {
  changed: { path: string; count: number }[];
  skipped: { path: string; start: number; reason: "vanished" | "alreadyLinked" | "tooManyFiles" }[];
  failed: { path: string; message: string }[];
  /** 一共链上了几**处**(不是几个文件)。由后端算,见 `mentions.rs`。 */
  linked: number;
};

/** 一篇来源笔记贡献的全部未链接提及。 */
export type MentionGroup = {
  path: string;
  /** 显示标题。取自链接索引 —— 它已经合并过内存标题和扫盘标题。 */
  title: string;
  hits: MentionHit[];
};

/**
 * 当前笔记的**可链接名字**:frontmatter 标题 + 文件名 stem。
 *
 * 两个都要,因为 `resolveLink` 两个都认(byStem → byTitle)。只给标题的话,正文里
 * 按文件名写的那些字样一处都扫不出来;只给 stem 就是 Markio 的行为 —— 用户把「草稿」
 * 改成「周报」之后,提及里再也看不到「周报」。
 *
 * 归一化后相同的只留一个(标题没改过的笔记两者相同),否则同一处会报两条。
 * 空标题不参与:未命名笔记的"名字"是文件名。
 */
export function mentionNamesOf(
  note: { path: string; title: string },
  indexedTitles: ReadonlyMap<string, string>,
): string[] {
  const names: string[] = [];
  const push = (value: string) => {
    const trimmed = value.trim();
    if (!trimmed) return;
    const key = normalizeLinkTarget(trimmed);
    if (!key) return;
    if (names.some((kept) => normalizeLinkTarget(kept) === key)) return;
    names.push(trimmed);
  };
  /* 标题走 `linkTitleOf`:内存里未读入的笔记标题是文件名占位,真标题在扫盘索引里。
     和链接索引读同一个口径(`buildLinkIndex` 也是这么取标题的)—— 两边不一致就会
     出现"提及扫的是占位名,而链接解析认的是真标题"。 */
  push(linkTitleOf(note, indexedTitles));
  push(stemOf(note.path));
  return names;
}

/**
 * 折出按来源分组的未链接提及。
 *
 * Rust 已按路径排序、组内按行号与行内位置排序,这里只补标题,并且丢掉空组。
 * `titleOf` 拿不到就回落到路径 —— 和反链同一个处理。
 */
export function collectMentions(
  sources: readonly MentionSource[],
  titleFor: (path: string) => string,
): MentionGroup[] {
  const groups: MentionGroup[] = [];
  for (const source of sources) {
    if (!source.mentions.length) continue;
    groups.push({
      path: source.path,
      title: titleFor(source.path),
      hits: [...source.mentions].sort((a, b) => a.line - b.line || a.start - b.start),
    });
  }
  return groups;
}

/** 提及总处数(不是来源篇数)。标题上那个计数用它。 */
export function countMentions(groups: readonly MentionGroup[]): number {
  return groups.reduce((sum, group) => sum + group.hits.length, 0);
}

/** 其中有几处是 confident —— 「全部链接」按钮上那个数。 */
export function countConfident(groups: readonly MentionGroup[]): number {
  return groups.reduce(
    (sum, group) => sum + group.hits.filter((hit) => hit.confidence === "confident").length,
    0,
  );
}

/**
 * 「全部链接」要提交的 target 列表:**只有 confident 的那些**。
 *
 * 见模块注释 —— ambiguous 的不进批量。返回空数组时调用方不该发请求。
 */
export function confidentTargets(groups: readonly MentionGroup[]): MentionTarget[] {
  const targets: MentionTarget[] = [];
  for (const group of groups) {
    for (const hit of group.hits) {
      if (hit.confidence !== "confident") continue;
      targets.push(targetOf(group.path, hit));
    }
  }
  return targets;
}

/**
 * 单独一处的 target。
 *
 * `text` 取 `hit.text`(命中处的原文),**不是** `hit.needle`(候选名)—— 后端校验的是
 * "这个区间里的原文还是不是当时那段",而两者在大小写不同时会不一样。Rust 侧有一条
 * 用例专门钉住这个区别(`hit_carries_both_the_candidate_name_and_the_prose_text`)。
 */
export function targetOf(path: string, hit: MentionHit): MentionTarget {
  return { path, start: hit.start, end: hit.end, text: hit.text };
}
