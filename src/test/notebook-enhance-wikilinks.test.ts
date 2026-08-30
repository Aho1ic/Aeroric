import { describe, expect, it } from "vitest";
import {
  WIKI_EMBED_CLASS,
  WIKI_LINK_CLASS,
  WIKI_LINK_MISSING_CLASS,
  enhanceWikiLinks,
  isWikiLinkClick,
  wikiLinkTargetFromEvent,
} from "../components/notebook/enhanceWikiLinks";
import { buildLinkIndex } from "../components/notebook/noteLinks";

const labels = {
  open: (title: string) => `打开 ${title}`,
  missing: (target: string) => `未找到笔记:${target}`,
  ambiguous: (title: string) => `多篇同名,打开 ${title}`,
};

function host(html: string): HTMLElement {
  const el = document.createElement("div");
  el.innerHTML = html;
  return el;
}

const index = buildLinkIndex([
  { path: "/v/foo.md", title: "Foo" },
  { path: "/v/bar.md", title: "条目二" },
]);

function links(el: HTMLElement): HTMLElement[] {
  return Array.from(el.querySelectorAll<HTMLElement>(`a.${WIKI_LINK_CLASS}`));
}

function embeds(el: HTMLElement): HTMLElement[] {
  return Array.from(el.querySelectorAll<HTMLElement>(`.${WIKI_EMBED_CLASS}`));
}

describe("enhanceWikiLinks", () => {
  it("把 [[目标]] 换成带路径的链接", () => {
    const el = host("<p>see [[Foo]] here</p>");
    enhanceWikiLinks(el, index, labels);

    const [link] = links(el);
    expect(link?.textContent).toBe("Foo");
    expect(link?.dataset.wikiPath).toBe("/v/foo.md");
    expect(link?.title).toBe("打开 Foo");
    // 前后的文本要留着,不能被整段吞掉。
    expect(el.textContent).toBe("see Foo here");
  });

  it("别名显示别名,目标仍指向笔记", () => {
    const el = host("<p>[[Foo|另一个说法]]</p>");
    enhanceWikiLinks(el, index, labels);
    const [link] = links(el);
    expect(link?.textContent).toBe("另一个说法");
    expect(link?.dataset.wikiPath).toBe("/v/foo.md");
  });

  it("小节进 dataset,供跳转后定位", () => {
    const el = host("<p>[[Foo#小节]]</p>");
    enhanceWikiLinks(el, index, labels);
    expect(links(el)[0]?.dataset.wikiHeading).toBe("小节");
  });

  it("解析不到的挂死链标记,并说明是没找到", () => {
    const el = host("<p>[[不存在]]</p>");
    enhanceWikiLinks(el, index, labels);
    const [link] = links(el);
    expect(link?.classList.contains(WIKI_LINK_MISSING_CLASS)).toBe(true);
    expect(link?.dataset.wikiPath).toBeUndefined();
    // 说"加载中"的话用户会等;说"没找到"他才会去检查自己写的名字。
    expect(link?.title).toBe("未找到笔记:不存在");
  });

  it("歧义在 title 上说出来", () => {
    const dup = buildLinkIndex([
      { path: "/v/a/dup.md", title: "甲" },
      { path: "/v/b/dup.md", title: "乙" },
    ]);
    const el = host("<p>[[dup]]</p>");
    enhanceWikiLinks(el, dup, labels);
    expect(links(el)[0]?.title).toBe("多篇同名,打开 甲");
  });

  it("代码块与行内代码里的方括号不动", () => {
    // `[[foo]]` 出现在代码示例里是内容,不是链接。这也是这一层走 DOM 而不是
    // 在源码上做正则替换的全部理由。
    const el = host("<pre><code>[[Foo]]</code></pre><p>x <code>[[Foo]]</code> y</p>");
    enhanceWikiLinks(el, index, labels);
    expect(links(el)).toHaveLength(0);
    expect(el.textContent).toContain("[[Foo]]");
  });

  it("已有链接内部不再套一层", () => {
    // 嵌套 <a> 是非法 HTML,点击行为不确定。
    const el = host('<p><a href="#x">[[Foo]]</a></p>');
    enhanceWikiLinks(el, index, labels);
    expect(links(el)).toHaveLength(0);
  });

  it("数学与 Mermaid 占位块不动", () => {
    // 里面放的是原始 TeX / 图源码,改了会让渲染失败。
    const el = host(
      '<span class="notebook-math">[[Foo]]</span><div class="notebook-mermaid">[[Foo]]</div>',
    );
    enhanceWikiLinks(el, index, labels);
    expect(links(el)).toHaveLength(0);
  });

  it("一行里多条链接各自成链", () => {
    const el = host("<p>[[Foo]] 和 [[条目二]] 都在</p>");
    enhanceWikiLinks(el, index, labels);
    expect(links(el).map((l) => l.dataset.wikiPath)).toEqual(["/v/foo.md", "/v/bar.md"]);
    expect(el.textContent).toBe("Foo 和 条目二 都在");
  });

  it("拆不出目标的原样留作文本", () => {
    const el = host("<p>[[]] 和 [[|只有别名]]</p>");
    enhanceWikiLinks(el, index, labels);
    expect(links(el)).toHaveLength(0);
    expect(el.textContent).toBe("[[]] 和 [[|只有别名]]");
  });

  it("再跑一次不会套两层,只刷新解析状态", () => {
    const el = host("<p>[[新笔记]]</p>");
    enhanceWikiLinks(el, index, labels);
    expect(links(el)[0]?.classList.contains(WIKI_LINK_MISSING_CLASS)).toBe(true);

    // 用户新建了这篇笔记之后再跑一次:死链要变活,而不是产出嵌套链接。
    const grown = buildLinkIndex([{ path: "/v/new.md", title: "新笔记" }]);
    enhanceWikiLinks(el, grown, labels);
    expect(links(el)).toHaveLength(1);
    expect(links(el)[0]?.classList.contains(WIKI_LINK_MISSING_CLASS)).toBe(false);
    expect(links(el)[0]?.dataset.wikiPath).toBe("/v/new.md");
  });

  it("没有链接的 DOM 保持原样(不打断文本选择)", () => {
    const el = host("<p>纯文本</p>");
    const before = el.firstElementChild?.firstChild;
    enhanceWikiLinks(el, index, labels);
    // 同一个文本节点对象还在 —— 无谓的 replaceChild 会清掉用户当前的选区。
    expect(el.firstElementChild?.firstChild).toBe(before);
  });

  it("链接键盘可达", () => {
    const el = host("<p>[[Foo]]</p>");
    enhanceWikiLinks(el, index, labels);
    const [link] = links(el);
    expect(link?.tabIndex).toBe(0);
    expect(link?.getAttribute("role")).toBe("link");
    // 不带 href:这是单页视图,`href="#"` 会改地址栏。
    expect(link?.hasAttribute("href")).toBe(false);
  });
});

