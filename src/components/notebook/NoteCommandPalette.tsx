/* 命令面板(⌘K)。一个输入框 + 一列候选,候选里混着命令和笔记。
 *
 * 打分与排序全在 `noteCommands.ts`,这里只负责画和键盘导航。命令的 `run` 由
 * `NotebookPanel` 装配,面板本身不知道每条命令做什么。
 *
 * 和全库搜索(`NoteSearchSheet`)的区别是**边打边过滤**、不需要回车确认:命令面板
 * 的候选来自内存(命令表 + 已加载的笔记列表),没有 IPC,所以每次按键重算整表是
 * 廉价的;全库搜索每次都要跑一遍 ripgrep,必须等回车。
 *
 * 用 `listbox` / `option` 而不是一堆 button:焦点要一直留在输入框上(否则打字就断了),
 * 于是「当前选中哪一条」只能靠 `aria-activedescendant` 告诉读屏软件 —— 那要求候选
 * 是 option 而不是可聚焦的控件。
 */

import { Clock, CornerDownLeft, Search } from "lucide-react";
import { useEffect, useRef } from "react";
import type React from "react";

import { moveSelection, type MatchSpan, type PaletteEntry } from "./noteCommands";

export type NoteCommandPaletteProps = {
  query: string;
  onQueryChange: (value: string) => void;
  entries: readonly PaletteEntry[];
  /** 当前选中的下标。列表为空时是 -1。 */
  selected: number;
  onSelectedChange: (index: number) => void;
  /** 执行选中的那一条。 */
  onRun: (entry: PaletteEntry) => void;
  onClose: () => void;
  inputRef: React.RefObject<HTMLInputElement | null>;
  t: (key: string, vars?: Record<string, string | number>) => string;
};

const LIST_ID = "notebook-command-palette-list";

function optionId(index: number): string {
  return `${LIST_ID}-option-${index}`;
}

/** 按命中区间把文本切成 <mark> 与普通片段。区间来自模型层,已经是升序且不重叠。 */
function Highlighted({ text, spans }: { text: string; spans: readonly MatchSpan[] }) {
  if (spans.length === 0) return <>{text}</>;
  /* 按码点切,不按码元:区间是模型层用 `[...text]` 数出来的下标,而 `slice` 按码元
     ——emoji 或生僻字的标题上两者不等,直接 slice 会把代理对切成两半。 */
  const chars = [...text];
  const parts: React.ReactNode[] = [];
  let at = 0;
  spans.forEach((span, index) => {
    if (span.from > at) parts.push(chars.slice(at, span.from).join(""));
    parts.push(
      <mark
        key={index}
        style={{
          background: "color-mix(in srgb, var(--accent) 30%, transparent)",
          color: "var(--text-primary)",
        }}
      >
        {chars.slice(span.from, span.to).join("")}
      </mark>,
    );
    at = span.to;
  });
  if (at < chars.length) parts.push(chars.slice(at).join(""));
  return <>{parts}</>;
}

