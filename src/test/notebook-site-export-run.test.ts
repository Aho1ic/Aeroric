/* 整库静态站点导出的执行层。
 *
 * 纯函数那部分(路径计算、DOM 改写、首页)在 notebook-export-model.test.ts。这里管的是
 * 循环本身:进度、取消、单篇失败不中断、链接改写有没有真的落到产物上。
 */

import { describe, expect, it, vi } from "vitest";
import {
  runSiteExport,
  type SiteExportDeps,
  type SiteExportProgress,
} from "../components/notebook/noteSiteExportRun";

type Written = { outDir: string; rel: string; content: string };

function deps(
  bodies: Record<string, string>,
  overrides: Partial<SiteExportDeps> = {},
): { deps: SiteExportDeps; written: Written[] } {
  const written: Written[] = [];
  return {
    written,
    deps: {
      readNote: vi.fn(async (path: string) => {
        const body = bodies[path];
        if (body === undefined) throw new Error(`读不到 ${path}`);
        return body;
      }),
      readImage: vi.fn().mockResolvedValue(new Uint8Array([0x68, 0x69]).buffer),
      writePage: vi.fn(async (outDir: string, rel: string, content: string) => {
        written.push({ outDir, rel, content });
      }),
      pageCountLabel: (count: number) => `共 ${count} 页`,
      embedPrefix: "↪",
      ...overrides,
    },
  };
}

const INPUT = {
  vault: "/v",
  siteTitle: "我的笔记",
  notes: [
    { path: "/v/a.md", title: "第一篇" },
    { path: "/v/sub/b.md", title: "第二篇" },
  ],
  outDir: "/Users/me/Desktop/site",
};

/** 从产物里取某一页。 */
function page(written: Written[], rel: string): string {
  const hit = written.find((w) => w.rel === rel);
  if (!hit) throw new Error(`没写出 ${rel},实际写了 ${written.map((w) => w.rel).join(", ")}`);
  return hit.content;
}

describe("整库导出", () => {
  it("每篇写一页,再补一个首页", async () => {
    const { deps: d, written } = deps({
      "/v/a.md": "# 第一篇\n\n内容甲",
      "/v/sub/b.md": "# 第二篇\n\n内容乙",
    });
    const result = await runSiteExport(INPUT, d);
    expect(result).toEqual({ written: 2, failed: 0, total: 2 });
    expect(written.map((w) => w.rel).sort()).toEqual(["a.html", "index.html", "sub/b.html"]);
    expect(page(written, "a.html")).toContain("内容甲");
    expect(page(written, "sub/b.html")).toContain("内容乙");
  });

  it("每页都是自洽的独立文档", async () => {
    const { deps: d, written } = deps({ "/v/a.md": "正文", "/v/sub/b.md": "正文" });
    await runSiteExport(INPUT, d);
    const html = page(written, "a.html");
    expect(html.startsWith("<!doctype html>")).toBe(true);
    expect(html).toContain("<style>");
    expect(html).toContain("<title>第一篇</title>");
  });

  it("首页列出全部页面和总数", async () => {
    const { deps: d, written } = deps({ "/v/a.md": "x", "/v/sub/b.md": "y" });
    await runSiteExport(INPUT, d);
    const index = page(written, "index.html");
    expect(index).toContain("我的笔记");
    expect(index).toContain("共 2 页");
    expect(index).toContain('href="a.html"');
    expect(index).toContain('href="sub/b.html"');
  });

  it("附件不参与导出", async () => {
    const { deps: d, written } = deps({ "/v/a.md": "x" });
    const result = await runSiteExport(
      {
        ...INPUT,
        notes: [
          { path: "/v/a.md", title: "第一篇" },
          { path: "/v/pic.png", title: "图" },
        ],
      },
      d,
    );
    expect(result.total).toBe(1);
    expect(written.map((w) => w.rel)).not.toContain("pic.html");
  });

  it("用 readNote 而不是打开笔记 —— 打开会登记编辑基线", async () => {
    const { deps: d } = deps({ "/v/a.md": "x", "/v/sub/b.md": "y" });
    await runSiteExport(INPUT, d);
    expect(d.readNote).toHaveBeenCalledWith("/v/a.md");
    expect(d.readNote).toHaveBeenCalledWith("/v/sub/b.md");
  });
});

describe("链接改写落到产物上", () => {
  it("wikilink 变成站内相对链接", async () => {
    const { deps: d, written } = deps({
      "/v/a.md": "看 [[第二篇]]",
      "/v/sub/b.md": "内容",
    });
    await runSiteExport(INPUT, d);
    expect(page(written, "a.html")).toContain('href="sub/b.html"');
  });

  it("从子目录往上链算得对", async () => {
    const { deps: d, written } = deps({
      "/v/a.md": "内容",
      "/v/sub/b.md": "回到 [[第一篇]]",
    });
    await runSiteExport(INPUT, d);
    expect(page(written, "sub/b.html")).toContain('href="../a.html"');
  });

  it("死链退化成纯文本,不留一个 404 链接", async () => {
    const { deps: d, written } = deps({ "/v/a.md": "看 [[不存在的]]", "/v/sub/b.md": "x" });
    await runSiteExport(INPUT, d);
    const html = page(written, "a.html");
    expect(html).toContain("不存在的");
    expect(html).not.toContain("notebook-wikilink");
    // 没有指向不存在页面的链接。
    expect(html).not.toMatch(/href="[^"]*不存在的[^"]*"/);
  });

  it("相对 .md 链接换成 .html", async () => {
    const { deps: d, written } = deps({
      "/v/a.md": "[去](sub/b.md)",
      "/v/sub/b.md": "x",
    });
    await runSiteExport(INPUT, d);
    expect(page(written, "a.html")).toContain('href="sub/b.html"');
  });

  it("链接索引用全部笔记建 —— 被过滤掉的目标也不该变成死链", async () => {
    // 目标是一篇会被导出的笔记,但索引如果只用「参与导出的」建,顺序或过滤出错时
    // 这条链接会静默退化成纯文本。
    const { deps: d, written } = deps({ "/v/a.md": "看 [[第二篇]]", "/v/sub/b.md": "x" });
    await runSiteExport(INPUT, d);
    expect(page(written, "a.html")).toMatch(/href="sub\/b\.html"/);
  });
});

