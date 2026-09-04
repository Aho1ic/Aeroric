/* 引用图谱的模型层:把「全库链接扫描」折成一张图,再算出每个节点画在哪。
 *
 * 纯函数,不碰 IPC 也不碰 DOM。和反链共用同一次扫描(`vaultLinks`)与同一条解析
 * 路径(`parseWikiLinkBody` + `resolveLink`)—— 两个视图各写一遍的话,"谁指向谁"
 * 会在某些边界上给出不同答案(自引用、死链、同一行两个链接),而那种偏差的表现是
 * "反链里有这条、图里没有",没人会往解析规则上想。
 *
 * 布局单独一个函数:jsdom 没有布局,坐标算在纯函数里才测得出"分环、排序、标签
 * 取舍"这些真正会出错的地方。画在 SVG 而不是 cytoscape,见 `NoteGraphSheet`。
 */

import {
  normalizeLinkTarget,
  parseWikiLinkBody,
  resolveLink,
  type VaultLinkIndex,
} from "./noteLinks";
import type { NoteLinkSource } from "./noteBacklinks";
import { compareNotebookPath } from "../../lib/notebookSort";

/** 图上的一篇笔记。 */
export type GraphNode = {
  /** 绝对路径,与笔记列表里的 `id` 同一个值。 */
  path: string;
  title: string;
  /** 有多少篇指向它。 */
  inDegree: number;
  /** 它指向多少篇。 */
  outDegree: number;
  /**
   * 到焦点的跳数,**按无向边**算。焦点自己是 0;连不到焦点时是 null。
   *
   * 无向是刻意的:"这篇和那篇有关系"是双向的认知,只顺着箭头走会把"被引用"那一
   * 半邻居排到图外,而那半通常更重要(反链就是它)。
   */
  depth: number | null;
};

/** 一条边。方向是 `from → to`(from 里写了指向 to 的链接)。 */
export type GraphEdge = {
  from: string;
  to: string;
  /**
   * 折叠掉的原始链接条数。同一篇里写三次 `[[目标]]` 是一条边、count 3 ——
   * Markio 原样画三条重合的线,看着和一条一样,却让"多少条链接"这个数虚高。
   */
  count: number;
  /** 两个方向都有链接。画成双箭头,而不是两条重合的线。 */
  mutual: boolean;
};

export type NoteGraph = {
  nodes: GraphNode[];
  edges: GraphEdge[];
  /**
   * 解析不到目标的链接条数。它们不成边(没有"那一头"可连),但要报出来 ——
   * 死链是图谱最该暴露的东西之一。
   */
  deadLinks: number;
  /** 既没指别人、也没被指的笔记数。不画进图里,只报数(见下)。 */
  orphans: number;
  /** 有边、但离焦点超过 `maxDepth` 的笔记数。 */
  hidden: number;
};

export type BuildGraphOptions = {
  /** 焦点笔记的绝对路径。给了就按到它的距离分环。 */
  focusPath?: string | null;
  /** 离焦点几跳以内画进来。不给就不限。 */
  maxDepth?: number;
};

/** 无向邻接表,BFS 用。键值都是归一化路径。 */
type Adjacency = Map<string, Set<string>>;

function link(adjacency: Adjacency, a: string, b: string): void {
  const forA = adjacency.get(a) ?? new Set<string>();
  forA.add(b);
  adjacency.set(a, forA);
  const forB = adjacency.get(b) ?? new Set<string>();
  forB.add(a);
  adjacency.set(b, forB);
}

/** 从 `start` 出发的无向 BFS,返回每个点的跳数。 */
function bfsDepths(adjacency: Adjacency, start: string): Map<string, number> {
  const depths = new Map<string, number>([[start, 0]]);
  let frontier = [start];
  let depth = 0;
  while (frontier.length) {
    depth += 1;
    const next: string[] = [];
    for (const node of frontier) {
      for (const neighbor of adjacency.get(node) ?? []) {
        if (depths.has(neighbor)) continue;
        depths.set(neighbor, depth);
        next.push(neighbor);
      }
    }
    frontier = next;
  }
  return depths;
}

