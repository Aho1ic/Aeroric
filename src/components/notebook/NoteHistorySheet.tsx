/* 随手记的版本历史。左侧是快照列表,右侧是它和当前内容的行级 diff。
 *
 * 为什么不复用 FileViewer 的 `LocalHistoryDialog`:那个组件写死了代码文件那套
 * 命令(按项目根定位)和 `file.*` 文案,而且它的"改了 N 行"是按行号逐行比 ——
 * 在开头插一行就会报成整篇都变了。这里的 diff 走 `lineDiff.ts` 的 LCS。
 *
 * 铺在面板内部(`position:absolute; inset:0`)而不是整个窗口:随手记面板可以
 * 只占项目视图的一半,盖住整个窗口会把用户正在参照的另一半也遮掉。
 */

import { useEffect, useMemo, useRef, type CSSProperties } from "react";
import { RotateCcw, X } from "lucide-react";

import { changedLineCount, collapseContext, diffLines, type DiffLine } from "./lineDiff";
import type { NoteSnapshot, NoteSnapshotEntry } from "./notebookApi";

export type NoteHistorySheetProps = {
  noteTitle: string;
  entries: NoteSnapshotEntry[];
  selectedId: string | null;
  /**
   * 选中快照的文件内容,没选中时为 null。
   *
   * 收的是**整个文件**(frontmatter 在里面),不是 `NoteSnapshot` —— 和它比的
   * `currentContent` 也必须是整个文件。收对象的话两边一个带 frontmatter 一个不带
   * 也照样能编译,而 diff 会把 frontmatter 的每一行报成删除。
   */
  snapshotContent: string | null;
  /** 当前笔记落盘会长什么样(见 `noteFileContent`)。磁盘上那一版可能更旧 ——
   *  用户想看的是"和我眼前这版差在哪"。 */
  currentContent: string;
  loading: boolean;
  snapshotLoading: boolean;
  restoring: boolean;
  error: string | null;
  onSelect: (entryId: string) => void;
  onRestore: () => void;
  onClose: () => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

const overlayStyle: CSSProperties = {
  position: "absolute",
  inset: 0,
  zIndex: 30,
  display: "flex",
  background: "var(--bg-panel)",
};

const listStyle: CSSProperties = {
  width: 176,
  flexShrink: 0,
  minHeight: 0,
  overflowY: "auto",
  borderRight: "1px solid var(--border-dim)",
  padding: 5,
};

const diffStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  minHeight: 0,
  overflow: "auto",
  margin: 0,
  padding: "6px 0",
  fontFamily: "var(--font-mono)",
  fontSize: 11.5,
  lineHeight: 1.55,
};

const hintStyle: CSSProperties = {
  margin: "auto",
  padding: 10,
  color: "var(--text-hint)",
  fontSize: 11.5,
};

function formatWhen(createdAtMs: number, t: NoteHistorySheetProps["t"]): string {
  const elapsed = Date.now() - createdAtMs;
  const minutes = Math.floor(elapsed / 60_000);
  if (minutes < 1) return t("notebook.historyJustNow");
  if (minutes < 60) return t("notebook.historyMinutesAgo", { count: String(minutes) });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t("notebook.historyHoursAgo", { count: String(hours) });
  return new Date(createdAtMs).toLocaleString();
}

/** 一行 diff 的底色与前缀。 */
const LINE_STYLE: Record<DiffLine["kind"], { background: string; prefix: string }> = {
  context: { background: "transparent", prefix: " " },
  added: {
    background: "color-mix(in srgb, var(--success, #3fb950) 16%, transparent)",
    prefix: "+",
  },
  removed: {
    background: "color-mix(in srgb, var(--danger, #f85149) 16%, transparent)",
    prefix: "-",
  },
};

