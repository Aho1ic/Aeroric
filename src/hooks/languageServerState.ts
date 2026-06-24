export type LspServerCommand = {
  program: string;
  args: string[];
};

export type LspServerStatus = {
  supported: boolean;
  available: boolean;
  languageId?: string | null;
  command?: LspServerCommand | null;
  installHint?: string | null;
};

export type BuildLspDocumentRequestInput = {
  projectPath: string;
  filePath: string;
  content: string;
  line: number;
  column: number;
};

export type LspDocumentRequest = {
  projectPath: string;
  filePath: string;
  content: string;
  line: number;
  character: number;
};

const SUPPORTED_EXTENSIONS = new Set(["ts", "tsx", "js", "jsx", "mjs", "cjs"]);

export function isLspSupportedFile(path: string): boolean {
  const extension = path.split(".").pop()?.toLowerCase();
  return extension ? SUPPORTED_EXTENSIONS.has(extension) : false;
}

export function buildLspDocumentRequest(input: BuildLspDocumentRequestInput): LspDocumentRequest {
  return {
    projectPath: input.projectPath,
    filePath: input.filePath,
    content: input.content,
    line: Math.max(0, input.line - 1),
    character: Math.max(0, input.column - 1),
  };
}

export function languageServerStatusMessage(status: LspServerStatus | null): string | null {
  if (!status) return null;
  if (!status.supported) return "Language server is not supported for this file type.";
  if (!status.available) {
    return status.installHint ?? "Install typescript-language-server to enable code intelligence.";
  }
  return null;
}
