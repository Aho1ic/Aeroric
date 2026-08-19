/**
 * 任务详情页:会话 / 终端 / 文件 / 变更四个图标 tab + 生命周期操作(取消/完成/恢复)。
 * 首次进入按任务状态选择会话或终端,之后尊重用户手动切换;审批与终端回退始终可达。
 */

import { Stack, useLocalSearchParams } from "expo-router";
import { FolderTree, GitCompare, MessageSquare, SquareTerminal } from "lucide-react-native";
import { useCallback, useState } from "react";
import { ActivityIndicator, Alert, StyleSheet, Text, View } from "react-native";
import { ChangesPane } from "../../src/changes/ChangesPane";
import { FilesPane } from "../../src/files/FilesPane";
import { t } from "../../src/i18n";
import { SessionPane } from "../../src/session/SessionPane";
import { useConnection } from "../../src/state/connection-context";
import { useTaskDetail } from "../../src/state/use-task-detail";
import { TerminalPane } from "../../src/terminal/TerminalPane";
import type { RemoteTaskActionResult } from "../../src/types";
import { taskAcceptsInput } from "../../src/types";
import { AnimatedSelection } from "../../src/ui/AnimatedSelection";
import { HeaderIconButton } from "../../src/ui/HeaderIconButton";
import { taskStatusMeta } from "../../src/ui/task-status";
import { radii, theme } from "../../src/ui/theme";

type TabKey = "session" | "terminal" | "files" | "changes";

