/**
 * 把渲染出来的那块 HTML 补成能用的预览:公式、Mermaid、图片尺寸、`[[wikilink]]`、
 * `![[嵌入]]`、悬浮预览、`notebook-query` 结果表、点链接跳转、勾选任务,以及分屏的
 * 同步滚动。
 *
 * 全都是"拿到 DOM 之后再动手"的一类:`renderNoteMarkdown` 只产出静态 HTML,交互和
 * 异步内容一律在这里补。收在一个 hook 里的关键理由是**顺序** —— 见不变量 2。
 *
 * 不变量:
 *
 * 1. **只在阅读 / 分屏态跑**。编辑态那半边由 CodeMirror 的 widget 自己负责,预览容器
 *    那时根本不在树上。
 *
 * 2. **effect 的声明顺序就是执行顺序,不能重排**。React 按声明顺序跑 effect,而这一串
 *    里有三处真实的先后依赖:
 *    - 嵌入要在 wikilink 之后 —— 占位节点是 `enhanceWikiLinks` 造的,反过来的话第一帧
 *      一个占位都找不到;
 *    - 查询要在嵌入之后 —— 嵌入进来的内容里也可能有查询块,先跑的话那些扫不到;
 *    - 懒渲染和图片尺寸在最前面扫的是**当时**的 DOM,扫不到后来才填进去的嵌入内容,
 *      所以嵌入自己在 `onFilled` 里对新内容再补一次。
 *
 * 3. **`markdownHtml` 变了就重挂**。`dangerouslySetInnerHTML` 会整块换掉 DOM,旧的
 *    IntersectionObserver / MutationObserver 还盯着已经不在文档里的节点,不 disconnect
 *    会漏。
 *
 * 4. **`linkIndex` 要进依赖**。新建 / 删除 / 改标题之后死链要变活(或反过来),而那时
 *    `markdownHtml` 一个字都没变 —— 只按 HTML 重跑的话链接会一直停在旧状态。
 *
 * 5. **解禁复选框那一个故意不写依赖数组**。`dangerouslySetInnerHTML={{ __html }}` 的属性
 *    值是每次渲染新建的对象,React 会在每次重渲染时重新写一遍 innerHTML(即使字符串一个
 *    字都没变),预览里的子节点被整批换成崭新的一份 —— 解禁、类名、aria-label 全丢。
 *    只按 `markdownHtml` 当依赖的话,这种重渲染之后 effect 不会重跑,复选框就永久点不动
 *    (随手打开大纲、侧栏,或者一次自动保存回填状态,都会触发)。
 *
 * 6. **点击一律走事件委托**。容器节点在重渲染里是稳定的,子节点会被整批换掉(见不变量 5);
 *    委托到容器就不受影响。链接和复选框都由 enhance* 塞进 DOM,不在 React 树里,但事件
 *    照样冒泡到容器上。
 *
 * 7. **勾选任务的依赖里必须有笔记 id,而且不能靠 `markdownHtml` 代替**。两篇正文完全相同
 *    的笔记渲染出的 HTML 是同一个字符串,切过去时 effect 不重挂,闭包里还是上一篇的 id ——
 *    点一下就把没显示的那篇改了(行号也对得上,乐观锁察觉不到),而当前这篇看着像没反应。
 */
import { useEffect } from "react";

import { attachWikiLinkHover } from "./hoverPreview";
import { enhanceMarkdownImages } from "./markdownImages";
import { enhanceNoteEmbeds } from "./noteEmbed";
import { enhanceNoteQueries } from "./enhanceNoteQueries";
import { enhanceTaskCheckboxes, taskToggleFromEvent } from "./enhanceTaskCheckboxes";
import { enhanceWikiLinks, isWikiLinkClick, wikiLinkTargetFromEvent } from "./enhanceWikiLinks";
import { normalizeLinkTarget, type VaultLinkIndex } from "./noteLinks";
import { peekNote, vaultFields } from "./notebookApi";
import { invalidateMermaidTheme, renderNoteVisualsLazy } from "./noteVisuals";
import type { NoteEditorHandle } from "./NoteSourceEditor";
import type { NoteViewMode } from "./NoteTitleBar";
import {
  paneFromElement,
  registerPane,
  resetSplitScrollSync,
  syncPreviewToSource,
} from "./splitScrollSync";

