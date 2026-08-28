/* 随手记的 Markdown 源码编辑器(CodeMirror 6)。
 *
 * 替代原来的 `<textarea>`。换的理由不是"更现代",而是三件 textarea 做不到的事:
 * 语法高亮、WYSIWYG 装饰(P1 后续)、以及分屏时按行同步滚动。
 *
 * 对外保持 textarea 的接口形状(`value` / `onChange` / 选区查询),这样面板里
 * 那一大堆格式化命令(加粗、列表、表格…)不用重写 —— 它们只需要"当前选区的
 * 起止偏移"和"用新文本替换选区"两个能力。
 */

import { useCallback, useImperativeHandle, useMemo, useRef, type Ref } from "react";
import ReactCodeMirror, {
  EditorSelection,
  EditorView,
  type ReactCodeMirrorRef,
} from "@uiw/react-codemirror";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { LanguageDescription } from "@codemirror/language";
import { EditorState, type Extension } from "@codemirror/state";
import { githubDark, githubLight } from "@uiw/codemirror-theme-github";
import { solarizedLight } from "@uiw/codemirror-theme-solarized";
import type { ThemeVariant } from "../../types";
import { wysiwygMarkdown } from "./wysiwyg";

/** 面板里的格式化命令需要的最小能力集,等价于 textarea 的选区 API。 */
export type NoteEditorHandle = {
  focus(): void;
  /** 选区起点的文档偏移。 */
  selectionStart(): number;
  /** 选区终点的文档偏移。 */
  selectionEnd(): number;
  /** 当前是否有非空选区。 */
  hasSelection(): boolean;
  /** 用 `text` 替换 [from, to),并把光标/选区放到指定位置。 */
  replaceRange(from: number, to: number, text: string, cursor?: "select" | "after"): void;
  /** 把选区设为 [from, to) 并滚动到可见。 */
  setSelection(from: number, to: number): void;
  /** 滚动到某个文档偏移所在的行。 */
  revealOffset(offset: number): void;
  /** 当前滚动位置的比例(0–1),用于视图切换时保位。 */
  scrollRatio(): number;
  /** 恢复滚动比例。 */
  restoreScrollRatio(ratio: number): void;
  /** 真实的滚动元素(`.cm-scroller`)。分屏同步滚动要直接监听它。 */
  scrollElement(): HTMLElement | null;
};

export type NoteSourceEditorProps = {
  value: string;
  onChange: (value: string) => void;
  /** 选区变化时通知面板刷新工具栏的可用状态。 */
  onSelectionChange?: () => void;
  onContextMenu?: (event: React.MouseEvent<HTMLDivElement>) => void;
  themeVariant: ThemeVariant;
  ariaLabel: string;
  editorRef?: Ref<NoteEditorHandle>;
  /**
   * 开启 WYSIWYG 装饰:隐藏 markdown 标记、标题变大字号、代码块和表格渲染成
   * widget。标记始终隐藏(不随光标现形,那会让布局抖动);块级 widget 在光标进入
   * 其范围时退回源码。
   *
   * 底层文档始终是纯 markdown —— 这只是**显示层**装饰,不改变保存的内容。
   */
  wysiwyg?: boolean;
  /**
   * 挂载后要恢复到的滚动比例(0–1)。
   *
   * 由编辑器自己在 view 建好之后应用,而不是让面板在 `useLayoutEffect` 里调
   * `restoreScrollRatio` —— 从阅读态切回编辑态时 CodeMirror 是重新挂载的,
   * 面板那个 effect 跑的时候新 view 还不存在,恢复会静默失败。
   */
  initialScrollRatio?: number;
};

/**
 * 围栏代码块的语言支持。
 *
 * 不用 `@codemirror/language-data`(它会把 100+ 个语言包全拉进依赖图)。Aeroric
 * 已经装了 13 个 `@codemirror/lang-*`,这里按需懒加载它们 —— 覆盖笔记里实际会
 * 贴的语言,认不出的围栏就不高亮,不影响编辑。
 */
