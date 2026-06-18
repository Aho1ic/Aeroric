import type { Terminal } from "@xterm/xterm";
import { IS_MAC_WEBKIT, IS_OTHER_WEBKIT } from "../platform";

type TerminalWithInput = Pick<Terminal, "input" | "textarea">;

export const POST_COMPOSITION_REPLAY_IGNORE_MS = 3000;

export function applyTerminalTextareaInputAttributes(term: { textarea?: HTMLTextAreaElement | null }): void {
  if (!term.textarea) return;
  term.textarea.setAttribute("autocomplete", "off");
  term.textarea.setAttribute("autocorrect", "off");
  term.textarea.setAttribute("autocapitalize", "off");
  term.textarea.setAttribute("spellcheck", "false");
  term.textarea.removeAttribute("inputmode");
}

function getPrintableSymbolInput(data: string | null): string | null {
  if (data === null || data.length === 0) return null;
  if (data.length > 8) return null;
  if (!/^[\p{P}\p{S}]+$/u.test(data)) return null;
  return data;
}

function isSymbolInputType(inputType: string): boolean {
  return inputType === "insertText" || inputType === "insertCompositionText";
}

function isTextInsertInputType(inputType: string): boolean {
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

function removeMatchedPostCompositionCandidates(
  candidates: ReadonlySet<string>,
  text: string,
): Set<string> {
  const next = new Set<string>();
  for (const candidate of candidates) {
    if (!shouldIgnorePostCompositionInsert(text, candidate, normalizeCommittedCompositionText(candidate))) {
      next.add(candidate);
    }
  }
  return next;
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

function shouldIgnoreReplayByCharacters(text: string, candidates: ReadonlySet<string>): boolean {
  if (text.length === 0) return true;
  for (let index = 1; index <= text.length; index += 1) {
    const prefix = text.slice(0, index);
    const normalizedPrefix = normalizeCommittedCompositionText(prefix);
    if (!candidates.has(prefix) && !candidates.has(normalizedPrefix)) {
      continue;
    }
    if (shouldIgnoreReplayByCharacters(text.slice(index), candidates)) {
      return true;
    }
  }
  return false;
}

export function isPostCompositionReplayPrefix(
  text: string | null | undefined,
  candidates: ReadonlySet<string>,
): boolean {
  if (!text || candidates.size === 0) return false;
  const normalized = normalizeCommittedCompositionText(text);
  for (const candidate of candidates) {
    if (candidate.startsWith(text) || candidate.startsWith(normalized)) return true;
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
  return isComposing && inputType === "insertCompositionText";
}

export function shouldDeferRomanizedCompositionCommit(
  text: string | null | undefined,
  preeditText: string | null | undefined,
): boolean {
  const value = text?.trim();
  if (!value) return false;
  if (/[\p{Script=Han}]/u.test(value)) return false;
  if (!/^[A-Za-z][A-Za-z'`\s]*$/u.test(value)) return false;
  const normalized = normalizeCommittedCompositionText(value);
  const normalizedPreedit = normalizeCommittedCompositionText(preeditText ?? "");
  return normalized === normalizedPreedit || normalizedPreedit.length === 0;
}

function containsCommittedCjkText(text: string | null | undefined): boolean {
  return Boolean(text && /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u.test(text));
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
  let ignoredReplayProgress = "";
  let ignoredPostCompositionCandidates = new Set<string>();
  let ignorePostCompositionUntil = 0;
  let pendingCompositionCommit:
    | {
        text: string;
        preeditText: string;
        timer: ReturnType<typeof globalThis.setTimeout>;
      }
    | null = null;

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

  const clearPendingCompositionCommit = () => {
    if (!pendingCompositionCommit) return;
    globalThis.clearTimeout(pendingCompositionCommit.timer);
    pendingCompositionCommit = null;
  };

  const commitCompositionText = (text: string, preeditText: string) => {
    const normalized = normalizeCommittedCompositionText(text);
    ignoredReplayProgress = "";
    ignoredPostCompositionCandidates = buildPostCompositionIgnoredCandidates(text, preeditText);
    ignorePostCompositionUntil = performance.now() + POST_COMPOSITION_REPLAY_IGNORE_MS;
    clearTextareaNowAndNextFrame();
    sendText(normalized);
  };

  const deferCompositionCommit = (text: string, preeditText: string) => {
    clearPendingCompositionCommit();
    pendingCompositionCommit = {
      text,
      preeditText,
      timer: globalThis.setTimeout(() => {
        const pending = pendingCompositionCommit;
        pendingCompositionCommit = null;
        if (!pending) return;
        commitCompositionText(pending.text, pending.preeditText);
      }, 30),
    };
    ignoredReplayProgress = "";
    ignoredPostCompositionCandidates = buildPostCompositionIgnoredCandidates(text, preeditText);
    ignorePostCompositionUntil = performance.now() + POST_COMPOSITION_REPLAY_IGNORE_MS;
    clearTextareaNowAndNextFrame();
  };

  const handleCompositionStartCapture = (event: CompositionEvent) => {
    clearPendingCompositionCommit();
    isComposing = true;
    compositionText = "";
    ignoredReplayProgress = "";
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
    if (
      performance.now() <= ignorePostCompositionUntil &&
      (shouldIgnorePostCompositionCandidate(text, ignoredPostCompositionCandidates) ||
        shouldIgnoreReplayByCharacters(text, ignoredPostCompositionCandidates))
    ) {
      isComposing = false;
      compositionText = "";
      clearTextareaNowAndNextFrame();
      event.stopImmediatePropagation();
      return;
    }
    isComposing = false;
    compositionText = "";
    event.stopImmediatePropagation();
    if (shouldDeferRomanizedCompositionCommit(text, preeditText)) {
      deferCompositionCommit(text, preeditText);
      return;
    }
    commitCompositionText(text, preeditText);
  };

  const handleBeforeInputCapture = (event: InputEvent) => {
    if (
      event.data &&
      isTextInsertInputType(event.inputType) &&
      performance.now() <= ignorePostCompositionUntil &&
      isPostCompositionReplayPrefix(ignoredReplayProgress + event.data, ignoredPostCompositionCandidates)
    ) {
      ignoredReplayProgress += event.data;
      if (shouldIgnorePostCompositionCandidate(ignoredReplayProgress, ignoredPostCompositionCandidates)) {
        ignoredPostCompositionCandidates = removeMatchedPostCompositionCandidates(
          ignoredPostCompositionCandidates,
          ignoredReplayProgress,
        );
        ignoredReplayProgress = "";
      }
      clearTextareaNowAndNextFrame();
      event.preventDefault();
      event.stopImmediatePropagation();
      return;
    }

    if (
      pendingCompositionCommit &&
      isTextInsertInputType(event.inputType) &&
      event.data
    ) {
      const pending = pendingCompositionCommit;
      clearPendingCompositionCommit();
      ignoredPostCompositionCandidates = buildPostCompositionIgnoredCandidates(
        event.data,
        pending.preeditText || pending.text,
      );
      ignorePostCompositionUntil = performance.now() + POST_COMPOSITION_REPLAY_IGNORE_MS;
      clearTextareaNowAndNextFrame();
      event.preventDefault();
      event.stopImmediatePropagation();
      sendText(event.data);
      return;
    }

    if (
      isTextInsertInputType(event.inputType) &&
      event.data &&
      performance.now() <= ignorePostCompositionUntil &&
      (shouldIgnorePostCompositionCandidate(event.data, ignoredPostCompositionCandidates) ||
        shouldIgnoreReplayByCharacters(event.data, ignoredPostCompositionCandidates))
    ) {
      clearTextareaNowAndNextFrame();
      event.preventDefault();
      event.stopImmediatePropagation();
      ignoredReplayProgress = "";
      ignoredPostCompositionCandidates = removeMatchedPostCompositionCandidates(
        ignoredPostCompositionCandidates,
        event.data,
      );
      return;
    }

    if (
      isComposing &&
      event.inputType === "insertText" &&
      containsCommittedCjkText(event.data)
    ) {
      const preeditText = compositionText;
      isComposing = false;
      compositionText = "";
      clearPendingCompositionCommit();
      ignoredReplayProgress = "";
      ignoredPostCompositionCandidates = buildPostCompositionIgnoredCandidates(event.data, preeditText);
      ignorePostCompositionUntil = performance.now() + POST_COMPOSITION_REPLAY_IGNORE_MS;
      clearTextareaNowAndNextFrame();
      event.preventDefault();
      event.stopImmediatePropagation();
      sendText(event.data);
      return;
    }

    if (
      isComposing &&
      event.inputType === "insertText" &&
      shouldDeferRomanizedCompositionCommit(event.data, compositionText)
    ) {
      const preeditText = compositionText;
      isComposing = false;
      compositionText = "";
      clearPendingCompositionCommit();
      deferCompositionCommit(event.data ?? "", preeditText);
      event.preventDefault();
      event.stopImmediatePropagation();
      return;
    }

    if (shouldSuppressBrowserCompositionPreview(event.inputType, isComposing)) {
      compositionText = event.data ?? compositionText;
      clearTextareaNowAndNextFrame();
      event.preventDefault();
      event.stopImmediatePropagation();
      return;
    }

    const symbol = isComposing ? null : getPrintableSymbolInput(event.data);
    if (symbol !== null && isSymbolInputType(event.inputType)) {
      clearTextareaNowAndNextFrame();
      event.preventDefault();
      event.stopImmediatePropagation();
      sendText(symbol);
      return;
    }

    if (isComposing && !isTextInsertInputType(event.inputType)) {
      clearTextareaNowAndNextFrame();
      event.preventDefault();
      event.stopImmediatePropagation();
    }
  };

  const handleInputCapture = (event: Event) => {
    if (isComposing || pendingCompositionCommit || performance.now() <= ignorePostCompositionUntil) {
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
      pendingCompositionCommit &&
      (shouldIgnorePostCompositionCandidate(
        data,
        buildPostCompositionIgnoredCandidates(
          pendingCompositionCommit.text,
          pendingCompositionCommit.preeditText,
        ),
      ) ||
        shouldIgnoreReplayByCharacters(
          data,
          buildPostCompositionIgnoredCandidates(
            pendingCompositionCommit.text,
            pendingCompositionCommit.preeditText,
          ),
        ))
    ) {
      return;
    }
    if (
      performance.now() <= ignorePostCompositionUntil &&
      data &&
      isPostCompositionReplayPrefix(ignoredReplayProgress + data, ignoredPostCompositionCandidates)
    ) {
      ignoredReplayProgress += data;
      if (shouldIgnorePostCompositionCandidate(ignoredReplayProgress, ignoredPostCompositionCandidates)) {
        ignoredPostCompositionCandidates = removeMatchedPostCompositionCandidates(
          ignoredPostCompositionCandidates,
          ignoredReplayProgress,
        );
        ignoredReplayProgress = "";
      }
      return;
    }
    ignoredReplayProgress = "";
    if (
      performance.now() <= ignorePostCompositionUntil &&
      (shouldIgnorePostCompositionCandidate(data, ignoredPostCompositionCandidates) ||
        shouldIgnoreReplayByCharacters(data, ignoredPostCompositionCandidates))
    ) {
      ignoredReplayProgress = "";
      ignoredPostCompositionCandidates = removeMatchedPostCompositionCandidates(
        ignoredPostCompositionCandidates,
        data,
      );
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
      clearPendingCompositionCommit();
      disposable.dispose();
    },
  };
}
