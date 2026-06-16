import { useRef } from "react";
import type React from "react";
import {
  buildPostCompositionIgnoredCandidates,
  normalizeCommittedCompositionText,
  POST_COMPOSITION_REPLAY_IGNORE_MS,
  shouldIgnorePostCompositionCandidate,
} from "./terminalInputFix";

type TextInputElement = HTMLInputElement | HTMLTextAreaElement;

export function useTextInputIMEFix<T extends TextInputElement>(setValue: (value: string) => void) {
  const compositionTextRef = useRef("");
  const ignoredPostCompositionCandidatesRef = useRef<Set<string>>(new Set());
  const ignorePostCompositionUntilRef = useRef(0);

  const onBeforeInputCapture = (e: React.FormEvent<T>) => {
    const event = e.nativeEvent as InputEvent;
    if (event.inputType !== "insertText" || !event.data) return;
    if (
      performance.now() <= ignorePostCompositionUntilRef.current &&
      shouldIgnorePostCompositionCandidate(event.data, ignoredPostCompositionCandidatesRef.current)
    ) {
      event.preventDefault();
      event.stopImmediatePropagation();
      e.stopPropagation();
      ignoredPostCompositionCandidatesRef.current = new Set();
    }
  };

  const onCompositionStart = () => {
    compositionTextRef.current = "";
  };

  const onCompositionUpdate = (event: React.CompositionEvent<T>) => {
    compositionTextRef.current = event.data ?? "";
  };

  const onCompositionEnd = (event: React.CompositionEvent<T>) => {
    const preeditText = compositionTextRef.current;
    const committedText = event.data || preeditText;
    ignoredPostCompositionCandidatesRef.current = buildPostCompositionIgnoredCandidates(
      committedText,
      preeditText,
    );
    ignorePostCompositionUntilRef.current = performance.now() + POST_COMPOSITION_REPLAY_IGNORE_MS;
    compositionTextRef.current = "";

    const normalizedCommitted = normalizeCommittedCompositionText(committedText);
    if (!committedText || normalizedCommitted === committedText) return;

    const currentValue = event.currentTarget.value;
    const index = currentValue.lastIndexOf(committedText);
    if (index >= 0) {
      setValue(
        `${currentValue.slice(0, index)}${normalizedCommitted}${currentValue.slice(index + committedText.length)}`,
      );
    }
  };

  return {
    onBeforeInputCapture,
    onCompositionStart,
    onCompositionUpdate,
    onCompositionEnd,
  };
}
