import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { FLOATING_LAYERS, OVERLAY_LAYERS, zLayers } from "../styles/zLayers";
import s from "../styles";

// jsdom 环境下 import.meta.url 不是 file: URL,改从项目根解析。
// (CSS 在 Vitest 里默认被 stub 成空串,`?raw` 也拿不到内容,只能读文件。)
const readSource = (relative: string) => readFileSync(resolve(process.cwd(), relative), "utf8");

const appCss = readSource("src/App.css");
const fontSelectorCss = readSource("src/styles/font-selector.css");

/** App.css 里声明的 --z-* 变量 → 数值。 */
function cssZVariables(): Record<string, number> {
  const map: Record<string, number> = {};
  for (const [, name, value] of appCss.matchAll(/(--z-[a-z-]+):\s*(\d+);/g)) {
    map[name] = Number(value);
  }
  return map;
}

/** kebab-case 的 CSS 变量名 → zLayers 的 camelCase key。 */
function toCamel(name: string): string {
  return name
    .replace(/^--z-/, "")
    .replace(/-([a-z])/g, (_, letter: string) => letter.toUpperCase());
}

describe("z-index 层级量表", () => {
  it("瞬态浮层严格高于所有模态遮罩层", () => {
    const highestOverlay = Math.max(...OVERLAY_LAYERS.map((layer) => zLayers[layer]));
    for (const layer of FLOATING_LAYERS) {
      expect(zLayers[layer], `${layer} 必须高于所有遮罩层`).toBeGreaterThan(highestOverlay);
    }
  });

  it("遮罩层嵌套顺序单调递增", () => {
    expect(zLayers.overlay).toBeLessThan(zLayers.overlayNested);
    expect(zLayers.overlayNested).toBeLessThan(zLayers.overlayNestedDeep);
    // 应用内确认框可能从最深那层嵌套模态里弹出,必须盖在它之上。
    expect(zLayers.overlayNestedDeep).toBeLessThan(zLayers.overlayConfirm);
  });

  it("浮层内部顺序:二级浮层高于一级,右键菜单高于其捕获层", () => {
    expect(zLayers.popover).toBeLessThan(zLayers.popoverNested);
    expect(zLayers.contextMenuBackdrop).toBeLessThan(zLayers.contextMenu);
    expect(zLayers.contextMenu).toBeLessThan(zLayers.toast);
  });

  it("内联下拉低于遮罩层(它随容器裁剪,不该盖住模态)", () => {
    expect(zLayers.dropdownInline).toBeLessThan(zLayers.overlay);
    expect(zLayers.sticky).toBeLessThan(zLayers.dropdownInline);
  });

  it("App.css 的 --z-* 变量与 zLayers.ts 逐一对应", () => {
    const cssVars = cssZVariables();
    expect(Object.keys(cssVars).length).toBe(Object.keys(zLayers).length);
    for (const [name, value] of Object.entries(cssVars)) {
      const key = toCamel(name) as keyof typeof zLayers;
      expect(zLayers[key], `${name} 与 zLayers.${key} 不一致`).toBe(value);
    }
  });

  it("CSS 里的浮层类均使用 --z-popover 而非裸数字", () => {
    for (const selector of [
      ".radix-select-content",
      ".branch-popover-content",
      ".file-viewer-tab-menu",
    ]) {
      // 只取以该选择器起头的规则块,避开 :where(...) 聚合选择器里的同名提及。
      const block = new RegExp(`^\\${selector}\\s*\\{([^}]*)\\}`, "m").exec(appCss)?.[1] ?? "";
      expect(block, `${selector} 应使用 var(--z-popover)`).toContain("z-index: var(--z-popover)");
    }
    expect(fontSelectorCss).toContain("z-index: var(--z-popover)");
  });

  it("CSS 中不再残留硬编码的高 z-index", () => {
    for (const css of [appCss, fontSelectorCss]) {
      const hardcoded = [...css.matchAll(/z-index:\s*(\d+)/g)].map(([, value]) => Number(value));
      for (const value of hardcoded) {
        expect(value, `z-index: ${value} 应改用 var(--z-*)`).toBeLessThan(zLayers.overlay);
      }
    }
  });

  it("SSH 连接弹窗的分组下拉高于其遮罩层(回归:曾被完全盖住)", () => {
    expect(s.settingsSelectContent.zIndex).toBe(zLayers.popover);
    expect(Number(s.settingsSelectContent.zIndex)).toBeGreaterThan(
      Number(s.sshDialogOverlay.zIndex),
    );
  });

  it("所有遮罩层样式都取自量表,浮层样式同样如此", () => {
    const overlayValues = new Set<number>(OVERLAY_LAYERS.map((layer) => zLayers[layer]));
    for (const key of [
      "modalOverlay",
      "appConfirmOverlay",
      "sshDialogOverlay",
      "databaseDialogOverlay",
      "fileSearchDialogBackdrop",
      "skillInstallOverlay",
      "skillConflictOverlay",
    ] as const) {
      expect(overlayValues.has(Number(s[key].zIndex)), `${key} 未使用遮罩层级`).toBe(true);
    }

    const floatingValues = new Set<number>(FLOATING_LAYERS.map((layer) => zLayers[layer]));
    for (const key of [
      "settingsSelectContent",
      "fileSearchTypeContent",
      "toolbarMenuContent",
      "toolbarActionMenuContent",
      "usagePopoverContent",
      "skillInstallProjectPopoverContent",
      "fileCtxMenu",
      "fileCtxBackdrop",
    ] as const) {
      expect(floatingValues.has(Number(s[key].zIndex)), `${key} 未使用浮层层级`).toBe(true);
    }
  });
});
