import { invoke } from "@tauri-apps/api/core";
import { Search, TerminalSquare, FileText } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties } from "react";
import { useI18n } from "../../i18n";
import {
  lspCommandName,
  lspInvokeArgs,
  type LspRemoteContext,
} from "../../hooks/languageServerState";
import type { LspSymbol } from "../../types";
import type { ProjectFileSearchResult } from "../file-explorer/types";
import {
  commandPaletteModeForInput,
  moveCommandPaletteSelection,
  rankCommandPaletteItems,
} from "./commandPaletteState";
import type { CommandPaletteItem } from "./types";

const QUICK_OPEN_LIMIT = 60;

export type CommandPaletteCommand = {
  id: string;
  title: string;
  subtitle?: string;
  keywords?: string[];
  run: () => void;
};

type CommandPaletteResult =
  | (CommandPaletteItem & { command: CommandPaletteCommand })
  | (CommandPaletteItem & { file: ProjectFileSearchResult })
  | (CommandPaletteItem & { symbol: LspSymbol });

export function CommandPalette({
  projectPath,
  activeFilePath,
  initialInput = "",
  commands,
  onOpenFile,
  onClose,
  remote,
}: {
  projectPath: string;
  activeFilePath?: string | null;
  initialInput?: string;
  commands: CommandPaletteCommand[];
  onOpenFile: (path: string, name: string, selection?: { line: number; column?: number }) => void;
  onClose: () => void;
  remote?: LspRemoteContext;
}) {
  const { t } = useI18n();
  const [input, setInput] = useState(initialInput);
  const [files, setFiles] = useState<ProjectFileSearchResult[]>([]);
  const [symbols, setSymbols] = useState<LspSymbol[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [activeIndex, setActiveIndex] = useState(0);
  const requestIdRef = useRef(0);
  const parsed = commandPaletteModeForInput(input);

  useEffect(() => {
    setActiveIndex(0);
  }, [parsed.mode, parsed.query]);

  useEffect(() => {
    if (parsed.mode !== "file") return;
    requestIdRef.current += 1;
    const requestId = requestIdRef.current;
    setLoading(true);
    setError(null);
    const timer = window.setTimeout(() => {
      invoke<ProjectFileSearchResult[]>("search_project_files", {
        projectPath,
        query: parsed.query,
        extensions: [],
        limit: QUICK_OPEN_LIMIT,
      })
        .then((results) => {
          if (requestId === requestIdRef.current) setFiles(results);
        })
        .catch((err: unknown) => {
          if (requestId !== requestIdRef.current) return;
          setFiles([]);
          setError(String(err));
        })
        .finally(() => {
          if (requestId === requestIdRef.current) setLoading(false);
        });
    }, 120);
    return () => window.clearTimeout(timer);
  }, [parsed.mode, parsed.query, projectPath]);

  useEffect(() => {
    if (parsed.mode !== "documentSymbol" && parsed.mode !== "workspaceSymbol") return;
    requestIdRef.current += 1;
    const requestId = requestIdRef.current;
    setLoading(true);
    setError(null);
    const timer = window.setTimeout(() => {
      const load =
        parsed.mode === "documentSymbol"
          ? activeFilePath
            ? invoke<string>(
                remote ? "remote_read_file_content" : "read_file_content",
                remote
                  ? {
                      connection: remote.connection,
                      remotePath: activeFilePath,
                      remoteProjectPath: remote.projectPath,
                    }
                  : { path: activeFilePath, projectPath },
              ).then((content) =>
                invoke<LspSymbol[]>(
                  lspCommandName("lsp_document_symbols", remote),
                  lspInvokeArgs(
                    {
                      request: {
                        projectPath,
                        filePath: activeFilePath,
                        content,
                        line: 0,
                        character: 0,
                      },
                    },
                    remote,
                  ),
                ),
              )
            : Promise.resolve([])
          : invoke<LspSymbol[]>(
              lspCommandName("lsp_workspace_symbols", remote),
              lspInvokeArgs(
                {
                  projectPath,
                  query: parsed.query,
                },
                remote,
              ),
            );
      load
        .then((results) => {
          if (requestId === requestIdRef.current) setSymbols(results);
        })
        .catch((err: unknown) => {
          if (requestId !== requestIdRef.current) return;
          setSymbols([]);
          setError(String(err));
        })
        .finally(() => {
          if (requestId === requestIdRef.current) setLoading(false);
        });
    }, 120);
    return () => window.clearTimeout(timer);
  }, [activeFilePath, parsed.mode, parsed.query, projectPath, remote]);

  const results: CommandPaletteResult[] = useMemo(() => {
    const commandItems: CommandPaletteResult[] = commands.map((command) => ({
      id: command.id,
      title: command.title,
      subtitle: command.subtitle,
      kind: "command",
      keywords: command.keywords,
      command,
    }));
    const fileItems: CommandPaletteResult[] = files.map((file) => ({
      id: file.path,
      title: file.name,
      subtitle: file.dir,
      kind: "file",
      file,
    }));
    const symbolItems: CommandPaletteResult[] = symbols.map((symbol) => ({
      id: `${symbol.path}:${symbol.selectionRange.start.line}:${symbol.selectionRange.start.character}:${symbol.name}`,
      title: symbol.name,
      subtitle: symbolSubtitle(symbol),
      kind: parsed.mode === "workspaceSymbol" ? "workspaceSymbol" : "documentSymbol",
      keywords: [symbol.detail, symbol.containerName].filter((value): value is string =>
        Boolean(value),
      ),
      symbol,
    }));
    return rankCommandPaletteItems(
      [...commandItems, ...fileItems, ...symbolItems],
      parsed.query,
      parsed.mode,
    ) as CommandPaletteResult[];
  }, [commands, files, parsed.mode, parsed.query, symbols]);

  const executeResult = (result: CommandPaletteResult | undefined) => {
    if (!result) return;
    if ("command" in result) {
      result.command.run();
    } else if ("file" in result) {
      onOpenFile(result.file.path, result.file.name);
    } else {
      onOpenFile(result.symbol.path, fileNameFromPath(result.symbol.path), {
        line: result.symbol.selectionRange.start.line + 1,
        column: result.symbol.selectionRange.start.character + 1,
      });
    }
    onClose();
  };

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={t("commandPalette.title")}
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 1200,
        background: "rgba(0,0,0,0.28)",
        display: "flex",
        alignItems: "flex-start",
        justifyContent: "center",
        paddingTop: 78,
      }}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        style={{
          width: "min(680px, calc(100vw - 28px))",
          maxHeight: "min(520px, calc(100vh - 120px))",
          display: "flex",
          flexDirection: "column",
          border: "1px solid var(--border-dim)",
          borderRadius: 8,
          background: "var(--bg-panel)",
          boxShadow: "0 18px 50px rgba(0,0,0,0.35)",
          overflow: "hidden",
        }}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div
          style={{
            height: 42,
            display: "flex",
            alignItems: "center",
            gap: 9,
            padding: "0 12px",
            borderBottom: "1px solid var(--border-dim)",
          }}
        >
          <Search size={15} color="var(--text-hint)" />
          <input
            autoFocus
            value={input}
            onChange={(event) => setInput(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                onClose();
              } else if (event.key === "ArrowDown") {
                event.preventDefault();
                setActiveIndex((index) => moveCommandPaletteSelection(index, 1, results.length));
              } else if (event.key === "ArrowUp") {
                event.preventDefault();
                setActiveIndex((index) => moveCommandPaletteSelection(index, -1, results.length));
              } else if (event.key === "Enter") {
                event.preventDefault();
                executeResult(results[activeIndex]);
              }
            }}
            placeholder={
              parsed.mode === "command"
                ? t("commandPalette.commandPlaceholder")
                : parsed.mode === "documentSymbol"
                  ? t("commandPalette.documentSymbolPlaceholder")
                  : parsed.mode === "workspaceSymbol"
                    ? t("commandPalette.workspaceSymbolPlaceholder")
                    : t("commandPalette.filePlaceholder")
            }
            style={{
              flex: 1,
              minWidth: 0,
              border: "none",
              outline: "none",
              background: "transparent",
              color: "var(--text-primary)",
              fontSize: 13,
            }}
          />
          <span style={{ fontSize: 11, color: "var(--text-hint)" }}>
            {parsed.mode === "command"
              ? ">"
              : parsed.mode === "documentSymbol"
                ? "@"
                : parsed.mode === "workspaceSymbol"
                  ? "#"
                  : "⌘P"}
          </span>
        </div>
        <div style={{ overflowY: "auto", padding: 6 }}>
          {loading && parsed.mode !== "command" ? (
            <div style={emptyStyle}>{t("common.loading")}</div>
          ) : error ? (
            <div style={emptyStyle}>{t("commandPalette.failed", { error })}</div>
          ) : results.length === 0 ? (
            <div style={emptyStyle}>{t("commandPalette.noResults")}</div>
          ) : (
            results.map((result, index) => {
              const active = index === activeIndex;
              return (
                <button
                  key={`${result.kind}:${result.id}`}
                  type="button"
                  onMouseEnter={() => setActiveIndex(index)}
                  onClick={() => executeResult(result)}
                  style={{
                    width: "100%",
                    height: 38,
                    display: "grid",
                    gridTemplateColumns: "22px minmax(0, 1fr)",
                    alignItems: "center",
                    gap: 8,
                    padding: "0 9px",
                    border: "none",
                    borderRadius: 6,
                    background: active ? "var(--bg-hover)" : "transparent",
                    color: "var(--text-primary)",
                    textAlign: "left",
                    cursor: "pointer",
                  }}
                >
                  {"command" in result ? (
                    <TerminalSquare size={15} color="var(--text-hint)" />
                  ) : (
                    <FileText size={15} color="var(--text-hint)" />
                  )}
                  <span style={{ minWidth: 0, display: "flex", flexDirection: "column", gap: 2 }}>
                    <span
                      style={{
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                        fontSize: 12.5,
                        fontWeight: 600,
                      }}
                    >
                      {result.title}
                    </span>
                    {result.subtitle && (
                      <span
                        style={{
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                          fontSize: 10.5,
                          color: "var(--text-hint)",
                        }}
                      >
                        {result.subtitle}
                      </span>
                    )}
                  </span>
                </button>
              );
            })
          )}
        </div>
      </div>
    </div>
  );
}

const emptyStyle = {
  padding: "28px 12px",
  color: "var(--text-muted)",
  textAlign: "center",
  fontSize: 12,
} satisfies CSSProperties;

function fileNameFromPath(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

function symbolSubtitle(symbol: LspSymbol): string {
  return [symbol.containerName, symbol.detail, symbol.path].filter(Boolean).join(" · ");
}
