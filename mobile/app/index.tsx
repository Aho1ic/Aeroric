/**
 * 首页:品牌头部 + 统计概览 + 项目列表(点击进项目页,右侧 ＋ 直接建任务)。
 * 任务不再内嵌在首页,改由 app/project/[projectId].tsx 展示。
 */

import { Link, Stack, router, useFocusEffect } from "expo-router";
import { ChevronDown, ChevronRight, Pin, Plus, SlidersHorizontal } from "lucide-react-native";
import { useCallback, useMemo, useState, type ReactNode } from "react";
import { Image, RefreshControl, SectionList, StyleSheet, Text, View } from "react-native";
import appIcon from "../assets/icon.png";
import { NewTaskSheet } from "../src/components/NewTaskSheet";
import { t } from "../src/i18n";
import { useConnection } from "../src/state/connection-context";
import { useHostStats } from "../src/state/use-host-stats";
import { useHosts } from "../src/state/hosts-context";
import { useHostTasks, type ProjectTasks } from "../src/state/use-host-tasks";
import type { Task } from "../src/types";
import { formatCount, formatDuration } from "../src/ui/format-duration";
import {
  UNGROUPED_PROJECT_GROUP,
  groupProjectEntries,
  visibleGroupEntries,
} from "../src/ui/group-projects";
import { radii, spacing, theme, typography } from "../src/ui/theme";
import { AnimatedPressable } from "../src/ui/AnimatedPressable";

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
          <AnimatedPressable hitSlop={8}>
            <Text style={styles.bannerAction}>{t("home.rePairAction")}</Text>
          </AnimatedPressable>
        </Link>
      ) : null}
    </View>
  );
}

function StatCard({
  value,
  icon,
  label,
  onPress,
  accessibilityLabel,
}: {
  value?: string;
  icon?: ReactNode;
  label: string;
  onPress?: () => void;
  accessibilityLabel?: string;
}) {
  const body = (
    <>
      {icon ? (
        <View style={styles.statIcon}>{icon}</View>
      ) : (
        <Text style={styles.statValue} numberOfLines={1}>
          {value}
        </Text>
      )}
      <Text style={styles.statLabel} numberOfLines={1}>
        {label}
      </Text>
    </>
  );
  if (!onPress) return <View style={styles.statCard}>{body}</View>;
  return (
    <AnimatedPressable
      style={({ pressed }) => [styles.statCard, pressed && styles.pressed]}
      accessibilityRole="button"
      accessibilityLabel={accessibilityLabel ?? label}
      onPress={onPress}
    >
      {body}
    </AnimatedPressable>
  );
}

function ProjectCard({
  section,
  onNewTask,
  onTogglePinned,
}: {
  section: ProjectTasks;
  onNewTask: (projectId: string) => void;
  onTogglePinned: (projectId: string, pinned: boolean) => void;
}) {
  const { project, tasks } = section;
  const needsInput = tasks.some((task) => task.status === "input_required");
  const activeCount = tasks.filter((task) =>
    ["pending", "running", "input_required"].includes(task.status),
  ).length;
  const pinned = Boolean(project.pinned);
  return (
    <AnimatedPressable
      style={({ pressed }) => [styles.projectCard, pressed && styles.pressed]}
      accessibilityRole="button"
      accessibilityLabel={t("home.openProject", { name: project.name })}
      onPress={() =>
        router.push({
          pathname: "/project/[projectId]",
          params: { projectId: project.id, name: project.name },
        })
      }
    >
      <View style={styles.projectBody}>
        <View style={styles.projectTitleRow}>
          <Text style={styles.projectName} numberOfLines={1}>
            {project.name}
          </Text>
          {needsInput ? <View style={styles.attentionDot} /> : null}
        </View>
        <Text style={styles.projectMeta} numberOfLines={1}>
          {project.path}
        </Text>
        <Text style={styles.projectMeta} numberOfLines={1}>
          {t("home.projectTaskCount", { count: tasks.length })}
          {activeCount > 0 ? `  ·  ${activeCount} ${t("status.running")}` : ""}
        </Text>
      </View>
      <AnimatedPressable
        hitSlop={6}
        style={({ pressed }) => [
          styles.addButton,
          pinned && styles.pinButtonActive,
          pressed && styles.pressed,
        ]}
        accessibilityRole="button"
        accessibilityState={{ selected: pinned }}
        accessibilityLabel={t(pinned ? "home.unpinProject" : "home.pinProject", {
          name: project.name,
        })}
        onPress={(e) => {
          e.stopPropagation();
          onTogglePinned(project.id, !pinned);
        }}
      >
        <Pin
          size={13}
          color={pinned ? theme.onAccent : theme.textSecondary}
          strokeWidth={pinned ? 2.6 : 2}
        />
      </AnimatedPressable>
      <AnimatedPressable
        hitSlop={6}
        style={({ pressed }) => [styles.addButton, pressed && styles.pressed]}
        accessibilityRole="button"
        accessibilityLabel={t("home.newTaskFor", { name: project.name })}
        onPress={(e) => {
          // 阻止冒泡到卡片,否则会同时导航进项目页
          e.stopPropagation();
          onNewTask(project.id);
        }}
      >
        <Plus size={15} color={theme.text} strokeWidth={2.4} />
      </AnimatedPressable>
    </AnimatedPressable>
  );
}

