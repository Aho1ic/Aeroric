/* 随手记的 Markdown 源码编辑器(CodeMirror 6)。
 *
 * 替代原来的 `<textarea>`。换的理由不是"更现代",而是三件 textarea 做不到的事:
 * 语法高亮、WYSIWYG 装饰(P1 后续)、以及分屏时按行同步滚动。
 *
 * 对外保持 textarea 的接口形状(`value` / `onChange` / 选区查询),这样面板里
 * 那一大堆格式化命令(加粗、列表、表格…)不用重写 —— 它们只需要"当前选区的
 * 起止偏移"和"用新文本替换选区"两个能力。
 */

import { useCallback, useEffect, useImperativeHandle, useMemo, useRef, type Ref } from "react";
import ReactCodeMirror, {
  EditorSelection,
  EditorView,
  type ReactCodeMirrorRef,
} from "@uiw/react-codemirror";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { LanguageDescription } from "@codemirror/language";
import { EditorState, Prec, type Extension } from "@codemirror/state";
import { keymap } from "@codemirror/view";
import { githubDark, githubLight } from "@uiw/codemirror-theme-github";
import { solarizedLight } from "@uiw/codemirror-theme-solarized";
import type { ThemeVariant } from "../../types";
import { detectTrigger, type TriggerKind } from "./noteTriggers";
import { attachmentContext, wysiwygMarkdown, type AttachmentContext } from "./wysiwyg";

/** 面板里的格式化命令需要的最小能力集,等价于 textarea 的选区 API。 */
export type NoteEditorHandle = {
  focus(): void;
  /** 选区起点的文档偏移。 */
  selectionStart(): number;
  /** 选区终点的文档偏移。 */
  selectionEnd(): number;
  /** 当前是否有非空选区。 */
  hasSelection(): boolean;
  /** 当前选中的文本。无选区时返回空串。剪切 / 复制用。 */
  selectedText(): string;
  /** 用 `text` 替换 [from, to),并把光标/选区放到指定位置。 */
  replaceRange(from: number, to: number, text: string, cursor?: "select" | "after"): void;
  /**
   * 用 `text` 替换**当前**选区,光标落在插入内容之后。
   *
   * 和 `replaceRange` 的区别是不接偏移。异步插入(存附件要等写盘)必须用这个:
   * 那期间用户可能继续打字,拿着出发时算的偏移去替换会插错位置甚至吃掉刚输入的
   * 字,而选区是 CodeMirror 自己跟着后续编辑一起映射的。
   */
  replaceSelection(text: string): void;
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
  /**
   * 视口坐标 → 文档偏移。点不在编辑器上时返回 null。
   *
   * 系统文件管理器拖入用它:那个事件是**整个窗口**的,不判落点的话把文件拖到
   * 笔记列表上也会往正文里插图。
   */
  posAtClientPoint(x: number, y: number): number | null;
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
  /**
   * 挂载后要把光标放到的偏移。
   *
   * 和 `initialScrollRatio` 同一个理由必须走 prop:切换笔记时 CodeMirror 是**重新
   * 挂载**的(key 带笔记 id),面板在 effect 里调 handle 时拿到的还是上一篇的 view,
   * 选区会设在正要被卸载的编辑器上,静默失效。
   *
   * 给值的时机不限于挂载那一刻:目标笔记的正文晚于编辑器到位时(反链指向一篇还没
   * 读入的笔记),这个 prop 会在之后某次渲染才第一次有值,那时也照样生效。
   */
  initialCursorOffset?: number;
  /**
   * 图片链接的解析上下文。笔记里的图是 `attachments/x.png` 这样的相对路径,
   * widget 单看 markdown 源码不知道它相对谁。
   */
  attachments?: AttachmentContext;
  /**
   * 粘贴 / 拖入了文件。`at` 是插入点的文档偏移(粘贴时是当前选区起点)。
   *
   * 返回 true 表示这次事件已经被接手,编辑器会阻止默认行为 —— 否则浏览器会把
   * 图片的文件名当纯文本插进去。
   */
  onDropFiles?: (files: File[], at: number) => boolean;
  /**
   * 光标前的触发序列变了(`/` `[[` `#` `@` `:`),或者不再有触发序列(传 null)。
   *
   * `coords` 是光标的**视口**坐标(`left` / `bottom`),菜单挂在它下面。
   *
   * 检测由编辑器做而不是面板:面板手里的 `body` 是受控值,而 CodeMirror 的文档在
   * 一次输入里先变、`onChange` 后到 —— 拿 `body` 算触发会永远慢一个字符,表现是
   * 打 `#` 不弹、打第二个字符才弹出上一次的候选。
   */
  onTriggerChange?: (state: TriggerState | null) => void;
  /**
   * 菜单开着时接管方向键 / 回车 / Tab / Esc。返回 true = 已处理,编辑器不再让
   * CodeMirror 的默认绑定看到这个键。
   *
   * 走 `Prec.highest` 的 keymap:`basicSetup` 里的 `defaultKeymap` 已经把 ArrowDown
   * 绑成"下移一行"、Enter 绑成"插入换行",不提权的话菜单永远抢不到。
   */
  onTriggerKey?: (key: TriggerKeyName) => boolean;
};

