/* 用户自定义模板的展开:占位符替换 + 转成命令面板要的形状。
 *
 * 纯模型。文件从磁盘来(后端 `notebook_list_user_templates`),这里只做文本替换。
 *
 * **为什么占位符在前端展开**:`{{date}}` / `{{time}}` 要按用户的本地时区算,而后端不知道
 * webview 的时区 —— 容器或服务里跑 UTC 的情况很常见,那样「今天」会在下午六点之后就变成
 * 明天。日期口径统一在前端(`ymd` 与内置模板共用同一份),后端只负责把文件原样读出来。
 *
 * **为什么和内置模板用两套占位符语法**:内置模板的正文是 i18n 键,`t(key, vars)` 认的是
 * 单花括号 `{date}`;而这里的正文是用户写在 `.md` 文件里的字面文本,不过 i18n。给它们统一
 * 语法要么让 i18n 的插值去认双花括号(改的是全项目共用的 `t`),要么把用户模板灌进 i18n
 * 表(键是运行时才知道的)。两条都不划算。双花括号也正好和 Markio 的模板文件兼容 ——
 * 用户能把手上那些模板直接拷过来。
 */

import { ymd } from "./noteTemplates";
import type { UserTemplate } from "./notebookApi";

/** 命令面板里的一条自定义模板。`id` 带 `user:` 前缀,和内置模板分开。 */
export type UserTemplateEntry = {
  id: string;
  title: string;
  /** 已展开日期占位符的默认标题。 */
  name: string;
  /** 已展开日期占位符的正文,`{{title}}` 还留着(要等最终标题定下来)。 */
  body: string;
};

function pad2(value: number): string {
  return String(value).padStart(2, "0");
}

/** `14:07`。本地时区。 */
export function hhmm(date: Date): string {
  return `${pad2(date.getHours())}:${pad2(date.getMinutes())}`;
}

/**
 * 展开 `{{date}}` 与 `{{time}}`。`{{title}}` **不动** —— 它要等标题最终定下来。
 *
 * 大小写不敏感:用户写 `{{Date}}` 的意图毫无疑问是同一个占位符,而留一个没展开的
 * `{{Date}}` 在正文里看起来就像功能坏了。
 */
export function fillDateTime(text: string, now: Date): string {
  return text.replace(/\{\{\s*date\s*\}\}/gi, ymd(now)).replace(/\{\{\s*time\s*\}\}/gi, hhmm(now));
}

/** 展开 `{{title}}`。标题定下来之后才调。 */
export function fillTitle(text: string, title: string): string {
  return text.replace(/\{\{\s*title\s*\}\}/gi, title);
}

/**
 * 把后端的一条模板展开成面板要的形状。
 *
 * `name` 里的 `{{title}}` 直接删掉:那是「标题」自己,拿它当标题的一部分是循环引用。
 * 删完只剩空白时回落到 `title` —— 一条没有标题的新笔记在列表里是一行点不动的空白。
 */
export function expandUserTemplate(template: UserTemplate, now: Date): UserTemplateEntry {
  const name = fillTitle(fillDateTime(template.name, now), "").trim();
  return {
    id: `user:${template.id}`,
    title: template.title,
    name: name.length > 0 ? name : template.title,
    body: fillDateTime(template.body, now),
  };
}

/**
 * 自定义模板在命令面板里的搜索别名。
 *
 * 标题本身由面板当 label 传进打分器,这里只补 id(文件名 stem)—— 用户可能记得的是
 * 文件叫什么,而不是 frontmatter 里写的显示名。两者相同时不重复给。
 */
export function userTemplateKeywords(template: UserTemplate): string[] {
  return template.id === template.title ? [] : [template.id];
}
