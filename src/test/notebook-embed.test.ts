import { describe, expect, it, vi } from "vitest";
import {
  EMBED_BODY_CLASS,
  EMBED_HEAD_CLASS,
  MAX_EMBED_DEPTH,
  enhanceNoteEmbeds,
  extractHeadingSection,
} from "../components/notebook/noteEmbed";
import {
  WIKI_EMBED_CLASS,
  WIKI_LINK_CLASS,
  WIKI_LINK_MISSING_CLASS,
  enhanceWikiLinks,
} from "../components/notebook/enhanceWikiLinks";
import { buildLinkIndex } from "../components/notebook/noteLinks";

const labels = {
  open: (title: string) => `打开 ${title}`,
  missing: (target: string) => `未找到笔记:${target}`,
  ambiguous: (title: string) => `多篇同名,打开 ${title}`,
  missingHeading: (heading: string) => `没有标题 ${heading}`,
  tooDeep: (target: string) => `层级过深或成环:${target}`,
  failed: (target: string, message: string) => `嵌入失败 ${target}:${message}`,
};

/** 一个内存 vault:路径 → 文件内容。 */
type Files = Record<string, string>;

function indexOf(files: Files) {
  return buildLinkIndex(
    Object.keys(files).map((path) => ({
      path,
      // 标题取 frontmatter 里的 title,没有就用 stem —— 和面板同一口径。
      title: /^---\n(?:.*\n)*?title: (.*)\n/.exec(files[path]!)?.[1] ?? "",
    })),
  );
}

/**
 * 造宿主 DOM 并跑完整两步(先增强造占位,再填充)。
 *
 * 用 `enhanceWikiLinks` 造占位而不是手写 span:占位的属性约定是两个模块之间的
 * 接口,手写等于把接口抄第二遍,改一边测试还是绿的。
 */
async function fill(
  source: string,
  files: Files,
  options: { hostPath?: string; read?: (path: string) => Promise<string> } = {},
) {
  const index = indexOf(files);
  const host = document.createElement("div");
  host.innerHTML = source;
  enhanceWikiLinks(host, index, labels);
  const read = options.read ?? (async (path: string) => files[path] ?? "");
  const handle = enhanceNoteEmbeds(host, {
    hostPath: options.hostPath ?? "/v/host.md",
    read,
    index,
    labels,
  });
  // 填充是异步且递归的:每一层都要 await 一轮微任务。多跑几轮把整棵树跑完。
  for (let round = 0; round < 8; round += 1) await Promise.resolve();
  return { host, handle, index };
}

function embeds(el: HTMLElement): HTMLElement[] {
  return Array.from(el.querySelectorAll<HTMLElement>(`.${WIKI_EMBED_CLASS}`));
}

function bodyOf(el: HTMLElement): HTMLElement | null {
  return el.querySelector<HTMLElement>(`.${EMBED_BODY_CLASS}`);
}

describe("extractHeadingSection", () => {
  const source = "# 一\n正文一\n\n## 一之一\n子正文\n\n# 二\n正文二\n";

  it("截到下一个同级或更高级标题为止,子标题算进来", () => {
    expect(extractHeadingSection(source, "一")).toBe("# 一\n正文一\n\n## 一之一\n子正文\n\n");
  });

  it("子标题只截自己那一段", () => {
    expect(extractHeadingSection(source, "一之一")).toBe("## 一之一\n子正文\n\n");
  });

  it("最后一个标题截到文末", () => {
    expect(extractHeadingSection(source, "二")).toBe("# 二\n正文二\n");
  });

  it("匹配折大小写、忽略首尾空白", () => {
    expect(extractHeadingSection("# Alpha\nx\n", "  alpha ")).toBe("# Alpha\nx\n");
  });

  it("找不到返回 null(而不是空串)", () => {
    // 调用方要能区分"这个小节不存在"(该提示)和"这个小节是空的"(该显示成空)。
    expect(extractHeadingSection(source, "三")).toBeNull();
    expect(extractHeadingSection(source, "")).toBeNull();
    expect(extractHeadingSection("# 空\n", "空")).toBe("# 空\n");
  });

  it("围栏里的 # 不当标题", () => {
    const fenced = "# 真标题\n```\n# 假标题\n```\n后文\n";
    expect(extractHeadingSection(fenced, "假标题")).toBeNull();
    expect(extractHeadingSection(fenced, "真标题")).toBe(fenced);
  });

  it("frontmatter 里的 # 不当标题", () => {
    const withFm = "---\ntags: [a]\n---\n# 正文标题\nx\n";
    expect(extractHeadingSection(withFm, "正文标题")).toBe("# 正文标题\nx\n");
  });
});

