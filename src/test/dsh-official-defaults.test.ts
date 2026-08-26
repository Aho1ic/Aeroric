import { describe, expect, it } from "vitest";
import {
  OFFICIAL_DSH_WEB_PLUGINS,
  mergeDshPluginInventory,
  type DshPluginInventoryEntry,
} from "../dshOfficialDefaults";

/**
 * 清单镜像自 deepseek-harness 的 `packages/bundle/{base,web-app}/cordis.patch.yml`。
 * 缺项会让插件面板少显示对应行,并把实际已启用的插件标成未知,所以上游新增的
 * 条目要有测试兜住。
 */

function entry(entryId: string): DshPluginInventoryEntry | undefined {
  return OFFICIAL_DSH_WEB_PLUGINS.find((candidate) => candidate.entryId === entryId);
}

function indexOf(entryId: string): number {
  return OFFICIAL_DSH_WEB_PLUGINS.findIndex((candidate) => candidate.entryId === entryId);
}

describe("official dsh web plugin manifest", () => {
  it("parses every line into a well-formed entry", () => {
    expect(OFFICIAL_DSH_WEB_PLUGINS.length).toBeGreaterThan(0);
    for (const plugin of OFFICIAL_DSH_WEB_PLUGINS) {
      expect(plugin.entryId).toBeTruthy();
      expect(plugin.moduleName.startsWith("@deepseek-ai/")).toBe(true);
      expect(plugin.builtIn).toBe(true);
      expect(plugin.fiberPhase).toBe(plugin.enabled ? "active" : null);
    }
  });

  it("has no duplicate entry ids", () => {
    const ids = OFFICIAL_DSH_WEB_PLUGINS.map((plugin) => plugin.entryId);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it.each([
    ["session-reference", "@deepseek-ai/dsh-session-reference"],
    ["file-reference-local", "@deepseek-ai/dsh-file-reference-local"],
    ["ui-renderer", "@deepseek-ai/dsh-client-ui-renderer"],
    ["ui-brand-official", "@deepseek-ai/dsh-client-ui-brand-official"],
    ["ui-attachment", "@deepseek-ai/dsh-client-ui-attachment"],
    ["ui-reference", "@deepseek-ai/dsh-client-ui-reference"],
  ])("carries the %s row enabled by default", (entryId, moduleName) => {
    const plugin = entry(entryId);
    expect(plugin).toBeDefined();
    expect(plugin?.moduleName).toBe(moduleName);
    expect(plugin?.enabled).toBe(true);
  });

  it("keeps the upstream ordering of the reference and brand rows", () => {
    // 顺序对应上游 web-app/cordis.patch.yml 的插入位置;面板按此顺序渲染。
    expect(indexOf("session-projection-cache")).toBeLessThan(indexOf("session-reference"));
    expect(indexOf("session-reference")).toBeLessThan(indexOf("file-reference-local"));
    expect(indexOf("file-reference-local")).toBeLessThan(indexOf("session-stats"));

    expect(indexOf("ui-layout")).toBeLessThan(indexOf("ui-renderer"));
    expect(indexOf("ui-renderer")).toBeLessThan(indexOf("ui-sidebar"));

    expect(indexOf("ui-conversation")).toBeLessThan(indexOf("ui-brand-official"));
    expect(indexOf("ui-brand-official")).toBeLessThan(indexOf("ui-attachment"));
    expect(indexOf("ui-attachment")).toBeLessThan(indexOf("ui-tool"));

    expect(indexOf("ui-subagent")).toBeLessThan(indexOf("ui-reference"));
    expect(indexOf("ui-reference")).toBeLessThan(indexOf("ui-jobs"));
  });

  it("lets a loaded entry override the bundled default", () => {
    const merged = mergeDshPluginInventory([
      {
        entryId: "ui-reference",
        moduleName: "@deepseek-ai/dsh-client-ui-reference",
        enabled: false,
        fiberPhase: null,
        builtIn: true,
      },
    ]);
    expect(merged.find((plugin) => plugin.entryId === "ui-reference")?.enabled).toBe(false);
    // 未被覆盖的行仍保留官方默认。
    expect(merged.find((plugin) => plugin.entryId === "ui-attachment")?.enabled).toBe(true);
  });
});
