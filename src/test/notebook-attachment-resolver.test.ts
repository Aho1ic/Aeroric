/* 附件 URL 的 blob 生命周期。
 *
 * 这一层的两个失败模式都不会报错:blob 漏了(翻几十篇图多的笔记攒下几百个,只有
 * 页面卸载才会被收走),或者 blob 被提前 revoke(图变成空白框)。都得靠断言 revoke
 * 的调用来锁。
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AttachmentUrlResolver } from "../components/notebook/attachmentUrls";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

describe("AttachmentUrlResolver", () => {
  let created: string[];
  let revoked: string[];

  beforeEach(() => {
    created = [];
    revoked = [];
    let counter = 0;
    vi.spyOn(URL, "createObjectURL").mockImplementation(() => {
      const url = `blob:mock-${(counter += 1)}`;
      created.push(url);
      return url;
    });
    vi.spyOn(URL, "revokeObjectURL").mockImplementation((url: string) => {
      revoked.push(url);
    });
    invoke.mockReset();
    invoke.mockResolvedValue(new Uint8Array([1, 2, 3]).buffer);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("绝对地址原样返回,不读盘", async () => {
    const resolver = new AttachmentUrlResolver();
    await expect(resolver.resolve("https://example.com/x.png", "/vault")).resolves.toBe(
      "https://example.com/x.png",
    );
    expect(invoke).not.toHaveBeenCalled();
    expect(created).toEqual([]);
  });

  it("相对地址按笔记目录解析成绝对路径后再读", async () => {
    const resolver = new AttachmentUrlResolver();
    await resolver.resolve("../attachments/x.png", "/vault/a");

    // 传下去的必须是**规范化过的绝对路径**:带 `..` 的话后端那道 allowlist 会
    // 直接拒,而这张图其实是合法的。
    expect(invoke).toHaveBeenCalledWith("notebook_attachment_read", {
      path: "/vault/attachments/x.png",
    });
  });

  it("地址两头的空白不带进文件名", async () => {
    const resolver = new AttachmentUrlResolver();
    await resolver.resolve("  attachments/x.png\n", "/vault");

    // markdown 的 `![a]( x.png )` 会把空格留在 URL 里。不剪掉的话后端拿到的是
    // 一个名字带空格的文件,读不出来 —— 而页面上只是一张空白图。
    expect(invoke).toHaveBeenCalledWith("notebook_attachment_read", {
      path: "/vault/attachments/x.png",
    });
  });

  it("同一张图只读一次", async () => {
    const resolver = new AttachmentUrlResolver();
    const [first, second] = await Promise.all([
      resolver.resolve("attachments/x.png", "/vault"),
      resolver.resolve("attachments/x.png", "/vault"),
    ]);

    // 一篇笔记里同一张图出现十次是常事(比如重复的图标)。不缓存的话十次 IPC。
    expect(first).toBe(second);
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("链接里的 %20 还原成空格", async () => {
    const resolver = new AttachmentUrlResolver();
    await resolver.resolve("attachments/my%20shot.png", "/vault");

    expect(invoke).toHaveBeenCalledWith("notebook_attachment_read", {
      path: "/vault/attachments/my shot.png",
    });
  });

  it("孤立的 % 不让整张图消失", async () => {
    const resolver = new AttachmentUrlResolver();
    // `decodeURIComponent("100%.png")` 会抛。让它抛出去的话文件名里带 `%` 的图
    // 就永远显示不出来,而这是个合法的文件名。
    await resolver.resolve("attachments/100%.png", "/vault");

    expect(invoke).toHaveBeenCalledWith("notebook_attachment_read", {
      path: "/vault/attachments/100%.png",
    });
  });

  it("release 回收所有 blob", async () => {
    const resolver = new AttachmentUrlResolver();
    await resolver.resolve("attachments/a.png", "/vault");
    await resolver.resolve("attachments/b.png", "/vault");
    expect(created).toHaveLength(2);

    resolver.release();

    expect(revoked.sort()).toEqual(created.sort());
  });

  it("release 之后重新解析会再读一次", async () => {
    const resolver = new AttachmentUrlResolver();
    await resolver.resolve("attachments/a.png", "/vault");
    resolver.release();
    await resolver.resolve("attachments/a.png", "/vault");

    // 缓存必须跟着 release 一起清:留着的话第二次会拿到一个已经 revoke 的 URL,
    // 图显示成空白而且没有任何报错。
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(created).toHaveLength(2);
  });

  it("release 之后才落地的读取自己回收,不把悬空 URL 交出去", async () => {
    const resolver = new AttachmentUrlResolver();
    let settle: ((value: ArrayBuffer) => void) | null = null;
    invoke.mockImplementationOnce(
      () =>
        new Promise<ArrayBuffer>((resolve) => {
          settle = resolve;
        }),
    );
    const pending = resolver.resolve("attachments/slow.png", "/vault");

    resolver.release();
    settle!(new Uint8Array([1]).buffer);

    // 交出去的话调用方会把它塞进 `<img src>`,而它已经不指向任何数据了。
    await expect(pending).rejects.toThrow(/released/);
    expect(revoked).toEqual(created);
  });

  it("读失败的不进缓存,重试还能成功", async () => {
    const resolver = new AttachmentUrlResolver();
    invoke.mockRejectedValueOnce(new Error("disk went away"));

    await expect(resolver.resolve("attachments/x.png", "/vault")).rejects.toThrow("disk went away");
    // 缓存住失败的话这张图永远显示不出来,连重新打开笔记都救不回 —— 而失败往往
    // 是暂时的(文件刚好在被写)。
    await expect(resolver.resolve("attachments/x.png", "/vault")).resolves.toMatch(/^blob:/);
  });

  it("按扩展名给 blob 定 MIME", async () => {
    const resolver = new AttachmentUrlResolver();
    const types: string[] = [];
    // 形参得是 `Blob | MediaSource`(DOM 的真实签名),不然 tsc 不认这个 mock。
    vi.spyOn(URL, "createObjectURL").mockImplementation((blob: Blob | MediaSource) => {
      types.push(blob instanceof Blob ? blob.type : "");
      return `blob:typed-${types.length}`;
    });

    await resolver.resolve("attachments/x.svg", "/vault");
    await resolver.resolve("attachments/x.png", "/vault");

    // SVG 尤其重要:不带 `image/svg+xml` 的 blob 会被当成下载而不是图片。
    expect(types).toEqual(["image/svg+xml", "image/png"]);
  });
});
