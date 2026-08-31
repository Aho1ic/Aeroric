/* 随手记的语义检索与 AI 上下文(P7)。
 *
 * 只做三件后端不管、而前端弄错就会出事的事:
 *
 * 1. **字符偏移要换坐标系。** 后端的 `charStart` / `charEnd` / `spans` 数的是 Unicode
 *    标量(Rust 的 `chars().count()`),而 JS 的 `String` 下标数的是 UTF-16 码元。
 *    BMP 内的字(含全部中日韩)两者一致,但 emoji、部分生僻字、音乐符号是代理对,
 *    一个标量占两个码元 —— 笔记里出现一个 emoji,它**之后**的所有跳转就都会偏,而且
 *    偏移量取决于前面有几个 emoji,表现为"有时候准有时候不准"。换算只在这里做。
 *
 * 2. **偏移是相对正文的,而跳转要的是文件行号。** 后端索引的是剥掉 frontmatter 的
 *    正文,面板的 `jumpToBacklink` 要的是按整个 `.md` 文件数的 1-based 行号。这与
 *    `bodyOffsetOfFileLine` 是同一个坐标系问题的反方向,所以复用同一套口径。
 *
 * 3. **上下文的当前笔记正文由调用方给,不由后端读盘。** 用户问的往往正是刚写下还没
 *    保存的那几行。
 */

import { invoke } from "@tauri-apps/api/core";

export type EmbedProvider = "ollama" | "openAi";

/** 与 Rust 的 `EmbedConfig` 对齐(serde camelCase)。 */
export type RagEmbedConfig = {
  provider: EmbedProvider;
  /** 形如 `http://127.0.0.1:11434` 或 `https://api.openai.com/v1`。末尾斜杠由后端归一。 */
  baseUrl: string;
  model: string;
  /** OpenAI 兼容一路的 key。Ollama 留空。 */
  apiKey: string;
};

export const DEFAULT_RAG_CONFIG: RagEmbedConfig = {
  provider: "ollama",
  baseUrl: "http://127.0.0.1:11434",
  model: "nomic-embed-text",
  apiKey: "",
};

export type RagIndexPhase = "scanning" | "chunking" | "embedding" | "done" | "failed" | "cancelled";

/** 进度事件的载荷。`vault` 由命令层平铺进来 —— 多个 vault 可以同时在建。 */
export type RagIndexProgress = {
  vault: string;
  phase: RagIndexPhase;
  total: number;
  done: number;
  failed: number;
  current: string | null;
  error: string | null;
};

export type RagIndexOutcome = {
  indexed: number;
  skipped: number;
  failed: number;
  removed: number;
  cancelled: boolean;
};

export type RagIndexFailure = {
  path: string;
  error: string;
  attempts: number;
};

export type RagIndexStats = {
  docs: number;
  indexed: number;
  pending: number;
  failed: number;
  stale: number;
  chunks: number;
  failures: RagIndexFailure[];
};

/** 高亮区间。单位是 Unicode 标量,不是 JS 下标 —— 用 `scalarToUtf16` 换。 */
export type RagSpan = { start: number; end: number };

export type RagHit = {
  /** 笔记绝对路径,与笔记列表里的 `id` 同一个值。 */
  path: string;
  title: string;
  /** 标题路径,形如 `设计 > 存储`。 */
  heading: string;
  /** 块的原文,引用预览显示这一份。 */
  body: string;
  score: number;
  /** `vector+fts` 这样的来源标签。 */
  source: string;
  /** 块在**正文**里的字符区间(标量)。 */
  charStart: number;
  charEnd: number;
  /** 高亮区间,相对 `body`(标量)。 */
  bodySpans: RagSpan[];
  /** 高亮区间,相对正文(标量)。 */
  sourceSpans: RagSpan[];
};

/** 哪一路降级了。`stage` 是 `vector` 或 `rerank`。 */
export type RagDegraded = { stage: string; detail: string };

export type RagSearchOutcome = {
  hits: RagHit[];
  degraded: RagDegraded[];
  /** 索引里一个向量都没有。这是"还没准备好"而不是故障 —— 该提示建索引。 */
  vectorsMissing: boolean;
};

/** 上下文里的一条引用。`index` 与命中字段平铺在一起。 */
export type RagCitation = RagHit & { index: number };

export type RagContextBundle = {
  text: string;
  citations: RagCitation[];
  tokens: number;
  truncated: boolean;
  degraded: RagDegraded[];
  vectorsMissing: boolean;
};

export type RagSearchOptions = {
  limit?: number;
  expandLinks?: boolean;
  /** 0 表示不限制同一篇贡献几块 —— 与其它上限不同,0 在这里是有意义的取值。 */
  perDoc?: number;
};

/**
 * Unicode 标量偏移 → JS 字符串下标。
 *
 * 代理对(emoji 等)一个标量占两个码元,所以只能顺着数过去。BMP 内的文本(含全部
 * 中日韩)两者相等,那时这个循环走 `scalar` 步就出来了。
 *
 * 越界收敛到 `text.length`:笔记在建索引之后被改短了是常态,那时"尽量靠近末尾"
 * 比落回 0 好 —— 跳到开头看起来像跳错了笔记。
 */
