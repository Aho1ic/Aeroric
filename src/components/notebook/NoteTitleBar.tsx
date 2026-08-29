/* 随手记正文区顶部的标题栏:标题输入 + 字数统计 + 视图切换 + 删除。
 *
 * 从 NotebookPanel 抽出来,JSX 逐字未改。
 *
 * 标题不等于文件名 —— 它存在 frontmatter 里,改标题不会改文件名。P4 的
 * `[[wikilink]]` 按文件名连接,静默改名会断链(见 notebookVault 的注释)。
 */

import { List, PanelLeft, Trash2 } from "lucide-react";
import type React from "react";

export type NoteViewMode = "edit" | "wysiwyg" | "split" | "read";

export type NoteTitleBarProps = {
  title: string;
  onTitleChange: (title: string) => void;
  /** 新建笔记后自动聚焦到标题框,由面板在 layout effect 里驱动。 */
  titleInputRef: React.RefObject<HTMLInputElement | null>;
  /** 字数与阅读时长。`words` 为 0 时整块不显示(空笔记没什么可统计的)。 */
  words: number;
  readingMinutes: number;
  mode: NoteViewMode;
  onModeChange: (mode: NoteViewMode) => void;
  /** 紧凑档才给笔记列表的开关 —— 别的档位列表一直在,按钮只是噪音。 */
  showListToggle: boolean;
  listOpen: boolean;
  onToggleList: () => void;
  /** 大纲面板的开关。 */
  outlineOpen: boolean;
  onToggleOutline: () => void;
  onDelete: () => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

export function NoteTitleBar({
  title,
  onTitleChange,
  titleInputRef,
  words,
  readingMinutes,
  mode,
  onModeChange,
  showListToggle,
  listOpen,
  onToggleList,
  outlineOpen,
  onToggleOutline,
  onDelete,
  t,
}: NoteTitleBarProps) {
  return (
    <div
      style={{
        minHeight: 38,
        display: "flex",
        alignItems: "center",
        gap: 8,
        padding: "0 10px",
        borderBottom: "1px solid var(--border-dim)",
      }}
    >
      <input
        ref={titleInputRef}
        aria-label={t("notebook.memoName")}
        value={title}
        onChange={(event) => onTitleChange(event.currentTarget.value)}
        style={{
          flex: 1,
          minWidth: 0,
          border: "none",
          outline: "none",
          background: "transparent",
          color: "var(--text-primary)",
          fontSize: 13,
          fontWeight: 700,
        }}
      />
      {words > 0 && (
        <span
          style={{ fontSize: 11, color: "var(--text-muted)", whiteSpace: "nowrap" }}
          title={t("notebook.statsTitle", {
            words: String(words),
            minutes: String(readingMinutes),
          })}
        >
          {t("notebook.stats", {
            words: String(words),
            minutes: String(readingMinutes),
          })}
        </span>
      )}
      <span style={{ fontSize: 11, color: "var(--text-muted)" }}>{"Markdown"}</span>
      {showListToggle && (
        <button
          type="button"
          aria-label={listOpen ? t("notebook.hideList") : t("notebook.showList")}
          title={listOpen ? t("notebook.hideList") : t("notebook.showList")}
          aria-pressed={listOpen}
          onClick={onToggleList}
          style={{
            height: 26,
            border: "1px solid var(--border-medium)",
            borderRadius: 6,
            background: listOpen ? "var(--control-active-bg)" : "var(--bg-card)",
            color: listOpen ? "var(--control-active-fg)" : "var(--text-primary)",
            cursor: "pointer",
            padding: "0 6px",
            display: "flex",
            alignItems: "center",
            flexShrink: 0,
          }}
        >
          <PanelLeft size={13} />
        </button>
      )}
      <button
        type="button"
        aria-label={outlineOpen ? t("notebook.hideOutline") : t("notebook.showOutline")}
        title={outlineOpen ? t("notebook.hideOutline") : t("notebook.showOutline")}
        aria-pressed={outlineOpen}
        onClick={onToggleOutline}
        style={{
          height: 26,
          border: "1px solid var(--border-medium)",
          borderRadius: 6,
          background: outlineOpen ? "var(--control-active-bg)" : "var(--bg-card)",
          color: outlineOpen ? "var(--control-active-fg)" : "var(--text-primary)",
          cursor: "pointer",
          padding: "0 6px",
          display: "flex",
          alignItems: "center",
          flexShrink: 0,
        }}
      >
        <List size={13} />
      </button>
      {
        // Markdown 有三态。用分段控件而不是循环切换按钮:三态下"下一个是
        // 什么"不直观,用户要点两次才能到想去的地方。
        <div
          role="group"
          aria-label={t("notebook.viewMode")}
          style={{ display: "inline-flex", flexShrink: 0 }}
        >
          {(
            [
              ["edit", t("notebook.source")],
              ["wysiwyg", t("notebook.wysiwyg")],
              ["split", t("notebook.split")],
              ["read", t("notebook.read")],
            ] as const
          ).map(([value, label], index, all) => (
            <button
              key={value}
              type="button"
              aria-pressed={mode === value}
              onClick={() => onModeChange(value)}
              style={{
                height: 26,
                border: "1px solid var(--border-medium)",
                // 三段拼成一个控件:只有首尾有圆角,中间的左边框省掉避免双线。
                borderRadius:
                  index === 0 ? "6px 0 0 6px" : index === all.length - 1 ? "0 6px 6px 0" : 0,
                borderLeftWidth: index === 0 ? 1 : 0,
                background: mode === value ? "var(--control-active-bg)" : "var(--bg-card)",
                color: mode === value ? "var(--control-active-fg)" : "var(--text-primary)",
                cursor: "pointer",
                padding: "0 8px",
                fontSize: 12,
              }}
            >
              {label}
            </button>
          ))}
        </div>
      }
      <button
        type="button"
        aria-label={t("common.delete")}
        title={t("common.delete")}
        onClick={onDelete}
        style={{
          border: "none",
          background: "transparent",
          color: "var(--text-muted)",
          cursor: "pointer",
          padding: 4,
        }}
      >
        <Trash2 size={14} />
      </button>
    </div>
  );
}
