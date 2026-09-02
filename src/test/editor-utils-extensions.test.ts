import { afterEach, describe, expect, it, vi } from "vitest";
import { EditorState, StateEffect } from "@codemirror/state";
import { EditorView } from "@uiw/react-codemirror";
import {
  createDebugBreakpointGutter,
  createInlineBlameExtension,
} from "../components/file-viewer/editorUtils";
import type { Extension } from "@codemirror/state";
import type { GitBlameLine } from "../types";

/**
 * 两个 CodeMirror 扩展工厂。要在 jsdom 里挂一个真 `EditorView` 才能看到
 * gutter / decoration 实际渲染出什么 —— 只断言「返回了个对象」是空断言,
 * 断点标记挂错行、blame 挂错位置都测不出来。
 */

const views: EditorView[] = [];

function mount(doc: string, extensions: Extension) {
  const parent = document.createElement("div");
  document.body.appendChild(parent);
  const view = new EditorView({
    state: EditorState.create({ doc, extensions }),
    parent,
  });
  views.push(view);
  return view;
}

afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
  document.body.innerHTML = "";
});

/**
 * blame 装饰挂在哪些行 —— 从 facet 里读,不是数 DOM。
 * CodeMirror 只渲染视口内的行,jsdom 没有布局,实测无论文档多长都只落 36 个
 * `.cm-inline-blame` 节点。想验证「截断到 5000 条」这类事情,数 DOM 是空断言。
 */
function decoratedLines(view: EditorView): number[] {
  const lineNumbers: number[] = [];
  for (const value of view.state.facet(EditorView.decorations)) {
    if (typeof value === "function") continue;
    const iter = value.iter();
    while (iter.value) {
      lineNumbers.push(view.state.doc.lineAt(iter.from).number);
      iter.next();
    }
  }
  return lineNumbers;
}

function blameLine(line: number, overrides: Partial<GitBlameLine> = {}): GitBlameLine {
  return {
    line,
    commit: `commit${line}0000000000000000000000000000000000`,
    shortCommit: `c${line}`,
    author: `author${line}`,
    authorTime: 1_700_000_000 + line,
    summary: `summary ${line}`,
    content: `line ${line} content`,
    ...overrides,
  };
}