/**
 * 折出引用图谱。
 *
 * 和反链一致的三条:**自引用不算**(一篇里写 `[[自己]]` 是排版手法,画成边是一个
 * 长度为 0 的看不见的圈,却让边数虚高)、**解析不到的不算边**(单独计数)、
 * **嵌入(`![[..]]`)算**(它是更强的引用,不是别的东西)。
 *
 * 节点取自索引(全库笔记)而不是扫描结果,否则没有任何链接的笔记根本不会出现,
 * "有几篇孤立"就报不出来。但孤立笔记**不画进图里** —— 五百篇笔记二十条链接时,
 * 画出来是四百八十个互不相连的点,那不是结构,是噪声。只报数。
 */
export function buildNoteGraph(
  sources: readonly NoteLinkSource[],
  index: VaultLinkIndex,
  options: BuildGraphOptions = {},
): NoteGraph {
  /** 归一化路径 → 节点。顺序跟着索引(= 笔记列表顺序),保证同一份库两次算出同一张图。 */
  const byKey = new Map<string, GraphNode>();
  for (const note of index.byPath.values()) {
    const key = normalizeLinkTarget(note.path);
    if (!key || byKey.has(key)) continue;
    byKey.set(key, {
      path: note.path,
      title: note.title,
      inDegree: 0,
      outDegree: 0,
      depth: null,
    });
  }

  const folded = new Map<string, GraphEdge>();
  const adjacency: Adjacency = new Map();
  let deadLinks = 0;

  for (const source of sources) {
    const fromKey = normalizeLinkTarget(source.path);
    if (!fromKey) continue;
    const fromNode = byKey.get(fromKey);
    // 扫描结果里有、索引里没有:笔记刚被删掉而扫描是旧的。跳过而不是造一个节点 ——
    // 造出来的那个点点开是空的。
    if (!fromNode) continue;

    for (const ref of source.links) {
      const parts = parseWikiLinkBody(ref.raw);
      if (!parts) continue;
      const match = resolveLink(index, parts.target);
      if (!match) {
        deadLinks += 1;
        continue;
      }
      const toKey = normalizeLinkTarget(match.note.path);
      if (!toKey || toKey === fromKey) continue;
      const toNode = byKey.get(toKey);
      if (!toNode) continue;

      fromNode.outDegree += 1;
      toNode.inDegree += 1;
      link(adjacency, fromKey, toKey);

      /* 互指的两条合成一条双箭头。键用排序后的一对,所以 A→B 和 B→A 落在同一个
         桶里 —— 画两条的话它们在直线布局下完全重合,看着是一条,却把边数算成两条。 */
      /* 分隔符用 \0 的转义写法,不是字面的 NUL 字节:字面写进源码会让整个文件对 git
         来说变成二进制(diff 显示成 `Bin ... bytes`)、grep 直接跳过它,而在编辑器里
         完全看不出来。用 \0 而不是空格是因为路径里可以有空格,拼出来会撞。 */
      const pair = fromKey < toKey ? `${fromKey}\0${toKey}` : `${toKey}\0${fromKey}`;
      const existing = folded.get(pair);
      if (!existing) {
        folded.set(pair, { from: fromNode.path, to: toNode.path, count: 1, mutual: false });
        continue;
      }
      existing.count += 1;
      // 已有那条的方向和这条相反 ⇒ 互指。
      if (normalizeLinkTarget(existing.from) !== fromKey) existing.mutual = true;
    }
  }

  const connected = new Set(adjacency.keys());
  const orphans = [...byKey.keys()].filter((key) => !connected.has(key)).length;

  const focusKey = options.focusPath ? normalizeLinkTarget(options.focusPath) : "";
  /* 焦点得自己在图上才谈得上"到它几跳"。孤立的焦点走整库模式(全部 depth null),
     而不是画一个只有它自己的图 —— 那样其余部分的结构就看不见了。 */
  const depths = focusKey && connected.has(focusKey) ? bfsDepths(adjacency, focusKey) : null;

  const nodes: GraphNode[] = [];
  let hidden = 0;
  for (const [key, node] of byKey) {
    if (!connected.has(key)) continue;
    const depth = depths?.get(key) ?? null;
    if (
      depths &&
      (depth === null || (options.maxDepth !== undefined && depth > options.maxDepth))
    ) {
      hidden += 1;
      continue;
    }
    nodes.push({ ...node, depth });
  }

  const kept = new Set(nodes.map((node) => normalizeLinkTarget(node.path)));
  const edges = [...folded.values()].filter(
    (edge) => kept.has(normalizeLinkTarget(edge.from)) && kept.has(normalizeLinkTarget(edge.to)),
  );

  return { nodes, edges, deadLinks, orphans, hidden };
}

