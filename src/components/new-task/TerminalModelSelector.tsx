import { useCallback, useEffect, useMemo, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ChevronDown, Cpu } from "lucide-react";
import * as Popover from "@radix-ui/react-popover";
import type { AgentType } from "../../types";
import { useI18n } from "../../i18n";
import s from "../../styles";
import type { ComposeMenu } from "./AgentPermSelector";
import { nextComposeMenuState } from "./AgentPermSelector";

interface AgentModels {
  models: string[];
}

function setMenuItemHover(el: HTMLElement, hover: boolean) {
  el.style.background = hover ? "var(--accent-subtle)" : "transparent";
}

export function terminalModelMenuContentStyle(): CSSProperties {
  return {
    ...s.toolbarMenuContent,
    maxHeight: "min(280px, var(--radix-popover-content-available-height))",
    overflow: "hidden",
  };
}

export function terminalModelMenuScrollStyle(): CSSProperties {
  return {
    display: "flex",
    flexDirection: "column",
    gap: 2,
    maxHeight: "min(280px, var(--radix-popover-content-available-height))",
    overflowY: "auto",
    overscrollBehavior: "contain",
  };
}

export function TerminalModelSelector({
  agent,
  selectedModel,
  compact = false,
  openMenu: controlledOpenMenu,
  onOpenMenuChange,
  onSetModel,
}: {
  agent: AgentType;
  selectedModel?: string;
  compact?: boolean;
  openMenu?: ComposeMenu;
  onOpenMenuChange?: (menu: ComposeMenu) => void;
  onSetModel: (model: string | undefined) => void;
}) {
  const { t } = useI18n();
  const [models, setModels] = useState<string[]>([]);
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(false);
  const [loadFailed, setLoadFailed] = useState(false);
  const [internalOpenMenu, setInternalOpenMenu] = useState<ComposeMenu>(null);
  const openMenu = controlledOpenMenu ?? internalOpenMenu;
  const open = openMenu === "model";
  const controlButtonStyle = {
    ...(compact ? s.toolbarBtnIconOnly : s.toolbarBtn),
    minHeight: 24,
    height: 24,
    padding: compact ? 0 : "2px 7px",
  };

  const setOpenMenu = (menu: ComposeMenu) => {
    if (onOpenMenuChange) {
      onOpenMenuChange(menu);
    } else {
      setInternalOpenMenu(menu);
    }
  };

  const loadModels = useCallback(async () => {
    setLoading(true);
    setLoadFailed(false);
    try {
      const result = await invoke<AgentModels>("detect_configured_agent_models", { agent });
      const next = Array.from(
        new Set(
          [...(result.models ?? []), selectedModel ?? ""].map((m) => m.trim()).filter(Boolean),
        ),
      );
      setModels(next);
      if (next.length > 0 && !selectedModel) {
        onSetModel(next[0]);
      }
      if (next.length === 0 && !selectedModel) {
        onSetModel(undefined);
      }
    } catch {
      setModels(selectedModel ? [selectedModel] : []);
      setLoadFailed(true);
      if (!selectedModel) onSetModel(undefined);
    } finally {
      setLoading(false);
    }
  }, [agent, onSetModel, selectedModel]);

  useEffect(() => {
    void loadModels();
  }, [loadModels]);

  useEffect(() => {
    setQuery("");
  }, [agent]);

  const currentModel = useMemo(() => {
    if (selectedModel && models.includes(selectedModel)) return selectedModel;
    return selectedModel || models[0] || "";
  }, [models, selectedModel]);

  const filteredModels = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return models.filter((model) => !needle || model.toLowerCase().includes(needle));
  }, [models, query]);

  const label = loading
    ? t("newTask.modelLoading")
    : currentModel || (loadFailed ? t("newTask.modelUnavailable") : t("newTask.model"));

  return (
    <Popover.Root
      open={open}
      onOpenChange={(nextOpen) => {
        setOpenMenu(nextComposeMenuState(openMenu, "model", nextOpen));
        if (nextOpen) void loadModels();
      }}
    >
      <Popover.Trigger asChild>
        <button style={controlButtonStyle} aria-label="Model" title={label}>
          <Cpu size={13} strokeWidth={2} color="var(--usage-codex)" />
          {!compact && <span>{label}</span>}
          {!compact && models.length > 0 && (
            <ChevronDown size={12} strokeWidth={2.5} style={{ opacity: 0.58 }} />
          )}
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          side="bottom"
          align="end"
          sideOffset={6}
          collisionPadding={8}
          style={terminalModelMenuContentStyle()}
        >
          <div style={terminalModelMenuScrollStyle()}>
            <div style={{ padding: "6px 8px 7px" }}>
              <input
                value={query}
                onChange={(event) => setQuery(event.currentTarget.value)}
                placeholder={t("newTask.modelSearchPlaceholder")}
                style={{
                  minWidth: 190,
                  width: "100%",
                  height: 28,
                  padding: "4px 8px",
                  borderRadius: 6,
                  border: "1px solid var(--border-medium)",
                  background: "var(--bg-input)",
                  color: "var(--text-primary)",
                  fontSize: 12,
                  fontFamily: "var(--font-mono)",
                  boxSizing: "border-box",
                  outline: "none",
                }}
                autoFocus
              />
            </div>
            {filteredModels.map((model) => (
              <button
                key={model}
                type="button"
                style={{
                  ...s.toolbarMenuItem,
                  width: "100%",
                  border: "none",
                  background: model === currentModel ? "var(--accent-subtle)" : "transparent",
                  textAlign: "left",
                }}
                onClick={() => {
                  onSetModel(model);
                  setOpenMenu(null);
                }}
                onFocus={(e) => setMenuItemHover(e.currentTarget, true)}
                onBlur={(e) => setMenuItemHover(e.currentTarget, model === currentModel)}
                onMouseEnter={(e) => setMenuItemHover(e.currentTarget, true)}
                onMouseLeave={(e) => setMenuItemHover(e.currentTarget, model === currentModel)}
              >
                {model}
              </button>
            ))}
            {models.length === 0 || filteredModels.length === 0 ? (
              <div
                style={{
                  padding: "8px 10px",
                  fontSize: 12,
                  color: "var(--text-hint)",
                  whiteSpace: "nowrap",
                }}
              >
                {loading
                  ? t("newTask.modelLoading")
                  : filteredModels.length === 0 && models.length > 0
                    ? t("newTask.modelNoResults")
                    : t("newTask.modelUnavailable")}
              </div>
            ) : null}
          </div>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
