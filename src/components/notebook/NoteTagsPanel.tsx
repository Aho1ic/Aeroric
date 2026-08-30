/* 侧栏的「标签」分区:全库有哪些 `#标签`,各有几处,分别在哪。
 *
 * 从 Markio 的 `TagLandscape.tsx` 移植,但换了两处骨架:
 *
 * - Markio 用字号编码处数(log 拉伸到 11–22px)。这里改成把数字直接写出来:字号是
 *   **只有视觉**的编码,屏读听不到,而且"这个比那个大一点"没法精确比较 —— 而用户
 *   真正要的答案("这个标签有几处")本来就是个数字。
 * - Markio 那份是铺满一整个 tab 的标签云 + 右侧详情。这里在侧栏那一列里,所以是
 *   竖排:筛选框 → 标签行 → 点开某个标签后就地展开它的引用。
 *
 * 扫描由父面板发起(它知道 vault 和什么时候该重扫),这个组件只负责画。
 */

import { CornerDownRight, Pencil, RefreshCw } from "lucide-react";
import type { TagEntry } from "./noteTags";

export type NoteTagsPanelProps = {
  entries: TagEntry[];
  /** 全库标签总处数(不是标签个数)。 */
  count: number;
  loading: boolean;
  /** 扫描失败时的文案。标签是只读视图,失败就地显示,不占用面板那条错误条。 */
  error: string | null;
  /** 筛选输入。受控 —— 展开状态和它一起归父面板管,切档回来时不丢。 */
  query: string;
  onQueryChange: (next: string) => void;
  /** 当前展开的标签 key,`null` = 没展开。 */
  openKey: string | null;
  onToggle: (key: string) => void;
  /** 跳到某一篇的某一行。 */
  onJump: (path: string, line: number) => void;
  /** 开跨文件重命名的小窗。`anchor` 是那个按钮的屏幕位置。 */
  onRename: (entry: TagEntry, anchor: { x: number; y: number }) => void;
  onRefresh: () => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

export function NoteTagsPanel({
  entries,
  count,
  loading,
  error,
  query,
  onQueryChange,
  openKey,
  onToggle,
  onJump,
  onRename,
  onRefresh,
  t,
}: NoteTagsPanelProps) {
  return (
    <aside
      aria-label={t("notebook.tags")}
      style={{ minWidth: 0, minHeight: 0, display: "flex", flexDirection: "column" }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 4,
          padding: "5px 6px 5px 8px",
          fontSize: 10,
          color: "var(--text-muted)",
          textTransform: "uppercase",
          letterSpacing: 0.4,
        }}
      >
        <span style={{ flex: 1, minWidth: 0 }}>
          {t("notebook.tagCount", {
            count: String(count),
            tags: String(entries.length),
          })}
        </span>
        <button
          type="button"
          aria-label={t("notebook.tagRefresh")}
          title={t("notebook.tagRefresh")}
          onClick={onRefresh}
          disabled={loading}
          style={{
            border: "none",
            background: "transparent",
            color: "var(--text-muted)",
            cursor: loading ? "progress" : "pointer",
            opacity: loading ? 0.45 : 1,
            padding: 2,
            display: "flex",
            flexShrink: 0,
          }}
        >
          <RefreshCw size={11} />
        </button>
      </div>

      {error ? (
        <div
          role="alert"
          style={{
            margin: "0 8px 8px",
            padding: "6px 8px",
            borderRadius: 6,
            background: "var(--danger-subtle, var(--bg-card))",
            color: "var(--danger, var(--text-primary))",
            fontSize: 11,
            lineHeight: 1.4,
            wordBreak: "break-word",
          }}
        >
          {error}
        </div>
      ) : null}

      <div style={{ padding: "0 8px 6px" }}>
        <input
          type="search"
          value={query}
          onChange={(event) => onQueryChange(event.target.value)}
          aria-label={t("notebook.tagFilter")}
          placeholder={t("notebook.tagFilter")}
          style={{
            width: "100%",
            boxSizing: "border-box",
            height: 22,
            border: "1px solid var(--border-medium)",
            borderRadius: 5,
            background: "var(--bg-card)",
            color: "var(--text-primary)",
            padding: "0 6px",
            fontSize: 11,
          }}
        />
      </div>

      {/* 加载中仍然显示上一次的结果 —— 清空会让「刷新」变成一次闪烁,而标签清单在
          两次扫描之间通常几乎不变。 */}
      {entries.length === 0 && !loading && !error ? (
        <div style={{ padding: "4px 10px 12px", fontSize: 11, color: "var(--text-hint)" }}>
          {query.trim() ? t("notebook.tagNoMatch") : t("notebook.tagEmpty")}
        </div>
      ) : (
        <div style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "0 6px 10px" }}>
          {entries.map((entry) => (
            <div key={entry.key} style={{ marginBottom: 2 }}>
              {/* 展开和重命名是两个**并列**的按钮:button 不能套 button,而重命名要有自己
                  的可及名("全库重命名 #x")—— 塞进展开按钮里的话屏读念不出它是干什么的。
                  底色挪到这层,好让它盖住两个按钮而不是只有半行。 */}
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  borderRadius: 4,
                  background: openKey === entry.key ? "var(--bg-hover)" : "transparent",
                }}
              >
                <button
                  type="button"
                  aria-expanded={openKey === entry.key}
                  /* 可及名里带上处数和篇数:视觉上它们在行尾的小字里,而屏读逐个念
                   按钮时"这个标签有多少"才是要听到的那半句。 */
                  aria-label={t("notebook.tagEntry", {
                    tag: entry.label,
                    count: String(entry.count),
                    notes: String(entry.notes),
                  })}
                  onClick={() => onToggle(entry.key)}
                  style={{
                    flex: 1,
                    minWidth: 0,
                    display: "flex",
                    alignItems: "center",
                    gap: 4,
                    border: "none",
                    borderRadius: 4,
                    background: "transparent",
                    color: "var(--text-primary)",
                    cursor: "pointer",
                    textAlign: "left",
                    padding: "3px 4px",
                    fontSize: 11,
                    lineHeight: 1.35,
                  }}
                >
                  <span
                    style={{
                      flex: 1,
                      minWidth: 0,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    #{entry.label}
                  </span>
                  <span aria-hidden="true" style={{ flexShrink: 0, color: "var(--text-muted)" }}>
                    {entry.count}
                  </span>
                </button>
                <button
                  type="button"
                  aria-label={t("notebook.tagRename", { tag: entry.label })}
                  title={t("notebook.tagRename", { tag: entry.label })}
                  onClick={(event) => {
                    /* 锚点取按钮自己的位置,不用鼠标坐标:键盘按下 Enter 时没有鼠标
                       坐标,拿 event.clientX 会得到 0 —— 窗跑到屏幕左上角。 */
                    const rect = event.currentTarget.getBoundingClientRect();
                    onRename(entry, { x: rect.left, y: rect.bottom + 4 });
                  }}
                  style={{
                    border: "none",
                    background: "transparent",
                    color: "var(--text-muted)",
                    cursor: "pointer",
                    padding: 3,
                    display: "flex",
                    flexShrink: 0,
                  }}
                >
                  <Pencil size={10} />
                </button>
              </div>
              {openKey === entry.key
                ? entry.refs.map((ref) => (
                    <button
                      key={`${ref.path}:${ref.line}`}
                      type="button"
                      aria-label={t("notebook.tagJump", {
                        title: ref.title,
                        line: String(ref.line),
                      })}
                      title={ref.preview}
                      onClick={() => onJump(ref.path, ref.line)}
                      style={{
                        width: "100%",
                        display: "flex",
                        alignItems: "flex-start",
                        gap: 3,
                        border: "none",
                        borderRadius: 4,
                        background: "transparent",
                        color: "var(--text-primary)",
                        cursor: "pointer",
                        textAlign: "left",
                        padding: "2px 4px 2px 12px",
                        fontSize: 11,
                        lineHeight: 1.35,
                      }}
                    >
                      <CornerDownRight
                        size={10}
                        aria-hidden="true"
                        style={{ marginTop: 2, flexShrink: 0, color: "var(--text-muted)" }}
                      />
                      <span
                        style={{
                          minWidth: 0,
                          overflow: "hidden",
                          display: "-webkit-box",
                          WebkitBoxOrient: "vertical",
                          WebkitLineClamp: 2,
                        }}
                      >
                        {ref.preview}
                      </span>
                    </button>
                  ))
                : null}
            </div>
          ))}
        </div>
      )}
    </aside>
  );
}
