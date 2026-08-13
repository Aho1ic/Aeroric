import { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  Check,
  ChevronDown,
  LoaderCircle,
  SlidersHorizontal,
  X,
} from "lucide-react";
import type { AgentOption } from "../agents";
import { agentDisplayLabel, isCodexLikeAgent } from "../agents";
import claudeLogo from "../assets/claude.svg";
import chatgptLogo from "../assets/chatgpt.svg";
import { useAgentOptions } from "../hooks/useAgentOptions";
import { getCachedAgentModels, refreshAgentModels } from "../hooks/agentModelCache";
import { useI18n } from "../i18n";
import { availableReasoningEfforts, type ReasoningEffort, type TaskSpeed } from "../modelOptions";
import type { AgentType, PermissionMode, Task } from "../types";
import { AnimatedSelectionGroup } from "./ui/AnimatedSelection";
import { Button } from "./ui/Button";
import s from "../styles";

export interface AgentConfigSwitchValues {
  agent: AgentType;
  selectedModel?: string;
  reasoningEffort?: ReasoningEffort | null;
  speed: TaskSpeed;
  permissionMode: PermissionMode;
}

interface AgentModelSnapshot {
  models: string[];
  reasoning_effort?: string | null;
  reasoning_speed?: string | null;
}

export function AgentConfigSwitchDialog({
  task,
  open,
  onClose,
  onSubmit,
}: {
  task: Task;
  open: boolean;
  onClose: () => void;
  onSubmit: (values: AgentConfigSwitchValues) => Promise<boolean | void> | boolean | void;
}) {
  const { t } = useI18n();
  const agentOptions = useAgentOptions();
  const currentAgent = task.agent;
  const [agent, setAgent] = useState<AgentType>(currentAgent);
  const [models, setModels] = useState<string[]>([]);
  const [selectedModel, setSelectedModel] = useState(task.selectedModel ?? "");
  const [reasoningEffort, setReasoningEffort] = useState<ReasoningEffort | null>(
    (task.reasoningEffort as ReasoningEffort | undefined) ?? null,
  );
  const [speed, setSpeed] = useState<TaskSpeed>(task.speed === "fast" ? "fast" : "standard");
  const [permissionMode, setPermissionMode] = useState<PermissionMode>(task.permissionMode);
  const [loadingModels, setLoadingModels] = useState(false);
  const [modelsError, setModelsError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const selectedOption = useMemo(
    () => agentOptions.find((option) => option.value === agent),
    [agent, agentOptions],
  );
  const groupedOptions = useMemo(
    () => ({
      claude: agentOptions.filter((option) => !option.codexLike),
      codex: agentOptions.filter((option) => option.codexLike),
    }),
    [agentOptions],
  );
  const codexLike = isCodexLikeAgent(agent, agentOptions);
  const efforts = availableReasoningEfforts(codexLike, selectedModel);

  useEffect(() => {
    if (!open) return;
    setAgent(currentAgent);
    setSelectedModel(task.selectedModel ?? "");
    setReasoningEffort((task.reasoningEffort as ReasoningEffort | undefined) ?? null);
    setSpeed(task.speed === "fast" ? "fast" : "standard");
    setPermissionMode(task.permissionMode);
    setModelsError(null);
    setSubmitting(false);
  }, [
    currentAgent,
    open,
    task.permissionMode,
    task.reasoningEffort,
    task.selectedModel,
    task.speed,
  ]);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    const cached = getCachedAgentModels(agent);
    if (cached) {
      setModels(cached.models);
      setSelectedModel(
        (current) => cached.models.find((model) => model === current) ?? cached.models[0] ?? "",
      );
    } else {
      setModels([]);
    }
    setLoadingModels(true);
    setModelsError(null);
    refreshAgentModels(agent)
      .then((result: AgentModelSnapshot) => {
        if (cancelled) return;
        setModels(result.models);
        setSelectedModel(
          (current) => result.models.find((model) => model === current) ?? result.models[0] ?? "",
        );
        if (
          result.reasoning_effort &&
          availableReasoningEfforts(codexLike, result.models[0]).includes(
            result.reasoning_effort as ReasoningEffort,
          )
        ) {
          setReasoningEffort((current) => current ?? (result.reasoning_effort as ReasoningEffort));
        }
        if (result.reasoning_speed === "fast") {
          setSpeed((current) => (current === "standard" ? "fast" : current));
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) setModelsError(error instanceof Error ? error.message : String(error));
      })
      .finally(() => {
        if (!cancelled) setLoadingModels(false);
      });
    return () => {
      cancelled = true;
    };
  }, [agent, codexLike, open]);

  useEffect(() => {
    if (reasoningEffort && !efforts.includes(reasoningEffort)) setReasoningEffort(null);
  }, [efforts, reasoningEffort]);

  if (!open) return null;

  const canSubmit = Boolean(
    selectedOption && !loadingModels && (models.length === 0 || selectedModel),
  );
  const sourceLabel = agentDisplayLabel(currentAgent, agentOptions);

  async function handleSubmit() {
    if (!canSubmit || submitting) return;
    setSubmitting(true);
    try {
      const applied = await onSubmit({
        agent,
        selectedModel: selectedModel || undefined,
        reasoningEffort,
        speed,
        permissionMode,
      });
      if (applied === false) setSubmitting(false);
    } catch {
      setSubmitting(false);
    }
  }

  const renderConfigGroup = (family: "claude" | "codex", label: string, options: AgentOption[]) => (
    <div className="agent-config-switch-group" role="group" aria-label={label}>
      <div className="agent-config-switch-group-title">
        <img src={family === "codex" ? chatgptLogo : claudeLogo} alt="" aria-hidden="true" />
        <span>{label}</span>
        <span className="agent-config-switch-group-count">{options.length}</span>
      </div>
      <div className="agent-config-switch-options">
        {options.length === 0 ? (
          <div className="agent-config-switch-empty">{t("newTask.noAgentConfigurations")}</div>
        ) : (
          options.map((option) => {
            const selected = option.value === agent;
            return (
              <button
                key={option.value}
                type="button"
                className={`agent-config-switch-option${selected ? " selected" : ""}`}
                aria-label={option.label}
                aria-pressed={selected}
                disabled={submitting}
                onClick={() => setAgent(option.value)}
              >
                <img src={option.codexLike ? chatgptLogo : claudeLogo} alt="" aria-hidden="true" />
                <span className="agent-config-switch-option-copy">
                  <span className="agent-config-switch-option-name">{option.label}</span>
                  <span className="agent-config-switch-option-path">
                    {option.configFile || t("running.builtInConfig")}
                  </span>
                </span>
                <span className="agent-config-switch-option-check" aria-hidden="true">
                  {selected && <Check size={13} strokeWidth={2.6} />}
                </span>
              </button>
            );
          })
        )}
      </div>
    </div>
  );

  return (
    <div
      role="presentation"
      style={s.modalOverlay}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !submitting) onClose();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="agent-config-switch-title"
        className="agent-config-switch-dialog"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="agent-config-switch-header">
          <div className="agent-config-switch-heading">
            <span className="agent-config-switch-heading-icon" aria-hidden="true">
              <SlidersHorizontal size={16} strokeWidth={2} />
            </span>
            <div>
              <div id="agent-config-switch-title" className="agent-config-switch-title">
                {t("running.switchConfigTitle")}
              </div>
              <div className="agent-config-switch-subtitle">
                {t("running.switchConfigHint", { agent: sourceLabel })}
              </div>
            </div>
          </div>
          <button
            type="button"
            style={s.modalCloseBtn}
            onClick={onClose}
            disabled={submitting}
            title={t("common.close")}
          >
            <X size={16} strokeWidth={2} />
          </button>
        </div>

        <div className="agent-config-switch-body">
          <section className="agent-config-switch-section">
            <div className="agent-config-switch-label">{t("running.switchConfigFile")}</div>
            <div className="agent-config-switch-groups">
              {renderConfigGroup("claude", t("newTask.claudeAgents"), groupedOptions.claude)}
              {renderConfigGroup("codex", t("newTask.codexAgents"), groupedOptions.codex)}
            </div>
          </section>

          <label className="agent-config-switch-field">
            <span className="agent-config-switch-label">{t("newTask.model")}</span>
            <span className="agent-config-switch-select-shell">
              <select
                aria-label={t("newTask.model")}
                value={selectedModel}
                onChange={(event) => setSelectedModel(event.currentTarget.value)}
                className="agent-config-switch-select"
                disabled={submitting || loadingModels || models.length === 0}
              >
                {models.length === 0 && <option value="">{t("newTask.modelsUnavailable")}</option>}
                {models.map((model) => (
                  <option key={model} value={model}>
                    {model}
                  </option>
                ))}
              </select>
              <ChevronDown size={14} strokeWidth={2} aria-hidden="true" />
            </span>
            {loadingModels && (
              <span className="agent-config-switch-status">
                <LoaderCircle size={12} className="spin" />
                {t("newTask.modelsLoading")}
              </span>
            )}
            {modelsError && (
              <span className="agent-config-switch-status error">
                <AlertTriangle size={12} />
                {modelsError}
              </span>
            )}
          </label>

          <div className="agent-config-switch-controls">
            <div className="agent-config-switch-field">
              <span className="agent-config-switch-label">{t("newTask.reasoningLabel")}</span>
              <AnimatedSelectionGroup
                value={reasoningEffort ?? "default"}
                options={[
                  { value: "default", label: t("newTask.modelDefault") },
                  ...efforts.map((effort) => ({
                    value: effort,
                    label: t(`newTask.reasoning.${effort}`),
                  })),
                ]}
                onChange={(value) =>
                  setReasoningEffort(value === "default" ? null : (value as ReasoningEffort))
                }
                ariaLabel={t("newTask.reasoningLabel")}
                equalWidth
                className="agent-config-switch-segmented reasoning"
                itemClassName="agent-config-switch-segmented-item"
              />
            </div>

            <div className="agent-config-switch-field">
              <span className="agent-config-switch-label">{t("newTask.speedLabel")}</span>
              <AnimatedSelectionGroup
                value={speed}
                options={[
                  { value: "standard", label: t("newTask.speed.standard") },
                  { value: "fast", label: t("newTask.speed.fast") },
                ]}
                onChange={setSpeed}
                ariaLabel={t("newTask.speedLabel")}
                equalWidth
                className="agent-config-switch-segmented"
                itemClassName="agent-config-switch-segmented-item"
              />
            </div>

            <div className="agent-config-switch-field">
              <span className="agent-config-switch-label">{t("running.switchPermission")}</span>
              <AnimatedSelectionGroup
                value={permissionMode}
                options={(["ask", "auto_edit", "full_access"] as PermissionMode[]).map((mode) => ({
                  value: mode,
                  label:
                    mode === "ask"
                      ? t("running.permission.ask")
                      : mode === "auto_edit"
                        ? t("running.permission.autoEdit")
                        : t("running.permission.fullAccess"),
                }))}
                onChange={setPermissionMode}
                ariaLabel={t("running.switchPermission")}
                equalWidth
                className="agent-config-switch-segmented"
                itemClassName="agent-config-switch-segmented-item"
              />
            </div>
          </div>
        </div>

        <div style={s.settingsFooter} className="agent-config-switch-footer">
          <Button variant="ghost" size="sm" onClick={onClose} disabled={submitting}>
            {t("common.cancel")}
          </Button>
          <Button size="sm" onClick={handleSubmit} disabled={!canSubmit || submitting}>
            {submitting ? <LoaderCircle size={13} className="spin" /> : <Check size={13} />}
            {submitting ? t("running.switchingConfig") : t("running.switchConfigApply")}
          </Button>
        </div>
      </div>
    </div>
  );
}