export type NotePreviewOptions = {
  mode: NoteViewMode;
  /** 承载渲染结果的容器。所有 enhance 都挂在它上面。 */
  previewRef: React.RefObject<HTMLDivElement | null>;
  /** 分屏态里预览侧的滚动容器。 */
  splitPreviewRef: React.RefObject<HTMLDivElement | null>;
  /** 分屏态里源码侧的滚动容器从它取。 */
  editorRef: React.RefObject<NoteEditorHandle | null>;
  /** 渲染结果。见不变量 3。 */
  markdownHtml: string;
  /** 链接索引。见不变量 4。 */
  linkIndex: VaultLinkIndex;
  /** 当前笔记的路径。嵌入的环路检测拿它当第 0 层祖先,勾选任务拿它当依赖(不变量 7)。 */
  activeNoteId: string | null;
  /** 查询块要扫全库字段,没有 vault 时它自己会空手而归。 */
  vault: string | null;
  setActiveId: (noteId: string) => void;
  /** 勾选阅读态的任务复选框。 */
  toggleTaskAtLine: (line: number, expectChecked: boolean) => void;
  t: (key: string, vars?: Record<string, string | number>) => string;
};

/**
 * 按 `[[笔记#小节]]` 里的锚点滚到预览里的对应标题。
 *
 * 跳笔记之后正文还要过一次异步读取 + 渲染,所以不能同步找节点。用 requestAnimationFrame
 * 等一帧:阅读态的 HTML 是 `dangerouslySetInnerHTML` 挂上去的,React 提交完那一帧节点
 * 就在了。容器在那一帧里现取 —— 这一帧之前面板可能已经被卸载。
 *
 * 匹配文本而不是 slug:用户写 `[[笔记#小节标题]]` 时写的是标题原文,而 slug 是我们自己
 * 算出来的(去标点、转小写),两者不一定一致。`noteEmbed` 里按小节裁正文用的是同一条
 * 口径。
 */
function scrollToWikiHeading(
  previewRef: React.RefObject<HTMLDivElement | null>,
  heading: string,
): void {
  requestAnimationFrame(() => {
    const host = previewRef.current;
    if (!host) return;
    const needle = heading.trim().toLowerCase();
    const found = Array.from(host.querySelectorAll<HTMLElement>("h1,h2,h3,h4,h5,h6")).find(
      (node) => (node.textContent ?? "").trim().toLowerCase() === needle,
    );
    found?.scrollIntoView({ behavior: "smooth", block: "start" });
  });
}

