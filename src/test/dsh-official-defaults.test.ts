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

  it("mirrors the row count of the upstream base+web-app composition", () => {
    // 上游 c291e7961a:base 的单个 insert 铺 84 行,web-app 再 insert 68 行,
    // 其余 26 个 web-app 条目都是按 id 覆盖。数目变了说明上游增删过行,清单要重生成。
    expect(OFFICIAL_DSH_WEB_PLUGINS).toHaveLength(152);
  });

  it.each([
    // base 层新增(DeepSeek 协议扩展、会话日志、包清单、独立的 http fetch 后端)
    ["deepseek-llm-api-extensions", "@deepseek-ai/dsh-deepseek-llm-api-extensions"],
    ["session-log-deepseek", "@deepseek-ai/dsh-session-log-deepseek"],
    ["plugin-package-inventory-deepseek", "@deepseek-ai/dsh-plugin-package-inventory-deepseek"],
    ["web-fetch-http", "@deepseek-ai/dsh-web-fetch-http"],
    // web-app 层新增:controller 族、上传、资源模型与新的 ui 行
    [
      "subagent-model-selection-settings",
      "@deepseek-ai/dsh-tool-subagent/model-selection-settings",
    ],
    ["open-in-app", "@deepseek-ai/dsh-host-open-in-app"],
    ["ui-open-in-app", "@deepseek-ai/dsh-client-ui-open-in-app"],
    ["session-turn-outline", "@deepseek-ai/dsh-session-turn-outline"],
    ["session-controller", "@deepseek-ai/dsh-api-session-controller"],
    ["workspace-files", "@deepseek-ai/dsh-api-workspace-files"],
    ["settings-controller", "@deepseek-ai/dsh-api-settings-controller"],
    ["workspace-controller", "@deepseek-ai/dsh-api-workspace-controller"],
    ["file-upload", "@deepseek-ai/dsh-client-file-upload"],
    ["ui-session", "@deepseek-ai/dsh-client-ui-session"],
    ["resources", "@deepseek-ai/dsh-client-resources"],
    ["ui-sidebar-right", "@deepseek-ai/dsh-client-ui-sidebar-right"],
    ["ui-sidebar-documentpreview", "@deepseek-ai/dsh-client-ui-sidebar-documentpreview"],
    ["ui-sidebar-files", "@deepseek-ai/dsh-client-ui-sidebar-files"],
    ["ui-approval", "@deepseek-ai/dsh-client-ui-approval"],
    ["ui-chat", "@deepseek-ai/dsh-client-ui-chat"],
  ])("carries the upstream-added %s row enabled by default", (entryId, moduleName) => {
    const plugin = entry(entryId);
    expect(plugin).toBeDefined();
    expect(plugin?.moduleName).toBe(moduleName);
    expect(plugin?.enabled).toBe(true);
  });

  it.each([
    // 上游删掉的行。留着会让面板显示已经不存在的插件,并把它算进"官方已知"集合。
    "tool-subagent-report",
    "tool-str-replace-editor",
    "api-gateway",
    "client-runtime",
  ])("drops the %s row that upstream removed", (entryId) => {
    expect(entry(entryId)).toBeUndefined();
  });

  it("keeps command-goal disabled now that the Web surface moved it behind presets", () => {
    // 唯一一处默认态翻转:web-app 层显式 `disabled: true`(命令归 agent preset)。
    // 标成启用会让面板把一个实际没挂载的行显示为 active。
    expect(entry("command-goal")?.enabled).toBe(false);
    expect(entry("command-goal")?.fiberPhase).toBe(null);
    // 同层一起被 preset 接管的行,默认态与之一致。
    expect(entry("tool-goal")?.enabled).toBe(false);
    expect(entry("tool-web")?.enabled).toBe(false);
    // goal 服务与会话驱动仍留在 host plane。
    expect(entry("goal")?.enabled).toBe(true);
    expect(entry("goal-round-driver")?.enabled).toBe(true);
  });

  it("keeps the upstream baseline for rows Aeroric overrides at runtime", () => {
    // 上游基线是启用(telemetry 只在用户反馈时上报);Aeroric 用 dsh_home.rs 的
    // 受管 patch 单独把它压成 disabled,清单不能替那层做决定。
    expect(entry("session-telemetry-otel")?.enabled).toBe(true);
  });

  it.each([
    // macOS/Linux 上按 `process.platform` 表达式求值的行。
    ["bash-sandbox", true],
    ["pwsh-sandbox", false],
    // 上游自带 `disabled: true` 的行。
    ["hmr", false],
    ["skill-badge", false],
    ["ui-schedule", false],
  ])("resolves the %s row default for the macOS/Linux Web profile", (entryId, enabled) => {
    expect(entry(entryId)?.enabled).toBe(enabled);
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
