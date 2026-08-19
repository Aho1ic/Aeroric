/** 任务行卡片:首页曾内联,现由项目页复用。点击进入任务详情。 */

import { router } from "expo-router";
import { Star } from "lucide-react-native";
import { StyleSheet, Text, View } from "react-native";
import { t } from "../i18n";
import type { Task } from "../types";
import { relativeTime } from "../ui/relative-time";
import { taskStatusMeta } from "../ui/task-status";
import { radii, spacing, theme, typography } from "../ui/theme";
import { AnimatedPressable } from "../ui/AnimatedPressable";

export function taskTitle(task: Task): string {
  return task.name?.trim() || task.prompt.trim().split("\n")[0] || t("home.unnamedTask");
}

export function TaskRow({ task }: { task: Task }) {
  const meta = taskStatusMeta(task.status);
  const title = taskTitle(task);
  const needsInput = task.status === "input_required";
  return (
    <AnimatedPressable
      style={({ pressed }) => [
        styles.taskRow,
        needsInput && styles.taskRowAttention,
        pressed && styles.taskRowPressed,
      ]}
      onPress={() =>
        router.push({
          pathname: "/task/[taskId]",
          params: { taskId: task.id, projectId: task.projectId, name: title },
        })
      }
    >
      <View style={[styles.statusDot, { backgroundColor: meta.color }]} />
      <View style={styles.taskBody}>
        <View style={styles.titleRow}>
          <Text style={styles.taskTitle} numberOfLines={1}>
            {title}
          </Text>
          {task.starred ? (
            <Star size={14} color={theme.warning} fill={theme.warning} strokeWidth={2} />
          ) : null}
        </View>
        <Text style={styles.taskMeta} numberOfLines={1}>
          <Text style={{ color: meta.color }}>{meta.label}</Text>
          {`  ·  ${task.agent}`}
          {task.selectedModel ? `  ·  ${task.selectedModel}` : ""}
          {task.reasoningEffort ? `  ·  ${task.reasoningEffort}` : ""}
          {`  ·  ${relativeTime(task.createdAt)}`}
        </Text>
        {task.worktreeBranch || task.additions !== undefined || task.deletions !== undefined ? (
          <Text style={styles.taskMeta} numberOfLines={1}>
            {task.worktreeBranch ? `branch ${task.worktreeBranch}` : ""}
            {task.additions !== undefined ? `  +${task.additions}` : ""}
            {task.deletions !== undefined ? `  -${task.deletions}` : ""}
          </Text>
        ) : null}
      </View>
    </AnimatedPressable>
  );
}

const styles = StyleSheet.create({
  taskRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    marginHorizontal: spacing.md,
    marginVertical: 3,
    paddingHorizontal: spacing.md,
    paddingVertical: 10,
    backgroundColor: theme.bgCard,
    borderRadius: radii.row,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
  },
  taskRowAttention: { borderColor: theme.warning, borderWidth: 1 },
  taskRowPressed: { opacity: 0.7 },
  statusDot: { width: 9, height: 9, borderRadius: radii.pill },
  taskBody: { flex: 1, gap: 2 },
  titleRow: { flexDirection: "row", alignItems: "center", gap: spacing.sm, minWidth: 0 },
  taskTitle: { flex: 1, color: theme.text, fontSize: 14.5, fontWeight: "600" },
  taskMeta: { color: theme.textHint, fontSize: typography.metaSize },
});
