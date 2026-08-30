/* 编辑器内的触发式菜单:`/` 插入、`[[` 双链、`#` 标签、`@` 提及、`:` emoji。
 *
 * 五种触发共用这一个浮层。它们的差别只在"候选从哪来"和"提交时插什么"——那两件事在
 * 模型层(`noteTriggers.ts` / `noteCompletions.ts` / `noteSlashItems.ts`),这里只画。
 *
 * ## 和命令面板(⌘K)的区别:焦点不动
 *
 * 命令面板有自己的输入框,焦点移进去;这个菜单没有输入框 —— 用户是在**正文里**打字,
 * 查询就是正文的一部分。焦点必须一直留在 CodeMirror 上,否则打字就断了。于是:
 *
 * - 键盘导航由编辑器那边的 keymap 接(`NotebookPanel` 装),这里纯展示。
 * - 「当前选中哪一条」靠 `aria-activedescendant` 告诉读屏软件,不靠 DOM 焦点。
 * - 行上监听 `mouseDown` 并 `preventDefault()`,不用 `click`:`click` 之前的
 *   `mousedown` 会先把焦点从编辑器抢走,而抢走焦点会让菜单关掉 —— 于是 `click`
 *   落到一个已经不存在的节点上,表现是"点候选没反应,只能用键盘"。
 *
 * 这套做法照 `DshTriggerMenu` 抄,它是同一个形状(输入框驱动的 caret 菜单)。
 */

import { useEffect, useRef } from "react";
import type React from "react";

import { zLayers } from "../../styles/zLayers";
import type { MatchSpan } from "./noteCommands";
import type { CompletionItem } from "./noteCompletions";
import type { SlashItem } from "./noteSlashItems";
import type { TriggerKind } from "./noteTriggers";

/** 浮层最大高度。再高就超出编辑器可视区了,而它是绝对定位的,不跟着滚。 */
const MAX_HEIGHT = 260;
/** 浮层宽度。够放"标题 + 文件名"两行而不换行。 */
const WIDTH = 290;
/** 浮层离视口边缘至少留这么多,避免贴边。 */
const EDGE_GAP = 8;

const LIST_ID = "notebook-trigger-menu-list";

function optionId(index: number): string {
  return `${LIST_ID}-option-${index}`;
}

/** 菜单里的一行(两种候选归一成同一个形状,画的时候就不用分叉了)。 */
export type TriggerRow = {
  id: string;
  glyph: string;
  label: string;
  detail?: string;
  spans: readonly MatchSpan[];
};

/** 把插入项折成菜单行。`t` 由调用方给 —— 插入表里存的是 i18n key。 */
export function slashRow(item: SlashItem, t: (key: string) => string): TriggerRow {
  return {
    id: item.id,
    glyph: item.glyph,
    label: t(item.labelKey),
    detail: t(item.hintKey),
    spans: [],
  };
}

/** 把补全项折成菜单行。 */
export function completionRow(item: CompletionItem): TriggerRow {
  return {
    id: item.id,
    glyph: item.glyph ?? "",
    label: item.label,
    detail: item.detail,
    spans: item.spans,
  };
}

/** 各触发种类的标题 i18n key。 */
const TITLE_KEY: Record<TriggerKind, string> = {
  slash: "notebook.slashMenu",
  wiki: "notebook.completionWiki",
  tag: "notebook.completionTag",
  mention: "notebook.completionMention",
  emoji: "notebook.completionEmoji",
};

/** 各触发种类在标题左边显示的徽标。 */
const BADGE: Record<TriggerKind, string> = {
  slash: "/",
  wiki: "[[",
  tag: "#",
  mention: "@",
  emoji: ":",
};

