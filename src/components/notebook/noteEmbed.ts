/* 笔记嵌入(transclusion):把 `![[note]]` / `![[note#小节]]` 的占位填成目标笔记的
 * 渲染内容。
 *
 * 分工:占位由 `enhanceWikiLinks` 造(纯 DOM,不取数),这一层负责取数 → 截段 →
 * 渲染 → 递归。拆开的理由是取数是异步的而增强是同步的 —— 合在一起会让"链接变活"
 * 这类纯 DOM 更新也得等一轮 IO。
 *
 * 与 Markio 的 `noteEmbed.ts` 的差别:
 *
 * 1. **小节截取复用大纲那一份**。Markio 另写了一个 `extractHeadingSection`,自己
 *    重扫源码判断围栏和层级 —— 于是同一篇笔记的大纲和嵌入可能对"这行是不是标题"
 *    给出不同答案(它的大纲来自 Rust,嵌入来自前端正则)。这里走 `analyzeNote` +
 *    `sectionSpans`,和大纲、章节重排是同一次扫描的结果。
 * 2. **环路检测从第 0 层就带上宿主**。Markio 的 `ancestors` 在 depth 0 是空集,
 *    根笔记自己的路径从来没进去过,所以 `A` 里写 `![[A]]` 第一层不会被拦,只能靠
 *    深度上限兜底 —— 用户看到的是自己那篇笔记套三层。这里把宿主路径设成必填,
 *    自嵌在第一层就被拦住。
 * 3. **取数不登记指纹**。走 `peekNote` 而不是 `openNote`,理由见 `notebookApi.ts`
 *    里 `peekNote` 的注释。
 * 4. **剥掉嵌入内容里的 heading id**。`renderNoteMarkdown` 每次调用新建一份 slug
 *    registry,嵌入内容的 id 会和宿主的撞;而大纲跳转用 `querySelector` 取文档序
 *    第一个,撞上之后点宿主的大纲会跳进嵌入块里。
 */

import {
  enhanceWikiLinks,
  WIKI_EMBED_CLASS,
  WIKI_LINK_CLASS,
  WIKI_LINK_MISSING_CLASS,
} from "./enhanceWikiLinks";
import type { WikiLinkLabels } from "./enhanceWikiLinks";
import { resolveLink, type VaultLinkIndex } from "./noteLinks";
import { splitNote } from "./noteFrontmatter";
import { analyzeNote } from "./noteOutline";
import { renderNoteMarkdown, stripHeadingIds } from "./noteRender";
import { sectionSpans } from "./noteSections";

/**
 * 嵌套层数上限。
 *
 * 3 层的意思是:宿主里的嵌入(第 0 层)、它内部的嵌入(第 1 层)、再一层(第 2 层)
 * 都会填,第 3 层拒绝。环路已经由 `ancestors` 拦掉,这个上限管的是**没有环但很深**
 * 的引用链(A→B→C→D…),那种情况下一次渲染要串行读几十个文件。
 */
export const MAX_EMBED_DEPTH = 3;

/** 填充后的容器类名。填好的头部 / 正文各一个,便于上样式。 */
export const EMBED_HEAD_CLASS = "notebook-embed-head";
export const EMBED_BODY_CLASS = "notebook-embed-body";

/** 嵌入相关的文案。和 `WikiLinkLabels` 一样由调用方注入,这个模块不 import i18n。 */
export type EmbedLabels = {
  /** 目标笔记里找不到这个小节。 */
  missingHeading: (heading: string) => string;
  /** 层级过深或成环。 */
  tooDeep: (target: string) => string;
  /** 取数或渲染失败。 */
  failed: (target: string, message: string) => string;
};

export type EmbedFillHandle = {
  /** 停止一切未完成的填充,并回收 `onFilled` 注册的清理。 */
  disconnect(): void;
};

type CancelSignal = { cancelled: boolean };

export type EmbedFillOptions = {
  /**
   * 宿主笔记的路径。**必填**,而且会被当成第 0 层的祖先。
   *
   * 设成必填是为了让"自嵌"在类型上就不可能被漏掉:见模块注释第 2 条。
   */
  hostPath: string;
  /** 取一篇笔记的内容。注入而不是直接 import,便于测试不过 IPC。 */
  read: (path: string) => Promise<string>;
  /** 解析索引。填好之后要给嵌入内容里的 `[[link]]` 再跑一次增强。 */
  index: VaultLinkIndex;
  labels: WikiLinkLabels & EmbedLabels;
  /**
   * 一块嵌入内容填好之后的回调。用来接公式 / Mermaid 的懒渲染和图片尺寸。
   *
   * 返回的清理函数会被 handle 收集:那些是 IntersectionObserver,内容被换掉之后
   * 不 disconnect 就会一直持有已经不在文档里的节点。
   */
  onFilled?: (body: HTMLElement) => (() => void) | void;
};

