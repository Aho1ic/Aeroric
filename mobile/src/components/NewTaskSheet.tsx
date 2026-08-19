/**
 * 新建任务表单:项目 / agent / 模型 / 权限 / prompt → RPC task.create(桌面端执行)。
 *
 * 两种用法:
 * - `mode="page"`:/new-task 整页(项目可选)
 * - `mode="modal"`:首页项目卡片 ＋ 触发的底部抽屉,`lockedProjectId` 锁定项目、不显示选择器
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Alert,
  KeyboardAvoidingView,
  Modal,
  Platform,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from "react-native";
import { BrainCircuit, BookmarkPlus, Gauge, Zap } from "lucide-react-native";
import { t } from "../i18n";
import { agentFamilyOf, reasoningOptionsForFamily } from "../agent-family";
import { useConnection } from "../state/connection-context";
import {
  loadLastModels,
  rememberLastModel,
  saveLastModels,
  type LastModelsByAgent,
} from "../storage/new-task-models";
import {
  PERMISSION_MODE_VALUES,
  type AgentChoice,
  type AgentFamily,
  type PermissionMode,
  type Project,
  type RemoteTaskActionResult,
  type Task,
} from "../types";
import { AnimatedPressable } from "../ui/AnimatedPressable";
import { AnimatedSelection } from "../ui/AnimatedSelection";
import { ANTHROPIC_BRAND, AnthropicIcon, OpenAIIcon } from "../ui/brand-icons";
import { Button, Field } from "../ui/primitives";
import { radii, spacing, theme, typography } from "../ui/theme";

/** 两栏各默认显示 5 行，更多配置在各自列内滚动。 */
const AGENT_ROW_HEIGHT = 40;
const AGENT_LIST_MAX_HEIGHT = AGENT_ROW_HEIGHT * 5 + 4 * 4 + 8;

interface AgentTaskSelection {
  reasoningEffort: string | null;
  speed: "standard" | "fast";
  permissionMode: PermissionMode;
  dshAgentPreset: string;
}

const DEFAULT_AGENT_TASK_SELECTION: AgentTaskSelection = {
  reasoningEffort: null,
  speed: "standard",
  permissionMode: "ask",
  dshAgentPreset: "standard",
};

function defaultSelectionForFamily(family: AgentFamily): AgentTaskSelection {
  return family === "dsh"
    ? { ...DEFAULT_AGENT_TASK_SELECTION, reasoningEffort: "high" }
    : DEFAULT_AGENT_TASK_SELECTION;
}

function normalizeModels(rawModels: string[]): string[] {
  const normalized: string[] = [];
  const seen = new Set<string>();
  for (const raw of rawModels) {
    const model = raw.trim();
    const key = model.toLocaleLowerCase();
    if (!model || seen.has(key)) continue;
    seen.add(key);
    normalized.push(model);
  }
  return normalized;
}

function preferredModel(models: string[], lastModel?: string): string {
  const remembered = lastModel?.trim().toLocaleLowerCase();
  if (remembered) {
    const matched = models.find((model) => model.toLocaleLowerCase() === remembered);
    if (matched) return matched;
  }
  return models[0] ?? "";
}

interface NewTaskFormProps {
  /** 锁定项目(来自项目卡片 ＋),不渲染项目选择器。 */
  lockedProjectId?: string;
  onClose: () => void;
  onCreated?: (task: Task) => void;
}

