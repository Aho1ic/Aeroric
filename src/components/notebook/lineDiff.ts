/**
 * 行级 diff。用于版本历史里"这条快照和现在差在哪"。
 *
 * 为什么不能按行号逐行比:在开头插一行,之后每一行的行号都会错开一位,逐行比
 * 会把整篇都报成改动 —— 那个数字对用户没有任何参考价值。这里求的是最长公共
 * 子序列(LCS),插入和删除各自成行,未变的行认出来。
 */

/** 一行 diff。`old`/`new` 是 1 基行号,不适用的那侧为 null。 */
export type DiffLine = {
  kind: "context" | "added" | "removed";
  text: string;
  oldLine: number | null;
  newLine: number | null;
};

/**
 * DP 表的单元格上限。
 *
 * 表是 (n+1)×(m+1) 个 Uint8,4M 格约 4MB。超过就退化成"整段删掉再整段插入"
 * —— 那仍然是一个正确的 diff,只是不再指出细粒度的对应关系。宁可退化,也不要
 * 让一篇被整体重写的长笔记把内存吃掉。
 */
const MAX_DIFF_CELLS = 4_000_000;

const DIAGONAL = 1;
const UP = 2;
const LEFT = 3;

export function diffLines(before: string, after: string): DiffLine[] {
  const oldLines = before.split("\n");
  const newLines = after.split("\n");

  // 先剥掉共同的头尾。绝大多数编辑只动几行,剥完之后中间那段通常很小,DP 的
  // 规模也就跟着塌下来。
  let head = 0;
  while (head < oldLines.length && head < newLines.length && oldLines[head] === newLines[head]) {
    head += 1;
  }
  let tail = 0;
  while (
    tail < oldLines.length - head &&
    tail < newLines.length - head &&
    oldLines[oldLines.length - 1 - tail] === newLines[newLines.length - 1 - tail]
  ) {
    tail += 1;
  }

  const oldMiddle = oldLines.slice(head, oldLines.length - tail);
  const newMiddle = newLines.slice(head, newLines.length - tail);

  const result: DiffLine[] = [];
  for (let index = 0; index < head; index += 1) {
    result.push({
      kind: "context",
      text: oldLines[index],
      oldLine: index + 1,
      newLine: index + 1,
    });
  }
  result.push(...diffMiddle(oldMiddle, newMiddle, head));
  for (let index = 0; index < tail; index += 1) {
    const oldIndex = oldLines.length - tail + index;
    const newIndex = newLines.length - tail + index;
    result.push({
      kind: "context",
      text: oldLines[oldIndex],
      oldLine: oldIndex + 1,
      newLine: newIndex + 1,
    });
  }
  return result;
}

