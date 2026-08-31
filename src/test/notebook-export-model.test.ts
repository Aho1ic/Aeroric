import { describe, expect, it } from "vitest";
import {
  escapeHtmlText,
  exportFileName,
  wrapStandaloneHtml,
} from "../components/notebook/noteExportHtml";
import {
  buildIndexHtml,
  exportableNotes,
  relativeHref,
  rewriteForSite,
  siteRelPath,
} from "../components/notebook/noteSiteExport";
import { WIKI_EMBED_CLASS, WIKI_LINK_CLASS } from "../components/notebook/enhanceWikiLinks";

describe("导出的独立 HTML", () => {
  it("是一个自洽的文档:声明、charset、内联样式都在", () => {
    const html = wrapStandaloneHtml("标题", "<p>正文</p>");
    expect(html.startsWith("<!doctype html>")).toBe(true);
    expect(html).toContain('<meta charset="utf-8" />');
    expect(html).toContain("<style>");
    expect(html).toContain("<p>正文</p>");
    // 没有任何外链:导出物离线打开也要是对的。
    expect(html).not.toMatch(/<link\b/);
    expect(html).not.toMatch(/<script\b/);
  });

  it("标题转义,正文原样放行", () => {
    const html = wrapStandaloneHtml("<script>x</script>", "<p><em>斜体</em></p>");
    // 标题是纯文本,尖括号必须变成实体,否则它会在 <title> 里提前闭合。
    expect(html).toContain("<title>&lt;script&gt;x&lt;/script&gt;</title>");
    // 正文已经过渲染管线的 sanitize,再转义一遍会把标签变成可见字符。
    expect(html).toContain("<p><em>斜体</em></p>");
  });

  it("lang 跟着界面语言走", () => {
    expect(wrapStandaloneHtml("t", "", "en")).toContain('<html lang="en">');
    expect(wrapStandaloneHtml("t", "")).toContain('<html lang="zh">');
  });

  it("打印样式把版心放开", () => {
    const html = wrapStandaloneHtml("t", "");
    expect(html).toContain("@media print");
    expect(html).toContain("break-inside: avoid");
  });

  it("escapeHtmlText 连引号一起转义", () => {
    // 这个函数也用在属性值上(站点首页的 href),漏了引号就能提前闭合属性。
    expect(escapeHtmlText(`a"b<c>d&e`)).toBe("a&quot;b&lt;c&gt;d&amp;e");
  });
});

describe("导出文件名", () => {
  it("剥掉 markdown 扩展名", () => {
    expect(exportFileName("会议纪要.md")).toBe("会议纪要");
    expect(exportFileName("note.MARKDOWN")).toBe("note");
  });

  it("路径分隔符换成下划线,否则会被当成目录", () => {
    expect(exportFileName("2026/03 复盘")).toBe("2026_03 复盘");
    expect(exportFileName("a\\b")).toBe("a_b");
  });

  it("Windows 保留字符换成下划线", () => {
    expect(exportFileName('a:b*c?d"e<f>g|h')).toBe("a_b_c_d_e_f_g_h");
  });

  it("空格和连字符留着 —— 它们在标题里很常见,落盘也合法", () => {
    expect(exportFileName("周会 - 三月")).toBe("周会 - 三月");
  });

  it("控制字符换成下划线", () => {
    // 用 fromCharCode 构造:源码里写字面控制字节会让整个文件被判成二进制。
    expect(exportFileName(`a${String.fromCharCode(9)}b`)).toBe("a_b");
  });

  it("结尾的点和空格去掉 —— Windows 会静默吃掉它们", () => {
    expect(exportFileName("草稿...")).toBe("草稿");
    expect(exportFileName("草稿   ")).toBe("草稿");
  });

  it("空标题退回 untitled,不产出空文件名", () => {
    expect(exportFileName("")).toBe("untitled");
    expect(exportFileName("   ")).toBe("untitled");
    expect(exportFileName("///")).toBe("___");
  });

  it("过长的标题截断", () => {
    expect(exportFileName("字".repeat(300))).toHaveLength(120);
  });
});

describe("站内路径", () => {
  it("剥 vault 前缀并把扩展名换成 .html", () => {
    expect(siteRelPath("/v/sub/note.md", "/v")).toBe("sub/note.html");
    expect(siteRelPath("/v/note.markdown", "/v")).toBe("note.html");
  });

  it("vault 结尾多余的分隔符不影响结果", () => {
    expect(siteRelPath("/v/note.md", "/v/")).toBe("note.html");
  });

  it("大小写不同也能剥掉 —— macOS 的文件系统大小写不敏感", () => {
    expect(siteRelPath("/Vault/note.md", "/vault")).toBe("note.html");
  });

  it("Windows 反斜杠归一成 /", () => {
    expect(siteRelPath("C:\\v\\sub\\note.md", "C:\\v")).toBe("sub/note.html");
  });

  it("不在 vault 里的路径不硬剥前缀", () => {
    // 剥错前缀会切出一条指向别处的假路径,那比留着绝对路径更难查。
    expect(siteRelPath("/other/note.md", "/v")).toBe("other/note.html");
  });
});

