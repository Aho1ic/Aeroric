/* 导出动作 → 结果文案。
 *
 * 和 `notebook-export-flow.test.ts` 的分工:那边验每条导出通道自己做对了什么(渲染、
 * 内联图、落盘调用),这边验跑完之后**说什么** —— 取消不该报「已导出」,部分失败不该
 * 报「完成」,有图没内联必须当场出声。这些分支是用户唯一能看见的东西,而它们全在
 * `noteExportRun.ts` 一处收口。
 */

import { describe, expect, it, vi } from "vitest";

import {
  runSingleExport,
  runSiteExportAction,
  vaultSiteTitle,
  SINGLE_ACTIONS,
  type ExportAction,
} from "../components/notebook/noteExportRun";
import type { ExportDeps, ExportSource } from "../components/notebook/noteExport";
import type { SiteExportDeps } from "../components/notebook/noteSiteExportRun";
import { staticT } from "../i18n";

const source: ExportSource = {
  path: "/vault/note.md",
  title: "标题",
  body: "# 标题\n\n正文",
};

function depsOf(overrides: Partial<ExportDeps> = {}): ExportDeps {
  return {
    pickPath: vi.fn(async () => "/out/标题.html"),
    write: vi.fn(async () => {}),
    writeText: vi.fn(async () => {}),
    print: vi.fn(async () => {}),
    readImage: vi.fn(async () => new ArrayBuffer(4)),
    lang: "zh",
    ...overrides,
  };
}

describe("runSingleExport", () => {
  it("落盘成功后报出路径", async () => {
    const out = await runSingleExport("html", source, depsOf(), staticT);
    expect(out.error).toBeNull();
    expect(out.notice).toContain("/out/标题.html");
  });

  it("对话框取消:既不报成功也不报失败", async () => {
    const write = vi.fn(async () => {});
    const out = await runSingleExport(
      "html",
      source,
      depsOf({ pickPath: async () => null, write }),
      staticT,
    );
    expect(out).toEqual({ notice: null, error: null });
    // 取消之后不该落盘 —— 这条是 noteExport 的职责,在这里再钉一次是因为
    // "取消却写了文件"会同时表现为"没有提示",光看文案分不出来。
    expect(write).not.toHaveBeenCalled();
  });

  it("复制类导出报「已复制」,不报路径", async () => {
    const out = await runSingleExport("copyMarkdown", source, depsOf(), staticT);
    expect(out.notice).toBe(staticT("notebook.exportCopied"));
  });

  it("PDF 没有落盘路径,顺利跑完时不出声", async () => {
    const print = vi.fn(async () => {});
    const out = await runSingleExport("pdf", source, depsOf({ print }), staticT);
    expect(print).toHaveBeenCalledTimes(1);
    expect(out).toEqual({ notice: null, error: null });
  });

  it("没有笔记时报「没有可导出的笔记」", async () => {
    const out = await runSingleExport("html", null, depsOf(), staticT);
    expect(out.error).toBe(staticT("notebook.exportNoNote"));
    expect(out.notice).toBeNull();
  });

  it("通道抛错时把原始消息带进文案", async () => {
    const out = await runSingleExport(
      "markdown",
      source,
      depsOf({
        write: async () => {
          throw new Error("磁盘满了");
        },
      }),
      staticT,
    );
    expect(out.notice).toBeNull();
    expect(out.error).toContain("磁盘满了");
  });

  it("抛出的不是 Error 也要能转成文案", async () => {
    const out = await runSingleExport(
      "markdown",
      source,
      depsOf({
        write: async () => {
          throw "字符串异常";
        },
      }),
      staticT,
    );
    expect(out.error).toContain("字符串异常");
  });

  it("site 不是单篇动作,走到这里要报失败而不是静默成功", async () => {
    const out = await runSingleExport("site", source, depsOf(), staticT);
    expect(out.notice).toBeNull();
    expect(out.error).not.toBeNull();
  });

  it("五条单篇动作都不抛未处理异常", async () => {
    for (const action of SINGLE_ACTIONS) {
      const out = await runSingleExport(action, source, depsOf(), staticT);
      expect(out.error, `${action} 不该失败`).toBeNull();
    }
  });
});

