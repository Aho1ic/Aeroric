/**
 * 全局默认字体注入:给 Text / TextInput 的 render 前置一层 fontFamily,
 * 避免逐个文件在 StyleSheet 里重复写字体。
 *
 * React 19 已移除函数组件的 defaultProps,因此改为 patch forwardRef 的 render;
 * 若 RN 内部结构变化拿不到 render,则静默跳过(退回系统默认字体,不崩)。
 * 组件自身 style 排在后面,单点覆盖(如等宽字体)依然生效。
 */
import { StyleSheet, Text, TextInput } from "react-native";
import { fontFamily } from "./fonts";

const baseStyles = StyleSheet.create({ font: { fontFamily } });

type Patchable = {
  render?: (props: Record<string, unknown>, ref: unknown) => unknown;
  __aeroricFontPatched?: boolean;
};

function patchFontFamily(component: unknown): void {
  const target = component as Patchable | null;
  if (!target || target.__aeroricFontPatched) return;
  const original = target.render;
  if (typeof original !== "function") return;
  target.render = function patchedRender(props, ref) {
    const style = (props as { style?: unknown }).style;
    return original.call(this, { ...props, style: [baseStyles.font, style] }, ref);
  };
  target.__aeroricFontPatched = true;
}

if (fontFamily) {
  patchFontFamily(Text);
  patchFontFamily(TextInput);
}
