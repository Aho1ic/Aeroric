import { useCallback, useState, useEffect, useRef, useMemo, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import { Marked } from "marked";
import DOMPurify from "dompurify";
import * as Popover from "@radix-ui/react-popover";
import {
  X,
  AlertCircle,
  Eye,
  PencilLine,
  MoreHorizontal,
  List,
  ChevronRight,
  Play,
  Check,
  WandSparkles,
  Columns2,
  GitBranch,
  Search,
  Lightbulb,
  Copy,
} from "lucide-react";
import { getFileColor } from "../utils";
import ReactCodeMirror, {
  Decoration,
  EditorView,
  GutterMarker,
  WidgetType,
  gutter,
  type ViewUpdate,
} from "@uiw/react-codemirror";
import { githubDark, githubLight } from "@uiw/codemirror-theme-github";
import { solarizedLight } from "@uiw/codemirror-theme-solarized";
import { StreamLanguage } from "@codemirror/language";
import type { Extension } from "@codemirror/state";
import { ImagePreviewPane } from "./file-viewer/ImagePreviewPane";
import type { OpenFileTab } from "../hooks/useProjectPanels";
import type {
  CondaEnvironment,
  DebugBreakpoint,
  DiagnosticItem,
  DiagnosticSeverity,
  FormatFileResult,
  GitBlameLine,
  GitBlameResult,
  LspInlayHint,
  LspSymbol,
  LocalHistoryEntry,
  LocalHistorySnapshot,
  SshConnection,
  TestCoverageSummary,
  ThemeVariant,
} from "../types";
import { useI18n } from "../i18n";
import { isRunnableScriptFile, selectRunnableCondaEnvironment } from "./file-viewer/run";
import { lineColumnToOffset } from "./file-viewer/position";
import type { OpenFileSelection } from "../hooks/projectPanelsState";
import { useLanguageServer } from "../hooks/useLanguageServer";
import { buildLspDocumentRequest } from "../hooks/languageServerState";
import { createLspCompletionExtension } from "./file-viewer/lspCompletion";
import { createLspHoverExtension } from "./file-viewer/lspHover";
import { createLspNavigationExtension, type LspOpenTarget } from "./file-viewer/lspNavigation";
import { createLspSignatureHelpExtension } from "./file-viewer/lspSignatureHelp";
import {
  findLspReferences,
  lspReferenceKey,
  lspReferencePreviewLine,
  lspReferenceToOpenTarget,
  type LspReferencePreview,
  type LspReferenceLocation,
} from "./file-viewer/lspReferences";
import {
  activeSymbolBreadcrumbs,
  fileBreadcrumbSegments,
  lspSymbolToSelection,
  outlineSymbolDepth,
  outlineSymbolKey,
  requestLspDocumentOutline,
} from "./file-viewer/lspOutline";
import {
  createLspInlayHintsExtension,
  requestLspInlayHints,
} from "./file-viewer/lspInlayHints";
import {
  applyLspWorkspaceEdit,
  requestLspRename,
  type LspApplyWorkspaceEditSummary,
  type LspWorkspaceEdit,
} from "./file-viewer/lspRename";
import {
  diagnosticsForLspCodeAction,
  executeLspCommand,
  requestLspCodeActions,
  type LspCodeAction,
} from "./file-viewer/lspCodeActions";
import {
  FILE_VIEWER_COMMAND_EVENT,
  isFileViewerCommand,
  type FileViewerCommand,
} from "./file-viewer/editorCommandEvents";
import {
  createDiagnosticsExtension,
  diagnosticsClipboardText,
  diagnosticSeverityCounts,
  diagnosticsForFile,
  filterDiagnosticsBySeverity,
  groupDiagnosticsBySource,
  nextDiagnosticTarget,
  type DiagnosticSeverityFilter,
} from "./file-viewer/diagnosticsExtension";
import {
  createTestRunGutter,
  testRunTargetsForContent,
  type EditorTestRunTarget,
} from "./file-viewer/testRunGutter";
import {
  coverageLinesForFile,
  createCoverageExtension,
} from "./file-viewer/coverageExtension";
import { debugBreakpointLinesForFile } from "./debug/debugBreakpointState";
import {
  inlineBlameText,
  inlineBlameTitle,
  projectRelativeGitPath,
} from "./git-advanced/gitAdvancedState";

type RemoteFileContext = {
  connection: SshConnection;
  projectPath: string;
};

type ProjectEditorSettings = {
  editor?: {
    format_on_save?: boolean;
  };
};

type SaveContentOptions = {
  formatAfterSave?: boolean;
};

function isMarkdownFile(fileName: string): boolean {
  const ext = fileName.split(".").pop()?.toLowerCase();
  return ext === "md" || ext === "mdx" || ext === "markdown";
}

type TocEntry = { depth: number; text: string; id: string };

// Render markdown to sanitized HTML and extract a table of contents in a single
// pass, so heading ids in the HTML and the TOC anchors are guaranteed to match.
function renderMarkdownWithToc(content: string): { html: string; toc: TocEntry[] } {
  const used = new Set<string>();
  const toc: TocEntry[] = [];
  const instance = new Marked({
    renderer: {
      heading(token) {
        const inlineHtml = this.parser.parseInline(token.tokens);
        const plain = inlineHtml.replace(/<[^>]*>/g, "").trim();
        const base =
          plain
            .toLowerCase()
            .replace(/[^\w一-龥 -]/g, "")
            .replace(/\s+/g, "-")
            .replace(/-+/g, "-")
            .replace(/^-+|-+$/g, "") || "section";
        let id = base;
        let n = 1;
        while (used.has(id)) id = `${base}-${n++}`;
        used.add(id);
        toc.push({ depth: token.depth, text: plain, id });
        return `<h${token.depth} id="${id}">${inlineHtml}</h${token.depth}>\n`;
      },
    },
  });
  const html = instance.parse(content, { async: false }) as string;
  return { html: DOMPurify.sanitize(html), toc };
}

function isPreviewableImageFile(fileName: string): boolean {
  const ext = fileName.split(".").pop()?.toLowerCase();
  return (
    ext === "png" ||
    ext === "jpg" ||
    ext === "jpeg" ||
    ext === "gif" ||
    ext === "webp" ||
    ext === "bmp" ||
    ext === "svg"
  );
}

async function loadLanguageExtension(fileName: string): Promise<Extension> {
  const shellLanguage = async () => {
    const { shell } = await import("@codemirror/legacy-modes/mode/shell");
    return StreamLanguage.define(shell);
  };
  const rubyLanguage = async () => {
    const { ruby } = await import("@codemirror/legacy-modes/mode/ruby");
    return StreamLanguage.define(ruby);
  };

  const nameMap: Record<string, () => Promise<Extension>> = {
    dockerfile: async () => {
      const { dockerFile } = await import("@codemirror/legacy-modes/mode/dockerfile");
      return StreamLanguage.define(dockerFile);
    },
    "dockerfile.dev": async () => {
      const { dockerFile } = await import("@codemirror/legacy-modes/mode/dockerfile");
      return StreamLanguage.define(dockerFile);
    },
    "dockerfile.prod": async () => {
      const { dockerFile } = await import("@codemirror/legacy-modes/mode/dockerfile");
      return StreamLanguage.define(dockerFile);
    },
    makefile: shellLanguage,
    gnumakefile: shellLanguage,
    justfile: shellLanguage,
    gemfile: rubyLanguage,
    rakefile: rubyLanguage,
    vagrantfile: rubyLanguage,
    procfile: shellLanguage,
    "cmakelists.txt": shellLanguage,
    ".gitignore": shellLanguage,
    ".dockerignore": shellLanguage,
    ".env": shellLanguage,
    ".env.local": shellLanguage,
    ".env.example": shellLanguage,
    ".npmrc": async () => {
      const { toml } = await import("@codemirror/legacy-modes/mode/toml");
      return StreamLanguage.define(toml);
    },
    ".yarnrc": async () => (await import("@codemirror/lang-yaml")).yaml(),
    "changelog.md": async () => (await import("@codemirror/lang-markdown")).markdown(),
    readme: async () => (await import("@codemirror/lang-markdown")).markdown(),
  };

  const lower = fileName.toLowerCase();
  if (nameMap[lower]) return nameMap[lower]();

  const ext = fileName.split(".").pop()?.toLowerCase() ?? "";
  switch (ext) {
    case "ts":
      return (await import("@codemirror/lang-javascript")).javascript({ typescript: true });
    case "tsx":
      return (await import("@codemirror/lang-javascript")).javascript({
        jsx: true,
        typescript: true,
      });
    case "js":
    case "mjs":
    case "cjs":
      return (await import("@codemirror/lang-javascript")).javascript();
    case "jsx":
      return (await import("@codemirror/lang-javascript")).javascript({ jsx: true });
    case "json":
    case "jsonc":
      return (await import("@codemirror/lang-json")).json();
    case "rs":
      return (await import("@codemirror/lang-rust")).rust();
    case "html":
    case "htm":
      return (await import("@codemirror/lang-html")).html();
    case "css":
    case "scss":
    case "sass":
      return (await import("@codemirror/lang-css")).css();
    case "md":
    case "mdx":
      return (await import("@codemirror/lang-markdown")).markdown();
    case "yaml":
    case "yml":
      return (await import("@codemirror/lang-yaml")).yaml();
    case "toml":
      return StreamLanguage.define((await import("@codemirror/legacy-modes/mode/toml")).toml);
    case "sh":
    case "bash":
    case "zsh":
    case "fish":
      return shellLanguage();
    case "py":
      return (await import("@codemirror/lang-python")).python();
    case "go":
      return (await import("@codemirror/lang-go")).go();
    case "java":
      return (await import("@codemirror/lang-java")).java();
    case "c":
    case "h":
      return (await import("@codemirror/lang-cpp")).cpp();
    case "cpp":
    case "cc":
    case "hpp":
      return (await import("@codemirror/lang-cpp")).cpp();
    case "sql":
      return (await import("@codemirror/lang-sql")).sql();
    case "xml":
      return (await import("@codemirror/lang-xml")).xml();
    case "swift":
      return StreamLanguage.define((await import("@codemirror/legacy-modes/mode/swift")).swift);
    case "kt":
      return StreamLanguage.define((await import("@codemirror/legacy-modes/mode/clike")).kotlin);
    case "rb":
      return rubyLanguage();
    case "lua":
      return StreamLanguage.define((await import("@codemirror/legacy-modes/mode/lua")).lua);
    case "r":
      return StreamLanguage.define((await import("@codemirror/legacy-modes/mode/r")).r);
    case "proto":
      return shellLanguage();
    default:
      return [];
  }
}

const editorBaseTheme = EditorView.theme({
  "&": {
    height: "100%",
    fontFamily: "var(--font-mono)",
    fontSize: "13px",
    background: "var(--bg-panel)",
    "-webkit-user-select": "text",
    "user-select": "text",
  },
  ".cm-editor": {
    background: "var(--bg-panel)",
  },
  ".cm-scroller": {
    overflow: "auto",
    lineHeight: "1.6",
    background: "var(--bg-panel)",
    "-webkit-user-select": "text",
    "user-select": "text",
  },
  ".cm-content": {
    padding: "12px 0",
    caretColor: "var(--text-primary)",
    "-webkit-user-select": "text",
    "user-select": "text",
  },
  ".cm-gutters": {
    borderRight: "1px solid var(--border-dim)",
    background: "var(--bg-panel)",
    fontSize: "12px",
    minWidth: "44px",
  },
  ".cm-lineNumbers .cm-gutterElement": {
    padding: "0 8px 0 4px",
    color: "var(--text-hint)",
  },
  ".cm-activeLineGutter": {
    background: "var(--code-line-hover-bg)",
  },
  ".cm-focused .cm-activeLine, .cm-activeLine": {
    background: "var(--code-line-hover-bg)",
  },
  ".cm-debug-breakpoint-gutter .cm-gutterElement": {
    width: "18px",
    padding: "0 3px",
    cursor: "pointer",
  },
  ".cm-debug-breakpoint-marker": {
    width: "8px",
    height: "8px",
    display: "inline-block",
    borderRadius: "999px",
    border: "1px solid transparent",
    boxSizing: "border-box",
  },
  ".cm-debug-breakpoint-marker.active": {
    background: "var(--danger)",
    borderColor: "var(--danger)",
    boxShadow: "0 0 0 2px color-mix(in srgb, var(--danger) 18%, transparent)",
  },
  ".cm-debug-breakpoint-marker.spacer": {
    opacity: 0,
  },
  ".cm-inline-blame": {
    marginLeft: "16px",
    color: "var(--text-hint)",
    fontSize: "11px",
    fontFamily: "var(--font-mono)",
    opacity: 0.72,
    whiteSpace: "nowrap",
    pointerEvents: "none",
  },
  ".cm-line:hover .cm-inline-blame": {
    opacity: 1,
  },
  ".cm-lsp-hover-tooltip": {
    maxWidth: "520px",
    maxHeight: "260px",
    overflow: "auto",
    padding: "8px 10px",
    border: "1px solid var(--border)",
    borderRadius: "6px",
    background: "var(--bg-card)",
    color: "var(--text-primary)",
    boxShadow: "0 10px 28px color-mix(in srgb, #000 24%, transparent)",
    fontFamily: "var(--font-mono)",
    fontSize: "12px",
    lineHeight: "1.5",
    whiteSpace: "normal",
  },
  ".cm-lsp-hover-tooltip p, .cm-lsp-signature-tooltip p": {
    margin: "0 0 6px",
  },
  ".cm-lsp-hover-tooltip p:last-child, .cm-lsp-signature-tooltip p:last-child": {
    marginBottom: 0,
  },
  ".cm-lsp-hover-tooltip code, .cm-lsp-signature-tooltip code": {
    padding: "1px 4px",
    borderRadius: "4px",
    background: "var(--bg-subtle)",
  },
  ".cm-lsp-signature-tooltip": {
    maxWidth: "540px",
    maxHeight: "260px",
    overflow: "auto",
    padding: "8px 10px",
    border: "1px solid var(--border)",
    borderRadius: "6px",
    background: "var(--bg-card)",
    color: "var(--text-primary)",
    boxShadow: "0 10px 28px color-mix(in srgb, #000 24%, transparent)",
    fontFamily: "var(--font-mono)",
    fontSize: "12px",
    lineHeight: "1.5",
  },
  ".cm-lsp-signature-label": {
    fontWeight: 700,
    whiteSpace: "pre-wrap",
  },
  ".cm-lsp-signature-parameter": {
    marginTop: "6px",
    color: "var(--text-secondary)",
    whiteSpace: "normal",
  },
  ".cm-lsp-signature-markdown": {
    display: "inline",
  },
  ".cm-lsp-signature-docs": {
    marginTop: "6px",
    color: "var(--text-muted)",
    whiteSpace: "pre-wrap",
  },
  ".cm-inlay-hint": {
    display: "inline-flex",
    alignItems: "center",
    maxWidth: "220px",
    margin: "0 2px",
    padding: "0 4px",
    borderRadius: "4px",
    background: "var(--bg-subtle)",
    color: "var(--text-hint)",
    fontFamily: "var(--font-mono)",
    fontSize: "0.82em",
    lineHeight: "1.35",
    whiteSpace: "nowrap",
    verticalAlign: "baseline",
    pointerEvents: "auto",
  },
  ".cm-inlay-hint[data-padding-left='true']": {
    marginLeft: "6px",
  },
  ".cm-inlay-hint[data-padding-right='true']": {
    marginRight: "6px",
  },
  ".cm-diagnostic-gutter .cm-gutterElement": {
    width: "16px",
    padding: "0 3px",
  },
  ".cm-diagnostic-marker": {
    width: "7px",
    height: "7px",
    display: "inline-block",
    borderRadius: "999px",
    verticalAlign: "middle",
  },
  ".cm-diagnostic-marker.error": {
    background: "var(--danger)",
  },
  ".cm-diagnostic-marker.warning": {
    background: "var(--warning)",
  },
  ".cm-diagnostic-marker.info": {
    background: "var(--accent)",
  },
  ".cm-diagnostic-underline": {
    textDecorationLine: "underline",
    textDecorationStyle: "wavy",
    textUnderlineOffset: "2px",
  },
  ".cm-diagnostic-underline.error": {
    textDecorationColor: "var(--danger)",
  },
  ".cm-diagnostic-underline.warning": {
    textDecorationColor: "var(--warning)",
  },
  ".cm-diagnostic-underline.info": {
    textDecorationColor: "var(--accent)",
  },
  ".cm-diagnostic-line.error": {
    background: "color-mix(in srgb, var(--danger) 7%, transparent)",
  },
  ".cm-diagnostic-line.warning": {
    background: "color-mix(in srgb, var(--warning) 7%, transparent)",
  },
  ".cm-diagnostic-line.info": {
    background: "color-mix(in srgb, var(--accent) 7%, transparent)",
  },
  ".cm-diagnostic-tooltip": {
    maxWidth: "460px",
    maxHeight: "220px",
    overflow: "auto",
    padding: "8px 10px",
    border: "1px solid var(--border)",
    borderRadius: "6px",
    background: "var(--bg-card)",
    color: "var(--text-primary)",
    boxShadow: "0 10px 28px color-mix(in srgb, #000 24%, transparent)",
    fontFamily: "var(--font-mono)",
    fontSize: "12px",
    lineHeight: "1.45",
  },
  ".cm-diagnostic-tooltip-item + .cm-diagnostic-tooltip-item": {
    marginTop: "6px",
    paddingTop: "6px",
    borderTop: "1px solid var(--border-dim)",
  },
  ".cm-diagnostic-tooltip-item.error": {
    color: "var(--danger-fg)",
  },
  ".cm-diagnostic-tooltip-item.warning": {
    color: "var(--warning)",
  },
  ".cm-coverage-line.covered": {
    boxShadow: "inset 3px 0 0 color-mix(in srgb, var(--success) 72%, transparent)",
  },
  ".cm-coverage-line.uncovered": {
    boxShadow: "inset 3px 0 0 color-mix(in srgb, var(--danger) 72%, transparent)",
  },
  ".cm-test-run-gutter .cm-gutterElement, .cm-test-debug-gutter .cm-gutterElement": {
    width: "18px",
    padding: "0 3px",
  },
  ".cm-test-run-gutter, .cm-test-debug-gutter": {
    minWidth: "18px",
  },
  ".cm-test-run-gutter, .cm-test-run-gutter .cm-gutterElement": {
    background: "var(--bg-panel)",
  },
  ".cm-test-debug-gutter, .cm-test-debug-gutter .cm-gutterElement": {
    background: "var(--bg-panel)",
  },
  ".cm-test-run-marker, .cm-test-debug-marker": {
    width: "14px",
    height: "14px",
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    border: "1px solid var(--border-dim)",
    borderRadius: "999px",
    background: "var(--bg-card)",
    color: "var(--success)",
    fontSize: "8px",
    lineHeight: 1,
    cursor: "pointer",
    padding: 0,
  },
  ".cm-test-run-marker:hover": {
    borderColor: "var(--success)",
    background: "color-mix(in srgb, var(--success) 12%, transparent)",
  },
  ".cm-test-debug-marker": {
    color: "var(--accent)",
    fontSize: "9px",
  },
  ".cm-test-debug-marker:hover": {
    borderColor: "var(--accent)",
    background: "color-mix(in srgb, var(--accent) 12%, transparent)",
  },
});

class DebugBreakpointGutterMarker extends GutterMarker {
  constructor(
    private readonly label: string,
    private readonly active: boolean,
  ) {
    super();
  }

  eq(other: GutterMarker): boolean {
    return (
      other instanceof DebugBreakpointGutterMarker &&
      other.label === this.label &&
      other.active === this.active
    );
  }

  toDOM(): Node {
    const marker = document.createElement("span");
    marker.className = `cm-debug-breakpoint-marker${this.active ? " active" : " spacer"}`;
    marker.title = this.label;
    return marker;
  }
}

function createDebugBreakpointGutter({
  breakpointLines,
  label,
  onToggleLine,
}: {
  breakpointLines: Set<number>;
  label: string;
  onToggleLine?: (line: number) => void;
}): Extension {
  if (!onToggleLine && breakpointLines.size === 0) return [];
  const activeMarker = new DebugBreakpointGutterMarker(label, true);
  const spacerMarker = new DebugBreakpointGutterMarker(label, false);
  return gutter({
    class: "cm-debug-breakpoint-gutter",
    renderEmptyElements: true,
    initialSpacer: () => spacerMarker,
    lineMarker: (view, line) => {
      const lineNumber = view.state.doc.lineAt(line.from).number;
      return breakpointLines.has(lineNumber) ? activeMarker : null;
    },
    lineMarkerChange: () => true,
    domEventHandlers: {
      mousedown(view, line, event) {
        if (!onToggleLine) return false;
        event.preventDefault();
        onToggleLine(view.state.doc.lineAt(line.from).number);
        return true;
      },
    },
  });
}

class InlineBlameWidget extends WidgetType {
  constructor(private readonly line: GitBlameLine) {
    super();
  }

  eq(other: WidgetType): boolean {
    return (
      other instanceof InlineBlameWidget &&
      other.line.commit === this.line.commit &&
      other.line.line === this.line.line &&
      other.line.author === this.line.author &&
      other.line.summary === this.line.summary
    );
  }

  toDOM(): HTMLElement {
    const marker = document.createElement("span");
    marker.className = "cm-inline-blame";
    marker.textContent = inlineBlameText(this.line);
    marker.title = inlineBlameTitle(this.line);
    return marker;
  }

  ignoreEvent(): boolean {
    return true;
  }
}

function createInlineBlameExtension({
  enabled,
  lines,
}: {
  enabled: boolean;
  lines: GitBlameLine[];
}): Extension {
  if (!enabled || lines.length === 0) return [];
  const sortedLines = [...lines].sort((a, b) => a.line - b.line).slice(0, 5000);
  return EditorView.decorations.compute([], (state) => {
    const widgets = [];
    for (const line of sortedLines) {
      if (line.line < 1 || line.line > state.doc.lines) continue;
      const docLine = state.doc.line(line.line);
      widgets.push(
        Decoration.widget({
          widget: new InlineBlameWidget(line),
          side: 1,
        }).range(docLine.to),
      );
    }
    return Decoration.set(widgets, true);
  });
}

type SaveStatus = "idle" | "dirty" | "saving" | "saved" | "error";
type ImagePreviewData = {
  dataUrl: string;
  mimeType: string;
  byteLength: number;
};

type CursorPosition = {
  line: number;
  column: number;
};

type EditorContextMenuState = {
  x: number;
  y: number;
};

const diagnosticFilterOptions: DiagnosticSeverityFilter[] = ["all", "error", "warning", "info"];

function diagnosticSeverityColor(severity: DiagnosticSeverity): string {
  if (severity === "error") return "var(--danger-fg)";
  if (severity === "warning") return "var(--warning)";
  return "var(--accent)";
}

type ReferencePreviewState =
  | { status: "loading" }
  | { status: "ready"; preview: LspReferencePreview }
  | { status: "error"; error: string };

const outlineMessageStyle: CSSProperties = {
  padding: "7px 8px",
  color: "var(--text-hint)",
  fontSize: 11.5,
  lineHeight: 1.35,
};

const outlineErrorStyle: CSSProperties = {
  ...outlineMessageStyle,
  color: "var(--warning)",
};

const breadcrumbStyle: CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  display: "flex",
  alignItems: "center",
  gap: 1,
  color: "var(--text-muted)",
  fontFamily: "var(--font-mono)",
  fontSize: 11,
  whiteSpace: "nowrap",
};

