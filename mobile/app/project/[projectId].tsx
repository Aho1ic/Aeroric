/**
 * 项目页:该项目下的任务列表 + 右上角文件浏览 / git 变更入口。
 * 数据复用 useHostTasks(首页同一份缓存与推送补丁),按 projectId 过滤。
 */

import { Stack, router, useLocalSearchParams } from "expo-router";
import { FolderTree, GitBranch, Plus, Search } from "lucide-react-native";
import { useCallback, useMemo, useState } from "react";
import { FlatList, RefreshControl, StyleSheet, Text, TextInput, View } from "react-native";
import { ChangesPane } from "../../src/changes/ChangesPane";
import { NewTaskSheet } from "../../src/components/NewTaskSheet";
import { TaskRow } from "../../src/components/TaskRow";
import { t } from "../../src/i18n";
import { useHostTasks } from "../../src/state/use-host-tasks";
import type { Task } from "../../src/types";
import { AnimatedSelection } from "../../src/ui/AnimatedSelection";
import { HeaderActions, HeaderIconButton } from "../../src/ui/HeaderIconButton";
import { filterTasks, type TaskListFilter } from "../../src/ui/filter-tasks";
import { EmptyState } from "../../src/ui/primitives";
import { taskStatusRank } from "../../src/ui/task-status";
import { spacing, theme, typography } from "../../src/ui/theme";

export default function ProjectScreen() {
  const params = useLocalSearchParams<{ projectId: string; name?: string }>();
  const projectId = typeof params.projectId === "string" ? params.projectId : "";
  const fallbackName = typeof params.name === "string" ? params.name : "";
  const { sections, loading, error, refresh, upsertTask } = useHostTasks();
  const [showChanges, setShowChanges] = useState(false);
  const [newTaskOpen, setNewTaskOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<TaskListFilter>("all");

  const section = useMemo(
    () => sections.find((s) => s.project.id === projectId),
    [projectId, sections],
  );

  const sortedTasks = useMemo(
    () =>
      [...(section?.tasks ?? [])].sort(
        (a, b) => taskStatusRank(a.status) - taskStatusRank(b.status) || b.createdAt - a.createdAt,
      ),
    [section],
  );
  const tasks = useMemo(
    () => filterTasks(sortedTasks, query, filter),
    [filter, query, sortedTasks],
  );

  const title = section?.project.name || fallbackName || t("home.projects");

  const openFiles = useCallback(() => {
    router.push({ pathname: "/files", params: { projectId, name: title } });
  }, [projectId, title]);

  const onCreated = useCallback(
    (task: Task) => {
      upsertTask(task);
      void refresh();
    },
    [refresh, upsertTask],
  );

  return (
    <View style={styles.screen}>
      <Stack.Screen
        options={{
          title,
          headerRight: () => (
            <HeaderActions>
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
            </HeaderActions>
          ),
        }}
      />

      {error ? <Text style={styles.errorText}>{error}</Text> : null}

      {showChanges ? (
        <ChangesPane projectId={projectId} active />
      ) : (
        <View style={styles.taskPane}>
          {sortedTasks.length > 0 ? (
            <View style={styles.taskTools}>
              <View style={styles.searchBox}>
                <Search size={16} color={theme.textHint} />
                <TextInput
                  value={query}
                  onChangeText={setQuery}
                  style={styles.searchInput}
                  placeholder={t("project.searchTasks")}
                  placeholderTextColor={theme.textHint}
                  autoCorrect={false}
                  autoCapitalize="none"
                  returnKeyType="search"
                  clearButtonMode="while-editing"
                />
              </View>
              <AnimatedSelection
                value={filter}
                options={(
                  [
                    ["all", "project.filter.all"],
                    ["active", "project.filter.active"],
                    ["completed", "project.filter.completed"],
                    ["starred", "project.filter.starred"],
                  ] as const
                ).map(([value, key]) => ({ value, label: t(key) }))}
                onChange={setFilter}
                dense
                style={styles.filterSelection}
              />
            </View>
          ) : null}
          <FlatList
            data={tasks}
            keyExtractor={(item) => item.id}
            renderItem={({ item }) => <TaskRow task={item} />}
            ListEmptyComponent={
              <EmptyState
                title={sortedTasks.length === 0 ? t("project.empty") : t("project.noMatchingTasks")}
              />
            }
            contentContainerStyle={tasks.length === 0 ? styles.listEmpty : styles.list}
            keyboardShouldPersistTaps="handled"
            refreshControl={
              <RefreshControl
                refreshing={loading}
                onRefresh={refresh}
                tintColor={theme.textSecondary}
              />
            }
          />
        </View>
      )}

      <NewTaskSheet
        visible={newTaskOpen}
        lockedProjectId={projectId}
        onClose={() => setNewTaskOpen(false)}
        onCreated={onCreated}
      />
    </View>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1, backgroundColor: theme.bg },
  taskPane: { flex: 1 },
  taskTools: {
    paddingHorizontal: spacing.md,
    paddingTop: spacing.sm,
    paddingBottom: spacing.xs,
    gap: spacing.sm,
    borderBottomWidth: StyleSheet.hairlineWidth,
    borderBottomColor: theme.border,
  },
  searchBox: {
    height: 38,
    flexDirection: "row",
    alignItems: "center",
    gap: spacing.sm,
    paddingHorizontal: spacing.md,
    borderRadius: 6,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
    backgroundColor: theme.bgCard,
  },
  searchInput: { flex: 1, color: theme.text, fontSize: 13.5, paddingVertical: 0 },
  filterSelection: { width: "100%" },
  list: { paddingTop: spacing.sm, paddingBottom: 32 },
  listEmpty: { flexGrow: 1, justifyContent: "center" },
  errorText: {
    color: theme.danger,
    fontSize: typography.metaSize,
    paddingHorizontal: spacing.lg,
    paddingTop: spacing.sm,
  },
});
