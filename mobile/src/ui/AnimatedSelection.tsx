import { useEffect, useRef, useState } from "react";
import { Animated, Pressable, StyleSheet, Text, View, type LayoutChangeEvent } from "react-native";
import { radii, theme, typography } from "./theme";

export function AnimatedSelection<T extends string>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: readonly { value: T; label: string }[];
  onChange: (value: T) => void;
}) {
  const translateX = useRef(new Animated.Value(0)).current;
  const translateY = useRef(new Animated.Value(0)).current;
  const hasMeasured = useRef(false);
  const [layouts, setLayouts] = useState<Record<string, { x: number; y: number; width: number; height: number }>>({});
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
  }, [activeLayout?.x, activeLayout?.y, translateX, translateY]);

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

  return (
    <View style={styles.root}>
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
      {options.map((option) => {
        const active = option.value === value;
        return (
          <Pressable
            key={option.value}
            onLayout={(event) => recordLayout(String(option.value), event)}
            style={styles.item}
            onPress={() => onChange(option.value)}
            accessibilityRole="button"
            accessibilityState={{ selected: active }}
          >
            <Text style={[styles.text, active && styles.activeText]} numberOfLines={1}>
              {option.label}
            </Text>
          </Pressable>
        );
      })}
    </View>
  );
}

const styles = StyleSheet.create({
  root: {
    position: "relative",
    flexDirection: "row",
    padding: 3,
    borderRadius: radii.button,
    backgroundColor: theme.bgElevated,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
  },
  indicator: {
    position: "absolute",
    left: 0,
    top: 0,
    borderRadius: radii.button - 2,
    backgroundColor: theme.accent,
  },
  item: {
    flex: 1,
    minHeight: 34,
    alignItems: "center",
    justifyContent: "center",
    paddingHorizontal: 8,
    borderRadius: radii.button - 2,
    zIndex: 1,
  },
  text: { color: theme.textSecondary, fontSize: typography.metaSize, fontWeight: "600" },
  activeText: { color: theme.onAccent },
});
