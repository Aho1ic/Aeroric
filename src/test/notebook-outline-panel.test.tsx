import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { NoteOutlinePanel } from "../components/notebook/NoteOutlinePanel";
import { analyzeNote } from "../components/notebook/noteOutline";
import { staticT } from "../i18n";

/** 大纲永远由 analyzeNote 产出,测试也走它 —— 手搓 items 会漏掉偏移这类字段。 */
const outlineOf = (source: string) => analyzeNote(source).outline;

const NESTED = "# Intro\n\n## Setup\n\n### Deps\n\n## Usage\n\n# Appendix\n";

function renderPanel(
  source: string,
  overrides: {
    onJump?: (index: number) => void;
    onReorder?: (from: number, to: number) => void;
  } = {},
) {
  const items = outlineOf(source);
  const onJump = vi.fn((item: (typeof items)[number]) => {
    overrides.onJump?.(items.indexOf(item));
  });
  const view = render(
    <NoteOutlinePanel items={items} onJump={onJump} onReorder={overrides.onReorder} t={staticT} />,
  );
  return { items, onJump, view };
}

/** 大纲里可见的标题按钮(排掉搜索框旁的折叠/清除按钮)。 */
const headingButtons = () =>
  screen
    .getAllByRole("button")
    .filter((button) => button.querySelector("svg") === null && button.textContent);

const visibleHeadings = () => headingButtons().map((button) => button.textContent);

describe("NoteOutlinePanel — 渲染", () => {
  it("按顺序列出全部标题", () => {
    renderPanel(NESTED);
    expect(visibleHeadings()).toEqual(["Intro", "Setup", "Deps", "Usage", "Appendix"]);
  });

  it("按层级缩进,并封到 4 级", () => {
    renderPanel("# L1\n\n## L2\n\n### L3\n\n#### L4\n\n##### L5\n\n###### L6\n");
    const rows = headingButtons().map(
      (button) => (button.parentElement as HTMLElement).style.paddingLeft,
    );
    // 5、6 级和 4 级同缩进 —— 再深就挤没了。
    expect(rows).toEqual(["0px", "10px", "20px", "30px", "30px", "30px"]);
  });

  it("没有标题时给空态文案", () => {
    renderPanel("just body text\n");
    expect(screen.getByText(staticT("notebook.outlineEmpty"))).toBeTruthy();
    expect(headingButtons()).toHaveLength(0);
  });

  it("点标题回调对应的那一项", async () => {
    const user = userEvent.setup();
    const jumped: number[] = [];
    renderPanel(NESTED, { onJump: (index) => jumped.push(index) });
    await user.click(screen.getByRole("button", { name: "Usage" }));
    expect(jumped).toEqual([3]);
  });

  it("可拖时表头提示改成拖动提示", () => {
    const { view } = renderPanel(NESTED, { onReorder: vi.fn() });
    expect(screen.getByText(staticT("notebook.outlineDragHint"))).toBeTruthy();
    view.unmount();
    renderPanel(NESTED);
    // 阅读态没有可编辑源码,不给拖动提示,免得点了没反应。
    expect(screen.getByText(staticT("notebook.outlineSections"))).toBeTruthy();
  });
});

