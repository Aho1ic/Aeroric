import type { CSSProperties } from "react";
import { Decoration, EditorView, GutterMarker, WidgetType, gutter } from "@uiw/react-codemirror";
import { StreamLanguage } from "@codemirror/language";
import type { Extension } from "@codemirror/state";
import type { DbEndpoint, DiagnosticSeverity, GitBlameLine } from "../../types";
import type { RemoteProjectTarget } from "../../types";
import { inlineBlameText, inlineBlameTitle } from "../git-advanced/gitAdvancedState";
import {
  hasMarkdownExtension,
  hasPreviewableImageExtension,
  hasSqliteDatabaseExtension,
} from "../../lib/fileExtensions";

export type RemoteFileContext = RemoteProjectTarget;

export type ProjectEditorSettings = {
  editor?: {
    format_on_save?: boolean;
  };
};

export type SaveContentOptions = {
  formatAfterSave?: boolean;
};

export type SaveStatus = "idle" | "dirty" | "saving" | "saved" | "error";
export type ImagePreviewData = {
  dataUrl: string;
  mimeType: string;
  byteLength: number;
};

export type CursorPosition = {
  line: number;
  column: number;
};

export type EditorContextMenuState = {
  x: number;
  y: number;
};

// 后缀表在 lib/fileExtensions.ts,与 file-explorer 的图标表共用同一份。
export function isMarkdownFile(fileName: string): boolean {
  return hasMarkdownExtension(fileName);
}

export function isPreviewableImageFile(fileName: string): boolean {
  return hasPreviewableImageExtension(fileName);
}

export function isSqliteDatabaseFile(fileName: string): boolean {
  return hasSqliteDatabaseExtension(fileName);
}

export async function loadLanguageExtension(fileName: string): Promise<Extension> {
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

export const editorBaseTheme = EditorView.theme({
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

export function createDebugBreakpointGutter({
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

export function createInlineBlameExtension({
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

export const diagnosticFilterOptions: DiagnosticSeverityFilter[] = [
  "all",
  "error",
  "warning",
  "info",
];

export type DiagnosticSeverityFilter = "all" | "error" | "warning" | "info";

export function diagnosticSeverityColor(severity: DiagnosticSeverity): string {
  if (severity === "error") return "var(--danger-fg)";
  if (severity === "warning") return "var(--warning)";
  return "var(--accent)";
}

export const editorContextMenuStyle: CSSProperties = {
  position: "fixed",
  minWidth: 180,
  padding: 4,
  border: "1px solid var(--border-dim)",
  borderRadius: 6,
  background: "var(--bg-card)",
  boxShadow: "0 14px 38px color-mix(in srgb, #000 28%, transparent)",
  zIndex: 30,
};

export const editorContextMenuItemStyle: CSSProperties = {
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

export function sqliteEndpointForFile(filePath: string, remote?: RemoteFileContext): DbEndpoint {
  if (remote?.kind === "ssh") {
    return {
      kind: "ssh",
      connection: remote.connection,
      path: filePath,
      projectPath: remote.projectPath,
    };
  }
  return { kind: "local", path: filePath };
}
