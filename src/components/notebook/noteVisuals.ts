/* 公式与 Mermaid 图的懒渲染。
 *
 * `noteRender.ts` 只产占位元素,真正的渲染在这里,并且**按视口优先、逐块让出
 * 主线程**(见 visualScheduler.ts)。原因:KaTeX 编译和 Mermaid 布局都是主线程
 * CPU 密集操作,一篇图多的笔记全量同步渲染会冻住界面好几秒。
 *
 * 两个库都懒加载。合计 tree-shake 后约 4.8 MB —— 进主 bundle 会拖慢冷启动,
 * 而多数笔记里既没有公式也没有图。
 *
 * CSP:两者都不需要 `unsafe-eval` / `wasm-unsafe-eval`。这是实测结论 ——
 * 打包探针后扫产物,零处相关构造(`scripts/scan-csp-hazards.mjs` 可复现)。
 * 注意 `d3` 的 UMD 产物里确实有 `new Function`(d3-dsv 的 CSV 解析器),但
 * Mermaid 走 ESM 入口,Vite 会整个 tree-shake 掉。
 */

import DOMPurify from "dompurify";
import {
  scheduleVisualBlocks,
  type VisualBlockHandle,
  type VisualSchedulerOptions,
} from "./visualScheduler";
import { NOTEBOOK_MATH_SELECTOR, NOTEBOOK_MERMAID_SELECTOR } from "./noteRender";

type KatexModule = typeof import("katex");
type MermaidModule = (typeof import("mermaid"))["default"];

/**
 * 从动态 import 的结果里取出真正的模块。
 *
 * katex 是 CJS 包。`import("katex")` 在不同环境下给的东西不一样:Vite 的浏览器
 * 构建会把 API 提到命名空间顶层,而 vitest 的 node 解析把它们留在 `.default`
 * 里。不解包的话 `katex.renderToString is not a function` —— 生产和测试里都会中,
 * 只是被错误分支兜住了,表现成「公式显示为原始 TeX」而不是崩掉。
 */
function unwrapModule<T>(module: T | { default: T }): T {
  if (module && typeof module === "object" && "default" in module) {
    const inner = (module as { default: T }).default;
    // 只有 default 里确实有东西时才用它 —— 某些包两边都有,顶层才是全的。
    if (inner) return inner;
  }
  return module as T;
}

let katexPromise: Promise<KatexModule> | null = null;
let katexCssLoaded = false;
let mermaidPromise: Promise<MermaidModule> | null = null;
/** Mermaid 只 initialize 一次,主题变了才重新配置(它是全局单例)。 */
let mermaidTheme: "dark" | "default" | null = null;
let mermaidCounter = 0;

/**
 * 懒加载 KaTeX。导出给 wysiwyg 的行内公式 widget 共用 —— 两处各自 import 会
 * 各自持有一份 promise,CSS 也会被请求两次。
 */
export async function getKatex(): Promise<KatexModule> {
  if (!katexPromise) {
    katexPromise = import("katex").then((module) => unwrapModule<KatexModule>(module));
    if (!katexCssLoaded) {
      katexCssLoaded = true;
      // CSS 跟 JS 一起懒加载,别让 24KB 常驻冷启动关键路径。
      void import("katex/dist/katex.min.css");
    }
  }
  return katexPromise;
}

function currentThemeIsDark(): boolean {
  return document.documentElement.classList.contains("dark");
}

async function getMermaid(): Promise<MermaidModule> {
  if (!mermaidPromise) {
    // mermaid 是 ESM,default 就是实例;仍走 unwrap 以防打包器再包一层。
    mermaidPromise = import("mermaid").then((module) =>
      unwrapModule<MermaidModule>(module.default ?? module),
    );
  }
  const mermaid = await mermaidPromise;
  const theme = currentThemeIsDark() ? "dark" : "default";
  if (mermaidTheme !== theme) {
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: "strict",
      theme,
      // 用原生 SVG <text> 而不是 <foreignObject> 里的 HTML:后者过 DOMPurify
      // 的 svg profile 时会被剥成空框,图上一个字都不剩。
      htmlLabels: false,
      flowchart: { htmlLabels: false },
    });
    mermaidTheme = theme;
  }
  return mermaid;
}

/** KaTeX 输出 MathML + HTML,两个 profile 都要开。 */
const MATH_SANITIZE = {
  USE_PROFILES: { html: true, mathMl: true, svg: true },
} as const;