const CODE_LANGUAGES: LanguageDescription[] = [
  LanguageDescription.of({
    name: "javascript",
    alias: ["js", "jsx", "mjs", "cjs"],
    load: () => import("@codemirror/lang-javascript").then((m) => m.javascript({ jsx: true })),
  }),
  LanguageDescription.of({
    name: "typescript",
    alias: ["ts", "tsx"],
    load: () =>
      import("@codemirror/lang-javascript").then((m) =>
        m.javascript({ typescript: true, jsx: true }),
      ),
  }),
  LanguageDescription.of({
    name: "python",
    alias: ["py"],
    load: () => import("@codemirror/lang-python").then((m) => m.python()),
  }),
  LanguageDescription.of({
    name: "rust",
    alias: ["rs"],
    load: () => import("@codemirror/lang-rust").then((m) => m.rust()),
  }),
  LanguageDescription.of({
    name: "go",
    load: () => import("@codemirror/lang-go").then((m) => m.go()),
  }),
  LanguageDescription.of({
    name: "java",
    load: () => import("@codemirror/lang-java").then((m) => m.java()),
  }),
  LanguageDescription.of({
    name: "cpp",
    alias: ["c", "c++", "cc", "h", "hpp"],
    load: () => import("@codemirror/lang-cpp").then((m) => m.cpp()),
  }),
  LanguageDescription.of({
    name: "json",
    alias: ["jsonc"],
    load: () => import("@codemirror/lang-json").then((m) => m.json()),
  }),
  LanguageDescription.of({
    name: "yaml",
    alias: ["yml"],
    load: () => import("@codemirror/lang-yaml").then((m) => m.yaml()),
  }),
  LanguageDescription.of({
    name: "sql",
    load: () => import("@codemirror/lang-sql").then((m) => m.sql()),
  }),
  LanguageDescription.of({
    name: "html",
    alias: ["htm"],
    load: () => import("@codemirror/lang-html").then((m) => m.html()),
  }),
  LanguageDescription.of({
    name: "css",
    alias: ["scss", "less"],
    load: () => import("@codemirror/lang-css").then((m) => m.css()),
  }),
  LanguageDescription.of({
    name: "xml",
    alias: ["svg"],
    load: () => import("@codemirror/lang-xml").then((m) => m.xml()),
  }),
];

function themeFor(variant: ThemeVariant): Extension {
  if (variant === "dark") return githubDark;
  // 护眼模式用 solarized:与 SftpPreview / FileViewer 保持一致,避免同一个
  // 应用里两套代码配色。
  if (variant === "eyecare") return solarizedLight;
  return githubLight;
}

/** 让 CodeMirror 融进面板:去掉自带边框、用应用的字体与配色变量。 */
const panelTheme = EditorView.theme({
  "&": {
    height: "100%",
    fontSize: "12.5px",
    backgroundColor: "var(--bg-panel)",
  },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": {
    fontFamily: "var(--font-mono)",
    lineHeight: "1.6",
    padding: "12px 0",
  },
  ".cm-content": { padding: "0 12px" },
  ".cm-gutters": { display: "none" },
  ".cm-line": { padding: "0" },
});

