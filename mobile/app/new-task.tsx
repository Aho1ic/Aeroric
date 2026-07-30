/** 新建任务页:选项目 / agent / 权限模式 + prompt → RPC task.create(桌面端执行)。 */

import { Stack, router } from "expo-router";
import { useCallback, useEffect, useState } from "react";
import {
  Alert,
  KeyboardAvoidingView,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";
import { t } from "../src/i18n";
import { useConnection } from "../src/state/connection-context";
import {
  PERMISSION_MODE_VALUES,
  type AgentChoice,
  type PermissionMode,
  type Project,
} from "../src/types";
import { theme } from "../src/ui/theme";

export default function NewTaskScreen() {
  const { status, request } = useConnection();
  const [projects, setProjects] = useState<Project[]>([]);
  const [agents, setAgents] = useState<AgentChoice[]>([]);
  const [projectId, setProjectId] = useState<string>("");
  const [agent, setAgent] = useState<string>("claude");
  const [permissionMode, setPermissionMode] = useState<PermissionMode>("ask");
  const [prompt, setPrompt] = useState("");
  const [loadError, setLoadError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (status !== "online") return;
    let cancelled = false;
    Promise.all([
      request<Project[]>("projects.list"),
      request<AgentChoice[]>("agents.list"),
    ])
      .then(([projectList, agentList]) => {
        if (cancelled) return;
        const visible = projectList
          .filter((p) => !p.hiddenFromRail)
          .sort(
            (a, b) =>
              (a.orderIndex ?? 1e15) - (b.orderIndex ?? 1e15) || b.lastOpenedAt - a.lastOpenedAt,
          );
        setProjects(visible);
        setAgents(agentList);
        setLoadError(null);
        setProjectId((prev) => prev || visible[0]?.id || "");
        setAgent((prev) => (agentList.some((a) => a.id === prev) ? prev : (agentList[0]?.id ?? "claude")));
      })
      .catch((err) => {
        if (!cancelled) setLoadError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [request, status]);

  const submit = useCallback(() => {
    const text = prompt.trim();
    if (!projectId || !text || submitting) return;
    setSubmitting(true);
    request("task.create", { projectId, prompt: text, agent, permissionMode })
      .then(() => {
        Alert.alert(t("newTask.sent"), t("newTask.sentBody"), [
          { text: t("newTask.ok"), onPress: () => router.back() },
        ]);
      })
      .catch((err) =>
        Alert.alert(t("newTask.createFailed"), err instanceof Error ? err.message : String(err)),
      )
      .finally(() => setSubmitting(false));
  }, [agent, permissionMode, projectId, prompt, request, submitting]);

  const canSubmit = status === "online" && !!projectId && !!prompt.trim() && !submitting;

  return (
    <KeyboardAvoidingView
      style={styles.screen}
      behavior={Platform.OS === "ios" ? "padding" : undefined}
      keyboardVerticalOffset={Platform.OS === "ios" ? 88 : 0}
    >
      <Stack.Screen options={{ title: t("nav.newTask") }} />
      <ScrollView contentContainerStyle={styles.content} keyboardShouldPersistTaps="handled">
        {status !== "online" ? (
          <Text style={styles.notice}>{t("newTask.offline")}</Text>
        ) : null}
        {loadError ? <Text style={styles.noticeError}>{loadError}</Text> : null}

        <Text style={styles.sectionLabel}>{t("newTask.project")}</Text>
        <View style={styles.chipWrap}>
          {projects.map((project) => (
            <Pressable
              key={project.id}
              style={[styles.chip, projectId === project.id && styles.chipActive]}
              onPress={() => setProjectId(project.id)}
            >
              <Text
                style={[styles.chipText, projectId === project.id && styles.chipTextActive]}
                numberOfLines={1}
              >
                {project.name}
              </Text>
            </Pressable>
          ))}
          {projects.length === 0 && status === "online" && !loadError ? (
            <Text style={styles.hint}>{t("newTask.noProjects")}</Text>
          ) : null}
        </View>

        <Text style={styles.sectionLabel}>Agent</Text>
        <View style={styles.chipWrap}>
          {agents.map((choice) => (
            <Pressable
              key={choice.id}
              style={[styles.chip, agent === choice.id && styles.chipActive]}
              onPress={() => setAgent(choice.id)}
            >
              <Text style={[styles.chipText, agent === choice.id && styles.chipTextActive]}>
                {choice.label}
              </Text>
            </Pressable>
          ))}
        </View>

        <Text style={styles.sectionLabel}>{t("newTask.permission")}</Text>
        <View style={styles.permList}>
          {PERMISSION_MODE_VALUES.map((mode) => (
            <Pressable
              key={mode}
              style={[styles.permRow, permissionMode === mode && styles.permRowActive]}
              onPress={() => setPermissionMode(mode)}
            >
              <View style={styles.permTextWrap}>
                <Text style={styles.permLabel}>{t(`perm.${mode}`)}</Text>
                <Text style={styles.permHint}>{t(`perm.${mode}.hint`)}</Text>
              </View>
              <View
                style={[styles.radio, permissionMode === mode && styles.radioActive]}
              />
            </Pressable>
          ))}
        </View>

        <Text style={styles.sectionLabel}>Prompt</Text>
        <TextInput
          style={styles.promptInput}
          value={prompt}
          onChangeText={setPrompt}
          placeholder={t("newTask.promptPlaceholder")}
          placeholderTextColor={theme.textHint}
          multiline
          textAlignVertical="top"
        />

        <Pressable
          style={[styles.submitButton, !canSubmit && styles.submitDisabled]}
          disabled={!canSubmit}
          onPress={submit}
        >
          <Text style={styles.submitText}>
            {submitting ? t("newTask.submitting") : t("newTask.submit")}
          </Text>
        </Pressable>
        <Text style={styles.hint}>{t("newTask.footnote")}</Text>
      </ScrollView>
    </KeyboardAvoidingView>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1, backgroundColor: theme.bg },
  content: { padding: 16, paddingBottom: 40, gap: 10 },
  notice: { color: theme.warning, fontSize: 12.5 },
  noticeError: { color: theme.danger, fontSize: 12.5 },
  sectionLabel: {
    color: theme.textSecondary,
    fontSize: 12,
    fontWeight: "700",
    textTransform: "uppercase",
    marginTop: 8,
  },
  chipWrap: { flexDirection: "row", flexWrap: "wrap", gap: 8 },
  chip: {
    paddingHorizontal: 13,
    paddingVertical: 8,
    borderRadius: 9,
    backgroundColor: theme.bgCard,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
    maxWidth: "100%",
  },
  chipActive: { backgroundColor: theme.accent, borderColor: theme.accent },
  chipText: { color: theme.text, fontSize: 13.5 },
  chipTextActive: { color: "#fff", fontWeight: "600" },
  permList: { gap: 6 },
  permRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    padding: 12,
    borderRadius: 10,
    backgroundColor: theme.bgCard,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
  },
  permRowActive: { borderColor: theme.accent, borderWidth: 1 },
  permTextWrap: { flex: 1, gap: 2 },
  permLabel: { color: theme.text, fontSize: 14, fontWeight: "600" },
  permHint: { color: theme.textHint, fontSize: 12 },
  radio: {
    width: 16,
    height: 16,
    borderRadius: 8,
    borderWidth: 2,
    borderColor: theme.border,
  },
  radioActive: { borderColor: theme.accent, backgroundColor: theme.accent },
  promptInput: {
    minHeight: 120,
    borderRadius: 10,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
    backgroundColor: theme.bgCard,
    color: theme.text,
    padding: 12,
    fontSize: 14,
    lineHeight: 20,
  },
  submitButton: {
    backgroundColor: theme.accent,
    borderRadius: 10,
    paddingVertical: 13,
    alignItems: "center",
    marginTop: 8,
  },
  submitDisabled: { opacity: 0.4 },
  submitText: { color: "#fff", fontSize: 15, fontWeight: "700" },
  hint: { color: theme.textHint, fontSize: 12, lineHeight: 18 },
});
