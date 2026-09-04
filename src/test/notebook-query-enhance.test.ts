/* `enhanceNoteQueries.ts` 的用例:占位怎么认领、取数怎么共享、DOM 怎么摆。
 *
 * 纯逻辑(解析 / 筛选 / 排序)在 `notebook-query.test.ts`,这里只验这一层的 DOM 行为与
 * 取消语义。
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  enhanceNoteQueries,
  QUERY_BLOCK_CLASS,
  QUERY_ERROR_CLASS,
  QUERY_HEAD_CLASS,
  type QueryLabels,
} from "../components/notebook/enhanceNoteQueries";
import type { NoteFieldSource } from "../components/notebook/noteFields";

const LABELS: QueryLabels = {
  head: ({ key, value, shown, total }) =>
    // 截断了就把两个数都说出来 —— 用例靠这段文案验"报数是否如实"。
    shown === total
      ? `${key}${value ? `=${value}` : ""} · ${total} 条`
      : `${key}${value ? `=${value}` : ""} · 显示 ${shown} / 共 ${total} 条`,
  empty: () => "没有匹配的笔记",
  noteColumn: () => "笔记",
  open: (title) => `打开 ${title}`,
  failed: (message) => `查询失败:${message}`,
  problem: (problem) => {
    switch (problem.code) {
      case "missingKey":
        return "缺少 key";
      case "unknownDirective":
        return `不认识的指令:${problem.name}`;
      case "badSort":
        return `sort 只能是 name 或 value,收到:${problem.value}`;
      case "badLimit":
        return `limit 要是正整数,收到:${problem.value}`;
    }
  },
};

const SOURCES: NoteFieldSource[] = [
  { path: "/v/b.md", fields: [{ key: "status", values: ["active"] }] },
  { path: "/v/a.md", fields: [{ key: "status", values: ["active"] }] },
  { path: "/v/c.md", fields: [{ key: "status", values: ["done"] }] },
];

const TITLES: Record<string, string> = { "/v/a.md": "甲", "/v/b.md": "乙", "/v/c.md": "丙" };

/** 造一个预览容器,里面放一个查询围栏(渲染器产出的形状)。 */
function host(source: string, language = "notebook-query"): HTMLElement {
  const root = document.createElement("div");
  const pre = document.createElement("pre");
  pre.dataset.language = language;
  const code = document.createElement("code");
  code.textContent = source;
  pre.append(code);
  root.append(pre);
  document.body.append(root);
  return root;
}

function run(root: HTMLElement, overrides: Partial<Parameters<typeof enhanceNoteQueries>[1]> = {}) {
  const scan = vi.fn(async () => SOURCES);
  const handle = enhanceNoteQueries(root, {
    vault: "/v",
    scan,
    titleOf: (path) => TITLES[path] ?? path,
    labels: LABELS,
    ...overrides,
  });
  return { handle, scan };
}

/** 等一轮微任务,让 await 链走完。 */
const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

