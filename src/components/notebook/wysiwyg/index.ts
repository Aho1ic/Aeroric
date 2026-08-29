/**
 * Markdown WYSIWYG decoration 插件(CodeMirror 6)。
 *
 * 思路:用 `syntaxTree` 拿 lezer 的 markdown AST,对每个语法节点生成 Decoration
 * —— 给整行加 class(标题大字号、引用左边线…)、给行内段落加 mark(粗体 / 斜体 /
 * 行内代码 / 链接 / 删除线)、把 markdown 标记字符(`#` `**` `` ` `` `>` `[]`
 * `![]()`…)替换成 widget 或直接隐藏。
 *
 * **marker 始终隐藏,不随光标现形。** 这是 Markio 的刻意设计:让 marker 在光标
 * 所在行现形会改变行长度,`drawSelection` 就把光标画到偏移后的视觉位置上,点击
 * 手感变得不对。Typora / iA Writer 都是稳定布局 —— 改样式靠工具栏和快捷键。
 *
 * 例外是**块级 widget**(代码块、表格、frontmatter):它们整块替换,里面放不下
 * 光标,所以光标进入该块的范围时退回显示源码。
 *
 * 模块划分(移植自 Markio 的 `src/components/editor/wysiwyg/`):
 * ```
 * index.ts        入口:组合 field + mousedown,再导出表格 API
 * build.ts        整文档 decoration 构建 + 敏感范围跟踪
 * state.ts        StateField,docChanged / selection 的重建策略
 * mousedown.ts    widget 点击行为
 * math.ts         行内/块级公式 widget(KaTeX 懒加载)
 * codeFence.ts    代码块 widget + 其 DOM 交互
 * table.ts        表格 widget + 单元格编辑
 * inlineWidgets.ts 轻量 widget(任务框、图片、分隔线…)
 * frontmatter.ts  frontmatter 折叠 widget
 * editPopover.ts  链接/图片的就地编辑浮层
 * highlight.ts    代码高亮子系统(懒加载)
 * util.ts         共享的 DOM/类型工具
 * ```
 *
 * **未移植的两块**(不在融合范围内):
 * - `wikilink.ts` —— `[[双链]]` 属于 P4,且依赖 vaultIndex store
 * - `visualFence.ts` —— mermaid/dot/chart 直接在编辑器里渲图,需要 charts 与
 *   graphviz WASM。编辑态一律走普通代码块 widget;阅读态的 Mermaid 由
 *   `noteVisuals` 负责。
 */

import { wysiwygMousedown } from "./mousedown";
import { wysiwygField } from "./state";

export {
  applyWysiwygTableAction,
  buildTableDom,
  buildTableSource,
  parseTableSource,
} from "./table";
export type { ParsedTable, WysiwygTableAction } from "./table";
export { attachmentContext, type AttachmentContext } from "./attachmentFacet";

/** 挂进 CodeMirror 的 extension 数组即可启用 WYSIWYG。 */
export const wysiwygMarkdown = [wysiwygField, wysiwygMousedown];
