export const MODEL_REASONING_EFFORTS = ["minimal", "low", "medium", "high", "xhigh"] as const;

export type ModelReasoningEffort = (typeof MODEL_REASONING_EFFORTS)[number];

const MODEL_REASONING_EFFORT_LINE =
  /^([ \t]*)model_reasoning_effort[ \t]*=[ \t]*"([^"\r\n]*)"([^\r\n]*)$/m;
const TOML_TABLE_HEADER_LINE = /^[ \t]*\[[^\r\n]+\][ \t]*(?:#[^\r\n]*)?$/m;

function findRootReasoningEffortLine(content: string): {
  index: number;
  text: string;
  indentation: string;
  value: string;
  suffix: string;
} | null {
  const tableHeader = content.match(TOML_TABLE_HEADER_LINE);
  const rootContent = content.slice(0, tableHeader?.index ?? content.length);
  const match = rootContent.match(MODEL_REASONING_EFFORT_LINE);
  if (!match || match.index === undefined) return null;
  return {
    index: match.index,
    text: match[0],
    indentation: match[1],
    value: match[2],
    suffix: match[3],
  };
}

export function readModelReasoningEffort(content: string): ModelReasoningEffort | null {
  const value = findRootReasoningEffortLine(content)?.value;
  return MODEL_REASONING_EFFORTS.includes(value as ModelReasoningEffort)
    ? (value as ModelReasoningEffort)
    : null;
}

export function setModelReasoningEffort(
  content: string,
  effort: ModelReasoningEffort | null,
): string {
  const existing = findRootReasoningEffortLine(content);
  if (existing) {
    if (effort === null) {
      return `${content.slice(0, existing.index)}${content.slice(existing.index + existing.text.length)}`
        .replace(/^\r?\n/, "")
        .replace(/\r?\n{3,}/g, "\n\n");
    }
    const replacement = `${existing.indentation}model_reasoning_effort = "${effort}"${existing.suffix}`;
    return `${content.slice(0, existing.index)}${replacement}${content.slice(
      existing.index + existing.text.length,
    )}`;
  }

  if (effort === null) return content;
  const prefix = `model_reasoning_effort = "${effort}"`;
  return content.trim().length === 0 ? `${prefix}\n` : `${prefix}\n${content}`;
}
