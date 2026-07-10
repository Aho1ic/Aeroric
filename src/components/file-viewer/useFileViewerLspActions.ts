import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { DiagnosticItem } from "../../types";
import type { OpenFileSelection } from "../../hooks/projectPanelsState";
import type { LanguageServerState } from "../../hooks/useLanguageServer";
import type { LspDocumentRequest, LspRemoteContext } from "../../hooks/languageServerState";
import { useI18n } from "../../i18n";
import {
  FILE_VIEWER_COMMAND_EVENT,
  isFileViewerCommand,
  type FileViewerCommand,
} from "./editorCommandEvents";
import {
  diagnosticsForLspCodeAction,
  executeLspCommand,
  requestLspCodeActions,
  type LspCodeAction,
} from "./lspCodeActions";
import {
  findLspReferences,
  lspReferenceKey,
  lspReferencePreviewLine,
  lspReferenceToOpenTarget,
  type LspReferenceLocation,
} from "./lspReferences";
import {
  applyLspWorkspaceEdit,
  requestLspRename,
  type LspApplyWorkspaceEditSummary,
  type LspWorkspaceEdit,
} from "./lspRename";
import type { ReferencePreviewState } from "./LspActionDialogs";

type SaveStatus = "idle" | "dirty" | "saving" | "saved" | "error";

type UseFileViewerLspActionsOptions = {
  projectPath: string;
  filePath: string;
  content: string | null;
  remote?: LspRemoteContext;
  currentFileDiagnostics: DiagnosticItem[];
  isPreviewableImage: boolean;
  languageServer: Pick<LanguageServerState, "supported" | "status" | "message">;
  currentRequest: () => LspDocumentRequest | null;
  saveStatus: SaveStatus;
  saveContent: (
    value: string,
    options?: {
      formatAfterSave?: boolean;
    },
  ) => Promise<boolean>;
  onOpenDefinition?: (path: string, name: string, selection?: OpenFileSelection) => void;
  onCurrentFileRefreshed: (content: string) => void;
};

