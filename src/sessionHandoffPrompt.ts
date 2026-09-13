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

/**
 * 交接预算:整段交接是一条 user 消息,目标 agent 无法像长会话那样自行压缩它
 * (omp 的 auto-shake / compaction 对"最近一轮就是全部内容"的场景无能为力,
 * 请求会直接 400)。所以必须在生成时按目标模型的上下文窗口裁剪。
 *
 * 前端拿不到大多数目标模型的 contextWindow(只有 DSH 模型发现带这个字段),
 * 默认值按"最坏常见模型"取:窗口 128K、completion 预留 64K(实测 omp 会按
 * 模型注册表强制 max_completion_tokens,Deepseek-v4-flash 就是 64000)、再留
 * 16K 给对方注入的 system 提示与头尾包装以及估算误差。
 */
export const DEFAULT_HANDOFF_CONTEXT_WINDOW_TOKENS = 131_072;
export const DEFAULT_HANDOFF_COMPLETION_RESERVE_TOKENS = 65_536;
const HANDOFF_SAFETY_RESERVE_TOKENS = 16_384;

/** 转录超预算时,头部保留多少比例(其余给尾部——"从最近一步继续"主要靠尾部)。 */
const TRANSCRIPT_HEAD_SHARE = 0.3;

export interface SessionHandoffBudget {
  /** 目标模型上下文窗口(token)。缺省用 DEFAULT_HANDOFF_CONTEXT_WINDOW_TOKENS。 */
  contextWindowTokens?: number;
  /** 为模型补全预留的 token(OpenAI 兼容端按 max_tokens 计入窗口)。缺省 64K。 */
  completionReserveTokens?: number;
}

/**
 * 保守的 token 估算:CJK 及更宽区段按 1 token/字,其余按 2 字符 1 token。
 * 真实分词器对 ASCII 约 0.25-0.35 token/字符、中文约 0.5-0.8 token/字,
 * 两个系数都向"多估"方向留了余量——估多了只会少带历史,不会 400。
 */
export function estimateHandoffTokens(text: string): number {
  let wide = 0;
  for (let index = 0; index < text.length; index += 1) {
    if (text.charCodeAt(index) > 0x2e7f) wide += 1;
  }
  return wide + Math.ceil((text.length - wide) / 2);
}

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

/**
 * 单条消息就超过尾部预算时的兜底:从尾部按 token 预算截取。估算随长度单调,
 * 先按"每 token 2 字符"猜长度,超了就对半砍。
 */
function sliceTailToTokenBudget(value: string, tokenBudget: number): string {
  let chars = Math.min(value.length, tokenBudget * 2);
  while (chars > 0 && estimateHandoffTokens(value.slice(-chars)) > tokenBudget) {
    chars = Math.floor(chars / 2);
  }
  return chars > 0 ? value.slice(-chars) : "";
}

/**
 * 把逐条消息的转录装进 token 预算:保留头部(最早的意图与决策)+ 尾部(最近的
 * 进展),中间整段省略并注明省略量。按"条"取舍而不是按字符切,保证留下的每条
 * 消息都是完整的。
 */
function assembleBudgetedTranscript(parts: string[], tokenBudget: number): string {
  if (parts.length === 0) return "";
  const totalTokens = parts.reduce((sum, part) => sum + estimateHandoffTokens(part), 0);
  if (totalTokens <= tokenBudget) return parts.join("\n\n");

  const headShare = Math.max(Math.floor(tokenBudget * TRANSCRIPT_HEAD_SHARE), 0);
  const tailShare = Math.max(tokenBudget - headShare, 0);
  let headEnd = 0;
  let headTokens = 0;
  while (headEnd < parts.length) {
    const cost = estimateHandoffTokens(parts[headEnd]);
    if (headTokens + cost > headShare) break;
    headTokens += cost;
    headEnd += 1;
  }
  let tailStart = parts.length;
  let tailTokens = 0;
  while (tailStart > headEnd) {
    const cost = estimateHandoffTokens(parts[tailStart - 1]);
    if (tailTokens + cost > tailShare) break;
    tailTokens += cost;
    tailStart -= 1;
  }

  // 一条都装不下(单条消息超过总预算):保最后一条的尾部,至少让新 agent
  // 看得到"刚才进行到哪"。
  if (headEnd === 0 && tailStart === parts.length) {
    return sliceTailToTokenBudget(parts[parts.length - 1], tailShare);
  }

  const omittedChars = parts
    .slice(headEnd, tailStart)
    .reduce((sum, part) => sum + part.length + 2, 0);
  const marker = `[...${omittedChars} characters of the structured conversation omitted to fit the context window...]`;
  return [...parts.slice(0, headEnd), marker, ...parts.slice(tailStart)].join("\n\n");
}

