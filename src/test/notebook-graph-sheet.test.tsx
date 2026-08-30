/* 图谱 sheet 的画法与交互。
 *
 * 和 `notebook-panel.test.tsx` 里那一组的分工:那边验"面板把图谱接对了"(共用扫描、
 * 互斥、跳转、跳数),这边验"给定一张图,画出来对不对" —— 直接构造 `NoteGraph`,
 * 不经过扫描。这些用例在面板级要先 seed 一个库、等编辑器读入,一条要一秒多,而它们
 * 想钉的东西(箭头、边粗细、悬停淡化、键盘激活)和库里有什么无关。
 */

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";

import { NoteGraphSheet, DEPTH_ALL } from "../components/notebook/NoteGraphSheet";
import type { GraphEdge, GraphNode, NoteGraph } from "../components/notebook/noteGraph";
import { staticT } from "../i18n";

function node(path: string, extra: Partial<GraphNode> = {}): GraphNode {
  return {
    path,
    title: path.replace(/^.*\//, "").replace(/\.md$/, ""),
    inDegree: 1,
    outDegree: 1,
    depth: 1,
    ...extra,
  };
}

function edge(from: string, to: string, extra: Partial<GraphEdge> = {}): GraphEdge {
  return { from, to, count: 1, mutual: false, ...extra };
}

function graphOf(partial: Partial<NoteGraph> = {}): NoteGraph {
  return { nodes: [], edges: [], deadLinks: 0, orphans: 0, hidden: 0, ...partial };
}

type Overrides = Partial<Parameters<typeof NoteGraphSheet>[0]>;

function renderSheet(graph: NoteGraph, overrides: Overrides = {}) {
  const onOpenNote = vi.fn();
  const onClose = vi.fn();
  const onRefresh = vi.fn();
  const onDepthChange = vi.fn();
  render(
    <NoteGraphSheet
      graph={graph}
      focusPath={null}
      loading={false}
      error={null}
      depth={2}
      onDepthChange={onDepthChange}
      onOpenNote={onOpenNote}
      onRefresh={onRefresh}
      onClose={onClose}
      t={staticT}
      {...overrides}
    />,
  );
  return { onOpenNote, onClose, onRefresh, onDepthChange };
}

/** 图上的一个节点组。 */
function nodeAt(path: string): SVGGElement {
  const found = document.querySelector<SVGGElement>(`[data-graph-node="${path}"]`);
  if (!found) throw new Error(`no node for ${path}`);
  return found;
}

function lineOf(from: string, to: string): SVGLineElement {
  const found = document.querySelector<SVGLineElement>(`[data-graph-edge="${from}|${to}"]`);
  if (!found) throw new Error(`no edge for ${from}|${to}`);
  return found;
}

/** 一个节点的圆点透明度。淡化用它表达。 */
function circleOpacity(path: string): string | null {
  return nodeAt(path).querySelector("circle")?.getAttribute("fill-opacity") ?? null;
}

describe("NoteGraphSheet", () => {
  it("每个节点都是可点、可键盘聚焦的按钮", () => {
    // Markio 的图谱只能用鼠标点 —— 键盘用户看得见图却进不去。
    const { onOpenNote } = renderSheet(
      graphOf({ nodes: [node("/v/a.md"), node("/v/b.md")], edges: [edge("/v/a.md", "/v/b.md")] }),
    );

    const button = screen.getByRole("button", { name: "Open a" });
    expect(button.getAttribute("tabindex")).toBe("0");
    fireEvent.click(button);
    expect(onOpenNote).toHaveBeenCalledWith("/v/a.md");
  });

  it("Enter 和空格都能打开,空格不滚页", () => {
    const { onOpenNote } = renderSheet(graphOf({ nodes: [node("/v/a.md")] }));
    const button = screen.getByRole("button", { name: "Open a" });

    fireEvent.keyDown(button, { key: "Enter" });
    expect(onOpenNote).toHaveBeenCalledWith("/v/a.md");

    // 空格的默认行为是滚动那块画布,不拦掉的话按一下图会跳走。
    const spacePrevented = !fireEvent.keyDown(button, { key: " " });
    expect(onOpenNote).toHaveBeenCalledTimes(2);
    expect(spacePrevented).toBe(true);
  });

  it("别的键不触发打开", () => {
    const { onOpenNote } = renderSheet(graphOf({ nodes: [node("/v/a.md")] }));
    fireEvent.keyDown(screen.getByRole("button", { name: "Open a" }), { key: "a" });
    expect(onOpenNote).not.toHaveBeenCalled();
  });

  it("当前这篇画成焦点色", () => {
    renderSheet(graphOf({ nodes: [node("/v/a.md"), node("/v/b.md")] }), { focusPath: "/v/b.md" });

    expect(nodeAt("/v/b.md").getAttribute("data-graph-focus")).toBe("true");
    expect(nodeAt("/v/a.md").getAttribute("data-graph-focus")).toBeNull();
    /* 光验那个 data-* 是不够的:它只是测试用的钩子,真正让用户认出"我在这儿"的是
       填色。两者分开断言,漏改填色时才看得出来。 */
    const focused = nodeAt("/v/b.md").querySelector("circle")?.getAttribute("fill");
    expect(focused).toBe("var(--accent)");
    expect(nodeAt("/v/a.md").querySelector("circle")?.getAttribute("fill")).not.toBe(focused);
  });

  it("单向边一个箭头,互指的边两头都有", () => {
    // 只画线的话双链和单链看着一样,而"互相引用"是图上最值得看见的关系。
    renderSheet(
      graphOf({
        nodes: [node("/v/a.md"), node("/v/b.md"), node("/v/c.md")],
        edges: [edge("/v/a.md", "/v/b.md"), edge("/v/a.md", "/v/c.md", { mutual: true })],
      }),
    );

    const single = lineOf("/v/a.md", "/v/b.md");
    expect(single.getAttribute("marker-end")).toBeTruthy();
    expect(single.getAttribute("marker-start")).toBeNull();

    const both = lineOf("/v/a.md", "/v/c.md");
    expect(both.getAttribute("marker-end")).toBeTruthy();
    expect(both.getAttribute("marker-start")).toBeTruthy();
    // 互指折成一条双箭头的边,不是两条重合的线。
    expect(document.querySelectorAll("[data-graph-edge]")).toHaveLength(2);
  });

  it("重复链接让边变粗,但有上限", () => {
    renderSheet(
      graphOf({
        nodes: [node("/v/a.md"), node("/v/b.md"), node("/v/c.md")],
        edges: [edge("/v/a.md", "/v/b.md"), edge("/v/a.md", "/v/c.md", { count: 40 })],
      }),
    );

    const thin = Number(lineOf("/v/a.md", "/v/b.md").getAttribute("stroke-width"));
    const thick = Number(lineOf("/v/a.md", "/v/c.md").getAttribute("stroke-width"));
    expect(thick).toBeGreaterThan(thin);
    // 一条 40 次的边不该变成根柱子。
    expect(thick).toBeLessThanOrEqual(3);
  });

  it("连不到画上的边不画(节点被裁掉时)", () => {
    /* 模型层已经把边过滤到保留节点了,这里是第二道:布局里查不到坐标就跳过,而不是
       画一条从 (0,0) 出发的线。两处的判据不同(那边看节点集,这边看布局结果),
       所以不算重复的闸门。 */
    renderSheet(graphOf({ nodes: [node("/v/a.md")], edges: [edge("/v/a.md", "/v/gone.md")] }));

    expect(document.querySelectorAll("[data-graph-edge]")).toHaveLength(0);
    expect(document.querySelectorAll("[data-graph-node]")).toHaveLength(1);
  });

  it("悬停高亮邻居,压暗其余", () => {
    renderSheet(
      graphOf({
        nodes: [node("/v/a.md"), node("/v/b.md"), node("/v/c.md")],
        // a→b 顺着箭头,c→a 逆着 —— 两个方向都算邻居。
        edges: [edge("/v/a.md", "/v/b.md"), edge("/v/c.md", "/v/a.md")],
      }),
    );

    // 没悬停时谁都不淡。
    expect(circleOpacity("/v/b.md")).toBe("1");

    fireEvent.mouseEnter(nodeAt("/v/a.md"));
    expect(circleOpacity("/v/a.md")).toBe("1");
    expect(circleOpacity("/v/b.md")).toBe("1");
    expect(circleOpacity("/v/c.md")).toBe("1");

    fireEvent.mouseEnter(nodeAt("/v/b.md"));
    // b 的邻居只有 a,c 该淡下去。
    expect(circleOpacity("/v/b.md")).toBe("1");
    expect(circleOpacity("/v/a.md")).toBe("1");
    expect(circleOpacity("/v/c.md")).not.toBe("1");

    fireEvent.mouseLeave(nodeAt("/v/b.md"));
    expect(circleOpacity("/v/c.md")).toBe("1");
  });

  it("键盘聚焦也高亮,和悬停同一套", () => {
    // 只挂 mouseenter 的话键盘用户走到一个点上看不出它连着谁。
    renderSheet(
      graphOf({
        nodes: [node("/v/a.md"), node("/v/b.md"), node("/v/c.md")],
        edges: [edge("/v/a.md", "/v/b.md")],
      }),
    );

    fireEvent.focus(nodeAt("/v/a.md"));
    expect(circleOpacity("/v/c.md")).not.toBe("1");
    fireEvent.blur(nodeAt("/v/a.md"));
    expect(circleOpacity("/v/c.md")).toBe("1");
  });

  it("悬停时无关的边也淡下去", () => {
    renderSheet(
      graphOf({
        nodes: [node("/v/a.md"), node("/v/b.md"), node("/v/c.md"), node("/v/d.md")],
        edges: [edge("/v/a.md", "/v/b.md"), edge("/v/c.md", "/v/d.md")],
      }),
    );

    const near = lineOf("/v/a.md", "/v/b.md");
    const far = lineOf("/v/c.md", "/v/d.md");
    const before = far.getAttribute("stroke-opacity");

    fireEvent.mouseEnter(nodeAt("/v/a.md"));

    expect(near.getAttribute("stroke-opacity")).toBe(before);
    expect(far.getAttribute("stroke-opacity")).not.toBe(before);
  });

  it("Esc 关掉,并且不往上冒", () => {
    const { onClose } = renderSheet(graphOf({ nodes: [node("/v/a.md")] }));
    const outer = vi.fn();
    document.addEventListener("keydown", outer);

    try {
      fireEvent.keyDown(screen.getByRole("dialog", { name: "Link graph" }), { key: "Escape" });
      expect(onClose).toHaveBeenCalledTimes(1);
      // 面板将来加一层 Esc 时,少了 stopPropagation 就变成一次按键关两层。
      expect(outer).not.toHaveBeenCalled();
    } finally {
      document.removeEventListener("keydown", outer);
    }
  });

  it("别的键不关", () => {
    const { onClose } = renderSheet(graphOf({ nodes: [node("/v/a.md")] }));
    fireEvent.keyDown(screen.getByRole("dialog", { name: "Link graph" }), { key: "Enter" });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("打开时焦点落在关闭按钮上", () => {
    // 不挪的话焦点还在编辑器上,上面那条 Esc 用例在真实使用里根本不成立。
    renderSheet(graphOf({ nodes: [node("/v/a.md")] }));
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Close graph" }));
  });

  it("换跳数把新值报上去,包括「整个库」", () => {
    const { onDepthChange } = renderSheet(graphOf({ nodes: [node("/v/a.md")] }));
    const select = screen.getByRole("combobox", { name: "Depth" });

    fireEvent.change(select, { target: { value: "1" } });
    expect(onDepthChange).toHaveBeenLastCalledWith(1);

    fireEvent.change(select, { target: { value: String(DEPTH_ALL) } });
    // 数字而不是字符串 —— 面板拿它和 `DEPTH_ALL` 比。
    expect(onDepthChange).toHaveBeenLastCalledWith(DEPTH_ALL);
  });

  it("重扫按钮把请求转出去", () => {
    const { onRefresh } = renderSheet(graphOf({ nodes: [node("/v/a.md")] }));
    fireEvent.click(screen.getByRole("button", { name: "Rescan" }));
    expect(onRefresh).toHaveBeenCalledTimes(1);
  });

  it("头部报出篇数、链接数、孤立、范围外、失效", () => {
    const sheet = (() => {
      renderSheet(
        graphOf({
          nodes: [node("/v/a.md"), node("/v/b.md")],
          edges: [edge("/v/a.md", "/v/b.md")],
          orphans: 3,
          hidden: 4,
          deadLinks: 5,
        }),
      );
      return screen.getByRole("dialog", { name: "Link graph" });
    })();

    expect(sheet).toHaveTextContent("2 notes");
    expect(sheet).toHaveTextContent("1 links");
    expect(sheet).toHaveTextContent("3 unlinked");
    expect(sheet).toHaveTextContent("4 out of range");
    expect(sheet).toHaveTextContent("5 broken");
  });

  it("三个计数为 0 时不占位置", () => {
    // 「0 篇未连接」是噪声:那是常态,不是要报告的事。
    renderSheet(
      graphOf({ nodes: [node("/v/a.md"), node("/v/b.md")], edges: [edge("/v/a.md", "/v/b.md")] }),
    );

    const sheet = screen.getByRole("dialog", { name: "Link graph" });
    expect(sheet).not.toHaveTextContent("unlinked");
    expect(sheet).not.toHaveTextContent("out of range");
    expect(sheet).not.toHaveTextContent("broken");
  });

  it("空图显示空态,不是空白", () => {
    renderSheet(graphOf());
    expect(screen.getByRole("dialog", { name: "Link graph" })).toHaveTextContent(
      "No links between notes yet",
    );
  });

  it("首次载入显示载入中而不是空态", () => {
    // 报空态会让用户以为库里真的没有链接,而实际上还没读完。
    renderSheet(graphOf(), { loading: true });
    const sheet = screen.getByRole("dialog", { name: "Link graph" });
    expect(sheet).toHaveTextContent("Scanning links…");
    expect(sheet).not.toHaveTextContent("No links between notes yet");
  });

  it("重扫时留着旧图,不退回载入中", () => {
    // 清空成"什么都没有"比留着旧结果更糟 —— 那看起来像扫完了、确实没有。
    renderSheet(
      graphOf({ nodes: [node("/v/a.md"), node("/v/b.md")], edges: [edge("/v/a.md", "/v/b.md")] }),
      { loading: true },
    );

    expect(document.querySelectorAll("[data-graph-node]")).toHaveLength(2);
    expect(screen.getByRole("dialog", { name: "Link graph" })).not.toHaveTextContent(
      "Scanning links…",
    );
  });

  it("扫描失败就地报错,不显示成空库", () => {
    renderSheet(graphOf(), { error: "scanning links failed" });

    const sheet = screen.getByRole("dialog", { name: "Link graph" });
    expect(sheet).toHaveTextContent("scanning links failed");
    // 「扫不动」和「确实没有链接」是两回事。
    expect(sheet).not.toHaveTextContent("No links between notes yet");
  });

  it("报错时旧图还在", () => {
    renderSheet(
      graphOf({ nodes: [node("/v/a.md"), node("/v/b.md")], edges: [edge("/v/a.md", "/v/b.md")] }),
      { error: "scanning links failed" },
    );

    const sheet = screen.getByRole("dialog", { name: "Link graph" });
    expect(sheet).toHaveTextContent("scanning links failed");
    expect(document.querySelectorAll("[data-graph-node]")).toHaveLength(2);
  });

  it("只给一部分节点画标签时,画的是度数高的那些", () => {
    /* 一环上挤不下所有标签时按度数取舍。Markio 给每个都画,节点一多就糊成一片。
       取舍规则本身在 `notebook-graph.test.ts` 里验,这里验"画出来的 text 只有那
       几个" —— 布局算对了但渲染时忽略了 `label` 的话那边测不出来。 */
    const nodes = Array.from({ length: 40 }, (_, index) =>
      node(`/v/n${String(index).padStart(2, "0")}.md`, {
        inDegree: 40 - index,
        outDegree: 0,
      }),
    );
    renderSheet(graphOf({ nodes }));

    const labels = [...document.querySelectorAll("[data-graph-canvas] text")].map(
      (text) => text.textContent,
    );
    expect(labels.length).toBeGreaterThan(0);
    expect(labels.length).toBeLessThan(nodes.length);
    // 度数最高的那个一定在。
    expect(labels).toContain("n00");
  });

  it("标签不吃鼠标事件", () => {
    // 吃了的话相邻标签会盖住旁边那个点,点不动。
    renderSheet(graphOf({ nodes: [node("/v/a.md")] }));
    const label = document.querySelector("[data-graph-canvas] text") as SVGTextElement;
    expect(label.style.pointerEvents).toBe("none");
  });

  it("铺满容器而不是整个窗口", () => {
    renderSheet(graphOf({ nodes: [node("/v/a.md")] }));
    const sheet = screen.getByRole("dialog", { name: "Link graph" });
    expect(sheet.style.position).toBe("absolute");
    expect(sheet.getAttribute("aria-modal")).toBe("true");
  });

  it("viewBox 随图的范围走", () => {
    // 写死一个 viewBox 的话外环会被裁掉,而那正是节点最多的一环。
    renderSheet(graphOf({ nodes: [node("/v/a.md", { depth: 1 })] }));
    /* 必须按 `[data-graph-canvas]` 取,不能 `querySelector("svg")` —— 头部那几个
       lucide 图标也是 <svg> 而且排在前面,取到的是图标,两次拿到的都是 undefined,
       断言就成了空的(验过:写死 viewBox 时这条照样绿)。 */
    const near = document.querySelector("[data-graph-canvas]")?.getAttribute("viewBox");

    render(
      <NoteGraphSheet
        graph={graphOf({ nodes: [node("/v/b.md", { depth: 3 })] })}
        focusPath={null}
        loading={false}
        error={null}
        depth={3}
        onDepthChange={() => {}}
        onOpenNote={() => {}}
        onRefresh={() => {}}
        onClose={() => {}}
        t={staticT}
      />,
    );
    const far = [...document.querySelectorAll("[data-graph-canvas]")]
      .pop()
      ?.getAttribute("viewBox");

    expect(near).toBeTruthy();
    expect(near).not.toBe(far);
    // 三跳那张更远,范围更大。
    expect(Number(far?.split(" ")[2])).toBeGreaterThan(Number(near?.split(" ")[2]));
  });
});
