/* 任务收集箱。全库的 `- [ ]` 折成一张按时间 / 优先级 / 笔记分组的清单,点一条跳到
 * 源码那一行。
 *
 * 从 Markio 的 `TaskInbox.tsx` 移植,换了三处骨架:
 *
 * - Markio 那份是侧栏里的一个常驻分区,数据来自 `fs_grep`(每个仓库最多 200 条)。
 *   这里是铺满面板的 sheet + 一次全库扫描,理由和字段浏览器一样:侧栏那 190px 已经被
 *   大纲 / 反链 / 标签三档占满,而这一档要同时显示分组、任务、来源三层。
 * - Markio 只收未完成的(grep 正则写死了 `\[\s+\]`),所以它说的"12 条"里没有已完成的
 *   那些,而用户看不出这个清单是筛过的。这里两种都收,默认只显示未完成,给一个开关 ——
 *   计数因此是诚实的。
 * - Markio 的分组第三档是「项目」(= 工作区)。Aeroric 的随手记一个 vault 就是一个库,
 *   同一级别的粒度是**笔记**,所以第三档是「按笔记」。
 *
 * **只读。** 这里不提供勾选:行号是 `tasks.rs` 那个坐标系(按整个 `.md` 文件数),而
 * 勾选写回要走 `noteTasks.ts` 的坐标系(按正文数)。混用会勾错行,见 `tasks.rs` 的
 * 模块注释。
 */

import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { CheckSquare, RefreshCw, X } from "lucide-react";

import {
  countOpenTasks,
  filterInboxTasks,
  groupInboxTasks,
  todayIso,
  type InboxTask,
  type TaskGroupMode,
} from "./noteTaskInbox";

export type NoteTaskInboxSheetProps = {
  tasks: InboxTask[];
  loading: boolean;
  error: string | null;
  /** 跳到某篇的某一行。行号按整个 `.md` 文件数。 */
  onJump: (path: string, line: number) => void;
  onRefresh: () => void;
  onClose: () => void;
  /** 右键菜单。`anchor` 是屏幕坐标。 */
  onContextMenu: (task: InboxTask, anchor: { x: number; y: number }) => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

const overlayStyle: CSSProperties = {
  position: "absolute",
  inset: 0,
  zIndex: 30,
  display: "flex",
  flexDirection: "column",
  background: "var(--bg-panel)",
};

const headerStyle: CSSProperties = {
  minHeight: 32,
  display: "flex",
  alignItems: "center",
  gap: 6,
  padding: "0 8px",
  borderBottom: "1px solid var(--border-dim)",
  color: "var(--text-muted)",
  fontSize: 11.5,
};

const iconButtonStyle: CSSProperties = {
  display: "flex",
  padding: 3,
  border: "none",
  borderRadius: 4,
  background: "transparent",
  color: "var(--text-hint)",
  cursor: "pointer",
};

const hintStyle: CSSProperties = {
  padding: 10,
  color: "var(--text-hint)",
  fontSize: 11.5,
};

const filterStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  height: 22,
  padding: "0 6px",
  border: "1px solid var(--border)",
  borderRadius: 4,
  background: "var(--bg-input, transparent)",
  color: "var(--text-primary)",
  fontSize: 11.5,
};

const PRIORITY_COLOR: Record<string, string> = {
  high: "var(--danger, #dc2626)",
  med: "var(--warning, #eab308)",
  low: "var(--success, #22c55e)",
  _: "var(--text-hint)",
};

const GROUP_MODES: TaskGroupMode[] = ["time", "priority", "note"];

/** 分组标题。「按笔记」用标题本身,另两档查 i18n。 */
function groupLabel(
  group: { kind: TaskGroupMode; key: string; title: string },
  t: NoteTaskInboxSheetProps["t"],
): string {
  if (group.kind === "note") return group.title;
  if (group.kind === "time") return t(`notebook.taskBucket.${group.key}`);
  return t(`notebook.taskPriority.${group.key === "_" ? "none" : group.key}`);
}

