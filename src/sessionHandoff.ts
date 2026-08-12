const ESC = String.fromCharCode(0x1b);
const BEL = String.fromCharCode(0x07);
const C1_CSI = String.fromCharCode(0x9b);
const OSC_SEQUENCE = new RegExp(`${ESC}\\][\\s\\S]*?(?:${BEL}|${ESC}\\\\)`, "g");
const CSI_SEQUENCE = new RegExp(`(?:${ESC}\\[|${C1_CSI})[0-?]*[ -/]*[@-~]`, "g");
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

/**
 * Convert raw PTY history into readable handoff text. Terminal redraws and
 * spinner frames are discarded; consecutive duplicate lines are collapsed.
 */
export function sanitizeTerminalHistoryForHandoff(value: string): string {
  const cleaned = stripTerminalControlSequences(normalizeCarriageReturns(value));
  const lines: string[] = [];
  let previous = "";

  for (const rawLine of cleaned.split("\n")) {
    const line = rawLine.replace(/[ \t]+$/g, "");
    const trimmed = line.trim();
    if (!trimmed) continue;
    // Terminal art and redraw fragments contain no useful words or numbers.
    if (!/[A-Za-z0-9\u3400-\u9fff]/.test(trimmed)) continue;
    if (trimmed === previous) continue;
    lines.push(line);
    previous = trimmed;
  }

  return lines.join("\n");
}
