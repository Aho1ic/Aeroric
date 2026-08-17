/**
 * The produced files of one DSH session, for surfaces that read a finished
 * transcript rather than follow a live one.
 *
 * The insights panel already folds the live stream; a transcript only needs the
 * vocabulary once, so this loads a single history page and folds it. Failure is
 * silent by design: with no vocabulary the prose simply renders as the model
 * wrote it.
 */

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { projectDshSessionEvents, readDshHistoryPage } from "../dshSessionFeatures";
import type { DshProducedFile } from "../dshSessionFeatures";
import type { DshSessionHistory } from "../types";

/** Shared empty result, so a session without files keeps a stable identity. */
const NO_FILES: readonly DshProducedFile[] = [];

/** How much history to fold — the same page size the insights panel requests. */
const HISTORY_PAGE = 200;

/**
 * Load the files a DSH session produced.
 *
 * @param sessionId - The session to read; undefined for a non-DSH surface.
 * @returns Produced files ascending by seq, empty until loaded or on failure.
 */
export function useDshProducedFiles(sessionId?: string): readonly DshProducedFile[] {
  const [files, setFiles] = useState<readonly DshProducedFile[]>(NO_FILES);

  useEffect(() => {
    setFiles(NO_FILES);
    if (!sessionId) return;
    let disposed = false;
    void (async () => {
      let history: DshSessionHistory | undefined;
      try {
        history = await invoke<DshSessionHistory>("get_dsh_session_history", {
          sessionId,
          maxMessages: HISTORY_PAGE,
        });
      } catch {
        // No vocabulary, no links: the prose stays exactly as the model wrote it.
      }
      if (disposed) return;
      const page = readDshHistoryPage(history?.events ?? []);
      const produced = projectDshSessionEvents(page.events).producedFiles;
      setFiles(produced.length === 0 ? NO_FILES : produced);
    })();
    return () => {
      disposed = true;
    };
  }, [sessionId]);

  return files;
}
