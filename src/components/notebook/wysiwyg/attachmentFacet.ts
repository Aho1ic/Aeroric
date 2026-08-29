/**
 * 编辑器里的图片 widget 怎么拿到"这张图在哪"。
 *
 * 笔记里的图片链接是相对**笔记所在目录**的,而 widget 只看得到 markdown 源码 ——
 * 它不知道当前编辑的是哪个文件。用一个 facet 把上下文交进来:facet 在建 view 时
 * 就生效,比挂载后用 effect 塞进去可靠(那时 view 还没建好)。
 *
 * 单独一个文件而不是并进 `inlineWidgets.ts`:那个文件被 `build.ts` 引,而这里要
 * 引外面的 resolver 类型,合在一起会让 wysiwyg 子包对面板产生方向不对的依赖。
 */

import { Facet } from "@codemirror/state";

export type AttachmentContext = {
  /** 当前笔记所在目录。空串表示还不知道(未保存的新笔记)。 */
  noteDir: string;
  /** 相对地址 → 可渲染 URL。返回原样即表示不需要解析。 */
  resolve: (url: string, noteDir: string) => Promise<string>;
};

const NO_CONTEXT: AttachmentContext = {
  noteDir: "",
  // 默认不解析:facet 没被配置时(比如别处复用这个编辑器)图片行为和以前一样。
  resolve: (url) => Promise.resolve(url),
};

export const attachmentContext = Facet.define<AttachmentContext, AttachmentContext>({
  combine: (values) => values[0] ?? NO_CONTEXT,
});
