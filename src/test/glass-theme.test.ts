import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { DARK_THEME, EYECARE_THEME, LIGHT_THEME } from "../components/terminalShared";
import { common, dialogs, layout, panels, task } from "../styles";

const themeCss = readFileSync(resolve(process.cwd(), "src/styles/themes.css"), "utf8");

describe("shared frosted glass theme", () => {
  it("applies blur only at elevated boundaries, never on persistent full-height panels", () => {
    // 常驻全高面板(sidebar / taskPanel)和滚动容器不能带 backdrop-filter:
    // 它们身后的内容一变就要重新采样并模糊,滚动与打字每帧都要付这笔钱。
    expect(layout.sidebar).not.toHaveProperty("backdropFilter");
    expect(layout.sidebar).not.toHaveProperty("WebkitBackdropFilter");
    expect(task.taskPanel).not.toHaveProperty("backdropFilter");
    expect(task.taskPanel).not.toHaveProperty("WebkitBackdropFilter");

    // 短暂出现的抬升层保留玻璃感。
    expect(panels.composeCard.backdropFilter).toBe("var(--glass-blur)");
    expect(dialogs.modalBox.backdropFilter).toBe("var(--glass-blur)");
    expect(common.usagePopoverContent.backdropFilter).toBe("var(--glass-blur-compact)");
  });

  it("keeps blur radii cheap and free of a second filter stage", () => {
    // saturate() 会在 blur 之后再挂一级滤镜;半径直接决定每帧采样成本。
    expect(themeCss).toContain("--glass-blur: blur(12px)");
    expect(themeCss).toContain("--glass-blur-compact: blur(8px)");
    expect(themeCss).toContain("--settings-glass-blur: blur(20px)");
    expect(themeCss).not.toContain("saturate(1.28)");
    expect(themeCss).not.toContain("saturate(1.2)");
  });

  it("keeps input and persistent panel surfaces opaque", () => {
    // 这些面板已去掉模糊,底色必须自己立得住;输入框同理。
    const opaque = /^#[0-9a-f]{6}$/i;
    for (const token of ["--bg-input", "--bg-sidebar"]) {
      const values = Array.from(
        themeCss.matchAll(new RegExp(`${token}:\\s*([^;]+);`, "g")),
        (match) => match[1].trim(),
      );
      expect(values).toHaveLength(3);
      for (const value of values) expect(value).toMatch(opaque);
    }
  });

  it("keeps settings surfaces translucent while applying heavy background blur", () => {
    expect(dialogs.settingsModalBox.background).toBe("var(--settings-glass-bg)");
    expect(dialogs.settingsModalBox.backdropFilter).toBe("var(--settings-glass-blur)");
    expect(dialogs.settingsModalBox.WebkitBackdropFilter).toBe("var(--settings-glass-blur)");

    const settingsBackgrounds = Array.from(
      themeCss.matchAll(/--settings-glass-bg:\s*([^;]+);/g),
      (match) => match[1].trim(),
    );
    expect(settingsBackgrounds).toEqual(["#f8fafc", "#f7eedb", "rgba(12, 15, 19, 0.84)"]);
  });

  it("uses a flat One Dark canvas and blue focus color in dark mode", () => {
    const darkTheme = themeCss.slice(themeCss.indexOf("html.dark {"));

    expect(darkTheme).toContain("--app-canvas: #050607");
    expect(darkTheme).toContain("--accent: #61afef");
    expect(darkTheme).not.toContain("radial-gradient");
    expect(darkTheme).not.toContain("#c084fc");
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

  // 深色终端逐值对齐 ~/.config/ghostty/themes/one-dark-pro。这里钉住的是"抄自
  // Ghostty"这件事本身:改动任何一项都意味着两边不再一致,必须是有意为之。
  // 不去读那个文件——测试不能依赖开发机上的用户配置。
  it("mirrors the Ghostty One Dark Pro palette in dark mode", () => {
    expect(DARK_THEME.foreground).toBe("#d7dae0");
    expect(DARK_THEME.cursor).toBe("#528bff");
    expect(DARK_THEME.cursorAccent).toBe("#1e2127");
    expect(DARK_THEME.selectionBackground).toBe("#3e4451");
    expect(DARK_THEME.selectionForeground).toBe("#e6e6e6");
    // ANSI 0 被上游刻意抬亮,压回接近背景会让 TUI 的分隔线与 dim 文字消失。
    expect(DARK_THEME.black).toBe("#4b5263");
    expect(DARK_THEME.brightBlack).toBe("#7f8899");
    expect(DARK_THEME.white).toBe("#d7dae0");
    expect(DARK_THEME.brightWhite).toBe("#e6e6e6");
  });
});
