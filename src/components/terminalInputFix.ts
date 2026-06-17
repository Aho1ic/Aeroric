import type { Terminal } from "@xterm/xterm";
import { IS_MAC_WEBKIT, IS_OTHER_WEBKIT } from "../platform";

type TerminalWithInput = Pick<Terminal, "input" | "textarea">;

export const POST_COMPOSITION_REPLAY_IGNORE_MS = 1200;

function getPrintableSymbolInput(data: string | null): string | null {
  if (data === null || data.length === 0) return null;
  if (data.length > 8) return null;
  if (!/^[\p{P}\p{S}]+$/u.test(data)) return null;
  return data;
}

function isSymbolInputType(inputType: string): boolean {
  return inputType === "insertText" || inputType === "insertCompositionText";
}

export function normalizeCommittedCompositionText(text: string): string {
  const mixedChineseWithPinyin = text.match(/^([\p{Script=Han}]+)([A-Za-z][A-Za-z']+)$/u);
  if (mixedChineseWithPinyin && mixedChineseWithPinyin[2].includes("'")) {
    return mixedChineseWithPinyin[1];
  }

  const apostropheCount = (text.match(/'/g) ?? []).length;
  const normalized = text.replace(/(?<=[A-Za-z])'(?=[A-Za-z])/g, "");
  if (apostropheCount === 1) {
    for (let size = 1; size <= Math.floor(normalized.length / 2); size += 1) {
      if (normalized.length % size !== 0) continue;
      const unit = normalized.slice(0, size);
      if (unit && unit.repeat(normalized.length / size) === normalized) return unit;
    }
  }
  return normalized;
}

function addIgnoredCandidate(candidates: Set<string>, text: string | null | undefined): void {
  if (!text) return;
  candidates.add(text);
  const normalized = normalizeCommittedCompositionText(text);
  if (normalized) candidates.add(normalized);
}

export function buildPostCompositionIgnoredCandidates(
  committedText: string | null | undefined,
  preeditText: string | null | undefined,
): Set<string> {
  const candidates = new Set<string>();
  addIgnoredCandidate(candidates, committedText);
  addIgnoredCandidate(candidates, preeditText);
  return candidates;
}

export function shouldIgnorePostCompositionCandidate(
  text: string | null | undefined,
  candidates: ReadonlySet<string>,
): boolean {
  if (!text || candidates.size === 0) return false;
  const normalized = normalizeCommittedCompositionText(text);
  if (candidates.has(text) || candidates.has(normalized)) return true;

  for (const candidate of candidates) {
    if (
      text.startsWith(candidate) &&
      shouldIgnorePostCompositionCandidate(text.slice(candidate.length), candidates)
    ) {
      return true;
    }
    if (shouldIgnorePostCompositionInsert(text, candidate, normalizeCommittedCompositionText(candidate))) {
      return true;
    }
  }
  return false;
}

export function shouldIgnorePostCompositionInsert(
  text: string | null | undefined,
  ignoredText: string | null,
  ignoredNormalized: string | null,
): boolean {
  if (!text || (!ignoredText && !ignoredNormalized)) return false;
  const normalized = normalizeCommittedCompositionText(text);
  return (
    text === ignoredText ||
    text === ignoredNormalized ||
    normalized === ignoredText ||
    normalized === ignoredNormalized
  );
}

export function shouldSuppressBrowserCompositionPreview(
  inputType: string,
  isComposing: boolean,
): boolean {
  return isComposing && (inputType === "insertCompositionText" || inputType === "insertText");
}

export function attachMacWebKitShiftInputFix(term: TerminalWithInput): () => void {
  if (!IS_MAC_WEBKIT || !term.textarea) return () => {};

  const textarea = term.textarea;
  let keydownHandledByXterm: string | null = null;

  const handleKeyDown = (event: KeyboardEvent) => {
    keydownHandledByXterm = null;
    if (
      event.keyCode !== 229 &&
      event.shiftKey &&
      !event.ctrlKey &&
      !event.altKey &&
      !event.metaKey &&
      getPrintableSymbolInput(event.key) !== null
    ) {
      keydownHandledByXterm = event.key;
    }
  };

  const handleBeforeInput = (event: InputEvent) => {
    const symbol = getPrintableSymbolInput(event.data);
    if (!isSymbolInputType(event.inputType) || symbol === null) {
      return;
    }
    if (keydownHandledByXterm === symbol) {
      keydownHandledByXterm = null;
      return;
    }
    term.input(symbol);
    event.preventDefault();
  };

  textarea.addEventListener("keydown", handleKeyDown);
  textarea.addEventListener("beforeinput", handleBeforeInput);

  return () => {
    textarea.removeEventListener("keydown", handleKeyDown);
    textarea.removeEventListener("beforeinput", handleBeforeInput);
  };
}

export function attachLinuxIMEFix(
  term: Terminal,
  onDataCallback: (data: string) => void,
): { dispose: () => void } {
  if (!(IS_OTHER_WEBKIT || IS_MAC_WEBKIT) || !term.textarea) {
    const disposable = term.onData(onDataCallback);
    return { dispose: () => disposable.dispose() };
  }

  const textarea = term.textarea;
  let isComposing = false;
  let compositionText = "";
  let ignoredPostCompositionCandidates = new Set<string>();
  let ignorePostCompositionUntil = 0;

  const clearTextarea = () => {
    textarea.value = "";
    try {
      textarea.setSelectionRange(0, 0);
    } catch {
      // Some WebKit IME states reject selection changes while committing.
    }
  };

  const clearTextareaNowAndNextFrame = () => {
    clearTextarea();
    const schedule =
      typeof window !== "undefined" && typeof window.requestAnimationFrame === "function"
        ? window.requestAnimationFrame.bind(window)
        : (callback: FrameRequestCallback) =>
            globalThis.setTimeout(() => callback(performance.now()), 0);
    schedule(() => clearTextarea());
  };

  const sendText = (text: string | null | undefined) => {
    if (!text) return;
    onDataCallback(normalizeCommittedCompositionText(text));
  };

  const handleCompositionStartCapture = (event: CompositionEvent) => {
    isComposing = true;
    compositionText = "";
    clearTextareaNowAndNextFrame();
    void event;
  };

  const handleCompositionUpdateCapture = (event: CompositionEvent) => {
    compositionText = event.data ?? "";
    clearTextareaNowAndNextFrame();
  };

  const handleCompositionEndCapture = (event: CompositionEvent) => {
    const preeditText = compositionText;
    const text = event.data || preeditText;
    isComposing = false;
    compositionText = "";
    const normalized = normalizeCommittedCompositionText(text);
    ignoredPostCompositionCandidates = buildPostCompositionIgnoredCandidates(text, preeditText);
    ignorePostCompositionUntil = performance.now() + POST_COMPOSITION_REPLAY_IGNORE_MS;
    clearTextareaNowAndNextFrame();
    event.stopImmediatePropagation();
    sendText(normalized);
  };

  const handleBeforeInputCapture = (event: InputEvent) => {
    if (
      (event.inputType === "insertText" || event.inputType === "insertCompositionText") &&
      event.data &&
      performance.now() <= ignorePostCompositionUntil &&
      shouldIgnorePostCompositionCandidate(event.data, ignoredPostCompositionCandidates)
    ) {
      clearTextareaNowAndNextFrame();
      event.preventDefault();
      event.stopImmediatePropagation();
      ignoredPostCompositionCandidates = new Set<string>();
      return;
    }

    if (shouldSuppressBrowserCompositionPreview(event.inputType, isComposing)) {
      compositionText = event.data ?? compositionText;
      clearTextareaNowAndNextFrame();
      event.preventDefault();
      event.stopImmediatePropagation();
      return;
    }

    if (event.inputType === "insertCompositionText") {
      compositionText = event.data ?? compositionText;
      clearTextareaNowAndNextFrame();
      event.preventDefault();
      event.stopImmediatePropagation();
      return;
    }

    const symbol = getPrintableSymbolInput(event.data);
    if (symbol !== null && isSymbolInputType(event.inputType)) {
      clearTextareaNowAndNextFrame();
      event.preventDefault();
      event.stopImmediatePropagation();
      sendText(symbol);
      return;
    }

    if (isComposing) {
      clearTextareaNowAndNextFrame();
      event.preventDefault();
      event.stopImmediatePropagation();
    }
  };

  const handleInputCapture = (event: Event) => {
    if (isComposing || performance.now() <= ignorePostCompositionUntil) {
      clearTextareaNowAndNextFrame();
      event.stopImmediatePropagation();
    }
  };

  const handleKeyDownCapture = (event: KeyboardEvent) => {
    if (!isComposing && event.keyCode === 229) event.stopImmediatePropagation();
  };

  const handleTerminalData = (data: string) => {
    if (isComposing && /^[\p{L}\p{N}'`]+$/u.test(data)) {
      return;
    }
    if (
      performance.now() <= ignorePostCompositionUntil &&
      shouldIgnorePostCompositionCandidate(data, ignoredPostCompositionCandidates)
    ) {
      ignoredPostCompositionCandidates = new Set<string>();
      return;
    }
    onDataCallback(data);
  };

  const disposable = term.onData(handleTerminalData);

  textarea.addEventListener("compositionstart", handleCompositionStartCapture, true);
  textarea.addEventListener("compositionupdate", handleCompositionUpdateCapture, true);
  textarea.addEventListener("compositionend", handleCompositionEndCapture, true);
  textarea.addEventListener("beforeinput", handleBeforeInputCapture, true);
  textarea.addEventListener("input", handleInputCapture, true);
  textarea.addEventListener("keydown", handleKeyDownCapture, true);

  return {
    dispose: () => {
      textarea.removeEventListener("compositionstart", handleCompositionStartCapture, true);
      textarea.removeEventListener("compositionupdate", handleCompositionUpdateCapture, true);
      textarea.removeEventListener("compositionend", handleCompositionEndCapture, true);
      textarea.removeEventListener("beforeinput", handleBeforeInputCapture, true);
      textarea.removeEventListener("input", handleInputCapture, true);
      textarea.removeEventListener("keydown", handleKeyDownCapture, true);
      disposable.dispose();
    },
  };
}
