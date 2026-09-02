import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { ThemePanel } from "../components/app-settings/ThemePanel";
import type { ThemeMode } from "../types";

/**
 * 主题面板是纯受控组件:自己不存状态,所有选择都往 onThemeModeChange 上抛。
 * 所以这里断言的都是「点了/按了什么键 → 抛出哪个 mode」,以及当前 mode 怎么显示。
 */

function renderPanel(props: { themeMode?: ThemeMode; systemPrefersDark?: boolean } = {}): {
  onThemeModeChange: ReturnType<typeof vi.fn>;
} {
  const onThemeModeChange = vi.fn();
  render(
    <I18nProvider>
      <ThemePanel
        themeMode={props.themeMode ?? "system"}
        systemPrefersDark={props.systemPrefersDark ?? true}
        onThemeModeChange={onThemeModeChange}
      />
    </I18nProvider>,
  );
  return { onThemeModeChange };
}

function systemSwitch() {
  return screen.getByRole("switch", { name: "Follow system theme" });
}

function radioGroup() {
  return screen.getByRole("radiogroup", { name: "Manual theme" });
}

function themeRadio(name: "Dark" | "Light" | "Eye Care") {
  return within(radioGroup()).getByRole("radio", { name });
}

describe("ThemePanel 当前状态展示", () => {
  it("跟随系统且系统是暗色时显示 Following system · Dark", () => {
    renderPanel({ themeMode: "system", systemPrefersDark: true });
    expect(screen.getByText("Following system · Dark")).toBeInTheDocument();
  });

  it("跟随系统且系统是亮色时显示 Following system · Light", () => {
    renderPanel({ themeMode: "system", systemPrefersDark: false });
    expect(screen.getByText("Following system · Light")).toBeInTheDocument();
  });

  it("手动暗色显示 Manual · Dark", () => {
    renderPanel({ themeMode: "dark" });
    expect(screen.getByText("Manual · Dark")).toBeInTheDocument();
  });

  it("手动亮色显示 Manual · Light", () => {
    renderPanel({ themeMode: "light" });
    expect(screen.getByText("Manual · Light")).toBeInTheDocument();
  });

  it("护眼色显示 Manual · Eye Care", () => {
    renderPanel({ themeMode: "eyecare" });
    expect(screen.getByText("Manual · Eye Care")).toBeInTheDocument();
  });

  it("手动模式下的摘要不受系统偏好影响", () => {
    // systemPrefersDark 只喂「跟随系统」那一行,手动模式读的是 themeMode 自己。
    renderPanel({ themeMode: "light", systemPrefersDark: true });
    expect(screen.getByText("Manual · Light")).toBeInTheDocument();
    expect(screen.queryByText(/Following system/)).not.toBeInTheDocument();
  });
});

describe("ThemePanel 跟随系统开关", () => {
  it("themeMode 为 system 时开关是打开的", () => {
    renderPanel({ themeMode: "system" });
    expect(systemSwitch()).toHaveAttribute("aria-checked", "true");
  });

  it("手动模式下开关是关闭的", () => {
    renderPanel({ themeMode: "dark" });
    expect(systemSwitch()).toHaveAttribute("aria-checked", "false");
  });

  it("从跟随系统关掉时落到 light", () => {
    const { onThemeModeChange } = renderPanel({ themeMode: "system" });
    fireEvent.click(systemSwitch());
    expect(onThemeModeChange).toHaveBeenCalledWith("light");
  });

  it("从手动模式打开时切回 system", () => {
    const { onThemeModeChange } = renderPanel({ themeMode: "eyecare" });
    fireEvent.click(systemSwitch());
    expect(onThemeModeChange).toHaveBeenCalledWith("system");
  });
});

describe("ThemePanel 手动选择", () => {
  it("三个选项都在,且只有当前那个是选中态", () => {
    renderPanel({ themeMode: "light" });
    expect(themeRadio("Dark")).toHaveAttribute("aria-checked", "false");
    expect(themeRadio("Light")).toHaveAttribute("aria-checked", "true");
    expect(themeRadio("Eye Care")).toHaveAttribute("aria-checked", "false");
  });

  it("跟随系统时三个手动选项都不是选中态", () => {
    renderPanel({ themeMode: "system" });
    for (const name of ["Dark", "Light", "Eye Care"] as const) {
      expect(themeRadio(name)).toHaveAttribute("aria-checked", "false");
    }
  });

  it("点选项抛出对应 mode", () => {
    const { onThemeModeChange } = renderPanel({ themeMode: "system" });
    fireEvent.click(themeRadio("Eye Care"));
    expect(onThemeModeChange).toHaveBeenCalledWith("eyecare");
  });

  it("点已选中的选项仍然会抛(受控组件不自己判重)", () => {
    const { onThemeModeChange } = renderPanel({ themeMode: "dark" });
    fireEvent.click(themeRadio("Dark"));
    expect(onThemeModeChange).toHaveBeenCalledWith("dark");
  });

  it("每个选项都带自己的说明文案", () => {
    renderPanel({ themeMode: "dark" });
    expect(screen.getByText("Always use the dark interface.")).toBeInTheDocument();
    expect(screen.getByText("Always use the light interface.")).toBeInTheDocument();
    expect(screen.getByText("Warm sepia palette for long reading sessions.")).toBeInTheDocument();
  });
});

