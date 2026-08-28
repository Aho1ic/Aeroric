/**
 * 围栏代码块 widget。
 *
 * 渲染思路：toDOM 先用 escape 后的明文填 <code>，然后 highlightCode (lazy)
 * 拉到 grammar 后回填到同一个 <code>。CodeFenceWidget.eq 比对 source/lang，
 * 内容不变时 CodeMirror 复用 DOM，高亮一直保留。
 *
 * 交互：langInput / edit button / body click 都 install 了 listener；widget
 * destroy 时统一 cleanup（installCodeFenceDomHandlers 返回的 fn）。
 */

import { EditorView, WidgetType } from "@codemirror/view";
import DOMPurify from "dompurify";

import { highlightCode } from "./highlight";

type Cleanup = () => void;

function codeLines(raw: string): string[] {
  const withoutFinalNewline = raw.endsWith("\n") ? raw.slice(0, -1) : raw;
  return withoutFinalNewline.length === 0 ? [""] : withoutFinalNewline.split("\n");
}

function safeLanguageClass(lang: string): string {
  return lang
    .trim()
    .toLowerCase()
    .replace(/[^\w-]/g, "");
}

function codeFenceRangeFromHost(
  view: EditorView,
  host: HTMLElement,
): { from: number; to: number; source: string } | null {
  const from = view.posAtDOM(host);
  const len = Number(host.dataset.sourceLength);
  if (from == null || !Number.isFinite(len) || len <= 0) return null;
  const to = Math.min(view.state.doc.length, from + len);
  if (to <= from) return null;
  return { from, to, source: view.state.doc.sliceString(from, to) };
}

function fencedBodyRangeFromSource(
  from: number,
  source: string,
): { from: number; to: number } | null {
  const firstBreak = source.search(/\r?\n/);
  if (firstBreak < 0) return null;
  const breakLength = source[firstBreak] === "\r" ? 2 : 1;
  const bodyStart = from + firstBreak + breakLength;
  const closing = source.match(/\r?\n[ \t]*(`{3,}|~{3,})\s*$/);
  const bodyEnd = closing ? from + source.length - closing[0].length : from + source.length;
  return { from: bodyStart, to: Math.max(bodyStart, bodyEnd) };
}

function commitCodeFenceLang(view: EditorView, host: HTMLElement, value: string): boolean {
  const range = codeFenceRangeFromHost(view, host);
  if (!range) return false;
  const firstLine = view.state.doc.lineAt(range.from);
  const marker = firstLine.text.match(/^(\s*(`{3,}|~{3,}))[ \t]*/);
  if (!marker) return false;
  const lang = value.trim();
  const from = firstLine.from + marker[0].length;
  const to = firstLine.to;
  const current = view.state.doc.sliceString(from, to);
  if (current === lang) return false;
  view.dispatch({
    changes: { from, to, insert: lang },
    userEvent: "input",
  });
  return true;
}

/** 删除整段围栏代码块（含语言行与结束 ```）。给所见即所得里「卡在 ``` 里的
 *  表格 / 误并进围栏的内容」一个直接删除入口——widget 吞事件，没法拖选删。 */
function deleteCodeFence(view: EditorView, host: HTMLElement): boolean {
  const range = codeFenceRangeFromHost(view, host);
  if (!range) return false;
  const doc = view.state.doc;
  let to = range.to;
  if (to < doc.length && doc.sliceString(to, to + 1) === "\n") to += 1;
  view.dispatch({
    changes: { from: range.from, to, insert: "" },
    selection: { anchor: range.from },
    userEvent: "delete",
  });
  view.focus();
  return true;
}

function commitCodeFenceBody(view: EditorView, host: HTMLElement, value: string): boolean {
  const range = codeFenceRangeFromHost(view, host);
  if (!range) return false;
  const body = fencedBodyRangeFromSource(range.from, range.source);
  if (!body) return false;
  const next = value.replace(/\r\n/g, "\n").replace(/\n$/, "");
  const current = view.state.doc.sliceString(body.from, body.to);
  if (current === next) return false;
  view.dispatch({
    changes: { from: body.from, to: body.to, insert: next },
    userEvent: "input",
  });
  return true;
}

function resizeCodeFenceTextarea(textarea: HTMLTextAreaElement) {
  // 贴合内容高度（单行命令不再撑出一大块空白），只保留一行的最小高度
  textarea.style.height = "auto";
  textarea.style.height = `${Math.max(24, textarea.scrollHeight)}px`;
}

/** 把行号槽刷成 count 行（多退少补），让编辑态行号跟着输入实时增减。 */
function setGutterLines(gutter: HTMLElement, count: number) {
  const n = Math.max(1, count);
  while (gutter.childElementCount > n) gutter.lastElementChild?.remove();
  while (gutter.childElementCount < n) gutter.appendChild(document.createElement("span"));
  for (let i = 0; i < n; i++) {
    (gutter.children[i] as HTMLElement).textContent = String(i + 1);
  }
}

