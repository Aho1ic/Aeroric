/* 阅读态任务复选框:渲染出来的行号属性 + DOM 解禁 + 从点击里认出目标。
 *
 * 分两段:`renderNoteMarkdown` 那段钉住"哪些 `<li>` 该带行号、带的是哪一行";
 * `enhanceTaskCheckboxes` 那段钉住"哪些复选框该被解禁"。中间的契约是 `data-task-line`。
 */

import { describe, expect, it } from "vitest";
import {
  enhanceTaskCheckboxes,
  taskToggleFromEvent,
  TASK_CHECKBOX_CLASS,
} from "../components/notebook/enhanceTaskCheckboxes";
import { renderNoteMarkdown } from "../components/notebook/noteRender";

/** 把 markdown 渲染进一个真节点。`taskLines` 默认开 —— 关掉的那几条会显式传 false。 */
function mount(source: string, taskLines = true): HTMLElement {
  const host = document.createElement("div");
  host.innerHTML = renderNoteMarkdown(source, { taskLines }).html;
  return host;
}

function items(host: HTMLElement): HTMLElement[] {
  return Array.from(host.querySelectorAll<HTMLElement>("li.notebook-task-item"));
}

function boxes(host: HTMLElement): HTMLInputElement[] {
  return Array.from(host.querySelectorAll<HTMLInputElement>('input[type="checkbox"]'));
}

const labels = { toggle: (text: string) => `勾选任务:${text}` };

describe("renderNoteMarkdown 的任务行号", () => {
  it("开着的时候每个任务项都带行号与勾选快照", () => {
    const host = mount("- [ ] 一\n- [x] 二");
    expect(items(host).map((li) => li.getAttribute("data-task-line"))).toEqual(["1", "2"]);
    expect(items(host).map((li) => li.getAttribute("data-task-checked"))).toEqual(["0", "1"]);
  });

  it("关着的时候只有类名,没有行号 —— 嵌入与悬浮预览走这条路", () => {
    const host = mount("- [ ] 一\n- [x] 二", false);
    expect(items(host)).toHaveLength(2);
    for (const li of items(host)) {
      expect(li.hasAttribute("data-task-line")).toBe(false);
      expect(li.hasAttribute("data-task-checked")).toBe(false);
    }
  });

  it("默认关:不传 options 就不带行号", () => {
    const host = document.createElement("div");
    host.innerHTML = renderNoteMarkdown("- [ ] 一").html;
    expect(host.querySelector("li.notebook-task-item")?.hasAttribute("data-task-line")).toBe(false);
  });

  it("非任务的列表项不带类名也不带行号", () => {
    const host = mount("- 普通\n- [ ] 任务");
    expect(items(host)).toHaveLength(1);
    expect(items(host)[0]?.getAttribute("data-task-line")).toBe("2");
  });

  it("嵌套任务:外层拿自己的行号,不被内层抢走", () => {
    const host = mount("- [ ] 外层\n  - [x] 内层\n- [ ] 后面");
    expect(items(host).map((li) => li.getAttribute("data-task-line"))).toEqual(["1", "2", "3"]);
  });

  it("公式之后的任务行号对着源码 —— 抽数学会压行,行号在那之前就算好了", () => {
    const host = mount("$$\na = 1\nb = 2\n$$\n\n- [ ] 公式后面");
    expect(host.querySelector("li.notebook-task-item")?.getAttribute("data-task-line")).toBe("6");
  });

  it("围栏里的 `- [ ]` 不产任务项", () => {
    const host = mount("```md\n- [ ] 假的\n```\n\n- [ ] 真的");
    expect(items(host)).toHaveLength(1);
    expect(items(host)[0]?.getAttribute("data-task-line")).toBe("5");
  });

  it("只产一个复选框(不要在 renderer 里再补一个)", () => {
    expect(boxes(mount("- [ ] 一"))).toHaveLength(1);
  });

  it("行号属性过得了 DOMPurify", () => {
    // 白名单漏了这两个键的话,属性会被静默剥掉,复选框就永远解禁不了。
    expect(renderNoteMarkdown("- [ ] 一", { taskLines: true }).html).toContain("data-task-line");
  });

  it("渲染出来默认是只读的 —— 没跑 enhance 的场景(导出、嵌入)保持现状", () => {
    expect(boxes(mount("- [ ] 一"))[0]?.disabled).toBe(true);
  });
});