describe("ThemePanel 键盘导航", () => {
  const order = ["dark", "light", "eyecare"] as const;

  // 实现里 `if (currentIndex === -1) return` 这一支够不着:mode 只可能来自
  // manualThemeModes 本身(三个选项是写死的),indexOf 永远找得到。把那句改成
  // throw 后 26 个用例仍全绿 —— 属于防御性死支,不为它造用例。

  it("ArrowRight 往后走一格", () => {
    const { onThemeModeChange } = renderPanel({ themeMode: "dark" });
    fireEvent.keyDown(themeRadio("Dark"), { key: "ArrowRight" });
    expect(onThemeModeChange).toHaveBeenCalledWith("light");
  });

  it("ArrowDown 与 ArrowRight 同义", () => {
    const { onThemeModeChange } = renderPanel({ themeMode: "light" });
    fireEvent.keyDown(themeRadio("Light"), { key: "ArrowDown" });
    expect(onThemeModeChange).toHaveBeenCalledWith("eyecare");
  });

  it("最后一格 ArrowRight 绕回第一格", () => {
    const { onThemeModeChange } = renderPanel({ themeMode: "eyecare" });
    fireEvent.keyDown(themeRadio("Eye Care"), { key: "ArrowRight" });
    expect(onThemeModeChange).toHaveBeenCalledWith("dark");
  });

  it("ArrowLeft 往前走一格", () => {
    const { onThemeModeChange } = renderPanel({ themeMode: "eyecare" });
    fireEvent.keyDown(themeRadio("Eye Care"), { key: "ArrowLeft" });
    expect(onThemeModeChange).toHaveBeenCalledWith("light");
  });

  it("ArrowUp 与 ArrowLeft 同义", () => {
    const { onThemeModeChange } = renderPanel({ themeMode: "light" });
    fireEvent.keyDown(themeRadio("Light"), { key: "ArrowUp" });
    expect(onThemeModeChange).toHaveBeenCalledWith("dark");
  });

  it("第一格 ArrowLeft 绕到最后一格(不是停在原地也不是越界)", () => {
    // 这里是 `(i - 1 + len) % len` 的实际验证:少了 `+ len` 会得到 -1 % 3 === -1,
    // 取到 undefined 而不是绕回 eyecare。
    const { onThemeModeChange } = renderPanel({ themeMode: "dark" });
    fireEvent.keyDown(themeRadio("Dark"), { key: "ArrowLeft" });
    expect(onThemeModeChange).toHaveBeenCalledWith("eyecare");
  });

  it("Home 跳到第一格", () => {
    const { onThemeModeChange } = renderPanel({ themeMode: "eyecare" });
    fireEvent.keyDown(themeRadio("Eye Care"), { key: "Home" });
    expect(onThemeModeChange).toHaveBeenCalledWith("dark");
  });

  it("End 跳到最后一格", () => {
    const { onThemeModeChange } = renderPanel({ themeMode: "dark" });
    fireEvent.keyDown(themeRadio("Dark"), { key: "End" });
    expect(onThemeModeChange).toHaveBeenCalledWith("eyecare");
  });

  it("导航键会吃掉默认行为(否则方向键会滚动设置面板)", () => {
    renderPanel({ themeMode: "light" });
    for (const key of ["ArrowRight", "ArrowLeft", "ArrowUp", "ArrowDown", "Home", "End"]) {
      const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
      themeRadio("Light").dispatchEvent(event);
      expect(event.defaultPrevented).toBe(true);
    }
  });

  it("其他键既不抛也不吃默认行为", () => {
    const { onThemeModeChange } = renderPanel({ themeMode: "light" });
    for (const key of ["a", "Tab", "Escape", "PageDown", " "]) {
      const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
      themeRadio("Light").dispatchEvent(event);
      expect(event.defaultPrevented).toBe(false);
    }
    expect(onThemeModeChange).not.toHaveBeenCalled();
  });

  it("从任一格出发按满一圈方向键能回到原点", () => {
    // 逐格验证环形导航,而不是只测一两个点位。
    for (let start = 0; start < order.length; start++) {
      const { onThemeModeChange } = renderPanel({ themeMode: order[start] });
      const label = (["Dark", "Light", "Eye Care"] as const)[start];
      const radios = screen.getAllByRole("radio", { name: label });
      fireEvent.keyDown(radios[radios.length - 1], { key: "ArrowRight" });
      expect(onThemeModeChange).toHaveBeenCalledWith(order[(start + 1) % order.length]);
    }
  });
});