async function renderMathBlock(node: HTMLElement): Promise<void> {
  if (node.dataset.rendered) return;
  // 占位元素的 textContent 就是原始 TeX(见 noteRender 的 restoreMath)。
  const tex = node.textContent ?? "";
  const display = node.classList.contains("notebook-math-display");
  // 记下源码:渲染后 textContent 会被 KaTeX 输出覆盖,复制/导出还要用原式。
  node.dataset.mathSource = tex;

  try {
    const katex = await getKatex();
    const html = katex.renderToString(tex, {
      displayMode: display,
      throwOnError: false,
      strict: "ignore",
      output: "htmlAndMathml",
    });
    node.innerHTML = DOMPurify.sanitize(html, MATH_SANITIZE);
    node.dataset.rendered = "1";
  } catch (error) {
    // 渲染失败时退回显示原式并把原因挂 title —— 比留一片空白有用。
    node.textContent = display ? `$$${tex}$$` : `$${tex}$`;
    node.title = (error as Error).message;
    node.dataset.rendered = "1";
    node.classList.add("notebook-math-error");
  }
}

async function renderMermaidBlock(node: HTMLElement): Promise<void> {
  if (node.dataset.rendered) return;
  const encoded = node.getAttribute("data-mermaid") ?? "";
  let source: string;
  try {
    source = decodeURIComponent(encoded);
  } catch {
    // 畸形百分号编码会让 decodeURIComponent 抛错。用原文兜底,别让异常逃出
    // 去把整个调度循环带走。
    source = encoded;
  }

  try {
    const mermaid = await getMermaid();
    const { svg } = await mermaid.render(`notebook-mermaid-${mermaidCounter++}`, source);
    node.innerHTML = DOMPurify.sanitize(svg, {
      USE_PROFILES: { svg: true, svgFilters: true, html: true },
    });
    node.dataset.rendered = "1";
  } catch (error) {
    const pre = document.createElement("pre");
    pre.className = "notebook-mermaid-error";
    pre.textContent = `${(error as Error).message}\n\n${source}`;
    node.replaceChildren(pre);
    node.dataset.rendered = "1";
  }
}

/**
 * 懒渲染容器内的公式与 Mermaid 图。
 *
 * 返回的 handle 要在容器卸载/内容换掉时 `disconnect()`,否则
 * IntersectionObserver 会一直持有已经不在文档里的节点。
 */
export function renderNoteVisualsLazy(
  root: HTMLElement,
  options: VisualSchedulerOptions = {},
): VisualBlockHandle {
  const math = scheduleVisualBlocks<HTMLElement>(
    root,
    NOTEBOOK_MATH_SELECTOR,
    renderMathBlock,
    options,
  );
  const mermaid = scheduleVisualBlocks<HTMLElement>(
    root,
    NOTEBOOK_MERMAID_SELECTOR,
    renderMermaidBlock,
    options,
  );
  return {
    disconnect: () => {
      math.disconnect();
      mermaid.disconnect();
    },
    flushAll: async () => {
      await math.flushAll();
      await mermaid.flushAll();
    },
  };
}

/** 立即渲染全部(导出 / 打印 / 测试用)。 */
export async function renderNoteVisuals(root: HTMLElement): Promise<void> {
  for (const node of Array.from(root.querySelectorAll<HTMLElement>(NOTEBOOK_MATH_SELECTOR))) {
    await renderMathBlock(node);
  }
  for (const node of Array.from(root.querySelectorAll<HTMLElement>(NOTEBOOK_MERMAID_SELECTOR))) {
    await renderMermaidBlock(node);
  }
}

/**
 * 主题切换后重绘已渲染的 Mermaid 图。
 *
 * Mermaid 把配色烧进 SVG,不重绘的话暗色主题下会留着一张亮色的图。KaTeX 用
 * `currentColor`,不受影响。
 */
export function invalidateMermaidTheme(root: HTMLElement): void {
  const nextTheme = currentThemeIsDark() ? "dark" : "default";
  if (mermaidTheme === nextTheme) return;
  for (const node of Array.from(
    root.querySelectorAll<HTMLElement>(".notebook-mermaid[data-rendered]"),
  )) {
    delete node.dataset.rendered;
    // 清掉旧 SVG,不然重绘前会闪一下上个主题的图。
    node.replaceChildren();
  }
}
