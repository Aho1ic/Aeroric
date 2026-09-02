import { describe, it, expect } from "vitest";

import { fileNameFromPath } from "../lib/filePath";

/**
 * 这个文件有两半:
 *
 * 前半钉 `fileNameFromPath` 自己的行为(它替换了 5 处逐字节相同的
 * `path.split(/[\\/]/).pop() ?? path`)。
 *
 * 后半钉**仓库里另外两种写法与它的差异**。那两种没有被合并进来,理由写在
 * `src/lib/filePath.ts` 的文件头。这里把差异做成断言,是为了让「再合并一步」
 * 这个动作先撞到红灯 —— 它们在边界上确实不等价,不是随手就能统一的。
 */
describe("fileNameFromPath", () => {
  it("取 POSIX 路径的最后一段", () => {
    expect(fileNameFromPath("/repo/src/App.tsx")).toBe("App.tsx");
  });

  it("同样认 Windows 的反斜杠", () => {
    expect(fileNameFromPath("C:\\repo\\src\\App.tsx")).toBe("App.tsx");
  });

  it("认混用的分隔符 —— WSL / SSH 路径拼接后真的会这样", () => {
    expect(fileNameFromPath("C:\\repo/src\\App.tsx")).toBe("App.tsx");
  });

  it("没有分隔符时原样返回", () => {
    expect(fileNameFromPath("App.tsx")).toBe("App.tsx");
  });

  it("空串返回空串", () => {
    expect(fileNameFromPath("")).toBe("");
  });

  it("保留文件名里的点和空格", () => {
    expect(fileNameFromPath("/repo/my file.test.tsx")).toBe("my file.test.tsx");
  });

  it("尾随分隔符会得到空串 —— 这是被替换掉那 5 处的原样行为", () => {
    // 调用点的路径都来自后端给的诊断 / 测试失败 / LSP location,一律是具体文件,
    // 不会带尾随分隔符。所以这个边界够不着;钉住它是为了防止有人"顺手修一下"
    // 就改成了 `.filter(Boolean)`,那会让下面两组的语义悄悄流进来。
    expect(fileNameFromPath("/repo/src/")).toBe("");
  });

  it("只有分隔符时也是空串", () => {
    expect(fileNameFromPath("/")).toBe("");
  });
});

describe("仓库里另外两种写法与它的差异(刻意未合并)", () => {
  // run/runConfigState.ts 与 debug/debugState.ts 用的是这一种
  const withFilterBoolean = (path: string): string =>
    path.split(/[\\/]/).filter(Boolean).pop() ?? path;

  // command-palette/CommandPalette.tsx 用的是这一种
  const withOrFallback = (path: string): string => path.split(/[\\/]/).pop() || path;

  it("`.filter(Boolean)` 版能容忍尾随斜杠,本函数不能", () => {
    expect(withFilterBoolean("/repo/src/")).toBe("src");
    expect(fileNameFromPath("/repo/src/")).toBe("");
  });

  it("`.filter(Boolean)` 版在纯分隔符上回落到原串,本函数给空串", () => {
    expect(withFilterBoolean("/")).toBe("/");
    expect(fileNameFromPath("/")).toBe("");
  });

  it("`||` 版把空结果兜回原串,`??` 挡不住空串", () => {
    expect(withOrFallback("/repo/src/")).toBe("/repo/src/");
    expect(fileNameFromPath("/repo/src/")).toBe("");
  });

  it("三种写法在正常文件路径上完全一致 —— 差异只在边界上", () => {
    for (const path of ["/repo/src/App.tsx", "C:\\repo\\App.tsx", "App.tsx"]) {
      expect(fileNameFromPath(path)).toBe(withFilterBoolean(path));
      expect(fileNameFromPath(path)).toBe(withOrFallback(path));
    }
  });
});