const breadcrumbSegmentStyle: CSSProperties = {
  minWidth: 0,
  display: "inline-flex",
  alignItems: "center",
  overflow: "hidden",
  flexShrink: 1,
};

const breadcrumbSymbolButtonStyle: CSSProperties = {
  ...breadcrumbSegmentStyle,
  border: "none",
  background: "transparent",
  color: "var(--text-secondary)",
  padding: 0,
  cursor: "pointer",
  font: "inherit",
};

const breadcrumbSeparatorStyle: CSSProperties = {
  flexShrink: 0,
  color: "var(--text-hint)",
};

const breadcrumbTextStyle: CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
};

const stickyScrollStyle: CSSProperties = {
  minHeight: 26,
  display: "flex",
  alignItems: "center",
  gap: 2,
  padding: "0 8px",
  borderBottom: "1px solid var(--border-dim)",
  background: "var(--bg-panel)",
  color: "var(--text-muted)",
  fontFamily: "var(--font-mono)",
  fontSize: 11,
  whiteSpace: "nowrap",
  overflow: "hidden",
  flexShrink: 0,
};

const stickyScrollButtonStyle: CSSProperties = {
  minWidth: 0,
  display: "inline-flex",
  alignItems: "center",
  gap: 2,
  border: "none",
  background: "transparent",
  color: "var(--text-secondary)",
  padding: 0,
  cursor: "pointer",
  font: "inherit",
};

const editorContextMenuStyle: CSSProperties = {
  position: "fixed",
  minWidth: 180,
  padding: 4,
  border: "1px solid var(--border-dim)",
  borderRadius: 6,
  background: "var(--bg-card)",
  boxShadow: "0 14px 38px color-mix(in srgb, #000 28%, transparent)",
  zIndex: 30,
};

const editorContextMenuItemStyle: CSSProperties = {
  width: "100%",
  height: 30,
  display: "flex",
  alignItems: "center",
  gap: 8,
  padding: "0 8px",
  border: "none",
  borderRadius: 5,
  background: "transparent",
  color: "var(--text-primary)",
  fontSize: 12,
  textAlign: "left",
  cursor: "pointer",
};

const localHistoryOverlayStyle: CSSProperties = {
  position: "absolute",
  inset: 0,
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  padding: 18,
  background: "color-mix(in srgb, #000 30%, transparent)",
  zIndex: 20,
};

const localHistoryDialogStyle: CSSProperties = {
  width: "min(980px, calc(100vw - 48px))",
  height: "min(620px, calc(100vh - 96px))",
  minHeight: 360,
  display: "flex",
  flexDirection: "column",
  border: "1px solid var(--border)",
  borderRadius: 8,
  background: "var(--bg-card)",
  boxShadow: "0 24px 60px color-mix(in srgb, #000 32%, transparent)",
  overflow: "hidden",
};

const localHistoryHeaderStyle: CSSProperties = {
  height: 40,
  display: "flex",
  alignItems: "center",
  gap: 8,
  padding: "0 12px",
  borderBottom: "1px solid var(--border-dim)",
  color: "var(--text-primary)",
  fontSize: 13,
  fontWeight: 600,
  flexShrink: 0,
};

const localHistoryBodyStyle: CSSProperties = {
  flex: 1,
  minHeight: 0,
  display: "grid",
  gridTemplateColumns: "240px minmax(0, 1fr)",
};

const localHistoryListStyle: CSSProperties = {
  minHeight: 0,
  overflowY: "auto",
  borderRight: "1px solid var(--border-dim)",
  padding: 6,
};

const localHistoryPaneStyle: CSSProperties = {
  minWidth: 0,
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
};

const localHistoryComparisonStyle: CSSProperties = {
  flex: 1,
  minHeight: 0,
  display: "grid",
  gridTemplateColumns: "1fr 1fr",
};

const localHistoryTextStyle: CSSProperties = {
  flex: 1,
  minHeight: 0,
  margin: 0,
  padding: 10,
  overflow: "auto",
  border: "none",
  borderTop: "1px solid var(--border-dim)",
  background: "var(--bg-panel)",
  color: "var(--text-secondary)",
  fontFamily: "var(--font-mono)",
  fontSize: 12,
  lineHeight: 1.5,
  whiteSpace: "pre",
};

function summarizeWorkspaceEdit(edit: LspWorkspaceEdit): { files: number; edits: number } {
  return {
    files: edit.files.length,
    edits: edit.files.reduce((count, file) => count + file.edits.length, 0),
  };
}

function MarkdownToc({
  toc,
  activeId,
  onJump,
}: {
  toc: TocEntry[];
  activeId: string | null;
  onJump: (id: string) => void;
}) {
  const { t } = useI18n();
  const [open, setOpen] = useState(true);
  const minDepth = useMemo(() => Math.min(...toc.map((entry) => entry.depth)), [toc]);

  return (
    <div className={`md-toc${open ? "" : " md-toc-collapsed"}`}>
      <button
        type="button"
        className="md-toc-toggle"
        onClick={() => setOpen((prev) => !prev)}
        title={t("file.outline")}
      >
        {open ? <List size={13} /> : <ChevronRight size={13} />}
        <span>{t("file.outline")}</span>
      </button>
      {open && (
        <nav className="md-toc-list">
          {toc.map((entry) => (
            <button
              key={entry.id}
              type="button"
              data-depth={Math.min(entry.depth - minDepth + 1, 6)}
              className={`md-toc-item${activeId === entry.id ? " active" : ""}`}
              onClick={() => onJump(entry.id)}
              title={entry.text}
            >
              {entry.text}
            </button>
          ))}
        </nav>
      )}
    </div>
  );
}

function CodeOutline({
  symbols,
  activeKeys,
  loading,
  error,
  truncated,
  onJump,
}: {
  symbols: LspSymbol[];
  activeKeys: Set<string>;
  loading: boolean;
  error: string | null;
  truncated: boolean;
  onJump: (symbol: LspSymbol) => void;
}) {
  const { t } = useI18n();
  const [open, setOpen] = useState(true);

  return (
    <div className={`md-toc${open ? "" : " md-toc-collapsed"}`}>
      <button
        type="button"
        className="md-toc-toggle"
        onClick={() => setOpen((prev) => !prev)}
        title={t("file.outline")}
      >
        {open ? <List size={13} /> : <ChevronRight size={13} />}
        <span>{t("file.outline")}</span>
      </button>
      {open && (
        <nav className="md-toc-list" aria-label={t("file.outline")}>
          {loading && symbols.length === 0 ? (
            <div style={outlineMessageStyle}>{t("file.outlineLoading")}</div>
          ) : error ? (
            <div style={outlineErrorStyle}>{t("file.outlineFailed", { error })}</div>
          ) : symbols.length === 0 ? (
            <div style={outlineMessageStyle}>{t("file.noOutlineSymbols")}</div>
          ) : (
            <>
              {truncated ? (
                <div style={outlineMessageStyle}>
                  {t("file.outlineTruncated", { count: String(symbols.length) })}
                </div>
              ) : null}
              {symbols.map((symbol) => {
                const key = outlineSymbolKey(symbol);
                return (
                  <button
                    key={key}
                    type="button"
                    data-depth={Math.min(outlineSymbolDepth(symbol, symbols), 6)}
                    className={`md-toc-item${activeKeys.has(key) ? " active" : ""}`}
                    onClick={() => onJump(symbol)}
                    title={[symbol.containerName, symbol.detail, symbol.name]
                      .filter(Boolean)
                      .join(" · ")}
                  >
                    {symbol.name}
                  </button>
                );
              })}
            </>
          )}
        </nav>
      )}
    </div>
  );
}