describe("enhanceNoteEmbeds", () => {
  it("把 ![[note]] 填成目标笔记的渲染内容", async () => {
    const { host } = await fill("<p>![[target]]</p>", {
      "/v/target.md": "---\ntitle: 目标篇\n---\n# 标题\n\n正文一段\n",
    });
    const [embed] = embeds(host);
    expect(embed?.dataset.embedState).toBe("filled");
    expect(bodyOf(embed!)?.querySelector("h1")?.textContent).toBe("标题");
    expect(bodyOf(embed!)?.textContent).toContain("正文一段");
  });

  it("整篇嵌入去掉 frontmatter", async () => {
    // 留着的话 `---` 会渲染成一条分隔线加一行 `title: x` 文本 —— 看着像内容其实是元数据。
    const { host } = await fill("<p>![[target]]</p>", {
      "/v/target.md": "---\ntitle: 目标篇\ntags: [x]\n---\n正文\n",
    });
    const body = bodyOf(embeds(host)[0]!);
    // trim 掉尾部换行:marked 在块级元素后面留一个 `\n`,它是渲染产物不是内容。
    expect(body?.textContent?.trim()).toBe("正文");
    expect(body?.querySelector("hr")).toBeNull();
  });

  it("头部显示真标题并且是一条可点的 wikilink", async () => {
    const { host } = await fill("<p>![[target]]</p>", {
      "/v/target.md": "---\ntitle: 目标篇\n---\n正文\n",
    });
    const head = host.querySelector<HTMLElement>(`a.${EMBED_HEAD_CLASS}`);
    // 标题而不是文件名:改过标题的笔记显示 `target.md` 会让人以为嵌错了。
    expect(head?.textContent).toBe("目标篇");
    // 类名和 dataset 按 enhanceWikiLinks 的约定挂 —— 面板那个点击监听才能原样复用。
    expect(head?.classList.contains(WIKI_LINK_CLASS)).toBe(true);
    expect(head?.dataset.wikiPath).toBe("/v/target.md");
  });

  it("没有 frontmatter 标题时头部退回文件名", async () => {
    const { host } = await fill("<p>![[bare]]</p>", { "/v/bare.md": "正文\n" });
    expect(host.querySelector(`a.${EMBED_HEAD_CLASS}`)?.textContent).toBe("bare.md");
  });

  it("带小节时只嵌那一段,头部标出小节名", async () => {
    const { host } = await fill("<p>![[target#二]]</p>", {
      "/v/target.md": "---\ntitle: 目标篇\n---\n# 一\n甲\n\n# 二\n乙\n",
    });
    const body = bodyOf(embeds(host)[0]!);
    expect(body?.textContent).toContain("乙");
    expect(body?.textContent).not.toContain("甲");
    expect(host.querySelector(`a.${EMBED_HEAD_CLASS}`)?.textContent).toBe("目标篇 › 二");
  });

  it("小节不存在时留下原始语法并说明是哪个标题没找到", async () => {
    const { host } = await fill("<p>![[target#没有这节]]</p>", {
      "/v/target.md": "# 一\n甲\n",
    });
    const [embed] = embeds(host);
    expect(embed?.dataset.embedState).toBe("error");
    expect(embed?.textContent).toBe("![[target#没有这节]]");
    expect(embed?.title).toBe("没有标题 没有这节");
    expect(embed?.classList.contains(WIKI_LINK_MISSING_CLASS)).toBe(true);
  });

  it("取数失败时留下原始语法并带上原因", async () => {
    const { host } = await fill(
      "<p>![[target]]</p>",
      {
        "/v/target.md": "正文\n",
      },
      {
        read: async () => {
          throw new Error("读盘失败");
        },
      },
    );
    const [embed] = embeds(host);
    expect(embed?.dataset.embedState).toBe("error");
    expect(embed?.textContent).toBe("![[target]]");
    expect(embed?.title).toBe("嵌入失败 target:读盘失败");
  });

  it("解析不到目标时不取数", async () => {
    const read = vi.fn(async () => "不该被读到\n");
    const { host } = await fill("<p>![[不存在]]</p>", { "/v/target.md": "x\n" }, { read });
    expect(read).not.toHaveBeenCalled();
    // 死链的样式和 title 已经由 enhanceWikiLinks 挂好,这一层只补状态防止反复认领。
    expect(embeds(host)[0]?.dataset.embedState).toBe("error");
    expect(embeds(host)[0]?.title).toBe("未找到笔记:不存在");
  });

  it("嵌入内容里的 [[link]] 照常可点", async () => {
    const { host } = await fill("<p>![[target]]</p>", {
      "/v/target.md": "看 [[other]]\n",
      "/v/other.md": "---\ntitle: 另一篇\n---\nx\n",
    });
    const inner = bodyOf(embeds(host)[0]!)?.querySelector<HTMLElement>(`a.${WIKI_LINK_CLASS}`);
    expect(inner?.dataset.wikiPath).toBe("/v/other.md");
  });

  it("嵌套的 ![[..]] 继续往下填", async () => {
    const { host } = await fill("<p>![[a]]</p>", {
      "/v/a.md": "甲\n\n![[b]]\n",
      "/v/b.md": "乙\n",
    });
    const all = embeds(host);
    expect(all).toHaveLength(2);
    expect(all.every((el) => el.dataset.embedState === "filled")).toBe(true);
    expect(host.textContent).toContain("乙");
  });

  it("自嵌在第一层就被拦住", async () => {
    // Markio 的 ancestors 在 depth 0 是空集,根笔记自己的路径从没进去过 ——
    // `A` 里写 `![[A]]` 第一层不被拦,只靠深度上限兜底,用户看到自己那篇套三层。
    const { host } = await fill("<p>![[host]]</p>", { "/v/host.md": "我自己\n" });
    const [embed] = embeds(host);
    expect(embed?.dataset.embedState).toBe("error");
    expect(embed?.title).toBe("层级过深或成环:host");
    expect(embed?.textContent).toBe("![[host]]");
  });

  it("A 嵌 B、B 嵌 A 时里层被拦,外层照常填", async () => {
    const { host } = await fill("<p>![[a]]</p>", {
      "/v/a.md": "甲\n\n![[b]]\n",
      "/v/b.md": "乙\n\n![[a]]\n",
    });
    const [outer, mid, inner] = embeds(host);
    expect(outer?.dataset.embedState).toBe("filled");
    expect(mid?.dataset.embedState).toBe("filled");
    expect(inner?.dataset.embedState).toBe("error");
    expect(inner?.title).toBe("层级过深或成环:a");
  });

  it("没有环但很深的引用链到上限就停", async () => {
    // a→b→c→d:第 0/1/2 层填,第 3 层拒绝。
    const { host } = await fill("<p>![[a]]</p>", {
      "/v/a.md": "1\n\n![[b]]\n",
      "/v/b.md": "2\n\n![[c]]\n",
      "/v/c.md": "3\n\n![[d]]\n",
      "/v/d.md": "4\n",
    });
    const states = embeds(host).map((el) => el.dataset.embedState);
    expect(states).toEqual(["filled", "filled", "filled", "error"]);
    expect(MAX_EMBED_DEPTH).toBe(3);
    expect(host.textContent).toContain("3");
    expect(host.textContent).not.toContain("4");
  });

  it("剥掉嵌入内容里的 heading id,不和宿主的锚点撞", async () => {
    /* renderNoteMarkdown 每次调用新建一份 slug registry,所以宿主和嵌入会算出同一个
       id;而大纲跳转用 querySelector 取文档序第一个,撞上之后点宿主的大纲会跳进
       嵌入块里。 */
    const { host } = await fill('<h1 id="标题">宿主标题</h1><p>![[target]]</p>', {
      "/v/target.md": "# 标题\n正文\n",
    });
    const inner = bodyOf(embeds(host)[0]!)?.querySelector("h1");
    expect(inner?.textContent).toBe("标题");
    expect(inner?.hasAttribute("id")).toBe(false);
    // 宿主自己那个 id 不受影响。
    expect(host.querySelectorAll('[id="标题"]')).toHaveLength(1);
  });

  it("同一篇被嵌两次只读一次", async () => {
    const files = { "/v/target.md": "正文\n" };
    const read = vi.fn(async (path: string) => files[path as keyof typeof files] ?? "");
    const { host } = await fill("<p>![[target]] 又 ![[target]]</p>", files, { read });
    expect(embeds(host)).toHaveLength(2);
    expect(embeds(host).every((el) => el.dataset.embedState === "filled")).toBe(true);
    // 缓存存的是 Promise,所以"同时发起"的第二次也命中。
    expect(read).toHaveBeenCalledTimes(1);
  });

  it("同一个占位不会被认领两次", async () => {
    const files = { "/v/target.md": "正文\n" };
    const read = vi.fn(async (path: string) => files[path as keyof typeof files] ?? "");
    const index = indexOf(files);
    const host = document.createElement("div");
    host.innerHTML = "<p>![[target]]</p>";
    enhanceWikiLinks(host, index, labels);
    const opts = { hostPath: "/v/host.md", read, index, labels };
    // 依赖连着变化会让面板那个 effect 连跑两轮。两轮各发一次请求的话,两次填充
    // 会互相覆盖,后到的那次还可能来自旧索引。
    enhanceNoteEmbeds(host, opts);
    enhanceNoteEmbeds(host, opts);
    for (let round = 0; round < 8; round += 1) await Promise.resolve();
    expect(read).toHaveBeenCalledTimes(1);
    expect(host.querySelectorAll(`.${EMBED_BODY_CLASS}`)).toHaveLength(1);
  });

  it("disconnect 之后不再写 DOM", async () => {
    // `!` 而不是 `| null`:TS 不跟踪回调里的赋值,可选调用会把类型收窄成 never。
    let release!: (value: string) => void;
    const index = indexOf({ "/v/target.md": "正文\n" });
    const host = document.createElement("div");
    host.innerHTML = "<p>![[target]]</p>";
    enhanceWikiLinks(host, index, labels);
    const handle = enhanceNoteEmbeds(host, {
      hostPath: "/v/host.md",
      read: () =>
        new Promise<string>((resolve) => {
          release = resolve;
        }),
      index,
      labels,
    });

    handle.disconnect();
    release("正文\n");
    for (let round = 0; round < 8; round += 1) await Promise.resolve();
    expect(bodyOf(embeds(host)[0]!)).toBeNull();
  });

  it("disconnect 之后连失败提示都不写", async () => {
    /* 取数返回之后还有两条会碰 DOM 的路径:小节找不到、渲染抛。只验成功路径的话,
       "取数后查一次取消"这一步删掉测试照样绿 —— 因为渲染后面还有第二次检查兜着
       成功那一路。这一条走的是"小节找不到"。 */
    let release!: (value: string) => void;
    const index = indexOf({ "/v/target.md": "# 别的\n" });
    const host = document.createElement("div");
    host.innerHTML = "<p>![[target#没有这节]]</p>";
    enhanceWikiLinks(host, index, labels);
    const handle = enhanceNoteEmbeds(host, {
      hostPath: "/v/host.md",
      read: () =>
        new Promise<string>((resolve) => {
          release = resolve;
        }),
      index,
      labels,
    });

    handle.disconnect();
    release("# 别的\n");
    for (let round = 0; round < 8; round += 1) await Promise.resolve();
    // disconnect 已经把状态摘回 undefined,这之后任何写入都是越界的。
    expect(embeds(host)[0]?.dataset.embedState).toBeUndefined();
    expect(embeds(host)[0]?.title).not.toBe(labels.missingHeading("没有这节"));
  });

  it("disconnect 把停在 pending 的占位放回去,下一轮能重来", async () => {
    /* 留着 pending 的话它既不显示内容、又不会被下一轮认领 —— 用户看到一段永远停在
       原始语法上的文字。而这种情况很常见:linkIndex 一变 effect 就重跑,上一轮的
       请求正好还在飞。 */
    const files = { "/v/target.md": "正文\n" };
    const index = indexOf(files);
    const host = document.createElement("div");
    host.innerHTML = "<p>![[target]]</p>";
    enhanceWikiLinks(host, index, labels);
    const first = enhanceNoteEmbeds(host, {
      hostPath: "/v/host.md",
      read: () => new Promise<string>(() => {}),
      index,
      labels,
    });
    expect(embeds(host)[0]?.dataset.embedState).toBe("pending");
    first.disconnect();
    expect(embeds(host)[0]?.dataset.embedState).toBeUndefined();

    enhanceNoteEmbeds(host, {
      hostPath: "/v/host.md",
      read: async (path: string) => files[path as keyof typeof files] ?? "",
      index,
      labels,
    });
    for (let round = 0; round < 8; round += 1) await Promise.resolve();
    expect(bodyOf(embeds(host)[0]!)?.textContent?.trim()).toBe("正文");
  });

  it("上一轮失败的占位会在新索引下重试", async () => {
    const host = document.createElement("div");
    host.innerHTML = "<p>![[新笔记]]</p>";
    const empty = buildLinkIndex([]);
    enhanceWikiLinks(host, empty, labels);
    enhanceNoteEmbeds(host, {
      hostPath: "/v/host.md",
      read: async () => "",
      index: empty,
      labels,
    });
    for (let round = 0; round < 4; round += 1) await Promise.resolve();
    expect(embeds(host)[0]?.dataset.embedState).toBe("error");

    // 用户新建了那篇笔记:enhanceWikiLinks 把路径补上,这一层要跟着再试一次。
    const files = { "/v/new.md": "---\ntitle: 新笔记\n---\n有了\n" };
    const grown = indexOf(files);
    enhanceWikiLinks(host, grown, labels);
    enhanceNoteEmbeds(host, {
      hostPath: "/v/host.md",
      read: async (path: string) => files[path as keyof typeof files] ?? "",
      index: grown,
      labels,
    });
    for (let round = 0; round < 8; round += 1) await Promise.resolve();
    expect(embeds(host)[0]?.dataset.embedState).toBe("filled");
    expect(bodyOf(embeds(host)[0]!)?.textContent?.trim()).toBe("有了");
  });

  it("已填好的占位不会被重填", async () => {
    const files = { "/v/target.md": "正文\n" };
    const read = vi.fn(async (path: string) => files[path as keyof typeof files] ?? "");
    const { host, index } = await fill("<p>![[target]]</p>", files, { read });
    enhanceNoteEmbeds(host, { hostPath: "/v/host.md", read, index, labels });
    for (let round = 0; round < 8; round += 1) await Promise.resolve();
    // 重填会闪一下,而且内容并没有变。
    expect(read).toHaveBeenCalledTimes(1);
  });

  it("onFilled 每块内容调一次,返回的清理由 disconnect 收回", async () => {
    const cleanup = vi.fn();
    const filled: string[] = [];
    const files = { "/v/a.md": "甲\n\n![[b]]\n", "/v/b.md": "乙\n" };
    const index = indexOf(files);
    const host = document.createElement("div");
    host.innerHTML = "<p>![[a]]</p>";
    enhanceWikiLinks(host, index, labels);
    const handle = enhanceNoteEmbeds(host, {
      hostPath: "/v/host.md",
      read: async (path: string) => files[path as keyof typeof files] ?? "",
      index,
      labels,
      onFilled: (body) => {
        filled.push(body.textContent ?? "");
        return cleanup;
      },
    });
    for (let round = 0; round < 8; round += 1) await Promise.resolve();
    expect(filled).toHaveLength(2);
    // 嵌套那块也要过 onFilled,否则嵌入里的公式和图永远不渲染。
    expect(filled.some((text) => text.includes("乙"))).toBe(true);
    expect(cleanup).not.toHaveBeenCalled();
    handle.disconnect();
    expect(cleanup).toHaveBeenCalledTimes(2);
  });
});
