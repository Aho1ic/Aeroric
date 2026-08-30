/* 预览里的 ```notebook-query 围栏 → 一张实时表格。
 *
 * 分工和嵌入(`noteEmbed.ts`)一样:解析与筛选是纯函数(`noteQuery.ts`),这一层只管
 * 认领占位、取数、把结果摆进 DOM。文案由调用方注入,这个模块不 import i18n。
 *
 * 不改渲染器:未知语言的围栏本来就渲染成 `<pre data-language="notebook-query">`,
 * 而 `data-language` 已经在 DOMPurify 白名单里。好处是增强没跑(或跑失败)时用户看到的
 * 是查询源码本身 —— 嵌入和悬浮预览这些只读语境里就是这样,那份"没变活"的观感恰好说明
 * 它现在是只读的。
 *
 * 表格用 `createElement` 一个个建,不拼 HTML 字符串。Markio 那份是 `innerHTML` 拼串 +
 * 手写 `escapeHtml`(而且漏了单引号)—— 这里的数据全部来自用户自己的笔记内容,拼串就是
 * 把"转义有没有漏"变成一个必须永远答对的问题,而建节点根本不问这个问题。
 */

import {
  MAX_QUERY_BLOCKS,
  parseNoteQuery,
  runNoteQuery,
  type QueryProblem,
  type QueryResult,
} from "./noteQuery";
import type { NoteFieldSource } from "./noteFields";

/** 围栏语言。 */
export const QUERY_LANGUAGE = "notebook-query";

/** 容器与各部件的类名。 */
export const QUERY_BLOCK_CLASS = "notebook-query";
export const QUERY_HEAD_CLASS = "notebook-query-head";
export const QUERY_BODY_CLASS = "notebook-query-body";
export const QUERY_EMPTY_CLASS = "notebook-query-empty";
export const QUERY_ERROR_CLASS = "notebook-query-error";

export type QueryLabels = {
  /** 表头:字段名 + 可选的值 + 条数。`shown` 与 `total` 不同时要把两个数都说出来。 */
  head: (info: { key: string; value?: string; shown: number; total: number }) => string;
  /** 一条都没匹配上。 */
  empty: () => string;
  /** 笔记那一列的表头。 */
  noteColumn: () => string;
  /** 打开某篇笔记(链接的 title)。 */
  open: (title: string) => string;
  /** 取数失败。 */
  failed: (message: string) => string;
  /** 语法错误,逐条。 */
  problem: (problem: QueryProblem) => string;
};

export type QueryFillOptions = {
  /** 当前 vault。没有 vault 时不取数(查询是全库范围的)。 */
  vault: string | null | undefined;
  /** 扫全库 frontmatter 字段。注入而不是直接 import,便于测试不过 IPC。 */
  scan: (vault: string) => Promise<NoteFieldSource[]>;
  /** 路径 → 标题。调用方手里有链接索引(那份合并过内存标题和扫盘标题)。 */
  titleOf: (path: string) => string;
  labels: QueryLabels;
};

export type QueryFillHandle = {
  disconnect(): void;
};

/** 建一个只有一行提示文字的块(空结果、语法错误、取数失败共用)。 */
function noticeBlock(doc: Document, className: string, lines: string[]): HTMLElement {
  const box = doc.createElement("div");
  box.className = QUERY_BLOCK_CLASS;
  for (const line of lines) {
    const row = doc.createElement("div");
    row.className = className;
    row.textContent = line;
    box.append(row);
  }
  return box;
}

/** 建结果表。 */
function tableBlock(
  doc: Document,
  key: string,
  value: string | undefined,
  result: QueryResult,
  labels: QueryLabels,
  openPath: (path: string, title: string) => HTMLElement,
): HTMLElement {
  const box = doc.createElement("div");
  box.className = QUERY_BLOCK_CLASS;

  const head = doc.createElement("div");
  head.className = QUERY_HEAD_CLASS;
  // 条数由 labels 决定怎么说 —— 截断了就得把总数也说出来,这是和 Markio 的分界线之一。
  head.textContent = labels.head({
    key,
    value,
    shown: result.rows.length,
    total: result.total,
  });
  box.append(head);

  if (result.rows.length === 0) {
    const empty = doc.createElement("div");
    empty.className = QUERY_EMPTY_CLASS;
    empty.textContent = labels.empty();
    box.append(empty);
    return box;
  }

  const table = doc.createElement("table");
  table.className = QUERY_BODY_CLASS;
  const thead = doc.createElement("thead");
  const headRow = doc.createElement("tr");
  for (const text of [labels.noteColumn(), key]) {
    const th = doc.createElement("th");
    th.textContent = text;
    headRow.append(th);
  }
  thead.append(headRow);
  table.append(thead);

  const tbody = doc.createElement("tbody");
  for (const row of result.rows) {
    const tr = doc.createElement("tr");
    const nameCell = doc.createElement("td");
    nameCell.append(openPath(row.path, row.title));
    const valueCell = doc.createElement("td");
    valueCell.textContent = row.value;
    tr.append(nameCell, valueCell);
    tbody.append(tr);
  }
  table.append(tbody);
  box.append(table);
  return box;
}

