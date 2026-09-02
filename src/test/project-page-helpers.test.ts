import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";

import {
  AUXILIARY_LAYOUT_STORAGE_PREFIX,
  readAuxiliaryLayouts,
} from "../components/project-page/auxiliaryLayout";
import { mergeLspDiagnostics } from "../components/project-page/lspDiagnostics";
import type { DiagnosticItem } from "../types";

/**
 * 这两个函数原先写在 `ProjectPage.tsx` 的模块层(3000 行组件的顶部),
 * 只能靠渲染整个页面间接覆盖。搬到 `project-page/` 之后可以直接调,
 * 所以这里补上它们的直接用例 —— 尤其是那几条**坏数据兜底**路径:
 * 渲染整个页面时很难构造出「localStorage 里存着半坏的 JSON」这种状态。
 */

function diagnostic(over: Partial<DiagnosticItem> = {}): DiagnosticItem {
  return {
    file: "/repo/src/App.tsx",
    line: 1,
    column: 1,
    severity: "error",
    message: "boom",
    source: "lsp:typescript",
    ...over,
  };
}

describe("readAuxiliaryLayouts", () => {
  const projectId = "proj-1";
  const key = `${AUXILIARY_LAYOUT_STORAGE_PREFIX}${projectId}`;

  beforeEach(() => {
    window.localStorage.clear();
  });

  afterEach(() => {
    window.localStorage.clear();
    vi.restoreAllMocks();
  });

  it("没存过时三个工作区都是 split", () => {
    expect(readAuxiliaryLayouts(projectId)).toEqual({
      ssh: "split",
      file: "split",
      terminal: "split",
    });
  });

  it("读回存进去的 full", () => {
    window.localStorage.setItem(key, JSON.stringify({ ssh: "full", file: "full" }));
    expect(readAuxiliaryLayouts(projectId)).toEqual({
      ssh: "full",
      file: "full",
      terminal: "split",
    });
  });

  it("坏 JSON 整份回落到 split,不抛错", () => {
    window.localStorage.setItem(key, "{不是 JSON");
    expect(readAuxiliaryLayouts(projectId)).toEqual({
      ssh: "split",
      file: "split",
      terminal: "split",
    });
  });

  it("半坏的数据只让坏字段回落,好字段保住", () => {
    // 逐字段判 `=== "full"` 的价值就在这:整份 as-cast 会让 `file: 123` 原样漏到 UI 上。
    window.localStorage.setItem(key, JSON.stringify({ ssh: "full", file: 123, terminal: null }));
    expect(readAuxiliaryLayouts(projectId)).toEqual({
      ssh: "full",
      file: "split",
      terminal: "split",
    });
  });

  it("任何非 full 的字符串都算 split", () => {
    window.localStorage.setItem(key, JSON.stringify({ ssh: "FULL", file: "fullscreen" }));
    expect(readAuxiliaryLayouts(projectId)).toEqual({
      ssh: "split",
      file: "split",
      terminal: "split",
    });
  });

  it("按项目 id 隔离 —— 另一个项目的偏好读不到", () => {
    window.localStorage.setItem(key, JSON.stringify({ ssh: "full" }));
    expect(readAuxiliaryLayouts("proj-2").ssh).toBe("split");
  });

  it("localStorage.getItem 抛错时也回落而不是崩", () => {
    vi.spyOn(window.localStorage, "getItem").mockImplementation(() => {
      throw new Error("SecurityError");
    });
    expect(readAuxiliaryLayouts(projectId)).toEqual({
      ssh: "split",
      file: "split",
      terminal: "split",
    });
  });
});

describe("mergeLspDiagnostics", () => {
  it("同一个文件的旧 LSP 诊断被整批换掉", () => {
    const current = [
      diagnostic({ line: 1, message: "old-a" }),
      diagnostic({ line: 2, message: "old-b" }),
    ];
    const next = [diagnostic({ line: 9, message: "new" })];
    expect(mergeLspDiagnostics(current, "/repo/src/App.tsx", next)).toEqual(next);
  });

  it("别的文件的 LSP 诊断不动", () => {
    const other = diagnostic({ file: "/repo/src/Other.tsx", message: "keep" });
    const merged = mergeLspDiagnostics([other], "/repo/src/App.tsx", []);
    expect(merged).toEqual([other]);
  });

  it("同一个文件里非 lsp: 来源的诊断保住", () => {
    // 这是那个 `||` 的另一半:eslint / tsc / cargo 那些跑批产生的条目挂在同一个文件上,
    // 一次 LSP publish 不能把它们顺手清掉。
    const fromEslint = diagnostic({ source: "eslint", message: "keep-me" });
    const fromLsp = diagnostic({ source: "lsp:typescript", message: "replace-me" });
    const next = [diagnostic({ source: "lsp:typescript", message: "fresh" })];
    const merged = mergeLspDiagnostics([fromEslint, fromLsp], "/repo/src/App.tsx", next);
    expect(merged).toEqual([fromEslint, ...next]);
  });

  it("空的新诊断等于把这个文件的 LSP 条目清干净", () => {
    const fromLsp = diagnostic({ source: "lsp:rust-analyzer" });
    expect(mergeLspDiagnostics([fromLsp], "/repo/src/App.tsx", [])).toEqual([]);
  });

  it("前缀是按 startsWith 判的 —— `lsp` 不带冒号不算", () => {
    const notLsp = diagnostic({ source: "lsp", message: "keep" });
    const merged = mergeLspDiagnostics([notLsp], "/repo/src/App.tsx", []);
    expect(merged).toEqual([notLsp]);
  });

  it("新诊断排在保留项之后", () => {
    const kept = diagnostic({ source: "tsc", message: "kept" });
    const fresh = diagnostic({ source: "lsp:typescript", message: "fresh" });
    const merged = mergeLspDiagnostics([kept], "/repo/src/App.tsx", [fresh]);
    expect(merged.map((d) => d.message)).toEqual(["kept", "fresh"]);
  });

  it("不改传进来的数组(返回新数组)", () => {
    const current = [diagnostic({ source: "lsp:typescript" })];
    const snapshot = [...current];
    mergeLspDiagnostics(current, "/repo/src/App.tsx", [diagnostic({ source: "lsp:typescript" })]);
    expect(current).toEqual(snapshot);
  });
});
