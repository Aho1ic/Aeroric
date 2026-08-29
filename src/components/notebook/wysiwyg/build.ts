/**
 * 整文档 decoration 重建。
 *
 * 调用时机：StateField.update 检测到 docChanged，或选区跨越了某个 widget
 * 的 cursor-sensitive 边界（参考 anySensitiveRangeFlipped）。其它选区变化
 * 走 fast-path 不重建。
 *
 * 性能模型：单次 build = 一次 doc.toString + math/wikilink 两次 regex 全扫 +
 * 一次 syntaxTree.iterate 整树遍历。lezer 增量解析对几千行只需要几 ms；
 * 几万行的大文档主要瓶颈在 syntaxTree iterate + WidgetType 实例化次数。
 */

import { syntaxTree, syntaxTreeAvailable } from "@codemirror/language";
import type { EditorState } from "@codemirror/state";
import { Decoration, type DecorationSet } from "@codemirror/view";

import { detectMathRanges } from "../mathRanges";

import { CodeFenceWidget } from "./codeFence";
import { FrontmatterWidget, parseFrontmatter } from "./frontmatter";
import {
  CalloutLabelWidget,
  HrWidget,
  ImageWidget,
  ListMarkerWidget,
  TableSepWidget,
  TaskCheckbox,
  isAbsoluteSafeUrl,
  normalizeCalloutType,
} from "./inlineWidgets";
import { MathWidget } from "./math";
import { TableWidget } from "./table";
import { needsVaultResolve } from "../attachmentUrls";
import { parseImageMarkdown } from "../markdownImages";

interface PendingDeco {
  from: number;
  to: number;
  deco: Decoration;
}

export interface BuildResult {
  decorations: DecorationSet;
  /** 隐藏掉的 marker 字符范围，给 EditorView.atomicRanges 用 ——
   *  防止鼠标拖动选区时光标"落进"被隐藏的字符里、导致选区视觉上溢出到下方行 */
  atomic: DecorationSet;
  /** 受光标位置影响是否展示 widget 的所有判定范围。selection-only 的 tr 触发
   *  rebuild 时，先按这些范围在新旧选区下的命中变化判断；任何一个翻转才 rebuild，
   *  否则直接复用上次结果，避免大文档每次方向键 / 鼠标选中都跑全文 syntaxTree。 */
  sensitive: SensitiveRange[];
  /** 本次 build 时 lezer 是否已把整篇文档解析完。大文档首帧 syntaxTree 只解析了
   *  视口附近一段（预算内），靠后的块在树里还没有节点 → 没有装饰、渲染成原始
   *  markdown。后台解析推进时 CM 派发的是无 docChanged 的事务，StateField 不会
   *  重建，于是「滚到下面好几秒后才忽然变成富文本」。state.ts 据此在解析完成的
   *  那一刻补一次 rebuild。 */
  fullyParsed: boolean;
}

export interface SensitiveRange {
  from: number;
  to: number;
  inclusive: boolean;
}

/** rangeHasCursor 的内核：把判断从 EditorState 解耦到 EditorSelection，
 *  让 fast-path 在 update() 里跨 prev / current selection 重放。 */
export function selectionHitsRange(
  selection: { ranges: ReadonlyArray<{ head: number; from: number; to: number }> },
  range: SensitiveRange,
): boolean {
  for (const sel of selection.ranges) {
    const head = sel.head;
    const headInside = range.inclusive
      ? head >= range.from && head <= range.to
      : head >= range.from && head < range.to;
    if (headInside) return true;
    if (sel.from < range.to && sel.to > range.from) return true;
  }
  return false;
}

/** 任何 sensitive range 的命中在两个选区下结果不同 → 必须重新 build。 */
export function anySensitiveRangeFlipped(
  ranges: SensitiveRange[],
  prevSel: { ranges: ReadonlyArray<{ head: number; from: number; to: number }> },
  newSel: { ranges: ReadonlyArray<{ head: number; from: number; to: number }> },
): boolean {
  for (const r of ranges) {
    if (selectionHitsRange(prevSel, r) !== selectionHitsRange(newSel, r)) {
      return true;
    }
  }
  return false;
}

