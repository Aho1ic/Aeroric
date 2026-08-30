import { describe, expect, it } from "vitest";
import { buildNoteGraph, layoutNoteGraph, type GraphNode } from "../components/notebook/noteGraph";
import { buildLinkIndex } from "../components/notebook/noteLinks";
import type { NoteLinkSource } from "../components/notebook/noteBacklinks";

/** 造一份索引。顺序即链接歧义时的优先级,也决定图上环内的稳定顺序。 */
function indexOf(...notes: Array<[path: string, title: string]>) {
  return buildLinkIndex(notes.map(([path, title]) => ({ path, title })));
}

/** 造一篇笔记的链接清单。`raw` 就是方括号里的原文。 */
function source(path: string, ...raws: string[]): NoteLinkSource {
  return {
    path,
    links: raws.map((raw, i) => ({ raw, line: i + 1, preview: `见 [[${raw}]]`, embed: false })),
  };
}

function edgeKeys(edges: readonly { from: string; to: string }[]): string[] {
  return edges.map((edge) => `${edge.from}->${edge.to}`).sort();
}

function nodeAt(nodes: readonly GraphNode[], path: string): GraphNode {
  const found = nodes.find((node) => node.path === path);
  if (!found) throw new Error(`no node for ${path}`);
  return found;
}

