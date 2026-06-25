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
} from "lucide-react";
import * as Select from "@radix-ui/react-select";
import * as Popover from "@radix-ui/react-popover";
import { useI18n } from "../../i18n";
import type { ComposeMenu } from "./AgentPermSelector";
import { nextComposeMenuState } from "./AgentPermSelector";
import s from "../../styles";

export type LaunchMode = "local" | "worktree";

interface GitBranchInfo {
  name: string;
  current: boolean;
  remote: string | null;
}

const MODES: LaunchMode[] = ["local", "worktree"];

function setMenuItemHover(el: HTMLElement, hover: boolean) {
  el.style.background = hover ? "var(--accent-subtle)" : "transparent";
}

export function LaunchModeSelector({
  projectPath,
  launchMode,
  baseBranch,
  compact = false,
  openMenu: controlledOpenMenu,
  onOpenMenuChange,
  onSetLaunchMode,
  onSetBaseBranch,
}: {
  projectPath: string;
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

  function modeIcon(mode: LaunchMode) {
    return mode === "local" ? (
      <Laptop size={13} strokeWidth={2} color="var(--accent)" />
    ) : (
      <GitPullRequestArrow size={13} strokeWidth={2} color="var(--usage-codex)" />
    );
  }

  function modeLabel(mode: LaunchMode) {
    return mode === "local" ? t("newTask.launchMode.local") : t("newTask.launchMode.worktree");
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
          style={controlButtonStyle}
          aria-label={t("newTask.launchMode")}
          title={modeLabel(launchMode)}
        >
          {modeIcon(launchMode)}
          {!compact && <span>{modeLabel(launchMode)}</span>}
          {!compact && (
            <Select.Icon>
              <ChevronDown size={12} strokeWidth={2.5} style={{ opacity: 0.58 }} />
            </Select.Icon>
          )}
        </Select.Trigger>
        <Select.Portal>
          <Select.Content position="popper" sideOffset={6} style={s.toolbarMenuContent}>
            <Select.Viewport>
              {MODES.map((mode) => (
                <Select.Item
                  key={mode}
                  value={mode}
                  style={s.toolbarMenuItem}
                  onFocus={(e) => setMenuItemHover(e.currentTarget, true)}
                  onBlur={(e) => setMenuItemHover(e.currentTarget, false)}
                  onMouseEnter={(e) => setMenuItemHover(e.currentTarget, true)}
                  onMouseLeave={(e) => setMenuItemHover(e.currentTarget, false)}
                >
                  {modeIcon(mode)}
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
            style={controlButtonStyle}
            aria-label={t("newTask.baseBranch")}
            title={baseBranch || t("newTask.selectBaseBranch")}
          >
            <GitBranch size={13} strokeWidth={2} color="var(--success)" />
            {!compact && <span>{baseBranch || t("newTask.selectBaseBranch")}</span>}
            {!compact && <ChevronDown size={12} strokeWidth={2.5} style={{ opacity: 0.58 }} />}
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