describe("runSingleExport 的图片提醒", () => {
  const withImage: ExportSource = {
    path: "/vault/note.md",
    title: "带图",
    body: "![图](./missing.png)",
  };

  /** 读图失败 → 那张图不会被内联 → skipped 计数上去。 */
  function failingImageDeps(overrides: Partial<ExportDeps> = {}): ExportDeps {
    return depsOf({
      readImage: async () => {
        throw new Error("读不到");
      },
      ...overrides,
    });
  }

  it("有图没内联时,成功文案后面追一行提醒", async () => {
    const out = await runSingleExport("html", withImage, failingImageDeps(), staticT);
    expect(out.error).toBeNull();
    expect(out.notice).toContain("/out/标题.html");
    // 提醒是独立一行,不是把路径挤掉。
    expect(out.notice?.split("\n")).toHaveLength(2);
    expect(out.notice).toContain("1");
  });

  it("取消了但有图没内联:仍然要提醒", async () => {
    const out = await runSingleExport(
      "html",
      withImage,
      failingImageDeps({ pickPath: async () => null }),
      staticT,
    );
    // 渲染已经发生过,用户下次导出同一篇还是会撞上同样的问题 —— 说了不亏。
    expect(out.notice).not.toBeNull();
    expect(out.notice).not.toContain("/out/");
    expect(out.error).toBeNull();
  });

  it("PDF 有图没内联时打破沉默", async () => {
    const out = await runSingleExport("pdf", withImage, failingImageDeps(), staticT);
    expect(out.notice).not.toBeNull();
  });

  it("Markdown 不经渲染,不会报图片提醒", async () => {
    const out = await runSingleExport(
      "markdown",
      withImage,
      failingImageDeps({ pickPath: async () => "/out/带图.md" }),
      staticT,
    );
    expect(out.notice).toBe(staticT("notebook.exportDone", { path: "/out/带图.md" }));
  });

  it("复制为 Markdown 同理,不提图片", async () => {
    const out = await runSingleExport("copyMarkdown", withImage, failingImageDeps(), staticT);
    expect(out.notice).toBe(staticT("notebook.exportCopied"));
  });
});

describe("vaultSiteTitle", () => {
  it("取目录名", () => {
    expect(vaultSiteTitle("/Users/me/Notes")).toBe("Notes");
  });

  it("尾部分隔符不会让标题变空", () => {
    expect(vaultSiteTitle("/Users/me/Notes/")).toBe("Notes");
    expect(vaultSiteTitle("/Users/me/Notes///")).toBe("Notes");
  });

  it("Windows 路径", () => {
    expect(vaultSiteTitle("C:\\Users\\me\\Notes")).toBe("Notes");
    expect(vaultSiteTitle("C:\\Users\\me\\Notes\\")).toBe("Notes");
  });

  it("只剩分隔符时回落到原串,不给空标题", () => {
    expect(vaultSiteTitle("/")).toBe("/");
  });
});