/** 一个节点画在哪。坐标以图心为原点。 */
export type GraphPlacement = {
  path: string;
  x: number;
  y: number;
  /** 圆点半径。按总度数放大 —— 只按入度会让"到处引用别人"的索引页缩成一个小点。 */
  r: number;
  /** 画不画标签。见 `layoutNoteGraph`。 */
  label: boolean;
};

export type LayoutOptions = {
  /** 相邻两环的间距。 */
  ringGap?: number;
  /** 一个标签大致占多宽。环上挤不到这么宽就不画标签。 */
  labelWidth?: number;
};

const DEFAULT_RING_GAP = 130;
const DEFAULT_LABEL_WIDTH = 54;

/** 半径:4~18,按总度数开方增长。 */
function radiusOf(node: GraphNode): number {
  return Math.max(4, Math.min(18, 4 + Math.sqrt(node.inDegree + node.outDegree) * 3));
}

/**
 * 算出每个节点的坐标。
 *
 * 按到焦点的跳数分同心环:焦点在圆心,一跳一环。`depth` 为 null 的(整库模式下的
 * 全部节点,或连不到焦点的)排在最外一环 —— 于是同一个函数同时覆盖"局部图"和
 * "整库图"两种模式,不用写两套。
 *
 * 环内顺序:总度数降序,同度按路径升序。**必须是全序** —— 重扫一次库如果顺序变了,
 * 整张图会重排,用户会以为结构变了。
 *
 * 标签按环的周长取舍:一环上每个节点分不到 `labelWidth` 宽时,只给这一环里度数
 * 最高的那几个画。圆心那个总是画。Markio 给每个节点都画,节点一多就糊成一片。
 */
export function layoutNoteGraph(
  nodes: readonly GraphNode[],
  options: LayoutOptions = {},
): { placements: GraphPlacement[]; extent: number } {
  const ringGap = options.ringGap ?? DEFAULT_RING_GAP;
  const labelWidth = options.labelWidth ?? DEFAULT_LABEL_WIDTH;

  const maxDepth = nodes.reduce((top, node) => Math.max(top, node.depth ?? 0), 0);
  /** null 排在最外面那一环,也就是 maxDepth + 1(有 null 时)。 */
  const outerRing = nodes.some((node) => node.depth === null) ? maxDepth + 1 : maxDepth;

  const rings = new Map<number, GraphNode[]>();
  for (const node of nodes) {
    const ring = node.depth ?? outerRing;
    const bucket = rings.get(ring) ?? [];
    bucket.push(node);
    rings.set(ring, bucket);
  }

  const placements: GraphPlacement[] = [];
  let extent = 0;

  for (const [ring, bucket] of [...rings.entries()].sort((a, b) => a[0] - b[0])) {
    bucket.sort((a, b) => {
      const byDegree = b.inDegree + b.outDegree - (a.inDegree + a.outDegree);
      return byDegree !== 0 ? byDegree : compareNotebookPath(a.path, b.path);
    });

    if (ring === 0) {
      // 圆心。一跳都还没走,这一环最多只有焦点自己。
      for (const node of bucket) {
        placements.push({ path: node.path, x: 0, y: 0, r: radiusOf(node), label: true });
      }
      extent = Math.max(extent, radiusOf(bucket[0]!));
      continue;
    }

    const radius = ring * ringGap;
    /* 挤不挤看周长:每个节点分到的弧长够不够放一个标签。不够就只给度数最高的那几个
       画 —— 能放几个就画几个。 */
    const fits = Math.max(1, Math.floor((2 * Math.PI * radius) / labelWidth));
    bucket.forEach((node, position) => {
      // 从正上方开始,顺时针铺开。
      const angle = (position / bucket.length) * Math.PI * 2 - Math.PI / 2;
      placements.push({
        path: node.path,
        x: radius * Math.cos(angle),
        y: radius * Math.sin(angle),
        r: radiusOf(node),
        label: position < fits,
      });
    });
    extent = Math.max(extent, radius + 24);
  }

  return { placements, extent };
}
