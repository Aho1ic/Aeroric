const ESC = String.fromCharCode(0x1b);
const BEL = String.fromCharCode(0x07);
const C1_CSI = String.fromCharCode(0x9b);
const OSC_SEQUENCE = new RegExp(`${ESC}\\][\\s\\S]*?(?:${BEL}|${ESC}\\\\)`, "g");
const CSI_SEQUENCE = new RegExp(`(?:${ESC}\\[|${C1_CSI})[0-?]*[ -/]*[@-~]`, "g");
const CSI_SEQUENCE_PARTS = new RegExp(`(?:${ESC}\\[|${C1_CSI})[0-?]*[ -/]*([@-~])`, "g");
const ESC_SEQUENCE = new RegExp(`${ESC}(?:[()][0-2A-Z]|[ -/]*[@-Z\\\\-_])`, "g");
const C0_CONTROL = new RegExp(
  `[${String.fromCharCode(0)}-${String.fromCharCode(8)}${String.fromCharCode(11)}${String.fromCharCode(12)}${String.fromCharCode(14)}-${String.fromCharCode(31)}${String.fromCharCode(127)}]`,
  "g",
);
const ZERO_WIDTH = /[\u200b-\u200d\ufeff]/g;

/**
 * Remove terminal protocol bytes without changing ordinary text content.
 * Handoff prompts are pasted into another agent's PTY, so embedded escape
 * sequences must never reach that PTY as executable control input.
 */
export function stripTerminalControlSequences(value: string): string {
  const stripped = value
    .replace(OSC_SEQUENCE, "")
    .replace(CSI_SEQUENCE, "")
    .replace(ESC_SEQUENCE, "")
    .replace(C0_CONTROL, "")
    .replace(ZERO_WIDTH, "");
  return normalizeCarriageReturns(stripped);
}

function normalizeCarriageReturns(value: string): string {
  return value
    .replace(/\r\n/g, "\n")
    .split("\n")
    .map((line) => {
      const segments = line.split("\r");
      return segments[segments.length - 1] ?? "";
    })
    .join("\n");
}

// CSI final bytes that move the cursor off the current row or erase part of the
// screen. Agent TUIs redraw their spinner and status bar with these instead of
// writing \r or \n, so each redraw frame has to become its own line before the
// escape bytes are dropped. Without this split every frame concatenates into one
// giant line such as "WWo•Wor•WorkWorki•Workin•Working".
const LINE_BREAKING_CSI_FINALS = new Set([
  "A", // cursor up
  "B", // cursor down
  "E", // cursor next line
  "F", // cursor previous line
  "H", // cursor position
  "f", // horizontal/vertical position
  "d", // line position absolute
  "J", // erase in display
  "K", // erase in line
  "L", // insert line
  "M", // delete line
  "S", // scroll up
  "T", // scroll down
]);

function splitTerminalRedrawFrames(value: string): string {
  return value
    .replace(OSC_SEQUENCE, "")
    .replace(CSI_SEQUENCE_PARTS, (_match, final: string) =>
      LINE_BREAKING_CSI_FINALS.has(final) ? "\n" : "",
    );
}

// Partial renders of the spinner labels. A redraw frame can start or end mid
// word ("orking", "Workin"), so any line made only of such fragments is noise.
const SPINNER_LABELS = [
  "Working",
  "Reconnecting",
  "Exploring",
  "Explored",
  "Thinking",
  "Waiting",
  "Running",
  "Ran",
];
const SPINNER_FRAGMENTS = new Set<string>();
for (const label of SPINNER_LABELS) {
  for (let start = 0; start < label.length; start += 1) {
    for (let end = start + 1; end <= label.length; end += 1) {
      SPINNER_FRAGMENTS.add(label.slice(start, end));
    }
  }
}

function isSpinnerFrame(line: string): boolean {
  const words = line.split(/[^A-Za-z]+/).filter(Boolean);
  // No latin words at all means this cannot be a partial spinner label. CJK rows
  // and bare numeric output must survive.
  if (!words.length) return false;
  return words.every((word) => SPINNER_FRAGMENTS.has(word));
}

// The status bar is re-rendered on every frame and carries no task context.
const STATUS_BAR_MARKERS = ["esc to interrupt", "esc to view transcript"];

// Redraw frames repeat the same rows many times but not necessarily back to
// back, so exact-previous-line dedupe cannot remove them.
const DEDUPE_WINDOW_LINES = 400;

/**
 * Convert raw PTY history into readable handoff text. Terminal redraws, spinner
 * frames and status bars are discarded; recently seen lines are collapsed.
 */
export function sanitizeTerminalHistoryForHandoff(value: string): string {
  const cleaned = normalizeCarriageReturns(
    splitTerminalRedrawFrames(value)
      .replace(ESC_SEQUENCE, "")
      .replace(C0_CONTROL, "")
      .replace(ZERO_WIDTH, ""),
  );
  const lines: string[] = [];
  const seen = new Set<string>();
  const order: string[] = [];

  for (const rawLine of cleaned.split("\n")) {
    const line = rawLine.replace(/[ \t]+$/g, "");
    const trimmed = line.trim();
    if (!trimmed) continue;
    // Terminal art and redraw fragments contain no useful words or numbers.
    if (!/[A-Za-z0-9㐀-鿿]/.test(trimmed)) continue;
    if (STATUS_BAR_MARKERS.some((marker) => trimmed.includes(marker))) continue;
    if (isSpinnerFrame(trimmed)) continue;
    if (seen.has(trimmed)) continue;
    seen.add(trimmed);
    order.push(trimmed);
    if (order.length > DEDUPE_WINDOW_LINES) {
      const evicted = order.shift();
      if (evicted !== undefined) seen.delete(evicted);
    }
    lines.push(line);
  }

  return lines.join("\n");
}