export function NoteSourceEditor({
  value,
  onChange,
  onSelectionChange,
  onContextMenu,
  themeVariant,
  ariaLabel,
  editorRef,
  wysiwyg = false,
  initialScrollRatio,
}: NoteSourceEditorProps) {
  const cmRef = useRef<ReactCodeMirrorRef>(null);
  /** 初始滚动只应用一次:之后用户自己的滚动不该被 prop 覆盖。 */
  const scrollApplied = useRef(false);

  const extensions = useMemo<Extension[]>(
    () => [
      // 围栏里的代码也高亮 —— 笔记里贴代码很常见。
      markdown({ base: markdownLanguage, codeLanguages: CODE_LANGUAGES }),
      panelTheme,
      EditorView.lineWrapping,
      // aria-label 走 facet 而不是挂载后用 effect 设:effect 跑的时候
      // ReactCodeMirror 的 view 还没建好,标签会丢。facet 在建 view 时就生效。
      EditorView.contentAttributes.of({ "aria-label": ariaLabel }),
      // 选区变化要通知面板:工具栏按钮的启用状态依赖"有没有选中文本"。
      EditorView.updateListener.of((update) => {
        if (update.selectionSet) onSelectionChange?.();
      }),
      // WYSIWYG 装饰。放在最后:它的 StateField 要能看到前面 extension 的效果。
      ...(wysiwyg ? wysiwygMarkdown : []),
    ],
    [ariaLabel, onSelectionChange, wysiwyg],
  );

  const view = () => cmRef.current?.view ?? null;

  useImperativeHandle(
    editorRef,
    (): NoteEditorHandle => ({
      focus: () => view()?.focus(),
      selectionStart: () => view()?.state.selection.main.from ?? 0,
      selectionEnd: () => view()?.state.selection.main.to ?? 0,
      hasSelection: () => {
        const range = view()?.state.selection.main;
        return range ? !range.empty : false;
      },
      replaceRange: (from, to, text, cursor = "select") => {
        const editor = view();
        if (!editor) return;
        // 一次 dispatch 同时改文档和选区:分两次会在撤销栈里留两步,
        // 用户按一次 ⌘Z 只能退回一半。
        editor.dispatch({
          changes: { from, to, insert: text },
          selection:
            cursor === "after"
              ? EditorSelection.cursor(from + text.length)
              : EditorSelection.range(from, from + text.length),
          scrollIntoView: true,
        });
        editor.focus();
      },
      setSelection: (from, to) => {
        const editor = view();
        if (!editor) return;
        const max = editor.state.doc.length;
        editor.dispatch({
          // 夹到文档范围内:替换后调用方算出的偏移可能已经越界。
          selection: EditorSelection.range(Math.min(from, max), Math.min(to, max)),
          scrollIntoView: true,
        });
      },
      revealOffset: (offset) => {
        const editor = view();
        if (!editor) return;
        const max = editor.state.doc.length;
        editor.dispatch({
          effects: EditorView.scrollIntoView(Math.min(offset, max), { y: "center" }),
        });
      },
      scrollRatio: () => {
        const scroller = view()?.scrollDOM;
        if (!scroller) return 0;
        const max = scroller.scrollHeight - scroller.clientHeight;
        return max > 0 ? scroller.scrollTop / max : 0;
      },
      restoreScrollRatio: (ratio) => {
        const scroller = view()?.scrollDOM;
        if (!scroller) return;
        const max = scroller.scrollHeight - scroller.clientHeight;
        scroller.scrollTop = ratio * Math.max(0, max);
      },
      scrollElement: () => view()?.scrollDOM ?? null,
    }),
    [],
  );

  const handleChange = useCallback((next: string) => onChange(next), [onChange]);

  /** ReactCodeMirror 把 view 建好后回调。这是能安全设滚动的最早时机。 */
  const handleCreateEditor = useCallback(
    (view: EditorView) => {
      if (scrollApplied.current || initialScrollRatio === undefined) return;
      scrollApplied.current = true;
      const scroller = view.scrollDOM;
      const max = scroller.scrollHeight - scroller.clientHeight;
      scroller.scrollTop = initialScrollRatio * Math.max(0, max);
    },
    [initialScrollRatio],
  );

  return (
    <div
      style={{ flex: 1, minHeight: 0, overflow: "hidden" }}
      onContextMenu={onContextMenu}
      onMouseUp={onSelectionChange}
      onKeyUp={onSelectionChange}
    >
      <ReactCodeMirror
        ref={cmRef}
        value={value}
        onChange={handleChange}
        onCreateEditor={handleCreateEditor}
        theme={themeFor(themeVariant)}
        extensions={extensions}
        height="100%"
        style={{ height: "100%" }}
        basicSetup={{
          lineNumbers: false,
          foldGutter: false,
          highlightActiveLine: false,
          highlightActiveLineGutter: false,
          // 括号自动配对在写 markdown 时更多是干扰(`[` 会变成 `[]`)。
          closeBrackets: false,
          // ⌘F 由面板自己的查找栏接,不用 CodeMirror 的。
          searchKeymap: false,
        }}
      />
    </div>
  );
}

/** 供测试构造独立的 EditorState(不经 React)。 */
export function createNoteEditorState(doc: string): EditorState {
  return EditorState.create({
    doc,
    extensions: [markdown({ base: markdownLanguage })],
  });
}
