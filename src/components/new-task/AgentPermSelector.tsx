import type React from "react";
import type { CSSProperties } from "react";
import { useMemo, useState } from "react";
import {
  BookmarkPlus,
  ChevronDown,
  Command,
  CornerDownLeft,
  Hand,
  ListChecks,
  Plus,
  Target,
  Zap,
} from "lucide-react";
import * as Popover from "@radix-ui/react-popover";
import * as Select from "@radix-ui/react-select";
import type { AgentType, PermissionMode } from "../../types";
import { agentDisplayLabel, agentFamily, isCodexLikeAgent, type AgentOption } from "../../agents";
import { useAgentOptions } from "../../hooks/useAgentOptions";
import { useI18n } from "../../i18n";
import s from "../../styles";
import claudeLogo from "../../assets/claude.svg";
import chatgptLogo from "../../assets/chatgpt.svg";
import deepseekLogo from "../../assets/deepseek.svg";
import type { DshAgentPreset } from "../../dshSettings";

const PERMS: PermissionMode[] = ["ask", "auto_edit", "full_access"];
export type ComposeMenu =
  | "more"
  | "agent"
  | "preset"
  | "permission"
  | "launch"
  | "branch"
  | "model"
  | "send"
  | null;

const DSH_PRESETS = [
  ["standard", "appSettings.dshPresetStandardName"],
  ["code", "appSettings.dshPresetCodeName"],
  ["minimal", "appSettings.dshPresetMinimalName"],
  ["cordis", "appSettings.dshPresetCordisName"],
] as const;

function agentIcon(agent: AgentType, options = [] as ReturnType<typeof useAgentOptions>): string {
  switch (agentFamily(agent, options)) {
    case "codex":
      return chatgptLogo;
    case "dsh":
      return deepseekLogo;
    default:
      return claudeLogo;
  }
}

function setMenuItemHover(el: HTMLElement, hover: boolean) {
  el.style.background = hover ? "var(--accent-subtle)" : "transparent";
  el.style.color = "var(--text-primary)";
  el.style.transform = hover ? "translateX(1px)" : "translateX(0)";
}

function SendShortcutIcon({ keys }: { keys: string[] }) {
  const modifierKey = keys.length > 1 ? keys[0] : null;

  return (
    <span style={s.sendShortcutIcon} aria-hidden="true">
      {modifierKey ? (
        modifierKey === "⌘" ? (
          <Command size={12} strokeWidth={2.2} />
        ) : (
          <span style={s.sendShortcutTextKey}>{modifierKey}</span>
        )
      ) : null}
      <CornerDownLeft size={13} strokeWidth={2.3} />
    </span>
  );
}

export function composePermissionLabel(mode: PermissionMode) {
  if (mode === "auto_edit") return "替我审批";
  if (mode === "full_access") return "完全访问";
  return "请求确认";
}

export function composeControlOrder(): string[] {
  return ["more", "agent", "preset", "permission", "launch", "branch", "model", "send"];
}

export function nextComposeMenuState(
  _current: ComposeMenu,
  target: Exclude<ComposeMenu, null>,
  open: boolean,
): ComposeMenu {
  return open ? target : null;
}

export function composeModelMenuContentStyle(): CSSProperties {
  return {
    ...s.toolbarMenuContent,
    minWidth: "var(--radix-select-trigger-width)",
    maxHeight: "min(280px, var(--radix-select-content-available-height))",
    overflow: "hidden",
  };
}

export function composeModelMenuViewportStyle(): CSSProperties {
  return {
    maxHeight: "min(280px, var(--radix-select-content-available-height))",
    overflowY: "auto",
    overscrollBehavior: "contain",
  };
}

export function composeAgentMenuContentStyle(): CSSProperties {
  return {
    ...s.toolbarMenuContent,
    width: "min(640px, calc(100vw - 16px))",
    minWidth: "min(520px, calc(100vw - 16px))",
    maxHeight: "min(320px, var(--radix-select-content-available-height))",
    overflow: "hidden",
  };
}

export function composeAgentMenuViewportStyle(): CSSProperties {
  return {
    display: "grid",
    gridTemplateColumns: "minmax(0, 1fr) 1px minmax(0, 1fr) 1px minmax(0, 1fr)",
    gap: 10,
    maxHeight: "min(320px, var(--radix-select-content-available-height))",
    overflow: "hidden",
    padding: 8,
  };
}