/** `offset` 是剥掉的头部行数,用来把行号还原成整篇里的位置。 */
function diffMiddle(oldMiddle: string[], newMiddle: string[], offset: number): DiffLine[] {
  if (oldMiddle.length === 0 && newMiddle.length === 0) return [];
  const cells = (oldMiddle.length + 1) * (newMiddle.length + 1);
  if (oldMiddle.length === 0 || newMiddle.length === 0 || cells > MAX_DIFF_CELLS) {
    return wholesaleReplace(oldMiddle, newMiddle, offset);
  }

  const rows = oldMiddle.length + 1;
  const columns = newMiddle.length + 1;
  const lengths = new Int32Array(rows * columns);
  const moves = new Uint8Array(rows * columns);

  // 从右下往左上填:lengths[i][j] = oldMiddle[i..] 和 newMiddle[j..] 的 LCS 长度。
  // 反着填是为了让回溯从 (0,0) 出发 —— 那样相邻的删除和插入天然是"先删后插",
  // 不用最后再排一遍。
  for (let i = oldMiddle.length - 1; i >= 0; i -= 1) {
    for (let j = newMiddle.length - 1; j >= 0; j -= 1) {
      const at = i * columns + j;
      if (oldMiddle[i] === newMiddle[j]) {
        lengths[at] = lengths[(i + 1) * columns + (j + 1)] + 1;
        moves[at] = DIAGONAL;
        continue;
      }
      const skipOld = lengths[(i + 1) * columns + j];
      const skipNew = lengths[i * columns + (j + 1)];
      // 平手时先走"删掉旧行"。反过来会把改动渲染成"先加后删",读起来别扭。
      if (skipOld >= skipNew) {
        lengths[at] = skipOld;
        moves[at] = UP;
      } else {
        lengths[at] = skipNew;
        moves[at] = LEFT;
      }
    }
  }

  const result: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < oldMiddle.length && j < newMiddle.length) {
    const move = moves[i * columns + j];
    if (move === DIAGONAL) {
      result.push({
        kind: "context",
        text: oldMiddle[i],
        oldLine: offset + i + 1,
        newLine: offset + j + 1,
      });
      i += 1;
      j += 1;
    } else if (move === UP) {
      result.push({
        kind: "removed",
        text: oldMiddle[i],
        oldLine: offset + i + 1,
        newLine: null,
      });
      i += 1;
    } else {
      result.push({ kind: "added", text: newMiddle[j], oldLine: null, newLine: offset + j + 1 });
      j += 1;
    }
  }
  // 一侧走完了,另一侧剩下的全是纯增或纯删。
  for (; i < oldMiddle.length; i += 1) {
    result.push({ kind: "removed", text: oldMiddle[i], oldLine: offset + i + 1, newLine: null });
  }
  for (; j < newMiddle.length; j += 1) {
    result.push({ kind: "added", text: newMiddle[j], oldLine: null, newLine: offset + j + 1 });
  }
  return result;
}

function wholesaleReplace(oldMiddle: string[], newMiddle: string[], offset: number): DiffLine[] {
  const result: DiffLine[] = [];
  oldMiddle.forEach((text, index) => {
    result.push({ kind: "removed", text, oldLine: offset + index + 1, newLine: null });
  });
  newMiddle.forEach((text, index) => {
    result.push({ kind: "added", text, oldLine: null, newLine: offset + index + 1 });
  });
  return result;
}

/** 改动的行数(增 + 删)。历史列表里那个"改了 N 行"用它。 */
export function changedLineCount(diff: DiffLine[]): number {
  return diff.reduce((count, line) => (line.kind === "context" ? count : count + 1), 0);
}

/** 折叠后的一段:一串要显示的行,或者一个"这里省略了 N 行"的占位。 */
export type DiffSegment =
  | { kind: "lines"; lines: DiffLine[] }
  | { kind: "gap"; hiddenLines: number };

/**
 * 把长段未变的行折叠掉,只在改动附近留 `radius` 行上下文。
 *
 * 只折叠"能省下东西"的段:一段 5 行的未变内容,留 3 行上下文后只剩 2 行可折,
 * 换来一个占位行,反而更长。所以 `hidden` 小于 2 时原样保留。
 */
export function collapseContext(diff: DiffLine[], radius = 3): DiffSegment[] {
  const segments: DiffSegment[] = [];
  let index = 0;
  while (index < diff.length) {
    if (diff[index].kind !== "context") {
      const start = index;
      while (index < diff.length && diff[index].kind !== "context") index += 1;
      segments.push({ kind: "lines", lines: diff.slice(start, index) });
      continue;
    }
    const start = index;
    while (index < diff.length && diff[index].kind === "context") index += 1;
    const run = diff.slice(start, index);
    // 首尾那两段只有一边挨着改动,另一边是文件边界,所以只留一侧的上下文。
    const leadingKept = start === 0 ? 0 : radius;
    const trailingKept = index === diff.length ? 0 : radius;
    const hidden = run.length - leadingKept - trailingKept;
    if (hidden < 2) {
      segments.push({ kind: "lines", lines: run });
      continue;
    }
    if (leadingKept > 0) segments.push({ kind: "lines", lines: run.slice(0, leadingKept) });
    segments.push({ kind: "gap", hiddenLines: hidden });
    if (trailingKept > 0)
      segments.push({ kind: "lines", lines: run.slice(run.length - trailingKept) });
  }
  return segments;
}
