/**
 * 导航栏方形图标按钮:32×32 居中容器,内容(图标或文字)天然居中。
 * 项目页、任务详情页共用,保证右上角按钮视觉一致。
 */

import type { ReactNode } from "react";
import { Pressable, StyleSheet, View } from "react-native";
import { radii, spacing, theme } from "./theme";

export function HeaderIconButton({
  label,
  active,
  disabled,
  onPress,
  children,
}: {
  label: string;
  active?: boolean;
  disabled?: boolean;
  onPress: () => void;
  children: ReactNode;
}) {
  return (
    <Pressable
      hitSlop={8}
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityState={{ selected: !!active, disabled: !!disabled }}
      disabled={disabled}
      onPress={onPress}
      style={({ pressed }) => [
        styles.iconButton,
        active && styles.iconButtonActive,
        disabled && styles.iconButtonDisabled,
        pressed && styles.pressed,
      ]}
    >
      {children}
    </Pressable>
  );
}

export function HeaderActions({ children }: { children: ReactNode }) {
  return <View style={styles.headerActions}>{children}</View>;
}

const styles = StyleSheet.create({
  headerActions: { flexDirection: "row", alignItems: "center", gap: spacing.sm },
  iconButton: {
    width: 32,
    height: 32,
    borderRadius: radii.button,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: theme.bgElevated,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
  },
  iconButtonActive: { backgroundColor: theme.accent, borderColor: theme.accent },
  iconButtonDisabled: { opacity: 0.45 },
  pressed: { opacity: 0.7 },
});