describe("enhanceWikiLinks 的 ![[嵌入]] 占位", () => {
  it("造占位而不是链接,并把前面那个 ! 摘掉", () => {
    const el = host("<p>看这个 ![[Foo]] 就懂了</p>");
    enhanceWikiLinks(el, index, labels);

    expect(links(el)).toHaveLength(0);
    const [embed] = embeds(el);
    expect(embed?.dataset.embedPath).toBe("/v/foo.md");
    expect(embed?.dataset.embedTarget).toBe("Foo");
    // `!` 不能留在正文里 —— 留着渲染出来是一个孤零零的叹号加一块嵌入内容。
    expect(el.textContent).toBe("看这个 ![[Foo]] 就懂了");
    expect(embed?.textContent).toBe("![[Foo]]");
  });

  it("填充前显示原始语法(取数是异步的,这段时间不能是空白)", () => {
    const el = host("<p>![[Foo]]</p>");
    enhanceWikiLinks(el, index, labels);
    expect(embeds(el)[0]?.textContent).toBe("![[Foo]]");
    // 解析成功也不换成标题:那段文字同时是填充失败时的降级显示。
    expect(embeds(el)[0]?.textContent).not.toBe("Foo");
  });

  it("小节进 dataset", () => {
    const el = host("<p>![[Foo#小节]]</p>");
    enhanceWikiLinks(el, index, labels);
    const [embed] = embeds(el);
    expect(embed?.dataset.embedHeading).toBe("小节");
    expect(embed?.dataset.embedRaw).toBe("Foo#小节");
  });

  it("解析不到的挂死链标记并说明没找到", () => {
    const el = host("<p>![[不存在]]</p>");
    enhanceWikiLinks(el, index, labels);
    const [embed] = embeds(el);
    expect(embed?.classList.contains(WIKI_LINK_MISSING_CLASS)).toBe(true);
    expect(embed?.dataset.embedPath).toBeUndefined();
    expect(embed?.title).toBe("未找到笔记:不存在");
  });

  it("同一段里连着两条嵌入各自成占位", () => {
    // 第二条的 `!` 判定只能看 match.index,不能拿"上一条结束的位置"代替。
    const el = host("<p>![[Foo]]![[条目二]]</p>");
    enhanceWikiLinks(el, index, labels);
    expect(embeds(el).map((e) => e.dataset.embedPath)).toEqual(["/v/foo.md", "/v/bar.md"]);
    expect(el.textContent).toBe("![[Foo]]![[条目二]]");
  });

  it("嵌入和普通链接混在一行时互不影响", () => {
    const el = host("<p>![[Foo]] 见 [[条目二]]</p>");
    enhanceWikiLinks(el, index, labels);
    expect(embeds(el)).toHaveLength(1);
    expect(links(el).map((l) => l.dataset.wikiPath)).toEqual(["/v/bar.md"]);
  });

  it("再跑一次不会把占位里的原始语法变成链接", () => {
    // 这是真会发生的:linkIndex 一变(新建笔记)这一层就重跑,而占位可能还没填上。
    const el = host("<p>![[Foo]]</p>");
    enhanceWikiLinks(el, index, labels);
    enhanceWikiLinks(el, index, labels);
    expect(embeds(el)).toHaveLength(1);
    expect(links(el)).toHaveLength(0);
    expect(embeds(el)[0]?.textContent).toBe("![[Foo]]");
  });

  it("没填好的占位会跟着索引变化从死链变活", () => {
    const el = host("<p>![[新笔记]]</p>");
    enhanceWikiLinks(el, index, labels);
    expect(embeds(el)[0]?.dataset.embedPath).toBeUndefined();

    const grown = buildLinkIndex([{ path: "/v/new.md", title: "新笔记" }]);
    enhanceWikiLinks(el, grown, labels);
    expect(embeds(el)[0]?.dataset.embedPath).toBe("/v/new.md");
    expect(embeds(el)[0]?.classList.contains(WIKI_LINK_MISSING_CLASS)).toBe(false);
  });

  it("已填好的占位不再被重算,内部链接照常增强", () => {
    // 填好之后 data-embed-state=filled,于是它退出跳过名单 —— 嵌入内容里的
    // [[link]] 才能被处理,而占位本身的 title 不该被宿主口径覆盖。
    const el = host(
      `<p><span class="${WIKI_EMBED_CLASS}" data-embed-state="filled"` +
        ` data-embed-target="Foo" title="别动我">正文里有 [[条目二]]</span></p>`,
    );
    enhanceWikiLinks(el, index, labels);
    expect(links(el).map((l) => l.dataset.wikiPath)).toEqual(["/v/bar.md"]);
    expect(embeds(el)[0]?.title).toBe("别动我");
  });

  it("目标被删掉之后占位要摘掉旧路径,而不是留着指向已经没了的文件", () => {
    /* 留着的话下一轮重试会拿这个路径去读一个不存在的文件 —— 用户看到的是"嵌入失败:
       没有这个文件",而实际情况是"这篇笔记被删了"。后者该显示成死链。 */
    const el = host("<p>![[Foo]]</p>");
    enhanceWikiLinks(el, index, labels);
    expect(embeds(el)[0]?.dataset.embedPath).toBe("/v/foo.md");

    const shrunk = buildLinkIndex([{ path: "/v/bar.md", title: "条目二" }]);
    enhanceWikiLinks(el, shrunk, labels);
    expect(embeds(el)[0]?.dataset.embedPath).toBeUndefined();
    expect(embeds(el)[0]?.classList.contains(WIKI_LINK_MISSING_CLASS)).toBe(true);
  });

  it("代码块里的 ![[..]] 不造占位", () => {
    const el = host("<pre><code>![[Foo]]</code></pre>");
    enhanceWikiLinks(el, index, labels);
    expect(embeds(el)).toHaveLength(0);
  });
});

