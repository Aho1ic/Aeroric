/**
 * 任务详情页:主视图(会话/终端合一)/ 文件 / 变更 三个图标 tab + 生命周期操作(取消/完成/恢复)。
 * 主视图按任务状态自动取舍:running 显示终端,结束后显示会话——两者不会同时出现,故合成一个按钮。
 */

import { Stack, useLocalSearchParams } from "expo-router";
import { FolderTree, GitCompare, MessageSquare, SquareTerminal } from "lucide-react-native";
import { useCallback, useState } from "react";
import { ActivityIndicator, Alert, Pressable, StyleSheet, Text, View } from "react-native";
import { ChangesPane } from "../../src/changes/ChangesPane";
import { FilesPane } from "../../src/files/FilesPane";
import { t } from "../../src/i18n";
import { SessionPane } from "../../src/session/SessionPane";
import { useConnection } from "../../src/state/connection-context";
import { useTaskDetail } from "../../src/state/use-task-detail";
import { TerminalPane } from "../../src/terminal/TerminalPane";
import { taskAcceptsInput } from "../../src/types";
import { HeaderIconButton } from "../../src/ui/HeaderIconButton";
import { taskStatusMeta } from "../../src/ui/task-status";
import { radii, theme } from "../../src/ui/theme";

type TabKey = "main" | "files" | "changes";

export default function TaskDetailScreen() {
  const params = useLocalSearchParams<{ taskId: string; projectId?: string; name?: string }>();
  const taskId = typeof params.taskId === "string" ? params.taskId : "";
  const projectId = typeof params.projectId === "string" ? params.projectId : "";
  const fallbackName = typeof params.name === "string" ? params.name : "";
  const { request } = useConnection();
  const { task, error, refresh } = useTaskDetail(projectId, taskId);
  const [tab, setTab] = useState<TabKey>("main");
  const [acting, setActing] = useState(false);

  const statusMeta = task ? taskStatusMeta(task.status) : null;
  const active = task ? taskAcceptsInput(task.status) : false;
  // 会话与终端合并:运行中看终端,结束后看会话。
  const mainPane: "session" | "terminal" = active ? "terminal" : "session";
  const showSession = tab === "main" && mainPane === "session";
  const showTerminal = tab === "main" && mainPane === "terminal";

  const title =
    task?.name?.trim() || task?.prompt.trim().split("\n")[0] || fallbackName || t("common.task");

  const runLifecycle = useCallback(
    (method: "task.cancel" | "task.complete" | "task.resume", doneHint: string) => {
      if (acting) return;
      setActing(true);
      request(method, { projectId, taskId })
        .then(() => {
          if (method === "task.resume") Alert.alert(doneHint);
          refresh();
        })
        .catch((err) =>
          Alert.alert(t("common.opFailed"), err instanceof Error ? err.message : String(err)),
        )
        .finally(() => setActing(false));
    },
    [acting, projectId, refresh, request, taskId],
  );

  const showActions = useCallback(() => {
    if (!task) return;
    const buttons = [];
    if (active) {
      buttons.push({
        text: t("task.markComplete"),
        onPress: () => runLifecycle("task.complete", ""),
      });
      buttons.push({
        text: t("task.cancelTask"),
        style: "destructive" as const,
        onPress: () => runLifecycle("task.cancel", ""),
      });
    } else {
      buttons.push({
        text: task.status === "todo" ? t("task.startTask") : t("task.resumeTask"),
        onPress: () =>
          runLifecycle(
            "task.resume",
            task.status === "todo" ? t("task.startRequested") : t("task.resumeRequested"),
          ),
      });
    }
    buttons.push({ text: t("common.close"), style: "cancel" as const });
    Alert.alert(
      title,
      statusMeta ? t("task.currentStatus", { label: statusMeta.label }) : undefined,
      buttons,
    );
  }, [active, runLifecycle, statusMeta, task, title]);

  const mainLabel = mainPane === "terminal" ? t("task.tab.terminal") : t("task.tab.session");

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
        <View style={styles.tabSwitch}>
          {(
            [
              ["main", mainLabel],
              ["files", t("task.tab.files")],
              ["changes", t("task.tab.changes")],
            ] as Array<[TabKey, string]>
          ).map(([key, label]) => {
            const on = tab === key;
            const color = on ? theme.onAccent : theme.textSecondary;
            return (
              <Pressable
                key={key}
                accessibilityRole="button"
                accessibilityLabel={label}
                accessibilityState={{ selected: on }}
                style={[styles.tabButton, on && styles.tabButtonActive]}
                onPress={() => setTab(key)}
              >
                {key === "main" ? (
                  mainPane === "terminal" ? (
                    <SquareTerminal size={17} color={color} />
                  ) : (
                    <MessageSquare size={17} color={color} />
                  )
                ) : key === "files" ? (
                  <FolderTree size={17} color={color} />
                ) : (
                  <GitCompare size={17} color={color} />
                )}
              </Pressable>
            );
          })}
        </View>
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
    flexDirection: "row",
    flexShrink: 0,
    backgroundColor: theme.bgElevated,
    borderRadius: radii.button,
    padding: 2,
    gap: 2,
  },
  tabButton: {
    width: 34,
    height: 26,
    alignItems: "center",
    justifyContent: "center",
    borderRadius: radii.button - 2,
  },
  tabButtonActive: { backgroundColor: theme.accent },
  paneWrap: { flex: 1 },
  paneHidden: { display: "none" },
});
