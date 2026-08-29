/* 随手记的回收站。删掉的笔记先软删到 `<vault>/.notebook/trash/`,在这里恢复或
 * 彻底删除。
 *
 * 为什么需要这个面板而不是让用户去系统回收站捞:软删只搬文件、不记原位置的话
 * 恢复就无从下手 —— 后端存了清单(原相对路径 + 删除时间),这个面板是那份清单
 * 唯一的出口。没有它,删掉的笔记就成了 `.notebook/trash/` 里一堆用户看不见也
 * 删不掉的数据。
 *
 * 铺在面板内部(`position:absolute; inset:0`)而不是整个窗口,和版本历史一致:
 * 随手记面板可以只占项目视图的一半,盖住整个窗口会把用户正在参照的另一半也遮掉。
 */

import { useEffect, useRef, type CSSProperties } from "react";
import { RotateCcw, Trash2, X } from "lucide-react";

import type { TrashItem } from "./notebookApi";

export type NoteTrashSheetProps = {
  items: TrashItem[];
  loading: boolean;
  /** 正在恢复 / 彻底删除的那条的 id。按条禁用,不整面板禁用 —— 清一条的时候
   *  其余几十条还该能点。 */
  busyId: string | null;
  purgingAll: boolean;
  error: string | null;
  onRestore: (id: string) => void;
  onPurge: (id: string) => void;
  onPurgeAll: () => void;
  onClose: () => void;
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
  gap: 8,
  padding: "0 8px",
  borderBottom: "1px solid var(--border-dim)",
  color: "var(--text-muted)",
  fontSize: 11.5,
};

const hintStyle: CSSProperties = {
  margin: "auto",
  padding: 10,
  color: "var(--text-hint)",
  fontSize: 11.5,
};

const actionStyle: CSSProperties = {
  display: "flex",
  padding: 3,
  border: "none",
  borderRadius: 4,
  background: "transparent",
  cursor: "pointer",
};

function formatWhen(deletedAtMs: number, t: NoteTrashSheetProps["t"]): string {
  const elapsed = Date.now() - deletedAtMs;
  const minutes = Math.floor(elapsed / 60_000);
  if (minutes < 1) return t("notebook.trashJustNow");
  if (minutes < 60) return t("notebook.trashMinutesAgo", { count: String(minutes) });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t("notebook.trashHoursAgo", { count: String(hours) });
  const days = Math.floor(hours / 24);
  return t("notebook.trashDaysAgo", { count: String(days) });
}

export function NoteTrashSheet({
  items,
  loading,
  busyId,
  purgingAll,
  error,
  onRestore,
  onPurge,
  onPurgeAll,
  onClose,
  t,
}: NoteTrashSheetProps) {
  const closeRef = useRef<HTMLButtonElement | null>(null);

  // 打开时把焦点挪进来:不挪的话 Esc 会被编辑器的按键处理先吃掉,而 Tab 会从
  // 面板背后的元素开始走。
  useEffect(() => {
    closeRef.current?.focus();
  }, []);

  return (
    <div
      style={overlayStyle}
      role="dialog"
      aria-modal="true"
      aria-label={t("notebook.trashTitle")}
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        // 拦住:面板外面还有 window 级的 Esc 监听(会去关整个视图)。
        event.preventDefault();
        event.stopPropagation();
        onClose();
      }}
    >
      <div style={headerStyle}>
        <span>{t("notebook.trashTitle")}</span>
        <button
          type="button"
          disabled={items.length === 0 || purgingAll}
          onClick={onPurgeAll}
          style={{
            marginLeft: "auto",
            height: 22,
            padding: "0 8px",
            border: "1px solid var(--border)",
            borderRadius: 4,
            background: "transparent",
            color: items.length > 0 && !purgingAll ? "var(--danger, #f85149)" : "var(--text-hint)",
            fontSize: 11.5,
            cursor: items.length > 0 && !purgingAll ? "pointer" : "default",
          }}
        >
          {purgingAll ? t("notebook.trashPurgingAll") : t("notebook.trashPurgeAll")}
        </button>
        <button
          ref={closeRef}
          type="button"
          aria-label={t("notebook.trashClose")}
          onClick={onClose}
          style={{ ...actionStyle, color: "var(--text-hint)" }}
        >
          <X size={13} aria-hidden />
        </button>
      </div>
      {loading ? (
        <div style={{ flex: 1, display: "flex" }}>
          <div style={hintStyle}>{t("notebook.trashLoading")}</div>
        </div>
      ) : error ? (
        <div style={{ flex: 1, display: "flex" }}>
          <div style={{ ...hintStyle, color: "var(--warning)" }}>{error}</div>
        </div>
      ) : items.length === 0 ? (
        <div style={{ flex: 1, display: "flex" }}>
          <div style={hintStyle}>{t("notebook.trashEmpty")}</div>
        </div>
      ) : (
        <div style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: 5 }}>
          {items.map((item) => (
            <div
              key={item.id}
              data-testid="note-trash-row"
              style={{
                display: "flex",
                alignItems: "center",
                gap: 8,
                padding: "4px 6px",
                borderRadius: 4,
                fontSize: 11.5,
                opacity: busyId === item.id ? 0.5 : 1,
              }}
            >
              <span
                style={{
                  minWidth: 0,
                  flex: 1,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                  color: "var(--text-secondary)",
                }}
                /* 完整相对路径进 title:列表里只放得下文件名,而"原来在哪"是
                   决定要不要恢复的关键信息(同名笔记可以有好几条)。 */
                title={item.relativePath}
              >
                {item.name}
              </span>
              <span style={{ color: "var(--text-hint)", flexShrink: 0 }}>
                {formatWhen(item.deletedAtMs, t)}
              </span>
              <button
                type="button"
                aria-label={t("notebook.trashRestore", { name: item.name })}
                disabled={busyId === item.id}
                onClick={() => onRestore(item.id)}
                style={{ ...actionStyle, color: "var(--text-secondary)", flexShrink: 0 }}
              >
                <RotateCcw size={12} aria-hidden />
              </button>
              <button
                type="button"
                aria-label={t("notebook.trashPurge", { name: item.name })}
                disabled={busyId === item.id}
                onClick={() => onPurge(item.id)}
                style={{ ...actionStyle, color: "var(--danger, #f85149)", flexShrink: 0 }}
              >
                <Trash2 size={12} aria-hidden />
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/** 面板持有的回收站状态。抽出来是为了让面板那边只留一个 useState。 */
export type NoteTrashState = {
  items: TrashItem[];
  loading: boolean;
  busyId: string | null;
  purgingAll: boolean;
  error: string | null;
};

/** 初始状态。`useState` 的初值不能共享同一个对象引用,所以是构造器不是常量。 */
export function freshTrashState(): NoteTrashState {
  return { items: [], loading: true, busyId: null, purgingAll: false, error: null };
}
