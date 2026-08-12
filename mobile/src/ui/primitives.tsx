import type { ReactNode } from "react";
import {
  ActivityIndicator,
  StyleSheet,
  Text,
  TextInput,
  View,
  type TextInputProps,
  type StyleProp,
  type ViewStyle,
} from "react-native";
import { AnimatedPressable } from "./AnimatedPressable";
import { radii, spacing, theme, typography } from "./theme";

export function Button({
  label,
  onPress,
  disabled,
  tone = "primary",
  style,
}: {
  label: string;
  onPress: () => void;
  disabled?: boolean;
  tone?: "primary" | "secondary" | "danger";
  style?: StyleProp<ViewStyle>;
}) {
  return (
    <AnimatedPressable
      accessibilityRole="button"
      accessibilityLabel={label}
      disabled={disabled}
      onPress={onPress}
      style={[
        styles.button,
        tone === "primary"
          ? styles.primaryButton
          : tone === "danger"
            ? styles.dangerButton
            : styles.secondaryButton,
        disabled && styles.disabled,
        style,
      ]}
    >
      <Text
        style={[
          styles.buttonText,
          tone === "primary" ? styles.primaryButtonText : styles.secondaryButtonText,
        ]}
      >
        {label}
      </Text>
    </AnimatedPressable>
  );
}

export function IconButton({
  label,
  children,
  onPress,
  active,
}: {
  label: string;
  children: ReactNode;
  onPress: () => void;
  active?: boolean;
}) {
  return (
    <AnimatedPressable
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityState={{ selected: active }}
      hitSlop={8}
      onPress={onPress}
      style={[styles.iconButton, active && styles.active]}
    >
      {children}
    </AnimatedPressable>
  );
}

export function Field({
  label,
  error,
  ...props
}: TextInputProps & { label: string; error?: string }) {
  return (
    <View style={styles.field}>
      <Text style={styles.label}>{label}</Text>
      <TextInput
        placeholderTextColor={theme.textHint}
        accessibilityLabel={label}
        style={[styles.input, Boolean(error) && styles.inputError]}
        {...props}
      />
      {error ? <Text style={styles.error}>{error}</Text> : null}
    </View>
  );
}

export function StatusBadge({
  label,
  tone = "neutral",
}: {
  label: string;
  tone?: "neutral" | "accent" | "success" | "warning" | "danger";
}) {
  const color = {
    neutral: theme.textSecondary,
    accent: theme.accent,
    success: theme.success,
    warning: theme.warning,
    danger: theme.danger,
  }[tone];
  return (
    <View style={[styles.badge, { borderColor: color }]}>
      <Text style={[styles.badgeText, { color }]}>{label}</Text>
    </View>
  );
}

export function Sheet({ children, style }: { children: ReactNode; style?: StyleProp<ViewStyle> }) {
  return <View style={[styles.sheet, style]}>{children}</View>;
}

export function SegmentedControl<T extends string>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: readonly { value: T; label: string }[];
  onChange: (value: T) => void;
}) {
  return (
    <View accessibilityRole="radiogroup" style={styles.segmented}>
      {options.map((option) => {
        const selected = value === option.value;
        return (
          <AnimatedPressable
            key={option.value}
            accessibilityRole="radio"
            accessibilityState={{ selected }}
            onPress={() => onChange(option.value)}
            style={[styles.segment, selected && styles.segmentActive]}
          >
            <Text style={[styles.segmentText, selected && styles.segmentTextActive]}>
              {option.label}
            </Text>
          </AnimatedPressable>
        );
      })}
    </View>
  );
}

export function ListRow({
  children,
  onPress,
  selected,
  style,
}: {
  children: ReactNode;
  onPress: () => void;
  selected?: boolean;
  style?: StyleProp<ViewStyle>;
}) {
  return (
    <AnimatedPressable
      accessibilityRole="button"
      accessibilityState={{ selected }}
      onPress={onPress}
      style={[styles.listRow, selected && styles.active, style]}
    >
      {children}
    </AnimatedPressable>
  );
}

export function EmptyState({
  title,
  description,
  action,
}: {
  title: string;
  description?: string;
  action?: ReactNode;
}) {
  return (
    <View style={styles.empty}>
      <Text style={styles.emptyTitle}>{title}</Text>
      {description ? <Text style={styles.emptyDescription}>{description}</Text> : null}
      {action}
    </View>
  );
}

export function Spinner({ label }: { label?: string }) {
  return (
    <View accessibilityRole="progressbar" accessibilityLabel={label} style={styles.spinner}>
      <ActivityIndicator color={theme.accent} />
    </View>
  );
}

const styles = StyleSheet.create({
  button: {
    minHeight: 44,
    paddingHorizontal: spacing.lg,
    borderRadius: radii.button,
    alignItems: "center",
    justifyContent: "center",
    borderWidth: StyleSheet.hairlineWidth,
  },
  primaryButton: { backgroundColor: theme.accent, borderColor: theme.accentBorder },
  secondaryButton: { backgroundColor: theme.bgCard, borderColor: theme.border },
  dangerButton: { backgroundColor: theme.bgCard, borderColor: theme.danger },
  disabled: { opacity: 0.45 },
  buttonText: { fontSize: typography.bodySize, fontWeight: "700" },
  primaryButtonText: { color: theme.onAccent },
  secondaryButtonText: { color: theme.text },
  iconButton: {
    width: 44,
    height: 44,
    alignItems: "center",
    justifyContent: "center",
    borderRadius: radii.button,
  },
  active: { backgroundColor: theme.accentSoft },
  field: { gap: spacing.xs },
  label: { color: theme.textSecondary, fontSize: typography.metaSize, fontWeight: "600" },
  input: {
    minHeight: 44,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
    borderRadius: radii.input,
    backgroundColor: theme.bgCard,
    color: theme.text,
    paddingHorizontal: spacing.md,
    fontSize: typography.bodySize,
  },
  inputError: { borderColor: theme.danger },
  error: { color: theme.danger, fontSize: typography.labelSize },
  badge: {
    minHeight: 24,
    justifyContent: "center",
    paddingHorizontal: spacing.sm,
    borderRadius: radii.pill,
    borderWidth: StyleSheet.hairlineWidth,
  },
  badgeText: { fontSize: typography.labelSize, fontWeight: "700" },
  sheet: {
    borderRadius: radii.card,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
    backgroundColor: theme.bgCard,
    overflow: "hidden",
  },
  segmented: {
    flexDirection: "row",
    padding: 3,
    borderRadius: radii.button,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
    backgroundColor: theme.bgElevated,
  },
  segment: { flex: 1, minHeight: 44, alignItems: "center", justifyContent: "center" },
  segmentActive: { backgroundColor: theme.accent, borderRadius: radii.button - 2 },
  segmentText: { color: theme.textSecondary, fontWeight: "600" },
  segmentTextActive: { color: theme.onAccent },
  listRow: {
    minHeight: 44,
    flexDirection: "row",
    alignItems: "center",
    paddingHorizontal: spacing.md,
    paddingVertical: spacing.sm,
  },
  empty: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
    gap: spacing.sm,
    padding: spacing.xl,
  },
  emptyTitle: { color: theme.text, fontSize: typography.titleSize, fontWeight: "700" },
  emptyDescription: { color: theme.textSecondary, textAlign: "center", lineHeight: 20 },
  spinner: { minWidth: 44, minHeight: 44, alignItems: "center", justifyContent: "center" },
});