describe("enhanceTaskCheckboxes", () => {
  it("解禁带行号的复选框并补无障碍名", () => {
    const host = mount("- [ ] 写周报");
    enhanceTaskCheckboxes(host, labels);
    const box = boxes(host)[0]!;
    expect(box.disabled).toBe(false);
    expect(box.hasAttribute("disabled")).toBe(false);
    expect(box.classList.contains(TASK_CHECKBOX_CLASS)).toBe(true);
    expect(box.getAttribute("aria-label")).toBe("勾选任务:写周报");
  });

  it("没有行号的保持只读", () => {
    const host = mount("- [ ] 别人的笔记", false);
    enhanceTaskCheckboxes(host, labels);
    const box = boxes(host)[0]!;
    expect(box.disabled).toBe(true);
    expect(box.classList.contains(TASK_CHECKBOX_CLASS)).toBe(false);
    expect(box.hasAttribute("aria-label")).toBe(false);
  });

  it("无障碍名只取自己那一层的文字,不含嵌套任务", () => {
    const host = mount("- [ ] 外层\n  - [x] 内层");
    enhanceTaskCheckboxes(host, labels);
    expect(boxes(host)[0]?.getAttribute("aria-label")).toBe("勾选任务:外层");
    expect(boxes(host)[1]?.getAttribute("aria-label")).toBe("勾选任务:内层");
  });

  it("幂等:再跑一次不会出问题", () => {
    const host = mount("- [ ] 一");
    enhanceTaskCheckboxes(host, labels);
    enhanceTaskCheckboxes(host, labels);
    const box = boxes(host)[0]!;
    expect(box.disabled).toBe(false);
    expect(box.className.split(/\s+/).filter((cls) => cls === TASK_CHECKBOX_CLASS)).toHaveLength(1);
  });

  /* 只有复选框该拿到 aria-label。任务项里的其它节点(li 自己、强调、链接)挂上无障碍名
     会让读屏软件把同一句话念两遍,而且它们并不可点。 */
  it("任务项里只有复选框拿到无障碍名", () => {
    const host = mount("- [ ] 带 **强调** 的任务");
    enhanceTaskCheckboxes(host, labels);
    const labelled = Array.from(host.querySelectorAll("[aria-label]"));
    expect(labelled).toHaveLength(1);
    expect(labelled[0]?.tagName).toBe("INPUT");
  });

  it("不碰任务项之外的复选框", () => {
    const host = mount("- [ ] 任务");
    const stray = document.createElement("input");
    stray.type = "checkbox";
    stray.disabled = true;
    host.append(stray);
    enhanceTaskCheckboxes(host, labels);
    expect(stray.disabled).toBe(true);
  });

  /* 混合场景:当前笔记的任务(带行号)和嵌入进来的任务(不带)在同一份 DOM 里。
     嵌入内容是 `noteEmbed` 事后塞进去的,渲染时没开 taskLines。 */
  it("同一份 DOM 里只解禁带行号的那些", () => {
    const host = mount("- [ ] 我的");
    const embedded = document.createElement("div");
    embedded.innerHTML = renderNoteMarkdown("- [ ] 别人的").html;
    host.append(embedded);
    enhanceTaskCheckboxes(host, labels);
    const all = boxes(host);
    expect(all).toHaveLength(2);
    expect(all[0]?.disabled).toBe(false);
    expect(all[1]?.disabled).toBe(true);
  });
});

describe("taskToggleFromEvent", () => {
  /** 造一次点击并把事件交给被测函数。 */
  function hitOf(target: Element) {
    let hit: ReturnType<typeof taskToggleFromEvent> = null;
    const onClick = (event: Event) => {
      hit = taskToggleFromEvent(event);
    };
    document.addEventListener("click", onClick);
    target.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    document.removeEventListener("click", onClick);
    return hit;
  }

  it("点复选框给出行号与渲染时的勾选状态", () => {
    const host = mount("- [ ] 一\n- [x] 二");
    document.body.append(host);
    expect(hitOf(boxes(host)[0]!)).toEqual({ line: 1, expectChecked: false });
    expect(hitOf(boxes(host)[1]!)).toEqual({ line: 2, expectChecked: true });
    host.remove();
  });

  it("点任务文本不算 —— 那里可能是 wikilink、行内代码", () => {
    const host = mount("- [ ] 一");
    document.body.append(host);
    expect(hitOf(items(host)[0]!)).toBeNull();
    host.remove();
  });

  it("点没有行号的复选框不算", () => {
    const host = mount("- [ ] 一", false);
    document.body.append(host);
    expect(hitOf(boxes(host)[0]!)).toBeNull();
    host.remove();
  });

  it("嵌套里点内层,给的是内层那一行", () => {
    const host = mount("- [ ] 外层\n  - [x] 内层");
    document.body.append(host);
    expect(hitOf(boxes(host)[1]!)).toEqual({ line: 2, expectChecked: true });
    host.remove();
  });

  /* 内层没对上行号、外层对上了:`closest("li[data-task-line]")` 会一路找到外层去,
     于是点内层的框改的是外层那一行。所以只认**最近**那个 `<li>`。 */
  it("内层没有行号时不往外层借", () => {
    const host = mount("- [ ] 外层\n  - [x] 内层");
    document.body.append(host);
    const inner = items(host)[1]!;
    inner.removeAttribute("data-task-line");
    expect(hitOf(boxes(host)[1]!)).toBeNull();
    host.remove();
  });

  it("行号不是正整数就不认", () => {
    const host = mount("- [ ] 一");
    document.body.append(host);
    const li = items(host)[0]!;
    for (const bad of ["0", "-1", "abc", "", "3abc", "1.5"]) {
      li.setAttribute("data-task-line", bad);
      expect(hitOf(boxes(host)[0]!)).toBeNull();
    }
    host.remove();
  });

  it("和任务无关的点击返回 null", () => {
    const plain = document.createElement("button");
    document.body.append(plain);
    expect(hitOf(plain)).toBeNull();
    plain.remove();
  });

  it("非复选框的 input 不认", () => {
    const host = mount("- [ ] 一");
    const text = document.createElement("input");
    text.type = "text";
    items(host)[0]!.append(text);
    document.body.append(host);
    expect(hitOf(text)).toBeNull();
    host.remove();
  });
});