export function scalarToUtf16(text: string, scalar: number): number {
  if (scalar <= 0) return 0;
  let index = 0;
  let count = 0;
  while (index < text.length && count < scalar) {
    // 代理对占两个码元。`codePointAt` 返回的码点 > 0xffff 就是代理对。
    const code = text.codePointAt(index)!;
    index += code > 0xffff ? 2 : 1;
    count += 1;
  }
  return index;
}

/**
 * 命中在正文里的标量偏移 → 整个 `.md` 文件里的 1-based 行号。
 *
 * `jumpToBacklink` 要的是文件行号(frontmatter 那几行也算),而索引给的是正文里的
 * 字符偏移。两个坐标系不换算的话会稳定地偏几行,偏多少取决于那篇笔记的 frontmatter
 * 有多长 —— 看起来像"有时候准有时候不准"。
 *
 * `fileContent` 由调用方用 `noteFileContent` 拼(和保存、和版本历史 diff 同一个函数)。
 */
export function fileLineOfBodyScalar(fileContent: string, body: string, scalar: number): number {
  const bodyOffset = scalarToUtf16(body, scalar);
  // 正文在文件里的起点。`noteFileContent` 是 frontmatter + 正文,所以差值就是它。
  const bodyStart = Math.max(0, fileContent.length - body.length);
  const fileOffset = bodyStart + bodyOffset;
  let line = 1;
  for (let index = 0; index < fileOffset && index < fileContent.length; index += 1) {
    if (fileContent.charCodeAt(index) === 10) line += 1;
  }
  return line;
}

/** 探一次 embedding 服务:通不通、维度多少。 */
export async function probeRagEmbed(config: RagEmbedConfig): Promise<number> {
  return invoke<number>("notebook_rag_probe", { config });
}

export async function ragIndexStats(vault: string): Promise<RagIndexStats> {
  return invoke<RagIndexStats>("notebook_rag_stats", { vault });
}

/**
 * 建索引。`scope` 为 `failedOnly` 时只重试上次失败的与没做完的。
 *
 * 会一直等到这一轮跑完(或被取消)。进度走 `notebook-rag-index-progress` 事件。
 */
export async function runRagIndex(
  vault: string,
  config: RagEmbedConfig,
  scope: "all" | "failedOnly" = "all",
): Promise<RagIndexOutcome> {
  return invoke<RagIndexOutcome>("notebook_rag_index", { vault, config, scope });
}

/** 取消正在跑的那一轮。返回是否真有一轮在跑。 */
export async function cancelRagIndex(vault: string): Promise<boolean> {
  return invoke<boolean>("notebook_rag_cancel", { vault });
}

/** 删掉整个索引库。 */
export async function clearRagIndex(vault: string): Promise<void> {
  return invoke<void>("notebook_rag_clear", { vault });
}

export async function ragSearch(
  vault: string,
  query: string,
  config: RagEmbedConfig,
  options?: RagSearchOptions,
): Promise<RagSearchOutcome> {
  return invoke<RagSearchOutcome>("notebook_rag_search", { vault, query, config, options });
}

/**
 * 装配一次问答的上下文。
 *
 * `currentBody` 传**编辑器里的当前内容**而不是让后端读盘:用户问的往往正是刚写下
 * 还没保存的那几行。
 */
export async function ragContext(args: {
  vault: string;
  query: string;
  config: RagEmbedConfig;
  currentPath?: string | null;
  currentBody?: string | null;
  searchOptions?: RagSearchOptions;
  contextOptions?: { maxTokens?: number; currentChars?: number };
}): Promise<RagContextBundle> {
  return invoke<RagContextBundle>("notebook_rag_context", {
    vault: args.vault,
    query: args.query,
    config: args.config,
    currentPath: args.currentPath ?? null,
    currentBody: args.currentBody ?? null,
    searchOptions: args.searchOptions ?? null,
    contextOptions: args.contextOptions ?? null,
  });
}

/** 一篇笔记的邻居(它引用的 + 引用它的)。走索引里的 links 表。 */
export async function ragNeighbors(vault: string, path: string, limit?: number): Promise<string[]> {
  return invoke<string[]>("notebook_rag_neighbors", { vault, path, limit: limit ?? null });
}

/** 索引是否值得建 / 重建。给面板上那个提示点用。 */
export function ragNeedsWork(stats: RagIndexStats | null): boolean {
  if (!stats) return false;
  return stats.pending > 0 || stats.stale > 0 || stats.failed > 0;
}

/** 进度百分比(0–100)。`total` 为 0 时返回 0 而不是 NaN。 */
export function ragProgressPercent(progress: RagIndexProgress | null): number {
  if (!progress || progress.total <= 0) return 0;
  return Math.min(100, Math.round((progress.done / progress.total) * 100));
}