/** 按命中区间把文本切成 <mark> 与普通片段。同 `NoteCommandPalette` 的那份。 */
function Highlighted({ text, spans }: { text: string; spans: readonly MatchSpan[] }) {
  if (spans.length === 0) return <>{text}</>;
  /* 按码点切,不按码元:区间是模型层用 `[...text]` 数出来的,而 `slice` 按码元 ——
     emoji 或生僻字的标题上两者不等,直接 slice 会把代理对切成两半。 */
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

export type NoteTriggerMenuProps = {
  kind: TriggerKind;
  /** 当前查询。只用来在标题上回显,过滤已经在模型层做了。 */
  query: string;
  rows: readonly TriggerRow[];
  /** 选中下标。列表为空时是 -1。 */
  selected: number;
  onSelectedChange: (index: number) => void;
  onPick: (index: number) => void;
  onDismiss: () => void;
  /** 光标的视口坐标(`coordsAtPos` 给的 left / bottom)。菜单挂在它下面。 */
  anchor: { x: number; y: number };
  t: (key: string, vars?: Record<string, string | number>) => string;
};

export function NoteTriggerMenu({
  kind,
  query,
  rows,
  selected,
  onSelectedChange,
  onPick,
  onDismiss,
  anchor,
  t,
}: NoteTriggerMenuProps) {
  const listRef = useRef<HTMLDivElement | null>(null);

  /* 选中项滚进可视区。焦点在编辑器上,浏览器不会替我们滚。 */
  useEffect(() => {
    if (selected < 0) return;
    const node = document.getElementById(optionId(selected));
    // jsdom 没实现 scrollIntoView。同 `DshTriggerMenu` 的处理 —— 滚动是纯视觉。
    if (node && typeof node.scrollIntoView === "function")
      node.scrollIntoView({ block: "nearest" });
  }, [selected]);

  /* 点到菜单外面就收起。不判"点在编辑器上"—— 点编辑器本身就该收(那是把光标挪走),
     而点菜单里的行走 `mouseDown` + `preventDefault`,不会冒到这里。 */
  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      if (!(event.target instanceof Node)) return;
      if (listRef.current?.contains(event.target)) return;
      onDismiss();
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => document.removeEventListener("pointerdown", onPointerDown, true);
  }, [onDismiss]);

  /* 夹进视口。`innerWidth` / `innerHeight` 在 jsdom 里是 1024×768,拿不到真实值时
     结果只是位置不理想,不影响可用性 —— 所以不做能力探测。 */
  const left = Math.max(EDGE_GAP, Math.min(anchor.x, window.innerWidth - WIDTH - EDGE_GAP));
  const spaceBelow = window.innerHeight - anchor.y - EDGE_GAP;
  /* 下面放不下就翻到光标上方。`anchor.y` 是光标行的 bottom,上翻要减去行高 ——
     这里用 18px 近似(编辑器 12.5px / 1.6 行高),差一两像素不影响。 */
  const flipUp = spaceBelow < 120 && anchor.y > spaceBelow;
  const maxHeight = Math.min(MAX_HEIGHT, flipUp ? anchor.y - 18 - EDGE_GAP : spaceBelow);

  return (
    <div
      ref={listRef}
      role="listbox"
      aria-label={t(TITLE_KEY[kind])}
      aria-activedescendant={selected >= 0 ? optionId(selected) : undefined}
      style={{
        position: "fixed",
        left,
        ...(flipUp ? { bottom: window.innerHeight - anchor.y + 18 } : { top: anchor.y + 4 }),
        width: WIDTH,
        maxHeight,
        zIndex: zLayers.dropdownInline,
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
        background: "var(--bg-panel)",
        border: "1px solid var(--border)",
        borderRadius: 6,
        boxShadow: "0 8px 24px rgba(0, 0, 0, 0.28)",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 6,
          padding: "5px 8px",
          borderBottom: "1px solid var(--border-dim)",
          background: "var(--bg-sidebar)",
          fontSize: 11,
          flexShrink: 0,
        }}
      >
        <span
          style={{
            fontFamily: "var(--font-mono)",
            color: "var(--accent)",
            fontWeight: 600,
          }}
        >
          {BADGE[kind]}
        </span>
        <span style={{ fontWeight: 600, color: "var(--text-primary)" }}>{t(TITLE_KEY[kind])}</span>
        {query !== "" && (
          <span style={{ color: "var(--text-muted)", minWidth: 0, overflow: "hidden" }}>
            · {query}
          </span>
        )}
        <span style={{ marginLeft: "auto", fontSize: 10, color: "var(--text-muted)" }}>
          {t("notebook.completionHint")}
        </span>
      </div>
      <div style={{ overflowY: "auto", minHeight: 0 }}>
        {rows.length === 0 ? (
          <div
            style={{
              padding: "14px 10px",
              textAlign: "center",
              fontSize: 11.5,
              color: "var(--text-muted)",
            }}
          >
            {t(kind === "slash" ? "notebook.slashEmpty" : "notebook.completionEmpty")}
          </div>
        ) : (
          rows.map((row, index) => {
            const active = index === selected;
            return (
              <div
                key={row.id}
                id={optionId(index)}
                role="option"
                aria-selected={active}
                onMouseDown={(event) => {
                  // 必须挡掉:默认行为会把焦点从编辑器抢走,而那会先把菜单关掉。
                  event.preventDefault();
                  onPick(index);
                }}
                onMouseEnter={() => onSelectedChange(index)}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  padding: "5px 8px",
                  cursor: "pointer",
                  background: active ? "var(--bg-hover)" : "transparent",
                  borderLeft: `2px solid ${active ? "var(--accent)" : "transparent"}`,
                }}
              >
                <span
                  style={{
                    width: 20,
                    flexShrink: 0,
                    textAlign: "center",
                    fontSize: 12,
                    fontFamily: "var(--font-mono)",
                    color: "var(--text-muted)",
                  }}
                >
                  {row.glyph}
                </span>
                <span style={{ minWidth: 0, flex: 1 }}>
                  <span
                    style={{
                      display: "block",
                      fontSize: 12,
                      color: "var(--text-primary)",
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    <Highlighted text={row.label} spans={row.spans} />
                  </span>
                  {/* 真值判断一道就够:`undefined` 和空串都不该画第二行,而分成
                      `!== undefined && !== ""` 两道时,谁都不是决定性的。 */}
                  {row.detail && (
                    <span
                      style={{
                        display: "block",
                        fontSize: 10.5,
                        color: "var(--text-muted)",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                      }}
                    >
                      {row.detail}
                    </span>
                  )}
                </span>
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}
