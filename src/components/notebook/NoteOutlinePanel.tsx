/* 随手记的大纲面板。
 *
 * 从 Markio 的 Outline.tsx 移植(`OutlinePanel` 那一半)。Markio 那个组件还带
 * 「信息」和「链接」两个 tab:信息就是字数/阅读时长,已经在标题栏上;链接是反链,
 * 属于 P4,这里不做。
 *
 * 折叠态按下标存,但只在**大纲结构真的变了**时才重置(按 level+文本取签名)——
 * 父组件每次敲字都会传进来一个新数组,按数组身份重置的话连续输入会把用户手动
 * 折叠的章节不断展开。改正文不改标题 → 签名不变 → 折叠保留。
 *
 * 拖动重排没有照搬 Markio 的 HTML5 draggable,改成复用笔记列表那套
 * `useNoteDragReorder`(Pointer Events):HTML5 drag 在 Tauri 的 WebView 里不可靠,
 * 而 pointer 那套在这个面板里已经跑通了。顺带整个随手记只剩一种拖动写法。
 * 下标当 id 用(hook 是按字符串 id 索引的)。
 */

import { useEffect, useMemo, useRef, useState } from "react";
import type React from "react";
import { ChevronDown, ChevronRight, Search, X } from "lucide-react";
import type { OutlineItem } from "./noteOutline";
import { useNoteDragReorder } from "./useNoteDragReorder";

