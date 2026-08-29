import { describe, expect, it } from "vitest";
import {
  buildLinkIndex,
  normalizeLinkTarget,
  linkTitleOf,
  outgoingLinks,
  parseWikiLinkBody,
  resolveLink,
  scanWikiLinks,
  type LinkableNote,
} from "../components/notebook/noteLinks";

/** 造一条笔记。path 决定 stem,title 是 frontmatter 里那个。 */
function note(path: string, title: string): LinkableNote {
  return { path, title };
}

describe("normalizeLinkTarget", () => {
  it("去掉 .md、首尾斜杠,并统一小写与分隔符", () => {
    expect(normalizeLinkTarget("  Foo.md  ")).toBe("foo");
    expect(normalizeLinkTarget("notes\\Sub\\Foo")).toBe("notes/sub/foo");
    expect(normalizeLinkTarget("/notes/foo/")).toBe("notes/foo");
    expect(normalizeLinkTarget("FOO.MD")).toBe("foo");
  });

  it("只剥结尾那个 .md", () => {
    // `a.md.md` 是个真实存在的手滑,剥两次会让它指向 `a`。
    expect(normalizeLinkTarget("a.md.md")).toBe("a.md");
    // 中间的 .md 不能动。
    expect(normalizeLinkTarget("a.md.txt")).toBe("a.md.txt");
  });

  it("解百分号编码,但孤立的百分号不能让它炸", () => {
    expect(normalizeLinkTarget("my%20note")).toBe("my note");
    // `50%完成` 里的 `%完` 不是合法编码,decodeURIComponent 会抛。
    expect(normalizeLinkTarget("50%完成")).toBe("50%完成");
  });
});

describe("parseWikiLinkBody", () => {
  it("拆出目标、小节与别名", () => {
    expect(parseWikiLinkBody("Foo")).toEqual({ target: "Foo", display: "Foo" });
    expect(parseWikiLinkBody("Foo#Bar")).toEqual({
      target: "Foo",
      display: "Foo#Bar",
      heading: "Bar",
    });
    expect(parseWikiLinkBody("Foo|别名")).toEqual({ target: "Foo", display: "别名" });
    expect(parseWikiLinkBody("Foo#Bar|别名")).toEqual({
      target: "Foo",
      display: "别名",
      heading: "Bar",
    });
  });

  it("先切别名再切小节 —— 别名里的 # 不算小节", () => {
    // 反过来切的话 `a|b#c` 的小节会变成 `c`,而 `c` 是别名的一部分。
    expect(parseWikiLinkBody("a|b#c")).toEqual({ target: "a", display: "b#c" });
  });

  it("没有目标就不是一条链接", () => {
    // 渲染成死链不如留作普通文本。
    expect(parseWikiLinkBody("")).toBeNull();
    expect(parseWikiLinkBody("   ")).toBeNull();
    expect(parseWikiLinkBody("#只有小节")).toBeNull();
    expect(parseWikiLinkBody("|只有别名")).toBeNull();
  });

  it("空别名退回目标,不产出空白链接", () => {
    // `[[Foo|]]` 显示成空的话页面上是一个点不到的零宽链接。
    expect(parseWikiLinkBody("Foo|")).toEqual({ target: "Foo", display: "Foo" });
    expect(parseWikiLinkBody("Foo|  ")).toEqual({ target: "Foo", display: "Foo" });
  });
});

