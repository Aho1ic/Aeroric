/**
 * 把 build() 输出的 DecorationSet 装进 CodeMirror StateField,并通过 provide
 * 把 decorations / atomicRanges 暴露给视图层。
 *
 * 为什么必须 StateField 而不是 ViewPlugin:CodeMirror 禁止 ViewPlugin 提供 block
 * 类型的 `Decoration.replace`(`block: true`),而块级公式 / 表格 / 代码块 widget
 * 都是 block widget。
 *
 * update 策略:
 * - `docChanged` → 完整 rebuild
 * - 仅选区变化 → 只在某个「现形/隐藏」边界被跨过时才 rebuild。否则方向键、
 *   鼠标拖选、简单点击都会触发整文档 syntaxTree 遍历,大文档下每次移动光标
 *   都卡一下。
 *
 * 移植自 Markio(`src/components/editor/wysiwyg/state.ts`)。去掉了
 * `wysiwygVaultSync` —— 那个 ViewPlugin 订阅 vaultIndex / workspace store,在
 * 仓库文件集变化时强制重建以刷新 `[[wikilink]]` 的解析状态。双链属于 P4,
 * 那两个 store 也还不存在。
 */

import { syntaxTreeAvailable } from "@codemirror/language";
import { StateField } from "@codemirror/state";
import { EditorView } from "@codemirror/view";

import { anySensitiveRangeFlipped, build, type BuildResult } from "./build";

export const wysiwygField = StateField.define<BuildResult>({
  create(state) {
    return build(state);
  },
  update(prev, tr) {
    // 文档变了 → 必须完整重算。
    if (tr.docChanged) {
      return build(tr.state);
    }
    // 上次 build 时 lezer 还没解析到文档尾(大文档首帧只解析视口预算内的一段)。
    // 后台解析推进时 CM 派发的事务既没 docChanged 也没 selection,上面那条不命中。
    // 这里在「解析刚好整篇完成」的那一刻补一次 rebuild,让此前渲染成原始 markdown
    // 的靠后区块即时变成富文本,而不是等用户编辑才偶然刷新。
    // `fullyParsed` 一旦为真,这个判断短路,不再有额外开销。
    if (!prev.fullyParsed && syntaxTreeAvailable(tr.state, tr.state.doc.length)) {
      return build(tr.state);
    }
    // 选区变了 → 只在跨过敏感边界时 rebuild。
    if (tr.selection) {
      if (anySensitiveRangeFlipped(prev.sensitive, tr.startState.selection, tr.state.selection)) {
        return build(tr.state);
      }
    }
    return prev;
  },
  provide: (field) => [
    EditorView.decorations.from(field, (value) => value.decorations),
    EditorView.atomicRanges.of((view) => view.state.field(field).atomic),
  ],
});