function startCodeFenceBodyEdit(view: EditorView, host: HTMLElement, source: string) {
  const body = host.querySelector<HTMLElement>(".cm-md-code-body");
  if (!body || body.querySelector(".cm-md-code-editor")) return;
  const pre = body.querySelector<HTMLElement>(".cm-md-code-pre");
  const gutter = body.querySelector<HTMLElement>(".cm-md-code-gutter");

  const textarea = document.createElement("textarea");
  textarea.className = "cm-md-code-editor";
  textarea.spellcheck = false;
  textarea.value = source;
  textarea.setAttribute("aria-label", "编辑代码块内容");

  // 高度贴合内容 + 行号随输入实时同步（保留行号槽，编辑态不再丢行号/左移）
  const sync = () => {
    resizeCodeFenceTextarea(textarea);
    if (gutter) setGutterLines(gutter, textarea.value.split(/\r?\n/).length);
  };
  const commit = () => {
    commitCodeFenceBody(view, host, textarea.value);
  };
  textarea.addEventListener("mousedown", (event) => event.stopPropagation());
  textarea.addEventListener("click", (event) => event.stopPropagation());
  textarea.addEventListener("input", sync);
  textarea.addEventListener("blur", commit);
  textarea.addEventListener("keydown", (event) => {
    event.stopPropagation();
    if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
      event.preventDefault();
      textarea.blur();
    } else if (event.key === "Escape") {
      event.preventDefault();
      textarea.value = source;
      textarea.blur();
    }
  });

  // 只替换 <pre>，保留左侧行号槽 —— 编辑态布局与渲染态一致，不丢行号、不左移
  if (pre) pre.replaceWith(textarea);
  else body.replaceChildren(textarea);
  sync();
  textarea.focus({ preventScroll: true });
}

/** 把所有挂上去的 listener 攒到 cleanups，返回一个统一拆除函数；widget destroy
 *  时调用，避免长会话 + 大文档累积 widget 时事件闭包持有 view / source 引用。 */
function installCodeFenceDomHandlers(view: EditorView, host: HTMLElement, source: string): Cleanup {
  const cleanups: Cleanup[] = [];
  const on = <K extends keyof HTMLElementEventMap>(
    target: HTMLElement,
    event: K,
    handler: (e: HTMLElementEventMap[K]) => void,
  ) => {
    target.addEventListener(event, handler);
    cleanups.push(() => target.removeEventListener(event, handler));
  };

  const input = host.querySelector<HTMLInputElement>(".cm-md-code-lang-input");
  const edit = host.querySelector<HTMLButtonElement>(".cm-md-code-edit");
  const del = host.querySelector<HTMLButtonElement>(".cm-md-code-delete");
  const body = host.querySelector<HTMLElement>(".cm-md-code-body");

  if (input) {
    on(input, "mousedown", (event) => event.stopPropagation());
    on(input, "click", (event) => event.stopPropagation());
    on(input, "blur", () => commitCodeFenceLang(view, host, input.value));
    on(input, "keydown", (event) => {
      event.stopPropagation();
      if (event.isComposing || event.keyCode === 229) return;
      if (event.key === "Enter") {
        event.preventDefault();
        input.blur();
      } else if (event.key === "Escape") {
        event.preventDefault();
        input.value = host.dataset.lang ?? "";
        input.blur();
      }
    });
  }

  if (edit) {
    on(edit, "mousedown", (event) => {
      event.preventDefault();
      event.stopPropagation();
    });
    on(edit, "click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      startCodeFenceBodyEdit(view, host, source);
    });
  }

  if (del) {
    on(del, "mousedown", (event) => {
      event.preventDefault();
      event.stopPropagation();
    });
    on(del, "click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      deleteCodeFence(view, host);
    });
  }

  if (body) {
    on(body, "mousedown", (event) => {
      if (event.target instanceof HTMLTextAreaElement) return;
      event.preventDefault();
      event.stopPropagation();
      startCodeFenceBodyEdit(view, host, source);
    });
  }

  return () => {
    for (const fn of cleanups) fn();
    cleanups.length = 0;
  };
}