export default function TaskDetailScreen() {
  const params = useLocalSearchParams<{ taskId: string; projectId?: string; name?: string }>();
  const taskId = typeof params.taskId === "string" ? params.taskId : "";
  const projectId = typeof params.projectId === "string" ? params.projectId : "";
  const fallbackName = typeof params.name === "string" ? params.name : "";
  const { request, capabilitiesReady, hasCapability } = useConnection();
  const { task, error, refresh, patchTask } = useTaskDetail(projectId, taskId);
  const [selectedTab, setSelectedTab] = useState<TabKey | null>(null);
  const [acting, setActing] = useState(false);
  const lifecycleSupported = !capabilitiesReady || hasCapability("tasks.lifecycle");
  const terminalSupported = !capabilitiesReady || hasCapability("terminal.stream");

  const statusMeta = task ? taskStatusMeta(task.status) : null;
  const active = task ? taskAcceptsInput(task.status) : false;
  const defaultTab: TabKey =
    active && task?.status !== "input_required" && terminalSupported ? "terminal" : "session";
  const tab = selectedTab ?? defaultTab;
  const showSession = tab === "session";
  const showTerminal = tab === "terminal";

  const title =
    task?.name?.trim() || task?.prompt.trim().split("\n")[0] || fallbackName || t("common.task");

  const runLifecycle = useCallback(
    (method: "task.cancel" | "task.complete" | "task.resume") => {
      if (acting) return;
      if (!lifecycleSupported) {
        Alert.alert(t("task.lifecycleUnsupported"));
        return;
      }
      const previousTask = task;
      setActing(true);
      if (previousTask) {
        patchTask({
          status:
            method === "task.resume"
              ? "pending"
              : method === "task.complete"
                ? "done"
                : "cancelled",
          approval: undefined,
          attentionRequestedAt: undefined,
        });
      }
      const operation =
        method === "task.resume"
          ? request<RemoteTaskActionResult>(method, { projectId, taskId })
          : request(method, { projectId, taskId });
      operation
        .then((result) => {
          if (method === "task.resume") {
            const response = result as RemoteTaskActionResult;
            if (response.task) patchTask(response.task);
            else
              patchTask({
                status: "pending",
                approval: undefined,
                attentionRequestedAt: undefined,
              });
          }
          void refresh();
        })
        .catch((err) => {
          if (previousTask) {
            patchTask({
              status: previousTask.status,
              approval: previousTask.approval,
              attentionRequestedAt: previousTask.attentionRequestedAt,
            });
          }
          Alert.alert(t("common.opFailed"), err instanceof Error ? err.message : String(err));
        })
        .finally(() => setActing(false));
    },
    [acting, lifecycleSupported, patchTask, projectId, refresh, request, task, taskId],
  );

  const showActions = useCallback(() => {
    if (!task) return;
    if (!lifecycleSupported) {
      Alert.alert(t("task.lifecycleUnsupported"));
      return;
    }
    const buttons = [];
    if (active) {
      buttons.push({
        text: t("task.markComplete"),
        onPress: () => runLifecycle("task.complete"),
      });
      buttons.push({
        text: t("task.cancelTask"),
        style: "destructive" as const,
        onPress: () => runLifecycle("task.cancel"),
      });
    } else {
      buttons.push({
        text: task.status === "todo" ? t("task.startTask") : t("task.resumeTask"),
        onPress: () => runLifecycle("task.resume"),
      });
    }
    buttons.push({ text: t("common.close"), style: "cancel" as const });
    Alert.alert(
      title,
      statusMeta ? t("task.currentStatus", { label: statusMeta.label }) : undefined,
      buttons,
    );
  }, [active, lifecycleSupported, runLifecycle, statusMeta, task, title]);

  return (
    <View style={styles.screen}>
      <Stack.Screen
        options={{
          title,
          headerRight: () => (
            <HeaderIconButton
              label={t("task.actions")}
              disabled={acting || !task}
              onPress={showActions}
            >
              {acting ? (
                <ActivityIndicator size="small" color={theme.accent} />
              ) : (
                <Text style={styles.headerActionText}>{t("task.actions")}</Text>
              )}
            </HeaderIconButton>
          ),
        }}
      />

      <View style={styles.statusBar}>
        {statusMeta ? (
          <>
            <View style={[styles.statusDot, { backgroundColor: statusMeta.color }]} />
            <Text style={[styles.statusText, { color: statusMeta.color }]}>{statusMeta.label}</Text>
          </>
        ) : (
          <Text style={styles.statusText}>
            {error ? t("task.loadFailed", { error }) : t("common.loading")}
          </Text>
        )}
        <AnimatedSelection
          value={tab}
          options={[
            {
              value: "session" as const,
              label: t("task.tab.session"),
              icon: (
                <MessageSquare
                  size={17}
                  color={tab === "session" ? theme.onAccent : theme.textSecondary}
                />
              ),
            },
            {
              value: "terminal" as const,
              label: t("task.tab.terminal"),
              icon: (
                <SquareTerminal
                  size={17}
                  color={tab === "terminal" ? theme.onAccent : theme.textSecondary}
                />
              ),
            },
            {
              value: "files" as const,
              label: t("task.tab.files"),
              icon: (
                <FolderTree
                  size={17}
                  color={tab === "files" ? theme.onAccent : theme.textSecondary}
                />
              ),
            },
            {
              value: "changes" as const,
              label: t("task.tab.changes"),
              icon: (
                <GitCompare
                  size={17}
                  color={tab === "changes" ? theme.onAccent : theme.textSecondary}
                />
              ),
            },
          ]}
          onChange={setSelectedTab}
          compact
          iconOnly
          style={styles.tabSwitch}
        />
      </View>

      <View style={[styles.paneWrap, !showSession && styles.paneHidden]}>
        {task ? (
          <SessionPane projectId={projectId} task={task} active={showSession} canSend={active} />
        ) : null}
      </View>
      <View style={[styles.paneWrap, !showTerminal && styles.paneHidden]}>
        <TerminalPane taskId={taskId} active={showTerminal} />
      </View>
      <View style={[styles.paneWrap, tab !== "files" && styles.paneHidden]}>
        <FilesPane projectId={projectId} active={tab === "files"} />
      </View>
      <View style={[styles.paneWrap, tab !== "changes" && styles.paneHidden]}>
        <ChangesPane projectId={projectId} taskId={taskId} active={tab === "changes"} />
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1, backgroundColor: theme.bg },
  headerActionText: {
    color: theme.accent,
    fontSize: 12,
    fontWeight: "600",
    textAlign: "center",
    includeFontPadding: false,
  },
  statusBar: {
    flexDirection: "row",
    alignItems: "center",
    gap: 7,
    paddingHorizontal: 14,
    paddingVertical: 8,
    backgroundColor: theme.bgCard,
    borderBottomWidth: StyleSheet.hairlineWidth,
    borderBottomColor: theme.border,
  },
  statusDot: { width: 8, height: 8, borderRadius: radii.pill },
  statusText: { fontSize: 12.5, color: theme.textSecondary, flex: 1 },
  tabSwitch: {
    flexShrink: 0,
  },
  paneWrap: { flex: 1 },
  paneHidden: { display: "none" },
});
