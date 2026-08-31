/* 导出时把本地图内联成 data URL。
 *
 * 为什么这层必须存在:阅读态的图是 blob URL(作用域限于当前文档),写进导出文件之后
 * 立刻是死链。Markio 在后端渲染时内联,Aeroric 的渲染在前端,所以这一步得自己做。
 */

import { describe, expect, it, vi } from "vitest";
import {
  bytesToBase64,
  inlineLocalImages,
  mimeFromPath,
} from "../components/notebook/noteExportImages";

/** 造一个装着 `html` 的 container。 */
function host(html: string): HTMLElement {
  const el = document.createElement("div");
  el.innerHTML = html;
  return el;
}

/** 一串固定字节,用来核对 base64 的落点。 */
function bytes(...values: number[]): ArrayBuffer {
  return new Uint8Array(values).buffer;
}

describe("MIME 推断", () => {
  it("按扩展名给出常见图片类型", () => {
    expect(mimeFromPath("/v/a.png")).toBe("image/png");
    expect(mimeFromPath("/v/a.JPG")).toBe("image/jpeg");
    expect(mimeFromPath("/v/a.jpeg")).toBe("image/jpeg");
    expect(mimeFromPath("/v/a.webp")).toBe("image/webp");
  });

  it("SVG 必须报对 —— 不带 image/svg+xml 的 data URL 不会被当成图片渲染", () => {
    expect(mimeFromPath("/v/a.svg")).toBe("image/svg+xml");
  });

  it("认不出的扩展名给 octet-stream", () => {
    expect(mimeFromPath("/v/a.xyz")).toBe("application/octet-stream");
    expect(mimeFromPath("/v/noext")).toBe("application/octet-stream");
  });

  it("查询串和锚点不参与扩展名判断", () => {
    expect(mimeFromPath("/v/a.png?v=2")).toBe("image/png");
    expect(mimeFromPath("/v/a.png#frag")).toBe("image/png");
  });
});

describe("base64 编码", () => {
  it("和 btoa 对齐", () => {
    // "hi" = 0x68 0x69
    expect(bytesToBase64(bytes(0x68, 0x69))).toBe(btoa("hi"));
  });

  it("空输入给空串", () => {
    expect(bytesToBase64(new ArrayBuffer(0))).toBe("");
  });

  /* 分块的理由:一次性 `fromCharCode(...bytes)` 在几百 KB 上会撑爆调用栈,而图片正好
     在那个量级。这条测试跨过 0x8000 的块边界,钉住拼接顺序没错。 */
  it("跨块边界的长输入编码正确", () => {
    const size = 0x8000 * 2 + 5;
    const data = new Uint8Array(size);
    for (let i = 0; i < size; i += 1) data[i] = i % 256;
    const encoded = bytesToBase64(data.buffer);
    // 解回来必须逐字节相同 —— 顺序错了或者块之间丢了字节,这里就对不上。
    const decoded = atob(encoded);
    expect(decoded).toHaveLength(size);
    expect(decoded.charCodeAt(0)).toBe(0);
    expect(decoded.charCodeAt(size - 1)).toBe((size - 1) % 256);
    expect(decoded.charCodeAt(0x8000)).toBe(0x8000 % 256);
  });
});

