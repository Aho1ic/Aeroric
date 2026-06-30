import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  buildLspDocumentRequest,
  isLspSupportedFile,
  lspCommandName,
  lspInvokeArgs,
  languageServerStatusMessage,
  type LspRemoteContext,
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
  remote?: LspRemoteContext;
};

export type LanguageServerState = {
  supported: boolean;
  status: LspServerStatus | null;
  message: string | null;
  loading: boolean;
  request: LspDocumentRequest | null;
  refreshStatus: () => Promise<void>;
};

type OpenLifecycleDocument = {
  key: string;
  projectPath: string;
  filePath: string;
  remote?: LspRemoteContext;
  content: string;
  version: number;
};

function lspLifecycleKey({
  projectPath,
  filePath,
  remote,
}: {
  projectPath: string;
  filePath: string;
  remote?: LspRemoteContext;
}): string {
  if (!remote) return `local:${projectPath}:${filePath}`;
  return `ssh:${remote.connection.id}:${remote.projectPath}:${filePath}`;
}

function invokeLifecycleCommand(
  command: string,
  args: Record<string, unknown>,
  remote?: LspRemoteContext,
) {
  void Promise.resolve(invoke(lspCommandName(command, remote), lspInvokeArgs(args, remote))).catch(
    () => {},
  );
}

export function useLanguageServer({
  projectPath,
  filePath,
  content,
  cursorLine,
  cursorColumn,
  enabled = true,
  remote,
}: UseLanguageServerOptions): LanguageServerState {
  const [status, setStatus] = useState<LspServerStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const runIdRef = useRef(0);
  const lifecycleRef = useRef<OpenLifecycleDocument | null>(null);
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
      const nextStatus = await invoke<LspServerStatus>(
        lspCommandName("lsp_server_status", remote),
        lspInvokeArgs({ projectPath, filePath }, remote),
      );
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
          installHint: remote
            ? "Install typescript-language-server and typescript on the remote host"
            : "pnpm add -D typescript-language-server typescript",
        });
      }
    } finally {
      if (runId === runIdRef.current) {
        setLoading(false);
      }
    }
  }, [filePath, projectPath, remote, supported]);

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  const closeLifecycleDocument = useCallback(() => {
    const current = lifecycleRef.current;
    if (!current) return;
    lifecycleRef.current = null;
    invokeLifecycleCommand(
      "lsp_close_document",
      {
        projectPath: current.projectPath,
        filePath: current.filePath,
      },
      current.remote,
    );
  }, []);

  const activeLifecycleKey = useMemo(() => {
    if (!supported || !filePath || content === null || !status?.available) return null;
    return lspLifecycleKey({ projectPath, filePath, remote });
  }, [content, filePath, projectPath, remote, status?.available, supported]);

  useEffect(() => {
    if (!activeLifecycleKey || !filePath || content === null) {
      closeLifecycleDocument();
      return;
    }
    if (lifecycleRef.current?.key === activeLifecycleKey) return;

    closeLifecycleDocument();
    lifecycleRef.current = {
      key: activeLifecycleKey,
      projectPath,
      filePath,
      remote,
      content,
      version: 1,
    };
    invokeLifecycleCommand(
      "lsp_open_document",
      {
        projectPath,
        filePath,
        content,
        version: 1,
      },
      remote,
    );
  }, [activeLifecycleKey, closeLifecycleDocument, content, filePath, projectPath, remote]);

  useEffect(() => {
    const current = lifecycleRef.current;
    if (!activeLifecycleKey || !current || current.key !== activeLifecycleKey || content === null) {
      return;
    }
    if (current.content === content) return;

    const nextVersion = current.version + 1;
    lifecycleRef.current = {
      ...current,
      content,
      version: nextVersion,
    };
    invokeLifecycleCommand(
      "lsp_change_document",
      {
        projectPath: current.projectPath,
        filePath: current.filePath,
        content,
        version: nextVersion,
      },
      current.remote,
    );
  }, [activeLifecycleKey, content]);

  useEffect(() => closeLifecycleDocument, [closeLifecycleDocument]);

  return {
    supported,
    status,
    message: languageServerStatusMessage(status),
    loading,
    request,
    refreshStatus,
  };
}
