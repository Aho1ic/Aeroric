import { Link, Stack, router, useFocusEffect } from "expo-router";
import { useCallback, useMemo, useState } from "react";
import {
  Pressable,
  RefreshControl,
  SectionList,
  StyleSheet,
  Text,
  View,
} from "react-native";
import { t } from "../src/i18n";
import { useConnection } from "../src/state/connection-context";
import { useHosts } from "../src/state/hosts-context";
import { useHostTasks, type ProjectTasks } from "../src/state/use-host-tasks";
import type { Task } from "../src/types";
import { relativeTime } from "../src/ui/relative-time";
import { taskStatusMeta, taskStatusRank } from "../src/ui/task-status";
import { theme } from "../src/ui/theme";

function ConnectionBanner() {
  const { status, authError } = useConnection();
  const meta = useMemo(() => {
    switch (status) {
      case "online":
        return { text: t("home.online"), color: theme.success };
      case "connecting":
      case "authenticating":
        return { text: t("home.connecting"), color: theme.accent };
      case "reconnecting":
        return { text: t("home.reconnecting"), color: theme.warning };
      case "unauthorized":
        return { text: authError ?? t("home.authExpired"), color: theme.danger };
      default:
        return null;
    }
  }, [authError, status]);
  if (!meta) return null;
  return (
    <View style={styles.banner}>
      <View style={[styles.bannerDot, { backgroundColor: meta.color }]} />
      <Text style={[styles.bannerText, { color: meta.color }]}>{meta.text}</Text>
      {status === "unauthorized" ? (
        <Link href="/pair" asChild>
          <Pressable hitSlop={8}>
            <Text style={styles.bannerAction}>{t("home.rePairAction")}</Text>
          </Pressable>
        </Link>
      ) : null}
    </View>
  );
}

function TaskRow({ task }: { task: Task }) {
  const meta = taskStatusMeta(task.status);
  const title = task.name?.trim() || task.prompt.trim().split("\n")[0] || t("home.unnamedTask");
  const needsInput = task.status === "input_required";
  return (
    <Pressable
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
        <Text style={styles.taskTitle} numberOfLines={1}>
          {title}
        </Text>
        <Text style={styles.taskMeta} numberOfLines={1}>
          <Text style={{ color: meta.color }}>{meta.label}</Text>
          {`  ·  ${task.agent}  ·  ${relativeTime(task.createdAt)}`}
        </Text>
      </View>
    </Pressable>
  );
}

function EmptyState({ hasHosts }: { hasHosts: boolean }) {
  if (!hasHosts) {
    return (
      <View style={styles.emptyWrap}>
        <Text style={styles.emptyTitle}>Aeroric</Text>
        <Text style={styles.emptyText}>{t("home.pairIntro")}</Text>
        <Link href="/pair" asChild>
          <Pressable style={styles.primaryButton}>
            <Text style={styles.primaryButtonText}>{t("home.pairNow")}</Text>
          </Pressable>
        </Link>
      </View>
    );
  }
  return (
    <View style={styles.emptyWrap}>
      <Text style={styles.emptyText}>{t("home.emptyTasks")}</Text>
    </View>
  );
}