describe("站内相对链接", () => {
  it("同目录直接给文件名", () => {
    expect(relativeHref("a/one.html", "a/two.html")).toBe("two.html");
  });

  it("往下钻", () => {
    expect(relativeHref("index.html", "sub/note.html")).toBe("sub/note.html");
  });

  it("往上爬", () => {
    expect(relativeHref("sub/deep/note.html", "top.html")).toBe("../../top.html");
  });

  it("跨分支", () => {
    expect(relativeHref("a/one.html", "b/two.html")).toBe("../b/two.html");
  });

  it("共同前缀只算到目录一级,不把文件名算进去", () => {
    // fromDir=['a'],to=['a','b.html']:共享 'a',结果是 'b.html' 而不是 '../a/b.html'。
    expect(relativeHref("a/x.html", "a/b.html")).toBe("b.html");
  });
});

describe("站点化改写", () => {
  function host(html: string): HTMLElement {
    const el = document.createElement("div");
    el.innerHTML = html;
    return el;
  }

  it("wikilink 变成站内相对链接", () => {
    const el = host(`<a class="${WIKI_LINK_CLASS}" data-wiki-path="/v/sub/target.md">目标</a>`);
    rewriteForSite(el, "index.html", "/v");
    const anchor = el.querySelector("a");
    expect(anchor?.getAttribute("href")).toBe("sub/target.html");
    expect(anchor?.textContent).toBe("目标");
    // 应用态的类名不该留在站点产物里,它挂的是应用内的点击行为。
    expect(anchor?.className).toBe("");
  });

  it("解析不到目标的 wikilink 退化成纯文本", () => {
    const el = host(`<p><a class="${WIKI_LINK_CLASS}">缺失的笔记</a></p>`);
    rewriteForSite(el, "index.html", "/v");
    expect(el.querySelector("a")).toBeNull();
    expect(el.textContent).toBe("缺失的笔记");
  });

  it("嵌入变成指向目标页的链接", () => {
    const el = host(
      `<span class="${WIKI_EMBED_CLASS}" data-embed-path="/v/t.md" data-embed-target="t"></span>`,
    );
    rewriteForSite(el, "sub/here.html", "/v", "↪");
    const anchor = el.querySelector("a");
    expect(anchor?.getAttribute("href")).toBe("../t.html");
    expect(anchor?.textContent).toBe("↪ t");
  });

  it("解析不到目标的嵌入留下原始语法", () => {
    const el = host(
      `<span class="${WIKI_EMBED_CLASS}" data-embed-target="t" data-embed-raw="![[t]]"></span>`,
    );
    rewriteForSite(el, "index.html", "/v");
    expect(el.textContent).toBe("![[t]]");
  });

  it("相对 .md 链接换成 .html,锚点保留", () => {
    const el = host(`<a href="other.md#section">链接</a>`);
    rewriteForSite(el, "index.html", "/v");
    expect(el.querySelector("a")?.getAttribute("href")).toBe("other.html#section");
  });

  it("外链、锚点、协议相对地址都不动", () => {
    const el = host(
      `<a href="https://example.com/a.md">外</a>` +
        `<a href="#anchor">锚</a>` +
        `<a href="//cdn/a.md">协议相对</a>` +
        `<a href="mailto:a@b.md">邮件</a>`,
    );
    rewriteForSite(el, "index.html", "/v");
    const hrefs = Array.from(el.querySelectorAll("a")).map((a) => a.getAttribute("href"));
    expect(hrefs).toEqual(["https://example.com/a.md", "#anchor", "//cdn/a.md", "mailto:a@b.md"]);
  });

  it("javascript: 链接不被当成站内 md 链接处理", () => {
    // 它带 scheme,所以走的是「不动」那条分支。真正拦下它的是渲染管线的 sanitize,
    // 这里只钉住这一层不会因为改写而把它变成别的东西。
    const el = host(`<a href="javascript:alert(1)">x</a>`);
    rewriteForSite(el, "index.html", "/v");
    expect(el.querySelector("a")?.getAttribute("href")).toBe("javascript:alert(1)");
  });
});

describe("站点首页", () => {
  it("按站内路径排序列出全部页面", () => {
    const html = buildIndexHtml(
      [
        { path: "/v/b.md", rel: "b.html", title: "第二" },
        { path: "/v/a.md", rel: "a.html", title: "第一" },
      ],
      "我的笔记",
      "共 2 页",
    );
    expect(html.indexOf("a.html")).toBeLessThan(html.indexOf("b.html"));
    expect(html).toContain("第一");
    expect(html).toContain("共 2 页");
    expect(html).toContain("<h1>我的笔记</h1>");
  });

  it("标题和路径都转义 —— 它们进的是属性值和文本节点", () => {
    const html = buildIndexHtml(
      [{ path: "/v/x.md", rel: 'a".html', title: "<b>粗</b>" }],
      "站点",
      "共 1 页",
    );
    expect(html).toContain("&lt;b&gt;粗&lt;/b&gt;");
    expect(html).toContain("a&quot;.html");
    expect(html).not.toContain("<b>粗</b>");
  });
});

describe("参与导出的笔记", () => {
  it("只留 markdown,附件不导出", () => {
    const notes = [
      { path: "/v/a.md" },
      { path: "/v/b.markdown" },
      { path: "/v/c.mdx" },
      { path: "/v/d.png" },
      { path: "/v/e.txt" },
    ];
    expect(exportableNotes(notes).map((n) => n.path)).toEqual([
      "/v/a.md",
      "/v/b.markdown",
      "/v/c.mdx",
    ]);
  });

  it("扩展名大小写不敏感", () => {
    expect(exportableNotes([{ path: "/v/A.MD" }])).toHaveLength(1);
  });
});
