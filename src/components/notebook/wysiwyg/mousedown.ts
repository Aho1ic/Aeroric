/**
 * WYSIWYG widget 的点击行为。
 *
 * 统一模式:点一个渲染出来的 widget 就把光标移到它对应的 markdown 源码起点,
 * 下一次 build 会因为「光标进了这一行」而还原成源码,于是可以直接编辑。
 * 任务复选框是例外 —— 点它直接切换 `[ ]` / `[x]`,不进源码。
 *
 * 移植自 Markio(`src/components/editor/wysiwyg/mousedown.ts`)。三处改动:
 * - **去掉 wikilink 分支**:双链属于 P4,widget 本身也没移植。
 * - **锚点跳转复用 `noteSlug.slugifyHeading`**:Markio 用 `@/lib/utils` 里的同名
 *   函数;这里用随手记自己那份,保证「预览里的锚点 id」与「编辑器里的跳转目标」
 *   出自同一套规则。
 * - **外链走 `@tauri-apps/plugin-opener`**:Markio 走它自己的 `linkNav` +
 *   tabs store。随手记还没有「在标签页里打开库内文件」的概念(那是 P2 的
 *   TabStrip),所以库内相对路径暂不处理,只处理锚点和外链。
 */

import { EditorView } from "@codemirror/view";
import { openUrl } from "@tauri-apps/plugin-opener";

import { createSlugRegistry, slugifyHeading } from "../noteSlug";
import { eventElementTarget } from "./util";

/** 在文档里找到 slug 匹配的标题行起点,找不到返回 null。 */
function headingPosForSlug(view: EditorView, slug: string): number | null {
  const doc = view.state.doc;
  // 用与渲染侧同一套去重规则(同名标题第二次出现加 `-1` 后缀),否则重复标题
  // 的锚点会跳到第一个而不是用户点的那个。
  const used = createSlugRegistry();
  for (let i = 1; i <= doc.lines; i += 1) {
    const line = doc.line(i);
    const match = /^(#{1,6})\s+(.+?)\s*$/.exec(line.text);
    if (!match) continue;
    if (slugifyHeading(match[2] ?? "", used) === slug) return line.from;
  }
  return null;
}

/** 只放行 http(s)。`file:` / `javascript:` 之类交给 opener 会是个安全洞。 */
function isSafeExternalUrl(href: string): boolean {
  try {
    const url = new URL(href);
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
}

export const wysiwygMousedown = EditorView.domEventHandlers({
  mousedown(event, view) {
    const target = eventElementTarget(event);
    if (!target) return;

    // 数学公式 widget → 光标移到公式源码起点(跳过第一个 `$`)。
    const mathHost = target.closest<HTMLElement>(".cm-md-math-inline, .cm-md-math-block");
    if (mathHost) {
      const pos = view.posAtDOM(mathHost);
      if (pos != null) {
        view.dispatch({ selection: { anchor: pos + 1 } });
        view.focus();
        event.preventDefault();
      }
      return;
    }

    // 行内链接 / autolink / 裸 URL。
    //   普通点击 → 锚点滚到标题 / 外链开浏览器
    //   Alt 点击 或 无 href → 光标移进去编辑
    const linkHost = target.closest<HTMLElement>(".cm-md-link");
    if (linkHost) {
      const href = linkHost.dataset.href;
      if (href && !event.altKey) {
        event.preventDefault();
        if (href.startsWith("#")) {
          let slug = href.slice(1);
          try {
            slug = decodeURIComponent(slug);
          } catch {
            // 畸形编码就用原值,别让异常打断点击。
          }
          const registry = createSlugRegistry();
          const pos = headingPosForSlug(view, slugifyHeading(slug, registry));
          if (pos != null) {
            view.dispatch({
              selection: { anchor: pos },
              effects: EditorView.scrollIntoView(pos, { y: "start" }),
            });
            view.focus();
          }
          return;
        }
        if (isSafeExternalUrl(href)) void openUrl(href);
        return;
      }
      const pos = view.posAtDOM(linkHost);
      if (pos != null) {
        view.dispatch({ selection: { anchor: pos } });
        view.focus();
        event.preventDefault();
      }
      return;
    }

    // 图片 widget → 光标移到 markdown 源码起点(`!` 处)。
    const imgHost = target.closest<HTMLElement>(".cm-md-img-widget");
    if (imgHost) {
      const pos = view.posAtDOM(imgHost);
      if (pos == null) return;
      view.dispatch({ selection: { anchor: pos } });
      view.focus();
      event.preventDefault();
      return;
    }

    // 注:Markio 在这里还有一个 `.cm-md-fenced-widget` 分支,处理 mermaid/dot/chart
    // 的可视化 widget。那个 class 由 `visualFence.ts` 产生,而该文件不在融合范围内
    // (需要 charts + graphviz WASM),所以这里没有对应分支。
    //
    // 普通代码块的 widget(`.cm-md-code-widget`)自己在 `codeFence.ts` 里装了
    // mousedown 处理,包括点空白处进编辑态 —— 不要在这里重复一遍。

    // 任务复选框 → 直接切换 `[ ]` / `[x]`,不进源码。
    if (!target.classList?.contains("cm-md-task")) return;
    const pos = view.posAtDOM(target);
    if (pos == null) return;
    const line = view.state.doc.lineAt(pos);
    const match = /^(\s*[-*+]\s+\[)([ xX])(\])/.exec(line.text);
    if (!match) return;
    const insert = (match[2] ?? " ").toLowerCase() === "x" ? " " : "x";
    const from = line.from + (match[1] ?? "").length;
    view.dispatch({ changes: { from, to: from + 1, insert } });
    event.preventDefault();
  },
});