type CancelSignal = { cancelled: boolean };

/**
 * 渲染 `root` 下的查询块。
 *
 * 认领的是两种节点:还没处理过的 `<pre data-language="notebook-query">`,以及上一轮渲染出的
 * 容器(源码存在 `data-query-source` 上)。后者要重渲染是因为库里别的笔记改了 frontmatter
 * 之后结果就该变,而宿主这篇的 HTML 一个字都没变 —— 只按 `<pre>` 找的话表格会一直停在旧
 * 结果上,且看不出它是旧的。
 */
export function enhanceNoteQueries(root: HTMLElement, options: QueryFillOptions): QueryFillHandle {
  const doc = root.ownerDocument;
  const signal: CancelSignal = { cancelled: false };
  const { labels } = options;

  const fresh = Array.from(
    root.querySelectorAll<HTMLElement>(`pre[data-language="${QUERY_LANGUAGE}"]`),
  );
  const rendered = Array.from(
    root.querySelectorAll<HTMLElement>(`.${QUERY_BLOCK_CLASS}[data-query-source]`),
  );
  // 上限按"这篇里的查询块个数"算,而不是按渲染出的行数 —— 行数已经在 noteQuery 里夹过。
  const targets = [...fresh, ...rendered].slice(0, MAX_QUERY_BLOCKS);
  if (targets.length === 0) return { disconnect: () => undefined };

  /* 一轮里只扫一次:一篇笔记放三个查询块是正常写法,每块各扫一遍全库纯属浪费。
     存 Promise 而不是结果,同一轮里后面的块在前面那次 await 期间就能命中。 */
  let scanning: Promise<NoteFieldSource[]> | null = null;
  const loadFields = (vault: string) => {
    if (!scanning) scanning = options.scan(vault);
    return scanning;
  };

  const openPath = (path: string, title: string): HTMLElement => {
    /* 走 wikilink 的那套约定(类名 + `data-wiki-path`),于是面板现成的点击监听不用改
       一行就能跳过去。和嵌入头部同一个做法。 */
    const link = doc.createElement("a");
    link.className = "notebook-wikilink";
    link.setAttribute("role", "link");
    link.tabIndex = 0;
    link.dataset.wikiTarget = path;
    link.dataset.wikiPath = path;
    link.textContent = title;
    link.title = labels.open(title);
    return link;
  };

  for (const el of targets) {
    // 源码:`<pre>` 取文本,已渲染的容器取存下来的那份。
    const src = el.dataset.querySource ?? el.textContent ?? "";
    const parsed = parseNoteQuery(src);

    const swap = (next: HTMLElement) => {
      // 源码留在容器上,下一轮才能重渲染(见函数注释)。
      next.dataset.querySource = src;
      el.replaceWith(next);
    };

    if (!parsed.ok) {
      // 语法错误不用取数,立刻就能说清 —— 而且要一次说完全部问题。
      swap(noticeBlock(doc, QUERY_ERROR_CLASS, parsed.problems.map(labels.problem)));
      continue;
    }

    const { query } = parsed;
    const vault = options.vault;
    if (!vault) {
      // 没有 vault 就没有"全库"可查。保持原样(用户看到的是查询源码),下一轮再试。
      continue;
    }

    void (async () => {
      try {
        const sources = await loadFields(vault);
        // 取消了、或者这个节点已经被别的轮次换掉了,就别再动 DOM。
        if (signal.cancelled || !el.isConnected) return;
        const result = runNoteQuery(sources, query, options.titleOf);
        swap(tableBlock(doc, query.key, query.value, result, labels, openPath));
      } catch (error) {
        if (signal.cancelled || !el.isConnected) return;
        swap(noticeBlock(doc, QUERY_ERROR_CLASS, [labels.failed((error as Error).message)]));
      }
    })();
  }

  return {
    disconnect: () => {
      signal.cancelled = true;
    },
  };
}
