import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ChevronDown,
  GitBranch,
  Laptop,
  GitPullRequestArrow,
  Check,
  Search,
  X,
  Globe,
} from "lucide-react";
import * as Select from "@radix-ui/react-select";
import * as Popover from "@radix-ui/react-popover";
import { useI18n } from "../../i18n";
import { agentFamily, type AgentOption } from "../../agents";
import type { ComposeMenu } from "./AgentPermSelector";
import type { AgentType } from "../../types";
import { MENU_ITEM_ICON_SIZE, nextComposeMenuState } from "./AgentPermSelector";
import s from "../../styles";

export type LaunchMode = "local" | "worktree" | "webui";

interface GitBranchInfo {
  name: string;
  current: boolean;
  remote: string | null;
}

const MODES: LaunchMode[] = ["local", "worktree", "webui"];

function setMenuItemHover(el: HTMLElement, hover: boolean) {
  el.style.background = hover ? "var(--accent-subtle)" : "transparent";
  el.style.color = "var(--text-primary)";
  el.style.transform = hover ? "translateX(1px)" : "translateX(0)";
}

export function LaunchModeSelector({
  projectPath,
  agent,
  agentOptions,
  launchMode,
  baseBranch,
  compact = false,
  openMenu: controlledOpenMenu,
  onOpenMenuChange,
  onSetLaunchMode,
  onSetBaseBranch,
}: {
  projectPath: string;
  agent: AgentType;
  agentOptions: AgentOption[];
  launchMode: LaunchMode;
  baseBranch: string;
  compact?: boolean;
  openMenu?: ComposeMenu;
  onOpenMenuChange?: (menu: ComposeMenu) => void;
  onSetLaunchMode: (mode: LaunchMode) => void;
  onSetBaseBranch: (branch: string) => void;
}) {
  const { t } = useI18n();
  const [branches, setBranches] = useState<GitBranchInfo[]>([]);
  const [internalOpenMenu, setInternalOpenMenu] = useState<ComposeMenu>(null);
  const [search, setSearch] = useState("");
  const openMenu = controlledOpenMenu ?? internalOpenMenu;
  const controlButtonStyle = compact ? s.toolbarBtnIconOnly : s.toolbarBtn;
  const setOpenMenu = (menu: ComposeMenu) => {
    if (onOpenMenuChange) {
      onOpenMenuChange(menu);
    } else {
      setInternalOpenMenu(menu);
    }
  };
  const modeOpen = openMenu === "launch";
  const pickerOpen = openMenu === "branch";

  const loadBranches = useCallback(
    async ({ applyDefault }: { applyDefault: boolean }) => {
      if (!projectPath) return;
      try {
        const list = await invoke<GitBranchInfo[]>("git_list_branches", { projectPath });
        setBranches(list);
        if (applyDefault && !baseBranch) {
          const current = list.find((b) => b.current);
          if (current) onSetBaseBranch(current.name);
        }
      } catch {
        setBranches([]);
      }
    },
    // baseBranch / onSetBaseBranch 只用于首次挂载默认值，避免后续刷新被它们触发
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [projectPath],
  );

  useEffect(() => {
    void loadBranches({ applyDefault: true });
  }, [loadBranches]);

  const localBranches = useMemo(() => {
    const q = search.trim().toLowerCase();
    return branches
      .filter((b) => b.remote === null)
      .filter((b) => !q || b.name.toLowerCase().includes(q));
  }, [branches, search]);

  const availableModes = useMemo(() => {
    const family = agentFamily(agent, agentOptions);
    if (family === "dsh") {
      return MODES;
    }
    return MODES.filter((m) => m !== "webui");
  }, [agent, agentOptions]);

  useEffect(() => {
    if (!availableModes.includes(launchMode)) {
      onSetLaunchMode("local");
    }
  }, [availableModes, launchMode, onSetLaunchMode]);

  function modeIcon(mode: LaunchMode, size: number) {
    if (mode === "local") {
      return <Laptop size={size} strokeWidth={2} color="var(--accent)" style={s.toolbarBtnIcon} />;
    }
    if (mode === "worktree") {
      return (
        <GitPullRequestArrow
          size={size}
          strokeWidth={2}
          color="var(--usage-codex)"
          style={s.toolbarBtnIcon}
        />
      );
    }
    return <Globe size={size} strokeWidth={2} color="var(--usage-dsh)" style={s.toolbarBtnIcon} />;
  }

  function modeLabel(mode: LaunchMode) {
    if (mode === "local") return t("newTask.launchMode.local");
    if (mode === "worktree") return t("newTask.launchMode.worktree");
    return t("newTask.launchMode.webui");
  }

  return (
    <>
      <Select.Root
        value={launchMode}
        open={modeOpen}
        onOpenChange={(open) => {
          setOpenMenu(nextComposeMenuState(openMenu, "launch", open));
        }}
        onValueChange={(v) => {
          onSetLaunchMode(v as LaunchMode);
          setOpenMenu(null);
        }}
      >
        <Select.Trigger
          style={{
            ...controlButtonStyle,
            ...(compact ? null : { flex: "0 1 auto", minWidth: 0, maxWidth: 128 }),
          }}
          aria-label={t("newTask.launchMode")}
          title={modeLabel(launchMode)}
          data-launch-mode-trigger
        >
          {modeIcon(launchMode, 14)}
          {!compact && <span style={s.toolbarBtnLabel}>{modeLabel(launchMode)}</span>}
          {!compact && (
            <Select.Icon style={s.toolbarBtnIcon}>
              <ChevronDown size={12} strokeWidth={2.5} style={{ opacity: 0.58 }} />
            </Select.Icon>
          )}
        </Select.Trigger>
        <Select.Portal>
          <Select.Content position="popper" sideOffset={6} style={s.toolbarMenuContentCompact}>
            <Select.Viewport>
              {availableModes.map((mode) => (
                <Select.Item
                  key={mode}
                  value={mode}
                  style={s.toolbarMenuItem}
                  onFocus={(e) => setMenuItemHover(e.currentTarget, true)}
                  onBlur={(e) => setMenuItemHover(e.currentTarget, false)}
                  onMouseEnter={(e) => setMenuItemHover(e.currentTarget, true)}
                  onMouseLeave={(e) => setMenuItemHover(e.currentTarget, false)}
                >
                  {modeIcon(mode, MENU_ITEM_ICON_SIZE)}
                  <Select.ItemText>{modeLabel(mode)}</Select.ItemText>
                </Select.Item>
              ))}
            </Select.Viewport>
          </Select.Content>
        </Select.Portal>
      </Select.Root>

      <Popover.Root
        open={pickerOpen}
        onOpenChange={(open) => {
          setOpenMenu(nextComposeMenuState(openMenu, "branch", open));
          if (!open) setSearch("");
        }}
      >
        <Popover.Trigger asChild>
          <button
            data-launch-base-branch-trigger
            style={{
              ...controlButtonStyle,
              ...(compact
                ? null
                : {
                    flex: "0 1 132px",
                    minWidth: 0,
                    maxWidth: 132,
                    overflow: "hidden",
                  }),
            }}
            aria-label={t("newTask.baseBranch")}
            title={baseBranch || t("newTask.selectBaseBranch")}
          >
            <GitBranch size={14} strokeWidth={2} color="var(--success)" style={s.toolbarBtnIcon} />
            {!compact && (
              <span style={s.toolbarBtnLabel}>{baseBranch || t("newTask.selectBaseBranch")}</span>
            )}
            {!compact && (
              <ChevronDown
                size={12}
                strokeWidth={2.5}
                style={{ ...s.toolbarBtnIcon, opacity: 0.58 }}
              />
            )}
          </button>
        </Popover.Trigger>
        <Popover.Portal>
          <Popover.Content className="branch-popover-content" sideOffset={6} align="start">
            <div className="branch-popover-search">
              <Search
                size={13}
                strokeWidth={2}
                color="var(--text-hint)"
                style={{ flexShrink: 0 }}
              />
              <input
                className="branch-popover-search-input"
                placeholder={t("branch.searchBranches")}
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                onKeyDown={(e) => e.stopPropagation()}
                autoFocus
              />
              {search && (
                <button className="branch-popover-clear" onClick={() => setSearch("")}>
                  <X size={11} />
                </button>
              )}
            </div>
            <div className="branch-popover-list">
              {localBranches.length === 0 ? (
                <div
                  style={{
                    padding: "12px 10px",
                    fontSize: 12,
                    color: "var(--text-hint)",
                    textAlign: "center",
                  }}
                >
                  {t("branch.noBranchesFound")}
                </div>
              ) : (
                localBranches.map((b) => (
                  <button
                    key={b.name}
                    className="branch-popover-item"
                    onClick={() => {
                      onSetBaseBranch(b.name);
                      setOpenMenu(null);
                    }}
                  >
                    <GitBranch
                      size={12}
                      strokeWidth={2}
                      color="var(--text-hint)"
                      style={{ flexShrink: 0 }}
                    />
                    <span className="branch-popover-item-name">{b.name}</span>
                    {baseBranch === b.name && (
                      <Check
                        size={12}
                        strokeWidth={2.5}
                        color="var(--accent)"
                        style={{ flexShrink: 0, marginLeft: "auto" }}
                      />
                    )}
                  </button>
                ))
              )}
            </div>
          </Popover.Content>
        </Popover.Portal>
      </Popover.Root>
    </>
  );
}
