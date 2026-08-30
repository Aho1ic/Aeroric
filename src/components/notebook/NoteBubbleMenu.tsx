/* 选区浮动气泡:拖选一段文字后贴在选区上方的一排格式化按钮。
 *
 * ## 为什么是重写而不是移植
 *
 * Markio 里这个功能**不存在**。`popovers.css` 有完整的 `.bubble` 样式、
 * `settings.ts` 有 `bubbleTrigger: "selection" | "rightClick"`、设置页也画了那张卡片,
 * 但 `bubbleTrigger` 全仓库没有一处读它,`shortcutStyle` 的 `"bubble"` / `"toolbar"`
 * 两档同样没有分支消费(只有 `"all" | "slash"` 被 `allowSlash` 读)。渲染 `.bubble`
 * 的 JSX 没有。也就是说那边是删掉实现之后留下的样式、设置和一句过时注释。
 *
 * 所以这里按 Aeroric 自己的情况实现:
 * - 只做**拖选触发**。Aeroric 已经有一份带格式化项的右键菜单(`NoteContextMenu`),
 *   Markio 那个 `rightClick` 档要么替掉它、要么和它打架,而拖选触发是 Typora /
 *   Notion / Obsidian 的通行做法,和右键菜单天然共存。
 * - 命令全部复用 `useNoteFormatting`,不新写文本变换 —— 那一层已经被工具栏和右键
 *   菜单验过,再抄一份就是两套「加粗」实现。
 *
 * ## 焦点
 *
 * 按钮一律走 `onMouseDown` + `preventDefault`,不用 `onClick`。`click` 之前的
 * `mousedown` 会把焦点从编辑器抢走,而失焦时选区就没了 —— 等 `click` 到达时要格式化
 * 的那段已经不再是选区,命令落到一个空选区上什么都不做。
 */

import { useEffect, useRef } from "react";
import {
  Bold,
  Braces,
  Code,
  Highlighter,
  Italic,
  Link2,
  List,
  Quote,
  Strikethrough,
  Underline,
} from "lucide-react";
import { zLayers } from "../../styles/zLayers";

/** 气泡能发出的动作。文本变换全部在面板侧用 `useNoteFormatting` 做。 */
export type BubbleAction =
  | "bold"
  | "italic"
  | "underline"
  | "strike"
  | "highlight"
  | "inlineCode"
  | "link"
  | "quote"
  | "bullet"
  | "codeBlock";

/** 选区的视口矩形。`NoteEditorHandle.selectionRect()` 给的那个。 */
export type BubbleAnchor = { left: number; right: number; top: number; bottom: number };

export type NoteBubbleMenuProps = {
  anchor: BubbleAnchor;
  onAction: (action: BubbleAction) => void;
  onDismiss: () => void;
  t: (key: string) => string;
};

const WIDTH = 336;
const HEIGHT = 34;
const GAP = 8;
const EDGE = 8;

type Item = { action: BubbleAction; labelKey: string; Icon: typeof Bold };

/* 分四组:行内强调 / 行内标记 / 链接 / 块级。顺序按使用频率,加粗在最左边 ——
   那是唯一一个用户会不看图标直接点的。 */
const GROUPS: Item[][] = [
  [
    { action: "bold", labelKey: "notebook.bold", Icon: Bold },
    { action: "italic", labelKey: "notebook.italic", Icon: Italic },
    { action: "underline", labelKey: "notebook.underline", Icon: Underline },
    { action: "strike", labelKey: "notebook.strike", Icon: Strikethrough },
  ],
  [
    { action: "highlight", labelKey: "notebook.highlight", Icon: Highlighter },
    { action: "inlineCode", labelKey: "notebook.bubbleInlineCode", Icon: Code },
  ],
  [{ action: "link", labelKey: "notebook.bubbleLink", Icon: Link2 }],
  [
    { action: "quote", labelKey: "notebook.slashQuote", Icon: Quote },
    { action: "bullet", labelKey: "notebook.bulletList", Icon: List },
    { action: "codeBlock", labelKey: "notebook.codeBlock", Icon: Braces },
  ],
];

export function NoteBubbleMenu({ anchor, onAction, onDismiss, t }: NoteBubbleMenuProps) {
  const rootRef = useRef<HTMLDivElement | null>(null);

  /* 点气泡外面收起。按钮自己走 `mouseDown` + `preventDefault`,不会冒到这里。
     用捕获阶段:编辑器上的 `mousedown` 会先改选区,那之后再判就晚了。 */
  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      if (!(event.target instanceof Node)) return;
      if (rootRef.current?.contains(event.target)) return;
      onDismiss();
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => document.removeEventListener("pointerdown", onPointerDown, true);
  }, [onDismiss]);

  /* 水平居中在选区上,再夹进视口。夹的是**气泡左边缘**,不是中心 —— 靠边的选区上
     只夹中心会让气泡有一半在屏幕外。 */
  const center = (anchor.left + anchor.right) / 2;
  const left = Math.max(EDGE, Math.min(center - WIDTH / 2, window.innerWidth - WIDTH - EDGE));
  /* 上方放不下就翻到选区下方。选区在第一行时 `top - HEIGHT - GAP` 会是负数,气泡的
     上半截会被窗口切掉,而那正好是按钮所在的地方。 */
  const above = anchor.top - HEIGHT - GAP;
  const flipDown = above < EDGE;
  const top = flipDown ? anchor.bottom + GAP : above;

  return (
    <div
      ref={rootRef}
      role="toolbar"
      aria-label={t("notebook.bubbleMenu")}
      aria-orientation="horizontal"
      style={{
        position: "fixed",
        left,
        top,
        height: HEIGHT,
        display: "flex",
        alignItems: "center",
        gap: 1,
        padding: "0 4px",
        zIndex: zLayers.dropdownInline,
        background: "var(--bg-panel)",
        border: "1px solid var(--border)",
        borderRadius: 8,
        boxShadow: "0 6px 20px rgba(0, 0, 0, 0.26)",
      }}
    >
      {GROUPS.map((group, groupIndex) => (
        <div key={groupIndex} style={{ display: "flex", alignItems: "center", gap: 1 }}>
          {groupIndex > 0 && (
            <span
              aria-hidden="true"
              style={{ width: 1, height: 18, margin: "0 3px", background: "var(--border-dim)" }}
            />
          )}
          {group.map(({ action, labelKey, Icon }) => (
            <button
              key={action}
              type="button"
              aria-label={t(labelKey)}
              title={t(labelKey)}
              onMouseDown={(event) => {
                // 必须挡掉:默认行为会移走焦点,而失焦时选区就没了。
                event.preventDefault();
                onAction(action);
              }}
              style={{
                width: 26,
                height: 26,
                display: "inline-flex",
                alignItems: "center",
                justifyContent: "center",
                border: "none",
                borderRadius: 6,
                background: "transparent",
                color: "var(--text-secondary)",
                cursor: "pointer",
              }}
            >
              <Icon size={13} aria-hidden="true" />
            </button>
          ))}
        </div>
      ))}
    </div>
  );
}
