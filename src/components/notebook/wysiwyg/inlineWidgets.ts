/**
 * 几个轻量级 inline widget：
 *
 * - ListMarkerWidget — 有序 / 无序列表的标记符（`1.` / `-` / `*`）渲染
 * - CalloutLabelWidget — Obsidian-style `> [!type]` 顶部彩色标签
 * - ImageWidget — `![alt](url)` 图片预览（URL 走 isAbsoluteSafeUrl 白名单 gate）
 * - TaskCheckbox — `- [ ]` / `- [x]` 替换成方框 / 勾，点击切换
 * - HrWidget — `---` 水平线
 * - TableSepWidget — markdown 表格的对齐分隔行 (`| --- |`) 隐藏成细线
 *
 * 这些 widget 体积都小（≤ 20 行），不挂 listener、不开 async render，无需
 * destroy / AbortController；放一个文件方便集中管理样式与类名常量。
 */

import { EditorView, WidgetType } from "@codemirror/view";

import {
  applyImageElementSizing,
  buildImageMarkdown,
  imageWidthFromTitle,
  parseImageMarkdown,
  setImageMarkdownWidth,
  type ImageParts,
} from "../markdownImages";
import { openEditPopover } from "./editPopover";

export class ListMarkerWidget extends WidgetType {
  constructor(
    private readonly label: string,
    private readonly ordered: boolean,
  ) {
    super();
  }
  eq(other: WidgetType): boolean {
    return (
      other instanceof ListMarkerWidget &&
      other.label === this.label &&
      other.ordered === this.ordered
    );
  }
  toDOM(): HTMLElement {
    const span = document.createElement("span");
    span.className = `cm-md-list-marker${this.ordered ? " ordered" : ""}`;
    span.textContent = this.label;
    return span;
  }
}

/** Allow only safe URL schemes; relative paths fall back to source. */
export function isAbsoluteSafeUrl(url: string): boolean {
  return /^(https?:|data:image\/|file:|asset:|tauri:|markio-asset:|markio-resource:)/i.test(url);
}

// Same canonical names as src/lib/callouts.ts so the in-editor preview matches
// the rendered preview (aliases like `caution` → `warning`).
const CALLOUT_ALIASES: Record<string, string> = {
  hint: "tip",
  important: "important",
  caution: "warning",
  attention: "warning",
  error: "danger",
  check: "success",
  done: "success",
  help: "question",
  faq: "question",
  abstract: "note",
  summary: "note",
  tldr: "note",
};

export function normalizeCalloutType(raw: string): string {
  const lower = raw.toLowerCase();
  return CALLOUT_ALIASES[lower] ?? lower;
}

export class CalloutLabelWidget extends WidgetType {
  constructor(private readonly type: string) {
    super();
  }
  eq(other: WidgetType): boolean {
    return other instanceof CalloutLabelWidget && other.type === this.type;
  }
  toDOM(): HTMLElement {
    const span = document.createElement("span");
    span.className = `cm-md-callout-label cm-md-callout-label-${this.type}`;
    span.textContent = this.type.toUpperCase();
    return span;
  }
}

