import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent,
  type ReactNode,
  type UIEvent,
} from "react";
import {
  ChevronDown,
  ChevronRight,
  Bot,
  Home,
  Moon,
  PanelLeftClose,
  PanelLeftOpen,
  Pin,
  PinOff,
  Play,
  Plus,
  RotateCcw,
  Star,
  Sun,
  Trash2,
  X,
} from "lucide-react";
import type { Project, Task, ThemeVariant } from "../types";
import { isActiveTaskStatus, resolveProjectLocation } from "../types";
import { ProjectAvatar } from "./ProjectAvatar";
import { StatusIcon } from "./StatusIcon";
import { NotificationBell } from "./NotificationBell";
import { useI18n } from "../i18n";
import { OPEN_APP_SETTINGS_EVENT } from "./app-settings/types";
import {
  normalizeProjectRailWidth,
  PROJECT_RAIL_COLLAPSED_WIDTH,
  PROJECT_RAIL_EXPANDED_WIDTH,
} from "./project-page/viewMode";
import { groupProjectsForRail, UNGROUPED_PROJECT_GROUP } from "../projectGroups";
import { hasTaskContinuationContext } from "../taskSession";
import s from "../styles";
import claudeWaveGif from "../assets/gif/claude-wave.gif";

type ProjectStatus = "attention" | "running" | null;
type ProjectPointerDragState = {
  id: string;
  pointerId: number;
  startY: number;
  hasMoved: boolean;
};

const POINTER_DRAG_MOVE_TOLERANCE = 5;

// 每个已挂载的 ProjectPage 都渲染自己的 ProjectRail 实例(见 App.tsx 的 mountedProjects
// 映射),切换项目实际是换掉整条侧栏。若不共享滚动位置,新实例会从 scrollTop=0 起绘,
// 表现为“点一下项目就跳回列表顶部”。这里把滚动位置提到模块级并在实例间同步。
let sharedRailScrollTop = 0;
const railScrollSubscribers = new Set<(top: number) => void>();

// 把滚动容器接到共享位置:挂载时恢复,滚动时广播给其他实例。
function useSharedRailScroll() {
  const scrollRef = useRef<HTMLDivElement>(null);
  // 应用共享值时会触发一次 scroll 事件，需忽略以免自己覆盖自己。
  const applyingRef = useRef(false);

  const applyScrollTop = useCallback((top: number) => {
    const el = scrollRef.current;
    if (!el || el.scrollTop === top) return;
    applyingRef.current = true;
    el.scrollTop = top;
    applyingRef.current = false;
  }, []);

  useLayoutEffect(() => {
    applyScrollTop(sharedRailScrollTop);
    railScrollSubscribers.add(applyScrollTop);
    return () => {
      railScrollSubscribers.delete(applyScrollTop);
    };
  }, [applyScrollTop]);

  const handleScroll = useCallback(
    (event: UIEvent<HTMLDivElement>) => {
      if (applyingRef.current) return;
      sharedRailScrollTop = event.currentTarget.scrollTop;
      for (const subscriber of railScrollSubscribers) {
        if (subscriber !== applyScrollTop) subscriber(sharedRailScrollTop);
      }
    },
    [applyScrollTop],
  );

  return { scrollRef, handleScroll };
}

function normalizeProjectSearchText(value: string) {
  return value.normalize("NFKC").toLocaleLowerCase();
}

export function projectMatchesRailSearch(project: Project, query: string) {
  const normalizedQuery = normalizeProjectSearchText(query.trim());
  if (!normalizedQuery) return true;

  return [project.name, project.path].some((value) =>
    normalizeProjectSearchText(value).includes(normalizedQuery),
  );
}

function getProjectStatus(tasks: Task[], projectId: string): ProjectStatus {
  const projectTasks = tasks.filter((t) => t.projectId === projectId);
  if (
    projectTasks.some(
      (t) => t.status === "input_required" || t.status === "detached" || t.status === "interrupted",
    )
  ) {
    return "attention";
  }
  if (projectTasks.some((t) => t.status === "running" || t.status === "pending")) return "running";
  return null;
}

export interface ProjectTaskGroup {
  project: Project;
  tasks: Task[];
}

export function buildProjectTaskGroups(projects: Project[], tasks: Task[]): ProjectTaskGroup[] {
  return projects.map((project) => ({
    project,
    tasks: tasks
      .filter((task) => task.projectId === project.id)
      .sort((a, b) => b.createdAt - a.createdAt),
  }));
}

export function getDefaultExpandedProjectIds(
  projects: Project[],
  activeProjectId: string,
): Set<string> {
  return new Set(
    projects.some((project) => project.id === activeProjectId) ? [activeProjectId] : [],
  );
}

export function updateExpandedProjectIds(
  current: ReadonlySet<string>,
  projectId: string,
  expand: boolean,
  maxExpanded = 3,
): Set<string> {
  const next = new Set(current);
  if (!expand) {
    next.delete(projectId);
    return next;
  }
  if (next.has(projectId)) return next;
  next.add(projectId);
  while (next.size > maxExpanded) {
    const oldest = next.values().next().value;
    if (!oldest) break;
    next.delete(oldest);
  }
  return next;
}

export type ProjectRailFooterAction =
  | "backHome"
  | "agentSettings"
  | "openProject"
  | "notifications"
  | "theme";

export function getProjectRailFooterActions(singleProjectMode: boolean): ProjectRailFooterAction[] {
  return singleProjectMode
    ? []
    : ["backHome", "agentSettings", "openProject", "notifications", "theme"];
}

export function projectTaskCountLabel(_count: number, _taskLabel: string): string | null {
  return null;
}

