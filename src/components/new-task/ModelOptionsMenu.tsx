import { useState, type CSSProperties, type ReactNode } from "react";
import { ChevronDown, ChevronRight, Cpu, Gauge, SlidersHorizontal, Zap } from "lucide-react";
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

type Panel = "model" | "reasoning" | "speed";

function setMenuItemHover(el: HTMLElement, hover: boolean, active: boolean) {
  const highlighted = hover || active;
  el.style.background = highlighted ? "var(--accent-subtle)" : "transparent";
  el.style.color = highlighted ? "var(--text-primary)" : "var(--text-secondary)";
  el.style.transform = hover ? "translateX(1px)" : "translateX(0)";
}

function optionButtonStyle(active: boolean): CSSProperties {
  return {
    ...s.toolbarMenuItem,
    width: "100%",
    boxSizing: "border-box",
    minHeight: 32,
    padding: "7px 9px",
    border: "none",
    background: active ? "var(--accent-subtle)" : "transparent",
    color: active ? "var(--text-primary)" : "var(--text-secondary)",
    textAlign: "left",
    whiteSpace: "nowrap",
    flexShrink: 0,
    transition: "background 0.12s ease, color 0.12s ease, transform 0.12s ease",
  };
}

function submenuStyle(active: boolean): CSSProperties {
  return {
    flex: active ? "0 1 auto" : "0 0 0px",
    width: active ? "max-content" : 0,
    maxWidth: active ? "calc(100vw - 24px)" : 0,
    minWidth: 0,
    opacity: active ? 1 : 0,
    transform: active ? "translateX(0)" : "translateX(-8px)",
    pointerEvents: active ? "auto" : "none",
    overflow: "hidden",
    transition: "opacity 0.14s ease, transform 0.17s ease",
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
  const [panel, setPanel] = useState<Panel | null>(null);

  function setMenuOpen(next: boolean) {
    setOpen(next);
    if (!next) setPanel(null);
    if (next) onOpen?.();
  }

  const modelLabel = selectedModel || (loading ? t("newTask.modelsLoading") : t("newTask.model"));
  const reasoningLabel = reasoningEffort
    ? t(`newTask.reasoning.${reasoningEffort}`)
    : t("newTask.modelDefault");
  const summaryTitle = `${modelLabel} · ${reasoningLabel}${
    speed === "fast" ? ` · ${t("newTask.speed.fast")}` : ""
  }`;
  const title = error ? `${error}\n${summaryTitle}` : summaryTitle;

  function renderModelOptions() {
    return models.length > 0 ? (
      models.map((model) => {
        const active = model === selectedModel;
        return (
          <button
            type="button"
            key={model}
            role="menuitemradio"
            aria-checked={active}
            data-model-option={model}
            style={optionButtonStyle(active)}
            onMouseEnter={(event) => setMenuItemHover(event.currentTarget, true, active)}
            onMouseLeave={(event) => setMenuItemHover(event.currentTarget, false, active)}
            onFocus={(event) => setMenuItemHover(event.currentTarget, true, active)}
            onBlur={(event) => setMenuItemHover(event.currentTarget, false, active)}
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
        );
      })
    ) : (
      <div style={{ padding: "12px 9px", color: "var(--text-hint)", fontSize: 12 }}>
        {loading ? t("newTask.modelsLoading") : t("newTask.modelsUnavailable")}
      </div>
    );
  }

  function renderReasoningOptions() {
    const options: Array<ReasoningEffort | null> = [null, ...efforts];
    return options.map((effort) => {
      const active = reasoningEffort === effort;
      return (
        <button
          type="button"
          key={effort ?? "default"}
          role="menuitemradio"
          aria-checked={active}
          style={optionButtonStyle(active)}
          onMouseEnter={(event) => setMenuItemHover(event.currentTarget, true, active)}
          onMouseLeave={(event) => setMenuItemHover(event.currentTarget, false, active)}
          onFocus={(event) => setMenuItemHover(event.currentTarget, true, active)}
          onBlur={(event) => setMenuItemHover(event.currentTarget, false, active)}
          onClick={() => {
            onReasoningChange(effort);
            setPanel(null);
          }}
        >
          {effort ? t(`newTask.reasoning.${effort}`) : t("newTask.modelDefault")}
        </button>
      );
    });
  }

  function renderSpeedOptions() {
    return (["standard", "fast"] as const).map((item) => {
      const active = speed === item;
      return (
        <button
          type="button"
          key={item}
          role="menuitemradio"
          aria-checked={active}
          style={optionButtonStyle(active)}
          onMouseEnter={(event) => setMenuItemHover(event.currentTarget, true, active)}
          onMouseLeave={(event) => setMenuItemHover(event.currentTarget, false, active)}
          onFocus={(event) => setMenuItemHover(event.currentTarget, true, active)}
          onBlur={(event) => setMenuItemHover(event.currentTarget, false, active)}
          onClick={() => {
            onSpeedChange(item);
            setPanel(null);
          }}
        >
          {item === "fast" && <Zap size={13} color="var(--speed-fast-fg)" aria-hidden="true" />}
          {t(`newTask.speed.${item}`)}
        </button>
      );
    });
  }

  function renderSubmenu(activePanel: Panel) {
    const content =
      activePanel === "model"
        ? renderModelOptions()
        : activePanel === "reasoning"
          ? renderReasoningOptions()
          : renderSpeedOptions();

    return (
      <div
        id={`model-options-panel-${activePanel}`}
        role="menu"
        aria-label={
          activePanel === "model"
            ? t("newTask.model")
            : activePanel === "reasoning"
              ? t("newTask.reasoningLabel")
              : t("newTask.speedLabel")
        }
        data-model-options-panel={activePanel}
        style={{
          ...submenuStyle(true),
          maxHeight:
            "min(320px, var(--radix-popover-content-available-height, calc(100vh - 42px)))",
          overflowY: "auto",
          overscrollBehavior: "contain",
          paddingLeft: 6,
          borderLeft: "1px solid var(--border-dim)",
        }}
      >
        {content}
      </div>
    );
  }

  function renderPanelTrigger(
    panelId: Panel,
    icon: ReactNode,
    label: string,
    value: ReactNode,
    valueTitle: string,
  ) {
    const active = panel === panelId;
    return (
      <button
        type="button"
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={active}
        aria-controls={active ? `model-options-panel-${panelId}` : undefined}
        data-model-options-trigger={panelId}
        style={optionButtonStyle(active)}
        onMouseEnter={() => setPanel(panelId)}
        onFocus={() => setPanel(panelId)}
        onClick={() => setPanel(panelId)}
        title={valueTitle}
      >
        {icon}
        <span style={{ flex: "0 0 auto", minWidth: 0, whiteSpace: "nowrap" }}>{label}</span>
        <span
          style={{
            minWidth: 0,
            maxWidth: "min(220px, 36vw)",
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
            flexShrink: 1,
            color: active ? "var(--text-secondary)" : "var(--text-hint)",
            fontSize: 11,
          }}
        >
          {value}
        </span>
        <ChevronRight
          size={13}
          strokeWidth={2.2}
          aria-hidden="true"
          data-model-options-arrow={panelId}
          style={{
            flexShrink: 0,
            color: active ? "var(--accent)" : "var(--text-hint)",
            opacity: active ? 1 : 0.72,
            transform: active ? "translateX(2px)" : "translateX(0)",
            transition: "color 0.12s ease, opacity 0.12s ease, transform 0.12s ease",
          }}
        />
      </button>
    );
  }

  return (
    <Popover.Root open={open} onOpenChange={setMenuOpen}>
      <Popover.Trigger asChild>
        <button
          type="button"
          role="combobox"
          aria-label={t("newTask.model")}
          aria-description={summaryTitle}
          aria-haspopup="menu"
          aria-expanded={open}
          title={title}
          data-model-options-menu-trigger
          style={{
            ...(compact ? s.toolbarBtnIconOnly : s.toolbarBtn),
            maxWidth: compact ? undefined : "min(240px, 32vw)",
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
              data-model-summary
              style={{
                minWidth: 0,
                flex: 1,
                display: "inline-flex",
                alignItems: "center",
                gap: 5,
                overflow: "hidden",
                whiteSpace: "nowrap",
              }}
            >
              <span
                data-model-summary-name
                data-testid="model-summary-name"
                style={{
                  minWidth: 0,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                  color: "var(--text-primary)",
                }}
              >
                {modelLabel}
              </span>
              <span aria-hidden="true" style={{ color: "var(--text-hint)", flexShrink: 0 }}>
                ·
              </span>
              <span
                data-model-summary-reasoning
                data-testid="model-summary-reasoning"
                style={{ color: "var(--text-secondary)", flexShrink: 0, fontSize: 11 }}
              >
                {reasoningLabel}
              </span>
              {speed === "fast" && (
                <span
                  data-fast-indicator
                  data-testid="fast-indicator"
                  role="img"
                  aria-label={t("newTask.speed.fast")}
                  title={t("newTask.speed.fast")}
                  style={{ display: "inline-flex", flexShrink: 0, color: "var(--speed-fast-fg)" }}
                >
                  <Zap size={13} strokeWidth={2.4} aria-hidden="true" />
                </span>
              )}
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
          avoidCollisions={false}
          data-model-options-content
          style={{
            ...s.toolbarMenuContent,
            width: "fit-content",
            minWidth: 0,
            maxWidth: "calc(100vw - 20px)",
            maxHeight:
              "min(360px, var(--radix-popover-content-available-height, calc(100vh - 24px)))",
            overflow: "hidden",
            padding: 6,
            zIndex: 4000,
          }}
        >
          <div
            style={{
              display: "flex",
              width: "fit-content",
              maxWidth: "100%",
              minWidth: 0,
              alignItems: "stretch",
              gap: panel ? 6 : 0,
            }}
          >
            <div
              role="menu"
              aria-label={t("newTask.modelOptions")}
              style={{
                display: "flex",
                flex: "0 0 auto",
                flexDirection: "column",
                gap: 2,
                width: "max-content",
                maxWidth: "100%",
                minWidth: 0,
              }}
            >
              {renderPanelTrigger(
                "model",
                <Cpu size={14} color="var(--usage-codex)" aria-hidden="true" />,
                t("newTask.model"),
                selectedModel || t("newTask.modelsUnavailable"),
                modelLabel,
              )}
              {renderPanelTrigger(
                "reasoning",
                <SlidersHorizontal size={14} color="var(--text-muted)" aria-hidden="true" />,
                t("newTask.reasoningLabel"),
                reasoningLabel,
                reasoningLabel,
              )}
              {renderPanelTrigger(
                "speed",
                <Gauge size={14} color="var(--text-muted)" aria-hidden="true" />,
                t("newTask.speedLabel"),
                <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
                  {speed === "fast" && (
                    <Zap size={12} color="var(--speed-fast-fg)" aria-hidden="true" />
                  )}
                  {t(`newTask.speed.${speed}`)}
                </span>,
                t(`newTask.speed.${speed}`),
              )}
            </div>
            <div style={submenuStyle(panel !== null)}>{panel ? renderSubmenu(panel) : null}</div>
          </div>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