function FileBreadcrumbs({
  pathSegments,
  symbols,
  label,
  onJump,
}: {
  pathSegments: { label: string; title: string }[];
  symbols: LspSymbol[];
  label: string;
  onJump: (symbol: LspSymbol) => void;
}) {
  return (
    <nav aria-label={label} style={breadcrumbStyle}>
      {pathSegments.map((segment, index) => (
        <span key={`${segment.title}:${index}`} style={breadcrumbSegmentStyle} title={segment.title}>
          {index > 0 ? <ChevronRight size={10} style={breadcrumbSeparatorStyle} /> : null}
          <span style={breadcrumbTextStyle}>{segment.label}</span>
        </span>
      ))}
      {symbols.map((symbol) => (
        <button
          key={outlineSymbolKey(symbol)}
          type="button"
          style={breadcrumbSymbolButtonStyle}
          title={symbol.detail ?? symbol.name}
          onClick={() => onJump(symbol)}
        >
          <ChevronRight size={10} style={breadcrumbSeparatorStyle} />
          <span style={breadcrumbTextStyle}>{symbol.name}</span>
        </button>
      ))}
    </nav>
  );
}

function CodeStickyScroll({
  symbols,
  label,
  onJump,
}: {
  symbols: LspSymbol[];
  label: string;
  onJump: (symbol: LspSymbol) => void;
}) {
  if (symbols.length === 0) return null;
  return (
    <nav aria-label={label} style={stickyScrollStyle}>
      {symbols.map((symbol, index) => (
        <button
          key={outlineSymbolKey(symbol)}
          type="button"
          style={stickyScrollButtonStyle}
          title={symbol.detail ?? symbol.name}
          onClick={() => onJump(symbol)}
        >
          {index > 0 ? <ChevronRight size={10} style={breadcrumbSeparatorStyle} /> : null}
          <span style={breadcrumbTextStyle}>{symbol.name}</span>
        </button>
      ))}
    </nav>
  );
}

function formatLocalHistoryDate(createdAtMs: number): string {
  return new Date(createdAtMs).toLocaleString();
}

