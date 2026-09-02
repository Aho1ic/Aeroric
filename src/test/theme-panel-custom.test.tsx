import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import { I18nProvider } from "../i18n";
import { ThemePanel } from "../components/app-settings/ThemePanel";
import { CUSTOM_THEME_STORAGE_KEY, CUSTOM_THEME_STYLE_ID } from "../customThemes";

/**
 * 自定义主题那一节的面板测试。与既有的 `theme-panel.test.tsx` 分开:那份的前提是
 * 「面板是纯受控组件,自己不存状态」,而这一节经由 `useCustomThemes` 是有状态的。
 *
 * 断言一律用**插值进去的数据**(主题名、KB 数)与结构(role / aria-label),不比中文文案 ——
 * jsdom 下 `navigator.language` 是 en,而且比文案会让改措辞就挂。
 */

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => invokeMock(cmd, args),
}));

const openMock = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: (...args: unknown[]) => openMock(...args) }));

const revealMock = vi.fn();
vi.mock("@tauri-apps/plugin-opener", () => ({
  revealItemInDir: (...args: unknown[]) => revealMock(...args),
}));

const SOLAR = { id: "solar", name: "我的 Solar", path: "/themes/solar.css", size: 2048 };
const MOSS = { id: "moss", name: "moss", path: "/themes/moss.css", size: 300 };

function stubBackend(handlers: Record<string, (args?: Record<string, unknown>) => unknown>) {
  invokeMock.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
    const handler = handlers[cmd];
    if (!handler) return Promise.reject(new Error(`unexpected command ${cmd}`));
    try {
      return Promise.resolve(handler(args));
    } catch (error) {
      return Promise.reject(error);
    }
  });
}

function renderPanel() {
  render(
    <I18nProvider>
      <ThemePanel themeMode="light" systemPrefersDark={false} onThemeModeChange={() => {}} />
    </I18nProvider>,
  );
}

function injected(): string | null {
  return document.getElementById(CUSTOM_THEME_STYLE_ID)?.textContent ?? null;
}

beforeEach(() => {
  invokeMock.mockReset();
  openMock.mockReset();
  revealMock.mockReset();
  document.getElementById(CUSTOM_THEME_STYLE_ID)?.remove();
  localStorage.clear();
});

