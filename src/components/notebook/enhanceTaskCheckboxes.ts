/* 让阅读态渲染出来的 `- [ ]` 复选框可点。
 *
 * 分工:行号从哪来、怎么写回在 `noteTasks.ts`,`<li>` 上的属性由 `noteRender.ts` 产出,
 * 这一层只管 DOM —— 解禁复选框、补无障碍属性、从一次点击里认出"点的是哪一行"。
 *
 * 为什么渲染出来的 HTML 仍然带 `disabled`,由这个函数摘掉:那是**默认只读**的兜底。
 * 嵌入、悬浮预览、导出的 HTML 都不会跑这个函数,于是它们保持现在的只读样子;只有当前
 * 笔记那一次渲染(带 `data-task-line`)会被解禁。有没有行号是唯一的闸门,不再另设开关。
 */

import { TASK_CHECKED_ATTR, TASK_LINE_ATTR } from "./noteRender";

/** 解禁后挂在 `<input>` 上,用来上"可点"的样式。 */
export const TASK_CHECKBOX_CLASS = "notebook-task-checkbox";

/** 文案由调用方注入 —— 这个模块不该 import i18n(和 `enhanceWikiLinks` 同一个理由)。 */
export type TaskCheckboxLabels = {
  /** 复选框的无障碍名。`text` 是任务文本,可能为空。 */
  toggle: (text: string) => string;
};

/**
 * 找出这个复选框所属的任务项 `<li>`,要求行号就挂在**最近**那个 `<li>` 上。
 *
 * 不能直接 `closest("li[data-task-line]")`:嵌套列表里,一个没对上行号的子项外面
 * 套着有行号的父项,那样会一路找到父项去 —— 点子项的框就会改父项那一行。
 */
function taskItemOf(el: Element): HTMLElement | null {
  const li = el.closest("li");
  if (!(li instanceof HTMLElement)) return null;
  return li.hasAttribute(TASK_LINE_ATTR) ? li : null;
}

/** 取任务项自己那一层的文字,不含嵌套列表(那些是别的任务的名字)。 */
function ownText(li: HTMLElement): string {
  const clone = li.cloneNode(true) as HTMLElement;
  for (const nested of Array.from(clone.querySelectorAll("ul,ol"))) nested.remove();
  return (clone.textContent ?? "").trim();
}

/**
 * 就地解禁 `host` 里带行号的任务复选框。
 *
 * 幂等:阅读态每次改字都会重挂 HTML,而主题切换、笔记列表变化会在同一份 DOM 上再调
 * 一次,重复解禁同一个框没有副作用。
 */
export function enhanceTaskCheckboxes(host: HTMLElement, labels: TaskCheckboxLabels): void {
  /* 选择器只管"是不是复选框",要不要解禁全交给 `taskItemOf`。
     不写成 `li.notebook-task-item input[...]`:那样"在任务项里"就成了第二道闸门,和
     `taskItemOf` 判的是同一件事 —— 两道闸门互相兜底,改坏任何一道测试都察觉不到。 */
  const boxes = host.querySelectorAll<HTMLInputElement>('input[type="checkbox"]');
  for (const box of Array.from(boxes)) {
    const li = taskItemOf(box);
    // 没对上行号的保持只读:不知道该改哪一行,可点就只会勾错。
    if (!li) continue;
    box.disabled = false;
    box.classList.add(TASK_CHECKBOX_CLASS);
    /* 显式补 aria-label:marked 产的是个裸 `<input>`,没有 `<label>` 包裹也没有
       `aria-labelledby`,读屏软件只会念"复选框"。 */
    box.setAttribute("aria-label", labels.toggle(ownText(li)));
  }
}

/** 一次点击命中的任务项。`expectChecked` 是渲染那一刻的状态,写回时当乐观锁。 */
export type TaskToggleHit = {
  line: number;
  expectChecked: boolean;
};

/**
 * 从一次事件里认出被点的任务复选框。返回 null 表示这次点击与任务无关,调用方不该拦。
 *
 * 只认复选框本身,不认整个 `<li>`:任务文本里可以有 wikilink、脚注、行内代码,
 * 点在那些上面应该是它们各自的行为(或者什么都不做),不是勾选。
 */
export function taskToggleFromEvent(event: Event): TaskToggleHit | null {
  const el = event.target;
  // 一个条件而不是两个 if:"是复选框吗"是一件事。拆成两道各自能被改坏而另一道兜住的
  // 闸门,只会让测试钉不住任何一道。
  if (!(el instanceof HTMLInputElement) || el.type !== "checkbox") return null;
  const li = taskItemOf(el);
  if (!li) return null;
  const raw = li.getAttribute(TASK_LINE_ATTR) ?? "";
  const line = Number.parseInt(raw, 10);
  // 行号必须是个正整数。`Number.parseInt` 对 `"3abc"` 会给 3,所以额外要求整串是数字。
  if (!/^\d+$/.test(raw) || !Number.isInteger(line) || line < 1) return null;
  return { line, expectChecked: li.getAttribute(TASK_CHECKED_ATTR) === "1" };
}
