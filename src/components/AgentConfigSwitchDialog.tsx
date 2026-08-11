import { useEffect, useMemo, useState, type CSSProperties } from "react";
import { AlertTriangle, Check, LoaderCircle, X } from "lucide-react";
import type { AgentOption } from "../agents";
import { agentDisplayLabel, isCodexLikeAgent } from "../agents";
import { useAgentOptions } from "../hooks/useAgentOptions";
import { getCachedAgentModels, refreshAgentModels } from "../hooks/agentModelCache";
import { useI18n } from "../i18n";
import { availableReasoningEfforts, type ReasoningEffort, type TaskSpeed } from "../modelOptions";
import type { AgentType, PermissionMode, Task } from "../types";
import { permissionModeLabel } from "../types";
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

function modelSelectStyle(): CSSProperties {
  return {
    width: "100%",
    minHeight: 34,
    padding: "7px 30px 7px 10px",
    border: "1px solid var(--border-medium)",
    borderRadius: "var(--radius-md)",
    background: "var(--bg-input)",
    color: "var(--text-primary)",
    fontFamily: "var(--font-ui)",
    fontSize: 12.5,
    outline: "none",
  };
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
  onSubmit: (values: AgentConfigSwitchValues) => Promise<void> | void;
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
      await onSubmit({
        agent,
        selectedModel: selectedModel || undefined,
        reasoningEffort,
        speed,
        permissionMode,
      });
    } catch {
      setSubmitting(false);
    }
  }

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
        style={{
          width: "min(620px, calc(100vw - 28px))",
          maxHeight: "min(760px, calc(100vh - 28px))",
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
          background: "var(--bg-card)",
          border: "1px solid var(--border-medium)",
          borderRadius: 12,
          boxShadow: "var(--shadow-popover)",
        }}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div style={{ ...s.settingsContentHeader, padding: "16px 18px 14px" }}>
          <div>
            <div id="agent-config-switch-title" style={s.settingsContentTitle}>
              {t("running.switchConfigTitle")}
            </div>
            <div style={{ marginTop: 4, color: "var(--text-hint)", fontSize: 11.5 }}>
              {t("running.switchConfigHint", { agent: sourceLabel })}
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

        <div style={{ padding: "18px", overflowY: "auto", display: "grid", gap: 16 }}>
          <label style={{ display: "grid", gap: 6 }}>
            <span style={s.modalLabel}>{t("running.switchConfigFile")}</span>
            <select
              aria-label={t("running.switchConfigFile")}
              value={agent}
              onChange={(event) => setAgent(event.currentTarget.value as AgentType)}
              style={modelSelectStyle()}
              disabled={submitting}
            >
              {agentOptions.map((option: AgentOption) => (
                <option key={option.value} value={option.value}>
                  {option.label} · {option.configFile || t("running.builtInConfig")}
                </option>
              ))}
            </select>
          </label>

          <label style={{ display: "grid", gap: 6 }}>
            <span style={s.modalLabel}>{t("newTask.model")}</span>
            <select
              aria-label={t("newTask.model")}
              value={selectedModel}
              onChange={(event) => setSelectedModel(event.currentTarget.value)}
              style={modelSelectStyle()}
              disabled={submitting || loadingModels || models.length === 0}
            >
              {models.length === 0 && <option value="">{t("newTask.modelsUnavailable")}</option>}
              {models.map((model) => (
                <option key={model} value={model}>
                  {model}
                </option>
              ))}
            </select>
            {loadingModels && (
              <span style={{ color: "var(--text-hint)", fontSize: 11.5 }}>
                <LoaderCircle
                  size={12}
                  className="spin"
                  style={{ verticalAlign: "-2px", marginRight: 4 }}
                />
                {t("newTask.modelsLoading")}
              </span>
            )}
            {modelsError && (
              <span style={{ color: "var(--danger)", fontSize: 11.5 }}>
                <AlertTriangle size={12} style={{ verticalAlign: "-2px", marginRight: 4 }} />
                {modelsError}
              </span>
            )}
          </label>

          <div style={{ display: "grid", gap: 7 }}>
            <span style={s.modalLabel}>{t("newTask.reasoningLabel")}</span>
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
              style={{ flexWrap: "wrap", padding: 3 }}
              itemStyle={{ minHeight: 28, padding: "5px 9px", fontSize: 11.5 }}
            />
          </div>

          <div style={{ display: "grid", gap: 7 }}>
            <span style={s.modalLabel}>{t("newTask.speedLabel")}</span>
            <AnimatedSelectionGroup
              value={speed}
              options={[
                { value: "standard", label: t("newTask.speed.standard") },
                { value: "fast", label: t("newTask.speed.fast") },
              ]}
              onChange={setSpeed}
              ariaLabel={t("newTask.speedLabel")}
              equalWidth
              style={{ maxWidth: 260, padding: 3 }}
              itemStyle={{ minHeight: 28, padding: "5px 12px", fontSize: 11.5 }}
            />
          </div>

          <div style={{ display: "grid", gap: 7 }}>
            <span style={s.modalLabel}>{t("running.switchPermission")}</span>
            <AnimatedSelectionGroup
              value={permissionMode}
              options={(["ask", "auto_edit", "full_access"] as PermissionMode[]).map((mode) => ({
                value: mode,
                label: permissionModeLabel(mode, agent),
              }))}
              onChange={setPermissionMode}
              ariaLabel={t("running.switchPermission")}
              style={{ flexWrap: "wrap", padding: 3 }}
              itemStyle={{ minHeight: 28, padding: "5px 9px", fontSize: 11.5 }}
            />
          </div>
        </div>

        <div style={s.settingsFooter}>
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