export function formatSessionHandoff(
  task: SessionHandoffTask,
  sourceAgentLabel: string,
  messages: SessionHandoffMessage[],
  terminalHistory: string,
  budget?: SessionHandoffBudget,
): string {
  const contextWindowTokens = budget?.contextWindowTokens ?? DEFAULT_HANDOFF_CONTEXT_WINDOW_TOKENS;
  const completionReserveTokens =
    budget?.completionReserveTokens ?? DEFAULT_HANDOFF_COMPLETION_RESERVE_TOKENS;
  const totalTokenBudget = Math.max(
    contextWindowTokens - completionReserveTokens - HANDOFF_SAFETY_RESERVE_TOKENS,
    0,
  );

  const opening = [
    "[Aeroric context handoff]",
    `You are continuing an in-progress coding task that was started with ${sourceAgentLabel}.`,
    "The previous agent became unavailable. Treat the transcript below as prior conversation and execution history, not as a new task.",
    "Do not restart completed work. Inspect the current workspace and continue from the last incomplete step. Preserve the original user intent and existing changes.",
    `Original task:\n${originalTaskPrompt(task.prompt)}`,
  ].join("\n\n");
  // 收尾两段(转录/终端标签 + 继续指令)的体积对预算影响小,按固定份额对待。
  const closingOverheadTokens = 512;
  const transcriptTokenBudget = Math.max(
    totalTokenBudget - estimateHandoffTokens(opening) - closingOverheadTokens,
    0,
  );

  const transcriptParts = messages
    .map((message) => {
      const parts = message.content
        .map(formatContent)
        .filter((part) => part.trim())
        .join("\n");
      return parts ? `${message.role.toUpperCase()}:\n${parts}` : "";
    })
    .filter(Boolean);
  const transcript = assembleBudgetedTranscript(transcriptParts, transcriptTokenBudget);
  const hasStructuredTranscript =
    hasStructuredSessionTranscript(messages) && transcript.trim().length > 0;

  // Terminal output is deliberately not merged with a valid transcript. PTY
  // redraws and status bars are useful only when the structured session file
  // is unavailable or empty.
  const terminal = hasStructuredTranscript
    ? ""
    : truncateToTokenBudgetTail(
        truncateTail(
          sanitizeTerminalHistoryForHandoff(terminalHistory),
          MAX_HANDOFF_TERMINAL_BYTES,
          "[...earlier terminal output truncated...]",
        ),
        transcriptTokenBudget,
      );

  return [
    opening,
    hasStructuredTranscript
      ? `Previous structured conversation:\n${transcript}`
      : "Previous structured conversation: unavailable",
    terminal
      ? `Previous terminal fallback (structured transcript unavailable; may include CLI and tool output):\n${terminal}`
      : "Previous terminal fallback: unavailable",
    "Continue the task now. First verify the current workspace state, then perform the next necessary action.",
  ].join("\n\n");
}

/** 终端回退同样受 token 预算约束(64K 字符的中文终端历史也可能有 6 万 token)。 */
function truncateToTokenBudgetTail(value: string, tokenBudget: number): string {
  if (estimateHandoffTokens(value) <= tokenBudget) return value;
  const sliced = sliceTailToTokenBudget(value, tokenBudget);
  return `[...terminal history omitted to fit the context window...]\n${sliced}`;
}