describe("NoteOutlinePanel — 折叠", () => {
  it("折叠父标题会藏起整棵子树", async () => {
    const user = userEvent.setup();
    renderPanel(NESTED);
    // Intro 下面挂着 Setup / Deps / Usage,Appendix 是同级的另一棵。
    await user.click(screen.getAllByRole("button", { name: staticT("notebook.collapse") })[0]!);
    expect(visibleHeadings()).toEqual(["Intro", "Appendix"]);
  });

  it("没有子节点的标题不给折叠按钮", () => {
    renderPanel("# A\n\n# B\n");
    expect(screen.queryByRole("button", { name: staticT("notebook.collapse") })).toBeNull();
  });

  it("全部折叠 / 全部展开", async () => {
    const user = userEvent.setup();
    renderPanel(NESTED);
    await user.click(screen.getByRole("button", { name: staticT("notebook.collapseAll") }));
    expect(visibleHeadings()).toEqual(["Intro", "Appendix"]);
    await user.click(screen.getByRole("button", { name: staticT("notebook.expandAll") }));
    expect(visibleHeadings()).toEqual(["Intro", "Setup", "Deps", "Usage", "Appendix"]);
  });

  it("改正文不改标题时保留折叠态", async () => {
    const user = userEvent.setup();
    const { view } = renderPanel(NESTED);
    await user.click(screen.getAllByRole("button", { name: staticT("notebook.collapse") })[0]!);
    expect(visibleHeadings()).toEqual(["Intro", "Appendix"]);

    // 敲字会让父组件传进来一个**新数组**。按数组身份重置折叠的话,连续输入
    // 会把用户手动折叠的章节不断展开 —— 这是这个测试要钉住的东西。
    view.rerender(
      <NoteOutlinePanel
        items={outlineOf(NESTED.replace("## Setup", "## Setup\n\nnew body text"))}
        onJump={vi.fn()}
        t={staticT}
      />,
    );
    expect(visibleHeadings()).toEqual(["Intro", "Appendix"]);
  });

  it("标题结构变了就重置折叠态", async () => {
    const user = userEvent.setup();
    const { view } = renderPanel(NESTED);
    await user.click(screen.getAllByRole("button", { name: staticT("notebook.collapse") })[0]!);
    expect(visibleHeadings()).toEqual(["Intro", "Appendix"]);

    // 折叠态是按**下标**存的。标题增删之后下标指向的已经是另一项了,
    // 留着会折错章节,只能整体清掉。
    //
    // 这里必须让 0 号在新结构里**仍然是个有子节点的父标题**,否则留着旧折叠态也
    // 藏不住任何东西,测试就看不出重置有没有发生(换成在前面插一个childless 标题
    // 时,两种实现的可见集合完全一样)。
    view.rerender(
      <NoteOutlinePanel items={outlineOf(`${NESTED}\n## Extra\n`)} onJump={vi.fn()} t={staticT} />,
    );
    expect(visibleHeadings()).toEqual(["Intro", "Setup", "Deps", "Usage", "Appendix", "Extra"]);
  });
});

describe("NoteOutlinePanel — 过滤", () => {
  const filter = () => screen.getByRole("searchbox", { name: staticT("notebook.filterSections") });

  it("只留命中项和它们的祖先链", async () => {
    const user = userEvent.setup();
    renderPanel(NESTED);
    await user.type(filter(), "deps");
    // Setup / Intro 自己没命中,但要留着给 Deps 提供层级上下文。
    expect(visibleHeadings()).toEqual(["Intro", "Setup", "Deps"]);
  });

  it("忽略大小写", async () => {
    const user = userEvent.setup();
    renderPanel(NESTED);
    await user.type(filter(), "USAGE");
    expect(visibleHeadings()).toEqual(["Intro", "Usage"]);
  });

  it("命中段落打高亮", async () => {
    const user = userEvent.setup();
    renderPanel(NESTED);
    await user.type(filter(), "pen");
    const marks = Array.from(document.querySelectorAll("mark")).map((node) => node.textContent);
    expect(marks).toEqual(["pen"]);
  });

  it("报命中条数", async () => {
    const user = userEvent.setup();
    renderPanel("# Set A\n\n# Set B\n\n# Other\n");
    await user.type(filter(), "set");
    expect(screen.getByText(staticT("notebook.outlineHits", { count: "2" }))).toBeTruthy();
  });

  it("没有命中就一条不留", async () => {
    const user = userEvent.setup();
    renderPanel(NESTED);
    await user.type(filter(), "nothing matches this");
    expect(visibleHeadings()).toEqual([]);
    expect(screen.getByText(staticT("notebook.outlineHits", { count: "0" }))).toBeTruthy();
  });

  it("搜索时禁掉折叠", async () => {
    const user = userEvent.setup();
    renderPanel(NESTED);
    await user.type(filter(), "deps");
    // 搜索结果是稀疏的,允许折叠会把刚找出来的祖先链又藏回去。
    expect(screen.getByRole("button", { name: staticT("notebook.collapseAll") })).toHaveProperty(
      "disabled",
      true,
    );
    for (const button of screen.getAllByRole("button", { name: staticT("notebook.collapse") })) {
      expect(button).toHaveProperty("disabled", true);
    }
  });

  it("清空过滤后恢复折叠态", async () => {
    const user = userEvent.setup();
    renderPanel(NESTED);
    await user.click(screen.getAllByRole("button", { name: staticT("notebook.collapse") })[0]!);
    await user.type(filter(), "deps");
    expect(visibleHeadings()).toEqual(["Intro", "Setup", "Deps"]);
    await user.click(screen.getByRole("button", { name: staticT("common.clear") }));
    expect(visibleHeadings()).toEqual(["Intro", "Appendix"]);
  });

  it("Esc 清空过滤且不冒泡出去", async () => {
    const user = userEvent.setup();
    const onOuterEscape = vi.fn();
    render(
      // 面板外层可能拿 Esc 做别的事(关掉查找栏之类),清过滤不该顺带触发它。
      <div onKeyDown={(event) => event.key === "Escape" && onOuterEscape()}>
        <NoteOutlinePanel items={outlineOf(NESTED)} onJump={vi.fn()} t={staticT} />
      </div>,
    );
    await user.type(filter(), "deps");
    await user.keyboard("{Escape}");
    expect((filter() as HTMLInputElement).value).toBe("");
    expect(onOuterEscape).not.toHaveBeenCalled();
    expect(visibleHeadings()).toEqual(["Intro", "Setup", "Deps", "Usage", "Appendix"]);
  });

  it("过滤为空时 Esc 照常冒泡", async () => {
    const user = userEvent.setup();
    const onOuterEscape = vi.fn();
    render(
      <div onKeyDown={(event) => event.key === "Escape" && onOuterEscape()}>
        <NoteOutlinePanel items={outlineOf(NESTED)} onJump={vi.fn()} t={staticT} />
      </div>,
    );
    filter().focus();
    await user.keyboard("{Escape}");
    // 没东西可清的时候不该吞掉按键 —— 用户按 Esc 是想关别的东西。
    expect(onOuterEscape).toHaveBeenCalled();
  });
});

