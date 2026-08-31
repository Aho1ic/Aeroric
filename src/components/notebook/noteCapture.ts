/* 快速捕获的模型层:落点路径 + 追加正文的拼法。
 *
 * 纯函数,不碰 IPC、不碰 DOM。窗在 `NoteQuickCapture.tsx`,落盘编排在面板里。
 *
 * 两个落点:今天的日记(和「今天的日记」那条命令同一个文件),和收集箱
 * `<vault>/Inbox.md`。收集箱刻意是**单个文件**而不是一篇篇新笔记 —— 捕获的东西
 * 多半是一句话,一句话一篇会让笔记列表在一周内变得没法用。
 */

import { dailyNotePath } from "./noteDaily";

export type CaptureTarget = "today" | "inbox";

/** 收集箱的文件名。和日记一样是约定路径 —— 它同时是 `[[Inbox]]` 的目标。 */
export const INBOX_NAME = "Inbox.md";

export function inboxNotePath(vault: string): string {
  return `${vault}/${INBOX_NAME}`;
}

export function capturePath(vault: string, target: CaptureTarget, date: Date): string {
  return target === "today" ? dailyNotePath(vault, date) : inboxNotePath(vault);
}

/**
 * 给用户看的落点,vault 相对。
 *
 * 自己拼而不是拿绝对路径去 `vaultRelativePath` 切:那个函数会在「路径不在 vault 里」
 * 时返回 null,而这两个落点是我们自己按 vault 拼出来的,不可能不在里面 —— 那条
 * null 分支在这里只能变成一句要处理但永不发生的类型噪音。
 */
export function captureRelativePath(target: CaptureTarget, date: Date): string {
  return target === "today" ? dailyNotePath("", date).slice(1) : INBOX_NAME;
}

/** `14:07`。捕获块的小标题 —— 一天里的多次捕获靠它区分。 */
export function captureTimeLabel(date: Date): string {
  const hours = String(date.getHours()).padStart(2, "0");
  const minutes = String(date.getMinutes()).padStart(2, "0");
  return `${hours}:${minutes}`;
}

/**
 * 把一次捕获追加到正文末尾,返回新正文。
 *
 * `body` 是**不含 frontmatter** 的正文(vault 层已经拆开了),所以直接往后接就行,
 * frontmatter 由 `joinNote` 在保存时拼回去。
 *
 * 拼法上有三处是刻意的:
 * - 用 `## 时间` 起一块,而不是直接追加裸文本。捕获是一条条独立的东西,不分块的话
 *   下午写的两句会和上午那句连成一段,读的时候分不出边界。
 * - 追加前把已有正文右侧的空白全裁掉再补两个换行。原文末尾有 0 个、1 个还是 3 个
 *   换行取决于上一次是谁写的(模板、用户、上一次捕获),不归一化的话块与块之间的
 *   间距每次都不一样。
 * - 捕获文本本身只 trim,内部换行原样保留 —— 用户按了回车就是想分行。
 *
 * 空白捕获返回原正文(调用方本来就该挡住,这里再兜一次:追加一个只有时间标题的空块
 * 比什么都不做更糟)。
 */
export function appendCapture(body: string, text: string, timeLabel: string): string {
  const captured = text.trim();
  if (captured.length === 0) return body;
  const block = `## ${timeLabel}\n\n${captured}\n`;
  const head = body.replace(/\s+$/, "");
  return head.length === 0 ? block : `${head}\n\n${block}`;
}