export function useFileViewerLspActions({
  projectPath,
  filePath,
  content,
  remote,
  currentFileDiagnostics,
  isPreviewableImage,
  languageServer,
  currentRequest,
  saveStatus,
  saveContent,
  onOpenDefinition,
  onCurrentFileRefreshed,
}: UseFileViewerLspActionsOptions) {
  const { t } = useI18n();
  const [navigationError, setNavigationError] = useState<string | null>(null);
  const [referencesLoading, setReferencesLoading] = useState(false);
  const [references, setReferences] = useState<LspReferenceLocation[] | null>(null);
  const [referencePreviews, setReferencePreviews] = useState<Record<string, ReferencePreviewState>>(
    {},
  );
  const [renameOpen, setRenameOpen] = useState(false);
  const [renameName, setRenameName] = useState("");
  const [renameLoading, setRenameLoading] = useState(false);
  const [renameApplying, setRenameApplying] = useState(false);
  const [renamePreview, setRenamePreview] = useState<LspWorkspaceEdit | null>(null);
  const [renameSummary, setRenameSummary] = useState<LspApplyWorkspaceEditSummary | null>(null);
  const [codeActionsLoading, setCodeActionsLoading] = useState(false);
  const [codeActions, setCodeActions] = useState<LspCodeAction[] | null>(null);
  const [codeActionApplying, setCodeActionApplying] = useState(false);
  const [codeActionSummary, setCodeActionSummary] = useState<LspApplyWorkspaceEditSummary | null>(
    null,
  );
  const [codeActionCommandSummary, setCodeActionCommandSummary] = useState<string | null>(null);
  const referencePreviewRunRef = useRef(0);

  const closeReferences = useCallback(() => {
    setReferences(null);
    setReferencePreviews({});
    referencePreviewRunRef.current += 1;
  }, []);

  const reset = useCallback(() => {
    setNavigationError(null);
    closeReferences();
    setReferencesLoading(false);
    setRenameOpen(false);
    setRenameName("");
    setRenameLoading(false);
    setRenameApplying(false);
    setRenamePreview(null);
    setRenameSummary(null);
    setCodeActionsLoading(false);
    setCodeActions(null);
    setCodeActionApplying(false);
    setCodeActionSummary(null);
    setCodeActionCommandSummary(null);
  }, [closeReferences]);

  const clearForContentChange = useCallback(() => {
    setNavigationError(null);
    setReferences(null);
    setRenameSummary(null);
    setCodeActions(null);
    setCodeActionSummary(null);
  }, []);

  const loadReferencePreviews = useCallback(
    async (locations: LspReferenceLocation[], sourceContent: string) => {
      const runId = referencePreviewRunRef.current + 1;
      referencePreviewRunRef.current = runId;
      setReferencePreviews(
        Object.fromEntries(
          locations.map((location, index) => [
            lspReferenceKey(location, index),
            { status: "loading" },
          ]),
        ),
      );

      const entries = await Promise.all(
        locations.map(async (location, index): Promise<[string, ReferencePreviewState]> => {
          const key = lspReferenceKey(location, index);
          try {
            const targetContent =
              location.path === filePath
                ? sourceContent
                : await invoke<string>(
                    remote ? "remote_read_file_content" : "read_file_content",
                    remote
                      ? {
                          connection: remote.connection,
                          remotePath: location.path,
                          remoteProjectPath: remote.projectPath,
                        }
                      : { path: location.path, projectPath },
                  );
            return [
              key,
              { status: "ready", preview: lspReferencePreviewLine(targetContent, location) },
            ];
          } catch (err) {
            return [key, { status: "error", error: String(err) }];
          }
        }),
      );

      if (referencePreviewRunRef.current !== runId) return;
      setReferencePreviews(Object.fromEntries(entries));
    },
    [filePath, projectPath, remote],
  );

  const findReferences = useCallback(async () => {
    const request = currentRequest();
    if (!request) return;
    closeReferences();
    setNavigationError(null);
    if (!languageServer.status?.available) {
      setNavigationError(languageServer.message ?? "Language server is unavailable.");
      return;
    }
    setReferencesLoading(true);
    try {
      const nextReferences = await findLspReferences(request, remote);
      if (nextReferences.length === 0) {
        setNavigationError(t("file.noReferencesFound"));
        return;
      }
      setReferences(nextReferences);
      void loadReferencePreviews(nextReferences, request.content);
    } catch (err) {
      setNavigationError(String(err));
    } finally {
      setReferencesLoading(false);
    }
  }, [closeReferences, currentRequest, languageServer, loadReferencePreviews, remote, t]);

  const openReference = useCallback(
    (reference: LspReferenceLocation) => {
      const target = lspReferenceToOpenTarget(reference);
      closeReferences();
      onOpenDefinition?.(target.path, target.name, target.selection);
    },
    [closeReferences, onOpenDefinition],
  );

  const openRename = useCallback(() => {
    setReferences(null);
    setNavigationError(null);
    setRenameSummary(null);
    setRenamePreview(null);
    setRenameName("");
    setRenameOpen(true);
  }, []);

  const previewRename = useCallback(async () => {
    const request = currentRequest();
    if (!request || content === null) return;
    const nextName = renameName.trim();
    if (!nextName) return;
    setNavigationError(null);
    setRenameSummary(null);
    setRenamePreview(null);
    if (!languageServer.status?.available) {
      setNavigationError(languageServer.message ?? "Language server is unavailable.");
      return;
    }
    setRenameLoading(true);
    try {
      if (saveStatus === "dirty") {
        const saved = await saveContent(content, { formatAfterSave: false });
        if (!saved) return;
      }
      setRenamePreview(await requestLspRename(request, nextName, remote));
    } catch (err) {
      setNavigationError(String(err));
    } finally {
      setRenameLoading(false);
    }
  }, [content, currentRequest, languageServer, remote, renameName, saveContent, saveStatus]);

  const refreshCurrentFileAfterWorkspaceEdit = useCallback(
    async (edit: LspWorkspaceEdit) => {
      if (!edit.files.some((file) => file.path === filePath)) return;
      const nextContent = await invoke<string>(
        remote ? "remote_read_file_content" : "read_file_content",
        remote
          ? {
              connection: remote.connection,
              remotePath: filePath,
              remoteProjectPath: remote.projectPath,
            }
          : {
              path: filePath,
              projectPath,
            },
      );
      onCurrentFileRefreshed(nextContent);
    },
    [filePath, onCurrentFileRefreshed, projectPath, remote],
  );

  const applyRename = useCallback(async () => {
    if (!renamePreview || renameApplying) return;
    setNavigationError(null);
    setRenameApplying(true);
    try {
      setRenameSummary(await applyLspWorkspaceEdit(projectPath, renamePreview, remote));
      setRenameOpen(false);
      setRenamePreview(null);
      await refreshCurrentFileAfterWorkspaceEdit(renamePreview);
    } catch (err) {
      setNavigationError(String(err));
    } finally {
      setRenameApplying(false);
    }
  }, [projectPath, refreshCurrentFileAfterWorkspaceEdit, remote, renameApplying, renamePreview]);

  const quickFix = useCallback(async () => {
    const request = currentRequest();
    if (!request || content === null) return;
    setNavigationError(null);
    setReferences(null);
    setRenameOpen(false);
    setCodeActions(null);
    setCodeActionSummary(null);
    setCodeActionCommandSummary(null);
    if (!languageServer.status?.available) {
      setNavigationError(languageServer.message ?? "Language server is unavailable.");
      return;
    }
    setCodeActionsLoading(true);
    try {
      if (saveStatus === "dirty") {
        const saved = await saveContent(content, { formatAfterSave: false });
        if (!saved) return;
      }
      const actions = await requestLspCodeActions(
        request,
        diagnosticsForLspCodeAction(request, currentFileDiagnostics),
        remote,
      );
      if (actions.length === 0) {
        setNavigationError(t("file.noCodeActionsFound"));
        return;
      }
      setCodeActions(actions);
    } catch (err) {
      setNavigationError(String(err));
    } finally {
      setCodeActionsLoading(false);
    }
  }, [
    content,
    currentFileDiagnostics,
    currentRequest,
    languageServer,
    remote,
    saveContent,
    saveStatus,
    t,
  ]);

  const applyCodeAction = useCallback(
    async (action: LspCodeAction) => {
      if ((!action.edit && !action.command) || codeActionApplying) return;
      const request = currentRequest();
      if (action.command && !request) return;
      setNavigationError(null);
      setCodeActionApplying(true);
      try {
        if (action.edit) {
          setCodeActionSummary(await applyLspWorkspaceEdit(projectPath, action.edit, remote));
          await refreshCurrentFileAfterWorkspaceEdit(action.edit);
        }
        if (action.command && request) {
          await executeLspCommand(request, action.command, remote);
          setCodeActionCommandSummary(action.command.title ?? action.title);
        }
        setCodeActions(null);
      } catch (err) {
        setNavigationError(String(err));
      } finally {
        setCodeActionApplying(false);
      }
    },
    [codeActionApplying, currentRequest, projectPath, refreshCurrentFileAfterWorkspaceEdit, remote],
  );

  const runEditorCommand = useCallback(
    (command: FileViewerCommand) => {
      if (isPreviewableImage || content === null || !languageServer.supported) return;
      if (command === "findReferences") {
        void findReferences();
      } else if (command === "renameSymbol") {
        openRename();
      } else {
        void quickFix();
      }
    },
    [content, findReferences, isPreviewableImage, languageServer.supported, openRename, quickFix],
  );

  useEffect(() => {
    const onEditorCommand = (event: Event) => {
      const command = (event as CustomEvent<{ command?: unknown }>).detail?.command;
      if (!isFileViewerCommand(command)) return;
      runEditorCommand(command);
    };
    window.addEventListener(FILE_VIEWER_COMMAND_EVENT, onEditorCommand);
    return () => window.removeEventListener(FILE_VIEWER_COMMAND_EVENT, onEditorCommand);
  }, [runEditorCommand]);

  return {
    state: {
      navigationError,
      referencesLoading,
      references,
      referencePreviews,
      renameOpen,
      renameName,
      renameLoading,
      renameApplying,
      renamePreview,
      renameSummary,
      codeActionsLoading,
      codeActions,
      codeActionApplying,
      codeActionSummary,
      codeActionCommandSummary,
    },
    actions: {
      setNavigationError,
      setRenameName,
      setRenameOpen,
      setCodeActions,
      closeReferences,
      reset,
      clearForContentChange,
      findReferences,
      openReference,
      openRename,
      previewRename,
      applyRename,
      quickFix,
      applyCodeAction,
      runEditorCommand,
    },
  };
}