function GroupHeader({
  name,
  count,
  collapsed,
  onToggle,
}: {
  name: string;
  count: number;
  collapsed: boolean;
  onToggle: () => void;
}) {
  const label = name === UNGROUPED_PROJECT_GROUP ? t("home.group.ungrouped") : name;
  const Chevron = collapsed ? ChevronRight : ChevronDown;
  return (
    <AnimatedPressable
      style={({ pressed }) => [styles.groupHeader, pressed && styles.pressed]}
      accessibilityRole="button"
      accessibilityState={{ expanded: !collapsed }}
      accessibilityLabel={t(collapsed ? "home.expandGroup" : "home.collapseGroup", { name: label })}
      onPress={onToggle}
    >
      <Chevron size={14} color={theme.textSecondary} strokeWidth={2.4} />
      <Text style={styles.groupHeaderText} numberOfLines={1}>
        {label}
      </Text>
      <Text style={styles.groupHeaderCount}>{count}</Text>
    </AnimatedPressable>
  );
}

function PairPrompt() {
  return (
    <View style={styles.emptyWrap}>
      <Image source={appIcon} style={styles.emptyLogo} />
      <Text style={styles.emptyTitle}>Aeroric</Text>
      <Text style={styles.emptyText}>{t("home.pairIntro")}</Text>
      <Link href="/pair" asChild>
        <AnimatedPressable style={styles.primaryButton}>
          <Text style={styles.primaryButtonText}>{t("home.pairNow")}</Text>
        </AnimatedPressable>
      </Link>
    </View>
  );
}