describe("createDebugBreakpointGutter", () => {
  it("既没有回调也没有断点时返回空扩展(不白占一条 gutter 宽度)", () => {
    expect(createDebugBreakpointGutter({ breakpointLines: new Set(), label: "bp" })).toEqual([]);
  });

  it("有回调但没有断点时仍然建 gutter(要能点空行下断点)", () => {
    const gutter = createDebugBreakpointGutter({
      breakpointLines: new Set(),
      label: "bp",
      onToggleLine: vi.fn(),
    });
    expect(gutter).not.toEqual([]);

    const view = mount("a\nb\nc", gutter);
    expect(view.dom.querySelector(".cm-debug-breakpoint-gutter")).not.toBeNull();
  });

  it("有断点但没有回调时也要渲染(只读地显示断点)", () => {
    const gutter = createDebugBreakpointGutter({
      breakpointLines: new Set([2]),
      label: "bp",
    });
    expect(gutter).not.toEqual([]);
    const view = mount("a\nb\nc", gutter);
    expect(view.dom.querySelector(".cm-debug-breakpoint-marker.active")).not.toBeNull();
  });

  it("只在有断点的行渲染 active 标记", () => {
    const view = mount(
      "line1\nline2\nline3\nline4",
      createDebugBreakpointGutter({
        breakpointLines: new Set([2, 4]),
        label: "断点",
        onToggleLine: vi.fn(),
      }),
    );
    const active = view.dom.querySelectorAll(".cm-debug-breakpoint-marker.active");
    expect(active).toHaveLength(2);
    for (const marker of active) {
      expect(marker.getAttribute("title")).toBe("断点");
    }
  });

  it("行号超出文档范围的断点不会渲染出来", () => {
    const view = mount(
      "only one line",
      createDebugBreakpointGutter({
        breakpointLines: new Set([1, 99]),
        label: "bp",
        onToggleLine: vi.fn(),
      }),
    );
    expect(view.dom.querySelectorAll(".cm-debug-breakpoint-marker.active")).toHaveLength(1);
  });

  it("active 标记落在对应行的 gutter 格子上,不是随便某一格", () => {
    // 这里才是 `lineMarker` 里 `doc.lineAt(line.from).number` 的实际验证:
    // 标记必须出现在第 2、第 4 行的格子上。offset 1 是 initialSpacer 那一格。
    const view = mount(
      "line1\nline2\nline3\nline4",
      createDebugBreakpointGutter({
        breakpointLines: new Set([2, 4]),
        label: "bp",
        onToggleLine: vi.fn(),
      }),
    );
    const elements = [
      ...view.dom.querySelectorAll(".cm-debug-breakpoint-gutter .cm-gutterElement"),
    ];
    const withActive = elements
      .map((el, index) => (el.querySelector(".cm-debug-breakpoint-marker.active") ? index : -1))
      .filter((index) => index >= 0);
    expect(withActive).toEqual([2, 4]);
    // 第 0 格是隐藏的 spacer,拿的是 spacer 标记而不是 active。
    expect(elements[0].querySelector(".cm-debug-breakpoint-marker.spacer")).not.toBeNull();
  });

  it("点 gutter 会转成 onToggleLine 调用并吃掉默认行为", () => {
    // 注意:CodeMirror 是按 `event.clientY` 反查行的,jsdom 没有布局、所有
    // 坐标都是 0,所以这里无论点哪一格拿到的都是第 1 行。行号映射不在这个
    // 用例里验证 —— 由上面那条「active 标记落在对应行」覆盖。
    const onToggleLine = vi.fn();
    const view = mount(
      "line1\nline2\nline3",
      createDebugBreakpointGutter({ breakpointLines: new Set(), label: "bp", onToggleLine }),
    );
    const element = view.dom.querySelector(".cm-debug-breakpoint-gutter .cm-gutterElement")!;
    const event = new MouseEvent("mousedown", { bubbles: true, cancelable: true });
    element.dispatchEvent(event);
    expect(onToggleLine).toHaveBeenCalledTimes(1);
    expect(onToggleLine.mock.calls[0][0]).toBeGreaterThanOrEqual(1);
    // 这里有两道闸门:实现里显式 `event.preventDefault()`,而 CodeMirror 自己在
    // `domEventHandlers` 返回 true 时也会 preventDefault。变异测试的结论:单独摘掉
    // 显式那句本文件全绿,连 `return true` 一起改成 false 才被抓到 —— 互相兜底。
    // 不为显式那句单独补测试(用户点不到那条路径),也不删它:留着才有自文档效果,
    // 而且删了 `event` 参数就变成未使用,`noUnusedParameters` 会报。
    expect(event.defaultPrevented).toBe(true);
  });

  it("没有回调时点击不抛异常,也不阻止默认行为", () => {
    const view = mount(
      "line1\nline2",
      createDebugBreakpointGutter({ breakpointLines: new Set([1]), label: "bp" }),
    );
    const element = view.dom.querySelector(".cm-debug-breakpoint-gutter .cm-gutterElement")!;
    const event = new MouseEvent("mousedown", { bubbles: true, cancelable: true });
    element.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
  });

  it("改文档不会把标记 DOM 节点换掉", () => {
    const view = mount(
      "a\nb",
      createDebugBreakpointGutter({
        breakpointLines: new Set([1]),
        label: "bp",
        onToggleLine: vi.fn(),
      }),
    );
    const before = view.dom.querySelector(".cm-debug-breakpoint-marker.active");
    view.dispatch({ changes: { from: view.state.doc.length, insert: "\nc" } });
    expect(view.dom.querySelector(".cm-debug-breakpoint-marker.active")).toBe(before);
  });

  it("换一份 label 相同的 gutter 后标记还在", () => {
    // 说明 `DebugBreakpointGutterMarker.eq()` 为什么没有对应用例:
    // 工厂里 activeMarker / spacerMarker 各只 new 一次,同一份配置内所有行共用
    // 同一个实例,CodeMirror 先比引用就短路了,eq 走不到;reconfigure 时 gutter
    // 会整棵重建 DOM,也不经过 eq。实测把 eq 改成恒 true 或恒 false,本文件
    // 26 个用例全绿 —— 它在当前设计下是够不着的代码,不是缺测试。
    // 结论记在 HANDOFF.md §4,要么删要么改成按值构造 marker,不在本次改。
    const view = mount(
      "a\nb",
      createDebugBreakpointGutter({ breakpointLines: new Set([1]), label: "bp" }),
    );
    view.dispatch({
      effects: StateEffect.reconfigure.of(
        createDebugBreakpointGutter({ breakpointLines: new Set([1]), label: "bp" }),
      ),
    });
    const after = view.dom.querySelector(".cm-debug-breakpoint-marker.active");
    expect(after).not.toBeNull();
    expect(after!.getAttribute("title")).toBe("bp");
  });

  it("label 变了就换 DOM 节点(eq 不命中)", () => {
    const view = mount(
      "a\nb",
      createDebugBreakpointGutter({ breakpointLines: new Set([1]), label: "旧" }),
    );
    const before = view.dom.querySelector(".cm-debug-breakpoint-marker.active");
    view.dispatch({
      effects: StateEffect.reconfigure.of(
        createDebugBreakpointGutter({ breakpointLines: new Set([1]), label: "新" }),
      ),
    });
    const after = view.dom.querySelector(".cm-debug-breakpoint-marker.active");
    expect(after).not.toBe(before);
    expect(after!.getAttribute("title")).toBe("新");
  });
});