export function NoteHistorySheet({
  noteTitle,
  entries,
  selectedId,
  snapshotContent,
  currentContent,
  loading,
  snapshotLoading,
  restoring,
  error,
  onSelect,
  onRestore,
  onClose,
  t,
}: NoteHistorySheetProps) {
  const closeRef = useRef<HTMLButtonElement | null>(null);

  // 打开时把焦点挪进来。不挪的话焦点还在编辑器上,Esc 会被编辑器的按键处理先
  // 吃掉,而 Tab 会从面板背后的元素开始走。
  useEffect(() => {
    closeRef.current?.focus();
  }, []);

  const diff = useMemo(
    () => (snapshotContent === null ? [] : diffLines(snapshotContent, currentContent)),
    [snapshotContent, currentContent],
  );
  const segments = useMemo(() => collapseContext(diff), [diff]);
  const changed = changedLineCount(diff);

  return (
    <div
      style={overlayStyle}
      role="dialog"
      aria-modal="true"
      aria-label={t("notebook.historyTitle", { name: noteTitle })}
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        // 拦住:面板外面还有 window 级的 Esc 监听(会去关整个视图)。
        event.preventDefault();
        event.stopPropagation();
        onClose();
      }}
    >
      <div style={listStyle}>
        {loading ? (
          <div style={hintStyle}>{t("notebook.historyLoading")}</div>
        ) : error ? (
          <div style={{ ...hintStyle, color: "var(--warning)" }}>{error}</div>
        ) : entries.length === 0 ? (
          <div style={hintStyle}>{t("notebook.historyEmpty")}</div>
        ) : (
          entries.map((entry) => (
            <button
              key={entry.id}
              type="button"
              aria-pressed={entry.id === selectedId}
              onClick={() => onSelect(entry.id)}
              style={{
                width: "100%",
                display: "block",
                textAlign: "left",
                padding: "5px 7px",
                border: "none",
                borderRadius: 4,
                background: entry.id === selectedId ? "var(--bg-hover)" : "transparent",
                color: "var(--text-secondary)",
                fontSize: 11.5,
                cursor: "pointer",
              }}
            >
              {formatWhen(entry.createdAtMs, t)}
            </button>
          ))
        )}
      </div>
      <div style={{ flex: 1, minWidth: 0, minHeight: 0, display: "flex", flexDirection: "column" }}>
        <div
          style={{
            minHeight: 32,
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "0 8px",
            borderBottom: "1px solid var(--border-dim)",
            color: "var(--text-muted)",
            fontSize: 11.5,
          }}
        >
          <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}>
            {snapshotLoading
              ? t("notebook.historyLoading")
              : snapshotContent !== null
                ? t("notebook.historyChangedLines", { count: String(changed) })
                : t("notebook.historyPickOne")}
          </span>
          <button
            type="button"
            disabled={snapshotContent === null || restoring}
            onClick={onRestore}
            style={{
              marginLeft: "auto",
              display: "flex",
              alignItems: "center",
              gap: 4,
              height: 22,
              padding: "0 8px",
              border: "1px solid var(--border)",
              borderRadius: 4,
              background: "transparent",
              color:
                snapshotContent !== null && !restoring
                  ? "var(--text-secondary)"
                  : "var(--text-hint)",
              fontSize: 11.5,
              cursor: snapshotContent !== null && !restoring ? "pointer" : "default",
            }}
          >
            <RotateCcw size={11} aria-hidden />
            {restoring ? t("notebook.historyRestoring") : t("notebook.historyRestore")}
          </button>
          <button
            ref={closeRef}
            type="button"
            aria-label={t("notebook.historyClose")}
            onClick={onClose}
            style={{
              display: "flex",
              padding: 3,
              border: "none",
              borderRadius: 4,
              background: "transparent",
              color: "var(--text-hint)",
              cursor: "pointer",
            }}
          >
            <X size={13} aria-hidden />
          </button>
        </div>
        {snapshotContent !== null ? (
          <pre style={diffStyle} data-testid="note-history-diff">
            {segments.map((segment, index) =>
              segment.kind === "gap" ? (
                <div
                  key={`gap-${index}`}
                  style={{
                    padding: "1px 8px",
                    color: "var(--text-hint)",
                    background: "var(--bg-hover)",
                  }}
                >
                  {t("notebook.historyHiddenLines", { count: String(segment.hiddenLines) })}
                </div>
              ) : (
                segment.lines.map((line) => {
                  const look = LINE_STYLE[line.kind];
                  return (
                    <div
                      key={`${line.kind}-${line.oldLine ?? "x"}-${line.newLine ?? "x"}`}
                      style={{
                        padding: "0 8px",
                        background: look.background,
                        whiteSpace: "pre-wrap",
                        wordBreak: "break-word",
                      }}
                    >
                      {/* 前缀跟着行走而不是靠底色单独表达:底色在高对比主题下会
                          被压掉,而增删是这里唯一要看的信息。 */}
                      {look.prefix}
                      {line.text}
                    </div>
                  );
                })
              ),
            )}
          </pre>
        ) : (
          <div style={{ flex: 1, display: "flex" }}>
            <div style={hintStyle}>
              {snapshotLoading ? t("notebook.historyLoading") : t("notebook.historyPickOne")}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

/** 面板持有的历史面板状态。抽出来是为了让面板那边只留一个 useState。 */
export type NoteHistoryState = {
  entries: NoteSnapshotEntry[];
  selectedId: string | null;
  snapshot: NoteSnapshot | null;
  loading: boolean;
  snapshotLoading: boolean;
  restoring: boolean;
  error: string | null;
};

export const emptyHistoryState: NoteHistoryState = {
  entries: [],
  selectedId: null,
  snapshot: null,
  loading: true,
  snapshotLoading: false,
  restoring: false,
  error: null,
};

/** 供面板复用的初始状态构造器。`useState` 的初值不能共享同一个对象引用。 */
export function freshHistoryState(): NoteHistoryState {
  return { ...emptyHistoryState };
}
