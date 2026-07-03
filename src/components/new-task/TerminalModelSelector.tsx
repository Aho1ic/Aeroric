import { useCallback, useEffect, useMemo, useState } from "react";
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
  const [manualModel, setManualModel] = useState(selectedModel ?? "");
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
      if (selectedModel && !next.includes(selectedModel)) {
        setManualModel(selectedModel);
      } else if (next.length > 0 && !selectedModel) {
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
    setManualModel(selectedModel ?? "");
  }, [selectedModel]);

  const currentModel = useMemo(() => {
    if (selectedModel && models.includes(selectedModel)) return selectedModel;
    return selectedModel || models[0] || "";
  }, [models, selectedModel]);

  const label = loading
    ? t("newTask.modelLoading")
    : currentModel || (loadFailed ? t("newTask.modelUnavailable") : t("newTask.model"));

  const commitManualModel = () => {
    const next = manualModel.trim();
    if (!next) return;
    setModels((prev) => (prev.includes(next) ? prev : [next, ...prev]));
    onSetModel(next);
    setOpenMenu(null);
  };

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
          avoidCollisions={false}
          style={s.toolbarMenuContent}
        >
          <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
            {models.map((model) => (
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
            <div style={{ display: "flex", gap: 6, padding: "6px 8px 4px" }}>
              <input
                value={manualModel}
                onChange={(event) => setManualModel(event.currentTarget.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    commitManualModel();
                  }
                }}
                placeholder={t("newTask.modelManualPlaceholder")}
                style={{
                  minWidth: 180,
                  flex: "1 1 auto",
                  height: 28,
                  padding: "4px 8px",
                  borderRadius: 6,
                  border: "1px solid var(--border-medium)",
                  background: "var(--bg-input)",
                  color: "var(--text-primary)",
                  fontSize: 12,
                  fontFamily: "var(--font-mono)",
                }}
              />
              <button
                type="button"
                style={{ ...s.toolbarMenuItem, border: "none", background: "var(--bg-card)" }}
                onClick={commitManualModel}
              >
                {t("newTask.modelUseManual")}
              </button>
            </div>
            {models.length === 0 && (
              <div
                style={{
                  padding: "8px 10px",
                  fontSize: 12,
                  color: "var(--text-hint)",
                  whiteSpace: "nowrap",
                }}
              >
                {loading ? t("newTask.modelLoading") : t("newTask.modelUnavailable")}
              </div>
            )}
          </div>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