describe("图片内联", () => {
  it("本地图变成 data URL,相对各自笔记所在目录", async () => {
    const { deps: d, written } = deps({
      "/v/sub/b.md": "![图](pic.png)",
      "/v/a.md": "x",
    });
    await runSiteExport(INPUT, d);
    expect(d.readImage).toHaveBeenCalledWith("/v/sub/pic.png");
    expect(page(written, "sub/b.html")).toContain("data:image/png;base64,");
  });

  it("读不到图时这一页照样写出去", async () => {
    const { deps: d, written } = deps(
      { "/v/a.md": "![图](missing.png)", "/v/sub/b.md": "x" },
      { readImage: vi.fn().mockRejectedValue(new Error("没有这张图")) },
    );
    const result = await runSiteExport(INPUT, d);
    expect(result.written).toBe(2);
    expect(page(written, "a.html")).toContain('src="missing.png"');
  });
});

describe("失败与取消", () => {
  it("单篇读不到时其余照常导出,失败计数如实报", async () => {
    const { deps: d, written } = deps({ "/v/sub/b.md": "只有这篇能读" });
    const result = await runSiteExport(INPUT, d);
    expect(result).toEqual({ written: 1, failed: 1, total: 2 });
    expect(written.map((w) => w.rel).sort()).toEqual(["index.html", "sub/b.html"]);
  });

  it("单篇写盘失败也不中断", async () => {
    const writePage = vi.fn(async (_out: string, rel: string) => {
      if (rel === "a.html") throw new Error("写不进去");
    });
    const { deps: d } = deps({ "/v/a.md": "x", "/v/sub/b.md": "y" }, { writePage });
    const result = await runSiteExport(INPUT, d);
    expect(result).toEqual({ written: 1, failed: 1, total: 2 });
  });

  it("首页写失败不算整体失败 —— 每页本身都能直接打开", async () => {
    const writePage = vi.fn(async (_out: string, rel: string) => {
      if (rel === "index.html") throw new Error("写不进去");
    });
    const { deps: d } = deps({ "/v/a.md": "x", "/v/sub/b.md": "y" }, { writePage });
    const result = await runSiteExport(INPUT, d);
    expect(result).toEqual({ written: 2, failed: 0, total: 2 });
  });

  it("全库为空时不写任何东西,也不报错", async () => {
    const { deps: d, written } = deps({});
    const result = await runSiteExport({ ...INPUT, notes: [] }, d);
    expect(result).toEqual({ written: 0, failed: 0, total: 0 });
    // 一个页面都没有时连首页也不写:一个空目录比一个空索引页更好懂。
    expect(written).toHaveLength(0);
  });

  it("取消信号让循环停在页边界,不写出半个文件", async () => {
    const controller = new AbortController();
    const { deps: d, written } = deps({ "/v/a.md": "x", "/v/sub/b.md": "y" });
    const readNote = vi.fn(async (path: string) => {
      // 第一篇读完就取消。
      controller.abort();
      return path === "/v/a.md" ? "x" : "y";
    });
    const result = await runSiteExport(INPUT, { ...d, readNote }, undefined, controller.signal);
    expect(result.written).toBe(1);
    expect(readNote).toHaveBeenCalledTimes(1);
    // 已经写出去的那一页仍然有入口。
    expect(written.map((w) => w.rel).sort()).toEqual(["a.html", "index.html"]);
  });

  it("一开始就取消时什么都不写", async () => {
    const controller = new AbortController();
    controller.abort();
    const { deps: d, written } = deps({ "/v/a.md": "x", "/v/sub/b.md": "y" });
    const result = await runSiteExport(INPUT, d, undefined, controller.signal);
    expect(result.written).toBe(0);
    expect(written).toHaveLength(0);
  });
});

describe("进度回报", () => {
  it("每篇开始前报一次,收尾再报一次", async () => {
    const { deps: d } = deps({ "/v/a.md": "x", "/v/sub/b.md": "y" });
    const seen: SiteExportProgress[] = [];
    await runSiteExport(INPUT, d, (p) => seen.push({ ...p }));
    expect(seen).toEqual([
      { done: 0, total: 2, current: "第一篇" },
      { done: 1, total: 2, current: "第二篇" },
      { done: 2, total: 2, current: "" },
    ]);
  });

  it("失败的那篇也计入收尾的 done —— 否则进度条永远差一截", async () => {
    const { deps: d } = deps({ "/v/sub/b.md": "y" });
    const seen: SiteExportProgress[] = [];
    await runSiteExport(INPUT, d, (p) => seen.push({ ...p }));
    expect(seen[seen.length - 1]).toEqual({ done: 2, total: 2, current: "" });
  });
});