describe("enhanceNoteQueries", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("把围栏换成结果表,按标题排", async () => {
    const root = host("key: status\nvalue: active");
    run(root);
    await flush();
    expect(root.querySelector("pre")).toBeNull();
    const rows = Array.from(root.querySelectorAll("tbody tr td:first-child"));
    expect(rows.map((td) => td.textContent)).toEqual(["甲", "乙"]);
    expect(root.querySelector(`.${QUERY_HEAD_CLASS}`)?.textContent).toBe("status=active · 2 条");
  });

  it("笔记那一列是 wikilink,带 data-wiki-path —— 面板现成的点击监听就能跳", async () => {
    const root = host("key: status\nvalue: active");
    run(root);
    await flush();
    const link = root.querySelector<HTMLAnchorElement>("tbody a");
    expect(link?.className).toBe("notebook-wikilink");
    expect(link?.dataset.wikiPath).toBe("/v/a.md");
    expect(link?.getAttribute("role")).toBe("link");
    expect(link?.title).toBe("打开 甲");
  });

  it("值那一列显示字段值,行序跟着标题排", async () => {
    const root = host("key: status");
    run(root);
    await flush();
    const rows = Array.from(root.querySelectorAll("tbody tr")).map((tr) =>
      Array.from(tr.querySelectorAll("td")).map((td) => td.textContent),
    );
    // 默认按标题排,固定的中文拼音比较器下 丙 < 甲 < 乙(bing < jia < yi)。
    expect(rows).toEqual([
      ["丙", "done"],
      ["甲", "active"],
      ["乙", "active"],
    ]);
  });

  it("一条都没匹配上时显示空提示,而不是一张空表", async () => {
    const root = host("key: nope");
    run(root);
    await flush();
    expect(root.querySelector("table")).toBeNull();
    expect(root.textContent).toContain("没有匹配的笔记");
  });

  // 这条是 Markio 报数不实的地方:它表头写的是截断后的条数。
  it("limit 截断时表头把总数也说出来", async () => {
    const root = host("key: status\nlimit: 1");
    run(root);
    await flush();
    expect(root.querySelector(`.${QUERY_HEAD_CLASS}`)?.textContent).toBe(
      "status · 显示 1 / 共 3 条",
    );
    expect(root.querySelectorAll("tbody tr")).toHaveLength(1);
  });

  it("语法错误一次列全,而且不取数", async () => {
    const root = host("sort: x\nnope: 1");
    const { scan } = run(root);
    await flush();
    expect(scan).not.toHaveBeenCalled();
    const lines = Array.from(root.querySelectorAll(`.${QUERY_ERROR_CLASS}`)).map(
      (el) => el.textContent,
    );
    expect(lines).toEqual(["sort 只能是 name 或 value,收到:x", "不认识的指令:nope", "缺少 key"]);
  });

  it("没有 vault 时保持原样,下一轮还能再试", async () => {
    const root = host("key: status");
    const { scan } = run(root, { vault: undefined });
    await flush();
    expect(scan).not.toHaveBeenCalled();
    // 还是那个 `<pre>`:用户看到查询源码,而不是一个空块。
    expect(root.querySelector("pre")).not.toBeNull();
    // vault 到位之后再跑一轮就该出表。
    run(root);
    await flush();
    expect(root.querySelector("pre")).toBeNull();
    expect(root.querySelectorAll("tbody tr")).toHaveLength(3);
  });

  it("取数失败时显示失败提示", async () => {
    const root = host("key: status");
    run(root, {
      scan: async () => {
        throw new Error("boom");
      },
    });
    await flush();
    expect(root.textContent).toContain("查询失败:boom");
  });

  it("一篇里多个查询块只扫一次全库", async () => {
    const root = host("key: status");
    const second = document.createElement("pre");
    second.dataset.language = "notebook-query";
    const code = document.createElement("code");
    code.textContent = "key: status\nvalue: done";
    second.append(code);
    root.append(second);

    const { scan } = run(root);
    await flush();
    expect(scan).toHaveBeenCalledTimes(1);
    expect(root.querySelectorAll("table")).toHaveLength(2);
  });

  it("重跑时已渲染的块会按新数据重渲染", async () => {
    // 库里别的笔记改了 frontmatter 之后结果就该变,而宿主这篇的 HTML 一个字都没变。
    const root = host("key: status");
    run(root);
    await flush();
    expect(root.querySelectorAll("tbody tr")).toHaveLength(3);

    run(root, { scan: async () => SOURCES.slice(0, 1) });
    await flush();
    expect(root.querySelectorAll("tbody tr")).toHaveLength(1);
    // 源码一直留在容器上,否则重渲染无从下手。
    expect(root.querySelector<HTMLElement>(`.${QUERY_BLOCK_CLASS}`)?.dataset.querySource).toBe(
      "key: status",
    );
  });

  it("disconnect 之后不再改 DOM", async () => {
    const root = host("key: status");
    let release: ((value: NoteFieldSource[]) => void) | undefined;
    const { handle } = run(root, {
      scan: () =>
        new Promise<NoteFieldSource[]>((resolve) => {
          release = resolve;
        }),
    });
    handle.disconnect();
    release?.(SOURCES);
    await flush();
    // 还是原来那个 `<pre>`:这一轮的结果被丢掉了,下一轮会重新认领。
    expect(root.querySelector("pre")).not.toBeNull();
    expect(root.querySelector("table")).toBeNull();
  });

  it("不是查询语言的围栏一概不动", async () => {
    const root = host("key: status", "ts");
    const { scan } = run(root);
    await flush();
    expect(scan).not.toHaveBeenCalled();
    expect(root.querySelector("pre")?.dataset.language).toBe("ts");
  });

  it("没有查询块时不取数", async () => {
    const root = document.createElement("div");
    document.body.append(root);
    const { scan } = run(root);
    await flush();
    expect(scan).not.toHaveBeenCalled();
  });
});