function RailTaskItem({
  task,
  selected,
  multiSelected,
  isNewTask,
  onSelect,
  onDelete,
  onToggleStar,
  onRunTodo,
  onResumeTask,
  allowSessionRecovery,
}: {
  task: Task;
  selected: boolean;
  multiSelected: boolean;
  isNewTask: boolean;
  onSelect: (event: MouseEvent<HTMLButtonElement>) => void;
  onDelete: () => void;
  onToggleStar: () => void;
  onRunTodo: () => void;
  onResumeTask?: () => void;
  allowSessionRecovery?: boolean;
}) {
  const { t } = useI18n();
  const [hovered, setHovered] = useState(false);
  const displayTitle = task.name ?? task.prompt;
  const canRunTodo = task.status === "todo";
  const canResumeTask =
    Boolean(onResumeTask) &&
    task.status !== "todo" &&
    !isActiveTaskStatus(task.status) &&
    !task.worktreeDiscarded &&
    Boolean(
      task.codexSessionId ||
      task.codexSessionPath ||
      task.claudeSessionId ||
      task.claudeSessionPath ||
      (allowSessionRecovery && hasTaskContinuationContext(task)),
    );

  return (
    <button
      type="button"
      onClick={onSelect}
      aria-selected={multiSelected}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        width: "100%",
        minHeight: 23,
        display: "flex",
        alignItems: "center",
        gap: 5,
        padding: "2px 3px 2px 7px",
        border: "none",
        borderRadius: 5,
        background:
          multiSelected || (selected && !isNewTask)
            ? "var(--bg-selected)"
            : hovered
              ? "var(--bg-hover)"
              : "transparent",
        color: "var(--text-primary)",
        cursor: "pointer",
        textAlign: "left",
        fontFamily: "var(--font-ui)",
      }}
    >
      <StatusIcon status={task.status} />
      <span
        style={{
          flex: 1,
          minWidth: 0,
          fontSize: 11.2,
          fontWeight: 560,
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          lineHeight: "18px",
        }}
      >
        {displayTitle.slice(0, 72)}
        {displayTitle.length > 72 ? "..." : ""}
      </span>
      <span style={{ display: "inline-flex", alignItems: "center", gap: 1, flexShrink: 0 }}>
        <span
          role="button"
          tabIndex={0}
          aria-label={task.starred ? t("task.unstar") : t("task.star")}
          title={task.starred ? t("task.unstar") : t("task.star")}
          onClick={(event) => {
            event.stopPropagation();
            onToggleStar();
          }}
          onKeyDown={(event) => {
            if (event.key !== "Enter" && event.key !== " ") return;
            event.preventDefault();
            event.stopPropagation();
            onToggleStar();
          }}
          style={{
            width: 18,
            height: 18,
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            borderRadius: 5,
            color: task.starred ? "var(--star-fg)" : "var(--text-hint)",
            opacity: task.starred || hovered ? 1 : 0.45,
          }}
        >
          <Star size={10.5} strokeWidth={2.2} fill={task.starred ? "currentColor" : "none"} />
        </span>
        {canRunTodo && (
          <span
            role="button"
            tabIndex={0}
            aria-label={t("task.runNow")}
            title={t("task.runNow")}
            onClick={(event) => {
              event.stopPropagation();
              onRunTodo();
            }}
            onKeyDown={(event) => {
              if (event.key !== "Enter" && event.key !== " ") return;
              event.preventDefault();
              event.stopPropagation();
              onRunTodo();
            }}
            style={{
              width: 18,
              height: 18,
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              borderRadius: 5,
              color: "var(--accent)",
            }}
          >
            <Play size={10} strokeWidth={2} fill="currentColor" />
          </span>
        )}
        {canResumeTask && (
          <span
            role="button"
            tabIndex={0}
            aria-label={t("task.continue")}
            title={t("task.continue")}
            onClick={(event) => {
              event.stopPropagation();
              onResumeTask?.();
            }}
            onKeyDown={(event) => {
              if (event.key !== "Enter" && event.key !== " ") return;
              event.preventDefault();
              event.stopPropagation();
              onResumeTask?.();
            }}
            style={{
              width: 18,
              height: 18,
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              borderRadius: 5,
              color: "var(--accent)",
            }}
          >
            <RotateCcw size={10} strokeWidth={2.2} />
          </span>
        )}
        <span
          role="button"
          tabIndex={0}
          aria-label={t("task.deleteTask")}
          title={t("task.deleteTask")}
          onClick={(event) => {
            event.stopPropagation();
            onDelete();
          }}
          onKeyDown={(event) => {
            if (event.key !== "Enter" && event.key !== " ") return;
            event.preventDefault();
            event.stopPropagation();
            onDelete();
          }}
          style={{
            width: 18,
            height: 18,
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            borderRadius: 5,
            color: hovered ? "var(--danger)" : "var(--text-hint)",
          }}
        >
          <Trash2 size={10.5} strokeWidth={2.2} />
        </span>
      </span>
    </button>
  );
}

// 待确认(input_required)任务数——用于黄色数量角标
function getAttentionCount(tasks: Task[], projectId: string): number {
  return tasks.filter((t) => t.projectId === projectId && t.status === "input_required").length;
}

// 项目状态指示:启用角标且存在待确认任务时显示数量角标,否则回退为小圆点。
// borderColor 用于与所在容器背景描边融合(rail 与 drawer 背景不同)。
function AttentionIndicator({
  status,
  count,
  showBadge,
  borderColor,
}: {
  status: ProjectStatus;
  count: number;
  showBadge: boolean;
  borderColor: string;
}) {
  if (!status) return null;
  const isAttention = status === "attention";
  if (showBadge && isAttention && count > 0) {
    return (
      <span style={{ ...s.railAttentionBadge, borderColor }}>{count > 99 ? "99+" : count}</span>
    );
  }
  return (
    <span
      style={{
        ...s.railStatusDot,
        background: isAttention ? "var(--color-warning)" : "var(--color-success)",
        borderColor,
      }}
    />
  );
}

