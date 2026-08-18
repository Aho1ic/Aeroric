import type { ReactNode } from "react";
import { useEffect, useRef, useState } from "react";
import {
  Animated,
  ScrollView,
  StyleSheet,
  Text,
  View,
  type LayoutChangeEvent,
  type StyleProp,
  type ViewStyle,
} from "react-native";
import { AnimatedPressable } from "./AnimatedPressable";
import { radii, theme } from "./theme";

export function AnimatedSelection<T extends string>({
  value,
  options,
  onChange,
  compact = false,
  dense = false,
  horizontal = false,
  iconOnly = false,
  wrap = false,
  showDividers = false,
  style,
}: {
  value: T;
  options: readonly {
    value: T;
    label: string;
    description?: string;
    icon?: ReactNode;
    accessibilityLabel?: string;
  }[];
  onChange: (value: T) => void;
  compact?: boolean;
  /** 紧凑等宽的一行选项，适合推理强度等较多的短标签。 */
  dense?: boolean;
  /** 保持一行并支持横向滑动，选中项会自动滚到可视区域。 */
  horizontal?: boolean;
  iconOnly?: boolean;
  wrap?: boolean;
  /** Draw a subtle divider between adjacent options in a dense provider switch. */
  showDividers?: boolean;
  style?: StyleProp<ViewStyle>;
}) {
  const translateX = useRef(new Animated.Value(0)).current;
  const translateY = useRef(new Animated.Value(0)).current;
  const horizontalScrollRef = useRef<ScrollView>(null);
  const hasMeasured = useRef(false);
  const [horizontalWidth, setHorizontalWidth] = useState(0);
  const [layouts, setLayouts] = useState<
    Record<string, { x: number; y: number; width: number; height: number }>
  >({});
  const activeLayout = layouts[String(value)];

  useEffect(() => {
    if (!activeLayout) return;
    if (!hasMeasured.current) {
      translateX.setValue(activeLayout.x);
      translateY.setValue(activeLayout.y);
      hasMeasured.current = true;
      return;
    }
    Animated.parallel([
      Animated.timing(translateX, {
        toValue: activeLayout.x,
        duration: 180,
        useNativeDriver: true,
      }),
      Animated.timing(translateY, {
        toValue: activeLayout.y,
        duration: 180,
        useNativeDriver: true,
      }),
    ]).start();
  }, [activeLayout, translateX, translateY]);

  useEffect(() => {
    if (!horizontal || !activeLayout || horizontalWidth <= 0) return;
    const x = Math.max(0, activeLayout.x - (horizontalWidth - activeLayout.width) / 2);
    horizontalScrollRef.current?.scrollTo({ x, animated: hasMeasured.current });
  }, [activeLayout, horizontal, horizontalWidth]);

  const recordLayout = (key: string, event: LayoutChangeEvent) => {
    const { x, y, width, height } = event.nativeEvent.layout;
    setLayouts((previous) => {
      const current = previous[key];
      if (
        current &&
        current.x === x &&
        current.y === y &&
        current.width === width &&
        current.height === height
      ) {
        return previous;
      }
      return { ...previous, [key]: { x, y, width, height } };
    });
  };

  const selection = (
    <View
      style={[
        styles.root,
        compact && styles.rootCompact,
        wrap && styles.rootWrap,
        horizontal && [styles.rootHorizontal, { minWidth: horizontalWidth || undefined }],
        !horizontal && style,
      ]}
    >
      <Animated.View
        pointerEvents="none"
        style={[
          styles.indicator,
          {
            width: activeLayout?.width ?? 0,
            height: activeLayout?.height ?? 0,
            opacity: activeLayout ? 1 : 0,
            transform: [{ translateX }, { translateY }],
          },
        ]}
      />
      {options.map((option, index) => {
        const active = option.value === value;
        return (
          <AnimatedPressable
            key={option.value}
            onLayout={(event) => recordLayout(String(option.value), event)}
            style={[
              styles.item,
              compact && styles.itemCompact,
              dense && styles.itemDense,
              horizontal && styles.itemHorizontal,
              wrap && styles.itemWrap,
              iconOnly && styles.itemIconOnly,
              showDividers && index > 0 && styles.itemDivider,
            ]}
            onPress={() => onChange(option.value)}
            accessibilityRole="button"
            accessibilityLabel={option.accessibilityLabel ?? option.label}
            accessibilityState={{ selected: active }}
          >
            {option.icon ? <View style={styles.icon}>{option.icon}</View> : null}
            {iconOnly ? null : (
              <View style={styles.optionCopy}>
                <Text style={[styles.text, active && styles.activeText]} numberOfLines={1}>
                  {option.label}
                </Text>
                {option.description ? (
                  <Text
                    style={[styles.description, active && styles.activeDescription]}
                    numberOfLines={2}
                  >
                    {option.description}
                  </Text>
                ) : null}
              </View>
            )}
          </AnimatedPressable>
        );
      })}
    </View>
  );

  if (!horizontal) return selection;

  return (
    <ScrollView
      ref={horizontalScrollRef}
      horizontal
      style={[styles.horizontalScroll, style]}
      contentContainerStyle={styles.horizontalContent}
      showsHorizontalScrollIndicator={false}
      onLayout={(event) => setHorizontalWidth(event.nativeEvent.layout.width)}
      keyboardShouldPersistTaps="handled"
    >
      {selection}
    </ScrollView>
  );
}

