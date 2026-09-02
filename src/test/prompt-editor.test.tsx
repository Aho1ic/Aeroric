import { fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import { useRef } from "react";
import type { Project, PromptSkill } from "../types";
import type { DshSlashCommand } from "../dshSlashCommands";
import type {
  CrossProjectRef,
  FileEntry,
  MentionItem,
} from "../components/new-task/MentionPopover";

/**
 * 提示词编辑器(contenteditable)。
 *
 * 这里的每个 bug 都直接作用在「用户刚敲完、还没发出去」的文本上:序列化错一个字符,
 * 发给 agent 的就是错的 prompt;chip 删除算错一个兄弟节点,用户的 @文件引用凭空消失;
 * slash 判定放松一格,`src/App.tsx` 里的斜杠就会把技能面板顶出来。
 * 所以断言全部落在**可观测产物**上 —— serialize() 的字符串、onContentChange 的载荷、
 * onSubmit/onSelectXxx 的调用,而不是内部状态。
 */

// APP_PLATFORM 在模块初始化时按 navigator 定死,jsdom 里会算成 "other"(Ctrl 系)。
// 固定成 macos,让提交/换行分支走 metaKey,与绝大多数用户的实际路径一致;
// 平台矩阵本身由 send-shortcut.test.ts 覆盖,这里只需要一个确定的平台。
vi.mock("../platform", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../platform")>();
  return { ...actual, APP_PLATFORM: "macos" as const };
});

const { PromptEditor, usePromptEditor, normalizeEditorCompositionText } =
  await import("../components/new-task/PromptEditor");
type Handle = import("../components/new-task/PromptEditor").PromptEditorHandle;
type Content = import("../components/new-task/PromptEditor").PromptEditorContent;
type SuggestionQuery = import("../components/new-task/PromptEditor").PromptSuggestionQuery;

// ---------------------------------------------------------------------------
// 固定夹具
// ---------------------------------------------------------------------------

const tsFile: FileEntry = { name: "App.tsx", path: "src/App.tsx", dir: "src", ext: "tsx" };
const mdFile: FileEntry = { name: "README.md", path: "README.md", dir: "", ext: "md" };
const crossRef: CrossProjectRef = { id: "p2", path: "/repos/other", name: "other" };
const project: Project = {
  id: "p2",
  name: "other",
  path: "/repos/other",
  lastOpenedAt: 0,
};
const skillA: PromptSkill = { name: "review", path: "/skills/review" };
const skillB: PromptSkill = { name: "refactor", path: "/skills/refactor" };
const cmdA: DshSlashCommand = {
  name: "compact",
  descriptionKey: "dsh.slash.compact",
  hasArg: false,
};
const cmdB: DshSlashCommand = { name: "export", descriptionKey: "dsh.slash.export", hasArg: false };

// ---------------------------------------------------------------------------
// Selection 辅助
// ---------------------------------------------------------------------------

/** 折叠光标落在 `node` 的 `offset` 处。node 可以是 Text 也可以是元素(此时 offset 是子节点下标)。 */
function caretAt(node: Node, offset: number) {
  const range = document.createRange();
  range.setStart(node, offset);
  range.collapse(true);
  const sel = window.getSelection()!;
  sel.removeAllRanges();
  sel.addRange(range);
  return range;
}

/** 非折叠选区,用来测「有选中内容时」的分支。 */
function selectRange(start: Node, startOffset: number, end: Node, endOffset: number) {
  const range = document.createRange();
  range.setStart(start, startOffset);
  range.setEnd(end, endOffset);
  const sel = window.getSelection()!;
  sel.removeAllRanges();
  sel.addRange(range);
  return range;
}

function clearSelection() {
  window.getSelection()!.removeAllRanges();
}

/**
 * 盯住事件处理器里抛出的异常。
 *
 * jsdom 会把 listener 抛的异常转成 window 的 `error` 事件,而不是让 fireEvent 抛出来 ——
 * 也就是说「守卫失效、读了 undefined.name 崩掉」和「守卫生效、什么都没做」在
 * `expect(spy).not.toHaveBeenCalled()` 上完全同构(变异测试实测过:摘掉越界守卫全绿)。
 * 所以凡是断言「什么都没发生」的用例,都得同时确认没崩。
 */
function watchErrors() {
  const errors: string[] = [];
  const onError = (e: ErrorEvent) => {
    errors.push(e.message || String(e.error));
  };
  window.addEventListener("error", onError);
  return {
    errors,
    stop: () => window.removeEventListener("error", onError),
  };
}

/** 造一个和组件内部 createChipElement 同构的 chip(仅 dataset 部分,序列化只读 dataset)。 */
function chipHtml(filePath: string, projectPath?: string, label = "chip") {
  const proj = projectPath ? ` data-project-path="${projectPath}"` : "";
  return `<span contenteditable="false" data-file-path="${filePath}"${proj}><span>${label}</span></span>`;
}

// ---------------------------------------------------------------------------
// 宿主
// ---------------------------------------------------------------------------

type HostProps = Partial<React.ComponentProps<typeof PromptEditor>>;

/** 回调 prop 的名字。 */
type CallbackProp =
  | "onSetIsEmpty"
  | "onUpdateSuggestions"
  | "onDismissSuggestions"
  | "onSelectFile"
  | "onSelectProject"
  | "onSelectSkill"
  | "onSelectDshCommand"
  | "onSetMentionIndex"
  | "onSetSkillIndex"
  | "onSetDshCommandIndex"
  | "onSubmit"
  | "onContentChange"
  | "onPasteLargeText";

/**
 * 每个 spy 都保留组件声明的那个签名(而不是宽松的 `Mock`)——
 * 这样 `toHaveBeenCalledWith` 的实参类型也归 tsc 管,写错参数在类型检查阶段就挂。
 */
type Spies = {
  [K in CallbackProp]: Mock<NonNullable<React.ComponentProps<typeof PromptEditor>[K]>>;
};

interface Captured {
  handle: Handle;
  editor: HTMLDivElement;
  isComposingRef: React.MutableRefObject<boolean>;
  spies: Spies;
}

let captured!: Captured;

function makeSpies(): Spies {
  return {
    onSetIsEmpty: vi.fn(),
    onUpdateSuggestions: vi.fn(),
    onDismissSuggestions: vi.fn(),
    onSelectFile: vi.fn(),
    onSelectProject: vi.fn(),
    onSelectSkill: vi.fn(),
    onSelectDshCommand: vi.fn(),
    onSetMentionIndex: vi.fn(),
    onSetSkillIndex: vi.fn(),
    onSetDshCommandIndex: vi.fn(),
    onSubmit: vi.fn(),
    onContentChange: vi.fn(),
    onPasteLargeText: vi.fn(),
  };
}

function Host({ overrides, spies }: { overrides: HostProps; spies: Spies }) {
  const { editorRef, isComposingRef, handle } = usePromptEditor();
  const spiesRef = useRef(spies);
  captured = {
    handle,
    editor: editorRef.current as HTMLDivElement,
    isComposingRef,
    spies: spiesRef.current,
  };
  return (
    <PromptEditor
      editorRef={editorRef}
      isComposingRef={isComposingRef}
      mentionItems={[]}
      mentionIndex={0}
      skillItems={[]}
      skillIndex={0}
      skillMenuOpen={false}
      skillCommandPrefix="/"
      dshCommandItems={[]}
      dshCommandIndex={0}
      dshCommandMenuOpen={false}
      slashSuggestionKind="skill"
      placeholder="Ask anything"
      onSetIsEmpty={spies.onSetIsEmpty}
      onUpdateSuggestions={spies.onUpdateSuggestions}
      onDismissSuggestions={spies.onDismissSuggestions}
      onSelectFile={spies.onSelectFile}
      onSelectProject={spies.onSelectProject}
      onSelectSkill={spies.onSelectSkill}
      onSelectDshCommand={spies.onSelectDshCommand}
      onSetMentionIndex={spies.onSetMentionIndex}
      onSetSkillIndex={spies.onSetSkillIndex}
      onSetDshCommandIndex={spies.onSetDshCommandIndex}
      sendShortcut="mod_enter"
      onSubmit={spies.onSubmit}
      onContentChange={spies.onContentChange}
      onPasteLargeText={spies.onPasteLargeText}
      {...overrides}
    />
  );
}

function setup(overrides: HostProps = {}) {
  const spies = makeSpies();
  const view = render(<Host overrides={overrides} spies={spies} />);
  const editor = view.container.querySelector<HTMLDivElement>(".prompt-editor")!;
  // 先聚焦一次,把编辑器变成 activeElement。
  //
  // 必须这么做:jsdom 对「还没聚焦」的 contenteditable 调 focus() 会把选区折叠到
  // (element, 0) —— 真实浏览器也是这个行为(点进一个未聚焦的编辑器,光标落到开头)。
  // 而组件内部 insertSkill / insertText 都会调 editor.focus(),如果这里不先聚焦,
  // 测试摆好的光标会在函数内部被冲掉,插入位置全部跑到最前面。
  // 聚焦之后 focus() 是空操作(jsdom 与浏览器一致),光标才留得住 ——
  // 这也正是真实场景:用户是在已聚焦的编辑器里打字。
  editor.focus();
  return {
    ...view,
    editor,
    spies,
    handle: () => captured.handle,
    isComposingRef: () => captured.isComposingRef,
    rerenderWith: (next: HostProps) =>
      view.rerender(<Host overrides={{ ...overrides, ...next }} spies={spies} />),
  };
}

/** 最后一次 onContentChange 的载荷。 */
function lastContent(spies: Spies): Content {
  const calls = spies.onContentChange.mock.calls;
  return calls[calls.length - 1][0] as Content;
}

/** 最后一次 onUpdateSuggestions 的载荷。 */
function lastQuery(spies: Spies): SuggestionQuery {
  const calls = spies.onUpdateSuggestions.mock.calls;
  return calls[calls.length - 1][0] as SuggestionQuery;
}

beforeEach(() => {
  clearSelection();
});

afterEach(() => {
  vi.restoreAllMocks();
});

// ---------------------------------------------------------------------------
// usePromptEditor —— 编辑器还没挂载时
// ---------------------------------------------------------------------------

/** 只用 hook、不渲染 PromptEditor:editorRef.current 恒为 null,专测未挂载分支。 */
function HookOnly({ onReady }: { onReady: (handle: Handle) => void }) {
  const { handle } = usePromptEditor();
  onReady(handle);
  return null;
}

