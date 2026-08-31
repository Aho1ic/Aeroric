/* 语义检索面板的画法与交互。
 *
 * 盯的是几件"画错了不报错、只是看起来正常"的事:进度阶段和取消按钮什么时候在场、
 * 降级提示会不会被静默吞掉(那是这个面板最重要的一条 —— 向量挂了的表现是"结果莫名
 * 变差")、以及「重试失败项」只在真有失败项时出现。
 */

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";

import { NoteAiSheet } from "../components/notebook/NoteAiSheet";
import type {
  RagContextBundle,
  RagHit,
  RagIndexProgress,
  RagIndexStats,
} from "../components/notebook/noteRag";
import { staticT } from "../i18n";

type Overrides = Partial<Parameters<typeof NoteAiSheet>[0]>;

const stats = (over: Partial<RagIndexStats> = {}): RagIndexStats => ({
  docs: 4,
  indexed: 4,
  pending: 0,
  failed: 0,
  stale: 0,
  chunks: 17,
  failures: [],
  ...over,
});

const progress = (over: Partial<RagIndexProgress> = {}): RagIndexProgress => ({
  vault: "/v",
  phase: "embedding",
  total: 10,
  done: 4,
  failed: 0,
  current: null,
  error: null,
  ...over,
});

const hit = (over: Partial<RagHit> = {}): RagHit => ({
  path: "/v/a.md",
  title: "甲",
  heading: "小节",
  body: "命中的那一段正文",
  score: 1,
  source: "vector+fts",
  charStart: 12,
  charEnd: 20,
  bodySpans: [],
  sourceSpans: [],
  ...over,
});

const bundle = (over: Partial<RagContextBundle> = {}): RagContextBundle => ({
  text: "## Current note: 甲\n\n正文",
  citations: [{ ...hit(), index: 1 }],
  tokens: 42,
  truncated: false,
  degraded: [],
  vectorsMissing: false,
  ...over,
});

function renderSheet(overrides: Overrides = {}) {
  const handlers = {
    onQueryChange: vi.fn(),
    onSearch: vi.fn(),
    onBuildContext: vi.fn(),
    onCopyContext: vi.fn(),
    onIndex: vi.fn(),
    onCancelIndex: vi.fn(),
    onClearIndex: vi.fn(),
    onOpenHit: vi.fn(),
    onClose: vi.fn(),
  };
  render(
    <NoteAiSheet
      stats={stats()}
      progress={null}
      query=""
      hits={[]}
      searched={false}
      searching={false}
      degraded={[]}
      vectorsMissing={false}
      context={null}
      contextBusy={false}
      copied={false}
      error={null}
      t={staticT}
      {...handlers}
      {...overrides}
    />,
  );
  return handlers;
}

