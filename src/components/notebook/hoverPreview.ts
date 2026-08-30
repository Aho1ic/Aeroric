/* wikilink 悬浮预览:鼠标停在一条已解析的 `[[链接]]` 上,弹出目标笔记开头那几行。
 *
 * 纯附加 —— 不改链接本身的渲染,也不改点击行为。挂在 `document.body` 上而不是预览区
 * 里面:预览区有 `overflow: auto`,弹卡放进去会被裁掉半张。
 *
 * 与 Markio 的 `hoverPreview.ts` 的差别:
 *
 * 1. **键盘也能唤出**。Markio 只听 mouseover / mouseout,而这些链接是 `tabIndex=0`
 *    的 —— 用键盘走到链接上什么都不会发生。这里同时听 focusin / focusout,Tab 过去
 *    就出卡(不带延迟:键盘用户是明确停在这一条上的,不存在"路过"的问题)。
 * 2. **取数不登记指纹**。走 `peekNote`,理由见 `notebookApi.ts` 里那段注释。
 * 3. **截断不切断围栏**。Markio 按行数硬切,切在 ```` ``` ```` 中间时剩下一个没闭合的
 *    围栏,marked 会把后面全部当代码 —— 一张全是灰底的卡片。这里切完补上闭合。
 * 4. **剥掉 heading id**。和嵌入同一个理由(见 `noteRender.ts` 的 `stripHeadingIds`)。
 * 5. **不给嵌入块的头部挂预览**。那块内容就在头部下面摊开着,再弹一张卡挡住它没有意义。
 */

import { WIKI_LINK_CLASS } from "./enhanceWikiLinks";
import { EMBED_HEAD_CLASS } from "./noteEmbed";
import { splitNote } from "./noteFrontmatter";
import { renderNoteMarkdown, stripHeadingIds } from "./noteRender";

/** 鼠标停多久才出卡。太短会在扫过一段满是链接的正文时连闪好几张。 */
export const HOVER_SHOW_DELAY = 380;
/** 移开多久才收卡。留一段时间是为了让鼠标能从链接移到卡片上。 */
export const HOVER_HIDE_DELAY = 180;
/** 预览最多取多少行。 */
export const HOVER_MAX_LINES = 40;

export const HOVER_CARD_CLASS = "notebook-hover-card";
export const HOVER_HEAD_CLASS = "notebook-hover-head";
export const HOVER_BODY_CLASS = "notebook-hover-body";

/** 弹卡与链接之间的间距,以及与视口边缘的最小留白。 */
const GAP = 6;
const EDGE = 8;

export type HoverPreviewLabels = {
  loading: () => string;
  failed: () => string;
};

export type HoverPreviewOptions = {
  /** 取一篇笔记的内容。注入而不是直接 import,便于测试不过 IPC。 */
  read: (path: string) => Promise<string>;
  /** 路径 → 显示标题。拿不到时调用方返回 undefined,这里退回文件名。 */
  titleOf: (path: string) => string | undefined;
  labels: HoverPreviewLabels;
};

export type HoverPreviewHandle = {
  disconnect(): void;
};

/**
 * 取正文前若干行作预览。
 *
 * 去掉 frontmatter 用 `splitNote` 而不是自己写正则:那套边界规则(未闭合的 `---` 不算
 * frontmatter 等)已经有一份,两份迟早对不上,而对不上的表现是"某些笔记的预览开头多
 * 出一行 `title:`"。
 *
 * 截断处如果落在围栏里,补一行闭合。不补的话 marked 会把后面剩下的全部当代码块 ——
 * 一张全是灰底的卡片,而且看不出是被截断了。
 */