describe("ThemePanel 的自定义主题一节", () => {
  it("列出已导入的主题,展示名保留原文", async () => {
    stubBackend({ theme_custom_list: () => [SOLAR, MOSS] });
    renderPanel();

    // 展示名可以是中文,id 才被清洗过 —— 用户看到的必须是自己起的名字。
    await waitFor(() => expect(screen.getByText("我的 Solar")).toBeInTheDocument());
    expect(screen.getByText("moss")).toBeInTheDocument();
  });

  it("体积按 KB 显示,不足 1KB 也不显示 0", async () => {
    stubBackend({ theme_custom_list: () => [MOSS] });
    renderPanel();

    // 300 字节 → 1 KB。显示 0 KB 会让用户以为文件是空的。
    await waitFor(() => expect(screen.getByText("1 KB")).toBeInTheDocument());
  });

  it("空列表给的是提示而不是错误", async () => {
    stubBackend({ theme_custom_list: () => [] });
    renderPanel();

    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("theme_custom_list", undefined));
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("导入走文件对话框,只放 css,取消不调后端", async () => {
    stubBackend({ theme_custom_list: () => [] });
    openMock.mockResolvedValue(null);
    renderPanel();
    await waitFor(() => expect(invokeMock).toHaveBeenCalled());

    const importButton = screen.getByRole("button", { name: /Import \.css/i });
    await act(async () => {
      importButton.click();
    });

    expect(openMock).toHaveBeenCalledTimes(1);
    const options = openMock.mock.calls[0][0] as {
      filters: { extensions: string[] }[];
      directory: boolean;
    };
    // 过滤器写错扩展名的话用户根本选不到自己的文件。
    expect(options.filters[0].extensions).toEqual(["css"]);
    expect(options.directory).toBe(false);
    expect(invokeMock).not.toHaveBeenCalledWith("theme_custom_import", expect.anything());
  });

  it("选中文件后导入并立刻应用", async () => {
    const themes = [MOSS];
    stubBackend({
      theme_custom_list: () => [...themes],
      theme_custom_import: () => {
        themes.push(SOLAR);
        return SOLAR;
      },
      theme_custom_read: () => ":root { --accent: #ff8800; }",
    });
    openMock.mockResolvedValue("/tmp/solar.css");
    renderPanel();
    await waitFor(() => expect(screen.getByText("moss")).toBeInTheDocument());

    await act(async () => {
      screen.getByRole("button", { name: /Import \.css/i }).click();
    });

    expect(invokeMock).toHaveBeenCalledWith("theme_custom_import", {
      sourcePath: "/tmp/solar.css",
    });
    await waitFor(() => expect(injected()).toContain("#ff8800"));
  });

  it("生效中的那套显示已应用而不是应用按钮", async () => {
    localStorage.setItem(CUSTOM_THEME_STORAGE_KEY, "solar");
    stubBackend({
      theme_custom_list: () => [SOLAR, MOSS],
      theme_custom_read: () => "body {}",
    });
    renderPanel();

    await waitFor(() => expect(screen.getByText("Applied")).toBeInTheDocument());
    // 两套主题,只有没生效的那套还有「应用」按钮。
    expect(screen.getAllByRole("button", { name: "Apply" })).toHaveLength(1);
    // 生效中才出现「停用」。
    expect(screen.getByRole("button", { name: /Turn off custom theme/i })).toBeInTheDocument();
  });

  it("没有生效中的主题时不画停用按钮", async () => {
    stubBackend({ theme_custom_list: () => [SOLAR] });
    renderPanel();

    await waitFor(() => expect(screen.getByText("我的 Solar")).toBeInTheDocument());
    expect(screen.queryByRole("button", { name: /Turn off custom theme/i })).toBeNull();
  });

  it("删除按钮的 aria-label 带上主题名", async () => {
    stubBackend({ theme_custom_list: () => [SOLAR, MOSS], theme_custom_delete: () => null });
    renderPanel();
    await waitFor(() => expect(screen.getByText("我的 Solar")).toBeInTheDocument());

    // 两个图标按钮长得一样,只有名字能区分它们删的是哪一套。
    const solarDelete = screen.getByRole("button", { name: "Delete theme 我的 Solar" });
    const mossDelete = screen.getByRole("button", { name: "Delete theme moss" });
    expect(solarDelete).not.toBe(mossDelete);

    await act(async () => {
      mossDelete.click();
    });
    expect(invokeMock).toHaveBeenCalledWith("theme_custom_delete", { id: "moss" });
  });

  it("后端报错走 alert", async () => {
    stubBackend({
      theme_custom_list: () => [],
      theme_custom_import: () => {
        throw new Error("Only .css files can be imported as a theme");
      },
    });
    openMock.mockResolvedValue("/tmp/theme.txt");
    renderPanel();
    await waitFor(() => expect(invokeMock).toHaveBeenCalled());

    await act(async () => {
      screen.getByRole("button", { name: /Import \.css/i }).click();
    });
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain(".css");
  });

  it("打开主题文件夹走 revealItemInDir", async () => {
    stubBackend({
      theme_custom_list: () => [],
      theme_custom_dir: () => "/home/u/.aeroric/themes",
    });
    renderPanel();
    await waitFor(() => expect(invokeMock).toHaveBeenCalled());

    await act(async () => {
      screen.getByRole("button", { name: /Open themes folder/i }).click();
    });
    expect(revealMock).toHaveBeenCalledWith("/home/u/.aeroric/themes");
  });

  it("应急快捷键的提示与限制说明都在场", async () => {
    stubBackend({ theme_custom_list: () => [] });
    renderPanel();
    await waitFor(() => expect(invokeMock).toHaveBeenCalled());

    // 提示必须和导入控件在一起 —— 界面被藏掉之后用户才知道怎么出来。
    expect(screen.getByText(/press .* to turn it off/i)).toBeInTheDocument();
    // 作用域与原生装饰这两条限制不写清会被当成 bug 报回来。
    expect(screen.getByText(/apply to the whole app/i)).toBeInTheDocument();
  });

  it("内置三档的选择不受自定义主题一节影响", async () => {
    stubBackend({ theme_custom_list: () => [SOLAR] });
    renderPanel();
    await waitFor(() => expect(screen.getByText("我的 Solar")).toBeInTheDocument());

    // 自定义 CSS 是叠加的,内置的 radiogroup 仍然是三个选项。
    const group = screen.getByRole("radiogroup");
    expect(group.querySelectorAll('[role="radio"]')).toHaveLength(3);
  });
});
