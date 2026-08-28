// Find inline `$..$` and block `$$..$$` math regions in markdown source.
//
// Used by the CodeMirror WYSIWYG plugin to know which spans to replace with
// a rendered KaTeX widget. Lezer-markdown has no native math node, so we
// scan the doc text directly.
//
// Rules:
//   - Inline:  `$x$`. Single-line. Inner cannot be empty (`$$` is reserved
//              for the block form). `\$` (escaped dollar) does not start/end.
//              No newline inside.
//   - Block:   `$$\n...\n$$` where the `$$` markers are on their own lines
//              (or at start/end of doc). Captures a multi-line range.
//
// Both forms strictly require source positions and do not allocate per-line.

export interface MathRange {
  /** Doc offset of the opening `$` (block: opening `$$`). */
  from: number;
  /** Doc offset just past the closing `$` / `$$`. */
  to: number;
  /** Inner math expression, trimmed of delimiters and whitespace. */
  source: string;
  /** true for `$$...$$` blocks, false for inline `$...$`. */
  display: boolean;
}

const isEscaped = (text: string, idx: number): boolean => {
  let n = 0;
  let i = idx - 1;
  while (i >= 0 && text[i] === "\\") {
    n++;
    i--;
  }
  return n % 2 === 1;
};

const isLineStart = (text: string, idx: number): boolean => idx === 0 || text[idx - 1] === "\n";

/**
 * `$$` block 起点判定：允许前导空白 / tab（列表项里的 block 公式天然带缩进，
 * 比如 ` 4. 列表 + 数学\n\n   $$...$$ `）。inline 的 `$..$` 不走这条路径。
 */
const isBlockOpenerStart = (text: string, idx: number): boolean => {
  let i = idx - 1;
  while (i >= 0) {
    const c = text[i];
    if (c === "\n") return true;
    if (c !== " " && c !== "\t") return false;
    i--;
  }
  return true;
};

/**
 * Detect all math ranges in `text`. Offsets are absolute (not relative to
 * a slice), so pass the full doc for stable positions.
 */
export function detectMathRanges(text: string): MathRange[] {
  const ranges: MathRange[] = [];
  const len = text.length;
  let i = 0;
  while (i < len) {
    const ch = text[i];

    // Skip past fenced code blocks (``` ... ```) — they should never be
    // interpreted as math even if they contain `$`.
    if (ch === "`" && text[i + 1] === "`" && text[i + 2] === "`" && isLineStart(text, i)) {
      const close = text.indexOf("\n```", i + 3);
      if (close < 0) break;
      // advance past the closing fence's newline (or end-of-line)
      const afterFence = text.indexOf("\n", close + 1);
      i = afterFence < 0 ? len : afterFence + 1;
      continue;
    }

    // Skip past inline code (single backticks). These also shadow `$`.
    if (ch === "`" && !isEscaped(text, i)) {
      const close = text.indexOf("`", i + 1);
      if (close < 0) {
        i += 1;
        continue;
      }
      i = close + 1;
      continue;
    }

    if (ch === "$" && !isEscaped(text, i)) {
      // `$$` adjacent: either a well-formed block (own line, paired close) or
      // a non-math `$$x^2$$` inline-ish sequence. In the latter case we must
      // skip the whole `$$` so the inline branch doesn't pick up the second
      // `$` as an opener.
      if (text[i + 1] === "$") {
        if (isBlockOpenerStart(text, i) && isWhitespaceUntilEol(text, i + 2)) {
          const start = i;
          const openEol = text.indexOf("\n", i + 2);
          const innerStart = openEol < 0 ? len : openEol + 1;
          const close = findClosingBlock(text, innerStart);
          if (close !== -1) {
            const innerEnd = close.startOfClose;
            const afterClose = close.endOfClose;
            const inner = text.slice(innerStart, innerEnd).trim();
            if (inner.length > 0) {
              ranges.push({
                from: start,
                to: afterClose,
                source: inner,
                display: true,
              });
              i = afterClose;
              continue;
            }
          }
        }
        // Either not at line start, no proper close, or empty body — skip both `$`.
        i += 2;
        continue;
      }

      // Inline `$...$` on a single line; the closer cannot itself be `$$`.
      //
      // 这里比 Markio 原版多两条判据(pandoc / markdown-it-katex 的标准规则):
      // 开定界符后不能紧跟空白,闭定界符前不能紧跟空白。
      //
      // 为什么加:原版只要求「内容 trim 后非空」,于是 `costs $5 and $9` 会被
      // 当成一个公式(内容 "5 and"),价格在预览里渲染成一坨数学。随手记里写
      // 金额太常见,这个假阳性看得见摸得着。加上这两条后 `$5 and $9` 因为闭
      // 定界符前是空格而被正确拒绝,而 `$E=mc^2$`、`$a_i$` 这类真公式不受影响。
      if (isSpace(text[i + 1])) {
        i += 1;
        continue;
      }
      const lineEnd = text.indexOf("\n", i + 1);
      const searchTo = lineEnd < 0 ? len : lineEnd;
      let j = i + 1;
      let closed = -1;
      while (j < searchTo) {
        if (
          text[j] === "$" &&
          !isEscaped(text, j) &&
          text[j + 1] !== "$" &&
          // 闭定界符前不能是空白
          !isSpace(text[j - 1])
        ) {
          closed = j;
          break;
        }
        j++;
      }
      if (closed > i + 1) {
        const inner = text.slice(i + 1, closed).trim();
        if (inner.length > 0) {
          ranges.push({
            from: i,
            to: closed + 1,
            source: inner,
            display: false,
          });
          i = closed + 1;
          continue;
        }
      }
    }
    i++;
  }
  return ranges;
}

/** 空白判定。`undefined`(越界)按非空白处理,让调用方的边界逻辑保持简单。 */
function isSpace(ch: string | undefined): boolean {
  return ch === " " || ch === "\t" || ch === "\r" || ch === "\n";
}

function isWhitespaceUntilEol(text: string, start: number): boolean {
  for (let i = start; i < text.length; i++) {
    const c = text[i];
    if (c === "\n") return true;
    if (c !== " " && c !== "\t" && c !== "\r") return false;
  }
  return true;
}

interface CloseInfo {
  startOfClose: number;
  endOfClose: number;
}

function findClosingBlock(text: string, from: number): CloseInfo | -1 {
  let line = from;
  while (line < text.length) {
    // Trim leading whitespace on the line
    let probe = line;
    while (probe < text.length && (text[probe] === " " || text[probe] === "\t")) {
      probe++;
    }
    if (text[probe] === "$" && text[probe + 1] === "$" && isWhitespaceUntilEol(text, probe + 2)) {
      const eol = text.indexOf("\n", probe + 2);
      return {
        startOfClose: line,
        endOfClose: eol < 0 ? text.length : eol + 1,
      };
    }
    const nextNl = text.indexOf("\n", line);
    if (nextNl < 0) return -1;
    line = nextNl + 1;
  }
  return -1;
}

/** Inclusive containment test: cursor at the boundary counts as inside. */
export function cursorInsideRange(range: MathRange, head: number): boolean {
  return head >= range.from && head <= range.to;
}
