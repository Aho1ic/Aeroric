import { beforeEach, describe, expect, it } from "vitest";
import {
  CUSTOM_THEME_STORAGE_KEY,
  CUSTOM_THEME_STYLE_ID,
  customThemeIsInjected,
  disableCustomTheme,
  isCustomThemePanicKey,
  readStoredThemeId,
  setInjectedCss,
  writeStoredThemeId,
} from "../customThemes";

function panicKey(overrides: Partial<KeyboardEventInit> = {}): KeyboardEvent {
  return new KeyboardEvent("keydown", {
    key: "T",
    altKey: true,
    shiftKey: true,
    metaKey: true,
    ...overrides,
  });
}

describe("自定义主题的注入层", () => {
  beforeEach(() => {
    document.getElementById(CUSTOM_THEME_STYLE_ID)?.remove();
    localStorage.clear();
  });

  it("注入两次只留一个节点,并且是改内容而不是重建", () => {
    setInjectedCss(":root { --accent: #111; }");
    const first = document.getElementById(CUSTOM_THEME_STYLE_ID);
    expect(first).not.toBeNull();

    setInjectedCss(":root { --accent: #222; }");
    expect(document.querySelectorAll(`#${CUSTOM_THEME_STYLE_ID}`)).toHaveLength(1);
    // 同一个节点对象 —— 删了重建会让浏览器丢一帧样式。
    expect(document.getElementById(CUSTOM_THEME_STYLE_ID)).toBe(first);
    expect(first?.textContent).toContain("#222");
  });

  it("注入的是 style 节点且挂在 head 上,顺序在最后", () => {
    setInjectedCss("body { color: red; }");
    const tag = document.getElementById(CUSTOM_THEME_STYLE_ID);
    expect(tag).toBeInstanceOf(HTMLStyleElement);
    expect(tag?.parentElement).toBe(document.head);
    // 叠加靠的就是「后来者胜」,所以它必须是 head 的最后一个子节点。
    expect(document.head.lastElementChild).toBe(tag);
  });

  it("传 null 移除节点", () => {
    setInjectedCss("body {}");
    expect(customThemeIsInjected()).toBe(true);
    setInjectedCss(null);
    expect(customThemeIsInjected()).toBe(false);
    // 本来就没有时移除不报错。
    expect(() => setInjectedCss(null)).not.toThrow();
  });

  it("持久化读写,空串与缺失都当没设过", () => {
    expect(readStoredThemeId()).toBeNull();
    writeStoredThemeId("solar");
    expect(localStorage.getItem(CUSTOM_THEME_STORAGE_KEY)).toBe("solar");
    expect(readStoredThemeId()).toBe("solar");

    localStorage.setItem(CUSTOM_THEME_STORAGE_KEY, "   ");
    expect(readStoredThemeId()).toBeNull();

    writeStoredThemeId(null);
    expect(localStorage.getItem(CUSTOM_THEME_STORAGE_KEY)).toBeNull();
  });
});

describe("应急停用快捷键的判定", () => {
  it("四个键齐了才算,Ctrl 与 Cmd 都收", () => {
    expect(isCustomThemePanicKey(panicKey())).toBe(true);
    expect(isCustomThemePanicKey(panicKey({ metaKey: false, ctrlKey: true }))).toBe(true);
  });

  it("缺任何一个修饰键都不算", () => {
    expect(isCustomThemePanicKey(panicKey({ altKey: false }))).toBe(false);
    expect(isCustomThemePanicKey(panicKey({ shiftKey: false }))).toBe(false);
    expect(isCustomThemePanicKey(panicKey({ metaKey: false, ctrlKey: false }))).toBe(false);
  });

  it("换别的键不算", () => {
    expect(isCustomThemePanicKey(panicKey({ key: "k" }))).toBe(false);
    expect(isCustomThemePanicKey(panicKey({ key: "Enter" }))).toBe(false);
  });

  it("大小写都认", () => {
    expect(isCustomThemePanicKey(panicKey({ key: "t" }))).toBe(true);
    expect(isCustomThemePanicKey(panicKey({ key: "T" }))).toBe(true);
  });
});

describe("disableCustomTheme", () => {
  beforeEach(() => {
    document.getElementById(CUSTOM_THEME_STYLE_ID)?.remove();
    localStorage.clear();
  });

  it("摘掉节点、清掉持久化,并报告确实停掉了一个", () => {
    setInjectedCss("body {}");
    writeStoredThemeId("solar");

    expect(disableCustomTheme()).toBe(true);
    expect(customThemeIsInjected()).toBe(false);
    // 光摘节点不清持久化的话,重启会把坏主题又装回来 —— 这条是逃生路的关键。
    expect(readStoredThemeId()).toBeNull();
  });

  it("没有生效中的主题时报告没停掉任何东西", () => {
    expect(disableCustomTheme()).toBe(false);
  });
});