export default function TaskDashboard() {
  const { ready, hosts, activeHost } = useHosts();
  const { sections, loading, error, refresh } = useHostTasks();
  // 项目默认全部折叠;expanded 记录用户点开的项目 id
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set());

  const toggleProject = useCallback((projectId: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(projectId)) next.delete(projectId);
      else next.add(projectId);
      return next;
    });
  }, []);

  // 从新建/详情页返回时同步一次(推送覆盖大多数场景,这里兜底)
  useFocusEffect(
    useCallback(() => {
      refresh();
    }, [refresh]),
  );

  const listSections = useMemo(
    () =>
      sections
        .filter((s: ProjectTasks) => s.tasks.length > 0)
        .map((s: ProjectTasks) => {
          const isExpanded = expanded.has(s.project.id);
          return {
            key: s.project.id,
            projectId: s.project.id,
            title: s.project.name,
            taskCount: s.tasks.length,
            needsInput: s.tasks.some((task) => task.status === "input_required"),
            isExpanded,
            data: isExpanded
              ? [...s.tasks].sort(
                  (a, b) =>
                    taskStatusRank(a.status) - taskStatusRank(b.status) ||
                    b.createdAt - a.createdAt,
                )
              : [],
          };
        }),
    [expanded, sections],
  );

  if (!ready) return <View style={styles.screen} />;

  return (
    <View style={styles.screen}>
      <Stack.Screen
        options={{
          headerRight: () =>
            hosts.length > 0 ? (
              <View style={styles.headerActions}>
                <Link href="/new-task" asChild>
                  <Pressable hitSlop={8}>
                    <Text style={styles.headerNew}>＋</Text>
                  </Pressable>
                </Link>
                <Link href="/hosts" asChild>
                  <Pressable hitSlop={8}>
                    <Text style={styles.headerAction}>
                      {activeHost?.name ?? t("home.hostsFallback")}
                    </Text>
                  </Pressable>
                </Link>
              </View>
            ) : null,
        }}
      />
      {hosts.length > 0 ? <ConnectionBanner /> : null}
      {error ? <Text style={styles.errorText}>{error}</Text> : null}
      <SectionList
        sections={listSections}
        keyExtractor={(item) => item.id}
        renderItem={({ item }) => <TaskRow task={item} />}
        renderSectionHeader={({ section }) => (
          <Pressable
            style={({ pressed }) => [styles.sectionHeader, pressed && styles.sectionHeaderPressed]}
            accessibilityRole="button"
            accessibilityLabel={
              section.isExpanded
                ? t("home.collapseProject", { name: section.title })
                : t("home.expandProject", { name: section.title })
            }
            onPress={() => toggleProject(section.projectId)}
          >
            <Text style={styles.sectionChevron}>{section.isExpanded ? "▾" : "▸"}</Text>
            <Text style={styles.sectionTitle} numberOfLines={1}>
              {section.title}
            </Text>
            {!section.isExpanded && section.needsInput ? (
              <View style={styles.sectionAttentionDot} />
            ) : null}
            <Text style={styles.sectionCount}>{section.taskCount}</Text>
          </Pressable>
        )}
        ListEmptyComponent={<EmptyState hasHosts={hosts.length > 0} />}
        contentContainerStyle={listSections.length === 0 ? styles.listEmpty : styles.list}
        refreshControl={
          <RefreshControl
            refreshing={loading}
            onRefresh={refresh}
            tintColor={theme.textSecondary}
          />
        }
        stickySectionHeadersEnabled={false}
      />
    </View>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1, backgroundColor: theme.bg },
  banner: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
    paddingHorizontal: 16,
    paddingVertical: 8,
    backgroundColor: theme.bgCard,
    borderBottomWidth: StyleSheet.hairlineWidth,
    borderBottomColor: theme.border,
  },
  bannerDot: { width: 8, height: 8, borderRadius: 4 },
  bannerText: { fontSize: 12.5, flex: 1 },
  bannerAction: { fontSize: 12.5, color: theme.accent, fontWeight: "600" },
  headerAction: { color: theme.accent, fontSize: 14, fontWeight: "600" },
  headerActions: { flexDirection: "row", alignItems: "center", gap: 16 },
  headerNew: { color: theme.accent, fontSize: 22, fontWeight: "600", marginTop: -2 },
  list: { paddingBottom: 32 },
  listEmpty: { flexGrow: 1, justifyContent: "center" },
  sectionHeader: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
    marginHorizontal: 12,
    marginTop: 10,
    paddingHorizontal: 12,
    paddingVertical: 11,
    backgroundColor: theme.bgCard,
    borderRadius: 10,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
  },
  sectionHeaderPressed: { opacity: 0.7 },
  sectionChevron: { color: theme.textSecondary, fontSize: 12, width: 14 },
  sectionTitle: {
    flex: 1,
    fontSize: 13.5,
    fontWeight: "700",
    color: theme.text,
  },
  sectionAttentionDot: {
    width: 8,
    height: 8,
    borderRadius: 4,
    backgroundColor: theme.warning,
  },
  sectionCount: {
    minWidth: 22,
    textAlign: "center",
    fontSize: 11.5,
    fontWeight: "600",
    color: theme.textSecondary,
    backgroundColor: theme.bgElevated,
    borderRadius: 9,
    paddingHorizontal: 6,
    paddingVertical: 2,
    overflow: "hidden",
  },
  taskRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    marginHorizontal: 12,
    marginVertical: 3,
    paddingHorizontal: 12,
    paddingVertical: 10,
    backgroundColor: theme.bgCard,
    borderRadius: 10,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
  },
  taskRowAttention: { borderColor: theme.warning, borderWidth: 1 },
  taskRowPressed: { opacity: 0.7 },
  statusDot: { width: 9, height: 9, borderRadius: 4.5 },
  taskBody: { flex: 1, gap: 2 },
  taskTitle: { color: theme.text, fontSize: 14.5, fontWeight: "600" },
  taskMeta: { color: theme.textHint, fontSize: 12 },
  emptyWrap: { alignItems: "center", paddingHorizontal: 32, gap: 14 },
  emptyTitle: { color: theme.text, fontSize: 26, fontWeight: "700" },
  emptyText: { color: theme.textSecondary, fontSize: 13.5, lineHeight: 21, textAlign: "center" },
  primaryButton: {
    backgroundColor: theme.accent,
    borderRadius: 10,
    paddingHorizontal: 26,
    paddingVertical: 12,
    marginTop: 6,
  },
  primaryButtonText: { color: "#fff", fontSize: 15, fontWeight: "600" },
  errorText: { color: theme.danger, fontSize: 12, paddingHorizontal: 16, paddingTop: 8 },
});
