/* 引用图谱。圆心是当前这篇笔记,一跳一环往外铺;点一个点跳过去。
 *
 * 画在 SVG 而不是 cytoscape。原计划写的是引入 cytoscape,改掉的理由:
 *
 * - **Markio 的笔记图谱本来就是手写 SVG**(`GraphView.tsx`)。它那份 cytoscape 只
 *   用在设置页的 RAG mini 图上(`RagGraphMini.tsx`),那属于 P6 的知识库,不是这里
 *   要对齐的能力。
 * - **cytoscape 画在 canvas 上,jsdom 里测不到任何东西。** 布局、标签取舍、点开哪
 *   一篇,全都只能靠"它没抛异常"来断言。SVG 是 DOM,这些都能真的断言。
 * - 442KB 的依赖,而这里要的是"看清结构 + 点得动",力导向那套交互不是必需。
 *
 * 布局和折图在 `noteGraph.ts`(纯函数,可测)。这里只管画和交互。
 *
 * 铺在面板内部(`position:absolute; inset:0`)而不是整个窗口,和历史 / 回收站 /
 * 字段浏览器一致:随手记面板可以只占项目视图的一半,盖住整个窗口会把用户正在参照
 * 的另一半也遮掉。
 */

import { useMemo, useState, type CSSProperties } from "react";
import { Share2, X } from "lucide-react";

import { layoutNoteGraph, type NoteGraph } from "./noteGraph";
import {
  noteSheetHeaderStyle,
  noteSheetIconButtonStyle,
  noteSheetOverlayStyle,
  useNoteSheetDismiss,
} from "./noteSheetChrome";

export type NoteGraphSheetProps = {
  graph: NoteGraph;
  /** 当前这篇的绝对路径。它是圆心,画成另一种颜色。 */
  focusPath: string | null;
  loading: boolean;
  error: string | null;
  /** 几跳以内。由面板持有 —— 换深度要重折图。 */
  depth: number;
  onDepthChange: (depth: number) => void;
  onOpenNote: (path: string) => void;
  onRefresh: () => void;
  onClose: () => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

/** 可选的跳数。0 不给 —— 那是"只看自己",没有信息量。 */
export const DEPTH_CHOICES = [1, 2, 3] as const;
/** 「不限」用一个大到不可能达到的值,而不是 undefined:选择器的值域统一成数字。 */
export const DEPTH_ALL = 99;

const headerStyle = noteSheetHeaderStyle(6);

const metaStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 6,
  padding: "4px 10px",
  color: "var(--text-hint)",
  fontSize: 11,
  borderBottom: "1px solid var(--border-dim)",
  flexWrap: "wrap",
};

const boardStyle: CSSProperties = {
  flex: 1,
  minHeight: 0,
  overflow: "auto",
  padding: 10,
};

const hintStyle: CSSProperties = {
  padding: 10,
  color: "var(--text-hint)",
  fontSize: 11.5,
};

const selectStyle: CSSProperties = {
  height: 20,
  border: "1px solid var(--border)",
  borderRadius: 4,
  background: "var(--bg-input, transparent)",
  color: "var(--text-primary)",
  fontSize: 11,
};