describe("createInlineBlameExtension", () => {
  it("关掉时返回空扩展", () => {
    expect(createInlineBlameExtension({ enabled: false, lines: [blameLine(1)] })).toEqual([]);
  });

  it("没有 blame 数据时返回空扩展", () => {
    expect(createInlineBlameExtension({ enabled: true, lines: [] })).toEqual([]);
  });

  it("每行行尾挂一个 blame 标记", () => {
    const view = mount(
      "a\nb\nc",
      createInlineBlameExtension({ enabled: true, lines: [blameLine(1), blameLine(3)] }),
    );
    const markers = view.dom.querySelectorAll(".cm-inline-blame");
    expect(markers).toHaveLength(2);
  });

  it("标记文案用短 hash,title 用完整 hash", () => {
    const line = blameLine(1, {
      shortCommit: "abc1234",
      commit: "abc1234def5678901234567890123456789012",
      author: "Lin",
      summary: "fix crash",
    });
    const view = mount("a", createInlineBlameExtension({ enabled: true, lines: [line] }));
    const marker = view.dom.querySelector(".cm-inline-blame")!;
    expect(marker.textContent).toBe("abc1234 Lin - fix crash");
    expect(marker.getAttribute("title")).toBe(
      "abc1234def5678901234567890123456789012 Lin - fix crash",
    );
  });

  it("作者为空白时回落成 Unknown", () => {
    const view = mount(
      "a",
      createInlineBlameExtension({
        enabled: true,
        lines: [blameLine(1, { shortCommit: "aaa", author: "   ", summary: "s" })],
      }),
    );
    expect(view.dom.querySelector(".cm-inline-blame")!.textContent).toBe("aaa Unknown - s");
  });

  it("summary 为空白时不留下悬空的分隔符", () => {
    const view = mount(
      "a",
      createInlineBlameExtension({
        enabled: true,
        lines: [blameLine(1, { shortCommit: "aaa", author: "Lin", summary: "  " })],
      }),
    );
    expect(view.dom.querySelector(".cm-inline-blame")!.textContent).toBe("aaa Lin");
  });

  it("超出文档行数的 blame 行被跳过,不抛异常", () => {
    // blame 是异步来的,文档可能已经被改短了。这里越界必须静默跳过 ——
    // `state.doc.line(n)` 越界会抛,抛出来整个编辑器就白屏。
    const view = mount(
      "a\nb",
      createInlineBlameExtension({
        enabled: true,
        lines: [blameLine(1), blameLine(2), blameLine(3), blameLine(999)],
      }),
    );
    expect(view.dom.querySelectorAll(".cm-inline-blame")).toHaveLength(2);
  });

  it("行号小于 1 的脏数据被跳过", () => {
    const view = mount(
      "a\nb",
      createInlineBlameExtension({
        enabled: true,
        lines: [blameLine(0), blameLine(-5), blameLine(1)],
      }),
    );
    expect(view.dom.querySelectorAll(".cm-inline-blame")).toHaveLength(1);
  });

  it("乱序传入也按行号排好", () => {
    const view = mount(
      "a\nb\nc\nd",
      createInlineBlameExtension({
        enabled: true,
        lines: [blameLine(4), blameLine(1), blameLine(3), blameLine(2)],
      }),
    );
    expect(decoratedLines(view)).toEqual([1, 2, 3, 4]);
  });

  it("超过 5000 行的 blame 截断到 5000 条", () => {
    const total = 5200;
    const doc = Array.from({ length: total }, (_, i) => `line${i + 1}`).join("\n");
    const lines = Array.from({ length: total }, (_, i) => blameLine(i + 1));
    const view = mount(doc, createInlineBlameExtension({ enabled: true, lines }));
    expect(decoratedLines(view)).toHaveLength(5000);
  });

  it("倒序传进 5200 行,留下的仍是第 1..5000 行(先排序再截断)", () => {
    // 这条是上面「乱序也排好」测不到的:`Decoration.set(widgets, true)` 自己会排,
    // 所以摘掉实现里的 `.sort()` 之后小样本照样有序 —— 那条用例杀不掉这个变异。
    // 只有在超过 5000 条时,排序才决定「保下来的是哪 5000 条」:
    // 不排序就会留下倒序输入的前 5000 条,也就是第 201..5200 行。
    const total = 5200;
    const doc = Array.from({ length: total }, (_, i) => `line${i + 1}`).join("\n");
    const lines = Array.from({ length: total }, (_, i) => blameLine(total - i));
    const view = mount(doc, createInlineBlameExtension({ enabled: true, lines }));
    const decorated = decoratedLines(view);
    expect(decorated).toHaveLength(5000);
    expect(decorated[0]).toBe(1);
    expect(decorated[decorated.length - 1]).toBe(5000);
  });

  it("换一份内容相同的 blame 后复用原 DOM 节点(eq 命中)", () => {
    const view = mount(
      "a\nb",
      createInlineBlameExtension({ enabled: true, lines: [blameLine(1)] }),
    );
    const before = view.dom.querySelector(".cm-inline-blame");
    view.dispatch({
      effects: StateEffect.reconfigure.of(
        createInlineBlameExtension({ enabled: true, lines: [blameLine(1)] }),
      ),
    });
    expect(view.dom.querySelector(".cm-inline-blame")).toBe(before);
  });

  it("commit 变了就换 DOM 节点(eq 不命中)", () => {
    const view = mount(
      "a\nb",
      createInlineBlameExtension({
        enabled: true,
        lines: [blameLine(1, { commit: "old", shortCommit: "old" })],
      }),
    );
    const before = view.dom.querySelector(".cm-inline-blame");
    view.dispatch({
      effects: StateEffect.reconfigure.of(
        createInlineBlameExtension({
          enabled: true,
          lines: [blameLine(1, { commit: "new", shortCommit: "new" })],
        }),
      ),
    });
    const after = view.dom.querySelector(".cm-inline-blame");
    expect(after).not.toBe(before);
    expect(after!.textContent).toContain("new");
  });

  it("blame 标记不吃鼠标事件(点它不该动光标)", () => {
    const view = mount(
      "hello",
      createInlineBlameExtension({ enabled: true, lines: [blameLine(1)] }),
    );
    const marker = view.dom.querySelector(".cm-inline-blame")!;
    const event = new MouseEvent("mousedown", { bubbles: true, cancelable: true });
    expect(() => marker.dispatchEvent(event)).not.toThrow();
    expect(view.state.doc.toString()).toBe("hello");
  });

  it("不改动文档内容,只是加装饰", () => {
    const view = mount(
      "hello\nworld",
      createInlineBlameExtension({ enabled: true, lines: [blameLine(1), blameLine(2)] }),
    );
    expect(view.state.doc.toString()).toBe("hello\nworld");
  });
});