/**
 * Read the fenced-code body for a `n === "FencedCode"` node. The lezer node
 * spans both fences; we strip the first line (```lang) and the trailing
 * ``` line. Returns the inner source code (no trailing newline).
 */
function extractFencedBody(state: EditorState, from: number, to: number): string {
  const firstLine = state.doc.lineAt(from);
  const bodyStart = Math.min(firstLine.to + 1, state.doc.length);
  if (bodyStart >= to) return "";
  const slice = state.doc.sliceString(bodyStart, to);
  // strip a trailing ``` / ~~~ line if present
  const stripped = slice.replace(/\r?\n?[ \t]*(`{3,}|~{3,})\s*$/, "");
  return stripped;
}

function extractFenceLang(state: EditorState, from: number): string {
  const firstLine = state.doc.lineAt(from);
  const m = firstLine.text.match(/^\s*(`{3,}|~{3,})\s*([\w-]+)/);
  return m ? m[2]! : "";
}

/**
 * YAML frontmatter 区间：文档以 `---\n` 开头 + 后续某行单独 `---` 结束。
 * 返回 frontmatter 结束后第一个字符的 offset；不存在时返回 0。
 *
 * 用途：lezer 把 `---` 解析为 HorizontalRule，没有 frontmatter 节点；如果
 * 不显式排除，文档头部和 `---` 闭合行都会被替换成 HR widget。
 */
function frontmatterEnd(text: string): number {
  if (!text.startsWith("---")) return 0;
  // 第一行必须就是 `---`（允许尾随 \r）
  const firstEol = text.indexOf("\n");
  if (firstEol < 0) return 0;
  const firstLine = text.slice(0, firstEol).trimEnd();
  if (firstLine !== "---") return 0;
  // 后续找一行只含 `---` 的关闭 delimiter
  let i = firstEol + 1;
  while (i < text.length) {
    const eol = text.indexOf("\n", i);
    const line = text.slice(i, eol < 0 ? text.length : eol).trimEnd();
    if (line === "---") {
      // 返回关闭 `---` 行内容结尾（不含其后换行）。之前返回 eol+1 指向下一行
      // 行首，使 doc.lineAt(fmEnd) 落到正文首行，导致首行被误标 frontmatter 样式、
      // 并被排除在空行压缩之外。
      return eol < 0 ? text.length : eol;
    }
    if (eol < 0) return 0;
    i = eol + 1;
  }
  return 0;
}