describe("NoteOutlinePanel — 拖动重排", () => {
  /** 给每一行铺一个 30px 高的假矩形,命中测试才有东西可读。 */
  const layoutRows = () => {
    const rows = headingButtons().map((button) => button.parentElement as HTMLElement);
    rows.forEach((row, index) => {
      const top = index * 30;
      row.getBoundingClientRect = () =>
        ({
          top,
          bottom: top + 30,
          height: 30,
          left: 0,
          right: 190,
          width: 190,
          x: 0,
          y: top,
          toJSON: () => ({}),
        }) as DOMRect;
    });
    return rows;
  };

  const dragRow = (from: number, toClientY: number) => {
    const button = headingButtons()[from]!;
    button.setPointerCapture = vi.fn();
    button.releasePointerCapture = vi.fn();
    fireEvent.pointerDown(button, { pointerId: 1, button: 0, clientY: from * 30 + 15 });
    fireEvent.pointerMove(button, { pointerId: 1, clientY: toClientY });
    fireEvent.pointerUp(button, { pointerId: 1, clientY: toClientY });
  };

  it("拖到另一项上给出源下标和目标下标", () => {
    const onReorder = vi.fn();
    renderPanel(NESTED, { onReorder });
    layoutRows();
    // Appendix(4)拖到 Setup(1)那一行。
    dragRow(4, 45);
    expect(onReorder).toHaveBeenCalledWith(4, 1);
  });

  it("原地轻点只跳转不重排", () => {
    const onReorder = vi.fn();
    const jumped: number[] = [];
    renderPanel(NESTED, { onReorder, onJump: (index) => jumped.push(index) });
    layoutRows();
    const button = headingButtons()[2]!;
    button.setPointerCapture = vi.fn();
    button.releasePointerCapture = vi.fn();
    // 位移在容差之内 —— 算点击。
    fireEvent.pointerDown(button, { pointerId: 1, button: 0, clientY: 75 });
    fireEvent.pointerMove(button, { pointerId: 1, clientY: 77 });
    fireEvent.pointerUp(button, { pointerId: 1, clientY: 77 });
    fireEvent.click(button);
    expect(onReorder).not.toHaveBeenCalled();
    expect(jumped).toEqual([2]);
  });

  it("拖动之后紧跟的 click 不触发跳转", () => {
    const onReorder = vi.fn();
    const jumped: number[] = [];
    renderPanel(NESTED, { onReorder, onJump: (index) => jumped.push(index) });
    layoutRows();
    dragRow(4, 45);
    // 松手后浏览器会补一次 click,不吞掉就会重排完还顺带跳一次。
    fireEvent.click(headingButtons()[4]!);
    expect(jumped).toEqual([]);
  });

  it("不给 onReorder 就不接受拖动", () => {
    const jumped: number[] = [];
    renderPanel(NESTED, { onJump: (index) => jumped.push(index) });
    layoutRows();
    dragRow(4, 45);
    fireEvent.click(headingButtons()[4]!);
    // 没有拖动通道,那次 click 就是普通点击,照常跳转。
    expect(jumped).toEqual([4]);
  });

  it("搜索时禁掉拖动", async () => {
    const user = userEvent.setup();
    const onReorder = vi.fn();
    renderPanel(NESTED, { onReorder });
    await user.type(
      screen.getByRole("searchbox", { name: staticT("notebook.filterSections") }),
      "s",
    );
    layoutRows();
    dragRow(headingButtons().length - 1, 15);
    expect(onReorder).not.toHaveBeenCalled();
  });
});
