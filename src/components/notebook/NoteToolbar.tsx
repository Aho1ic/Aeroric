/* 随手记的格式化工具栏。
 *
 * 从 NotebookPanel 抽出来,JSX 与两个子组件(ToolButton / ColorTool)逐字未改。
 *
 * 工具栏的每个按钮都是「往源码里插 markdown 语法」,不是「切换富文本格式」——
 * 所以没有按下态(pressed),也不要求先有选区。这是富文本编辑器下线后的形态:
 * 光标处直接插,有选区则包裹。
 */

import type React from "react";
import {
  Bold,
  Code2,
  Highlighter,
  Italic,
  List,
  ListOrdered,
  PaintBucket,
  Palette,
  Strikethrough,
  Table2,
  Underline,
} from "lucide-react";

function ToolButton({
  label,
  children,
  onClick,
  onMouseDown,
  pressed,
  disabled = false,
}: {
  label: string;
  children: React.ReactNode;
  onClick: () => void;
  onMouseDown?: (event: React.MouseEvent<HTMLButtonElement>) => void;
  pressed?: boolean;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={typeof pressed === "boolean" ? pressed : undefined}
      title={label}
      disabled={disabled}
      onMouseDown={onMouseDown}
      onClick={onClick}
      style={{
        width: 26,
        height: 26,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        border: "1px solid var(--border-dim)",
        borderRadius: 5,
        background: pressed ? "var(--control-active-bg)" : "var(--bg-card)",
        color: disabled
          ? "var(--text-muted)"
          : pressed
            ? "var(--control-active-fg)"
            : "var(--text-secondary)",
        cursor: disabled ? "not-allowed" : "pointer",
        opacity: disabled ? 0.55 : 1,
        flexShrink: 0,
      }}
    >
      {children}
    </button>
  );
}

function ColorTool({
  label,
  value,
  children,
  onMouseDown,
  onChange,
  disabled = false,
}: {
  label: string;
  value: string;
  children: React.ReactNode;
  onMouseDown?: (event: React.MouseEvent<HTMLLabelElement>) => void;
  onChange: (value: string) => void;
  disabled?: boolean;
}) {
  return (
    <label
      title={label}
      onMouseDown={onMouseDown}
      style={{
        position: "relative",
        width: 34,
        height: 26,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        gap: 2,
        border: "1px solid var(--border-dim)",
        borderRadius: 5,
        background: "var(--bg-card)",
        color: disabled ? "var(--text-muted)" : "var(--text-secondary)",
        cursor: disabled ? "not-allowed" : "pointer",
        opacity: disabled ? 0.55 : 1,
        flexShrink: 0,
      }}
    >
      {children}
      <span
        aria-hidden="true"
        style={{
          width: 8,
          height: 8,
          borderRadius: 2,
          border: "1px solid var(--border-medium)",
          background: value,
        }}
      />
      <input
        type="color"
        aria-label={label}
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(event.currentTarget.value)}
        style={{
          position: "absolute",
          inset: 0,
          opacity: 0,
          cursor: disabled ? "not-allowed" : "pointer",
        }}
      />
    </label>
  );
}

export type NoteToolbarProps = {
  /** 没有笔记时整排按钮置灰。 */
  enabled: boolean;
  /** 行内包裹:加粗 / 斜体 / 下划线 / 删除线 / 高亮 / 颜色。 */
  onInlineWrap: (before: string, after: string) => void;
  /** 行前缀:标题层级。 */
  onHeading: (prefix: string) => void;
  onList: (ordered: boolean) => void;
  onBodyText: () => void;
  onCodeBlock: () => void;
  onTable: () => void;
  onClearBackground: () => void;
  textColor: string;
  onTextColorChange: (value: string) => void;
  backgroundColor: string;
  onBackgroundColorChange: (value: string) => void;
  t: (key: string) => string;
};

export function NoteToolbar({
  enabled,
  onInlineWrap,
  onHeading,
  onList,
  onBodyText,
  onCodeBlock,
  onTable,
  onClearBackground,
  textColor,
  onTextColorChange,
  backgroundColor,
  onBackgroundColorChange,
  t,
}: NoteToolbarProps) {
  return (
    <div
      aria-label={t("notebook.markdownToolbar")}
      style={{
        minHeight: 34,
        display: "flex",
        alignItems: "center",
        gap: 5,
        padding: "4px 8px",
        borderBottom: "1px solid var(--border-dim)",
        overflowX: "auto",
        flexShrink: 0,
      }}
    >
      <ToolButton
        label={t("notebook.bold")}
        disabled={!enabled}
        onClick={() => onInlineWrap("**", "**")}
      >
        <Bold size={13} />
      </ToolButton>
      <ToolButton
        label={t("notebook.italic")}
        disabled={!enabled}
        onClick={() => onInlineWrap("*", "*")}
      >
        <Italic size={13} />
      </ToolButton>
      <ToolButton
        label={t("notebook.underline")}
        disabled={!enabled}
        onClick={() => onInlineWrap("<u>", "</u>")}
      >
        <Underline size={13} />
      </ToolButton>
      <ToolButton
        label={t("notebook.strike")}
        disabled={!enabled}
        onClick={() => onInlineWrap("~~", "~~")}
      >
        <Strikethrough size={13} />
      </ToolButton>
      <ToolButton
        label={t("notebook.highlight")}
        disabled={!enabled}
        onClick={() => onInlineWrap("<mark>", "</mark>")}
      >
        <Highlighter size={13} />
      </ToolButton>
      <ColorTool
        label={t("notebook.textColor")}
        value={textColor}
        disabled={!enabled}
        onChange={(value) => {
          onTextColorChange(value);
          onInlineWrap(`<span style="color:${value}">`, "</span>");
        }}
      >
        <Palette size={13} />
      </ColorTool>
      <ColorTool
        label={t("notebook.backgroundColor")}
        value={backgroundColor}
        disabled={!enabled}
        onChange={(value) => {
          onBackgroundColorChange(value);
          onInlineWrap(`<span style="background-color:${value}">`, "</span>");
        }}
      >
        <PaintBucket size={13} />
      </ColorTool>
      <ToolButton label={t("notebook.noColor")} disabled={!enabled} onClick={onClearBackground}>
        Ø
      </ToolButton>
      <ToolButton label={t("notebook.heading")} disabled={!enabled} onClick={() => onHeading("# ")}>
        H1
      </ToolButton>
      <ToolButton
        label={t("notebook.subheading")}
        disabled={!enabled}
        onClick={() => onHeading("## ")}
      >
        H2
      </ToolButton>
      <ToolButton label={t("notebook.bodyText")} disabled={!enabled} onClick={onBodyText}>
        T
      </ToolButton>
      <ToolButton
        label={t("notebook.bulletList")}
        disabled={!enabled}
        onClick={() => onList(false)}
      >
        <List size={13} />
      </ToolButton>
      <ToolButton
        label={t("notebook.numberedList")}
        disabled={!enabled}
        onClick={() => onList(true)}
      >
        <ListOrdered size={13} />
      </ToolButton>
      <ToolButton label={t("notebook.codeBlock")} disabled={!enabled} onClick={onCodeBlock}>
        <Code2 size={13} />
      </ToolButton>
      <div style={{ position: "relative", flexShrink: 0 }}>
        <ToolButton label={t("notebook.table")} disabled={!enabled} onClick={onTable}>
          <Table2 size={13} />
        </ToolButton>
      </div>
    </div>
  );
}