type FillContext = EmbedFillOptions & {
  signal: CancelSignal;
  /**
   * 本轮已读过的笔记内容。一篇笔记在一页里被嵌入多次(或多层里重复出现)是常见
   * 写法,每次都过一趟 IPC 纯属浪费。
   *
   * 只活在一次填充里,所以不存在"缓存拿到旧内容"的问题 —— 内容变了会重挂 HTML,
   * 那时候是新的一轮。
   */
  cache: Map<string, Promise<string>>;
  /** `disconnect()` 时要跑的清理。 */
  cleanups: (() => void)[];
  /** 本轮认领的占位。取消时要把状态摘掉,否则它们会永远停在 pending。 */
  claimed: Set<HTMLElement>;
};

/** 环路判定用的身份。和 `normalizeLinkTarget` 同口径(折斜杠、折大小写)。 */
function cycleKey(path: string): string {
  return path.replace(/\\/g, "/").toLowerCase();
}

/**
 * 从源码里截出某个小节:该标题行起,到下一个**层级不深于它**的标题为止。
 *
 * 匹配按标题文本(折大小写、trim),和 `scrollToWikiHeading` 一致 —— 用户写
 * `[[笔记#小节]]` 时写的是原文,不是我们算出来的 slug。
 *
 * 找不到返回 null(而不是空串):调用方要能区分"这个小节不存在"和"这个小节是空的",
 * 前者该提示,后者该显示成空。
 */
export function extractHeadingSection(source: string, heading: string): string | null {
  const want = heading.trim().toLowerCase();
  if (!want) return null;
  const { outline } = analyzeNote(source);
  const spans = sectionSpans(outline, source.length);
  const at = outline.findIndex((item) => item.text.trim().toLowerCase() === want);
  if (at < 0) return null;
  const span = spans[at];
  if (!span) return null;
  return source.slice(span.from, span.to);
}

/** 从路径取文件名(带扩展名)。头部显示用 —— 没有更好的名字时的兜底。 */
function fileNameOf(path: string): string {
  return path.split(/[/\\]/).pop() ?? path;
}

/** 把占位摆成"失败"态:保留原始语法那行字,挂死链样式,状态记成 error。 */
function markError(el: HTMLElement, message: string): void {
  el.classList.add(WIKI_LINK_MISSING_CLASS);
  el.dataset.embedState = "error";
  el.title = message;
}

function readCached(context: FillContext, path: string): Promise<string> {
  const hit = context.cache.get(path);
  if (hit) return hit;
  // 存 Promise 而不是结果:同一页里两处嵌入同一篇时,第二处在第一处 await 期间
  // 就要能命中,否则缓存对"同时发起"这种最常见的情况完全无效。
  const pending = context.read(path);
  context.cache.set(path, pending);
  return pending;
}

