/* 单篇导出的编排:渲染 → 内联图 → 落盘 / 打印 / 剪贴板。
 *
 * 外部通道(保存对话框、剪贴板、打印、读图)全部注入,所以这里不需要 mock 模块。
 * `printViaIframe` 的真实行为(WebView 唤起打印)在 jsdom 里跑不到 —— jsdom 不实现
 * `print()`,srcdoc 也不排版。那条路径只钉住「iframe 建了、清理了」,真正的打印
 * 效果只能在桌面应用里手验。
 */

import { describe, expect, it, vi } from "vitest";
import {
  copyAsHtml,
  copyAsMarkdown,
  exportAsHtml,
  exportAsMarkdown,
  exportAsPdf,
  printViaIframe,
  renderForExport,
  type ExportDeps,
  type ExportSource,
} from "../components/notebook/noteExport";

const SOURCE: ExportSource = {
  path: "/vault/sub/note.md",
  title: "会议纪要",
  body: "# 标题\n\n正文一段。",
};

function deps(overrides: Partial<ExportDeps> = {}): ExportDeps {
  return {
    pickPath: vi.fn().mockResolvedValue("/Users/me/Desktop/out.html"),
    write: vi.fn().mockResolvedValue(undefined),
    writeText: vi.fn().mockResolvedValue(undefined),
    print: vi.fn().mockResolvedValue(undefined),
    readImage: vi.fn().mockResolvedValue(new Uint8Array([0x68, 0x69]).buffer),
    lang: "zh",
    ...overrides,
  };
}

describe("为导出渲染", () => {
  it("markdown 变成 HTML", async () => {
    const { html } = await renderForExport(SOURCE, vi.fn());
    expect(html).toContain("标题");
    expect(html).toMatch(/<h1[^>]*>/);
    expect(html).toContain("正文一段。");
  });

  it("不带任务行号 —— 导出物里的复选框点不了,带出去只是噪音", async () => {
    const { html } = await renderForExport({ ...SOURCE, body: "- [ ] 待办" }, vi.fn());
    expect(html).not.toContain("data-task-line");
  });

  it("本地图内联成 data URL,相对笔记所在目录解析", async () => {
    const read = vi.fn().mockResolvedValue(new Uint8Array([0x68, 0x69]).buffer);
    const { html, images } = await renderForExport(
      { ...SOURCE, body: "![图](attachments/a.png)" },
      read,
    );
    expect(read).toHaveBeenCalledWith("/vault/sub/attachments/a.png");
    expect(html).toContain("data:image/png;base64,");
    expect(images).toEqual({ inlined: 1, skipped: 0 });
  });
});

describe("导出成 HTML", () => {
  it("落盘的是完整独立页面,标题和语言都带上", async () => {
    const d = deps();
    const outcome = await exportAsHtml(SOURCE, d);
    expect(outcome.cancelled).toBe(false);
    expect(outcome.path).toBe("/Users/me/Desktop/out.html");
    const [path, content] = (d.write as ReturnType<typeof vi.fn>).mock.calls[0]!;
    expect(path).toBe("/Users/me/Desktop/out.html");
    expect(content).toContain("<!doctype html>");
    expect(content).toContain("<title>会议纪要</title>");
    expect(content).toContain('<html lang="zh">');
    expect(content).toContain("正文一段。");
  });

  it("默认文件名来自标题,扩展名是 html", async () => {
    const d = deps();
    await exportAsHtml(SOURCE, d);
    expect(d.pickPath).toHaveBeenCalledWith("会议纪要", "html", "HTML");
  });

  it("用户取消对话框时不落盘", async () => {
    const d = deps({ pickPath: vi.fn().mockResolvedValue(null) });
    const outcome = await exportAsHtml(SOURCE, d);
    expect(outcome.cancelled).toBe(true);
    expect(outcome.path).toBeNull();
    expect(d.write).not.toHaveBeenCalled();
  });

  it("英文界面下 lang 跟着变", async () => {
    const d = deps({ lang: "en" });
    await exportAsHtml(SOURCE, d);
    const [, content] = (d.write as ReturnType<typeof vi.fn>).mock.calls[0]!;
    expect(content).toContain('<html lang="en">');
  });

  it("落盘失败向上抛,不吞掉", async () => {
    const d = deps({ write: vi.fn().mockRejectedValue(new Error("磁盘满了")) });
    await expect(exportAsHtml(SOURCE, d)).rejects.toThrow("磁盘满了");
  });
});

describe("导出成 PDF", () => {
  it("把独立 HTML 交给打印通道,不落盘", async () => {
    const d = deps();
    const outcome = await exportAsPdf(SOURCE, d);
    expect(outcome.cancelled).toBe(false);
    expect(outcome.path).toBeNull();
    expect(d.write).not.toHaveBeenCalled();
    // 保存对话框也不开:PDF 的落点由系统打印对话框决定。
    expect(d.pickPath).not.toHaveBeenCalled();
    const [html] = (d.print as ReturnType<typeof vi.fn>).mock.calls[0]!;
    expect(html).toContain("<!doctype html>");
    expect(html).toContain("@media print");
  });
});

