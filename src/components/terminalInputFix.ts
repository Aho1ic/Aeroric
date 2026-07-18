import type { Terminal } from "@xterm/xterm";
import { IS_MAC_WEBKIT, IS_OTHER_WEBKIT } from "../platform";

type TerminalWithInput = Pick<Terminal, "input" | "textarea">;

// 诊断开关：置为 true 会在 webview 控制台输出 IME 事件流（需 release 包启用
// tauri "devtools" feature 才能在 app 内打开开发者工具）。排查 IME 问题时开启，
// 正式使用置为 false 以避免控制台噪声。详见 docs/terminal-ime-switch-fix.md。
const IME_DEBUG = false;
function imeDbg(label: string, extra: Record<string, unknown> = {}): void {
  if (!IME_DEBUG) return;
  try {
    // eslint-disable-next-line no-console
    console.log(`[IME] ${label}`, { ...extra });
  } catch {
    // 忽略 console 不可用
  }
}
function keySummary(event: KeyboardEvent): Record<string, unknown> {
  return {
    key: event.key,
    code: event.code,
    keyCode: event.keyCode,
    ctrlKey: event.ctrlKey,
    metaKey: event.metaKey,
    altKey: event.altKey,
    shiftKey: event.shiftKey,
    repeat: event.repeat,
    isComposing: event.isComposing,
  };
}

export const POST_COMPOSITION_REPLAY_IGNORE_MS = 3000;
const ROMANIZED_COMPOSITION_COMMIT_DELAY_MS = 90;
const ROMANIZED_COMPOSITION_KEY_FALLBACK_MS = 180;
// 非候选键触发的罗马化提交（IME 切换 / 回车提交原始拼音）无需等待中文候选，
// 用 0ms（下一个宏任务）提交即可。同步到达的中文 beforeinput 仍能在定时器触发前
// cancel 掉这次提交（见测试 "sends committed Chinese from WebKit beforeinput ..."）。
const ROMANIZED_COMPOSITION_PROMPT_COMMIT_DELAY_MS = 0;
const POST_COMPOSITION_TEXTAREA_CLEAR_DELAYS_MS = [0, 16, 40, 80, 160, 320, 640];
const TEXTAREA_INPUT_CLIENT_RESET_MS = 24;
const IME_PROCESS_KEY_GUARD_MS = 180;
// WebKit can expose the first IME letter as a normal key before compositionstart.
// Hold matching xterm data for one frame so that composition can claim it.
const IME_FIRST_KEY_HANDOFF_DELAY_MS = 32;
const IME_FIRST_KEY_AFTER_EDIT_HANDOFF_DELAY_MS = 120;
const IME_RECENT_EDIT_GUARD_MS = 260;
const CLEARED_COMPOSITION_REPLAY_GUARD_MS = 800;

