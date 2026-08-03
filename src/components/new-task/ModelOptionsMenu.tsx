import {
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
} from "react";
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

const SUBMENU_OPEN_DELAY_MS = 150;
const MOUSE_MOVEMENT_TOLERANCE_PX = 2;

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
    justifyContent: "flex-start",
    whiteSpace: "nowrap",
    flexShrink: 0,
    transition:
      "background-color 180ms ease, color 180ms ease, transform 220ms cubic-bezier(0.22, 1, 0.36, 1)",
  };
}

function submenuContentStyle(): CSSProperties {
  return {
    width: "fit-content",
    maxWidth: "calc(100vw - 24px)",
    minWidth: 0,
    maxHeight: "min(320px, var(--radix-popover-content-available-height, calc(100vh - 42px)))",
    overflowY: "auto",
    overscrollBehavior: "contain",
    padding: 6,
  };
}

function isModelOptionsSubmenuTarget(target: EventTarget | null) {
  return (
    target instanceof Element && target.closest("[data-model-options-submenu-content]") !== null
  );
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
  const pendingPanelRef = useRef<Panel | null>(null);
  const hoverTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastPointerPositionRef = useRef<{ x: number; y: number } | null>(null);

  function clearHoverTimer() {
    if (hoverTimerRef.current !== null) {
      window.clearTimeout(hoverTimerRef.current);
      hoverTimerRef.current = null;
    }
  }

  function schedulePanelOpen(panelId: Panel) {
    clearHoverTimer();
    pendingPanelRef.current = panelId;
    hoverTimerRef.current = window.setTimeout(() => {
      if (pendingPanelRef.current === panelId) {
        pendingPanelRef.current = null;
        setPanel(panelId);
      }
      hoverTimerRef.current = null;
    }, SUBMENU_OPEN_DELAY_MS);
  }

  function cancelPendingPanel(panelId: Panel) {
    if (pendingPanelRef.current !== panelId) return;
    pendingPanelRef.current = null;
    lastPointerPositionRef.current = null;
    clearHoverTimer();
  }

  function handlePanelMouseEnter(panelId: Panel, event: ReactMouseEvent<HTMLButtonElement>) {
    lastPointerPositionRef.current = { x: event.clientX, y: event.clientY };
    if (panel === panelId) return;
    schedulePanelOpen(panelId);
  }

  function handlePanelMouseMove(panelId: Panel, event: ReactMouseEvent<HTMLButtonElement>) {
    if (panel === panelId) return;
    const previous = lastPointerPositionRef.current;
    const current = { x: event.clientX, y: event.clientY };
    lastPointerPositionRef.current = current;
    if (!previous) {
      schedulePanelOpen(panelId);
      return;
    }
    if (
      Math.abs(current.x - previous.x) <= MOUSE_MOVEMENT_TOLERANCE_PX &&
      Math.abs(current.y - previous.y) <= MOUSE_MOVEMENT_TOLERANCE_PX
    ) {
      return;
    }
    schedulePanelOpen(panelId);
  }

  function handlePanelMouseLeave(panelId: Panel) {
    cancelPendingPanel(panelId);
  }

  function setMenuOpen(next: boolean) {
    setOpen(next);
    if (!next) {
      clearHoverTimer();
      pendingPanelRef.current = null;
      lastPointerPositionRef.current = null;
      setPanel(null);
    }
    if (next) onOpen?.();
  }

  useEffect(() => {
    return () => {
      if (hoverTimerRef.current !== null) {
        window.clearTimeout(hoverTimerRef.current);
      }
    };
  }, []);

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
          }}
        >
          <span style={{ flex: 1, minWidth: 0, textAlign: "left" }}>
            {t(`newTask.speed.${item}`)}
          </span>
          {item === "fast" && <Zap size={13} color="var(--speed-fast-fg)" aria-hidden="true" />}
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
        role="menu"
        aria-label={
          activePanel === "model"
            ? t("newTask.model")
            : activePanel === "reasoning"
              ? t("newTask.reasoningLabel")
              : t("newTask.speedLabel")
        }
        data-model-options-panel={activePanel}
        data-model-options-submenu-content={activePanel}
        className="model-options-scroll"
        style={submenuContentStyle()}
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
      <Popover.Root open={active} modal={false} onOpenChange={() => {}}>
        <Popover.Anchor asChild>
          <button
            type="button"
            aria-label={label}
            aria-haspopup="menu"
            aria-expanded={active}
            aria-controls={active ? `model-options-panel-${panelId}` : undefined}
            data-model-options-trigger={panelId}
            style={optionButtonStyle(active)}
            onMouseEnter={(event) => {
              setMenuItemHover(event.currentTarget, true, active);
              handlePanelMouseEnter(panelId, event);
            }}
            onMouseMove={(event) => handlePanelMouseMove(panelId, event)}
            onMouseLeave={(event) => {
              setMenuItemHover(event.currentTarget, false, active);
              handlePanelMouseLeave(panelId);
            }}
            onFocus={(event) => setMenuItemHover(event.currentTarget, true, active)}
            onBlur={(event) => setMenuItemHover(event.currentTarget, false, active)}
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
        </Popover.Anchor>
        <Popover.Portal>
          <Popover.Content
            id={`model-options-panel-${panelId}`}
            side="right"
            align="start"
            sideOffset={6}
            collisionPadding={8}
            avoidCollisions={false}
            data-model-options-submenu-content={panelId}
            className="model-options-submenu model-options-scroll"
            onEscapeKeyDown={(event) => {
              event.preventDefault();
              setMenuOpen(false);
            }}
            onOpenAutoFocus={(event) => event.preventDefault()}
            onCloseAutoFocus={(event) => event.preventDefault()}
            style={{
              ...s.toolbarMenuContent,
              ...submenuContentStyle(),
              borderLeft: "none",
              borderRight: "none",
              zIndex: 4001,
            }}
          >
            {active ? renderSubmenu(panelId) : null}
          </Popover.Content>
        </Popover.Portal>
      </Popover.Root>
    );
  }

  return (
    <Popover.Root open={open} modal={false} onOpenChange={setMenuOpen}>
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
                  className="model-options-fast-indicator"
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
          onInteractOutside={(event) => {
            if (isModelOptionsSubmenuTarget(event.target)) {
              event.preventDefault();
            }
          }}
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
                  {t(`newTask.speed.${speed}`)}
                  {speed === "fast" && (
                    <Zap size={12} color="var(--speed-fast-fg)" aria-hidden="true" />
                  )}
                </span>,
                t(`newTask.speed.${speed}`),
              )}
            </div>
          </div>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