describe("wikiLinkTargetFromEvent", () => {
  it("从链接内部的元素也能找到目标", () => {
    const el = host("<p>[[Foo]]</p>");
    enhanceWikiLinks(el, index, labels);
    const link = links(el)[0]!;
    // 别名带 markdown 强调时,点击落在 <em> 上而不是 <a> 上。
    const inner = document.createElement("em");
    inner.textContent = "x";
    link.append(inner);

    const event = new MouseEvent("click", { bubbles: true });
    inner.dispatchEvent(event);
    // 事件已经派发完,用一个手工构造的对象复现 target 关系。
    expect(wikiLinkTargetFromEvent({ target: inner } as unknown as Event)).toEqual({
      path: "/v/foo.md",
    });
  });

  it("带小节时一并返回", () => {
    const el = host("<p>[[Foo#节]]</p>");
    enhanceWikiLinks(el, index, labels);
    expect(wikiLinkTargetFromEvent({ target: links(el)[0] } as unknown as Event)).toEqual({
      path: "/v/foo.md",
      heading: "节",
    });
  });

  it("死链没有可跳的目标,但仍算一次 wikilink 点击", () => {
    const el = host("<p>[[缺]]</p>");
    enhanceWikiLinks(el, index, labels);
    const link = links(el)[0]!;
    // 两个函数分工:一个决定跳哪,一个决定要不要 preventDefault。死链要拦但不跳。
    expect(wikiLinkTargetFromEvent({ target: link } as unknown as Event)).toBeNull();
    expect(isWikiLinkClick({ target: link } as unknown as Event)).toBe(true);
  });

  it("与 wikilink 无关的点击不拦", () => {
    const el = host("<p>普通段落</p>");
    const p = el.firstElementChild!;
    expect(wikiLinkTargetFromEvent({ target: p } as unknown as Event)).toBeNull();
    expect(isWikiLinkClick({ target: p } as unknown as Event)).toBe(false);
    // target 不是元素(document / window)时也不能炸。
    expect(isWikiLinkClick({ target: null } as unknown as Event)).toBe(false);
  });
});
