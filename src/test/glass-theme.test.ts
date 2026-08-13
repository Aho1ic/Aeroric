import { describe, expect, it } from "vitest";
import { DARK_THEME, EYECARE_THEME, LIGHT_THEME } from "../components/terminalShared";
import { common, dialogs, layout, panels, task } from "../styles";

describe("shared frosted glass theme", () => {
  it("applies blur at shared structural and elevated boundaries", () => {
    expect(layout.sidebar.backdropFilter).toBe("var(--glass-blur)");
    expect(task.taskPanel.backdropFilter).toBe("var(--glass-blur)");
    expect(panels.composeCard.backdropFilter).toBe("var(--glass-blur)");
    expect(dialogs.modalBox.backdropFilter).toBe("var(--glass-blur)");
    expect(common.usagePopoverContent.backdropFilter).toBe("var(--glass-blur-compact)");
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

  it("uses transparent terminal canvases for all desktop themes", () => {
    expect(LIGHT_THEME.background).toContain("rgba(");
    expect(DARK_THEME.background).toContain("rgba(");
    expect(EYECARE_THEME.background).toContain("rgba(");
  });
});
