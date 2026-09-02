/**
 * 自定义 CSS 主题的前端侧:命令包装、`<style>` 注入、应急停用快捷键的判定。
 *
 * 与内置主题的关系是**叠加,不是替换**。内置三档走 `documentElement` 上的
 * `dark` / `eyecare` class(`App.tsx`)+ `themes.css` 里的三个选择器;自定义 CSS 是一个
 * 后插入到 `document.head` 的 `<style>`,同优先级下后来者胜,所以它能覆盖 `:root` 上的
 * token 而**不需要改动内置的任何一档**。
 *
 * 两条限制,UI 里要明说:
 *
 * 1. **改不到原生窗口装饰。** macOS 透明标题栏透出的背景色由 `nativeWindowBackgroundForVariant`
 *    (`appThemeState.ts`)在 Tauri 侧给,CSS 到不了那里。
 * 2. **作用于整个应用,不只随手记。** 注入点是 `document.head`。要限定到某棵子树得有一个
 *    可靠的 CSS 选择器改写器,而 `@scope` 的浏览器支持还不齐 —— 与其做一个会在边角上
 *    改错用户 CSS 的半成品,不如把范围如实说清。
 *
 * 这里**不消毒 CSS**。自定义 CSS 的能力边界就是 CSS 本身:能改外观、能把元素藏起来,
 * 但没有脚本入口。真正的风险是用户导入一份写坏的 CSS 把界面弄成没法操作 —— 那是可用性
 * 问题,由下面的应急停用快捷键兜。
 */

import { invoke } from "@tauri-apps/api/core";

/** 与 Rust 的 `CustomTheme` 一一对应(`custom_themes.rs`)。 */
export interface CustomTheme {
  /** 清洗后的标识,同时是磁盘文件名。只含 `[A-Za-z0-9_-]`。 */
  id: string;
  /** 展示名,取源文件名 stem 的原文,可以是中文。 */
  name: string;
  path: string;
  size: number;
}

/** 注入节点的固定 id。固定是为了「注入两次只有一个节点」。 */
export const CUSTOM_THEME_STYLE_ID = "aeroric-custom-theme";

/** 记住用户选了哪一套。key 前缀与其它应用级偏好一致(`aeroric:`)。 */
export const CUSTOM_THEME_STORAGE_KEY = "aeroric:customTheme";

export function listCustomThemes(): Promise<CustomTheme[]> {
  return invoke<CustomTheme[]>("theme_custom_list");
}

export function importCustomTheme(sourcePath: string): Promise<CustomTheme> {
  return invoke<CustomTheme>("theme_custom_import", { sourcePath });
}

export function readCustomTheme(id: string): Promise<string> {
  return invoke<string>("theme_custom_read", { id });
}

export function deleteCustomTheme(id: string): Promise<void> {
  return invoke<void>("theme_custom_delete", { id });
}

export function customThemeDir(): Promise<string> {
  return invoke<string>("theme_custom_dir");
}

export function readStoredThemeId(): string | null {
  const raw = localStorage.getItem(CUSTOM_THEME_STORAGE_KEY);
  return raw && raw.trim() ? raw : null;
}

export function writeStoredThemeId(id: string | null): void {
  if (id === null) {
    localStorage.removeItem(CUSTOM_THEME_STORAGE_KEY);
    return;
  }
  localStorage.setItem(CUSTOM_THEME_STORAGE_KEY, id);
}

/**
 * 把 CSS 注入根节点。`css === null` 表示移除。
 *
 * 已存在时改 `textContent` 而不是删了重建:重建会让浏览器丢掉一帧样式,视觉上是闪一下。
 */
export function setInjectedCss(css: string | null): void {
  const existing = document.getElementById(CUSTOM_THEME_STYLE_ID);
  if (css === null) {
    existing?.remove();
    return;
  }
  if (existing instanceof HTMLStyleElement) {
    existing.textContent = css;
    return;
  }
  const tag = document.createElement("style");
  tag.id = CUSTOM_THEME_STYLE_ID;
  tag.textContent = css;
  document.head.appendChild(tag);
}

export function customThemeIsInjected(): boolean {
  return document.getElementById(CUSTOM_THEME_STYLE_ID) !== null;
}

/**
 * 应急停用快捷键:`Cmd/Ctrl + Alt + Shift + T`。
 *
 * 为什么需要它:用户可以导入一份把界面全藏起来的 CSS(`* { display: none }`)。那不是
 * 安全问题,是可用性问题 —— **设置面板本身也被藏了**,而激活的 id 是持久化的,所以
 * 重启也回不来。没有这条路的话,唯一的出路是手工去删 `~/.aeroric/themes/`。
 *
 * 四个键一起按是刻意的:它会丢掉用户的选择,不该被误触。`metaKey` 与 `ctrlKey` 都收,
 * 换平台不用改肌肉记忆。
 *
 * 判定单独导出是为了能直接测 —— 它是这条逃生路唯一的入口条件。
 */
export function isCustomThemePanicKey(event: KeyboardEvent): boolean {
  return (
    event.key.toLowerCase() === "t" &&
    event.altKey &&
    event.shiftKey &&
    (event.metaKey || event.ctrlKey)
  );
}

/**
 * 停用当前自定义主题:摘掉注入节点并清掉持久化。
 *
 * 返回是否真的停掉了一个 —— 没有生效中的主题时按快捷键不该产生任何反馈。
 */
export function disableCustomTheme(): boolean {
  const wasInjected = customThemeIsInjected();
  setInjectedCss(null);
  writeStoredThemeId(null);
  return wasInjected;
}
