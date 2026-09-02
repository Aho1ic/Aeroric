import { act, render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useCustomThemes, type UseCustomThemesResult } from "../hooks/useCustomThemes";
import {
  CUSTOM_THEME_STORAGE_KEY,
  CUSTOM_THEME_STYLE_ID,
  readStoredThemeId,
  type CustomTheme,
} from "../customThemes";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => invokeMock(cmd, args),
}));

const SOLAR: CustomTheme = {
  id: "solar",
  name: "solar",
  path: "/themes/solar.css",
  size: 2048,
};
const MOSS: CustomTheme = { id: "moss", name: "moss", path: "/themes/moss.css", size: 1024 };

/**
 * 真实的 `invoke` 永远返回 promise,失败是 rejection 而不是同步 throw。stub 里同步抛会让
 * 错误在 `invoke()` 调用点就炸出去,走的不是被测代码里的 catch —— 所以这里一律用
 * `Promise.reject`。
 */
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

function injectedCss(): string | null {
  return document.getElementById(CUSTOM_THEME_STYLE_ID)?.textContent ?? null;
}

/** 把 hook 的最新返回值抓出来。 */
function harness() {
  const seen: { current: UseCustomThemesResult | null } = { current: null };
  function Probe() {
    seen.current = useCustomThemes();
    return null;
  }
  const view = render(<Probe />);
  return { seen, view };
}

beforeEach(() => {
  invokeMock.mockReset();
  document.getElementById(CUSTOM_THEME_STYLE_ID)?.remove();
  localStorage.clear();
});

