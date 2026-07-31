/**
 * 项目页:该项目下的任务列表 + 右上角文件浏览 / git 变更入口。
 * 数据复用 useHostTasks(首页同一份缓存与推送补丁),按 projectId 过滤。
 */

import { Stack, router, useLocalSearchParams } from "expo-router";
import { FolderTree, GitBranch, Plus } from "lucide-react-native";
import { useCallback, useMemo, useState } from "react";
import { FlatList, Pressable, RefreshControl, StyleSheet, Text, View } from "react-native";
import { ChangesPane } from "../../src/changes/ChangesPane";
import { NewTaskSheet } from "../../src/components/NewTaskSheet";
import { TaskRow } from "../../src/components/TaskRow";
import { t } from "../../src/i18n";
import { useHostTasks } from "../../src/state/use-host-tasks";
import { taskStatusRank } from "../../src/ui/task-status";
import { radii, spacing, theme, typography } from "../../src/ui/theme";

function HeaderIconButton({
  label,
  active,
  onPress,
  children,
}: {
  label: string;
  active?: boolean;
  onPress: () => void;
  children: React.ReactNode;
}) {
  return (
    <Pressable
      hitSlop={8}
      accessibilityRole="button"
      accessibilityLabel={label}
      onPress={onPress}
      style={({ pressed }) => [
        styles.iconButton,
        active && styles.iconButtonActive,
        pressed && styles.pressed,
      ]}
    >
      {children}
    </Pressable>
  );
}

export default function ProjectScreen() {
  const params = useLocalSearchParams<{ projectId: string; name?: string }>();
  const projectId = typeof params.projectId === "string" ? params.projectId : "";
  const fallbackName = typeof params.name === "string" ? params.name : "";
  const { sections, loading, error, refresh } = useHostTasks();
  const [showChanges, setShowChanges] = useState(false);
  const [newTaskOpen, setNewTaskOpen] = useState(false);

  const section = useMemo(
    () => sections.find((s) => s.project.id === projectId),
    [projectId, sections],
  );

  const tasks = useMemo(
    () =>
      [...(section?.tasks ?? [])].sort(
        (a, b) => taskStatusRank(a.status) - taskStatusRank(b.status) || b.createdAt - a.createdAt,
      ),
    [section],
  );

  const title = section?.project.name || fallbackName || t("home.projects");

  const openFiles = useCallback(() => {
    router.push({ pathname: "/files", params: { projectId, name: title } });
  }, [projectId, title]);

  return (
    <View style={styles.screen}>
      <Stack.Screen
        options={{
          title,
          headerRight: () => (
            <View style={styles.headerActions}>
              <HeaderIconButton label={t("project.browseFiles")} onPress={openFiles}>
                <FolderTree size={17} color={theme.text} />
              </HeaderIconButton>
              <HeaderIconButton
                label={t("project.viewChanges")}
                active={showChanges}
                onPress={() => setShowChanges((prev) => !prev)}
              >
                <GitBranch size={17} color={showChanges ? theme.onAccent : theme.text} />
              </HeaderIconButton>
              <HeaderIconButton
                label={t("home.newTaskFor", { name: title })}
                onPress={() => setNewTaskOpen(true)}
              >
                <Plus size={18} color={theme.text} strokeWidth={2.4} />
              </HeaderIconButton>
            </View>
          ),
        }}
      />

      {error ? <Text style={styles.errorText}>{error}</Text> : null}

      {showChanges ? (
        <ChangesPane projectId={projectId} active />
      ) : (
        <FlatList
          data={tasks}
          keyExtractor={(item) => item.id}
          renderItem={({ item }) => <TaskRow task={item} />}
          ListEmptyComponent={
            <View style={styles.emptyWrap}>
              <Text style={styles.emptyText}>{t("project.empty")}</Text>
            </View>
          }
          contentContainerStyle={tasks.length === 0 ? styles.listEmpty : styles.list}
          refreshControl={
            <RefreshControl
              refreshing={loading}
              onRefresh={refresh}
              tintColor={theme.textSecondary}
            />
          }
        />
      )}

      <NewTaskSheet
        visible={newTaskOpen}
        lockedProjectId={projectId}
        onClose={() => setNewTaskOpen(false)}
        onCreated={refresh}
      />
    </View>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1, backgroundColor: theme.bg },
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
  pressed: { opacity: 0.7 },
  list: { paddingTop: spacing.sm, paddingBottom: 32 },
  listEmpty: { flexGrow: 1, justifyContent: "center" },
  emptyWrap: { alignItems: "center", paddingHorizontal: 32 },
  emptyText: { color: theme.textSecondary, fontSize: 13.5, lineHeight: 21, textAlign: "center" },
  errorText: {
    color: theme.danger,
    fontSize: typography.metaSize,
    paddingHorizontal: spacing.lg,
    paddingTop: spacing.sm,
  },
});
