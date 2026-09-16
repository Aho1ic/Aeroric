import { useAppearanceStore } from "./appearanceStore";

/**
 * ProjectPage 等子树读取外观的统一入口。
 * App 仍持有权威 state（localStorage 持久化），通过 hydrate 镜像进 store。
 */
export function useAppearance() {
  const themeMode = useAppearanceStore((s) => s.themeMode);
  const themeVariant = useAppearanceStore((s) => s.themeVariant);
  const terminalFontSize = useAppearanceStore((s) => s.terminalFontSize);
  const uiFontFamily = useAppearanceStore((s) => s.uiFontFamily);
  const monoFontFamily = useAppearanceStore((s) => s.monoFontFamily);
  const attentionBadge = useAppearanceStore((s) => s.attentionBadge);
  const taskDisplayWindow = useAppearanceStore((s) => s.taskDisplayWindow);
  return {
    themeMode,
    themeVariant,
    terminalFontSize,
    uiFontFamily,
    monoFontFamily,
    attentionBadge,
    taskDisplayWindow,
  };
}

export { useAppearanceStore };