export function composeAgentMenuColumnStyle(): CSSProperties {
  return {
    display: "flex",
    flexDirection: "column",
    minWidth: 0,
    minHeight: 0,
    border: 0,
    borderRadius: 0,
    background: "transparent",
    overflow: "hidden",
  };
}

export function composeAgentMenuColumnViewportStyle(): CSSProperties {
  return {
    minHeight: 0,
    maxHeight: "min(250px, calc(var(--radix-select-content-available-height) - 58px))",
    overflowY: "auto",
    scrollbarWidth: "none",
    msOverflowStyle: "none",
    overscrollBehavior: "contain",
    padding: 4,
  };
}

export function groupAgentOptions(options: AgentOption[]): {
  claude: AgentOption[];
  codex: AgentOption[];
  dsh: AgentOption[];
} {
  return {
    claude: options.filter((option) => option.family === "claude"),
    codex: options.filter((option) => option.family === "codex"),
    dsh: options.filter((option) => option.family === "dsh"),
  };
}

function ModeMenuItem({
  enabled,
  icon,
  label,
  onToggle,
}: {
  enabled: boolean;
  icon: React.ReactNode;
  label: string;
  onToggle: () => void;
}) {
  return (
    <button
      role="switch"
      aria-checked={enabled}
      style={{
        ...s.toolbarMenuItem,
        width: "100%",
        border: "none",
        background: "none",
        justifyContent: "space-between",
      }}
      onClick={onToggle}
      onMouseEnter={(e) => setMenuItemHover(e.currentTarget, true)}
      onMouseLeave={(e) => setMenuItemHover(e.currentTarget, false)}
      onFocus={(e) => setMenuItemHover(e.currentTarget, true)}
      onBlur={(e) => setMenuItemHover(e.currentTarget, false)}
    >
      <span style={{ display: "inline-flex", alignItems: "center", gap: 8 }}>
        {icon}
        {label}
      </span>
      <span
        style={{
          ...s.toolbarSwitchTrack,
          background: enabled ? "var(--primary-action-bg)" : "var(--border-medium)",
        }}
      >
        <span
          style={{
            ...s.toolbarSwitchThumb,
            transform: enabled ? "translateX(16px)" : "translateX(0)",
          }}
        />
      </span>
    </button>
  );
}

