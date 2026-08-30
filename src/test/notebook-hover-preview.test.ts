import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  HOVER_BODY_CLASS,
  HOVER_CARD_CLASS,
  HOVER_HEAD_CLASS,
  HOVER_HIDE_DELAY,
  HOVER_MAX_LINES,
  HOVER_SHOW_DELAY,
  attachWikiLinkHover,
  computeHoverPosition,
  previewSnippet,
} from "../components/notebook/hoverPreview";
import { enhanceWikiLinks } from "../components/notebook/enhanceWikiLinks";
import { buildLinkIndex } from "../components/notebook/noteLinks";

const labels = {
  loading: () => "载入中…",
  failed: () => "无法载入预览",
};

const wikiLabels = {
  open: (title: string) => `打开 ${title}`,
  missing: (target: string) => `未找到笔记:${target}`,
  ambiguous: (title: string) => `多篇同名,打开 ${title}`,
};

describe("previewSnippet", () => {
  it("去掉 frontmatter,只留正文", () => {
    expect(previewSnippet("---\ntitle: 甲\ntags: [x]\n---\n正文一行\n")).toBe("正文一行");
  });

  it("没有 frontmatter 时原样返回(去首尾空白)", () => {
    expect(previewSnippet("\n\n就是正文\n\n")).toBe("就是正文");
  });

  it("未闭合的 --- 不算 frontmatter", () => {
    // 这条边界和 splitNote 共用一份,所以这里钉的是"确实共用了"。
    expect(previewSnippet("---\ntitle: 甲\n没有闭合\n")).toBe("---\ntitle: 甲\n没有闭合");
  });

  it("超过行数上限时截断并补省略号", () => {
    const long = Array.from({ length: 60 }, (_, i) => `行 ${i}`).join("\n");
    const snippet = previewSnippet(long, 5);
    expect(snippet.startsWith("行 0")).toBe(true);
    expect(snippet).toContain("行 4");
    expect(snippet).not.toContain("行 5");
    expect(snippet.endsWith("\n\n…")).toBe(true);
  });

  it("刚好等于上限时不截断", () => {
    const exact = "a\nb\nc";
    expect(previewSnippet(exact, 3)).toBe("a\nb\nc");
  });

  it("围栏里的另一种围栏不算闭合", () => {
    // ``` 块里的 ~~~ 是内容,不是收尾 —— 补的那一行必须跟开头同一种。
    const source = "```\n~~~\na\nb\n~~~\nc\n";
    const snippet = previewSnippet(source, 3);
    expect(snippet.endsWith("```\n\n…")).toBe(true);
    expect(snippet.match(/```/g)).toHaveLength(2);
  });

  it("切在围栏里时补上闭合", () => {
    /* 不补的话 marked 会把后面剩下的全部当代码块 —— 一张全是灰底的卡片,而且
       看不出是被截断了。Markio 按行数硬切,没有这一步。 */
    const source = "前言\n```js\nconst a = 1;\nconst b = 2;\nconst c = 3;\n```\n后文\n";
    const snippet = previewSnippet(source, 4);
    // 围栏必须成对:开一个补一个。
    expect(snippet.match(/```/g)).toHaveLength(2);
    expect(snippet).toContain("const b");
    expect(snippet).not.toContain("const c");
  });

  it("切在围栏外时不多补", () => {
    const source = "```\nx\n```\n后面还有很多\n再一行\n";
    const snippet = previewSnippet(source, 4);
    expect(snippet.match(/```/g)).toHaveLength(2);
  });

  it("~~~ 围栏同样处理", () => {
    const source = "~~~\na\nb\nc\n~~~\n尾\n";
    const snippet = previewSnippet(source, 3);
    expect(snippet.match(/~~~/g)).toHaveLength(2);
  });

  it("默认上限是 40 行", () => {
    const long = Array.from({ length: 41 }, (_, i) => `行 ${i}`).join("\n");
    expect(HOVER_MAX_LINES).toBe(40);
    expect(previewSnippet(long)).toContain("行 39");
    expect(previewSnippet(long)).not.toContain("行 40");
  });
});

describe("computeHoverPosition", () => {
  const card = { width: 200, height: 100 };
  const viewport = { width: 1000, height: 800 };

  it("默认贴在链接下方", () => {
    const at = computeHoverPosition({ top: 100, bottom: 120, left: 300 }, card, viewport);
    expect(at).toEqual({ top: 126, left: 300 });
  });

  it("下面放不下且上面放得下时翻到上方", () => {
    const at = computeHoverPosition({ top: 700, bottom: 720, left: 300 }, card, viewport);
    expect(at.top).toBe(594);
  });

  it("上下都放不下时仍留在下方(不往上翻出视口)", () => {
    // 视口很矮:翻上去会超出顶部,那样更糟 —— 卡片至少要能看见开头。
    const at = computeHoverPosition({ top: 40, bottom: 60, left: 10 }, card, {
      width: 1000,
      height: 120,
    });
    expect(at.top).toBe(66);
  });

  it("超出右边界时往左推", () => {
    const at = computeHoverPosition({ top: 100, bottom: 120, left: 900 }, card, viewport);
    expect(at.left).toBe(792);
  });

  it("上方只差一点点放不下时不翻上去", () => {
    /* 边界那一格:上方剩 6px,比留白 8px 小。翻上去会贴死在顶边(或者被夹到 8
       之后压在链接上),留在下方更好。 */
    const at = computeHoverPosition({ top: 112, bottom: 130, left: 10 }, card, {
      width: 1000,
      height: 200,
    });
    expect(at.top).toBe(136);
  });

  it("链接被滚到视口上方时不给出负的 top", () => {
    // 阅读区滚动时链接可以走到视口外,算出来的位置必须还在视口里。
    const at = computeHoverPosition({ top: -200, bottom: -180, left: 10 }, card, viewport);
    expect(at.top).toBe(8);
  });

  it("视口比卡片还窄时贴左边界", () => {
    // 往左推会算出负数,兜底那一步必须在后面跑。
    const at = computeHoverPosition({ top: 100, bottom: 120, left: 50 }, card, {
      width: 150,
      height: 800,
    });
    expect(at.left).toBe(8);
  });
});

describe("attachWikiLinkHover", () => {
  const index = buildLinkIndex([
    { path: "/v/target.md", title: "目标篇" },
    { path: "/v/other.md", title: "另一篇" },
  ]);

  let root: HTMLElement;

  beforeEach(() => {
    vi.useFakeTimers();
    root = document.createElement("div");
    document.body.append(root);
  });

  afterEach(() => {
    vi.useRealTimers();
    root.remove();
    for (const card of Array.from(document.querySelectorAll(`.${HOVER_CARD_CLASS}`))) {
      card.remove();
    }
  });

  function setup(
    html: string,
    options: {
      read?: (path: string) => Promise<string>;
      titleOf?: (path: string) => string | undefined;
    } = {},
  ) {
    root.innerHTML = html;
    enhanceWikiLinks(root, index, wikiLabels);
    const read = options.read ?? (async () => "被预览的正文\n");
    const handle = attachWikiLinkHover(root, {
      read,
      titleOf: options.titleOf ?? ((path) => (path === "/v/target.md" ? "目标篇" : undefined)),
      labels,
    });
    return { handle, link: root.querySelector<HTMLElement>("a.notebook-wikilink")! };
  }

  function card(): HTMLElement | null {
    return document.querySelector<HTMLElement>(`.${HOVER_CARD_CLASS}`);
  }

  function bodyText(): string {
    return document.querySelector(`.${HOVER_BODY_CLASS}`)?.textContent?.trim() ?? "";
  }

  /** 推进假时钟并把已排队的微任务跑完。 */
  async function tick(ms: number) {
    await vi.advanceTimersByTimeAsync(ms);
  }

  it("停留够久之后弹出目标笔记的开头", async () => {
    const { link } = setup("<p>见 [[目标篇]]</p>");
    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));

    // 延迟没到之前什么都不该出现 —— 扫过一段满是链接的正文不该连闪好几张卡。
    await tick(HOVER_SHOW_DELAY - 50);
    expect(card()).toBeNull();

    await tick(60);
    expect(card()?.style.display).toBe("block");
    expect(document.querySelector(`.${HOVER_HEAD_CLASS}`)?.textContent).toBe("目标篇");
    expect(bodyText()).toBe("被预览的正文");
  });

  /**
   * 在测试期间接管 `offsetHeight`,让 `place()` 能算出真的位置。
   * jsdom 没有布局,这个值恒为 0,所以"卡片多高"这件事只能自己喂。
   */
  function withCardHeight(read: () => number): () => void {
    const original = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetHeight");
    Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
      configurable: true,
      get: read,
    });
    return () => {
      if (original) Object.defineProperty(HTMLElement.prototype, "offsetHeight", original);
    };
  }

  it("载入中那一帧就已经摆到链接旁边", async () => {
    // 不先摆一次的话,"载入中"会先闪在视口左上角再跳到链接旁边。
    const { link } = setup("<p>[[目标篇]]</p>", { read: () => new Promise<string>(() => {}) });
    stubRect(link, { top: 100, bottom: 120, left: 40 });
    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);

    expect(bodyText()).toBe("载入中…");
    expect(card()?.style.top).toBe("126px");
    expect(card()?.style.left).toBe("40px");
  });

  it("内容比骨架高时重新摆一次", async () => {
    /* 骨架只有一行"载入中",内容可能有几十行。按骨架的高度摆完就不管,内容一填上来
       就会捅出视口下边 —— 必须拿新的高度再摆一次。 */
    let cardHeight = 40;
    const restore = withCardHeight(() => cardHeight);
    try {
      let release!: (value: string) => void;
      const { link } = setup("<p>[[目标篇]]</p>", {
        read: () =>
          new Promise<string>((resolve) => {
            release = resolve;
          }),
      });
      // 链接靠底:骨架放得下,长内容放不下。
      stubRect(link, { top: 600, bottom: 620, left: 40 });
      link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
      await tick(HOVER_SHOW_DELAY);
      expect(card()?.style.top).toBe("626px");

      cardHeight = 400;
      release("很长的正文\n");
      await tick(0);
      // 翻到链接上方:600 - 6 - 400。
      expect(card()?.style.top).toBe("194px");
    } finally {
      restore();
    }
  });

  it("弹出前先显示载入中,之后换成内容", async () => {
    let release!: (value: string) => void;
    const { link } = setup("<p>[[目标篇]]</p>", {
      read: () =>
        new Promise<string>((resolve) => {
          release = resolve;
        }),
    });
    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    expect(bodyText()).toBe("载入中…");

    release("真正的内容\n");
    await tick(0);
    expect(bodyText()).toBe("真正的内容");
  });

  it("取数失败时卡片说明加载不出来,不抛出去", async () => {
    const { link } = setup("<p>[[目标篇]]</p>", {
      read: () => Promise.reject(new Error("读盘失败")),
    });
    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    expect(bodyText()).toBe("无法载入预览");
  });

  it("移开之后收起卡片", async () => {
    const { link } = setup("<p>[[目标篇]]</p>");
    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    expect(card()?.style.display).toBe("block");

    link.dispatchEvent(new MouseEvent("mouseout", { bubbles: true }));
    await tick(HOVER_HIDE_DELAY - 50);
    // 收起也带延迟:这段时间是留给"从链接移到卡片上"的。
    expect(card()?.style.display).toBe("block");
    await tick(60);
    expect(card()?.style.display).toBe("none");
  });

  it("移到卡片上不算离开", async () => {
    const { link } = setup("<p>[[目标篇]]</p>");
    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);

    link.dispatchEvent(new MouseEvent("mouseout", { bubbles: true, relatedTarget: card()! }));
    await tick(HOVER_HIDE_DELAY + 50);
    expect(card()?.style.display).toBe("block");
  });

  it("移开前就走掉的话根本不取数", async () => {
    const read = vi.fn(async () => "不该被读到\n");
    const { link } = setup("<p>[[目标篇]]</p>", { read });
    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY - 100);
    link.dispatchEvent(new MouseEvent("mouseout", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY + HOVER_HIDE_DELAY);
    expect(read).not.toHaveBeenCalled();
  });

  it("在链接内部移动不会重排定时器", async () => {
    // 链接里可能有 <em>(别名带 markdown 强调时)。每次 mouseover 都重排的话,
    // 鼠标在链接上轻微移动会让卡片永远出不来。
    const { link } = setup("<p>[[目标篇]]</p>");
    link.innerHTML = "<em>目标篇</em>";
    const inner = link.querySelector("em")!;
    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY - 60);
    inner.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(80);
    expect(card()?.style.display).toBe("block");
  });

  /** 给某个元素钉一个假的位置。jsdom 没有布局,所有 rect 默认全是 0。 */
  function stubRect(el: HTMLElement, rect: { top: number; bottom: number; left: number }) {
    el.getBoundingClientRect = () =>
      ({ ...rect, right: rect.left + 80, width: 80, height: rect.bottom - rect.top }) as DOMRect;
  }

  it("鼠标移到另一条链接上时(先 mouseout 再 mouseover)换成那一篇", async () => {
    /* 真实的移动一定是这个顺序:离开 A 会先发 mouseout。mouseout 已经排上收起,
       arm(B) 不把它取消掉的话,B 的卡片刚弹出来就被那次收起关掉 —— 更糟的是 hide
       会清掉 current,于是 B 的内容回来时被守卫拦下,卡片停在"载入中"。 */
    root.innerHTML = "<p>[[目标篇]] 和 [[另一篇]]</p>";
    enhanceWikiLinks(root, index, wikiLabels);
    attachWikiLinkHover(root, {
      read: async (path) => `${path} 的内容\n`,
      titleOf: () => undefined,
      labels,
    });
    const [first, second] = Array.from(root.querySelectorAll<HTMLElement>("a.notebook-wikilink"));

    first!.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    first!.dispatchEvent(new MouseEvent("mouseout", { bubbles: true }));
    second!.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY + HOVER_HIDE_DELAY);

    expect(card()?.style.display).toBe("block");
    expect(bodyText()).toBe("/v/other.md 的内容");
  });

  it("延迟没走完就挪到另一条上时,前一条不抢先弹出来", async () => {
    root.innerHTML = "<p>[[目标篇]] 和 [[另一篇]]</p>";
    enhanceWikiLinks(root, index, wikiLabels);
    attachWikiLinkHover(root, {
      read: async (path) => `${path} 的内容\n`,
      titleOf: () => undefined,
      labels,
    });
    const [first, second] = Array.from(root.querySelectorAll<HTMLElement>("a.notebook-wikilink"));

    first!.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(200);
    second!.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    // 到这里 A 原本排的那一枪(380ms)已经过点了,B 的还没到。
    await tick(220);
    expect(card()).toBeNull();

    await tick(HOVER_SHOW_DELAY);
    expect(bodyText()).toBe("/v/other.md 的内容");
  });

  it("迟到的取数不把卡片挪回旧链接旁边", async () => {
    /* 内容写不脏(旧容器已经被 replaceChildren 摘下来了),但定位会:没有守卫的话
       A 的那次回来时会按 A 的位置摆卡,于是卡片显示着 B 的内容却贴在 A 旁边。 */
    const pending = new Map<string, (value: string) => void>();
    root.innerHTML = "<p>[[目标篇]] 和 [[另一篇]]</p>";
    enhanceWikiLinks(root, index, wikiLabels);
    attachWikiLinkHover(root, {
      read: (path) =>
        new Promise<string>((resolve) => {
          pending.set(path, resolve);
        }),
      titleOf: () => undefined,
      labels,
    });
    const [first, second] = Array.from(root.querySelectorAll<HTMLElement>("a.notebook-wikilink"));
    stubRect(first!, { top: 100, bottom: 120, left: 40 });
    stubRect(second!, { top: 500, bottom: 520, left: 300 });

    first!.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    first!.dispatchEvent(new MouseEvent("mouseout", { bubbles: true }));
    second!.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);

    pending.get("/v/other.md")!("B 的内容\n");
    await tick(0);
    const placed = card()!.style.top;
    expect(placed).toBe("526px");

    // A 的那次现在才回来。
    pending.get("/v/target.md")!("A 的内容\n");
    await tick(0);
    expect(card()!.style.top).toBe(placed);
    expect(bodyText()).toBe("B 的内容");
  });

  it("迟到的失败也不把卡片挪回旧链接旁边", async () => {
    const pending = new Map<string, (reason: Error) => void>();
    const resolved = new Map<string, (value: string) => void>();
    root.innerHTML = "<p>[[目标篇]] 和 [[另一篇]]</p>";
    enhanceWikiLinks(root, index, wikiLabels);
    attachWikiLinkHover(root, {
      read: (path) =>
        new Promise<string>((resolve, reject) => {
          resolved.set(path, resolve);
          pending.set(path, reject);
        }),
      titleOf: () => undefined,
      labels,
    });
    const [first, second] = Array.from(root.querySelectorAll<HTMLElement>("a.notebook-wikilink"));
    stubRect(first!, { top: 100, bottom: 120, left: 40 });
    stubRect(second!, { top: 500, bottom: 520, left: 300 });

    first!.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    first!.dispatchEvent(new MouseEvent("mouseout", { bubbles: true }));
    second!.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    resolved.get("/v/other.md")!("B 的内容\n");
    await tick(0);
    const placed = card()!.style.top;

    pending.get("/v/target.md")!(new Error("读盘失败"));
    await tick(0);
    expect(card()!.style.top).toBe(placed);
    expect(bodyText()).toBe("B 的内容");
  });

  it("换到另一条链接时换成那一篇", async () => {
    const seen: string[] = [];
    root.innerHTML = "<p>[[目标篇]] 和 [[另一篇]]</p>";
    enhanceWikiLinks(root, index, wikiLabels);
    attachWikiLinkHover(root, {
      read: async (path) => {
        seen.push(path);
        return `${path} 的内容\n`;
      },
      titleOf: (path) => (path === "/v/target.md" ? "目标篇" : "另一篇"),
      labels,
    });
    const [first, second] = Array.from(root.querySelectorAll<HTMLElement>("a.notebook-wikilink"));

    first!.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    expect(bodyText()).toContain("/v/target.md");

    second!.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    expect(bodyText()).toContain("/v/other.md");
    expect(seen).toEqual(["/v/target.md", "/v/other.md"]);
  });

  it("同一篇再看一次不重复取数", async () => {
    const read = vi.fn(async () => "缓存过的内容\n");
    const { link } = setup("<p>[[目标篇]]</p>", { read });
    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    link.dispatchEvent(new MouseEvent("mouseout", { bubbles: true }));
    await tick(HOVER_HIDE_DELAY + 10);

    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    // 卡片要真的重新露出来,不只是内容对 —— 缓存那一路也得走 openCard。
    expect(card()?.style.display).toBe("block");
    expect(bodyText()).toBe("缓存过的内容");
    expect(read).toHaveBeenCalledTimes(1);
  });

  it("鼠标从链接挪到卡片上不收卡(拿不到 relatedTarget 时靠卡片自己拦)", async () => {
    /* 合成事件里 relatedTarget 常常是 null,真实浏览器在某些路径下也一样。这时
       onOut 会照常排收起,靠卡片上的 mouseenter 把它取消掉。 */
    const { link } = setup("<p>[[目标篇]]</p>");
    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);

    link.dispatchEvent(new MouseEvent("mouseout", { bubbles: true }));
    card()!.dispatchEvent(new MouseEvent("mouseenter"));
    await tick(HOVER_HIDE_DELAY + 50);
    expect(card()?.style.display).toBe("block");
  });

  it("从卡片上移开之后收卡", async () => {
    const { link } = setup("<p>[[目标篇]]</p>");
    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    card()!.dispatchEvent(new MouseEvent("mouseenter"));
    card()!.dispatchEvent(new MouseEvent("mouseleave"));
    await tick(HOVER_HIDE_DELAY + 50);
    expect(card()?.style.display).toBe("none");
  });

  it("卡片是 tooltip,不抢焦点", async () => {
    const { link } = setup("<p>[[目标篇]]</p>");
    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    expect(card()?.getAttribute("role")).toBe("tooltip");
  });

  it("键盘快速跳过中间那条时不取它的内容", async () => {
    const seen: string[] = [];
    root.innerHTML = "<p>[[目标篇]] 和 [[另一篇]]</p>";
    enhanceWikiLinks(root, index, wikiLabels);
    attachWikiLinkHover(root, {
      read: async (path) => {
        seen.push(path);
        return `${path}\n`;
      },
      titleOf: () => undefined,
      labels,
    });
    const [first, second] = Array.from(root.querySelectorAll<HTMLElement>("a.notebook-wikilink"));

    // focusin 不带延迟,所以这里两条都会取 —— 钉的是"取的是各自那一篇,没串"。
    first!.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    second!.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    await tick(0);
    expect(seen).toEqual(["/v/target.md", "/v/other.md"]);
    expect(bodyText()).toBe("/v/other.md");
  });

  it("迟到的那一次取数不覆盖后发的那一次", async () => {
    /* 同一条链接上快速进出会发多次。慢的那次后回来时,卡片里已经是新的内容了 ——
       没有序号守卫的话它会把新的盖回旧的。 */
    const pending: Array<(value: string) => void> = [];
    const { link } = setup("<p>[[目标篇]]</p>", {
      read: () =>
        new Promise<string>((resolve) => {
          pending.push(resolve);
        }),
    });
    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    /* 移开并等收起真正落地,再回来 —— 这样第二次会重新取数,而且"用户已经移开"那条
       守卫此刻是不成立的(current 又是这一条了),留下的只有序号这一条。 */
    link.dispatchEvent(new MouseEvent("mouseout", { bubbles: true }));
    await tick(HOVER_HIDE_DELAY + 10);
    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    expect(pending).toHaveLength(2);

    pending[1]!("后发的内容\n");
    await tick(0);
    expect(bodyText()).toBe("后发的内容");

    pending[0]!("先发的内容\n");
    await tick(0);
    expect(bodyText()).toBe("后发的内容");
  });

  it("键盘走到链接上立刻出卡,不等延迟", async () => {
    /* 这些链接是 tabIndex=0 的。Markio 只听鼠标事件 —— 用键盘走到链接上什么都不会
       发生,预览这个功能对键盘用户等于不存在。延迟只用来过滤鼠标"路过",而 Tab 到
       某一条上是明确的选择。 */
    const { link } = setup("<p>[[目标篇]]</p>");
    link.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    await tick(0);
    expect(card()?.style.display).toBe("block");
    expect(bodyText()).toBe("被预览的正文");
  });

  it("焦点离开就收卡", async () => {
    const { link } = setup("<p>[[目标篇]]</p>");
    link.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    await tick(0);
    link.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    expect(card()?.style.display).toBe("none");
  });

  it("按 Esc 收卡", async () => {
    const { link } = setup("<p>[[目标篇]]</p>");
    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(card()?.style.display).toBe("none");
  });

  it("预览区内部滚动就收卡", async () => {
    /* 卡片是 position: fixed,滚动之后它会停在原地而链接已经走了。
       事件从后代上发出 —— scroll 不冒泡,所以这里同时钉住"监听必须是 capture 的":
       真实滚动发生在预览区内层的可滚动容器上,不是 root 本身。 */
    const { link } = setup("<p>[[目标篇]]</p>");
    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    link.parentElement!.dispatchEvent(new Event("scroll"));
    expect(card()?.style.display).toBe("none");
  });

  it("死链不弹卡", async () => {
    const read = vi.fn(async () => "x\n");
    root.innerHTML = "<p>[[还没写]]</p>";
    enhanceWikiLinks(root, index, wikiLabels);
    attachWikiLinkHover(root, { read, titleOf: () => undefined, labels });
    const link = root.querySelector<HTMLElement>("a.notebook-wikilink")!;
    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    expect(card()).toBeNull();
    expect(read).not.toHaveBeenCalled();
  });

  it("嵌入块的头部不弹卡", async () => {
    /* 那块内容就在头部下面摊开着,再弹一张卡挡住它没有意义。 */
    const read = vi.fn(async () => "x\n");
    root.innerHTML =
      '<a class="notebook-wikilink notebook-embed-head" data-wiki-path="/v/target.md">目标篇</a>';
    attachWikiLinkHover(root, { read, titleOf: () => "目标篇", labels });
    const head = root.querySelector<HTMLElement>("a")!;
    head.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    expect(card()).toBeNull();
    expect(read).not.toHaveBeenCalled();
  });

  it("嵌入内容里的普通链接照常弹卡", async () => {
    // 排除的只是头部那一条,不是整个嵌入块。
    root.innerHTML =
      '<span class="notebook-embed" data-embed-state="filled">' +
      '<a class="notebook-wikilink notebook-embed-head" data-wiki-path="/v/target.md">目标篇</a>' +
      '<span class="notebook-embed-body"><p>再看 [[另一篇]]</p></span></span>';
    enhanceWikiLinks(root, index, wikiLabels);
    attachWikiLinkHover(root, {
      read: async () => "另一篇的内容\n",
      titleOf: () => "另一篇",
      labels,
    });
    const inner = root.querySelector<HTMLElement>(".notebook-embed-body a.notebook-wikilink")!;
    inner.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    expect(bodyText()).toBe("另一篇的内容");
  });

  it("拿不到标题时头部显示文件名", async () => {
    const { link } = setup("<p>[[目标篇]]</p>", { titleOf: () => undefined });
    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    expect(document.querySelector(`.${HOVER_HEAD_CLASS}`)?.textContent).toBe("target.md");
  });

  it("卡片里的标题不带 id,不和宿主的锚点撞", async () => {
    const { link } = setup("<p>[[目标篇]]</p>", { read: async () => "# 同名标题\n正文\n" });
    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    const heading = document.querySelector(`.${HOVER_BODY_CLASS} h1`);
    expect(heading?.textContent).toBe("同名标题");
    expect(heading?.hasAttribute("id")).toBe(false);
  });

  it("disconnect 之后不再弹卡,也不留残留的卡片", async () => {
    const { link, handle } = setup("<p>[[目标篇]]</p>");
    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    expect(card()).not.toBeNull();

    handle.disconnect();
    // 卡片挂在 body 上,不摘会在切模式之后留一张浮在界面上。
    expect(card()).toBeNull();

    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    expect(card()).toBeNull();

    // 键盘那条路也要断掉,而且它更容易漏:不带延迟,一个 focusin 就现造一张新卡挂回 body。
    link.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    expect(card()).toBeNull();
  });

  it("disconnect 时排在路上的出卡定时器不会再造一张卡", async () => {
    /* 出卡走 openCard,而 openCard 会现造一张卡挂到 body 上 —— 定时器不清掉的话,
       面板已经切走了,几百毫秒后又浮出来一张,而且再也没人负责摘它。 */
    const { link, handle } = setup("<p>[[目标篇]]</p>");
    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY - 100);
    handle.disconnect();
    await tick(300);
    expect(card()).toBeNull();
  });

  it("鼠标挪到一条死链上,原来那张卡照常收起", async () => {
    /* 死链不弹卡,但也不该顺手把已经排上的收起取消掉 —— 那样上一篇的预览会一直
       挂在那里。 */
    root.innerHTML = "<p>[[目标篇]] 和 [[还没写]]</p>";
    enhanceWikiLinks(root, index, wikiLabels);
    attachWikiLinkHover(root, {
      read: async () => "目标篇的正文\n",
      titleOf: () => "目标篇",
      labels,
    });
    const [live, dead] = Array.from(root.querySelectorAll<HTMLElement>("a.notebook-wikilink"));

    live!.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    expect(card()?.style.display).toBe("block");

    live!.dispatchEvent(new MouseEvent("mouseout", { bubbles: true }));
    dead!.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_HIDE_DELAY + 50);
    expect(card()?.style.display).toBe("none");
  });

  it("卡片里是截断过的正文,不含 frontmatter", async () => {
    // 钉的是 fill 真的走了 previewSnippet,不是把整篇原文直接丢给 marked。
    const long = [
      "---",
      "title: 目标篇",
      "---",
      ...Array.from({ length: 60 }, (_, i) => `行 ${i}`),
    ];
    const { link } = setup("<p>[[目标篇]]</p>", { read: async () => `${long.join("\n")}\n` });
    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);

    const text = bodyText();
    expect(text).not.toContain("title:");
    expect(text).toContain("行 0");
    expect(text).not.toContain("行 59");
    expect(text).toContain("…");
  });

  it("disconnect 之后飞在路上的取数不再写 DOM", async () => {
    let release!: (value: string) => void;
    const { link, handle } = setup("<p>[[目标篇]]</p>", {
      read: () =>
        new Promise<string>((resolve) => {
          release = resolve;
        }),
    });
    link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await tick(HOVER_SHOW_DELAY);
    handle.disconnect();
    release("迟到的内容\n");
    await tick(10);
    expect(document.body.textContent).not.toContain("迟到的内容");
  });
});