export function NoteCommandPalette({
  query,
  onQueryChange,
  entries,
  selected,
  onSelectedChange,
  onRun,
  onClose,
  inputRef,
  t,
}: NoteCommandPaletteProps) {
  const listRef = useRef<HTMLDivElement | null>(null);

  /* 选中项滚进可视区。用键盘一路按下去时,选中项会走到列表外面 —— 焦点在输入框上,
     浏览器不会替我们滚。 */
  useEffect(() => {
    if (selected < 0) return;
    const node = listRef.current?.querySelector(`#${optionId(selected)}`);
    // jsdom 没有实现 scrollIntoView。同 `DshTriggerMenu` 的处理 —— 滚动是纯视觉,
    // 缺了它键盘导航本身照常工作,不值得为此在测试环境里塞一个假实现。
    if (node && typeof node.scrollIntoView === "function")
      node.scrollIntoView({ block: "nearest" });
  }, [selected]);

  const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      onSelectedChange(moveSelection(selected, event.key === "ArrowDown" ? 1 : -1, entries.length));
      return;
    }
    if (event.key !== "Enter") return;
    /* IME 组字过程中的回车是「确认候选词」,不是「执行」。不挡的话中文输入法下
       打第一个字就把面板执行掉了。 */
    if (event.nativeEvent.isComposing) return;
    event.preventDefault();
    const entry = entries[selected];
    if (entry) onRun(entry);
  };

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={t("notebook.commandPalette")}
      style={{
        position: "absolute",
        inset: 0,
        zIndex: 31,
        display: "flex",
        flexDirection: "column",
        background: "var(--bg-panel)",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 6,
          padding: "7px 9px",
          borderBottom: "1px solid var(--border-dim)",
          background: "var(--bg-sidebar)",
        }}
      >
        <Search size={13} color="var(--text-muted)" />
        <input
          ref={inputRef}
          role="combobox"
          aria-expanded
          aria-controls={LIST_ID}
          aria-autocomplete="list"
          aria-activedescendant={selected >= 0 ? optionId(selected) : undefined}
          aria-label={t("notebook.commandPalette")}
          value={query}
          onChange={(event) => onQueryChange(event.currentTarget.value)}
          onKeyDown={handleKeyDown}
          placeholder={t("notebook.commandPalettePlaceholder")}
          style={{
            flex: 1,
            minWidth: 0,
            height: 26,
            border: "1px solid var(--border-medium)",
            borderRadius: 6,
            background: "var(--bg-input)",
            color: "var(--text-primary)",
            padding: "0 8px",
            fontSize: 12,
            outline: "none",
          }}
        />
      </div>
      <div
        ref={listRef}
        id={LIST_ID}
        role="listbox"
        aria-label={t("notebook.commandPalette")}
        style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "5px 6px 8px" }}
      >
        {entries.length === 0 ? (
          <div style={{ padding: "8px 6px", fontSize: 11, color: "var(--text-muted)" }}>
            {t("notebook.commandPaletteEmpty")}
          </div>
        ) : (
          entries.map((entry, index) => {
            const active = index === selected;
            const isCommand = entry.kind === "command";
            const label = isCommand ? entry.command.label : entry.title;
            const disabled = isCommand && entry.command.disabled === true;
            return (
              <div
                key={isCommand ? `c:${entry.command.id}` : `n:${entry.noteId}`}
                id={optionId(index)}
                role="option"
                aria-selected={active}
                aria-disabled={disabled || undefined}
                /* 鼠标按下就执行(而不是 click):click 之前会先 mousedown,那一下会
                   把焦点从输入框挪走并让面板关掉,于是 click 落到已经不存在的节点上。 */
                onMouseDown={(event) => {
                  event.preventDefault();
                  onRun(entry);
                }}
                onMouseEnter={() => onSelectedChange(index)}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 7,
                  borderRadius: 5,
                  background: active ? "var(--control-active-bg)" : "transparent",
                  color: disabled
                    ? "var(--text-muted)"
                    : active
                      ? "var(--control-active-fg)"
                      : "var(--text-secondary)",
                  padding: "4px 6px",
                  cursor: disabled ? "not-allowed" : "pointer",
                  fontSize: 12,
                  opacity: disabled ? 0.55 : 1,
                }}
              >
                {!isCommand && entry.recent ? (
                  <Clock size={11} color="var(--text-muted)" aria-hidden="true" />
                ) : null}
                <span
                  style={{
                    flex: 1,
                    minWidth: 0,
                    whiteSpace: "nowrap",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                  }}
                >
                  <Highlighted text={label} spans={entry.spans} />
                </span>
                {isCommand ? (
                  <span style={{ fontSize: 10, color: "var(--text-muted)" }}>
                    {t(entry.command.group)}
                  </span>
                ) : (
                  <span style={{ fontSize: 10, color: "var(--text-muted)" }}>
                    {t("notebook.commandPaletteNote")}
                  </span>
                )}
                {isCommand && entry.command.hint ? (
                  <span
                    style={{
                      fontSize: 10,
                      color: "var(--text-muted)",
                      fontFamily: "var(--font-mono, monospace)",
                    }}
                  >
                    {entry.command.hint}
                  </span>
                ) : null}
                {active ? <CornerDownLeft size={11} aria-hidden="true" /> : null}
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}
