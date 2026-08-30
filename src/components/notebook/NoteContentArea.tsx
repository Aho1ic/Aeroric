/* 随手记的正文区:按视图模式在「源码 / 所见即所得 / 分屏 / 阅读」之间路由。
 *
 * 从 NotebookPanel 抽出来,JSX 逐字未改。
 *
 * 源码编辑器由面板构造后当节点传进来(`sourceEditor`),不在这里 new ——
 * 它的接线牵涉正文更新、右键菜单和滚动恢复,那些都是面板的状态。这里只管布局。
 */

import type React from "react";
import { useMemo } from "react";
import type { NoteViewMode } from "./NoteTitleBar";

export type NoteContentAreaProps = {
  mode: NoteViewMode;
  /** 面板构造好的 `<NoteSourceEditor>`。编辑 / 所见即所得 / 分屏三态共用同一个。 */
  sourceEditor: React.ReactNode;
  /** 渲染好的 Markdown。阅读态与分屏态的预览侧都用它。 */
  markdownHtml: string;
  /** 阅读态的滚动容器。面板靠它做「切视图后恢复滚动位置」。 */
  readContentRef: React.RefObject<HTMLDivElement | null>;
  /** 分屏态预览侧的滚动容器(同步滚动用)。 */
  splitPreviewRef: React.RefObject<HTMLDivElement | null>;
  /** 承载渲染结果的容器。公式和 Mermaid 的懒渲染挂在它上面。 */
  previewRef: React.RefObject<HTMLDivElement | null>;
};

export function NoteContentArea({
  mode,
  sourceEditor,
  markdownHtml,
  readContentRef,
  splitPreviewRef,
  previewRef,
}: NoteContentAreaProps) {
  /* `{ __html }` 这个对象必须**跨渲染保持同一个**,不能每次现写字面量。
   *
   * React 对 `dangerouslySetInnerHTML` 的比较是按属性值的**身份**做的:新对象就重写一遍
   * innerHTML,即使里面的字符串一个字都没变。而重写会把预览里的子节点整批换新 ——
   * 所有 DOM 增强(wikilink 的 `<a>`、嵌入占位、相对路径图片的 src、KaTeX 与 Mermaid 的
   * `data-rendered`、任务复选框的解禁)当场全丢。
   *
   * 而这些增强的 effect 依赖里都带着 `markdownHtml`,这种重渲染下它没变,effect 不重跑,
   * 于是增强**不会自己回来**:wikilink 直接退回字面 `[[Target]]`,公式和图退回源码。
   * 触发它的都是日常操作 —— 开一下大纲、切一下侧栏档、一次自动保存回填保存状态。
   *
   * memo 掉之后 React 跳过重写,DOM 保持原样,增强也就不需要重做了。 */
  const html = useMemo(() => ({ __html: markdownHtml }), [markdownHtml]);

  if (mode === "edit" || mode === "wysiwyg" || mode === "split") {
    // 编辑态和分屏态用**同一套容器结构**,只改列数和预览列的存在性。
    //
    // 不能写成「分屏时套一层 grid、编辑时直接放编辑器」—— React 按树中
    // 位置 reconcile,位置变了就会卸载重挂 CodeMirror,光标和撤销栈全丢。
    return (
      <div
        style={{
          flex: 1,
          minHeight: 0,
          display: "grid",
          gridTemplateColumns:
            mode === "split" ? "minmax(0, 1fr) minmax(0, 1fr)" : "minmax(0, 1fr)",
        }}
      >
        <div
          style={{
            minWidth: 0,
            minHeight: 0,
            display: "flex",
            borderRight: mode === "split" ? "1px solid var(--border-dim)" : "none",
          }}
        >
          {sourceEditor}
        </div>
        {mode === "split" && (
          <div
            ref={splitPreviewRef}
            style={{ minWidth: 0, minHeight: 0, overflow: "auto", padding: 14 }}
          >
            <div
              ref={previewRef}
              className="md-preview notebook-markdown-preview"
              dangerouslySetInnerHTML={html}
            />
          </div>
        )}
      </div>
    );
  }
  // 阅读态。所有笔记都是 Markdown,所以只有这一条路径 —— 富文本的
  // `dangerouslySetInnerHTML` 分支随 contentEditable 一起删掉了。
  return (
    <div ref={readContentRef} style={{ flex: 1, minHeight: 0, overflow: "auto", padding: 14 }}>
      <div
        ref={previewRef}
        className="md-preview notebook-markdown-preview"
        dangerouslySetInnerHTML={html}
      />
    </div>
  );
}
