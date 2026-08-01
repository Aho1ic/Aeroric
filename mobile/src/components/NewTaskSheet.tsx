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
  const [selectedModel, setSelectedModel] = useState<string>("");
  const [reasoningEffort, setReasoningEffort] = useState<string | null>(null);
  const [speed, setSpeed] = useState<"standard" | "fast">("standard");
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
    setSelectedModel("");
    setReasoningEffort(null);
    setSpeed("standard");
    request<{ models: string[] }>("agents.models", { agent })
      .then((res) => {
        if (cancelled) return;
        const normalized: string[] = [];
        const seen = new Set<string>();
        for (const raw of res.models ?? []) {
          const model = raw.trim();
          const key = model.toLocaleLowerCase();
          if (!model || seen.has(key)) continue;
          seen.add(key);
          normalized.push(model);
        }
        setModels(normalized);
      })
      .catch(() => {
        // 模型列表拿不到时不提供默认模型,避免大小写/配置漂移导致误用模型。
        if (!cancelled) setModels([]);
      })
      .finally(() => {
        if (!cancelled) setModelsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [agent, request, status]);

  const selectedAgent = agents.find((item) => item.id === agent);
  const codexLike = Boolean(selectedAgent?.codexLike);
  const supportsUltra = !codexLike || selectedModel.trim().toLocaleLowerCase() === "gpt-5.6-sol";
  const reasoningOptions = codexLike
    ? supportsUltra
      ? ["minimal", "low", "medium", "high", "xhigh", "max", "ultra"]
      : ["minimal", "low", "medium", "high", "xhigh", "max"]
    : ["low", "medium", "high", "xhigh", "max", "ultra"];

  useEffect(() => {
    if (reasoningEffort === "minimal" && !codexLike) {
      setReasoningEffort(null);
      return;
    }
    if (reasoningEffort === "ultra" && codexLike && !supportsUltra) {
      setReasoningEffort(null);
    }
  }, [codexLike, reasoningEffort, supportsUltra]);

  const submit = useCallback(() => {
    const text = prompt.trim();
    const model = selectedModel.trim();
    if (!projectId || !text || submitting) return;
    setSubmitting(true);
    request("task.create", {
      projectId,
      prompt: text,
      agent,
      permissionMode,
      ...(model ? { selectedModel: model } : {}),
      ...(reasoningEffort ? { reasoningEffort } : {}),
      speed,
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
    reasoningEffort,
    speed,
    submitting,
  ]);

  const canSubmit =
    status === "online" && !!projectId && !!prompt.trim() && !submitting;
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
      <View style={styles.agentColumns}>
        {([false, true] as const).map((codexLike) => {
          const family = agents.filter((choice) => choice.codexLike === codexLike);
          return (
            <View key={codexLike ? "openai" : "anthropic"} style={styles.agentColumn}>
              <Text style={styles.columnTitle}>
                {t(codexLike ? "newTask.openai" : "newTask.anthropic")}
              </Text>
              <View style={styles.agentRows}>
                {family.length > 0 ? family.map((choice) => (
                  <Pressable
                    key={choice.id}
                    style={[styles.agentRow, agent === choice.id && styles.agentRowActive]}
                    onPress={() => setAgent(choice.id)}
                  >
                    <Text
                      style={[styles.agentRowText, agent === choice.id && styles.chipTextActive]}
                      numberOfLines={1}
                      ellipsizeMode="tail"
                    >
                      {choice.label}
                    </Text>
                  </Pressable>
                )) : <Text style={styles.hint}>—</Text>}
              </View>
            </View>
          );
        })}
      </View>

      <Text style={styles.sectionLabel}>{t("newTask.model")}</Text>
      {modelsLoading ? (
        <Text style={styles.hint}>{t("newTask.modelsLoading")}</Text>
      ) : models ? (
        <View style={styles.modelList}>
          <Pressable
            style={[styles.modelRow, !selectedModel && styles.chipActive]}
            onPress={() => setSelectedModel("")}
          >
            <Text style={[styles.modelText, !selectedModel && styles.chipTextActive]}>
              {t("newTask.modelAuto")}
            </Text>
          </Pressable>
          {models.map((model) => (
            <Pressable
              key={model}
              style={[styles.modelRow, selectedModel === model && styles.chipActive]}
              onPress={() => setSelectedModel(model)}
            >
              <Text
                style={[styles.modelText, selectedModel === model && styles.chipTextActive]}
                numberOfLines={1}
                ellipsizeMode="tail"
              >
                {model}
              </Text>
            </Pressable>
          ))}
          {models.length === 0 ? <Text style={styles.hint}>{t("newTask.modelsUnavailable")}</Text> : null}
        </View>
      ) : (
        <Text style={styles.hint}>{t("newTask.modelsUnavailable")}</Text>
      )}

      <Text style={styles.sectionLabel}>{t("newTask.reasoning")}</Text>
      <View style={styles.optionGrid}>
        {reasoningOptions.map((effort) => (
          <Pressable
            key={effort}
            style={[styles.optionButton, reasoningEffort === effort && styles.chipActive]}
            onPress={() => setReasoningEffort(effort)}
          >
            <Text style={[styles.chipText, reasoningEffort === effort && styles.chipTextActive]}>
              {t(`newTask.reasoning.${effort}` as Parameters<typeof t>[0])}
            </Text>
          </Pressable>
        ))}
      </View>

      <Text style={styles.sectionLabel}>{t("newTask.speed")}</Text>
      <View style={styles.optionGrid}>
        {(["standard", "fast"] as const).map((value) => (
          <Pressable
            key={value}
            style={[styles.optionButton, speed === value && styles.chipActive]}
            onPress={() => setSpeed(value)}
          >
            <Text style={[styles.chipText, speed === value && styles.chipTextActive]}>
              {t(`newTask.speed.${value}` as Parameters<typeof t>[0])}
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
  agentColumns: { flexDirection: "row", gap: spacing.sm },
  agentColumn: {
    flex: 1,
    minWidth: 0,
    borderRadius: radii.row,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
    backgroundColor: theme.bgCard,
    overflow: "hidden",
  },
  columnTitle: {
    color: theme.textSecondary,
    fontSize: typography.metaSize,
    fontWeight: "700",
    paddingHorizontal: spacing.sm,
    paddingVertical: spacing.sm,
    borderBottomWidth: StyleSheet.hairlineWidth,
    borderBottomColor: theme.border,
  },
  agentRows: { padding: 4, gap: 4 },
  agentRow: {
    minHeight: 38,
    justifyContent: "center",
    paddingHorizontal: spacing.sm,
    borderRadius: radii.button,
  },
  agentRowActive: { backgroundColor: theme.accent },
  agentRowText: { color: theme.text, fontSize: 13.5, flex: 1 },
  modelList: {
    borderRadius: radii.row,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
    backgroundColor: theme.bgCard,
    padding: 4,
    gap: 4,
  },
  modelRow: {
    minHeight: 38,
    justifyContent: "center",
    paddingHorizontal: spacing.md,
    borderRadius: radii.button,
  },
  modelText: { color: theme.text, fontSize: 13, flex: 1 },
  optionGrid: { flexDirection: "row", flexWrap: "wrap", gap: spacing.sm },
  optionButton: {
    minWidth: 74,
    alignItems: "center",
    paddingHorizontal: spacing.md,
    paddingVertical: spacing.sm,
    borderRadius: radii.button,
    backgroundColor: theme.bgCard,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
  },
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