function formatLocalHistorySize(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / 1024 / 1024).toFixed(1)} MB`;
}

function changedLineCount(snapshotContent: string, currentContent: string): number {
  const snapshotLines = snapshotContent.split("\n");
  const currentLines = currentContent.split("\n");
  const count = Math.max(snapshotLines.length, currentLines.length);
  let changed = 0;
  for (let index = 0; index < count; index += 1) {
    if ((snapshotLines[index] ?? "") !== (currentLines[index] ?? "")) changed += 1;
  }
  return changed;
}

function LocalHistoryDialog({
  targetName,
  entries,
  selectedEntryId,
  snapshot,
  currentContent,
  loading,
  snapshotLoading,
  restoring,
  error,
  onSelectEntry,
  onRestore,
  onClose,
}: {
  targetName: string;
  entries: LocalHistoryEntry[];
  selectedEntryId: string | null;
  snapshot: LocalHistorySnapshot | null;
  currentContent: string;
  loading: boolean;
  snapshotLoading: boolean;
  restoring: boolean;
  error: string | null;
  onSelectEntry: (entryId: string) => void;
  onRestore: () => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const changedLines = snapshot ? changedLineCount(snapshot.content, currentContent) : 0;

  return (
    <div style={localHistoryOverlayStyle}>
      <section role="dialog" aria-modal="true" aria-label={t("file.localHistory")} style={localHistoryDialogStyle}>
        <div style={localHistoryHeaderStyle}>
          <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}>
            {t("file.localHistory")} - {targetName}
          </span>
          <button
            type="button"
            disabled={!snapshot || restoring}
            onClick={onRestore}
            style={{
              marginLeft: "auto",
              height: 26,
              padding: "0 10px",
              border: "1px solid var(--accent)",
              borderRadius: 5,
              background: snapshot && !restoring ? "var(--accent)" : "var(--bg-hover)",
              color: snapshot && !restoring ? "var(--accent-fg)" : "var(--text-hint)",
              fontSize: 12,
              cursor: snapshot && !restoring ? "pointer" : "default",
            }}
          >
            {restoring ? t("file.localHistoryRestoring") : t("file.localHistoryRestore")}
          </button>
          <button
            type="button"
            aria-label={t("common.close")}
            onClick={onClose}
            style={{
              border: "none",
              background: "transparent",
              color: "var(--text-hint)",
              cursor: "pointer",
              padding: 3,
              display: "flex",
            }}
          >
            <X size={15} />
          </button>
        </div>
        <div style={localHistoryBodyStyle}>
          <div style={localHistoryListStyle}>
            {loading ? (
              <div style={outlineMessageStyle}>{t("file.localHistoryLoading")}</div>
            ) : error ? (
              <div style={outlineErrorStyle}>{t("file.localHistoryFailed", { error })}</div>
            ) : entries.length === 0 ? (
              <div style={outlineMessageStyle}>{t("file.localHistoryEmpty")}</div>
            ) : (
              entries.map((entry) => {
                const active = entry.id === selectedEntryId;
                return (
                  <button
                    key={entry.id}
                    type="button"
                    onClick={() => onSelectEntry(entry.id)}
                    className="file-viewer-tab-menu-item"
                    style={{
                      width: "100%",
                      minHeight: 44,
                      display: "flex",
                      flexDirection: "column",
                      alignItems: "flex-start",
                      justifyContent: "center",
                      gap: 2,
                      borderRadius: 5,
                      background: active ? "var(--bg-hover)" : "transparent",
                    }}
                  >
                    <span style={{ color: "var(--text-secondary)" }}>
                      {formatLocalHistoryDate(entry.createdAtMs)}
                    </span>
                    <span style={{ color: "var(--text-hint)", fontSize: 11 }}>
                      {formatLocalHistorySize(entry.size)}
                    </span>
                  </button>
                );
              })
            )}
          </div>
          <div style={localHistoryPaneStyle}>
            <div
              style={{
                minHeight: 32,
                display: "flex",
                alignItems: "center",
                gap: 8,
                padding: "0 10px",
                color: "var(--text-muted)",
                fontSize: 12,
                borderBottom: "1px solid var(--border-dim)",
              }}
            >
              {snapshotLoading
                ? t("file.localHistorySnapshotLoading")
                : snapshot
                  ? t("file.localHistoryChangedLines", { count: String(changedLines) })
                  : t("file.localHistoryNoSnapshot")}
            </div>
            <div style={localHistoryComparisonStyle}>
              <div style={{ ...localHistoryPaneStyle, borderRight: "1px solid var(--border-dim)" }}>
                <div style={{ padding: "7px 10px", fontSize: 12, color: "var(--text-muted)" }}>
                  {t("file.localHistorySnapshot")}
                </div>
                <pre style={localHistoryTextStyle}>
                  {snapshot?.content ?? ""}
                </pre>
              </div>
              <div style={localHistoryPaneStyle}>
                <div style={{ padding: "7px 10px", fontSize: 12, color: "var(--text-muted)" }}>
                  {t("file.localHistoryCurrent")}
                </div>
                <pre style={localHistoryTextStyle}>{currentContent}</pre>
              </div>
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}

function FilePreviewPane({
  filePath,
  fileName,
  projectPath,
  themeVariant,
  previewMode,
  selection,
  remote,
  diagnostics = [],
  coverage,
  debugBreakpoints = [],
  onToggleDebugBreakpoint,
  onRunTestTarget,
  onDebugTestTarget,
  onOpenDefinition,
  onDirtyChange,
}: {
  filePath: string;
  fileName: string;
  projectPath: string;
  themeVariant: ThemeVariant;
  previewMode: boolean;
  selection?: OpenFileSelection;
  remote?: RemoteFileContext;
  diagnostics?: DiagnosticItem[];
  coverage?: TestCoverageSummary | null;
  debugBreakpoints?: DebugBreakpoint[];
  onToggleDebugBreakpoint?: (filePath: string, line: number) => void;
  onRunTestTarget?: (target: EditorTestRunTarget) => void;
  onDebugTestTarget?: (target: EditorTestRunTarget) => void;
  onOpenDefinition?: (path: string, name: string, selection?: OpenFileSelection) => void;
  onDirtyChange?: (path: string, dirty: boolean) => void;
}) {
  const editorTheme =
    themeVariant === "dark"
      ? githubDark
      : themeVariant === "eyecare"
        ? solarizedLight
        : githubLight;
  const { t } = useI18n();
  const [content, setContent] = useState<string | null>(null);
  const [imagePreview, setImagePreview] = useState<ImagePreviewData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [saveStatus, setSaveStatus] = useState<SaveStatus>("idle");
  const [formatting, setFormatting] = useState(false);
  const [formatError, setFormatError] = useState<string | null>(null);
  const [navigationError, setNavigationError] = useState<string | null>(null);
  const [referencesLoading, setReferencesLoading] = useState(false);
  const [references, setReferences] = useState<LspReferenceLocation[] | null>(null);
  const [referencePreviews, setReferencePreviews] = useState<Record<string, ReferencePreviewState>>(
    {},
  );
  const [outlineSymbols, setOutlineSymbols] = useState<LspSymbol[]>([]);
  const [outlineLoading, setOutlineLoading] = useState(false);
  const [outlineLoaded, setOutlineLoaded] = useState(false);
  const [outlineError, setOutlineError] = useState<string | null>(null);
  const [outlineTruncated, setOutlineTruncated] = useState(false);
  const [inlayHints, setInlayHints] = useState<LspInlayHint[]>([]);
  const [inlayHintsLoading, setInlayHintsLoading] = useState(false);
  const [inlayHintsError, setInlayHintsError] = useState<string | null>(null);
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
  const [formatOnSave, setFormatOnSave] = useState(false);
  const [inlineBlameVisible, setInlineBlameVisible] = useState(false);
  const [inlineBlameLoading, setInlineBlameLoading] = useState(false);
  const [inlineBlame, setInlineBlame] = useState<GitBlameResult | null>(null);
  const [inlineBlameError, setInlineBlameError] = useState<string | null>(null);
  const isMarkdown = isMarkdownFile(fileName);
  const isPreviewableImage = isPreviewableImageFile(fileName);
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const savedResetRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const saveRevisionRef = useRef(0);
  const formatRunRef = useRef(0);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const editorViewRef = useRef<EditorView | null>(null);
  const appliedSelectionKeyRef = useRef<string | null>(null);
  const referencePreviewRunRef = useRef(0);
  const [activeHeadingId, setActiveHeadingId] = useState<string | null>(null);
  const [cursorPosition, setCursorPosition] = useState<CursorPosition>({ line: 1, column: 1 });
  const [stickyPosition, setStickyPosition] = useState<CursorPosition>({ line: 1, column: 1 });
  const [editorContextMenu, setEditorContextMenu] = useState<EditorContextMenuState | null>(null);
  const [diagnosticSeverityFilter, setDiagnosticSeverityFilter] =
    useState<DiagnosticSeverityFilter>("all");
  const [diagnosticCopyCount, setDiagnosticCopyCount] = useState<number | null>(null);
  const [diagnosticCopyError, setDiagnosticCopyError] = useState<string | null>(null);
  const showMarkdownPreview = isMarkdown && previewMode && content !== null;
  const { html: markdownHtml, toc } = useMemo(
    () => (isMarkdown && content !== null ? renderMarkdownWithToc(content) : { html: "", toc: [] }),
    [isMarkdown, content],
  );
  const selectionKey = selection ? `${filePath}:${selection.line}:${selection.column ?? 1}` : null;
  const breakpointLines = useMemo(
    () => debugBreakpointLinesForFile(debugBreakpoints, projectPath, filePath),
    [debugBreakpoints, filePath, projectPath],
  );
  const currentFileDiagnostics = useMemo(
    () => diagnosticsForFile(diagnostics, filePath),
    [diagnostics, filePath],
  );
  const diagnosticCounts = useMemo(
    () => diagnosticSeverityCounts(currentFileDiagnostics),
    [currentFileDiagnostics],
  );
  const filteredFileDiagnostics = useMemo(
    () => filterDiagnosticsBySeverity(currentFileDiagnostics, diagnosticSeverityFilter),
    [currentFileDiagnostics, diagnosticSeverityFilter],
  );
  const groupedFileDiagnostics = useMemo(
    () => groupDiagnosticsBySource(filteredFileDiagnostics),
    [filteredFileDiagnostics],
  );
  const handleCopyDiagnostics = useCallback(async () => {
    if (filteredFileDiagnostics.length === 0) return;
    setDiagnosticCopyCount(null);
    setDiagnosticCopyError(null);
    try {
      if (typeof navigator === "undefined" || !navigator.clipboard?.writeText) {
        throw new Error(t("file.diagnosticsCopyUnavailable"));
      }
      await navigator.clipboard.writeText(diagnosticsClipboardText(filteredFileDiagnostics));
      setDiagnosticCopyCount(filteredFileDiagnostics.length);
    } catch (err) {
      setDiagnosticCopyError(String(err));
    }
  }, [filteredFileDiagnostics, t]);
  const diagnosticsExtension = useMemo(
    () => createDiagnosticsExtension(currentFileDiagnostics),
    [currentFileDiagnostics],
  );
  const currentFileCoverageLines = useMemo(
    () => coverageLinesForFile(coverage, filePath),
    [coverage, filePath],
  );
  const coverageExtension = useMemo(
    () => createCoverageExtension(currentFileCoverageLines),
    [currentFileCoverageLines],
  );
  const inlineBlamePath = useMemo(
    () => projectRelativeGitPath(projectPath, filePath),
    [filePath, projectPath],
  );
  const debugBreakpointGutter = useMemo(
    () =>
      createDebugBreakpointGutter({
        breakpointLines,
        label: t("file.toggleBreakpoint"),
        onToggleLine: remote
          ? undefined
          : (line) => {
              onToggleDebugBreakpoint?.(filePath, line);
            },
      }),
    [breakpointLines, filePath, onToggleDebugBreakpoint, remote, t],
  );
  const testRunTargets = useMemo(
    () => (content === null ? [] : testRunTargetsForContent(content, filePath)),
    [content, filePath],
  );
  const testRunGutter = useMemo(
    () =>
      createTestRunGutter({
        targets: testRunTargets,
        label: t("file.runTest"),
        debugLabel: t("file.debugTest"),
        onRunTarget: onRunTestTarget,
        onDebugTarget: onDebugTestTarget,
      }),
    [onDebugTestTarget, onRunTestTarget, t, testRunTargets],
  );
  const inlineBlameExtension = useMemo(
    () =>
      createInlineBlameExtension({
        enabled: inlineBlameVisible,
        lines: inlineBlame?.filePath === inlineBlamePath ? inlineBlame.lines : [],
      }),
    [inlineBlame, inlineBlamePath, inlineBlameVisible],
  );
  const languageServer = useLanguageServer({
    projectPath,
    filePath,
    content,
    cursorLine: cursorPosition.line,
    cursorColumn: cursorPosition.column,
    remote,
  });
  const currentLspRequest = useCallback(() => {
    if (!languageServer.request || content === null) return null;
    const view = editorViewRef.current;
    if (!view) return languageServer.request;
    const anchor = view.state.selection.main.head;
    const line = view.state.doc.lineAt(anchor);
    return buildLspDocumentRequest({
      projectPath,
      filePath,
      content,
      line: line.number,
      column: anchor - line.from + 1,
    });
  }, [content, filePath, languageServer.request, projectPath]);
  const breadcrumbSegments = useMemo(
    () => fileBreadcrumbSegments(projectPath, filePath),
    [filePath, projectPath],
  );
  const activeOutlineSymbols = useMemo(
    () => activeSymbolBreadcrumbs(outlineSymbols, cursorPosition),
    [cursorPosition, outlineSymbols],
  );
  const activeOutlineKeys = useMemo(
    () => new Set(activeOutlineSymbols.map(outlineSymbolKey)),
    [activeOutlineSymbols],
  );
  const stickyOutlineSymbols = useMemo(
    () => activeSymbolBreadcrumbs(outlineSymbols, stickyPosition),
    [outlineSymbols, stickyPosition],
  );
  const showCodeOutline = Boolean(
    !isMarkdown &&
      !isPreviewableImage &&
      content !== null &&
      languageServer.supported &&
      (outlineLoading || outlineLoaded || outlineError),
  );
  const showStickyScroll = Boolean(
    !isMarkdown &&
      !isPreviewableImage &&
      content !== null &&
      languageServer.supported &&
      stickyOutlineSymbols.length > 0,
  );
  const jumpToOutlineSymbol = useCallback(
    (symbol: LspSymbol) => {
      if (content === null) return;
      const view = editorViewRef.current;
      if (!view) return;
      const selection = lspSymbolToSelection(symbol);
      const offset = lineColumnToOffset(content, selection);
      view.dispatch({
        selection: { anchor: offset },
        effects: EditorView.scrollIntoView(offset, { y: "center" }),
      });
      setCursorPosition({ line: selection.line, column: selection.column ?? 1 });
      setStickyPosition({ line: selection.line, column: selection.column ?? 1 });
      setNavigationError(null);
      view.focus();
    },
    [content],
  );
  const handleOpenLspTarget = useCallback(
    (target: LspOpenTarget) => {
      setNavigationError(null);
      onOpenDefinition?.(target.path, target.name, target.selection);
    },
    [onOpenDefinition],
  );
  const lspNavigationExtension = useMemo(
    () =>
      createLspNavigationExtension({
        request: languageServer.request,
        available: Boolean(languageServer.status?.available),
        unavailableMessage: languageServer.message,
        remote,
        onOpenTarget: handleOpenLspTarget,
        onError: setNavigationError,
      }),
    [
      handleOpenLspTarget,
      languageServer.message,
      languageServer.request,
      languageServer.status?.available,
      remote,
    ],
  );
  const lspHoverExtension = useMemo(
    () =>
      createLspHoverExtension({
        request: languageServer.request,
        available: Boolean(languageServer.status?.available),
        unavailableMessage: languageServer.message,
        remote,
        onError: setNavigationError,
      }),
    [languageServer.message, languageServer.request, languageServer.status?.available, remote],
  );
  const lspCompletionExtension = useMemo(
    () =>
      createLspCompletionExtension({
        request: languageServer.request,
        available: Boolean(languageServer.status?.available),
        unavailableMessage: languageServer.message,
        remote,
        onError: setNavigationError,
      }),
    [languageServer.message, languageServer.request, languageServer.status?.available, remote],
  );
  const lspSignatureHelpExtension = useMemo(
    () =>
      createLspSignatureHelpExtension({
        request: languageServer.request,
        available: Boolean(languageServer.status?.available),
        unavailableMessage: languageServer.message,
        remote,
        onError: setNavigationError,
      }),
    [languageServer.message, languageServer.request, languageServer.status?.available, remote],
  );
  const lspInlayHintsExtension = useMemo(
    () => createLspInlayHintsExtension(inlayHints),
    [inlayHints],
  );
  const loadReferencePreviews = useCallback(
    async (locations: LspReferenceLocation[], sourceContent: string) => {
      const runId = referencePreviewRunRef.current + 1;
      referencePreviewRunRef.current = runId;
      setReferencePreviews(
        Object.fromEntries(
          locations.map((location, index) => [lspReferenceKey(location, index), { status: "loading" }]),
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
            return [key, { status: "ready", preview: lspReferencePreviewLine(targetContent, location) }];
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
  const handleFindReferences = useCallback(async () => {
    const request = currentLspRequest();
    if (!request) return;
    setReferences(null);
    setReferencePreviews({});
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
  }, [
    currentLspRequest,
    languageServer.message,
    languageServer.status?.available,
    loadReferencePreviews,
    remote,
    t,
  ]);
  const openReference = useCallback(
    (reference: LspReferenceLocation) => {
      const target = lspReferenceToOpenTarget(reference);
      setReferences(null);
      setReferencePreviews({});
      referencePreviewRunRef.current += 1;
      onOpenDefinition?.(target.path, target.name, target.selection);
    },
    [onOpenDefinition],
  );

  const jumpToHeading = (id: string) => {
    const target = scrollRef.current?.querySelector<HTMLElement>(`#${CSS.escape(id)}`);
    target?.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  useEffect(() => {
    const root = scrollRef.current;
    if (!root || !showMarkdownPreview || toc.length === 0) return;
    const headings = toc
      .map((entry) => root.querySelector<HTMLElement>(`#${CSS.escape(entry.id)}`))
      .filter((el): el is HTMLElement => el !== null);
    if (headings.length === 0) return;
    const observer = new IntersectionObserver(
      (entries) => {
        const visible = entries
          .filter((e) => e.isIntersecting)
          .sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top);
        if (visible[0]) setActiveHeadingId(visible[0].target.id);
      },
      { root, rootMargin: "0px 0px -65% 0px", threshold: 0 },
    );
    headings.forEach((el) => observer.observe(el));
    return () => observer.disconnect();
  }, [showMarkdownPreview, toc]);

  useEffect(() => {
    if (isPreviewableImage || isMarkdown || content === null || !languageServer.supported) {
      setOutlineSymbols([]);
      setOutlineLoading(false);
      setOutlineLoaded(false);
      setOutlineError(null);
      setOutlineTruncated(false);
      return;
    }
    if (!languageServer.status) return;
    if (!languageServer.status.available) {
      setOutlineSymbols([]);
      setOutlineLoading(false);
      setOutlineLoaded(true);
      setOutlineError(languageServer.message ?? "Language server is unavailable.");
      setOutlineTruncated(false);
      return;
    }

    let cancelled = false;
    const request = buildLspDocumentRequest({
      projectPath,
      filePath,
      content,
      line: 1,
      column: 1,
    });
    setOutlineLoading(true);
    setOutlineError(null);
    const timer = window.setTimeout(() => {
      void requestLspDocumentOutline(request, remote)
        .then((outline) => {
          if (cancelled) return;
          setOutlineSymbols(outline.symbols);
          setOutlineTruncated(outline.truncated);
          setOutlineLoaded(true);
        })
        .catch((err) => {
          if (cancelled) return;
          setOutlineSymbols([]);
          setOutlineTruncated(false);
          setOutlineLoaded(true);
          setOutlineError(String(err));
        })
        .finally(() => {
          if (!cancelled) setOutlineLoading(false);
        });
    }, 250);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [
    content,
    filePath,
    isMarkdown,
    isPreviewableImage,
    languageServer.message,
    languageServer.status,
    languageServer.supported,
    projectPath,
    remote,
  ]);

  useEffect(() => {
    if (isPreviewableImage || isMarkdown || content === null || !languageServer.supported) {
      setInlayHints([]);
      setInlayHintsLoading(false);
      setInlayHintsError(null);
      return;
    }
    if (!languageServer.status) return;
    if (!languageServer.status.available) {
      setInlayHints([]);
      setInlayHintsLoading(false);
      setInlayHintsError(languageServer.message ?? "Language server is unavailable.");
      return;
    }

    let cancelled = false;
    const request = buildLspDocumentRequest({
      projectPath,
      filePath,
      content,
      line: 1,
      column: 1,
    });
    setInlayHintsLoading(true);
    setInlayHintsError(null);
    const timer = window.setTimeout(() => {
      void requestLspInlayHints(request, remote)
        .then((hints) => {
          if (!cancelled) setInlayHints(hints);
        })
        .catch((err) => {
          if (!cancelled) {
            setInlayHints([]);
            setInlayHintsError(String(err));
          }
        })
        .finally(() => {
          if (!cancelled) setInlayHintsLoading(false);
        });
    }, 300);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [
    content,
    filePath,
    isMarkdown,
    isPreviewableImage,
    languageServer.message,
    languageServer.status,
    languageServer.supported,
    projectPath,
    remote,
  ]);

  useEffect(() => {
    let cancelled = false;

    saveRevisionRef.current += 1;
    setLoading(true);
    setContent(null);
    setImagePreview(null);
    setError(null);
    setFormatError(null);
    setNavigationError(null);
    setReferences(null);
    setReferencePreviews({});
    referencePreviewRunRef.current += 1;
    setOutlineSymbols([]);
    setOutlineLoading(false);
    setOutlineLoaded(false);
    setOutlineError(null);
    setOutlineTruncated(false);
    setInlayHints([]);
    setInlayHintsLoading(false);
    setInlayHintsError(null);
    setRenameOpen(false);
    setRenamePreview(null);
    setRenameSummary(null);
    setCodeActions(null);
    setCodeActionSummary(null);
    setCodeActionCommandSummary(null);
    setCodeActionCommandSummary(null);
    setSaveStatus("idle");
    setInlineBlameVisible(false);
    setInlineBlameLoading(false);
    setInlineBlame(null);
    setInlineBlameError(null);
    setCursorPosition({ line: 1, column: 1 });
    setStickyPosition({ line: 1, column: 1 });
    setDiagnosticSeverityFilter("all");
    setDiagnosticCopyCount(null);
    setDiagnosticCopyError(null);
    editorViewRef.current = null;
    appliedSelectionKeyRef.current = null;
    onDirtyChange?.(filePath, false);

    const loadFile = isPreviewableImage
      ? invoke<ImagePreviewData>(
          remote ? "remote_read_image_preview" : "read_image_preview",
          remote
            ? {
                connection: remote.connection,
                remotePath: filePath,
                remoteProjectPath: remote.projectPath,
              }
            : { path: filePath, projectPath },
        ).then((preview) => {
          if (cancelled) return;
          setImagePreview(preview);
          setLoading(false);
        })
      : invoke<string>(
          remote ? "remote_read_file_content" : "read_file_content",
          remote
            ? {
                connection: remote.connection,
                remotePath: filePath,
                remoteProjectPath: remote.projectPath,
              }
            : { path: filePath, projectPath },
        ).then((nextContent) => {
          if (cancelled) return;
          setContent(nextContent);
          setLoading(false);
        });

    loadFile.catch((err) => {
      if (cancelled) return;
      setError(String(err));
      setLoading(false);
    });

    return () => {
      cancelled = true;
    };
  }, [filePath, projectPath, isPreviewableImage, remote, onDirtyChange]);

  useEffect(() => {
    let cancelled = false;

    if (remote) {
      setFormatOnSave(false);
      return () => {
        cancelled = true;
      };
    }

    invoke<ProjectEditorSettings>("read_project_config", { projectPath })
      .then((config) => {
        if (!cancelled) setFormatOnSave(Boolean(config.editor?.format_on_save));
      })
      .catch(() => {
        if (!cancelled) setFormatOnSave(false);
      });

    return () => {
      cancelled = true;
    };
  }, [projectPath, remote]);

  useEffect(
    () => () => {
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
      if (savedResetRef.current) clearTimeout(savedResetRef.current);
    },
    [],
  );

  const saveContent = useCallback(
    async (value: string, options: SaveContentOptions = {}) => {
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
      if (savedResetRef.current) clearTimeout(savedResetRef.current);
      const revision = saveRevisionRef.current + 1;
      saveRevisionRef.current = revision;
      setSaveStatus("saving");
      setFormatError(null);
      try {
        if (remote) {
          await invoke("remote_write_file_content", {
            connection: remote.connection,
            remotePath: filePath,
            remoteProjectPath: remote.projectPath,
            content: value,
          });
        } else {
          await invoke("write_file_content", { path: filePath, content: value, projectPath });
        }
        if (saveRevisionRef.current !== revision) return false;
        onDirtyChange?.(filePath, false);

        const shouldFormatAfterSave = !remote && (options.formatAfterSave ?? formatOnSave);
        if (shouldFormatAfterSave) {
          const formatRun = formatRunRef.current + 1;
          formatRunRef.current = formatRun;
          setFormatting(true);
          try {
            await invoke<FormatFileResult>("format_file", { projectPath, filePath });
            if (saveRevisionRef.current !== revision) return false;
            const nextContent = await invoke<string>("read_file_content", {
              path: filePath,
              projectPath,
            });
            if (saveRevisionRef.current !== revision) return false;
            setContent(nextContent);
          } catch (err) {
            if (saveRevisionRef.current !== revision) return false;
            setSaveStatus("error");
            setFormatError(String(err));
            return false;
          } finally {
            if (formatRunRef.current === formatRun) setFormatting(false);
          }
        }

        if (saveRevisionRef.current !== revision) return false;
        setSaveStatus("saved");
        savedResetRef.current = setTimeout(() => setSaveStatus("idle"), 2000);
        return true;
      } catch {
        if (saveRevisionRef.current !== revision) return false;
        setSaveStatus("error");
        return false;
      }
    },
    [filePath, formatOnSave, onDirtyChange, projectPath, remote],
  );

  const handleOpenRename = useCallback(() => {
    setReferences(null);
    setNavigationError(null);
    setRenameSummary(null);
    setRenamePreview(null);
    setRenameName("");
    setRenameOpen(true);
  }, []);

  const handlePreviewRename = useCallback(async () => {
    const request = currentLspRequest();
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
      const edit = await requestLspRename(request, nextName, remote);
      setRenamePreview(edit);
    } catch (err) {
      setNavigationError(String(err));
    } finally {
      setRenameLoading(false);
    }
  }, [
    content,
    currentLspRequest,
    languageServer.message,
    languageServer.status?.available,
    renameName,
    remote,
    saveContent,
    saveStatus,
  ]);

  const handleApplyRename = useCallback(async () => {
    if (!renamePreview || renameApplying) return;
    setNavigationError(null);
    setRenameApplying(true);
    try {
      const summary = await applyLspWorkspaceEdit(projectPath, renamePreview, remote);
      setRenameSummary(summary);
      setRenameOpen(false);
      setRenamePreview(null);
      const touchesCurrentFile = renamePreview.files.some((file) => file.path === filePath);
      if (touchesCurrentFile) {
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
        setContent(nextContent);
        onDirtyChange?.(filePath, false);
        setSaveStatus("saved");
        savedResetRef.current = setTimeout(() => setSaveStatus("idle"), 2000);
      }
    } catch (err) {
      setNavigationError(String(err));
    } finally {
      setRenameApplying(false);
    }
  }, [filePath, onDirtyChange, projectPath, remote, renameApplying, renamePreview]);

  const handleQuickFix = useCallback(async () => {
    const request = currentLspRequest();
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
    currentLspRequest,
    currentFileDiagnostics,
    languageServer.message,
    languageServer.status?.available,
    remote,
    saveContent,
    saveStatus,
    t,
  ]);

  const runEditorLspCommand = useCallback(
    (command: FileViewerCommand) => {
      if (isPreviewableImage || content === null || !languageServer.supported) return;

      if (command === "findReferences") {
        void handleFindReferences();
      } else if (command === "renameSymbol") {
        handleOpenRename();
      } else {
        void handleQuickFix();
      }
    },
    [
      content,
      handleFindReferences,
      handleOpenRename,
      handleQuickFix,
      isPreviewableImage,
      languageServer.supported,
    ],
  );

  useEffect(() => {
    const onEditorCommand = (event: Event) => {
      const command = (event as CustomEvent<{ command?: unknown }>).detail?.command;
      if (!isFileViewerCommand(command)) return;
      runEditorLspCommand(command);
    };

    window.addEventListener(FILE_VIEWER_COMMAND_EVENT, onEditorCommand);
    return () => window.removeEventListener(FILE_VIEWER_COMMAND_EVENT, onEditorCommand);
  }, [runEditorLspCommand]);

  const closeEditorContextMenu = useCallback(() => {
    setEditorContextMenu(null);
  }, []);

  const runEditorContextMenuCommand = useCallback(
    (command: FileViewerCommand) => {
      closeEditorContextMenu();
      runEditorLspCommand(command);
    },
    [closeEditorContextMenu, runEditorLspCommand],
  );

  useEffect(() => {
    if (!editorContextMenu) return;
    const closeOnWindowClick = () => closeEditorContextMenu();
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") closeEditorContextMenu();
    };
    window.addEventListener("click", closeOnWindowClick);
    window.addEventListener("keydown", closeOnEscape);
    window.addEventListener("resize", closeEditorContextMenu);
    return () => {
      window.removeEventListener("click", closeOnWindowClick);
      window.removeEventListener("keydown", closeOnEscape);
      window.removeEventListener("resize", closeEditorContextMenu);
    };
  }, [closeEditorContextMenu, editorContextMenu]);

  useEffect(() => {
    closeEditorContextMenu();
  }, [closeEditorContextMenu, filePath]);

  const handleApplyCodeAction = useCallback(
    async (action: LspCodeAction) => {
      if ((!action.edit && !action.command) || codeActionApplying) return;
      const request = currentLspRequest();
      if (action.command && !request) return;
      setNavigationError(null);
      setCodeActionApplying(true);
      try {
        if (action.edit) {
          const summary = await applyLspWorkspaceEdit(projectPath, action.edit, remote);
          setCodeActionSummary(summary);
          const touchesCurrentFile = action.edit.files.some((file) => file.path === filePath);
          if (touchesCurrentFile) {
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
            setContent(nextContent);
            onDirtyChange?.(filePath, false);
            setSaveStatus("saved");
            savedResetRef.current = setTimeout(() => setSaveStatus("idle"), 2000);
          }
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
    [codeActionApplying, currentLspRequest, filePath, onDirtyChange, projectPath, remote],
  );

  const handleFormatFile = useCallback(async () => {
    if (remote || content === null || isPreviewableImage || formatting) return;
    const formatRun = formatRunRef.current + 1;
    formatRunRef.current = formatRun;
    setFormatting(true);
    setFormatError(null);
    try {
      if (saveStatus === "dirty") {
        const saved = await saveContent(content, { formatAfterSave: false });
        if (!saved) return;
      }
      const revision = saveRevisionRef.current + 1;
      saveRevisionRef.current = revision;
      await invoke<FormatFileResult>("format_file", { projectPath, filePath });
      if (saveRevisionRef.current !== revision) return;
      const nextContent = await invoke<string>("read_file_content", {
        path: filePath,
        projectPath,
      });
      if (saveRevisionRef.current !== revision) return;
      setContent(nextContent);
      onDirtyChange?.(filePath, false);
      setSaveStatus("saved");
      savedResetRef.current = setTimeout(() => setSaveStatus("idle"), 2000);
    } catch (err) {
      setSaveStatus("error");
      setFormatError(String(err));
    } finally {
      if (formatRunRef.current === formatRun) setFormatting(false);
    }
  }, [
    content,
    filePath,
    formatting,
    isPreviewableImage,
    onDirtyChange,
    projectPath,
    remote,
    saveContent,
    saveStatus,
  ]);

  const toggleInlineBlame = useCallback(async () => {
    if (remote || !inlineBlamePath || content === null || isPreviewableImage) return;
    if (inlineBlameVisible) {
      setInlineBlameVisible(false);
      return;
    }
    if (inlineBlame?.filePath === inlineBlamePath) {
      setInlineBlameVisible(true);
      return;
    }

    setInlineBlameLoading(true);
    setInlineBlameError(null);
    try {
      const result = await invoke<GitBlameResult>("git_blame_file", {
        projectPath,
        filePath: inlineBlamePath,
      });
      setInlineBlame(result);
      setInlineBlameVisible(true);
    } catch (err) {
      setInlineBlameVisible(false);
      setInlineBlameError(String(err));
    } finally {
      setInlineBlameLoading(false);
    }
  }, [
    content,
    inlineBlame,
    inlineBlamePath,
    inlineBlameVisible,
    isPreviewableImage,
    projectPath,
    remote,
  ]);

  const handleChange = (value: string) => {
    saveRevisionRef.current += 1;
    setContent(value);
    if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
    if (savedResetRef.current) clearTimeout(savedResetRef.current);
    setFormatError(null);
    setNavigationError(null);
    setReferences(null);
    setRenameSummary(null);
    setCodeActions(null);
    setCodeActionSummary(null);
    onDirtyChange?.(filePath, true);
    setSaveStatus("dirty");
    saveTimerRef.current = setTimeout(() => {
      void saveContent(value);
    }, 1500);
  };

  const [languageExtension, setLanguageExtension] = useState<Extension>([]);
  useEffect(() => {
    let cancelled = false;
    setLanguageExtension([]);
    loadLanguageExtension(fileName)
      .then((extension) => {
        if (!cancelled) setLanguageExtension(extension);
      })
      .catch((e: unknown) => {
        console.error(e);
        if (!cancelled) setLanguageExtension([]);
      });
    return () => {
      cancelled = true;
    };
  }, [fileName]);
  const extensions = useMemo(
    () => [
      languageExtension,
      testRunGutter,
      debugBreakpointGutter,
      coverageExtension,
      diagnosticsExtension,
      inlineBlameExtension,
      lspNavigationExtension,
      lspHoverExtension,
      lspCompletionExtension,
      lspSignatureHelpExtension,
      lspInlayHintsExtension,
      editorBaseTheme,
    ],
    [
      coverageExtension,
      debugBreakpointGutter,
      diagnosticsExtension,
      inlineBlameExtension,
      languageExtension,
      lspCompletionExtension,
      lspHoverExtension,
      lspInlayHintsExtension,
      lspNavigationExtension,
      lspSignatureHelpExtension,
      testRunGutter,
    ],
  );

  const applySelection = useCallback(
    (view: EditorView, value: string) => {
      if (!selection || !selectionKey || appliedSelectionKeyRef.current === selectionKey) return;
      const offset = lineColumnToOffset(value, selection);
      view.dispatch({
        selection: { anchor: offset },
        effects: EditorView.scrollIntoView(offset, { y: "center" }),
      });
      const line = view.state.doc.lineAt(offset);
      setCursorPosition({ line: line.number, column: offset - line.from + 1 });
      setStickyPosition({ line: line.number, column: 1 });
      appliedSelectionKeyRef.current = selectionKey;
      view.focus();
    },
    [selection, selectionKey],
  );

  const updateCursorPosition = useCallback((update: ViewUpdate) => {
    const anchor = update.state.selection.main.head;
    const line = update.state.doc.lineAt(anchor);
    setCursorPosition({ line: line.number, column: anchor - line.from + 1 });
    const updateView = (update as ViewUpdate & { view?: { viewport?: { from?: number } } }).view;
    const viewportFrom =
      typeof updateView?.viewport?.from === "number" ? Math.max(0, updateView.viewport.from) : anchor;
    const viewportLine = update.state.doc.lineAt(viewportFrom);
    setStickyPosition({ line: viewportLine.number, column: 1 });
  }, []);

  const jumpToDiagnostic = useCallback(
    (direction: 1 | -1) => {
      if (content === null || currentFileDiagnostics.length === 0) return;
      const target = nextDiagnosticTarget(currentFileDiagnostics, cursorPosition, direction);
      const view = editorViewRef.current;
      if (!target || !view) return;
      const offset = lineColumnToOffset(content, target);
      view.dispatch({
        selection: { anchor: offset },
        effects: EditorView.scrollIntoView(offset, { y: "center" }),
      });
      view.focus();
      setCursorPosition(target);
      setStickyPosition({ line: target.line, column: 1 });
      setNavigationError(null);
    },
    [content, currentFileDiagnostics, cursorPosition],
  );

  const openDiagnostic = useCallback(
    (diagnostic: DiagnosticItem) => {
      if (content === null) return;
      const view = editorViewRef.current;
      if (!view) return;
      const target = { line: diagnostic.line, column: diagnostic.column };
      const offset = lineColumnToOffset(content, target);
      view.dispatch({
        selection: { anchor: offset },
        effects: EditorView.scrollIntoView(offset, { y: "center" }),
      });
      view.focus();
      setCursorPosition(target);
      setStickyPosition({ line: target.line, column: 1 });
      setNavigationError(null);
    },
    [content],
  );

  useEffect(() => {
    if (content === null || isPreviewableImage) return;
    const view = editorViewRef.current;
    if (!view) return;
    applySelection(view, content);
  }, [applySelection, content, isPreviewableImage]);

  const saveLabel =
    saveStatus === "saving"
      ? t("file.saving")
      : saveStatus === "dirty"
        ? t("file.unsaved")
        : saveStatus === "saved"
          ? t("file.saved")
          : saveStatus === "error"
            ? t("file.saveFailed")
            : null;
  const statusLabel = isPreviewableImage
    ? imagePreview
      ? `${imagePreview.mimeType} · ${t("file.readOnly")}`
      : t("file.imagePreview")
    : saveLabel;
  const languageServerLabel =
    !isPreviewableImage && content !== null && languageServer.supported
      ? languageServer.loading
        ? t("file.lspChecking")
        : languageServer.status?.available
          ? t("file.lspReady")
          : languageServer.message
            ? t("file.lspUnavailable")
            : null
      : null;

  return (
    <div
      style={{
        flex: 1,
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
        minWidth: 0,
        minHeight: 0,
        background: "var(--bg-panel)",
      }}
      onKeyDownCapture={(event) => {
        if (
          (event.metaKey || event.ctrlKey) &&
          event.key.toLowerCase() === "s" &&
          content !== null
        ) {
          event.preventDefault();
          event.stopPropagation();
          void saveContent(content);
        }
        if (event.key === "F2" && content !== null) {
          event.preventDefault();
          event.stopPropagation();
          jumpToDiagnostic(event.shiftKey ? -1 : 1);
        }
        if (
          (event.metaKey || event.ctrlKey) &&
          event.key === "." &&
          content !== null &&
          languageServer.supported
        ) {
          event.preventDefault();
          event.stopPropagation();
          void handleQuickFix();
        }
      }}
    >
      <div
        style={{
          flex: 1,
          overflow: "hidden",
          position: "relative",
          userSelect: "text",
          minWidth: 0,
          minHeight: 0,
        }}
      >
        {loading && (
          <div
            style={{
              position: "absolute",
              inset: 0,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              color: "var(--text-hint)",
              fontSize: 12,
            }}
          >
            {t("common.loading")}
          </div>
        )}
        {error && !loading && (
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              alignItems: "center",
              justifyContent: "center",
              height: "100%",
              gap: 10,
              color: "var(--text-muted)",
            }}
          >
            <AlertCircle size={24} strokeWidth={1.5} />
            <span style={{ fontSize: 12.5 }}>{error}</span>
          </div>
        )}
        {!loading &&
          !error &&
          (isPreviewableImage && imagePreview ? (
            <ImagePreviewPane
              src={imagePreview.dataUrl}
              fileName={fileName}
              mimeType={imagePreview.mimeType}
              byteLength={imagePreview.byteLength}
            />
          ) : content !== null ? (
            isMarkdown && previewMode ? (
              <div className="md-preview-layout">
                {toc.length > 0 && (
                  <MarkdownToc toc={toc} activeId={activeHeadingId} onJump={jumpToHeading} />
                )}
                <div ref={scrollRef} className="md-preview-scroll">
                  <div className="md-preview" dangerouslySetInnerHTML={{ __html: markdownHtml }} />
                </div>
              </div>
            ) : (
              <div style={{ height: "100%", display: "flex", minWidth: 0, minHeight: 0 }}>
                {showCodeOutline ? (
                  <CodeOutline
                    symbols={outlineSymbols}
                    activeKeys={activeOutlineKeys}
                    loading={outlineLoading}
                    error={outlineError}
                    truncated={outlineTruncated}
                    onJump={jumpToOutlineSymbol}
                  />
                ) : null}
                <div
                  style={{
                    flex: 1,
                    minWidth: 0,
                    minHeight: 0,
                    display: "flex",
                    flexDirection: "column",
                  }}
                >
                  {showStickyScroll ? (
                    <CodeStickyScroll
                      symbols={stickyOutlineSymbols}
                      label={t("file.stickyScroll")}
                      onJump={jumpToOutlineSymbol}
                    />
                  ) : null}
                  <div
                    style={{ flex: 1, minWidth: 0, minHeight: 0 }}
                    onContextMenu={(event) => {
                      if (isPreviewableImage || content === null || !languageServer.supported) {
                        closeEditorContextMenu();
                        return;
                      }
                      event.preventDefault();
                      setEditorContextMenu({
                        x: Math.max(4, Math.min(event.clientX, window.innerWidth - 196)),
                        y: Math.max(4, Math.min(event.clientY, window.innerHeight - 104)),
                      });
                    }}
                  >
                    <ReactCodeMirror
                      value={content}
                      onChange={handleChange}
                      onCreateEditor={(view) => {
                        editorViewRef.current = view;
                        applySelection(view, content);
                      }}
                      onUpdate={updateCursorPosition}
                      theme={editorTheme}
                      extensions={extensions}
                      height="100%"
                      style={{ height: "100%" }}
                      basicSetup={{
                        lineNumbers: true,
                        foldGutter: true,
                        highlightActiveLine: true,
                        highlightSelectionMatches: true,
                        autocompletion: true,
                        searchKeymap: true,
                      }}
                    />
                    {editorContextMenu ? (
                      <div
                        role="menu"
                        aria-label={t("file.editorActions")}
                        style={{
                          ...editorContextMenuStyle,
                          left: editorContextMenu.x,
                          top: editorContextMenu.y,
                        }}
                        onClick={(event) => event.stopPropagation()}
                        onMouseDown={(event) => event.stopPropagation()}
                      >
                        <button
                          type="button"
                          role="menuitem"
                          className="file-viewer-tab-menu-item"
                          style={editorContextMenuItemStyle}
                          onClick={() => runEditorContextMenuCommand("findReferences")}
                        >
                          <Search size={13} />
                          {t("file.findReferences")}
                        </button>
                        <button
                          type="button"
                          role="menuitem"
                          className="file-viewer-tab-menu-item"
                          style={editorContextMenuItemStyle}
                          onClick={() => runEditorContextMenuCommand("renameSymbol")}
                        >
                          <PencilLine size={13} />
                          {t("file.renameSymbol")}
                        </button>
                        <button
                          type="button"
                          role="menuitem"
                          className="file-viewer-tab-menu-item"
                          style={editorContextMenuItemStyle}
                          onClick={() => runEditorContextMenuCommand("quickFix")}
                        >
                          <Lightbulb size={13} />
                          {t("file.quickFix")}
                        </button>
                      </div>
                    ) : null}
                  </div>
                </div>
              </div>
            )
          ) : null)}
        {references && (
          <div
            role="dialog"
            aria-label={t("file.referencesTitle", { count: String(references.length) })}
            style={{
              position: "absolute",
              right: 12,
              bottom: 10,
              width: "min(460px, calc(100% - 24px))",
              maxHeight: "min(280px, calc(100% - 24px))",
              display: "flex",
              flexDirection: "column",
              border: "1px solid var(--border-dim)",
              borderRadius: 6,
              background: "var(--bg-card)",
              boxShadow: "0 16px 40px color-mix(in srgb, #000 26%, transparent)",
              overflow: "hidden",
              zIndex: 8,
            }}
          >
            <div
              style={{
                height: 30,
                display: "flex",
                alignItems: "center",
                gap: 8,
                padding: "0 9px",
                borderBottom: "1px solid var(--border-dim)",
                color: "var(--text-primary)",
                fontSize: 12,
                fontWeight: 600,
              }}
            >
              <Search size={13} />
              <span style={{ flex: 1 }}>
                {t("file.referencesTitle", { count: String(references.length) })}
              </span>
              <button
                type="button"
                aria-label={t("common.close")}
                onClick={() => {
                  setReferences(null);
                  setReferencePreviews({});
                  referencePreviewRunRef.current += 1;
                }}
                style={{
                  border: "none",
                  background: "transparent",
                  color: "var(--text-hint)",
                  cursor: "pointer",
                  padding: 2,
                  display: "flex",
                }}
              >
                <X size={13} />
              </button>
            </div>
            <div style={{ overflowY: "auto", padding: 4 }}>
              {references.map((reference, index) => {
                const line = reference.range.start.line + 1;
                const column = reference.range.start.character + 1;
                const label = `${reference.path}:${line}:${column}`;
                const preview = referencePreviews[lspReferenceKey(reference, index)];
                return (
                  <button
                    key={`${reference.uri}:${line}:${column}:${index}`}
                    type="button"
                    aria-label={label}
                    onClick={() => openReference(reference)}
                    style={{
                      width: "100%",
                      minHeight: 48,
                      display: "flex",
                      flexDirection: "column",
                      alignItems: "stretch",
                      gap: 8,
                      padding: "5px 7px",
                      border: "none",
                      borderRadius: 4,
                      background: "transparent",
                      color: "var(--text-secondary)",
                      cursor: "pointer",
                      textAlign: "left",
                      fontFamily: "var(--font-mono)",
                      fontSize: 11.5,
                    }}
                    onMouseEnter={(event) => {
                      event.currentTarget.style.background = "var(--bg-hover)";
                    }}
                    onMouseLeave={(event) => {
                      event.currentTarget.style.background = "transparent";
                    }}
                  >
                    <span style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
                      <span
                        style={{
                          minWidth: 42,
                          color: "var(--text-hint)",
                          fontVariantNumeric: "tabular-nums",
                        }}
                      >
                        {line}:{column}
                      </span>
                      <span
                        style={{
                          minWidth: 0,
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                        }}
                      >
                        {reference.path}
                      </span>
                    </span>
                    <span
                      style={{
                        minWidth: 0,
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                        color: preview?.status === "error" ? "var(--warning)" : "var(--text-muted)",
                        fontSize: 11,
                      }}
                    >
                      {preview?.status === "ready"
                        ? preview.preview.text || t("file.referencePreviewEmpty")
                        : preview?.status === "error"
                          ? t("file.referencePreviewFailed", { error: preview.error })
                          : t("file.referencePreviewLoading")}
                    </span>
                  </button>
                );
              })}
            </div>
          </div>
        )}
        {renameOpen && (
          <div
            role="dialog"
            aria-label={t("file.renameSymbol")}
            style={{
              position: "absolute",
              right: 12,
              bottom: 10,
              width: "min(520px, calc(100% - 24px))",
              maxHeight: "min(360px, calc(100% - 24px))",
              display: "flex",
              flexDirection: "column",
              border: "1px solid var(--border-dim)",
              borderRadius: 6,
              background: "var(--bg-card)",
              boxShadow: "0 16px 40px color-mix(in srgb, #000 26%, transparent)",
              overflow: "hidden",
              zIndex: 9,
            }}
          >
            <div
              style={{
                height: 32,
                display: "flex",
                alignItems: "center",
                gap: 8,
                padding: "0 9px",
                borderBottom: "1px solid var(--border-dim)",
                color: "var(--text-primary)",
                fontSize: 12,
                fontWeight: 600,
              }}
            >
              <PencilLine size={13} />
              <span style={{ flex: 1 }}>{t("file.renameSymbol")}</span>
              <button
                type="button"
                aria-label={t("common.close")}
                onClick={() => setRenameOpen(false)}
                style={{
                  border: "none",
                  background: "transparent",
                  color: "var(--text-hint)",
                  cursor: "pointer",
                  padding: 2,
                  display: "flex",
                }}
              >
                <X size={13} />
              </button>
            </div>
            <div
              style={{
                display: "flex",
                gap: 8,
                padding: 9,
                borderBottom: "1px solid var(--border-dim)",
              }}
            >
              <input
                aria-label={t("file.renameNewName")}
                value={renameName}
                onChange={(event) => setRenameName(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") void handlePreviewRename();
                  if (event.key === "Escape") setRenameOpen(false);
                }}
                autoFocus
                style={{
                  minWidth: 0,
                  flex: 1,
                  height: 28,
                  border: "1px solid var(--border)",
                  borderRadius: 5,
                  background: "var(--bg-panel)",
                  color: "var(--text-primary)",
                  padding: "0 8px",
                  fontSize: 12,
                  fontFamily: "var(--font-mono)",
                }}
              />
              <button
                type="button"
                disabled={renameLoading || !renameName.trim()}
                onClick={() => void handlePreviewRename()}
                aria-label={t("file.renamePreview")}
                style={{
                  height: 28,
                  padding: "0 9px",
                  border: "1px solid var(--border-dim)",
                  borderRadius: 5,
                  background: "var(--bg-hover)",
                  color:
                    renameLoading || !renameName.trim()
                      ? "var(--text-hint)"
                      : "var(--text-primary)",
                  fontSize: 12,
                  cursor: renameLoading || !renameName.trim() ? "default" : "pointer",
                  flexShrink: 0,
                }}
              >
                {renameLoading ? t("file.renamePreviewing") : t("file.renamePreview")}
              </button>
            </div>
            {renamePreview && (
              <>
                <div
                  style={{
                    padding: "7px 9px",
                    borderBottom: "1px solid var(--border-dim)",
                    color: "var(--text-secondary)",
                    fontSize: 12,
                    fontWeight: 600,
                  }}
                >
                  {(() => {
                    const summary = summarizeWorkspaceEdit(renamePreview);
                    return t("file.renamePreviewTitle", {
                      files: String(summary.files),
                      edits: String(summary.edits),
                    });
                  })()}
                </div>
                <div style={{ overflowY: "auto", padding: 4, flex: 1 }}>
                  {renamePreview.files.map((file) => (
                    <div key={file.uri} style={{ padding: "5px 7px" }}>
                      <div
                        style={{
                          color: "var(--text-secondary)",
                          fontFamily: "var(--font-mono)",
                          fontSize: 11.5,
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                        }}
                      >
                        {file.path}
                      </div>
                      <div
                        style={{
                          marginTop: 3,
                          color: "var(--text-hint)",
                          fontSize: 11,
                          fontFamily: "var(--font-mono)",
                        }}
                      >
                        {t("file.renameFileEdits", { count: String(file.edits.length) })}
                      </div>
                    </div>
                  ))}
                </div>
                <div
                  style={{
                    display: "flex",
                    justifyContent: "flex-end",
                    gap: 8,
                    padding: 9,
                    borderTop: "1px solid var(--border-dim)",
                  }}
                >
                  <button
                    type="button"
                    onClick={() => setRenameOpen(false)}
                    style={{
                      height: 28,
                      padding: "0 9px",
                      border: "1px solid var(--border-dim)",
                      borderRadius: 5,
                      background: "transparent",
                      color: "var(--text-secondary)",
                      fontSize: 12,
                      cursor: "pointer",
                    }}
                  >
                    {t("common.cancel")}
                  </button>
                  <button
                    type="button"
                    disabled={renameApplying}
                    onClick={() => void handleApplyRename()}
                    aria-label={t("file.renameApply")}
                    style={{
                      height: 28,
                      padding: "0 9px",
                      border: "1px solid var(--accent)",
                      borderRadius: 5,
                      background: "var(--accent)",
                      color: "var(--accent-fg)",
                      fontSize: 12,
                      cursor: renameApplying ? "default" : "pointer",
                    }}
                  >
                    {renameApplying ? t("file.renameApplying") : t("file.renameApply")}
                  </button>
                </div>
              </>
            )}
          </div>
        )}
        {codeActions && (
          <div
            role="dialog"
            aria-label={t("file.quickFix")}
            style={{
              position: "absolute",
              right: 12,
              bottom: 10,
              width: "min(460px, calc(100% - 24px))",
              maxHeight: "min(300px, calc(100% - 24px))",
              display: "flex",
              flexDirection: "column",
              border: "1px solid var(--border-dim)",
              borderRadius: 6,
              background: "var(--bg-card)",
              boxShadow: "0 16px 40px color-mix(in srgb, #000 26%, transparent)",
              overflow: "hidden",
              zIndex: 9,
            }}
          >
            <div
              style={{
                height: 30,
                display: "flex",
                alignItems: "center",
                gap: 8,
                padding: "0 9px",
                borderBottom: "1px solid var(--border-dim)",
                color: "var(--text-primary)",
                fontSize: 12,
                fontWeight: 600,
              }}
            >
              <Lightbulb size={13} />
              <span style={{ flex: 1 }}>{t("file.quickFixTitle")}</span>
              <button
                type="button"
                aria-label={t("common.close")}
                onClick={() => setCodeActions(null)}
                style={{
                  border: "none",
                  background: "transparent",
                  color: "var(--text-hint)",
                  cursor: "pointer",
                  padding: 2,
                  display: "flex",
                }}
              >
                <X size={13} />
              </button>
            </div>
            <div style={{ overflowY: "auto", padding: 4 }}>
              {codeActions.map((action, index) => {
                const canApply = Boolean(action.edit || action.command) && !codeActionApplying;
                return (
                  <button
                    key={`${action.title}:${index}`}
                    type="button"
                    aria-label={action.title}
                    disabled={!canApply}
                    onClick={() => void handleApplyCodeAction(action)}
                    style={{
                      width: "100%",
                      minHeight: 32,
                      display: "flex",
                      alignItems: "center",
                      gap: 8,
                      padding: "5px 7px",
                      border: "none",
                      borderRadius: 4,
                      background: "transparent",
                      color: canApply ? "var(--text-secondary)" : "var(--text-hint)",
                      cursor: canApply ? "pointer" : "default",
                      textAlign: "left",
                      fontSize: 12,
                    }}
                    onMouseEnter={(event) => {
                      if (canApply) event.currentTarget.style.background = "var(--bg-hover)";
                    }}
                    onMouseLeave={(event) => {
                      event.currentTarget.style.background = "transparent";
                    }}
                  >
                    <Lightbulb size={13} />
                    <span
                      style={{
                        minWidth: 0,
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                      }}
                    >
                      {action.title}
                    </span>
                  </button>
                );
              })}
            </div>
          </div>
        )}
      </div>

      <div
        style={{
          height: 22,
          display: "flex",
          alignItems: "center",
          padding: "0 12px",
          borderTop: "1px solid var(--border-dim)",
          background: "var(--bg-subtle)",
          flexShrink: 0,
          gap: 8,
        }}
      >
        <FileBreadcrumbs
          pathSegments={breadcrumbSegments}
          symbols={activeOutlineSymbols}
          label={t("file.breadcrumbs")}
          onJump={jumpToOutlineSymbol}
        />
        {!isPreviewableImage && content !== null && (
          <span
            style={{
              marginLeft: "auto",
              fontSize: 11,
              color: "var(--text-muted)",
              fontFamily: "var(--font-mono)",
              flexShrink: 0,
            }}
          >
            {t("file.cursorPosition", {
              line: String(cursorPosition.line),
              column: String(cursorPosition.column),
            })}
          </span>
        )}
        {!isPreviewableImage && content !== null && currentFileDiagnostics.length > 0 && (
          <Popover.Root>
            <Popover.Trigger asChild>
              <button
                type="button"
                title={currentFileDiagnostics.map((diagnostic) => diagnostic.message).join("\n")}
                style={{
                  height: 18,
                  display: "inline-flex",
                  alignItems: "center",
                  border: "1px solid var(--border-dim)",
                  borderRadius: 5,
                  background: "transparent",
                  color: "var(--warning)",
                  fontSize: 11,
                  fontFamily: "var(--font-mono)",
                  cursor: "pointer",
                  flexShrink: 0,
                  padding: "0 6px",
                }}
              >
                {t("file.diagnosticsCount", { count: String(currentFileDiagnostics.length) })}
              </button>
            </Popover.Trigger>
            <Popover.Portal>
              <Popover.Content
                align="end"
                sideOffset={6}
                style={{
                  width: 360,
                  maxHeight: 260,
                  overflow: "auto",
                  border: "1px solid var(--border)",
                  borderRadius: 8,
                  background: "var(--bg-card)",
                  boxShadow: "var(--shadow-lg)",
                  color: "var(--text-primary)",
                  zIndex: 60,
                  padding: 8,
                }}
              >
                <div
                  style={{
                    fontSize: 12,
                    fontWeight: 700,
                    marginBottom: 6,
                  }}
                >
                  {t("file.diagnosticsDetails")}
                </div>
                <div
                  role="group"
                  aria-label={t("file.diagnosticsSeverityFilter")}
                  style={{ display: "flex", gap: 4, marginBottom: 8, flexWrap: "wrap" }}
                >
                  {diagnosticFilterOptions.map((filter) => {
                    const count =
                      filter === "all"
                        ? currentFileDiagnostics.length
                        : diagnosticCounts[filter];
                    const active = diagnosticSeverityFilter === filter;
                    return (
                      <button
                        key={filter}
                        type="button"
                        aria-pressed={active}
                        onClick={() => {
                          setDiagnosticSeverityFilter(filter);
                          setDiagnosticCopyCount(null);
                          setDiagnosticCopyError(null);
                        }}
                        style={{
                          height: 24,
                          display: "inline-flex",
                          alignItems: "center",
                          gap: 5,
                          border: `1px solid ${active ? "var(--accent)" : "var(--border-dim)"}`,
                          borderRadius: 5,
                          background: active ? "var(--bg-hover)" : "transparent",
                          color: active ? "var(--text-primary)" : "var(--text-muted)",
                          cursor: "pointer",
                          fontSize: 11,
                          fontFamily: "var(--font-mono)",
                          padding: "0 7px",
                        }}
                      >
                        {t(
                          filter === "all"
                            ? "file.diagnosticsFilterAll"
                            : filter === "error"
                              ? "file.diagnosticsFilterErrors"
                              : filter === "warning"
                                ? "file.diagnosticsFilterWarnings"
                                : "file.diagnosticsFilterInfo",
                          { count: String(count) },
                        )}
                      </button>
                    );
                  })}
                </div>
                <div
                  style={{
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 8,
                    marginBottom: 8,
                  }}
                >
                  <button
                    type="button"
                    disabled={filteredFileDiagnostics.length === 0}
                    onClick={() => void handleCopyDiagnostics()}
                    style={{
                      height: 26,
                      display: "inline-flex",
                      alignItems: "center",
                      gap: 6,
                      border: "1px solid var(--border-dim)",
                      borderRadius: 5,
                      background: "transparent",
                      color:
                        filteredFileDiagnostics.length === 0
                          ? "var(--text-hint)"
                          : "var(--text-secondary)",
                      cursor: filteredFileDiagnostics.length === 0 ? "default" : "pointer",
                      fontSize: 11,
                      padding: "0 8px",
                    }}
                  >
                    <Copy size={12} />
                    {t("file.diagnosticsCopyVisible")}
                  </button>
                  {(diagnosticCopyCount !== null || diagnosticCopyError) && (
                    <span
                      role="status"
                      title={diagnosticCopyError ?? undefined}
                      style={{
                        minWidth: 0,
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                        color: diagnosticCopyError ? "var(--danger-fg)" : "var(--text-muted)",
                        fontSize: 11,
                        fontFamily: "var(--font-mono)",
                      }}
                    >
                      {diagnosticCopyError
                        ? t("file.diagnosticsCopyFailed", { error: diagnosticCopyError })
                        : t("file.diagnosticsCopied", {
                            count: String(diagnosticCopyCount ?? 0),
                          })}
                    </span>
                  )}
                </div>
                {groupedFileDiagnostics.length === 0 ? (
                  <div
                    style={{
                      borderTop: "1px solid var(--border-dim)",
                      padding: "9px 6px",
                      color: "var(--text-muted)",
                      fontSize: 12,
                    }}
                  >
                    {t("file.diagnosticsNoMatches")}
                  </div>
                ) : (
                  groupedFileDiagnostics.map((group) => (
                    <section key={group.source}>
                      <div
                        style={{
                          borderTop: "1px solid var(--border-dim)",
                          padding: "7px 6px 3px",
                          color: "var(--text-muted)",
                          fontFamily: "var(--font-mono)",
                          fontSize: 10.5,
                          fontWeight: 700,
                        }}
                      >
                        {t("file.diagnosticsSourceGroup", {
                          source: group.source,
                          count: String(group.diagnostics.length),
                        })}
                      </div>
                      {group.diagnostics.map((diagnostic) => (
                        <button
                          key={`${diagnostic.file}:${diagnostic.line}:${diagnostic.column}:${diagnostic.message}`}
                          type="button"
                          onClick={() => openDiagnostic(diagnostic)}
                          style={{
                            width: "100%",
                            display: "grid",
                            gridTemplateColumns: "auto 1fr",
                            gap: 8,
                            alignItems: "start",
                            padding: "7px 6px",
                            border: 0,
                            background: "transparent",
                            color: "var(--text-primary)",
                            textAlign: "left",
                            cursor: "pointer",
                            font: "inherit",
                          }}
                        >
                          <span
                            style={{
                              color: diagnosticSeverityColor(diagnostic.severity),
                              fontFamily: "var(--font-mono)",
                              fontSize: 10.5,
                              fontWeight: 700,
                              textTransform: "uppercase",
                            }}
                          >
                            {diagnostic.severity}
                          </span>
                          <span style={{ minWidth: 0 }}>
                            <span
                              style={{
                                display: "block",
                                fontSize: 12,
                                lineHeight: 1.35,
                              }}
                            >
                              {diagnostic.message}
                            </span>
                            <span
                              style={{
                                display: "block",
                                marginTop: 3,
                                color: "var(--text-muted)",
                                fontFamily: "var(--font-mono)",
                                fontSize: 10.5,
                              }}
                            >
                              {diagnostic.code ? `(${diagnostic.code}) · ` : ""}
                              {diagnostic.line}:{diagnostic.column}
                            </span>
                          </span>
                        </button>
                      ))}
                    </section>
                  ))
                )}
              </Popover.Content>
            </Popover.Portal>
          </Popover.Root>
        )}
        {!remote && !isPreviewableImage && content !== null && (
          <button
            type="button"
            disabled={inlineBlameLoading || !inlineBlamePath}
            onClick={() => void toggleInlineBlame()}
            title={
              inlineBlameError
                ? t("file.inlineBlameFailed", { error: inlineBlameError })
                : t("file.inlineBlame")
            }
            aria-label={t("file.inlineBlame")}
            style={{
              height: 18,
              display: "inline-flex",
              alignItems: "center",
              gap: 4,
              padding: "0 6px",
              border: "1px solid var(--border-dim)",
              borderRadius: 5,
              background: inlineBlameVisible ? "var(--bg-hover)" : "transparent",
              color: inlineBlameError ? "var(--danger-fg)" : "var(--text-muted)",
              fontSize: 10.5,
              cursor: inlineBlameLoading || !inlineBlamePath ? "default" : "pointer",
              flexShrink: 0,
            }}
          >
            <GitBranch size={11} />
            {inlineBlameLoading ? t("file.inlineBlameLoading") : t("file.inlineBlame")}
          </button>
        )}
        {!remote && !isPreviewableImage && content !== null && (
          <button
            type="button"
            disabled={formatting}
            onClick={() => void handleFormatFile()}
            title={t("file.formatCurrent")}
            aria-label={t("file.formatCurrent")}
            style={{
              height: 18,
              display: "inline-flex",
              alignItems: "center",
              gap: 4,
              padding: "0 6px",
              border: "1px solid var(--border-dim)",
              borderRadius: 5,
              background: formatting ? "var(--bg-hover)" : "transparent",
              color: "var(--text-muted)",
              fontSize: 10.5,
              cursor: formatting ? "default" : "pointer",
              flexShrink: 0,
            }}
          >
            <WandSparkles size={11} />
            {formatting ? t("file.formatting") : t("file.format")}
          </button>
        )}
        {!isPreviewableImage && content !== null && languageServer.supported && (
          <button
            type="button"
            disabled={referencesLoading}
            onClick={() => void handleFindReferences()}
            title={t("file.findReferences")}
            aria-label={t("file.findReferences")}
            style={{
              height: 18,
              display: "inline-flex",
              alignItems: "center",
              gap: 4,
              padding: "0 6px",
              border: "1px solid var(--border-dim)",
              borderRadius: 5,
              background: references ? "var(--bg-hover)" : "transparent",
              color: referencesLoading ? "var(--text-hint)" : "var(--text-muted)",
              fontSize: 10.5,
              cursor: referencesLoading ? "default" : "pointer",
              flexShrink: 0,
            }}
          >
            <Search size={11} />
            {referencesLoading ? t("file.referencesLoading") : t("file.references")}
          </button>
        )}
        {!isPreviewableImage && content !== null && languageServer.supported && (
          <button
            type="button"
            disabled={renameLoading || renameApplying}
            onClick={handleOpenRename}
            title={t("file.renameSymbol")}
            aria-label={t("file.renameSymbol")}
            style={{
              height: 18,
              display: "inline-flex",
              alignItems: "center",
              gap: 4,
              padding: "0 6px",
              border: "1px solid var(--border-dim)",
              borderRadius: 5,
              background: renameOpen ? "var(--bg-hover)" : "transparent",
              color: renameLoading || renameApplying ? "var(--text-hint)" : "var(--text-muted)",
              fontSize: 10.5,
              cursor: renameLoading || renameApplying ? "default" : "pointer",
              flexShrink: 0,
            }}
          >
            <PencilLine size={11} />
            {t("file.rename")}
          </button>
        )}
        {!isPreviewableImage && content !== null && languageServer.supported && (
          <button
            type="button"
            disabled={codeActionsLoading || codeActionApplying}
            onClick={() => void handleQuickFix()}
            title={t("file.quickFix")}
            aria-label={t("file.quickFix")}
            style={{
              height: 18,
              display: "inline-flex",
              alignItems: "center",
              gap: 4,
              padding: "0 6px",
              border: "1px solid var(--border-dim)",
              borderRadius: 5,
              background: codeActions ? "var(--bg-hover)" : "transparent",
              color:
                codeActionsLoading || codeActionApplying ? "var(--text-hint)" : "var(--text-muted)",
              fontSize: 10.5,
              cursor: codeActionsLoading || codeActionApplying ? "default" : "pointer",
              flexShrink: 0,
            }}
          >
            <Lightbulb size={11} />
            {codeActionsLoading ? t("file.quickFixLoading") : t("file.quickFixShort")}
          </button>
        )}
        {languageServerLabel && (
          <span
            title={languageServer.message ?? undefined}
            style={{
              fontSize: 11,
              color: languageServer.status?.available ? "var(--text-muted)" : "var(--warning)",
              fontFamily: "var(--font-mono)",
              flexShrink: 0,
            }}
          >
            {languageServerLabel}
          </span>
        )}
        {inlayHintsLoading && (
          <span
            style={{
              fontSize: 11,
              color: "var(--text-muted)",
              fontFamily: "var(--font-mono)",
              flexShrink: 0,
            }}
          >
            {t("file.inlayHintsLoading")}
          </span>
        )}
        {inlayHintsError && (
          <span
            title={inlayHintsError}
            style={{
              maxWidth: 220,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              fontSize: 11,
              color: "var(--warning)",
              fontFamily: "var(--font-mono)",
            }}
          >
            {t("file.inlayHintsFailed", { error: inlayHintsError })}
          </span>
        )}
        {statusLabel && (
          <span
            style={{
              marginLeft: isPreviewableImage || content === null ? "auto" : 0,
              fontSize: 11,
              color: saveStatus === "error" ? "var(--danger-fg)" : "var(--text-muted)",
              fontFamily: "var(--font-mono)",
            }}
          >
            {statusLabel}
          </span>
        )}
        {formatError && (
          <span
            title={formatError}
            style={{
              maxWidth: 220,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              fontSize: 11,
              color: "var(--danger-fg)",
              fontFamily: "var(--font-mono)",
            }}
          >
            {t("file.formatFailed", { error: formatError })}
          </span>
        )}
        {navigationError && (
          <span
            title={navigationError}
            style={{
              maxWidth: 220,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              fontSize: 11,
              color: "var(--warning)",
              fontFamily: "var(--font-mono)",
            }}
          >
            {navigationError}
          </span>
        )}
        {renameSummary && (
          <span
            style={{
              maxWidth: 260,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              fontSize: 11,
              color: "var(--text-muted)",
              fontFamily: "var(--font-mono)",
            }}
          >
            {t("file.renameApplied", {
              files: String(renameSummary.filesChanged),
              edits: String(renameSummary.editsApplied),
            })}
          </span>
        )}
        {codeActionSummary && (
          <span
            style={{
              maxWidth: 260,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              fontSize: 11,
              color: "var(--text-muted)",
              fontFamily: "var(--font-mono)",
            }}
          >
            {t("file.quickFixApplied", {
              files: String(codeActionSummary.filesChanged),
              edits: String(codeActionSummary.editsApplied),
            })}
          </span>
        )}
        {codeActionCommandSummary && (
          <span
            style={{
              maxWidth: 260,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              fontSize: 11,
              color: "var(--text-muted)",
              fontFamily: "var(--font-mono)",
            }}
          >
            {t("file.quickFixCommandExecuted", { command: codeActionCommandSummary })}
          </span>
        )}
        {inlineBlameError && (
          <span
            title={inlineBlameError}
            style={{
              maxWidth: 220,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              fontSize: 11,
              color: "var(--danger-fg)",
              fontFamily: "var(--font-mono)",
            }}
          >
            {t("file.inlineBlameFailed", { error: inlineBlameError })}
          </span>
        )}
      </div>
    </div>
  );
}