describe("NoteAiSheet 的结构", () => {
  it("是个带名字的 dialog", () => {
    renderSheet();
    expect(screen.getByRole("dialog", { name: staticT("notebook.aiTitle") })).toBeTruthy();
  });

  it("Esc 关面板", () => {
    /* 只验关闭。`useNoteSheetDismiss` 里那句 `stopPropagation` 目前没有可观测效果
       (React 在根上做事件委派,挂在 DOM 父节点上的原生监听一律先跑),理由见
       `noteSheetChrome` 的注释 —— 那里已经写明它是为将来给面板加 Esc 留的。 */
    const { onClose } = renderSheet();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("其它按键不关面板", () => {
    const { onClose } = renderSheet();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Enter" });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("没搜过时给的是提示而不是「没有命中」", () => {
    // 「没有命中」会让刚打开的面板看起来像是搜空了。
    renderSheet();
    expect(screen.getByText(staticT("notebook.aiHint"))).toBeTruthy();
    expect(screen.queryByText(staticT("notebook.aiNoHits"))).toBeNull();
  });

  it("搜过且真的没结果时才说「没有命中」", () => {
    renderSheet({ searched: true });
    expect(screen.getByText(staticT("notebook.aiNoHits"))).toBeTruthy();
  });
});

describe("索引状态与进度", () => {
  it("空闲时画统计和建索引按钮,不画取消", () => {
    renderSheet();
    expect(screen.getByText(staticT("notebook.aiIndexRun"))).toBeTruthy();
    expect(screen.queryByText(staticT("notebook.aiCancel"))).toBeNull();
    expect(screen.queryByRole("progressbar")).toBeNull();
  });

  it("正在跑时画进度条与取消,不画建索引", () => {
    // 建索引按钮留在场会让用户点第二次,而后端会直接拒绝 —— 看起来像点了没反应。
    const { onCancelIndex } = renderSheet({ progress: progress() });
    const bar = screen.getByRole("progressbar", { name: staticT("notebook.aiIndexProgress") });
    expect(bar.getAttribute("aria-valuenow")).toBe("40");
    expect(screen.queryByText(staticT("notebook.aiIndexRun"))).toBeNull();
    fireEvent.click(screen.getByText(staticT("notebook.aiCancel")));
    expect(onCancelIndex).toHaveBeenCalledTimes(1);
  });

  it("扫描阶段 total 还是 0,进度条不画成满格", () => {
    renderSheet({ progress: progress({ phase: "scanning", total: 0, done: 0 }) });
    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("0");
    expect(screen.getByText(staticT("notebook.aiPhase.scanning"), { exact: false })).toBeTruthy();
  });

  it("没有失败项时不画「重试失败项」", () => {
    // 一个永远禁用的按钮只会让人猜它干什么。
    renderSheet();
    expect(screen.queryByText(staticT("notebook.aiIndexRetry"))).toBeNull();
  });

  it("有失败项时画出重试,并按 failedOnly 发起", () => {
    const { onIndex } = renderSheet({
      stats: stats({ failed: 2, failures: [{ path: "/v/b.md", error: "上游 500", attempts: 3 }] }),
    });
    fireEvent.click(screen.getByText(staticT("notebook.aiIndexRetry")));
    expect(onIndex).toHaveBeenCalledWith("failedOnly");
  });

  it("失败清单把原因一起列出来", () => {
    // 只说"失败 2 篇"的话,用户无从判断该改配置还是该改笔记。
    renderSheet({
      stats: stats({ failed: 1, failures: [{ path: "/v/b.md", error: "上游 500", attempts: 3 }] }),
    });
    expect(screen.getByText("b.md")).toBeTruthy();
    expect(screen.getByText("上游 500")).toBeTruthy();
  });

  it("建索引按全库发起", () => {
    const { onIndex } = renderSheet();
    fireEvent.click(screen.getByText(staticT("notebook.aiIndexRun")));
    expect(onIndex).toHaveBeenCalledWith("all");
  });
});

describe("降级必须显示", () => {
  it("向量那一路挂了要说出来", () => {
    // 静默降级的表现是「结果莫名变差」,用户无从判断是索引没建好还是模型没连上。
    renderSheet({ degraded: [{ stage: "vector", detail: "connection refused" }] });
    const line = screen.getByText(staticT("notebook.aiDegraded.vector"));
    expect(line).toBeTruthy();
    // 原始错误进 title —— 列表里放不下,而它是唯一能定位问题的信息。
    expect(line.getAttribute("title")).toBe("connection refused");
  });

  it("两路同时挂时两条都在", () => {
    // 挤在一个字段里会丢掉其中一条,而本地 Ollama 没开时它们通常一起挂。
    renderSheet({
      degraded: [
        { stage: "vector", detail: "refused" },
        { stage: "rerank", detail: "timeout" },
      ],
    });
    expect(screen.getByText(staticT("notebook.aiDegraded.vector"))).toBeTruthy();
    expect(screen.getByText(staticT("notebook.aiDegraded.rerank"))).toBeTruthy();
  });

  it("「还没建索引」和「模型挂了」是两条不同的提示", () => {
    // 前者该提示去建索引,后者该提示查配置。混成一条会把用户引到错的方向。
    renderSheet({ vectorsMissing: true });
    expect(screen.getByText(staticT("notebook.aiVectorsMissing"))).toBeTruthy();
    expect(screen.queryByText(staticT("notebook.aiDegraded.vector"))).toBeNull();
  });
});

describe("命中与上下文", () => {
  it("点命中交回给调用方", () => {
    const target = hit();
    const { onOpenHit } = renderSheet({ hits: [target], searched: true });
    fireEvent.click(screen.getAllByTestId("note-ai-hit")[0]!);
    expect(onOpenHit).toHaveBeenCalledWith(target);
  });

  it("命中正文按纯文本画,不当 Markdown", () => {
    // 命中块常常是表格或代码,渲染出来会把列表撑破。
    renderSheet({ hits: [hit({ body: "# 不该变成标题" })], searched: true });
    expect(screen.getByText("# 不该变成标题")).toBeTruthy();
    expect(screen.queryByRole("heading")).toBeNull();
  });

  it("高亮区间标成 mark", () => {
    renderSheet({
      hits: [hit({ body: "前面命中后面", bodySpans: [{ start: 2, end: 4 }] })],
      searched: true,
    });
    const marks = document.querySelectorAll("mark");
    expect(marks.length).toBe(1);
    expect(marks[0]!.textContent).toBe("命中");
  });

  it("查询为空时不能装配上下文", () => {
    // 空查询的检索没有意义,而按钮可点会让人以为是后端出了错。
    renderSheet({ query: "  " });
    const assemble = screen.getByText(staticT("notebook.aiAssemble")).closest("button");
    expect((assemble as HTMLButtonElement).disabled).toBe(true);
  });

  it("装配好之后显示 token 数与引用条数", () => {
    renderSheet({ query: "问题", context: bundle() });
    expect(
      screen.getByText(staticT("notebook.aiContextSummary", { tokens: "42", count: "1" })),
    ).toBeTruthy();
    expect(screen.getByTestId("note-ai-context").textContent).toContain("Current note");
  });

  it("被预算裁过要说出来", () => {
    // 不说的话用户会以为上下文是完整的,而模型答不上来时无从判断原因。
    renderSheet({ query: "问题", context: bundle({ truncated: true }) });
    expect(screen.getByText(`· ${staticT("notebook.aiContextTruncated")}`)).toBeTruthy();
  });

  it("复制之后按钮改口说已复制", () => {
    renderSheet({ query: "问题", context: bundle(), copied: true });
    expect(screen.getByText(staticT("notebook.aiContextCopied"))).toBeTruthy();
  });

  it("提交查询框走搜索", () => {
    const { onSearch } = renderSheet({ query: "问题" });
    fireEvent.click(screen.getByText(staticT("notebook.aiSearch")));
    expect(onSearch).toHaveBeenCalledTimes(1);
  });
});
