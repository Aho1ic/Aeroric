import { useState, useEffect, useMemo, useRef, type ReactNode } from "react";
import {
  ChevronDown,
  ChevronRight,
  Home,
  Moon,
  PanelLeftClose,
  PanelLeftOpen,
  PinOff,
  Play,
  Plus,
  Star,
  Sun,
  Trash2,
} from "lucide-react";
import type { Project, Task, ThemeVariant } from "../types";
import { ProjectAvatar } from "./ProjectAvatar";
import { StatusIcon } from "./StatusIcon";
import { NotificationBell } from "./NotificationBell";
import { useI18n } from "../i18n";
import { PROJECT_RAIL_EXPANDED_WIDTH } from "./project-page/viewMode";
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

export type ProjectRailFooterAction = "backHome" | "openProject" | "notifications" | "theme";

export function getProjectRailFooterActions(singleProjectMode: boolean): ProjectRailFooterAction[] {
  return singleProjectMode ? [] : ["backHome", "openProject", "notifications", "theme"];
}

export function projectTaskCountLabel(_count: number, _taskLabel: string): string | null {
  return null;
}

function RailTaskItem({
  task,
  selected,
  isNewTask,
  onSelect,
  onDelete,
  onToggleStar,
  onRunTodo,
}: {
  task: Task;
  selected: boolean;
  isNewTask: boolean;
  onSelect: () => void;
  onDelete: () => void;
  onToggleStar: () => void;
  onRunTodo: () => void;
}) {
  const { t } = useI18n();
  const [hovered, setHovered] = useState(false);
  const displayTitle = task.name ?? task.prompt;
  const canRunTodo = task.status === "todo";

  return (
    <button
      type="button"
      onClick={onSelect}
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
          selected && !isNewTask
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
  onToggleTaskStar,
  onRunTodo,
  onReorderProjects,
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
  onSelectTask: (id: string) => void;
  onDeleteTask: (id: string) => void;
  onToggleTaskStar: (id: string) => void;
  onRunTodo: (task: Task) => void;
  onReorderProjects?: (orderedProjectIds: string[]) => void;
  themeVariant: ThemeVariant;
  onToggleTheme: () => void;
  singleProjectMode?: boolean;
  forceCollapsed?: boolean;
}) {
  const { t } = useI18n();
  const [addHov, setAddHov] = useState(false);
  const [homeHov, setHomeHov] = useState(false);
  const [themeHov, setThemeHov] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
  const [draggedProjectId, setDraggedProjectId] = useState<string | null>(null);
  const [dragOverProjectId, setDragOverProjectId] = useState<string | null>(null);
  const projectItemRefs = useRef<Map<string, HTMLElement>>(new Map());
  const projectPointerDragRef = useRef<ProjectPointerDragState | null>(null);
  const suppressNextProjectClickRef = useRef(false);
  const isDark = themeVariant === "dark";
  const effectiveCollapsed = forceCollapsed || collapsed;
  const [expandedProjectIds, setExpandedProjectIds] = useState<Set<string>>(() =>
    getDefaultExpandedProjectIds(projects, activeProjectId),
  );

  const projectGroups = useMemo(
    () => buildProjectTaskGroups(projects, allTasks),
    [projects, allTasks],
  );

  const reorderProjectIds = (draggedId: string, targetId: string) => {
    if (draggedId === targetId) return;
    const ids = projectGroups.map((group) => group.project.id);
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
    event: React.PointerEvent<HTMLDivElement>,
    projectId: string,
  ) => {
    if (!onReorderProjects || event.button !== 0) return;
    const target = event.target;
    if (target instanceof Element && target.closest("[data-project-rail-no-drag]")) return;
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

  const handleProjectPointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const drag = projectPointerDragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    event.preventDefault();
    if (Math.abs(event.clientY - drag.startY) > POINTER_DRAG_MOVE_TOLERANCE) {
      drag.hasMoved = true;
    }
    setDragOverProjectId(projectIdAtClientY(event.clientY));
  };

  const handleProjectPointerUp = (event: React.PointerEvent<HTMLDivElement>) => {
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

  const handleProjectPointerCancel = (event: React.PointerEvent<HTMLDivElement>) => {
    const drag = projectPointerDragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    resetProjectPointerDrag();
  };

  useEffect(() => {
    setExpandedProjectIds((prev) => {
      if (prev.has(activeProjectId)) return prev;
      const next = new Set(prev);
      next.add(activeProjectId);
      return next;
    });
  }, [activeProjectId]);

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

  if (effectiveCollapsed) {
    return (
      <div
        style={{
          position: "relative",
          width: 52,
          flexShrink: 0,
          background: "var(--bg-sidebar)",
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

        {railProjects.map((project) => (
          <RailItem
            key={project.id}
            project={project}
            isActive={project.id === activeProjectId}
            status={getProjectStatus(allTasks, project.id)}
            attentionCount={getAttentionCount(allTasks, project.id)}
            showBadge={attentionBadge}
            waveNonce={waveNonces.get(project.id) ?? 0}
            onSwitch={onSwitch}
          />
        ))}

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
        width: PROJECT_RAIL_EXPANDED_WIDTH,
        flexShrink: 0,
        background: "var(--bg-sidebar)",
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

      <div style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: "8px 8px 10px" }}>
        {projectGroups.map(({ project, tasks }) => {
          const isActive = project.id === activeProjectId;
          const expanded = expandedProjectIds.has(project.id);
          const status = getProjectStatus(allTasks, project.id);
          const attentionCount = getAttentionCount(allTasks, project.id);
          const taskCountLabel = projectTaskCountLabel(tasks.length, t("task.tasks"));
          return (
            <div key={project.id} style={{ marginBottom: 6 }}>
              <div
                ref={setProjectItemRef(project.id)}
                onPointerDown={(event) => handleProjectPointerDown(event, project.id)}
                onPointerMove={handleProjectPointerMove}
                onPointerUp={handleProjectPointerUp}
                onPointerCancel={handleProjectPointerCancel}
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
                    dragOverProjectId === project.id ? "inset 0 0 0 1px var(--accent)" : "none",
                  cursor: onReorderProjects
                    ? draggedProjectId === project.id
                      ? "grabbing"
                      : "grab"
                    : "default",
                  touchAction: "none",
                  userSelect: "none",
                  transition:
                    "background 0.14s ease, opacity 0.14s ease, transform 0.16s ease, box-shadow 0.16s ease",
                }}
              >
                <button
                  type="button"
                  data-project-rail-no-drag
                  onClick={() =>
                    setExpandedProjectIds((prev) => {
                      const next = new Set(prev);
                      if (next.has(project.id)) next.delete(project.id);
                      else next.add(project.id);
                      return next;
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
                  aria-label={project.name}
                  onClick={(event) => {
                    if (suppressNextProjectClickRef.current) {
                      suppressNextProjectClickRef.current = false;
                      event.preventDefault();
                      return;
                    }
                    onSwitch(project);
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
                  <span style={{ position: "relative", flexShrink: 0 }}>
                    <ProjectAvatar name={project.name} size={25} />
                    <AttentionIndicator
                      status={status}
                      count={attentionCount}
                      showBadge={attentionBadge}
                      borderColor={isActive ? "var(--accent-subtle)" : "var(--bg-sidebar)"}
                    />
                  </span>
                  <span style={{ flex: 1, minWidth: 0 }}>
                    <span
                      style={{
                        display: "block",
                        fontSize: 12.8,
                        fontWeight: isActive ? 700 : 620,
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                      }}
                    >
                      {project.name}
                    </span>
                    {taskCountLabel && (
                      <span
                        style={{ display: "block", fontSize: 10.8, color: "var(--text-muted)" }}
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
                        isNewTask={isNewTask}
                        onSelect={() => {
                          if (project.id !== activeProjectId) onSwitch(project);
                          onSelectTask(task.id);
                        }}
                        onDelete={() => onDeleteTask(task.id)}
                        onToggleStar={() => onToggleTaskStar(task.id)}
                        onRunTodo={() => onRunTodo(task)}
                      />
                    ))
                  )}
                </div>
              )}
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
    </div>
  );
}