export function NoteGraphSheet({
  graph,
  focusPath,
  loading,
  error,
  depth,
  onDepthChange,
  onOpenNote,
  onRefresh,
  onClose,
  t,
}: NoteGraphSheetProps) {
  const { closeRef, overlayProps } = useNoteSheetDismiss(t("notebook.graphTitle"), onClose);
  const [hover, setHover] = useState<string | null>(null);

  const { placements, extent } = useMemo(() => layoutNoteGraph(graph.nodes), [graph.nodes]);

  const byPath = useMemo(() => {
    const map = new Map<string, (typeof placements)[number]>();
    for (const placement of placements) map.set(placement.path, placement);
    return map;
  }, [placements]);

  const titleOf = useMemo(() => {
    const map = new Map<string, string>();
    for (const node of graph.nodes) map.set(node.path, node.title);
    return map;
  }, [graph.nodes]);

  /** 悬停时高亮它和它的直接邻居,其余压暗。 */
  const lit = useMemo(() => {
    if (!hover) return null;
    const set = new Set<string>([hover]);
    for (const edge of graph.edges) {
      if (edge.from === hover) set.add(edge.to);
      if (edge.to === hover) set.add(edge.from);
    }
    return set;
  }, [hover, graph.edges]);

  const view = extent * 2;

  return (
    <div style={noteSheetOverlayStyle} {...overlayProps}>
      <div style={headerStyle}>
        <Share2 size={13} aria-hidden />
        <span>{t("notebook.graphTitle")}</span>
        <label style={{ marginLeft: 12, display: "flex", alignItems: "center", gap: 4 }}>
          {t("notebook.graphDepth")}
          <select
            value={depth}
            onChange={(event) => onDepthChange(Number(event.target.value))}
            aria-label={t("notebook.graphDepth")}
            style={selectStyle}
          >
            {DEPTH_CHOICES.map((choice) => (
              <option key={choice} value={choice}>
                {t("notebook.graphDepthHops", { count: String(choice) })}
              </option>
            ))}
            <option value={DEPTH_ALL}>{t("notebook.graphDepthAll")}</option>
          </select>
        </label>
        <button
          type="button"
          onClick={onRefresh}
          style={{ ...noteSheetIconButtonStyle, marginLeft: "auto", padding: "2px 6px" }}
        >
          {t("notebook.graphRefresh")}
        </button>
        <button
          ref={closeRef}
          type="button"
          aria-label={t("notebook.graphClose")}
          onClick={onClose}
          style={noteSheetIconButtonStyle}
        >
          <X size={13} aria-hidden />
        </button>
      </div>

      <div style={metaStyle}>
        <span>{t("notebook.graphNotes", { count: String(graph.nodes.length) })}</span>
        <span aria-hidden>·</span>
        <span>{t("notebook.graphLinks", { count: String(graph.edges.length) })}</span>
        {graph.orphans > 0 && (
          <>
            <span aria-hidden>·</span>
            <span>{t("notebook.graphOrphans", { count: String(graph.orphans) })}</span>
          </>
        )}
        {graph.hidden > 0 && (
          <>
            <span aria-hidden>·</span>
            <span>{t("notebook.graphHidden", { count: String(graph.hidden) })}</span>
          </>
        )}
        {graph.deadLinks > 0 && (
          <>
            <span aria-hidden>·</span>
            {/* 死链是图谱最该暴露的东西之一,不藏进折叠里。 */}
            <span>{t("notebook.graphDeadLinks", { count: String(graph.deadLinks) })}</span>
          </>
        )}
      </div>

      {/* 扫描失败用 warning 而不是 danger,和历史 / 回收站 / 字段浏览器一致:
          图上少几条边是降级,不是错误。 */}
      {error && <div style={{ ...hintStyle, color: "var(--warning)" }}>{error}</div>}

      <div style={boardStyle}>
        {loading && graph.nodes.length === 0 ? (
          <div style={hintStyle}>{t("notebook.graphLoading")}</div>
        ) : graph.nodes.length === 0 ? (
          /* 没有边的库不是故障:先写笔记后建链接是常态。但**扫不动**的时候不能报这
             一句 —— 那会让用户以为库里真的没有链接,而实际上是这一次没读到。上面
             那条错误已经把情况说清了,这里让位。 */
          error ? null : (
            <div style={hintStyle}>{t("notebook.graphEmpty")}</div>
          )
        ) : (
          <svg
            viewBox={`${-extent} ${-extent} ${view} ${view}`}
            preserveAspectRatio="xMidYMid meet"
            width="100%"
            height="100%"
            role="group"
            aria-label={t("notebook.graphTitle")}
            style={{ display: "block", minHeight: 240 }}
            // 头部那几个 lucide 图标也是 <svg>,而且排在前面。给画布一个标记,测试和
            // 将来的样式才认得出哪一个是图。
            data-graph-canvas
          >
            <defs>
              {/* 箭头。图上要看得出"谁指向谁" —— 只画线的话双链和单链看着一样。 */}
              <marker
                id="notebook-graph-arrow"
                viewBox="0 0 8 8"
                refX="7"
                refY="4"
                markerWidth="5"
                markerHeight="5"
                orient="auto-start-reverse"
              >
                <path d="M 0 1 L 7 4 L 0 7 z" fill="var(--border-strong)" />
              </marker>
            </defs>

            {graph.edges.map((edge) => {
              const from = byPath.get(edge.from);
              const to = byPath.get(edge.to);
              if (!from || !to) return null;
              const dim = lit !== null && !(lit.has(edge.from) && lit.has(edge.to));
              return (
                <line
                  key={`${edge.from}|${edge.to}`}
                  x1={from.x}
                  y1={from.y}
                  x2={to.x}
                  y2={to.y}
                  stroke={dim ? "var(--border-dim)" : "var(--border-strong)"}
                  strokeOpacity={dim ? 0.3 : 0.75}
                  // 粗细跟着折叠掉的链接条数走,但压住上限:一条 20 次的边不该是根柱子。
                  strokeWidth={Math.min(3, 0.8 + (edge.count - 1) * 0.4)}
                  markerEnd="url(#notebook-graph-arrow)"
                  markerStart={edge.mutual ? "url(#notebook-graph-arrow)" : undefined}
                  data-graph-edge={`${edge.from}|${edge.to}`}
                  data-graph-mutual={edge.mutual ? "true" : undefined}
                />
              );
            })}

            {placements.map((placement) => {
              const title = titleOf.get(placement.path) ?? placement.path;
              const isFocus = placement.path === focusPath;
              const dim = lit !== null && !lit.has(placement.path);
              return (
                <g
                  key={placement.path}
                  transform={`translate(${placement.x}, ${placement.y})`}
                  /* 每个点都是按钮:图谱的主要用途就是"看到那篇然后跳过去",而这些
                     点在 Markio 里只能用鼠标点 —— 键盘用户看得见图却进不去。 */
                  role="button"
                  tabIndex={0}
                  aria-label={t("notebook.graphOpenNote", { title })}
                  onClick={() => onOpenNote(placement.path)}
                  onKeyDown={(event) => {
                    if (event.key !== "Enter" && event.key !== " ") return;
                    // 空格默认会滚动那块画布。
                    event.preventDefault();
                    onOpenNote(placement.path);
                  }}
                  onMouseEnter={() => setHover(placement.path)}
                  onMouseLeave={() => setHover(null)}
                  onFocus={() => setHover(placement.path)}
                  onBlur={() => setHover(null)}
                  style={{ cursor: "pointer" }}
                  data-graph-node={placement.path}
                  data-graph-focus={isFocus ? "true" : undefined}
                >
                  <circle
                    r={placement.r}
                    fill={isFocus ? "var(--accent)" : "var(--control-active-bg)"}
                    fillOpacity={dim ? 0.35 : 1}
                    stroke={isFocus ? "var(--accent)" : "var(--border-strong)"}
                    strokeWidth={1}
                  />
                  {placement.label && (
                    <text
                      x={0}
                      y={placement.r + 10}
                      textAnchor="middle"
                      fontSize={9}
                      fill={dim ? "var(--text-hint)" : "var(--text-secondary)"}
                      // 标签不吃鼠标事件,否则相邻标签会盖住旁边那个点。
                      style={{ pointerEvents: "none" }}
                    >
                      {title}
                    </text>
                  )}
                </g>
              );
            })}
          </svg>
        )}
      </div>
    </div>
  );
}
