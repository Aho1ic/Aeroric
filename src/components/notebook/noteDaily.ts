/* 日记(Daily Note):约定落点 `<vault>/Daily/YYYY-MM-DD.md`。
 *
 * 纯模型:算路径、从路径反解日期。开/建由面板做。
 *
 * 为什么日记不走 `createNoteInVault`(按标题分配文件名):那条路会去重 —— 第二次以
 * 同一个标题新建会得到 `2026-08-28-2.md`。而「打开今天的日记」必须每次都落到**同一个**
 * 文件上,所以这里自己拼死路径,交给 `createNote`,把 `ALREADY_EXISTS` 当成正常分支
 * (= 已经有了,直接打开)。
 *
 * 文件名同时是 wikilink 的目标(见 memory `notebook-filename-is-a-link-target`),
 * `YYYY-MM-DD` 这个形状正好让 `[[2026-08-28]]` 指得到,所以更不能改名。
 */

/** 日记所在的子目录名。`createNote` 会 `create_dir_all`,不用先建。 */
export const DAILY_DIR = "Daily";

function pad2(value: number): string {
  return String(value).padStart(2, "0");
}

/** `2026-08-28`。日记的文件名(不含扩展名),也是它的默认标题。 */
export function dailyNoteName(date: Date): string {
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`;
}

/** 某天日记的绝对路径。 */
export function dailyNotePath(vault: string, date: Date): string {
  return `${vault}/${DAILY_DIR}/${dailyNoteName(date)}.md`;
}

/* 整个文件名必须就是日期,不能只是「以日期结尾」。
   `Daily/会议-2026-08-28.md` 那种不是日记 —— 认成日记之后「前一天」会从它跳到
   `2026-08-27.md`,而用户以为自己在翻会议记录。 */
const DAILY_STEM_RE = /(?:^|\/)(\d{4})-(\d{2})-(\d{2})\.md$/i;

/**
 * 从路径反解日记日期。不是日记返回 `null`。
 *
 * 两道判断都必须有:
 * 1. 路径里得有 `Daily/` 这一段 —— 否则任意一篇名字长得像日期的笔记都会被当成日记。
 * 2. 年月日要**能往回对上**。`new Date(2026, 1, 30)` 不会失败,它会滚到 3 月 2 日,
 *    于是 `2026-02-30.md` 被解析成 3 月 2 日,「前一天」跳到 3 月 1 日 —— 一个不存在的
 *    日期静默变成了另一个存在的日期。`Number.isNaN` 抓不到这种,只有回对一遍才行。
 */
export function dailyDateFromPath(path: string): Date | null {
  const normalized = path.replace(/\\/g, "/");
  if (!/\/daily\//i.test(normalized)) return null;
  const match = DAILY_STEM_RE.exec(normalized);
  if (!match) return null;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const date = new Date(year, month - 1, day);
  if (date.getFullYear() !== year || date.getMonth() !== month - 1 || date.getDate() !== day) {
    return null;
  }
  return date;
}

/**
 * 相对某一天偏移若干天。
 *
 * 用 `new Date(y, m, d + delta)` 而不是加减毫秒:跨夏令时的那一天只有 23 小时,
 * 按毫秒加一天会落回同一天的 23:00。构造函数会自己处理月末与闰年溢出。
 */
export function shiftDays(date: Date, delta: number): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() + delta);
}

/**
 * 「前一天 / 后一天」的基准日:当前打开的这篇是日记就以它为准,否则以今天为准。
 *
 * 以当前这篇为准是为了能连着翻:在 08-28 上按前一天到 08-27,再按一次要到 08-26,
 * 而不是又回到「今天减一天」。
 */
export function dailyStepFrom(activePath: string | null, today: Date, delta: number): Date {
  const base = (activePath ? dailyDateFromPath(activePath) : null) ?? today;
  return shiftDays(base, delta);
}