const styles = StyleSheet.create({
  root: {
    position: "relative",
    flexDirection: "row",
    alignItems: "stretch",
    padding: 3,
    borderRadius: radii.button,
    backgroundColor: theme.bgElevated,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
  },
  rootCompact: { alignSelf: "flex-start" },
  rootWrap: { flexWrap: "wrap", justifyContent: "center", gap: 3 },
  rootHorizontal: { alignSelf: "flex-start", flexWrap: "nowrap" },
  horizontalScroll: { width: "100%" },
  horizontalContent: { flexGrow: 1 },
  indicator: {
    position: "absolute",
    left: 0,
    top: 0,
    borderRadius: radii.button - 2,
    backgroundColor: theme.accent,
    // 软阴影让选中态更有层次。不加 elevation:Android 上 elevation 会盖过
    // item 的 zIndex,把标签压到指示器下面。
    shadowColor: theme.accent,
    shadowOpacity: 0.35,
    shadowRadius: 8,
    shadowOffset: { width: 0, height: 2 },
  },
  item: {
    flex: 1,
    flexDirection: "row",
    gap: 6,
    minHeight: 38,
    alignItems: "center",
    justifyContent: "center",
    paddingHorizontal: 8,
    paddingVertical: 5,
    borderRadius: radii.button - 2,
    zIndex: 1,
  },
  itemCompact: { flex: 0 },
  itemDense: { minHeight: 34, paddingHorizontal: 3, paddingVertical: 4 },
  itemHorizontal: { flex: 0, minWidth: 88, maxWidth: 260, paddingHorizontal: 12 },
  itemWrap: { flex: 0, minWidth: 72, maxWidth: "100%", flexShrink: 1 },
  itemIconOnly: { width: 38, minWidth: 38, paddingHorizontal: 5 },
  itemDivider: {
    borderLeftWidth: StyleSheet.hairlineWidth,
    borderLeftColor: theme.border,
  },
  icon: { alignItems: "center", justifyContent: "center" },
  // flexShrink 而非 flex:1 —— flex:1 会把 Text 撑满整个 item,文字随之左对齐
  optionCopy: { flexShrink: 1, minWidth: 0, alignItems: "center", gap: 2 },
  text: {
    color: theme.textSecondary,
    fontSize: 13.5,
    fontWeight: "600",
    textAlign: "center",
    includeFontPadding: false,
    textAlignVertical: "center",
  },
  activeText: { color: theme.onAccent, fontWeight: "700" },
  description: {
    color: theme.textHint,
    fontSize: 11,
    lineHeight: 15,
    textAlign: "center",
    includeFontPadding: false,
  },
  activeDescription: { color: theme.onAccent, opacity: 0.82 },
});
