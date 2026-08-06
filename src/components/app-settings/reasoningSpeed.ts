export type ModelReasoningSpeed = "standard" | "fast";

export const MODEL_REASONING_SPEEDS: readonly ModelReasoningSpeed[] = ["standard", "fast"];

const MODEL_REASONING_SPEED_LINE =
  /^([ \t]*)model_reasoning_speed[ \t]*=[ \t]*"([^"\r\n]*)"([^\r\n]*)$/m;
const TOML_TABLE_HEADER_LINE = /^[ \t]*\[[^\r\n]+\][ \t]*(?:#[^\r\n]*)?$/m;

function findRootReasoningSpeedLine(content: string): {
  index: number;
  text: string;
  indentation: string;
  value: string;
  suffix: string;
} | null {
  const tableHeader = content.match(TOML_TABLE_HEADER_LINE);
  const rootContent = content.slice(0, tableHeader?.index ?? content.length);
  const match = rootContent.match(MODEL_REASONING_SPEED_LINE);
  if (!match || match.index === undefined) return null;
  return {
    index: match.index,
    text: match[0],
    indentation: match[1],
    value: match[2],
    suffix: match[3],
  };
}

function normalizeSpeedValue(value: string | undefined): ModelReasoningSpeed | null {
  if (!value) return null;
  const normalized = value.toLocaleLowerCase();
  return MODEL_REASONING_SPEEDS.includes(normalized as ModelReasoningSpeed)
    ? (normalized as ModelReasoningSpeed)
    : null;
}

function readJsonReasoningSpeed(content: string): ModelReasoningSpeed | null {
  const trimmed = content.trim();
  if (!trimmed.startsWith("{")) return null;
  try {
    const parsed = JSON.parse(trimmed) as Record<string, unknown>;
    const value = parsed.model_reasoning_speed;
    if (typeof value !== "string") return null;
    return normalizeSpeedValue(value);
  } catch {
    return null;
  }
}

export function readModelReasoningSpeed(content: string): ModelReasoningSpeed | null {
  const line = findRootReasoningSpeedLine(content)?.value;
  const speed = normalizeSpeedValue(line ?? undefined);
  if (speed) return speed;
  return readJsonReasoningSpeed(content);
}

export function setModelReasoningSpeed(content: string, speed: ModelReasoningSpeed | null): string {
  const existing = findRootReasoningSpeedLine(content);
  if (existing) {
    if (speed === null) {
      return `${content.slice(0, existing.index)}${content.slice(existing.index + existing.text.length)}`
        .replace(/^\r?\n/, "")
        .replace(/\r?\n{3,}/g, "\n\n");
    }
    const replacement = `${existing.indentation}model_reasoning_speed = "${speed}"${existing.suffix}`;
    return `${content.slice(0, existing.index)}${replacement}${content.slice(
      existing.index + existing.text.length,
    )}`;
  }

  const trimmed = content.trim();
  if (trimmed.startsWith("{")) {
    try {
      const parsed = JSON.parse(trimmed) as Record<string, unknown>;
      if (speed === null) {
        if ("model_reasoning_speed" in parsed) {
          delete parsed.model_reasoning_speed;
        }
        return `${JSON.stringify(parsed, null, 2)}\n`;
      }
      const next: Record<string, unknown> = { ...parsed, model_reasoning_speed: speed };
      return `${JSON.stringify(next, null, 2)}\n`;
    } catch {
      // fall through to TOML-style handling
    }
  }

  if (speed === null) return content;
  const prefix = `model_reasoning_speed = "${speed}"`;
  return trimmed.length === 0 ? `${prefix}\n` : `${prefix}\n${content}`;
}
