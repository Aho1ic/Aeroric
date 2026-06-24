import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  buildLspDocumentRequest,
  isLspSupportedFile,
  languageServerStatusMessage,
  type LspDocumentRequest,
  type LspServerStatus,
} from "./languageServerState";

type UseLanguageServerOptions = {
  projectPath: string;
  filePath: string | null;
  content: string | null;
  cursorLine: number;
  cursorColumn: number;
  enabled?: boolean;
};

export type LanguageServerState = {
  supported: boolean;
  status: LspServerStatus | null;
  message: string | null;
  loading: boolean;
  request: LspDocumentRequest | null;
  refreshStatus: () => Promise<void>;
};

export function useLanguageServer({
  projectPath,
  filePath,
  content,
  cursorLine,
  cursorColumn,
  enabled = true,
}: UseLanguageServerOptions): LanguageServerState {
  const [status, setStatus] = useState<LspServerStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const runIdRef = useRef(0);
  const supported = Boolean(enabled && filePath && isLspSupportedFile(filePath));

  const request = useMemo(() => {
    if (!supported || !filePath || content === null) return null;
    return buildLspDocumentRequest({
      projectPath,
      filePath,
      content,
      line: cursorLine,
      column: cursorColumn,
    });
  }, [content, cursorColumn, cursorLine, filePath, projectPath, supported]);

  const refreshStatus = useCallback(async () => {
    if (!supported || !filePath) {
      runIdRef.current += 1;
      setStatus(null);
      setLoading(false);
      return;
    }
    const runId = ++runIdRef.current;
    setLoading(true);
    try {
      const nextStatus = await invoke<LspServerStatus>("lsp_server_status", {
        projectPath,
        filePath,
      });
      if (runId === runIdRef.current) {
        setStatus(nextStatus);
      }
    } catch {
      if (runId === runIdRef.current) {
        setStatus({
          supported: true,
          available: false,
          languageId: null,
          command: null,
          installHint: "pnpm add -D typescript-language-server typescript",
        });
      }
    } finally {
      if (runId === runIdRef.current) {
        setLoading(false);
      }
    }
  }, [filePath, projectPath, supported]);

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  return {
    supported,
    status,
    message: languageServerStatusMessage(status),
    loading,
    request,
    refreshStatus,
  };
}