describe("本地图内联", () => {
  it("相对路径的图换成 data URL", async () => {
    const el = host(`<img src="attachments/a.png" />`);
    const read = vi.fn().mockResolvedValue(bytes(0x68, 0x69));
    const result = await inlineLocalImages(el, "/v/sub", read);
    expect(read).toHaveBeenCalledWith("/v/sub/attachments/a.png");
    expect(el.querySelector("img")?.getAttribute("src")).toBe(
      `data:image/png;base64,${btoa("hi")}`,
    );
    expect(result).toEqual({ inlined: 1, skipped: 0 });
  });

  it("`..` 在拼路径时折叠掉 —— 带 `..` 的路径会被后端那道闸门直接拒", async () => {
    const el = host(`<img src="../shared/a.png" />`);
    const read = vi.fn().mockResolvedValue(bytes(1));
    await inlineLocalImages(el, "/v/sub", read);
    expect(read).toHaveBeenCalledWith("/v/shared/a.png");
  });

  it("URL 编码的空格还原成真空格", async () => {
    const el = host(`<img src="attachments/my%20pic.png" />`);
    const read = vi.fn().mockResolvedValue(bytes(1));
    await inlineLocalImages(el, "/v", read);
    expect(read).toHaveBeenCalledWith("/v/attachments/my pic.png");
  });

  it("孤立的 % 不让整张图丢掉", async () => {
    // decodeURIComponent 遇到它会抛;那时候要原样送下去,而不是放弃这张图。
    const el = host(`<img src="attachments/100%.png" />`);
    const read = vi.fn().mockResolvedValue(bytes(1));
    const result = await inlineLocalImages(el, "/v", read);
    expect(read).toHaveBeenCalledWith("/v/attachments/100%.png");
    expect(result.inlined).toBe(1);
  });

  it("远端图保持原样,不发请求", async () => {
    const el = host(`<img src="https://example.com/a.png" />`);
    const read = vi.fn();
    const result = await inlineLocalImages(el, "/v", read);
    expect(read).not.toHaveBeenCalled();
    expect(el.querySelector("img")?.getAttribute("src")).toBe("https://example.com/a.png");
    expect(result).toEqual({ inlined: 0, skipped: 1 });
  });

  it("已经是 data URL 的不重复处理", async () => {
    const el = host(`<img src="data:image/png;base64,AAA" />`);
    const read = vi.fn();
    const result = await inlineLocalImages(el, "/v", read);
    expect(read).not.toHaveBeenCalled();
    expect(result.skipped).toBe(1);
  });

  it("读失败的图留原路径,不让整次导出失败", async () => {
    const el = host(`<img src="missing.png" />`);
    const read = vi.fn().mockRejectedValue(new Error("nope"));
    const result = await inlineLocalImages(el, "/v", read);
    expect(el.querySelector("img")?.getAttribute("src")).toBe("missing.png");
    expect(result).toEqual({ inlined: 0, skipped: 1 });
  });

  it("超过上限的图不内联 —— 一个几十 MB 的 data URL 会让导出文件打不开", async () => {
    const el = host(`<img src="huge.png" />`);
    const read = vi.fn().mockResolvedValue(new ArrayBuffer(8 * 1024 * 1024 + 1));
    const result = await inlineLocalImages(el, "/v", read);
    expect(el.querySelector("img")?.getAttribute("src")).toBe("huge.png");
    expect(result).toEqual({ inlined: 0, skipped: 1 });
  });

  it("刚好等于上限的图仍然内联", async () => {
    const el = host(`<img src="big.png" />`);
    const read = vi.fn().mockResolvedValue(new ArrayBuffer(8 * 1024 * 1024));
    const result = await inlineLocalImages(el, "/v", read);
    expect(result.inlined).toBe(1);
  });

  it("同一张图出现多次只读一次", async () => {
    const el = host(`<img src="a.png" /><img src="a.png" /><img src="a.png" />`);
    const read = vi.fn().mockResolvedValue(bytes(1));
    const result = await inlineLocalImages(el, "/v", read);
    expect(read).toHaveBeenCalledTimes(1);
    expect(result.inlined).toBe(3);
    // 三张都换掉了,不是只换第一张。
    for (const img of Array.from(el.querySelectorAll("img"))) {
      expect(img.getAttribute("src")?.startsWith("data:")).toBe(true);
    }
  });

  it("读失败的结果也进缓存,不重试到超时", async () => {
    const el = host(`<img src="missing.png" /><img src="missing.png" />`);
    const read = vi.fn().mockRejectedValue(new Error("nope"));
    await inlineLocalImages(el, "/v", read);
    expect(read).toHaveBeenCalledTimes(1);
  });

  it("混合场景各自归位", async () => {
    const el = host(
      `<img src="ok.png" />` +
        `<img src="https://example.com/remote.png" />` +
        `<img src="bad.png" />`,
    );
    const read = vi.fn(async (path: string) => {
      if (path.endsWith("bad.png")) throw new Error("nope");
      return bytes(0x68, 0x69);
    });
    const result = await inlineLocalImages(el, "/v", read);
    expect(result).toEqual({ inlined: 1, skipped: 2 });
  });

  it("没有图时不报错", async () => {
    const el = host(`<p>只有文字</p>`);
    const result = await inlineLocalImages(el, "/v", vi.fn());
    expect(result).toEqual({ inlined: 0, skipped: 0 });
  });
});