export class ImageWidget extends WidgetType {
  constructor(
    private readonly parts: ImageParts,
    private readonly inlinePreview: boolean = false,
    private readonly sourceLength: number = 0,
  ) {
    super();
  }
  eq(other: WidgetType): boolean {
    return (
      other instanceof ImageWidget &&
      other.parts.alt === this.parts.alt &&
      other.parts.url === this.parts.url &&
      other.parts.title === this.parts.title &&
      other.inlinePreview === this.inlinePreview &&
      other.sourceLength === this.sourceLength
    );
  }
  toDOM(view: EditorView): HTMLElement {
    const wrap = document.createElement("span");
    wrap.className = "cm-md-img-widget";
    if (this.inlinePreview) wrap.classList.add("cm-md-img-inline-preview");
    wrap.dataset.src = this.parts.url;
    wrap.dataset.alt = this.parts.alt;
    if (this.parts.title) wrap.dataset.title = this.parts.title;
    if (this.sourceLength > 0) wrap.dataset.sourceLength = String(this.sourceLength);
    const img = document.createElement("img");
    img.src = this.parts.url;
    img.alt = this.parts.alt;
    if (this.parts.title) img.title = this.parts.title;
    applyImageElementSizing(img, this.parts.title);
    img.loading = "lazy";
    img.draggable = false;
    img.addEventListener("error", () => {
      wrap.classList.add("cm-md-img-error");
      wrap.title = `图片加载失败：${this.parts.url}`;
    });
    wrap.appendChild(img);

    const edit = document.createElement("button");
    edit.type = "button";
    edit.className = "cm-md-img-edit";
    edit.textContent = "编辑";
    edit.title = "编辑图片（描述 / 地址 / 宽度）";
    wrap.appendChild(edit);

    // 点击图片或「编辑」按钮都打开浮层；阻止冒泡到 CM（否则会移动光标显形原文）
    const open = (e: MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      openImageEditor(view, wrap);
    };
    wrap.addEventListener("mousedown", (e) => {
      e.preventDefault();
      e.stopPropagation();
    });
    wrap.addEventListener("click", open);
    return wrap;
  }
  ignoreEvent() {
    return true;
  }
}

function imageRangeFromHost(
  view: EditorView,
  host: HTMLElement,
): { from: number; to: number; source: string } | null {
  const from = view.posAtDOM(host);
  const len = Number(host.dataset.sourceLength);
  if (from == null || !Number.isFinite(len) || len <= 0) return null;
  const to = Math.min(view.state.doc.length, from + len);
  return to > from ? { from, to, source: view.state.doc.sliceString(from, to) } : null;
}

function openImageEditor(view: EditorView, host: HTMLElement) {
  const range = imageRangeFromHost(view, host);
  if (!range) return;
  const parts = parseImageMarkdown(range.source) ?? {
    alt: host.dataset.alt ?? "",
    url: host.dataset.src ?? "",
  };
  const width = imageWidthFromTitle(parts.title) ?? "";
  openEditPopover(
    view,
    host,
    [
      { key: "alt", label: "描述", value: parts.alt, placeholder: "alt 文本" },
      { key: "url", label: "地址", value: parts.url, placeholder: "https://…" },
      { key: "width", label: "宽度", value: width, placeholder: "如 480 或 60%" },
    ],
    (v) => {
      const url = v.url!.trim();
      if (!url) return;
      let next = buildImageMarkdown({ alt: v.alt ?? "", url, title: parts.title });
      next = setImageMarkdownWidth(next, v.width!.trim() || null) ?? next;
      if (next !== range.source) {
        view.dispatch({
          changes: { from: range.from, to: range.to, insert: next },
          userEvent: "input",
        });
      }
    },
  );
}

export class TaskCheckbox extends WidgetType {
  constructor(private readonly checked: boolean) {
    super();
  }
  eq(other: WidgetType): boolean {
    return other instanceof TaskCheckbox && other.checked === this.checked;
  }
  toDOM(): HTMLElement {
    const el = document.createElement("span");
    el.className = "cm-md-task " + (this.checked ? "checked" : "");
    el.setAttribute("role", "checkbox");
    el.setAttribute("aria-checked", String(this.checked));
    el.setAttribute("aria-label", this.checked ? "标记为未完成" : "标记为完成");
    return el;
  }
  ignoreEvent() {
    return false;
  }
}

export class HrWidget extends WidgetType {
  toDOM() {
    const el = document.createElement("span");
    el.className = "cm-md-hr-line";
    return el;
  }
  eq() {
    return true;
  }
}

export class TableSepWidget extends WidgetType {
  toDOM() {
    const el = document.createElement("span");
    el.className = "cm-md-table-sep";
    return el;
  }
  eq() {
    return true;
  }
}