describe("runSiteExportAction", () => {
  function siteDeps(overrides: Partial<SiteExportDeps> = {}): SiteExportDeps {
    return {
      readNote: vi.fn(async () => "# 一页\n\n正文"),
      readImage: vi.fn(async () => new ArrayBuffer(4)),
      writePage: vi.fn(async () => {}),
      pageCountLabel: (count) => `共 ${count} 页`,
      embedPrefix: "↪",
      ...overrides,
    };
  }

  const notes = [
    { path: "/vault/a.md", title: "A" },
    { path: "/vault/b.md", title: "B" },
  ];

  function ctxOf(deps: SiteExportDeps, pickDir = vi.fn(async () => "/out" as string | null)) {
    return { vault: "/vault", siteTitle: "库", notes, pickDir, deps };
  }

  it("全部成功报页数", async () => {
    const out = await runSiteExportAction(ctxOf(siteDeps()), staticT);
    expect(out.error).toBeNull();
    expect(out.notice).toBe(staticT("notebook.exportSiteDone", { written: "2" }));
  });

  it("选目录取消:什么都不说,也不写盘", async () => {
    const deps = siteDeps();
    const out = await runSiteExportAction(
      ctxOf(
        deps,
        vi.fn(async () => null),
      ),
      staticT,
    );
    expect(out).toEqual({ notice: null, error: null });
    expect(deps.writePage).not.toHaveBeenCalled();
  });

  it("部分失败当 notice 报,带上失败数", async () => {
    const deps = siteDeps({
      readNote: vi.fn(async (path: string) => {
        if (path.endsWith("b.md")) throw new Error("读不到");
        return "# A";
      }),
    });
    const out = await runSiteExportAction(ctxOf(deps), staticT);
    // 部分失败不是 error:多数页面已经落盘,那是个能用的产物。
    expect(out.error).toBeNull();
    expect(out.notice).toBe(
      staticT("notebook.exportSiteDoneWithFailures", { written: "1", failed: "1" }),
    );
  });

  it("取消报「已取消」而不是「完成」", async () => {
    const controller = new AbortController();
    const deps = siteDeps({
      readNote: vi.fn(async () => {
        controller.abort();
        return "# A";
      }),
    });
    const out = await runSiteExportAction(ctxOf(deps), staticT, undefined, controller.signal);
    expect(out.notice).toContain(staticT("notebook.exportSiteCancelled", { written: "1" }));
    expect(out.notice).not.toContain(staticT("notebook.exportSiteDone", { written: "1" }));
  });

  it("进度回调按篇上报,收尾再报一次", async () => {
    const seen: string[] = [];
    await runSiteExportAction(ctxOf(siteDeps()), staticT, (p) => seen.push(p.current));
    expect(seen).toEqual(["A", "B", ""]);
  });

  it("写盘整体抛错时报导出失败", async () => {
    const deps = siteDeps({
      writePage: vi.fn(async () => {
        throw new Error("目录不可写");
      }),
    });
    const out = await runSiteExportAction(ctxOf(deps), staticT);
    // 每页失败被 runSiteExport 吞掉计入 failed,首页也失败 → 0 成功 2 失败。
    expect(out.notice).toBe(
      staticT("notebook.exportSiteDoneWithFailures", { written: "0", failed: "2" }),
    );
  });

  it("选目录本身抛错要报出来", async () => {
    const out = await runSiteExportAction(
      ctxOf(
        siteDeps(),
        vi.fn(async () => {
          throw new Error("对话框炸了");
        }),
      ),
      staticT,
    );
    expect(out.notice).toBeNull();
    expect(out.error).toContain("对话框炸了");
  });

  it("空库:不报错,页数为 0", async () => {
    const deps = siteDeps();
    const out = await runSiteExportAction(
      { vault: "/vault", siteTitle: "库", notes: [], pickDir: async () => "/out", deps },
      staticT,
    );
    expect(out.error).toBeNull();
    expect(out.notice).toBe(staticT("notebook.exportSiteDone", { written: "0" }));
    // 一页都没有就不该写首页 —— 一个只有空目录的产物比没有产物更让人困惑。
    expect(deps.writePage).not.toHaveBeenCalled();
  });

  it("导出目录来自 pickDir,不是别处猜的", async () => {
    const deps = siteDeps();
    await runSiteExportAction(
      ctxOf(
        deps,
        vi.fn(async () => "/somewhere/else"),
      ),
      staticT,
    );
    const calls = (deps.writePage as ReturnType<typeof vi.fn>).mock.calls;
    expect(calls.length).toBeGreaterThan(0);
    for (const call of calls) {
      expect(call[0]).toBe("/somewhere/else");
    }
  });
});

describe("SINGLE_ACTIONS", () => {
  it("不含整库导出 —— 它和当前笔记无关", () => {
    expect(SINGLE_ACTIONS).not.toContain("site" as ExportAction);
    expect(SINGLE_ACTIONS).toHaveLength(5);
  });
});
