/**
 * 字体族:使用系统内置中文字体,不引入 expo-font,零包体开销。
 * `fontFamily` 走全局默认注入(app/_layout.tsx),等宽族由终端/diff/代码视图显式引用。
 */
import { Platform } from "react-native";

/** 中文优化正文字体族。 */
export const fontFamily = Platform.select({
  ios: "PingFang SC",
  android: "Noto Sans CJK SC",
  default: undefined,
});

/** 等宽字体族(收编原先散落在 ChangesPane / MarkdownText / file-view 的 MONO 定义)。 */
export const monoFamily = Platform.select({ ios: "Menlo", default: "monospace" });