async function fillOne(
  el: HTMLElement,
  context: FillContext,
  depth: number,
  ancestors: ReadonlySet<string>,
): Promise<void> {
  const target = el.dataset.embedTarget ?? "";
  const path = el.dataset.embedPath;
  // 解析不到目标:`enhanceWikiLinks` 已经挂好死链样式和 title 了,这里只补状态,
  // 免得下一轮又认领一次。
  if (!path) {
    el.dataset.embedState = "error";
    return;
  }

  const key = cycleKey(path);
  if (depth >= MAX_EMBED_DEPTH || ancestors.has(key)) {
    markError(el, context.labels.tooDeep(target));
    return;
  }

  try {
    const source = await readCached(context, path);
    if (context.signal.cancelled) return;

    const heading = el.dataset.embedHeading;
    let content: string;
    if (heading) {
      const section = extractHeadingSection(source, heading);
      if (section == null) {
        markError(el, context.labels.missingHeading(heading));
        return;
      }
      content = section;
    } else {
      // 整篇嵌入时去掉 frontmatter:`---` 包起来的那段在 markdown 里会渲染成一条
      // 分隔线加一行 `key: value` 文本,看着像内容其实是元数据。
      content = splitNote(source).body;
    }

    const { html } = renderNoteMarkdown(content);
    if (context.signal.cancelled) return;

    const doc = el.ownerDocument;
    el.classList.remove(WIKI_LINK_MISSING_CLASS);
    el.textContent = "";

    /* 头部是一条真正的 wikilink:类名和 `data-wiki-path` 都按 `enhanceWikiLinks`
       的约定挂,于是面板那个点击监听不用改一行就能跳过去。 */
    const head = doc.createElement("a");
    head.className = `${WIKI_LINK_CLASS} ${EMBED_HEAD_CLASS}`;
    head.setAttribute("role", "link");
    head.tabIndex = 0;
    head.dataset.wikiTarget = target;
    head.dataset.wikiPath = path;
    if (heading) head.dataset.wikiHeading = heading;
    /* 头部显示真标题(frontmatter 里那个),不是文件名 —— 改过标题的笔记显示文件名
       会让人以为嵌错了。走 `resolveLink` 而不是直接查 `index.byPath`:那张表的键过了
       `normalizeLinkTarget`(去 `.md`、去首尾 `/`、折大小写),拿原始路径去查一定
       落空,而落空的表现只是"标题退回文件名",不会报错、很难看出来。 */
    const title = resolveLink(context.index, path)?.note.title;
    const name = title || fileNameOf(path);
    head.textContent = heading ? `${name} › ${heading}` : name;
    head.title = context.labels.open(name);
    el.append(head);

    const body = doc.createElement("span");
    body.className = EMBED_BODY_CLASS;
    body.innerHTML = stripHeadingIds(html);
    el.append(body);

    /* 状态要在增强**之前**置成 filled:`enhanceWikiLinks` 会跳过没填好的占位
       (见它的 SKIP_SELECTOR),不先改状态的话嵌入内容里的 `[[link]]` 一条都不会
       被处理。 */
    el.dataset.embedState = "filled";

    const cleanup = context.onFilled?.(body);
    if (cleanup) context.cleanups.push(cleanup);

    /* 嵌入内容里的 `[[link]]` 照常增强。这一步的顺序不能和下一步换:嵌套的
       `![[..]]` 此刻还是纯文本,占位是 `enhanceWikiLinks` 造出来的,先填后增强的话
       第二层永远是空的。 */
    enhanceWikiLinks(body, context.index, context.labels);
    // 嵌套嵌入继续往下填(带环路 + 深度保护)。
    enhanceEmbedContent(body, context, depth + 1, new Set([...ancestors, key]));
  } catch (error) {
    if (context.signal.cancelled) return;
    markError(el, context.labels.failed(target, (error as Error).message));
  }
}

/**
 * 认领并填充 `root` 下的嵌入占位。
 *
 * 认领标准:还没有状态的,或者上一轮失败的。失败的要重来是因为死链会变活 ——
 * 用户新建了那篇笔记之后,`enhanceWikiLinks` 会把 `data-embed-path` 补上,这一层
 * 得跟着再试一次。已经 filled 的不动:内容还在,重填只会闪一下。
 */
function enhanceEmbedContent(
  root: HTMLElement,
  context: FillContext,
  depth: number,
  ancestors: ReadonlySet<string>,
): void {
  const pending = Array.from(
    root.querySelectorAll<HTMLElement>(
      `.${WIKI_EMBED_CLASS}:not([data-embed-state]),` +
        `.${WIKI_EMBED_CLASS}[data-embed-state="error"]`,
    ),
  );
  for (const el of pending) {
    // 先占位再 await:同一轮里两次调用(依赖变化连着触发)不能对同一个占位各发
    // 一次请求 —— 两次填充会互相覆盖,后到的那次还可能是旧索引算出来的。
    el.dataset.embedState = "pending";
    context.claimed.add(el);
    void fillOne(el, context, depth, ancestors);
  }
}

/**
 * 填充 `root` 下所有嵌入占位。返回的 handle 要在内容换掉 / 容器卸载时 disconnect。
 */
export function enhanceNoteEmbeds(root: HTMLElement, options: EmbedFillOptions): EmbedFillHandle {
  const context: FillContext = {
    ...options,
    signal: { cancelled: false },
    cache: new Map(),
    cleanups: [],
    claimed: new Set(),
  };
  enhanceEmbedContent(root, context, 0, new Set([cycleKey(options.hostPath)]));
  return {
    disconnect: () => {
      context.signal.cancelled = true;
      for (const el of context.claimed) {
        /* 停在 pending 的占位要把状态摘掉。留着的话它既不显示内容、又不会被下一轮
           认领 —— 用户看到的是一段永远停在原始语法上的文字。这种情况很常见:
           `linkIndex` 一变这个 effect 就重跑,上一轮的请求正好还在飞。 */
        if (el.dataset.embedState === "pending") delete el.dataset.embedState;
      }
      for (const cleanup of context.cleanups) cleanup();
    },
  };
}
