/* 给一条笔记挑图标。
 *
 * popover 而不是整面 sheet:它是个小网格,和右键菜单同源(菜单点开它,位置也跟着
 * 鼠标)。关掉复用面板那个 outside-click 监听 —— 它认 `data-notebook-context-menu`
 * 属性,所以这里也带上。
 *
 * 图标名到组件的映射只在这里。`noteIcons.ts` 那边是纯字符串,免得纯逻辑模块拖上
 * 一整个图标库的依赖(测试里也就不必渲染 SVG)。
 */

import {
  Archive,
  Book,
  Calendar,
  CheckSquare,
  Clock,
  Cloud,
  Code,
  Database,
  Flame,
  Hash,
  Image,
  Lightbulb,
  Link,
  List,
  MessageSquare,
  Palette,
  Sparkles,
  Star,
  StickyNote,
  Table,
  Tag,
  Target,
  X,
  type LucideIcon,
} from "lucide-react";
import { zLayers } from "../../styles/zLayers";
import { NOTE_ICON_NAMES, noteIconLabelKey, type NoteIconName } from "./noteIcons";

/** 图标名 → 组件。缺一个就会在类型上报错(Record 是全量的)。 */
const ICONS: Record<NoteIconName, LucideIcon> = {
  note: StickyNote,
  book: Book,
  calendar: Calendar,
  target: Target,
  checkSquare: CheckSquare,
  list: List,
  table: Table,
  image: Image,
  link: Link,
  tag: Tag,
  hash: Hash,
  lightbulb: Lightbulb,
  palette: Palette,
  archive: Archive,
  database: Database,
  cloud: Cloud,
  sparkle: Sparkles,
  message: MessageSquare,
  code: Code,
  clock: Clock,
  flame: Flame,
  star: Star,
};

/** 取图标组件。给列表行用 —— 它只有名字,不该知道映射表长什么样。 */
export function noteIconComponent(name: NoteIconName): LucideIcon {
  return ICONS[name];
}

export type NoteIconPickerState = {
  x: number;
  y: number;
  /** 要改图标的那条笔记的路径(= 它的 id)。 */
  noteId: string;
};

export type NoteIconPickerProps = {
  state: NoteIconPickerState;
  /** 当前图标。没设过就是 undefined,那时"恢复默认"是禁用的。 */
  current: NoteIconName | undefined;
  onPick: (icon: NoteIconName | null) => void;
  onClose: () => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

export function NoteIconPicker({ state, current, onPick, onClose, t }: NoteIconPickerProps) {
  return (
    <div
      role="dialog"
      aria-label={t("notebook.iconPick")}
      data-notebook-context-menu
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        // 不冒泡:面板外面还有一层 Esc(关面板 / 退出全屏),按一次不该同时
        // 关掉两个东西。
        event.stopPropagation();
        onClose();
      }}
      style={{
        position: "fixed",
        left: state.x,
        top: state.y,
        zIndex: zLayers.contextMenu,
        width: 232,
        padding: 8,
        border: "1px solid var(--border-dim)",
        borderRadius: 8,
        background: "var(--bg-sidebar)",
        boxShadow: "var(--shadow-popover)",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 6 }}>
        <span style={{ flex: 1, fontSize: 12, fontWeight: 600, color: "var(--text-primary)" }}>
          {t("notebook.iconPick")}
        </span>
        <button
          type="button"
          aria-label={t("common.close")}
          title={t("common.close")}
          onClick={onClose}
          style={{
            border: "none",
            background: "transparent",
            color: "var(--text-muted)",
            cursor: "pointer",
            padding: 2,
            display: "flex",
          }}
        >
          <X size={12} />
        </button>
      </div>
      <div
        role="group"
        aria-label={t("notebook.iconPick")}
        style={{ display: "grid", gridTemplateColumns: "repeat(7, 1fr)", gap: 2 }}
      >
        {NOTE_ICON_NAMES.map((name) => {
          const Icon = ICONS[name];
          const active = name === current;
          const label = t(noteIconLabelKey(name));
          return (
            <button
              key={name}
              type="button"
              aria-label={label}
              title={label}
              aria-pressed={active}
              onClick={() => onPick(name)}
              style={{
                height: 28,
                border: active ? "1px solid var(--control-active-bg)" : "1px solid transparent",
                borderRadius: 5,
                background: active ? "var(--control-active-bg)" : "transparent",
                color: active ? "var(--control-active-fg)" : "var(--text-primary)",
                cursor: "pointer",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                padding: 0,
              }}
            >
              <Icon size={15} />
            </button>
          );
        })}
      </div>
      <button
        type="button"
        onClick={() => onPick(null)}
        disabled={current === undefined}
        style={{
          width: "100%",
          height: 24,
          marginTop: 8,
          border: "none",
          borderRadius: 5,
          background: "transparent",
          // 没设过图标时"恢复默认"无事可做,禁用而不是隐藏:按钮消失会让弹窗
          // 高度在设/未设之间跳一下。
          color: current === undefined ? "var(--text-hint)" : "var(--text-muted)",
          cursor: current === undefined ? "default" : "pointer",
          fontSize: 12,
        }}
      >
        {t("notebook.iconReset")}
      </button>
    </div>
  );
}
