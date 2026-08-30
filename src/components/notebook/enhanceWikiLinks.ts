/* 把渲染后 HTML 里的 `[[...]]` 文本换成可点的链接。
 *
 * 为什么在 DOM 上做而不是在 markdown 渲染前做:`[[` 不是 markdown 语法,marked
 * 会原样放行,于是它会穿过代码块、行内代码、数学公式、链接文本 —— 在源码上用
 * 正则替换的话,代码示例里的 `[[foo]]` 会被变成链接。走 DOM 之后这些容器天然
 * 就在跳过名单里,不需要自己实现一套"这段是不是代码"的判断。
 *
 * 解析规则全部来自 `noteLinks.ts`。这一层只管 DOM。
 */

import { parseWikiLinkBody, resolveLink, type VaultLinkIndex } from "./noteLinks";

/** 链接元素上挂的 dataset 键。点击处理按 `data-wiki-path` 决定跳哪。 */
export const WIKI_LINK_CLASS = "notebook-wikilink";
/** 解析不到目标时额外挂的类名,用来上"死链"样式。 */
export const WIKI_LINK_MISSING_CLASS = "notebook-wikilink-missing";
/** `![[...]]` 的占位容器。取数与填充在 `noteEmbed.ts`,这一层只造壳。 */
export const WIKI_EMBED_CLASS = "notebook-embed";

/**
 * 不进去替换的容器。
 *
 * - `pre`/`code`:代码里的 `[[` 是内容,不是链接。
 * - `a`:已经是链接了,套一层会产出嵌套 `<a>`(HTML 非法,点击行为也不确定)。
 * - `.notebook-math`/`.katex`:数学占位块里放的是原始 TeX,动它会让公式渲染失败。
 * - `.notebook-mermaid`:同理,里面是图的源码。
 * - **还没填好**的嵌入占位:它的 textContent 是原始的 `![[body]]`(优雅降级用),
 *   再跑一遍这个函数会把那段文字变成链接,于是占位就永远填不上了 —— 填充逻辑找的
 *   是文本内容而不是 dataset。填好之后这条选择器不再命中(`data-embed-state` 变成
 *   `filled`),嵌入内容里的 `[[link]]` 才能照常被增强。
 */
const SKIP_SELECTOR =
  "pre,code,a,button,textarea,script,style,.notebook-math,.katex,.notebook-mermaid," +
  `.${WIKI_EMBED_CLASS}:not([data-embed-state="filled"])`;

const WIKI_LINK_RE = /\[\[([^\]\n]{1,200})\]\]/g;

function isSkippable(node: Text): boolean {
  const parent = node.parentElement;
  // 没有父元素的文本节点已经脱离文档,替换它没有意义(而且 replaceChild 会抛)。
  if (!parent) return true;
  return Boolean(parent.closest(SKIP_SELECTOR));
}

/** 给一个链接元素挂上"指向哪 / 是不是死链"。 */
function applyState(
  link: HTMLElement,
  index: VaultLinkIndex,
  target: string,
  labels: WikiLinkLabels,
): void {
  const hit = resolveLink(index, target);
  if (!hit) {
    link.classList.add(WIKI_LINK_MISSING_CLASS);
    // 死链的 title 要说清是"没找到"而不是"加载中" —— 用户第一反应会是自己写错了名字。
    link.title = labels.missing(target);
    delete link.dataset.wikiPath;
    return;
  }
  link.classList.remove(WIKI_LINK_MISSING_CLASS);
  link.dataset.wikiPath = hit.note.path;
  // 歧义要在 title 上说出来:同名的两篇笔记点进去看着一样,不提示的话用户
  // 会以为链接指向的是另一篇。
  link.title = hit.ambiguous ? labels.ambiguous(hit.note.title) : labels.open(hit.note.title);
}

/**
 * 给一个嵌入占位挂上"指向哪 / 是不是死链"。
 *
 * 和 `applyState` 分开写而不是共用:嵌入占位的可见文字是原始语法(`![[x]]`),
 * 解析成功时**不能**把它换成标题 —— 那段文字是填充失败时的降级显示,提前换掉
 * 会让"加载中"看起来像"已经填好了"。
 */
function applyEmbedState(
  el: HTMLElement,
  index: VaultLinkIndex,
  target: string,
  labels: WikiLinkLabels,
): void {
  const hit = resolveLink(index, target);
  if (!hit) {
    el.classList.add(WIKI_LINK_MISSING_CLASS);
    el.title = labels.missing(target);
    delete el.dataset.embedPath;
    return;
  }
  el.classList.remove(WIKI_LINK_MISSING_CLASS);
  el.dataset.embedPath = hit.note.path;
  el.title = hit.ambiguous ? labels.ambiguous(hit.note.title) : labels.open(hit.note.title);
}

/** 文案由调用方注入 —— 这个模块不该 import i18n(测试里也就不用套 Provider)。 */
export type WikiLinkLabels = {
  open: (title: string) => string;
  missing: (target: string) => string;
  ambiguous: (title: string) => string;
};

/**
 * 就地把 `host` 里的 wikilink 文本换成 `<a>`。
 *
 * 幂等:已经处理过的链接(带 `WIKI_LINK_CLASS`)只重算解析状态,不会再套一层。
 * 阅读态每次改字都会重挂 HTML,但主题切换、笔记列表变化会在同一份 DOM 上再调
 * 一次 —— 那时候要更新的是"死链变成了活链",不是重新解析文本。
 */