export type NoteOutlinePanelProps = {
  items: OutlineItem[];
  /** 点标题时滚到对应位置。由面板实现 —— 阅读态按 id 找,源码态按偏移滚。 */
  onJump: (item: OutlineItem) => void;
  /** 拖动重排章节。不传就不可拖(阅读态没有可编辑的源码)。 */
  onReorder?: (sourceIndex: number, targetIndex: number) => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

/** 把 text 按 query(忽略大小写)切片,命中段用 `<mark>` 包起来。 */
function highlightText(text: string, needle: string): React.ReactNode {
  if (!needle) return text;
  const lower = text.toLocaleLowerCase();
  const parts: React.ReactNode[] = [];
  let cursor = 0;
  while (cursor < text.length) {
    const hit = lower.indexOf(needle, cursor);
    if (hit < 0) {
      parts.push(text.slice(cursor));
      break;
    }
    if (hit > cursor) parts.push(text.slice(cursor, hit));
    const end = hit + needle.length;
    parts.push(
      <mark key={`hit-${hit}`} style={{ background: "var(--highlight-bg)", color: "inherit" }}>
        {text.slice(hit, end)}
      </mark>,
    );
    cursor = end;
  }
  return parts;
}

export function NoteOutlinePanel({ items, onJump, onReorder, t }: NoteOutlinePanelProps) {
  const [query, setQuery] = useState("");
  const [collapsed, setCollapsed] = useState<Set<number>>(() => new Set());
  const searchRef = useRef<HTMLInputElement | null>(null);
  const drag = useNoteDragReorder((draggedId, targetId) => {
    onReorder?.(Number(draggedId), Number(targetId));
  });

  // 只在结构变化时重置折叠态(见文件头注释)。
  //
  // 两道闸其实互为冗余:`[signature]` 依赖已经能拦住"只改正文"的重渲染,ref 比对
  // 单独也能拦住。留着 ref 是因为依赖数组太容易被顺手改坏 —— 有人为了消 exhaustive-deps
  // 的告警把 `items` 加进去,折叠态就会在连续输入时不停被清掉,而那是很难察觉的手感
  // 退化。测试钉的是行为(两道都拆掉才会红),不是其中某一道。
  const signature = useMemo(
    () => items.map((item) => `${item.level} ${item.text}`).join("\n"),
    [items],
  );
  const previousSignature = useRef(signature);
  useEffect(() => {
    if (previousSignature.current === signature) return;
    previousSignature.current = signature;
    setCollapsed(new Set());
  }, [signature]);

  // 每项的直接父节点下标(没有父则 null),用栈按层级推断。
  const parentOf = useMemo(() => {
    const parents: (number | null)[] = new Array(items.length).fill(null);
    const stack: number[] = [];
    for (let index = 0; index < items.length; index += 1) {
      const level = items[index]!.level;
      while (stack.length > 0 && items[stack[stack.length - 1]!]!.level >= level) stack.pop();
      parents[index] = stack.length > 0 ? stack[stack.length - 1]! : null;
      stack.push(index);
    }
    return parents;
  }, [items]);

  const hasChildren = useMemo(() => {
    const flags = new Array<boolean>(items.length).fill(false);
    for (const parent of parentOf) if (parent !== null) flags[parent] = true;
    return flags;
  }, [items.length, parentOf]);

  const needle = query.trim().toLocaleLowerCase();

  const matched = useMemo(() => {
    if (!needle) return null;
    const set = new Set<number>();
    items.forEach((item, index) => {
      if (item.text.toLocaleLowerCase().includes(needle)) set.add(index);
    });
    return set;
  }, [items, needle]);

  // 可见集合。搜索时是「命中项 + 它们的祖先链」(保留层级上下文);
  // 平时是「没有被折叠的祖先挡住的」。
  const visible = useMemo(() => {
    const set = new Set<number>();
    if (matched) {
      for (const index of matched) {
        set.add(index);
        let parent = parentOf[index] ?? null;
        while (parent !== null) {
          set.add(parent);
          parent = parentOf[parent] ?? null;
        }
      }
      return set;
    }
    for (let index = 0; index < items.length; index += 1) {
      let hidden = false;
      let parent = parentOf[index] ?? null;
      while (parent !== null) {
        if (collapsed.has(parent)) {
          hidden = true;
          break;
        }
        parent = parentOf[parent] ?? null;
      }
      if (!hidden) set.add(index);
    }
    return set;
  }, [collapsed, items.length, matched, parentOf]);

  const allExpanded = collapsed.size === 0;
  // 搜索时强制展开(要让用户看到祖先链),所以也不允许拖 —— 可见集合是稀疏的,
  // 「拖到祖先」和「拖到命中项之间」语义不直观。
  const dragEnabled = Boolean(onReorder) && !matched;

  const toggleCollapse = (index: number) => {
    setCollapsed((current) => {
      const next = new Set(current);
      if (!next.delete(index)) next.add(index);
      return next;
    });
  };

  const toggleAll = () => {
    if (!allExpanded) {
      setCollapsed(new Set());
      return;
    }
    const all = new Set<number>();
    items.forEach((_item, index) => {
      if (hasChildren[index]) all.add(index);
    });
    setCollapsed(all);
  };

  return (
    <aside
      aria-label={t("notebook.outline")}
      style={{
        minWidth: 0,
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
        borderLeft: "1px solid var(--border-dim)",
        background: "var(--bg-sidebar)",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 4,
          padding: "6px 6px 6px 8px",
          borderBottom: "1px solid var(--border-dim)",
        }}
      >
        <div style={{ position: "relative", flex: 1, minWidth: 0, display: "flex" }}>
          <Search
            size={11}
            style={{
              position: "absolute",
              left: 6,
              top: "50%",
              transform: "translateY(-50%)",
              color: "var(--text-muted)",
              pointerEvents: "none",
            }}
          />
          <input
            ref={searchRef}
            type="search"
            aria-label={t("notebook.filterSections")}
            placeholder={t("notebook.filterSections")}
            value={query}
            spellCheck={false}
            onChange={(event) => setQuery(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key !== "Escape" || !query) return;
              // 不让 Esc 冒出去 —— 面板外层可能拿它做别的事(关闭查找栏之类)。
              event.preventDefault();
              event.stopPropagation();
              setQuery("");
            }}
            style={{
              flex: 1,
              minWidth: 0,
              height: 22,
              padding: "0 6px 0 21px",
              border: "1px solid var(--border-medium)",
              borderRadius: 5,
              background: "var(--bg-card)",
              color: "var(--text-primary)",
              fontSize: 11,
              outline: "none",
            }}
          />
          {query && (
            <button
              type="button"
              aria-label={t("common.clear")}
              title={t("common.clear")}
              onClick={() => {
                setQuery("");
                searchRef.current?.focus();
              }}
              style={{
                position: "absolute",
                right: 3,
                top: "50%",
                transform: "translateY(-50%)",
                border: "none",
                background: "transparent",
                color: "var(--text-muted)",
                cursor: "pointer",
                padding: 2,
                display: "flex",
              }}
            >
              <X size={10} />
            </button>
          )}
        </div>
        <button
          type="button"
          aria-label={allExpanded ? t("notebook.collapseAll") : t("notebook.expandAll")}
          title={allExpanded ? t("notebook.collapseAll") : t("notebook.expandAll")}
          onClick={toggleAll}
          disabled={Boolean(matched)}
          style={{
            border: "none",
            background: "transparent",
            color: "var(--text-muted)",
            cursor: matched ? "not-allowed" : "pointer",
            opacity: matched ? 0.45 : 1,
            padding: 3,
            display: "flex",
            flexShrink: 0,
          }}
        >
          {allExpanded ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
        </button>
      </div>

      <div
        style={{
          padding: "5px 8px",
          fontSize: 10,
          color: "var(--text-muted)",
          textTransform: "uppercase",
          letterSpacing: 0.4,
        }}
      >
        {matched
          ? t("notebook.outlineHits", { count: String(matched.size) })
          : dragEnabled
            ? t("notebook.outlineDragHint")
            : t("notebook.outlineSections")}
      </div>

      {items.length === 0 ? (
        <div style={{ padding: "4px 10px 12px", fontSize: 11, color: "var(--text-hint)" }}>
          {t("notebook.outlineEmpty")}
        </div>
      ) : (
        <div style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "0 4px 10px" }}>
          {items.map((item, index) => {
            if (!visible.has(index)) return null;
            const expanded = matched ? true : !collapsed.has(index);
            const isMatch = matched?.has(index) ?? false;
            const key = String(index);
            const isDragging = drag.draggedNoteId === key;
            const isDropTarget = drag.dragOverNoteId === key && !isDragging;
            return (
              <div
                key={`${item.anchor}-${index}`}
                ref={drag.setNoteItemRef(key)}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 2,
                  // 按层级缩进。封到 4 级 —— 再深就挤没了。
                  paddingLeft: (Math.min(item.level, 4) - 1) * 10,
                  borderTop: isDropTarget ? "2px solid var(--accent)" : "2px solid transparent",
                  opacity: isDragging ? 0.5 : 1,
                }}
              >
                {hasChildren[index] ? (
                  <button
                    type="button"
                    aria-label={expanded ? t("notebook.collapse") : t("notebook.expand")}
                    aria-expanded={expanded}
                    onClick={() => toggleCollapse(index)}
                    disabled={Boolean(matched)}
                    style={{
                      border: "none",
                      background: "transparent",
                      color: "var(--text-muted)",
                      cursor: matched ? "not-allowed" : "pointer",
                      padding: 1,
                      display: "flex",
                      flexShrink: 0,
                    }}
                  >
                    {expanded ? <ChevronDown size={10} /> : <ChevronRight size={10} />}
                  </button>
                ) : (
                  <span style={{ width: 14, flexShrink: 0 }} />
                )}
                <button
                  type="button"
                  onClick={(event) => {
                    // 拖动松手会紧跟一次 click,吞掉它,否则重排完还会顺带跳一次。
                    if (drag.suppressNextClickRef.current) {
                      drag.suppressNextClickRef.current = false;
                      event.preventDefault();
                      return;
                    }
                    onJump(item);
                  }}
                  onPointerDown={
                    dragEnabled ? (event) => drag.onPointerDown(event, key) : undefined
                  }
                  onPointerMove={dragEnabled ? drag.onPointerMove : undefined}
                  onPointerUp={dragEnabled ? drag.onPointerUp : undefined}
                  onPointerCancel={dragEnabled ? drag.onPointerCancel : undefined}
                  title={item.text}
                  style={{
                    flex: 1,
                    minWidth: 0,
                    height: 22,
                    border: "none",
                    borderRadius: 4,
                    background: "transparent",
                    color: isMatch ? "var(--text-primary)" : "var(--text-secondary)",
                    cursor: "pointer",
                    textAlign: "left",
                    padding: "0 4px",
                    fontSize: 11,
                    fontWeight: item.level <= 2 ? 600 : 400,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                    // 触摸设备上不让浏览器把纵向拖当成滚动,否则 pointermove 收不全。
                    touchAction: dragEnabled ? "none" : undefined,
                  }}
                >
                  {highlightText(item.text, needle)}
                </button>
              </div>
            );
          })}
        </div>
      )}
    </aside>
  );
}
