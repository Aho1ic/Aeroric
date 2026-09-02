/* 导入的编排:provider 清单 → 选源 → 调命令 → 报告读成文案。
 *
 * 外部通道(两个文件对话框、七个后端命令)全部注入,所以这里不 mock 模块。
 *
 * 这一组测试盯的是三处**会静默出错**的地方,不是「函数被调到了」:
 *
 * 1. 每个 provider 的源形态不同。给 Obsidian 弹文件选择器、给 Notion 弹目录选择器
 *    都不会报错 —— 用户选完之后后端才失败,而报出来的错和真正的原因无关。
 * 2. 取消不是失败。混成一档会让「用户按了 Esc」显示成一条错误。
 * 3. `resourceLost` / `degraded` 是跨状态计数,不参与 imported/skipped/failed 的划分。
 *    加在一起会得出比真实条目数还大的总数。
 */

import { describe, expect, it, vi } from "vitest";
import {
  availableImportProviders,
  importIssueText,
  importStatusText,
  importSummary,
  isProviderAvailable,
  runImport,
  runProviderCommand,
  IMPORT_PROVIDERS,
  type ImportDeps,
  type ImportProviderId,
} from "../components/notebook/noteImport";
import type { ImportReport } from "../components/notebook/notebookApi";

/** 文案不进断言 —— 这里只关心 key 和插值,不关心中英。 */
const t = (key: string, vars?: Record<string, string>) =>
  vars ? `${key}(${JSON.stringify(vars)})` : key;

function report(overrides: Partial<ImportReport> = {}): ImportReport {
  return {
    provider: "obsidian",
    dest: "imports/obsidian",
    imported: 0,
    skipped: 0,
    failed: 0,
    resourceLost: 0,
    degraded: 0,
    items: [],
    truncated: 0,
    reportPath: "imports/obsidian/导入报告 · obsidian.md",
    ...overrides,
  };
}

function deps(overrides: Partial<ImportDeps> = {}): ImportDeps {
  return {
    pickDirectory: vi.fn().mockResolvedValue("/src/vault"),
    pickFile: vi.fn().mockResolvedValue("/src/export.zip"),
    run: vi.fn().mockResolvedValue(report()),
    ...overrides,
  };
}

function providerBy(id: ImportProviderId) {
  const found = IMPORT_PROVIDERS.find((provider) => provider.id === id);
  if (!found) throw new Error(`no provider ${id}`);
  return found;
}