describe("buildNoteGraph", () => {
  it("把解析得到的链接折成有向边", () => {
    const index = indexOf(["/v/a.md", "甲"], ["/v/b.md", "乙"]);
    const graph = buildNoteGraph([source("/v/a.md", "b")], index);

    expect(edgeKeys(graph.edges)).toEqual(["/v/a.md->/v/b.md"]);
    expect(nodeAt(graph.nodes, "/v/a.md").outDegree).toBe(1);
    expect(nodeAt(graph.nodes, "/v/a.md").inDegree).toBe(0);
    expect(nodeAt(graph.nodes, "/v/b.md").inDegree).toBe(1);
  });

  it("按标题写的链接也能连上", () => {
    // 边的解析必须和反链走同一条路,否则"反链里有、图里没有"。
    const index = indexOf(["/v/a.md", "甲"], ["/v/hou-xie.md", "乙的标题"]);
    const graph = buildNoteGraph([source("/v/a.md", "乙的标题")], index);
    expect(edgeKeys(graph.edges)).toEqual(["/v/a.md->/v/hou-xie.md"]);
  });

  it("自引用不成边", () => {
    /* 一篇里写 [[自己]] 是排版手法(目录、模板)。画成边是一个长度为 0 看不见的
       圈,却让边数虚高 —— 和反链排除自引用是同一个判断。 */
    const index = indexOf(["/v/a.md", "甲"], ["/v/b.md", "乙"]);
    const graph = buildNoteGraph([source("/v/a.md", "a", "b")], index);

    expect(edgeKeys(graph.edges)).toEqual(["/v/a.md->/v/b.md"]);
    expect(nodeAt(graph.nodes, "/v/a.md").outDegree).toBe(1);
  });

  it("同一篇里重复指向同一篇折成一条边,并记下条数", () => {
    // Markio 原样画三条重合的线:看着和一条一样,却把"多少条链接"报成 3。
    const index = indexOf(["/v/a.md", "甲"], ["/v/b.md", "乙"]);
    const graph = buildNoteGraph([source("/v/a.md", "b", "b", "b")], index);

    expect(graph.edges).toHaveLength(1);
    expect(graph.edges[0]!.count).toBe(3);
    expect(graph.edges[0]!.mutual).toBe(false);
    // 度数数的是链接条数,不是边数。
    expect(nodeAt(graph.nodes, "/v/a.md").outDegree).toBe(3);
  });

  it("互指合成一条双箭头边", () => {
    const index = indexOf(["/v/a.md", "甲"], ["/v/b.md", "乙"]);
    const graph = buildNoteGraph([source("/v/a.md", "b"), source("/v/b.md", "a")], index);

    expect(graph.edges).toHaveLength(1);
    expect(graph.edges[0]!.mutual).toBe(true);
    expect(graph.edges[0]!.count).toBe(2);
  });

  it("同向重复不会被当成互指", () => {
    const index = indexOf(["/v/a.md", "甲"], ["/v/b.md", "乙"]);
    const graph = buildNoteGraph([source("/v/a.md", "b", "b")], index);
    expect(graph.edges[0]!.mutual).toBe(false);
  });

  it("解析不到的链接单独计数,不成边", () => {
    const index = indexOf(["/v/a.md", "甲"], ["/v/b.md", "乙"]);
    const graph = buildNoteGraph([source("/v/a.md", "b", "还没写", "也没写")], index);

    expect(graph.edges).toHaveLength(1);
    expect(graph.deadLinks).toBe(2);
    // 死链不该算进出度 —— 那一头不存在。
    expect(nodeAt(graph.nodes, "/v/a.md").outDegree).toBe(1);
  });

  it("嵌入算引用", () => {
    const index = indexOf(["/v/a.md", "甲"], ["/v/b.md", "乙"]);
    const graph = buildNoteGraph(
      [{ path: "/v/a.md", links: [{ raw: "b", line: 1, preview: "![[b]]", embed: true }] }],
      index,
    );
    expect(edgeKeys(graph.edges)).toEqual(["/v/a.md->/v/b.md"]);
  });

  it("孤立笔记只报数,不进图", () => {
    /* 五百篇笔记二十条链接时,把孤立的都画出来是四百八十个互不相连的点 ——
       那不是结构,是噪声。 */
    const index = indexOf(["/v/a.md", "甲"], ["/v/b.md", "乙"], ["/v/lone.md", "独"]);
    const graph = buildNoteGraph([source("/v/a.md", "b")], index);

    expect(graph.nodes.map((node) => node.path)).toEqual(["/v/a.md", "/v/b.md"]);
    expect(graph.orphans).toBe(1);
  });

  it("扫描结果里有、索引里已经没有的笔记跳过", () => {
    // 笔记刚被删掉而扫描是旧的。造一个节点出来的话,点开是空的。
    const index = indexOf(["/v/b.md", "乙"]);
    const graph = buildNoteGraph([source("/v/gone.md", "b")], index);

    expect(graph.nodes.map((node) => node.path)).toEqual([]);
    expect(graph.edges).toHaveLength(0);
  });

  it("链接指向已经不在索引里的笔记时算死链", () => {
    const index = indexOf(["/v/a.md", "甲"]);
    const graph = buildNoteGraph([source("/v/a.md", "已删")], index);
    expect(graph.deadLinks).toBe(1);
    expect(graph.edges).toHaveLength(0);
  });

  describe("焦点与跳数", () => {
    /* a → b → c,另有 d → a。以 a 为焦点时:a 是 0,b 和 d 是 1,c 是 2。
       d 是**指向** a 的,深度必须和 b 一样 —— 只顺箭头走会把反链那一半邻居
       排到图外,而那一半通常更重要。 */
    const index = indexOf(
      ["/v/a.md", "甲"],
      ["/v/b.md", "乙"],
      ["/v/c.md", "丙"],
      ["/v/d.md", "丁"],
    );
    const sources = [source("/v/a.md", "b"), source("/v/b.md", "c"), source("/v/d.md", "a")];

    it("按无向边算跳数,被引用的邻居和引用的邻居同深", () => {
      const graph = buildNoteGraph(sources, index, { focusPath: "/v/a.md" });
      expect(nodeAt(graph.nodes, "/v/a.md").depth).toBe(0);
      expect(nodeAt(graph.nodes, "/v/b.md").depth).toBe(1);
      expect(nodeAt(graph.nodes, "/v/d.md").depth).toBe(1);
      expect(nodeAt(graph.nodes, "/v/c.md").depth).toBe(2);
    });

    it("maxDepth 把远处的裁掉并报数", () => {
      const graph = buildNoteGraph(sources, index, { focusPath: "/v/a.md", maxDepth: 1 });
      expect(graph.nodes.map((node) => node.path).sort()).toEqual([
        "/v/a.md",
        "/v/b.md",
        "/v/d.md",
      ]);
      expect(graph.hidden).toBe(1);
      // 被裁掉的点连的边也不能留下 —— 那会是一条连到图外的悬空线。
      expect(edgeKeys(graph.edges)).toEqual(["/v/a.md->/v/b.md", "/v/d.md->/v/a.md"]);
    });

    it("度数按全库算,不受 maxDepth 影响", () => {
      // 裁的是画多少,不是"它其实有几条链接"。
      const graph = buildNoteGraph(sources, index, { focusPath: "/v/a.md", maxDepth: 1 });
      expect(nodeAt(graph.nodes, "/v/b.md").outDegree).toBe(1);
    });

    it("连不到焦点的那部分算 hidden", () => {
      const withIsland = indexOf(
        ["/v/a.md", "甲"],
        ["/v/b.md", "乙"],
        ["/v/x.md", "戊"],
        ["/v/y.md", "己"],
      );
      const graph = buildNoteGraph([source("/v/a.md", "b"), source("/v/x.md", "y")], withIsland, {
        focusPath: "/v/a.md",
      });
      expect(graph.nodes.map((node) => node.path)).toEqual(["/v/a.md", "/v/b.md"]);
      expect(graph.hidden).toBe(2);
    });

    it("焦点自己是孤立的时候退回整库图", () => {
      /* 只画焦点自己(一个孤零零的点)会把其余部分的结构全藏起来。孤立这件事由
         orphans 那个数说明,不用把整张图让给它。 */
      const withLone = indexOf(["/v/a.md", "甲"], ["/v/b.md", "乙"], ["/v/lone.md", "独"]);
      const graph = buildNoteGraph([source("/v/a.md", "b")], withLone, {
        focusPath: "/v/lone.md",
        maxDepth: 1,
      });
      expect(graph.nodes.map((node) => node.path)).toEqual(["/v/a.md", "/v/b.md"]);
      expect(graph.nodes.every((node) => node.depth === null)).toBe(true);
      expect(graph.hidden).toBe(0);
    });

    it("不给焦点时全部 depth 为 null", () => {
      const graph = buildNoteGraph(sources, index);
      expect(graph.nodes.every((node) => node.depth === null)).toBe(true);
    });
  });
});