describe("导出成 Markdown", () => {
  it("原文落盘,不经渲染", async () => {
    const d = deps({ pickPath: vi.fn().mockResolvedValue("/Users/me/Desktop/out.md") });
    const outcome = await exportAsMarkdown(SOURCE, d);
    const [, content] = (d.write as ReturnType<typeof vi.fn>).mock.calls[0]!;
    expect(content).toBe("# 标题\n\n正文一段。");
    expect(d.pickPath).toHaveBeenCalledWith("会议纪要", "md", "Markdown");
    expect(outcome.path).toBe("/Users/me/Desktop/out.md");
  });

  it("不读图 —— markdown 里的图仍然是相对链接", async () => {
    const d = deps({ pickPath: vi.fn().mockResolvedValue("/x/out.md") });
    await exportAsMarkdown({ ...SOURCE, body: "![图](a.png)" }, d);
    expect(d.readImage).not.toHaveBeenCalled();
  });

  it("取消时不落盘", async () => {
    const d = deps({ pickPath: vi.fn().mockResolvedValue(null) });
    const outcome = await exportAsMarkdown(SOURCE, d);
    expect(outcome.cancelled).toBe(true);
    expect(d.write).not.toHaveBeenCalled();
  });
});

describe("复制到剪贴板", () => {
  it("复制 HTML 给的是片段,不带 <html> 外壳", async () => {
    const d = deps();
    await copyAsHtml(SOURCE, d);
    const [text] = (d.writeText as ReturnType<typeof vi.fn>).mock.calls[0]!;
    expect(text).toMatch(/<h1[^>]*>/);
    // 片段是要粘到别的编辑器里的,带上 doctype 和 <style> 会被当成一整篇文档。
    expect(text).not.toContain("<!doctype html>");
    expect(text).not.toContain("<style>");
  });

  it("复制 HTML 时图也内联 —— 粘到别处 blob 链接是死的", async () => {
    const d = deps();
    await copyAsHtml({ ...SOURCE, body: "![图](a.png)" }, d);
    const [text] = (d.writeText as ReturnType<typeof vi.fn>).mock.calls[0]!;
    expect(text).toContain("data:image/png;base64,");
  });

  it("复制 Markdown 给的是原文", async () => {
    const d = deps();
    await copyAsMarkdown(SOURCE, d);
    expect(d.writeText).toHaveBeenCalledWith("# 标题\n\n正文一段。");
  });

  it("复制 Markdown 不触发渲染和读图", async () => {
    const d = deps();
    await copyAsMarkdown({ ...SOURCE, body: "![图](a.png)" }, d);
    expect(d.readImage).not.toHaveBeenCalled();
  });

  it("剪贴板写失败向上抛", async () => {
    const d = deps({ writeText: vi.fn().mockRejectedValue(new Error("剪贴板不可用")) });
    await expect(copyAsMarkdown(SOURCE, d)).rejects.toThrow("剪贴板不可用");
  });
});

describe("打印通道", () => {
  /* jsdom 不实现 window.print,也不给 srcdoc 排版。这几条只钉住 DOM 生命周期:
     iframe 进过文档、最后被移走。真正「打印出来对不对」只能在桌面应用里手验。 */
  it("建一个 iframe 再清理掉", async () => {
    const before = document.querySelectorAll("iframe").length;
    const promise = printViaIframe("<!doctype html><html><body>x</body></html>");
    // srcdoc 的 load 在 jsdom 里是异步的,手动触发。
    const frame = document.querySelector("iframe");
    expect(frame).not.toBeNull();
    frame?.dispatchEvent(new Event("load"));
    await promise;
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(document.querySelectorAll("iframe").length).toBe(before);
  });

  it("iframe 对无障碍树隐藏", async () => {
    const promise = printViaIframe("<html></html>");
    const frame = document.querySelector("iframe");
    expect(frame?.getAttribute("aria-hidden")).toBe("true");
    frame?.dispatchEvent(new Event("load"));
    await promise;
  });

  it("print 抛异常时仍然 resolve 并清理 —— 没打印机不该让导出变成崩溃", async () => {
    const promise = printViaIframe("<html></html>");
    const frame = document.querySelector("iframe") as HTMLIFrameElement;
    // contentWindow 在 jsdom 里存在,给它一个会抛的 print。
    Object.defineProperty(frame.contentWindow, "print", {
      configurable: true,
      value: () => {
        throw new Error("no printer");
      },
    });
    frame.dispatchEvent(new Event("load"));
    await expect(promise).resolves.toBeUndefined();
  });
});
