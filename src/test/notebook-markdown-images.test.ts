/* 图片 markdown 的解析 / 重建,以及"宽度写在 title 里"这个约定。
 *
 * 宽度没有标准语法,这里用的是 `![a](x.png "width=320")` —— 把它塞进 title 是因为
 * title 是 CommonMark 里唯一能带任意文字又不影响别的渲染器的位置:不认这个约定的
 * 编辑器只会把它当成鼠标悬停提示,图照样显示。
 */

import { describe, expect, it } from "vitest";
import {
  applyImageElementSizing,
  buildImageMarkdown,
  enhanceMarkdownImages,
  imageWidthFromTitle,
  parseImageMarkdown,
  setImageMarkdownWidth,
} from "../components/notebook/markdownImages";

describe("parseImageMarkdown", () => {
  it("拆出 alt / url / title", () => {
    expect(parseImageMarkdown('![a](x.png "t")')).toEqual({ alt: "a", url: "x.png", title: "t" });
    expect(parseImageMarkdown("![a](x.png 't')")).toEqual({ alt: "a", url: "x.png", title: "t" });
    expect(parseImageMarkdown("![](x.png)")).toEqual({ alt: "", url: "x.png", title: undefined });
  });

  it("两头的空白不影响", () => {
    expect(parseImageMarkdown("  ![a](x.png)  ")?.url).toBe("x.png");
  });

  it("不是单独一张图的就不认", () => {
    // 链接、带前后文的行、URL 里带空格的,都交给别的路径处理。
    expect(parseImageMarkdown("[a](x.png)")).toBeNull();
    expect(parseImageMarkdown("see ![a](x.png)")).toBeNull();
    expect(parseImageMarkdown("![a](x.png) tail")).toBeNull();
    expect(parseImageMarkdown("![a](my file.png)")).toBeNull();
  });
});

describe("imageWidthFromTitle", () => {
  it("认 width / w / scale / zoom", () => {
    expect(imageWidthFromTitle("width=320")).toBe("320px");
    expect(imageWidthFromTitle("w=50%")).toBe("50%");
    expect(imageWidthFromTitle("scale=120px")).toBe("120px");
    expect(imageWidthFromTitle("zoom = 80")).toBe("80px");
  });

  it("裸数字补 px,带单位的原样留着", () => {
    // 补 px 是因为 CSS 的 width 不接受裸数字,写进 style 会被整条丢掉。
    expect(imageWidthFromTitle("width=200")).toBe("200px");
    expect(imageWidthFromTitle("width=200%")).toBe("200%");
  });

  it("没有宽度标注就返回 null", () => {
    expect(imageWidthFromTitle(undefined)).toBeNull();
    expect(imageWidthFromTitle("一张截图")).toBeNull();
    // 宽度必须是独立的一段:`maxwidth=3` 不该被当成 `width=3`。
    expect(imageWidthFromTitle("maxwidth=300")).toBeNull();
  });
});

describe("buildImageMarkdown", () => {
  it("alt 里的 `]` 转义掉", () => {
    // 不转义的话 `]` 会提前闭合 alt,整条链接渲染成字面文本。
    expect(buildImageMarkdown({ alt: "a]b", url: "x.png" })).toBe("![a\\]b](x.png)");
  });

  it("title 里的引号和反斜杠转义掉", () => {
    expect(buildImageMarkdown({ alt: "a", url: "x.png", title: 'say "hi"' })).toBe(
      '![a](x.png "say \\"hi\\"")',
    );
    expect(buildImageMarkdown({ alt: "a", url: "x.png", title: "c:\\p" })).toBe(
      '![a](x.png "c:\\\\p")',
    );
  });

  it("空 title 不写出来", () => {
    expect(buildImageMarkdown({ alt: "a", url: "x.png", title: "   " })).toBe("![a](x.png)");
  });
});

describe("setImageMarkdownWidth", () => {
  it("加宽度", () => {
    expect(setImageMarkdownWidth("![a](x.png)", "320px")).toBe('![a](x.png "width=320px")');
  });

  it("改宽度不留下旧值", () => {
    // 留着的话 title 会一路长成 `width=100 width=200 width=300`,而读的是第一个 ——
    // 于是用户改了宽度却没反应。
    expect(setImageMarkdownWidth('![a](x.png "width=100")', "300px")).toBe(
      '![a](x.png "width=300px")',
    );
  });

  it("清宽度时保留 title 里其余的话", () => {
    expect(setImageMarkdownWidth('![a](x.png "一张截图 width=100")', null)).toBe(
      '![a](x.png "一张截图")',
    );
    expect(setImageMarkdownWidth('![a](x.png "width=100")', null)).toBe("![a](x.png)");
  });

  it("不是图片就返回 null", () => {
    expect(setImageMarkdownWidth("[a](x.png)", "10px")).toBeNull();
  });
});

describe("enhanceMarkdownImages", () => {
  it("把 title 里的宽度写进 style", () => {
    const host = document.createElement("div");
    host.innerHTML = `<img src="a.png" title="width=320"><img src="b.png" title="一张截图">`;
    const [sized, plain] = Array.from(host.querySelectorAll("img"));

    enhanceMarkdownImages(host);

    // 不跑这一步的话同一张图在编辑态是 320px,切到阅读态就撑满整行。
    expect(sized!.style.width).toBe("320px");
    expect(sized!.style.height).toBe("auto");
    expect(sized!.dataset.markioImageWidth).toBe("320px");
    // 没有宽度标注的一个字都不改 —— 否则会覆盖样式表里的默认约束。
    expect(plain!.style.width).toBe("");
    expect(plain!.dataset.markioImageWidth).toBeUndefined();
  });

  it("maxWidth 一起写,大图不会顶破容器", () => {
    const img = document.createElement("img");
    applyImageElementSizing(img, "width=2000");
    expect(img.style.maxWidth).toBe("100%");
  });

  it("跑两次结果一样", () => {
    // 预览重渲染会再扫一轮。
    const host = document.createElement("div");
    host.innerHTML = `<img src="a.png" title="width=320">`;
    enhanceMarkdownImages(host);
    enhanceMarkdownImages(host);
    expect(host.querySelector("img")!.style.width).toBe("320px");
  });
});
