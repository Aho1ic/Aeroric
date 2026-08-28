/**
 * KaTeX 块级 / 行内公式 widget。
 *
 * KaTeX 整库 ~256KB，走 lazy import；首个公式才触发拉取。MathWidget 用
 * AbortController 在 destroy 时取消后续写 host —— 避免 widget 已失效但 lazy
 * chunk 才到时浪费 sanitize。
 */

import { WidgetType } from "@codemirror/view";
import DOMPurify from "dompurify";
// 复用 lib/math 的懒加载器：它同时按需拉取 katex CSS（已从 index.css 移除）
import { getKatex } from "../noteVisuals";

// 渲染失败时不让 widget 退化为空白 —— 显示带 ❗ 的灰字，让用户知道写错了。
// signal 来自 widget destroy：如果 widget 在 katex lazy chunk 拉完前就被销毁，
// 跳过后续写 host，避免写到游离 DOM（无副作用但清洁，且省 sanitize 调用）。
function renderKatexInto(
  host: HTMLElement,
  source: string,
  display: boolean,
  signal?: AbortSignal,
) {
  void getKatex()
    .then((katex) => {
      if (signal?.aborted) return;
      try {
        const html = katex.renderToString(source, {
          displayMode: display,
          throwOnError: false,
          strict: "ignore",
          output: "htmlAndMathml",
        });
        if (signal?.aborted) return;
        host.innerHTML = DOMPurify.sanitize(html, {
          USE_PROFILES: { html: true, mathMl: true, svg: true },
        });
      } catch (err) {
        if (signal?.aborted) return;
        host.classList.add("cm-md-math-error");
        host.textContent = `❗ ${(err as Error).message}`;
      }
    })
    .catch((err) => {
      if (signal?.aborted) return;
      host.classList.add("cm-md-math-error");
      host.textContent = `❗ KaTeX 加载失败：${(err as Error).message}`;
    });
}

export class MathWidget extends WidgetType {
  private readonly abort = new AbortController();
  constructor(
    private readonly source: string,
    private readonly display: boolean,
  ) {
    super();
  }
  /** display 公式是块级，给约 58px 估高；行内公式走默认行高（-1）。 */
  get estimatedHeight(): number {
    return this.display ? 58 : -1;
  }
  eq(other: WidgetType): boolean {
    return (
      other instanceof MathWidget && other.source === this.source && other.display === this.display
    );
  }
  toDOM(): HTMLElement {
    const el = document.createElement(this.display ? "div" : "span");
    el.className = this.display ? "cm-md-math-block" : "cm-md-math-inline";
    el.dataset.mathDisplay = String(this.display);
    el.textContent = this.display ? `$$${this.source}$$` : `$${this.source}$`;
    renderKatexInto(el, this.source, this.display, this.abort.signal);
    return el;
  }
  ignoreEvent() {
    // Let mousedown bubble so the wysiwyg plugin can move the caret into the source.
    return false;
  }
  destroy() {
    this.abort.abort();
  }
}
