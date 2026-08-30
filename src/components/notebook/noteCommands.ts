/* 命令面板(⌘K)的模型层:模糊匹配打分 + 候选排序。
 *
 * 纯函数,不碰 DOM、不碰 IPC。面板本体在 `NoteCommandPalette.tsx`,命令清单由
 * `NotebookPanel` 现场组装(每条命令的 `run` 都是面板自己的处理函数)。
 *
 * 打分为什么不直接抄一个通用 fuzzy 库:命令面板的候选里既有 i18n 文案(中英
 * 混排),也有用户自己起的笔记标题。通用库的启发式基本都建立在「词」上
 * ——首字母缩写、camelCase 边界、连字符分段——而中日韩没有词边界
 * (见 memory `cjk-has-no-word-boundaries`),那些加分项在中文候选上全部失效,
 * 于是中文候选永远排在英文候选后面。
 *
 * 这里的规则只有一条是特殊的:**查询里相邻的两个中日韩字符,在候选里也必须
 * 相邻**。理由是汉字单字就是语素,允许跳字会让 `全库` 命中「**全**文搜索的
 * 词**库**统计」这种毫无关系的候选;而拉丁字母的跳字匹配(`nsr` → `NoteSearchResult`)
 * 是用户真正期待的行为,所以只对中日韩收紧,不一刀切。
 */

/** 候选被匹配上的一段(左闭右开,单位是 UTF-16 码元下标)。用来画高亮。 */
export type MatchSpan = { from: number; to: number };

export type MatchResult = {
  score: number;
  spans: MatchSpan[];
};

/* 和 `noteFindText.ts` 保持同一份定义:汉字、平假名、片假名。谚文不算
   —— 韩文分词写空格,词边界在它上面是成立的。 */
const CJK_RE = /^[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}]$/u;

function isCjk(ch: string): boolean {
  return ch !== "" && CJK_RE.test(ch);
}

/** 词首:候选的第 0 个字符,或者前一个字符不是字母数字。 */
function isWordStart(chars: string[], index: number): boolean {
  if (index === 0) return true;
  return !/[\p{L}\p{N}]/u.test(chars[index - 1] ?? "");
}

const SCORE_BASE = 1;
const SCORE_CONSECUTIVE = 8;
const SCORE_WORD_START = 6;
const SCORE_TARGET_START = 10;

/**
 * 把 `query` 按顺序匹配到 `target` 上,返回得分与命中区间;匹配不上返回 null。
 *
 * 匹配大小写不敏感(命令面板里没人会为了大小写重打一遍)。空查询返回 0 分且没有
 * 命中区间 —— 调用方据此走「不过滤、按原序显示」那条路。
 *
 * 贪心从左到右,不做回溯。回溯能在个别输入上多找出一处命中,但代价是最坏情况
 * 指数级,而命令面板每次按键都要重算整表 —— 这里选可预测的时间。
 *
 * 「命中越靠前越好」只由 `SCORE_TARGET_START` / `SCORE_WORD_START` 表达,没有再
 * 叠一个「首个命中之前每跳一格扣分」。两者是同一个偏好的两套写法,同时存在的时候
 * 谁都不是决定性的 —— 去掉任意一个,排序结果都不变,于是也就没有测试能守住它们。
 */
export function scoreFuzzyMatch(query: string, target: string): MatchResult | null {
  const q = [...query];
  const chars = [...target];
  if (q.length === 0) return { score: 0, spans: [] };
  if (chars.length === 0) return null;

  const lowerQ = q.map((ch) => ch.toLocaleLowerCase());
  const lowerT = chars.map((ch) => ch.toLocaleLowerCase());

  const spans: MatchSpan[] = [];
  let score = 0;
  let at = 0;
  let prevIndex = -1;

  for (let qi = 0; qi < lowerQ.length; qi += 1) {
    /* 查询里相邻的两个中日韩字符,在候选里也必须相邻。前一个字符匹配在
       `prevIndex`,那这一个只能落在 `prevIndex + 1`。 */
    const glued = qi > 0 && isCjk(q[qi] ?? "") && isCjk(q[qi - 1] ?? "");
    const from = glued ? prevIndex + 1 : at;
    let found = -1;
    for (let ti = from; ti < lowerT.length; ti += 1) {
      if (lowerT[ti] === lowerQ[qi]) {
        found = ti;
        break;
      }
      // 粘连的那一步只许看一格,看不到就是不匹配。
      if (glued) break;
    }
    if (found < 0) return null;

    score += SCORE_BASE;
    if (found === prevIndex + 1 && prevIndex >= 0) score += SCORE_CONSECUTIVE;
    if (found === 0) score += SCORE_TARGET_START;
    else if (isWordStart(chars, found)) score += SCORE_WORD_START;

    const last = spans[spans.length - 1];
    if (last && last.to === found) last.to = found + 1;
    else spans.push({ from: found, to: found + 1 });

    prevIndex = found;
    at = found + 1;
  }

  return { score, spans };
}

/**
 * 在多个字段上打分,取最高分的那个字段的结果。
 *
 * 命令有 `label` 和 `keywords`(别名),笔记有标题和文件名 —— 用户可能按任一个
 * 搜。返回值带上命中的是哪个字段的下标,因为高亮只能画在真正显示出来的那段
 * 文本上:`keywords` 匹配上但显示的是 `label` 时,把 `keywords` 的区间画到
 * `label` 上会高亮到错的字符。
 */