export function enhanceWikiLinks(
  host: HTMLElement,
  index: VaultLinkIndex,
  labels: WikiLinkLabels,
): void {
  // 先刷新已有链接的解析状态(新建了目标笔记之后,原来的死链要变活)。
  for (const link of Array.from(host.querySelectorAll<HTMLElement>(`a.${WIKI_LINK_CLASS}`))) {
    applyState(link, index, link.dataset.wikiTarget ?? "", labels);
  }
  /* 嵌入占位同理,但**只刷没填好的**:已经填好的那些,内部有一整棵渲染出来的
     DOM(还含它自己的头部链接),重算解析状态只会把 title 覆盖成宿主口径的文案,
     而它的内容早就不来自这一次解析了。 */
  for (const embed of Array.from(
    host.querySelectorAll<HTMLElement>(`.${WIKI_EMBED_CLASS}:not([data-embed-state="filled"])`),
  )) {
    applyEmbedState(embed, index, embed.dataset.embedTarget ?? "", labels);
  }

  const doc = host.ownerDocument;
  const walker = doc.createTreeWalker(host, NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      const text = node.nodeValue ?? "";
      // 便宜的前置筛选:绝大多数文本节点里没有 `[[`。
      if (!text.includes("[[")) return NodeFilter.FILTER_REJECT;
      if (isSkippable(node as Text)) return NodeFilter.FILTER_REJECT;
      return NodeFilter.FILTER_ACCEPT;
    },
  });

  // 先收集再改:TreeWalker 在遍历中途改 DOM 的行为未定义。
  const targets: Text[] = [];
  let node: Node | null;
  while ((node = walker.nextNode())) targets.push(node as Text);

  for (const textNode of targets) {
    const text = textNode.nodeValue ?? "";
    WIKI_LINK_RE.lastIndex = 0;
    const fragment = doc.createDocumentFragment();
    let cursor = 0;
    let match: RegExpExecArray | null;

    while ((match = WIKI_LINK_RE.exec(text))) {
      const raw = match[1] ?? "";
      const parts = parseWikiLinkBody(raw);
      // 拆不出目标(`[[]]`、`[[|x]]`)的原样留着 —— 它不是链接,是用户写的文本。
      if (!parts) continue;

      /* `![[...]]` 是嵌入。那个 `!` 在正则之外,要从前导文本里摘掉,否则渲染出来
         会是一个孤零零的叹号加一块嵌入内容。

         `match.index > cursor` 这个判断不能拿来代替 `> 0`:`cursor` 在同一个文本
         节点里连着两条嵌入时等于前一条的结尾,而 `!` 的位置只和 `match.index` 有关。 */
      const isEmbed = match.index > 0 && text[match.index - 1] === "!";
      const precedingEnd = isEmbed ? match.index - 1 : match.index;
      if (precedingEnd > cursor) {
        fragment.append(doc.createTextNode(text.slice(cursor, precedingEnd)));
      }

      if (isEmbed) {
        // 用 `span` 而不是 `div`:嵌入语法可以出现在段落中间,而 `<div>` 在 `<p>`
        // 里是非法嵌套,浏览器会把段落截断。块级排版靠 CSS。
        const embed = doc.createElement("span");
        embed.className = WIKI_EMBED_CLASS;
        embed.dataset.embedTarget = parts.target;
        embed.dataset.embedRaw = raw;
        if (parts.heading) embed.dataset.embedHeading = parts.heading;
        // 填充前显示原始语法:取数是异步的,这段时间里用户看到的是他自己写的字,
        // 而不是一块空白。填充失败时也就天然留在这个状态。
        embed.textContent = `![[${raw}]]`;
        applyEmbedState(embed, index, parts.target, labels);
        fragment.append(embed);
        cursor = match.index + match[0].length;
        continue;
      }

      const link = doc.createElement("a");
      // `href="#"` 会在点击时把 URL 改成 `#`,而这是个桌面应用的单页视图 ——
      // 用 button role 的 `<a>` 不带 href,靠 tabindex 保住键盘可达性。
      link.className = WIKI_LINK_CLASS;
      link.setAttribute("role", "link");
      link.tabIndex = 0;
      link.dataset.wikiTarget = parts.target;
      link.dataset.wikiRaw = raw;
      if (parts.heading) link.dataset.wikiHeading = parts.heading;
      link.textContent = parts.display;
      applyState(link, index, parts.target, labels);
      fragment.append(link);
      cursor = match.index + match[0].length;
    }

    // 一条都没换成链接时不要动这个节点 —— 无谓的 replaceChild 会打断
    // 浏览器的文本选择。
    if (cursor === 0) continue;
    if (cursor < text.length) fragment.append(doc.createTextNode(text.slice(cursor)));
    textNode.parentNode?.replaceChild(fragment, textNode);
  }
}

/**
 * 从一次点击事件里找出被点的 wikilink 目标路径。
 *
 * 用 `closest` 而不是判断 `event.target` 本身:链接里可能有 `<em>`(别名带
 * markdown 强调时),点在那上面 target 是 `<em>` 而不是 `<a>`。
 *
 * 返回 null 表示这次点击与 wikilink 无关,调用方不该拦。
 */
export function wikiLinkTargetFromEvent(event: Event): { path: string; heading?: string } | null {
  const el = event.target;
  if (!(el instanceof Element)) return null;
  const link = el.closest(`a.${WIKI_LINK_CLASS}`);
  if (!(link instanceof HTMLElement)) return null;
  const path = link.dataset.wikiPath;
  // 死链也要吞掉点击(它是个 `<a>`,不吞会有默认行为),但没有可跳的目标。
  if (!path) return null;
  const heading = link.dataset.wikiHeading;
  return heading ? { path, heading } : { path };
}

/** 这次点击是否落在一条 wikilink 上(含死链)。用来决定要不要 preventDefault。 */
export function isWikiLinkClick(event: Event): boolean {
  const el = event.target;
  if (!(el instanceof Element)) return false;
  return Boolean(el.closest(`a.${WIKI_LINK_CLASS}`));
}