describe("usePromptEditor 在编辑器未挂载时", () => {
  let handle!: Handle;
  beforeEach(() => {
    render(<HookOnly onReady={(h) => (handle = h)} />);
  });

  it("serialize() 返回空串而不是抛错", () => {
    expect(handle.serialize()).toBe("");
  });

  it("clear() / focus() 是安全的空操作", () => {
    expect(() => handle.clear()).not.toThrow();
    expect(() => handle.focus()).not.toThrow();
  });

  it("insertSkill() / insertText() 返回 false", () => {
    // 返回值就是调用方的判据:NewTaskView 拿 false 时不关面板、不清空输入,
    // 所以「未挂载返回 false」是契约的一部分,不只是防御。
    expect(handle.insertSkill("review", "/")).toBe(false);
    expect(handle.insertText("hi")).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// serialize() —— 发给 agent 的最终文本
// ---------------------------------------------------------------------------

describe("serialize()", () => {
  /** 直接铺 innerHTML 再 serialize。这是「发出去的到底是什么」的唯一出口。 */
  function serializeHtml(html: string) {
    const { editor, handle } = setup();
    editor.innerHTML = html;
    return handle().serialize();
  }

  it("纯文本按原样返回并去掉首尾空白", () => {
    expect(serializeHtml("  hello world  ")).toBe("hello world");
  });

  it("空编辑器返回空串", () => {
    expect(serializeHtml("")).toBe("");
  });

  it("<br> 变成换行", () => {
    expect(serializeHtml("a<br>b")).toBe("a\nb");
  });

  it("同项目 chip 序列化成 @路径", () => {
    // 显示的是 "App.tsx",发出去必须是完整路径 —— agent 拿相对路径才能找到文件。
    expect(serializeHtml(`x ${chipHtml("src/App.tsx", undefined, "App.tsx")} y`)).toBe(
      "x @src/App.tsx y",
    );
  });

  it("跨项目 chip 序列化成 @项目绝对路径/文件相对路径", () => {
    expect(serializeHtml(chipHtml("src/lib.rs", "/repos/other", "other/lib.rs"))).toBe(
      "@/repos/other/src/lib.rs",
    );
  });

  it("chip 内部的 <span> 文字不参与序列化", () => {
    // chip 是 dataset 驱动的:命中 data-file-path 就整块换成路径,不再往下走子节点。
    // 否则 "other" / "/" / "lib.rs" 这几个装饰性 span 会被拼进 prompt。
    const out = serializeHtml(chipHtml("src/lib.rs", "/repos/other", "other/lib.rs"));
    expect(out).not.toContain("lib.rs\n");
    expect(out).toBe("@/repos/other/src/lib.rs");
  });

  it("<div> 之间补一个换行", () => {
    expect(serializeHtml("<div>a</div><div>b</div>")).toBe("a\nb");
  });

  it("<p> 与 <div> 同等对待", () => {
    expect(serializeHtml("<p>a</p><p>b</p>")).toBe("a\nb");
  });

  it("首个 <div> 前不补换行", () => {
    // parts 为空时不插 —— 否则每次都以 "\n" 起头,靠 trim() 兜底会掩盖问题,
    // 但 "a\n\nb" 这种中间的多余空行 trim 收不掉,所以这条要单独钉。
    expect(serializeHtml("<div>a</div>")).toBe("a");
  });

  it("已经以换行结尾时不再重复补换行", () => {
    // <br> 已经压了一个 "\n",紧跟的 <div> 不能再压一个。
    expect(serializeHtml("a<br><div>b</div>")).toBe("a\nb");
  });

  it("非块级元素(span / b)只贡献自己的文字", () => {
    expect(serializeHtml("<span>a</span><b>b</b>")).toBe("ab");
  });

  it("嵌套块级元素只补一层换行", () => {
    expect(serializeHtml("<div><div>a</div></div>")).toBe("a");
  });

  it("不换行空格(nbsp)还原成普通空格", () => {
    // contenteditable 连续敲空格时浏览器会塞 nbsp,原样发出去 agent 侧的
    // 路径/参数匹配会失败。
    expect(serializeHtml("a\u00a0\u00a0b")).toBe("a  b");
  });

  it("零宽空格被删掉", () => {
    expect(serializeHtml("a\u200bb")).toBe("ab");
  });

  it("注释节点被忽略", () => {
    expect(serializeHtml("a<!-- note -->b")).toBe("ab");
  });

  it("chip + 换行 + 文本的混排保持顺序", () => {
    expect(
      serializeHtml(`review ${chipHtml("src/a.ts")}<br>and ${chipHtml("src/b.ts")} please`),
    ).toBe("review @src/a.ts\nand @src/b.ts please");
  });
});

describe("clear() / focus()", () => {
  it("clear() 清空 innerHTML", () => {
    const { editor, handle } = setup();
    editor.innerHTML = "a<br>b";
    handle().clear();
    expect(editor.innerHTML).toBe("");
    expect(handle().serialize()).toBe("");
  });

  it("focus() 落到编辑器上", () => {
    const { editor, handle } = setup();
    const spy = vi.spyOn(editor, "focus");
    handle().focus();
    expect(spy).toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// 建议查询 —— 决定弹不弹面板、弹哪个面板
// ---------------------------------------------------------------------------

describe("建议查询(slash / @ 的识别)", () => {
  /**
   * 铺文本 → 放光标 → 触发 input,读回 onUpdateSuggestions 的载荷。
   *
   * 用 input 而不是 select 驱动:两者最终都调同一个 updateSuggestions,
   * 但 React 的 onSelect 是从 keyup/mousedown 等原生事件合成的,
   * 在 jsdom 里还依赖 activeElement,拿不到稳定的触发点。
   */
  function queryFor(text: string, caretOffset = text.length, overrides: HostProps = {}) {
    const { editor, spies } = setup(overrides);
    editor.textContent = text;
    caretAt(editor.firstChild!, caretOffset);
    fireEvent.input(editor);
    return lastQuery(spies);
  }

  it("没有选区时不出建议", () => {
    const { editor, spies } = setup();
    editor.textContent = "/rev";
    clearSelection();
    fireEvent.input(editor);
    expect(lastQuery(spies)).toBeNull();
  });

  it("既没有 / 也没有 @ 时不出建议", () => {
    expect(queryFor("just typing")).toBeNull();
  });

  it("开头的 /rev 出技能建议", () => {
    expect(queryFor("/rev")).toEqual({ kind: "skill", query: "rev" });
  });

  it("只敲一个 / 时 query 是空串(而不是不出建议)", () => {
    // 空 query 要出完整目录,这是用户敲 "/" 后看到全部命令的前提。
    expect(queryFor("/")).toEqual({ kind: "skill", query: "" });
  });

  it("dsh 会话里同一个 / 出的是命令建议", () => {
    expect(queryFor("/comp", 5, { slashSuggestionKind: "dsh-command" })).toEqual({
      kind: "dsh-command",
      query: "comp",
    });
  });

  it("句中的 / 不出建议", () => {
    // "hello /rev":slash 前面有实字,不是提示词开头。
    expect(queryFor("hello /rev")).toBeNull();
  });

  it("src/App.tsx 这样的相对路径不会顶出技能面板", () => {
    expect(queryFor("src/App.tsx")).toBeNull();
  });

  it("/Users/foo 这样的绝对路径不会顶出技能面板", () => {
    // 第一个 / 在开头,但 lastIndexOf 取到的是第二个 —— 它前面有 "/Users",
    // 所以判定为非开头。这正是「只看最后一个 /」还能挡住绝对路径的原因。
    expect(queryFor("/Users/foo")).toBeNull();
  });

  it("query 里出现空格后收起建议", () => {
    expect(queryFor("/rev now")).toBeNull();
  });

  it("query 里出现换行后收起建议", () => {
    expect(queryFor("/rev\nnow")).toBeNull();
  });

  it("/ 前面只有空白仍算提示词开头", () => {
    expect(queryFor("  /rev")).toEqual({ kind: "skill", query: "rev" });
  });

  it("/ 前面有 chip 时不算提示词开头", () => {
    // chip 不贡献可见文字给 textContent 之外的判断,但 Range.toString() 会把它算进去 ——
    // 这是「@文件 之后再敲斜杠不该弹技能面板」的依据。
    const { editor, spies } = setup();
    editor.innerHTML = `${chipHtml("src/a.ts")} /rev`;
    const tail = editor.lastChild as Text;
    caretAt(tail, tail.length);
    fireEvent.input(editor);
    expect(lastQuery(spies)).toBeNull();
  });

  it("光标落在编辑器外的文本节点上时按提示词开头处理", () => {
    // 走的是 isPromptStartOffset 里「找不到 contenteditable 祖先」的分支:
    // 没有编辑器可比对,就只看节点内部的前缀。
    const stray = document.createTextNode("/rev");
    document.body.appendChild(stray);
    try {
      const { editor, spies } = setup();
      caretAt(stray, stray.length);
      fireEvent.input(editor);
      expect(lastQuery(spies)).toEqual({ kind: "skill", query: "rev" });
    } finally {
      stray.remove();
    }
  });

  it("@ 出文件建议", () => {
    expect(queryFor("@App")).toEqual({ kind: "mention", query: "App" });
  });

  it("@ 后面出现空格就收起", () => {
    expect(queryFor("@App tsx")).toBeNull();
  });

  it("句中的 @ 照样出建议(与 / 不同,@ 不限位置)", () => {
    expect(queryFor("please read @App")).toEqual({ kind: "mention", query: "App" });
  });

  it("/ 与 @ 同时存在时 / 优先", () => {
    // "/@x":slash 在开头且 query 无空格 → 技能优先,@ 不再参与。
    expect(queryFor("/@x")).toEqual({ kind: "skill", query: "@x" });
  });

  it("/ 不合格时回落到 @", () => {
    // "/rev @x":slash 的 query 含空格被否掉,才轮到 @。
    expect(queryFor("/rev @x")).toEqual({ kind: "mention", query: "x" });
  });

  it("有选中内容(非折叠光标)时不出建议", () => {
    const { editor, spies } = setup();
    editor.textContent = "/review";
    selectRange(editor.firstChild!, 1, editor.firstChild!, 4);
    fireEvent.input(editor);
    expect(lastQuery(spies)).toBeNull();
  });

  it("光标落在元素节点上时不出建议", () => {
    const { editor, spies } = setup();
    editor.innerHTML = "<div>/rev</div>";
    caretAt(editor, 0);
    fireEvent.input(editor);
    expect(lastQuery(spies)).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// insertSkill() / insertText()
// ---------------------------------------------------------------------------

describe("insertSkill()", () => {
  it("没有 slash 上下文时返回 false 且不改文本", () => {
    const { editor, handle } = setup();
    editor.textContent = "hello";
    caretAt(editor.firstChild!, 5);
    expect(handle().insertSkill("review", "/")).toBe(false);
    expect(editor.textContent).toBe("hello");
  });

  it("没有选区时返回 false", () => {
    const { editor, handle } = setup();
    editor.textContent = "/rev";
    clearSelection();
    expect(handle().insertSkill("review", "/")).toBe(false);
    expect(editor.textContent).toBe("/rev");
  });

  it("把已敲的片段整体换成 /技能名 加一个空格", () => {
    const { editor, handle } = setup();
    editor.textContent = "/rev";
    caretAt(editor.firstChild!, 4);
    expect(handle().insertSkill("review", "/")).toBe(true);
    expect(editor.textContent).toBe("/review ");
  });

  it("codex 会话用 $ 前缀", () => {
    const { editor, handle } = setup();
    editor.textContent = "/ref";
    caretAt(editor.firstChild!, 4);
    expect(handle().insertSkill("refactor", "$")).toBe(true);
    expect(editor.textContent).toBe("$refactor ");
  });

  it("/ 前面的空白留着不动", () => {
    // 替换范围的起点是 slashOffset,不是 0。写死 0 的话这条会挂:
    // 用户在 " /rev" 上选技能,前导空白会被一起吃掉。
    const { editor, handle } = setup();
    editor.textContent = "  /rev";
    caretAt(editor.firstChild!, 6);
    expect(handle().insertSkill("review", "/")).toBe(true);
    expect(editor.textContent).toBe("  /review ");
  });

  it("只替换 / 到光标之间的片段,光标后的文字留在原处", () => {
    const { editor, handle } = setup();
    editor.textContent = "/revXY";
    caretAt(editor.firstChild!, 4);
    expect(handle().insertSkill("review", "/")).toBe(true);
    expect(editor.textContent).toBe("/review XY");
  });

  it("插入后光标停在技能名后面的空格之后", () => {
    // 用「紧接着再插一段文字」验证光标落点 —— 这是用户敲完技能名继续打字的落点,
    // 断言 Selection 的内部字段容易被 jsdom 实现细节带偏,插一次文字最直接。
    const { editor, handle } = setup();
    editor.textContent = "/revXY";
    caretAt(editor.firstChild!, 4);
    handle().insertSkill("review", "/");
    handle().insertText("!");
    expect(editor.textContent).toBe("/review !XY");
  });

  it("插入后把焦点还给编辑器", () => {
    const { editor, handle } = setup();
    editor.textContent = "/rev";
    caretAt(editor.firstChild!, 4);
    const spy = vi.spyOn(editor, "focus");
    handle().insertSkill("review", "/");
    expect(spy).toHaveBeenCalled();
  });
});

describe("insertText()", () => {
  it("没有选区时返回 false", () => {
    const { handle } = setup();
    clearSelection();
    expect(handle().insertText("hi")).toBe(false);
  });

  it("在光标处插入并返回 true", () => {
    const { editor, handle } = setup();
    editor.textContent = "ab";
    caretAt(editor.firstChild!, 1);
    expect(handle().insertText("X")).toBe(true);
    expect(editor.textContent).toBe("aXb");
  });

  it("有选中内容时替换掉选中的部分", () => {
    const { editor, handle } = setup();
    editor.textContent = "abcd";
    selectRange(editor.firstChild!, 1, editor.firstChild!, 3);
    expect(handle().insertText("X")).toBe(true);
    expect(editor.textContent).toBe("aXd");
  });

  it("连续插入按顺序衔接", () => {
    const { editor, handle } = setup();
    editor.textContent = "";
    caretAt(editor, 0);
    handle().insertText("a");
    handle().insertText("b");
    expect(editor.textContent).toBe("ab");
  });

  it("插入前先聚焦编辑器", () => {
    const { editor, handle } = setup();
    editor.textContent = "";
    caretAt(editor, 0);
    const spy = vi.spyOn(editor, "focus");
    handle().insertText("a");
    expect(spy).toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// normalizeEditorCompositionText —— 拼音残留清理
// ---------------------------------------------------------------------------

describe("normalizeEditorCompositionText", () => {
  function makeEditor(html: string) {
    const el = document.createElement("div");
    el.contentEditable = "true";
    el.innerHTML = html;
    document.body.appendChild(el);
    return el;
  }

  const created: HTMLDivElement[] = [];
  function editorWith(html: string) {
    const el = makeEditor(html);
    created.push(el);
    return el;
  }
  afterEach(() => {
    for (const el of created.splice(0)) el.remove();
  });

  it("空编辑器返回 false", () => {
    expect(normalizeEditorCompositionText(editorWith(""))).toBe(false);
  });

  it("文本无需归一化时返回 false 且不动 DOM", () => {
    const el = editorWith("hello");
    expect(normalizeEditorCompositionText(el)).toBe(false);
    expect(el.textContent).toBe("hello");
  });

  it("重复的拼音音节被收成一份", () => {
    // WebKitGTK 上 IME 提交后会把音节重放一次,变成 "ni'ni" 这种。
    const el = editorWith("ni'ni");
    expect(normalizeEditorCompositionText(el)).toBe(true);
    expect(el.textContent).toBe("ni");
  });

  it("单个隔音撇号被删掉", () => {
    const el = editorWith("he'llo");
    expect(normalizeEditorCompositionText(el)).toBe(true);
    expect(el.textContent).toBe("hello");
  });

  it("中文后面跟带撇号的拼音残留时只留中文", () => {
    const el = editorWith("你好ni'hao");
    expect(normalizeEditorCompositionText(el)).toBe(true);
    expect(el.textContent).toBe("你好");
  });

  it("多个文本节点逐个归一化", () => {
    const el = editorWith("<span>ni'ni</span><span>he'llo</span>");
    expect(normalizeEditorCompositionText(el)).toBe(true);
    expect(el.textContent).toBe("nihello");
  });

  it("改过之后把光标收到末尾", () => {
    const el = editorWith("<span>ok</span><span>ni'ni</span>");
    normalizeEditorCompositionText(el);
    const sel = window.getSelection()!;
    expect(sel.isCollapsed).toBe(true);
    // 落点用「再插一段文字」验证:必须接在最后,而不是回到某个中间节点。
    const range = sel.getRangeAt(0);
    range.insertNode(document.createTextNode("!"));
    expect(el.textContent).toBe("okni!");
  });
});

// ---------------------------------------------------------------------------
// onInput —— 空态与内容回传
// ---------------------------------------------------------------------------

describe("输入事件", () => {
  it("有文字时报告非空", () => {
    const { editor, spies } = setup();
    editor.textContent = "hi";
    fireEvent.input(editor);
    expect(spies.onSetIsEmpty).toHaveBeenLastCalledWith(false);
  });

  it("只剩空白时报告为空", () => {
    // 空态决定发送按钮的 disabled,"   " 必须算空,否则用户能把纯空白发出去。
    const { editor, spies } = setup();
    editor.textContent = "   ";
    fireEvent.input(editor);
    expect(spies.onSetIsEmpty).toHaveBeenLastCalledWith(true);
  });

  it("完全空时报告为空", () => {
    const { editor, spies } = setup();
    editor.innerHTML = "";
    fireEvent.input(editor);
    expect(spies.onSetIsEmpty).toHaveBeenLastCalledWith(true);
  });

  it("只有 chip、没有文字时仍算非空", () => {
    // 只 @ 了一个文件就发送是合法用法,不能因为 textContent 空就禁用发送。
    const { editor, spies } = setup();
    editor.innerHTML = chipHtml("src/a.ts", undefined, "");
    fireEvent.input(editor);
    expect(spies.onSetIsEmpty).toHaveBeenLastCalledWith(false);
  });

  it("回传 html / text / hasChips 三件套", () => {
    const { editor, spies } = setup();
    editor.innerHTML = `hi ${chipHtml("src/a.ts", undefined, "a.ts")}`;
    fireEvent.input(editor);
    const content = lastContent(spies);
    expect(content.text).toBe("hi a.ts");
    expect(content.hasChips).toBe(true);
    expect(content.html).toContain('data-file-path="src/a.ts"');
  });

  it("没有 chip 时 hasChips 为 false", () => {
    const { editor, spies } = setup();
    editor.textContent = "plain";
    fireEvent.input(editor);
    expect(lastContent(spies).hasChips).toBe(false);
  });

  it("父组件没传 onContentChange 时不报错", () => {
    const { editor } = setup({ onContentChange: undefined });
    expect(() => {
      editor.textContent = "hi";
      fireEvent.input(editor);
    }).not.toThrow();
  });

  it("输入时顺带清理拼音残留", () => {
    // Linux WebKitGTK 上 IME 会把音节重放,靠这一步在 input 阶段收掉。
    const { editor } = setup();
    editor.textContent = "ni'ni";
    fireEvent.input(editor);
    expect(editor.textContent).toBe("ni");
  });

  it("组字过程中完全不处理", () => {
    // 关键防线:组字中途插手会在 Linux WebKitGTK 上把预编辑文本复制一份。
    const { editor, spies, isComposingRef } = setup();
    isComposingRef().current = true;
    editor.textContent = "ni'ni";
    fireEvent.input(editor);
    expect(editor.textContent).toBe("ni'ni");
    expect(spies.onSetIsEmpty).not.toHaveBeenCalled();
    expect(spies.onContentChange).not.toHaveBeenCalled();
    expect(spies.onUpdateSuggestions).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// 组字事件
// ---------------------------------------------------------------------------

describe("组字事件", () => {
  it("compositionstart 打开组字标记", () => {
    const { editor, isComposingRef } = setup();
    fireEvent.compositionStart(editor);
    expect(isComposingRef().current).toBe(true);
  });

  it("compositionupdate 期间直接报告非空", () => {
    // 预编辑文本还没进 textContent,但用户已经在打字了 —— 此时禁用发送按钮会闪。
    const { editor, spies, isComposingRef } = setup();
    fireEvent.compositionStart(editor);
    fireEvent.compositionUpdate(editor, { data: "ni" });
    expect(isComposingRef().current).toBe(true);
    expect(spies.onSetIsEmpty).toHaveBeenLastCalledWith(false);
    expect(spies.onContentChange).toHaveBeenCalled();
  });

  it("compositionend 关掉组字标记并归一化落定的文本", () => {
    const { editor, spies, isComposingRef } = setup();
    fireEvent.compositionStart(editor);
    fireEvent.compositionUpdate(editor, { data: "ni" });
    editor.textContent = "ni'ni";
    fireEvent.compositionEnd(editor, { data: "ni" });
    expect(isComposingRef().current).toBe(false);
    expect(editor.textContent).toBe("ni");
    expect(spies.onSetIsEmpty).toHaveBeenLastCalledWith(false);
    expect(spies.onUpdateSuggestions).toHaveBeenCalled();
  });

  it("compositionend 之后空编辑器报告为空", () => {
    const { editor, spies } = setup();
    fireEvent.compositionStart(editor);
    fireEvent.compositionEnd(editor, { data: "" });
    expect(spies.onSetIsEmpty).toHaveBeenLastCalledWith(true);
  });

  it("compositionend 没带 data 时照样走完归一化与回传", () => {
    // 部分 IME 的 compositionend 不带 data,组件回落到 compositionupdate 存的预编辑文本。
    //
    // 说明:这条只钉住「不抛错、归一化与回传照走」。那份回落文本的唯一用途是喂给
    // `ignoredPostCompositionCandidatesRef`,而读它的 handleBeforeInputCapture 在
    // React 19 下不可达(见本文件末尾「IME 重放去重层」一节),所以回落本身
    // 目前没有可观测差异 —— 不为它编造断言。
    const { editor, spies } = setup();
    fireEvent.compositionStart(editor);
    fireEvent.compositionUpdate(editor, { data: "你好" });
    editor.textContent = "你好";
    fireEvent.compositionEnd(editor, { data: "" });
    expect(editor.textContent).toBe("你好");
    expect(lastContent(spies).text).toBe("你好");
  });
});

// ---------------------------------------------------------------------------
// 粘贴
// ---------------------------------------------------------------------------

/** 造一次粘贴。jsdom 的 ClipboardEvent 不带 clipboardData,得自己挂。 */
function firePaste(
  editor: HTMLElement,
  opts: { text?: string; types?: string[] } = {},
): { defaultPrevented: boolean } {
  const text = opts.text ?? "";
  const items = (opts.types ?? (text ? ["text/plain"] : [])).map((type) => ({ type }));
  const event = new Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "clipboardData", {
    value: { items, files: [], getData: () => text },
  });
  fireEvent(editor, event);
  return { defaultPrevented: event.defaultPrevented };
}

describe("粘贴", () => {
  it("剪贴板里有图片时交给父组件、不拦默认行为", () => {
    // 图片由 NewTaskView 统一处理成附件,这里必须原样放过 ——
    // 一旦 preventDefault,父层的 paste 监听拿不到 items,图片就丢了。
    const { editor, spies } = setup();
    editor.textContent = "";
    caretAt(editor, 0);
    const { defaultPrevented } = firePaste(editor, { types: ["image/png"] });
    expect(defaultPrevented).toBe(false);
    expect(spies.onContentChange).not.toHaveBeenCalled();
    expect(spies.onPasteLargeText).not.toHaveBeenCalled();
  });

  it("普通文本插到光标处并回传内容", () => {
    const { editor, spies } = setup();
    editor.textContent = "ab";
    caretAt(editor.firstChild!, 1);
    const { defaultPrevented } = firePaste(editor, { text: "X" });
    expect(defaultPrevented).toBe(true);
    expect(editor.textContent).toBe("aXb");
    expect(spies.onSetIsEmpty).toHaveBeenLastCalledWith(false);
    expect(lastContent(spies).text).toBe("aXb");
    expect(spies.onUpdateSuggestions).toHaveBeenCalled();
  });

  it("粘贴会替换掉选中的内容", () => {
    const { editor } = setup();
    editor.textContent = "abcd";
    selectRange(editor.firstChild!, 1, editor.firstChild!, 3);
    firePaste(editor, { text: "X" });
    expect(editor.textContent).toBe("aXd");
  });

  it("空文本只拦默认行为,不改内容", () => {
    const { editor, spies } = setup();
    editor.textContent = "ab";
    caretAt(editor.firstChild!, 1);
    const { defaultPrevented } = firePaste(editor, { types: ["text/plain"], text: "" });
    expect(defaultPrevented).toBe(true);
    expect(editor.textContent).toBe("ab");
    expect(spies.onContentChange).not.toHaveBeenCalled();
  });

  it("没有选区时不插入", () => {
    const { editor, spies } = setup();
    editor.textContent = "ab";
    clearSelection();
    firePaste(editor, { text: "X" });
    expect(editor.textContent).toBe("ab");
    expect(spies.onContentChange).not.toHaveBeenCalled();
  });

  it("1000 字以上转成附件、不塞进编辑器", () => {
    // 大段日志粘进 contenteditable 会把编辑器卡死,所以改走附件。
    const { editor, spies } = setup();
    editor.textContent = "";
    caretAt(editor, 0);
    const big = "x".repeat(1000);
    firePaste(editor, { text: big });
    expect(spies.onPasteLargeText).toHaveBeenCalledWith(big);
    expect(editor.textContent).toBe("");
  });

  it("999 字仍然直接插入", () => {
    // 边界钉在 1000:阈值写成 > 而不是 >= 时这条会挂。
    const { editor, spies } = setup();
    editor.textContent = "";
    caretAt(editor, 0);
    const text = "x".repeat(999);
    firePaste(editor, { text });
    expect(spies.onPasteLargeText).not.toHaveBeenCalled();
    expect(editor.textContent).toBe(text);
  });

  it("父组件没提供附件通道时,大段文本仍然插进编辑器", () => {
    const { editor, spies } = setup({ onPasteLargeText: undefined });
    editor.textContent = "";
    caretAt(editor, 0);
    const big = "x".repeat(1200);
    firePaste(editor, { text: big });
    expect(editor.textContent).toBe(big);
    expect(spies.onSetIsEmpty).toHaveBeenLastCalledWith(false);
  });
});

// ---------------------------------------------------------------------------
// 撤销
// ---------------------------------------------------------------------------

describe("撤销快捷键", () => {
  it("Cmd+Z 原样交给浏览器", () => {
    // contenteditable 的撤销栈是浏览器维护的,组件一旦 preventDefault 就永久废掉撤销。
    const { editor, spies } = setup();
    const ok = fireEvent.keyDown(editor, { key: "z", metaKey: true });
    expect(ok).toBe(true);
    expect(spies.onSubmit).not.toHaveBeenCalled();
  });

  it("Ctrl+Z 同样放过", () => {
    const { editor } = setup();
    expect(fireEvent.keyDown(editor, { key: "z", ctrlKey: true })).toBe(true);
  });

  it("撤销快捷键优先于 chip 删除之外的一切处理", () => {
    // 面板开着时也不能截胡 Cmd+Z —— 否则用户在候选列表里没法撤销刚插入的技能名。
    const { editor, spies } = setup({
      skillItems: [skillA],
      skillMenuOpen: true,
    });
    fireEvent.keyDown(editor, { key: "z", metaKey: true });
    expect(spies.onSetSkillIndex).not.toHaveBeenCalled();
    expect(spies.onDismissSuggestions).not.toHaveBeenCalled();
  });

  /*
   * 上面三条钉的是「Cmd/Ctrl+Z 没被吃掉」这个结果,钉不住**哪一行**保证了它。
   *
   * 变异测试结论:把 `if (!isComposingRef.current && isPromptUndoShortcut(e)) return;`
   * 整行删掉,本文件 175 条全绿。原因是 "z" 这个键在后面的所有分支里都不匹配 ——
   * 既不是 Backspace/Delete、不是 Escape、不是方向键,也过不了
   * shouldSubmitPromptKey / shouldInsertPromptNewlineKey 的 `key === "Enter"`。
   * 也就是说这道守卫与下游那一串 `key === …` 判断互相兜底,今天是个恒等操作。
   *
   * 不删它:它是零成本的前置声明,一旦将来有人给 handleKeyDown 加一条覆盖面更宽的
   * 分支(比如统一拦某类组合键),它就是唯一防线。记为「多道闸门互相兜底」,
   * 收敛候选见 HANDOFF §4。
   *
   * 同理也没写「组字期间的 Cmd+Z」和「Alt+Z」——「提前 return」与「走完全程」
   * 在 DOM 与回调上完全同构,断言必然是空的。isPromptUndoShortcut 自身的
   * altKey / 大小写矩阵由 send-shortcut.test.ts 直接覆盖。
   */
});

// ---------------------------------------------------------------------------
// chip 删除
// ---------------------------------------------------------------------------

describe("Backspace / Delete 删 chip", () => {
  /** 铺 `前缀 + chip + 后缀`,返回各节点。 */
  function withChip(prefix: string, suffix: string) {
    const { editor, spies } = setup();
    editor.innerHTML = `${prefix}${chipHtml("src/a.ts", undefined, "a.ts")}${suffix}`;
    const chip = editor.querySelector<HTMLElement>("[data-file-path]")!;
    return { editor, spies, chip };
  }

  it("光标紧跟在 chip 后面时,Backspace 整块删掉", () => {
    // chip 是 contenteditable=false 的整块,浏览器默认删法会把它拆成碎片(先删掉里面的
    // span,留一个空壳)。所以必须自己接管。
    const { editor, chip } = withChip("hi ", "");
    const after = document.createTextNode("");
    chip.after(after);
    caretAt(after, 0);
    const ok = fireEvent.keyDown(editor, { key: "Backspace" });
    expect(ok).toBe(false); // preventDefault 了
    expect(editor.querySelector("[data-file-path]")).toBeNull();
    expect(editor.textContent).toBe("hi ");
  });

  it("删掉 chip 时连带删掉它后面那个空格", () => {
    // 插入 chip 时会自动补一个空格,删的时候也要一起收,否则连删几个 chip 会攒出一串空格。
    const { editor } = withChip("", " ");
    caretAt(editor, 2); // chip 之后、空格文本节点之前
    fireEvent.keyDown(editor, { key: "Backspace" });
    expect(editor.textContent).toBe("");
    expect(editor.childNodes.length).toBe(0);
  });

  it("chip 后面是多个空格时不误删", () => {
    // 只吃恰好等于单个空格的那个节点 —— 用户自己敲的空格要留着。
    const { editor } = setup();
    editor.innerHTML = `${chipHtml("src/a.ts", undefined, "a.ts")}  x`;
    caretAt(editor, 1);
    fireEvent.keyDown(editor, { key: "Backspace" });
    expect(editor.textContent).toBe("  x");
  });

  /*
   * 尾随空格那行还有一个 `nodeType === Node.TEXT_NODE` 判断,摘掉本文件全绿
   * (变异测试实测)。要让它有可观测差异,chip 的下一个兄弟得是一个 textContent
   * 恰好为 " " 的**元素**(比如 <span> </span>)—— 而插入 chip 的代码只会造 Text 节点,
   * 粘贴走的也是纯文本路径,编辑器里没有任何会生成 wrapper 元素的格式化命令。
   * 也就是说这是一道防御性判断,没有可达路径,记为等价变异,不为它编造 DOM 状态。
   */
  it("chip 后面是纯空白但不止一个空格时也不删", () => {
    // 这条才钉得住「恰好等于一个空格」这个判断本身。
    // 上一条("  x")对 `=== " "` 和「trim 后为空」两种写法表现一致(变异测试实测),
    // 只有整个节点都是空白、且不止一个字符时两者才分岔:
    // 判断放松成 trim 会把用户后来自己敲的空格一起吃掉。
    const { editor } = setup();
    editor.innerHTML = `${chipHtml("src/a.ts", undefined, "a.ts")}   `;
    caretAt(editor, 1);
    fireEvent.keyDown(editor, { key: "Backspace" });
    expect(editor.querySelector("[data-file-path]")).toBeNull();
    expect(editor.textContent).toBe("   ");
  });

  it("光标在 chip 之前时 Delete 删掉它", () => {
    const { editor } = setup();
    editor.innerHTML = `${chipHtml("src/a.ts", undefined, "a.ts")}tail`;
    caretAt(editor, 0);
    const ok = fireEvent.keyDown(editor, { key: "Delete" });
    expect(ok).toBe(false);
    expect(editor.querySelector("[data-file-path]")).toBeNull();
    expect(editor.textContent).toBe("tail");
  });

  it("光标前有实字时 Backspace 交给浏览器", () => {
    const { editor } = withChip("hi", "");
    const tail = document.createTextNode("abc");
    editor.appendChild(tail);
    caretAt(tail, 3);
    const ok = fireEvent.keyDown(editor, { key: "Backspace" });
    expect(ok).toBe(true);
    expect(editor.querySelector("[data-file-path]")).not.toBeNull();
  });

  it("光标后有实字时 Delete 交给浏览器", () => {
    const { editor } = setup();
    editor.innerHTML = `${chipHtml("src/a.ts", undefined, "a.ts")}`;
    const tail = document.createTextNode("abc");
    editor.appendChild(tail);
    caretAt(tail, 0);
    const ok = fireEvent.keyDown(editor, { key: "Delete" });
    expect(ok).toBe(true);
    expect(editor.querySelector("[data-file-path]")).not.toBeNull();
  });

  it("跨过纯空白文本节点也能找到 chip", () => {
    const { editor } = setup();
    editor.innerHTML = `${chipHtml("src/a.ts", undefined, "a.ts")}`;
    const blank = document.createTextNode("  ");
    editor.appendChild(blank);
    caretAt(blank, 2);
    fireEvent.keyDown(editor, { key: "Backspace" });
    expect(editor.querySelector("[data-file-path]")).toBeNull();
  });

  it("Delete 从空白文本节点往后找 chip", () => {
    // 与 Backspace 镜像的方向:光标在 chip 前面的空白里,Delete 要往后看。
    const { editor } = setup();
    const blank = document.createTextNode("  ");
    editor.appendChild(blank);
    editor.insertAdjacentHTML("beforeend", chipHtml("src/a.ts", undefined, "a.ts"));
    caretAt(blank, 0);
    const ok = fireEvent.keyDown(editor, { key: "Delete" });
    expect(ok).toBe(false);
    expect(editor.querySelector("[data-file-path]")).toBeNull();
  });

  it("Delete 也会跨过多个空白文本节点", () => {
    const { editor } = setup();
    const first = document.createTextNode("");
    const second = document.createTextNode(" ");
    editor.append(first, second);
    editor.insertAdjacentHTML("beforeend", chipHtml("src/a.ts", undefined, "a.ts"));
    caretAt(first, 0);
    fireEvent.keyDown(editor, { key: "Delete" });
    expect(editor.querySelector("[data-file-path]")).toBeNull();
  });

  it("光标落在注释节点里时不删任何东西", () => {
    // Range 起点落在注释节点是合法 DOM(既不是 TEXT 也不是 ELEMENT)。
    // 这一档守的就是「容器不认识 → 什么都不做」,而不是拿着 undefined 往下走。
    const { editor } = setup();
    const comment = document.createComment("x");
    editor.appendChild(comment);
    editor.insertAdjacentHTML("beforeend", chipHtml("src/a.ts", undefined, "a.ts"));
    caretAt(comment, 0);
    const ok = fireEvent.keyDown(editor, { key: "Backspace" });
    expect(ok).toBe(true);
    expect(editor.querySelector("[data-file-path]")).not.toBeNull();
  });

  it("相邻兄弟不是 chip 时交给浏览器", () => {
    const { editor } = setup();
    editor.innerHTML = "<span>plain</span>";
    const tail = document.createTextNode("");
    editor.appendChild(tail);
    caretAt(tail, 0);
    const ok = fireEvent.keyDown(editor, { key: "Backspace" });
    expect(ok).toBe(true);
    expect(editor.querySelector("span")).not.toBeNull();
  });

  it("已经在最前面时 Backspace 交给浏览器", () => {
    const { editor } = setup();
    editor.textContent = "";
    caretAt(editor, 0);
    expect(fireEvent.keyDown(editor, { key: "Backspace" })).toBe(true);
  });

  it("有选中内容时不走 chip 删除", () => {
    // 非折叠选区交给浏览器的默认删除 —— 用户框选了一段,期望整段消失。
    //
    // 选区起点刻意压在 chip **正后方**:这是唯一能暴露折叠判断的摆法。
    // 起点前面还有别的字符时,「往前找兄弟」本来就找不到 chip,摘掉 `!range.collapsed`
    // 也看不出差别(变异测试实测过)。压在 chip 后面才真的危险 ——
    // 缺了这道判断,用户框选一段按 Backspace 会连带删掉框外的那个 chip。
    const { editor } = setup();
    editor.innerHTML = `${chipHtml("src/a.ts", undefined, "a.ts")}yz`;
    const tail = editor.lastChild as Text;
    selectRange(tail, 0, tail, 1);
    const ok = fireEvent.keyDown(editor, { key: "Backspace" });
    expect(ok).toBe(true);
    expect(editor.querySelector("[data-file-path]")).not.toBeNull();
    expect(editor.textContent).toBe("a.tsyz");
  });

  it("没有选区时不走 chip 删除", () => {
    const { editor } = setup();
    editor.innerHTML = chipHtml("src/a.ts", undefined, "a.ts");
    clearSelection();
    const ok = fireEvent.keyDown(editor, { key: "Backspace" });
    expect(ok).toBe(true);
    expect(editor.querySelector("[data-file-path]")).not.toBeNull();
  });

  it("组字期间不走 chip 删除", () => {
    // 组字中的 Backspace 是删候选,不是删 chip。
    const { editor, isComposingRef } = setup();
    editor.innerHTML = chipHtml("src/a.ts", undefined, "a.ts");
    caretAt(editor, 1);
    isComposingRef().current = true;
    const ok = fireEvent.keyDown(editor, { key: "Backspace" });
    expect(ok).toBe(true);
    expect(editor.querySelector("[data-file-path]")).not.toBeNull();
  });

  it("删完 chip 后重新报告空态并回传内容", () => {
    const { editor, spies } = setup();
    editor.innerHTML = chipHtml("src/a.ts", undefined, "a.ts");
    caretAt(editor, 1);
    fireEvent.keyDown(editor, { key: "Backspace" });
    expect(spies.onSetIsEmpty).toHaveBeenLastCalledWith(true);
    expect(lastContent(spies).hasChips).toBe(false);
    expect(spies.onUpdateSuggestions).toHaveBeenCalled();
  });

  it("还剩别的 chip 时空态仍为非空", () => {
    const { editor, spies } = setup();
    editor.innerHTML = `${chipHtml("src/a.ts", undefined, "a")}${chipHtml("src/b.ts", undefined, "b")}`;
    caretAt(editor, 2);
    fireEvent.keyDown(editor, { key: "Backspace" });
    expect(editor.querySelectorAll("[data-file-path]").length).toBe(1);
    expect(spies.onSetIsEmpty).toHaveBeenLastCalledWith(false);
  });

  it("删完后光标落在 chip 原来的位置", () => {
    const { editor, handle } = setup();
    editor.innerHTML = `A${chipHtml("src/a.ts", undefined, "a.ts")}B`;
    caretAt(editor, 2);
    fireEvent.keyDown(editor, { key: "Backspace" });
    handle().insertText("|");
    expect(editor.textContent).toBe("A|B");
  });

  it("其他键不触发 chip 删除", () => {
    const { editor } = setup();
    editor.innerHTML = chipHtml("src/a.ts", undefined, "a.ts");
    caretAt(editor, 1);
    fireEvent.keyDown(editor, { key: "a" });
    expect(editor.querySelector("[data-file-path]")).not.toBeNull();
  });
});

// ---------------------------------------------------------------------------
// 候选面板的键盘导航
// ---------------------------------------------------------------------------

describe("dsh 命令面板的键盘导航", () => {
  const items = [cmdA, cmdB];

  it("ArrowDown 下移一格", () => {
    const { editor, spies } = setup({ dshCommandItems: items, dshCommandIndex: 0 });
    expect(fireEvent.keyDown(editor, { key: "ArrowDown" })).toBe(false);
    expect(spies.onSetDshCommandIndex).toHaveBeenCalledWith(1);
  });

  it("ArrowDown 到底不越界", () => {
    const { editor, spies } = setup({ dshCommandItems: items, dshCommandIndex: 1 });
    fireEvent.keyDown(editor, { key: "ArrowDown" });
    expect(spies.onSetDshCommandIndex).toHaveBeenCalledWith(1);
  });

  it("ArrowUp 上移一格", () => {
    const { editor, spies } = setup({ dshCommandItems: items, dshCommandIndex: 1 });
    fireEvent.keyDown(editor, { key: "ArrowUp" });
    expect(spies.onSetDshCommandIndex).toHaveBeenCalledWith(0);
  });

  it("ArrowUp 到顶不越界", () => {
    const { editor, spies } = setup({ dshCommandItems: items, dshCommandIndex: 0 });
    fireEvent.keyDown(editor, { key: "ArrowUp" });
    expect(spies.onSetDshCommandIndex).toHaveBeenCalledWith(0);
  });

  it("Enter 插入当前选中的命令", () => {
    const { editor, spies } = setup({
      dshCommandItems: items,
      dshCommandIndex: 1,
      slashSuggestionKind: "dsh-command",
    });
    editor.textContent = "/exp";
    caretAt(editor.firstChild!, 4);
    expect(fireEvent.keyDown(editor, { key: "Enter" })).toBe(false);
    expect(spies.onSelectDshCommand).toHaveBeenCalledWith(cmdB);
    expect(editor.textContent).toBe("/export ");
    expect(spies.onSubmit).not.toHaveBeenCalled();
  });

  it("Tab 与 Enter 等效", () => {
    const { editor, spies } = setup({
      dshCommandItems: items,
      dshCommandIndex: 0,
      slashSuggestionKind: "dsh-command",
    });
    editor.textContent = "/comp";
    caretAt(editor.firstChild!, 5);
    fireEvent.keyDown(editor, { key: "Tab" });
    expect(spies.onSelectDshCommand).toHaveBeenCalledWith(cmdA);
    expect(editor.textContent).toBe("/compact ");
  });

  it("命令始终用 / 前缀,即使技能前缀是 $", () => {
    // dsh 会话里 skillCommandPrefix 可能是 $(codex),但 dsh 命令自己只认 /。
    const { editor } = setup({
      dshCommandItems: items,
      dshCommandIndex: 0,
      skillCommandPrefix: "$",
      slashSuggestionKind: "dsh-command",
    });
    editor.textContent = "/comp";
    caretAt(editor.firstChild!, 5);
    fireEvent.keyDown(editor, { key: "Enter" });
    expect(editor.textContent).toBe("/compact ");
  });

  it("下标越界时 Enter 不选任何命令", () => {
    // 目录刚被过滤掉、index 还停在旧位置的瞬间会命中这里 —— 不能崩。
    const watch = watchErrors();
    try {
      const { editor, spies } = setup({ dshCommandItems: items, dshCommandIndex: 9 });
      editor.textContent = "/x";
      caretAt(editor.firstChild!, 2);
      fireEvent.keyDown(editor, { key: "Enter" });
      expect(spies.onSelectDshCommand).not.toHaveBeenCalled();
      expect(spies.onSubmit).not.toHaveBeenCalled();
      // 守卫真的守住了,而不是读 undefined.name 崩了(崩了也是「没调用」)。
      expect(watch.errors).toEqual([]);
    } finally {
      watch.stop();
    }
  });

  it("插不进去时不报告已选中", () => {
    // 没有 slash 上下文(比如选区丢了)→ insertSkillAtCaret 返回 false,
    // 此时通知父组件「已选中」会让面板关掉,而编辑器里什么都没插进去。
    const { editor, spies } = setup({ dshCommandItems: items, dshCommandIndex: 0 });
    editor.textContent = "no slash here";
    caretAt(editor.firstChild!, 3);
    fireEvent.keyDown(editor, { key: "Enter" });
    expect(spies.onSelectDshCommand).not.toHaveBeenCalled();
  });

  it("面板开着时 Escape 收起面板", () => {
    const { editor, spies } = setup({ dshCommandItems: items, dshCommandMenuOpen: true });
    expect(fireEvent.keyDown(editor, { key: "Escape" })).toBe(false);
    expect(spies.onDismissSuggestions).toHaveBeenCalled();
  });

  it("目录为空时方向键交给浏览器", () => {
    const { editor, spies } = setup({ dshCommandItems: [] });
    expect(fireEvent.keyDown(editor, { key: "ArrowDown" })).toBe(true);
    expect(spies.onSetDshCommandIndex).not.toHaveBeenCalled();
  });

  it("面板开着时普通字符键照常落到编辑器", () => {
    // 面板开着不等于接管键盘 —— 只有方向键 / Enter / Tab / Escape 归面板,
    // 其余按键必须原样放过,否则用户没法继续把命令名打完。
    const { editor, spies } = setup({ dshCommandItems: items, dshCommandMenuOpen: true });
    expect(fireEvent.keyDown(editor, { key: "a" })).toBe(true);
    expect(spies.onSetDshCommandIndex).not.toHaveBeenCalled();
    expect(spies.onSelectDshCommand).not.toHaveBeenCalled();
    expect(spies.onDismissSuggestions).not.toHaveBeenCalled();
  });
});

describe("技能面板的键盘导航", () => {
  const items = [skillA, skillB];

  it("ArrowDown / ArrowUp 在两端夹住", () => {
    const down = setup({ skillItems: items, skillIndex: 1 });
    fireEvent.keyDown(down.editor, { key: "ArrowDown" });
    expect(down.spies.onSetSkillIndex).toHaveBeenCalledWith(1);

    const up = setup({ skillItems: items, skillIndex: 0 });
    fireEvent.keyDown(up.editor, { key: "ArrowUp" });
    expect(up.spies.onSetSkillIndex).toHaveBeenCalledWith(0);
  });

  it("ArrowDown 正常下移", () => {
    const { editor, spies } = setup({ skillItems: items, skillIndex: 0 });
    fireEvent.keyDown(editor, { key: "ArrowDown" });
    expect(spies.onSetSkillIndex).toHaveBeenCalledWith(1);
  });

  it("ArrowUp 正常上移", () => {
    const { editor, spies } = setup({ skillItems: items, skillIndex: 1 });
    fireEvent.keyDown(editor, { key: "ArrowUp" });
    expect(spies.onSetSkillIndex).toHaveBeenCalledWith(0);
  });

  it("Enter 插入技能名", () => {
    const { editor, spies } = setup({ skillItems: items, skillIndex: 0 });
    editor.textContent = "/rev";
    caretAt(editor.firstChild!, 4);
    fireEvent.keyDown(editor, { key: "Enter" });
    expect(spies.onSelectSkill).toHaveBeenCalledWith(skillA);
    expect(editor.textContent).toBe("/review ");
    expect(spies.onSetIsEmpty).toHaveBeenLastCalledWith(false);
  });

  it("Tab 与 Enter 等效", () => {
    const { editor, spies } = setup({ skillItems: items, skillIndex: 1 });
    editor.textContent = "/ref";
    caretAt(editor.firstChild!, 4);
    fireEvent.keyDown(editor, { key: "Tab" });
    expect(spies.onSelectSkill).toHaveBeenCalledWith(skillB);
    expect(editor.textContent).toBe("/refactor ");
  });

  it("codex 会话用 $ 前缀插入", () => {
    const { editor } = setup({ skillItems: items, skillIndex: 0, skillCommandPrefix: "$" });
    editor.textContent = "/rev";
    caretAt(editor.firstChild!, 4);
    fireEvent.keyDown(editor, { key: "Enter" });
    expect(editor.textContent).toBe("$review ");
  });

  it("下标越界时不选任何技能", () => {
    const watch = watchErrors();
    try {
      const { editor, spies } = setup({ skillItems: items, skillIndex: 9 });
      editor.textContent = "/x";
      caretAt(editor.firstChild!, 2);
      fireEvent.keyDown(editor, { key: "Enter" });
      expect(spies.onSelectSkill).not.toHaveBeenCalled();
      expect(watch.errors).toEqual([]);
    } finally {
      watch.stop();
    }
  });

  it("插不进去时不报告已选中", () => {
    const { editor, spies } = setup({ skillItems: items, skillIndex: 0 });
    editor.textContent = "no slash";
    caretAt(editor.firstChild!, 3);
    fireEvent.keyDown(editor, { key: "Enter" });
    expect(spies.onSelectSkill).not.toHaveBeenCalled();
    expect(spies.onSetIsEmpty).not.toHaveBeenCalled();
  });

  it("面板开着时 Escape 收起面板", () => {
    const { editor, spies } = setup({ skillItems: items, skillMenuOpen: true });
    fireEvent.keyDown(editor, { key: "Escape" });
    expect(spies.onDismissSuggestions).toHaveBeenCalled();
    expect(spies.onSetSkillIndex).not.toHaveBeenCalled();
  });

  it("dsh 命令目录非空时优先于技能目录", () => {
    // 两个目录同时有内容只会出现在切换会话类型的瞬间;先来先服务的顺序要固定,
    // 否则同一次 Enter 在两种会话里插出不同的东西。
    const { editor, spies } = setup({
      dshCommandItems: [cmdA],
      dshCommandIndex: 0,
      skillItems: items,
      skillIndex: 0,
    });
    editor.textContent = "/c";
    caretAt(editor.firstChild!, 2);
    fireEvent.keyDown(editor, { key: "Enter" });
    expect(spies.onSelectDshCommand).toHaveBeenCalled();
    expect(spies.onSelectSkill).not.toHaveBeenCalled();
  });

  it("目录为空时方向键交给浏览器", () => {
    const { editor, spies } = setup({ skillItems: [] });
    expect(fireEvent.keyDown(editor, { key: "ArrowDown" })).toBe(true);
    expect(spies.onSetSkillIndex).not.toHaveBeenCalled();
  });

  it("面板开着时普通字符键照常落到编辑器", () => {
    const { editor, spies } = setup({ skillItems: items, skillMenuOpen: true });
    expect(fireEvent.keyDown(editor, { key: "b" })).toBe(true);
    expect(spies.onSetSkillIndex).not.toHaveBeenCalled();
    expect(spies.onSelectSkill).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// @ 提及:文件 chip 与项目切换
// ---------------------------------------------------------------------------

describe("@ 提及的选中", () => {
  const fileItem: MentionItem = { kind: "file", file: tsFile };
  const crossItem: MentionItem = { kind: "file", file: mdFile, crossProject: crossRef };
  const projectItem: MentionItem = { kind: "project", project };

  /** 铺一段以 `@查询词` 结尾的文本并把光标放到末尾。 */
  function withMention(text: string, overrides: HostProps) {
    const view = setup(overrides);
    view.editor.textContent = text;
    caretAt(view.editor.firstChild!, text.length);
    return view;
  }

  it("ArrowDown / ArrowUp 在两端夹住", () => {
    const down = setup({ mentionItems: [fileItem, projectItem], mentionIndex: 1 });
    fireEvent.keyDown(down.editor, { key: "ArrowDown" });
    expect(down.spies.onSetMentionIndex).toHaveBeenCalledWith(1);

    const up = setup({ mentionItems: [fileItem, projectItem], mentionIndex: 0 });
    fireEvent.keyDown(up.editor, { key: "ArrowUp" });
    expect(up.spies.onSetMentionIndex).toHaveBeenCalledWith(0);
  });

  it("ArrowDown / ArrowUp 正常移动", () => {
    const down = setup({ mentionItems: [fileItem, projectItem], mentionIndex: 0 });
    fireEvent.keyDown(down.editor, { key: "ArrowDown" });
    expect(down.spies.onSetMentionIndex).toHaveBeenCalledWith(1);

    const up = setup({ mentionItems: [fileItem, projectItem], mentionIndex: 1 });
    fireEvent.keyDown(up.editor, { key: "ArrowUp" });
    expect(up.spies.onSetMentionIndex).toHaveBeenCalledWith(0);
  });

  it("Enter 把 @查询词 换成文件 chip", () => {
    const { editor, spies, handle } = withMention("read @App", {
      mentionItems: [fileItem],
      mentionIndex: 0,
    });
    expect(fireEvent.keyDown(editor, { key: "Enter" })).toBe(false);
    expect(spies.onSelectFile).toHaveBeenCalledWith(tsFile, undefined);
    const chip = editor.querySelector<HTMLElement>("[data-file-path]")!;
    expect(chip.dataset.filePath).toBe("src/App.tsx");
    expect(chip.dataset.fileExt).toBe("tsx");
    // 显示名是短名,发出去的是完整路径。
    expect(chip.textContent).toContain("App.tsx");
    expect(handle().serialize()).toBe("read @src/App.tsx");
    expect(spies.onSubmit).not.toHaveBeenCalled();
  });

  it("Tab 与 Enter 等效", () => {
    const { editor, spies } = withMention("@App", { mentionItems: [fileItem], mentionIndex: 0 });
    fireEvent.keyDown(editor, { key: "Tab" });
    expect(spies.onSelectFile).toHaveBeenCalledWith(tsFile, undefined);
  });

  it("chip 后面自动补一个空格,且光标落在空格之后", () => {
    // 不补空格的话紧接着打字会粘在 chip 上,浏览器可能把新字符并进 chip 节点。
    const { editor, handle } = withMention("@App", {
      mentionItems: [fileItem],
      mentionIndex: 0,
    });
    fireEvent.keyDown(editor, { key: "Enter" });
    handle().insertText("go");
    expect(handle().serialize()).toBe("@src/App.tsx go");
  });

  it("跨项目文件 chip 带上项目信息", () => {
    const { editor, spies, handle } = withMention("@READ", {
      mentionItems: [crossItem],
      mentionIndex: 0,
    });
    fireEvent.keyDown(editor, { key: "Enter" });
    expect(spies.onSelectFile).toHaveBeenCalledWith(mdFile, crossRef);
    const chip = editor.querySelector<HTMLElement>("[data-file-path]")!;
    expect(chip.dataset.projectId).toBe("p2");
    expect(chip.dataset.projectPath).toBe("/repos/other");
    expect(chip.dataset.projectName).toBe("other");
    // 跨项目要发绝对路径 —— agent 的工作目录是当前项目,相对路径找不到。
    expect(handle().serialize()).toBe("@/repos/other/README.md");
  });

  it("跨项目 chip 显示「项目名 / 文件名」", () => {
    const { editor } = withMention("@READ", { mentionItems: [crossItem], mentionIndex: 0 });
    fireEvent.keyDown(editor, { key: "Enter" });
    const chip = editor.querySelector<HTMLElement>("[data-file-path]")!;
    expect(chip.textContent).toContain("other");
    expect(chip.textContent).toContain("README.md");
  });

  it("代码文件与文本文件用不同图标", () => {
    // 图标是 innerHTML 塞进去的 SVG,靠 path 数量区分:代码 4 条、文本 5 条。
    const code = withMention("@App", { mentionItems: [fileItem], mentionIndex: 0 });
    fireEvent.keyDown(code.editor, { key: "Enter" });
    const codePaths = code.editor.querySelectorAll("[data-file-path] svg path").length;

    const text = withMention("@READ", {
      mentionItems: [{ kind: "file", file: mdFile }],
      mentionIndex: 0,
    });
    fireEvent.keyDown(text.editor, { key: "Enter" });
    const textPaths = text.editor.querySelectorAll("[data-file-path] svg path").length;

    expect(codePaths).toBe(4);
    expect(textPaths).toBe(5);
    expect(codePaths).not.toBe(textPaths);
  });

  it("跨项目 chip 有底色,同项目没有", () => {
    const cross = withMention("@READ", { mentionItems: [crossItem], mentionIndex: 0 });
    fireEvent.keyDown(cross.editor, { key: "Enter" });
    const crossChip = cross.editor.querySelector<HTMLElement>("[data-file-path]")!;
    expect(crossChip.style.background).toContain("color-mix");

    const same = withMention("@App", { mentionItems: [fileItem], mentionIndex: 0 });
    fireEvent.keyDown(same.editor, { key: "Enter" });
    const sameChip = same.editor.querySelector<HTMLElement>("[data-file-path]")!;
    expect(sameChip.style.background).toBe("none");
  });

  it("chip 自身标记为不可编辑", () => {
    // 断言的是 JS 属性,不是 contenteditable 特性 —— jsdom 没有实现 contentEditable 的
    // IDL 反射,赋值只落在普通属性上,getAttribute 拿到 null。真实浏览器里这一行会写出
    // contenteditable="false",光标才进不去 chip 内部(prompt-html.test.ts 里持久化
    // 的 HTML 带着这个特性,可以侧面印证)。这条能钉住「这行被删掉」,
    // 但钉不住浏览器层面的后果。
    const { editor } = withMention("@App", { mentionItems: [fileItem], mentionIndex: 0 });
    fireEvent.keyDown(editor, { key: "Enter" });
    const chip = editor.querySelector<HTMLElement>("[data-file-path]")!;
    expect(chip.contentEditable).toBe("false");
  });

  it("插入 chip 后报告非空并回传", () => {
    const { editor, spies } = withMention("@App", { mentionItems: [fileItem], mentionIndex: 0 });
    fireEvent.keyDown(editor, { key: "Enter" });
    expect(spies.onSetIsEmpty).toHaveBeenLastCalledWith(false);
    expect(lastContent(spies).hasChips).toBe(true);
  });

  it("选项目时插入 @项目名/ 让用户接着挑文件", () => {
    const { editor, spies } = withMention("@oth", {
      mentionItems: [projectItem],
      mentionIndex: 0,
    });
    fireEvent.keyDown(editor, { key: "Enter" });
    expect(spies.onSelectProject).toHaveBeenCalledWith(project);
    expect(editor.textContent).toBe("@other/");
    expect(editor.querySelector("[data-file-path]")).toBeNull();
  });

  it("选项目后光标落在斜杠之后", () => {
    const { editor, handle } = withMention("@oth", {
      mentionItems: [projectItem],
      mentionIndex: 0,
    });
    fireEvent.keyDown(editor, { key: "Enter" });
    handle().insertText("RE");
    expect(editor.textContent).toBe("@other/RE");
  });

  it("选项目不改空态(还没有实质内容)", () => {
    // 与选文件的差别:@项目名/ 只是个中间态,发送按钮的空态由后续输入决定。
    const { editor, spies } = withMention("@oth", {
      mentionItems: [projectItem],
      mentionIndex: 0,
    });
    fireEvent.keyDown(editor, { key: "Enter" });
    expect(spies.onSetIsEmpty).not.toHaveBeenCalled();
    expect(spies.onContentChange).toHaveBeenCalled();
  });

  it("没有 @ 上下文时选文件不插入任何东西", () => {
    const { editor, spies } = withMention("plain text", {
      mentionItems: [fileItem],
      mentionIndex: 0,
    });
    fireEvent.keyDown(editor, { key: "Enter" });
    expect(editor.querySelector("[data-file-path]")).toBeNull();
    expect(spies.onSelectFile).not.toHaveBeenCalled();
  });

  it("没有 @ 上下文时选项目不插入任何东西", () => {
    const { editor, spies } = withMention("plain text", {
      mentionItems: [projectItem],
      mentionIndex: 0,
    });
    fireEvent.keyDown(editor, { key: "Enter" });
    expect(editor.textContent).toBe("plain text");
    expect(spies.onSelectProject).not.toHaveBeenCalled();
  });

  it("下标越界时不选任何项", () => {
    const watch = watchErrors();
    try {
      const { editor, spies } = withMention("@App", {
        mentionItems: [fileItem],
        mentionIndex: 5,
      });
      fireEvent.keyDown(editor, { key: "Enter" });
      expect(spies.onSelectFile).not.toHaveBeenCalled();
      expect(spies.onSelectProject).not.toHaveBeenCalled();
      expect(spies.onSubmit).not.toHaveBeenCalled();
      expect(watch.errors).toEqual([]);
    } finally {
      watch.stop();
    }
  });

  it("Escape 收起提及面板", () => {
    const { editor, spies } = setup({ mentionItems: [fileItem] });
    expect(fireEvent.keyDown(editor, { key: "Escape" })).toBe(false);
    expect(spies.onDismissSuggestions).toHaveBeenCalled();
  });

  it("技能目录非空时优先于提及目录", () => {
    const { editor, spies } = setup({
      skillItems: [skillA],
      skillIndex: 0,
      mentionItems: [fileItem],
      mentionIndex: 0,
    });
    editor.textContent = "/rev";
    caretAt(editor.firstChild!, 4);
    fireEvent.keyDown(editor, { key: "Enter" });
    expect(spies.onSelectSkill).toHaveBeenCalled();
    expect(spies.onSelectFile).not.toHaveBeenCalled();
  });

  it("目录为空时 Escape 交给浏览器", () => {
    // 没有面板时 Escape 要能冒泡出去 —— 上层用它关整个新建任务面板。
    const { editor, spies } = setup({ mentionItems: [] });
    expect(fireEvent.keyDown(editor, { key: "Escape" })).toBe(true);
    expect(spies.onDismissSuggestions).not.toHaveBeenCalled();
  });

  it("目录为空时方向键交给浏览器", () => {
    const { editor, spies } = setup({ mentionItems: [] });
    expect(fireEvent.keyDown(editor, { key: "ArrowDown" })).toBe(true);
    expect(spies.onSetMentionIndex).not.toHaveBeenCalled();
  });

  it("面板开着时普通字符键照常落到编辑器", () => {
    const { editor, spies } = setup({ mentionItems: [fileItem] });
    expect(fireEvent.keyDown(editor, { key: "c" })).toBe(true);
    expect(spies.onSetMentionIndex).not.toHaveBeenCalled();
    expect(spies.onSelectFile).not.toHaveBeenCalled();
    expect(spies.onDismissSuggestions).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// 发送与换行
// ---------------------------------------------------------------------------

describe("发送与换行(mod_enter,macOS)", () => {
  it("Cmd+Enter 立即发送", () => {
    const { editor, spies } = setup();
    expect(fireEvent.keyDown(editor, { key: "Enter", metaKey: true })).toBe(false);
    expect(spies.onSubmit).toHaveBeenCalledWith(true);
  });

  it("单独 Enter 交给浏览器换行", () => {
    // mod_enter 模式下 Enter 是换行,由 contenteditable 自己处理。
    const { editor, spies } = setup();
    expect(fireEvent.keyDown(editor, { key: "Enter" })).toBe(true);
    expect(spies.onSubmit).not.toHaveBeenCalled();
  });

  it("Shift+Cmd+Enter 不发送", () => {
    const { editor, spies } = setup();
    fireEvent.keyDown(editor, { key: "Enter", metaKey: true, shiftKey: true });
    expect(spies.onSubmit).not.toHaveBeenCalled();
  });

  it("组字期间的 Cmd+Enter 不发送", () => {
    // 这是最要紧的一条:中文用户按 Enter 是在确认候选词,误判成发送会把半成品发出去。
    const { editor, spies, isComposingRef } = setup();
    isComposingRef().current = true;
    expect(fireEvent.keyDown(editor, { key: "Enter", metaKey: true })).toBe(true);
    expect(spies.onSubmit).not.toHaveBeenCalled();
  });

  it("面板开着时 Enter 归面板,不发送", () => {
    const { editor, spies } = setup({ skillItems: [skillA], skillIndex: 0 });
    editor.textContent = "/rev";
    caretAt(editor.firstChild!, 4);
    fireEvent.keyDown(editor, { key: "Enter", metaKey: true });
    expect(spies.onSubmit).not.toHaveBeenCalled();
    expect(spies.onSelectSkill).toHaveBeenCalled();
  });
});

describe("发送与换行(enter 模式,macOS)", () => {
  const enterMode: HostProps = { sendShortcut: "enter" };

  it("单独 Enter 发送", () => {
    const { editor, spies } = setup(enterMode);
    expect(fireEvent.keyDown(editor, { key: "Enter" })).toBe(false);
    expect(spies.onSubmit).toHaveBeenCalledWith(true);
  });

  it("Cmd+Enter 插入换行而不是发送", () => {
    const { editor, spies } = setup(enterMode);
    editor.textContent = "ab";
    caretAt(editor.firstChild!, 1);
    expect(fireEvent.keyDown(editor, { key: "Enter", metaKey: true })).toBe(false);
    expect(spies.onSubmit).not.toHaveBeenCalled();
    expect(editor.textContent).toBe("a\nb");
    expect(spies.onSetIsEmpty).toHaveBeenLastCalledWith(false);
    expect(spies.onUpdateSuggestions).toHaveBeenCalled();
  });

  it("换行会替换掉选中的内容", () => {
    const { editor } = setup(enterMode);
    editor.textContent = "abcd";
    selectRange(editor.firstChild!, 1, editor.firstChild!, 3);
    fireEvent.keyDown(editor, { key: "Enter", metaKey: true });
    expect(editor.textContent).toBe("a\nd");
  });

  it("换行后光标落在换行符之后", () => {
    const { editor, handle } = setup(enterMode);
    editor.textContent = "ab";
    caretAt(editor.firstChild!, 1);
    fireEvent.keyDown(editor, { key: "Enter", metaKey: true });
    handle().insertText("X");
    expect(editor.textContent).toBe("a\nXb");
  });

  it("没有选区时换行不改内容", () => {
    const { editor, spies } = setup(enterMode);
    editor.textContent = "ab";
    clearSelection();
    fireEvent.keyDown(editor, { key: "Enter", metaKey: true });
    expect(editor.textContent).toBe("ab");
    // 阻止默认仍然发生 —— 否则浏览器会自己插一个 <div>,与组件的换行模型打架。
    expect(spies.onSetIsEmpty).toHaveBeenLastCalledWith(false);
  });

  it("Shift+Enter 既不发送也不插换行", () => {
    const { editor, spies } = setup(enterMode);
    expect(fireEvent.keyDown(editor, { key: "Enter", shiftKey: true })).toBe(true);
    expect(spies.onSubmit).not.toHaveBeenCalled();
  });

  it("组字期间的 Enter 不发送", () => {
    const { editor, spies, isComposingRef } = setup(enterMode);
    isComposingRef().current = true;
    expect(fireEvent.keyDown(editor, { key: "Enter" })).toBe(true);
    expect(spies.onSubmit).not.toHaveBeenCalled();
  });

  it("组字期间的 Cmd+Enter 不插换行", () => {
    const { editor, isComposingRef } = setup(enterMode);
    editor.textContent = "ab";
    caretAt(editor.firstChild!, 1);
    isComposingRef().current = true;
    fireEvent.keyDown(editor, { key: "Enter", metaKey: true });
    expect(editor.textContent).toBe("ab");
  });

  it("换行后的文本序列化成真正的换行", () => {
    const { editor, handle } = setup(enterMode);
    editor.textContent = "ab";
    caretAt(editor.firstChild!, 1);
    fireEvent.keyDown(editor, { key: "Enter", metaKey: true });
    expect(handle().serialize()).toBe("a\nb");
  });
});

// ---------------------------------------------------------------------------
// editorRef 是 prop:父组件没接上时的契约
// ---------------------------------------------------------------------------

describe("父组件传进来的 editorRef 没指向节点时", () => {
  /**
   * `editorRef` 是 **prop**,挂不挂到节点上由父组件决定 —— 所以「ref.current 为 null」
   * 是组件对外契约的一部分,不是不可达的内部状态。这里用一个只读 null 的 ref 模拟
   * 父组件没接上的情形:每个入口都必须安静地什么都不做,而不是抛 TypeError
   * 把整个新建任务面板炸掉。
   */
  function setupDetached(overrides: HostProps = {}) {
    const spies = makeSpies();
    const nullRef = {
      get current() {
        return null;
      },
      set current(_v: HTMLDivElement | null) {
        /* 吞掉 React 的写入,保持恒为 null */
      },
    } as React.RefObject<HTMLDivElement | null>;
    const view = render(<Host overrides={{ ...overrides, editorRef: nullRef }} spies={spies} />);
    const editor = view.container.querySelector<HTMLDivElement>(".prompt-editor")!;
    return { editor, spies };
  }

  it("输入事件什么都不做", () => {
    const { editor, spies } = setupDetached();
    expect(() => fireEvent.input(editor)).not.toThrow();
    expect(spies.onSetIsEmpty).not.toHaveBeenCalled();
    expect(spies.onContentChange).not.toHaveBeenCalled();
  });

  it("组字过程中的内容回传什么都不做", () => {
    const { editor, spies } = setupDetached();
    expect(() => fireEvent.compositionUpdate(editor, { data: "ni" })).not.toThrow();
    // onSetIsEmpty 仍然会被调(它不依赖编辑器节点),但内容回传要跳过。
    expect(spies.onContentChange).not.toHaveBeenCalled();
  });

  it("组字结束时跳过归一化但仍收尾", () => {
    const { editor, spies } = setupDetached();
    expect(() => fireEvent.compositionEnd(editor, { data: "ni" })).not.toThrow();
    expect(spies.onContentChange).not.toHaveBeenCalled();
    expect(spies.onUpdateSuggestions).toHaveBeenCalled();
  });

  it("Enter 选文件时不插 chip", () => {
    const { editor, spies } = setupDetached({
      mentionItems: [{ kind: "file", file: tsFile }],
      mentionIndex: 0,
    });
    editor.textContent = "@App";
    caretAt(editor.firstChild!, 4);
    fireEvent.keyDown(editor, { key: "Enter" });
    expect(spies.onSelectFile).not.toHaveBeenCalled();
    expect(editor.querySelector("[data-file-path]")).toBeNull();
  });

  it("Enter 选项目时不插文本", () => {
    const { editor, spies } = setupDetached({
      mentionItems: [{ kind: "project", project }],
      mentionIndex: 0,
    });
    editor.textContent = "@oth";
    caretAt(editor.firstChild!, 4);
    fireEvent.keyDown(editor, { key: "Enter" });
    expect(spies.onSelectProject).not.toHaveBeenCalled();
    expect(editor.textContent).toBe("@oth");
  });

  it("Enter 选技能时不插技能名", () => {
    const { editor, spies } = setupDetached({ skillItems: [skillA], skillIndex: 0 });
    editor.textContent = "/rev";
    caretAt(editor.firstChild!, 4);
    fireEvent.keyDown(editor, { key: "Enter" });
    expect(spies.onSelectSkill).not.toHaveBeenCalled();
    expect(editor.textContent).toBe("/rev");
  });

  it("Enter 选 dsh 命令时不插命令名", () => {
    const { editor, spies } = setupDetached({ dshCommandItems: [cmdA], dshCommandIndex: 0 });
    editor.textContent = "/comp";
    caretAt(editor.firstChild!, 5);
    fireEvent.keyDown(editor, { key: "Enter" });
    expect(spies.onSelectDshCommand).not.toHaveBeenCalled();
    expect(editor.textContent).toBe("/comp");
  });

  it("Backspace 不走 chip 删除", () => {
    const { editor } = setupDetached();
    editor.innerHTML = chipHtml("src/a.ts", undefined, "a.ts");
    caretAt(editor, 1);
    expect(fireEvent.keyDown(editor, { key: "Backspace" })).toBe(true);
    expect(editor.querySelector("[data-file-path]")).not.toBeNull();
  });
});

// ---------------------------------------------------------------------------
// IME 重放去重层 —— 现状记录
// ---------------------------------------------------------------------------

describe("IME 重放去重层(handleBeforeInputCapture)", () => {
  /**
   * 这一层现在**收不到事件**,本用例把这个事实钉住。
   *
   * React 19 的 `onBeforeInput` 不是从原生 `beforeinput` 合成的 ——
   * registerTwoPhaseEvent("onBeforeInput", ["compositionend","keypress","textInput","paste"]),
   * 原生 `beforeinput` 根本不在订阅列表里(已实测:派发原生 beforeinput,handler 0 次调用;
   * 派发 compositionend,handler 1 次调用)。
   * 而能到达它的那几种事件(CompositionEvent / KeyboardEvent / TextEvent)都没有
   * `inputType` 字段,于是 `event.inputType !== "insertText"` 恒真、函数首行就 return。
   *
   * 结论:去重层是死代码,PromptEditor 的拼音残留清理实际只靠
   * compositionend 里的 normalizeEditorCompositionText 那一路(上面已覆盖)。
   * 同样的写法还在 src/components/useTextInputIMEFix.ts:19。
   *
   * 这条用例断言的是「现状」而不是「期望」:一旦有人给编辑器补上真正的原生
   * beforeinput 监听(那才是修法),它会失败并提醒去重层重新活了、需要配套用例。
   */
  it("原生 beforeinput 到不了组件,去重层不生效", () => {
    const { editor } = setup();
    editor.textContent = "";
    caretAt(editor, 0);

    // 先走一遍完整的组字,把「待忽略候选」装满 —— 这是去重层生效的前置条件。
    fireEvent.compositionStart(editor);
    fireEvent.compositionUpdate(editor, { data: "ni" });
    editor.textContent = "你好";
    fireEvent.compositionEnd(editor, { data: "你好" });

    // 紧接着重放一次同样的文本:去重层若活着,应当 preventDefault 把它拦下来。
    const replay = new InputEvent("beforeinput", {
      bubbles: true,
      cancelable: true,
      data: "你好",
      inputType: "insertText",
    });
    editor.dispatchEvent(replay);

    expect(replay.defaultPrevented).toBe(false);
    expect(editor.textContent).toBe("你好");
  });
});