export function AgentPermSelector({
  agent,
  permMode,
  planMode,
  goalMode,
  fastMode = false,
  isClaudeAgent = false,
  hasContent,
  hasAttachments,
  saveAsTodoDisabledReason,
  sendShortcutKeys,
  compact = false,
  launchControls,
  modelControls,
  openMenu: controlledOpenMenu,
  onOpenMenuChange,
  onSetAgent,
  onSetPermMode,
  onTogglePlanMode,
  onToggleGoalMode,
  onToggleFastMode,
  dshAgentPreset = "standard",
  dshCustomPresets = [],
  onSetDshAgentPreset,
  onSubmit,
}: {
  agent: AgentType;
  permMode: PermissionMode;
  planMode: boolean;
  goalMode: boolean;
  fastMode?: boolean;
  isClaudeAgent?: boolean;
  hasContent: boolean;
  hasAttachments: boolean;
  saveAsTodoDisabledReason?: string;
  sendShortcutKeys: string[];
  compact?: boolean;
  launchControls?: React.ReactNode;
  modelControls?: React.ReactNode;
  openMenu?: ComposeMenu;
  onOpenMenuChange?: (menu: ComposeMenu) => void;
  onSetAgent: (agent: AgentType) => void;
  onSetPermMode: (mode: PermissionMode) => void;
  onTogglePlanMode: () => void;
  onToggleGoalMode: () => void;
  onToggleFastMode?: () => void;
  dshAgentPreset?: string;
  dshCustomPresets?: DshAgentPreset[];
  onSetDshAgentPreset?: (preset: string) => void;
  onSubmit: (immediate: boolean) => void;
}) {
  const { t } = useI18n();
  const agentOptions = useAgentOptions();
  const [internalOpenMenu, setInternalOpenMenu] = useState<ComposeMenu>(null);
  const openMenu = controlledOpenMenu ?? internalOpenMenu;
  const setOpenMenu = (menu: ComposeMenu) => {
    if (onOpenMenuChange) {
      onOpenMenuChange(menu);
    } else {
      setInternalOpenMenu(menu);
    }
  };
  const sendShortcutLabel = sendShortcutKeys.join("");
  const sendLabel = hasContent
    ? t("newTask.send")
    : agentFamily(agent, agentOptions) === "dsh"
      ? t("newTask.startSession")
      : t("newTask.startTerminal");
  const controlButtonStyle = compact ? s.toolbarBtnIconOnly : s.toolbarBtn;
  const saveAsTodoDisabled = hasAttachments || !!saveAsTodoDisabledReason;
  const saveAsTodoTitle = hasAttachments ? t("newTask.imagesMustSend") : saveAsTodoDisabledReason;
  const groupedAgents = groupAgentOptions(agentOptions);
  const dshPresets = useMemo(() => {
    const builtInIds = new Set<string>(DSH_PRESETS.map(([value]) => value));
    return [
      ...DSH_PRESETS.map(([value, label]) => ({ value, label: t(label) })),
      ...dshCustomPresets
        .filter((preset) => preset.id.trim() && !builtInIds.has(preset.id))
        .map((preset) => ({ value: preset.id, label: preset.name?.trim() || preset.id })),
    ];
  }, [dshCustomPresets, t]);
  const currentDshPreset =
    dshPresets.find((preset) => preset.value === dshAgentPreset) ?? dshPresets[0];

  function renderAgentItem(item: AgentType) {
    return (
      <Select.Item
        key={item}
        value={item}
        style={{
          ...s.toolbarMenuItem,
          boxSizing: "border-box",
          width: "100%",
          minWidth: 0,
          whiteSpace: "normal",
        }}
        onFocus={(e) => setMenuItemHover(e.currentTarget, true)}
        onBlur={(e) => setMenuItemHover(e.currentTarget, false)}
        onMouseEnter={(e) => setMenuItemHover(e.currentTarget, true)}
        onMouseLeave={(e) => setMenuItemHover(e.currentTarget, false)}
      >
        <img
          src={agentIcon(item, agentOptions)}
          style={{
            ...s.toolbarMenuItemIcon,
            opacity: isCodexLikeAgent(item, agentOptions) ? 0.72 : 1,
          }}
        />
        <Select.ItemText>
          <span style={{ minWidth: 0, overflowWrap: "anywhere" }}>
            {agentDisplayLabel(item, agentOptions)}
          </span>
        </Select.ItemText>
      </Select.Item>
    );
  }

  function renderAgentColumn(
    family: "claude" | "codex" | "dsh",
    label: string,
    options: typeof agentOptions,
  ) {
    return (
      <div
        role="group"
        aria-label={label}
        data-agent-family={family}
        style={composeAgentMenuColumnStyle()}
      >
        <div className={`compose-agent-menu-title compose-agent-menu-title--${family}`}>
          <img
            src={agentIcon(options[0]?.value ?? family, agentOptions)}
            alt=""
            aria-hidden="true"
          />
          <span>{label}</span>
        </div>
        <div
          className="compose-agent-menu-column-viewport"
          style={composeAgentMenuColumnViewportStyle()}
        >
          {options.length > 0 ? (
            options.map(({ value: item }) => renderAgentItem(item))
          ) : (
            <div
              style={{
                padding: "12px 9px",
                color: "var(--text-hint)",
                fontSize: 11,
              }}
            >
              {t("newTask.noAgentConfigurations")}
            </div>
          )}
        </div>
      </div>
    );
  }

  return (
    <div style={s.toolbar}>
      <div style={s.toolbarLeft}>
        <Popover.Root
          open={openMenu === "more"}
          onOpenChange={(open) => setOpenMenu(nextComposeMenuState(openMenu, "more", open))}
        >
          <Popover.Trigger asChild>
            <button style={s.toolbarPlusBtn} aria-label={t("newTask.moreComposeActions")}>
              <Plus size={16} strokeWidth={1.9} />
            </button>
          </Popover.Trigger>
          <Popover.Portal>
            <Popover.Content
              side="top"
              align="start"
              sideOffset={8}
              style={s.toolbarActionMenuContent}
            >
              <ModeMenuItem
                enabled={planMode}
                icon={<ListChecks size={15} strokeWidth={2} color="var(--text-muted)" />}
                label={t("newTask.planMode")}
                onToggle={onTogglePlanMode}
              />
              <ModeMenuItem
                enabled={goalMode}
                icon={<Target size={15} strokeWidth={2} color="var(--text-muted)" />}
                label={t("newTask.goalMode")}
                onToggle={onToggleGoalMode}
              />
              {isClaudeAgent && onToggleFastMode && (
                <ModeMenuItem
                  enabled={fastMode}
                  icon={<Zap size={15} strokeWidth={2} color="var(--text-muted)" />}
                  label={t("newTask.fastMode")}
                  onToggle={onToggleFastMode}
                />
              )}
            </Popover.Content>
          </Popover.Portal>
        </Popover.Root>

        <Select.Root
          value={agent}
          open={openMenu === "agent"}
          onOpenChange={(open) => setOpenMenu(nextComposeMenuState(openMenu, "agent", open))}
          onValueChange={(v) => {
            onSetAgent(v as AgentType);
            setOpenMenu(null);
          }}
        >
          <Select.Trigger
            style={controlButtonStyle}
            aria-label={t("settings.agent")}
            title={agentDisplayLabel(agent, agentOptions)}
          >
            <img
              src={agentIcon(agent, agentOptions)}
              style={{
                ...s.toolbarMenuItemIcon,
                opacity: isCodexLikeAgent(agent, agentOptions) ? 0.72 : 1,
              }}
            />
            {!compact && <span>{agentDisplayLabel(agent, agentOptions)}</span>}
            {!compact && (
              <Select.Icon>
                <ChevronDown size={12} strokeWidth={2.5} style={{ opacity: 0.58 }} />
              </Select.Icon>
            )}
          </Select.Trigger>
          <Select.Portal>
            <Select.Content
              position="popper"
              side="bottom"
              align="start"
              sideOffset={6}
              avoidCollisions={false}
              style={composeAgentMenuContentStyle()}
            >
              <Select.Viewport style={composeAgentMenuViewportStyle()}>
                {renderAgentColumn("claude", t("newTask.claudeAgents"), groupedAgents.claude)}
                <div
                  className="compose-agent-menu-separator"
                  data-agent-menu-separator
                  aria-hidden="true"
                />
                {renderAgentColumn("codex", t("newTask.codexAgents"), groupedAgents.codex)}
                <div
                  className="compose-agent-menu-separator"
                  data-agent-menu-separator
                  aria-hidden="true"
                />
                {renderAgentColumn("dsh", t("newTask.dshAgents"), groupedAgents.dsh)}
              </Select.Viewport>
            </Select.Content>
          </Select.Portal>
        </Select.Root>

        {agentFamily(agent, agentOptions) === "dsh" && onSetDshAgentPreset && (
          <Select.Root
            value={dshAgentPreset}
            open={openMenu === "preset"}
            onOpenChange={(open) => setOpenMenu(nextComposeMenuState(openMenu, "preset", open))}
            onValueChange={(value) => {
              onSetDshAgentPreset(value);
              setOpenMenu(null);
            }}
          >
            <Select.Trigger
              style={{
                ...controlButtonStyle,
                ...(compact
                  ? null
                  : {
                      flex: "0 1 148px",
                      maxWidth: 148,
                    }),
              }}
              aria-label={t("newTask.dshAgentPreset")}
              title={currentDshPreset.label}
              data-dsh-agent-preset-trigger
            >
              <BookmarkPlus size={14} strokeWidth={2} color="var(--usage-dsh)" />
              {!compact && (
                <span
                  style={{
                    minWidth: 0,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {currentDshPreset.label}
                </span>
              )}
              {!compact && (
                <Select.Icon>
                  <ChevronDown size={12} strokeWidth={2.5} style={{ opacity: 0.58 }} />
                </Select.Icon>
              )}
            </Select.Trigger>
            <Select.Portal>
              <Select.Content position="popper" sideOffset={6} style={s.toolbarMenuContent}>
                <Select.Viewport>
                  {dshPresets.map(({ value, label }) => (
                    <Select.Item
                      key={value}
                      value={value}
                      style={s.toolbarMenuItem}
                      onFocus={(event) => setMenuItemHover(event.currentTarget, true)}
                      onBlur={(event) => setMenuItemHover(event.currentTarget, false)}
                      onMouseEnter={(event) => setMenuItemHover(event.currentTarget, true)}
                      onMouseLeave={(event) => setMenuItemHover(event.currentTarget, false)}
                    >
                      <BookmarkPlus size={13} strokeWidth={2} color="var(--usage-dsh)" />
                      <Select.ItemText>{label}</Select.ItemText>
                    </Select.Item>
                  ))}
                </Select.Viewport>
              </Select.Content>
            </Select.Portal>
          </Select.Root>
        )}

        <Select.Root
          value={permMode}
          open={openMenu === "permission"}
          onOpenChange={(open) => setOpenMenu(nextComposeMenuState(openMenu, "permission", open))}
          onValueChange={(v) => {
            onSetPermMode(v as PermissionMode);
            setOpenMenu(null);
          }}
        >
          <Select.Trigger
            style={controlButtonStyle}
            aria-label={t("settings.defaultPermissionMode")}
            title={composePermissionLabel(permMode)}
          >
            <Hand size={14} strokeWidth={2} color="var(--warning)" />
            {!compact && <span>{composePermissionLabel(permMode)}</span>}
            {!compact && (
              <Select.Icon>
                <ChevronDown size={12} strokeWidth={2.5} style={{ opacity: 0.58 }} />
              </Select.Icon>
            )}
          </Select.Trigger>
          <Select.Portal>
            <Select.Content position="popper" sideOffset={6} style={s.toolbarMenuContent}>
              <Select.Viewport>
                {PERMS.map((perm) => (
                  <Select.Item
                    key={perm}
                    value={perm}
                    style={s.toolbarMenuItem}
                    onFocus={(e) => setMenuItemHover(e.currentTarget, true)}
                    onBlur={(e) => setMenuItemHover(e.currentTarget, false)}
                    onMouseEnter={(e) => setMenuItemHover(e.currentTarget, true)}
                    onMouseLeave={(e) => setMenuItemHover(e.currentTarget, false)}
                  >
                    <Select.ItemText>{composePermissionLabel(perm)}</Select.ItemText>
                  </Select.Item>
                ))}
              </Select.Viewport>
            </Select.Content>
          </Select.Portal>
        </Select.Root>
        {launchControls}
      </div>

      <div style={s.toolbarSpacer} />

      {modelControls}

      <div style={s.sendSplit}>
        <button
          style={{
            ...s.sendBtn,
            ...(compact ? s.sendBtnIconOnly : {}),
            borderRadius: "8px 0 0 8px",
            borderRight: "1px solid rgba(255,255,255,0.18)",
          }}
          onClick={() => {
            onSubmit(true);
          }}
          aria-label={`${sendLabel} (${sendShortcutLabel})`}
          title={sendShortcutLabel}
        >
          {!compact && <span>{sendLabel}</span>}
          <SendShortcutIcon keys={sendShortcutKeys} />
        </button>
        <Popover.Root
          open={openMenu === "send"}
          onOpenChange={(open) => setOpenMenu(nextComposeMenuState(openMenu, "send", open))}
        >
          <Popover.Trigger asChild>
            <button
              style={{
                ...s.sendBtn,
                minWidth: 22,
                minHeight: 28,
                padding: "5px 5px",
                borderRadius: "0 8px 8px 0",
                borderLeft: "none",
              }}
            >
              <ChevronDown size={12} strokeWidth={2.5} />
            </button>
          </Popover.Trigger>
          <Popover.Portal>
            <Popover.Content side="bottom" align="end" sideOffset={6} style={s.toolbarMenuContent}>
              <Popover.Close asChild>
                <button
                  style={{
                    ...s.toolbarMenuItem,
                    gap: 8,
                    width: "100%",
                    border: "none",
                    background: "transparent",
                    cursor: saveAsTodoDisabled ? "not-allowed" : "pointer",
                    opacity: saveAsTodoDisabled ? 0.4 : 1,
                  }}
                  disabled={saveAsTodoDisabled}
                  title={saveAsTodoTitle}
                  onMouseEnter={(e) => {
                    if (!saveAsTodoDisabled) setMenuItemHover(e.currentTarget, true);
                  }}
                  onMouseLeave={(e) => setMenuItemHover(e.currentTarget, false)}
                  onFocus={(e) => {
                    if (!saveAsTodoDisabled) setMenuItemHover(e.currentTarget, true);
                  }}
                  onBlur={(e) => setMenuItemHover(e.currentTarget, false)}
                  onClick={() => {
                    if (saveAsTodoDisabled) return;
                    if (hasContent) onSubmit(false);
                  }}
                >
                  <BookmarkPlus size={13} strokeWidth={2} color="var(--text-muted)" />
                  {t("newTask.saveAsTodo")}
                </button>
              </Popover.Close>
            </Popover.Content>
          </Popover.Portal>
        </Popover.Root>
      </div>
    </div>
  );
}