describe("layoutNoteGraph", () => {
  function node(path: string, depth: number | null, inDeg = 0, outDeg = 0): GraphNode {
    return { path, title: path, inDegree: inDeg, outDegree: outDeg, depth };
  }

  function placementOf(placements: ReturnType<typeof layoutNoteGraph>["placements"], path: string) {
    const found = placements.find((item) => item.path === path);
    if (!found) throw new Error(`no placement for ${path}`);
    return found;
  }

  it("焦点画在圆心", () => {
    const { placements } = layoutNoteGraph([node("/v/a.md", 0)]);
    expect(placementOf(placements, "/v/a.md")).toMatchObject({ x: 0, y: 0, label: true });
  });

  it("一跳一环", () => {
    const { placements } = layoutNoteGraph([node("/v/a.md", 0), node("/v/b.md", 1)], {
      ringGap: 100,
    });
    const b = placementOf(placements, "/v/b.md");
    expect(Math.hypot(b.x, b.y)).toBeCloseTo(100);
  });

  it("两跳在更外一环", () => {
    const { placements } = layoutNoteGraph(
      [node("/v/a.md", 0), node("/v/b.md", 1), node("/v/c.md", 2)],
      { ringGap: 100 },
    );
    expect(Math.hypot(...positionOf(placements, "/v/c.md"))).toBeCloseTo(200);
  });

  function positionOf(
    placements: ReturnType<typeof layoutNoteGraph>["placements"],
    path: string,
  ): [number, number] {
    const found = placementOf(placements, path);
    return [found.x, found.y];
  }

  it("depth 为 null 的排在最外一环", () => {
    const { placements } = layoutNoteGraph(
      [node("/v/a.md", 0), node("/v/b.md", 1), node("/v/x.md", null)],
      { ringGap: 100 },
    );
    // 最深是 1,null 于是落在第 2 环。
    expect(Math.hypot(...positionOf(placements, "/v/x.md"))).toBeCloseTo(200);
  });

  it("整库模式(全部 null)铺成一个环", () => {
    const { placements } = layoutNoteGraph(
      [node("/v/a.md", null), node("/v/b.md", null), node("/v/c.md", null)],
      { ringGap: 100 },
    );
    for (const path of ["/v/a.md", "/v/b.md", "/v/c.md"]) {
      expect(Math.hypot(...positionOf(placements, path))).toBeCloseTo(100);
    }
    // 没有圆心那个点 —— 整库模式下谁都不是焦点。
    expect(placements.every((item) => item.x !== 0 || item.y !== 0)).toBe(true);
  });

  it("环内按总度数降序,同度按路径升序", () => {
    /* 必须是全序:重扫一次库如果顺序变了,整张图会重排,用户会以为结构变了。
       同度时用路径兜底 —— 度数相同的节点在库里的相对顺序不保证稳定。 */
    const { placements } = layoutNoteGraph(
      [
        node("/v/z.md", 1, 1, 0),
        node("/v/hot.md", 1, 5, 5),
        node("/v/a.md", 1, 1, 0),
        node("/v/mid.md", 1, 2, 0),
      ],
      { ringGap: 100 },
    );
    // 正上方开始顺时针,所以数组顺序就是环上顺序。
    expect(placements.map((item) => item.path)).toEqual([
      "/v/hot.md",
      "/v/mid.md",
      "/v/a.md",
      "/v/z.md",
    ]);
  });

  it("半径随总度数增长,并且有上下限", () => {
    const { placements } = layoutNoteGraph([
      node("/v/small.md", null, 0, 0),
      node("/v/big.md", null, 200, 200),
    ]);
    expect(placementOf(placements, "/v/small.md").r).toBe(4);
    expect(placementOf(placements, "/v/big.md").r).toBe(18);
  });

  it("入度和出度一起算进半径", () => {
    // 只按入度会让"到处引用别人"的索引页缩成一个小点。
    const { placements } = layoutNoteGraph([
      node("/v/in.md", null, 4, 0),
      node("/v/out.md", null, 0, 4),
    ]);
    expect(placementOf(placements, "/v/out.md").r).toBe(placementOf(placements, "/v/in.md").r);
  });

  it("环上挤不下时只给度数最高的几个画标签", () => {
    // Markio 给每个节点都画标签,节点一多就糊成一整片。
    const many = Array.from({ length: 40 }, (_, i) =>
      node(`/v/n${String(i).padStart(2, "0")}.md`, 1, 40 - i, 0),
    );
    const { placements } = layoutNoteGraph(many, { ringGap: 100, labelWidth: 50 });
    const labelled = placements.filter((item) => item.label);

    // 周长 2π·100 ≈ 628,除以 50 → 12 个。
    expect(labelled).toHaveLength(12);
    // 画的是度数最高的那几个。
    expect(labelled.map((item) => item.path)).toEqual(many.slice(0, 12).map((item) => item.path));
  });

  it("环上放得下时每个都画标签", () => {
    const { placements } = layoutNoteGraph(
      [node("/v/a.md", 1), node("/v/b.md", 1), node("/v/c.md", 1)],
      { ringGap: 100, labelWidth: 50 },
    );
    expect(placements.every((item) => item.label)).toBe(true);
  });

  it("extent 罩住最外那一环", () => {
    const { extent } = layoutNoteGraph([node("/v/a.md", 0), node("/v/b.md", 2)], {
      ringGap: 100,
    });
    expect(extent).toBeGreaterThanOrEqual(200);
  });

  it("只有焦点一个点时 extent 也是正数", () => {
    // viewBox 拿它算,是 0 的话整个 svg 会塌掉。
    const { extent } = layoutNoteGraph([node("/v/a.md", 0)]);
    expect(extent).toBeGreaterThan(0);
  });

  it("空图不炸", () => {
    const { placements, extent } = layoutNoteGraph([]);
    expect(placements).toEqual([]);
    expect(extent).toBe(0);
  });
});
