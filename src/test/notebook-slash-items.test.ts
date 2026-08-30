import { describe, expect, it } from "vitest";

import { en } from "../i18n/en";
import { zh } from "../i18n/zh";
import { resolveSlashInsert, SLASH_ITEMS } from "../components/notebook/noteSlashItems";

const byId = (id: string) => {
  const item = SLASH_ITEMS.find((entry) => entry.id === id);
  if (!item) throw new Error(`没有 id 为 ${id} 的插入项`);
  return item;
};

describe("SLASH_ITEMS", () => {
  it("id 唯一", () => {
    const ids = SLASH_ITEMS.map((item) => item.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("每条的 i18n key 在 en 和 zh 里都有", () => {
    for (const item of SLASH_ITEMS) {
      expect(en[item.labelKey], `en 缺 ${item.labelKey}`).toBeTruthy();
      expect(zh[item.labelKey], `zh 缺 ${item.labelKey}`).toBeTruthy();
      expect(en[item.hintKey], `en 缺 ${item.hintKey}`).toBeTruthy();
      expect(zh[item.hintKey], `zh 缺 ${item.hintKey}`).toBeTruthy();
    }
  });

  it("cursorOffset 落在插入文本范围内", () => {
    for (const item of SLASH_ITEMS) {
      if (item.cursorOffset === undefined) continue;
      expect(item.cursorOffset, item.id).toBeGreaterThanOrEqual(0);
      expect(item.cursorOffset, item.id).toBeLessThanOrEqual(item.text.length);
    }
  });

  it("不含 Aeroric 渲染不出来的块", () => {
    /* Markio 的 slash 菜单里有这些,Aeroric 的预览没有对应渲染器 —— 插进去只是一段
       谁都不认的围栏文本。 */
    const unsupported = ["callout", "chart", "graphviz", "plantuml", "server"];
    for (const id of unsupported) {
      expect(SLASH_ITEMS.some((item) => item.id.startsWith(id))).toBe(false);
    }
  });
});

describe("resolveSlashInsert", () => {
  it("原样给出插入文本,不补前置换行", () => {
    /* 不补换行是刻意的:`detectTrigger` 只在行首或列表 / 引用标记之后返回 slash,
       插入点前面不会有正文 —— 补了反而把 `- /quote` 写成 `- \n> `。 */
    expect(resolveSlashInsert(byId("h1"))).toEqual({ text: "# ", cursor: 2 });
  });

  it("代码块光标落在围栏中间", () => {
    const { text, cursor } = resolveSlashInsert(byId("code"));
    expect(text).toBe("```\n\n```\n");
    // 插完就能直接贴代码,不用再手动上移一行。
    expect(text.slice(0, cursor)).toBe("```\n");
  });

  it("双链光标落在方括号中间 —— 顺带把 [[ 补全带起来", () => {
    const { text, cursor } = resolveSlashInsert(byId("wiki"));
    expect(text.slice(0, cursor)).toBe("[[");
    expect(text.slice(cursor)).toBe("]]");
  });

  it("嵌入的光标也在方括号里,而不是在 ! 后面", () => {
    const { text, cursor } = resolveSlashInsert(byId("embed"));
    expect(text.slice(0, cursor)).toBe("![[");
  });

  it("数学块光标落在 $$ 中间", () => {
    const { text, cursor } = resolveSlashInsert(byId("math"));
    expect(text.slice(0, cursor)).toBe("$$\n");
  });

  it("链接光标落在方括号里", () => {
    const { text, cursor } = resolveSlashInsert(byId("link"));
    expect(text).toBe("[]()");
    expect(cursor).toBe(1);
  });

  it("没给 cursorOffset 的落在末尾", () => {
    const { text, cursor } = resolveSlashInsert(byId("table"));
    expect(cursor).toBe(text.length);
  });
});
