import { useState, type CSSProperties } from "react";
import { ChevronDown, ChevronLeft, Cpu, Gauge, SlidersHorizontal } from "lucide-react";
import * as Popover from "@radix-ui/react-popover";
import type { AgentType } from "../../types";
import { isCodexLikeAgent } from "../../agents";
import { useAgentOptions } from "../../hooks/useAgentOptions";
import { useI18n } from "../../i18n";
import {
  availableReasoningEfforts,
  type ReasoningEffort,
  type TaskSpeed,
} from "../../modelOptions";
import s from "../../styles";

type Panel = "model" | "reasoning" | "speed" | null;

function optionButtonStyle(active: boolean): CSSProperties {
  return {
    display: "flex",
    alignItems: "center",
    gap: 8,
    width: "100%",
    minHeight: 32,
    padding: "6px 8px",
    border: "none",
    borderRadius: 7,
    background: active ? "var(--accent-subtle)" : "transparent",
    color: active ? "var(--text-primary)" : "var(--text-secondary)",
    textAlign: "left",
    cursor: "pointer",
    fontSize: 12,
  };
}

export function ModelOptionsMenu({
  agent,
  models,
  selectedModel,
  onModelChange,
  reasoningEffort,
  onReasoningChange,
  speed,
  onSpeedChange,
  loading,
  error,
  compact = false,
  onOpen,
}: {
  agent: AgentType;
  models: string[];
  selectedModel: string;
  onModelChange: (model: string) => void;
  reasoningEffort: ReasoningEffort | null;
  onReasoningChange: (effort: ReasoningEffort | null) => void;
  speed: TaskSpeed;
  onSpeedChange: (speed: TaskSpeed) => void;
  loading: boolean;
  error: string | null;
  compact?: boolean;
  onOpen?: () => void;
}) {
  const { t } = useI18n();
  const agentOptions = useAgentOptions();
  const codexLike = isCodexLikeAgent(agent, agentOptions);
  const efforts = availableReasoningEfforts(codexLike, selectedModel);
  const [open, setOpen] = useState(false);
  const [panel, setPanel] = useState<Panel>(null);

  function setMenuOpen(next: boolean) {
    setOpen(next);
    if (!next) setPanel(null);
    if (next) onOpen?.();
  }

  const valueLabel = selectedModel || (loading ? t("newTask.modelsLoading") : t("newTask.model"));
  const title = error || selectedModel || t("newTask.modelsUnavailable");

  return (
    <Popover.Root open={open} onOpenChange={setMenuOpen}>
      <Popover.Trigger asChild>
        <button
          type="button"
          role="combobox"
          aria-label={t("newTask.model")}
          title={title}
          style={{
            ...(compact ? s.toolbarBtnIconOnly : s.toolbarBtn),
            maxWidth: compact ? undefined : 190,
            minHeight: 24,
            height: 24,
            padding: compact ? 0 : "2px 7px",
            display: "inline-flex",
            alignItems: "center",
            gap: 5,
          }}
        >
          <Cpu size={14} strokeWidth={2} color="var(--usage-codex)" />
          {!compact && (
            <span
              style={{
                minWidth: 0,
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
            >
              {valueLabel}
            </span>
          )}
          {!compact && (
            <ChevronDown size={12} strokeWidth={2.5} style={{ opacity: 0.58, flexShrink: 0 }} />
          )}
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          side="bottom"
          align="end"
          sideOffset={6}
          collisionPadding={8}
          style={{
            ...s.toolbarMenuContent,
            width: "min(330px, calc(100vw - 20px))",
            maxHeight: "min(360px, calc(100vh - 24px))",
            overflow: "auto",
            padding: 7,
            zIndex: 4000,
          }}
        >
          {panel ? (
            <>
              <button
                type="button"
                style={{ ...optionButtonStyle(false), color: "var(--text-hint)", marginBottom: 3 }}
                onClick={() => setPanel(null)}
              >
                <ChevronLeft size={14} />
                {t("newTask.modelOptionsBack")}
              </button>
              {panel === "model" && (
                <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
                  {models.length > 0 ? (
                    models.map((model) => (
                      <button
                        type="button"
                        key={model}
                        style={optionButtonStyle(model === selectedModel)}
                        onClick={() => {
                          onModelChange(model);
                          setPanel(null);
                        }}
                        title={model}
                      >
                        <Cpu size={13} color="var(--text-muted)" />
                        <span
                          style={{
                            minWidth: 0,
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                            whiteSpace: "nowrap",
                          }}
                        >
                          {model}
                        </span>
                      </button>
                    ))
                  ) : (
                    <div style={{ padding: "12px 8px", color: "var(--text-hint)", fontSize: 12 }}>
                      {loading ? t("newTask.modelsLoading") : t("newTask.modelsUnavailable")}
                    </div>
                  )}
                </div>
              )}
              {panel === "reasoning" && (
                <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
                  <button
                    type="button"
                    style={optionButtonStyle(reasoningEffort === null)}
                    onClick={() => {
                      onReasoningChange(null);
                      setPanel(null);
                    }}
                  >
                    {t("newTask.modelDefault")}
                  </button>
                  {efforts.map((effort) => (
                    <button
                      type="button"
                      key={effort}
                      style={optionButtonStyle(reasoningEffort === effort)}
                      onClick={() => {
                        onReasoningChange(effort);
                        setPanel(null);
                      }}
                    >
                      {t(`newTask.reasoning.${effort}`)}
                    </button>
                  ))}
                </div>
              )}
              {panel === "speed" && (
                <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
                  {(["standard", "fast"] as const).map((item) => (
                    <button
                      type="button"
                      key={item}
                      style={optionButtonStyle(speed === item)}
                      onClick={() => {
                        onSpeedChange(item);
                        setPanel(null);
                      }}
                    >
                      {t(`newTask.speed.${item}`)}
                    </button>
                  ))}
                </div>
              )}
            </>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
              <button
                type="button"
                aria-label={t("newTask.model")}
                style={optionButtonStyle(false)}
                onClick={() => setPanel("model")}
              >
                <Cpu size={14} />
                <span style={{ flex: 1 }}>{t("newTask.model")}</span>
                <span
                  style={{
                    maxWidth: 150,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                    color: "var(--text-hint)",
                  }}
                >
                  {selectedModel || t("newTask.modelsUnavailable")}
                </span>
              </button>
              <button
                type="button"
                aria-label={t("newTask.reasoningLabel")}
                style={optionButtonStyle(false)}
                onClick={() => setPanel("reasoning")}
              >
                <SlidersHorizontal size={14} />
                <span style={{ flex: 1 }}>{t("newTask.reasoningLabel")}</span>
                <span style={{ color: "var(--text-hint)" }}>
                  {reasoningEffort
                    ? t(`newTask.reasoning.${reasoningEffort}`)
                    : t("newTask.modelDefault")}
                </span>
              </button>
              <button
                type="button"
                aria-label={t("newTask.speedLabel")}
                style={optionButtonStyle(false)}
                onClick={() => setPanel("speed")}
              >
                <Gauge size={14} />
                <span style={{ flex: 1 }}>{t("newTask.speedLabel")}</span>
                <span style={{ color: "var(--text-hint)" }}>{t(`newTask.speed.${speed}`)}</span>
              </button>
            </div>
          )}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
