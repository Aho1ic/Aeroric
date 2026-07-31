/**
 * 新建任务表单:项目 / agent / 模型 / 权限 / prompt → RPC task.create(桌面端执行)。
 *
 * 两种用法:
 * - `mode="page"`:/new-task 整页(项目可选)
 * - `mode="modal"`:首页项目卡片 ＋ 触发的底部抽屉,`lockedProjectId` 锁定项目、不显示选择器
 */

import { useCallback, useEffect, useState } from "react";
import {
  Alert,
  KeyboardAvoidingView,
  Modal,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";
import { t } from "../i18n";
import { useConnection } from "../state/connection-context";
import {
  PERMISSION_MODE_VALUES,
  type AgentChoice,
  type PermissionMode,
  type Project,
} from "../types";
import { radii, spacing, theme, typography } from "../ui/theme";

interface NewTaskFormProps {
  /** 锁定项目(来自项目卡片 ＋),不渲染项目选择器。 */
  lockedProjectId?: string;
  onClose: () => void;
  onCreated?: () => void;
}

function NewTaskForm({ lockedProjectId, onClose, onCreated }: NewTaskFormProps) {
  const { status, request } = useConnection();
  const [projects, setProjects] = useState<Project[]>([]);
  const [agents, setAgents] = useState<AgentChoice[]>([]);
  const [projectId, setProjectId] = useState<string>(lockedProjectId ?? "");
  const [agent, setAgent] = useState<string>("claude");
  const [models, setModels] = useState<string[] | null>(null);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [selectedModel, setSelectedModel] = useState<string | null>(null);
  const [permissionMode, setPermissionMode] = useState<PermissionMode>("ask");
  const [prompt, setPrompt] = useState("");
  const [loadError, setLoadError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (status !== "online") return;
    let cancelled = false;
    Promise.all([request<Project[]>("projects.list"), request<AgentChoice[]>("agents.list")])
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
        if (!lockedProjectId) setProjectId((prev) => prev || visible[0]?.id || "");
        setAgent((prev) =>
          agentList.some((a) => a.id === prev) ? prev : (agentList[0]?.id ?? "claude"),
        );
      })
      .catch((err) => {
        if (!cancelled) setLoadError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [lockedProjectId, request, status]);

  // agent 变化 → 重新拉该 agent 的模型列表,并清掉上一个 agent 的选择
  useEffect(() => {
    if (status !== "online" || !agent) return;
    let cancelled = false;
    setModelsLoading(true);
    setModels(null);
    setSelectedModel(null);
    request<{ models: string[] }>("agents.models", { agent })
      .then((res) => {
        if (cancelled) return;
        setModels(res.models ?? []);
      })
      .catch(() => {
        // 模型列表拿不到不阻塞建任务:留空表示「用桌面端默认模型」
        if (!cancelled) setModels([]);
      })
      .finally(() => {
        if (!cancelled) setModelsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [agent, request, status]);

  const submit = useCallback(() => {
    const text = prompt.trim();
    if (!projectId || !text || submitting) return;
    setSubmitting(true);
    request("task.create", {
      projectId,
      prompt: text,
      agent,
      permissionMode,
      ...(selectedModel ? { selectedModel } : {}),
    })
      .then(() => {
        onCreated?.();
        Alert.alert(t("newTask.sent"), t("newTask.sentBody"), [
          { text: t("newTask.ok"), onPress: onClose },
        ]);
      })
      .catch((err) =>
        Alert.alert(t("newTask.createFailed"), err instanceof Error ? err.message : String(err)),
      )
      .finally(() => setSubmitting(false));
  }, [
    agent,
    onClose,
    onCreated,
    permissionMode,
    projectId,
    prompt,
    request,
    selectedModel,
    submitting,
  ]);

  const canSubmit = status === "online" && !!projectId && !!prompt.trim() && !submitting;
  const lockedProject = lockedProjectId
    ? projects.find((p) => p.id === lockedProjectId)
    : undefined;

  return (
    <ScrollView contentContainerStyle={styles.content} keyboardShouldPersistTaps="handled">
      {status !== "online" ? <Text style={styles.notice}>{t("newTask.offline")}</Text> : null}
      {loadError ? <Text style={styles.noticeError}>{loadError}</Text> : null}

      {lockedProjectId ? (
        lockedProject ? (
          <Text style={styles.lockedProject} numberOfLines={1}>
            {lockedProject.name}
          </Text>
        ) : null
      ) : (
        <>
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
        </>
      )}

      <Text style={styles.sectionLabel}>{t("newTask.agent")}</Text>
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

      <Text style={styles.sectionLabel}>{t("newTask.model")}</Text>
      {modelsLoading ? (
        <Text style={styles.hint}>{t("newTask.modelsLoading")}</Text>
      ) : models && models.length > 0 ? (
        <View style={styles.chipWrap}>
          <Pressable
            style={[styles.chip, selectedModel === null && styles.chipActive]}
            onPress={() => setSelectedModel(null)}
          >
            <Text style={[styles.chipText, selectedModel === null && styles.chipTextActive]}>
              {t("newTask.modelAuto")}
            </Text>
          </Pressable>
          {models.map((model) => (
            <Pressable
              key={model}
              style={[styles.chip, selectedModel === model && styles.chipActive]}
              onPress={() => setSelectedModel(model)}
            >
              <Text
                style={[styles.chipText, selectedModel === model && styles.chipTextActive]}
                numberOfLines={1}
              >
                {model}
              </Text>
            </Pressable>
          ))}
        </View>
      ) : (
        <Text style={styles.hint}>{t("newTask.modelsUnavailable")}</Text>
      )}

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
            <View style={[styles.radio, permissionMode === mode && styles.radioActive]} />
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
  );
}

/** 整页用法(/new-task)。 */
export function NewTaskPage({ onClose }: { onClose: () => void }) {
  return (
    <KeyboardAvoidingView
      style={styles.screen}
      behavior={Platform.OS === "ios" ? "padding" : undefined}
      keyboardVerticalOffset={Platform.OS === "ios" ? 88 : 0}
    >
      <NewTaskForm onClose={onClose} />
    </KeyboardAvoidingView>
  );
}

/** 弹窗用法(首页项目卡片 ＋)。 */
export function NewTaskSheet({
  visible,
  lockedProjectId,
  onClose,
  onCreated,
}: {
  visible: boolean;
  lockedProjectId?: string;
  onClose: () => void;
  onCreated?: () => void;
}) {
  return (
    <Modal
      visible={visible}
      animationType="slide"
      presentationStyle="pageSheet"
      onRequestClose={onClose}
    >
      <KeyboardAvoidingView
        style={styles.screen}
        behavior={Platform.OS === "ios" ? "padding" : undefined}
      >
        <View style={styles.sheetHeader}>
          <Text style={styles.sheetTitle}>{t("nav.newTask")}</Text>
          <Pressable hitSlop={10} onPress={onClose}>
            <Text style={styles.sheetClose}>{t("newTask.cancel")}</Text>
          </Pressable>
        </View>
        {/* key:每次打开都重置表单状态 */}
        <NewTaskForm
          key={lockedProjectId ?? "any"}
          lockedProjectId={lockedProjectId}
          onClose={onClose}
          onCreated={onCreated}
        />
      </KeyboardAvoidingView>
    </Modal>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1, backgroundColor: theme.bg },
  sheetHeader: {
    flexDirection: "row",
    alignItems: "center",
    paddingHorizontal: spacing.lg,
    paddingTop: Platform.OS === "ios" ? spacing.lg : 44,
    paddingBottom: spacing.md,
    borderBottomWidth: StyleSheet.hairlineWidth,
    borderBottomColor: theme.border,
  },
  sheetTitle: { flex: 1, color: theme.text, fontSize: 17, fontWeight: "700" },
  sheetClose: { color: theme.accent, fontSize: typography.bodySize, fontWeight: "600" },
  content: { padding: spacing.lg, paddingBottom: 40, gap: 10 },
  notice: { color: theme.warning, fontSize: 12.5 },
  noticeError: { color: theme.danger, fontSize: 12.5 },
  lockedProject: { color: theme.text, fontSize: 15, fontWeight: "700" },
  sectionLabel: {
    color: theme.textSecondary,
    fontSize: typography.metaSize,
    fontWeight: "700",
    textTransform: "uppercase",
    marginTop: spacing.sm,
  },
  chipWrap: { flexDirection: "row", flexWrap: "wrap", gap: spacing.sm },
  chip: {
    paddingHorizontal: 13,
    paddingVertical: spacing.sm,
    borderRadius: radii.button,
    backgroundColor: theme.bgCard,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
    maxWidth: "100%",
  },
  chipActive: { backgroundColor: theme.accent, borderColor: theme.accent },
  chipText: { color: theme.text, fontSize: 13.5 },
  chipTextActive: { color: theme.onAccent, fontWeight: "600" },
  permList: { gap: 6 },
  permRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    padding: spacing.md,
    borderRadius: radii.row,
    backgroundColor: theme.bgCard,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
  },
  permRowActive: { borderColor: theme.accent, borderWidth: 1 },
  permTextWrap: { flex: 1, gap: 2 },
  permLabel: { color: theme.text, fontSize: typography.bodySize, fontWeight: "600" },
  permHint: { color: theme.textHint, fontSize: typography.metaSize },
  radio: {
    width: 16,
    height: 16,
    borderRadius: radii.pill,
    borderWidth: 2,
    borderColor: theme.border,
  },
  radioActive: { borderColor: theme.accent, backgroundColor: theme.accent },
  promptInput: {
    minHeight: 120,
    borderRadius: radii.input,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
    backgroundColor: theme.bgCard,
    color: theme.text,
    padding: spacing.md,
    fontSize: typography.bodySize,
    lineHeight: 20,
  },
  submitButton: {
    backgroundColor: theme.accent,
    borderRadius: radii.button,
    paddingVertical: 13,
    alignItems: "center",
    marginTop: spacing.sm,
  },
  submitDisabled: { opacity: 0.4 },
  submitText: { color: theme.onAccent, fontSize: 15, fontWeight: "700" },
  hint: { color: theme.textHint, fontSize: typography.metaSize, lineHeight: 18 },
});