export default function HomeScreen() {
  const { ready, hosts, activeHost } = useHosts();
  const { sections, loading, error, refresh, upsertTask, setPinned } = useHostTasks();
  const { stats, refresh: refreshStats } = useHostStats();
  const [newTaskProjectId, setNewTaskProjectId] = useState<string | null>(null);
  // 默认全部折叠；仅记录用户主动展开的分组，新出现的分组也自然保持折叠。
  // 置顶项目的折叠可见规则仍由 visibleGroupEntries 统一处理。
  const [expandedGroups, setExpandedGroups] = useState<Set<string>>(() => new Set());

  // 从新建/详情页返回时同步一次(推送覆盖大多数场景,这里兜底)
  useFocusEffect(
    useCallback(() => {
      refresh();
      refreshStats();
    }, [refresh, refreshStats]),
  );

  const onRefresh = useCallback(() => {
    void refresh();
    refreshStats();
  }, [refresh, refreshStats]);

  const onCreated = useCallback(
    (task: Task) => {
      upsertTask(task);
      void refresh();
      refreshStats();
    },
    [refresh, refreshStats, upsertTask],
  );

  const toggleGroup = useCallback((name: string) => {
    setExpandedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  }, []);

  const groups = useMemo(() => groupProjectEntries(sections), [sections]);
  // 折叠的分组仍保留置顶项目,所以 data 可能少于 entries;分组头计数用整组数量
  const listSections = useMemo(
    () =>
      groups.map((group) => ({
        ...group,
        data: visibleGroupEntries(group, !expandedGroups.has(group.name)),
      })),
    [expandedGroups, groups],
  );
  // 只有「未分组」一组时不必显示分组头,避免平铺列表多出一行噪音
  const showGroupHeaders = groups.length > 1 || (groups.length === 1 && !groups[0].isUngrouped);

  if (!ready) return <View style={styles.screen} />;

  const hasHosts = hosts.length > 0;

  return (
    <View style={styles.screen}>
      <Stack.Screen options={{ headerShown: false }} />
      <View style={styles.brandBar}>
        <Image source={appIcon} style={styles.brandLogo} />
        <Text style={styles.brandName}>Aeroric</Text>
        <View style={styles.brandSpacer} />
        {hasHosts ? (
          <Link href="/hosts" asChild>
            <AnimatedPressable hitSlop={8}>
              <Text style={styles.headerAction}>{activeHost?.name ?? t("home.hostsFallback")}</Text>
            </AnimatedPressable>
          </Link>
        ) : null}
      </View>

      {hasHosts ? <ConnectionBanner /> : null}
      {error ? <Text style={styles.errorText}>{error}</Text> : null}

      <SectionList
        sections={hasHosts ? listSections : []}
        keyExtractor={(item) => item.project.id}
        stickySectionHeadersEnabled={false}
        renderItem={({ item }) => (
          <ProjectCard section={item} onNewTask={setNewTaskProjectId} onTogglePinned={setPinned} />
        )}
        renderSectionHeader={({ section }) =>
          showGroupHeaders ? (
            <GroupHeader
              name={section.name}
              count={section.entries.length}
              collapsed={!expandedGroups.has(section.name)}
              onToggle={() => toggleGroup(section.name)}
            />
          ) : null
        }
        ListHeaderComponent={
          hasHosts ? (
            <View>
              <View style={styles.statsRow}>
                <StatCard value={formatCount(sections.length)} label={t("home.statProjects")} />
                <StatCard
                  value={stats ? formatDuration(stats.agentTimeMs) : "—"}
                  label={t("home.statAgentTime")}
                />
                <StatCard
                  icon={<SlidersHorizontal size={20} color={theme.text} strokeWidth={2.2} />}
                  label={t("home.statAgentConfig")}
                  onPress={() => router.push("/agent-config")}
                />
              </View>
              {sections.length > 0 ? (
                <Text style={styles.sectionLabel}>{t("home.projects")}</Text>
              ) : null}
            </View>
          ) : null
        }
        ListEmptyComponent={
          hasHosts ? (
            <View style={styles.emptyWrap}>
              <Text style={styles.emptyText}>{t("home.emptyProjects")}</Text>
            </View>
          ) : (
            <PairPrompt />
          )
        }
        contentContainerStyle={hasHosts && sections.length > 0 ? styles.list : styles.listEmpty}
        refreshControl={
          <RefreshControl
            refreshing={loading}
            onRefresh={onRefresh}
            tintColor={theme.textSecondary}
          />
        }
      />

      <NewTaskSheet
        visible={newTaskProjectId !== null}
        lockedProjectId={newTaskProjectId ?? undefined}
        onClose={() => setNewTaskProjectId(null)}
        onCreated={onCreated}
      />
    </View>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1, backgroundColor: theme.bg },
  brandBar: {
    flexDirection: "row",
    alignItems: "center",
    gap: spacing.sm,
    paddingHorizontal: spacing.lg,
    paddingTop: 56,
    paddingBottom: spacing.md,
  },
  brandLogo: { width: 26, height: 26, borderRadius: radii.button - 3 },
  brandName: { color: theme.text, fontSize: 20, fontWeight: "700" },
  brandSpacer: { flex: 1 },
  banner: {
    flexDirection: "row",
    alignItems: "center",
    gap: spacing.sm,
    paddingHorizontal: spacing.lg,
    paddingVertical: spacing.sm,
    backgroundColor: theme.bgCard,
    borderBottomWidth: StyleSheet.hairlineWidth,
    borderBottomColor: theme.border,
  },
  bannerDot: { width: 8, height: 8, borderRadius: radii.pill },
  bannerText: { fontSize: 12.5, flex: 1 },
  bannerAction: { fontSize: 12.5, color: theme.accent, fontWeight: "600" },
  headerAction: { color: theme.accent, fontSize: typography.bodySize, fontWeight: "600" },
  list: { paddingBottom: 32 },
  listEmpty: { flexGrow: 1, justifyContent: "center" },
  statsRow: {
    flexDirection: "row",
    gap: spacing.sm,
    paddingHorizontal: spacing.md,
    paddingTop: spacing.md,
  },
  statCard: {
    flex: 1,
    alignItems: "center",
    gap: 3,
    paddingVertical: spacing.md,
    paddingHorizontal: spacing.xs,
    backgroundColor: theme.bgCard,
    borderRadius: radii.row,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
  },
  statValue: { color: theme.text, fontSize: typography.titleSize, fontWeight: "700" },
  statIcon: {
    height: typography.titleSize + 4,
    alignItems: "center",
    justifyContent: "center",
  },
  statLabel: { color: theme.textSecondary, fontSize: typography.labelSize, fontWeight: "500" },
  sectionLabel: {
    color: theme.textSecondary,
    fontSize: typography.metaSize,
    fontWeight: "700",
    paddingHorizontal: spacing.lg,
    paddingTop: spacing.xl,
    paddingBottom: spacing.sm,
  },
  groupHeader: {
    flexDirection: "row",
    alignItems: "center",
    gap: spacing.xs,
    marginHorizontal: spacing.md,
    paddingHorizontal: spacing.sm,
    paddingTop: spacing.md,
    paddingBottom: spacing.xs,
  },
  groupHeaderText: {
    flex: 1,
    color: theme.textSecondary,
    fontSize: typography.metaSize,
    fontWeight: "700",
  },
  groupHeaderCount: { color: theme.textHint, fontSize: typography.metaSize, fontWeight: "600" },
  projectCard: {
    flexDirection: "row",
    alignItems: "center",
    gap: spacing.sm,
    marginHorizontal: spacing.md,
    marginVertical: spacing.xs,
    paddingHorizontal: spacing.md,
    paddingVertical: spacing.sm,
    backgroundColor: theme.bgCard,
    borderRadius: radii.card,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
  },
  pressed: { opacity: 0.7 },
  projectBody: { flex: 1, gap: 2 },
  projectTitleRow: { flexDirection: "row", alignItems: "center", gap: spacing.sm },
  projectName: { color: theme.text, fontSize: 15, fontWeight: "600", flexShrink: 1 },
  attentionDot: { width: 8, height: 8, borderRadius: radii.pill, backgroundColor: theme.warning },
  projectMeta: { color: theme.textHint, fontSize: typography.metaSize },
  addButton: {
    width: 28,
    height: 28,
    borderRadius: radii.button,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: theme.bgElevated,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
  },
  // 置顶按下态:与桌面端 --control-active-bg 同一视觉语义
  pinButtonActive: { backgroundColor: theme.accent, borderColor: theme.accent },
  emptyWrap: { alignItems: "center", paddingHorizontal: 32, gap: 14 },
  emptyLogo: { width: 64, height: 64, borderRadius: radii.card },
  emptyTitle: { color: theme.text, fontSize: 26, fontWeight: "700" },
  emptyText: { color: theme.textSecondary, fontSize: 13.5, lineHeight: 21, textAlign: "center" },
  primaryButton: {
    backgroundColor: theme.accent,
    borderRadius: radii.button,
    paddingHorizontal: 26,
    paddingVertical: 12,
    marginTop: 6,
  },
  primaryButtonText: { color: theme.onAccent, fontSize: 15, fontWeight: "600" },
  errorText: {
    color: theme.danger,
    fontSize: typography.metaSize,
    paddingHorizontal: spacing.lg,
    paddingTop: spacing.sm,
  },
});