export type FieldMatch = MatchResult & { field: number };

export function scoreFields(query: string, fields: readonly string[]): FieldMatch | null {
  let best: FieldMatch | null = null;
  fields.forEach((text, field) => {
    const hit = scoreFuzzyMatch(query, text);
    if (!hit) return;
    if (!best || hit.score > best.score) best = { ...hit, field };
  });
  return best;
}

/** 一条命令。`run` 由面板提供,模型层不关心它做什么。 */
export type NoteCommand = {
  id: string;
  /** 已经过 i18n 的显示文案。 */
  label: string;
  /** 分组的 i18n **键**(渲染时才翻译,便于分组标题去重)。 */
  group: string;
  /** 别名:也参与匹配,但不显示。用来收「新建 / new / add」这类同义词。 */
  keywords?: readonly string[];
  /** 快捷键提示,例如 `⌘⇧F`。只显示,不绑定。 */
  hint?: string;
  /** 当前不可用(例如没有打开的笔记时的「删除这篇」)。仍然显示,但不能执行。 */
  disabled?: boolean;
  run: () => void;
};

/** 面板里的一行。命令和笔记共用一套渲染,所以收成同一个类型。 */
export type PaletteEntry =
  | { kind: "command"; command: NoteCommand; spans: MatchSpan[] }
  | { kind: "note"; noteId: string; title: string; recent: boolean; spans: MatchSpan[] };

export const PALETTE_NOTE_LIMIT = 30;

export type PaletteNote = { id: string; title: string; fileName: string };

/**
 * 组装面板里要显示的行。
 *
 * 空查询时:命令按传入顺序全列,笔记只列最近打开过的(按最近在前)。理由是空
 * 查询下「全部笔记」会把命令挤到看不见的地方,而刚打开过的那几篇才是用户按
 * ⌘K 想跳回去的目标。
 *
 * 有查询时:两类都按分数降序。命令排在同分笔记前面 —— 命令是有限的、可穷举的,
 * 笔记数量无上限,让笔记插到命令中间会让「⌘K 然后打几个字执行命令」这条路变得
 * 不可预测。
 */
export function buildPaletteEntries({
  query,
  commands,
  notes,
  recentNoteIds,
  noteLimit = PALETTE_NOTE_LIMIT,
}: {
  query: string;
  commands: readonly NoteCommand[];
  notes: readonly PaletteNote[];
  recentNoteIds: readonly string[];
  noteLimit?: number;
}): PaletteEntry[] {
  const trimmed = query.trim();
  const recentRank = new Map(recentNoteIds.map((id, index) => [id, index]));

  if (!trimmed) {
    const commandEntries: PaletteEntry[] = commands.map((command) => ({
      kind: "command",
      command,
      spans: [],
    }));
    /* 只取最近打开过的,并且按 recents 的顺序 —— 不是笔记列表的顺序。
       `recentNoteIds` 里可能有已经删掉的笔记,靠这次 join 自然滤掉。 */
    const byId = new Map(notes.map((note) => [note.id, note]));
    const noteEntries: PaletteEntry[] = [];
    for (const id of recentNoteIds) {
      const note = byId.get(id);
      if (!note) continue;
      noteEntries.push({ kind: "note", noteId: id, title: note.title, recent: true, spans: [] });
      if (noteEntries.length >= noteLimit) break;
    }
    return [...commandEntries, ...noteEntries];
  }

  const scoredCommands: { entry: PaletteEntry; score: number; order: number }[] = [];
  commands.forEach((command, order) => {
    const hit = scoreFields(trimmed, [command.label, ...(command.keywords ?? [])]);
    if (!hit) return;
    scoredCommands.push({
      // 只有命中 `label`(字段 0)时才画高亮:别名的区间落在别名上,画到 label 上会错位。
      entry: { kind: "command", command, spans: hit.field === 0 ? hit.spans : [] },
      score: hit.score,
      order,
    });
  });

  const scoredNotes: { entry: PaletteEntry; score: number; order: number }[] = [];
  notes.forEach((note, order) => {
    const hit = scoreFields(trimmed, [note.title, note.fileName]);
    if (!hit) return;
    /* 最近打开过的加一点分,但不足以越过一次真正更好的匹配 —— recents 是
       「同样像的时候优先」,不是「永远置顶」。 */
    const bonus = recentRank.has(note.id) ? 3 : 0;
    scoredNotes.push({
      entry: {
        kind: "note",
        noteId: note.id,
        title: note.title,
        recent: recentRank.has(note.id),
        spans: hit.field === 0 ? hit.spans : [],
      },
      score: hit.score + bonus,
      order,
    });
  });

  // 同分按原序:笔记列表是修改时间倒序,命令是面板给的顺序,两者都有意义。
  const byScore = (
    a: { score: number; order: number },
    b: { score: number; order: number },
  ): number => (b.score - a.score !== 0 ? b.score - a.score : a.order - b.order);

  scoredCommands.sort(byScore);
  scoredNotes.sort(byScore);

  return [
    ...scoredCommands.map((item) => item.entry),
    ...scoredNotes.slice(0, noteLimit).map((item) => item.entry),
  ];
}

/** 键盘上下移动后的新下标。空列表返回 -1;到头就停,不循环。 */
export function moveSelection(current: number, delta: number, count: number): number {
  if (count <= 0) return -1;
  const next = current + delta;
  if (next < 0) return 0;
  if (next >= count) return count - 1;
  return next;
}