export function FileViewer({
  tabs,
  activeFilePath,
  projectPath,
  onSelectTab,
  onCloseTab,
  onCloseOtherTabs,
  onCloseTabsToRight,
  onCloseAllTabs,
  themeVariant,
  onRunMakeTarget: _onRunMakeTarget,
  remote,
  condaEnvironments = [],
  selectedCondaEnvPath,
  onSelectedCondaEnvPathChange,
  onRunPythonFile,
  onRunTestTarget,
  onDebugTestTarget,
  debugBreakpoints,
  diagnostics,
  coverage,
  onToggleDebugBreakpoint,
  onOpenDefinition,
  onFocusGroup,
  onSplitRight,
}: {
  tabs: OpenFileTab[];
  activeFilePath: string | null;
  projectPath: string;
  onSelectTab: (path: string) => void;
  onCloseTab: (path: string) => void;
  onCloseOtherTabs: (path: string) => void;
  onCloseTabsToRight: (path: string) => void;
  onCloseAllTabs: () => void;
  themeVariant: ThemeVariant;
  onRunMakeTarget?: (target: string) => void;
  remote?: RemoteFileContext;
  condaEnvironments?: CondaEnvironment[];
  selectedCondaEnvPath?: string | null;
  onSelectedCondaEnvPathChange?: (path: string | null) => void;
  onRunPythonFile?: (path: string) => void;
  onRunTestTarget?: (target: EditorTestRunTarget) => void;
  onDebugTestTarget?: (target: EditorTestRunTarget) => void;
  debugBreakpoints?: DebugBreakpoint[];
  diagnostics?: DiagnosticItem[];
  coverage?: TestCoverageSummary | null;
  onToggleDebugBreakpoint?: (filePath: string, line: number) => void;
  onOpenDefinition?: (path: string, name: string, selection?: OpenFileSelection) => void;
  onFocusGroup?: () => void;
  onSplitRight?: () => void;
}) {
  const { t } = useI18n();
  const [previewModes, setPreviewModes] = useState<Record<string, boolean>>({});
  const [menuOpen, setMenuOpen] = useState(false);
  const [dirtyTabs, setDirtyTabs] = useState<Record<string, boolean>>({});
  const [localHistoryTarget, setLocalHistoryTarget] = useState<{
    path: string;
    name: string;
  } | null>(null);
  const [localHistoryEntries, setLocalHistoryEntries] = useState<LocalHistoryEntry[]>([]);
  const [localHistorySelectedId, setLocalHistorySelectedId] = useState<string | null>(null);
  const [localHistorySnapshot, setLocalHistorySnapshot] =
    useState<LocalHistorySnapshot | null>(null);
  const [localHistoryCurrentContent, setLocalHistoryCurrentContent] = useState("");
  const [localHistoryLoading, setLocalHistoryLoading] = useState(false);
  const [localHistorySnapshotLoading, setLocalHistorySnapshotLoading] = useState(false);
  const [localHistoryRestoring, setLocalHistoryRestoring] = useState(false);
  const [localHistoryError, setLocalHistoryError] = useState<string | null>(null);
  const [reloadVersions, setReloadVersions] = useState<Record<string, number>>({});

  const handleDirtyChange = useCallback((path: string, dirty: boolean) => {
    setDirtyTabs((prev) => {
      if (dirty) return prev[path] ? prev : { ...prev, [path]: true };
      if (!prev[path]) return prev;
      const next = { ...prev };
      delete next[path];
      return next;
    });
  }, []);

  const openLocalHistory = useCallback((tab: OpenFileTab) => {
    setMenuOpen(false);
    setLocalHistoryTarget({ path: tab.path, name: tab.name });
    setLocalHistoryEntries([]);
    setLocalHistorySelectedId(null);
    setLocalHistorySnapshot(null);
    setLocalHistoryCurrentContent("");
    setLocalHistoryError(null);
  }, []);

  const closeLocalHistory = useCallback(() => {
    setLocalHistoryTarget(null);
    setLocalHistoryEntries([]);
    setLocalHistorySelectedId(null);
    setLocalHistorySnapshot(null);
    setLocalHistoryCurrentContent("");
    setLocalHistoryError(null);
  }, []);

  useEffect(() => {
    if (!localHistoryTarget) return;
    let cancelled = false;
    setLocalHistoryLoading(true);
    setLocalHistoryError(null);
    setLocalHistorySnapshot(null);
    setLocalHistorySelectedId(null);

    void Promise.all([
      invoke<LocalHistoryEntry[]>("list_local_history", {
        projectPath,
        filePath: localHistoryTarget.path,
      }),
      invoke<string>("read_file_content", {
        projectPath,
        path: localHistoryTarget.path,
      }),
    ])
      .then(([entries, currentContent]) => {
        if (cancelled) return;
        setLocalHistoryEntries(entries);
        setLocalHistoryCurrentContent(currentContent);
        setLocalHistorySelectedId(entries[0]?.id ?? null);
      })
      .catch((err) => {
        if (cancelled) return;
        setLocalHistoryEntries([]);
        setLocalHistoryCurrentContent("");
        setLocalHistoryError(String(err));
      })
      .finally(() => {
        if (!cancelled) setLocalHistoryLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [localHistoryTarget, projectPath]);

  useEffect(() => {
    if (!localHistoryTarget || !localHistorySelectedId) {
      setLocalHistorySnapshot(null);
      return;
    }
    let cancelled = false;
    setLocalHistorySnapshotLoading(true);
    setLocalHistoryError(null);

    void invoke<LocalHistorySnapshot>("read_local_history_entry", {
      projectPath,
      filePath: localHistoryTarget.path,
      entryId: localHistorySelectedId,
    })
      .then((snapshot) => {
        if (!cancelled) setLocalHistorySnapshot(snapshot);
      })
      .catch((err) => {
        if (!cancelled) {
          setLocalHistorySnapshot(null);
          setLocalHistoryError(String(err));
        }
      })
      .finally(() => {
        if (!cancelled) setLocalHistorySnapshotLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [localHistorySelectedId, localHistoryTarget, projectPath]);

  const restoreLocalHistory = useCallback(async () => {
    if (!localHistoryTarget || !localHistorySnapshot || localHistoryRestoring) return;
    const confirmed = await confirm(t("file.localHistoryRestoreConfirm"), {
      title: t("file.localHistory"),
      kind: "warning",
    });
    if (!confirmed) return;
    setLocalHistoryRestoring(true);
    setLocalHistoryError(null);
    try {
      const restored = await invoke<LocalHistorySnapshot>("restore_local_history_entry", {
        projectPath,
        filePath: localHistoryTarget.path,
        entryId: localHistorySnapshot.entry.id,
      });
      setLocalHistorySnapshot(restored);
      setLocalHistoryCurrentContent(restored.content);
      setReloadVersions((prev) => ({
        ...prev,
        [localHistoryTarget.path]: (prev[localHistoryTarget.path] ?? 0) + 1,
      }));
      handleDirtyChange(localHistoryTarget.path, false);
      const entries = await invoke<LocalHistoryEntry[]>("list_local_history", {
        projectPath,
        filePath: localHistoryTarget.path,
      });
      setLocalHistoryEntries(entries);
    } catch (err) {
      setLocalHistoryError(String(err));
    } finally {
      setLocalHistoryRestoring(false);
    }
  }, [
    handleDirtyChange,
    localHistoryRestoring,
    localHistorySnapshot,
    localHistoryTarget,
    projectPath,
    t,
  ]);

  useEffect(() => {
    setPreviewModes((prev) => {
      const next: Record<string, boolean> = {};
      for (const tab of tabs) {
        if (prev[tab.path]) next[tab.path] = true;
      }
      return Object.keys(next).length === Object.keys(prev).length ? prev : next;
    });
  }, [tabs]);

  useEffect(() => {
    setDirtyTabs((prev) => {
      const openPaths = new Set(tabs.map((tab) => tab.path));
      const next = Object.fromEntries(Object.entries(prev).filter(([path]) => openPaths.has(path)));
      return Object.keys(next).length === Object.keys(prev).length ? prev : next;
    });
  }, [tabs]);

  const activeTab = useMemo(
    () => tabs.find((tab) => tab.path === activeFilePath) ?? tabs[tabs.length - 1] ?? null,
    [tabs, activeFilePath],
  );

  if (!activeTab) return null;

  const activePreviewMode = !!previewModes[activeTab.path];
  const activeIsMarkdown = isMarkdownFile(activeTab.name);
  const activeCondaEnv = selectRunnableCondaEnvironment(
    condaEnvironments,
    selectedCondaEnvPath,
    Boolean(remote),
  );
  const selectableCondaEnvironments = condaEnvironments;
  const canRunScript = isRunnableScriptFile(activeTab.path, Boolean(remote)) && !!onRunPythonFile;
  const canCloseOtherTabs = tabs.length > 1;
  const activeTabIndex = tabs.findIndex((tab) => tab.path === activeTab.path);
  const canCloseTabsToRight = activeTabIndex !== -1 && activeTabIndex < tabs.length - 1;
  const activeCanShowLocalHistory = !remote && !isPreviewableImageFile(activeTab.name);

  return (
    <div
      onMouseDown={onFocusGroup}
      style={{
        flex: 1,
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
        minWidth: 0,
        minHeight: 0,
        background: "var(--bg-panel)",
        position: "relative",
      }}
    >
      <div
        style={{
          height: 40,
          display: "flex",
          alignItems: "center",
          borderBottom: "1px solid var(--border-dim)",
          flexShrink: 0,
          background: "var(--bg-sidebar)",
          minWidth: 0,
        }}
      >
        <div
          className="file-viewer-tab-strip"
          style={{
            flex: 1,
            minWidth: 0,
            height: "100%",
            display: "flex",
            alignItems: "stretch",
            overflowX: "auto",
            overflowY: "hidden",
            paddingLeft: 4,
          }}
        >
          {tabs.map((tab) => {
            const isActive = tab.path === activeTab.path;
            const fileColor = getFileColor(tab.name);
            const isDirty = Boolean(dirtyTabs[tab.path]);
            return (
              <button
                key={tab.path}
                onClick={() => onSelectTab(tab.path)}
                title={tab.path}
                style={{
                  height: "100%",
                  minWidth: 0,
                  maxWidth: 220,
                  display: "flex",
                  alignItems: "center",
                  gap: 6,
                  padding: "0 10px 0 12px",
                  border: "none",
                  borderRight: "1px solid var(--border-dim)",
                  borderTop: isActive ? "2px solid var(--accent)" : "2px solid transparent",
                  background: isActive ? "var(--bg-panel)" : "transparent",
                  fontSize: 12.5,
                  fontWeight: isActive ? 500 : 400,
                  color: isActive ? "var(--text-primary)" : "var(--text-secondary)",
                  cursor: "pointer",
                  flexShrink: 0,
                }}
              >
                <span
                  style={{
                    width: isDirty ? 8 : 5,
                    height: isDirty ? 8 : 14,
                    borderRadius: isDirty ? 999 : 2,
                    background: isDirty ? "var(--warning)" : fileColor,
                    boxShadow: isDirty
                      ? "0 0 0 2px color-mix(in srgb, var(--warning) 20%, transparent)"
                      : undefined,
                    flexShrink: 0,
                    display: "inline-block",
                  }}
                />
                <span
                  style={{
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {tab.name}
                </span>
                <span
                  onClick={(event) => {
                    event.stopPropagation();
                    onCloseTab(tab.path);
                  }}
                  style={{
                    background: "none",
                    border: "none",
                    cursor: "pointer",
                    padding: "2px",
                    borderRadius: 3,
                    display: "flex",
                    alignItems: "center",
                    color: "var(--text-hint)",
                    marginLeft: 2,
                  }}
                  role="button"
                  aria-label={t("file.closeTab", { name: tab.name })}
                >
                  <X size={12} />
                </span>
              </button>
            );
          })}
        </div>
        <div
          style={{
            marginLeft: 8,
            marginRight: 8,
            display: "flex",
            alignItems: "center",
            gap: 4,
            flexShrink: 0,
          }}
        >
          {activeIsMarkdown && (
            <button
              onClick={() =>
                setPreviewModes((prev) => ({
                  ...prev,
                  [activeTab.path]: !prev[activeTab.path],
                }))
              }
              title={activePreviewMode ? t("common.edit") : t("common.preview")}
              style={{
                background: "none",
                border: "none",
                cursor: "pointer",
                padding: "3px 8px",
                borderRadius: 4,
                display: "flex",
                alignItems: "center",
                gap: 4,
                color: activePreviewMode ? "var(--accent)" : "var(--text-hint)",
                fontSize: 11.5,
                fontFamily: "var(--font-ui)",
                flexShrink: 0,
              }}
              onMouseEnter={(e) => (e.currentTarget.style.background = "var(--bg-hover)")}
              onMouseLeave={(e) => (e.currentTarget.style.background = "none")}
            >
              {activePreviewMode ? <PencilLine size={13} /> : <Eye size={13} />}
              {activePreviewMode ? t("common.edit") : t("common.preview")}
            </button>
          )}
          {onSplitRight && (
            <button
              type="button"
              onClick={onSplitRight}
              title={t("file.splitRight")}
              aria-label={t("file.splitRight")}
              style={{
                background: "none",
                border: "none",
                cursor: "pointer",
                padding: "4px",
                borderRadius: 4,
                display: "flex",
                alignItems: "center",
                color: "var(--text-hint)",
              }}
              onMouseEnter={(e) => (e.currentTarget.style.background = "var(--bg-hover)")}
              onMouseLeave={(e) => (e.currentTarget.style.background = "none")}
            >
              <Columns2 size={15} />
            </button>
          )}
          <button
            type="button"
            disabled={!canRunScript}
            onClick={() => onRunPythonFile?.(activeTab.path)}
            title={canRunScript ? t("file.runCurrent") : t("file.runScriptOnly")}
            aria-label={t("file.runCurrent")}
            style={{
              background: "none",
              border: "none",
              cursor: canRunScript ? "pointer" : "not-allowed",
              padding: "4px",
              borderRadius: 4,
              display: "flex",
              alignItems: "center",
              color: canRunScript ? "var(--accent)" : "var(--text-hint)",
              opacity: canRunScript ? 1 : 0.42,
            }}
            onMouseEnter={(e) => {
              if (canRunScript) e.currentTarget.style.background = "var(--bg-hover)";
            }}
            onMouseLeave={(e) => (e.currentTarget.style.background = "none")}
          >
            <Play size={15} fill="currentColor" />
          </button>
          <Popover.Root open={menuOpen} onOpenChange={setMenuOpen}>
            <Popover.Trigger asChild>
              <button
                title={t("file.tabActions")}
                aria-label={t("file.tabActions")}
                style={{
                  background: "none",
                  border: "none",
                  cursor: "pointer",
                  padding: "4px",
                  borderRadius: 4,
                  display: "flex",
                  alignItems: "center",
                  color: "var(--text-hint)",
                }}
                onMouseEnter={(e) => (e.currentTarget.style.background = "var(--bg-hover)")}
                onMouseLeave={(e) => (e.currentTarget.style.background = "none")}
              >
                <MoreHorizontal size={15} />
              </button>
            </Popover.Trigger>
            <Popover.Portal>
              <Popover.Content
                sideOffset={6}
                align="end"
                onOpenAutoFocus={(event) => event.preventDefault()}
                className="file-viewer-tab-menu"
              >
                <button
                  type="button"
                  disabled={!activeCanShowLocalHistory}
                  title={
                    activeCanShowLocalHistory
                      ? t("file.showLocalHistory")
                      : t("file.localHistoryUnavailable")
                  }
                  onClick={() => openLocalHistory(activeTab)}
                  className="file-viewer-tab-menu-item"
                >
                  {t("file.showLocalHistory")}
                </button>
                <button
                  type="button"
                  disabled={!canCloseOtherTabs}
                  onClick={() => {
                    onCloseOtherTabs(activeTab.path);
                    setMenuOpen(false);
                  }}
                  className="file-viewer-tab-menu-item"
                >
                  {t("file.closeOtherTabs")}
                </button>
                <button
                  type="button"
                  disabled={!canCloseTabsToRight}
                  onClick={() => {
                    onCloseTabsToRight(activeTab.path);
                    setMenuOpen(false);
                  }}
                  className="file-viewer-tab-menu-item"
                >
                  {t("file.closeTabsToRight")}
                </button>
                <button
                  type="button"
                  disabled={tabs.length === 0}
                  onClick={() => {
                    onCloseAllTabs();
                    setMenuOpen(false);
                  }}
                  className="file-viewer-tab-menu-item"
                >
                  {t("file.closeAllTabs")}
                </button>
              </Popover.Content>
            </Popover.Portal>
          </Popover.Root>
        </div>
      </div>

      <div
        style={{
          flex: 1,
          position: "relative",
          minWidth: 0,
          minHeight: 0,
        }}
      >
        {tabs.map((tab) => {
          const isActive = tab.path === activeTab.path;
          return (
            <div
              key={tab.path}
              style={{
                position: "absolute",
                inset: 0,
                display: "flex",
                flexDirection: "column",
                visibility: isActive ? "visible" : "hidden",
                pointerEvents: isActive ? "auto" : "none",
              }}
            >
              <FilePreviewPane
                key={`${tab.path}:${reloadVersions[tab.path] ?? 0}`}
                filePath={tab.path}
                fileName={tab.name}
                projectPath={projectPath}
                themeVariant={themeVariant}
                previewMode={!!previewModes[tab.path]}
                selection={tab.selection}
                remote={remote}
                debugBreakpoints={debugBreakpoints}
                diagnostics={diagnostics}
                coverage={coverage}
                onToggleDebugBreakpoint={onToggleDebugBreakpoint}
                onRunTestTarget={onRunTestTarget}
                onDebugTestTarget={onDebugTestTarget}
                onOpenDefinition={onOpenDefinition}
                onDirtyChange={handleDirtyChange}
              />
            </div>
          );
        })}
      </div>
      {localHistoryTarget ? (
        <LocalHistoryDialog
          targetName={localHistoryTarget.name}
          entries={localHistoryEntries}
          selectedEntryId={localHistorySelectedId}
          snapshot={localHistorySnapshot}
          currentContent={localHistoryCurrentContent}
          loading={localHistoryLoading}
          snapshotLoading={localHistorySnapshotLoading}
          restoring={localHistoryRestoring}
          error={localHistoryError}
          onSelectEntry={setLocalHistorySelectedId}
          onRestore={restoreLocalHistory}
          onClose={closeLocalHistory}
        />
      ) : null}

      <div
        style={{
          height: 24,
          flexShrink: 0,
          display: "flex",
          alignItems: "center",
          justifyContent: "flex-end",
          gap: 6,
          padding: "0 8px",
          borderTop: "1px solid var(--border-dim)",
          background: "color-mix(in srgb, var(--bg-sidebar) 84%, transparent)",
        }}
      >
        <Popover.Root>
          <Popover.Trigger asChild>
            <button
              type="button"
              title={t("file.condaEnvironment")}
              aria-label={t("file.condaEnvironment")}
              style={{
                maxWidth: 220,
                height: 18,
                display: "inline-flex",
                alignItems: "center",
                gap: 5,
                padding: "0 7px",
                border: "1px solid var(--border-dim)",
                borderRadius: 999,
                background: "color-mix(in srgb, var(--bg-card) 70%, transparent)",
                color: "var(--text-muted)",
                fontSize: 10.5,
                cursor: selectableCondaEnvironments.length > 0 ? "pointer" : "default",
                fontFamily: "var(--font-ui)",
              }}
            >
              <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                {activeCondaEnv?.name ?? t("file.noCondaEnvironment")}
              </span>
            </button>
          </Popover.Trigger>
          <Popover.Portal>
            <Popover.Content sideOffset={6} align="end" className="file-viewer-tab-menu">
              {selectableCondaEnvironments.length === 0 ? (
                <div
                  className="file-viewer-tab-menu-item"
                  style={{ cursor: "default", color: "var(--text-hint)" }}
                >
                  {t("file.noCondaEnvironment")}
                </div>
              ) : (
                selectableCondaEnvironments.map((env) => (
                  <button
                    type="button"
                    key={env.path}
                    className="file-viewer-tab-menu-item"
                    onClick={() => onSelectedCondaEnvPathChange?.(env.path)}
                  >
                    <span
                      style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}
                    >
                      {env.name}
                    </span>
                    {activeCondaEnv?.path === env.path && <Check size={12} color="var(--accent)" />}
                  </button>
                ))
              )}
            </Popover.Content>
          </Popover.Portal>
        </Popover.Root>
      </div>
    </div>
  );
}