export class CodeFenceWidget extends WidgetType {
  /** 当前 DOM 上的 listener 拆除函数；destroy 时调用，避免 widget 失效后
   *  闭包持有 view / source / handler 延长 GC。 */
  private cleanup: Cleanup | null = null;
  /** 留着 view / host 引用，destroy 时尝试把 textarea 内未提交的草稿写回 doc，
   *  挽救"外部 docChanged 把 widget 替换、用户正在 textarea 里写的几行字
   *  随旧 DOM 一起消失"的边角场景。 */
  private view: EditorView | null = null;
  private host: HTMLElement | null = null;
  constructor(
    private readonly lang: string,
    private readonly source: string,
    private readonly sourceLength: number,
  ) {
    super();
  }
  /** 给 CM6 一个接近真实的块高估计：标题栏 + 每行代码约 21px + 体内边距。
   *  off-screen widget 用它建高度图，减少快速滚动时的错位空白；上屏后改用实测高。 */
  get estimatedHeight(): number {
    const lines = Math.max(1, this.source.split("\n").length);
    return 34 + lines * 21 + 24;
  }
  eq(other: WidgetType): boolean {
    return (
      other instanceof CodeFenceWidget &&
      other.lang === this.lang &&
      other.source === this.source &&
      other.sourceLength === this.sourceLength
    );
  }
  toDOM(view: EditorView): HTMLElement {
    const lang = this.lang.trim();
    const safeLang = safeLanguageClass(lang);
    const figure = document.createElement("figure");
    figure.className = "cm-md-code-widget";
    figure.dataset.lang = lang;
    figure.dataset.sourceLength = String(this.sourceLength);

    const head = document.createElement("figcaption");
    head.className = "cm-md-code-head";

    const input = document.createElement("input");
    input.className = "cm-md-code-lang-input";
    input.value = lang;
    input.placeholder = "plain text";
    input.spellcheck = false;
    input.setAttribute("aria-label", "代码块语言");

    const edit = document.createElement("button");
    edit.type = "button";
    edit.className = "cm-md-code-edit";
    edit.textContent = "编辑";
    edit.setAttribute("aria-label", "编辑代码块内容");

    const del = document.createElement("button");
    del.type = "button";
    del.className = "cm-md-code-delete";
    del.textContent = "删除";
    del.setAttribute("aria-label", "删除代码块");

    head.append(input, edit, del);

    const body = document.createElement("div");
    body.className = "cm-md-code-body";
    body.title = "点击编辑代码内容";

    const gutter = document.createElement("div");
    gutter.className = "cm-md-code-gutter";
    codeLines(this.source).forEach((_, index) => {
      const line = document.createElement("span");
      line.textContent = String(index + 1);
      gutter.append(line);
    });

    const pre = document.createElement("pre");
    pre.className = "cm-md-code-pre";
    const code = document.createElement("code");
    code.className = `hljs${safeLang ? ` language-${safeLang}` : ""}`;
    // 初始直接用 textContent 铺明文：比 escape + DOMPurify 便宜得多，且天然安全。
    // 语法高亮推到空闲帧再回填——否则快速滚动进入代码密集区时，多块代码在同一渲染
    // 周期里同步高亮会抢占主线程，拖住 CM6 的视口绘制，表现为"滚动后大片空白要过
    // 一会才渲染出来"。grammar 拉完后回填到同一个 <code>（eq 比对 source/lang，
    // 内容没变 DOM 被 CodeMirror 复用，回填后高亮保留）。
    const sourceForHighlight = this.source;
    code.textContent = sourceForHighlight;
    const runHighlight = () => {
      void highlightCode(sourceForHighlight, lang).then((html) => {
        if (!code.isConnected) return;
        code.innerHTML = DOMPurify.sanitize(html, { USE_PROFILES: { html: true } });
      });
    };
    if (typeof requestIdleCallback === "function") {
      requestIdleCallback(runHighlight, { timeout: 1200 });
    } else {
      setTimeout(runHighlight, 48);
    }
    pre.append(code);
    body.append(gutter, pre);

    figure.append(head, body);
    this.view = view;
    this.host = figure;
    this.cleanup = installCodeFenceDomHandlers(view, figure, this.source);
    return figure;
  }
  ignoreEvent() {
    return true;
  }
  destroy() {
    // 顺序：先尝试 commit 草稿（commit 内会再 dispatch，触发新一轮 build；新
    // widget 在 toDOM 时 source 已是最新值），再拆 listener / 解引用。
    flushPendingCodeFenceEdit(this.view, this.host);
    this.cleanup?.();
    this.cleanup = null;
    this.view = null;
    this.host = null;
  }
}

/** widget destroy 时挽救正在编辑的 textarea：如果 host 内还有未 commit 的
 *  textarea，且 view.posAtDOM(host) 仍能定位到原始 source 位置，就把 textarea
 *  value 写回 doc。posAtDOM 在 detached host 上可能返回 null，commit 会 noop。 */
function flushPendingCodeFenceEdit(view: EditorView | null, host: HTMLElement | null) {
  if (!view || !host) return;
  const textarea = host.querySelector<HTMLTextAreaElement>(".cm-md-code-editor");
  if (!textarea) return;
  try {
    commitCodeFenceBody(view, host, textarea.value);
  } catch {
    // view 已 destroy / host 已 detach 时 dispatch 会抛，吞掉以免影响 destroy
  }
}