/** `onTriggerChange` 报上来的一次触发态。 */
export type TriggerState = {
  kind: TriggerKind;
  /** 触发符第一个字符的文档偏移。提交时的替换起点。 */
  start: number;
  query: string;
  /** 光标的视口坐标。 */
  coords: { x: number; y: number };
};

/** 菜单要接管的键。 */
export type TriggerKeyName = "ArrowUp" | "ArrowDown" | "Enter" | "Tab" | "Escape";

/**
 * 从剪贴板 / 拖放数据里挑出图片文件。
 *
 * 只认 `image/*`:粘贴一段带格式的文本时 `dataTransfer` 里也有 `Files`(某些应用
 * 会塞一份 HTML 的快照),不过滤的话复制粘贴文字会莫名多出一张图。
 */
function imageFilesFrom(data: DataTransfer | null): File[] {
  if (!data || data.files.length === 0) return [];
  return Array.from(data.files).filter((file) => file.type.startsWith("image/"));
}

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
  initialCursorOffset,
  attachments,
  onDropFiles,
  onTriggerChange,
  onTriggerKey,
}: NoteSourceEditorProps) {
  const cmRef = useRef<ReactCodeMirrorRef>(null);
  /* 触发相关的两个回调走 ref。它们每次渲染都是新函数(依赖菜单状态),进 `extensions`
     的 memo 依赖会让整个 extension 数组每次输入都重建 —— 那会重置 WYSIWYG 的
     StateField 和撤销栈。 */
  const onTriggerChangeRef = useRef(onTriggerChange);
  const onTriggerKeyRef = useRef(onTriggerKey);
  useEffect(() => {
    onTriggerChangeRef.current = onTriggerChange;
    onTriggerKeyRef.current = onTriggerKey;
  }, [onTriggerChange, onTriggerKey]);
  /** 初始滚动只应用一次:之后用户自己的滚动不该被 prop 覆盖。 */
  const scrollApplied = useRef(false);
  /**
   * 还没落下的初始光标。`undefined` = 没有待办。
   *
   * 不能在 `onCreateEditor` 里直接读 prop:ReactCodeMirror 要等 container 的 ref 落地
   * (一次额外渲染)才建 view,比拿到 prop 的那次渲染晚一整轮 effect;而给这个 prop
   * 的一方(反链跳转)在同一次 commit 的 effect 里就把落点清掉了 —— 它没法知道编辑器
   * 什么时候才读。等回调真的跑起来,prop 已经是 `undefined`,光标停在 0。
   *
   * 消费掉就置回 `undefined`,顺带保证"只应用一次":用户之后自己移动光标不会被拽回去。
   */
  const pendingCursor = useRef<number | undefined>(undefined);

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
        if (!update.docChanged && !update.selectionSet) return;
        const report = onTriggerChangeRef.current;
        if (!report) return;
        const range = update.state.selection.main;
        // 有选区时不弹:那是在选中一段,不是在打字。
        if (!range.empty) {
          report(null);
          return;
        }
        const found = detectTrigger(update.state.doc.toString(), range.head);
        if (!found) {
          report(null);
          return;
        }
        /* 光标被滚出可视区时 `coordsAtPos` 给 null。这时**不能**据此关掉菜单 ——
           触发序列还在、用户还在打字,关掉就是"补全偶尔莫名消失"。退回编辑器自己的
           矩形,菜单贴在编辑器左上角,位置不理想但功能完整。
           (jsdom 没有排版引擎,那里 `coordsAtPos` 恒为 null —— 如果这里直接关,整套
           菜单在测试里就永远不出现,所有交互都没法验。) */
        const rect = update.view.coordsAtPos(range.head);
        const box = update.view.contentDOM.getBoundingClientRect();
        report({
          ...found,
          coords: rect ? { x: rect.left, y: rect.bottom } : { x: box.left, y: box.top },
        });
      }),
      /* 菜单开着时抢方向键 / 回车 / Tab / Esc。`Prec.highest` 是必须的:
         `basicSetup` 的 `defaultKeymap` 已经绑了这些键,同优先级下它在前面就赢了。 */
      Prec.highest(
        keymap.of(
          (["ArrowUp", "ArrowDown", "Enter", "Tab", "Escape"] as const).map((key) => ({
            key,
            run: () => onTriggerKeyRef.current?.(key) ?? false,
          })),
        ),
      ),
      // WYSIWYG 装饰。放在最后:它的 StateField 要能看到前面 extension 的效果。
      ...(wysiwyg ? wysiwygMarkdown : []),
      ...(attachments ? [attachmentContext.of(attachments)] : []),
      // 粘贴 / 拖入图片。走 CodeMirror 的 domEventHandlers 而不是在外层 div 上挂
      // React 的 onPaste:CodeMirror 的 contentDOM 是它自己管的,外层拿到的
      // paste 事件里 clipboardData 已经被它处理过了。
      EditorView.domEventHandlers({
        paste: (event, editor) => {
          const files = imageFilesFrom(event.clipboardData);
          if (files.length === 0) return false;
          return onDropFiles?.(files, editor.state.selection.main.from) ?? false;
        },
        drop: (event, editor) => {
          const files = imageFilesFrom(event.dataTransfer);
          if (files.length === 0) return false;
          // 落点按鼠标位置算,不是当前光标 —— 用户拖到哪就插到哪。
          const at =
            editor.posAtCoords({ x: event.clientX, y: event.clientY }) ??
            editor.state.selection.main.from;
          return onDropFiles?.(files, at) ?? false;
        },
      }),
    ],
    [ariaLabel, attachments, onDropFiles, onSelectionChange, wysiwyg],
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
      selectedText: () => {
        const editor = view();
        if (!editor) return "";
        const range = editor.state.selection.main;
        return editor.state.doc.sliceString(range.from, range.to);
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
      replaceSelection: (text) => {
        const editor = view();
        if (!editor) return;
        editor.dispatch(editor.state.replaceSelection(text), { scrollIntoView: true });
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
      posAtClientPoint: (x, y) => {
        const editor = view();
        if (!editor) return null;
        const rect = editor.scrollDOM.getBoundingClientRect();
        if (x < rect.left || x > rect.right || y < rect.top || y > rect.bottom) return null;
        // `precise: false` 让它给最近的位置而不是 null —— 落在最后一行下方的空白
        // 处也该有个落点(文末),不然拖到编辑器下半部分会没反应。
        return editor.posAtCoords({ x, y }, false);
      },
    }),
    [],
  );

  const handleChange = useCallback((next: string) => onChange(next), [onChange]);

  /** 把光标放到 `at`(夹到文档长度内)。 */
  const placeCursor = useCallback((view: EditorView, at: number) => {
    pendingCursor.current = undefined;
    // 文档可能比落点短(笔记在外部被截断过),越界的选区会让 CodeMirror 抛。
    const clamped = Math.min(at, view.state.doc.length);
    view.dispatch({ selection: EditorSelection.cursor(clamped), scrollIntoView: true });
  }, []);

  /* 收下一个新的落点。
   *
   * view 已经在场就当场落 —— 这是"笔记正文比编辑器晚到"那一路:反链指向的笔记还
   * 没读入时,点下去只能先挂一个空编辑器,偏移要等正文回来才算得出来,那时不会再
   * 有第二次挂载。view 还没建好则记下来,由 `onCreateEditor` 消费。 */
  useEffect(() => {
    if (initialCursorOffset === undefined) return;
    const view = cmRef.current?.view;
    if (view) placeCursor(view, initialCursorOffset);
    else pendingCursor.current = initialCursorOffset;
  }, [initialCursorOffset, placeCursor]);

  /** ReactCodeMirror 把 view 建好后回调。这是能安全设滚动和光标的最早时机。 */
  const handleCreateEditor = useCallback(
    (view: EditorView) => {
      const cursor = pendingCursor.current;
      if (cursor !== undefined) placeCursor(view, cursor);
      if (scrollApplied.current || initialScrollRatio === undefined) return;
      scrollApplied.current = true;
      const scroller = view.scrollDOM;
      const max = scroller.scrollHeight - scroller.clientHeight;
      scroller.scrollTop = initialScrollRatio * Math.max(0, max);
    },
    [initialScrollRatio, placeCursor],
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