describe("resolveLink", () => {
  it("stem 优先于标题", () => {
    // `foo.md` 的标题被改成了别的,另一篇笔记的标题正好叫 foo。
    // 用户写 `[[foo]]` 要的是那个文件。
    const index = buildLinkIndex([note("/v/foo.md", "改过的标题"), note("/v/other.md", "foo")]);
    const hit = resolveLink(index, "foo");
    expect(hit?.note.path).toBe("/v/foo.md");
    expect(hit?.via).toBe("stem");
  });

  it("stem 撞不上时按 frontmatter 标题解析", () => {
    // 这是 Aeroric 与 Markio 的实质差异:文件名新建时定死,标题随后改了,
    // 只认 stem 的话用户写新标题会解析不到自己那篇。
    const index = buildLinkIndex([note("/v/cao-gao.md", "周报")]);
    const hit = resolveLink(index, "周报");
    expect(hit?.note.path).toBe("/v/cao-gao.md");
    expect(hit?.via).toBe("title");
  });

  it("同键多篇时报歧义,而不是静默取第一篇", () => {
    const index = buildLinkIndex([note("/v/a/foo.md", "A"), note("/v/b/foo.md", "B")]);
    const hit = resolveLink(index, "foo");
    expect(hit?.ambiguous).toBe(true);
    // 入参顺序即优先级 —— 列表按修改时间排,于是命中"更近的那篇"。
    expect(hit?.note.path).toBe("/v/a/foo.md");
  });

  it("同一篇笔记的 stem 与标题相同时不算歧义", () => {
    // 标题没改过的笔记(绝大多数)两张表都会收它。自己把自己判成歧义的话,
    // 面板上每条链接都会挂一个歧义提示。
    const index = buildLinkIndex([note("/v/foo.md", "foo")]);
    expect(resolveLink(index, "foo")?.ambiguous).toBe(false);
  });

  it("带斜杠的目标只走路径,不去撞 stem 和标题", () => {
    // 标题里带斜杠的笔记不该被 `[[notes/foo]]` 命中。
    const index = buildLinkIndex([note("/v/x.md", "notes/foo"), note("/v/notes/foo.md", "别的")]);
    const hit = resolveLink(index, "notes/foo");
    expect(hit?.note.path).toBe("/v/notes/foo.md");
    expect(hit?.via).toBe("path");
  });

  it("路径尾段匹配要求边界对齐", () => {
    const index = buildLinkIndex([note("/v/ab/foo.md", "X")]);
    // `b/foo` 不能命中 `/v/ab/foo.md` —— 尾段前面必须是分隔符。
    expect(resolveLink(index, "b/foo")).toBeNull();
    expect(resolveLink(index, "ab/foo")?.note.path).toBe("/v/ab/foo.md");
  });

  it("解析不到就是 null", () => {
    const index = buildLinkIndex([note("/v/foo.md", "Foo")]);
    expect(resolveLink(index, "bar")).toBeNull();
    expect(resolveLink(index, "")).toBeNull();
    expect(resolveLink(index, "   ")).toBeNull();
  });

  it("空 vault 不炸", () => {
    expect(resolveLink(buildLinkIndex([]), "foo")).toBeNull();
  });

  it("大小写与 .md 后缀都不影响命中", () => {
    const index = buildLinkIndex([note("/v/Foo.md", "标题")]);
    expect(resolveLink(index, "foo")?.note.path).toBe("/v/Foo.md");
    expect(resolveLink(index, "FOO.md")?.note.path).toBe("/v/Foo.md");
  });
});

describe("scanWikiLinks", () => {
  it("扫出位置与内容,含嵌入语法", () => {
    const links = scanWikiLinks("see [[Foo]] and ![[Bar]] done");
    expect(links).toHaveLength(2);
    expect(links[0]).toMatchObject({ target: "Foo", embed: false, raw: "Foo" });
    expect(links[1]).toMatchObject({ target: "Bar", embed: true, raw: "Bar" });
    // 嵌入的 from 要含上前面那个 `!`,否则按 from/to 替换会留下一个孤立的 `!`。
    expect("see [[Foo]] and ![[Bar]] done".slice(links[1]!.from, links[1]!.to)).toBe("![[Bar]]");
    expect("see [[Foo]] and ![[Bar]] done".slice(links[0]!.from, links[0]!.to)).toBe("[[Foo]]");
  });

  it("不跨行,也不吃掉没闭合的方括号", () => {
    // 允许跨行的话一个孤立的 `[[` 会一路吃到几百行之后的某个 `]]`。
    expect(scanWikiLinks("[[Foo\nBar]]")).toHaveLength(0);
    expect(scanWikiLinks("[[unclosed")).toHaveLength(0);
  });

  it("拆不出目标的那些不算链接", () => {
    expect(scanWikiLinks("[[]] [[|x]] [[#h]]")).toHaveLength(0);
  });

  it("超长内容不当成链接", () => {
    // 上限挡的是带很多方括号的代码/正则,不是真实链接。
    const long = "x".repeat(201);
    expect(scanWikiLinks(`[[${long}]]`)).toHaveLength(0);
    expect(scanWikiLinks(`[[${"y".repeat(200)}]]`)).toHaveLength(1);
  });

  it("连续多条都能扫到", () => {
    // 正则带 g,lastIndex 没重置的话第二次调用会从上次的位置起扫。
    expect(scanWikiLinks("[[a]][[b]][[c]]").map((l) => l.target)).toEqual(["a", "b", "c"]);
    expect(scanWikiLinks("[[a]][[b]][[c]]").map((l) => l.target)).toEqual(["a", "b", "c"]);
  });
});