function NewTaskForm({ lockedProjectId, onClose, onCreated }: NewTaskFormProps) {
  const { status, request, capabilitiesReady, hasCapability } = useConnection();
  const [projects, setProjects] = useState<Project[]>([]);
  const [agents, setAgents] = useState<AgentChoice[]>([]);
  const [projectId, setProjectId] = useState<string>(lockedProjectId ?? "");
  const [agent, setAgent] = useState<string>("claude");
  const [modelState, setModelState] = useState<{
    agent: string;
    models: string[] | null;
    loading: boolean;
  }>({ agent: "", models: null, loading: false });
  const [selectedModel, setSelectedModel] = useState<string>("");
  const [reasoningEffort, setReasoningEffort] = useState<string | null>(null);
  const [speed, setSpeed] = useState<"standard" | "fast">("standard");
  const [permissionMode, setPermissionMode] = useState<PermissionMode>("ask");
  const [dshAgentPreset, setDshAgentPreset] = useState("standard");
  const [lastModels, setLastModels] = useState<LastModelsByAgent>({});
  const [lastModelsLoaded, setLastModelsLoaded] = useState(false);
  const [prompt, setPrompt] = useState("");
  const [loadError, setLoadError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const modelCacheRef = useRef(new Map<string, string[]>());
  const lastModelsRef = useRef<LastModelsByAgent>({});
  const agentSelectionsRef = useRef(new Map<string, AgentTaskSelection>());

  const models = modelState.agent === agent ? modelState.models : null;
  const modelsLoading = modelState.agent === agent && modelState.loading;
  const modelsSupported = !capabilitiesReady || hasCapability("tasks.models");
  const taskCreationSupported =
    modelsSupported && (!capabilitiesReady || hasCapability("tasks.lifecycle"));

  useEffect(() => {
    let cancelled = false;
    void loadLastModels().then((stored) => {
      if (cancelled) return;
      lastModelsRef.current = stored;
      setLastModels(stored);
      setLastModelsLoaded(true);
    });
    return () => {
      cancelled = true;
    };
  }, []);

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

  const anthropicAgents = useMemo(
    () => agents.filter((choice) => agentFamilyOf(choice) === "claude"),
    [agents],
  );
  const openaiAgents = useMemo(
    () => agents.filter((choice) => agentFamilyOf(choice) === "codex"),
    [agents],
  );
  const dshAgents = useMemo(
    () => agents.filter((choice) => agentFamilyOf(choice) === "dsh"),
    [agents],
  );
  const selectedAgent = useMemo(() => agents.find((item) => item.id === agent), [agent, agents]);
  const selectedFamily = agentFamilyOf(selectedAgent);

  const selectAgent = useCallback(
    (next: AgentChoice) => {
      if (next.id === agent) return;
      agentSelectionsRef.current.set(agent, {
        reasoningEffort,
        speed,
        permissionMode,
        dshAgentPreset,
      });
      const restored =
        agentSelectionsRef.current.get(next.id) ?? defaultSelectionForFamily(agentFamilyOf(next));
      setAgent(next.id);
      setSelectedModel("");
      setReasoningEffort(restored.reasoningEffort);
      setSpeed(restored.speed);
      setPermissionMode(restored.permissionMode);
      setDshAgentPreset(restored.dshAgentPreset);
    },
    [agent, dshAgentPreset, permissionMode, reasoningEffort, speed],
  );

  // 配置切换时优先命中本次表单的缓存；首次才向桌面端请求，避免来回切换有空白和卡顿。
  useEffect(() => {
    if (status !== "online" || !agent) {
      setModelState({ agent, models: null, loading: false });
      return;
    }
    if (!modelsSupported) {
      setModelState({ agent, models: [], loading: false });
      return;
    }
    let cancelled = false;
    setSelectedModel("");
    const cached = modelCacheRef.current.get(agent);
    if (cached) {
      setModelState({ agent, models: cached, loading: false });
      return () => {
        cancelled = true;
      };
    }
    setModelState({ agent, models: null, loading: true });
    request<{ models: string[] }>("agents.models", { agent })
      .then((res) => {
        if (cancelled) return;
        const normalized = normalizeModels(res.models ?? []);
        modelCacheRef.current.set(agent, normalized);
        setModelState({ agent, models: normalized, loading: false });
      })
      .catch(() => {
        // 模型列表拿不到时不显示“默认”，避免大小写/配置漂移导致误用模型。
        if (!cancelled) setModelState({ agent, models: [], loading: false });
      });
    return () => {
      cancelled = true;
    };
  }, [agent, modelsSupported, request, status]);

  useEffect(() => {
    if (!models || !lastModelsLoaded) return;
    setSelectedModel(preferredModel(models, lastModels[agent]));
  }, [agent, lastModels, lastModelsLoaded, models]);

  const selectModel = useCallback(
    (model: string) => {
      setSelectedModel(model);
      const next = rememberLastModel(lastModelsRef.current, agent, model);
      if (next === lastModelsRef.current) return;
      lastModelsRef.current = next;
      setLastModels(next);
      void saveLastModels(next);
    },
    [agent],
  );

  const reasoningOptions = useMemo(
    () => reasoningOptionsForFamily(selectedFamily, selectedModel),
    [selectedFamily, selectedModel],
  );

  useEffect(() => {
    if (!reasoningEffort || !reasoningOptions.includes(reasoningEffort)) {
      const fallback = selectedFamily === "dsh" ? "high" : null;
      setReasoningEffort(fallback);
      agentSelectionsRef.current.set(agent, {
        reasoningEffort: fallback,
        speed,
        permissionMode,
        dshAgentPreset,
      });
    }
  }, [
    agent,
    dshAgentPreset,
    permissionMode,
    reasoningEffort,
    reasoningOptions,
    selectedFamily,
    speed,
  ]);

  const selectReasoningEffort = useCallback(
    (next: string) => {
      const value = next === "__none__" ? null : next;
      setReasoningEffort(value);
      agentSelectionsRef.current.set(agent, {
        reasoningEffort: value,
        speed,
        permissionMode,
        dshAgentPreset,
      });
    },
    [agent, dshAgentPreset, permissionMode, speed],
  );

  const selectSpeed = useCallback(
    (next: "standard" | "fast") => {
      setSpeed(next);
      agentSelectionsRef.current.set(agent, {
        reasoningEffort,
        speed: next,
        permissionMode,
        dshAgentPreset,
      });
    },
    [agent, dshAgentPreset, permissionMode, reasoningEffort],
  );

  const selectPermissionMode = useCallback(
    (next: PermissionMode) => {
      setPermissionMode(next);
      agentSelectionsRef.current.set(agent, {
        reasoningEffort,
        speed,
        permissionMode: next,
        dshAgentPreset,
      });
    },
    [agent, dshAgentPreset, reasoningEffort, speed],
  );

  const selectDshAgentPreset = useCallback(
    (next: string) => {
      setDshAgentPreset(next);
      agentSelectionsRef.current.set(agent, {
        reasoningEffort,
        speed,
        permissionMode,
        dshAgentPreset: next,
      });
    },
    [agent, permissionMode, reasoningEffort, speed],
  );

  const submit = useCallback(() => {
    const text = prompt.trim();
    const model = selectedModel.trim();
    if (!projectId || !text || submitting) return;
    if (!taskCreationSupported) {
      Alert.alert(t("newTask.unsupported"));
      return;
    }
    setSubmitting(true);
    request<RemoteTaskActionResult>("task.create", {
      projectId,
      prompt: text,
      agent,
      permissionMode,
      ...(model ? { selectedModel: model } : {}),
      ...(reasoningEffort ? { reasoningEffort } : {}),
      ...(selectedFamily === "dsh" ? {} : { speed }),
      ...(selectedFamily === "dsh" ? { dshAgentPreset } : {}),
    })
      .then((result) => {
        // 新版桌面端回传权威快照;旧版只回 taskId 时用最小本地快照兜底,
        // 让手机在下一次一致性刷新前也能立即看到这条任务。
        const task: Task = result.task ?? {
          id: result.taskId ?? `${Date.now()}`,
          projectId,
          prompt: text,
          agent,
          selectedModel: model || undefined,
          reasoningEffort: reasoningEffort ?? undefined,
          ...(selectedFamily === "dsh" ? {} : { speed }),
          status: "pending",
          createdAt: Date.now(),
        };
        onCreated?.(task);
        onClose();
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
    selectedFamily,
    dshAgentPreset,
    speed,
    submitting,
    taskCreationSupported,
  ]);

  const canSubmit =
    status === "online" &&
    taskCreationSupported &&
    !!projectId &&
    !!prompt.trim() &&
    !submitting &&
    models !== null &&
    !modelsLoading &&
    lastModelsLoaded;
  const lockedProject = lockedProjectId
    ? projects.find((p) => p.id === lockedProjectId)
    : undefined;

  return (
    <ScrollView contentContainerStyle={styles.content} keyboardShouldPersistTaps="handled">
      {status !== "online" ? <Text style={styles.notice}>{t("newTask.offline")}</Text> : null}
      {status === "online" && !taskCreationSupported ? (
        <Text style={styles.noticeError}>{t("newTask.unsupported")}</Text>
      ) : null}
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
              <AnimatedPressable
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
              </AnimatedPressable>
            ))}
            {projects.length === 0 && status === "online" && !loadError ? (
              <Text style={styles.hint}>{t("newTask.noProjects")}</Text>
            ) : null}
          </View>
        </>
      )}

      <Text style={styles.sectionLabel}>{t("newTask.agent")}</Text>
      <View style={styles.agentColumns}>
        <View style={styles.agentColumn}>
          <View style={styles.agentColumnHeader}>
            <AnthropicIcon size={15} color={ANTHROPIC_BRAND} />
            <Text style={styles.agentColumnTitle}>{t("newTask.anthropic")}</Text>
          </View>
          <ScrollView
            style={styles.agentList}
            contentContainerStyle={styles.agentRows}
            nestedScrollEnabled
            directionalLockEnabled
            keyboardShouldPersistTaps="handled"
          >
            {anthropicAgents.length > 0 ? (
              anthropicAgents.map((choice) => (
                <AnimatedPressable
                  key={choice.id}
                  style={[styles.agentRow, agent === choice.id && styles.agentRowActive]}
                  onPress={() => selectAgent(choice)}
                  accessibilityRole="button"
                  accessibilityLabel={choice.label}
                  accessibilityState={{ selected: agent === choice.id }}
                >
                  <Text
                    style={[styles.agentRowText, agent === choice.id && styles.agentRowTextActive]}
                    numberOfLines={1}
                    ellipsizeMode="tail"
                  >
                    {choice.label}
                  </Text>
                </AnimatedPressable>
              ))
            ) : (
              <Text style={styles.agentEmpty}>—</Text>
            )}
          </ScrollView>
        </View>

        <View style={styles.agentColumn}>
          <View style={styles.agentColumnHeader}>
            <OpenAIIcon size={15} color={theme.text} />
            <Text style={styles.agentColumnTitle}>{t("newTask.openai")}</Text>
          </View>
          <ScrollView
            style={styles.agentList}
            contentContainerStyle={styles.agentRows}
            nestedScrollEnabled
            directionalLockEnabled
            keyboardShouldPersistTaps="handled"
          >
            {openaiAgents.length > 0 ? (
              openaiAgents.map((choice) => (
                <AnimatedPressable
                  key={choice.id}
                  style={[styles.agentRow, agent === choice.id && styles.agentRowActive]}
                  onPress={() => selectAgent(choice)}
                  accessibilityRole="button"
                  accessibilityLabel={choice.label}
                  accessibilityState={{ selected: agent === choice.id }}
                >
                  <Text
                    style={[styles.agentRowText, agent === choice.id && styles.agentRowTextActive]}
                    numberOfLines={1}
                    ellipsizeMode="tail"
                  >
                    {choice.label}
                  </Text>
                </AnimatedPressable>
              ))
            ) : (
              <Text style={styles.agentEmpty}>—</Text>
            )}
          </ScrollView>
        </View>

        <View style={styles.agentColumn}>
          <View style={styles.agentColumnHeader}>
            <BrainCircuit size={15} color={theme.accent} />
            <Text style={styles.agentColumnTitle}>{t("newTask.deepseek")}</Text>
          </View>
          <ScrollView
            style={styles.agentList}
            contentContainerStyle={styles.agentRows}
            nestedScrollEnabled
            directionalLockEnabled
            keyboardShouldPersistTaps="handled"
          >
            {dshAgents.length > 0 ? (
              dshAgents.map((choice) => (
                <AnimatedPressable
                  key={choice.id}
                  style={[styles.agentRow, agent === choice.id && styles.agentRowActive]}
                  onPress={() => selectAgent(choice)}
                  accessibilityRole="button"
                  accessibilityLabel={choice.label}
                  accessibilityState={{ selected: agent === choice.id }}
                >
                  <Text
                    style={[styles.agentRowText, agent === choice.id && styles.agentRowTextActive]}
                    numberOfLines={1}
                    ellipsizeMode="tail"
                  >
                    {choice.label}
                  </Text>
                </AnimatedPressable>
              ))
            ) : (
              <Text style={styles.agentEmpty}>—</Text>
            )}
          </ScrollView>
        </View>
      </View>

      <Text style={styles.sectionLabel}>{t("newTask.model")}</Text>
      {modelsLoading || !lastModelsLoaded ? (
        <Text style={styles.hint}>{t("newTask.modelsLoading")}</Text>
      ) : models && models.length > 0 ? (
        <AnimatedSelection
          value={selectedModel}
          options={models.map((model) => ({ value: model, label: model }))}
          onChange={selectModel}
          horizontal
          style={styles.modelSelection}
        />
      ) : (
        <Text style={styles.hint}>{t("newTask.modelsUnavailable")}</Text>
      )}

      <Text style={styles.sectionLabel}>{t("newTask.reasoning")}</Text>
      <AnimatedSelection
        value={reasoningEffort ?? "__none__"}
        options={[
          ...(selectedFamily === "dsh"
            ? []
            : [{ value: "__none__", label: t("newTask.reasoning.default") }]),
          ...reasoningOptions.map((effort) => ({
            value: effort,
            label: t(`newTask.reasoning.${effort}` as Parameters<typeof t>[0]),
          })),
        ]}
        onChange={selectReasoningEffort}
        dense
        style={styles.optionSelection}
      />

      {selectedFamily === "dsh" ? (
        <>
          <Text style={styles.sectionLabel}>{t("newTask.dshAgentPreset")}</Text>
          <AnimatedSelection
            value={dshAgentPreset}
            options={[
              {
                value: "standard",
                label: t("newTask.dshPreset.standard"),
                icon: <BookmarkPlus size={14} color={theme.accent} />,
              },
              {
                value: "code",
                label: t("newTask.dshPreset.code"),
                icon: <BookmarkPlus size={14} color={theme.accent} />,
              },
              {
                value: "minimal",
                label: t("newTask.dshPreset.minimal"),
                icon: <BookmarkPlus size={14} color={theme.accent} />,
              },
              {
                value: "cordis",
                label: t("newTask.dshPreset.cordis"),
                icon: <BookmarkPlus size={14} color={theme.accent} />,
              },
            ]}
            onChange={selectDshAgentPreset}
            horizontal
            style={styles.optionSelection}
          />
        </>
      ) : null}

      {selectedFamily !== "dsh" ? (
        <>
          <Text style={styles.sectionLabel}>{t("newTask.speed")}</Text>
          <AnimatedSelection
            value={speed}
            options={(["standard", "fast"] as const).map((value) => ({
              value,
              label: t(`newTask.speed.${value}` as Parameters<typeof t>[0]),
              icon:
                value === "fast" ? (
                  <Zap size={14} color={speed === value ? theme.onAccent : theme.accent} />
                ) : (
                  <Gauge size={14} color={speed === value ? theme.onAccent : theme.textSecondary} />
                ),
            }))}
            onChange={selectSpeed}
            style={styles.optionSelection}
          />
        </>
      ) : null}

      <Text style={styles.sectionLabel}>{t("newTask.permission")}</Text>
      <AnimatedSelection
        value={permissionMode}
        options={PERMISSION_MODE_VALUES.map((mode) => ({
          value: mode,
          label: t(`perm.${mode}`),
        }))}
        onChange={selectPermissionMode}
        style={styles.permissionSelection}
      />

      <Field
        label="Prompt"
        value={prompt}
        onChangeText={setPrompt}
        placeholder={t("newTask.promptPlaceholder")}
        multiline
        textAlignVertical="top"
        style={styles.promptInput}
      />

      <Button
        label={submitting ? t("newTask.submitting") : t("newTask.submit")}
        disabled={!canSubmit}
        onPress={submit}
        style={styles.submitButton}
      />
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
  onCreated?: (task: Task) => void;
}) {
  return (
    <Modal
      visible={visible}
      animationType="slide"
      presentationStyle="fullScreen"
      onRequestClose={onClose}
    >
      <KeyboardAvoidingView
        style={styles.screen}
        behavior={Platform.OS === "ios" ? "padding" : undefined}
      >
        <View style={styles.sheetHeader}>
          <Text style={styles.sheetTitle}>{t("nav.newTask")}</Text>
          <AnimatedPressable hitSlop={10} onPress={onClose} style={styles.closeButton}>
            <Text style={styles.sheetClose}>{t("newTask.cancel")}</Text>
          </AnimatedPressable>
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
    // 全屏 Modal 后 header 会顶到状态栏下,56 与首页 brandBar 的 paddingTop 保持同一约定
    paddingTop: Platform.OS === "ios" ? 56 : 44,
    paddingBottom: spacing.md,
    borderBottomWidth: StyleSheet.hairlineWidth,
    borderBottomColor: theme.border,
  },
  sheetTitle: { flex: 1, color: theme.text, fontSize: 17, fontWeight: "700" },
  sheetClose: { color: theme.accent, fontSize: typography.bodySize, fontWeight: "600" },
  content: { padding: spacing.lg, paddingBottom: 40, gap: spacing.sm },
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
    minHeight: 42,
    alignItems: "center",
    justifyContent: "center",
    paddingHorizontal: 13,
    paddingVertical: spacing.xs,
    borderRadius: radii.button,
    backgroundColor: theme.bgCard,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
    maxWidth: "100%",
  },
  chipActive: { backgroundColor: theme.accent, borderColor: theme.accent },
  chipText: {
    color: theme.text,
    fontSize: 13.5,
    includeFontPadding: false,
    textAlignVertical: "center",
  },
  chipTextActive: { color: theme.onAccent, fontWeight: "600" },
  agentColumns: { flexDirection: "row", alignItems: "stretch", gap: spacing.sm },
  agentColumn: { flex: 1, minWidth: 0, gap: spacing.xs },
  agentColumnHeader: {
    minHeight: 28,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "center",
    gap: 6,
  },
  agentColumnTitle: {
    color: theme.textSecondary,
    fontSize: typography.metaSize,
    fontWeight: "700",
  },
  agentList: {
    height: AGENT_LIST_MAX_HEIGHT,
    borderRadius: radii.row,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
    backgroundColor: theme.bgCard,
    overflow: "hidden",
  },
  agentRows: { padding: 4, gap: 4 },
  agentEmpty: {
    color: theme.textHint,
    fontSize: typography.metaSize,
    paddingVertical: spacing.md,
    textAlign: "center",
  },
  agentRow: {
    minHeight: AGENT_ROW_HEIGHT,
    alignItems: "center",
    justifyContent: "center",
    paddingHorizontal: spacing.sm,
    borderRadius: radii.button,
  },
  agentRowActive: { backgroundColor: theme.accent },
  // 不加 flex:1 —— flex:1 会把 Text 撑满整行,文字随之左对齐、无法真正居中
  agentRowText: {
    color: theme.text,
    fontSize: 14,
    flexShrink: 1,
    textAlign: "center",
    includeFontPadding: false,
    textAlignVertical: "center",
  },
  agentRowTextActive: { color: theme.onAccent, fontWeight: "600" },
  optionSelection: { alignSelf: "stretch" },
  modelSelection: { alignSelf: "stretch" },
  permissionSelection: { alignSelf: "stretch" },
  promptInput: {
    minHeight: 120,
    lineHeight: 20,
    paddingTop: spacing.md,
  },
  submitButton: {
    minHeight: 50,
    marginTop: spacing.sm,
  },
  closeButton: {
    minHeight: 34,
    alignItems: "center",
    justifyContent: "center",
    paddingHorizontal: spacing.xs,
  },
  hint: { color: theme.textHint, fontSize: typography.metaSize, lineHeight: 18 },
});