describe("provider 清单", () => {
  it("目录型弹目录选择器,文件型弹文件选择器 —— 反了的话后端才失败,而且报错和原因无关", async () => {
    for (const id of ["obsidian", "logseq"] as const) {
      const d = deps();
      await runImport(providerBy(id), "/vault", d, t);
      expect(d.pickDirectory, id).toHaveBeenCalledTimes(1);
      expect(d.pickFile, id).not.toHaveBeenCalled();
    }

    for (const id of ["notion", "bear", "roam", "evernote"] as const) {
      const d = deps();
      await runImport(providerBy(id), "/vault", d, t);
      expect(d.pickFile, id).toHaveBeenCalledTimes(1);
      expect(d.pickDirectory, id).not.toHaveBeenCalled();
    }
  });

  it("Roam 收 zip 不是 json —— 两种导出都打在 zip 里,过滤器写 json 会让用户选不到自己的文件", async () => {
    const d = deps();
    await runImport(providerBy("roam"), "/vault", d, t);
    const [, extensions] = vi.mocked(d.pickFile).mock.calls[0];
    expect(extensions).toEqual(["zip"]);
  });

  it("Evernote 过滤 .enex", async () => {
    const d = deps();
    await runImport(providerBy("evernote"), "/vault", d, t);
    const [, extensions] = vi.mocked(d.pickFile).mock.calls[0];
    expect(extensions).toEqual(["enex"]);
  });

  it("Apple 备忘录不弹任何对话框 —— 它没有源路径,走 osascript 问 Notes.app", async () => {
    const d = deps();
    const outcome = await runImport(providerBy("appleNotes"), "/vault", d, t);
    expect(d.pickDirectory).not.toHaveBeenCalled();
    expect(d.pickFile).not.toHaveBeenCalled();
    expect(outcome.status).toBe("done");
    // sourcePath 必须是 null 而不是空字符串:空串会被后端当成相对路径去解析。
    expect(d.run).toHaveBeenCalledWith("appleNotes", "/vault", null);
  });

  it("Apple 备忘录只在 macOS 列出 —— 别的平台后端直接返回错误,画出来就是个必然失败的按钮", () => {
    const apple = providerBy("appleNotes");
    expect(isProviderAvailable(apple, "macos")).toBe(true);
    expect(isProviderAvailable(apple, "windows")).toBe(false);
    expect(isProviderAvailable(apple, "other")).toBe(false);

    expect(availableImportProviders("macos").map((p) => p.id)).toContain("appleNotes");
    expect(availableImportProviders("windows").map((p) => p.id)).not.toContain("appleNotes");
  });

  it("除 Apple 之外的 provider 在每个平台都在 —— 平台门控只该挡住系统型那一个", () => {
    for (const platform of ["macos", "windows", "other"] as const) {
      const ids = availableImportProviders(platform).map((provider) => provider.id);
      for (const id of ["obsidian", "logseq", "notion", "bear", "roam", "evernote"] as const) {
        expect(ids, platform).toContain(id);
      }
    }
  });

  it("每个 provider 的 label / hint key 都不同 —— 复制粘贴漏改会让两行显示同一段文案", () => {
    const labels = IMPORT_PROVIDERS.map((provider) => provider.labelKey);
    const hints = IMPORT_PROVIDERS.map((provider) => provider.hintKey);
    expect(new Set(labels).size).toBe(IMPORT_PROVIDERS.length);
    expect(new Set(hints).size).toBe(IMPORT_PROVIDERS.length);
  });
});

describe("取消", () => {
  it("目录选择器取消 → cancelled,而且不调后端", async () => {
    const d = deps({ pickDirectory: vi.fn().mockResolvedValue(null) });
    const outcome = await runImport(providerBy("obsidian"), "/vault", d, t);
    expect(outcome).toEqual({ status: "cancelled" });
    expect(d.run).not.toHaveBeenCalled();
  });

  it("文件选择器取消 → cancelled,而且不调后端", async () => {
    const d = deps({ pickFile: vi.fn().mockResolvedValue(null) });
    const outcome = await runImport(providerBy("notion"), "/vault", d, t);
    expect(outcome).toEqual({ status: "cancelled" });
    expect(d.run).not.toHaveBeenCalled();
  });
});

describe("失败", () => {
  it("后端报错 → failed,带上原文", async () => {
    const d = deps({ run: vi.fn().mockRejectedValue("打开 roam zip 失败:文件不存在") });
    const outcome = await runImport(providerBy("roam"), "/vault", d, t);
    expect(outcome).toEqual({
      status: "failed",
      message: "打开 roam zip 失败:文件不存在",
    });
  });

  it("对话框本身抛也算 failed —— 不能静默当成取消,那样用户按了按钮却什么都没发生", async () => {
    const d = deps({
      pickDirectory: vi.fn().mockRejectedValue(new Error("dialog plugin missing")),
    });
    const outcome = await runImport(providerBy("obsidian"), "/vault", d, t);
    expect(outcome).toEqual({ status: "failed", message: "dialog plugin missing" });
    expect(d.run).not.toHaveBeenCalled();
  });
});

describe("命令分派", () => {
  it("非 Apple 的 provider 拿到 null 源路径要抛 —— 退化成空字符串会让后端把它当相对路径,报出的错和真正的原因无关", () => {
    for (const id of ["obsidian", "logseq", "notion", "bear", "roam", "evernote"] as const) {
      expect(() => runProviderCommand(id, "/vault", null), id).toThrow(/requires a source path/);
    }
  });
});

