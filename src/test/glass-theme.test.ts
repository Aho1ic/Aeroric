import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { DARK_THEME, EYECARE_THEME, LIGHT_THEME } from "../components/terminalShared";
import { common, dialogs, layout, panels, task } from "../styles";

const themeCss = readFileSync(resolve(process.cwd(), "src/styles/themes.css"), "utf8");

describe("shared frosted glass theme", () => {
  it("applies blur at shared structural and elevated boundaries", () => {
    expect(layout.sidebar.backdropFilter).toBe("var(--glass-blur)");
    expect(task.taskPanel.backdropFilter).toBe("var(--glass-blur)");
    expect(panels.composeCard.backdropFilter).toBe("var(--glass-blur)");
    expect(dialogs.modalBox.backdropFilter).toBe("var(--glass-blur)");
    expect(common.usagePopoverContent.backdropFilter).toBe("var(--glass-blur-compact)");
  });

  it("keeps settings surfaces opaque while applying heavy background blur", () => {
    expect(dialogs.settingsModalBox.background).toBe("var(--settings-glass-bg)");
    expect(dialogs.settingsModalBox.backdropFilter).toBe("var(--settings-glass-blur)");
    expect(dialogs.settingsModalBox.WebkitBackdropFilter).toBe("var(--settings-glass-blur)");
    expect(themeCss).toContain("--settings-glass-blur: blur(64px) saturate(1.12)");

    const settingsBackgrounds = Array.from(
      themeCss.matchAll(/--settings-glass-bg:\s*([^;]+);/g),
      (match) => match[1].trim(),
    );
    expect(settingsBackgrounds).toEqual(["#f8fafc", "#f7eedb", "#18181d"]);
  });

  it("keeps the home surfaces above the recursive animation free of blur", () => {
    // 首页动画画布在这两层下方，任何 backdrop-filter 都会把它糊成虚影。
    expect(layout.welcomePane).not.toHaveProperty("backdropFilter");
    expect(layout.welcomePane).not.toHaveProperty("WebkitBackdropFilter");
    expect(layout.welcomePane.background).toBe("transparent");
    expect(layout.searchRow).not.toHaveProperty("backdropFilter");
    expect(layout.searchRow).not.toHaveProperty("WebkitBackdropFilter");
    expect(layout.searchRow.background).toBe("transparent");
  });

  it("keeps the light terminal white while preserving themed variants", () => {
    expect(LIGHT_THEME.background).toBe("#ffffff");
    expect(DARK_THEME.background).toContain("rgba(");
    expect(EYECARE_THEME.background).toContain("rgba(");
  });
});
