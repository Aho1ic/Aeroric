import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  buildLspDocumentRequest,
  isLspSupportedFile,
  languageServerStatusMessage,
  type LspRemoteContext,
  type LspDocumentRequest,
  type LspServerStatus,
} from "./languageServerState";
import { LSP_MIRRORS } from "../lib/api/lsp";
import { projectArgs, resolveCommand } from "../lib/invokeFacade";
import { localTarget, type InvokeTarget } from "../lib/target";

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

function lspInvokeTarget(
  projectPath: string,
  remote?: LspRemoteContext,
): InvokeTarget {
  if (!remote) return localTarget(projectPath);
  return {
    kind: "ssh",
    connection: remote.connection,
    projectPath: remote.projectPath,
  };
}

/** LSP 请求体里的 `projectPath` 始终是「项目根路径」（SSH 时为远端路径）。 */
function lspProjectPath(target: InvokeTarget, fallback: string): string {
  if (target.kind === "local") return target.path || fallback;
  return target.projectPath;
}

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
  key: keyof typeof LSP_MIRRORS,
  args: Record<string, unknown>,
  target: InvokeTarget,
) {
  void Promise.resolve(invoke(resolveCommand(LSP_MIRRORS[key], target), args)).catch(() => {});
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
  const invokeTarget = useMemo(
    () => lspInvokeTarget(projectPath, remote),
    [projectPath, remote],
  );

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
        resolveCommand(LSP_MIRRORS.serverStatus, invokeTarget),
        {
          ...projectArgs(invokeTarget),
          projectPath: lspProjectPath(invokeTarget, projectPath),
          filePath,
        },
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
  }, [filePath, invokeTarget, projectPath, remote, supported]);

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  const closeLifecycleDocument = useCallback(() => {
    const current = lifecycleRef.current;
    if (!current) return;
    lifecycleRef.current = null;
    const target = lspInvokeTarget(current.projectPath, current.remote);
    invokeLifecycleCommand(
      "closeDocument",
      {
        ...projectArgs(target),
        projectPath: lspProjectPath(target, current.projectPath),
        filePath: current.filePath,
      },
      target,
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
      "openDocument",
      {
        ...projectArgs(invokeTarget),
        projectPath: lspProjectPath(invokeTarget, projectPath),
        filePath,
        content,
        version: 1,
      },
      invokeTarget,
    );
  }, [activeLifecycleKey, closeLifecycleDocument, content, filePath, invokeTarget, projectPath, remote]);

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
    const target = lspInvokeTarget(current.projectPath, current.remote);
    invokeLifecycleCommand(
      "changeDocument",
      {
        ...projectArgs(target),
        projectPath: lspProjectPath(target, current.projectPath),
        filePath: current.filePath,
        content,
        version: nextVersion,
      },
      target,
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