describe("useCustomThemes", () => {
  it("启动时列出主题,没记住任何一套时不注入", async () => {
    stubBackend({ theme_custom_list: () => [SOLAR, MOSS] });
    const { seen } = harness();

    await waitFor(() => expect(seen.current?.themes).toHaveLength(2));
    expect(seen.current?.activeId).toBeNull();
    expect(injectedCss()).toBeNull();
    expect(invokeMock).not.toHaveBeenCalledWith("theme_custom_read", expect.anything());
  });

  it("启动时把记住的那套读回来并注入", async () => {
    localStorage.setItem(CUSTOM_THEME_STORAGE_KEY, "solar");
    stubBackend({
      theme_custom_list: () => [SOLAR],
      theme_custom_read: () => ":root { --accent: #ff8800; }",
    });
    const { seen } = harness();

    await waitFor(() => expect(injectedCss()).toContain("#ff8800"));
    expect(seen.current?.activeId).toBe("solar");
  });

  it("记住的那套读不回来时清掉持久化并报错", async () => {
    localStorage.setItem(CUSTOM_THEME_STORAGE_KEY, "gone");
    stubBackend({
      theme_custom_list: () => [],
      theme_custom_read: () => {
        throw new Error("The theme does not exist");
      },
    });
    const { seen } = harness();

    await waitFor(() => expect(seen.current?.error).not.toBeNull());
    // 不清的话每次启动都失败一次,而用户看不出是哪一套坏了。
    expect(readStoredThemeId()).toBeNull();
    expect(seen.current?.activeId).toBeNull();
    expect(injectedCss()).toBeNull();
  });

  it("apply(null) 撤掉注入并清持久化", async () => {
    localStorage.setItem(CUSTOM_THEME_STORAGE_KEY, "solar");
    stubBackend({ theme_custom_list: () => [SOLAR], theme_custom_read: () => "body {}" });
    const { seen } = harness();
    await waitFor(() => expect(seen.current?.activeId).toBe("solar"));

    await act(async () => {
      await seen.current?.apply(null);
    });
    expect(injectedCss()).toBeNull();
    expect(readStoredThemeId()).toBeNull();
    expect(seen.current?.activeId).toBeNull();
  });

  it("导入之后直接应用新导入的那套", async () => {
    stubBackend({
      theme_custom_list: () => [SOLAR],
      theme_custom_import: () => SOLAR,
      theme_custom_read: () => "body { color: red; }",
    });
    const { seen } = harness();
    await waitFor(() => expect(seen.current?.themes).toHaveLength(1));

    await act(async () => {
      await seen.current?.importFrom("/tmp/solar.css");
    });
    expect(seen.current?.activeId).toBe("solar");
    expect(injectedCss()).toContain("color: red");
    expect(seen.current?.busy).toBe(false);
  });

  it("导入失败时报错并解禁", async () => {
    stubBackend({
      theme_custom_list: () => [],
      theme_custom_import: () => {
        throw new Error("Only .css files can be imported as a theme");
      },
    });
    const { seen } = harness();
    await waitFor(() => expect(seen.current).not.toBeNull());

    await act(async () => {
      await seen.current?.importFrom("/tmp/theme.txt");
    });
    expect(seen.current?.error).toContain(".css");
    expect(seen.current?.busy).toBe(false);
  });

  it("删除生效中的那套会先撤掉注入", async () => {
    localStorage.setItem(CUSTOM_THEME_STORAGE_KEY, "solar");
    const remaining: CustomTheme[] = [SOLAR];
    stubBackend({
      theme_custom_list: () => [...remaining],
      theme_custom_read: () => "body {}",
      theme_custom_delete: () => {
        remaining.length = 0;
        return null;
      },
    });
    const { seen } = harness();
    await waitFor(() => expect(injectedCss()).not.toBeNull());

    await act(async () => {
      await seen.current?.remove("solar");
    });
    // 留着注入就等于界面上有一份已经不存在的样式。
    expect(injectedCss()).toBeNull();
    expect(seen.current?.activeId).toBeNull();
    expect(seen.current?.themes).toHaveLength(0);
  });

  it("删除没在用的那套不动当前注入", async () => {
    localStorage.setItem(CUSTOM_THEME_STORAGE_KEY, "solar");
    const remaining: CustomTheme[] = [SOLAR, MOSS];
    stubBackend({
      theme_custom_list: () => [...remaining],
      theme_custom_read: () => ":root { --accent: #ff8800; }",
      theme_custom_delete: () => {
        remaining.splice(1, 1);
        return null;
      },
    });
    const { seen } = harness();
    await waitFor(() => expect(injectedCss()).toContain("#ff8800"));

    await act(async () => {
      await seen.current?.remove("moss");
    });
    expect(injectedCss()).toContain("#ff8800");
    expect(seen.current?.activeId).toBe("solar");
  });

  it("应急快捷键摘掉注入并清持久化", async () => {
    localStorage.setItem(CUSTOM_THEME_STORAGE_KEY, "solar");
    stubBackend({ theme_custom_list: () => [SOLAR], theme_custom_read: () => "body {}" });
    const { seen } = harness();
    await waitFor(() => expect(injectedCss()).not.toBeNull());

    await act(async () => {
      document.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "T",
          altKey: true,
          shiftKey: true,
          metaKey: true,
        }),
      );
    });
    expect(injectedCss()).toBeNull();
    expect(readStoredThemeId()).toBeNull();
    expect(seen.current?.activeId).toBeNull();
  });

  it("卸载之后快捷键不再摘掉注入", async () => {
    localStorage.setItem(CUSTOM_THEME_STORAGE_KEY, "solar");
    stubBackend({ theme_custom_list: () => [SOLAR], theme_custom_read: () => "body {}" });
    const { view } = harness();
    // **先等注入真的落地**。启动链是两段 await,不等的话卸载会发生在读存储之前,
    // 注入节点本来就没出现过 —— 那时断言「为 null」和监听有没有摘掉毫无关系。
    await waitFor(() => expect(injectedCss()).not.toBeNull());

    view.unmount();
    await act(async () => {
      document.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "T",
          altKey: true,
          shiftKey: true,
          metaKey: true,
        }),
      );
    });
    // 卸载不该清掉用户的选择,所以节点仍在、持久化仍在。
    expect(injectedCss()).not.toBeNull();
    expect(readStoredThemeId()).toBe("solar");
  });
});