function RailItem({
  project,
  isActive,
  status,
  attentionCount,
  showBadge,
  waveNonce,
  onSwitch,
}: {
  project: Project;
  isActive: boolean;
  status: ProjectStatus;
  attentionCount: number;
  showBadge: boolean;
  waveNonce: number;
  onSwitch: (p: Project) => void;
}) {
  const [hov, setHov] = useState(false);
  const [waving, setWaving] = useState(false);

  // waveNonce 每次递增(出现新的待确认任务)就触发一次性招手,3.6s 后卸载。
  // 卸载+重新挂载可让 gif 从首帧重播,同时重启 CSS 探头/缩回动画。
  useEffect(() => {
    if (waveNonce <= 0) return;
    setWaving(true);
    const id = setTimeout(() => setWaving(false), 3600);
    return () => clearTimeout(id);
  }, [waveNonce]);

  return (
    <button
      title={project.name}
      onClick={() => onSwitch(project)}
      onMouseEnter={() => setHov(true)}
      onMouseLeave={() => setHov(false)}
      className={isActive ? "rail-active" : undefined}
      style={{
        position: "relative",
        width: 36,
        height: 36,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: "none",
        border: "none",
        borderRadius: 10,
        cursor: isActive ? "default" : "pointer",
        padding: 0,
        outline: isActive
          ? "2px solid var(--accent)"
          : hov
            ? "2px solid var(--border-medium)"
            : "2px solid transparent",
        outlineOffset: 1,
        transition: isActive ? "none" : "outline-color 0.12s",
      }}
    >
      {waving && (
        <img
          key={waveNonce}
          src={claudeWaveGif}
          alt=""
          className="rail-mascot-wave"
          style={s.railMascot}
        />
      )}
      <ProjectAvatar name={project.name} size={28} style={s.railAvatarStacked} />
      <AttentionIndicator
        status={status}
        count={attentionCount}
        showBadge={showBadge}
        borderColor="var(--bg-sidebar)"
      />
    </button>
  );
}

