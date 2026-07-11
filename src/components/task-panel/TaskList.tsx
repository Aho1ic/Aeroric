import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent,
  type UIEvent,
} from "react";
import { Play, Star, Trash2, X } from "lucide-react";
import type { Task, TaskDisplayWindow } from "../../types";
import { isActiveTaskStatus } from "../../types";
import { TaskListItem } from "./TaskListItem";
import { useI18n } from "../../i18n";
import s from "../../styles";

const GROUP_ROW_HEIGHT = 27;
const TASK_ROW_HEIGHT = 47;
const OVERSCAN_ROWS = 8;

type VirtualRow =
  | { type: "group"; key: string; label: string; height: number }
  | {
      type: "task";
      key: string;
      task: Task;
      showRunTodo: boolean;
      showResumeTask: boolean;
      height: number;
    };

function findRowIndex(offsets: number[], value: number) {
  if (offsets.length <= 1) return 0;

  let low = 0;
  let high = offsets.length - 2;

  while (low < high) {
    const mid = Math.floor((low + high) / 2);
    if (offsets[mid + 1] < value) {
      low = mid + 1;
    } else {
      high = mid;
    }
  }

  return low;
}

export function TaskList({
  tasks,
  taskDisplayWindow,
  query,
  selectedId,
  isNewTask,
  onSelectTask,
  onDeleteTask,
  onToggleTaskStar,
  onRunTodo,
  onResumeTask,
}: {
  tasks: Task[];
  taskDisplayWindow: TaskDisplayWindow;
  query: string;
  selectedId: string | null;
  isNewTask: boolean;
  onSelectTask: (id: string) => void;
  onDeleteTask: (id: string) => void;
  onToggleTaskStar: (id: string) => void;
  onRunTodo: (task: Task) => void;
  onResumeTask?: (taskId: string) => void;
}) {
  const { t } = useI18n();
  const scrollRef = useRef<HTMLDivElement>(null);
  const [viewportHeight, setViewportHeight] = useState(0);
  const [scrollTop, setScrollTop] = useState(0);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const selectionAnchorRef = useRef<string | null>(null);

  useEffect(() => {
    const available = new Set(tasks.map((task) => task.id));
    setSelectedIds((current) => new Set([...current].filter((id) => available.has(id))));
  }, [tasks]);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;

    const updateViewportHeight = () => setViewportHeight(el.clientHeight);
    updateViewportHeight();

    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", updateViewportHeight);
      return () => window.removeEventListener("resize", updateViewportHeight);
    }

    const resizeObserver = new ResizeObserver(updateViewportHeight);
    resizeObserver.observe(el);
    return () => resizeObserver.disconnect();
  }, []);

  const handleScroll = useCallback((event: UIEvent<HTMLDivElement>) => {
    setScrollTop(event.currentTarget.scrollTop);
  }, []);

  const filtered = useMemo(() => {
    if (!query.trim()) return tasks;
    const q = query.toLowerCase();
    return tasks.filter((t) => t.prompt.toLowerCase().includes(q));
  }, [tasks, query]);

  const sorted = useMemo(() => {
    return [...filtered].sort((a, b) => {
      if (a.starred !== b.starred) return a.starred ? -1 : 1;
      const aNeedsAttention =
        a.status === "input_required" || a.status === "detached" || a.status === "interrupted";
      const bNeedsAttention =
        b.status === "input_required" || b.status === "detached" || b.status === "interrupted";
      if (aNeedsAttention && !bNeedsAttention) return -1;
      if (!aNeedsAttention && bNeedsAttention) return 1;
      if (aNeedsAttention && bNeedsAttention) {
        return (b.attentionRequestedAt ?? b.createdAt) - (a.attentionRequestedAt ?? a.createdAt);
      }
      return b.createdAt - a.createdAt;
    });
  }, [filtered]);

  const { todayTs, cutoffTs } = useMemo(() => {
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    const todayTs = d.getTime();
    const cutoffTs =
      taskDisplayWindow === "all"
        ? Number.NEGATIVE_INFINITY
        : todayTs - taskDisplayWindow * 24 * 60 * 60 * 1000;
    return { todayTs, cutoffTs };
  }, [taskDisplayWindow]);

  const rows = useMemo<VirtualRow[]>(() => {
    const attentionTasks: Task[] = [];
    const pendingMergeTasks: Task[] = [];
    const starredTasks: Task[] = [];
    const todoTasks: Task[] = [];
    const todayTasks: Task[] = [];
    const earlierTasks: Task[] = [];

    for (const task of sorted) {
      if (
        task.status === "input_required" ||
        task.status === "detached" ||
        task.status === "interrupted"
      ) {
        attentionTasks.push(task);
      } else if (task.status === "done" && !!task.worktreePath && !task.worktreeDiscarded) {
        pendingMergeTasks.push(task);
      } else if (task.starred) {
        starredTasks.push(task);
      } else if (task.status === "todo") {
        todoTasks.push(task);
      } else if (task.createdAt >= todayTs) {
        todayTasks.push(task);
      } else if (task.createdAt >= cutoffTs) {
        earlierTasks.push(task);
      }
    }

    const nextRows: VirtualRow[] = [];
    const canResumeTask = (task: Task) => {
      if (!onResumeTask || task.status === "todo" || isActiveTaskStatus(task.status)) return false;
      if (task.worktreeDiscarded) return false;
      return Boolean(
        task.codexSessionId ||
        task.codexSessionPath ||
        task.claudeSessionId ||
        task.claudeSessionPath,
      );
    };
    const appendGroup = (key: string, label: string, groupTasks: Task[], showRunTodo = false) => {
      if (groupTasks.length === 0) return;
      nextRows.push({ type: "group", key, label, height: GROUP_ROW_HEIGHT });
      groupTasks.forEach((task) => {
        nextRows.push({
          type: "task",
          key: task.id,
          task,
          showRunTodo: showRunTodo || task.status === "todo",
          showResumeTask: canResumeTask(task),
          height: TASK_ROW_HEIGHT,
        });
      });
    };

    appendGroup("attention", t("task.needsAttention"), attentionTasks);
    appendGroup("pending_merge", t("task.pendingMerge"), pendingMergeTasks);
    appendGroup("starred", t("task.starred"), starredTasks);
    appendGroup("todo", t("status.todo"), todoTasks, true);
    appendGroup("today", t("task.today"), todayTasks);
    appendGroup("earlier", t("task.earlier"), earlierTasks);

    return nextRows;
  }, [cutoffTs, onResumeTask, sorted, t, todayTs]);

  const selectableTasks = useMemo(
    () =>
      rows
        .filter((row): row is Extract<VirtualRow, { type: "task" }> => row.type === "task")
        .map((row) => row.task),
    [rows],
  );

  const handleTaskClick = useCallback(
    (task: Task, event: MouseEvent) => {
      const additive = event.metaKey || event.ctrlKey;
      if (event.shiftKey && selectionAnchorRef.current) {
        const anchorIndex = selectableTasks.findIndex(
          (item) => item.id === selectionAnchorRef.current,
        );
        const targetIndex = selectableTasks.findIndex((item) => item.id === task.id);
        if (anchorIndex >= 0 && targetIndex >= 0) {
          const [start, end] =
            anchorIndex < targetIndex ? [anchorIndex, targetIndex] : [targetIndex, anchorIndex];
          const range = selectableTasks.slice(start, end + 1).map((item) => item.id);
          setSelectedIds((current) => new Set(additive ? [...current, ...range] : range));
          return;
        }
      }
      if (additive) {
        setSelectedIds((current) => {
          const next = new Set(current);
          if (next.has(task.id)) next.delete(task.id);
          else next.add(task.id);
          return next;
        });
        selectionAnchorRef.current = task.id;
        return;
      }
      setSelectedIds(new Set());
      selectionAnchorRef.current = task.id;
      onSelectTask(task.id);
    },
    [onSelectTask, selectableTasks],
  );

  const selectedTasks = selectableTasks.filter((task) => selectedIds.has(task.id));

  const offsets = useMemo(() => {
    const nextOffsets = [0];
    for (const row of rows) {
      nextOffsets.push(nextOffsets[nextOffsets.length - 1] + row.height);
    }
    return nextOffsets;
  }, [rows]);

  const totalHeight = offsets[offsets.length - 1] ?? 0;
  const startIndex = Math.max(0, findRowIndex(offsets, scrollTop) - OVERSCAN_ROWS);
  const endIndex = Math.min(
    rows.length,
    findRowIndex(offsets, scrollTop + viewportHeight) + OVERSCAN_ROWS + 1,
  );
  const visibleRows = rows.slice(startIndex, endIndex);

  return (
    <div ref={scrollRef} style={s.taskListScroll} onScroll={handleScroll}>
      {selectedTasks.length > 0 && (
        <div className="task-multi-actions">
          <strong>{selectedTasks.length}</strong>
          <button
            title={t("task.star")}
            onClick={() =>
              selectedTasks
                .filter((task) => !task.starred)
                .forEach((task) => onToggleTaskStar(task.id))
            }
          >
            <Star size={13} />
          </button>
          <button
            title={t("task.continue")}
            onClick={() => selectedTasks.forEach((task) => onResumeTask?.(task.id))}
          >
            <Play size={13} />
          </button>
          <button
            disabled={selectedTasks.every((task) => task.starred)}
            title={t("task.deleteTask")}
            onClick={() =>
              selectedTasks.filter((task) => !task.starred).forEach((task) => onDeleteTask(task.id))
            }
          >
            <Trash2 size={13} />
          </button>
          <button title="Clear selection" onClick={() => setSelectedIds(new Set())}>
            <X size={13} />
          </button>
        </div>
      )}
      {tasks.length === 0 && <div style={s.taskListEmpty}>{t("task.noTasksYet")}</div>}
      <div style={{ height: totalHeight, position: "relative" }}>
        {visibleRows.map((row, visibleIndex) => {
          const rowIndex = startIndex + visibleIndex;
          const top = offsets[rowIndex] ?? 0;

          return (
            <div
              key={row.key}
              style={{
                position: "absolute",
                top,
                left: 0,
                right: 0,
                height: row.height,
                overflow: "hidden",
              }}
            >
              {row.type === "group" ? (
                <div style={s.groupLabel}>{row.label}</div>
              ) : (
                <TaskListItem
                  task={row.task}
                  selected={
                    (selectedId === row.task.id && !isNewTask) || selectedIds.has(row.task.id)
                  }
                  multiSelected={selectedIds.has(row.task.id)}
                  onClick={(event) => handleTaskClick(row.task, event)}
                  onDelete={() => onDeleteTask(row.task.id)}
                  onToggleStar={() => onToggleTaskStar(row.task.id)}
                  onRunTodo={row.showRunTodo ? () => onRunTodo(row.task) : undefined}
                  onResumeTask={
                    row.showResumeTask && onResumeTask ? () => onResumeTask(row.task.id) : undefined
                  }
                />
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