export function applyTerminalTextareaInputAttributes(term: {
  textarea?: HTMLTextAreaElement | null;
}): void {
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

function isCompositionDeletionInputType(inputType: string): boolean {
  return (
    inputType === "deleteCompositionText" ||
    inputType === "deleteContentBackward" ||
    inputType === "deleteContentForward"
  );
}

function isUserCompositionDeletionInputType(inputType: string): boolean {
  return inputType === "deleteContentBackward" || inputType === "deleteContentForward";
}

function isRepeatableEditingKey(key: string): boolean {
  return (
    key === "Backspace" ||
    key === "Delete" ||
    key === "ArrowLeft" ||
    key === "ArrowRight" ||
    key === "ArrowUp" ||
    key === "ArrowDown"
  );
}

function isPrintableKey(key: string): boolean {
  return key.length === 1;
}

function getPotentialRomanizedImeHandoffKey(event: KeyboardEvent): string | null {
  if (
    event.isComposing ||
    event.repeat ||
    event.ctrlKey ||
    event.metaKey ||
    event.altKey ||
    event.shiftKey ||
    !/^[A-Za-z]$/.test(event.key)
  ) {
    return null;
  }
  return event.key;
}

function isCandidateCommitKey(event: KeyboardEvent): boolean {
  const legacyKeyCode = event.keyCode;
  return (
    event.key === " " ||
    event.key === "Spacebar" ||
    event.code === "Space" ||
    event.key === "Enter" ||
    event.code === "Enter" ||
    /^[1-9]$/.test(event.key) ||
    /^(?:Digit|Numpad)[1-9]$/.test(event.code) ||
    legacyKeyCode === 13 ||
    legacyKeyCode === 32 ||
    (legacyKeyCode >= 49 && legacyKeyCode <= 57) ||
    (legacyKeyCode >= 97 && legacyKeyCode <= 105)
  );
}

function getRomanizedImeProcessKey(event: KeyboardEvent): string | null {
  if (
    event.keyCode !== 229 ||
    event.repeat ||
    event.ctrlKey ||
    event.metaKey ||
    event.altKey ||
    event.shiftKey
  ) {
    return null;
  }
  if (/^[A-Za-z]$/.test(event.key)) return event.key.toLowerCase();
  const letterCode = event.code.match(/^Key([A-Z])$/);
  if (letterCode) return letterCode[1].toLowerCase();
  if (event.code === "Quote") return "'";
  if (event.code === "Backquote") return "`";
  return null;
}

function isPlainSpaceKey(event: KeyboardEvent): boolean {
  return (
    !event.isComposing &&
    event.keyCode === 32 &&
    (event.key === " " || event.key === "Spacebar" || event.code === "Space")
  );
}

export function shouldSuppressPrintableKeyRepeat(event: KeyboardEvent): boolean {
  if (event.type === "keydown" && event.repeat && event.keyCode === 229) {
    return true;
  }
  return (
    event.type === "keydown" &&
    event.repeat &&
    isPrintableKey(event.key) &&
    !isRepeatableEditingKey(event.key)
  );
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
    if (
      !shouldIgnorePostCompositionInsert(
        text,
        candidate,
        normalizeCommittedCompositionText(candidate),
      )
    ) {
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
    if (
      shouldIgnorePostCompositionInsert(
        text,
        candidate,
        normalizeCommittedCompositionText(candidate),
      )
    ) {
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

export function shouldPreserveBrowserCompositionPreview(
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
  return Boolean(
    text && /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u.test(text),
  );
}

function normalizeRomanizedReplayText(text: string): string {
  return text.replace(/['`\s]/g, "").toLowerCase();
}

function hasRomanizedSeparator(text: string): boolean {
  return /['`\s]/.test(text);
}

function normalizeTerminalDataAfterIme(
  text: string,
  candidates: ReadonlySet<string> = new Set(),
): string {
  if (!containsCommittedCjkText(text)) return text;
  const mixedCjkWithRomanizedTail = text.match(
    /^([\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]+)([A-Za-z][A-Za-z'`\s]+)$/u,
  );
  if (mixedCjkWithRomanizedTail) {
    const romanizedTail = normalizeRomanizedReplayText(mixedCjkWithRomanizedTail[2]);
    const matchesKnownPreedit = Array.from(candidates).some((candidate) => {
      if (!candidate || containsCommittedCjkText(candidate)) return false;
      return normalizeRomanizedReplayText(candidate) === romanizedTail;
    });
    if (mixedCjkWithRomanizedTail[2].includes("'") || matchesKnownPreedit) {
      return mixedCjkWithRomanizedTail[1];
    }
  }
  return normalizeCommittedCompositionText(text);
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
  let textareaClearGeneration = 0;
  let textareaClearTimers: Array<ReturnType<typeof globalThis.setTimeout>> = [];
  let textareaInputClientResetTimer: ReturnType<typeof globalThis.setTimeout> | null = null;
  let textareaDisabledByCjkReset = false;
  let textareaDisabledBeforeCjkReset = false;
  let isReleasingXtermComposition = false;
  let suppressNextTextInsertAfterRepeatedKey: string | true | null = null;
  let imeProcessKeyGuardUntil = 0;
  let deferNextRomanizedCompositionCommit = false;
  let compositionDeletionInProgress = false;
  let candidateKeyCommitTimer: ReturnType<typeof globalThis.setTimeout> | null = null;
  let candidateCommitText = "";
  let preservedRomanizedCompositionText = "";
  let pendingRomanizedProcessText = "";
  let potentialRomanizedImeHandoffKey = "";
  let potentialRomanizedImeHandoffUntil = 0;
  let extendedRomanizedImeHandoffUntil = 0;
  let pendingRomanizedImeHandoffData: {
    data: string;
    timer: ReturnType<typeof globalThis.setTimeout>;
  } | null = null;
  let recentlyClearedRomanizedText = "";
  let recentlyClearedRomanizedUntil = 0;
  let pendingCompositionCommit: {
    text: string;
    preeditText: string;
    fromCandidateKey: boolean;
    timer: ReturnType<typeof globalThis.setTimeout>;
  } | null = null;

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

  const hideXtermCompositionView = () => {
    const terminalElement = textarea.closest<HTMLElement>(".xterm");
    const compositionView = terminalElement?.querySelector<HTMLElement>(".composition-view");
    if (!compositionView) return;
    compositionView.classList.remove("active");
    compositionView.textContent = "";
  };

  const terminalElement = textarea.closest<HTMLElement>(".xterm");
  const compositionView = terminalElement?.querySelector<HTMLElement>(".composition-view") ?? null;
  const restoreCompositionPreviewAfterEvent = () => {
    if (!compositionView || !compositionText) return;
    queueMicrotask(() => {
      if (!compositionText) return;
      compositionView.textContent = compositionText;
      compositionView.classList.add("active");
    });
  };

  const clearRecentlyClearedRomanizedText = () => {
    recentlyClearedRomanizedText = "";
    recentlyClearedRomanizedUntil = 0;
  };

  const rememberRecentlyClearedRomanizedText = (text: string) => {
    if (!shouldDeferRomanizedCompositionCommit(text, text)) return;
    recentlyClearedRomanizedText = text.trim();
    recentlyClearedRomanizedUntil = performance.now() + CLEARED_COMPOSITION_REPLAY_GUARD_MS;
  };

  const getRecentlyClearedRomanizedText = () => {
    if (performance.now() > recentlyClearedRomanizedUntil) {
      clearRecentlyClearedRomanizedText();
    }
    return recentlyClearedRomanizedText;
  };

  const updateCompositionText = (nextText: string) => {
    const previousText = compositionText;
    compositionText = nextText;
    if (shouldDeferRomanizedCompositionCommit(nextText, nextText)) {
      preservedRomanizedCompositionText = nextText.trim();
    } else if (!nextText && compositionDeletionInProgress) {
      preservedRomanizedCompositionText = "";
    }
    if (previousText && !nextText) {
      ignoredReplayProgress = "";
      if (compositionDeletionInProgress) {
        clearRecentlyClearedRomanizedText();
      } else {
        rememberRecentlyClearedRomanizedText(previousText);
      }
      // 候选键正在提交英文候选时，微信输入法会先发一个空的 compositionupdate 清空预编辑，
      // 再用 compositionend 带回英文单词（例如 plan）。这种“清空”是提交流程的一部分，
      // 不能把即将回来的英文单词当成 replay 抑制掉——否则按空格/回车/数字选英文候选时
      // 会什么都打不出来（显示空白）。仅在没有候选键提交进行中时才布防 replay 守卫。
      const candidateCommitInFlight =
        deferNextRomanizedCompositionCommit ||
        candidateKeyCommitTimer !== null ||
        candidateCommitText !== "";
      if (!candidateCommitInFlight) {
        ignoredPostCompositionCandidates = buildPostCompositionIgnoredCandidates(
          previousText,
          previousText,
        );
        ignorePostCompositionUntil = performance.now() + CLEARED_COMPOSITION_REPLAY_GUARD_MS;
      }
    }
  };
  const compositionObserver = compositionView
    ? new MutationObserver(() => {
        if (compositionText) return;
        if (compositionView.classList.contains("active")) {
          compositionView.classList.remove("active");
        }
        if (compositionView.textContent) {
          compositionView.textContent = "";
        }
        clearTextarea();
      })
    : null;
  compositionObserver?.observe(compositionView!, {
    attributes: true,
    attributeFilter: ["class"],
    childList: true,
    characterData: true,
    subtree: true,
  });

  const hideEmptyXtermCompositionViewAfterEvent = () => {
    queueMicrotask(() => {
      if (compositionText) return;
      hideXtermCompositionView();
      clearTextarea();
      const schedule =
        typeof window !== "undefined" && typeof window.requestAnimationFrame === "function"
          ? window.requestAnimationFrame.bind(window)
          : (callback: FrameRequestCallback) =>
              globalThis.setTimeout(() => callback(performance.now()), 0);
      schedule(() => {
        if (compositionText) return;
        hideXtermCompositionView();
        clearTextarea();
      });
    });
  };

  const clearScheduledTextareaClears = () => {
    for (const timer of textareaClearTimers) {
      globalThis.clearTimeout(timer);
    }
    textareaClearTimers = [];
  };

  const clearTextareaInputClientReset = () => {
    if (!textareaInputClientResetTimer) return;
    globalThis.clearTimeout(textareaInputClientResetTimer);
    textareaInputClientResetTimer = null;
  };

  const restoreTextareaAfterCjkReset = () => {
    if (textareaDisabledByCjkReset && !textareaDisabledBeforeCjkReset) {
      textarea.disabled = false;
    }
    textareaDisabledByCjkReset = false;
  };
  const resetTextareaInputClientAfterCjkCommit = () => {
    clearTextareaInputClientReset();
    // Capture the pre-reset disabled state only once per reset sequence.
    // Back-to-back calls within the timeout window must not observe the
    // disabled flag we ourselves just set, otherwise the textarea would
    // stay permanently disabled and block further input.
    if (!textareaDisabledByCjkReset) {
      textareaDisabledBeforeCjkReset = textarea.disabled;
      textareaDisabledByCjkReset = true;
    }
    textarea.disabled = true;
    textareaInputClientResetTimer = globalThis.setTimeout(() => {
      textareaInputClientResetTimer = null;
      restoreTextareaAfterCjkReset();
      clearTextarea();
      if (!textarea.disabled) {
        textarea.focus({ preventScroll: true });
      }
    }, TEXTAREA_INPUT_CLIENT_RESET_MS);
  };

  const clearTextareaAfterWebKitReplay = () => {
    clearScheduledTextareaClears();
    hideXtermCompositionView();
    clearTextareaNowAndNextFrame();
    textareaClearGeneration += 1;
    const generation = textareaClearGeneration;
    textareaClearTimers = POST_COMPOSITION_TEXTAREA_CLEAR_DELAYS_MS.map((delay) =>
      globalThis.setTimeout(() => {
        if (generation !== textareaClearGeneration) return;
        clearTextarea();
      }, delay),
    );
  };

  const sendText = (text: string | null | undefined) => {
    if (!text) return;
    const normalized = normalizeCommittedCompositionText(text);
    imeDbg("sendText", { raw: text, normalized });
    onDataCallback(normalized);
  };

  const clearPendingCompositionCommit = () => {
    if (!pendingCompositionCommit) return;
    globalThis.clearTimeout(pendingCompositionCommit.timer);
    pendingCompositionCommit = null;
  };

  const clearCandidateKeyCommit = () => {
    if (!candidateKeyCommitTimer) return;
    globalThis.clearTimeout(candidateKeyCommitTimer);
    candidateKeyCommitTimer = null;
  };

  const clearPotentialRomanizedImeHandoff = () => {
    potentialRomanizedImeHandoffKey = "";
    potentialRomanizedImeHandoffUntil = 0;
  };

  const cancelPendingRomanizedImeHandoff = (): string => {
    const pendingData = pendingRomanizedImeHandoffData?.data ?? "";
    if (pendingRomanizedImeHandoffData) {
      globalThis.clearTimeout(pendingRomanizedImeHandoffData.timer);
      pendingRomanizedImeHandoffData = null;
    }
    const potentialKey =
      performance.now() <= potentialRomanizedImeHandoffUntil ? potentialRomanizedImeHandoffKey : "";
    clearPotentialRomanizedImeHandoff();
    return pendingData || potentialKey;
  };

  const flushPendingRomanizedImeHandoff = () => {
    if (!pendingRomanizedImeHandoffData) {
      clearPotentialRomanizedImeHandoff();
      return;
    }
    const pending = pendingRomanizedImeHandoffData;
    pendingRomanizedImeHandoffData = null;
    globalThis.clearTimeout(pending.timer);
    clearPotentialRomanizedImeHandoff();
    onDataCallback(pending.data);
  };

  const commitCompositionText = (text: string, preeditText: string) => {
    const normalized = normalizeCommittedCompositionText(text);
    candidateCommitText = "";
    preservedRomanizedCompositionText = "";
    pendingRomanizedProcessText = "";
    cancelPendingRomanizedImeHandoff();
    clearRecentlyClearedRomanizedText();
    ignoredReplayProgress = "";
    ignoredPostCompositionCandidates = buildPostCompositionIgnoredCandidates(text, preeditText);
    ignorePostCompositionUntil = performance.now() + POST_COMPOSITION_REPLAY_IGNORE_MS;
    clearTextareaAfterWebKitReplay();
    if (containsCommittedCjkText(normalized)) {
      resetTextareaInputClientAfterCjkCommit();
    }
    sendText(normalized);
  };

  const releaseXtermCompositionState = () => {
    hideXtermCompositionView();
    clearTextareaNowAndNextFrame();
    if (typeof CompositionEvent === "undefined" || isReleasingXtermComposition) return;
    isReleasingXtermComposition = true;
    try {
      textarea.dispatchEvent(new CompositionEvent("compositionend", { data: "" }));
    } finally {
      isReleasingXtermComposition = false;
    }
  };

  const getActiveCompositionText = () => {
    const visibleCompositionText =
      compositionView?.classList.contains("active") && compositionView.textContent
        ? compositionView.textContent
        : "";
    return (
      compositionText ||
      textarea.value ||
      visibleCompositionText ||
      candidateCommitText ||
      preservedRomanizedCompositionText
    );
  };

  const commitActiveRomanizedComposition = (): string | null => {
    const text = getActiveCompositionText();
    const defer = shouldDeferRomanizedCompositionCommit(text, text);
    imeDbg("commitActiveRomanizedComposition", {
      text,
      internalIsComposing: isComposing,
      shouldDefer: defer,
    });
    if (!isComposing || !defer) {
      return null;
    }
    const normalized = normalizeCommittedCompositionText(text);
    isComposing = false;
    compositionText = "";
    candidateCommitText = "";
    preservedRomanizedCompositionText = "";
    pendingRomanizedProcessText = "";
    clearCandidateKeyCommit();
    clearPendingCompositionCommit();
    commitCompositionText(text, text);
    releaseXtermCompositionState();
    return normalized;
  };

  const recoverRecentlyClearedRomanizedComposition = (): boolean => {
    const text = getRecentlyClearedRomanizedText();
    if (!shouldDeferRomanizedCompositionCommit(text, text)) return false;
    isComposing = true;
    compositionText = text;
    candidateCommitText = text;
    preservedRomanizedCompositionText = text;
    pendingRomanizedProcessText = "";
    return true;
  };

  const flushPendingCompositionCommit = (): boolean => {
    const pending = pendingCompositionCommit;
    if (!pending) return false;
    pendingCompositionCommit = null;
    globalThis.clearTimeout(pending.timer);
    commitCompositionText(pending.text, pending.preeditText);
    return true;
  };

  const deferCompositionCommit = (
    text: string,
    preeditText: string,
    delayMs: number = ROMANIZED_COMPOSITION_COMMIT_DELAY_MS,
  ) => {
    clearPendingCompositionCommit();
    pendingCompositionCommit = {
      text,
      preeditText,
      fromCandidateKey: deferNextRomanizedCompositionCommit,
      timer: globalThis.setTimeout(() => {
        const pending = pendingCompositionCommit;
        pendingCompositionCommit = null;
        if (!pending) return;
        commitCompositionText(pending.text, pending.preeditText);
      }, delayMs),
    };
    ignoredReplayProgress = "";
    ignoredPostCompositionCandidates = buildPostCompositionIgnoredCandidates(text, preeditText);
    ignorePostCompositionUntil = performance.now() + POST_COMPOSITION_REPLAY_IGNORE_MS;
    clearTextareaAfterWebKitReplay();
  };

  const commitOrDeferRomanizedComposition = (text: string, preeditText: string) => {
    if (deferNextRomanizedCompositionCommit) {
      deferCompositionCommit(text, preeditText, ROMANIZED_COMPOSITION_COMMIT_DELAY_MS);
      return;
    }
    if (!hasRomanizedSeparator(text || preeditText)) {
      // 无候选键、无分隔符：IME 切换提交原始拼音，尽快提交（0ms），
      // 避免切换瞬间“什么都不显示”及随后按空格重复发送。
      deferCompositionCommit(text, preeditText, ROMANIZED_COMPOSITION_PROMPT_COMMIT_DELAY_MS);
      return;
    }
    commitCompositionText(text, preeditText);
  };

  const handleCompositionStartCapture = (event: CompositionEvent) => {
    imeDbg("compositionstart", { data: event.data });
    const handoffText = cancelPendingRomanizedImeHandoff();
    textareaClearGeneration += 1;
    clearScheduledTextareaClears();
    clearCandidateKeyCommit();
    clearPendingCompositionCommit();
    suppressNextTextInsertAfterRepeatedKey = null;
    deferNextRomanizedCompositionCommit = false;
    compositionDeletionInProgress = false;
    isComposing = true;
    compositionText = "";
    candidateCommitText = "";
    clearRecentlyClearedRomanizedText();
    const initialRomanizedText = event.data || pendingRomanizedProcessText || handoffText;
    preservedRomanizedCompositionText = shouldDeferRomanizedCompositionCommit(
      initialRomanizedText,
      initialRomanizedText,
    )
      ? initialRomanizedText.trim()
      : "";
    pendingRomanizedProcessText = "";
    ignoredReplayProgress = "";
    void event;
  };

  const handleCompositionUpdateCapture = (event: CompositionEvent) => {
    const eventText = event.data ?? "";
    const textareaText = textarea.value;
    updateCompositionText(
      compositionDeletionInProgress
        ? eventText
        : eventText ||
            (textareaText && (!compositionText || textareaText !== compositionText)
              ? textareaText
              : ""),
    );
    imeDbg("compositionupdate", { data: event.data, compositionText });
    // Do not clear xterm's helper textarea while WebKit is composing. xterm
    // reads that value after this capture listener to paint `.composition-view`;
    // clearing it here made WeChat IME show candidates but hide the pinyin.
    if (compositionText) {
      // WeChat can emit an empty `compositionupdate.data` while keeping the
      // current pinyin in the helper textarea. xterm processes the empty event
      // after this capture listener and clears its preview, so restore it once
      // the event has finished propagating.
      restoreCompositionPreviewAfterEvent();
    } else {
      hideEmptyXtermCompositionViewAfterEvent();
    }
  };

  const handleCompositionEndCapture = (event: CompositionEvent) => {
    imeDbg("compositionend", {
      data: event.data,
      internalIsComposing: isComposing,
      compositionText,
      textareaValue: textarea.value,
      ignoreUntil: ignorePostCompositionUntil,
      now: performance.now(),
    });
    if (isReleasingXtermComposition) return;
    clearCandidateKeyCommit();
    pendingRomanizedProcessText = "";
    compositionDeletionInProgress = false;
    const preeditText = getActiveCompositionText();
    const text = event.data || preeditText;
    if (
      performance.now() <= ignorePostCompositionUntil &&
      (shouldIgnorePostCompositionCandidate(text, ignoredPostCompositionCandidates) ||
        shouldIgnoreReplayByCharacters(text, ignoredPostCompositionCandidates))
    ) {
      imeDbg("compositionend ignored as replay", { text });
      isComposing = false;
      compositionText = "";
      preservedRomanizedCompositionText = "";
      hideXtermCompositionView();
      clearTextareaNowAndNextFrame();
      return;
    }
    isComposing = false;
    compositionText = "";
    void event;
    if (!text) {
      deferNextRomanizedCompositionCommit = false;
      preservedRomanizedCompositionText = "";
      hideXtermCompositionView();
      clearTextareaNowAndNextFrame();
      return;
    }
    if (shouldDeferRomanizedCompositionCommit(text, preeditText)) {
      imeDbg("compositionend -> defer/commit romanized", {
        text,
        preeditText,
        deferNext: deferNextRomanizedCompositionCommit,
      });
      commitOrDeferRomanizedComposition(text, preeditText);
      deferNextRomanizedCompositionCommit = false;
      return;
    }
    deferNextRomanizedCompositionCommit = false;
    imeDbg("compositionend -> commit immediate", { text, preeditText });
    commitCompositionText(text, preeditText);
  };

  const handleBeforeInputCapture = (event: InputEvent) => {
    if (
      suppressNextTextInsertAfterRepeatedKey !== null &&
      event.data &&
      isTextInsertInputType(event.inputType)
    ) {
      const suppressedText = suppressNextTextInsertAfterRepeatedKey;
      suppressNextTextInsertAfterRepeatedKey = null;
      if (suppressedText === true || event.data === suppressedText) {
        clearTextareaNowAndNextFrame();
        event.preventDefault();
        event.stopImmediatePropagation();
        return;
      }
    }

    const recentlyClearedText = getRecentlyClearedRomanizedText();
    if (
      !isComposing &&
      event.data &&
      isTextInsertInputType(event.inputType) &&
      recentlyClearedText &&
      normalizeRomanizedReplayText(event.data) === normalizeRomanizedReplayText(recentlyClearedText)
    ) {
      ignoredReplayProgress = "";
      ignoredPostCompositionCandidates = new Set();
      ignorePostCompositionUntil = 0;
      event.preventDefault();
      event.stopImmediatePropagation();
      commitCompositionText(event.data, recentlyClearedText);
      return;
    }

    if (isComposing && isCompositionDeletionInputType(event.inputType)) {
      compositionDeletionInProgress = isUserCompositionDeletionInputType(event.inputType);
      if (compositionDeletionInProgress) {
        clearRecentlyClearedRomanizedText();
        extendedRomanizedImeHandoffUntil = performance.now() + IME_RECENT_EDIT_GUARD_MS;
      }
      return;
    }

    if (
      isComposing &&
      event.data &&
      event.inputType === "insertText" &&
      event.isComposing &&
      !containsCommittedCjkText(event.data) &&
      shouldDeferRomanizedCompositionCommit(event.data, compositionText)
    ) {
      compositionText = event.data;
      preservedRomanizedCompositionText = event.data.trim();
      // Let WebKit and xterm keep the hidden textarea/composition view in sync.
      // handleTerminalData already keeps this live pinyin out of the PTY.
      return;
    }

    const committedRomanizedText =
      isComposing &&
      event.data &&
      isTextInsertInputType(event.inputType) &&
      !containsCommittedCjkText(event.data) &&
      !shouldDeferRomanizedCompositionCommit(event.data, compositionText)
        ? commitActiveRomanizedComposition()
        : null;
    if (committedRomanizedText !== null) {
      const insertedText = event.data ?? "";
      event.preventDefault();
      event.stopImmediatePropagation();
      if (normalizeCommittedCompositionText(insertedText) !== committedRomanizedText) {
        sendText(insertedText);
      }
      return;
    }

    if (
      !isComposing &&
      !event.isComposing &&
      event.data &&
      isTextInsertInputType(event.inputType) &&
      performance.now() <= ignorePostCompositionUntil &&
      isPostCompositionReplayPrefix(
        ignoredReplayProgress + event.data,
        ignoredPostCompositionCandidates,
      )
    ) {
      ignoredReplayProgress += event.data;
      if (
        shouldIgnorePostCompositionCandidate(
          ignoredReplayProgress,
          ignoredPostCompositionCandidates,
        )
      ) {
        ignoredPostCompositionCandidates = removeMatchedPostCompositionCandidates(
          ignoredPostCompositionCandidates,
          ignoredReplayProgress,
        );
        ignoredReplayProgress = "";
      }
      clearTextareaAfterWebKitReplay();
      event.preventDefault();
      event.stopImmediatePropagation();
      return;
    }

    if (
      event.data &&
      isTextInsertInputType(event.inputType) &&
      containsCommittedCjkText(event.data)
    ) {
      const normalizedTerminalInput = normalizeTerminalDataAfterIme(
        event.data,
        ignoredPostCompositionCandidates,
      );
      if (normalizedTerminalInput !== event.data) {
        const pending = pendingCompositionCommit;
        clearPendingCompositionCommit();
        const preeditText = pending?.preeditText || getActiveCompositionText();
        const nextCandidates = buildPostCompositionIgnoredCandidates(event.data, preeditText);
        for (const candidate of ignoredPostCompositionCandidates) {
          nextCandidates.add(candidate);
        }
        addIgnoredCandidate(nextCandidates, normalizedTerminalInput);
        ignoredReplayProgress = "";
        ignoredPostCompositionCandidates = nextCandidates;
        ignorePostCompositionUntil = performance.now() + POST_COMPOSITION_REPLAY_IGNORE_MS;
        isComposing = false;
        compositionText = "";
        clearTextareaAfterWebKitReplay();
        resetTextareaInputClientAfterCjkCommit();
        event.preventDefault();
        event.stopImmediatePropagation();
        sendText(normalizedTerminalInput);
        return;
      }
    }

    if (pendingCompositionCommit && isTextInsertInputType(event.inputType) && event.data) {
      const pending = pendingCompositionCommit;
      clearPendingCompositionCommit();
      if (!containsCommittedCjkText(event.data)) {
        commitCompositionText(pending.text, pending.preeditText);
        event.preventDefault();
        event.stopImmediatePropagation();
        sendText(event.data);
        return;
      }
      ignoredPostCompositionCandidates = buildPostCompositionIgnoredCandidates(
        event.data,
        pending.preeditText || pending.text,
      );
      ignorePostCompositionUntil = performance.now() + POST_COMPOSITION_REPLAY_IGNORE_MS;
      clearTextareaAfterWebKitReplay();
      event.preventDefault();
      event.stopImmediatePropagation();
      sendText(event.data);
      return;
    }

    if (
      !isComposing &&
      !event.isComposing &&
      isTextInsertInputType(event.inputType) &&
      event.data &&
      performance.now() <= ignorePostCompositionUntil &&
      (shouldIgnorePostCompositionCandidate(event.data, ignoredPostCompositionCandidates) ||
        shouldIgnoreReplayByCharacters(event.data, ignoredPostCompositionCandidates))
    ) {
      clearTextareaAfterWebKitReplay();
      event.preventDefault();
      event.stopImmediatePropagation();
      ignoredReplayProgress = "";
      ignoredPostCompositionCandidates = removeMatchedPostCompositionCandidates(
        ignoredPostCompositionCandidates,
        event.data,
      );
      return;
    }

    if (isComposing && event.inputType === "insertText" && containsCommittedCjkText(event.data)) {
      const preeditText = getActiveCompositionText();
      isComposing = false;
      compositionText = "";
      clearPendingCompositionCommit();
      ignoredReplayProgress = "";
      ignoredPostCompositionCandidates = buildPostCompositionIgnoredCandidates(
        event.data,
        preeditText,
      );
      ignorePostCompositionUntil = performance.now() + POST_COMPOSITION_REPLAY_IGNORE_MS;
      clearTextareaAfterWebKitReplay();
      resetTextareaInputClientAfterCjkCommit();
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
      commitOrDeferRomanizedComposition(event.data ?? "", preeditText);
      deferNextRomanizedCompositionCommit = false;
      event.preventDefault();
      event.stopImmediatePropagation();
      return;
    }

    if (shouldPreserveBrowserCompositionPreview(event.inputType, isComposing)) {
      compositionText = event.data ?? compositionText;
      if (shouldDeferRomanizedCompositionCommit(compositionText, compositionText)) {
        preservedRomanizedCompositionText = compositionText.trim();
      }
      // Do not clear or cancel live composition input: xterm needs the native
      // textarea value to render pinyin such as `ce'shi` above the candidates.
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
    if (isComposing && typeof InputEvent !== "undefined" && event instanceof InputEvent) {
      if (isCompositionDeletionInputType(event.inputType)) {
        updateCompositionText(textarea.value);
        compositionDeletionInProgress = false;
        if (compositionText) {
          restoreCompositionPreviewAfterEvent();
        } else {
          hideEmptyXtermCompositionViewAfterEvent();
        }
        return;
      }
      if (
        event.isComposing &&
        isTextInsertInputType(event.inputType) &&
        !containsCommittedCjkText(event.data)
      ) {
        compositionText = event.data ?? compositionText;
        if (shouldDeferRomanizedCompositionCommit(compositionText, compositionText)) {
          preservedRomanizedCompositionText = compositionText.trim();
        }
        return;
      }
      const text = getActiveCompositionText();
      if (
        !event.isComposing &&
        isTextInsertInputType(event.inputType) &&
        shouldDeferRomanizedCompositionCommit(text, text) &&
        commitActiveRomanizedComposition() !== null
      ) {
        event.stopImmediatePropagation();
        return;
      }
    }
    if (
      isComposing ||
      pendingCompositionCommit ||
      performance.now() <= ignorePostCompositionUntil
    ) {
      clearTextareaNowAndNextFrame();
      event.stopImmediatePropagation();
    }
  };

  const handleKeyDownCapture = (event: KeyboardEvent) => {
    imeDbg("keydown", {
      ...keySummary(event),
      internalIsComposing: isComposing,
      compositionText,
      pending: !!pendingCompositionCommit,
    });
    const now = performance.now();
    if (
      !event.isComposing &&
      !event.repeat &&
      (event.key === "Backspace" ||
        event.code === "Backspace" ||
        event.key === "Delete" ||
        event.code === "Delete")
    ) {
      // 微信输入法在删除后立即开始下一段拼音时，WKWebView 的 compositionstart
      // 往往比普通输入慢一到数帧。给下一枚可打印键更长的交接窗口，避免 xterm
      // 先把 d/y/s 等首字母当作英文写入 PTY；普通英文输入仍走较短窗口。
      extendedRomanizedImeHandoffUntil = now + IME_RECENT_EDIT_GUARD_MS;
    }
    const potentialHandoffKey = getPotentialRomanizedImeHandoffKey(event);
    if (potentialHandoffKey !== null) {
      const handoffDelay =
        now <= extendedRomanizedImeHandoffUntil
          ? IME_FIRST_KEY_AFTER_EDIT_HANDOFF_DELAY_MS
          : IME_FIRST_KEY_HANDOFF_DELAY_MS;
      potentialRomanizedImeHandoffKey = potentialHandoffKey;
      potentialRomanizedImeHandoffUntil = now + handoffDelay;
      // 必须在 xterm 的 textarea keydown 处理器之前截住这一键，但不要
      // preventDefault。否则 xterm 会把首字母立即写入 PTY 并取消浏览器默认
      // 输入，后续即使 compositionstart 到达，也已经无法把该字母交还给 IME。
      // 保留默认行为后，普通英文会通过 textarea input 事件进入 xterm；
      // 中文输入法则可以正常启动 composition。
      event.stopImmediatePropagation();
    } else if (!event.isComposing && event.keyCode !== 229) {
      flushPendingRomanizedImeHandoff();
    }
    if (event.keyCode === 229) {
      imeProcessKeyGuardUntil = performance.now() + IME_PROCESS_KEY_GUARD_MS;
    } else if (!event.isComposing && isPrintableKey(event.key)) {
      imeProcessKeyGuardUntil = 0;
    }

    const romanizedProcessKey = getRomanizedImeProcessKey(event);
    if (romanizedProcessKey !== null) {
      if (isComposing) {
        preservedRomanizedCompositionText += romanizedProcessKey;
      } else {
        pendingRomanizedProcessText += romanizedProcessKey;
      }
    } else if (
      isComposing &&
      !event.repeat &&
      (event.key === "Backspace" || event.code === "Backspace")
    ) {
      preservedRomanizedCompositionText = preservedRomanizedCompositionText.slice(0, -1);
    } else if (!isComposing && event.keyCode !== 229 && !isCandidateCommitKey(event)) {
      pendingRomanizedProcessText = "";
    }

    let recoveredProcessCandidate = false;
    if (
      !isComposing &&
      isCandidateCommitKey(event) &&
      shouldDeferRomanizedCompositionCommit(
        pendingRomanizedProcessText,
        pendingRomanizedProcessText,
      )
    ) {
      const recoveredText = pendingRomanizedProcessText.trim();
      isComposing = true;
      compositionText = recoveredText;
      preservedRomanizedCompositionText = recoveredText;
      candidateCommitText = recoveredText;
      pendingRomanizedProcessText = "";
      recoveredProcessCandidate = true;
      imeDbg("keydown candidate recovered without compositionstart", {
        text: recoveredText,
        key: event.key,
        code: event.code,
      });
    }

    let recoveredClearedCandidate = false;
    if (!isComposing && isCandidateCommitKey(event)) {
      recoveredClearedCandidate = recoverRecentlyClearedRomanizedComposition();
      if (recoveredClearedCandidate) {
        ignoredReplayProgress = "";
        ignoredPostCompositionCandidates = new Set();
        ignorePostCompositionUntil = 0;
        imeDbg("keydown recovered cleared English candidate", {
          text: candidateCommitText,
          key: event.key,
          code: event.code,
        });
      }
    }

    // 检测 IME 切换快捷键（composition 期间按下）：CapsLock，或带 Ctrl/Meta/Alt 修饰的空格/数字。
    // 这些不是普通候选选择键，而是切换输入法——立即提交罗马化拼音。
    const looksLikeImeSwitchShortcut =
      isComposing &&
      (event.key === "CapsLock" ||
        event.code === "CapsLock" ||
        ((event.ctrlKey || event.metaKey || event.altKey) && isCandidateCommitKey(event)));
    if (looksLikeImeSwitchShortcut && commitActiveRomanizedComposition() !== null) {
      imeDbg("keydown IME-switch shortcut -> committed", { key: event.key });
      event.preventDefault();
      event.stopImmediatePropagation();
      return;
    }

    if (
      !recoveredProcessCandidate &&
      isComposing &&
      isPlainSpaceKey(event) &&
      commitActiveRomanizedComposition() !== null
    ) {
      imeDbg("keydown plain-space during composition -> committed");
      event.preventDefault();
      event.stopImmediatePropagation();
      return;
    }
    // IME 切换后 compositionend 已触发（isComposing=false），但延迟提交尚未 flush
    // 时按下的普通空格：立即提交罗马化拼音并抑制该空格，避免 ceshi + 多余空格。
    if (
      !isComposing &&
      pendingCompositionCommit &&
      !pendingCompositionCommit.fromCandidateKey &&
      isPlainSpaceKey(event)
    ) {
      flushPendingCompositionCommit();
      event.preventDefault();
      event.stopImmediatePropagation();
      return;
    }
    if (isComposing && isCandidateCommitKey(event)) {
      deferNextRomanizedCompositionCommit = true;
      const activeText = getActiveCompositionText();
      // WebKit may clear both the helper textarea and composition state before
      // the fallback runs, so preserve the visible English candidate now.
      if (shouldDeferRomanizedCompositionCommit(activeText, activeText)) {
        candidateCommitText = activeText;
        // The empty update may have marked this same word as a replay. A
        // candidate key proves it is being selected rather than merely deleted.
        ignoredReplayProgress = "";
        ignoredPostCompositionCandidates = new Set();
        ignorePostCompositionUntil = 0;
      }
      clearCandidateKeyCommit();
      candidateKeyCommitTimer = globalThis.setTimeout(() => {
        candidateKeyCommitTimer = null;
        commitActiveRomanizedComposition();
      }, ROMANIZED_COMPOSITION_KEY_FALLBACK_MS);
      if (recoveredClearedCandidate) {
        event.preventDefault();
        event.stopImmediatePropagation();
        return;
      }
    }
    if (shouldSuppressPrintableKeyRepeat(event)) {
      suppressNextTextInsertAfterRepeatedKey =
        event.keyCode === 229 ? true : isPrintableKey(event.key) ? event.key : true;
      event.preventDefault();
      event.stopImmediatePropagation();
      return;
    }
    suppressNextTextInsertAfterRepeatedKey = null;
    if (!isComposing && event.keyCode === 229) event.stopImmediatePropagation();
  };

  const handleKeyPressCapture = (event: KeyboardEvent) => {
    const matchesPendingHandoff =
      performance.now() <= potentialRomanizedImeHandoffUntil &&
      event.key.length === 1 &&
      event.key.toLowerCase() === potentialRomanizedImeHandoffKey.toLowerCase();
    if (!isComposing && !matchesPendingHandoff) return;
    // keydown 已交给浏览器默认输入后，WebKit 仍可能继续发 keypress。
    // 阻止 xterm 在 keypress 阶段再次直接发送字符，最终统一由 input /
    // composition 事件决定这是英文输入还是 IME 预编辑。
    event.stopImmediatePropagation();
  };

  const handleKeyUpCapture = (event: KeyboardEvent) => {
    if (!isCandidateCommitKey(event) || candidateKeyCommitTimer !== null) return;
    const recoveredClearedCandidate = !isComposing && recoverRecentlyClearedRomanizedComposition();
    const activeText = getActiveCompositionText();
    if (!shouldDeferRomanizedCompositionCommit(activeText, activeText)) return;

    deferNextRomanizedCompositionCommit = true;
    candidateCommitText = activeText;
    ignoredReplayProgress = "";
    ignoredPostCompositionCandidates = new Set();
    ignorePostCompositionUntil = 0;
    clearCandidateKeyCommit();
    candidateKeyCommitTimer = globalThis.setTimeout(() => {
      candidateKeyCommitTimer = null;
      commitActiveRomanizedComposition();
    }, ROMANIZED_COMPOSITION_KEY_FALLBACK_MS);

    if (recoveredClearedCandidate || event.keyCode === 229 || event.isComposing) {
      event.preventDefault();
      event.stopImmediatePropagation();
    }
  };

  const handleBlurCapture = () => {
    imeDbg("textarea blur", { internalIsComposing: isComposing, compositionText });
    commitActiveRomanizedComposition();
  };

  const handleWindowBlur = () => {
    imeDbg("window blur", { internalIsComposing: isComposing, compositionText });
    commitActiveRomanizedComposition();
  };

  const handleTerminalData = (data: string) => {
    if (isComposing) {
      cancelPendingRomanizedImeHandoff();
      return;
    }
    if (performance.now() <= imeProcessKeyGuardUntil && /^[A-Za-z0-9]$/u.test(data)) {
      imeProcessKeyGuardUntil = 0;
      clearTextareaNowAndNextFrame();
      return;
    }
    if (
      !pendingCompositionCommit &&
      performance.now() <= potentialRomanizedImeHandoffUntil &&
      data.length === 1 &&
      data.toLowerCase() === potentialRomanizedImeHandoffKey.toLowerCase()
    ) {
      // A new keydown is stronger evidence than the stale-pinyin replay window.
      // Keeping the `ignorePostCompositionUntil` exclusion here made every new
      // first letter typed within three seconds of a Chinese commit bypass the
      // handoff completely and leak straight to the PTY.
      if (pendingRomanizedImeHandoffData) {
        pendingRomanizedImeHandoffData.data += data;
        return;
      }
      pendingRomanizedImeHandoffData = {
        data,
        timer: globalThis.setTimeout(
          () => {
            const pending = pendingRomanizedImeHandoffData;
            pendingRomanizedImeHandoffData = null;
            clearPotentialRomanizedImeHandoff();
            if (pending) onDataCallback(pending.data);
          },
          Math.max(
            IME_FIRST_KEY_HANDOFF_DELAY_MS,
            potentialRomanizedImeHandoffUntil - performance.now(),
          ),
        ),
      };
      return;
    }
    flushPendingRomanizedImeHandoff();
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
    if (pendingCompositionCommit && data) {
      flushPendingCompositionCommit();
      onDataCallback(normalizeTerminalDataAfterIme(data, ignoredPostCompositionCandidates));
      return;
    }
    if (
      performance.now() <= ignorePostCompositionUntil &&
      data &&
      isPostCompositionReplayPrefix(ignoredReplayProgress + data, ignoredPostCompositionCandidates)
    ) {
      ignoredReplayProgress += data;
      if (
        shouldIgnorePostCompositionCandidate(
          ignoredReplayProgress,
          ignoredPostCompositionCandidates,
        )
      ) {
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
    onDataCallback(normalizeTerminalDataAfterIme(data, ignoredPostCompositionCandidates));
  };

  const disposable = term.onData(handleTerminalData);

  textarea.addEventListener("compositionstart", handleCompositionStartCapture, true);
  textarea.addEventListener("compositionupdate", handleCompositionUpdateCapture, true);
  textarea.addEventListener("compositionend", handleCompositionEndCapture, true);
  textarea.addEventListener("beforeinput", handleBeforeInputCapture, true);
  textarea.addEventListener("input", handleInputCapture, true);
  const keydownTarget = terminalElement ?? textarea;
  keydownTarget.addEventListener("keydown", handleKeyDownCapture, true);
  keydownTarget.addEventListener("keypress", handleKeyPressCapture, true);
  keydownTarget.addEventListener("keyup", handleKeyUpCapture, true);
  textarea.addEventListener("blur", handleBlurCapture, true);
  window.addEventListener("blur", handleWindowBlur);

  return {
    dispose: () => {
      textarea.removeEventListener("compositionstart", handleCompositionStartCapture, true);
      textarea.removeEventListener("compositionupdate", handleCompositionUpdateCapture, true);
      textarea.removeEventListener("compositionend", handleCompositionEndCapture, true);
      textarea.removeEventListener("beforeinput", handleBeforeInputCapture, true);
      textarea.removeEventListener("input", handleInputCapture, true);
      keydownTarget.removeEventListener("keydown", handleKeyDownCapture, true);
      keydownTarget.removeEventListener("keypress", handleKeyPressCapture, true);
      keydownTarget.removeEventListener("keyup", handleKeyUpCapture, true);
      textarea.removeEventListener("blur", handleBlurCapture, true);
      window.removeEventListener("blur", handleWindowBlur);
      clearPendingCompositionCommit();
      clearCandidateKeyCommit();
      cancelPendingRomanizedImeHandoff();
      textareaClearGeneration += 1;
      clearScheduledTextareaClears();
      clearTextareaInputClientReset();
      restoreTextareaAfterCjkReset();
      compositionObserver?.disconnect();
      disposable.dispose();
    },
  };
}