export function ProjectRail({
  projects,
  allTasks,
  activeProjectId,
  selectedTaskId,
  isNewTask,
  attentionBadge = true,
  onSwitch,
  onOpen,
  onBack,
  onNewTask,
  onSelectTask,
  onDeleteTask,
  onDeleteTasks,
  onToggleTaskStar,
  onRunTodo,
  onResumeTask,
  onReorderProjects,
  onToggleProjectPinned,
  projectGroups: projectGroupNames = [],
  collapsedProjectGroups: controlledCollapsedProjectGroups,
  onCollapsedProjectGroupsChange,
  projectRailWidth = PROJECT_RAIL_EXPANDED_WIDTH,
  onProjectRailWidthChange,
  themeVariant,
  onToggleTheme,
  singleProjectMode = false,
  forceCollapsed = false,
}: {
  projects: Project[];
  allTasks: Task[];
  activeProjectId: string;
  selectedTaskId: string | null;
  isNewTask: boolean;
  attentionBadge?: boolean;
  onSwitch: (project: Project) => void;
  onOpen: () => void;
  onBack: () => void;
  onNewTask: () => void;
  onSelectTask: (projectId: string, id: string) => void;
  onDeleteTask: (id: string) => void;
  onDeleteTasks?: (ids: string[]) => void;
  onToggleTaskStar: (id: string) => void;
  onRunTodo: (task: Task) => void;
  onResumeTask?: (taskId: string) => void;
  onReorderProjects?: (orderedProjectIds: string[]) => void;
  onToggleProjectPinned?: (projectId: string) => void;
  projectGroups?: string[];
  collapsedProjectGroups?: ReadonlySet<string>;
  onCollapsedProjectGroupsChange?: (groups: Set<string>) => void;
  projectRailWidth?: number;
  onProjectRailWidthChange?: (width: number) => void;
  themeVariant: ThemeVariant;
  onToggleTheme: () => void;
  singleProjectMode?: boolean;
  forceCollapsed?: boolean;
}) {
  const { t } = useI18n();
  const [addHov, setAddHov] = useState(false);
  const [homeHov, setHomeHov] = useState(false);
  const [agentSettingsHov, setAgentSettingsHov] = useState(false);
  const [themeHov, setThemeHov] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
  // 未置顶项目的空心 Pin 只在悬停/当前项目上露出,避免整列图标噪音。
  const [hoveredProjectId, setHoveredProjectId] = useState<string | null>(null);
  const [draggedProjectId, setDraggedProjectId] = useState<string | null>(null);
  const [dragOverProjectId, setDragOverProjectId] = useState<string | null>(null);
  const [resizing, setResizing] = useState(false);
  const projectItemRefs = useRef<Map<string, HTMLElement>>(new Map());
  const projectPointerDragRef = useRef<ProjectPointerDragState | null>(null);
  const railResizeRef = useRef<{
    pointerId: number;
    startX: number;
    startWidth: number;
  } | null>(null);
  const suppressNextProjectClickRef = useRef(false);
  const { scrollRef: projectListScrollRef, handleScroll: handleProjectListScroll } =
    useSharedRailScroll();
  const isDark = themeVariant === "dark";
  const effectiveCollapsed = forceCollapsed || collapsed;
  const [expandedProjectIds, setExpandedProjectIds] = useState<Set<string>>(() =>
    getDefaultExpandedProjectIds(projects, activeProjectId),
  );
  const [localCollapsedProjectGroups, setLocalCollapsedProjectGroups] = useState<Set<string>>(
    new Set(),
  );
  const [selectedTaskIds, setSelectedTaskIds] = useState<Set<string>>(new Set());
  const taskSelectionAnchorRef = useRef<{ projectId: string; taskId: string } | null>(null);
  const usesControlledProjectGroupCollapse =
    controlledCollapsedProjectGroups !== undefined && onCollapsedProjectGroupsChange !== undefined;
  const collapsedProjectGroups = usesControlledProjectGroupCollapse
    ? controlledCollapsedProjectGroups
    : localCollapsedProjectGroups;
  const updateCollapsedProjectGroups = useCallback(
    (updater: (current: ReadonlySet<string>) => Set<string>) => {
      if (usesControlledProjectGroupCollapse) {
        onCollapsedProjectGroupsChange(updater(controlledCollapsedProjectGroups));
        return;
      }
      setLocalCollapsedProjectGroups((current) => updater(current));
    },
    [
      controlledCollapsedProjectGroups,
      onCollapsedProjectGroupsChange,
      usesControlledProjectGroupCollapse,
    ],
  );

  const projectTaskGroups = useMemo(
    () => buildProjectTaskGroups(projects, allTasks),
    [projects, allTasks],
  );
  const railProjectGroups = useMemo(() => {
    const taskGroups = new Map(projectTaskGroups.map((group) => [group.project.id, group.tasks]));
    const grouped = groupProjectsForRail(projects, projectGroupNames);
    return grouped.map((group) => ({
      ...group,
      projects: group.projects.map((project) => ({
        project,
        tasks: taskGroups.get(project.id) ?? [],
      })),
    }));
  }, [projectGroupNames, projectTaskGroups, projects]);

  useEffect(() => {
    const availableTaskIds = new Set(allTasks.map((task) => task.id));
    setSelectedTaskIds(
      (current) => new Set([...current].filter((taskId) => availableTaskIds.has(taskId))),
    );
    const anchor = taskSelectionAnchorRef.current;
    if (anchor && !availableTaskIds.has(anchor.taskId)) {
      taskSelectionAnchorRef.current = null;
    }
  }, [allTasks]);

  const reorderProjectIds = (draggedId: string, targetId: string) => {
    if (draggedId === targetId) return;
    const draggedGroup =
      projects.find((project) => project.id === draggedId)?.group?.trim() ||
      UNGROUPED_PROJECT_GROUP;
    const targetGroup =
      projects.find((project) => project.id === targetId)?.group?.trim() || UNGROUPED_PROJECT_GROUP;
    if (draggedGroup !== targetGroup) return;
    const ids = railProjectGroups.flatMap((group) =>
      group.projects.map(({ project }) => project.id),
    );
    const from = ids.indexOf(draggedId);
    const to = ids.indexOf(targetId);
    if (from < 0 || to < 0) return;
    const next = ids.filter((id) => id !== draggedId);
    const targetIndex = next.indexOf(targetId);
    next.splice(from < to ? targetIndex + 1 : targetIndex, 0, draggedId);
    onReorderProjects?.(next);
  };

  const setProjectItemRef = (projectId: string) => (element: HTMLDivElement | null) => {
    if (element) {
      projectItemRefs.current.set(projectId, element);
    } else {
      projectItemRefs.current.delete(projectId);
    }
  };

  const projectIdAtClientY = (clientY: number) => {
    let fallback: string | null = null;
    let fallbackDistance = Number.POSITIVE_INFINITY;
    for (const [projectId, element] of projectItemRefs.current) {
      const rect = element.getBoundingClientRect();
      if (clientY >= rect.top && clientY <= rect.bottom) return projectId;
      const center = rect.top + rect.height / 2;
      const distance = Math.abs(clientY - center);
      if (distance < fallbackDistance) {
        fallback = projectId;
        fallbackDistance = distance;
      }
    }
    return fallback;
  };

  const resetProjectPointerDrag = () => {
    projectPointerDragRef.current = null;
    setDraggedProjectId(null);
    setDragOverProjectId(null);
  };

  const handleProjectPointerDown = (
    event: React.PointerEvent<HTMLButtonElement>,
    projectId: string,
  ) => {
    if (!onReorderProjects || event.button !== 0) return;
    const currentTarget = event.currentTarget;
    projectPointerDragRef.current = {
      id: projectId,
      pointerId: event.pointerId,
      startY: event.clientY,
      hasMoved: false,
    };
    setDraggedProjectId(projectId);
    setDragOverProjectId(projectId);
    currentTarget.setPointerCapture?.(event.pointerId);
  };

  const handleProjectPointerMove = (event: React.PointerEvent<HTMLButtonElement>) => {
    const drag = projectPointerDragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    event.preventDefault();
    if (Math.abs(event.clientY - drag.startY) > POINTER_DRAG_MOVE_TOLERANCE) {
      drag.hasMoved = true;
    }
    setDragOverProjectId(projectIdAtClientY(event.clientY));
  };

  const handleProjectPointerUp = (event: React.PointerEvent<HTMLButtonElement>) => {
    const drag = projectPointerDragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const targetId = drag.hasMoved ? projectIdAtClientY(event.clientY) : null;
    event.currentTarget.releasePointerCapture?.(event.pointerId);
    resetProjectPointerDrag();
    if (!targetId) return;
    suppressNextProjectClickRef.current = true;
    event.preventDefault();
    reorderProjectIds(drag.id, targetId);
  };

  const handleProjectPointerCancel = (event: React.PointerEvent<HTMLButtonElement>) => {
    const drag = projectPointerDragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    resetProjectPointerDrag();
  };

  const handleRailResizePointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (!onProjectRailWidthChange || event.button !== 0) return;
    railResizeRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startWidth: normalizeProjectRailWidth(projectRailWidth),
    };
    setResizing(true);
    event.currentTarget.setPointerCapture?.(event.pointerId);
    event.preventDefault();
  };

  const handleRailResizePointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const resize = railResizeRef.current;
    if (!resize || resize.pointerId !== event.pointerId) return;
    onProjectRailWidthChange?.(
      normalizeProjectRailWidth(resize.startWidth + event.clientX - resize.startX),
    );
    event.preventDefault();
  };

  const finishRailResize = (event: React.PointerEvent<HTMLDivElement>) => {
    const resize = railResizeRef.current;
    if (!resize || resize.pointerId !== event.pointerId) return;
    event.currentTarget.releasePointerCapture?.(event.pointerId);
    railResizeRef.current = null;
    setResizing(false);
  };

  useEffect(() => {
    setExpandedProjectIds((prev) => {
      if (prev.has(activeProjectId)) return prev;
      return updateExpandedProjectIds(prev, activeProjectId, true);
    });
  }, [activeProjectId]);

  const showProjectGroupHeaders = railProjectGroups.some((group) => !group.isUngrouped);

  // 折叠窄条只显示常驻项目；当前激活项目即使被设为非常驻也始终保留，避免失去当前上下文。
  const railProjects = useMemo(
    () => projects.filter((p) => !p.hiddenFromRail || p.id === activeProjectId),
    [projects, activeProjectId],
  );

  // 招手触发:记录每个项目上一次的待确认数量,数量增加(0→≥1 或 n→n+1)时给该项目
  // 递增一个 nonce,RailItem 据此播一次招手动画。首帧只做初始化播种,不为已有任务招手。
  const prevAttentionRef = useRef<Map<string, number>>(new Map());
  const seededRef = useRef(false);
  const [waveNonces, setWaveNonces] = useState<Map<string, number>>(new Map());

  useEffect(() => {
    const triggered: string[] = [];
    for (const p of railProjects) {
      const count = getAttentionCount(allTasks, p.id);
      const prev = prevAttentionRef.current.get(p.id) ?? 0;
      if (seededRef.current && count > prev) triggered.push(p.id);
      prevAttentionRef.current.set(p.id, count);
    }
    seededRef.current = true;
    if (triggered.length === 0) return;
    setWaveNonces((prev) => {
      const next = new Map(prev);
      for (const id of triggered) next.set(id, (next.get(id) ?? 0) + 1);
      return next;
    });
  }, [allTasks, railProjects]);

  const footerIconButton = (
    title: string,
    icon: ReactNode,
    onClick: () => void,
    hovered: boolean,
    setHovered: (value: boolean) => void,
    active = false,
  ) => (
    <button
      type="button"
      title={title}
      aria-label={title}
      onClick={onClick}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        width: 32,
        height: 32,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: active
          ? "var(--accent-subtle)"
          : hovered
            ? "var(--bg-hover)"
            : "var(--bg-card)",
        border: "1px solid var(--border-dim)",
        borderRadius: 8,
        cursor: "pointer",
        color: active ? "var(--accent)" : hovered ? "var(--text-primary)" : "var(--text-muted)",
        transition: "background 0.12s, color 0.12s, border-color 0.12s",
      }}
    >
      {icon}
    </button>
  );

  const openAgentSettings = () => {
    window.dispatchEvent(
      new CustomEvent(OPEN_APP_SETTINGS_EVENT, { detail: { initialNav: "__all_agent_configs__" } }),
    );
  };

  const handleProjectClick = (project: Project) => {
    setSelectedTaskIds(new Set());
    taskSelectionAnchorRef.current = null;
    // 点击项目总是展开其历史对话列表。activeProjectId 的 effect 只覆盖“切换到新项目”，
    // 重复点击当前项目(已折叠)不会触发，所以这里显式展开。
    setExpandedProjectIds((prev) => updateExpandedProjectIds(prev, project.id, true));
    onSwitch(project);
  };

  const handleTaskClick = (
    event: MouseEvent<HTMLButtonElement>,
    project: Project,
    tasks: Task[],
    task: Task,
  ) => {
    const anchor = taskSelectionAnchorRef.current;
    const additive = event.metaKey || event.ctrlKey;

    if (event.shiftKey && anchor?.projectId === project.id) {
      const anchorIndex = tasks.findIndex((item) => item.id === anchor.taskId);
      const targetIndex = tasks.findIndex((item) => item.id === task.id);
      if (anchorIndex >= 0 && targetIndex >= 0) {
        const start = Math.min(anchorIndex, targetIndex);
        const end = Math.max(anchorIndex, targetIndex);
        const rangeIds = tasks.slice(start, end + 1).map((item) => item.id);
        const projectTaskIds = new Set(tasks.map((item) => item.id));
        setSelectedTaskIds(
          (current) =>
            new Set(
              additive
                ? [...current].filter((taskId) => projectTaskIds.has(taskId)).concat(rangeIds)
                : rangeIds,
            ),
        );
        return;
      }
    }

    if (additive) {
      setSelectedTaskIds((current) => {
        const projectTaskIds = new Set(tasks.map((item) => item.id));
        const next = new Set([...current].filter((taskId) => projectTaskIds.has(taskId)));
        if (next.has(task.id)) next.delete(task.id);
        else next.add(task.id);
        return next;
      });
      taskSelectionAnchorRef.current = { projectId: project.id, taskId: task.id };
      return;
    }

    setSelectedTaskIds(new Set());
    taskSelectionAnchorRef.current = { projectId: project.id, taskId: task.id };
    if (project.id !== activeProjectId) onSwitch(project);
    onSelectTask(project.id, task.id);
  };

  if (effectiveCollapsed) {
    return (
      <div
        style={{
          position: "relative",
          width: PROJECT_RAIL_COLLAPSED_WIDTH,
          flexShrink: 0,
          background: "var(--bg-sidebar)",
          backdropFilter: "var(--glass-blur)",
          WebkitBackdropFilter: "var(--glass-blur)",
          borderRight: "1px solid var(--border-dim)",
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          paddingTop: 10,
          paddingBottom: 10,
          gap: 5,
          overflow: "visible",
        }}
      >
        <button
          type="button"
          title={t("task.showTasks")}
          aria-label={t("task.showTasks")}
          onClick={() => setCollapsed(false)}
          style={{
            width: 32,
            height: 32,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            border: "1px solid var(--border-dim)",
            borderRadius: 8,
            background: "var(--bg-card)",
            color: "var(--text-muted)",
            cursor: "pointer",
          }}
        >
          <PanelLeftOpen size={15} strokeWidth={2} />
        </button>

        {railProjects.map((project) => {
          return (
            <RailItem
              key={project.id}
              project={project}
              isActive={project.id === activeProjectId}
              status={getProjectStatus(allTasks, project.id)}
              attentionCount={getAttentionCount(allTasks, project.id)}
              showBadge={attentionBadge}
              waveNonce={waveNonces.get(project.id) ?? 0}
              onSwitch={() => handleProjectClick(project)}
            />
          );
        })}

        <div style={{ flex: 1 }} />

        {!singleProjectMode &&
          footerIconButton(
            t("project.backHome"),
            <Home size={14} strokeWidth={2.2} />,
            onBack,
            homeHov,
            setHomeHov,
          )}

        {!singleProjectMode &&
          footerIconButton(
            t("appSettings.agentSettings"),
            <Bot size={14} strokeWidth={2.1} />,
            openAgentSettings,
            agentSettingsHov,
            setAgentSettingsHov,
          )}

        {!singleProjectMode &&
          footerIconButton(
            t("welcome.openProject"),
            <Plus size={14} strokeWidth={2.5} />,
            onOpen,
            addHov,
            setAddHov,
          )}

        {!singleProjectMode && (
          <NotificationBell
            buttonStyle={{
              width: 32,
              height: 32,
              justifyContent: "center",
              border: "1px solid var(--border-dim)",
              background: "var(--bg-card)",
              opacity: 1,
            }}
            iconSize={14}
          />
        )}

        {!singleProjectMode &&
          footerIconButton(
            isDark ? t("theme.switchToLight") : t("theme.switchToDark"),
            isDark ? <Sun size={14} strokeWidth={2} /> : <Moon size={14} strokeWidth={2} />,
            onToggleTheme,
            themeHov,
            setThemeHov,
          )}
      </div>
    );
  }

  return (
    <div
      style={{
        position: "relative",
        width: normalizeProjectRailWidth(projectRailWidth),
        flexShrink: 0,
        background: "var(--bg-sidebar)",
        backdropFilter: "var(--glass-blur)",
        WebkitBackdropFilter: "var(--glass-blur)",
        borderRight: "1px solid var(--border-dim)",
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
      }}
    >
      <div
        style={{
          height: 48,
          flexShrink: 0,
          display: "flex",
          alignItems: "center",
          gap: 8,
          padding: "0 10px 0 12px",
          borderBottom: "1px solid var(--border-dim)",
        }}
      >
        <span
          style={{
            flex: 1,
            minWidth: 0,
            fontSize: 13,
            fontWeight: 720,
            color: "var(--text-primary)",
          }}
        >
          {t("welcome.projects")}
        </span>
        <button
          type="button"
          title={t("task.hideTasks")}
          aria-label={t("task.hideTasks")}
          onClick={() => setCollapsed(true)}
          style={{
            width: 28,
            height: 28,
            border: "none",
            borderRadius: 6,
            background: "transparent",
            color: "var(--text-hint)",
            cursor: "pointer",
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          <PanelLeftClose size={15} strokeWidth={2} />
        </button>
      </div>

      <div
        ref={projectListScrollRef}
        onScroll={handleProjectListScroll}
        style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: "8px 8px 10px" }}
      >
        {railProjectGroups.map((railGroup) => {
          const groupKey = railGroup.isUngrouped ? UNGROUPED_PROJECT_GROUP : railGroup.name;
          const groupCollapsed = collapsedProjectGroups.has(groupKey);
          // 折叠时置顶项目仍然露出;分组头的计数保持整组数量,不受折叠影响。
          const visibleProjects = groupCollapsed
            ? railGroup.projects.filter((entry) => entry.project.pinned)
            : railGroup.projects;
          return (
            <div key={groupKey} style={{ marginBottom: showProjectGroupHeaders ? 8 : 0 }}>
              {showProjectGroupHeaders && (
                <button
                  type="button"
                  aria-label={
                    groupCollapsed
                      ? t("projectRail.expandGroup", {
                          name: railGroup.isUngrouped
                            ? t("projectGroups.ungrouped")
                            : railGroup.name,
                        })
                      : t("projectRail.collapseGroup", {
                          name: railGroup.isUngrouped
                            ? t("projectGroups.ungrouped")
                            : railGroup.name,
                        })
                  }
                  onClick={() =>
                    updateCollapsedProjectGroups((current) => {
                      const next = new Set(current);
                      if (next.has(groupKey)) next.delete(groupKey);
                      else next.add(groupKey);
                      return next;
                    })
                  }
                  style={{
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    width: "100%",
                    height: 28,
                    margin: "2px 6px 4px",
                    padding: "0 8px",
                    boxSizing: "border-box",
                    border: "none",
                    borderBottom: "1px solid color-mix(in srgb, #16a34a 45%, var(--border-dim))",
                    background: "transparent",
                    color: "var(--text-muted)",
                    fontFamily: "var(--font-ui)",
                    fontSize: 10.5,
                    fontWeight: 700,
                    cursor: "pointer",
                    textAlign: "left",
                  }}
                >
                  <span
                    style={{
                      display: "inline-flex",
                      alignItems: "center",
                      gap: 5,
                      minWidth: 0,
                    }}
                  >
                    {groupCollapsed ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
                    <span
                      style={{
                        minWidth: 0,
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                      }}
                    >
                      {railGroup.isUngrouped ? t("projectGroups.ungrouped") : railGroup.name}
                    </span>
                  </span>
                  <span style={{ flexShrink: 0, marginLeft: 8 }}>{railGroup.projects.length}</span>
                </button>
              )}

              {visibleProjects.map(({ project, tasks }) => {
                const isActive = project.id === activeProjectId;
                const expanded = expandedProjectIds.has(project.id);
                const status = getProjectStatus(allTasks, project.id);
                const attentionCount = getAttentionCount(allTasks, project.id);
                const taskCountLabel = projectTaskCountLabel(tasks.length, t("task.tasks"));
                const selectedProjectTasks = tasks.filter((task) => selectedTaskIds.has(task.id));
                const deletableSelectedTaskIds = selectedProjectTasks
                  .filter((task) => !task.starred)
                  .map((task) => task.id);
                return (
                  <div key={project.id} style={{ marginBottom: 6 }}>
                    <div
                      ref={setProjectItemRef(project.id)}
                      data-project-rail-row
                      onMouseEnter={() => setHoveredProjectId(project.id)}
                      onMouseLeave={() =>
                        setHoveredProjectId((current) => (current === project.id ? null : current))
                      }
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: 8,
                        minHeight: 38,
                        padding: "4px 4px",
                        borderRadius: 8,
                        background:
                          dragOverProjectId === project.id
                            ? "var(--bg-hover)"
                            : isActive
                              ? "var(--accent-subtle)"
                              : "transparent",
                        opacity: draggedProjectId === project.id ? 0.55 : 1,
                        transform:
                          draggedProjectId === project.id
                            ? "scale(0.985)"
                            : dragOverProjectId === project.id
                              ? "translateY(2px)"
                              : "none",
                        boxShadow:
                          dragOverProjectId === project.id
                            ? "inset 0 0 0 1px var(--accent)"
                            : "none",
                        cursor: "default",
                        transition:
                          "background 0.14s ease, opacity 0.14s ease, transform 0.16s ease, box-shadow 0.16s ease",
                      }}
                    >
                      <button
                        type="button"
                        data-project-rail-no-drag
                        onClick={() =>
                          setExpandedProjectIds((prev) => {
                            return updateExpandedProjectIds(
                              prev,
                              project.id,
                              !prev.has(project.id),
                            );
                          })
                        }
                        title={expanded ? t("task.hideTasks") : t("task.showTasks")}
                        aria-label={expanded ? t("task.hideTasks") : t("task.showTasks")}
                        style={{
                          width: 22,
                          height: 22,
                          border: "none",
                          background: "transparent",
                          color: "var(--text-hint)",
                          cursor: "pointer",
                          display: "inline-flex",
                          alignItems: "center",
                          justifyContent: "center",
                        }}
                      >
                        {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                      </button>
                      <button
                        type="button"
                        aria-label={t("projectRail.dragProject", { name: project.name })}
                        title={t("projectRail.dragProject", { name: project.name })}
                        onPointerDown={(event) => handleProjectPointerDown(event, project.id)}
                        onPointerMove={handleProjectPointerMove}
                        onPointerUp={handleProjectPointerUp}
                        onPointerCancel={handleProjectPointerCancel}
                        style={{
                          width: 29,
                          height: 29,
                          position: "relative",
                          flexShrink: 0,
                          display: "inline-flex",
                          alignItems: "center",
                          justifyContent: "center",
                          border: "none",
                          borderRadius: 7,
                          background: "transparent",
                          color: "var(--text-primary)",
                          cursor: draggedProjectId === project.id ? "grabbing" : "grab",
                          padding: 0,
                          touchAction: "none",
                          userSelect: "none",
                        }}
                      >
                        <ProjectAvatar name={project.name} size={25} />
                        <AttentionIndicator
                          status={status}
                          count={attentionCount}
                          showBadge={attentionBadge}
                          borderColor={isActive ? "var(--accent-subtle)" : "var(--bg-sidebar)"}
                        />
                      </button>
                      <button
                        type="button"
                        aria-label={project.name}
                        onClick={(event) => {
                          if (suppressNextProjectClickRef.current) {
                            suppressNextProjectClickRef.current = false;
                            event.preventDefault();
                            return;
                          }
                          handleProjectClick(project);
                        }}
                        style={{
                          flex: 1,
                          minWidth: 0,
                          display: "flex",
                          alignItems: "center",
                          gap: 8,
                          border: "none",
                          background: "transparent",
                          color: isActive ? "var(--accent)" : "var(--text-primary)",
                          cursor: draggedProjectId === project.id ? "grabbing" : "pointer",
                          textAlign: "left",
                          fontFamily: "var(--font-ui)",
                        }}
                      >
                        <span style={{ flex: 1, minWidth: 0 }}>
                          <span
                            style={{
                              display: "block",
                              fontSize: 12.8,
                              fontWeight: isActive ? 700 : 620,
                              overflow: "hidden",
                              textOverflow: "clip",
                              whiteSpace: "nowrap",
                            }}
                          >
                            {project.name}
                          </span>
                          {taskCountLabel && (
                            <span
                              style={{
                                display: "block",
                                fontSize: 10.8,
                                color: "var(--text-muted)",
                              }}
                            >
                              {taskCountLabel}
                            </span>
                          )}
                        </span>
                        {project.hiddenFromRail && (
                          <PinOff
                            size={12}
                            strokeWidth={2}
                            color="var(--text-hint)"
                            style={s.railHiddenIcon}
                          />
                        )}
                      </button>
                      {/* 已置顶常驻显示;未置顶只在悬停或当前项目上露出,避免整列图标噪音。 */}
                      {(project.pinned || hoveredProjectId === project.id || isActive) && (
                        <button
                          type="button"
                          data-project-rail-no-drag
                          title={project.pinned ? t("projectRail.unpin") : t("projectRail.pin")}
                          aria-label={
                            project.pinned ? t("projectRail.unpin") : t("projectRail.pin")
                          }
                          aria-pressed={project.pinned ?? false}
                          onClick={(event) => {
                            event.stopPropagation();
                            onToggleProjectPinned?.(project.id);
                          }}
                          style={{
                            width: 26,
                            height: 26,
                            border: "1px solid var(--border-dim)",
                            borderRadius: 6,
                            background: project.pinned
                              ? "var(--control-active-bg)"
                              : "var(--bg-card)",
                            color: project.pinned
                              ? "var(--control-active-fg)"
                              : "var(--text-muted)",
                            cursor: "pointer",
                            display: "inline-flex",
                            alignItems: "center",
                            justifyContent: "center",
                            flexShrink: 0,
                          }}
                        >
                          <Pin size={13} strokeWidth={project.pinned ? 2.6 : 2} />
                        </button>
                      )}
                      {isActive && (
                        <button
                          type="button"
                          data-project-rail-no-drag
                          title={t("task.newTask")}
                          aria-label={t("task.newTask")}
                          onClick={onNewTask}
                          style={{
                            width: 26,
                            height: 26,
                            border: "1px solid var(--border-dim)",
                            borderRadius: 6,
                            background: isNewTask ? "var(--control-active-bg)" : "var(--bg-card)",
                            color: isNewTask ? "var(--control-active-fg)" : "var(--text-muted)",
                            cursor: "pointer",
                            display: "inline-flex",
                            alignItems: "center",
                            justifyContent: "center",
                            flexShrink: 0,
                          }}
                        >
                          <Plus size={14} strokeWidth={2.4} />
                        </button>
                      )}
                    </div>

                    {expanded && (
                      <div
                        style={{
                          marginLeft: 22,
                          paddingTop: 3,
                          display: "flex",
                          flexDirection: "column",
                          gap: 1,
                        }}
                      >
                        {selectedProjectTasks.length > 0 && (
                          <div
                            role="toolbar"
                            aria-label={t("task.selectedActions")}
                            style={{
                              minHeight: 30,
                              display: "flex",
                              alignItems: "center",
                              gap: 4,
                              margin: "2px 0 3px",
                              padding: "3px 4px 3px 8px",
                              border: "1px solid var(--border-medium)",
                              borderRadius: 6,
                              background: "var(--bg-card)",
                            }}
                          >
                            <span
                              style={{
                                flex: 1,
                                minWidth: 0,
                                fontSize: 10.8,
                                fontWeight: 650,
                                color: "var(--text-secondary)",
                              }}
                            >
                              {t("task.selectedCount", { count: selectedProjectTasks.length })}
                            </span>
                            <button
                              type="button"
                              aria-label={t("task.deleteSelected")}
                              title={t("task.deleteSelected")}
                              disabled={deletableSelectedTaskIds.length === 0 || !onDeleteTasks}
                              onClick={() => onDeleteTasks?.(deletableSelectedTaskIds)}
                              style={{
                                minWidth: 24,
                                height: 24,
                                display: "inline-flex",
                                alignItems: "center",
                                justifyContent: "center",
                                gap: 4,
                                padding: "0 6px",
                                border: "none",
                                borderRadius: 5,
                                background: "transparent",
                                color:
                                  deletableSelectedTaskIds.length > 0
                                    ? "var(--danger)"
                                    : "var(--text-hint)",
                                cursor:
                                  deletableSelectedTaskIds.length > 0 && onDeleteTasks
                                    ? "pointer"
                                    : "default",
                                opacity:
                                  deletableSelectedTaskIds.length > 0 && onDeleteTasks ? 1 : 0.45,
                                fontFamily: "var(--font-ui)",
                                fontSize: 10.8,
                              }}
                            >
                              <Trash2 size={11} strokeWidth={2.2} />
                              <span>{t("common.delete")}</span>
                            </button>
                            <button
                              type="button"
                              aria-label={t("task.clearSelection")}
                              title={t("task.clearSelection")}
                              onClick={() => {
                                setSelectedTaskIds(new Set());
                                taskSelectionAnchorRef.current = null;
                              }}
                              style={{
                                width: 24,
                                height: 24,
                                display: "inline-flex",
                                alignItems: "center",
                                justifyContent: "center",
                                border: "none",
                                borderRadius: 5,
                                background: "transparent",
                                color: "var(--text-muted)",
                                cursor: "pointer",
                              }}
                            >
                              <X size={11} strokeWidth={2.2} />
                            </button>
                          </div>
                        )}
                        {tasks.length === 0 ? (
                          <div
                            style={{
                              padding: "10px 8px 12px",
                              fontSize: 11.5,
                              color: "var(--text-hint)",
                            }}
                          >
                            {t("task.noTasksYet")}
                          </div>
                        ) : (
                          tasks.map((task) => (
                            <RailTaskItem
                              key={task.id}
                              task={task}
                              selected={selectedTaskId === task.id}
                              multiSelected={selectedTaskIds.has(task.id)}
                              isNewTask={isNewTask}
                              onSelect={(event) => handleTaskClick(event, project, tasks, task)}
                              onDelete={() => onDeleteTask(task.id)}
                              onToggleStar={() => onToggleTaskStar(task.id)}
                              onRunTodo={() => onRunTodo(task)}
                              allowSessionRecovery={
                                resolveProjectLocation(project).kind === "local"
                              }
                              onResumeTask={onResumeTask ? () => onResumeTask(task.id) : undefined}
                            />
                          ))
                        )}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          );
        })}
      </div>

      <div
        style={{
          flexShrink: 0,
          display: "flex",
          flexDirection: "row",
          alignItems: "center",
          justifyContent: "center",
          gap: 6,
          padding: "8px 10px 10px",
          borderTop: "1px solid var(--border-dim)",
        }}
      >
        {getProjectRailFooterActions(singleProjectMode).includes("backHome") ? (
          <>
            {footerIconButton(
              t("project.backHome"),
              <Home size={14} strokeWidth={2.2} />,
              onBack,
              homeHov,
              setHomeHov,
            )}

            {footerIconButton(
              t("appSettings.agentSettings"),
              <Bot size={14} strokeWidth={2.1} />,
              openAgentSettings,
              agentSettingsHov,
              setAgentSettingsHov,
            )}

            {footerIconButton(
              t("welcome.openProject"),
              <Plus size={14} strokeWidth={2.5} />,
              onOpen,
              addHov,
              setAddHov,
            )}

            <NotificationBell
              buttonStyle={{
                width: 32,
                height: 32,
                justifyContent: "center",
                border: "1px solid var(--border-dim)",
                background: "var(--bg-card)",
                opacity: 1,
              }}
              iconSize={14}
            />

            {footerIconButton(
              isDark ? t("theme.switchToLight") : t("theme.switchToDark"),
              isDark ? <Sun size={14} strokeWidth={2} /> : <Moon size={14} strokeWidth={2} />,
              onToggleTheme,
              themeHov,
              setThemeHov,
            )}
          </>
        ) : null}
      </div>

      {onProjectRailWidthChange && (
        <div
          role="separator"
          aria-label={t("projectRail.resize")}
          aria-orientation="vertical"
          onPointerDown={handleRailResizePointerDown}
          onPointerMove={handleRailResizePointerMove}
          onPointerUp={finishRailResize}
          onPointerCancel={finishRailResize}
          style={{
            position: "absolute",
            top: 0,
            right: 0,
            bottom: 0,
            width: 6,
            cursor: "col-resize",
            touchAction: "none",
            zIndex: 10,
            background: resizing ? "var(--accent)" : "transparent",
            opacity: resizing ? 0.7 : 0,
            transition: "opacity 0.12s ease",
          }}
          onMouseEnter={(event) => {
            if (!resizing) event.currentTarget.style.opacity = "0.45";
          }}
          onMouseLeave={(event) => {
            if (!resizing) event.currentTarget.style.opacity = "0";
          }}
        />
      )}
    </div>
  );
}
