/* 笔记模板(周报 / 月度 Retro / OKR / 项目启动 / 会议纪要 / Bug 复盘 / 日记)。
 *
 * 纯模型:算日期占位符、按 i18n 键取出正文。不碰 IPC、不碰 DOM。
 *
 * **正文走 i18n,不写死在这里。** 模板正文是给用户写字用的骨架,标题一栏是「今日要做」
 * 还是「To do」取决于他用哪种语言,而不是取决于模板作者。所以每个模板的正文是一个
 * i18n 键,日期之类的东西用 `{date}` / `{week}` 占位符,由 `t(key, vars)` 展开。
 *
 * Markio 那边有两个模板没搬:
 * - `blank`(空白笔记)—— 就是现有的「新建随手记」,再加一条只是同一件事的第二个入口。
 * - `folder`(新建文件夹)—— 随手记面板是平铺列表,`flattenTree` 会把目录整个丢掉,
 *   面板里也没有任何文件夹 UI(`createFolder` 只有 SFTP 面板在用)。搬过来的话点一下
 *   会在磁盘上建出一个面板里看不见的空目录,那不是「功能少一个」,是一个死入口。
 */

/** 模板 id。日记那条由 `noteDaily.ts` 单独用 —— 它的落点是固定路径,不走这套新建流程。 */
export type NoteTemplateId =
  | "daily"
  | "weekly"
  | "monthlyRetro"
  | "okr"
  | "kickoff"
  | "meeting"
  | "bugPostmortem";

export type NoteTemplate = {
  id: NoteTemplateId;
  /** 显示名的 i18n 键。 */
  titleKey: string;
  /** 一行说明的 i18n 键。 */
  subKey: string;
  /** 默认笔记标题的 i18n 键。文件名由后端从标题分配(slug + 去重)。 */
  nameKey: string;
  /** 正文的 i18n 键。 */
  bodyKey: string;
  /** 命令面板的别名,中英各给几个。 */
  keywords: readonly string[];
};

/** 模板正文 / 标题里能用的占位符。 */
export type NoteTemplateVars = {
  /** `2026-08-28` */
  date: string;
  /** `2026` —— ISO 周所属的年,跨年那周和日历年不是一回事。 */
  year: number;
  /** `2026-08` */
  month: string;
  /** `35`,两位补零。 */
  week: string;
  /** 1..4 */
  quarter: number;
};

function pad2(value: number): string {
  return String(value).padStart(2, "0");
}

export function ymd(date: Date): string {
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`;
}

/**
 * ISO-8601 周号与它所属的年。
 *
 * 为什么要绕 UTC:算法要把日期挪到「本周的星期四」,而本地时区遇到夏令时切换的那一周,
 * `setDate` 跨过的那一天是 23 或 25 小时,按毫秒差除 86400000 会差出一天。取 UTC 之后
 * 每天恒定 24 小时。
 *
 * 跨年那几天是这段唯一容易错的地方,所以 `year` 取的是**挪过之后**那一天的年份,
 * 不是入参的年份:2027-01-01 是周五,它属于 2026-W53(2026 的 1 月 1 日是周四,
 * 所以 2026 有 53 个 ISO 周);反过来 2025-12-29 是周一,它属于 2026-W01。
 */
export function isoWeek(date: Date): { year: number; week: number } {
  const moved = new Date(Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()));
  // getUTCDay 里周日是 0,ISO 里周日是 7。
  const dayNumber = moved.getUTCDay() || 7;
  moved.setUTCDate(moved.getUTCDate() + 4 - dayNumber);
  const yearStart = new Date(Date.UTC(moved.getUTCFullYear(), 0, 1));
  const week = Math.ceil(((moved.getTime() - yearStart.getTime()) / 86_400_000 + 1) / 7);
  return { year: moved.getUTCFullYear(), week };
}

export function templateVars(now: Date): NoteTemplateVars {
  const { year, week } = isoWeek(now);
  return {
    date: ymd(now),
    year,
    month: `${now.getFullYear()}-${pad2(now.getMonth() + 1)}`,
    week: pad2(week),
    quarter: Math.floor(now.getMonth() / 3) + 1,
  };
}

export const NOTE_TEMPLATES: readonly NoteTemplate[] = [
  {
    id: "weekly",
    titleKey: "notebook.templateWeekly",
    subKey: "notebook.templateWeeklySub",
    nameKey: "notebook.templateWeeklyName",
    bodyKey: "notebook.templateWeeklyBody",
    keywords: ["weekly", "week", "report", "周报", "zhoubao"],
  },
  {
    id: "monthlyRetro",
    titleKey: "notebook.templateMonthlyRetro",
    subKey: "notebook.templateMonthlyRetroSub",
    nameKey: "notebook.templateMonthlyRetroName",
    bodyKey: "notebook.templateMonthlyRetroBody",
    keywords: ["retro", "monthly", "review", "月度", "复盘", "fupan"],
  },
  {
    id: "okr",
    titleKey: "notebook.templateOkr",
    subKey: "notebook.templateOkrSub",
    nameKey: "notebook.templateOkrName",
    bodyKey: "notebook.templateOkrBody",
    keywords: ["okr", "objective", "goal", "季度", "目标", "mubiao"],
  },
  {
    id: "kickoff",
    titleKey: "notebook.templateKickoff",
    subKey: "notebook.templateKickoffSub",
    nameKey: "notebook.templateKickoffName",
    bodyKey: "notebook.templateKickoffBody",
    keywords: ["kickoff", "project", "启动", "项目", "qidong"],
  },
  {
    id: "meeting",
    titleKey: "notebook.templateMeeting",
    subKey: "notebook.templateMeetingSub",
    nameKey: "notebook.templateMeetingName",
    bodyKey: "notebook.templateMeetingBody",
    keywords: ["meeting", "minutes", "notes", "会议", "纪要", "huiyi"],
  },
  {
    id: "bugPostmortem",
    titleKey: "notebook.templateBugPostmortem",
    subKey: "notebook.templateBugPostmortemSub",
    nameKey: "notebook.templateBugPostmortemName",
    bodyKey: "notebook.templateBugPostmortemBody",
    keywords: ["bug", "postmortem", "incident", "复盘", "故障", "guzhang"],
  },
];

/** 日记用的模板。不在 `NOTE_TEMPLATES` 里 —— 它的落点是固定路径,不走「新建」那条路。 */
export const DAILY_TEMPLATE = {
  nameKey: "notebook.templateDailyName",
  bodyKey: "notebook.templateDailyBody",
} as const;

/**
 * 把模板展开成「标题 + 正文」。
 *
 * 标题会交给后端分配文件名(slug + 去重),所以这里不管重名。
 */
export function buildTemplate(
  template: { nameKey: string; bodyKey: string },
  now: Date,
  t: (key: string, vars?: Record<string, string | number>) => string,
): { title: string; body: string } {
  const vars = templateVars(now);
  return {
    title: t(template.nameKey, vars),
    body: t(template.bodyKey, vars),
  };
}