export function previewSnippet(source: string, maxLines = HOVER_MAX_LINES): string {
  const body = splitNote(source).body;
  const lines = body.split("\n");
  if (lines.length <= maxLines) return body.trim();

  const kept = lines.slice(0, maxLines);
  // 数围栏而不是配对:```` ``` ```` 和 `~~~` 都算,奇数说明切在里面。
  let fence: string | null = null;
  for (const line of kept) {
    const match = /^\s*(```+|~~~+)/.exec(line);
    if (!match) continue;
    const mark = match[1]!;
    if (fence === null) {
      fence = mark;
    } else if (mark[0] === fence[0] && mark.length >= fence.length) {
      fence = null;
    }
  }
  if (fence) kept.push(fence);
  // 省略号单独成段:让"还有更多"这件事在卡片里看得见。
  return `${kept.join("\n").trim()}\n\n…`;
}

export type HoverRect = { top: number; bottom: number; left: number };
export type HoverBox = { width: number; height: number };
export type HoverViewport = { width: number; height: number };

/**
 * 算弹卡的位置。抽成纯函数是为了能直接测"贴边翻转"这件事 —— jsdom 没有布局,
 * `offsetWidth` / `getBoundingClientRect` 全是 0,在 DOM 上测等于什么都没测。
 *
 * 规则:默认贴在链接下方;下面放不下**而且**上面放得下时翻到上方;水平方向超出右边界
 * 就往左推,但不越过左边界。
 */
export function computeHoverPosition(
  anchor: HoverRect,
  card: HoverBox,
  viewport: HoverViewport,
): { top: number; left: number } {
  let top = anchor.bottom + GAP;
  const belowOverflows = top + card.height > viewport.height - EDGE;
  const aboveFits = anchor.top - GAP - card.height > EDGE;
  if (belowOverflows && aboveFits) top = anchor.top - GAP - card.height;

  let left = anchor.left;
  if (left + card.width > viewport.width - EDGE) left = viewport.width - EDGE - card.width;
  // 顺序不能反:窄视口下上面那一步会算出负数,这一步兜住它。
  if (left < EDGE) left = EDGE;

  return { top: Math.max(EDGE, top), left };
}

/** 悬浮要认的链接:wikilink,但不是嵌入块的头部。 */
const HOVER_SELECTOR = `a.${WIKI_LINK_CLASS}:not(.${EMBED_HEAD_CLASS})`;

/**
 * 事件目标 → 要预览的链接和它的路径。
 *
 * "有没有路径"这件事只在这里判一次。选择器里不再重复写 `[data-wiki-path]` —— 两处
 * 各判一遍的话,谁都不是唯一的那道闸,改了一处另一处会默默兜住,测试也就钉不住任何
 * 一处。死链(没有路径)在这里就被挡掉。
 */
function hoverTargetFrom(target: EventTarget | null): { link: HTMLElement; path: string } | null {
  if (!(target instanceof Element)) return null;
  const link = target.closest<HTMLElement>(HOVER_SELECTOR);
  const path = link?.dataset.wikiPath;
  if (!link || !path) return null;
  return { link, path };
}

function fileNameOf(path: string): string {
  return path.split(/[/\\]/).pop() ?? path;
}

/**
 * 给 `root` 里的 wikilink 装上悬浮预览。返回的 handle 要在内容换掉 / 容器卸载时
 * `disconnect()` —— 它会解绑监听并把弹卡从 body 上摘掉。
 */
export function attachWikiLinkHover(
  root: HTMLElement,
  options: HoverPreviewOptions,
): HoverPreviewHandle {
  const doc = root.ownerDocument;
  const view = doc.defaultView ?? window;
  let card: HTMLElement | null = null;
  let showTimer = 0;
  let hideTimer = 0;
  /**
   * 当前"意图预览"的链接。
   *
   * 不变式:`showTimer` 一旦排上,它一定属于此刻的 `current` —— 改 `current` 的路径
   * (`arm` / `hide` / `disconnect`)都先把 `showTimer` 清掉。定时器回调里因此不需要
   * 再核对一次链接。
   */
  let current: HTMLElement | null = null;
  /** 渲染结果按路径缓存。只活到 disconnect —— 内容一变面板就重挂,那时是新的一轮。 */
  const cache = new Map<string, string>();

  function ensureCard(): HTMLElement {
    if (card) return card;
    const el = doc.createElement("div");
    el.className = HOVER_CARD_CLASS;
    el.setAttribute("role", "tooltip");
    // 鼠标移进卡片时取消收起 —— 卡片里可能有要读的长文本。
    el.addEventListener("mouseenter", () => view.clearTimeout(hideTimer));
    el.addEventListener("mouseleave", scheduleHide);
    doc.body.append(el);
    card = el;
    return el;
  }

  function place(link: HTMLElement): void {
    if (!card) return;
    const rect = link.getBoundingClientRect();
    const at = computeHoverPosition(
      { top: rect.top, bottom: rect.bottom, left: rect.left },
      { width: card.offsetWidth, height: card.offsetHeight },
      { width: view.innerWidth, height: view.innerHeight },
    );
    card.style.top = `${at.top}px`;
    card.style.left = `${at.left}px`;
  }

  function hide(): void {
    view.clearTimeout(showTimer);
    view.clearTimeout(hideTimer);
    current = null;
    if (card) card.style.display = "none";
  }

  function scheduleHide(): void {
    view.clearTimeout(hideTimer);
    hideTimer = view.setTimeout(hide, HOVER_HIDE_DELAY);
  }

  /**
   * 摆好卡片的骨架(头部 + 正文容器),返回正文容器。
   *
   * `replaceChildren()` 把上一次的正文容器摘下来 —— 这就是"迟到的取数写不脏卡片"的
   * 真正机制:它手里那个容器已经不在卡片上了,往里写没人看得见。
   */
  function openCard(path: string): HTMLElement {
    const el = ensureCard();
    el.replaceChildren();
    const head = doc.createElement("div");
    head.className = HOVER_HEAD_CLASS;
    // textContent 而不是拼 HTML:标题来自 frontmatter,是用户写的内容。
    head.textContent = options.titleOf(path) || fileNameOf(path);
    const next = doc.createElement("div");
    next.className = HOVER_BODY_CLASS;
    el.append(head, next);
    el.style.display = "block";
    return next;
  }

  async function fill(link: HTMLElement, path: string): Promise<void> {
    const cached = cache.get(path);
    if (cached) {
      openCard(path).innerHTML = cached;
      place(link);
      return;
    }

    const target = openCard(path);
    target.textContent = options.labels.loading();
    // 先摆一次:骨架的尺寸已经能用来定位,不然"载入中"那一帧会闪在左上角。
    place(link);

    /* 异步回来时只有一条守卫:这条链接还是当前那条吗。
       它不是为了防止写脏内容 —— `openCard` 的 `replaceChildren` 已经把旧容器摘下来了,
       写进去也没人看得见。它挡的是后面那句 `place()`:一次迟到的取数会按**旧链接**的
       位置把卡片挪走,于是卡片显示着 B 的内容却贴在 A 旁边。 */
    try {
      const source = await options.read(path);
      if (current !== link) return;
      const html = stripHeadingIds(renderNoteMarkdown(previewSnippet(source)).html);
      cache.set(path, html);
      target.innerHTML = html;
      place(link);
    } catch {
      if (current !== link) return;
      target.textContent = options.labels.failed();
      place(link);
    }
  }

  function arm(link: HTMLElement, path: string, delay: number): void {
    view.clearTimeout(hideTimer);
    view.clearTimeout(showTimer);
    current = link;
    if (delay <= 0) {
      void fill(link, path);
      return;
    }
    // 不用再核对 current:见它声明处的不变式。
    showTimer = view.setTimeout(() => void fill(link, path), delay);
  }

  const onOver = (event: Event): void => {
    const hit = hoverTargetFrom(event.target);
    if (!hit) return;
    // 已经是当前这一条:只取消收起,不重排定时器 —— 否则在链接内部移动
    // (链接里有 `<em>` 时)会让卡片永远出不来。
    if (hit.link === current) {
      view.clearTimeout(hideTimer);
      return;
    }
    arm(hit.link, hit.path, HOVER_SHOW_DELAY);
  };

  const onOut = (event: Event): void => {
    if (!hoverTargetFrom(event.target)) return;
    // 移到卡片上不算离开。
    const to = (event as MouseEvent).relatedTarget;
    if (to instanceof Node && card?.contains(to)) return;
    view.clearTimeout(showTimer);
    scheduleHide();
  };

  /* 键盘路径不带延迟:延迟是为了过滤鼠标"路过",而 Tab 到某一条链接上是明确的选择。 */
  const onFocusIn = (event: Event): void => {
    const hit = hoverTargetFrom(event.target);
    if (!hit) return;
    arm(hit.link, hit.path, 0);
  };

  const onFocusOut = (event: Event): void => {
    if (!hoverTargetFrom(event.target)) return;
    hide();
  };

  /* 滚动就收:弹卡是 `position: fixed`,滚动之后它会停在原地而链接已经走了。
     capture 是必须的 —— 滚动发生在预览区内部,不冒泡到 root。 */
  const onScroll = (): void => hide();
  const onKeyDown = (event: KeyboardEvent): void => {
    if (event.key === "Escape") hide();
  };

  root.addEventListener("mouseover", onOver);
  root.addEventListener("mouseout", onOut);
  root.addEventListener("focusin", onFocusIn);
  root.addEventListener("focusout", onFocusOut);
  root.addEventListener("scroll", onScroll, { passive: true, capture: true });
  doc.addEventListener("keydown", onKeyDown);

  return {
    disconnect() {
      root.removeEventListener("mouseover", onOver);
      root.removeEventListener("mouseout", onOut);
      root.removeEventListener("focusin", onFocusIn);
      root.removeEventListener("focusout", onFocusOut);
      root.removeEventListener("scroll", onScroll, { capture: true });
      doc.removeEventListener("keydown", onKeyDown);
      /* 走 `hide()` 而不是把它那几行抄一遍:清两个定时器 + 清 `current` 在这两处是
         同一件事。排在路上的出卡定时器尤其不能留 —— 它会走 `openCard`,而 `openCard`
         会现造一张新卡挂到 body 上,卸载之后又浮出来一张,而且再没人负责摘它。 */
      hide();
      // 卡片挂在 body 上,不摘会在切模式 / 卸载之后留一张浮在界面上。
      card?.remove();
      card = null;
    },
  };
}