describe("报告摘要", () => {
  it("三个划分计数总是显示", () => {
    const text = importSummary(report({ imported: 12, skipped: 3, failed: 1 }), t);
    expect(text).toContain("notebook.importSummaryCounts");
    expect(text).toContain('"imported":"12"');
    expect(text).toContain('"skipped":"3"');
    expect(text).toContain('"failed":"1"');
  });

  it("resourceLost / degraded 为零时不出现 —— 「0 条附件丢失」是噪音", () => {
    const text = importSummary(report({ imported: 5 }), t);
    expect(text).not.toContain("ResourceLost");
    expect(text).not.toContain("Degraded");
    expect(text).not.toContain("Truncated");
  });

  it("resourceLost 各自成句,不加进三个划分计数里 —— 它是跨状态的受影响条目数", () => {
    /* 一条 imported 同时带资源丢失:2 条全部导入成功,其中 1 条丢了图。
       如果实现把 resourceLost 加进总数,摘要会说出「3 条」这个不存在的数字。 */
    const text = importSummary(report({ imported: 2, resourceLost: 1 }), t);
    expect(text).toContain('"imported":"2"');
    expect(text).toContain("notebook.importSummaryResourceLost");
    expect(text).toContain('"count":"1"');
    // imported 那句里的数字必须还是 2,没被 resourceLost 抬高。
    expect(text).not.toContain('"imported":"3"');
  });

  it("degraded 同样各自成句", () => {
    const text = importSummary(report({ imported: 4, degraded: 2 }), t);
    expect(text).toContain("notebook.importSummaryDegraded");
    expect(text).toContain('"count":"2"');
  });

  it("明细截断时点明差额 —— 计数不封顶而明细封顶,不说的话用户以为只有这么多条", () => {
    const text = importSummary(report({ imported: 5000, truncated: 3000 }), t);
    expect(text).toContain("notebook.importSummaryTruncated");
    expect(text).toContain('"count":"3000"');
  });
});

describe("明细文案", () => {
  it("三档状态各自成文", () => {
    expect(importStatusText({ kind: "imported" }, t)).toBe("notebook.importStatusImported");
    expect(importStatusText({ kind: "failed", detail: "XML 截断" }, t)).toContain("XML 截断");
    expect(importStatusText({ kind: "skipped", reason: { kind: "alreadyImported" } }, t)).toContain(
      "notebook.importSkipAlreadyImported",
    );
  });

  it("六个跳过理由都有各自的文案 —— 「已经导过 300 条」和「格式不支持 300 条」对用户是两件事", () => {
    const reasons = [
      { kind: "alreadyImported" as const },
      { kind: "unsupported" as const, extension: "edn" },
      { kind: "tooLarge" as const, bytes: 99_000_000 },
      { kind: "limitReached" as const, limit: "total-bytes" },
      { kind: "unreadable" as const, detail: "备忘录被锁定" },
      { kind: "symlink" as const },
    ];
    const texts = reasons.map((reason) => importStatusText({ kind: "skipped", reason }, t));
    // 每一档都必须落到不同的 key 上,不能有两档共用一句。
    expect(new Set(texts).size).toBe(reasons.length);
    // 带参数的那几档要把参数带出来,否则用户看不到「大多少」「哪个上限」。
    expect(texts[1]).toContain("edn");
    expect(texts[2]).toContain("99000000");
    expect(texts[3]).toContain("total-bytes");
    expect(texts[4]).toContain("备忘录被锁定");
  });

  it("两类 issue 各自成文,而且带出目标 —— 用户要靠它回源端找那张图", () => {
    const lost = importIssueText(
      { kind: "resourceLost", target: "image.png", detail: "resource 里没有对应 hash" },
      t,
    );
    expect(lost).toContain("image.png");
    expect(lost).toContain("resource 里没有对应 hash");

    const degraded = importIssueText({ kind: "degraded", detail: ".org 未转换" }, t);
    expect(degraded).toContain(".org 未转换");
  });
});
