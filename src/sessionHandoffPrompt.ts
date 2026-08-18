import { sanitizeTerminalHistoryForHandoff, stripTerminalControlSequences } from "./sessionHandoff";

export interface SessionHandoffContent {
  type: "text" | "tool_use" | "tool_result" | "thinking";
  text?: string;
  id?: string;
  name?: string;
  input?: string;
  output?: string;
  thinking?: string;
}

export interface SessionHandoffMessage {
  role: "user" | "assistant";
  content: SessionHandoffContent[];
}

export interface SessionHandoffTask {
  prompt: string;
}

const MAX_HANDOFF_TERMINAL_BYTES = 64 * 1024;
const MAX_HANDOFF_TRANSCRIPT_BYTES = 512 * 1024;

/** Remove wrappers produced by an earlier config switch before creating a new one. */
export function originalTaskPrompt(prompt: string): string {
  let current = stripTerminalControlSequences(prompt).trim();
  for (let attempt = 0; attempt < 8; attempt += 1) {
    if (!current.startsWith("[Aeroric context handoff]")) return current;
    const match = current.match(
      /^\[Aeroric context handoff\][\s\S]*?\nOriginal task:\n([\s\S]*?)(?=\n\nPrevious structured conversation:|\n\nPrevious terminal (?:fallback|output):|\n\nContinue the task now\.|$)/,
    );
    if (!match) return current;
    current = match[1].trim();
  }
  return current;
}

function formatContent(content: SessionHandoffContent): string {
  if (content.type === "text") return stripTerminalControlSequences(content.text ?? "");
  if (content.type === "thinking") {
    return `[thinking]\n${stripTerminalControlSequences(content.thinking ?? "")}`;
  }
  if (content.type === "tool_result") {
    return `[tool result ${content.id ? `(${content.id})` : ""}]\n${stripTerminalControlSequences(content.output ?? "")}`;
  }
  return `[tool ${content.name ?? "unknown"} ${content.id ? `(${content.id})` : ""}]\n${stripTerminalControlSequences(content.input ?? "")}`;
}

function contentHasPayload(content: SessionHandoffContent): boolean {
  switch (content.type) {
    case "text":
      return Boolean(stripTerminalControlSequences(content.text ?? "").trim());
    case "thinking":
      return Boolean(stripTerminalControlSequences(content.thinking ?? "").trim());
    case "tool_result":
      return Boolean(stripTerminalControlSequences(content.output ?? "").trim() || content.id);
    case "tool_use":
      return Boolean(
        stripTerminalControlSequences(content.input ?? "").trim() || content.name || content.id,
      );
  }
}

export function hasStructuredSessionTranscript(messages: SessionHandoffMessage[]): boolean {
  return messages.some((message) => message.content.some(contentHasPayload));
}

function truncateTail(value: string, maxBytes: number, marker: string): string {
  if (value.length <= maxBytes) return value;
  const tail = value.slice(-maxBytes);
  const firstNewline = tail.indexOf("\n");
  return `${marker}\n${tail.slice(firstNewline >= 0 ? firstNewline : 0)}`;
}

export function formatSessionHandoff(
  task: SessionHandoffTask,
  sourceAgentLabel: string,
  messages: SessionHandoffMessage[],
  terminalHistory: string,
): string {
  const transcript = truncateTail(
    messages
      .map((message) => {
        const parts = message.content
          .map(formatContent)
          .filter((part) => part.trim())
          .join("\n");
        return parts ? `${message.role.toUpperCase()}:\n${parts}` : "";
      })
      .filter(Boolean)
      .join("\n\n"),
    MAX_HANDOFF_TRANSCRIPT_BYTES,
    "[...earlier conversation truncated...]",
  );
  const hasStructuredTranscript =
    hasStructuredSessionTranscript(messages) && transcript.trim().length > 0;

  // Terminal output is deliberately not merged with a valid transcript. PTY
  // redraws and status bars are useful only when the structured session file
  // is unavailable or empty.
  const terminal = hasStructuredTranscript
    ? ""
    : truncateTail(
        sanitizeTerminalHistoryForHandoff(terminalHistory),
        MAX_HANDOFF_TERMINAL_BYTES,
        "[...earlier terminal output truncated...]",
      );

  return [
    "[Aeroric context handoff]",
    `You are continuing an in-progress coding task that was started with ${sourceAgentLabel}.`,
    "The previous agent became unavailable. Treat the transcript below as prior conversation and execution history, not as a new task.",
    "Do not restart completed work. Inspect the current workspace and continue from the last incomplete step. Preserve the original user intent and existing changes.",
    `Original task:\n${originalTaskPrompt(task.prompt)}`,
    hasStructuredTranscript
      ? `Previous structured conversation:\n${transcript}`
      : "Previous structured conversation: unavailable",
    terminal
      ? `Previous terminal fallback (structured transcript unavailable; may include CLI and tool output):\n${terminal}`
      : "Previous terminal fallback: unavailable",
    "Continue the task now. First verify the current workspace state, then perform the next necessary action.",
  ].join("\n\n");
}
