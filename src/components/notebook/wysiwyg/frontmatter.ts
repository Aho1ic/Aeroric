/**
 * YAML frontmatter 块 widget —— 就地属性编辑器。
 *
 * 文档开头的 `---\n…\n---` 在 WYSIWYG 下渲染成一张可编辑属性表（类似表格
 * widget）：每个字段的值就地改，回车 / 失焦把整块 frontmatter 重新序列化写回
 * 源码。提交会把 YAML 规范化（引号 / 列表写法可能变，但语义不变）。
 *
 * 只处理三种常见结构：`key: value`、`key:` + 缩进 `- item` 列表、折叠/字面
 * 标量（`>-` `|` 等）的缩进续行。遇到无法归类的行时 parse().ok = false，由
 * build.ts 退回原始 YAML 文本编辑，避免就地编辑丢数据。
 */

import { EditorView, WidgetType } from "@codemirror/view";

function unquote(s: string): string {
  return s.trim().replace(/^["']|["']$/g, "");
}

export type FmKind = "scalar" | "list";
export interface FmProp {
  key: string;
  kind: FmKind;
  value: string;
  items: string[];
}
export interface ParsedFrontmatter {
  ok: boolean;
  props: FmProp[];
}

export function parseFrontmatter(src: string): ParsedFrontmatter {
  let lines = src.replace(/\r\n/g, "\n").split("\n");
  if (lines[0]?.trim() === "---") lines = lines.slice(1);
  const closeIdx = lines.findIndex((l) => l.trim() === "---");
  if (closeIdx >= 0) lines = lines.slice(0, closeIdx);

  const props: FmProp[] = [];
  let ok = true;
  for (const raw of lines) {
    const line = raw.replace(/\s+$/, "");
    if (!line.trim()) continue;
    const indented = /^[ \t]/.test(line);
    const trimmed = line.replace(/^\s+/, "");

    const li = trimmed.match(/^-\s+(.*)$/);
    if (li && props.length) {
      const p = props[props.length - 1]!;
      p.kind = "list";
      p.items.push(unquote(li[1]!));
      continue;
    }
    if (!indented) {
      const idx = trimmed.indexOf(":");
      if (idx > 0) {
        const key = trimmed.slice(0, idx).trim();
        if (/^[\w-]+$/.test(key)) {
          const rawVal = trimmed.slice(idx + 1).trim();
          const folded = [">-", ">", "|", "|-", "|+"].includes(rawVal);
          // 多行块标量（| / >）的换行无法在「就地编辑」widget 里保留——commit 会
          // 把整块拼成单行丢掉换行。标记 ok=false 让整段 frontmatter 退回原始文本
          // 编辑，避免改其它字段时连带毁掉多行块。
          if (folded) ok = false;
          props.push({
            key,
            kind: "scalar",
            value: folded ? "" : unquote(rawVal),
            items: [],
          });
          continue;
        }
      }
    }
    // 缩进续行（折叠标量）拼到最近 scalar 的值
    const last = props[props.length - 1];
    if (last && last.kind === "scalar") {
      last.value = last.value ? `${last.value} ${trimmed}` : trimmed;
      continue;
    }
    ok = false;
  }
  return { ok, props };
}

function needsQuote(v: string): boolean {
  if (v === "") return true;
  if (/[:#]/.test(v)) return true;
  if (/^[>|[\]{}*&!@`'"%,?-]/.test(v)) return true;
  if (/^\s|\s$/.test(v)) return true;
  return false;
}
function quote(v: string): string {
  return `'${v.replace(/'/g, "''")}'`;
}
function emit(v: string): string {
  return needsQuote(v) ? quote(v) : v;
}

export function buildFrontmatter(props: FmProp[]): string {
  const out: string[] = ["---"];
  for (const p of props) {
    const key = p.key.trim();
    if (!key) continue;
    if (p.kind === "list") {
      out.push(`${key}:`);
      for (const it of p.items) {
        const t = it.trim();
        if (t) out.push(`  - ${emit(t)}`);
      }
    } else {
      const v = p.value.trim();
      out.push(v ? `${key}: ${emit(v)}` : `${key}:`);
    }
  }
  out.push("---");
  return `${out.join("\n")}\n`;
}

function frontmatterRange(
  view: EditorView,
  host: HTMLElement,
): { from: number; to: number; source: string } | null {
  const from = view.posAtDOM(host);
  const len = Number(host.dataset.sourceLength);
  if (from == null || !Number.isFinite(len) || len <= 0) return null;
  const to = Math.min(view.state.doc.length, from + len);
  return to > from ? { from, to, source: view.state.doc.sliceString(from, to) } : null;
}

/** 从 DOM 收集各行最新值 → 重建 props → 序列化 → 写回源码。 */
function commitFrontmatter(view: EditorView, host: HTMLElement): boolean {
  const range = frontmatterRange(view, host);
  if (!range) return false;
  const rows = Array.from(host.querySelectorAll<HTMLElement>(".cm-md-fm-row"));
  const props: FmProp[] = rows.map((row) => {
    const key = row.dataset.key ?? "";
    const kind = (row.dataset.kind as FmKind) ?? "scalar";
    const editor = row.querySelector<HTMLTextAreaElement>(".cm-md-fm-value");
    const value = editor?.value ?? "";
    if (kind === "list") {
      return {
        key,
        kind,
        value: "",
        items: value
          .split(",")
          .map((s) => s.trim())
          .filter(Boolean),
      };
    }
    return { key, kind, value: value.trim(), items: [] };
  });
  const next = buildFrontmatter(props);
  if (next === range.source) return false;
  view.dispatch({
    changes: { from: range.from, to: range.to, insert: next },
    userEvent: "input",
  });
  return true;
}

function resizeValueEditor(el: HTMLTextAreaElement) {
  el.style.height = "auto";
  el.style.height = `${Math.max(22, el.scrollHeight)}px`;
}

export class FrontmatterWidget extends WidgetType {
  constructor(private readonly source: string) {
    super();
  }
  /** 属性表块高估计：约每行 30px + 边距。 */
  get estimatedHeight(): number {
    const lines = Math.max(2, this.source.split("\n").length);
    return lines * 30 + 16;
  }
  eq(other: WidgetType): boolean {
    return other instanceof FrontmatterWidget && other.source === this.source;
  }
  toDOM(view: EditorView): HTMLElement {
    const wrap = document.createElement("div");
    wrap.className = "cm-md-frontmatter-widget";
    wrap.setAttribute("contenteditable", "false");
    wrap.dataset.sourceLength = String(this.source.length);

    const { props } = parseFrontmatter(this.source);
    const table = document.createElement("table");
    const tbody = document.createElement("tbody");
    for (const p of props) {
      const tr = document.createElement("tr");
      tr.className = "cm-md-fm-row";
      tr.dataset.key = p.key;
      tr.dataset.kind = p.kind;

      const th = document.createElement("th");
      th.textContent = p.key;
      const td = document.createElement("td");
      const editor = document.createElement("textarea");
      editor.className = "cm-md-fm-value";
      editor.spellcheck = false;
      editor.rows = 1;
      editor.value = p.kind === "list" ? p.items.join(", ") : p.value;
      editor.setAttribute("aria-label", p.kind === "list" ? `${p.key}（逗号分隔多项）` : p.key);
      if (p.kind === "list") editor.placeholder = "用逗号分隔多项";
      td.append(editor);
      tr.append(th, td);
      tbody.append(tr);
    }
    table.append(tbody);
    wrap.append(table);

    // cell 编辑提交：Enter / 失焦写回；Esc 还原
    wrap.addEventListener("focusin", (e) => {
      const cell = (e.target as HTMLElement)?.closest<HTMLTextAreaElement>(".cm-md-fm-value");
      if (cell) resizeValueEditor(cell);
      e.stopPropagation();
    });
    wrap.addEventListener("input", (e) => {
      const cell = (e.target as HTMLElement)?.closest<HTMLTextAreaElement>(".cm-md-fm-value");
      if (cell) resizeValueEditor(cell);
      e.stopPropagation();
    });
    wrap.addEventListener("focusout", (e) => {
      const cell = (e.target as HTMLElement)?.closest(".cm-md-fm-value");
      if (cell) commitFrontmatter(view, wrap);
      e.stopPropagation();
    });
    wrap.addEventListener("keydown", (e) => {
      const cell = (e.target as HTMLElement)?.closest<HTMLTextAreaElement>(".cm-md-fm-value");
      if (!cell) return;
      // IME 组字中的 Enter/Escape 是候选词确认/取消，别当成提交/失焦
      if (e.isComposing || e.keyCode === 229) return;
      if (e.key === "Enter") {
        e.preventDefault();
        e.stopPropagation();
        commitFrontmatter(view, wrap);
        cell.blur();
      } else if (e.key === "Escape") {
        e.stopPropagation();
        cell.blur();
      }
    });
    return wrap;
  }
  ignoreEvent(): boolean {
    return true;
  }
}