export function build(state: EditorState): BuildResult {
  const decos: PendingDeco[] = [];
  const atomic: PendingDeco[] = [];
  /** 把每个"是否生成 widget 由光标位置决定"的范围都登记进来，让 update()
   *  的 selection-only fast path 能判断"翻转"。 */
  const sensitive: SensitiveRange[] = [];
  const trackCursor = (from: number, to: number, inclusive: boolean = true): boolean => {
    sensitive.push({ from, to, inclusive });
    return selectionHitsRange(state.selection, { from, to, inclusive });
  };

  // doc.toString() 是大文档（兆级笔记）的真正 cost；math / wikilink 两个
  // regex 扫共享同一份字符串，避免重复转换。
  const docText = state.doc.toString();
  const fmEnd = frontmatterEnd(docText);

  // 记录所有被某种「块 / 行级」处理接管的行号：标题、引用、列表、代码块、
  // frontmatter，以及被块级 widget 替换掉的多行区间（块公式 / 代码 / frontmatter）。
  // 段落之间的空行压缩只作用于「真正空且不落在上述任何区间内」的行。
  const markedLines = new Set<number>();
  const markLineRange = (from: number, to: number) => {
    const start = state.doc.lineAt(Math.max(0, from)).number;
    const end = state.doc.lineAt(Math.min(state.doc.length, to)).number;
    for (let ln = start; ln <= end; ln++) markedLines.add(ln);
  };

  // Math regions: detect once over the full doc (regex-only, no AST since
  // lezer-markdown has no math node by default). Skip the widget when the
  // cursor is inside so the user can edit the source plainly.
  const mathRanges = detectMathRanges(docText);
  for (const range of mathRanges) {
    if (trackCursor(range.from, range.to, true)) continue;
    decos.push({
      from: range.from,
      to: range.to,
      deco: Decoration.replace({
        widget: new MathWidget(range.source, range.display),
        block: range.display,
      }),
    });
    atomic.push({
      from: range.from,
      to: range.to,
      deco: Decoration.mark({}),
    });
    if (range.display) markLineRange(range.from, range.to);
  }

  // 注:Markio 在这里扫 [[wikilink]] 并渲染成 widget。双链属于 P4,那部分连带
  // 依赖 vaultIndex store,移植时一并摘掉 —— 现在 [[…]] 在编辑器里是普通文本,
  // 不被装饰,也不报错。

  /** 用 replace 把 [from,to) 之间的字符隐藏起来。
   *
   *  注意：之前实现"光标在本行时还原 marker"会导致行长度变化，drawSelection
   *  把 cursor 画到新的视觉位置上，看起来"鼠标点击位置和实际位置不符"——
   *  Typora / iA Writer 等成熟 WYSIWYG 都是稳定布局：marker 始终隐藏，靠
   *  快捷键 / 工具栏改样式。所以这里去掉 cursorOnSameLine 兜底，保证视觉稳定。
   *
   *  同时把范围登记进 atomic，CM 选区移动时整段跳过被隐藏的 marker。 */
  const hide = (from: number, to: number) => {
    if (from >= to) return;
    decos.push({ from, to, deco: Decoration.replace({}) });
    atomic.push({ from, to, deco: Decoration.mark({}) });
  };

  /** 给一个范围加 mark 装饰（行内文字样式），可选挂 DOM 属性（如 data-href） */
  const mark = (from: number, to: number, cls: string, attributes?: Record<string, string>) => {
    if (from >= to) return;
    decos.push({
      from,
      to,
      deco: Decoration.mark(attributes ? { class: cls, attributes } : { class: cls }),
    });
  };

  let visibleFrom = 0;
  let visibleTo = state.doc.length;

  /** 给一行加 class（标题 / 引用整行） */
  const lineMark = (pos: number, cls: string) => {
    if (pos < visibleFrom || pos > visibleTo) return;
    const line = state.doc.lineAt(pos);
    markedLines.add(line.number);
    decos.push({
      from: line.from,
      to: line.from,
      deco: Decoration.line({ class: cls }),
    });
  };

  const markLines = (from: number, to: number, cls: string) => {
    const fromPos = Math.max(from, visibleFrom);
    const toPos = Math.min(to, visibleTo);
    if (fromPos > toPos) return;
    const startLine = state.doc.lineAt(fromPos).number;
    const endLine = state.doc.lineAt(toPos).number;
    for (let ln = startLine; ln <= endLine; ln++) {
      const line = state.doc.line(ln);
      markedLines.add(ln);
      decos.push({
        from: line.from,
        to: line.from,
        deco: Decoration.line({ class: cls }),
      });
    }
  };

  // ─── YAML frontmatter ───
  // 渲染成就地可编辑的属性表（始终渲染，不退回原始块）。仅当 YAML 能被干净
  // 解析时才上 widget——否则退回原始文本编辑，避免就地编辑丢数据。
  // 无论哪种都跳过区间内的 lezer 节点（见 enter 开头的 guard），避免
  // `- 脚本` 被当成无序列表、`title:` 被当成裸段落。
  if (fmEnd > 0) {
    const source = docText.slice(0, fmEnd);
    const fm = parseFrontmatter(source);
    if (fm.ok && fm.props.length > 0) {
      decos.push({
        from: 0,
        to: fmEnd,
        deco: Decoration.replace({
          widget: new FrontmatterWidget(source),
          block: true,
        }),
      });
      atomic.push({ from: 0, to: fmEnd, deco: Decoration.mark({}) });
      markLineRange(0, fmEnd);
    } else {
      markLines(0, fmEnd, "cm-md-line cm-md-frontmatter-line");
    }
  }

  // 块级 decoration 必须从 StateField 提供 → 没有 view.visibleRanges，
  // 直接遍历整个 doc。lezer 解析是增量的，全树 iterate 对几千行也只是几 ms。
  const tree = syntaxTree(state);
  const ranges = [{ from: 0, to: state.doc.length }];
  for (const range of ranges) {
    visibleFrom = range.from;
    visibleTo = range.to;
    tree.iterate({
      from: visibleFrom,
      to: visibleTo,
      enter: (node) => {
        const n = node.name;

        // frontmatter 区间整体由上面的 widget / line-mark 接管，跳过**完全落在**
        // 区间内的 lezer 节点（含被误判成 Hr / ListItem / ListMark 的 `---` 与
        // `- x`）。注意必须判 node.to <= fmEnd：文档根节点 from=0 也 < fmEnd，
        // 若只判 from 会把整棵树（含正文）一并跳过，导致正文完全不渲染。
        if (fmEnd > 0 && node.to <= fmEnd) return false;

        // ─── 标题 ATX ───
        if (/^ATXHeading[1-6]$/.test(n)) {
          const lvl = Number(n.slice(-1));
          lineMark(node.from, `cm-md-line cm-md-h${lvl}`);
          return;
        }
        if (n === "SetextHeading1") {
          lineMark(node.from, "cm-md-line cm-md-h1");
          return;
        }
        if (n === "SetextHeading2") {
          lineMark(node.from, "cm-md-line cm-md-h2");
          return;
        }

        // ─── 引用 / Callout ───
        if (n === "Blockquote") {
          const firstLine = state.doc.lineAt(node.from);
          // `> [!type][+|-]?` marker on the first line of the quote → callout
          const marker = firstLine.text.match(/^(\s*>\s*)\[!([a-zA-Z][\w-]*)\]([+-])?/);
          if (marker) {
            const rawType = marker[2]!;
            const type = normalizeCalloutType(rawType);
            const tokenStart = firstLine.from + marker[1]!.length;
            const tokenEnd = firstLine.from + marker[0].length;
            // 把 [!type] 这段隐藏起来，前面塞一个样式化的标签 widget
            if (!trackCursor(tokenStart, tokenEnd, true)) {
              decos.push({
                from: tokenStart,
                to: tokenEnd,
                deco: Decoration.replace({ widget: new CalloutLabelWidget(type) }),
              });
              atomic.push({
                from: tokenStart,
                to: tokenEnd,
                deco: Decoration.mark({}),
              });
            }
            markLines(
              node.from,
              node.to,
              `cm-md-line cm-md-quote-line cm-md-callout cm-md-callout-${type}`,
            );
            return;
          }
          markLines(node.from, node.to, "cm-md-line cm-md-quote-line");
          return;
        }

        // ─── 列表 ───
        if (n === "ListItem") {
          // 用父节点判断有序 / 无序，加不同 line class（CSS 给 .cm-md-list-ol 加序号样式）
          const parent = node.node.parent?.name;
          const isOrdered = parent === "OrderedList";
          lineMark(
            node.from,
            isOrdered ? "cm-md-line cm-md-list cm-md-list-ol" : "cm-md-line cm-md-list",
          );
          return;
        }
        // 列表标记（- / * / 1. ）光标不在本行时隐藏；本行时保留以便编辑
        if (n === "ListMark") {
          const line = state.doc.lineAt(node.from);
          const after = state.doc.sliceString(node.to, node.to + 1);
          const to = after === " " ? node.to + 1 : node.to;
          const rest = state.doc.sliceString(to, line.to);
          const isTask = /^\s*\[[ xX]\]/i.test(rest);
          if (isTask) {
            hide(node.from, to);
            return;
          }
          const marker = state.doc.sliceString(node.from, node.to).trim();
          const ordered = /^\d+\./.test(marker);
          decos.push({
            from: node.from,
            to,
            deco: Decoration.replace({
              widget: new ListMarkerWidget(ordered ? marker : "•", ordered),
            }),
          });
          atomic.push({ from: node.from, to, deco: Decoration.mark({}) });
          return;
        }

        // ─── 表格 ───
        if (n === "Table") {
          const source = state.doc.sliceString(node.from, node.to);
          decos.push({
            from: node.from,
            to: node.to,
            deco: Decoration.replace({
              widget: new TableWidget(source),
              block: true,
            }),
          });
          atomic.push({
            from: node.from,
            to: node.to,
            deco: Decoration.mark({}),
          });
          return false;
        }
        // TableDelimiter 在 lezer-markdown 里有两种用法：
        //   * 单字符 `|`（每行内的 cell 分隔符）—— 此时 to-from === 1，保留显示
        //   * 整行 `|---|---|` 的对齐分隔行 —— 此时长度 > 1，替换成一条细线
        if (n === "TableDelimiter") {
          if (node.to - node.from > 1) {
            decos.push({
              from: node.from,
              to: node.to,
              deco: Decoration.replace({ widget: new TableSepWidget() }),
            });
            atomic.push({ from: node.from, to: node.to, deco: Decoration.mark({}) });
          }
          return;
        }

        // ─── 水平线 ───
        if (n === "HorizontalRule") {
          // YAML frontmatter 的开 / 闭 `---` 不当 HR 渲染 —— 否则首行变成横线、
          // 中间 YAML 字段悬空，整个 frontmatter 块视觉错乱
          if (node.from < fmEnd) return;
          const line = state.doc.lineAt(node.from);
          decos.push({
            from: line.from,
            to: line.from,
            deco: Decoration.line({ class: "cm-md-line cm-md-hr" }),
          });
          // 始终把 --- 替换为视觉横线（稳定布局，不依赖 cursor）
          decos.push({
            from: node.from,
            to: node.to,
            deco: Decoration.replace({ widget: new HrWidget() }),
          });
          atomic.push({ from: node.from, to: node.to, deco: Decoration.mark({}) });
          return;
        }

        // ─── Marker 隐藏 ───
        if (n === "HeaderMark") {
          // 包括 # 后面那个空格也吃掉
          const after = state.doc.sliceString(node.to, node.to + 1);
          const to = after === " " ? node.to + 1 : node.to;
          hide(node.from, to);
          return;
        }
        if (n === "QuoteMark") {
          const after = state.doc.sliceString(node.to, node.to + 1);
          const to = after === " " ? node.to + 1 : node.to;
          hide(node.from, to);
          return;
        }
        if (n === "EmphasisMark" || n === "StrikethroughMark" || n === "CodeMark") {
          hide(node.from, node.to);
          return;
        }
        if (n === "LinkMark") {
          // [ ] ( ) 这些
          hide(node.from, node.to);
          return;
        }
        if (n === "URL") {
          const parentName = node.node.parent?.name;
          if (parentName === "Link" || parentName === "Image" || parentName === "LinkReference") {
            // [label](url) / ![alt](url) 的目标，或引用定义里的 url —— 隐藏
            hide(node.from, node.to);
          } else if (parentName !== "Autolink") {
            // 裸 URL（GFM autolink）：URL 本身就是可见正文，保持显形且可点
            const raw = state.doc.sliceString(node.from, node.to);
            mark(node.from, node.to, "cm-md-link", { "data-href": raw });
          }
          // parent === Autolink 时已在 Autolink 分支整体标过，跳过
          return;
        }

        // ─── 行内样式包裹 ───
        if (n === "StrongEmphasis") {
          mark(node.from, node.to, "cm-md-bold");
          return;
        }
        if (n === "Emphasis") {
          mark(node.from, node.to, "cm-md-italic");
          return;
        }
        if (n === "Strikethrough") {
          mark(node.from, node.to, "cm-md-strike");
          return;
        }
        if (n === "InlineCode") {
          mark(node.from, node.to, "cm-md-code");
          return;
        }
        if (n === "Link") {
          // 把目标 URL 挂到 span 上，点击时 mousedown handler 直接读 data-href
          // 路由（外链 / 库内文件 / 锚点），无需再回解析语法树。
          const urlNode = node.node.getChild("URL");
          const href = urlNode ? state.doc.sliceString(urlNode.from, urlNode.to) : "";
          mark(node.from, node.to, "cm-md-link", href ? { "data-href": href } : undefined);
          return;
        }
        if (n === "Autolink") {
          // <https://…> / <a@b.com>：尖括号交给 LinkMark 隐藏，URL 文本保持显形可点
          const urlNode = node.node.getChild("URL");
          if (urlNode) {
            const raw = state.doc.sliceString(urlNode.from, urlNode.to);
            // 邮箱 autolink 没有 scheme，补 mailto: 才能交给系统邮件
            const href =
              !/^[a-z][a-z0-9+.-]*:/i.test(raw) && raw.includes("@") ? `mailto:${raw}` : raw;
            mark(urlNode.from, urlNode.to, "cm-md-link", { "data-href": href });
          }
          return;
        }
        if (n === "Image") {
          const text = state.doc.sliceString(node.from, node.to);
          const parts = parseImageMarkdown(text);
          // 始终渲染图片 widget（点击进编辑浮层），不再光标显形 markdown 源码。
          // vault 里的相对路径也要进 widget:它是随手记最常见的图片来源(粘贴 /
          // 拖入都产出相对链接)。退回源码的话用户会看到自己刚粘的图变成一行
          // `![](attachments/x.png)`。src 由 widget 异步解析。
          if (parts && (isAbsoluteSafeUrl(parts.url) || needsVaultResolve(parts.url))) {
            decos.push({
              from: node.from,
              to: node.to,
              deco: Decoration.replace({
                widget: new ImageWidget(parts, false, node.to - node.from),
              }),
            });
            atomic.push({
              from: node.from,
              to: node.to,
              deco: Decoration.mark({}),
            });
            return;
          }
          mark(node.from, node.to, "cm-md-image");
          return false;
        }

        // ─── 任务列表 ───
        if (n === "TaskMarker") {
          const text = state.doc.sliceString(node.from, node.to);
          const checked = /x/i.test(text);
          // 始终用 □ / ☑ 替代 [ ] / [x]，点击 widget 切换；保持布局稳定
          decos.push({
            from: node.from,
            to: node.to,
            deco: Decoration.replace({
              widget: new TaskCheckbox(checked),
            }),
          });
          atomic.push({ from: node.from, to: node.to, deco: Decoration.mark({}) });
          // 标记整行
          lineMark(node.from, `cm-md-line cm-md-task-line${checked ? " done" : ""}`);
          return;
        }

        // ─── 代码块 ───
        if (n === "FencedCode") {
          const lang = extractFenceLang(state, node.from);
          const cursorInBlock = trackCursor(node.from, node.to, false);
          if (!cursorInBlock) {
            const source = extractFencedBody(state, node.from, node.to);
            // Markio 在这里对 mermaid/dot/chart 用 VisualFenceWidget 直接渲图。那需要
            // charts + graphviz WASM,不在融合范围内;编辑态一律走普通代码块 widget,
            // 阅读态的 Mermaid 由 noteVisuals 负责。
            const widget = new CodeFenceWidget(lang, source, node.to - node.from);
            decos.push({
              from: node.from,
              to: node.to,
              deco: Decoration.replace({ widget, block: true }),
            });
            atomic.push({
              from: node.from,
              to: node.to,
              deco: Decoration.mark({}),
            });
            markLineRange(node.from, node.to);
            return;
          }
          markLines(node.from, node.to, "cm-md-line cm-md-codeblock");
          return;
        }
        if (n === "CodeBlock") {
          markLines(node.from, node.to, "cm-md-line cm-md-codeblock");
          return;
        }
      },
    });
  }

  // 段落之间的空行在 wysiwyg 下仍是真实 .cm-line，按正文行高（~1.75）各占满一整行，
  // 比分屏预览（间距由块 margin 控制、源码空行被折叠）观感空旷得多。给「真正空且未被
  // 任何块 / 行级处理接管」的行加 cm-md-blank，CSS 据此压低其行高，贴近预览的段间距。
  // 代码块 / frontmatter / 块公式内部的空行已登记进 markedLines，不受影响。
  for (let ln = 1; ln <= state.doc.lines; ln++) {
    if (markedLines.has(ln)) continue;
    const line = state.doc.line(ln);
    if (line.length === 0 || line.text.trim().length === 0) {
      decos.push({
        from: line.from,
        to: line.from,
        deco: Decoration.line({ class: "cm-md-blank" }),
      });
    }
  }

  decos.sort((a, b) => a.from - b.from || a.to - b.to);
  atomic.sort((a, b) => a.from - b.from || a.to - b.to);
  return {
    decorations: Decoration.set(
      decos.map((d) => d.deco.range(d.from, d.to)),
      true,
    ),
    atomic: Decoration.set(
      atomic.map((d) => d.deco.range(d.from, d.to)),
      true,
    ),
    sensitive,
    fullyParsed: syntaxTreeAvailable(state, state.doc.length),
  };
}
