import type { Terminal } from "@xterm/xterm";
import { IS_MAC_WEBKIT, IS_OTHER_WEBKIT } from "../platform";

type TerminalWithInput = Pick<Terminal, "input" | "textarea">;

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

export function shouldLetBrowserRenderCompositionPreview(
  inputType: string,
  isComposing: boolean,
): boolean {
  return isComposing && inputType === "insertCompositionText";
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
  let ignoredPostCompositionText: string | null = null;
  let ignoredPostCompositionNormalized: string | null = null;
  let ignorePostCompositionUntil = 0;

  const sendText = (text: string | null | undefined) => {
    if (!text) return;
    onDataCallback(normalizeCommittedCompositionText(text));
  };

  const handleCompositionStartCapture = (event: CompositionEvent) => {
    isComposing = true;
    compositionText = "";
    void event;
  };

  const handleCompositionUpdateCapture = (event: CompositionEvent) => {
    compositionText = event.data ?? "";
  };

  const handleCompositionEndCapture = (event: CompositionEvent) => {
    const text = event.data || compositionText;
    isComposing = false;
    compositionText = "";
    const normalized = normalizeCommittedCompositionText(text);
    ignoredPostCompositionText = text || null;
    ignoredPostCompositionNormalized = normalized || null;
    ignorePostCompositionUntil = performance.now() + 180;
    textarea.value = "";
    event.stopImmediatePropagation();
    sendText(normalized);
  };

  const handleBeforeInputCapture = (event: InputEvent) => {
    if (
      event.inputType === "insertText" &&
      event.data &&
      performance.now() <= ignorePostCompositionUntil &&
      shouldIgnorePostCompositionInsert(
        event.data,
        ignoredPostCompositionText,
        ignoredPostCompositionNormalized,
      )
    ) {
      textarea.value = "";
      event.preventDefault();
      event.stopImmediatePropagation();
      ignoredPostCompositionText = null;
      ignoredPostCompositionNormalized = null;
      return;
    }

    if (shouldLetBrowserRenderCompositionPreview(event.inputType, isComposing)) {
      compositionText = event.data ?? compositionText;
      event.stopImmediatePropagation();
      return;
    }

    if (event.inputType === "insertCompositionText") {
      compositionText = event.data ?? compositionText;
      event.stopImmediatePropagation();
      return;
    }

    const symbol = getPrintableSymbolInput(event.data);
    if (symbol !== null && isSymbolInputType(event.inputType)) {
      textarea.value = "";
      event.preventDefault();
      event.stopImmediatePropagation();
      sendText(symbol);
      return;
    }

    if (isComposing) {
      event.preventDefault();
      event.stopImmediatePropagation();
    }
  };

  const handleKeyDownCapture = (event: KeyboardEvent) => {
    if (!isComposing && event.keyCode === 229) event.stopImmediatePropagation();
  };

  const disposable = term.onData(onDataCallback);

  textarea.addEventListener("compositionstart", handleCompositionStartCapture, true);
  textarea.addEventListener("compositionupdate", handleCompositionUpdateCapture, true);
  textarea.addEventListener("compositionend", handleCompositionEndCapture, true);
  textarea.addEventListener("beforeinput", handleBeforeInputCapture, true);
  textarea.addEventListener("keydown", handleKeyDownCapture, true);

  return {
    dispose: () => {
      textarea.removeEventListener("compositionstart", handleCompositionStartCapture, true);
      textarea.removeEventListener("compositionupdate", handleCompositionUpdateCapture, true);
      textarea.removeEventListener("compositionend", handleCompositionEndCapture, true);
      textarea.removeEventListener("beforeinput", handleBeforeInputCapture, true);
      textarea.removeEventListener("keydown", handleKeyDownCapture, true);
      disposable.dispose();
    },
  };
}