describe("outgoingLinks", () => {
  it("只留解析得到的目标,并按笔记去重", () => {
    const index = buildLinkIndex([note("/v/foo.md", "Foo"), note("/v/bar.md", "Bar")]);
    const links = outgoingLinks(index, "[[Foo]] [[foo]] [[Bar]] [[Missing]]");
    // `[[Foo]]` 和 `[[foo]]` 是同一篇,只算一次。死链不进反链 —— 反链是"谁指向我"。
    expect(links.sort()).toEqual(["/v/bar.md", "/v/foo.md"]);
  });

  it("代码块里的链接也算提到", () => {
    // 刻意不排除:反链统计的是"这篇笔记提到过谁"。在扫描层一刀切会让反链
    // 静默漏掉一部分。
    const index = buildLinkIndex([note("/v/foo.md", "Foo")]);
    expect(outgoingLinks(index, "```\n[[Foo]]\n```")).toEqual(["/v/foo.md"]);
  });

  it("嵌入也算一条出链", () => {
    const index = buildLinkIndex([note("/v/foo.md", "Foo")]);
    expect(outgoingLinks(index, "![[Foo]]")).toEqual(["/v/foo.md"]);
  });
});

describe("linkTitleOf", () => {
  it("内存里只是文件名顶着时采信索引", () => {
    // 列表只读目录项 —— 没读入的笔记 title 就是 stem。真标题在 frontmatter 里,
    // 只有扫盘索引知道。少了这一档,`[[周报]]` 在目标被打开过之前是死链。
    const titles = new Map([["/v/cao-gao.md", "周报"]]);
    expect(linkTitleOf({ path: "/v/cao-gao.md", title: "cao-gao" }, titles)).toBe("周报");
  });

  it("内存里是真标题时不被索引里的旧值顶掉", () => {
    // 刚在列表里改过标题:内存是新的,索引还是上一次扫盘的结果。反过来采信索引
    // 会让用户按新标题写的链接解析不到 —— 而他们眼前的列表已经显示新标题了。
    const titles = new Map([["/v/cao-gao.md", "旧标题"]]);
    expect(linkTitleOf({ path: "/v/cao-gao.md", title: "新标题" }, titles)).toBe("新标题");
  });

  it("索引里没有这条笔记时用内存里那份", () => {
    // 刚新建、还没重扫的笔记走这条路。
    expect(linkTitleOf({ path: "/v/fresh.md", title: "Fresh" }, new Map())).toBe("Fresh");
  });

  it("占位判定不受大小写与扩展名写法影响", () => {
    // stem 是 `Cao-Gao`、内存里是 `cao-gao` 这种差异来自不同平台的文件系统,
    // 不该被当成"用户改过标题"。
    const titles = new Map([["/v/Cao-Gao.md", "周报"]]);
    expect(linkTitleOf({ path: "/v/Cao-Gao.md", title: "cao-gao" }, titles)).toBe("周报");
  });

  it("索引里是空串时不把标题清掉", () => {
    // 后端读不动文件头时标题会回落到 stem;真出现空串也不能让笔记变成无名 ——
    // 无名的笔记在任何一张表里都建不了索引键,链接会整片失效。
    const titles = new Map([["/v/cao-gao.md", ""]]);
    expect(linkTitleOf({ path: "/v/cao-gao.md", title: "cao-gao" }, titles)).toBe("cao-gao");
  });
});