export function NoteTaskInboxSheet({
  tasks,
  loading,
  error,
  onJump,
  onRefresh,
  onClose,
  onContextMenu,
  t,
}: NoteTaskInboxSheetProps) {
  const closeRef = useRef<HTMLButtonElement | null>(null);
  const [query, setQuery] = useState("");
  const [mode, setMode] = useState<TaskGroupMode>("time");
  const [showDone, setShowDone] = useState(false);

  // 打开时把焦点挪进来,理由同字段浏览器:不挪的话下面那个 onKeyDown 收不到 Esc。
  useEffect(() => {
    closeRef.current?.focus();
  }, []);

  /* 今天算一次,分组和徽标共用。每条任务各算一次的话,跨过午夜的那一瞬间清单里会
     同时存在两个"今天"。 */
  const today = useMemo(() => todayIso(), []);
  const shown = useMemo(() => filterInboxTasks(tasks, query, showDone), [tasks, query, showDone]);
  const groups = useMemo(() => groupInboxTasks(shown, mode, today), [shown, mode, today]);
  const openCount = countOpenTasks(tasks);

  return (
    <div
      style={overlayStyle}
      role="dialog"
      aria-modal="true"
      aria-label={t("notebook.taskInboxTitle")}
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        // 不往上冒,和字段浏览器 / 图谱一致。
        event.preventDefault();
        event.stopPropagation();
        onClose();
      }}
    >
      <div style={headerStyle}>
        <CheckSquare size={12} aria-hidden />
        <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}>
          {t("notebook.taskInboxTitle")}
        </span>
        <span style={{ color: "var(--text-hint)" }}>
          {t("notebook.taskInboxCount", { count: String(openCount) })}
        </span>
        <button
          type="button"
          aria-label={t("notebook.taskInboxRefresh")}
          title={t("notebook.taskInboxRefresh")}
          onClick={onRefresh}
          disabled={loading}
          style={{
            ...iconButtonStyle,
            marginLeft: "auto",
            cursor: loading ? "progress" : "pointer",
            opacity: loading ? 0.45 : 1,
          }}
        >
          <RefreshCw size={12} aria-hidden />
        </button>
        <button
          ref={closeRef}
          type="button"
          aria-label={t("notebook.taskInboxClose")}
          onClick={onClose}
          style={iconButtonStyle}
        >
          <X size={13} aria-hidden />
        </button>
      </div>

      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 6,
          padding: 5,
          borderBottom: "1px solid var(--border-dim)",
        }}
      >
        <input
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder={t("notebook.taskInboxFilter")}
          aria-label={t("notebook.taskInboxFilter")}
          style={filterStyle}
        />
        <div role="group" aria-label={t("notebook.taskInboxGroupBy")} style={{ display: "flex" }}>
          {GROUP_MODES.map((value, index) => (
            <button
              key={value}
              type="button"
              aria-pressed={mode === value}
              onClick={() => setMode(value)}
              style={{
                height: 22,
                padding: "0 7px",
                border: "1px solid var(--border-medium)",
                borderRadius:
                  index === 0
                    ? "4px 0 0 4px"
                    : index === GROUP_MODES.length - 1
                      ? "0 4px 4px 0"
                      : 0,
                borderLeftWidth: index === 0 ? 1 : 0,
                background: mode === value ? "var(--control-active-bg)" : "var(--bg-card)",
                color: mode === value ? "var(--control-active-fg)" : "var(--text-primary)",
                cursor: "pointer",
                fontSize: 10.5,
                whiteSpace: "nowrap",
              }}
            >
              {t(`notebook.taskGroupBy.${value}`)}
            </button>
          ))}
        </div>
        <label
          style={{
            display: "flex",
            alignItems: "center",
            gap: 4,
            color: "var(--text-muted)",
            fontSize: 11,
            whiteSpace: "nowrap",
            cursor: "pointer",
          }}
        >
          <input
            type="checkbox"
            checked={showDone}
            onChange={(event) => setShowDone(event.target.checked)}
          />
          {t("notebook.taskInboxShowDone")}
        </label>
      </div>

      <div
        style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: 5 }}
        data-testid="note-task-inbox-list"
      >
        {loading && tasks.length === 0 ? (
          <div style={hintStyle}>{t("notebook.taskInboxLoading")}</div>
        ) : error ? (
          <div style={{ ...hintStyle, color: "var(--warning)" }}>{error}</div>
        ) : groups.length === 0 ? (
          <div style={hintStyle}>
            {tasks.length === 0 ? t("notebook.taskInboxEmpty") : t("notebook.taskInboxNoMatch")}
          </div>
        ) : (
          groups.map((group) => (
            <div key={group.key} style={{ marginBottom: 6 }}>
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 6,
                  padding: "3px 6px",
                  color: "var(--text-muted)",
                  fontSize: 10.5,
                  textTransform: group.kind === "note" ? "none" : "uppercase",
                  letterSpacing: group.kind === "note" ? 0 : 0.4,
                }}
              >
                {group.kind === "time" && (group.key === "overdue" || group.key === "today") ? (
                  <span
                    aria-hidden
                    style={{
                      width: 2,
                      alignSelf: "stretch",
                      borderRadius: 1,
                      background:
                        group.key === "overdue" ? "var(--danger, #dc2626)" : "var(--accent)",
                    }}
                  />
                ) : null}
                <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}>
                  {groupLabel(group, t)}
                </span>
                <span style={{ color: "var(--text-hint)" }}>{group.tasks.length}</span>
              </div>
              {group.tasks.map((item) => (
                <button
                  key={`${item.path}:${item.line}`}
                  type="button"
                  /* 可及名里带上来源和行号:视觉上它们在第二行的小字里,而屏读逐个念
                     按钮时"这条在哪"才是要听到的那半句。 */
                  aria-label={t("notebook.taskInboxJump", {
                    text: item.text || t("notebook.taskInboxNoText"),
                    title: item.title,
                    line: String(item.line),
                  })}
                  title={`${item.raw}\n${item.path}:${item.line}`}
                  onClick={() => onJump(item.path, item.line)}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    event.stopPropagation();
                    const rect = event.currentTarget.getBoundingClientRect();
                    /* 键盘上下文菜单键没有鼠标坐标(clientX 是 0),那时候退回按钮自己
                       的位置 —— 不退的话菜单会跑到屏幕左上角。 */
                    onContextMenu(item, {
                      x: event.clientX || rect.left,
                      y: event.clientY || rect.bottom,
                    });
                  }}
                  style={{
                    width: "100%",
                    display: "flex",
                    alignItems: "flex-start",
                    gap: 6,
                    padding: "4px 6px",
                    border: "none",
                    borderRadius: 4,
                    background: "transparent",
                    color: "var(--text-primary)",
                    textAlign: "left",
                    cursor: "pointer",
                    fontSize: 11.5,
                    opacity: item.checked ? 0.55 : 1,
                  }}
                >
                  <span
                    aria-hidden
                    style={{
                      marginTop: 4,
                      width: 6,
                      height: 6,
                      flexShrink: 0,
                      borderRadius: 3,
                      background: PRIORITY_COLOR[item.priority ?? "_"],
                    }}
                  />
                  <span style={{ flex: 1, minWidth: 0 }}>
                    <span
                      style={{
                        display: "block",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                        textDecoration: item.checked ? "line-through" : "none",
                      }}
                    >
                      {item.text || t("notebook.taskInboxNoText")}
                    </span>
                    <span
                      style={{
                        display: "flex",
                        gap: 5,
                        flexWrap: "wrap",
                        marginTop: 1,
                        color: "var(--text-hint)",
                        fontSize: 10.5,
                      }}
                    >
                      {/* 「按笔记」时来源已经写在组标题上,再写一遍是噪声。 */}
                      {group.kind !== "note" ? <span>{item.title}</span> : null}
                      {item.due ? (
                        <span
                          style={{
                            color: item.due < today ? "var(--danger, #dc2626)" : "var(--text-hint)",
                          }}
                        >
                          {item.due}
                        </span>
                      ) : null}
                      {item.tags.map((tag) => (
                        <span key={tag}>#{tag}</span>
                      ))}
                    </span>
                  </span>
                </button>
              ))}
            </div>
          ))
        )}
      </div>
    </div>
  );
}