export function useNotePreviewEnhancers({
  mode,
  previewRef,
  splitPreviewRef,
  editorRef,
  markdownHtml,
  linkIndex,
  activeNoteId,
  vault,
  setActiveId,
  toggleTaskAtLine,
  t,
}: NotePreviewOptions): void {
  // 阅读态的公式与 Mermaid 图:视口优先懒渲染。见不变量 3。
  useEffect(() => {
    if (mode !== "read" && mode !== "split") return;
    const host = previewRef.current;
    if (!host) return;
    const handle = renderNoteVisualsLazy(host);
    return () => handle.disconnect();
  }, [markdownHtml, mode, previewRef]);

  /* `![a](x.png "width=320")` 里的宽度标注。
   *
   * 编辑态的 widget 自己调 applyImageElementSizing,阅读态没人调 —— 于是同一张图
   * 在编辑时是 320px,切到阅读就撑满整行。这一步补上那半边。 */
  useEffect(() => {
    if (mode !== "read" && mode !== "split") return;
    const host = previewRef.current;
    if (!host) return;
    enhanceMarkdownImages(host);
  }, [markdownHtml, mode, previewRef]);

  // `[[wikilink]]` → 可点的链接。见不变量 4。
  useEffect(() => {
    if (mode !== "read" && mode !== "split") return;
    const host = previewRef.current;
    if (!host) return;
    enhanceWikiLinks(host, linkIndex, {
      open: (title) => t("notebook.wikiLinkOpen", { title }),
      missing: (target) => t("notebook.wikiLinkMissing", { target }),
      ambiguous: (title) => t("notebook.wikiLinkAmbiguous", { title }),
    });
  }, [markdownHtml, mode, linkIndex, t, previewRef]);

  /* `![[note]]` → 嵌进来的笔记内容。**必须声明在上面那个 effect 之后**,见不变量 2。
   *
   * `activeNoteId` 进依赖是因为它同时是环路检测的第 0 层祖先 —— 切笔记之后宿主换了,
   * 拿旧路径当祖先会把"新宿主嵌入旧宿主"错判成自嵌。 */
  useEffect(() => {
    if (mode !== "read" && mode !== "split") return;
    const host = previewRef.current;
    if (!host) return;
    const hostPath = activeNoteId;
    if (!hostPath) return;
    const handle = enhanceNoteEmbeds(host, {
      hostPath,
      // 只读取数:`openNote` 会登记指纹,而嵌入不是"打开"(见 peekNote 的注释)。
      read: async (path) => (await peekNote(path)).content,
      index: linkIndex,
      labels: {
        open: (title) => t("notebook.wikiLinkOpen", { title }),
        missing: (target) => t("notebook.wikiLinkMissing", { target }),
        ambiguous: (title) => t("notebook.wikiLinkAmbiguous", { title }),
        missingHeading: (heading) => t("notebook.embedMissingHeading", { heading }),
        tooDeep: (target) => t("notebook.embedTooDeep", { target }),
        failed: (target, message) => t("notebook.embedFailed", { target, message }),
      },
      /* 嵌入内容是这个 effect 之后才进 DOM 的,上面那两个 effect(懒渲染、图片尺寸)
         扫的是当时的 DOM,扫不到它。所以在这里给每块填好的内容补一次。见不变量 2。 */
      onFilled: (body) => {
        enhanceMarkdownImages(body);
        const visuals = renderNoteVisualsLazy(body);
        return () => visuals.disconnect();
      },
    });
    return () => handle.disconnect();
  }, [markdownHtml, mode, linkIndex, t, activeNoteId, previewRef]);

  /* wikilink 悬浮预览。
   *
   * 依赖里带 `linkIndex`:它同时供标题查表用,而"目标改了标题"要反映到卡片头部上。
   * 卡片挂在 body 上,所以 disconnect 是必须的 —— 不摘会在切模式之后留一张浮在界面上。 */
  useEffect(() => {
    if (mode !== "read" && mode !== "split") return;
    const host = previewRef.current;
    if (!host) return;
    const handle = attachWikiLinkHover(host, {
      // 只读取数,和嵌入同一条路径(见 peekNote 的注释)。
      read: async (path) => (await peekNote(path)).content,
      titleOf: (path) => linkIndex.byPath.get(normalizeLinkTarget(path))?.title,
      labels: {
        loading: () => t("notebook.hoverPreviewLoading"),
        failed: () => t("notebook.hoverPreviewFailed"),
      },
    });
    return () => handle.disconnect();
  }, [markdownHtml, mode, linkIndex, t, previewRef]);

  /* ```notebook-query 围栏 → 按 frontmatter 字段查全库的结果表。
   *
   * 声明在嵌入之后(见不变量 2)。表格里的笔记名是按 wikilink 的约定造的,所以下面那个
   * 点击监听不用改就能跳过去。
   *
   * 依赖里带 `linkIndex`:标题从它来(那份合并过内存标题和扫盘标题),改了标题要跟着变。 */
  useEffect(() => {
    if (mode !== "read" && mode !== "split") return;
    const host = previewRef.current;
    if (!host) return;
    const handle = enhanceNoteQueries(host, {
      vault,
      scan: vaultFields,
      titleOf: (path) => linkIndex.byPath.get(normalizeLinkTarget(path))?.title || path,
      labels: {
        head: ({ key, value, shown, total }) =>
          `${
            value === undefined
              ? t("notebook.queryHeadKey", { key })
              : t("notebook.queryHeadKeyValue", { key, value })
          } · ${
            shown === total
              ? t("notebook.queryCount", { count: total })
              : t("notebook.queryCountLimited", { shown, total })
          }`,
        empty: () => t("notebook.queryEmpty"),
        noteColumn: () => t("notebook.queryNoteColumn"),
        open: (title) => t("notebook.wikiLinkOpen", { title }),
        failed: (message) => t("notebook.queryFailed", { message }),
        problem: (problem) => {
          switch (problem.code) {
            case "missingKey":
              return t("notebook.queryProblemMissingKey");
            case "unknownDirective":
              return t("notebook.queryProblemUnknownDirective", { name: problem.name });
            case "badSort":
              return t("notebook.queryProblemBadSort", { value: problem.value });
            case "badLimit":
              return t("notebook.queryProblemBadLimit", { value: problem.value });
          }
        },
      },
    });
    return () => handle.disconnect();
  }, [markdownHtml, mode, vault, linkIndex, t, previewRef]);

  // 点 wikilink 跳笔记。见不变量 6。
  useEffect(() => {
    if (mode !== "read" && mode !== "split") return;
    const host = previewRef.current;
    if (!host) return;
    const onClick = (event: MouseEvent) => {
      if (!isWikiLinkClick(event)) return;
      // 死链也要拦:它是个 `<a>`,不拦会走默认行为(在这个 webview 里是跳到
      // 一个空 fragment,顺带把滚动位置打到顶部)。
      event.preventDefault();
      const hit = wikiLinkTargetFromEvent(event);
      if (!hit) return;
      setActiveId(hit.path);
      if (hit.heading) scrollToWikiHeading(previewRef, hit.heading);
    };
    host.addEventListener("click", onClick);
    return () => host.removeEventListener("click", onClick);
  }, [markdownHtml, mode, setActiveId, previewRef]);

  // 阅读态勾选任务:点击处理。见不变量 6、7。
  useEffect(() => {
    if (mode !== "read" && mode !== "split") return;
    const host = previewRef.current;
    if (!host) return;
    const onClick = (event: MouseEvent) => {
      const hit = taskToggleFromEvent(event);
      if (!hit) return;
      /* 拦掉默认行为:原生复选框会先把自己的 `checked` 翻过来,而正文改没改要等
         `toggleTaskLine` 说话(乐观锁不符时它拒绝写)。勾选状态的唯一来源是正文,
         不该由控件自己先改一版。 */
      event.preventDefault();
      toggleTaskAtLine(hit.line, hit.expectChecked);
    };
    host.addEventListener("click", onClick);
    return () => host.removeEventListener("click", onClick);
    /* toggleTaskAtLine 刻意不进依赖:它每次渲染都是新函数,进依赖就变成每渲染一次重挂
       一次监听。旧闭包也不会写错 —— 它唯一在意的响应式值是笔记 id(已在依赖里),正文
       则是在 `setNotes` 的 updater 里现读的。 */
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode, activeNoteId]);

  /* 阅读态勾选任务:解禁复选框。**故意不写依赖数组**,见不变量 5。
   *
   * 嵌入进来的内容不受影响:那些渲染时没开 `taskLines`,天然不带行号,
   * `enhanceTaskCheckboxes` 会跳过 —— 嵌入的是别人的笔记,不解禁正是想要的结果。 */
  useEffect(() => {
    if (mode !== "read" && mode !== "split") return;
    const host = previewRef.current;
    if (!host) return;
    enhanceTaskCheckboxes(host, {
      toggle: (text) => t("notebook.taskToggle", { text }),
    });
  });

  // 分屏同步滚动。两侧注册进总线,由它做比例对齐和防回声。
  useEffect(() => {
    if (mode !== "split") return;
    const previewEl = splitPreviewRef.current;
    const sourceEl = editorRef.current?.scrollElement();
    if (!previewEl || !sourceEl) return;
    registerPane("source", paneFromElement(sourceEl));
    registerPane("preview", paneFromElement(previewEl));
    return () => resetSplitScrollSync();
  }, [mode, activeNoteId, splitPreviewRef, editorRef]);

  // 预览内容换掉之后(改字、公式渲染完)重新对齐一次:预览侧高度变了,原来的
  // scrollTop 对应的位置已经不是同一段内容。
  useEffect(() => {
    if (mode !== "split") return;
    syncPreviewToSource();
  }, [markdownHtml, mode]);

  // 主题切换后重绘 Mermaid:它把配色烧进 SVG,暗色主题下不重绘会留一张亮色图。
  // KaTeX 用 currentColor,不受影响。
  useEffect(() => {
    if (mode !== "read") return;
    const host = previewRef.current;
    if (!host) return;
    const observer = new MutationObserver(() => {
      invalidateMermaidTheme(host);
      // 清掉 data-rendered 之后要再跑一轮才会重画。
      renderNoteVisualsLazy(host);
    });
    // 主题靠 documentElement 上的 `dark` 类切换。
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });
    return () => observer.disconnect();
  }, [mode, previewRef]);
}
