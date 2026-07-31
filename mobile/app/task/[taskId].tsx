/**
 * 任务详情页:会话 / 终端双 tab + 生命周期操作(取消/完成/恢复)。
 * 会话 tab 是 vibe coding 主界面(审批卡 + prompt 输入);终端 tab 永远是兜底。
 */

import { Stack, useLocalSearchParams } from "expo-router";
import { useCallback, useRef, useState } from "react";
import { Alert, Pressable, StyleSheet, Text, View } from "react-native";
import { ChangesPane } from "../../src/changes/ChangesPane";
import { t } from "../../src/i18n";
import { SessionPane } from "../../src/session/SessionPane";
import { useConnection } from "../../src/state/connection-context";
import { useTaskDetail } from "../../src/state/use-task-detail";
import { TerminalPane } from "../../src/terminal/TerminalPane";
import { taskAcceptsInput } from "../../src/types";
import { taskStatusMeta } from "../../src/ui/task-status";
import { theme } from "../../src/ui/theme";
import { useKeyboardInset } from "../../src/ui/use-keyboard-inset";

type TabKey = "session" | "terminal" | "changes";

export default function TaskDetailScreen() {
  const params = useLocalSearchParams<{ taskId: string; projectId?: string; name?: string }>();
  const taskId = typeof params.taskId === "string" ? params.taskId : "";
  const projectId = typeof params.projectId === "string" ? params.projectId : "";
  const fallbackName = typeof params.name === "string" ? params.name : "";
  const { request } = useConnection();
  const { task, error, refresh } = useTaskDetail(projectId, taskId);
  // Session/Terminal 融合:运行中默认看终端,结束后默认看会话;
  // 用户手动点过 tab 后不再自动切换。
  const [tab, setTabState] = useState<TabKey | null>(null);
  const userPickedRef = useRef(false);
  const [acting, setActing] = useState(false);

  const setTab = useCallback((key: TabKey) => {
    userPickedRef.current = true;
    setTabState(key);
  }, []);

  const statusMeta = task ? taskStatusMeta(task.status) : null;
  const active = task ? taskAcceptsInput(task.status) : false;
  const autoTab: TabKey = active ? "terminal" : "session";
  const effectiveTab: TabKey = tab ?? autoTab;

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
    Alert.alert(title, statusMeta ? t("task.currentStatus", { label: statusMeta.label }) : undefined, buttons);
  }, [active, runLifecycle, statusMeta, task, title]);

  const keyboardInset = useKeyboardInset();

  return (
    <View style={[styles.screen, { paddingBottom: keyboardInset }]}>
      <Stack.Screen
        options={{
          title,
          headerRight: () => (
            <Pressable hitSlop={10} onPress={showActions} disabled={acting || !task}>
              <Text style={styles.headerAction}>{acting ? "…" : t("task.actions")}</Text>
            </Pressable>
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
          <Text style={styles.statusText}>{error ? t("task.loadFailed", { error }) : t("common.loading")}</Text>
        )}
        <View style={styles.tabSwitch}>
          {(
            [
              ["session", t("task.tab.session")],
              ["terminal", t("task.tab.terminal")],
              ["changes", t("task.tab.changes")],
            ] as Array<[TabKey, string]>
          ).map(([key, label]) => (
            <Pressable
              key={key}
              style={[styles.tabButton, effectiveTab === key && styles.tabButtonActive]}
              onPress={() => setTab(key)}
            >
              <Text style={[styles.tabText, effectiveTab === key && styles.tabTextActive]}>
                {label}
              </Text>
            </Pressable>
          ))}
        </View>
      </View>

      <View style={[styles.paneWrap, effectiveTab !== "session" && styles.paneHidden]}>
        {task ? (
          <SessionPane
            projectId={projectId}
            task={task}
            active={effectiveTab === "session"}
            canSend={active}
          />
        ) : null}
      </View>
      <View style={[styles.paneWrap, effectiveTab !== "terminal" && styles.paneHidden]}>
        <TerminalPane taskId={taskId} active={effectiveTab === "terminal"} />
      </View>
      <View style={[styles.paneWrap, effectiveTab !== "changes" && styles.paneHidden]}>
        <ChangesPane projectId={projectId} taskId={taskId} active={effectiveTab === "changes"} />
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1, backgroundColor: theme.bg },
  headerAction: { color: theme.accent, fontSize: 14.5, fontWeight: "600" },
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
  statusDot: { width: 8, height: 8, borderRadius: 4 },
  statusText: { fontSize: 12.5, color: theme.textSecondary, flex: 1 },
  tabSwitch: {
    flexDirection: "row",
    flexShrink: 0,
    backgroundColor: theme.bgElevated,
    borderRadius: 8,
    padding: 2,
    gap: 2,
  },
  tabButton: { paddingHorizontal: 12, paddingVertical: 5, borderRadius: 6 },
  tabButtonActive: { backgroundColor: theme.accent },
  tabText: { color: theme.textSecondary, fontSize: 13, fontWeight: "600" },
  tabTextActive: { color: "#fff" },
  paneWrap: { flex: 1 },
  paneHidden: { display: "none" },
});
