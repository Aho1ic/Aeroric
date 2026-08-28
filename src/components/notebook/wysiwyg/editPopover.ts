/**
 * 轻量就地编辑浮层。
 *
 * 图片 / wikilink 等 inline widget 点击「编辑」时弹出：在锚点元素附近浮出一个
 * 表单，保存把各字段值回调出去（由调用方写回 markdown 源码）。浮层自管生命
 * 周期——保存 / 取消 / 点击外部 / Esc 都会关闭并清理监听。
 */

import type { EditorView } from "@codemirror/view";

export interface PopoverField {
  key: string;
  label: string;
  value: string;
  placeholder?: string;
  multiline?: boolean;
}

export function openEditPopover(
  view: EditorView,
  anchorEl: HTMLElement,
  fields: PopoverField[],
  onSubmit: (values: Record<string, string>) => void,
): void {
  // 同时只保留一个浮层
  document.querySelectorAll(".cm-md-edit-popover").forEach((el) => el.remove());

  const pop = document.createElement("div");
  pop.className = "cm-md-edit-popover";
  const inputs: Record<string, HTMLInputElement | HTMLTextAreaElement> = {};

  const submit = () => {
    const values: Record<string, string> = {};
    for (const f of fields) values[f.key] = inputs[f.key]!.value;
    close();
    onSubmit(values);
    view.focus();
  };

  const close = () => {
    document.removeEventListener("mousedown", onDocMouseDown, true);
    pop.remove();
  };

  const onDocMouseDown = (e: MouseEvent) => {
    if (!pop.contains(e.target as Node)) close();
  };

  for (const f of fields) {
    const row = document.createElement("label");
    row.className = "cm-md-edit-popover-row";
    const span = document.createElement("span");
    span.textContent = f.label;
    const input = f.multiline
      ? document.createElement("textarea")
      : document.createElement("input");
    if (input instanceof HTMLInputElement) input.type = "text";
    input.className = "cm-md-edit-popover-input";
    input.value = f.value;
    if (f.placeholder) input.placeholder = f.placeholder;
    input.spellcheck = false;
    input.addEventListener("mousedown", (e) => e.stopPropagation());
    input.addEventListener("keydown", (ev) => {
      const e = ev as KeyboardEvent;
      e.stopPropagation();
      if (e.isComposing || e.keyCode === 229) return;
      if (e.key === "Enter" && !(input instanceof HTMLTextAreaElement)) {
        e.preventDefault();
        submit();
      } else if (e.key === "Escape") {
        e.preventDefault();
        close();
        view.focus();
      }
    });
    inputs[f.key] = input;
    row.append(span, input);
    pop.append(row);
  }

  const actions = document.createElement("div");
  actions.className = "cm-md-edit-popover-actions";
  const cancel = document.createElement("button");
  cancel.type = "button";
  cancel.className = "cm-md-edit-popover-btn";
  cancel.textContent = "取消";
  cancel.addEventListener("mousedown", (e) => {
    e.preventDefault();
    e.stopPropagation();
    close();
    view.focus();
  });
  const save = document.createElement("button");
  save.type = "button";
  save.className = "cm-md-edit-popover-btn primary";
  save.textContent = "保存";
  save.addEventListener("mousedown", (e) => {
    e.preventDefault();
    e.stopPropagation();
    submit();
  });
  actions.append(cancel, save);
  pop.append(actions);

  document.body.append(pop);

  // 定位在锚点下方；越界则翻到上方 / 贴边
  const r = anchorEl.getBoundingClientRect();
  const pr = pop.getBoundingClientRect();
  let left = r.left;
  let top = r.bottom + 6;
  if (left + pr.width > window.innerWidth - 8) {
    left = window.innerWidth - pr.width - 8;
  }
  if (top + pr.height > window.innerHeight - 8) {
    top = r.top - pr.height - 6;
  }
  pop.style.left = `${Math.max(8, left)}px`;
  pop.style.top = `${Math.max(8, top)}px`;

  // 延一帧再挂外部点击监听，避免触发本次打开的那次 click
  setTimeout(() => {
    document.addEventListener("mousedown", onDocMouseDown, true);
  }, 0);
  inputs[fields[0]!.key]?.focus();
}
